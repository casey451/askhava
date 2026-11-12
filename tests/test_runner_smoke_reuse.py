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
    assert env.log[-1] == f"smoke on {HEAD[:12]}: still running in the background (0 s per runner call)"
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
    env.smoke_wait, env.monotonic, env.wait_from = wait, lambda: now[0], now[0]

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


# ---- re-review: ordering, stopping, one wait per call, a stale child pid, reuse after a stale smoke


def test_own_smoke_for_sees_a_red_written_just_before_the_smoke_ended(job: Path, tmp_path: Path) -> None:
    env, _ = _env(tmp_path)
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_BG).write_text(json.dumps({"sha": HEAD, "job": "77-1", "pid": 4242}))

    def ends(pid: int) -> bool:
        ge.write_pre_smoke(_bg_result(code=101))  # writes red, then is gone
        return False

    env.bg_alive = ends  # type: ignore[assignment]  # reason: the race between the two reads
    assert env.own_smoke_for(HEAD) is True, "never claims beside its own red"


def test_a_smoke_that_wrote_its_result_and_ended_is_used_not_lost(job: Path, tmp_path: Path) -> None:
    env, _ = _env(tmp_path)
    env.smoke_background = True
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_BG).write_text(json.dumps({"sha": HEAD, "job": "77-1", "pid": 4242}))

    def ends(pid: int) -> bool:
        ge.write_pre_smoke(_bg_result(code=101))  # after smoke_main's first look: writes red, then is gone
        return False

    env.bg_alive = ends  # type: ignore[assignment]  # reason: the race between the two reads
    env.bg_start = lambda head, root: pytest.fail("its result is there")  # type: ignore[assignment]
    assert env.smoke_main() == (False, HEAD) and not env.smoke_timed_out, "a red result, not 'no verdict'"
    assert env.smoke_source == ge.BACKGROUND and "the background smoke ended without a result" not in env.log[-1]


def test_stop_background_smoke_kills_a_smoke_sh_group_that_ignores_sigterm(job: Path, tmp_path: Path) -> None:
    ready = tmp_path / "ready"
    ignore = f"import pathlib, signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); pathlib.Path({str(ready)!r}).write_text('1'); time.sleep(60)"
    parent = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"], start_new_session=True)
    child = subprocess.Popen([sys.executable, "-c", ignore], start_new_session=True)
    try:
        for _ in range(200):
            if ready.exists():
                break
            subprocess.run(["sleep", "0.02"], check=True)
        job.mkdir(parents=True, exist_ok=True)
        (job / ge.SMOKE_BG).write_text(json.dumps({"sha": HEAD, "job": "77-1", "pid": parent.pid}))
        (job / ge.SMOKE_BG_CHILD).write_text(str(child.pid))
        assert ge.stop_background_smoke(sleep=lambda s: subprocess.run(["sleep", "0.05"], check=True)) == "stopped"
        assert child.wait(timeout=10) == -9, "SIGKILL reached the group that ignored SIGTERM"
        assert parent.wait(timeout=10) is not None
    finally:
        for proc in (parent, child):
            proc.kill()
            proc.wait()


def test_a_lost_race_retry_in_the_same_call_does_not_wait_again(job: Path, tmp_path: Path) -> None:
    env, _ = _env(tmp_path)
    _background(env, job, [], [True])
    sleeps = _clocked(env, 12)
    assert env.smoke_main() == (False, HEAD) and env.smoke_pending and sum(sleeps) == 12
    assert env.smoke_main() == (False, HEAD) and env.smoke_pending and sum(sleeps) == 12, "no second wait in one call"


def test_a_stale_child_pid_is_removed_before_a_new_smoke_starts(job: Path, tmp_path: Path) -> None:
    env, _ = _env(tmp_path)
    _background(env, job, [_bg_result()], [False])
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_BG_CHILD).write_text("99999")

    def start(head: str, root: Path) -> int:
        assert not (job / ge.SMOKE_BG_CHILD).exists(), "an old smoke.sh pid must never be signalled for the new smoke"
        return 4242

    env.bg_start = start  # type: ignore[assignment]  # reason: checks the order
    assert env.smoke_main() == (True, HEAD)


class SettlingEnv(OwnSmokeEnv):
    """OwnSmokeEnv that also stops a smoke of another head first, as the session's env does."""

    def settle_background_smoke(self, head: str) -> None:
        self.real.settle_background_smoke(head)


def test_a_smoke_of_an_older_head_is_stopped_and_another_lanes_green_on_the_new_head_stands(job: Path, tmp_path: Path) -> None:
    real, calls = _env(tmp_path, smoke=(1, "must not run"))
    real.smoke_background = True
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_BG).write_text(json.dumps({"sha": OLD, "job": "77-1", "pid": 4242}))
    running = [True]
    real.bg_alive = lambda pid: running[0]  # type: ignore[assignment]  # reason: no real process
    real.bg_stop = lambda: running.__setitem__(0, False) or "stopped"  # type: ignore[assignment]  # reason: no real process
    real.bg_start = lambda head, root: pytest.fail("a reusable green needs no new smoke")  # type: ignore[assignment]
    env, cfg = SettlingEnv(real), config([make_task("T001")], tmp_path)
    since = st.iso(NOW - timedelta(minutes=5))
    env.store = dict(env.store, smoke_runs=[_run(HEAD, at=since)])
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING and env.runs[0][0] == "T001"
    assert running == [False] and env.store["smoke_runs"][-1]["reused"] == since
    assert f"the background smoke of {OLD[:12]} stopped: main is now {HEAD[:12]}" in real.log


def test_a_smoke_of_the_head_itself_still_running_is_never_claimed_beside(job: Path, tmp_path: Path) -> None:
    real, _ = _env(tmp_path)
    real.smoke_background = True
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_BG).write_text(json.dumps({"sha": HEAD, "job": "77-1", "pid": 4242}))
    real.bg_alive = lambda pid: True  # type: ignore[assignment]  # reason: no real process
    real.bg_stop = lambda: pytest.fail("its own head's smoke is collected, not stopped")  # type: ignore[assignment]
    real.smoke_wait = 0
    env, cfg = SettlingEnv(real), config([make_task("T001")], tmp_path)

    def smoke() -> tuple[bool, str]:
        ok, head = real.smoke_main()
        env.smoke_timed_out, env.smoke_pending = real.smoke_timed_out, real.smoke_pending
        return ok, head

    env.smoke_main = smoke  # type: ignore[method-assign]  # reason: carries smoke_pending
    env.store = dict(env.store, smoke_runs=[_run(HEAD)])
    assert cycle.run_cycle(cfg, env, NOW) == cycle.SMOKE_PENDING and env.runs == []


# ---- D-3058: a real green on an ancestor stands across DONE tasks' squash merges, and a duplicate smoke yields

CHAIN = [c * 40 for c in "0123456789"]  # main's first-parent history, head first: CHAIN[0] is the head


def _merged(*shas: str, **extra: Any) -> list[dict[str, Any]]:
    """DONE tasks whose recorded squash commits (`merged_sha`) are `shas`."""
    return [dict(make_task(f"T{900 + i}"), status=st.DONE, attempts=1, merged_sha=sha, merged_at=st.iso(NOW), **extra)
            for i, sha in enumerate(shas)]


def _ancestry_state(distance: int, *runs: dict[str, Any], **extra: Any) -> st.State:
    """A real green run on CHAIN[distance] 30 minutes ago and DONE task merges on CHAIN[0] to CHAIN[distance - 1]."""
    base = _run(CHAIN[distance], at=NOW - timedelta(minutes=30))
    return dict(_state(base, *runs), tasks=_merged(*CHAIN[:distance]), **extra)


class AncestryEnv(FakeEnv):
    """A FakeEnv on CHAIN[0] that answers `main_first_parents` from CHAIN and `main`'s checks per commit."""

    def __init__(self) -> None:
        super().__init__()
        self.head, self.chain, self.checks_by_sha = CHAIN[0], list(CHAIN), {}
        self.asked: list[tuple[str, int]] = []

    def main_first_parents(self, head: str, count: int) -> list[str] | None:
        self.asked.append((head, count))
        return self.chain[:count]

    def main_checks_green(self, head: str) -> bool | None:
        return self.checks_by_sha.get(head, self.checks_green)


@pytest.mark.parametrize("distance", [1, 2, 3])
def test_an_ancestors_real_green_stands_within_n_merges_and_is_recorded(distance: int) -> None:
    env = AncestryEnv()
    env.smoke_green = False  # would turn main red if it ran
    state = _ancestry_state(distance)
    out, ok, head = cycle._start_smoke(state, env, NOW, 3)
    since = st.iso(NOW - timedelta(minutes=30))
    assert (ok, head, env.smokes, env.asked) == (True, CHAIN[0], [], [(CHAIN[0], 4)])
    assert out["smoke_runs"][-1] == {"sha": CHAIN[0], "ok": True, "at": st.iso(NOW), "reused": since, "base": CHAIN[distance],
                                     "why": f"start routine (ancestry reuse of {CHAIN[distance][:12]}, {since})"}


def test_in_the_cycle_an_ancestry_reuse_claims_and_the_setting_off_smokes(tmp_path: Path) -> None:
    env, cfg = AncestryEnv(), config([make_task("T001")], tmp_path)
    env.store = _ancestry_state(2)
    cfg.smoke_ancestry = 3
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING and env.smokes == [] and env.runs[0][0] == "T001"
    assert env.store["smoke_runs"][-1]["base"] == CHAIN[2]
    off = AncestryEnv()
    _, ok, _ = cycle._start_smoke(_ancestry_state(1), off, NOW, 0)
    assert ok is True and off.smokes == [CHAIN[0]] and off.asked == [], "N=0 turns it off: the smoke runs"
    cycle._start_smoke(_ancestry_state(1), off, NOW)
    assert off.smokes == [CHAIN[0]] * 2, "and so does a caller that passes no N"


@pytest.mark.parametrize("limit", [1, 2, 3])
def test_one_merge_past_n_runs_a_fresh_smoke(limit: int) -> None:
    env = AncestryEnv()
    out, ok, _ = cycle._start_smoke(_ancestry_state(limit + 1), env, NOW, limit)
    assert ok is True and env.smokes == [CHAIN[0]] and out["smoke_runs"][-1]["why"] == "start routine"
    assert st.ancestry_smoke(_ancestry_state(limit + 1), CHAIN, set(CHAIN), NOW, limit) is None
    assert st.ancestry_smoke(_ancestry_state(limit), CHAIN, set(CHAIN), NOW, limit) is not None


@pytest.mark.parametrize("gap", ["not_a_task", "fix_main", "empty_handin", "not_done"])
@pytest.mark.parametrize("where", [0, 1])
def test_a_commit_in_between_that_is_not_a_done_tasks_squash_merge_forces_a_smoke(gap: str, where: int) -> None:
    env = AncestryEnv()
    state = _ancestry_state(2)
    task = state["tasks"][where]
    if gap == "not_a_task":
        task["merged_sha"] = "f" * 40  # a plan or harness merge on main is no task's
    elif gap == "fix_main":
        task["id"] = st.FIX_MAIN
    elif gap == "empty_handin":
        task["empty_handin"] = True
    else:
        task["status"] = st.MERGE_PENDING
    cycle._start_smoke(state, env, NOW, 3)
    assert env.smokes == [CHAIN[0]]


def _doubt(state: st.State, env: AncestryEnv, kind: str, sha: str) -> None:
    red = {"check": "smoke", "sha": sha, "task": "", "revert_sha": "", "files": [], "set_at": st.iso(NOW)}
    if kind == "red_run":
        state["smoke_runs"].append(_run(sha, ok=False))
    elif kind == "timed_out_run":
        state["smoke_runs"].append(_run(sha, ok=False, timed_out=True))
    elif kind == "strike":
        state["smoke_env_problems"] = [{"sha": sha, "at": st.iso(NOW), "why": "smoke red while the checks were green"}]
    elif kind == "main_red_history":
        state["main_red_history"] = [dict(red, cleared_at=st.iso(NOW), cleared_on="x")]
    elif kind == "nightly_red":
        env.nightly = {"sha": sha, "conclusion": "failure"}
    else:
        env.checks_by_sha[sha] = False


DOUBTS = ["red_run", "timed_out_run", "strike", "main_red_history", "nightly_red", "checks_red"]


@pytest.mark.parametrize("kind", DOUBTS)
@pytest.mark.parametrize("where", [0, 1, 2])
def test_any_doubt_on_the_head_an_in_between_commit_or_the_base_forces_a_smoke(kind: str, where: int) -> None:
    env, state = AncestryEnv(), _ancestry_state(2)
    _doubt(state, env, kind, CHAIN[where])
    out, _, _ = cycle._start_smoke(state, env, NOW, 3)
    assert env.smokes == [CHAIN[0]] and "base" not in out["smoke_runs"][-1]


@pytest.mark.parametrize("kind", DOUBTS)
def test_a_doubt_on_a_commit_older_than_the_base_changes_nothing(kind: str) -> None:
    env, state = AncestryEnv(), _ancestry_state(2)
    _doubt(state, env, kind, CHAIN[5])
    _, ok, _ = cycle._start_smoke(state, env, NOW, 3)
    assert ok is True and env.smokes == []


def test_never_while_main_is_red() -> None:
    red = {"check": "nightly", "sha": CHAIN[0], "task": "T900", "revert_sha": CHAIN[0], "files": [], "set_at": st.iso(NOW)}
    state = _ancestry_state(1, main_red=red)
    assert st.ancestry_smoke(state, CHAIN, set(CHAIN), NOW, 3) is None
    env = AncestryEnv()
    cycle._start_smoke(state, env, NOW, 3)
    assert env.smokes == [CHAIN[0]]


def test_the_base_must_be_a_real_run_so_a_real_smoke_comes_every_n_plus_one_merges() -> None:
    since = st.iso(NOW - timedelta(minutes=30))
    chained = [_run(CHAIN[d], why="start routine (ancestry reuse)", reused=since, base=CHAIN[3]) for d in (1, 2)]
    state = _ancestry_state(3, *chained)
    assert st.ancestry_smoke(state, CHAIN, set(CHAIN), NOW, 3) == (CHAIN[3], state["smoke_runs"][0])
    assert st.ancestry_smoke(state, CHAIN, set(CHAIN), NOW, 2) is None, "reuses never become a base"
    same_head = _run(CHAIN[1], reused=since)  # a same-head reuse (D-2560) is no real run either
    assert st.ancestry_smoke(dict(_state(same_head), tasks=_merged(CHAIN[0])), CHAIN, set(CHAIN), NOW, 3) is None
    env = AncestryEnv()
    cycle._start_smoke(state, env, NOW, 2)
    assert env.smokes == [CHAIN[0]]


def test_the_base_obeys_the_six_hour_cap_and_every_d2560_rule() -> None:
    old = dict(_state(_run(CHAIN[1], at=NOW - timedelta(hours=6, seconds=1))), tasks=_merged(CHAIN[0]))
    assert st.ancestry_smoke(old, CHAIN, set(CHAIN), NOW, 3) is None
    edge = dict(_state(_run(CHAIN[1], at=NOW - st.SMOKE_REUSE_MAX_AGE)), tasks=_merged(CHAIN[0]))
    assert st.ancestry_smoke(edge, CHAIN, set(CHAIN), NOW, 3) is not None
    refreshed = dict(old, smoke_runs=[*old["smoke_runs"], _run(CHAIN[1], reused=st.iso(NOW))])
    assert st.ancestry_smoke(refreshed, CHAIN, set(CHAIN), NOW, 3) is None, "the real run's age counts, not a later reuse"
    later_red = _ancestry_state(1, _run(CHAIN[1], ok=False))
    assert st.ancestry_smoke(later_red, CHAIN, set(CHAIN), NOW, 3) is None
    future = dict(_state(_run(CHAIN[1], at=NOW + timedelta(minutes=5))), tasks=_merged(CHAIN[0]))
    assert st.ancestry_smoke(future, CHAIN, set(CHAIN), NOW, 3) is None


def test_a_head_with_a_real_run_of_its_own_is_left_to_the_same_head_rule() -> None:
    stale = _ancestry_state(1, _run(CHAIN[0], at=NOW - timedelta(hours=7)))
    assert st.reusable_smoke(stale, CHAIN[0], NOW) is None
    assert st.ancestry_smoke(stale, CHAIN, set(CHAIN), NOW, 3) is None, "an old green on the head is never bridged around"


def test_an_ancestry_reuse_on_the_head_is_judged_again_not_reused_as_is() -> None:
    env, state = AncestryEnv(), _ancestry_state(2)
    out, ok, _ = cycle._start_smoke(state, env, NOW, 3)
    assert ok is True and st.reusable_smoke(out, CHAIN[0], NOW) is None, "it names a base: the same-head rule passes"
    out, ok, _ = cycle._start_smoke(out, env, NOW + timedelta(minutes=1), 3)
    assert ok is True and env.smokes == [] and out["smoke_runs"][-1]["base"] == CHAIN[2]
    env.checks_by_sha[CHAIN[1]] = False
    cycle._start_smoke(out, env, NOW + timedelta(minutes=2), 3)
    assert env.smokes == [CHAIN[0]], "a doubt that arrived since is seen"


@pytest.mark.parametrize("fault", ["raises", "none", "other_head", "missing", "bad_time", "bad_runs"])
def test_any_error_or_unreadable_answer_falls_back_to_a_fresh_smoke(fault: str) -> None:
    env, state = AncestryEnv(), _ancestry_state(1)
    if fault == "raises":
        env.main_first_parents = lambda head, count: (_ for _ in ()).throw(RuntimeError("git"))  # type: ignore[method-assign]
    elif fault == "none":
        env.main_first_parents = lambda head, count: None  # type: ignore[method-assign]
    elif fault == "other_head":
        env.chain = CHAIN[1:]
    elif fault == "missing":
        env = FakeEnv()  # type: ignore[assignment]  # reason: an env with no first-parent reader
        env.head = CHAIN[0]
    elif fault == "bad_time":
        state["smoke_runs"][0]["at"] = "not a time"
    else:
        state["smoke_runs"] = "garbage"
    cycle._start_smoke(state, env, NOW, 3)
    assert env.smokes == [CHAIN[0]]


def test_clearing_a_red_main_never_takes_an_ancestry_reuse(tmp_path: Path, monkeypatch: Any) -> None:
    env, cfg = go_red(tmp_path)
    cfg.smoke_ancestry = 3
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == "fix_main"
    env.prs[st.FIX_MAIN] = green_pr(sha="revert01", tree="t-revert")
    env.head = "main0002"
    monkeypatch.setattr(st, "ancestry_smoke", lambda *a: pytest.fail("clear_main_red asked for an ancestry reuse"))
    env.smoke_green = False
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=15)) == "main_red" and env.smokes[-1] == "main0002"
    env.smoke_green = True
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=20)) == "main_cleared"


# ---- D-3058: the pre-session step skips on an ancestry reuse too


def _with_chain(env: Any, chain: list[str] | None) -> None:
    """The fake git answers `git rev-list --first-parent` with `chain` (None: git fails)."""
    plain = env._run

    def run(a: list[str], i: str | None, c: Path) -> tuple[int, str, str]:
        if a[:3] == ["git", "rev-list", "--first-parent"]:
            return (0, "\n".join(chain) + "\n", "") if chain is not None else (128, "", "fatal: bad object")
        return plain(a, i, c)

    env._run = run


def test_the_step_skips_on_an_ancestry_reuse_when_the_setting_allows_it(job: Path, tmp_path: Path, monkeypatch: Any) -> None:
    env, _ = _env(tmp_path)
    env.clock, env.smoke_ancestry = (lambda: NOW), 3
    seen = _pre(env)
    _with_chain(env, [HEAD, *CHAIN[1:4]])
    since = st.iso(NOW - timedelta(minutes=30))
    _state_file(tmp_path, monkeypatch, dict(_state(_run(CHAIN[2], at=since)), tasks=_merged(HEAD, CHAIN[1])))
    assert env.pre_smoke() == (f"pre-session smoke skipped on {HEAD[:12]}: the green smoke of {CHAIN[2][:12]} ({since}), "
                               "2 proven merge(s) back, stands for it (D-3058, D-3156); the session reuses it")
    assert seen == [] and not (job / ge.SMOKE_PRE).exists()


@pytest.mark.parametrize("why", ["off", "not_a_task", "too_far", "git_fails", "red_between"])
def test_the_step_smokes_when_no_ancestry_reuse_is_allowed(job: Path, tmp_path: Path, monkeypatch: Any, why: str) -> None:
    env, _ = _env(tmp_path)
    env.clock, env.smoke_ancestry = (lambda: NOW), (0 if why == "off" else 1 if why == "too_far" else 3)
    seen = _pre(env)
    _with_chain(env, None if why == "git_fails" else [HEAD, *CHAIN[1:4]])
    tasks = _merged(HEAD) if why == "not_a_task" else _merged(HEAD, CHAIN[1])
    runs = [_run(CHAIN[2]), *([_run(CHAIN[1], ok=False)] if why == "red_between" else [])]
    _state_file(tmp_path, monkeypatch, dict(_state(*runs), tasks=tasks))
    assert env.pre_smoke().startswith(f"pre-session smoke on {HEAD[:12]}: exit 0;") and len(seen) == 1


def test_main_first_parents_reads_git_and_refuses_an_answer_not_from_the_head(tmp_path: Path) -> None:
    env, _ = _env(tmp_path)
    _with_chain(env, [HEAD, CHAIN[1]])
    assert env.main_first_parents(HEAD, 4) == [HEAD, CHAIN[1]]
    assert env.main_first_parents(CHAIN[3], 4) is None
    _with_chain(env, None)
    assert env.main_first_parents(HEAD, 4) is None


def test_the_setting_is_read_from_kit_runner_json_fails_safe_and_is_wired(tmp_path: Path) -> None:
    import inspect

    from runner import __main__ as entry

    f = tmp_path / "runner.json"
    assert entry.smoke_ancestry(f) == 0
    for raw, want in (("{not json", 0), ('{"lanes": 2}', 0), ('{"smoke_ancestry_reuse": 0}', 0),
                      ('{"smoke_ancestry_reuse": 2}', 2), ('{"smoke_ancestry_reuse": -1}', 0),
                      ('{"smoke_ancestry_reuse": true}', 0), ('{"smoke_ancestry_reuse": "3"}', 0), ("[]", 0)):
        f.write_text(raw)
        assert entry.smoke_ancestry(f) == want, raw
    assert entry.smoke_ancestry(Path(__file__).resolve().parent.parent / "kit" / "runner.json") == 5, "D-3058 shipped N=3; D-3156 raised it to 5"
    assert 'smoke_ancestry=smoke_ancestry(ROOT / "kit" / "runner.json")' in inspect.getsource(entry.build)
    assert "env.smoke_ancestry = cfg.smoke_ancestry" in inspect.getsource(entry.build)
    assert 'env.smoke_ancestry = smoke_ancestry(ROOT / "kit" / "runner.json")' in inspect.getsource(entry.pre_smoke)
    assert "cfg.smoke_ancestry" in inspect.getsource(cycle._next_step)


# ---- D-3058: an own smoke still running yields to a fully reusable green another lane recorded on the head


class YieldingEnv(SettlingEnv):
    """SettlingEnv that also offers the real `yield_own_smoke` and carries `smoke_pending`, as the session's env does."""

    def yield_own_smoke(self, head: str) -> bool:
        return bool(self.real.yield_own_smoke(head))

    def smoke_main(self) -> tuple[bool, str]:
        ok, head = self.real.smoke_main()
        self.smoke_timed_out, self.smoke_pending = self.real.smoke_timed_out, self.real.smoke_pending
        return ok, head


def _running_own(job: Path, tmp_path: Path, stop_writes: dict[str, Any] | None = None) -> tuple[YieldingEnv, Any, list[str]]:
    """This job's background smoke of HEAD still running with no result; `bg_stop` stops it (writing `stop_writes`
    first when given, as a smoke that finished while being stopped)."""
    real, _ = _env(tmp_path)
    real.smoke_background, real.smoke_wait = True, 0
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_BG).write_text(json.dumps({"sha": HEAD, "job": "77-1", "pid": 4242}))
    running, stops = [True], []
    real.bg_alive = lambda pid: running[0]  # type: ignore[assignment]  # reason: no real process

    def stop() -> str:
        stops.append("stop")
        if stop_writes is not None:
            ge.write_pre_smoke(stop_writes)
        running[0] = False
        return "stopped"

    real.bg_stop = stop  # type: ignore[assignment]  # reason: no real process
    real.bg_start = lambda head, root: pytest.fail("no second smoke beside the first")  # type: ignore[assignment]
    env, cfg = YieldingEnv(real), config([make_task("T001")], tmp_path)
    return env, cfg, stops


def test_a_running_own_smoke_stops_for_another_lanes_fully_reusable_green(job: Path, tmp_path: Path) -> None:
    env, cfg, stops = _running_own(job, tmp_path)
    since = st.iso(NOW - timedelta(minutes=3))
    env.store = dict(env.store, smoke_runs=[_run(HEAD, at=since, why="start routine (background)")])
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING and env.runs[0][0] == "T001" and stops == ["stop"]
    assert env.store["smoke_runs"][-1]["reused"] == since and not (job / ge.SMOKE_BG).exists()
    assert f"the background smoke of {HEAD[:12]} stopped: a green smoke another lane recorded on it stands (D-3058)" in env.real.log


@pytest.mark.parametrize("why", ["checks_red", "nightly_unfinished", "later_red", "strike", "too_old", "no_green", "ancestry_only"])
def test_a_running_own_smoke_is_never_stopped_for_a_green_that_is_not_fully_reusable(job: Path, tmp_path: Path, why: str) -> None:
    env, cfg, stops = _running_own(job, tmp_path)
    runs = {"later_red": [_run(HEAD), _run(HEAD, ok=False)], "too_old": [_run(HEAD, at=NOW - timedelta(hours=7))],
            "no_green": [], "ancestry_only": [_run(CHAIN[1]), _run(HEAD, reused=st.iso(NOW), base=CHAIN[1])]}.get(why, [_run(HEAD)])
    env.store = dict(env.store, smoke_runs=runs)
    if why == "checks_red":
        env.checks_green = False
    elif why == "nightly_unfinished":
        env.nightly = {"sha": HEAD, "conclusion": None}  # a red one turns main red before any claim, never reaching here
    elif why == "strike":
        env.store["smoke_env_problems"] = [{"sha": HEAD, "at": st.iso(NOW), "why": "smoke red while the checks were green"}]
    elif why == "ancestry_only":
        env.store["tasks"] = [*env.store.get("tasks", []), *_merged(HEAD)]
        cfg.smoke_ancestry = 3
    assert cycle.run_cycle(cfg, env, NOW) == cycle.SMOKE_PENDING and env.runs == [] and stops == []


def test_a_finished_own_result_is_always_judged_never_yielded(job: Path, tmp_path: Path) -> None:
    env, cfg = _own_red(job, tmp_path)
    yields: list[str] = []
    env.yield_own_smoke = lambda head: yields.append(head) or False  # type: ignore[attr-defined]  # reason: counts asks
    assert cycle.run_cycle(cfg, env, NOW) == "main_red" and env.runs == [] and env.store["main_red"]["sha"] == HEAD
    real, _ = _env(tmp_path)
    assert real.yield_own_smoke(HEAD) is False, "a result of this job for the head is never stopped"


def test_a_smoke_that_finishes_while_being_stopped_is_judged(job: Path, tmp_path: Path) -> None:
    env, cfg, stops = _running_own(job, tmp_path, stop_writes=_bg_result(code=101))
    env.store = dict(env.store, smoke_runs=[_run(HEAD, at=NOW - timedelta(minutes=3))])
    assert env.real.yield_own_smoke(HEAD) is False and stops == ["stop"]
    assert cycle.run_cycle(cfg, env, NOW) == "main_red" and env.runs == [] and env.store["smoke_runs"][-1]["ok"] is False


def test_yield_own_smoke_only_stops_this_jobs_live_smoke_of_the_head(job: Path, tmp_path: Path) -> None:
    real, _ = _env(tmp_path)
    real.bg_stop = lambda: pytest.fail("nothing to stop")  # type: ignore[assignment]
    real.bg_alive = lambda pid: True  # type: ignore[assignment]
    assert real.yield_own_smoke(HEAD) is False, "no background smoke"
    job.mkdir(parents=True, exist_ok=True)
    for record in ({"sha": OLD, "job": "77-1", "pid": 4242}, {"sha": HEAD, "job": "78-1", "pid": 4242}, {"sha": HEAD, "job": "77-1"}):
        (job / ge.SMOKE_BG).write_text(json.dumps(record))
        assert real.yield_own_smoke(HEAD) is False, record
    (job / ge.SMOKE_BG).write_text(json.dumps({"sha": HEAD, "job": "77-1", "pid": 4242}))
    real.bg_alive = lambda pid: False  # type: ignore[assignment]
    assert real.yield_own_smoke(HEAD) is False, "an ended smoke is judged as before (D-2560)"
    real.bg_alive = lambda pid: True  # type: ignore[assignment]
    ge.write_pre_smoke(_bg_result())
    assert real.yield_own_smoke(HEAD) is False, "still running, but its result is written: it is judged"


# ---- D-3156: a merge that is no DONE task's (a plan, harness or docs PR) stands in the chain when a commit with its
# identical tree passed Ci, Twin and Ally


class TwinEnv(AncestryEnv):
    """AncestryEnv whose `green_twin_tree` proves the commits in `twins` (a set) and records every question."""

    def __init__(self, twins: set[str] | None = None) -> None:
        super().__init__()
        self.twins = set(twins or ())
        self.proved: list[str] = []

    def green_twin_tree(self, sha: str) -> bool:
        self.proved.append(sha)
        return sha in self.twins


def _plan_merges(state: st.State, *where: int) -> st.State:
    """The DONE tasks recorded on CHAIN[where] get another task's SHA: those commits are plan merges, no task's."""
    for i in where:
        state["tasks"][i]["merged_sha"] = f"{i:x}" * 39 + "f"
    return state


def test_a_plan_squash_whose_identical_tree_is_green_stands_in_the_chain() -> None:
    env = TwinEnv({CHAIN[0], CHAIN[1]})
    env.smoke_green = False  # would turn main red if it ran
    out, ok, _ = cycle._start_smoke(_plan_merges(_ancestry_state(2), 0), env, NOW, 3)
    assert ok is True and env.smokes == [] and out["smoke_runs"][-1]["base"] == CHAIN[2]
    assert env.proved == [CHAIN[0]], "only the commit that is no DONE task's is asked about, and the base never"


@pytest.mark.parametrize("where", [0, 1])
def test_a_plan_squash_not_proven_green_forces_a_smoke(where: int) -> None:
    env = TwinEnv()  # the env answers False: the PR was behind main (a different tree), or a check failed or is missing
    out, _, _ = cycle._start_smoke(_plan_merges(_ancestry_state(2), where), env, NOW, 3)
    assert env.smokes == [CHAIN[0]] and "base" not in out["smoke_runs"][-1] and env.proved == [CHAIN[where]]


def test_an_env_without_the_prover_or_one_that_raises_forces_a_smoke() -> None:
    bare = AncestryEnv()
    cycle._start_smoke(_plan_merges(_ancestry_state(2), 0), bare, NOW, 3)
    assert bare.smokes == [CHAIN[0]], "the drills' env proves nothing"
    env = TwinEnv()
    env.green_twin_tree = lambda sha: (_ for _ in ()).throw(RuntimeError("api"))  # type: ignore[method-assign]
    cycle._start_smoke(_plan_merges(_ancestry_state(2), 0), env, NOW, 3)
    assert env.smokes == [CHAIN[0]]
    odd = TwinEnv()
    odd.green_twin_tree = lambda sha: "yes"  # type: ignore[method-assign,assignment,return-value]
    cycle._start_smoke(_plan_merges(_ancestry_state(2), 0), odd, NOW, 3)
    assert odd.smokes == [CHAIN[0]], "only a real True proves"


@pytest.mark.parametrize("limit, smokes", [(4, []), (5, []), (3, [CHAIN[0]])])
def test_a_mixed_chain_of_task_and_plan_merges_within_the_limit_reuses(limit: int, smokes: list[str]) -> None:
    env = TwinEnv({CHAIN[1], CHAIN[3]})
    state = _plan_merges(_ancestry_state(4), 1, 3)  # head T, plan, T, plan, base
    out, ok, _ = cycle._start_smoke(state, env, NOW, limit)
    assert ok is True and env.smokes == smokes
    assert ("base" in out["smoke_runs"][-1]) is (not smokes)


@pytest.mark.parametrize("kind", DOUBTS)
def test_any_doubt_on_a_proven_plan_merge_still_forces_a_smoke(kind: str) -> None:
    env, state = TwinEnv({CHAIN[1]}), _plan_merges(_ancestry_state(2), 1)
    _doubt(state, env, kind, CHAIN[1])
    cycle._start_smoke(state, env, NOW, 3)
    assert env.smokes == [CHAIN[0]]


def test_a_proven_plan_merge_never_bridges_a_red_main() -> None:
    red = {"check": "smoke", "sha": CHAIN[0], "task": "", "revert_sha": "", "files": [], "set_at": st.iso(NOW)}
    state = _plan_merges(_ancestry_state(1, main_red=red), 0)
    assert st.ancestry_smoke(state, CHAIN, set(), NOW, 3, lambda sha: True) is None
    env = TwinEnv({CHAIN[0]})
    cycle._start_smoke(state, env, NOW, 3)
    assert env.smokes == [CHAIN[0]] and env.proved == []


def test_the_prover_is_never_asked_past_the_limit_nor_about_a_doubted_commit() -> None:
    asked: list[str] = []
    state = _plan_merges(_ancestry_state(4), 0, 1, 2, 3)
    assert st.ancestry_smoke(state, CHAIN, set(), NOW, 3, lambda sha: asked.append(sha) or True) is None
    assert asked == CHAIN[:3], "the commit at the limit cannot be stepped over, so it is not asked about"
    asked.clear()
    state["smoke_runs"].append(_run(CHAIN[0], ok=False))
    assert st.ancestry_smoke(state, CHAIN, set(), NOW, 3, lambda sha: asked.append(sha) or True) is None and asked == []


# ---- D-3156: the real env's `green_twin_tree` (REST: the merged PR, both trees, the PR head's check runs)

TWIN_HEAD = "a" * 40
MAIN_C = "c" * 40
ACTIONS = {"slug": "github-actions"}


def _green_runs(head: str = TWIN_HEAD) -> list[dict[str, Any]]:
    return [{"name": n, "head_sha": head, "conclusion": "success", "status": "completed", "app": ACTIONS}
            for n in ("Ci", "Twin", "Ally")]


def _twin_answers(main: str = MAIN_C, **over: Any) -> dict[str, Any]:
    """REST answers, by path, for `main` squash-merged from a PR whose head is TWIN_HEAD with the same tree."""
    answers: dict[str, Any] = {
        f"repos/o/r/commits/{main}/pulls": [{"number": 7, "merge_commit_sha": main, "merged_at": "2026-11-12T05:40:00Z",
                                             "head": {"sha": TWIN_HEAD}}],
        f"repos/o/r/git/commits/{main}": {"tree": {"sha": "1" * 40}},
        f"repos/o/r/git/commits/{TWIN_HEAD}": {"tree": {"sha": "1" * 40}},
        f"repos/o/r/commits/{TWIN_HEAD}/check-runs?per_page=100&filter=all": {"total_count": 3, "check_runs": _green_runs()},
    }
    answers.update(over)
    return answers


def _with_api(env: Any, answers: dict[str, Any]) -> list[str]:
    """The fake gh answers `gh api <path>` from `answers` (an Exception value: gh fails); records the paths asked."""
    plain, asked = env._run, []

    def run(a: list[str], i: str | None, c: Path) -> tuple[int, str, str]:
        if a[:2] == ["gh", "api"]:
            asked.append(a[-1])
            got = answers.get(a[-1], RuntimeError("404"))
            return (1, "", "HTTP 404") if isinstance(got, Exception) else (0, json.dumps(got), "")
        return plain(a, i, c)

    env._run = run
    return asked


def test_green_twin_tree_proves_a_squash_whose_up_to_date_pr_head_passed_every_check(tmp_path: Path) -> None:
    env, _ = _env(tmp_path)
    asked = _with_api(env, _twin_answers())
    assert env.green_twin_tree(MAIN_C) is True
    assert asked[0] == f"repos/o/r/commits/{MAIN_C}/pulls"
    assert f"repos/o/r/commits/{TWIN_HEAD}/check-runs?per_page=100&filter=all" in asked, "every run, not only the latest"
    stray = [*_green_runs(), dict(_green_runs()[0], app={"slug": "some-bot"}, conclusion="failure")]
    _with_api(env, _twin_answers(**_check_runs(stray)))
    assert env.green_twin_tree(MAIN_C) is True, "another app's run named Ci neither proves nor refuses"


def test_the_smoke_never_gets_the_jobs_tokens(monkeypatch: Any) -> None:
    for name in ("RUNNER_CHECKS_TOKEN", "RUNNER_PUSH_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"):
        monkeypatch.setenv(name, "fake-not-a-token")
    monkeypatch.setenv("SOME_OTHER", "kept")
    env = ge._smoke_env()
    assert not set(ge.SMOKE_DROPPED_ENV) & set(env) and env["SOME_OTHER"] == "kept" and env["CARGO_INCREMENTAL"] == "0"
    smoke = (Path(__file__).resolve().parent.parent / "scripts" / "smoke.sh").read_text()
    assert not any(name in smoke for name in ge.SMOKE_DROPPED_ENV), "smoke.sh needs none of them"


def _check_runs(runs: list[dict[str, Any]], total: int | None = None) -> dict[str, Any]:
    return {f"repos/o/r/commits/{TWIN_HEAD}/check-runs?per_page=100&filter=all": {"total_count": len(runs) if total is None else total,
                                                                       "check_runs": runs}}


def _rerun(name: str, conclusion: str | None) -> list[dict[str, Any]]:
    return [*_green_runs(), {"name": name, "head_sha": TWIN_HEAD, "conclusion": conclusion, "status": "completed", "app": ACTIONS}]


NOT_PROVEN = {
    "tree_differs": {f"repos/o/r/git/commits/{TWIN_HEAD}": {"tree": {"sha": "2" * 40}}},  # the PR was behind main
    "no_tree": {f"repos/o/r/git/commits/{TWIN_HEAD}": {"tree": {"sha": ""}}},
    "ci_missing": _check_runs(_green_runs()[1:]),
    "twin_missing": _check_runs([r for r in _green_runs() if r["name"] != "Twin"]),
    "ally_missing": _check_runs(_green_runs()[:2]),
    "ci_failed": _check_runs([dict(_green_runs()[0], conclusion="failure"), *_green_runs()[1:]]),
    "twin_rerun_failed": _check_runs(_rerun("Twin", "failure")),
    "ally_cancelled": _check_runs(_rerun("Ally", "cancelled")),
    "ci_running": _check_runs(_rerun("Ci", None)),
    "run_on_another_sha": _check_runs([dict(r, head_sha="b" * 40) for r in _green_runs()]),
    "more_than_a_page": _check_runs(_green_runs(), total=101),
    "no_merged_pr": {f"repos/o/r/commits/{MAIN_C}/pulls": [{"merge_commit_sha": "b" * 40, "merged_at": "x", "head": {"sha": TWIN_HEAD}}]},
    "unmerged_pr": {f"repos/o/r/commits/{MAIN_C}/pulls": [{"merge_commit_sha": MAIN_C, "merged_at": None, "head": {"sha": TWIN_HEAD}}]},
    "two_prs": {f"repos/o/r/commits/{MAIN_C}/pulls": [{"merge_commit_sha": MAIN_C, "merged_at": "x", "head": {"sha": TWIN_HEAD}}] * 2},
    "pulls_error": {f"repos/o/r/commits/{MAIN_C}/pulls": RuntimeError("500")},
    "tree_error": {f"repos/o/r/git/commits/{MAIN_C}": RuntimeError("403")},
    "checks_error": {f"repos/o/r/commits/{TWIN_HEAD}/check-runs?per_page=100&filter=all": RuntimeError("403")},
    "garbage": {f"repos/o/r/commits/{MAIN_C}/pulls": {"message": "odd"}},
    "null_checks": {f"repos/o/r/commits/{TWIN_HEAD}/check-runs?per_page=100&filter=all": None},
    "ci_from_another_app": _check_runs([dict(_green_runs()[0], app={"slug": "some-bot"}), *_green_runs()[1:]]),
    "twin_with_no_app": _check_runs([r if r["name"] != "Twin" else {k: v for k, v in r.items() if k != "app"} for r in _green_runs()]),
    "a_run_not_an_object": _check_runs([*_green_runs(), "Ci"]),  # type: ignore[list-item]
}


@pytest.mark.parametrize("why", sorted(NOT_PROVEN))
def test_green_twin_tree_proves_nothing_on_a_different_tree_a_check_short_of_green_or_any_error(tmp_path: Path, why: str) -> None:
    env, _ = _env(tmp_path)
    _with_api(env, _twin_answers(**NOT_PROVEN[why]))
    assert env.green_twin_tree(MAIN_C) is False


def test_the_twin_reads_use_the_job_token_and_nothing_else_moves_to_it() -> None:
    for path in (f"repos/o/r/commits/{MAIN_C}/pulls", f"repos/o/r/git/commits/{MAIN_C}",
                 f"repos/o/r/commits/{TWIN_HEAD}/check-runs?per_page=100&filter=all"):
        assert ge._job_token_read(["gh", "api", "-H", "Accept: application/vnd.github+json", path]), path
    for path in ("repos/o/r/pulls?head=o:claude/task/T001&state=all&per_page=20", "repos/o/r/pulls/7",
                 f"repos/o/r/commits/{MAIN_C}/pulls/extra", "repos/o/r/git/commits/HEAD", f"repos/o/r/commits/{MAIN_C}"):
        assert not ge._job_token_read(["gh", "api", "-H", "Accept: application/vnd.github+json", path]), path
    flow = (Path(__file__).resolve().parent.parent / ".github" / "workflows" / "backup-runner.yml").read_text()
    step = flow.split("- name: Run the start-routine smoke before the session (D-2028)")[1].split("- name:")[0]
    assert "RUNNER_CHECKS_TOKEN: ${{ github.token }}" in step, "the pre-session step can prove a merge too"


def test_the_step_skips_across_a_plan_merge_proven_green_and_smokes_when_not(job: Path, tmp_path: Path, monkeypatch: Any) -> None:
    for proven, want_skip in ((True, True), (False, False)):
        env, _ = _env(tmp_path)
        env.clock, env.smoke_ancestry = (lambda: NOW), 5
        seen = _pre(env)
        _with_chain(env, [HEAD, CHAIN[1], CHAIN[2]])  # main's head is a plan merge, then a DONE task's, then the base
        _with_api(env, _twin_answers(HEAD) if proven else _twin_answers(HEAD, **NOT_PROVEN["tree_differs"]))
        since = st.iso(NOW - timedelta(minutes=30))
        _state_file(tmp_path, monkeypatch, dict(_state(_run(CHAIN[2], at=since)), tasks=_merged(CHAIN[1])))
        line = env.pre_smoke()
        if want_skip:
            assert line == (f"pre-session smoke skipped on {HEAD[:12]}: the green smoke of {CHAIN[2][:12]} ({since}), "
                            "2 proven merge(s) back, stands for it (D-3058, D-3156); the session reuses it") and seen == []
        else:
            assert line.startswith(f"pre-session smoke on {HEAD[:12]}:") and len(seen) == 1
