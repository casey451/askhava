"""Validator for kit/tasks.json, the build task graph (kit step K6).

Checks that every task is well formed; dependencies exist and form no cycle;
every requirement in docs/spec/ (read with scripts/trace_spec.py), every
test-design entry (TD-...) and every end-to-end journey is covered by a task;
owned paths stay out of protected paths unless the task is a harness task;
no two tasks that can run in either order own the same path; each task fits
its size budget; models are valid full IDs; money, privacy, security and
recovery tasks (and every harness task) have an Opus reviewer; reads, decisions
and citations resolve; and the owner gates sit where BUILD_BRIEF.md section 6
puts them. Prints counts; exits 1 on any error.

Usage: python3 scripts/validate_tasks.py [--root REPO_ROOT] [--tasks PATH]
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

import trace_spec  # sibling script, imported after its folder is on the path

TASKS_FILE = Path("kit/tasks.json")
TEST_DESIGN = Path("docs/test-design.md")
TASK_ID = re.compile(r"^T\d{3}$")
SLUG = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
TD_ROW = re.compile(r"^\| (?P<id>TD-[A-Z0-9]+-\d{2}) \|")
DECISION_ID = re.compile(r"^D-\d{3}$")
CITE = re.compile(r"^(?P<path>[^\s:]+):(?P<line>\d+)$")

MODELS = frozenset({"claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5"})
OPUS = "claude-opus-5-5"
HAIKU = "claude-haiku-5-5"
REVIEWERS = frozenset({"claude-sonnet-5-5", OPUS})
EFFORTS = frozenset({"low", "medium", "high"})
TYPES = frozenset({"build", "harness", "docs", "cleanup", "audit", "gate"})
UNCOVERED_TYPES = frozenset({"cleanup", "gate"})
AREAS = frozenset(
    {
        "money",
        "privacy",
        "security",
        "recovery",
        "licensing",
        "compliance",
        "accessibility",
        "docs",
        "harness",
    }
)
OPUS_REVIEW_AREAS = frozenset({"money", "privacy", "security", "recovery"})
STAGES = ("1", "2", "3", "4", "5", "checkpoint", "0.5", "6", "7")
CHECKS = ("Ci", "Twin", "Ally")

# Fixed here, not read from the file, so the task file can't loosen them.
PROTECTED = (
    ".github/",
    ".claude/",
    "runner/",
    "stand_ins/",
    "sources/",
    "twin/",
    "tests/twin/",
    "tests/mutation/",
    "tests/test_trace_spec.py",
    "tests/test_check_catalog.py",
    "scripts/validate_tasks.py",
    "scripts/trace_spec.py",
    "tests/test_validate_tasks.py",
    "kit/layers.json",  # the machine-readable layer table (D-479)
    "kit/allowed_crates.txt",  # the approved-crate list (D-479)
)
# Paths K7 to K9 create; a task may read them before any task in this file makes them.
KIT_OUTPUTS = (
    "kit/prompts/",
    "runner/",
    "stand_ins/",
    "twin/",
    "tests/twin/",
    "tests/mutation/",
    "tests/contract/",
    "tests/coverage_floor.json",
    "Cargo.lock",
)
# Owned paths that make a task money, privacy, security or recovery work, whatever it declares.
SENSITIVE_PREFIXES = {
    "app/payments/": "money",
    "app/privacy/": "privacy",
    "app/picks/": "privacy",
    "app/portals/": "security",
    "app/core/": "security",
    "app/adapters/": "security",
    "app/recovery/": "recovery",
}
FORBIDDEN = ("sources/",)
# Crate manifests are shared: a module or test crate gains a path dependency when a later task needs it (D-453).
CRATE_MODULES = (
    "core",
    "ops",
    "adapters",
    "catalog",
    "privacy",
    "ai",
    "messaging",
    "payments",
    "licensing",
    "businesses",
    "jobs",
    "calls",
    "quotes",
    "picks",
    "intake",
    "recovery",
    "portals",
    "site",
    "server",
)
TEST_CRATES = ("integration", "security", "recovery", "fixtures")  # unit and property tests are in-crate (D-481)
ALLOWED_SHARED = frozenset(
    {
        "docs/architecture.md",
        "docs/decisions.md",
        "docs/for-casey-review.md",
        "app/server/src/main.rs",
        "Cargo.toml",
        "Cargo.lock",
        ".sqlx/",
        "package.json",
        "package-lock.json",
        "kit/plan.lock",  # the blueprint fingerprint, rewritten by any PR that changes a locked file (D-479)
    }
    | {f"app/{m}/Cargo.toml" for m in CRATE_MODULES}
    | {f"app/{m}/CLAUDE.md" for m in CRATE_MODULES}  # crate docs any task updates (D-473)
    | {f"app/{m}/src/tests/mod.rs" for m in CRATE_MODULES}  # in-crate test module list, one `mod` line per task (D-481)
    | {f"tests/{layer}/Cargo.toml" for layer in TEST_CRATES}
)
JOURNEYS = ("J1", "J2", "J3", "J4")
GATE_KINDS = (
    "account-checkpoint",
    "stage-0.5-go",
    "real-texts-casey-phone",
    "finished",
    "gate-b",
    "gate-c",
    "first-live-charge",
)
# Whether each gate waits for Casey (BUILD_BRIEF.md:546-553).
# Only the real-texts gate is conditional.
GATE_WAITS = {
    "account-checkpoint": True,
    "stage-0.5-go": True,
    "real-texts-casey-phone": True,
    "finished": False,
    "gate-b": True,
    "gate-c": True,
    "first-live-charge": True,
}
CONDITIONAL_GATE = "real-texts-casey-phone"
NO_DEPENDENTS = ("stage-0.5-go", "first-live-charge")
TWILIO_SANDBOX_PATH = "app/adapters/src/twilio_sms.rs"
MAX_OWNS = 14
MAX_FILES = 14
MAX_LINES = 800
MAX_CONTEXT_PCT = 20
MAX_REQUIREMENTS = 25
MAX_TESTS = 6

REQUIRED_KEYS: dict[str, type | tuple[type, ...]] = {
    "id": str,
    "slug": str,
    "title": str,
    "goal": str,
    "type": str,
    "phase": int,
    "stage": str,
    "module": (str, type(None)),
    "deps": list,
    "model": (str, type(None)),
    "effort": (str, type(None)),
    "reviewer_model": (str, type(None)),
    "review_areas": list,
    "owns": list,
    "reads": list,
    "requirements": list,
    "tests": list,
    "decisions": list,
    "cites": list,
    "budget": dict,
    "checks": list,
    "branch": (str, type(None)),
    "prompt": (str, type(None)),
}

Task = dict[str, Any]


@dataclass
class Graph:
    tasks: list[Task]
    by_id: dict[str, Task] = field(default_factory=dict)
    ancestors: dict[str, set[str]] = field(default_factory=dict)


def where(task: Task) -> str:
    return f"{task.get('id', '?')} ({task.get('slug', '?')})"


def check_shape(task: Task) -> list[str]:
    """Every key present with the right JSON type."""
    errors = [
        f"{where(task)}: '{key}' missing or not {kind}"
        for key, kind in REQUIRED_KEYS.items()
        if not isinstance(task.get(key), kind) or isinstance(task.get(key), bool)
    ]
    for key in (
        "deps",
        "review_areas",
        "owns",
        "reads",
        "requirements",
        "tests",
        "decisions",
        "cites",
        "checks",
    ):
        if isinstance(task.get(key), list) and not all(isinstance(v, str) for v in task[key]):
            errors.append(f"{where(task)}: '{key}' must list strings")
    return errors


def check_identity(task: Task) -> list[str]:
    """ID, slug, type, stage, branch and prompt follow the conventions."""
    tid, slug = task["id"], task["slug"]
    errors: list[str] = []
    if not TASK_ID.match(tid):
        errors.append(f"{where(task)}: id must look like T001")
    if not SLUG.match(slug):
        errors.append(f"{where(task)}: slug must be lower-case words joined by hyphens")
    if task["type"] not in TYPES:
        errors.append(f"{where(task)}: unknown type {task['type']!r}")
    if task["stage"] not in STAGES:
        errors.append(f"{where(task)}: unknown stage {task['stage']!r}")
    is_gate = task["type"] == "gate"
    branch = None if is_gate else f"claude/task/{tid}-{slug}"
    prompt = None if is_gate else f"kit/prompts/{tid}.md"
    if task["branch"] != branch:
        errors.append(f"{where(task)}: branch must be {branch!r}")
    if task["prompt"] != prompt:
        errors.append(f"{where(task)}: prompt must be {prompt!r}")
    return errors


def derived_areas(task: Task) -> set[str]:
    """Sensitive areas implied by the owned paths (a directory implies all it contains)."""
    return {
        area
        for path in task["owns"]
        for prefix, area in SENSITIVE_PREFIXES.items()
        if overlaps(path, prefix)
    }


def check_areas(task: Task) -> list[str]:
    """Review areas are known and include every area the owned paths imply."""
    errors: list[str] = []
    derived = derived_areas(task)
    unknown = sorted((set(task["review_areas"]) | derived) - AREAS)
    if unknown:
        errors.append(f"{where(task)}: unknown review areas {unknown}")
    missing = sorted(derived - set(task["review_areas"]))
    if missing:
        errors.append(f"{where(task)}: review_areas must include {missing} (from owned paths)")
    return errors


def check_models(task: Task) -> list[str]:
    """Full model IDs; Opus for money, privacy, security and recovery work; effort rules."""
    errors: list[str] = []
    areas = set(task["review_areas"]) | derived_areas(task)
    if task["type"] == "gate":
        if task["model"] or task["effort"] or task["reviewer_model"]:
            errors.append(f"{where(task)}: a gate runs no model")
        return errors
    if task["model"] not in MODELS:
        errors.append(f"{where(task)}: model {task['model']!r} is not a full model ID")
    if task["reviewer_model"] not in REVIEWERS:
        errors.append(
            f"{where(task)}: reviewer_model {task['reviewer_model']!r} is not Sonnet or Opus"
        )
    if task["effort"] not in EFFORTS:
        errors.append(f"{where(task)}: effort {task['effort']!r} is not low, medium or high")
    needs_opus = areas & OPUS_REVIEW_AREAS or task["type"] == "harness"
    if needs_opus and task["reviewer_model"] != OPUS:
        errors.append(
            f"{where(task)}: money, privacy, security, recovery and harness tasks"
            " need an Opus reviewer"
        )
    # The Opus *model* is required for money, privacy, security and recovery application code
    # (owned paths under the sensitive prefixes); docs, harness, audit and test-only work in those
    # areas needs the Opus *reviewer* above, and Sonnet may write it (D-483 narrows BUILD_BRIEF.md:498).
    owns_app_code = any(o.startswith(("app/", "migrations/")) for o in task["owns"])
    writes_sensitive_code = task["type"] == "build" and owns_app_code and bool(
        derived_areas(task) or areas & {"money", "privacy", "recovery"}
    )
    if writes_sensitive_code and task["model"] != OPUS:
        errors.append(
            f"{where(task)}: money, privacy, security and recovery application code"
            " runs on Opus (BUILD_BRIEF.md:498); test-only, docs and harness work in those areas may"
            " run on Sonnet with an Opus reviewer (D-483)"
        )
    if task["effort"] == "high" and task["model"] != OPUS:
        errors.append(f"{where(task)}: high effort is only for Opus tasks")
    if task["model"] == HAIKU and areas & OPUS_REVIEW_AREAS:
        errors.append(f"{where(task)}: Haiku may not do money, privacy, security or recovery work")
    return errors


def check_size(task: Task) -> list[str]:
    """The task fits the size budget: few files, few lines, a small slice of context."""
    errors: list[str] = []
    budget = task["budget"]
    files, lines, ctx = budget.get("files"), budget.get("lines"), budget.get("context_pct")
    if not all(isinstance(v, int) and not isinstance(v, bool) for v in (files, lines, ctx)):
        return [f"{where(task)}: budget needs integer files, lines and context_pct"]
    assert isinstance(files, int) and isinstance(lines, int) and isinstance(ctx, int)
    limits = (
        (len(task["owns"]), MAX_OWNS, "owned paths"),
        (files, MAX_FILES, "budget files"),
        (lines, MAX_LINES, "budget lines"),
        (ctx, MAX_CONTEXT_PCT, "budget context_pct"),
        (len(task["requirements"]), MAX_REQUIREMENTS, "requirements"),
        (len(task["tests"]), MAX_TESTS, "tests"),
    )
    errors += [
        f"{where(task)}: {name} {value} over the limit of {limit}"
        for value, limit, name in limits
        if value > limit
    ]
    has_dir = any(p.endswith("/") for p in task["owns"])
    if task["type"] != "gate" and not has_dir and files != len(task["owns"]):
        errors.append(
            f"{where(task)}: budget files {files} does not match"
            f" the {len(task['owns'])} owned files"
        )
    if has_dir and task["type"] not in ("cleanup", "harness"):
        errors.append(f"{where(task)}: only cleanup and harness tasks may own a directory")
    if has_dir and task["reviewer_model"] != OPUS:
        errors.append(f"{where(task)}: a task owning a directory needs an Opus reviewer")
    return errors


def check_gate_fields(task: Task) -> list[str]:
    """Gates own nothing and carry their kind; work tasks pass all three required checks."""
    if task["type"] != "gate":
        missing = [c for c in CHECKS if c not in task["checks"]]
        extra = "gate" in task
        work = [f"{where(task)}: checks must include {missing}"] if missing else []
        return work + ([f"{where(task)}: only a gate task has 'gate'"] if extra else [])
    gate = task.get("gate")
    if not isinstance(gate, dict) or gate.get("kind") not in GATE_KINDS:
        return [f"{where(task)}: a gate needs 'gate.kind', one of {list(GATE_KINDS)}"]
    kind = gate["kind"]
    errors: list[str] = []
    if gate.get("waits_on_owner") is not GATE_WAITS[kind]:
        errors.append(f"{where(task)}: gate {kind} must have waits_on_owner {GATE_WAITS[kind]}")
    if gate.get("conditional", False) is not (kind == CONDITIONAL_GATE):
        errors.append(f"{where(task)}: only gate {CONDITIONAL_GATE} is conditional, and it must be")
    if task["owns"] or task["checks"]:
        errors.append(f"{where(task)}: a gate owns no paths and runs no checks")
    return errors


def check_coverage_fields(task: Task) -> list[str]:
    """Each task says why it exists: requirements, tests, decisions or citations."""
    if task["type"] in UNCOVERED_TYPES:
        return [] if task["goal"].strip() else [f"{where(task)}: goal is empty"]
    if task["type"] == "build" and not (task["requirements"] or task["tests"]):
        return [f"{where(task)}: a build task must cover at least one requirement or test entry"]
    if not (task["requirements"] or task["tests"] or task["decisions"] or task["cites"]):
        return [f"{where(task)}: names no requirement, test, decision or citation"]
    return []


def check_task(task: Task) -> list[str]:
    """All per-task checks; the shape check runs first because the rest rely on it."""
    errors = check_shape(task)
    if errors:
        return errors
    for check in (
        check_identity,
        check_areas,
        check_models,
        check_size,
        check_gate_fields,
        check_coverage_fields,
    ):
        errors += check(task)
    return errors


def build_graph(tasks: list[Task]) -> tuple[Graph, list[str]]:
    """Index tasks; check IDs are unique, deps exist and the graph has no cycle."""
    graph = Graph(tasks)
    errors: list[str] = []
    for task in tasks:
        if task["id"] in graph.by_id:
            errors.append(f"{where(task)}: duplicate id")
        graph.by_id[task["id"]] = task
    slugs = Counter(t["slug"] for t in tasks)
    errors += [f"duplicate slug {s!r}" for s, n in slugs.items() if n > 1]
    for task in tasks:
        for dep in task["deps"]:
            if dep == task["id"]:
                errors.append(f"{where(task)}: depends on itself")
            elif dep not in graph.by_id:
                errors.append(f"{where(task)}: depends on unknown task {dep}")
    if errors:
        return graph, errors
    order, cycle = topo_order(graph)
    if cycle:
        return graph, [f"dependency cycle among {sorted(cycle)}"]
    for tid in order:
        deps = graph.by_id[tid]["deps"]
        graph.ancestors[tid] = set(deps).union(*(graph.ancestors[d] for d in deps))
    return graph, []


def topo_order(graph: Graph) -> tuple[list[str], set[str]]:
    """Kahn's algorithm: an order, or the tasks left on a cycle."""
    indegree = {tid: len(task["deps"]) for tid, task in graph.by_id.items()}
    children: dict[str, list[str]] = {tid: [] for tid in graph.by_id}
    for tid, task in graph.by_id.items():
        for dep in task["deps"]:
            children[dep].append(tid)
    ready = sorted(tid for tid, n in indegree.items() if n == 0)
    order: list[str] = []
    while ready:
        tid = ready.pop(0)
        order.append(tid)
        for child in children[tid]:
            indegree[child] -= 1
            if indegree[child] == 0:
                ready.append(child)
    left = {tid for tid, n in indegree.items() if n > 0}
    return order, left


def overlaps(a: str, b: str) -> bool:
    """Same path, or one is a directory (ending in '/') that contains the other."""
    if a == b:
        return True
    return (a.endswith("/") and b.startswith(a)) or (b.endswith("/") and a.startswith(b))


def check_path_rules(task: Task) -> list[str]:
    """Owned paths are relative, never under sources/, and protected only for harness tasks."""
    errors: list[str] = []
    for path in task["owns"]:
        if path.startswith("/") or ".." in Path(path).parts or not path:
            errors.append(f"{where(task)}: owned path {path!r} must be relative to the repository")
        elif any(path.startswith(f) for f in FORBIDDEN):
            errors.append(f"{where(task)}: owns {path}, but sources/ is read-only")
        elif any(overlaps(path, p) for p in PROTECTED) and task["type"] != "harness":
            errors.append(f"{where(task)}: owns protected path {path} but is not a harness task")
    return errors


def ordered(graph: Graph, a: str, b: str) -> bool:
    return a in graph.ancestors[b] or b in graph.ancestors[a]


def check_ownership(graph: Graph) -> list[str]:
    """No two unordered tasks own a path; only a harness task takes over an earlier task's test."""
    errors: list[str] = []
    owners = [(t["id"], p) for t in graph.tasks for p in t["owns"] if p not in ALLOWED_SHARED]
    for i, (tid_a, path_a) in enumerate(owners):
        for tid_b, path_b in owners[i + 1 :]:
            if tid_a == tid_b or not overlaps(path_a, path_b):
                continue
            if not ordered(graph, tid_a, tid_b):
                errors.append(
                    f"{tid_a} and {tid_b} can run in either order but both own {path_a} / {path_b}"
                )
            elif path_a.startswith("tests/") and path_b.startswith("tests/"):
                later = tid_b if tid_a in graph.ancestors[tid_b] else tid_a
                if graph.by_id[later]["type"] != "harness":
                    errors.append(
                        f"{later}: changes test {path_b} owned by an earlier task;"
                        " only a harness task may"
                    )
    return errors


def from_kit(path: str) -> bool:
    """A directory entry covers what it contains; a file entry only itself."""
    return any(
        (k.endswith("/") and (path.startswith(k) or path == k.rstrip("/"))) or path == k
        for k in KIT_OUTPUTS
    )


def read_known(root: Path, graph: Graph, task: Task, path: str) -> bool:
    """A read path exists, is made by the kit (K7 to K9), or is owned by an ancestor task."""
    if (root / path).exists() or from_kit(path):
        return True
    return any(
        overlaps(path, p) for a in graph.ancestors[task["id"]] for p in graph.by_id[a]["owns"]
    )


def check_reads(root: Path, graph: Graph) -> list[str]:
    return [
        f"{where(task)}: reads {path}, which doesn't exist and no earlier task makes"
        for task in graph.tasks
        for path in task["reads"]
        if not read_known(root, graph, task, path)
    ]


def td_entries(root: Path) -> list[str]:
    path = root / TEST_DESIGN
    if not path.is_file():
        return []
    lines = path.read_text(encoding="utf-8").splitlines()
    return [m.group("id") for line in lines if (m := TD_ROW.match(line))]


def check_coverage(
    graph: Graph, req_ids: set[str], td_ids: set[str], withdrawn: set[str] = frozenset()
) -> list[str]:
    """Every live requirement, TD entry and journey has a task; tasks name only known, live IDs."""
    errors: list[str] = []
    covered_reqs: set[str] = set()
    covered_tests: set[str] = set()
    for task in graph.tasks:
        covered_reqs.update(task["requirements"])
        covered_tests.update(task["tests"])
        errors += [
            f"{where(task)}: unknown requirement {r}"
            for r in task["requirements"]
            if r not in req_ids and r not in withdrawn
        ]
        errors += [
            f"{where(task)}: withdrawn requirement {r} (drop it from the task, D-479)"
            for r in task["requirements"]
            if r in withdrawn
        ]
        errors += [
            f"{where(task)}: unknown test entry {t}"
            for t in task["tests"]
            if t not in td_ids and t not in JOURNEYS
        ]
    errors += [f"requirement {r} is covered by no task" for r in sorted(req_ids - covered_reqs)]
    errors += [f"test entry {t} is covered by no task" for t in sorted(td_ids - covered_tests)]
    errors += [f"journey {j} is covered by no task" for j in JOURNEYS if j not in covered_tests]
    return errors


def check_references(root: Path, graph: Graph) -> list[str]:
    """Decisions exist in docs/decisions.md; citations point at a non-blank repository line."""
    known = trace_spec.known_decisions(root)
    cache: dict[str, list[str] | None] = {}
    errors: list[str] = []
    for task in graph.tasks:
        for ref in task["decisions"]:
            if not DECISION_ID.match(ref) or ref not in known:
                errors.append(f"{where(task)}: decision {ref} is not in docs/decisions.md")
        for cite in task["cites"]:
            match = CITE.match(cite)
            if match is None:
                errors.append(f"{where(task)}: citation {cite!r} is not path:LINE")
                continue
            problem = trace_spec.check_citation(
                root, trace_spec.Citation(match["path"], int(match["line"])), cache
            )
            if problem:
                errors.append(f"{where(task)}: citation {problem}")
    return errors


def gates_by_kind(graph: Graph) -> tuple[dict[str, str], list[str]]:
    kinds: dict[str, str] = {}
    errors: list[str] = []
    for task in graph.tasks:
        if task["type"] != "gate":
            continue
        kind = task["gate"]["kind"]
        if kind in kinds:
            errors.append(f"{where(task)}: second gate of kind {kind}")
        kinds[kind] = task["id"]
    errors += [f"no gate of kind {k}" for k in GATE_KINDS if k not in kinds]
    return kinds, errors


def check_gate_order(graph: Graph) -> list[str]:
    """Gates sit where BUILD_BRIEF.md section 6 and section 16 put them."""
    kinds, errors = gates_by_kind(graph)
    if errors:
        return errors
    checkpoint, finished = kinds["account-checkpoint"], kinds["finished"]
    for task in graph.tasks:
        tid, is_work = task["id"], task["type"] != "gate"
        if task["phase"] == 1 and tid not in graph.ancestors[checkpoint]:
            errors.append(f"{where(task)}: Phase 1 task must finish before the account checkpoint")
        if task["phase"] == 2 and is_work and checkpoint not in graph.ancestors[tid]:
            errors.append(f"{where(task)}: Phase 2 task must come after the account checkpoint")
        if task["phase"] == 2 and is_work and tid not in graph.ancestors[finished]:
            errors.append(f"{where(task)}: Phase 2 task must finish before 'Finished'")
    must_follow = (
        ("stage-0.5-go", "account-checkpoint"),
        ("real-texts-casey-phone", "account-checkpoint"),
        ("gate-b", "finished"),
        ("gate-c", "gate-b"),
        ("gate-c", "finished"),
        ("first-live-charge", "gate-c"),
    )
    for later, earlier in must_follow:
        if kinds[earlier] not in graph.ancestors[kinds[later]]:
            errors.append(f"gate {later} must come after gate {earlier}")
    return errors + check_gate_dependents(graph, kinds)


def check_gate_dependents(graph: Graph, kinds: dict[str, str]) -> list[str]:
    """Nothing waits on the page go or the first charge; only Gate C waits on Gate B;
    the real-texts question comes after the Twilio sandbox task (D-116)."""
    errors: list[str] = []
    for task in graph.tasks:
        for kind in NO_DEPENDENTS:
            if kinds[kind] in task["deps"]:
                errors.append(f"{where(task)}: nothing may depend on gate {kind}")
        if kinds["gate-b"] in task["deps"] and task["id"] != kinds["gate-c"]:
            errors.append(f"{where(task)}: only gate-c may depend on gate-b")
    sandbox = [t["id"] for t in graph.tasks if TWILIO_SANDBOX_PATH in t["owns"]]
    texts = kinds[CONDITIONAL_GATE]
    if not any(s in graph.ancestors[texts] for s in sandbox):
        errors.append(
            f"gate {CONDITIONAL_GATE} must come after the task owning {TWILIO_SANDBOX_PATH}"
        )
    return errors


def check_stage_order(graph: Graph) -> list[str]:
    """No task depends on a task from a later stage."""
    rank = {stage: i for i, stage in enumerate(STAGES)}
    errors: list[str] = []
    for task in graph.tasks:
        for dep in task["deps"]:
            if rank[graph.by_id[dep]["stage"]] > rank[task["stage"]]:
                errors.append(f"{where(task)}: depends on {dep} from a later stage")
    expected_phase = {
        "1": 1,
        "2": 1,
        "3": 1,
        "4": 1,
        "5": 1,
        "checkpoint": 0,
        "0.5": 0,
        "6": 2,
        "7": 2,
    }
    errors += [
        f"{where(t)}: stage {t['stage']} is phase {expected_phase[t['stage']]}"
        for t in graph.tasks
        if t["phase"] != expected_phase[t["stage"]]
    ]
    return errors


def check_file_header(data: dict[str, Any]) -> list[str]:
    """The file's own lists agree with the constants this script enforces."""
    errors: list[str] = []
    if data.get("schema") != 1:
        errors.append("tasks file: schema must be 1")
    if not set(data.get("protected_paths", [])) >= set(PROTECTED):
        errors.append("tasks file: protected_paths must list every protected path")
    if not set(data.get("shared_paths", [])) <= ALLOWED_SHARED:
        errors.append(f"tasks file: shared_paths may only be {sorted(ALLOWED_SHARED)}")
    if data.get("kit_outputs") != list(KIT_OUTPUTS):
        errors.append(f"tasks file: kit_outputs must be exactly {list(KIT_OUTPUTS)}")
    if not isinstance(data.get("tasks"), list) or not data["tasks"]:
        errors.append("tasks file: 'tasks' must be a non-empty list")
    return errors


@dataclass
class Report:
    tasks: list[Task]
    requirements: int
    td_entries: int
    errors: list[str]


def load(path: Path) -> tuple[dict[str, Any] | None, str | None]:
    if not path.is_file():
        return None, f"tasks file not found: {path}"
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        return None, f"tasks file is not valid JSON: {exc}"
    if not isinstance(data, dict):
        return None, "tasks file must hold a JSON object"
    return data, None


def run(root: Path, tasks_path: Path | None = None) -> Report:
    """Load the task file and run every check."""
    data, problem = load(tasks_path or root / TASKS_FILE)
    if data is None:
        return Report([], 0, 0, [problem or "unreadable tasks file"])
    errors = check_file_header(data)
    if errors:
        return Report([], 0, 0, errors)
    tasks: list[Task] = [t if isinstance(t, dict) else {"id": "?"} for t in data["tasks"]]
    for task in tasks:
        errors += check_task(task)
    if errors:
        return Report(tasks, 0, 0, errors)
    graph, graph_errors = build_graph(tasks)
    if graph_errors:
        return Report(tasks, 0, 0, graph_errors)
    spec = trace_spec.run(root)
    withdrawn = {r.req_id for r in spec.requirements if r.text.startswith("Withdrawn")}
    req_ids = {r.req_id for r in spec.requirements} - withdrawn
    td_ids = set(td_entries(root))
    errors += [f"spec: {e}" for e in spec.errors]
    errors += [e for t in tasks for e in check_path_rules(t)]
    errors += check_ownership(graph) + check_reads(root, graph)
    errors += check_coverage(graph, req_ids, td_ids, withdrawn) + check_references(root, graph)
    errors += check_gate_order(graph) + check_stage_order(graph)
    return Report(tasks, len(req_ids), len(td_ids), errors)


def format_counts(report: Report) -> list[str]:
    tasks = report.tasks
    lines = [f"Tasks: {len(tasks)}"]
    for label, key in (("By type", "type"), ("By stage", "stage"), ("By model", "model")):
        counts = Counter(str(t.get(key)) for t in tasks)
        lines.append(f"{label}: " + ", ".join(f"{k} {v}" for k, v in sorted(counts.items())))
    opus_reviews = sum(1 for t in tasks if t.get("reviewer_model") == OPUS)
    lines.append(f"Opus reviewer: {opus_reviews}")
    lines.append(f"Requirements in docs/spec: {report.requirements}")
    lines.append(f"Test-design entries: {report.td_entries}")
    lines.append(f"Errors: {len(report.errors)}")
    return lines


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="repository root (default: the parent of scripts/)",
    )
    parser.add_argument(
        "--tasks", type=Path, default=None, help="task file (default: ROOT/kit/tasks.json)"
    )
    args = parser.parse_args(argv)
    report = run(args.root, args.tasks)
    for line in format_counts(report):
        print(line)
    for error in report.errors:
        print(f"ERROR {error}", file=sys.stderr)
    return 1 if report.errors else 0


if __name__ == "__main__":
    sys.exit(main())
