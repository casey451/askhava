"""`--after` smokes a fix round's head itself (D-3530, T443): a hand-in that followed a fix round (more than one worker
agent id, or a hold already used) gets main's full scripts/smoke.sh on its head outside the model, in the background
as D-2560's `--pre-smoke`, keyed to the head in the local hand-off file. `smoke_pending` pushes and writes nothing; a
red is held once (`smoke_red`), then fails `tests_failed`; a non-verdict is logged and let through; a first-round
hand-in runs none. Fake data only; no real cargo, network or state branch."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import pytest

from runner import __main__ as entry, cycle, github_env as ge, state as st
from test_runner_live import (  # noqa: F401  # reason: `repo` is the shared fixture
    MODELS,
    NOW,
    TASK,
    git,
    live_config,
    make_env,
    repo,
    transcript,
    worker_commit,
)
from test_runner_pre_smoke import HEAD, _env, job  # noqa: F401  # reason: `job` is a fixture used by name
from test_runner_smoke_reuse import OLD, _background, _bg_result

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github" / "workflows" / "backup-runner.yml"
RED_LOG = "compiling\nerror: test failed in crate fake_one ghp_" + "a" * 30 + "\nFAILED: cargo test\n"


def _handoff(root: Path, monkeypatch: pytest.MonkeyPatch, **extra: Any) -> None:
    """A local hand-off file under `root` (the session's checkout) for session-a's K10."""
    monkeypatch.setattr(entry, "ROOT", root)
    (root / ".git").mkdir(exist_ok=True)
    (root / entry.HANDOFF).write_text(json.dumps({"session": "session-a", "task": "K10", **extra}))


def _kept() -> dict[str, Any]:
    return entry.read_handoff()


def _no_start(head: str, root: Path) -> int:
    raise AssertionError(f"no new smoke may start for {head}")


# ---- which hand-ins get the runner's smoke


def test_a_first_round_hand_in_runs_no_smoke() -> None:
    hand = {"session": "session-a"}
    assert not entry.fix_round(["a1"], hand), "one worker, no hold: its worker ran the one full smoke"
    assert entry.fix_round(["a1", "b2"], hand) and entry.fix_round(["a1,b2"], hand)
    assert entry.fix_round(["a1"], {**hand, "prehandin_holds": 1}) and entry.fix_round(["a1"], {**hand, "after_smoke_holds": 1})
    assert not entry.fix_round(["a1", "b2"], {}), "no hand-off file: nothing to key the run to"


def _after_args(*workers: str) -> argparse.Namespace:
    argv = ["--after", "--reviewer-agent", "r9"]
    for worker in workers:
        argv += ["--worker-agent", worker]
    return entry.parse_args(argv)


@pytest.mark.parametrize(("workers", "smoked"), [(("a1",), False), (("a1", "b2"), True)])
def test_after_passes_the_hand_off_record_only_for_a_fix_round(
    workers: tuple[str, ...], smoked: bool, tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    _handoff(tmp_path, monkeypatch)
    env = ge.GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x")
    seen: list[dict[str, Any]] = []

    def collect(task: dict[str, Any], *args: Any, **kw: Any) -> cycle.WorkerResult:
        seen.append(kw)
        if kw["smoke"] is not None:
            kw["smoke"].update(sha=HEAD, job="77-1", pid=4242)
            return cycle.WorkerResult("tests_failed", log=f"{ge.AFTER_SMOKE_PENDING} on {HEAD[:12]}")
        return cycle.WorkerResult("submitted")

    env.collect_result = collect  # type: ignore[method-assign]  # reason: the wiring only, no git
    monkeypatch.setattr(cycle, "after_handoff", lambda cfg, e, res_for, hold, own_models=False: hold(res_for({"id": "K10"})) or "finished")
    result = entry.after(_after_args(*workers), SimpleNamespace(models={}), env)  # type: ignore[arg-type]  # reason: after reads only models
    assert (seen[0]["smoke"] is not None) is smoked
    assert result == (entry.SMOKE_PENDING if smoked else "finished")
    assert (_kept().get("after_smoke") == {"sha": HEAD, "job": "77-1", "pid": 4242}) is smoked, "kept for the next --after"


# ---- the whole hand-in: pending pushes nothing, then a green hands in


def _claimed(repo: dict[str, Path], tmp_path: Path) -> tuple[ge.GitHubEnv, cycle.Config, list[str]]:
    env = make_env(repo["a"], tmp_path)
    env.update_state(lambda s: st.claim(st.with_task(s, "K10"), "K10", "session-a", NOW))
    env.enable_auto_merge = lambda b, h: (True, "fake")  # type: ignore[method-assign]  # reason: no GitHub API offline
    work, review = transcript(tmp_path, "w", MODELS["sonnet"]), transcript(tmp_path, "r", MODELS["opus"])
    return env, live_config("session-a", tmp_path, env), [work, review]


def _hand_in(cfg: cycle.Config, env: ge.GitHubEnv, paths: list[str]) -> str:
    """`after`'s wiring for a fix round: the hand-off's record goes to collect_result and is kept afterwards."""
    run = dict(_kept().get("after_smoke") or {})
    result = cycle.after_handoff(cfg, env, lambda t: env.collect_result(t, "submitted", [paths[0]], [paths[1]], smoke=run),
                                 entry.hold_hand_in)
    entry.keep_after_smoke(run)
    return result


def test_a_fix_round_hand_in_is_pending_and_pushes_nothing_then_a_green_verdict_hands_in(
    repo: dict[str, Path], tmp_path: Path, job: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str],
) -> None:
    _handoff(tmp_path, monkeypatch, prehandin_holds=1)
    env, cfg, paths = _claimed(repo, tmp_path)
    head = worker_commit(repo["a"], TASK["branch"], "docs/k10/note.md")
    before = git(repo["origin"], "rev-parse", "claude/kit-state")
    started = _background(env, job, [None], [True])
    assert _hand_in(cfg, env, paths) == entry.SMOKE_PENDING
    assert f"{ge.AFTER_SMOKE_PENDING} on {head[:12]}" in capsys.readouterr().out
    assert git(repo["origin"], "rev-parse", "claude/kit-state") == before, "no state written while it runs"
    assert git(repo["origin"], "branch", "--list", TASK["branch"]) == "", "nothing pushed"
    assert _kept()["after_smoke"]["sha"] == head and started == [head]
    entry.close_handoff(entry.SMOKE_PENDING)
    assert (tmp_path / entry.HANDOFF).exists()

    _background(env, job, [_bg_result(head, 0)], [False])
    env.bg_start = _no_start  # type: ignore[assignment]  # reason: the same head waits for the same run
    assert _hand_in(cfg, env, paths) == st.MERGE_PENDING
    assert git(repo["origin"], "rev-parse", TASK["branch"]) == head
    assert any(row.startswith(f"runner smoke on {head[:12]}: exit 0") for row in env.log)


# ---- a red verdict: held once, then the attempt fails


def test_a_red_verdict_holds_once_then_fails_tests_failed_with_the_output(
    repo: dict[str, Path], tmp_path: Path, job: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str],
) -> None:
    _handoff(tmp_path, monkeypatch, prehandin_holds=1)
    env, cfg, paths = _claimed(repo, tmp_path)
    head = worker_commit(repo["a"], TASK["branch"], "docs/k10/note.md")
    _background(env, job, [_bg_result(head, 1)], [False])
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_PRE_LOG).write_text(RED_LOG)
    assert _hand_in(cfg, env, paths) == entry.SMOKE_HELD
    printed = capsys.readouterr().out
    assert f"runner: {entry.SMOKE_HELD}" in printed and "FAILED: cargo test" in printed and "ghp_" not in printed
    kept = _kept()
    assert kept["after_smoke_holds"] == 1 and kept["after_smoke_log"].startswith(ge.AFTER_SMOKE_RED)
    assert st.status(env.read_state(), "K10") == st.IN_PROGRESS

    env.bg_start = _no_start  # type: ignore[assignment]  # reason: a re-run on the same head takes its result
    assert _hand_in(cfg, env, paths) != entry.SMOKE_HELD
    task = st.entry(env.read_state(), "K10")
    assert task["status"] != st.IN_PROGRESS and ge.AFTER_SMOKE_RED in str(task["last_failure"])
    assert "FAILED: cargo test" in str(task["last_failure"]) and git(repo["origin"], "branch", "--list", TASK["branch"]) == ""


def test_a_held_red_is_counted_in_the_hand_off_and_no_file_means_no_hold(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    red = cycle.WorkerResult("tests_failed", log=f"{ge.AFTER_SMOKE_RED} on abc: FAILED: cargo test")
    monkeypatch.setattr(entry, "ROOT", tmp_path)
    assert entry.hold_after_smoke(red) == "", "RUNNER_WORKER_CMD and tests have no hand-off: no hold"
    _handoff(tmp_path, monkeypatch)
    assert [entry.hold_after_smoke(red) for _ in range(2)] == [entry.SMOKE_HELD, ""]
    assert entry.hold_after_smoke(cycle.WorkerResult("tests_failed", log="worker said so")) == ""
    entry.close_handoff(entry.SMOKE_HELD)
    assert (tmp_path / entry.HANDOFF).exists(), "a held hand-in keeps the hand-off, so the safety step still frees the lease"


# ---- no verdict: logged and let through


@pytest.mark.parametrize(("result", "line"), [
    ({**_bg_result(HEAD, 124), "timed_out": True}, "exit 124"),
    (_bg_result(HEAD, 3), "exit 3"),
    (None, "exit 124; the background smoke ended without a result"),
])
def test_a_timeout_another_exit_or_a_run_without_a_result_lets_the_hand_in_through(
    result: dict[str, Any] | None, line: str, tmp_path: Path, job: Path,
) -> None:
    env, _ = _env(tmp_path)
    _background(env, job, [result], [False])
    assert env.after_smoke(HEAD, {}) == ""
    assert line in env.log[-1] and env.log[-1].endswith("(not a verdict: let through)")


def test_a_head_git_cannot_check_out_a_dry_run_or_no_actions_job_runs_nothing(tmp_path: Path, job: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    env, _ = _env(tmp_path)
    env.bg_start = _no_start  # type: ignore[assignment]  # reason: nothing may start
    env._has_commit = lambda sha: False  # type: ignore[method-assign]  # reason: a missing worktree head
    assert env.after_smoke(HEAD, {}) == "" and env.log[-1].endswith(f"not run (no worktree of {HEAD[:12]}; let through)")
    env.dry_run = True
    assert env.after_smoke(HEAD, {}) == "" and env.log[-1].endswith("not run (dry-run; let through)")
    env.dry_run = False
    monkeypatch.delenv("GITHUB_RUN_ID")
    assert env.after_smoke(HEAD, {}) == "" and env.log[-1].endswith("not run (no Actions job; let through)")


# ---- keyed to the head


def test_a_new_head_after_a_hold_starts_a_new_run_and_stops_the_old_one(tmp_path: Path, job: Path) -> None:
    env, _ = _env(tmp_path)
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_PRE_LOG).write_text(RED_LOG)
    started = _background(env, job, [_bg_result(HEAD, 1)], [False])
    run: dict[str, Any] = {}
    assert env.after_smoke(HEAD, run).startswith(ge.AFTER_SMOKE_RED) and run["sha"] == HEAD and run["exit"] == 1
    env.bg_start = _no_start  # type: ignore[assignment]  # reason: the same head takes its kept result
    assert env.after_smoke(HEAD, dict(run)).startswith(ge.AFTER_SMOKE_RED)

    started = _background(env, job, [None, None], [False, True])
    assert env.after_smoke(OLD, run).startswith(ge.AFTER_SMOKE_PENDING) and started == [OLD]
    assert run == {"sha": OLD, "job": "77-1", "pid": 4242}, "the old head's result is gone"
    stops: list[str] = []
    env.bg_stop = lambda: stops.append("stop") or "stopped"  # type: ignore[assignment]  # reason: no real process
    started = _background(env, job, [None, _bg_result(HEAD, 0)], [True, False])
    assert env.after_smoke(HEAD, run) == "" and started == [HEAD] and stops == ["stop"]
    assert f"the background smoke of {OLD[:12]} stopped: the hand-in's head is now {HEAD[:12]}" in env.log
    assert json.loads((job / ge.SMOKE_BG).read_text())["sha"] == HEAD


# ---- a pending result writes no cycle-log row


def test_a_pending_after_writes_no_log_row_and_exits_0(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]) -> None:
    _handoff(tmp_path, monkeypatch)
    rows: list[dict[str, Any]] = []
    env = SimpleNamespace(starter="actions", runner_sha="x", log=[], first_state=None, last_state=None, write_cycle_log=rows.append)
    monkeypatch.setattr(entry, "build", lambda args, outbox: (SimpleNamespace(lanes=4, session="session-a"), env))
    monkeypatch.setattr(entry, "run", lambda args, cfg, e: entry.SMOKE_PENDING)
    monkeypatch.setattr(entry, "lane_report", lambda *a: pytest.fail("no refill while the smoke runs"))
    assert entry.main(["--after", "--worker-agent", "a1,b2"]) == 0
    assert rows == [] and "runner: smoke_pending" in capsys.readouterr().out and (tmp_path / entry.HANDOFF).exists()


# ---- the session prompt


def _step(text: str, label: str) -> str:
    return next(line.strip() for line in text.splitlines() if line.strip().startswith(f"{label} "))


def test_fix_rounds_run_the_scoped_checks_and_steps_5_and_5a_name_both_results() -> None:
    text = WORKFLOW.read_text(encoding="utf-8")
    for label in ("4c.", "5b."):
        step = _step(text, label)
        assert "once at the end" not in step and "run both scripts again" not in step and "scoped fix-round checks" in step
    four_c = _step(text, "4c.")
    for check in ("`cargo fmt --check`", "`cargo clippy --workspace --all-targets -- -D warnings`", "`cargo test -p <crate>`",
                  "`bash scripts/prehandin.sh` last", "only when its fixes touched more than one crate"):
        assert check in four_c
    assert "runner: smoke_pending" in _step(text, "5.") and "run step 5 again, the same way" in _step(text, "5.")
    assert "runner: smoke_red" in _step(text, "5a.") and "NEW foreground fix-round worker" in _step(text, "5a.")
    assert re.search(r"runner: prehandin_red.*runner: review_missing", _step(text, "5b."))


def test_the_workflow_holds_the_exact_marker_line() -> None:
    assert subprocess.run(["grep", "-qxF", "# T443: after-smoke live", str(WORKFLOW)], check=False).returncode == 0
