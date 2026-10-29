"""Validator for kit/tasks.json, the build task graph (kit step K6).

Checks that every task is well formed; dependencies exist and form no cycle;
every requirement in docs/spec/ (read with scripts/trace_spec.py), every
test-design entry (TD-...) and every end-to-end journey is covered by a task;
owned paths stay out of protected paths unless the task is a harness task;
no two tasks that can run in either order own the same path; each task fits
its size budget; models are valid full IDs; money, privacy, security and
recovery tasks (and every harness task) have an Opus reviewer; reads, decisions
and citations resolve; and the owner gates sit where BUILD_BRIEF.md section 6
puts them. Plan change P-1 (D-485) adds: build and harness tasks name tests or an
acceptance line (V1); every "killed by" test of a named mutant in
docs/test-design.md section 4 is owned by an ancestor of the task that writes the
mutant (V3); the coverage, twin and end-to-end harness come before the code they
guard (V4); a goal never just repeats its title (V5); the owner of a cargo-mutants
rules file has an Opus reviewer (V6); a task reads the spec file of every
requirement it names (V7); and migration numbers rise in the runner's serial order
(V9). Prints counts; exits 1 on any error.

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
        "app/server/src/bin/worker.rs",  # the claim loop (T013) and the SIGTERM drain (T019) edit the worker binary T009 created (D-491)
        "Cargo.toml",
        "Cargo.lock",
        ".sqlx/",
        "package.json",
        "package-lock.json",
        "kit/plan.lock",  # the blueprint fingerprint, rewritten by any PR that changes a locked file (D-479)
        "app/core/src/settings.rs",  # typed settings: T011 creates it, each later task adds its own keys (D-493)
    }
    | {f"app/{m}/Cargo.toml" for m in CRATE_MODULES}
    | {f"app/{m}/CLAUDE.md" for m in CRATE_MODULES}  # crate docs any task updates (D-473)
    | {f"app/{m}/src/tests/mod.rs" for m in CRATE_MODULES}  # in-crate test module list, one `mod` line per task (D-481)
    | {f"app/{m}/src/lib.rs" for m in CRATE_MODULES}  # hand-written module list, one `mod` line per task (D-481, D-484)
    | {f"app/{m}/src/api.rs" for m in CRATE_MODULES}  # each task adds its own public functions (D-484)
    | {f"tests/{layer}/Cargo.toml" for layer in TEST_CRATES}
    | {f"app/{m}/sqlx.toml" for m in CRATE_MODULES}  # query-macro type overrides, added by a module's first database task (D-488)
    | {f"app/{m}/askama.toml" for m in CRATE_MODULES}  # template `dirs` pointing at the shared layout, added by a crate's first template task (D-491)
    | {f"app/{m}/src/status.rs" for m in CRATE_MODULES}  # the crate's status enums; each task adds its own (SEC-031, D-491)
    | {"app/messaging/templates/en.json", "app/messaging/templates/es.json"}  # every texting task adds only its own keys (D-491)
    | {"kit/prompts/"}  # regenerated by any task that changes a pasted sentence (D-491)
    | {f"tests/{layer}/sqlx.toml" for layer in TEST_CRATES}
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
MAX_OWNS = 10  # P-1 (D-485): one session's work; was 14 (D-119)
MAX_FILES = 10
MAX_DIR_FILES = 14  # a cleanup or harness task owning a directory; the runner holds its diff to budget.files
MAX_LINES = 600  # was 800 (D-119)
MAX_CONTEXT_PCT = 20  # derived from bytes by scripts/gen_prompts.py (D-485)
# A harness task edits existing tooling it must read whole (its owned files count in full), so it may use
# up to 30%; every other task stays at 20% (D-485).
MAX_HARNESS_CONTEXT_PCT = 30
MAX_REQUIREMENTS = 12  # was 25 (D-119)
MAX_TESTS = 6
# Harness, docs, audit and cleanup work keeps the D-119 limits; the P-1 limits above bind build tasks,
# the ones a single worker session writes application code for (D-485).
NON_BUILD_LIMITS = {"owns": 14, "files": 14, "lines": 800, "requirements": 25}
OPTIONAL_KEYS: dict[str, type] = {"acceptance": str, "gate": dict}
# cargo-mutants targets (docs/test-design.md section 4): their owners need an Opus reviewer (V6).
MUTANT_RULE_MODULES = ("payments", "catalog", "licensing", "quotes", "messaging", "intake")
MUTANTS_FILE = "tests/mutation/mutants.toml"
COVERAGE_FLOOR = "tests/coverage_floor.json"
COVERED_CODE = ("app/privacy/", "app/licensing/", "app/payments/")
TWIN_GATE = "twin/gate.py"
TWIN_WORKFLOW = ".github/workflows/twin.yml"
E2E_HARNESS = (".github/workflows/a11y.yml", "tests/e2e/playwright.config.ts")
MIGRATION = re.compile(r"^migrations/(?P<number>\d{4})_[a-z0-9_]+\.sql$")
TD_RANGE = re.compile(r"(TD-[A-Z0-9]+-)(\d{2}) to TD-[A-Z0-9]+-(\d{2})")
TD_TOKEN = re.compile(r"TD-[A-Z0-9]+-\d{2}")
MUTANT_OVERRIDE = re.compile(r"killed by (?P<td>TD-[A-Z0-9]+-\d{2}), owned by (?P<task>T\d{3})")

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
    errors += [
        f"{where(task)}: '{key}' must be {kind.__name__}"
        for key, kind in OPTIONAL_KEYS.items()
        if key in task and not isinstance(task[key], kind)
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
    owns_app_code = any(
        o.startswith(("app/", "migrations/")) and "/src/tests/" not in o for o in task["owns"]
    )
    writes_sensitive_code = task["type"] == "build" and owns_app_code and bool(
        derived_areas(task) or areas & OPUS_REVIEW_AREAS
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
    has_dir = any(p.endswith("/") for p in task["owns"])
    build = task["type"] == "build"
    max_files = MAX_FILES if build else NON_BUILD_LIMITS["files"]
    limits = (
        (len(task["owns"]), MAX_OWNS if build else NON_BUILD_LIMITS["owns"], "owned paths"),
        (files, MAX_DIR_FILES if has_dir else max_files, "budget files"),
        (lines, MAX_LINES if build else NON_BUILD_LIMITS["lines"], "budget lines"),
        (ctx, MAX_HARNESS_CONTEXT_PCT if task["type"] == "harness" else MAX_CONTEXT_PCT, "budget context_pct"),
        (len(task["requirements"]), MAX_REQUIREMENTS if build else NON_BUILD_LIMITS["requirements"], "requirements"),
        (len(task["tests"]), MAX_TESTS, "tests"),
    )
    errors += [
        f"{where(task)}: {name} {value} over the limit of {limit}"
        for value, limit, name in limits
        if value > limit
    ]
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


def check_done_defined(task: Task) -> list[str]:
    """V1: a build or harness task names its tests or says in 'acceptance' what done means."""
    if task["type"] not in ("build", "harness") or task["tests"]:
        return []
    if str(task.get("acceptance", "")).strip():
        return []
    return [f"{where(task)}: a {task['type']} task with no tests needs an 'acceptance' line"]


def check_goal(task: Task) -> list[str]:
    """V5: the goal says what done looks like; a copy of the title says nothing."""
    if task["goal"].strip() == task["title"].strip():
        return [f"{where(task)}: goal repeats the title; write what done looks like"]
    return []


def check_rules_reviewer(task: Task) -> list[str]:
    """V6: the owner of a cargo-mutants rules file has an Opus reviewer."""
    targets = [f"app/{m}/src/rules.rs" for m in MUTANT_RULE_MODULES]
    owned = [t for t in targets if any(overlaps(t, p) for p in task["owns"])]
    if owned and task["type"] != "gate" and task["reviewer_model"] != OPUS:
        return [f"{where(task)}: owns {owned[0]}, a cargo-mutants target, and needs an Opus reviewer"]
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
        check_done_defined,
        check_goal,
        check_rules_reviewer,
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


def td_covers(root: Path) -> dict[str, list[str]]:
    """Each test-design row's covered requirement IDs."""
    path = root / TEST_DESIGN
    if not path.is_file():
        return {}
    out: dict[str, list[str]] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        m = TD_ROW.match(line)
        if m:
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            out[m.group("id")] = re.findall(r"\b[A-Z]{2,5}-\d{3}\b", cells[3]) if len(cells) > 3 else []
    return out


def check_row_cases_have_a_builder(root: Path, graph: Graph) -> list[str]:
    """V11 (D-491): a test row's case for requirement R is written by a task that owns R.

    When a task lists a row whose covered requirement R is owned only by tasks that run after it and do
    not share the row, nobody writes R's case: the cold reads found this shape about sixty times. The
    rule passes when R's owner is the task itself, one of its ancestors, or another task that lists the
    same row.
    """
    covers = td_covers(root)
    owners: dict[str, set[str]] = {}
    row_tasks: dict[str, set[str]] = {}
    for task in graph.tasks:
        for r in task["requirements"]:
            owners.setdefault(r, set()).add(task["id"])
        for row in task["tests"]:
            row_tasks.setdefault(row, set()).add(task["id"])
    errors = []
    for task in graph.tasks:
        for row in task["tests"]:
            for r in covers.get(row, []):
                who = owners.get(r)
                if not who or task["id"] in who or who & graph.ancestors[task["id"]] or who & row_tasks[row]:
                    continue
                errors.append(
                    f"{where(task)}: {row} covers {r}, owned only by {sorted(who)}, which run after this task and do not list the row (nobody writes that case; V11)"
                )
    return errors


def check_crate_exists_before_its_files(graph: Graph) -> list[str]:
    """V12 (D-493): a path under `app/<crate>/` is owned only by the crate's manifest owner or a task after it.

    The workspace lists `members = ["app/*"]`, so a directory under `app/` with no `Cargo.toml` fails
    every cargo command for every task that runs until the manifest arrives. The fifth cold read found
    a harness task placing data files under two crates that did not exist yet.
    """
    manifest_owners: dict[str, set[str]] = {}
    for task in graph.tasks:
        for path in task["owns"]:
            m = re.fullmatch(r"app/([a-z_]+)/Cargo\.toml", path)
            if m:
                manifest_owners.setdefault(m.group(1), set()).add(task["id"])
    errors = []
    for task in graph.tasks:
        for path in task["owns"]:
            m = re.match(r"app/([a-z_]+)/", path)
            if not m or path.endswith("Cargo.toml"):
                continue
            crate = m.group(1)
            owners = manifest_owners.get(crate, set())
            if not owners:
                if crate in CRATE_MODULES:
                    errors.append(f"{where(task)}: owns {path} but no task owns app/{crate}/Cargo.toml (V12)")
                continue
            if task["id"] in owners or owners & graph.ancestors[task["id"]]:
                continue
            errors.append(
                f"{where(task)}: owns {path} before {sorted(owners)} create app/{crate}/Cargo.toml (cargo fails on the manifest-less member; V12)"
            )
    return errors


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


def check_spec_reads(graph: Graph, req_files: dict[str, str]) -> list[str]:
    """V7: a task reads the spec file of every requirement it names."""
    return [
        f"{where(task)}: names {req} but does not read {req_files[req]}"
        for task in graph.tasks
        for req in task["requirements"]
        if req in req_files and req_files[req] not in task["reads"]
    ]


def expand_tds(cell: str) -> list[str]:
    """TD IDs in a table cell, with 'TD-CON-01 to TD-CON-06' expanded."""
    found: list[str] = []
    for prefix, start, end in TD_RANGE.findall(cell):
        found += [f"{prefix}{n:02d}" for n in range(int(start), int(end) + 1)]
    return found + [t for t in TD_TOKEN.findall(TD_RANGE.sub("", cell)) if t not in found]


def mutant_rows(root: Path) -> list[tuple[str, str, str]]:
    """(rule, mutants cell, killed-by cell) for each row of docs/test-design.md section 4."""
    path = root / TEST_DESIGN
    if not path.is_file():
        return []
    rows: list[tuple[str, str, str]] = []
    in_section = False
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("## "):
            in_section = line.startswith("## 4.")
        if in_section and line.startswith("| **"):
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if len(cells) >= 3:
                rule = re.sub(r"\(.*?\)", "", cells[0]).replace("*", "").strip().lower()
                rows.append((rule, cells[1], cells[2]))
    return rows


def killer_problem(graph: Graph, td: str, mutant_task: str) -> str | None:
    owners = [t["id"] for t in graph.tasks if td in t["tests"]]
    allowed = graph.ancestors.get(mutant_task, set())
    if any(o in allowed for o in owners):
        return None
    return f"{td} kills a named mutant of {mutant_task} but no ancestor of {mutant_task} owns it (owners: {owners})"


def check_mutant_killers(root: Path, graph: Graph) -> list[str]:
    """V3: every 'killed by' test is owned by an ancestor of the task that writes the mutant."""
    writers = [t for t in graph.tasks if MUTANTS_FILE in t["owns"]]
    errors: list[str] = []
    for rule, mutants, killers in mutant_rows(root):
        task = next((t for t in writers if rule in t["title"].lower()), None)
        if task is None:
            errors.append(f"test-design section 4: no task owning {MUTANTS_FILE} names '{rule}' in its title")
            continue
        overrides = {m["td"]: m["task"] for m in MUTANT_OVERRIDE.finditer(mutants)}
        for td in dict.fromkeys([*expand_tds(killers), *overrides]):
            owner = overrides.get(td, task["id"])
            problem = killer_problem(graph, td, owner) if owner in graph.by_id else f"{td}: unknown task {owner}"
            if problem:
                errors.append(f"test-design section 4 ({rule}): {problem}")
    return errors


def owners_of(graph: Graph, path: str, harness_only: bool = False) -> list[str]:
    return [
        t["id"]
        for t in graph.tasks
        if any(overlaps(path, p) for p in t["owns"]) and (t["type"] == "harness" or not harness_only)
    ]


def check_harness_order(graph: Graph) -> list[str]:
    """V4: the coverage gate precedes vault, licensing and money code; the twin gate precedes twin work;
    the twin workflow and the end-to-end harness exist."""
    errors: list[str] = []
    floors = set(owners_of(graph, COVERAGE_FLOOR, harness_only=True))
    for task in graph.tasks:
        guarded = [p for p in task["owns"] if not p.endswith("/") and p.startswith(COVERED_CODE)]
        if guarded and not floors & graph.ancestors[task["id"]]:
            errors.append(f"{where(task)}: owns {guarded[0]} but no harness task owning {COVERAGE_FLOOR} comes first")
    gates = set(owners_of(graph, TWIN_GATE))
    for task in graph.tasks:
        if not any(overlaps(p, "twin/") for p in task["owns"]):
            continue
        root_gate = task["id"] in gates and not gates & graph.ancestors[task["id"]]
        if not root_gate and not gates & graph.ancestors[task["id"]]:
            errors.append(f"{where(task)}: changes twin/ before the task owning {TWIN_GATE}")
    if not owners_of(graph, TWIN_WORKFLOW):
        errors.append(f"no task owns {TWIN_WORKFLOW}, so Twin never becomes the gate")
    e2e = [t for t in graph.tasks if t["type"] == "harness" and all(p in t["owns"] for p in E2E_HARNESS)]
    if not e2e:
        errors.append(f"no harness task owns {' and '.join(E2E_HARNESS)} (the e2e and Ally harness)")
    return errors


def serial_order(tasks: list[Task]) -> list[Task]:
    """The order a one-at-a-time runner meets the tasks: the first task in file order whose deps are done."""
    done: set[str] = set()
    order: list[Task] = []
    left = list(tasks)
    while left:
        nxt = next((t for t in left if set(t["deps"]) <= done), None)
        if nxt is None:
            break
        order.append(nxt)
        done.add(nxt["id"])
        left.remove(nxt)
    return order


def check_migration_order(graph: Graph) -> list[str]:
    """V9: migration numbers rise in the runner's serial order, so a later task never adds a lower number."""
    errors: list[str] = []
    highest, highest_by = -1, ""
    for task in serial_order(graph.tasks):
        for path in task["owns"]:
            match = MIGRATION.match(path)
            if not match:
                continue
            number = int(match["number"])
            if number <= highest:
                errors.append(
                    f"{where(task)}: migration {path} is numbered at or below {highest:04d}"
                    f" ({highest_by}), which runs earlier; number migrations by serial position"
                )
            else:
                highest, highest_by = number, task["id"]
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
    req_files = {r.req_id: r.spec_file for r in spec.requirements}
    errors += check_spec_reads(graph, req_files) + check_mutant_killers(root, graph)
    errors += check_harness_order(graph) + check_migration_order(graph)
    errors += check_row_cases_have_a_builder(root, graph) + check_crate_exists_before_its_files(graph)
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
