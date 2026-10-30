"""Stage proofs the runner records itself, from results it checks (D-551; docs/governance.md section 6).

A stage's named proof (its row in docs/build-readiness.md section 3) must "have run in `Ci` or nightly
on `main`". For a stage listed in MACHINE_PROOFS every clause of the row is a test in the suite that
nightly-main runs (`Ci`'s pytest, `Twin`, `Ally`), so the runner records `stage_proofs[N]` itself, and
only when all of these hold on the state and kit/nightly.json it reads:

  1. every task of the stage is DONE (a gate may be SKIPPED), except the stage's cleanup task waiting
     only for this proof (runner.state.awaits_proof);
  2. kit/nightly.json is green on a commit of main that contains the merge of every task of the stage
     (`git merge-base --is-ancestor`), so that run tested all of the stage's work;
  3. each test file named for the stage exists in that commit with no skip path (SKIP_PATHS), so the
     clause it checks ran there and could not pass by skipping;
  4. every requirement owned only by tasks of this stage or earlier (naming it, or owning a test
     entry that covers it) is verified by scripts/progress.py's rule.

The evidence string names the nightly commit, the merges, the tests and the requirements. A stage not
listed keeps `python3 -m runner --record-proof N --evidence TEXT`. Because the proof needs the whole
stage, a listed stage's cleanup task waiting for it does not hold its own stage's later tasks
(runner.state.eligible, `proof_stages`); stage 1's T167 and T170 depend on T008, its cleanup.
"""

from __future__ import annotations

import re
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from typing import Any

from runner import state as st
from runner.state import State

# Stage -> the tests that check each clause of its exit row (docs/build-readiness.md section 3).
MACHINE_PROOFS: dict[str, tuple[str, ...]] = {
    "1": (
        "tests/twin/test_gate.py",  # the twin gate exits non-zero on planted failures (T172)
        "tests/test_check_catalog.py",  # every worked example passes check_catalog.py
        "tests/test_matching_examples.py",  # every matching example passes check_catalog.py
        "tests/contract/test_stand_in_server.py",  # the stand-ins over HTTP, contract tests green (T167)
    ),
}


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
# no skip path at all (D-551).
SKIP_PATHS = re.compile(r"pytest\.skip|skipif|importorskip|mark\.skip|xfail|SkipTest|unittest\.skip")


def _test_gaps(sha: str, tests: tuple[str, ...], world: World) -> list[str]:
    """Each named test file missing from the nightly commit, or holding a skip path there."""
    gaps = []
    for path in tests:
        text = world.read_file(sha, path)
        if text is None:
            gaps.append(f"{path} in nightly commit {sha[:12]}")
        elif SKIP_PATHS.search(text):
            gaps.append(f"{path} with no skip path (pytest.skip, skipif, importorskip, xfail) in nightly commit {sha[:12]}")
    return gaps


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
    tests = MACHINE_PROOFS.get(stage)
    if tests is None:
        return None, [f"stage {stage}'s proof is recorded by hand (`python3 -m runner --record-proof {stage}`)"]
    tasks = [t for t in graph if str(t.get("stage")) == stage]
    waits = _open_tasks(tasks, state)
    if waits:
        return None, waits
    sha, waits = _nightly_gaps(tasks, state, world)
    if waits:
        return None, waits
    waits = _test_gaps(sha, tests, world)
    owned = owned_up_to(graph, world.rows(state), stage)
    waits += [f"requirement {row.req_id} verified" for row in owned if not row.verified]
    if waits:
        return None, waits
    return {"evidence": _evidence(stage, tasks, state, sha, tests, owned), "main_sha": sha, "by": "runner"}, []


def _evidence(stage: str, tasks: list[dict[str, Any]], state: State, sha: str, tests: tuple[str, ...], owned: list[Any]) -> str:
    merges = ", ".join(f"{t['id']} {str(st.entry(state, t['id']).get('merged_sha'))[:12]}" for t in tasks if t.get("type") != "gate")
    reqs = ", ".join(row.req_id for row in owned) or "none"
    return (
        f"Recorded by the runner (D-551): nightly-main green (Ci, Twin, Ally) on main {sha}, which contains every "
        f"stage-{stage} merge ({merges}); the exit row's tests ran in it ({', '.join(tests)}); every requirement "
        f"owned only by tasks of stage {stage} or earlier is verified ({reqs})."
    )
