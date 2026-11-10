"""A recorded green smoke on main's head stands for later start routines, and the session's smoke runs detached when
main moved after the pre-session smoke (D-2560). Red-main protection is unchanged: a reuse needs the LAST run on the
head green and finished, `main` not red and never red on that head, the nightly and `main`'s checks not red on it,
and a red `main` clears only on the runner's own new run (docs/governance.md 5.3). Fake data only; no real cargo,
network or state branch."""

from __future__ import annotations

import json
import subprocess
import sys
from datetime import timedelta
from pathlib import Path
from typing import Any

import pytest

from runner import cycle, github_env as ge, state as st
from test_failure_drills import NOW, FakeEnv, config, go_red, green_pr, make_task
from test_runner_keeper import _flow
from test_runner_pre_smoke import HEAD, PreSmokeEnv, _env, _pre, job  # noqa: F401  # reason: `job` is a fixture used by name

OLD = "d" * 40


def _run(sha: str, ok: bool = True, at: Any = NOW, **extra: Any) -> dict[str, Any]:
    return {"sha": sha, "ok": ok, "at": st.iso(at) if not isinstance(at, str) else at, "why": "start routine", **extra}


def _state(*runs: dict[str, Any], **extra: Any) -> st.State:
    return dict(st.new_state(), smoke_runs=list(runs), **extra)


# ---- the rule (state.reusable_smoke)


def test_the_last_green_finished_run_on_the_head_is_reusable() -> None:
    green = _run(HEAD, at=NOW - timedelta(hours=1))
    assert st.reusable_smoke(_state(_run(OLD, ok=False), green), HEAD, NOW) == green
    assert st.reusable_smoke(_state(_run(OLD)), HEAD, NOW) is None, "a green on another commit never stands for this one"
    assert st.reusable_smoke(_state(green), "", NOW) is None


@pytest.mark.parametrize("later", [_run(HEAD, ok=False), _run(HEAD, ok=False, timed_out=True), _run(HEAD, ok=True, timed_out=True)])
def test_a_later_red_or_timed_out_run_on_the_same_head_ends_the_reuse(later: dict[str, Any]) -> None:
    assert st.reusable_smoke(_state(_run(HEAD), later), HEAD, NOW) is None


def test_a_red_run_on_the_head_ends_the_reuse_even_when_a_green_came_after() -> None:
    assert st.reusable_smoke(_state(_run(HEAD, ok=False), _run(HEAD)), HEAD, NOW) is None
    assert st.reusable_smoke(_state(_run(HEAD, ok=False, timed_out=True), _run(HEAD)), HEAD, NOW) is not None, "a timeout is no verdict"
    strike = {"sha": HEAD, "at": st.iso(NOW), "why": "smoke red while Ci, Twin and Ally were green on the head"}
    assert st.reusable_smoke(_state(_run(HEAD), smoke_env_problems=[strike]), HEAD, NOW) is None
    assert st.reusable_smoke(_state(_run(HEAD), smoke_env_problems=[dict(strike, sha=OLD)]), HEAD, NOW) is not None


def test_never_while_main_is_red_nor_on_a_head_that_was_once_red() -> None:
    red = {"check": "smoke", "sha": HEAD, "task": "T001", "revert_sha": "m1", "files": [], "set_at": st.iso(NOW)}
    assert st.reusable_smoke(_state(_run(HEAD), main_red=red), HEAD, NOW) is None
    cleared = dict(red, cleared_at=st.iso(NOW), cleared_on=HEAD)
    assert st.reusable_smoke(_state(_run(HEAD), main_red_history=[cleared]), HEAD, NOW) is None
    assert st.reusable_smoke(_state(_run(HEAD), main_red_history=[dict(cleared, sha=OLD)]), HEAD, NOW) is not None


def test_the_reuse_ages_from_the_green_run_it_stands_on() -> None:
    edge = NOW - st.SMOKE_REUSE_MAX_AGE
    assert st.reusable_smoke(_state(_run(HEAD, at=edge)), HEAD, NOW) is not None
    assert st.reusable_smoke(_state(_run(HEAD, at=edge - timedelta(seconds=1))), HEAD, NOW) is None
    chained = _run(HEAD, at=NOW, reused=st.iso(edge - timedelta(minutes=1)))
    assert st.reusable_smoke(_state(chained), HEAD, NOW) is None, "a reuse of a reuse never extends the age"
    assert st.reusable_smoke(_state(_run(HEAD, at=NOW + timedelta(minutes=5))), HEAD, NOW) is None, "a future stamp is doubted"
    assert st.reusable_smoke(_state(_run(HEAD, at="not a time")), HEAD, NOW) is None


# ---- the start routine (cycle._start_smoke)


def test_a_cycle_on_a_head_already_smoked_green_reuses_it_and_records_the_reuse(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    since = st.iso(NOW - timedelta(minutes=30))
    env.store = dict(env.store, smoke_runs=[_run(env.head, at=since)])
    env.smoke_green = False  # would turn main red if it ran: the reuse never runs it
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING and env.smokes == [] and env.runs[0][0] == "T001"
    assert env.store["smoke_runs"][-1] == {"sha": env.head, "ok": True, "at": st.iso(NOW),
                                           "why": f"start routine (reused {since})", "reused": since}


def test_a_new_head_is_smoked_and_a_red_run_still_turns_main_red(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    env.store = dict(env.store, smoke_runs=[_run("main0000")])
    env.smoke_green = False
    assert cycle.run_cycle(cfg, env, NOW) == "main_red" and env.smokes == [env.head]
    assert env.store["main_red"]["check"] == "smoke" and env.store["main_red"]["sha"] == env.head


@pytest.mark.parametrize("doubt", ["nightly_red", "nightly_unfinished", "checks_red"])
def test_a_red_nightly_or_red_checks_on_the_head_make_the_start_routine_smoke(tmp_path: Path, doubt: str) -> None:
    env = FakeEnv()
    state = _state(_run(env.head))
    if doubt == "checks_red":
        env.checks_green = False
    else:
        env.nightly = {"sha": env.head, "conclusion": "failure" if doubt == "nightly_red" else None}
    env.smoke_green = False
    out, ok, head = cycle._start_smoke(state, env, NOW)
    assert (ok, head, env.smokes) == (False, env.head, [env.head]) and out["smoke_runs"][-1]["why"] == "start routine"


def test_unknown_checks_or_an_old_nightly_do_not_stop_a_reuse(tmp_path: Path) -> None:
    env = FakeEnv()
    env.checks_green, env.nightly = None, {"sha": "main0000", "conclusion": "failure"}
    out, ok, _ = cycle._start_smoke(_state(_run(env.head)), env, NOW)
    assert ok is True and env.smokes == [] and out["smoke_runs"][-1]["reused"] == st.iso(NOW)


def test_clearing_a_red_main_always_runs_its_own_smoke(tmp_path: Path) -> None:
    """Governance 5.3: a green recorded on the new head before is no verdict for clearing; the runner's own run is."""
    env, cfg = go_red(tmp_path)
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == "fix_main"
    env.prs[st.FIX_MAIN] = green_pr(sha="revert01", tree="t-revert")
    env.head = "main0002"
    env.store = dict(env.store, smoke_runs=[*env.store.get("smoke_runs", []), _run("main0002", at=NOW + timedelta(minutes=12))])
    env.smoke_green = False
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=15)) == "main_red"
    assert env.smokes[-1] == "main0002" and env.store["main_red"] and env.store["smoke_runs"][-1]["ok"] is False
    env.smoke_green = True
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=20)) == "main_cleared"
    assert env.store["smoke_runs"][-1]["why"] == "verify FIX-MAIN" and "reused" not in env.store["smoke_runs"][-1]


# ---- a smoke still running in the background (smoke_pending)


class PendingEnv(FakeEnv):
    """A FakeEnv whose smoke is still running in the background after this call's wait."""

    def __init__(self) -> None:
        super().__init__()
        self.smoke_pending = True

    def smoke_main(self) -> tuple[bool, str]:
        self.smokes.append(self.head)
        return False, self.head


def test_a_pending_smoke_claims_nothing_records_nothing_and_never_turns_main_red(tmp_path: Path) -> None:
    env, cfg = PendingEnv(), config([make_task("T001")], tmp_path)
    env.checks_green = False
    assert cycle.run_cycle(cfg, env, NOW) == cycle.SMOKE_PENDING
    assert env.runs == [] and not env.store.get("smoke_runs") and not env.store.get("main_red")
    assert not env.store.get("smoke_env_problems"), "no D-498 strike either"


def test_a_pending_verify_smoke_keeps_main_red(tmp_path: Path) -> None:
    env, cfg = go_red(tmp_path)
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == "fix_main"
    env.prs[st.FIX_MAIN] = green_pr(sha="revert01", tree="t-revert")
    pending = PendingEnv()
    pending.__dict__.update({k: v for k, v in env.__dict__.items() if k != "smoke_pending"})
    runs = list(pending.store.get("smoke_runs", []))
    assert cycle.run_cycle(cfg, pending, NOW + timedelta(minutes=15)) == cycle.SMOKE_PENDING
    assert pending.store["main_red"] and pending.store.get("smoke_runs", []) == runs and st.status(pending.store, st.FIX_MAIN) == st.DONE


# ---- the real Env: the session's smoke in the background


def _background(env: Any, job: Path, results: list[dict[str, Any] | None], alive: list[bool]) -> list[str]:
    """Background on; `bg_start` records the head and writes the next of `results` (None: nothing yet); `bg_alive`
    answers from `alive` in turn (the last answer repeats)."""
    started: list[str] = []
    env.smoke_background, env.smoke_wait, env.sleep = True, 0, lambda s: None

    def start(head: str, root: Path) -> int:
        started.append(head)
        return 4242

    def is_alive(pid: int) -> bool:
        result = results.pop(0) if results else None
        if result is not None:
            ge.write_pre_smoke(result)
        return alive.pop(0) if len(alive) > 1 else alive[0]

    env.bg_start, env.bg_alive = start, is_alive
    return started


def _bg_result(head: str = HEAD, code: int = 0) -> dict[str, Any]:
    return {"sha": head, "ok": code == 0, "timed_out": False, "exit": code, "seconds": 611, "at": "x", "job": "77-1",
            "last": "smoke: all green" if code == 0 else "smoke: FAILED", "source": ge.BACKGROUND}


def test_a_stale_pre_session_result_starts_the_smoke_in_the_background_and_uses_its_result(job: Path, tmp_path: Path) -> None:
    env, calls = _env(tmp_path, smoke=(1, "must not run in the call"))
    _pre(env)
    env.pre_smoke()
    env.main_head = lambda: OLD  # type: ignore[method-assign]  # reason: main moved after the step
    started = _background(env, job, [_bg_result(OLD)], [False])
    calls.clear()
    assert env.smoke_main() == (True, OLD) and started == [OLD] and ["bash", "scripts/smoke.sh"] not in calls
    assert env.smoke_source == ge.BACKGROUND and env.smoke_seconds == 611 and not env.smoke_pending
    assert json.loads((job / ge.SMOKE_BG).read_text())["sha"] == OLD and (job / ge.SMOKE_RAN).exists()
    assert env.log[-1].endswith("(background, 611 s, this session's background smoke)")


def test_a_smoke_still_running_is_pending_and_the_next_call_waits_for_the_same_one(job: Path, tmp_path: Path) -> None:
    env, _ = _env(tmp_path)
    started = _background(env, job, [None], [True])
    assert env.smoke_main() == (False, HEAD) and env.smoke_pending and not env.smoke_timed_out
    assert env.log[-1] == f"smoke on {HEAD[:12]}: still running in the background after 0 s"
    _background(env, job, [_bg_result(code=101)], [True])
    env.bg_start = lambda head, root: pytest.fail("a second smoke beside the running one")  # type: ignore[assignment]  # reason: must not run
    assert env.smoke_main() == (False, HEAD) and not env.smoke_pending and not env.smoke_timed_out, "a red result is red"
    assert started == [HEAD]


def test_a_background_smoke_that_ended_without_a_result_is_no_verdict(job: Path, tmp_path: Path) -> None:
    env, _ = _env(tmp_path)
    _background(env, job, [None], [False])
    assert env.smoke_main() == (False, HEAD) and env.smoke_timed_out and not env.smoke_pending
    assert env.log[-1] == f"smoke on {HEAD[:12]}: exit 124; the background smoke ended without a result"
    assert not (job / ge.SMOKE_BG).exists(), "the next call starts afresh"


def test_one_for_an_older_head_still_running_is_stopped_before_the_new_head_is_smoked(job: Path, tmp_path: Path) -> None:
    env, _ = _env(tmp_path)
    started = _background(env, job, [None], [True])
    env.smoke_main()
    stops: list[str] = []
    env.bg_stop = lambda: stops.append("stop") or "stopped"  # type: ignore[assignment]  # reason: no real process
    env.main_head = lambda: OLD  # type: ignore[method-assign]  # reason: main moved while it ran
    _background(env, job, [_bg_result(OLD)], [True])
    env.bg_start = lambda head, root: started.append(head) or 4343  # type: ignore[assignment]  # reason: count starts
    assert env.smoke_main() == (True, OLD) and stops == ["stop"] and started == [HEAD, OLD], "stopped, never run beside it"
    assert f"the background smoke of {HEAD[:12]} stopped: main is now {OLD[:12]}" in env.log
    assert json.loads((job / ge.SMOKE_BG).read_text())["sha"] == OLD


def _clocked(env: Any, wait: float) -> list[float]:
    """A real wait of `wait` seconds on a fake clock that each sleep moves on; returns the sleeps."""
    now, sleeps = [1000.0], []
    env.smoke_wait, env.monotonic = wait, lambda: now[0]

    def sleep(seconds: float) -> None:
        sleeps.append(seconds)
        now[0] += seconds

    env.sleep = sleep
    return sleeps


def test_a_call_waits_for_the_background_result_up_to_its_budget(job: Path, tmp_path: Path) -> None:
    env, _ = _env(tmp_path)
    _background(env, job, [None, None, _bg_result()], [True])
    sleeps = _clocked(env, 12)
    assert env.smoke_main() == (True, HEAD) and sleeps == [ge.SMOKE_POLL, ge.SMOKE_POLL] and not env.smoke_pending
    env, _ = _env(tmp_path)
    (job / ge.SMOKE_PRE).unlink()
    (job / ge.SMOKE_BG).unlink()
    _background(env, job, [], [True])
    sleeps = _clocked(env, 12)
    assert env.smoke_main() == (False, HEAD) and env.smoke_pending and sleeps == [5, 5, 2], "the whole budget, then pending"
    assert ge.SMOKE_WAIT + 120 <= 600 and env.__class__(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x").smoke_wait == ge.SMOKE_WAIT


def test_stop_background_smoke_ends_both_process_groups_and_the_record(job: Path, tmp_path: Path) -> None:
    parent = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"], start_new_session=True)
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"], start_new_session=True)
    try:
        job.mkdir(parents=True, exist_ok=True)
        (job / ge.SMOKE_BG).write_text(json.dumps({"sha": HEAD, "job": "77-1", "pid": parent.pid, "at": "x"}))
        (job / ge.SMOKE_BG_CHILD).write_text(str(child.pid))
        assert ge.stop_background_smoke() == "stopped"
        assert parent.wait(timeout=10) is not None and child.wait(timeout=10) is not None
        assert not (job / ge.SMOKE_BG).exists() and not (job / ge.SMOKE_BG_CHILD).exists()
        assert ge.stop_background_smoke() == "none"
    finally:
        for proc in (parent, child):
            proc.kill()
            proc.wait()


def test_a_background_smoke_says_where_its_smoke_sh_runs(job: Path, tmp_path: Path, monkeypatch: Any) -> None:
    out = tmp_path / "logs" / "out.log"
    assert ge.run_pre_smoke(["sh", "-c", "exit 0"], tmp_path, 30, out) == 0 and not (job / ge.SMOKE_BG_CHILD).exists()
    monkeypatch.setenv(ge.PRE_SMOKE_HEAD, HEAD)
    assert ge.run_pre_smoke(["sh", "-c", "exit 0"], tmp_path, 30, out) == 0
    assert int((job / ge.SMOKE_BG_CHILD).read_text()) > 0


def test_own_smoke_for_is_this_jobs_result_for_the_head_or_its_running_smoke(job: Path, tmp_path: Path, monkeypatch: Any) -> None:
    env, _ = _env(tmp_path)
    env.bg_alive = lambda pid: False  # type: ignore[assignment]  # reason: no real process
    assert env.own_smoke_for(HEAD) is False
    _pre(env, code=1)
    env.pre_smoke()
    assert env.own_smoke_for(HEAD) is True and env.own_smoke_for(OLD) is False
    (job / ge.SMOKE_BG).write_text(json.dumps({"sha": HEAD, "job": "77-1", "pid": 4242}))
    env.bg_alive = lambda pid: pid == 4242  # type: ignore[assignment]  # reason: a running smoke of any head counts
    assert env.own_smoke_for(OLD) is True
    monkeypatch.setenv("GITHUB_RUN_ATTEMPT", "2")
    assert env.own_smoke_for(HEAD) is False, "another job's files are not this job's smoke"


def test_without_the_switch_or_outside_actions_the_smoke_runs_in_the_call(job: Path, tmp_path: Path, monkeypatch: Any) -> None:
    env, calls = _env(tmp_path)
    assert env.smoke_background is False and env.smoke_main() == (True, HEAD) and ["bash", "scripts/smoke.sh"] in calls
    monkeypatch.setenv(ge.SMOKE_BACKGROUND, "1")
    monkeypatch.delenv("GITHUB_RUN_ID")
    env, calls = _env(tmp_path)
    assert env.smoke_background is True and env.smoke_main() == (True, HEAD) and ["bash", "scripts/smoke.sh"] in calls


def test_the_background_pre_smoke_smokes_the_given_head_and_says_so(job: Path, tmp_path: Path, monkeypatch: Any) -> None:
    monkeypatch.setenv(ge.PRE_SMOKE_HEAD, OLD)
    env, calls = _env(tmp_path)
    _pre(env)
    assert env.pre_smoke().startswith(f"pre-session smoke on {OLD[:12]}: exit 0;")
    assert ["git", "worktree", "add", "--detach", str(ge.smoke_tree_dir()), OLD] in calls
    got = json.loads((job / ge.SMOKE_PRE).read_text())
    assert got["sha"] == OLD and got["source"] == ge.BACKGROUND


def test_in_the_cycle_a_pending_then_finished_background_smoke_claims_on_the_second_call(job: Path, tmp_path: Path) -> None:
    real, _ = _env(tmp_path)
    _background(real, job, [None], [True])
    env, cfg = PreSmokeEnv(real), config([make_task("T001")], tmp_path)

    def smoke() -> tuple[bool, str]:
        ok, head = real.smoke_main()
        env.smoke_timed_out, env.smoke_source, env.smoke_seconds, env.smoke_pending = (
            real.smoke_timed_out, real.smoke_source, real.smoke_seconds, real.smoke_pending)
        return ok, head

    env.smoke_main = smoke  # type: ignore[method-assign]  # reason: carries smoke_pending too
    assert cycle.run_cycle(cfg, env, NOW) == cycle.SMOKE_PENDING and env.runs == []
    _background(real, job, [_bg_result()], [True])
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=9)) == st.MERGE_PENDING and env.runs[0][0] == "T001"
    assert env.store["smoke_runs"][-1]["why"] == "start routine (background)" and env.store["smoke_runs"][-1]["seconds"] == 611


def test_process_alive_reaps_a_finished_child_and_knows_a_missing_pid() -> None:
    proc = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
    try:
        assert ge.process_alive(proc.pid) is True
    finally:
        proc.kill()
        proc.wait()
    done = subprocess.Popen([sys.executable, "-c", "pass"])
    for _ in range(100):
        if not ge.process_alive(done.pid):
            break
        subprocess.run(["sleep", "0.05"], check=True)
    assert ge.process_alive(done.pid) is False


# ---- the pre-session step skips a smoke the start routine will reuse


def _state_file(tmp_path: Path, monkeypatch: Any, state: st.State) -> None:
    path = tmp_path / "pre_smoke_state.json"
    path.write_text(json.dumps(state))
    monkeypatch.setenv(ge.PRE_SMOKE_STATE, str(path))


def test_the_step_skips_the_smoke_when_a_reusable_green_is_recorded(job: Path, tmp_path: Path, monkeypatch: Any) -> None:
    env, calls = _env(tmp_path)
    env.clock = lambda: NOW
    seen = _pre(env)
    _state_file(tmp_path, monkeypatch, _state(_run(HEAD, at=NOW - timedelta(hours=2))))
    line = env.pre_smoke()
    assert line == (f"pre-session smoke skipped on {HEAD[:12]}: a green smoke of this head is recorded "
                    f"({st.iso(NOW - timedelta(hours=2))}); the session reuses it")
    assert seen == [] and not (job / ge.SMOKE_PRE).exists() and not [c for c in calls if c[:2] == ["git", "worktree"]]


@pytest.mark.parametrize("why", ["red_main", "later_red", "other_head", "too_old", "no_file", "broken_file"])
def test_the_step_smokes_when_nothing_reusable_is_recorded(job: Path, tmp_path: Path, monkeypatch: Any, why: str) -> None:
    env, _ = _env(tmp_path)
    env.clock = lambda: NOW
    seen = _pre(env)
    red = {"check": "nightly", "sha": HEAD, "task": "", "revert_sha": "", "files": [], "set_at": st.iso(NOW)}
    states = {"red_main": _state(_run(HEAD), main_red=red), "later_red": _state(_run(HEAD), _run(HEAD, ok=False)),
              "other_head": _state(_run(OLD)), "too_old": _state(_run(HEAD, at=NOW - timedelta(hours=7)))}
    if why in states:
        _state_file(tmp_path, monkeypatch, states[why])
    elif why == "broken_file":
        (tmp_path / "broken.json").write_text("{not json")
        monkeypatch.setenv(ge.PRE_SMOKE_STATE, str(tmp_path / "broken.json"))
    assert env.pre_smoke().startswith(f"pre-session smoke on {HEAD[:12]}: exit 0;") and len(seen) == 1


def test_the_workflow_reads_the_state_for_the_step_and_lets_the_session_collect_a_background_smoke() -> None:
    steps = _flow()["jobs"]["backup"]["steps"]
    order = [step.get("name") or step.get("uses") for step in steps]
    read = steps[order.index("Read the recorded smoke runs for the pre-session smoke (D-2560)")]
    pre = steps[order.index("Run the start-routine smoke before the session (D-2028)")]
    assert order.index("Warm the smoke build before the session (D-2007)") < order.index(read["name"]) < order.index(pre["name"])
    assert read["continue-on-error"] is True and "secrets." not in json.dumps(read)
    assert "ref=claude/kit-state" in read["run"] and "pre_smoke_state.json" in read["run"] and "git push" not in read["run"]
    assert pre["env"][ge.PRE_SMOKE_STATE] == "${{ runner.temp }}/pre_smoke_state.json"
    session = steps[order.index("anthropics/claude-code-action@v1")]
    assert session["env"][ge.SMOKE_BACKGROUND] == "1"
    assert "If the result is `smoke_pending`" in session["with"]["prompt"] and "run step 1 again" in session["with"]["prompt"]


def test_start_background_smoke_detaches_a_pre_smoke_of_the_head(job: Path, tmp_path: Path, monkeypatch: Any) -> None:
    seen: dict[str, Any] = {}

    class Proc:
        pid = 4343

        def __init__(self, args: list[str], **kw: Any) -> None:
            seen.update(args=args, **kw)

    monkeypatch.setattr(ge.subprocess, "Popen", Proc)
    assert ge.start_background_smoke(OLD, tmp_path) == 4343
    assert seen["args"] == [sys.executable, "-m", "runner", "--pre-smoke"] and seen["cwd"] == tmp_path
    assert seen["start_new_session"] is True and seen["stdin"] == subprocess.DEVNULL, "outlives the runner call and its Bash call"
    assert seen["env"][ge.PRE_SMOKE_HEAD] == OLD and seen["env"]["GITHUB_RUN_ID"] == "77", "the same job, so the session takes its result"
    assert Path(seen["stdout"].name) == job / ge.SMOKE_BG_LOG
    ge._BACKGROUND.clear()


# ---- a job's own red is never replaced by another lane's green (review H1)


class OwnSmokeEnv(PreSmokeEnv):
    """PreSmokeEnv that also answers `own_smoke_for` from the real GitHubEnv, as the session's env does."""

    def own_smoke_for(self, head: str) -> bool:
        return bool(self.real.own_smoke_for(head))


def _own_red(job: Path, tmp_path: Path) -> tuple[OwnSmokeEnv, Any]:
    real, calls = _env(tmp_path, smoke=(0, "must not run"))
    _pre(real, code=101, lines="error: test failed, to rerun pass `--test call_pins`\n")
    real.pre_smoke()
    env, cfg = OwnSmokeEnv(real), config([make_task("T001")], tmp_path)
    env.store = dict(env.store, smoke_runs=[_run(HEAD, at=NOW - timedelta(minutes=5), why="start routine (pre-session)")])
    return env, cfg


@pytest.mark.parametrize("checks", [None, True])
def test_a_red_result_of_this_job_beats_another_lanes_green_on_the_same_head(job: Path, tmp_path: Path, checks: Any) -> None:
    env, cfg = _own_red(job, tmp_path)
    env.checks_green = checks
    result = cycle.run_cycle(cfg, env, NOW)
    assert env.runs == [] and env.store["smoke_runs"][-1]["ok"] is False
    if checks:
        assert result == "smoke_env" and env.store["smoke_env_problems"][-1]["sha"] == HEAD, "a D-498 strike"
    else:
        assert result == "main_red" and env.store["main_red"]["sha"] == HEAD


def test_after_a_lost_race_to_a_reuse_the_re_read_still_judges_the_own_red(job: Path, tmp_path: Path) -> None:
    env, cfg = _own_red(job, tmp_path)
    plain = env.push_state

    def push(new: st.State, base: st.State) -> bool:
        if (new.get("smoke_runs") or [{}])[-1].get("ok") is False and not env.store.get("raced"):
            reuse = _run(HEAD, why="start routine (reused x)", reused=st.iso(NOW - timedelta(minutes=5)))
            env.store = dict(env.store, raced=True, smoke_runs=[*env.store["smoke_runs"], reuse])
            return False
        return plain(new, base)

    env.push_state = push  # type: ignore[method-assign]  # reason: lane 2 writes its reuse first
    assert cycle.run_cycle(cfg, env, NOW) == "lost_race"
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=1)) == "main_red" and env.runs == []
    assert env.store["smoke_runs"][-1]["ok"] is False and env.store["main_red"]["check"] == "smoke"


def test_clear_main_red_never_asks_for_a_reuse(tmp_path: Path, monkeypatch: Any) -> None:
    env, cfg = go_red(tmp_path)
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == "fix_main"
    env.prs[st.FIX_MAIN] = green_pr(sha="revert01", tree="t-revert")
    env.head = "main0002"
    env.store = dict(env.store, smoke_runs=[_run("main0002", at=NOW + timedelta(minutes=12))])
    for name in ("reusable_smoke",):
        monkeypatch.setattr(st, name, lambda *a: pytest.fail("clear_main_red asked for a reuse"))
    monkeypatch.setattr(cycle, "_start_smoke", lambda *a: pytest.fail("clear_main_red took the start routine's path"))
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=20)) == "main_cleared" and env.smokes[-1] == "main0002"


def test_process_alive_reads_an_unreaped_zombie_as_dead(monkeypatch: Any) -> None:
    proc = subprocess.Popen([sys.executable, "-c", "pass"])
    stat = Path(f"/proc/{proc.pid}/stat")
    for _ in range(200):
        if stat.read_text().rsplit(")", 1)[-1].split()[0] == "Z":
            break
        subprocess.run(["sleep", "0.02"], check=True)
    monkeypatch.setattr(ge.os, "waitpid", lambda *a: (_ for _ in ()).throw(ChildProcessError()))
    try:
        assert ge.process_alive(proc.pid) is False
    finally:
        monkeypatch.undo()
        proc.wait()


def test_the_cache_save_is_skipped_when_a_background_smoke_had_to_be_stopped() -> None:
    steps = _flow()["jobs"]["backup"]["steps"]
    order = [step.get("name") or step.get("uses") for step in steps]
    stop = steps[order.index("Stop a background smoke before the cache save (D-2560)")]
    save = steps[order.index("Smoke build cache, save (D-1911)")]
    assert order.index("anthropics/claude-code-action@v1") < order.index(stop["name"]) < order.index(save["name"])
    assert stop["id"] == "bg-smoke" and stop["if"].startswith("always()") and stop["continue-on-error"] is True
    assert "stop_background_smoke" in stop["run"] and "stopped=" in stop["run"]
    assert save["if"].endswith("&& steps.bg-smoke.outputs.stopped != 'stopped'")
