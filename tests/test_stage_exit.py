"""Tests for scripts/stage_exit.py (stage exit tests, docs/governance.md section 6, D-486). Fake data only."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from typing import Any

import stage_exit as se

GRAPH = [
    {"id": "T010", "stage": "2", "type": "build"},
    {"id": "T011", "stage": "2", "type": "build"},
    {"id": "T057", "stage": "2", "type": "cleanup"},
    {"id": "T058", "stage": "3", "type": "build"},
    {"id": "T127", "stage": "checkpoint", "type": "gate"},
]
NIGHTLY = {"conclusion": "success", "sha": "n1"}


def done_state(**extra: Any) -> dict[str, Any]:
    tasks = [{"id": t, "status": "DONE", "merged_sha": f"m-{t}"} for t in ("T010", "T011", "T057")]
    return {"tasks": tasks, "stage_proofs": {"2": {"evidence": "J1 steps 1-6 green, run 42"}}, **extra}


def always(answer: bool | None) -> se.Ancestor:
    return lambda older, newer: answer


def test_stage_passes_when_all_done_nightly_contains_cleanup_and_proof_recorded() -> None:
    assert se.evaluate("2", GRAPH, done_state(), NIGHTLY, {}, always(True)) == []
    assert se.passed_stages(GRAPH, done_state(), NIGHTLY, {}, always(True)) == {"2"}


def test_a_blocked_task_keeps_the_stage_closed_and_git_is_not_asked() -> None:
    state = done_state()
    state["tasks"][0]["status"] = "BLOCKED"
    asked: list[str] = []
    problems = se.evaluate("2", GRAPH, state, NIGHTLY, {}, lambda a, b: asked.append(a) or True)
    assert problems == ["T010 is BLOCKED, not DONE"] and asked == []


def test_nightly_must_be_green_and_contain_the_cleanup_merge() -> None:
    assert "not a green run" in se.evaluate("2", GRAPH, done_state(), {"conclusion": "failure", "sha": "n1"}, {}, always(True))[0]
    assert "not a green run" in se.evaluate("2", GRAPH, done_state(), None, {}, always(True))[0]
    problems = se.evaluate("2", GRAPH, done_state(), NIGHTLY, {}, always(False))
    assert problems == ["T057: merge m-T057 is not in the nightly run's commit n1"]
    assert se.evaluate("2", GRAPH, done_state(), NIGHTLY, {}, always(None))  # git cannot answer: fail closed


def test_only_cleanup_merges_are_checked_when_the_stage_has_one() -> None:
    checked: list[str] = []
    se.evaluate("2", GRAPH, done_state(), NIGHTLY, {}, lambda a, b: checked.append(a) or True)
    assert checked == ["m-T057"]


def test_the_named_proof_must_be_recorded() -> None:
    state = done_state(stage_proofs={"2": {"evidence": "  "}})
    assert se.evaluate("2", GRAPH, state, NIGHTLY, {}, always(True)) == [
        "stage 2's named proof is not recorded in state (stage_proofs[2].evidence)"
    ]


def test_ci_flags_must_be_green_when_present() -> None:
    assert se.evaluate("2", GRAPH, done_state(), NIGHTLY, {"coverage": "green", "mutants": "success"}, always(True)) == []
    problems = se.evaluate("2", GRAPH, done_state(ci_flags={"mutants": "3 survived"}), NIGHTLY, {}, always(True))
    assert problems == ["Ci flag mutants is '3 survived', not green"]


def test_gates_may_be_skipped_and_unknown_stages_fail() -> None:
    state = {"tasks": [{"id": "T127", "status": "SKIPPED"}]}
    assert se._status_problems([GRAPH[4]], {t["id"]: t for t in state["tasks"]}) == []
    assert se.evaluate("9", GRAPH, state, NIGHTLY, {}, always(True)) == ["no task has stage 9"]


def test_named_proofs_come_from_build_readiness() -> None:
    root = Path(__file__).resolve().parent.parent
    proofs = se.named_proofs(root)
    assert set(proofs) >= {"1", "2", "3", "4", "5", "6", "7"} and "Ledger reconciles" in proofs["3"]


def test_git_ancestry_on_a_real_repository(tmp_path: Path) -> None:
    def git(*args: str) -> str:
        return subprocess.run(["git", "-C", str(tmp_path), *args], capture_output=True, text=True, check=True).stdout.strip()

    git("init", "-q", "-b", "main")
    git("config", "user.email", "t@example.com")
    git("config", "user.name", "t")
    git("commit", "-q", "--allow-empty", "-m", "one")
    first = git("rev-parse", "HEAD")
    git("commit", "-q", "--allow-empty", "-m", "two")
    second = git("rev-parse", "HEAD")
    check = se.git_is_ancestor(tmp_path)
    assert check(first, second) is True and check(second, first) is False and check("f" * 40, second) is None


def test_cli_exit_codes(tmp_path: Path) -> None:
    root = tmp_path
    (root / "kit").mkdir()
    (root / "kit" / "tasks.json").write_text(json.dumps({"tasks": GRAPH}))
    state_path = root / "state.json"
    state_path.write_text(json.dumps(done_state()))
    (root / "nightly.json").write_text(json.dumps(NIGHTLY))
    assert se.main(["--stage", "3", "--state", str(state_path), "--root", str(root)]) == 1
    assert se.main(["--stage", "2", "--state", str(state_path), "--root", str(root)]) == 1  # no git repo: ancestry unknown
