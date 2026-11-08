"""The start routine's smoke run before the session (D-2028): `python3 -m runner --pre-smoke` runs scripts/smoke.sh
on main's head in the workflow step "Run the start-routine smoke before the session", where no 600-second Bash call
caps it, and the session's `smoke_main` takes that result for the same head in the same job instead of running the
smoke again. A red or timed-out result has exactly the consequences of one run in the session. Fake data only; no
real cargo, network or state branch."""

from __future__ import annotations

import json
import subprocess
import time
from pathlib import Path
from typing import Any

import pytest

from runner import cycle, github_env as ge, state as st
from test_failure_drills import NOW, FakeEnv, config, make_task
from test_runner_keeper import _flow, _index

HEAD = "e" * 40


@pytest.fixture
def job(monkeypatch: Any, tmp_path: Path) -> Path:
    """This test's Actions job (run 77, attempt 1), its RUNNER_TEMP and `target`; returns the target dir."""
    monkeypatch.setenv("RUNNER_TEMP", str(tmp_path / "temp"))
    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path / "target"))
    monkeypatch.setenv("GITHUB_RUN_ID", "77")
    monkeypatch.setenv("GITHUB_RUN_ATTEMPT", "1")
    return tmp_path / "target"


def _env(tmp_path: Path, smoke: tuple[int, str] = (0, "smoke: all green")) -> tuple[Any, list[list[str]]]:
    """A GitHubEnv whose fake git checks out HEAD into the smoke tree and whose in-session smoke.sh answers `smoke`."""
    tree, calls = ge.smoke_tree_dir(), []

    def run(a: list[str], i: str | None, c: Path) -> tuple[int, str, str]:
        calls.append(list(a))
        if a[:3] == ["git", "worktree", "add"]:
            _index(tree, {"Cargo.toml": "w1"})
        if a[:2] == ["git", "ls-files"]:
            return 0, "100644 " + "w" * 40 + " 0\tCargo.toml\0", ""
        if a[:2] == ["git", "rev-parse"]:
            return 0, HEAD + "\n", ""
        if a[:2] == ["bash", "scripts/smoke.sh"]:
            return smoke[0], smoke[1], ""
        return 0, "", ""

    env = ge.GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=run)
    env.main_head = lambda: HEAD  # type: ignore[method-assign]  # reason: no remote in this test
    return env, calls


def _pre(env: Any, code: int = 0, timeout: bool = False, lines: str = "smoke: all green\n") -> list[tuple[list[str], Path, float]]:
    """Replace `pre_run` with a fake smoke.sh that writes `lines`, exits `code` or outruns its budget."""
    seen: list[tuple[list[str], Path, float]] = []

    def pre_run(args: list[str], cwd: Path, budget: float, out: Path) -> int:
        seen.append((args, cwd, budget))
        out.write_text(lines)
        if timeout:
            raise subprocess.TimeoutExpired(args, budget)
        return code

    env.pre_run = pre_run
    return seen


def _result(target: Path) -> dict[str, Any]:
    return json.loads((target / ge.SMOKE_PRE).read_text())


def test_the_pre_smoke_runs_smoke_sh_in_the_smokes_own_tree_and_records_this_job(job: Path, tmp_path: Path) -> None:
    env, calls = _env(tmp_path)
    seen = _pre(env)
    line = env.pre_smoke()
    assert line.startswith(f"pre-session smoke on {HEAD[:12]}: exit 0; smoke: all green; ")
    tree = ge.smoke_tree_dir()
    assert seen == [(["bash", "scripts/smoke.sh"], tree, ge.PRE_SMOKE_TIMEOUT)]
    add = calls.index(["git", "worktree", "add", "--detach", str(tree), HEAD])
    assert calls.index(["git", "worktree", "remove", "--force", str(tree)]) < add < calls.index(["git", "ls-files", "-s", "-z"])
    assert ["bash", "scripts/smoke.sh"] not in calls, "only through pre_run, with no 600-second cap"
    assert not [c for c in calls if c[:2] in (["git", "push"], ["git", "fetch"]) or c[:1] == ["gh"]], "no state, lease or network"
    got = _result(job)
    assert {k: got[k] for k in ("sha", "ok", "timed_out", "exit", "job", "last")} == {
        "sha": HEAD, "ok": True, "timed_out": False, "exit": 0, "job": "77-1", "last": "smoke: all green"}
    assert isinstance(got["seconds"], int) and got["at"].endswith("Z")
    assert (job / "smoke" / ge.SMOKE_MTIMES).exists() and not tree.exists()
    assert not (job / ge.SMOKE_RAN).exists(), "a finished smoke leaves the cache judgement to the warm-up (D-2007)"


def test_the_session_uses_the_result_for_the_same_head_and_job_without_running_the_smoke(job: Path, tmp_path: Path) -> None:
    env, calls = _env(tmp_path, smoke=(1, "must not run"))
    _pre(env)
    env.pre_smoke()
    calls.clear()
    assert env.smoke_main() == (True, HEAD)
    assert calls == [], "no tree, no smoke.sh: the pre-session run is this cycle's smoke"
    assert env.smoke_source == ge.PRE_SESSION and not env.smoke_timed_out and isinstance(env.smoke_seconds, int)
    assert env.log[-1].startswith(f"smoke on {HEAD[:12]}: exit 0; smoke: all green (pre-session, ")


@pytest.mark.parametrize("change", ["sha", "run", "attempt"])
def test_a_result_for_another_head_or_another_job_is_ignored(job: Path, tmp_path: Path, monkeypatch: Any, change: str) -> None:
    env, calls = _env(tmp_path, smoke=(0, "in-session green"))
    _pre(env, code=1, lines="error: red before\n")
    env.pre_smoke()
    if change == "sha":
        env.main_head = lambda: "f" * 40  # type: ignore[method-assign]  # reason: main moved after the step
    else:
        monkeypatch.setenv("GITHUB_RUN_ID" if change == "run" else "GITHUB_RUN_ATTEMPT", "78")
    calls.clear()
    ok, head = env.smoke_main()
    assert ok is True and ["bash", "scripts/smoke.sh"] in calls, "the session runs the smoke itself, as before D-2028"
    assert env.smoke_source == "" and (job / ge.SMOKE_RAN).exists()
    why = f"ran on {HEAD[:12]}, main is now {'f' * 12}" if change == "sha" else "is from another job"
    assert any(why in row and row.endswith(": the smoke runs here") for row in env.log)
    assert env.log[-1] == f"smoke on {head[:12]}: exit 0; in-session green"


@pytest.mark.parametrize("content", [None, "not json", json.dumps({"sha": HEAD, "job": "77-1", "exit": True})])
def test_a_missing_or_broken_result_falls_back_to_the_smoke_in_the_session(job: Path, tmp_path: Path, content: str | None) -> None:
    env, calls = _env(tmp_path)
    if content is not None:
        job.mkdir(parents=True, exist_ok=True)
        (job / ge.SMOKE_PRE).write_text(content)
    assert env.smoke_main() == (True, HEAD) and ["bash", "scripts/smoke.sh"] in calls
    assert "no pre-session smoke result: the smoke runs here" in env.log and env.smoke_source == ""


def test_outside_actions_nothing_is_written_or_read(job: Path, tmp_path: Path, monkeypatch: Any) -> None:
    monkeypatch.delenv("GITHUB_RUN_ID")
    env, calls = _env(tmp_path)
    seen = _pre(env)
    assert env.pre_smoke() == f"pre-session smoke not run on {HEAD[:12]}: no Actions job to tie it to"
    assert seen == [] and not (job / ge.SMOKE_PRE).exists()
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_PRE).write_text(json.dumps({"sha": HEAD, "job": "", "exit": 1, "last": "red"}))
    assert env.smoke_main() == (True, HEAD) and ["bash", "scripts/smoke.sh"] in calls
    assert not [row for row in env.log if "pre-session" in row], "no job, no result used and nothing to explain"
    env.dry_run = True
    monkeypatch.setenv("GITHUB_RUN_ID", "77")
    assert env.pre_smoke() == f"pre-session smoke not run on {HEAD[:12]}: dry-run" and seen == []


def test_a_red_pre_smoke_is_a_red_smoke(job: Path, tmp_path: Path) -> None:
    env, calls = _env(tmp_path)
    _pre(env, code=101, lines="test result: FAILED. 1 failed\nsmoke: FAILED\n")
    assert env.pre_smoke().startswith(f"pre-session smoke on {HEAD[:12]}: exit 101; smoke: FAILED; ")
    assert _result(job)["ok"] is False
    calls.clear()
    assert env.smoke_main() == (False, HEAD) and calls == []
    assert env.smoke_timed_out is False and env.log[-1].startswith(f"smoke on {HEAD[:12]}: exit 101; smoke: FAILED (pre-session, ")


def test_a_pre_smoke_timeout_is_a_smoke_timeout_and_keeps_the_partial_build(job: Path, tmp_path: Path) -> None:
    env, _ = _env(tmp_path)
    _pre(env, timeout=True, lines="   Compiling askhava-core\n")
    line = env.pre_smoke()
    assert f"exit 124; smoke timed out after {ge.PRE_SMOKE_TIMEOUT} seconds;" in line
    assert _result(job)["timed_out"] is True and not ge.smoke_tree_dir().exists()
    assert (job / ge.SMOKE_RAN).exists(), "a smoke that ran out of time may have been compiling: saved (D-1970)"
    assert env.smoke_main() == (False, HEAD) and env.smoke_timed_out is True and env.smoke_source == ge.PRE_SESSION


class PreSmokeEnv(FakeEnv):
    """The drills' FakeEnv whose smoke is the real GitHubEnv.smoke_main over a pre-session result (D-2028)."""

    def __init__(self, real: Any) -> None:
        super().__init__()
        self.real, self.head = real, HEAD

    def smoke_main(self) -> tuple[bool, str]:
        ok, head = self.real.smoke_main()
        self.smoke_timed_out, self.smoke_source, self.smoke_seconds = self.real.smoke_timed_out, self.real.smoke_source, self.real.smoke_seconds
        return ok, head


def _cycle_with(job: Path, tmp_path: Path, **pre: Any) -> tuple[str, PreSmokeEnv]:
    real, calls = _env(tmp_path, smoke=(0, "must not run"))
    _pre(real, **pre)
    real.pre_smoke()
    env, cfg = PreSmokeEnv(real), config([make_task("T001")], tmp_path)
    calls.clear()
    result = cycle.run_cycle(cfg, env, NOW)
    assert ["bash", "scripts/smoke.sh"] not in calls
    return result, env


def test_in_the_cycle_a_green_pre_smoke_claims_and_says_where_it_came_from(job: Path, tmp_path: Path) -> None:
    result, env = _cycle_with(job, tmp_path)
    assert result == st.MERGE_PENDING and env.runs[0][0] == "T001"
    run = env.store["smoke_runs"][-1]
    assert run["why"] == "start routine (pre-session)" and run["ok"] is True and isinstance(run["seconds"], int)


def test_in_the_cycle_a_red_pre_smoke_turns_main_red_as_a_red_smoke_does(job: Path, tmp_path: Path) -> None:
    result, env = _cycle_with(job, tmp_path, code=1)
    assert result == "main_red" and env.store["main_red"]["check"] == "smoke" and env.store["main_red"]["sha"] == HEAD
    assert env.runs == [] and env.store["smoke_runs"][-1]["ok"] is False


def test_in_the_cycle_a_red_pre_smoke_with_green_checks_is_a_smoke_env_strike(job: Path, tmp_path: Path) -> None:
    real, _ = _env(tmp_path)
    _pre(real, code=1)
    real.pre_smoke()
    env, cfg = PreSmokeEnv(real), config([make_task("T001")], tmp_path)
    env.checks_green = True
    assert cycle.run_cycle(cfg, env, NOW) == "smoke_env" and env.store.get("main_red") is None


def test_in_the_cycle_a_timed_out_pre_smoke_is_a_smoke_timeout(job: Path, tmp_path: Path) -> None:
    result, env = _cycle_with(job, tmp_path, timeout=True)
    assert result == "smoke_timeout" and env.store.get("main_red") is None and env.runs == []
    run = env.store["smoke_runs"][-1]
    assert run["timed_out"] is True and run["why"] == "start routine (pre-session)", "the chain step's first_timeout reads this (D-1970)"


def test_run_pre_smoke_uses_the_smoke_environment_writes_its_output_and_kills_the_group(monkeypatch: Any, tmp_path: Path) -> None:
    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path / "target"))
    out = tmp_path / "logs" / ge.SMOKE_PRE_LOG
    assert ge.run_pre_smoke(["sh", "-c", 'echo "$CARGO_TARGET_DIR $CARGO_INCREMENTAL"; echo oops >&2; exit 3'], tmp_path, 30, out) == 3
    assert out.read_text().split() == [str(tmp_path / "target" / "smoke"), "0", "oops"]
    pid_file = tmp_path / "child.pid"
    with pytest.raises(subprocess.TimeoutExpired):
        ge.run_pre_smoke(["sh", "-c", f"sleep 30 & echo $! > {pid_file}; wait"], tmp_path, 1, out)
    child = int(pid_file.read_text())
    for _ in range(50):  # SIGKILL is delivered asynchronously: give the kernel up to 5 s to finish the child
        try:
            state = Path(f"/proc/{child}/stat").read_text().split(")")[-1].split()[0]
        except OSError:
            state = "gone"
        if state in ("gone", "Z"):
            break
        time.sleep(0.1)
    assert state in ("gone", "Z"), "a background child (a test binary, rustc) is killed with the group"


def test_the_pre_smoke_cli_never_reads_state_and_always_exits_zero(job: Path, monkeypatch: Any, capsys: Any) -> None:
    from runner import __main__ as entry

    assert entry.parse_args(["--pre-smoke"]).pre_smoke is True
    monkeypatch.setattr(entry, "build", lambda *a: (_ for _ in ()).throw(AssertionError("no cycle")))
    monkeypatch.setattr(ge.GitHubEnv, "pre_smoke", lambda self: (_ for _ in ()).throw(RuntimeError("git worktree add failed")))
    assert entry.main(["--pre-smoke"]) == 0
    assert capsys.readouterr().out.strip().endswith("runner: pre-session smoke failed: git worktree add failed")
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_PRE_LOG).write_text("smoke: all green\n")
    monkeypatch.setattr(ge.GitHubEnv, "pre_smoke", lambda self: "pre-session smoke on x: exit 0")
    assert entry.main(["--pre-smoke"]) == 0
    assert capsys.readouterr().out == "smoke: all green\nrunner: pre-session smoke on x: exit 0\n", "the output reaches the job log"


def test_the_pre_smoke_step_runs_after_the_warm_up_and_before_the_session() -> None:
    """D-2028: in the backup job after the warm-up (which judges whether the build is worth saving) and before the
    session; it can never fail the job, its own time cap is above PRE_SMOKE_TIMEOUT plus the preparation, no secret."""
    steps = _flow()["jobs"]["backup"]["steps"]
    order = [step.get("name") or step.get("uses") for step in steps]
    name = "Run the start-routine smoke before the session (D-2028)"
    pre = steps[order.index(name)]
    assert order.index("Warm the smoke build before the session (D-2007)") < order.index(name)
    assert order.index(name) < order.index("anthropics/claude-code-action@v1") < order.index("Smoke build cache, save (D-1911)")
    assert pre["continue-on-error"] is True and pre["run"].strip() == "python3 -m runner --pre-smoke"
    assert ge.PRE_SMOKE_TIMEOUT / 60 + 5 <= pre["timeout-minutes"] <= 40
    assert pre["if"] == "hashFiles('Cargo.lock') != ''" and "secrets." not in json.dumps(pre)
    assert ge.PRE_SMOKE_TIMEOUT > ge.SMOKE_TIMEOUT


def test_a_seed_or_pin_timeout_in_the_session_is_still_a_recorded_timeout(job: Path, tmp_path: Path) -> None:
    """D-1178's crash mode: a TimeoutExpired before smoke.sh starts must give exit 124 and a duration, not a crash."""
    env, calls = _env(tmp_path)
    base = env._run

    def run(a: list[str], i: str | None, c: Path) -> tuple[int, str, str]:
        if a[:2] == ["git", "ls-files"]:
            raise subprocess.TimeoutExpired(a, 600)
        return base(a, i, c)

    env._run = run
    assert env.smoke_main() == (False, HEAD) and env.smoke_timed_out is True
    assert isinstance(env.smoke_seconds, int) and ["bash", "scripts/smoke.sh"] not in calls
    assert env.log[-1] == f"smoke on {HEAD[:12]}: exit 124; smoke timed out after {ge.SMOKE_TIMEOUT} seconds"
    assert not ge.smoke_tree_dir().exists()


def test_a_stale_result_of_this_job_is_gone_when_the_pre_smoke_fails(job: Path, tmp_path: Path) -> None:
    """The old result is removed before the run, so a step that dies leaves nothing the session could take."""
    env, calls = _env(tmp_path, smoke=(1, "in-session red"))
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_PRE).write_text(json.dumps({"sha": HEAD, "job": "77-1", "exit": 0, "last": "old green"}))

    def broken(args: list[str], cwd: Path, budget: float, out: Path) -> int:
        raise OSError("disk full")

    env.pre_run = broken
    with pytest.raises(OSError):
        env.pre_smoke()
    assert not (job / ge.SMOKE_PRE).exists() and not ge.smoke_tree_dir().exists()
    assert env.smoke_main() == (False, HEAD) and ["bash", "scripts/smoke.sh"] in calls, "judged by a fresh smoke"


def test_the_result_is_written_whole_and_its_last_line_redacted(job: Path, tmp_path: Path) -> None:
    token = "gh" + "p_" + "A" * 30  # built at run time: a fake token shape, never a real one
    env, _ = _env(tmp_path)
    _pre(env, code=1, lines=f"error: auth with {token} refused\n")
    env.pre_smoke()
    assert sorted(p.name for p in job.glob("smoke-pre*")) == sorted([ge.SMOKE_PRE, ge.SMOKE_PRE_LOG]), "no .part file left"
    assert _result(job)["last"] == "error: auth with [redacted] refused"
    raw = json.loads((job / ge.SMOKE_PRE).read_text())
    (job / ge.SMOKE_PRE).write_text(json.dumps(dict(raw, last=f"leak {token}")))
    env.smoke_main()
    assert token not in "\n".join(env.log) and "leak [redacted]" in env.log[-1]
