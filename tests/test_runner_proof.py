"""Tests for runner/proof.py (D-551): the runner records a stage's named proof itself only when every
condition of it holds on the state and kit/nightly.json; otherwise it says what the proof waits for.
Fake data only; git, the nightly and scripts/progress.py are injected.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from runner import proof, state as st

ROOT = Path(__file__).resolve().parent.parent
TESTS = proof.MACHINE_PROOFS["1"]


@dataclass
class Row:
    req_id: str
    tasks: list[str]
    td: list[str] = field(default_factory=list)
    verified: bool = True


def graph() -> list[dict[str, Any]]:
    return [
        {"id": "T002", "stage": "1", "type": "build", "deps": [], "tests": []},
        {"id": "T154", "stage": "1", "type": "build", "deps": [], "tests": ["TD-OPS-28"]},
        {"id": "T008", "stage": "1", "type": "cleanup", "deps": ["T002", "T154"], "tests": []},
        {"id": "T167", "stage": "1", "type": "harness", "deps": ["T008"], "tests": []},
        {"id": "T009", "stage": "2", "type": "build", "deps": ["T008"], "tests": ["TD-SEC-13"]},
    ]


def done_state() -> st.State:
    state = st.sync_graph(st.new_state(), graph())
    for tid in ("T002", "T154", "T167"):
        state = st.with_task(state, tid, status=st.DONE, merged_sha=f"m-{tid}")
    return st.with_task(state, "T008", status=st.BLOCKED, reason=st.PROOF_MISSING, merged_sha="m-T008")


def rows(*extra: Row) -> list[Row]:
    return [Row("OPS-053", ["T154"], ["TD-OPS-28"]), Row("SEC-032", ["T167"], ["TD-SEC-13"], verified=False), *extra]


def world(nightly: dict[str, Any] | None = None, outside: set[str] | None = None, missing: set[str] | None = None,
          extra: tuple[Row, ...] = ()) -> proof.World:
    run = nightly if nightly is not None else {"conclusion": "success", "sha": "n" * 40}
    return proof.World(run, lambda older, newer: older not in (outside or set()), lambda sha, path: path not in (missing or set()),
                       lambda state: rows(*extra))


def test_every_condition_true_records_the_proof_with_its_evidence() -> None:
    record, waits = proof.machine_proof("1", graph(), done_state(), world())
    assert waits == [] and record is not None and record["main_sha"] == "n" * 40 and record["by"] == "runner"
    text = record["evidence"]
    assert "n" * 40 in text and "T167 m-T167" in text and "T008 m-T008" in text and "OPS-053" in text
    assert all(path in text for path in TESTS)
    assert "SEC-032" not in text  # owned also by the stage-2 task T009 through TD-SEC-13: not this stage's to verify


def test_an_open_task_of_the_stage_keeps_the_proof_unrecorded() -> None:
    state = st.with_task(done_state(), "T167", status=st.QUEUED)
    assert proof.machine_proof("1", graph(), state, world()) == (None, ["T167 is QUEUED"])


def test_a_red_or_missing_nightly_keeps_the_proof_unrecorded() -> None:
    for run in ({"conclusion": "failure", "sha": "n1"}, {"conclusion": "success"}):
        assert proof.machine_proof("1", graph(), done_state(), world(nightly=run))[0] is None
    record, waits = proof.machine_proof("1", graph(), done_state(), proof.World(None, lambda a, b: True, lambda a, b: True, lambda s: []))
    assert record is None and waits == ["a green nightly-main run on main (kit/nightly.json)"]


def test_a_nightly_older_than_a_merge_of_the_stage_keeps_the_proof_unrecorded() -> None:
    record, waits = proof.machine_proof("1", graph(), done_state(), world(outside={"m-T167"}))
    assert record is None and waits == [f"a green nightly-main run containing T167's merge m-T167 (last green run: {'n' * 12})"]


def test_a_missing_merge_or_test_file_or_unverified_requirement_keeps_the_proof_unrecorded() -> None:
    state = st.with_task(done_state(), "T002", merged_sha=None)
    assert proof.machine_proof("1", graph(), state, world())[1] == ["T002: no merged_sha recorded"]
    record, waits = proof.machine_proof("1", graph(), done_state(), world(missing={"tests/contract/test_stand_in_server.py"}))
    assert record is None and waits == [f"tests/contract/test_stand_in_server.py in nightly commit {'n' * 12}"]
    record, waits = proof.machine_proof("1", graph(), done_state(), world(extra=(Row("LIC-001", ["T002"], verified=False),)))
    assert record is None and waits == ["requirement LIC-001 verified"]


def test_a_stage_not_listed_is_proved_by_hand() -> None:
    record, waits = proof.machine_proof("2", graph(), done_state(), world())
    assert record is None and "--record-proof 2" in waits[0]


def test_owned_up_to_counts_earlier_stages_and_skips_tasks_without_a_numeric_stage() -> None:
    g = [*graph(), {"id": "K9", "deps": [], "tests": []}]
    found = proof.owned_up_to(g, [Row("A-001", ["T002", "T009"]), Row("B-001", ["T002"]), Row("C-001", ["K9"]), Row("D-001", [])], "2")
    assert [r.req_id for r in found] == ["A-001", "B-001"]


def test_stage_one_is_the_only_stage_proved_by_the_runner_and_its_row_names_what_its_tests_check() -> None:
    """A stage joins MACHINE_PROOFS only by a plan change naming a test for each clause of its row (D-551)."""
    assert set(proof.MACHINE_PROOFS) == {"1"}
    row = next(line for line in (ROOT / "docs" / "build-readiness.md").read_text(encoding="utf-8").splitlines() if line.startswith("| **1."))
    for clause in ("planted failure", "worked example", "matching_examples.yaml", "stand-ins answer over HTTP", "progress.py"):
        assert clause in row
    owners = {t["id"]: t for t in json.loads((ROOT / "kit" / "tasks.json").read_text(encoding="utf-8"))["tasks"]}
    assert "tests/contract/test_stand_in_server.py" in owners["T167"]["owns"] and str(owners["T167"]["stage"]) == "1"
    assert all((ROOT / path).is_file() for path in TESTS if path != "tests/contract/test_stand_in_server.py")
