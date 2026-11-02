"""Generate kit/prompts/T###.md from kit/tasks.json and kit/prompts/TEMPLATE.md (kit step K7).

Every prompt comes from the one template, so they stay consistent. Gate tasks
have no prompt. Each prompt pastes what its task rests on, so a worker never
loads whole documents to find its slice (plan change P-1, D-485):

- the full sentence of every requirement the task names (from docs/spec, read
  through scripts/trace_spec.py), with its file and line;
- the full row of every test-design entry it names, and the text of any
  journey (J1 to J4) from docs/test-design.md section 5;
- the question (200 characters) and the choice (600 characters from "Choice:") of every decision it names;
- the exact text of every cited source line;
- a "Run your tests" block built from the task's owned test files.

`reads` stay as pointers. A goal equal to the title is an error: every task
says what done looks like.

budget.context_pct is derived, never guessed: the bytes of the generated
prompt, of the owned regular files that exist and of the reads that exist,
over a 200k-token window at about 4 bytes per token, rounded up. A spec file,
the test design and the decisions log count only through the lines the prompt
pastes from them; an owned directory counts nothing and an owned file is never
counted twice through a read (D-1036). A read of docs/architecture.md counts
as its section 2 (the crate layout every prompt points to) plus the lines that
name the task's module (`<module>` or app/<module>/), not the whole file: it is
over 170k bytes and workers read only section 2 and line ranges (D-1350). A
task that owns it counts it whole. Generating writes the value back into
kit/tasks.json.

With --check, nothing is written: the script exits 1 if a prompt is missing,
stale (differs from what the template and the stored task produce) or
orphaned, and prints each prompt's token estimate. The estimate is
recomputed live, so a value that drifted as files appeared is a note for the
next generation: every merge moves some estimates, and a task branch cannot
rewrite kit/tasks.json, so drift never fails Ci. With --check --strict (a
plan-change session, before it commits) a measurement over the task's limit
(scripts/validate_tasks.py context_limit) is a problem, and so is a stored
value more than CONTEXT_DRIFT_POINTS from the measurement (D-1036, D-1075).
Generating keeps a stored value that is still within those points and the
limit, so the steady growth of docs/architecture.md, which almost every task
reads, does not rewrite every prompt at each plan change (D-1075).

Usage: python3 scripts/gen_prompts.py [--root REPO_ROOT] [--check [--strict]]
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path
from string import Template
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

import pii_scan  # sibling scripts, imported after their folder is on the path
import trace_spec
import validate_tasks

TASKS_FILE = Path("kit/tasks.json")
TEMPLATE_FILE = Path("kit/prompts/TEMPLATE.md")
PROMPTS_DIR = Path("kit/prompts")
TEST_DESIGN = Path("docs/test-design.md")
DECISIONS = Path("docs/decisions.md")
NONE_NOTE = "none for this task"
INTRO = "_intro"  # the section 5 paragraph before the journeys: setup and the closing checks
WINDOW_TOKENS = 200_000
BYTES_PER_TOKEN = 4
CONTEXT_DRIFT_POINTS = 3  # how far a stored context_pct may sit from its measurement (D-1075)
DECISION_QUESTION_CHARS = 200
DECISION_CHOICE_CHARS = 600
SUPERSEDED_FILE = "kit/superseded_decisions.json"
ARCHITECTURE = "docs/architecture.md"
ARCHITECTURE_LAYOUT = "## 2. "  # the section every prompt's start routine names (crate layout, D-1350)
PASTED_READS = ("docs/spec/", "docs/test-design.md", "docs/decisions.md", "docs/brains/")
BRAINS_DIR = "docs/brains/"
BRAINS_NOTE = "none (this task lists no brain under its reads)."
REQUIRED_KEYS = (
    "id",
    "slug",
    "title",
    "goal",
    "type",
    "phase",
    "stage",
    "model",
    "effort",
    "reviewer_model",
    "review_areas",
    "owns",
    "reads",
    "requirements",
    "tests",
    "decisions",
    "cites",
    "budget",
    "checks",
    "branch",
    "prompt",
)
BUDGET_KEYS = ("files", "lines", "context_pct")
TESTS_DONE = "The commands under \"Run your tests\" pass, and every test entry named above has, in a file you own, a test carrying its marker for the cases that fall to your requirements (an entry shared with another task is completed by that task's cases)."
NO_TESTS_DONE = "This task names no test entries: done means its acceptance line holds and the commands under \"Run your tests\" pass."
NO_TEST_FILES = "This task owns no test file. Run `bash scripts/smoke.sh` and whatever its acceptance line names."
COPY_PATH = re.compile(r"(^|/)(templates|static)/|^docs/(copy|legal)/")
COPY_STEPS = (
    "this task owns customer copy, so run `python3 scripts/check_ai_smell.py` and fix every hit by rewriting the line. "
    "Then run the cold read (WORD-028): a reviewer subagent that sees only the finished copy, not this prompt or your "
    "reasoning, reads it aloud as the person receiving it would and flags any line that sounds machine-written or unlike "
    "a local person explaining it to a neighbour (`docs/voice.md`, \"AI smell\"). Rewrite every flagged line. In the same "
    "step, run the copy panel (WORD-029): a reviewer subagent that sees only the finished copy, its reader and the page's "
    "job scores it with the persona and ten tests in `docs/copy/copy-panel.md`; apply every rewrite or say why not in your "
    "report, and run `python3 scripts/check_copy.py`. When a word or pattern would have caught it, log it in "
    "`docs/decisions.md` as \"AI-smell list: add <word or pattern>\" (T161 adds logged items to `docs/ai_smell.yaml`)."
)
COPY_SKIP = (
    "not part of this task, which owns no templates, static files, message files, `docs/copy` or `docs/legal`. "
    "If you find yourself writing words a homeowner or business reads, stop and report."
)
COPY_DONE = (
    "`scripts/check_ai_smell.py` and `scripts/check_copy.py` report 0 problems, and the cold read and the copy panel "
    "find nothing open in any changed customer copy."
)
COPY_DONE_SKIP = "none changed (this task owns no customer copy)."


@dataclass
class Sources:
    """What prompts paste: requirement sentences, test rows, journeys, decisions and source lines."""

    root: Path
    requirements: dict[str, tuple[str, str]] = field(default_factory=dict)
    tests: dict[str, str] = field(default_factory=dict)
    journeys: dict[str, str] = field(default_factory=dict)
    decisions: dict[str, str] = field(default_factory=dict)
    superseded: dict[str, str] = field(default_factory=dict)
    lines: dict[str, list[str] | None] = field(default_factory=dict)


def load_sources(root: Path) -> Sources:
    """Read the spec, the test design and the decisions log once."""
    src = Sources(root)
    if (root / trace_spec.SPEC_DIR).is_dir():
        for req in trace_spec.run(root).requirements:
            src.requirements[req.req_id] = (req.text, req.where)
    design = root / TEST_DESIGN
    if design.is_file():
        text = design.read_text(encoding="utf-8")
        for line in text.splitlines():
            match = re.match(r"^\| (TD-[A-Z0-9]+-\d{2}) \|", line)
            if match:
                src.tests[match.group(1)] = line
        src.journeys = parse_journeys(text)
    log = root / DECISIONS
    if log.is_file():
        for line in log.read_text(encoding="utf-8").splitlines():
            match = re.match(r"^- \*\*(D-\d{3,})\*\* (.*)$", line)
            if match:
                src.decisions[match.group(1)] = match.group(2)
    extra_path = root / SUPERSEDED_FILE
    extra = json.loads(extra_path.read_text(encoding="utf-8")).get("superseded", {}) if extra_path.is_file() else {}
    src.superseded = superseded_map(src.decisions, extra)
    return src


def parse_journeys(text: str) -> dict[str, str]:
    """Each journey's paragraphs from docs/test-design.md section 5, keyed J1 to J4."""
    journeys: dict[str, list[str]] = {}
    current: str | None = None
    in_section = False
    for line in text.splitlines():
        if line.startswith("## "):
            in_section, current = line.startswith("## 5."), None
            continue
        match = re.match(r"^\*\*(J\d) ", line)
        if in_section and match:
            current = match.group(1)
        if in_section:
            journeys.setdefault(current or INTRO, []).append(line)
    intro = "\n".join(journeys.pop(INTRO, [])).strip()
    return {k: (intro + "\n\n" if intro else "") + "\n".join(v).strip() for k, v in journeys.items()}


def bullets(items: list[str], empty: str = NONE_NOTE) -> str:
    """Render a list as markdown bullets, or a plain note when empty."""
    if not items:
        return empty
    return "\n".join(f"- `{item}`" for item in items)


def require_keys(task: dict[str, Any]) -> None:
    """Raise a clear error naming the task if a required key is missing or the goal is the title."""
    name = task.get("id", "<no id>")
    missing = [k for k in REQUIRED_KEYS if k not in task]
    budget = task.get("budget")
    if isinstance(budget, dict):
        missing += [f"budget.{k}" for k in BUDGET_KEYS if k not in budget]
    if missing:
        raise ValueError(f"task {name}: missing required key(s): {', '.join(missing)}")
    if str(task["goal"]).strip() == str(task["title"]).strip():
        raise ValueError(f"task {name}: goal repeats the title; write what done looks like (D-485)")


def paste_requirements(src: Sources, ids: list[str]) -> str:
    if not ids:
        return NONE_NOTE
    out = []
    for rid in ids:
        text, where = src.requirements.get(rid, ("(not found in docs/spec)", "?"))
        out.append(f"- **{rid}** {text} (`{where}`)")
    return "\n".join(out)


def paste_tests(src: Sources, ids: list[str]) -> str:
    if not ids:
        return NONE_NOTE
    out = []
    for tid in ids:
        if tid in src.journeys:
            out.append(f"- **{tid}** (docs/test-design.md section 5, with its closing checks):\n\n" + indent(src.journeys[tid]))
            continue
        row = src.tests.get(tid)
        if row is None:
            out.append(f"- **{tid}** (not found in docs/test-design.md)")
            continue
        cells = [c.strip() for c in row.strip().strip("|").split("|")]
        kind, layer, covers, cases = (cells + ["", "", "", ""])[1:5]
        out.append(f"- **{tid}** (kind {kind}; layers {layer}; covers {covers}): {cases}")
    return "\n".join(out)


def indent(text: str) -> str:
    return "\n".join(f"  {line}" if line else "" for line in text.splitlines())


SUPERSEDED_PATTERNS = (
    re.compile(r"\b(D-\d{3,}) (?:is |was |now )?(?:withdrawn|superseded|replaced|reversed)(?: by (D-\d{3,}))?"),
    re.compile(r"\b(?:withdraws|supersedes|replaces|reverses) (D-\d{3,})"),
)


def superseded_map(decisions: dict[str, str], extra: dict[str, str] | None = None) -> dict[str, str]:
    """old decision -> the decision that withdrew, superseded, replaced or reversed it.

    Read from the decisions' own words (the two sentence shapes above) and from kit/superseded_decisions.json,
    which lists the pairs reviewers found written in other words (D-491). A later entry wins.
    """
    found: dict[str, str] = {}
    for new_id, text in decisions.items():
        for pattern in SUPERSEDED_PATTERNS:
            for match in pattern.finditer(text):
                old = match.group(1)
                by = match.group(2) if match.lastindex and match.lastindex >= 2 and match.group(2) else new_id
                if old != by:
                    found[old] = by
    found.update(extra or {})
    return found


def paste_decisions(src: Sources, ids: list[str]) -> str:
    """Each decision's excerpt; a superseded one is marked and its successor is pasted too (D-491)."""
    if not ids:
        return NONE_NOTE
    out = []
    shown = list(ids)
    for did in ids:
        by = src.superseded.get(did)
        if by and by not in shown:
            shown.append(by)
    for did in shown:
        text = src.decisions.get(did, "(not found in docs/decisions.md)")
        by = src.superseded.get(did)
        mark = f" **(superseded by {by}; history only, build {by})**" if by else ""
        out.append(f"- **{did}**{mark} {decision_excerpt(text)}")
    return "\n".join(out)


def _cut(text: str, limit: int) -> str:
    return text if len(text) <= limit else text[:limit].rstrip() + " …"


def decision_excerpt(text: str) -> str:
    """The question's opening and the choice itself: a worker needs what was decided, not only why it was asked."""
    marker = "Choice:"
    if marker not in text:
        return _cut(text, DECISION_QUESTION_CHARS + DECISION_CHOICE_CHARS)
    question, choice = text.split(marker, 1)
    return f"{_cut(question.strip(), DECISION_QUESTION_CHARS)} {marker}{_cut(choice, DECISION_CHOICE_CHARS)}"


def cited_line(src: Sources, cite: str) -> str:
    path, _, number = cite.rpartition(":")
    if path not in src.lines:
        file = src.root / path
        src.lines[path] = file.read_text(encoding="utf-8").splitlines() if file.is_file() else None
    lines = src.lines[path]
    if lines is None or not number.isdigit() or not 0 < int(number) <= len(lines):
        return "(line not found)"
    return lines[int(number) - 1].strip()


def paste_cites(src: Sources, cites: list[str]) -> str:
    if not cites:
        return NONE_NOTE
    return "\n".join(f"- `{c}`: {cited_line(src, c)}" for c in cites)


def brain_sections(text: str) -> list[tuple[str, list[str]]]:
    """A brain's `## ` sections as (heading, lines); text before the first heading is dropped."""
    sections: list[tuple[str, list[str]]] = []
    for line in text.splitlines():
        if line.startswith("## "):
            sections.append((line[3:].strip(), []))
        elif sections:
            sections[-1][1].append(line)
    return sections


def brain_excerpt(text: str, req_ids: list[str]) -> str:
    """The lines of a brain that name one of the task's requirements, plus its review checklist, in full.

    The rest of the brain stays a pointer (the README says to read the sections for your task), so a
    prompt carries the specialist's rules that bind this task without the whole file's bytes.
    """
    pattern = re.compile(r"\b(" + "|".join(re.escape(r) for r in req_ids) + r")\b") if req_ids else None
    out: list[str] = []
    for heading, lines in brain_sections(text):
        lower = heading.lower()
        if "checklist" in lower and "casey" not in lower:  # the review checklist, not a hands-on list for Casey
            out.append(f"### {heading}")
            out.extend(line for line in lines if line.strip())
        elif pattern and "sources" not in lower:
            hits = [line for line in lines if pattern.search(line)]
            if hits:
                out.append(f"### {heading} (lines naming this task's requirements)")
                out.extend(hits)
    return "\n".join(out)


def paste_brains(root: Path, task: dict[str, Any]) -> str:
    """Each brain under the task's reads, excerpted by brain_excerpt."""
    brains = [r for r in task["reads"] if r.startswith(BRAINS_DIR) and not r.endswith("README.md")]
    if not brains:
        return BRAINS_NOTE
    parts = []
    for path in brains:
        target = root / path
        if not target.is_file():
            raise SystemExit(f"{task['id']}: brain {path} does not exist")
        excerpt = brain_excerpt(target.read_text(encoding="utf-8"), task["requirements"])
        if not excerpt:
            raise SystemExit(f"{task['id']}: brain {path} has no checklist and no line naming this task's requirements")
        parts.append(f"From `{path}` (cite its tags in the PR; read other sections only when a design choice needs them):\n\n{excerpt}")
    return "\n\n".join(parts) or BRAINS_NOTE


def test_command(path: str) -> str | None:
    """The command that runs one owned test file, or None for a file run through another."""
    crate = re.match(r"^app/(?P<crate>[a-z_]+)/src/tests/(?P<name>[a-z0-9_]+)\.rs$", path)
    if crate and crate["name"] != "mod":
        return f"cargo test -p askhava-{crate['crate']} tests::{crate['name']}"
    crate_test = re.match(r"^app/(?P<crate>[a-z_]+)/tests/(?P<name>[a-z0-9_]+)\.rs$", path)
    if crate_test:  # a crate's own integration test, such as the rules_bridge contract test (D-491)
        return f"cargo test -p askhava-{crate_test['crate']} --test {crate_test['name']}"
    layer = re.match(r"^tests/(?P<layer>integration|security|recovery|fixtures)/tests/(?P<name>[a-z0-9_]+)\.rs$", path)
    if layer:
        package = "askhava-recovery-tests" if layer["layer"] == "recovery" else f"askhava-{layer['layer']}"
        return f"cargo test -p {package} --test {layer['name']}"
    if re.match(r"^tests/a11y/.+\.spec\.ts$", path):
        return f"npx playwright test --config tests/a11y/playwright.config.ts {path}"
    if re.match(r"^tests/.+\.spec\.ts$", path):
        return f"npx playwright test --config tests/e2e/playwright.config.ts {path}"
    if re.match(r"^tests/.+\.py$", path) and Path(path).name.startswith("test_"):
        return f"python3 -m pytest {path} -q -p no:cacheprovider"
    return None


def run_tests_block(task: dict[str, Any]) -> str:
    commands = [c for c in (test_command(p) for p in task["owns"]) if c]
    if not commands:
        return NO_TEST_FILES
    lines = "\n".join(dict.fromkeys(commands))
    return f"Run these from the repository root (each must pass before hand-in):\n\n```bash\n{lines}\n```"


def owns_copy(task: dict[str, Any]) -> bool:
    """A task writes customer words when it owns a copy path or is flagged `copy` (it adds template keys, D-491)."""
    return bool(task.get("copy")) or any(COPY_PATH.search(p) for p in task["owns"])


def render(template: Template, task: dict[str, Any], src: Sources | None = None) -> str:
    """Fill the template for one task."""
    require_keys(task)
    src = src or Sources(Path("."))
    budget = task["budget"]
    copy = owns_copy(task)
    acceptance = str(task.get("acceptance", "")).strip()
    values = {
        "id": task["id"],
        "title": task["title"],
        "type": task["type"],
        "phase": task["phase"],
        "stage": task["stage"],
        "slug": task["slug"],
        "branch": task["branch"],
        "model": task["model"],
        "effort": task["effort"],
        "reviewer_model": task["reviewer_model"],
        "review_areas": ", ".join(task["review_areas"]) or "none",
        "goal": task["goal"],
        "acceptance_section": f"Acceptance: {acceptance}\n\n" if acceptance else "",
        "tests_done": TESTS_DONE if task["tests"] else NO_TESTS_DONE,
        "owns": bullets(task["owns"], "none (no files are owned; report only)"),
        "reads": bullets(task["reads"]),
        "requirements": paste_requirements(src, task["requirements"]),
        "tests": paste_tests(src, task["tests"]),
        "decisions": paste_decisions(src, task["decisions"]),
        "cites": paste_cites(src, task["cites"]),
        "brains": paste_brains(src.root, task),
        "run_tests": run_tests_block(task),
        "copy_step": COPY_STEPS if copy else COPY_SKIP,
        "copy_done": COPY_DONE if copy else COPY_DONE_SKIP,
        "budget_files": budget["files"],
        "budget_lines": budget["lines"],
        "budget_context": budget["context_pct"],
        "checks": ", ".join(f"`{c}`" for c in task["checks"]),
    }
    return template.substitute(values)


def path_bytes(root: Path, path: str) -> int:
    """Bytes of a file, or of every file under a directory; 0 when it does not exist yet."""
    target = root / path
    if target.is_file():
        return target.stat().st_size
    if target.is_dir():
        return sum(p.stat().st_size for p in target.rglob("*") if p.is_file() and "__pycache__" not in p.parts)
    return 0


def context_bytes(root: Path, task: dict[str, Any], prompt: str) -> int:
    """Prompt + owned regular files that exist + reads that exist (pasted documents count through the prompt).

    An owned directory (a cleanup or harness task's tree) counts nothing: the worker greps and sweeps it, it
    does not load it. A directory under reads still counts in full, since the worker reads it, but a file the
    task owns is counted once, never again through a read that names it or a directory read holding it
    (D-1036).
    """
    owned = [o for o in task["owns"] if (root / o).is_file()]
    owned_set = set(owned)
    reads = [r for r in task["reads"] if not r.startswith(PASTED_READS) and r not in owned_set]
    total = len(prompt.encode("utf-8")) + sum(path_bytes(root, p) for p in dict.fromkeys(owned))
    sized = (architecture_bytes(root, task.get("module")) if r == ARCHITECTURE else read_bytes(root, r, owned_set) for r in dict.fromkeys(reads))
    return total + sum(sized)


def architecture_bytes(root: Path, module: str | None) -> int:
    """What a worker loads of docs/architecture.md: section 2 whole, and elsewhere only the lines naming its
    module in backticks or as `app/<module>/` (the module table row, its adapters and runbook lines); 0
    when the file does not exist (D-1350)."""
    target = root / ARCHITECTURE
    if not target.is_file():
        return 0
    marks = (f"`{module}`", f"app/{module}/") if module else ()
    total, in_layout = 0, False
    for line in target.read_text(encoding="utf-8").splitlines(keepends=True):
        if line.startswith("## "):
            in_layout = line.startswith(ARCHITECTURE_LAYOUT)
        if in_layout or any(mark in line for mark in marks):
            total += len(line.encode("utf-8"))
    return total


def read_bytes(root: Path, path: str, owned: set[str]) -> int:
    """Bytes of one read, leaving out the owned files a directory read holds (they already counted)."""
    target = root / path
    if not target.is_dir():
        return path_bytes(root, path)
    return sum(
        p.stat().st_size
        for p in target.rglob("*")
        if p.is_file() and "__pycache__" not in p.parts and p.relative_to(root).as_posix() not in owned
    )


def percent(byte_count: int) -> int:
    return math.ceil(byte_count / BYTES_PER_TOKEN * 100 / WINDOW_TOKENS)


def settled_pct(task: dict[str, Any], measured: int) -> int:
    """The context_pct to store: the stored one while it and the measurement are within the task's limit and
    at most CONTEXT_DRIFT_POINTS apart, else the measurement (D-1075)."""
    stored = task["budget"]["context_pct"]
    if not isinstance(stored, int) or abs(stored - measured) > CONTEXT_DRIFT_POINTS:
        return measured
    return stored if max(stored, measured) <= validate_tasks.context_limit(task) else measured


def derive_context(root: Path, template: Template, task: dict[str, Any], src: Sources) -> str:
    """Settle budget.context_pct (settled_pct) until the prompt that states it agrees; return the prompt."""
    prompt = render(template, task, src)
    for _ in range(5):
        pct = settled_pct(task, percent(context_bytes(root, task, prompt)))
        if pct == task["budget"]["context_pct"]:
            return prompt
        task["budget"]["context_pct"] = pct
        prompt = render(template, task, src)
    return prompt


def prompt_tasks(data: dict[str, Any]) -> list[dict[str, Any]]:
    """Tasks that have a prompt file (gates do not)."""
    return [t for t in data["tasks"] if t.get("prompt")]


def check_task(root: Path, template: Template, task: dict[str, Any], src: Sources) -> list[str]:
    """Return the problems with one task's prompt file."""
    path = root / task["prompt"]
    if not path.is_file():
        return [f"{task['id']}: missing {task['prompt']}"]
    text = path.read_text(encoding="utf-8")
    if text != render(template, task, src):
        return [f"{task['id']}: stale, differs from the template output"]
    return []


def estimate_line(root: Path, task: dict[str, Any], prompt: str) -> str:
    byte_count = context_bytes(root, task, prompt)
    pct = percent(byte_count)
    note = "" if settled_pct(task, pct) == task["budget"]["context_pct"] else f" (stored {task['budget']['context_pct']}%; regenerate to refresh)"
    return f"{task['id']}: about {byte_count // BYTES_PER_TOKEN} tokens, {pct}% of the window{note}"


def budget_problem(root: Path, task: dict[str, Any], prompt: str) -> list[str]:
    """--strict (D-1036, D-1075): the measurement is within the task's limit and the stored value has settled."""
    pct = percent(context_bytes(root, task, prompt))
    stored = task["budget"]["context_pct"]
    limit = validate_tasks.context_limit(task)
    if pct > limit:
        return [f"{task['id']}: measures {pct}% of the window, over its {limit}% limit; trim its reads or split it"]
    if settled_pct(task, pct) != stored:
        return [f"{task['id']}: stored context_pct {stored}% but it measures {pct}%; regenerate"]
    return []


def check(
    root: Path, data: dict[str, Any], template: Template, src: Sources | None = None, strict: bool = False
) -> list[str]:
    """Return every problem across all prompts; print each prompt's token estimate."""
    src = src or load_sources(root)
    problems: list[str] = []
    tasks = prompt_tasks(data)
    for task in tasks:
        problems.extend(check_task(root, template, task, src))
        prompt = render(template, task, src)
        print(estimate_line(root, task, prompt))
        if strict:
            problems.extend(budget_problem(root, task, prompt))
    if strict:
        for warning in pii_scan.size_warnings(root):
            print(f"WARNING {warning}")
        problems.extend(pii_scan.size_stops(root))
    expected = {Path(task["prompt"]).name for task in tasks}
    for path in sorted((root / PROMPTS_DIR).glob("T[0-9][0-9][0-9].md")):
        if path.name not in expected:
            problems.append(f"orphan prompt with no task: {path.name}")
    return problems


def generate(root: Path, data: dict[str, Any], template: Template, src: Sources | None = None) -> int:
    """Write every prompt, deriving budget.context_pct; return how many were written."""
    src = src or load_sources(root)
    tasks = prompt_tasks(data)
    for task in tasks:
        path = root / task["prompt"]
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(derive_context(root, template, task, src), encoding="utf-8")
    return len(tasks)


def write_tasks(root: Path, data: dict[str, Any]) -> None:
    """Write kit/tasks.json back in its committed format (one-space indent, UTF-8)."""
    text = json.dumps(data, indent=1, ensure_ascii=False) + "\n"
    (root / TASKS_FILE).write_text(text, encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parent.parent
    )
    parser.add_argument("--check", action="store_true", help="verify, write nothing")
    parser.add_argument("--strict", action="store_true", help="with --check: a stale stored budget fails (D-1036)")
    args = parser.parse_args(argv)
    root: Path = args.root
    data = json.loads((root / TASKS_FILE).read_text(encoding="utf-8"))
    template = Template((root / TEMPLATE_FILE).read_text(encoding="utf-8"))
    try:
        return run(root, data, template, args.check, args.strict)
    except ValueError as error:
        print(f"ERROR {error}")
        return 1


def run(root: Path, data: dict[str, Any], template: Template, check_only: bool, strict: bool = False) -> int:
    """Check or generate; ValueError means a malformed task."""
    if check_only:
        problems = check(root, data, template, strict=strict)
        for problem in problems:
            print(f"ERROR {problem}")
        count = len(prompt_tasks(data))
        print(f"checked {count} prompts, {len(problems)} problems")
        return 1 if problems else 0
    written = generate(root, data, template)
    write_tasks(root, data)
    print(f"wrote {written} prompts and their context budgets")
    return 0


if __name__ == "__main__":
    sys.exit(main())
