"""Tests for runner/proof.py (D-551, D-1473): the runner records a stage's named proof itself only when every
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
TESTS = proof.tests_of("1")
GREEN_JOBS = {"ci": "success", "twin": "success", "a11y": "success", "named_mutants": "success", "stripe_ips": "success"}


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
          extra: tuple[Row, ...] = (), texts: dict[str, str] | None = None) -> proof.World:
    run = nightly if nightly is not None else {"conclusion": "success", "sha": "n" * 40, "results": dict(GREEN_JOBS)}

    def read(sha: str, path: str) -> str | None:
        return None if path in (missing or set()) else (texts or {}).get(path, "def test_fake() -> None:\n    assert True\n")

    return proof.World(run, lambda older, newer: older not in (outside or set()), read, lambda state: rows(*extra))


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
    record, waits = proof.machine_proof("1", graph(), done_state(), proof.World(None, lambda a, b: True, lambda a, b: "", lambda s: []))
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


def test_a_named_test_that_can_skip_itself_keeps_the_proof_unrecorded() -> None:
    """A test that skips is green without checking anything (D-551 review)."""
    server = "tests/contract/test_stand_in_server.py"
    for body in ("pytest.skip('no server')", "@pytest.mark.skipif(True, reason='x')", "pytest.importorskip('x')",
                 "@pytest.mark.xfail", "raise unittest.SkipTest('x')"):
        record, waits = proof.machine_proof("1", graph(), done_state(), world(texts={server: f"import pytest\n{body}\n"}))
        assert record is None and waits == [f"{server} with no skip path ({proof.SKIP_WORDS}) in nightly commit {'n' * 12}"]


def test_the_committed_row_tests_have_no_skip_path() -> None:
    for path in (path for stage in proof.MACHINE_PROOFS for path in proof.tests_of(stage)):
        if (ROOT / path).is_file():
            assert not proof.SKIP_PATHS.search((ROOT / path).read_text(encoding="utf-8")), path


def test_a_stage_not_listed_is_proved_by_hand_and_says_why() -> None:
    record, waits = proof.machine_proof("3", graph(), done_state(), world())
    assert record is None and "--record-proof 3" in waits[0] and "cargo-mutants" in waits[0]
    record, waits = proof.machine_proof("7", graph(), done_state(), world())
    assert record is None and waits == ["stage 7's proof is recorded by hand (`python3 -m runner --record-proof 7 --evidence TEXT`)"]


def test_owned_up_to_counts_earlier_stages_and_skips_tasks_without_a_numeric_stage() -> None:
    g = [*graph(), {"id": "K9", "deps": [], "tests": []}]
    found = proof.owned_up_to(g, [Row("A-001", ["T002", "T009"]), Row("B-001", ["T002"]), Row("C-001", ["K9"]), Row("D-001", [])], "2")
    assert [r.req_id for r in found] == ["A-001", "B-001"]


def kit_tasks() -> list[dict[str, Any]]:
    return list(json.loads((ROOT / "kit" / "tasks.json").read_text(encoding="utf-8"))["tasks"])


def exit_row(stage: str) -> str:
    lines = (ROOT / "docs" / "build-readiness.md").read_text(encoding="utf-8").splitlines()
    return next(line for line in lines if line.startswith(f"| **{stage}."))


# For each machine-proved stage, words of its exit row each clause list must account for (D-551, D-1473).
ROW_WORDS = {
    "1": ("planted failure", "worked example", "matching_examples.yaml", "stand-ins answer over HTTP", "progress.py"),
    "2": ("planted cross-layer import", "session-proof compile-fail", "migrations apply from empty", "reduced J1",
          "320/768/1280", "coverage floors", "named mutants for license filter, consent, quiet hours", "jiff-sqlx", "progress"),
    "4": ("stage-4 slices of J1", "J2 (business)", "authorization matrix", "masked call", "CALL-025", "license import",
          "live-lookup fixture", "sentinel scan", "progress"),
}


def test_stages_one_two_and_four_are_machine_proved_and_three_five_and_six_by_hand_with_a_reason() -> None:
    """A stage joins MACHINE_PROOFS only by a plan change naming a test for each clause of its row (D-551);
    a stage with a clause no nightly result proves stays hand-recorded, and says why (D-1473)."""
    assert set(proof.MACHINE_PROOFS) == {"1", "2", "4"}
    assert set(proof.HAND_RECORDED) == {"3", "5", "6"}
    for stage, words in ROW_WORDS.items():
        row = exit_row(stage)
        assert all(word in row for word in words), stage
    for stage, why in (("3", "cargo-mutants"), ("5", "twin chaos"), ("6", "war games")):
        assert why in proof.HAND_RECORDED[stage] and why.split()[-1] in exit_row(stage)


def test_every_named_path_exists_or_is_owned_by_a_task_of_that_stage_or_earlier() -> None:
    """A test a later task writes is named by the path that task owns, so the proof waits for it (D-1473)."""
    order = {str(n): n for n in range(1, 8)}
    owner_stage = {path: order.get(str(t.get("stage")), 99) for t in kit_tasks() for path in t.get("owns", [])}
    for stage, clauses in proof.MACHINE_PROOFS.items():
        for clause in clauses:
            for path in clause.tests + tuple(p for p, _ in clause.marks):
                assert (ROOT / path).is_file() or owner_stage.get(path, 99) <= int(stage), (stage, path)
            assert set(clause.jobs) <= {"ci", "twin", "a11y", "named_mutants"}, clause


def test_the_committed_marks_are_present_except_the_reduced_j1_steps_d1280_left_out() -> None:
    for stage, clauses in proof.MACHINE_PROOFS.items():
        for clause in clauses:
            for path, text in clause.marks:
                body = (ROOT / path).read_text(encoding="utf-8") if (ROOT / path).is_file() else None
                if body is not None and (path, text) not in proof.J1_REDUCED_STEPS:
                    assert text in body, (stage, path, text)


def stage_graph(stage: str) -> list[dict[str, Any]]:
    return [
        {"id": "T900", "stage": stage, "type": "build", "deps": [], "tests": []},
        {"id": "T901", "stage": stage, "type": "cleanup", "deps": ["T900"], "tests": []},
    ]


def stage_state(stage: str) -> st.State:
    state = st.with_task(st.sync_graph(st.new_state(), stage_graph(stage)), "T900", status=st.DONE, merged_sha="m-T900")
    return st.with_task(state, "T901", status=st.BLOCKED, reason=st.PROOF_MISSING, merged_sha="m-T901")


def marked(stage: str, drop: tuple[str, str] | None = None) -> dict[str, str]:
    """Each file a stage's clauses name, holding every mark of that stage except `drop`."""
    texts: dict[str, list[str]] = {}
    for clause in proof.MACHINE_PROOFS[stage]:
        for path in clause.tests:
            texts.setdefault(path, [])
        for path, text in clause.marks:
            texts.setdefault(path, []).extend([] if (path, text) == drop else [text])
    return {path: "\n".join(["// fake test", *marks]) for path, marks in texts.items()}


def test_stage_two_records_once_every_clause_holds_and_names_each_clause_in_its_evidence() -> None:
    record, waits = proof.machine_proof("2", stage_graph("2"), stage_state("2"), world(texts=marked("2")))
    assert waits == [] and record is not None
    for path in ("tests/e2e/j1_reduced.spec.ts", "tests/a11y/axe_all_pages.spec.ts", "tests/mutation/mutants.toml",
                 "tests/coverage_floor.json", "tests/integration/tests/intake_timeouts.rs", "named_mutants"):
        assert path in record["evidence"]


def test_stage_two_waits_for_the_reduced_j1_steps_the_file_does_not_run_yet() -> None:
    """D-1280: the reduced J1 runs step 5 and part of step 6 only; the clause is J1 steps 1 to 6."""
    texts = dict(marked("2"), **{"tests/e2e/j1_reduced.spec.ts": 'test("J1 step 5: quotes")\ntest("J1 step 6 (partial): form")'})
    record, waits = proof.machine_proof("2", stage_graph("2"), stage_state("2"), world(texts=texts))
    assert record is None and [w.split("'")[1] for w in waits] == [f'test("J1 step {n}:' for n in (1, 2, 3, 4, 6)]


def test_stage_two_waits_for_a_missing_mutant_group_or_a_red_named_mutants_job() -> None:
    drop = ("tests/mutation/mutants.toml", 'rule = "Quiet hours"')
    record, waits = proof.machine_proof("2", stage_graph("2"), stage_state("2"), world(texts=marked("2", drop)))
    assert record is None and waits == [f"'rule = \"Quiet hours\"' in tests/mutation/mutants.toml in nightly commit {'n' * 12} "
                                         "(named mutants for license filter, consent, quiet hours all killed)"]
    run = {"conclusion": "success", "sha": "n" * 40, "results": dict(GREEN_JOBS, named_mutants="failure")}
    record, waits = proof.machine_proof("2", stage_graph("2"), stage_state("2"), world(nightly=run, texts=marked("2")))
    assert record is None and waits == ["nightly-main job named_mutants green in kit/nightly.json (recorded: failure)"]


def test_a_nightly_without_job_results_keeps_the_proof_unrecorded() -> None:
    run = {"conclusion": "success", "sha": "n" * 40}
    record, waits = proof.machine_proof("1", graph(), done_state(), world(nightly=run))
    assert record is None and waits[0] == "nightly-main job ci green in kit/nightly.json (recorded: None)"


def test_stage_four_waits_for_each_test_its_unbuilt_tasks_will_write() -> None:
    missing = {"tests/e2e/j1_homeowner.spec.ts", "tests/security/tests/auth_matrix.rs", "tests/integration/tests/pin_routing.rs"}
    record, waits = proof.machine_proof("4", stage_graph("4"), stage_state("4"), world(missing=missing, texts=marked("4")))
    assert record is None
    assert waits == [f"tests/e2e/j1_homeowner.spec.ts in nightly commit {'n' * 12}",
                     f"tests/security/tests/auth_matrix.rs in nightly commit {'n' * 12}",
                     f"tests/integration/tests/pin_routing.rs in nightly commit {'n' * 12}",
                     f"'CALL-025' in tests/integration/tests/pin_routing.rs in nightly commit {'n' * 12} "
                     "(masked call through the fake Twilio with the PIN and the CALL-025 limits (T077, T080))"]
    record, waits = proof.machine_proof("4", stage_graph("4"), stage_state("4"), world(texts=marked("4")))
    assert waits == [] and record is not None and "tests/integration/tests/voice_calls.rs" in record["evidence"]


def test_rust_and_playwright_skip_paths_count_as_skips() -> None:
    for body in ("#[ignore]\nfn t() {}", "#[ignore = \"slow\"]", 'test.skip("x", () => {})', "test.fixme(true)",
                 'test.only("x", () => {})', 'test.describe.skip("x", () => {})'):
        assert proof.SKIP_PATHS.search(body), body
    for body in ('it.skip("x", () => {})', "it.only('x', fn)", 'describe.skip("x", () => {})', 'describe.only("x", f)',
                 '#[cfg_attr(not(feature = "db"), ignore)]', "testInfo.skip(true)", "test.info().skip()", "test.fail()",
                 'test.describe.fixme("x", f)', "test.describe.only('x', f)"):
        assert proof.SKIP_PATHS.search(body), body
    for body in ("rows.iter().skip(1)", "let only = 1;", "fn skip_the_header() {}", "let mut it = v.iter(); it.skip(1);",
                 "#[cfg_attr(test, derive(Debug))]", "fn test_failure_is_reported() {}"):
        assert not proof.SKIP_PATHS.search(body), body


def test_a_playwright_config_or_fixture_that_skips_or_filters_keeps_the_proof_unrecorded() -> None:
    """D-1473: a spec is only as unskipped as the config and fixtures that run it."""
    config, fixtures = "tests/e2e/playwright.config.ts", "tests/e2e/fixtures.ts"
    ok = marked("2")
    record, _ = proof.machine_proof("2", stage_graph("2"), stage_state("2"), world(texts=dict(ok, **{config: "// a stray `test.only` fails the run\nexport default {};"})))
    assert record is not None  # a comment naming test.only is not a skip
    for path, body in ((config, "export default { testIgnore: ['j1_*'] };"), (config, "export default { grep: /quote/ };"),
                       (config, "export default { grepInvert: /J1/ };"), (fixtures, "test.skip(true, 'no stand-ins');")):
        record, waits = proof.machine_proof("2", stage_graph("2"), stage_state("2"), world(texts=dict(ok, **{path: body})))
        assert record is None and waits == [f"{path} with no skip, testIgnore or grep in nightly commit {'n' * 12}"], body
    record, waits = proof.machine_proof("2", stage_graph("2"), stage_state("2"), world(texts=ok, missing={config}))
    assert record is None and waits == [f"{config} in nightly commit {'n' * 12}"]
