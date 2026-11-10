"""Tests for scripts/plan_pins.py, plan-health check (d), report-only (D-2550): the T290 and T231 patterns
(D-2386, D-2429) as fake repositories, the cases that are no warning, and plan_health's report. Fake tasks only."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import plan_health as ph
import plan_pins as pp
from test_plan_health import make_repo, plan_task

from runner import state as st

EXAMPLES = "examples:\n  - id: W11\n  - id: W12\n  - id: W13\n  - id: W26\n"
WORKED = '''//! Fake worked-example test.
const EXAMPLES: &str = include_str!("../../../../docs/catalog/worked_examples.yaml");
/// Rows with a recurring period.
const RECURRING: [&str; 3] = ["W11", "W12", "W13"];
'''
RECURRING = '''//! Fake recurring test.
const EXAMPLES: &str = include_str!("../../../../docs/catalog/worked_examples.yaml");

#[test]
fn every_recurring_row_matches() {
    let rows = parse(EXAMPLES);
    let mut checked = 0;
    for row in &rows {
        checked += 1;
    }
    assert_eq!(checked, 4);
}
'''
MATRIX = '''//! Fake authorization matrix.
fn door(path: &str) -> Door {
    let under = |root: &str| path == root || path.starts_with(&format!("{root}/"));
    if under("/webhooks") {
        Door::Webhook
    } else if path.contains("/sign-in") {
        Door::SignIn
    } else if ["/q/{token}", "/f/{token}"].iter().any(|root| path.starts_with(root)) {
        Door::Link
    } else if under("/b") {
        Door::Business
    } else {
        panic!("{path} has no door in auth_matrix.rs: add its matrix rule (SEC-019)")
    }
}

fn route_table(line: &str) -> Option<&str> {
    let (_, call) = line.split_once(".route(")?;
    let open = call.strip_prefix('"');
    open
}

#[test]
fn every_path_has_a_door() {
    for (path, methods) in &table {
        let realm = matches!(door(path), Door::Business);
        assert!(!realm || methods.is_empty(), "{path} is served: extend the matrix");
    }
}
'''
ADD_ROWS = "Add rows to `docs/catalog/worked_examples.yaml` after W13. Each row is fake."
SERVE = "This task also serves the answer page (D-2128): `app/server/src/checkin.rs` builds its `GET`."


def write(root: Path, rel: str, text: str) -> None:
    (root / rel).parent.mkdir(parents=True, exist_ok=True)
    (root / rel).write_text(text, encoding="utf-8")


def t290_repo(tmp_path: Path, worked: str = WORKED, recurring: str = RECURRING, **over: Any) -> Path:
    """T290 before D-2386: it owns the examples file and adds rows; T058 and T188 own the two tests."""
    tasks = [plan_task("T058", owns=["app/payments/src/tests/worked_examples.rs"]),
             plan_task("T188", owns=["app/payments/src/tests/recurring.rs"]),
             plan_task("T290", ["T058"], type="harness", owns=["docs/catalog/worked_examples.yaml"], goal=ADD_ROWS)]
    log = over.pop("log", "")
    tasks[2].update(over)
    root = make_repo(tmp_path, tasks, log)
    write(root, "docs/catalog/worked_examples.yaml", EXAMPLES)
    write(root, "app/payments/src/tests/worked_examples.rs", worked)
    write(root, "app/payments/src/tests/recurring.rs", recurring)
    return root


def t231_repo(tmp_path: Path, goal: str = SERVE, deps: list[str] | None = None, owns_matrix: str = "T088") -> Path:
    """T231 before D-2429: it serves a page from app/ source; another task owns the route matrix."""
    tasks = [plan_task("T088", owns=["tests/security/tests/auth_matrix.rs"] if owns_matrix == "T088" else []),
             plan_task("T348", owns=["tests/security/tests/auth_matrix.rs"] if owns_matrix == "T348" else []),
             plan_task("T231", deps or [], owns=["app/server/src/checkin.rs"], goal=goal)]
    root = make_repo(tmp_path, tasks)
    write(root, "tests/security/tests/auth_matrix.rs", MATRIX)
    return root


def warned(root: Path, state: st.State, tid: str) -> list[str]:
    return pp.warnings(root, state, [tid])


def done(*ids: str) -> st.State:
    state = st.new_state()
    for tid in ids:
        state = st.with_task(state, tid, status=st.DONE)
    return state


# ---- T290's pattern: rows added to a data file two tests pin by data (D-2386)

def test_t290_rows_added_to_a_file_two_tests_pin_are_warned(tmp_path: Path) -> None:
    root = t290_repo(tmp_path)
    found = warned(root, done("T058", "T188"), "T290")
    assert len(found) == 2
    recurring = next(w for w in found if "recurring.rs" in w)
    worked = next(w for w in found if "worked_examples.rs:" in w)
    assert "app/payments/src/tests/recurring.rs:11 checked (exact count == 4)" in recurring
    assert "owner T188 (DONE)" in recurring and "T290 may not edit app/payments/src/tests/recurring.rs" in recurring
    assert "app/payments/src/tests/worked_examples.rs:4 RECURRING (exact list of 3 ids)" in worked
    assert "its goal adds to docs/catalog/worked_examples.yaml" in worked and "report-only" in worked


def test_a_task_that_owns_the_tests_is_not_warned(tmp_path: Path) -> None:
    """D-2386's fix: T290 also owns both tests, for data only."""
    owns = ["docs/catalog/worked_examples.yaml", "app/payments/src/tests/worked_examples.rs",
            "app/payments/src/tests/recurring.rs"]
    assert warned(t290_repo(tmp_path, owns=owns), done("T058", "T188"), "T290") == []


def test_a_decision_that_lets_the_task_edit_the_test_clears_it(tmp_path: Path) -> None:
    log = ("- **D-001** Question: x? Choice: T290 may update `app/payments/src/tests/recurring.rs` and "
           "`app/payments/src/tests/worked_examples.rs` by data only.\n")
    root = t290_repo(tmp_path, decisions=["D-001"], log=log)
    assert warned(root, done("T058", "T188"), "T290") == []


def test_a_decision_that_only_names_the_test_does_not_clear_it(tmp_path: Path) -> None:
    log = "- **D-001** Question: x? Choice: `app/payments/src/tests/recurring.rs` must pass unchanged.\n"
    root = t290_repo(tmp_path, decisions=["D-001"], log=log)
    assert len(warned(root, done("T058", "T188"), "T290")) == 2


def test_assertions_that_are_not_exact_are_no_pin(tmp_path: Path) -> None:
    """`>=`, a count of 1 (one row the test made) and a list of plain words are no pins."""
    worked = WORKED.replace('const RECURRING: [&str; 3] = ["W11", "W12", "W13"];',
                            'const KINDS: [&str; 2] = ["id", "examples"];')
    recurring = RECURRING.replace("assert_eq!(checked, 4);", "assert!(checked >= 4);\n    assert_eq!(rows.len(), 1);")
    assert warned(t290_repo(tmp_path, worked, recurring), done("T058", "T188"), "T290") == []


def test_a_goal_that_does_not_add_to_the_file_is_not_warned(tmp_path: Path) -> None:
    root = t290_repo(tmp_path, goal="Read `docs/catalog/worked_examples.yaml` and check W11 against the rules.")
    assert warned(root, done("T058", "T188"), "T290") == []


def test_a_task_that_cannot_write_the_file_is_not_warned(tmp_path: Path) -> None:
    """Adding to a file the task does not own is check (b)'s gap, not this one."""
    assert warned(t290_repo(tmp_path, owns=["app/t290.rs"]), done("T058", "T188"), "T290") == []


def test_a_python_test_that_writes_its_own_fake_file_is_no_pin(tmp_path: Path) -> None:
    root = t290_repo(tmp_path)
    write(root, "tests/test_fake.py", 'def test_x(tmp_path):\n    (tmp_path / "docs/catalog/worked_examples.yaml")'
                                      '.write_text("x")\n    rows = load("docs/catalog/worked_examples.yaml")\n'
                                      '    assert len(rows) == 4\n')
    assert [p.test for p in pp.find_pins(root)] == ["app/payments/src/tests/recurring.rs",
                                                    "app/payments/src/tests/worked_examples.rs"]


# ---- T231's pattern: a route served that the matrix's door function does not name (D-2429)

def test_t231_a_served_page_with_no_named_path_is_warned(tmp_path: Path) -> None:
    found = warned(t231_repo(tmp_path), done("T088"), "T231")
    assert len(found) == 1
    assert found[0].startswith("T231: its goal serves a route it does not name (the classifier knows only "
                               "/webhooks, /sign-in, /q/{token}, /f/{token}, /b)")
    assert "tests/security/tests/auth_matrix.rs:13 door()" in found[0] and "owner T088 (DONE)" in found[0]


def test_a_named_path_no_door_names_is_warned_and_a_covered_one_is_not(tmp_path: Path) -> None:
    named = "This task also serves `GET /c/{token}/{answer}` and its `POST`."
    assert "/c/{token}/{answer} falls under no branch" in warned(t231_repo(tmp_path, named), done("T088"), "T231")[0]
    covered = "It also serves a new route `/q/{token}/close` and `/b/sign-in`."
    assert warned(t231_repo(tmp_path / "b", covered), done("T088"), "T231") == []


def test_a_named_path_under_a_door_the_test_fails_once_served_is_warned(tmp_path: Path) -> None:
    found = warned(t231_repo(tmp_path, "It serves `/b/jobs`."), done("T088"), "T231")
    assert "/b/jobs is a Business route, which the test fails once served" in found[0]


def test_a_dependency_not_yet_done_that_owns_the_matrix_clears_it(tmp_path: Path) -> None:
    """D-2440: T348 extends the matrix before every serving task; once DONE, a hit is real again."""
    root = t231_repo(tmp_path, deps=["T348"], owns_matrix="T348")
    assert warned(root, st.new_state(), "T231") == []
    assert len(warned(root, done("T348"), "T231")) == 1


def test_a_goal_where_another_task_serves_or_no_route_is_added_is_not_warned(tmp_path: Path) -> None:
    goal = "T301 serves the email action links; no route is added. `stand_ins/stripe.py` serves `GET /v1/charges`."
    assert warned(t231_repo(tmp_path, goal), done("T088"), "T231") == []


def test_a_task_that_owns_the_matrix_is_not_warned(tmp_path: Path) -> None:
    root = t231_repo(tmp_path)
    doc = json.loads((root / "kit/tasks.json").read_text(encoding="utf-8"))
    doc["tasks"][2]["owns"].append("tests/security/tests/auth_matrix.rs")
    (root / "kit/tasks.json").write_text(json.dumps(doc), encoding="utf-8")
    assert warned(root, done("T088"), "T231") == []


# ---- plan_health: printed, never a gap, never the exit code (D-2550)

def test_plan_health_prints_warnings_but_holds_nothing(tmp_path: Path, capsys: Any) -> None:
    root = t290_repo(tmp_path)
    (root / "docs/catalog/worked_examples.yaml").write_text(EXAMPLES, encoding="utf-8")
    state_file = root / "state.json"
    state_file.write_text(json.dumps(done("T058", "T188")), encoding="utf-8")
    doc = json.loads((root / "kit/tasks.json").read_text(encoding="utf-8"))
    assert ph.checker(root, doc)(done("T058", "T188"), "T290") == []  # the runner's hook: nothing held
    assert ph.main(["--root", str(root), "--state", str(state_file), "--pinned", "T290"]) == 0
    out = capsys.readouterr().out
    assert out.count("WARN T290: its goal adds to docs/catalog/worked_examples.yaml") == 2
    assert "with gaps, 2 report-only pinned-list warning(s) (D-2550)" in out
    assert ph.main(["--root", str(root), "--state", str(state_file), "--pinned", "--json", "T290"]) == 0
    found = json.loads(capsys.readouterr().out)
    assert found["T290"] == [] and len(found[pp.WARN_KEY]) == 2
    assert ph.main(["--root", str(root), "--state", str(state_file), "T290"]) == 0
    assert "WARN" not in capsys.readouterr().out  # a single-task check without --pinned prints only (a) to (c)
