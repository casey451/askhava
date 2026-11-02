"""Stage proofs the runner records itself, from results it checks (D-551, D-1473; docs/governance.md section 6).

A stage's named proof (its row in docs/build-readiness.md section 3) must "have run in `Ci` or nightly
on `main`". For a stage listed in MACHINE_PROOFS every clause of the row is a test in the suite that
nightly-main runs (`Ci`'s Checks, Coverage and E2e jobs, `Twin`, `Ally`, the named-mutants job), so the
runner records `stage_proofs[N]` itself, and only when all of these hold on the state and kit/nightly.json:

  1. every task of the stage is DONE (a gate may be SKIPPED), except the stage's cleanup task waiting
     only for this proof (runner.state.awaits_proof);
  2. kit/nightly.json is green on a commit of main that contains the merge of every task of the stage
     (`git merge-base --is-ancestor`), so that run tested all of the stage's work, and each nightly job
     a clause names (`results`: ci, twin, a11y, named_mutants) is recorded as success;
  3. each test file named for a clause exists in that commit with no skip path (SKIP_PATHS), so the
     clause it checks ran there and could not pass by skipping, and each mark a clause names (a test
     title, a mutant group, the axe widths) is in its file there;
  4. every requirement owned only by tasks of this stage or earlier (naming it, or owning a test
     entry that covers it) is verified by scripts/progress.py's rule.

A clause whose test a later task writes names the path that task owns (kit/tasks.json `owns`), so the
proof waits until the file exists. A stage with a clause no nightly result proves is in HAND_RECORDED,
with the reason, and keeps `python3 -m runner --record-proof N --evidence TEXT`. Because the proof needs
the whole stage, a listed stage's cleanup task waiting for it does not hold its own stage's later tasks
(runner.state.eligible, `proof_stages`); stage 1's T167 and T170 depend on T008, its cleanup.
"""

from __future__ import annotations

import re
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from typing import Any

from runner import state as st
from runner.state import State

@dataclass(frozen=True)
class Clause:
    """One clause of a stage's exit row and what proves it in a nightly-main run on `main`.

    `tests` are test files that must exist in the nightly commit with no skip path; `marks` are
    (path, text) pairs that must appear in that commit (a test or entry the clause needs, such as a
    mutant group or the axe widths); `jobs` are the kit/nightly.json `results` keys that must be
    "success" (ci holds Checks, Coverage and E2e; a11y is Ally; named_mutants the nightly job).

    A mark is a text proxy that the clause's subject is in place, never the proof itself: `"floors"` in
    tests/coverage_floor.json says the floors exist, and the ci job's Coverage compare proves them held;
    the jiff-sqlx marks say intake_timeouts.rs writes and reads a `timestamptz`, and that test passing in
    the ci job proves the round trip."""

    text: str
    tests: tuple[str, ...] = ()
    marks: tuple[tuple[str, str], ...] = ()
    jobs: tuple[str, ...] = ("ci",)


# The reduced J1 is J1 steps 1 to 6 (docs/build-readiness.md section 3, stage 2); the file runs only
# step 5 and part of step 6 today (D-1280), so the clause waits for a test per step (D-1473).
J1_REDUCED_STEPS = tuple(("tests/e2e/j1_reduced.spec.ts", f'test("J1 step {n}:') for n in range(1, 7))
AXE_WIDTHS = ("tests/a11y/fixtures.ts", "WIDTHS = [320, 768, 1280]")
MUTANTS = "tests/mutation/mutants.toml"

# Stage -> each clause of its exit row (docs/build-readiness.md section 3) and the tests that prove it.
MACHINE_PROOFS: dict[str, tuple[Clause, ...]] = {
    "1": (
        Clause("twin gate exits non-zero on a planted failure (T172)", ("tests/twin/test_gate.py",), jobs=("ci", "twin")),
        Clause("every worked example passes check_catalog.py", ("tests/test_check_catalog.py",)),
        Clause("every matching example passes check_catalog.py", ("tests/test_matching_examples.py",)),
        Clause("stand-ins over HTTP, contract tests green (T167)", ("tests/contract/test_stand_in_server.py",)),
    ),
    "2": (
        Clause("a planted cross-layer import refused (the architecture test scans a planted tree; no `cargo build` is "
               "planted); session-proof compile-fail cases (T223)",
               ("tests/integration/tests/architecture.rs", "tests/security/tests/session_proof_compile_fail.rs"),
               (("tests/integration/tests/architecture.rs", "fn td_sec_10_planted_violations_fail"),)),
        Clause("migrations apply from empty in number order", ("tests/integration/tests/migrations.rs",)),
        Clause("reduced J1 steps 1 to 6 in Playwright against stand-ins (T173)", ("tests/e2e/j1_reduced.spec.ts",),
               J1_REDUCED_STEPS),
        Clause("axe clean at 320/768/1280", ("tests/a11y/axe_all_pages.spec.ts",), (AXE_WIDTHS,), ("a11y",)),
        Clause("coverage floors active in Ci (Coverage job)", (), (("tests/coverage_floor.json", '"floors"'),)),
        Clause("named mutants for license filter, consent, quiet hours all killed", (),
               ((MUTANTS, 'rule = "License filter"'), (MUTANTS, 'rule = "Consent and opt-out"'), (MUTANTS, 'rule = "Quiet hours"')),
               ("named_mutants",)),
        Clause("a timestamptz round trip through jiff-sqlx", ("tests/integration/tests/intake_timeouts.rs",),
               (("tests/integration/tests/intake_timeouts.rs", ".to_sqlx()"),
                ("tests/integration/tests/intake_timeouts.rs", "jiff_sqlx::Timestamp"))),
    ),
    "4": (
        Clause("stage-4 slices of J1 and J2 green (T098, T099)",
               ("tests/e2e/j1_homeowner.spec.ts", "tests/e2e/j2_business.spec.ts",
                "tests/a11y/j1_keyboard.spec.ts", "tests/a11y/j2_keyboard.spec.ts"), jobs=("ci", "a11y")),
        Clause("authorization matrix covers every route (T088)", ("tests/security/tests/auth_matrix.rs",)),
        Clause("masked call through the fake Twilio with the PIN and the CALL-025 limits (T077, T080)",
               ("tests/integration/tests/pin_routing.rs", "tests/integration/tests/voice_calls.rs"),
               (("tests/integration/tests/pin_routing.rs", "CALL-025"),)),
        Clause("license import and the live-lookup fixture pass",
               ("tests/integration/tests/license_imports.rs", "tests/integration/tests/license_lookup_failures.rs")),
        Clause("the stage-2 sentinel scan still clean", ("tests/security/tests/sentinel_db_logs.rs",)),
    ),
}

# Stages whose row holds a clause no nightly-main result proves, so their proof stays a hand-run
# `--record-proof` from the tool runs and reports that prove it (D-1473). Never moved to MACHINE_PROOFS
# by mapping such a clause to a test that does not check it.
HAND_RECORDED: dict[str, str] = {
    "3": "its cargo-mutants clause: nightly-main's cargo-mutants job is report only until "
    "tests/mutation/equivalent.toml holds reviewed reasons (D-1286), kit/nightly.json records no result for it, "
    "and app/catalog/ has no rules.rs to mutate",
    "5": "its twin chaos run runs inside T126, not in nightly-main, and T125's no-Critical-or-High audit is a report",
    "6": "its real sandboxes, test site, ZAP scan, restore drill, T242 review and war games run outside nightly-main, "
    "and T140's audit and T171's drill are reports",
}


def hand_reason(stage: str) -> str:
    """What a stage the runner does not prove waits for: the hand-run command, and why when known."""
    why = f" ({HAND_RECORDED[stage]})" if stage in HAND_RECORDED else ""
    return f"stage {stage}'s proof is recorded by hand (`python3 -m runner --record-proof {stage} --evidence TEXT`){why}"


def tests_of(stage: str) -> tuple[str, ...]:
    """Every test file named for the stage, once each, in clause order."""
    return tuple(dict.fromkeys(path for clause in MACHINE_PROOFS.get(stage, ()) for path in clause.tests))


def jobs_of(stage: str) -> tuple[str, ...]:
    """Every kit/nightly.json result the stage's clauses need green."""
    return tuple(dict.fromkeys(job for clause in MACHINE_PROOFS.get(stage, ()) for job in clause.jobs))


@dataclass(frozen=True)
class World:
    """What the check reads from outside the state; injected so tests need no git or network.

    rows(state) is scripts/progress.py's compute() over that state and the same nightly: objects with
    `req_id`, `tasks`, `td` and `verified`."""

    nightly: dict[str, Any] | None
    is_ancestor: Callable[[str, str], bool | None]
    read_file: Callable[[str, str], str | None]  # (commit, path) -> the file's text there, None when absent
    rows: Callable[[State], Iterable[Any]]


# A test that can skip itself can be green without checking anything, so a named test file must have
# no skip path at all (D-551): pytest's; Rust's `#[ignore]`, also under `cfg_attr`; Playwright's skip,
# fixme, only and fail, on `test`, `test.describe` or `describe`, and `testInfo.skip`; and a mocha-style
# `it.skip("…")` (D-1473). The Playwright config and fixtures beside a named spec are scanned too, with
# their `//` comments left out, for the same calls and for `testIgnore` and `grep` (SUITE_FILES).
SKIP_PATHS = re.compile(
    r"pytest\.skip|skipif|importorskip|mark\.skip|xfail|SkipTest|unittest\.skip"
    r"|#\[ignore|cfg_attr\(.*\bignore\b"
    r"|\b(?:test|describe)(?:\.describe)?\.(?:skip|fixme|only|fail)\b|\bit\.(?:skip|fixme|only|fail)\(\s*[\"'`]"
    r"|\.info\(\)\.skip|testInfo\.skip"
)
SKIP_WORDS = "pytest.skip, skipif, importorskip, xfail, #[ignore], test.skip, test.fixme, test.only, test.fail"
CONFIG_SKIPS = re.compile(r"testIgnore|\bgrep(?:Invert)?\s*:")
SUITE_FILES: dict[str, tuple[str, ...]] = {
    "tests/e2e/": ("tests/e2e/playwright.config.ts", "tests/e2e/fixtures.ts"),
    "tests/a11y/": ("tests/a11y/playwright.config.ts", "tests/a11y/fixtures.ts"),
}


def _code(text: str) -> str:
    """The text without its whole-line `//` comments (a config's comment may name `test.only`)."""
    return "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("//"))


def _suite_gaps(sha: str, tests: tuple[str, ...], world: World) -> list[str]:
    """A named spec's Playwright config missing, or its config or fixtures able to skip or filter tests."""
    gaps = []
    for prefix, files in SUITE_FILES.items():
        if not any(path.startswith(prefix) for path in tests):
            continue
        for path in files:
            text = world.read_file(sha, path)
            if text is None:
                gaps += [f"{path} in nightly commit {sha[:12]}"] if path.endswith("config.ts") else []
            elif SKIP_PATHS.search(_code(text)) or CONFIG_SKIPS.search(_code(text)):
                gaps.append(f"{path} with no skip, testIgnore or grep in nightly commit {sha[:12]}")
    return gaps


def _test_gaps(sha: str, tests: tuple[str, ...], world: World) -> list[str]:
    """Each named test file missing from the nightly commit, or holding a skip path there."""
    gaps = []
    for path in tests:
        text = world.read_file(sha, path)
        if text is None:
            gaps.append(f"{path} in nightly commit {sha[:12]}")
        elif SKIP_PATHS.search(text):
            gaps.append(f"{path} with no skip path ({SKIP_WORDS}) in nightly commit {sha[:12]}")
    return gaps


def _mark_gaps(sha: str, stage: str, world: World) -> list[str]:
    """Each (path, text) a clause needs that the nightly commit lacks."""
    gaps = []
    for clause in MACHINE_PROOFS.get(stage, ()):
        for path, text in clause.marks:
            body = world.read_file(sha, path)
            if body is None or text not in body:
                gaps.append(f"{text!r} in {path} in nightly commit {sha[:12]} ({clause.text})")
    return gaps


def _job_gaps(stage: str, world: World) -> list[str]:
    """Each nightly-main job the stage's clauses need whose recorded result is not success."""
    results = (world.nightly or {}).get("results") or {}
    return [f"nightly-main job {job} green in kit/nightly.json (recorded: {results.get(job)})"
            for job in jobs_of(stage) if results.get(job) != "success"]


def _numeric(stage: Any) -> float | None:
    try:
        return float(stage)
    except (TypeError, ValueError):
        return None


def _open_tasks(tasks: list[dict[str, Any]], state: State) -> list[str]:
    """The stage's tasks that are neither DONE, a SKIPPED gate, nor its cleanup waiting for this proof."""
    out = []
    for task in tasks:
        entry = st.entry(state, task["id"])
        status = entry.get("status", st.QUEUED)
        if status == st.DONE or (task.get("type") == "gate" and status == st.SKIPPED) or st.awaits_proof(entry):
            continue
        out.append(f"{task['id']} is {status}")
    return out


def _nightly_gaps(tasks: list[dict[str, Any]], state: State, world: World) -> tuple[str, list[str]]:
    """(the nightly commit, what keeps that run from covering every merge of the stage)."""
    nightly = world.nightly or {}
    sha = str(nightly.get("sha") or "")
    if nightly.get("conclusion") != "success" or not sha:
        return "", ["a green nightly-main run on main (kit/nightly.json)"]
    gaps = []
    for task in (t for t in tasks if t.get("type") != "gate"):
        merged = str(st.entry(state, task["id"]).get("merged_sha") or "")
        if not merged:
            gaps.append(f"{task['id']}: no merged_sha recorded")
        elif world.is_ancestor(merged, sha) is not True:
            gaps.append(f"a green nightly-main run containing {task['id']}'s merge {merged[:12]} (last green run: {sha[:12]})")
    return sha, gaps


def owned_up_to(graph: list[dict[str, Any]], rows: Iterable[Any], stage: str) -> list[Any]:
    """The progress rows of requirements owned only by tasks of `stage` or an earlier numeric stage."""
    limit = _numeric(stage)
    stage_of = {t["id"]: _numeric(t.get("stage")) for t in graph}
    td_owner: dict[str, list[str]] = {}
    for task in graph:
        for td in task.get("tests", []):
            td_owner.setdefault(td, []).append(task["id"])
    out = []
    for row in rows:
        owners = set(row.tasks) | {o for td in row.td for o in td_owner.get(td, [])}
        stages = [stage_of.get(o) for o in owners]
        if owners and limit is not None and all(s is not None and s <= limit for s in stages):
            out.append(row)
    return out


def machine_proof(stage: str, graph: list[dict[str, Any]], state: State, world: World) -> tuple[dict[str, Any] | None, list[str]]:
    """(the proof record for `stage`, or None, and what the proof still waits for)."""
    if stage not in MACHINE_PROOFS:
        return None, [hand_reason(stage)]
    tasks = [t for t in graph if str(t.get("stage")) == stage]
    waits = _open_tasks(tasks, state)
    if waits:
        return None, waits
    sha, waits = _nightly_gaps(tasks, state, world)
    if waits:
        return None, waits
    tests = tests_of(stage)
    waits = _job_gaps(stage, world) + _test_gaps(sha, tests, world) + _suite_gaps(sha, tests, world) + _mark_gaps(sha, stage, world)
    owned = owned_up_to(graph, world.rows(state), stage)
    waits += [f"requirement {row.req_id} verified" for row in owned if not row.verified]
    if waits:
        return None, waits
    return {"evidence": _evidence(stage, tasks, state, sha, owned), "main_sha": sha, "by": "runner"}, []


def _evidence(stage: str, tasks: list[dict[str, Any]], state: State, sha: str, owned: list[Any]) -> str:
    merges = ", ".join(f"{t['id']} {str(st.entry(state, t['id']).get('merged_sha'))[:12]}" for t in tasks if t.get("type") != "gate")
    clauses = "; ".join(f"{c.text}: {', '.join(dict.fromkeys(c.tests + tuple(p for p, _ in c.marks)))} [{', '.join(c.jobs)}]"
                        for c in MACHINE_PROOFS[stage])
    reqs = ", ".join(row.req_id for row in owned) or "none"
    return (
        f"Recorded by the runner (D-551, D-1473): nightly-main green ({', '.join(jobs_of(stage))}) on main {sha}, which "
        f"contains every stage-{stage} merge ({merges}); each clause of the exit row ran in it ({clauses}); every "
        f"requirement owned only by tasks of stage {stage} or earlier is verified ({reqs})."
    )
