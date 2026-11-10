"""Plan-health check (d), report-only: does a task add to a list an existing test pins? (D-2550)

A test that pins a list by exact count or exact contents fails the moment a later task adds to that
list. When the later task may not edit the test (another task owns it, or it is protected), its
attempt fails on a gap in the plan, not in its work. Two such failures on November 10, 2026:

- T290 (D-2386) added rows W41 to W62 to `docs/catalog/worked_examples.yaml`, which
  `app/payments/src/tests/worked_examples.rs` pins by its 13-id `RECURRING` list and
  `app/payments/src/tests/recurring.rs` by `assert_eq!(checked, 14)`; it owned neither test.
- T231 (D-2429) served a new route, `/c/{token}/{answer}`; `tests/security/tests/auth_matrix.rs`
  reads every `.route(` in `app/*/src` and its `door` function panics on a path no door names; it
  did not own the matrix.

This module finds both kinds of pin from the test files alone, then judges a task from the plan
alone (`kit/tasks.json`, its goal, its decisions, kit/state.json):

**Data pins.** A test file that reads a repository data file (a quoted path such as
`include_str!("../../docs/catalog/x.yaml")` or `"tests/fixtures/x.json"`; not a path the test
writes itself, not `docs/decisions.md` or `kit/`) and holds an exact count: `assert_eq!(x.len(), N)`
or `assert_eq!(counter, N)` for a counter the file increments (Rust), `assert len(x) == N`
(Python), `toHaveLength(N)` (TypeScript), or a `const NAME: [&str; N]` of ids (each holds a digit)
every one of which appears in a data file the test reads. N below 2 is no pin (one row the test
made). A count belongs to the data files its function names; in a Rust test that reads one data
file and whose function names none, to that file. A task trips one when it may write the data file
(owns it or it is shared) and a sentence of its goal names the file and says it adds to it (add,
append, insert, extend, new row).

**Route pins.** A Rust test that reads the route table from source (a `".route("` literal) and
classifies each path in a `fn x(path: &str) -> Door` that panics on a path no branch names. A task
trips one when it owns `app/` source and a sentence of its goal has the task itself serve something
("This task also serves", "it serves", "a new route"; not a sentence where another task, a stand-in
or no task serves): each route path such a sentence quotes in backticks must fall under a branch,
and not under one the test fails once served (`matches!(door(path), Door::A | ...)` beside
`methods.is_empty()`); a serving sentence that quotes no path is reported too, with the prefixes the
classifier knows (T231's goal named no path; its worker chose `/c/{token}/{answer}`).

A pin is no warning when the task may edit the test (the hand-in rule, runner/verify.py
`check_paths`: it owns it or it is shared), when a task that owns the test is a dependency not yet
DONE (that task is planned to extend it, as T348 is for the matrix, D-2440), or when a decision the
task pastes names the test path in a sentence that lets it edit the test (D-2386's form).

Report-only (D-2550): `warnings` never holds a task or fails a check; scripts/plan_health.py prints
them as WARN lines under `--all` or `--pinned`. D-2551 (reserved) makes them holding problems once
they have shown no false positives.

Usage: python3 scripts/plan_pins.py [--root REPO] [--state PATH] [--pins] [T### ...]
  (no task IDs: every task not DONE or SKIPPED; --pins lists the pins found instead)
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections.abc import Iterable, Iterator
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from runner import verify  # the hand-in path rule (D-1058)

ROOT = Path(__file__).resolve().parent.parent
SKIP_DIRS = {".git", "target", "node_modules", "__pycache__", ".venv", "fixtures"}
DATA_PATH = re.compile(
    r"""(["'])(?:[^"'\n]*?/)?((?:docs|kit|migrations|tests|app|twin|stand_ins)/[A-Za-z0-9_./-]+"""
    r"""\.(?:ya?ml|json|toml|csv|txt|md|sql|html))\1"""
)
# A module-level name bound on the path's line (`const FILE: &str = include_str!(..)`, `GOLDEN = ...`); a
# name bound inside a function is not needed (the path itself is then in the function's text).
BINDING = re.compile(r"^(?:pub\s+)?(?:(?:const|static|let|export\s+const)\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*(?::[^=\n]*)?=")
RUST_EQ = re.compile(r"assert_eq!\(\s*([^,;]+?)\s*,\s*([^,;]+?)\s*[,)]")
PY_LEN = re.compile(r"^\s*assert\s+((?:len|sum)\(.+?\))\s*==\s*(\d+)\b", re.MULTILINE)
TS_LEN = re.compile(r"(\S+?)(?:\)\.toHaveLength\(|\.length\)\.toBe\(|\.length\s*===?\s*)(\d+)\)?")
CONST_LIST = re.compile(r"const\s+([A-Z_][A-Z0-9_]*)\s*:\s*\[&str;\s*(\d+)\]\s*=\s*\[(.*?)\];", re.DOTALL)
STRING = re.compile(r'"([^"\n]+)"')
FN_START = re.compile(r"^\s*(?:pub\s+)?(?:async\s+)?(?:fn|def|function)\s+\w+|^\s*(?:test|it)\(", re.MULTILINE)
WRITES = re.compile(r"write_text\(|write_bytes\(|\.write\(|writeFileSync\(|fs::write\(")
STRING_END = re.compile(r'(?<!\\)"')
ROUTE_SCAN = re.compile(r'"[^"\n]*\.route\([^"\n]*"')
CLASSIFIER = re.compile(r"fn\s+(\w+)\s*\(\s*\w+\s*:\s*&str\s*\)\s*->\s*(\w+)\s*\{")
VARIANT = re.compile(r"\b([A-Z]\w*)::([A-Z]\w*)\b")
ROUTE_QUOTE = re.compile(r"`(?:(?:GET|POST|PUT|PATCH|DELETE) )?(/[A-Za-z0-9_{}./-]*)`")
# A sentence in which the task itself serves something: "This task also serves the answer page", "it
# serves", "a new route" (a sentence where another task, a stand-in or "no task" serves is not one).
SERVES = re.compile(r"\b(?:[Tt]his task|[Ii]t)(?: also)? serves\b|\bnew route\b|\badds? (?:a|the|its|one) (?:new )?route\b")
ADDS = re.compile(r"\b(?:adds?|adding|append(?:s|ing)?|insert(?:s|ing)?|extends?|new rows?)\b", re.IGNORECASE)
GRANTS = re.compile(r"\b(?:also owns?|owns? (?:both|it|them|the)|may (?:edit|update|change))\b")
SENTENCE = re.compile(r"(?<=[.;])\s+(?=[A-Z(`])")
# The plan's own files: every task appends to them, so a count over them is no list a task adds to.
PLAN_FILES = ("docs/decisions.md", "kit/")


@dataclass(frozen=True)
class Door:
    """One branch of a route classifier: the result it gives and the path literals it tests."""

    name: str
    prefixes: tuple[str, ...]
    contains: tuple[str, ...] = ()

    def covers(self, path: str) -> bool:
        """The path equals a prefix, sits under one, or holds a `contains(...)` literal."""
        under = any(path == p or path.startswith(p.rstrip("/") + "/") for p in self.prefixes)
        return under or any(c in path for c in self.contains)


@dataclass(frozen=True)
class Pin:
    """An exact list or count in a test: `data` pins name the data files, `route` pins the doors."""

    kind: str
    test: str
    line: int
    symbol: str
    detail: str
    sources: tuple[str, ...] = ()
    doors: tuple[Door, ...] = ()
    refused: frozenset[str] = field(default_factory=frozenset)

    def where(self) -> str:
        """`path:line` of the pinning assertion."""
        return f"{self.test}:{self.line}"


def test_files(root: Path) -> Iterator[Path]:
    """Rust, Python and TypeScript test files under app/ and tests/ (no build output, no fixtures)."""
    for top in ("app", "tests"):
        for path in sorted((root / top).rglob("*")) if (root / top).is_dir() else []:
            rel = path.relative_to(root).parts
            if SKIP_DIRS.intersection(rel) or path.suffix not in (".rs", ".py", ".ts") or not path.is_file():
                continue
            if "tests" in rel[:-1] or path.name.startswith("test_") or path.name.endswith((".spec.ts", ".test.ts")):
                yield path


def line_of(text: str, offset: int) -> int:
    """1-based line number of a character offset."""
    return text.count("\n", 0, offset) + 1


def data_reads(text: str) -> dict[str, set[str]]:
    """Each repository data file a test reads, with the names bound to it (`const FILE = include_str!(..)`).
    A path the test writes itself (a fake repository built in a temporary folder) is no read."""
    found: dict[str, set[str]] = {}
    written = {m.group(2) for line in text.splitlines() if WRITES.search(line) for m in DATA_PATH.finditer(line)}
    for line in text.splitlines():
        for match in DATA_PATH.finditer(line):
            if match.group(2) in written or under(match.group(2), PLAN_FILES):
                continue
            names = found.setdefault(match.group(2), set())
            bound = BINDING.match(line)
            if bound:
                names.add(bound.group(1))
    return found


def region_sources(text: str, offset: int, reads: dict[str, set[str]], whole: bool) -> tuple[str, ...]:
    """The data files the function around `offset` names (by path or binding); when it names none and
    `whole` (a Rust test), the test's one data file if it reads only one (a test about one file, which
    loads it through the crate, as catalog_settings.rs does), else none. Python and TypeScript tests
    mostly build fake files, so theirs must be named in the function."""
    starts = [m.start() for m in FN_START.finditer(text, 0, offset)]
    body = text[starts[-1] if starts else 0 : offset]
    named = [p for p, names in reads.items() if p in body or any(re.search(rf"\b{n}\b", body) for n in names)]
    return tuple(sorted(named or (reads if whole and len(reads) == 1 else ())))


def exact_count(left: str, right: str, text: str) -> tuple[str, int] | None:
    """`(expression, N)` when one side is a literal N >= 2 and the other a length or a counter the file
    increments (`checked += 1`); None otherwise."""
    for expr, lit in ((left, right), (right, left)):
        number = lit.replace("_", "")
        if not number.isdigit() or int(number) < 2:
            continue
        expr = expr.strip()
        is_len = expr.endswith((".len()", ".count()")) and ".matches(" not in expr
        is_counter = re.fullmatch(r"[a-z_][a-z0-9_]*", expr) and re.search(rf"\b{expr}\s*\+=\s*1\b", text)
        if is_len or is_counter:
            return expr, int(number)
    return None


def count_pins(rel: str, text: str, reads: dict[str, set[str]]) -> list[Pin]:
    """Exact-count assertions in a test that reads data files (Rust, Python, TypeScript forms)."""
    hits: list[tuple[int, str, int]] = []
    for m in RUST_EQ.finditer(text):
        found = exact_count(m.group(1), m.group(2), text)
        if found:
            hits.append((m.start(), *found))
    hits += [(m.start(), m.group(1), int(m.group(2))) for m in PY_LEN.finditer(text) if int(m.group(2)) >= 2]
    hits += [(m.start(), f"{m.group(1)}.length", int(m.group(2))) for m in TS_LEN.finditer(text) if int(m.group(2)) >= 2]
    pins = [Pin("data", rel, line_of(text, at), e, f"exact count == {n}", region_sources(text, at, reads, rel.endswith(".rs"))) for at, e, n in hits]
    return [p for p in pins if p.sources]


def list_pins(rel: str, text: str, reads: dict[str, set[str]], data: dict[str, str]) -> list[Pin]:
    """`const NAME: [&str; N]` lists of ids (each literal holds a digit, as `W11` or `LIC-012` does) whose
    every literal is a token of a data file the test reads."""
    pins: list[Pin] = []
    for m in CONST_LIST.finditer(text):
        items = STRING.findall(m.group(3))
        if int(m.group(2)) < 2 or not items or not all(re.search(r"\d", i) for i in items):
            continue
        holders = tuple(p for p in sorted(reads) if all(re.search(rf"(?<![\w-]){re.escape(i)}(?![\w-])", data.get(p, "")) for i in items))
        if holders:
            pins.append(Pin("data", rel, line_of(text, m.start()), m.group(1), f"exact list of {m.group(2)} ids", holders))
    return pins


def fn_body(text: str, start: int) -> str:
    """The brace-balanced body of the Rust function whose opening brace is at `start - 1` (string
    literals are skipped, so a `{token}` inside one does not count)."""
    depth, at = 1, start
    while at < len(text) and depth:
        if text.startswith(("'\"'", "'{'", "'}'"), at):
            at += 3
            continue
        if text[at] == '"':
            close = STRING_END.search(text, at + 1)
            at = close.end() if close else len(text)
            continue
        depth += {"{": 1, "}": -1}.get(text[at], 0)
        at += 1
    return text[start:at]


def doors_of(body: str) -> tuple[Door, ...]:
    """The branches of an if/else classifier: each branch's result variant and its path literals."""
    doors: list[Door] = []
    for part in re.split(r"\}\s*else\b", body):
        variants = VARIANT.findall(part)
        literals = [s for s in STRING.findall(part) if s.startswith("/")]
        if not variants or not literals:
            continue
        contains = tuple(s for s in literals if f'contains("{s}")' in part)
        doors.append(Door(variants[-1][1], tuple(s for s in literals if s not in contains), contains))
    return tuple(doors)


def route_pins(rel: str, text: str) -> list[Pin]:
    """A test that reads the route table from source and classifies each path in a function that
    panics on a path no branch names (T088's `door`, SEC-019); the doors it fails once served too."""
    if not rel.endswith(".rs") or not ROUTE_SCAN.search(text):
        return []  # only a Rust test: a Python test that holds such Rust as a fixture string is none
    pins: list[Pin] = []
    for m in CLASSIFIER.finditer(text):
        body = fn_body(text, m.end())
        if "panic!(" not in body and "unreachable!(" not in body:
            continue
        refused: set[str] = set()
        for served in re.finditer(rf"matches!\(\s*{m.group(1)}\(\w+\),\s*([^)]+)\)", text):
            if "is_empty()" in text[served.end() : served.end() + 200]:
                refused |= {v for _, v in VARIANT.findall(served.group(1))}
        doors = doors_of(body)
        if not doors:
            continue
        line = line_of(text, m.start() + body.find("panic!(") + len(m.group(0)))
        pins.append(Pin("route", rel, line, f"{m.group(1)}()", "every served route must fall under a branch",
                        ("app/",), doors, frozenset(refused)))
    return pins


def find_pins(root: Path) -> list[Pin]:
    """Every data and route pin in the repository's test files."""
    pins: list[Pin] = []
    for path in test_files(root):
        rel = path.relative_to(root).as_posix()
        text = path.read_text(encoding="utf-8", errors="replace")
        pins += route_pins(rel, text)
        reads = {p: n for p, n in data_reads(text).items() if (root / p).is_file()}
        if not reads:
            continue
        data = {p: (root / p).read_text(encoding="utf-8", errors="replace") for p in reads}
        pins += count_pins(rel, text, reads) + list_pins(rel, text, reads, data)
    return pins


def sentences(text: str) -> list[str]:
    """A goal split into sentences, roughly (after a full stop or semicolon)."""
    return [s for s in SENTENCE.split(text) if s.strip()]


def ancestors(graph: dict[str, dict[str, Any]], task_id: str) -> set[str]:
    """Every task the given one depends on, directly or through other dependencies."""
    seen: set[str] = set()
    todo = list(graph.get(task_id, {}).get("deps", []))
    while todo:
        dep = todo.pop()
        if dep not in seen:
            seen.add(dep)
            todo.extend(graph.get(dep, {}).get("deps", []))
    return seen


def under(path: str, prefixes: Iterable[str]) -> bool:
    """A path equals one of the prefixes, or sits under one that is a folder (ends with "/")."""
    return any(path == p or (p.endswith("/") and path.startswith(p)) for p in prefixes)


def owners(doc: dict[str, Any], path: str) -> list[str]:
    """IDs of the tasks that own the path by name; when none does, those owning a folder above it
    (cleanup and war-games tasks own `app/` whole, which says nothing about this test)."""
    exact = [t["id"] for t in doc["tasks"] if path in t.get("owns", [])]
    return exact or [t["id"] for t in doc["tasks"] if under(path, t.get("owns", []))]


def decision_texts(root: Path) -> dict[str, str]:
    """Each `- **D-###**` entry of docs/decisions.md by its ID (the last entry wins, as gen_prompts reads it)."""
    path = root / "docs/decisions.md"
    text = path.read_text(encoding="utf-8") if path.is_file() else ""
    return {m.group(1): m.group(2) for m in re.finditer(r"^- \*\*(D-\d+)\*\*(.*)$", text, re.MULTILINE)}


@dataclass(frozen=True)
class Plan:
    """What the judgement reads: the task graph, each task's status, and the decision log."""

    doc: dict[str, Any]
    status: dict[str, str]
    decisions: dict[str, str]

    def task(self, task_id: str) -> dict[str, Any] | None:
        """The task's entry in kit/tasks.json, or None."""
        return next((t for t in self.doc["tasks"] if t["id"] == task_id), None)


def excused(plan: Plan, task: dict[str, Any], test: str) -> bool:
    """The task may edit the test (runner/verify.py `check_paths`), a dependency not yet DONE owns it
    (planned to extend it, D-2440), or a decision it pastes lets it edit the test (D-2386's form)."""
    if not verify.check_paths([test], task, plan.doc.get("protected_paths", []), plan.doc.get("shared_paths", [])):
        return True
    graph = {t["id"]: t for t in plan.doc["tasks"]}
    upstream = ancestors(graph, task["id"])
    named = [t["id"] for t in plan.doc["tasks"] if test in t.get("owns", [])]
    if any(o in upstream and plan.status.get(o, "QUEUED") not in ("DONE", "SKIPPED") for o in named):
        return True
    said = [s for did in task.get("decisions", []) for s in sentences(plan.decisions.get(did, ""))]
    return any(test in s and GRANTS.search(s) for s in said)


def data_reason(plan: Plan, task: dict[str, Any], pin: Pin) -> str | None:
    """Why the task adds to a data file the pin reads: it may write the file and a goal sentence names
    the file with an adding verb; None when it does not."""
    shared = plan.doc.get("shared_paths", [])
    for source in pin.sources:
        if not (under(source, task.get("owns", [])) or under(source, shared)):
            continue
        name = Path(source).name
        for sentence in sentences(str(task.get("goal", ""))):
            if (source in sentence or name in sentence) and ADDS.search(sentence):
                return f"its goal adds to {source} (\"{sentence.strip()[:140]}\")"
    return None


def route_reason(task: dict[str, Any], pin: Pin) -> str | None:
    """Why the task's served routes fail the classifier: a quoted path no branch names or one under a
    door the test fails once served; or a serving goal that quotes no path at all."""
    goal = str(task.get("goal", ""))
    writes_app = any(o.startswith("app/") and o.endswith(".rs") and "/tests/" not in o for o in task.get("owns", []))
    serving = [s for s in sentences(goal) if SERVES.search(s) and "stand_ins/" not in s]
    if not writes_app or not serving:
        return None
    paths = sorted({p for s in serving for p in ROUTE_QUOTE.findall(s) if len(p) > 1})
    if not paths:
        known = ", ".join(p for d in pin.doors for p in d.prefixes + d.contains)
        return f"its goal serves a route it does not name (the classifier knows only {known})"
    bad: list[str] = []
    for path in paths:
        door = next((d for d in pin.doors if d.covers(path)), None)
        if door is None:
            bad.append(f"{path} falls under no branch")
        elif door.name in pin.refused:
            bad.append(f"{path} is a {door.name} route, which the test fails once served")
    return f"its goal serves {'; '.join(bad)}" if bad else None


def owner_note(plan: Plan, test: str) -> str:
    """`owner T058 (DONE)`, with `protected` when the test sits on a protected path."""
    named = [f"{o} ({plan.status.get(o, 'QUEUED')})" for o in owners(plan.doc, test)] or ["no task"]
    guard = ", protected" if under(test, plan.doc.get("protected_paths", [])) else ""
    return f"owner {', '.join(named)}{guard}"


def task_warnings(plan: Plan, pins: list[Pin], task_id: str) -> list[str]:
    """One report-only warning per pinning test the task would trip, naming each pinning line."""
    task = plan.task(task_id)
    if task is None or task.get("type") == "gate":
        return []
    tripped: dict[str, tuple[str, list[Pin]]] = {}
    for pin in pins:
        reason = data_reason(plan, task, pin) if pin.kind == "data" else route_reason(task, pin)
        if reason and not excused(plan, task, pin.test):
            tripped.setdefault(pin.test, (reason, []))[1].append(pin)
    out: list[str] = []
    for test, (reason, hits) in tripped.items():
        lines = "; ".join(f"{p.where()} {p.symbol} ({p.detail})" for p in hits)
        out.append(f"{task_id}: {reason}, but {lines} pins it ({owner_note(plan, test)}) "
                   f"and {task_id} may not edit {test} (D-2550, report-only)")
    return out


def read_plan(root: Path, state: dict[str, Any]) -> Plan:
    """The plan as the judgement needs it, from kit/tasks.json, the given state and docs/decisions.md."""
    doc = json.loads((root / "kit/tasks.json").read_text(encoding="utf-8"))
    status = {t["id"]: str(t.get("status")) for t in state.get("tasks", []) if isinstance(t, dict) and "id" in t}
    return Plan(doc, status, decision_texts(root))


def warnings(root: Path, state: dict[str, Any], task_ids: Iterable[str]) -> list[str]:
    """Check (d) over the given tasks: every warning, in task order. Never a holding problem (D-2550)."""
    plan = read_plan(root, state)
    pins = find_pins(root)
    return [w for tid in task_ids for w in task_warnings(plan, pins, tid)]


def main(argv: list[str] | None = None) -> int:
    """Print the warnings (or with --pins the pins found); exit 0 whatever is found, 2 on unreadable inputs."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
    parser.add_argument("task", nargs="*", help="task IDs such as T290 (default: every task not DONE or SKIPPED)")
    parser.add_argument("--state", type=Path, help="a kit/state.json file (default: no task is DONE)")
    parser.add_argument("--pins", action="store_true", help="list the pins found instead")
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args(argv)
    try:
        state = json.loads(args.state.read_text(encoding="utf-8")) if args.state else {"tasks": []}
        plan = read_plan(args.root, state)
        pins = find_pins(args.root)
    except (OSError, ValueError, KeyError) as error:
        print(f"ERROR inputs unreadable: {str(error)[:300]}")
        return 2
    if args.pins:
        print("\n".join(f"{p.kind:5} {p.where()} {p.symbol} ({p.detail}) <- {', '.join(p.sources)}" for p in pins))
        return 0
    ids = args.task or [t["id"] for t in plan.doc["tasks"] if plan.status.get(t["id"]) not in ("DONE", "SKIPPED")]
    found = [w for tid in ids for w in task_warnings(plan, pins, tid)]
    print("\n".join(f"WARN {w}" for w in found) + ("\n" if found else "") + f"checked {len(ids)} tasks, {len(found)} warning(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
