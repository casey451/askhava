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


def test_one_for_an_older_head_still_running_is_waited_out_never_doubled(job: Path, tmp_path: Path) -> None:
    env, _ = _env(tmp_path)
    started = _background(env, job, [None], [True])
    env.smoke_main()
    env.main_head = lambda: OLD  # type: ignore[method-assign]  # reason: main moved while it ran
    assert env.smoke_main() == (False, OLD) and env.smoke_pending and started == [HEAD]
    assert env.log[-1] == f"the background smoke of {HEAD[:12]} still runs; main is now {OLD[:12]}"


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
