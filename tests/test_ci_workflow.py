"""The shape of `.github/workflows/ci.yml` that keeps the required check `Ci` complete while its jobs run in
parallel (D-2782, D-3142): `Ci` needs every other job and is green only when each one is; the Rust tests are
split between "Checks" and "Lint and slow tests" by one list of libtest filters, so every test runs in exactly one
of them under the same `cargo test` command; both jobs decide "Rust changed" by the same rule; fmt, clippy and the
offline query check still run once; the trybuild cache guard sits in the job that runs the trybuild tests.
Reads the workflow only; no network.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any

import yaml

ROOT = Path(__file__).resolve().parent.parent
CI = ROOT / ".github/workflows/ci.yml"
TEST_COMMAND = "cargo test --workspace --locked --"
RUST_RULE = re.compile(r"grep -Eq '([^']+)'\s*&&\s*rust=true")


def _ci() -> dict[str, Any]:
    return yaml.safe_load(CI.read_text(encoding="utf-8"))


def _runs(job: dict[str, Any]) -> list[str]:
    return [str(step.get("run", "")) for step in job["steps"]]


def _jobs_running(text: str) -> list[str]:
    return [name for name, job in _ci()["jobs"].items() if any(text in run for run in _runs(job))]


def test_ci_needs_every_other_job_and_requires_each_to_succeed() -> None:
    """D-2782, D-3142: a job left out of `needs`, or out of the final comparison, would not gate `Ci`."""
    jobs = _ci()["jobs"]
    gate = jobs["ci"]
    assert gate["name"] == "Ci" and gate["if"] == "always()"
    others = [name for name in jobs if name != "ci"]
    assert sorted(gate["needs"]) == sorted(others)
    (step,) = gate["steps"]
    for name in others:
        var = next(k for k, v in step["env"].items() if v == f"${{{{ needs.{name}.result }}}}")
        assert f'[ "${var}" = "success" ]' in step["run"], name
    assert step["run"].strip().splitlines()[-1].count("&&") == len(others) - 1


def test_the_split_tests_run_in_exactly_one_job_under_the_same_command() -> None:
    """D-3142: Checks skips each SPLIT_TESTS filter, the other job runs only those filters; libtest's `--skip f`
    and filter `f` are the same substring match, so the two runs partition every test (doc-tests included), and
    the identical `cargo test --workspace --locked` keeps the workspace's feature unification."""
    ci = _ci()
    filters = ci["env"]["SPLIT_TESTS"].split()
    assert filters
    checks = [run for run in _runs(ci["jobs"]["checks"]) if TEST_COMMAND in run]
    lint = [run for run in _runs(ci["jobs"]["lint"]) if TEST_COMMAND in run]
    assert len(checks) == 1 and len(lint) == 1
    read = 'read -ra filters <<< "$SPLIT_TESTS"'
    assert read in checks[0] and read in lint[0]
    assert 'for filter in "${filters[@]}"; do skip+=(--skip "$filter"); done' in checks[0]
    assert f'{TEST_COMMAND} "${{skip[@]}}"' in checks[0]
    assert f'{TEST_COMMAND} "${{filters[@]}}"' in lint[0]
    assert _jobs_running(TEST_COMMAND) == ["checks", "lint"]


def test_each_split_filter_names_a_test_that_exists() -> None:
    """D-3142: a filter that matched nothing would leave the job running nothing (still complete, but pointless)."""
    files = [*ROOT.glob("tests/**/*.rs"), *ROOT.glob("app/**/*.rs")]
    sources = "\n".join(p.read_text(encoding="utf-8") for p in files if "target" not in p.parts)
    for name in _ci()["env"]["SPLIT_TESTS"].split():
        assert re.search(rf"\bfn \w*{re.escape(name)}\w*", sources), name


def test_both_rust_test_jobs_decide_rust_changed_by_the_same_rule() -> None:
    """D-489, D-3142: the copy in "Lint and slow tests" equals Checks' rule (which tests/test_prehandin.py ties to
    scripts/prehandin.sh), so a change that runs Checks' tests also runs the split tests and the lints."""
    jobs = _ci()["jobs"]
    rules = {name: [m.group(1) for run in _runs(jobs[name]) for m in RUST_RULE.finditer(run)] for name in ("checks", "lint")}
    assert len(rules["checks"]) == 1 and rules["lint"] == rules["checks"]


def test_fmt_clippy_and_the_query_check_still_run_once() -> None:
    """D-488, D-3142: moved out of Checks, not dropped; their commands are unchanged."""
    for command in ("cargo fmt --all --check",
                    "cargo clippy --workspace --all-targets --locked -- -D warnings -F clippy::allow_attributes_without_reason",
                    "cargo sqlx prepare --workspace --check -- --all-targets --all-features"):
        assert _jobs_running(command) == ["lint"], command
    assert _jobs_running("sqlx migrate run") == ["checks", "lint"]
    assert _jobs_running("python3 scripts/run_named_mutants.py --base origin/main") == ["checks"]
    assert _jobs_running("cargo build -p askhava-server --locked") == ["checks"]
    assert _jobs_running("cargo deny check licenses bans sources advisories") == ["checks"]


def test_the_trybuild_cache_and_its_guard_live_where_the_trybuild_tests_run() -> None:
    """D-488, D-693, D-3142: the job that runs the compile-fail drivers caches target/tests/trybuild and fails an
    exact cache hit that lost it; Checks no longer names that folder, so its guard would have nothing to check."""
    jobs = _ci()["jobs"]
    named = [name for name, job in jobs.items()
             if any(s.get("with", {}).get("cache-directories") == "target/tests/trybuild" for s in job["steps"])]
    assert named == ["lint"]
    guard = _jobs_running('test -d target/tests/trybuild || { echo "the cache lost target/tests/trybuild"; exit 1; }')
    assert guard == ["lint"]
    assert "_cannot_be_forged_outside_" in _ci()["env"]["SPLIT_TESTS"].split()
