"""Plan-health check (e), report-only: will a task's change break a file it does not own? (D-3496)

From November 10 to 13, 2026 nineteen worker attempts ended on a plan gap: the task's change broke,
or needed, a test or a source file another task owns, and the worker could only report it (D-2859).
Examples: T317 removed `Webhooks::names`, which four done tests build (D-3083); T386's goal named
`tests/integration/tests/voice_calls.rs`, whose placeholder list it had to extend (D-3467); T428's goal
said `tests/integration/tests/fee_fixed_at_pick.rs` passes unchanged and it did not (D-3430). Each such
attempt costs a whole worker session. This module finds the likely ones from the plan and the tree
alone, before the claim, in two parts:

**Changed symbols.** The public Rust items the task's owned `.rs` files define (`pub` or `pub(crate)`
`fn`, `struct`, `enum`, `trait`, `const`, `static`, `type`, plus the `as` names the crate's `api.rs`
re-exports them under). A sentence of the task's goal or acceptance that says it renames, removes or
changes something (`CHANGE`) and is not one that keeps it (`KEEP`) names a symbol when a backticked span
in it holds that item's name. Every `.rs` file under `app/` or `tests/` the task may not edit (the hand-in
rule, runner/verify.py `check_paths`: not owned, not shared) that names the symbol is a likely gap. A
name in another crate counts only where the file names the defining crate (`picks::`, `use picks`), and
a name shorter than `MIN_NAME` or in `COMMON` never counts, which keeps `new` and `run` out.

**Named tests.** Each test file the goal or acceptance names (a backticked path, or a bare file name
that matches one file in the tree) that exists and that the task may not edit. A goal that says such a
test "passes unchanged" is reported too: T428's did and was wrong.

A file owned by a dependency not yet DONE is no warning (that task is planned to change it first).

Report-only (D-3496): `warnings` never holds a task or sets an exit code; scripts/plan_health.py prints
them as `WARN` lines under `--all` or `--symbols`, after check (d). Calibration against the plan-gap
fixes since November 10 is in D-3496.

Usage: python3 scripts/plan_symbols.py [--root REPO] [--state PATH] [T### ...]
  (no task IDs: every task not DONE or SKIPPED)
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import re
import sys
from collections.abc import Iterable, Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from runner import state as st
from runner import verify  # the hand-in path rule (D-1058)

ROOT = Path(__file__).resolve().parent.parent
SKIP_DIRS = {".git", "target", "node_modules", "__pycache__", ".venv"}
DEFINE = re.compile(
    r"^\s*pub(?:\(crate\))?\s+(?:(?:async|unsafe|const|extern)\s+(?=fn\b|unsafe\b|async\b))*"
    r"(fn|struct|enum|trait|const|static|type)\s+([A-Za-z_][A-Za-z0-9_]*)",
    re.MULTILINE,
)
ALIAS = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\s+as\s+([A-Za-z_][A-Za-z0-9_]*)\b")
PUB_USE = re.compile(r"pub\s+use\s+[^;]+;", re.DOTALL)
IMPL = re.compile(r"^impl(?:<[^>\n]*>)?\s+(?:[\w:<>, ]+?\s+for\s+)?([A-Za-z_]\w*)", re.MULTILINE)
CLAUSE = re.compile(r"[;:()]")
ENUM_WORDS = re.compile(r"\b[A-Z][A-Za-z0-9_]*\b")
WILDCARD = re.compile(r"\b_\s*=>")
COMMENT = re.compile(r"//[^\n]*")
QUOTED = re.compile(r'"([^"\n]{1,80})"')
VALUE_MARK = re.compile(r"^[^\s]*(?:@|\{[a-z_]+\}|[a-z0-9]-[a-z0-9])[^\s]*$")
FILE_NAME = re.compile(r"\.(?:rs|py|ts|js|md|json|ya?ml|toml|sql|html|css|txt)\b|^(?:app|tests|docs|kit|scripts|migrations)/")
PINS = re.compile(r"\bpin(?:s|ned|ning)?\b|\bto (?:a|its) (?:commit )?SHA\b", re.IGNORECASE)
ID_LIKE = re.compile(r"^(?:T\d{3}|D-\d+|TD-|[A-Z]+-\d{3})")
TICKS = re.compile(r"`([^`\n]+)`")
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
CHANGE = re.compile(
    r"\b(?:renam\w*|remov\w*|delet\w*|drops?|dropped|replac\w*|signatures?|gains?|now takes|takes (?:a |an )?new|returns? .{0,40}instead"
    r"|now returns|becomes|no longer|instead of|moves?|moved|splits?|merges?|changes?|changed"
    r"|new (?:field|argument|parameter|variant)s?|adds? (?:a |an |one |two |the |its )?(?:new )?"
    r"(?:field|argument|parameter|variant)s?)\b",
    re.IGNORECASE,
)
KEEP = re.compile(
    r"\b(?:keeps?|kept|unchanged|untouched|stays?|as it is|byte-identical|does not change|neither|nor|never"
    r"|no(?! longer))\b",
    re.I,
)
GAINS = re.compile(r"gains?", re.I)
BREAKS = re.compile(r"\b(?:fields?|arguments?|parameters?|variants?)\b|::", re.I)
UNCHANGED = re.compile(r"\b(?:unchanged|untouched|byte-identical|does not change|need(?:s)? none)\b", re.I)
SENTENCE = re.compile(r"(?<=[.;:])\s+(?=[A-Z(`])")
TEST_FILE = re.compile(r"(?:^|/)tests?/|(?:^|/)test_[^/]+\.py$|\.spec\.ts$|\.test\.ts$")
CODE_EXT = (".rs", ".py", ".ts")
MIN_NAME = 4
MIN_LITERAL = 6
MAX_LITERAL_FILES = 6
COMMON = {
    "new", "from", "into", "default", "build", "check", "name", "names", "run", "open", "send", "read",
    "write", "load", "save", "list", "get", "set", "update", "delete", "insert", "parse", "render", "handle",
    "state", "status", "kind", "text", "body", "Error", "Result", "Config", "Settings", "Router", "routes",
    "router", "test", "tests", "main", "init", "start", "stop", "close", "apply", "value", "values", "label",
}


@dataclass(frozen=True)
class Symbol:
    """A public Rust item a task's owned file defines."""

    name: str
    kind: str
    file: str
    owner: str = ""
    additive: bool = False

    @property
    def crate(self) -> str:
        """The crate folder name (`picks` for `app/picks/src/x.rs`), or "" outside `app/`."""
        parts = self.file.split("/")
        return parts[1] if parts[0] == "app" and len(parts) > 2 else ""


@dataclass
class Plan:
    """The tree and the plan, read once for every task the survey checks."""

    root: Path
    doc: dict[str, Any]
    state: dict[str, Any] | None
    files: dict[str, str]

    @property
    def graph(self) -> dict[str, dict[str, Any]]:
        """Tasks by ID."""
        return {t["id"]: t for t in self.doc["tasks"]}


def under(path: str, prefixes: Iterable[str]) -> bool:
    """A path equals one of the prefixes, or sits under one that is a folder (ends with "/")."""
    return any(path == p or (p.endswith("/") and path.startswith(p)) for p in prefixes)


def code_files(root: Path) -> Iterator[Path]:
    """Rust, Python and TypeScript files under app/ and tests/, without build output."""
    for top in ("app", "tests"):
        base = root / top
        for path in sorted(base.rglob("*")) if base.is_dir() else []:
            if path.suffix in CODE_EXT and path.is_file() and not SKIP_DIRS.intersection(path.relative_to(root).parts):
                yield path


def read_plan(root: Path, state: dict[str, Any] | None) -> Plan:
    """kit/tasks.json and every code file's text, keyed by repository path."""
    doc = json.loads((root / "kit/tasks.json").read_text(encoding="utf-8"))
    files = {p.relative_to(root).as_posix(): p.read_text(encoding="utf-8", errors="replace") for p in code_files(root)}
    return Plan(root, doc, state, files)


def may_edit(plan: Plan, task: dict[str, Any], path: str) -> bool:
    """The hand-in rule: the task owns the path or it is shared (protected paths aside)."""
    return not verify.check_paths([path], {**task, "type": "harness"}, [], plan.doc.get("shared_paths", []))


def impl_of(text: str, match: re.Match[str]) -> str:
    """The type an indented `pub fn` is a method of (the last `impl` block opened above it), else ""."""
    if match.group(1) != "fn" or not text[text.rfind("\n", 0, match.start()) + 1 : match.start() + 1].isspace():
        return ""
    blocks = list(IMPL.finditer(text, 0, match.start()))
    return blocks[-1].group(1) if blocks else ""


def owned_symbols(plan: Plan, task: dict[str, Any]) -> list[Symbol]:
    """Public items in the task's owned `.rs` files, plus their `as` names in the crate's `api.rs`."""
    owned = [p for p in plan.files if p.endswith(".rs") and under(p, task.get("owns", []))]
    found = [Symbol(m.group(2), m.group(1), p, impl_of(plan.files[p], m)) for p in owned
             for m in DEFINE.finditer(plan.files[p])]
    for sym in list(found):
        api = plan.files.get(f"app/{sym.crate}/src/api.rs", "") if sym.crate else ""
        for stmt in PUB_USE.findall(api):
            found += [Symbol(alias, sym.kind, sym.file, sym.owner) for orig, alias in ALIAS.findall(stmt) if orig == sym.name]
    return list({(s.name, s.file): s for s in found}.values())


def sentences(task: dict[str, Any]) -> list[str]:
    """The goal's and the acceptance's sentences."""
    text = f"{task.get('goal', '')} {task.get('acceptance', '')}"
    return [s for s in SENTENCE.split(text) if s.strip()]


def clauses(sentence: str) -> list[tuple[str, list[str]]]:
    """A sentence cut into clauses at `;`, `:` and parentheses outside backticks (not at commas, so "no
    route, `TextIn` or `receive_reply` signature changes" keeps its "no"), each as its text with
    the backticked spans blanked out (so `X-Twilio-Signature` is no verb) and the spans it holds."""
    spans = TICKS.findall(sentence)
    masked = TICKS.sub("\x00", sentence)
    out: list[tuple[str, list[str]]] = []
    for part in CLAUSE.split(masked):
        held = [spans.pop(0) for _ in range(part.count("\x00"))]
        out.append((part.replace("\x00", " "), held))
    return out


def lead_idents(span: str) -> list[str]:
    """The identifiers of a span's leading path, before any `(` (`rotate_photos(db, seal)` gives only
    `rotate_photos`); a span with a slash or a dot (a file, `picks.release`, a field read) gives none."""
    head = span.split("(", 1)[0].strip()
    return [] if "/" in head or "." in head else IDENT.findall(head)


def changed(task: dict[str, Any], symbols: list[Symbol], files: dict[str, str]) -> list[Symbol]:
    """The owned symbols named by a span in a clause whose own words (spans left out) change something and
    keep nothing ("no ... changes" keeps). A clause that only gains something (a method, a wrapper) changes
    nothing a user sees unless it gains a field, an argument or a variant (`BREAKS`). `Enum::NewVariant`, a
    variant the enum's file does not hold yet, is an additive change."""
    by_name: dict[str, list[Symbol]] = {}
    for sym in symbols:
        by_name.setdefault(sym.name, []).append(sym)
    hit: dict[tuple[str, str], Symbol] = {}
    for sentence in sentences(task):
        for words, spans in clauses(sentence):
            verbs = [m.group(0) for m in CHANGE.finditer(words)]
            if not verbs or KEEP.search(words) or (all(GAINS.fullmatch(v) for v in verbs) and not BREAKS.search(words)):
                continue
            for span in spans:
                idents = lead_idents(span)
                for i, ident in enumerate(idents):
                    for sym in by_name.get(ident, []):
                        new = sym.kind == "enum" and i + 1 < len(idents) and idents[i + 1] not in ENUM_WORDS.findall(
                            files.get(sym.file, ""))
                        hit.setdefault((sym.name, sym.file), dataclasses.replace(sym, additive=new))
    return [s for s in hit.values() if len(s.name) >= MIN_NAME and s.name not in COMMON]


def names_symbol(text: str, path: str, sym: Symbol) -> bool:
    """The code of `text` (comments left out) uses the symbol: a method as `Type::name`, or `.name(` in a
    file that names its type; a free function as a call or a path (`name(`, `::name`); a type, trait or
    constant as a word; for an enum gaining a variant, only a file that matches it with no `_ =>` arm
    (an exhaustive match the new variant breaks). In another crate the file names the defining crate too."""
    code = COMMENT.sub("", text)
    word = re.escape(sym.name)
    if sym.owner:
        typed = re.search(rf"\b{re.escape(sym.owner)}\b", code) and re.search(rf"\.{word}\s*\(", code)
        found = bool(re.search(rf"\b{re.escape(sym.owner)}::{word}\b", code) or typed)
    elif sym.kind == "fn":
        found = bool(re.search(rf"\b{word}\s*(?:\(|::<)|::{word}\b", code))
    elif sym.additive:
        found = bool(re.search(rf"\b{word}::\w+[^\n]*=>", code)) and not WILDCARD.search(code)
    else:
        found = bool(re.search(rf"\b{word}\b", code))
    if not found:
        return False
    if not sym.crate or path.startswith(f"app/{sym.crate}/"):
        return True
    return bool(re.search(rf"\b{sym.crate}::|\buse\s+{sym.crate}\b", code))


def planned_first(plan: Plan, task: dict[str, Any], path: str) -> bool:
    """A dependency not yet DONE owns the path: it is planned to change it before this task runs."""
    if plan.state is None:
        return False
    graph = plan.graph
    seen: set[str] = set()
    todo = list(task.get("deps", []))
    while todo:
        dep = todo.pop()
        if dep in seen:
            continue
        seen.add(dep)
        todo += graph.get(dep, {}).get("deps", [])
        if st.status(plan.state, dep) not in (st.DONE, st.SKIPPED) and under(path, graph.get(dep, {}).get("owns", [])):
            return True
    return False


def owners(plan: Plan, path: str) -> str:
    """`T088, T231` for the tasks that own the path by name (a folder owner, such as a sweep's, is left
    out), or `no task by name`."""
    ids = [t["id"] for t in plan.doc["tasks"] if path in t.get("owns", [])]
    return ", ".join(ids) or "no task by name"


def symbol_warnings(plan: Plan, task: dict[str, Any]) -> list[str]:
    """Part one: unowned files that name a symbol the task's goal changes."""
    out: list[str] = []
    for sym in sorted(changed(task, owned_symbols(plan, task), plan.files), key=lambda s: (s.name, s.file)):
        users = [
            p for p, text in plan.files.items()
            if p.endswith(".rs") and p != sym.file and names_symbol(text, p, sym)
            and not may_edit(plan, task, p) and not planned_first(plan, task, p)
        ]
        if users:
            shown = ", ".join(f"{p} ({owners(plan, p)})" for p in users[:4])
            more = f" and {len(users) - 4} more" if len(users) > 4 else ""
            risk = "; risk: exhaustive match on the new variant" if sym.additive else ""
            out.append(f"{task['id']}: its goal changes {sym.kind} `{sym.name}` ({sym.file}), which {shown}{more} "
                       f"name{risk}; it may not edit them (D-3496, report-only)")
    return out


def named_paths(plan: Plan, sentence: str) -> set[str]:
    """Code files a sentence names: backticked repository paths, or a bare file name with one match."""
    found: set[str] = set()
    for span in TICKS.findall(sentence):
        span = span.split(":")[0].strip()
        if not span.endswith(CODE_EXT):
            continue
        if "/" in span:
            found.update([span] if span in plan.files else [])
            continue
        matches = [p for p in plan.files if p.rsplit("/", 1)[-1] == span]
        found.update(matches if len(matches) == 1 else [])
    return found


def blocked(plan: Plan, task: dict[str, Any], path: str) -> bool:
    """A test file the task may not edit and no unfinished dependency is planned to change first."""
    return bool(TEST_FILE.search(path)) and not may_edit(plan, task, path) and not planned_first(plan, task, path)


def test_warnings(plan: Plan, task: dict[str, Any]) -> list[str]:
    """Part two: the tests the goal or acceptance names that exist and that the task may not edit, in one
    line; those a sentence says pass unchanged (or need no change) are listed apart."""
    named: dict[str, bool] = {}
    for sentence in sentences(task):
        for path in sorted(named_paths(plan, sentence)):
            if blocked(plan, task, path):
                named[path] = named.get(path, False) or bool(UNCHANGED.search(sentence))
    if not named:
        return []
    claims = [p for p, unchanged in named.items() if unchanged]
    told = f"; it says {', '.join(claims)} pass unchanged, which only a run can show" if claims else ""
    return [f"{task['id']}: its goal names {len(named)} test(s) it may not edit: {', '.join(named)}{told} "
            "(D-3496, report-only)"]


def literals(task: dict[str, Any]) -> set[str]:
    """Value literals a change sentence of the goal or acceptance quotes (`CHANGE`, or one that pins):
    backticked or double-quoted text of `MIN_LITERAL` or more characters that is no Rust path, no
    identifier, no file name and no route, holds two letters and no space, and has the mark of a pinned
    value: an `@` (an action tag), a `{placeholder}` or a kebab-case code; a list a change sentence opens
    with `:` or `;` carries its change on to the full stop: `actions/cache/restore@v4`,
    `charged-after-cancel`, `{cant_take}`. Words and SQL with spaces are left out: they match prose."""
    found: set[str] = set()
    carried = False
    for sentence in sentences(task):
        changes = carried or bool(CHANGE.search(sentence) or PINS.search(sentence))
        if changes:
            found.update(x.strip() for x in TICKS.findall(sentence) + QUOTED.findall(sentence))
        carried = changes and sentence.rstrip().endswith((":", ";"))
    return {
        s for s in found
        if len(s) >= MIN_LITERAL and VALUE_MARK.search(s) and re.search("[A-Za-z]{2}", s) and "::" not in s and not IDENT.fullmatch(s)
        and not FILE_NAME.search(s) and not s.startswith(("/", "GET ", "POST ")) and not ID_LIKE.match(s)
    }


def literal_warnings(plan: Plan, task: dict[str, Any]) -> list[str]:
    """Part three: test files the task may not edit that hold, verbatim, a value literal its goal quotes.
    A literal in more than `MAX_LITERAL_FILES` such tests is a common phrase, not a pinned value."""
    out: list[str] = []
    for lit in sorted(literals(task)):
        users = [p for p, text in plan.files.items() if lit in text and blocked(plan, task, p)]
        if users and len(users) <= MAX_LITERAL_FILES:
            out.append(f"{task['id']}: its goal quotes {lit!r}, which {', '.join(users)} "
                       f"({'; '.join(owners(plan, p) for p in users)}) hold; it may not edit them (D-3496, report-only)")
    return out


def task_warnings(plan: Plan, task_id: str) -> list[str]:
    """Both parts for one task; a gate, a missing task or a task with no goal gives none."""
    task = plan.graph.get(task_id)
    if task is None or task.get("type") == "gate":
        return []
    return symbol_warnings(plan, task) + literal_warnings(plan, task) + test_warnings(plan, task)


def warnings(root: Path, state: dict[str, Any] | None, task_ids: Iterable[str]) -> list[str]:
    """Every warning for the tasks, in their order; the tree is read once."""
    plan = read_plan(root, state)
    return [w for tid in task_ids for w in task_warnings(plan, tid)]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
    parser.add_argument("task", nargs="*", help="task IDs; none means every task not DONE or SKIPPED")
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--state", type=Path, help="a kit/state.json file; without it no dependency is excused")
    args = parser.parse_args(argv)
    state = json.loads(args.state.read_text(encoding="utf-8")) if args.state else None
    doc = json.loads((args.root / "kit/tasks.json").read_text(encoding="utf-8"))
    ids = args.task or [t["id"] for t in doc["tasks"] if st.status(state or st.new_state(), t["id"]) not in (st.DONE, st.SKIPPED)]
    found = warnings(args.root, state, ids)
    print("\n".join(f"WARN {line}" for line in found) + ("\n" if found else "") + f"{len(found)} warning(s) (D-3496)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
