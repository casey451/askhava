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


# TD-OPS-31 (SEC-022, D-1760): the integration-closure register, condition 5. Fake rows only.
CLOSURE_GRAPH = [*GRAPH, {"id": "T090", "stage": "3", "type": "build"}]
HEADER = "| ID | Handoff | Decision or finding | Producer | Consumer | Area | Owner | Test | Status | Evidence |\n|---|---|---|---|---|---|---|---|---|---|\n"


def register(tmp_path: Path, *rows: str) -> Path:
    path = tmp_path / "integration-closure.md"
    body = "".join(f"| {r} |\n" for r in rows)
    path.write_text(f"# Register\n\nIntro text.\n\n{HEADER}{body}\nAfter the table.\n", encoding="utf-8")
    return path


def row(area: str, owner: str, status: str = "open", evidence: str = "-", rid: str = "IC-90") -> str:
    return f"{rid} | A fake handoff | D-1 | a | b | {area} | {owner} | TD-X-01 | {status} | {evidence}"


def closure(stage: str, path: Path, state: dict[str, Any] | None = None) -> list[str]:
    return se.evaluate(stage, CLOSURE_GRAPH, state or done_state(), NIGHTLY, {}, always(True), path)


def test_td_ops_31_open_gating_row_with_a_queued_owner_passes(tmp_path: Path) -> None:
    path = register(tmp_path, row("security", "T058"), row("money", "T011, T058"), row("privacy", "T010, T058"))
    assert closure("2", path) == []
    assert closure("3", path) == ["T058 is QUEUED, not DONE", "T090 is QUEUED, not DONE"]
    gate_owned = register(tmp_path, row("security", "T127"))
    assert closure("2", gate_owned) == []  # a gate's non-numeric stage is never at or below N: printed, not judged


def test_td_ops_31_done_owners_with_one_in_a_later_stage_pass_earlier_and_fail_later(tmp_path: Path) -> None:
    path = register(tmp_path, row("money", "T011, T090"))
    state = done_state()
    state["tasks"].append({"id": "T090", "status": "DONE", "merged_sha": "m-T090"})
    assert closure("2", path, state) == []
    problems = closure("3", path, state)
    assert [p for p in problems if "IC-90" in p] == ["IC-90 (money): every owner (T011, T090) is DONE or SKIPPED; name a live owner or close it"]
    problems, notes = se.closure_review("2", CLOSURE_GRAPH, state, se.read_register(path) or [])
    assert problems == [] and notes == ["IC-90 (money) is open; judged at the stage of T090"]


def test_td_ops_31_only_done_or_skipped_owners_fail(tmp_path: Path) -> None:
    state = done_state()
    state["tasks"].append({"id": "T127", "status": "SKIPPED"})
    path = register(tmp_path, row("privacy", "T010, T011"))
    assert closure("2", path, state) == ["IC-90 (privacy): every owner (T010, T011) is DONE or SKIPPED; name a live owner or close it"]


def test_td_ops_31_no_owner_or_an_unknown_task_fails_at_every_stage(tmp_path: Path) -> None:
    path = register(tmp_path, row("security", "none"), row("money", "T999, T058", rid="IC-91"))
    for stage in ("2", "3"):
        problems = [p for p in closure(stage, path) if p.startswith("IC-")]
        assert problems == ["IC-90 (security): no owner task", "IC-91 (money): owner T999 is not a task in kit/tasks.json"]


def test_td_ops_31_a_closed_row_needs_a_commit_or_pr_and_a_test(tmp_path: Path) -> None:
    bare = register(tmp_path, row("security", "T010", "closed", "merged"))
    assert closure("2", bare) == ["IC-90 (security): closed without evidence (a merged commit SHA or PR number and a test)"]
    no_test = register(tmp_path, row("security", "T010", "closed", "#375"))
    assert len(closure("2", no_test)) == 1
    good = register(tmp_path, row("security", "T010", "closed", "#375, test_fake_case"), row("money", "T011", "closed", "f183f5bd TD-FEE-46"))
    assert closure("2", good) == []


def test_td_ops_31_a_non_gating_unowned_row_passes_with_a_printed_line(tmp_path: Path, capsys: Any) -> None:
    root = tmp_path
    (root / "kit").mkdir()
    (root / "kit" / "tasks.json").write_text(json.dumps({"tasks": CLOSURE_GRAPH}))
    (root / "docs").mkdir()
    (root / "docs" / "integration-closure.md").write_text(register(tmp_path, row("reliability", "none", "tracked")).read_text())
    state_path = root / "state.json"
    state_path.write_text(json.dumps(done_state()))
    assert closure("2", root / "docs" / "integration-closure.md") == []
    se.main(["--stage", "2", "--state", str(state_path), "--root", str(root)])
    assert "  note: IC-90 (reliability) is tracked; not gating" in capsys.readouterr().out.splitlines()


def test_td_ops_31_a_missing_register_fails(tmp_path: Path, capsys: Any) -> None:
    missing = tmp_path / "docs" / "integration-closure.md"
    assert closure("2", missing) == [f"the integration-closure register {missing} is missing"]
    assert se.read_register(missing) is None
    state = done_state()
    state["tasks"][0]["status"] = "BLOCKED"
    assert closure("2", missing, state) == ["T010 is BLOCKED, not DONE", f"the integration-closure register {missing} is missing"]
    assert se.passed_stages(CLOSURE_GRAPH, done_state(), NIGHTLY, {}, always(True), missing) == set()
