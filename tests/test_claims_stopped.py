"""The keeper judges work with the cycle's own stage gate, and STATUS.md says when claims have stopped (D-5568):
runner/keeper.py `stage_gate`, `has_work`, `refill_config` and `claims_stopped`, runner/github_env.py's
"Needs attention" line. Fake data only, no network."""

from __future__ import annotations

import json
import shutil
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from runner import github_env as ge, keeper as kp, state as st

ROOT = Path(__file__).resolve().parent.parent
NOW = datetime(2026, 11, 22, 18, 0, tzinfo=timezone.utc)
MERGE, GREEN, OLD, HEAD = "a" * 40, "b" * 40, "c" * 40, "d" * 40
GRAPH = [
    {"id": "T100", "deps": [], "type": "cleanup", "stage": "1", "owns": ["app/one/src/a.rs"], "model": "claude-opus-5-5"},
    {"id": "T200", "deps": [], "type": "build", "stage": "2", "owns": ["app/two/src/a.rs"], "model": "claude-opus-5-5"},
]
RED_TWICE = {"conclusion": "failure", "sha": OLD, "failures_since_green": 2, "last_green": {"sha": GREEN}}


def state(**extra: Any) -> dict[str, Any]:
    tasks = [{"id": "T100", "status": "DONE", "deps": [], "type": "cleanup", "stage": "1", "merged_sha": MERGE},
             {"id": "T200", "status": "QUEUED", "deps": [], "type": "build", "stage": "2"}]
    return {"schema": 1, "backup_runner": "on", "lease": None, "build_hold": None, "tasks": tasks,
            "stage_proofs": {"1": {"evidence": "fake proof"}}, **extra}


def world(nightly: dict[str, Any] | None = RED_TWICE, answer: bool | None = True, **kw: Any) -> kp.World:
    return kp.World(GRAPH, nightly, HEAD, lambda _older, _newer: answer, **kw)


def test_a_twice_red_nightly_with_no_stage_passed_is_no_work_and_says_claims_stopped() -> None:
    s = state()
    assert kp.stage_gate(s, world()) == set()
    assert not kp.has_work(s, NOW, world()) and kp.has_work(s, NOW)  # today's superset still says work
    assert kp.wake_after(s, {}, NOW, cfg=None, world=world())[0] == kp.CAP_MINUTES
    assert kp.claims_stopped(s, NOW, world()) == (
        f"CLAIMS STOPPED: nightly red on {OLD[:12]}, failures_since_green 2; no stage passed its exit test")


def test_a_passed_stage_or_one_old_failure_dispatches_as_today() -> None:
    green = {"conclusion": "success", "sha": GREEN}
    once = {"conclusion": "failure", "sha": OLD, "failures_since_green": 1, "last_green": {"sha": GREEN}}
    for nightly in (green, once):
        assert kp.stage_gate(state(), world(nightly)) == {"1"}
        assert kp.has_work(state(), NOW, world(nightly)) and kp.claims_stopped(state(), NOW, world(nightly)) == ""
        assert kp.wake_after(state(), {}, NOW, world=world(nightly)) == (20, "idle")


def test_any_doubt_about_the_gate_judges_without_it() -> None:
    def boom(_older: str, _newer: str) -> bool | None:
        raise RuntimeError("fake failure")

    once = {"conclusion": "failure", "sha": OLD, "failures_since_green": 1, "last_green": {"sha": GREEN}}
    doubts = [world(None), world(once, None), world({"conclusion": "success", "sha": GREEN}, None),
              kp.World(GRAPH, once, HEAD, boom), kp.World([], RED_TWICE, HEAD, boom), kp.World(GRAPH, RED_TWICE, "", boom), None]
    for each in doubts:
        assert kp.stage_gate(state(), each) is None
        assert kp.has_work(state(), NOW, each) and kp.claims_stopped(state(), NOW, each) == ""


def test_merge_pending_and_a_red_main_count_as_work_under_a_red_nightly() -> None:
    pending = state()
    pending["tasks"][1]["status"] = st.MERGE_PENDING
    assert kp.has_work(pending, NOW, world())
    red = state(main_red=True)
    assert kp.has_work(red, NOW, world()) and kp.claims_stopped(red, NOW, world()) == ""


def test_a_task_awaiting_its_stage_proof_is_work_only_while_the_nightly_is_green() -> None:
    s = state()
    s["tasks"][0].update(status=st.BLOCKED, reason=st.PROOF_MISSING)
    green = {"conclusion": "success", "sha": GREEN}
    assert kp.has_work(s, NOW, world(green, proof_stages=frozenset({"1"})))
    assert not kp.has_work(s, NOW, world(proof_stages=frozenset({"1"})))
    assert not kp.has_work(s, NOW, world(green, proof_stages=frozenset()))  # not a stage whose proof the runner records


def _kit(tmp_path: Path) -> Path:
    (tmp_path / "kit").mkdir()
    shutil.copy(ROOT / "kit" / "runner.json", tmp_path / "kit" / "runner.json")
    real = json.loads((ROOT / "kit" / "tasks.json").read_text(encoding="utf-8"))
    kit = {"tasks": GRAPH, "models": real["models"], "protected_paths": [], "shared_paths": []}
    (tmp_path / "kit" / "tasks.json").write_text(json.dumps(kit), encoding="utf-8")
    return tmp_path


def test_refill_gives_no_slot_under_a_twice_red_nightly_and_todays_slots_otherwise(tmp_path: Path) -> None:
    root = _kit(tmp_path)
    assert kp.free_slots(state(), {}, NOW, kp.refill_config(root, world())) == 0
    today = kp.free_slots(state(), {}, NOW, kp.refill_config(root))
    assert today == 1 and kp.refill_config(root).stage_gate is None  # type: ignore[union-attr]
    for each in (world({"conclusion": "success", "sha": GREEN}), world(None), world(RED_TWICE | {"failures_since_green": 1}, None)):
        assert kp.free_slots(state(), {}, NOW, kp.refill_config(root, each)) == today


def test_remote_world_reads_main_the_nightly_and_ancestry_through_gh() -> None:
    calls: list[str] = []
    answers = {"repos/o/r/commits/main": HEAD, f"repos/o/r/contents/kit/tasks.json?ref={HEAD}": json.dumps({"tasks": GRAPH}),
               "repos/o/r/contents/kit/nightly.json?ref=claude/kit-state": json.dumps(RED_TWICE),
               f"repos/o/r/compare/{MERGE}...{GREEN}": "ahead\n"}

    def gh(path: str, _jq: str) -> str | None:
        calls.append(path)
        return answers.get(path)

    built = kp.remote_world("o/r", gh)
    assert built is not None and (built.head, built.graph, built.nightly) == (HEAD, GRAPH, RED_TWICE)
    assert built.ancestor(MERGE, GREEN) is True and built.ancestor(MERGE, GREEN) is True and built.ancestor(OLD, HEAD) is None
    assert calls.count(f"repos/o/r/compare/{MERGE}...{GREEN}") == 1  # one call per question
    assert kp.remote_world("", gh) is None and kp.remote_world("o/r", lambda _p, _j: None) is None


def _env(tmp_path: Path, nightly: dict[str, Any] | None) -> ge.GitHubEnv:
    env = ge.GitHubEnv(tmp_path, GRAPH, tmp_path, repo="o/r", starter="actions", runner_sha="x",
                       run=lambda cmd, _stdin, _cwd: (0, "", ""))
    env.read_nightly = lambda: nightly  # type: ignore[method-assign]
    env.main_head = lambda: HEAD  # type: ignore[method-assign]
    return env


def test_status_shows_claims_stopped_exactly_when_the_gate_empties_eligibility(tmp_path: Path) -> None:
    line = f"- CLAIMS STOPPED: nightly red on {OLD[:12]}"
    stalled = _env(tmp_path, RED_TWICE).board(NOW, state())
    assert line in ge.render_status(state(), "", NOW, board=stalled).split("## Needs attention\n", 1)[1]
    for nightly in ({"conclusion": "success", "sha": GREEN}, None):
        assert "CLAIMS STOPPED" not in ge.render_status(state(), "", NOW, board=_env(tmp_path, nightly).board(NOW, state()))
    assert "CLAIMS STOPPED" not in ge.render_status(state(main_red=True), "", NOW, board=_env(tmp_path, RED_TWICE).board(NOW, state(main_red=True)))
    assert _env(tmp_path, RED_TWICE).board(NOW).stopped == "" and "CLAIMS STOPPED" not in ge.render_status(state(), "", NOW)
