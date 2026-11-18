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


PROOF_CALLS = {"checks": [("Checks", "rust"), ("Checks", "npm")], "lint": [("Lint and slow tests", "rust")],
               "coverage": [("Coverage", "rust")], "e2e": [("E2e", "run")]}
EARLY_EXIT = 'if [ "$EVENT_NAME" = "schedule" ] || [ "$EVENT_NAME" = "workflow_dispatch" ] || [ "$GITHUB_REF_NAME" = "main" ]; then'


def _changes(job: str) -> dict[str, Any]:
    (step,) = [s for s in _ci()["jobs"][job]["steps"] if s.get("id") == "changes"]
    return step


def test_each_changes_step_passes_its_own_rule_and_trusts_only_a_proven_line() -> None:
    """D-4281: the script reads the rule from the step's own script ("$0"), one per flag, and only an exact
    `proven=<40 hex>` line sets a flag back to false; the step's token reads the parent's runs."""
    rule_of = re.compile(r"grep -Eq '([^']+)'\s*&&\s*(\w+)=true")
    for job, calls in PROOF_CALLS.items():
        step, name = _changes(job), _ci()["jobs"][job]["name"]
        text = step["run"]
        assert step["env"]["GH_TOKEN"] == "${{ github.token }}", job
        assert 'line="$(python3 scripts/ci_proven.py --job "$1" --flag "$2" --step-script "$0" || true)"' in text
        assert '[[ "$line" =~ ^proven=[0-9a-f]{40}$ ]]' in text and text.count("ci_proven.py") == 1, job
        flags = [flag for _rule, flag in rule_of.findall(text)]
        assert sorted(flags) == sorted(flag for _name, flag in calls) and len(set(flags)) == len(flags), job
        for job_name, flag in calls:
            assert job_name == name
            assert f'if [ "${flag}" = true ] && proven "{name}" {flag}; then {flag}=false; fi' in text, job


def test_the_early_exit_is_unchanged_and_comes_before_the_proof() -> None:
    """D-489, D-4281: schedule, workflow_dispatch and main runs still compile everything, before any proof."""
    for job in PROOF_CALLS:
        text = _changes(job)["run"]
        assert text.count(EARLY_EXIT) == 1 and text.index(EARLY_EXIT) < text.index("proven()"), job
        exit_line = text[text.index(EARLY_EXIT):].splitlines()[1]
        assert exit_line.strip().endswith("exit 0") and "=true" in exit_line, job


def test_the_delete_guard_precedes_the_checks_changes_step() -> None:
    """D-479: a deleted compiled input fails the run before any flag is decided."""
    names = [s.get("name", "") for s in _ci()["jobs"]["checks"]["steps"]]
    guard = names.index("A check that exists on main cannot be skipped by deleting its inputs (D-479)")
    assert guard < [s.get("id") for s in _ci()["jobs"]["checks"]["steps"]].index("changes")


def test_no_scan_lock_rule_or_python_step_depends_on_the_flags() -> None:
    """D-4281: only compile steps sit behind the flags; the scans, the plan lock and change rules, the Rust source
    rules, the approved crates, the compiled-in-file check and the whole Python job run on every head."""
    jobs = _ci()["jobs"]
    always = ("Sources unchanged", "Secret scan", "PII scan", "No git conflict markers", "Plan lock and change rules",
              "A check that exists on main", "Rust source rules", "Approved crates only", "Every compiled-in file")
    for prefix in always:
        (step,) = [s for s in jobs["checks"]["steps"] if s.get("name", "").startswith(prefix)]
        assert "steps.changes" not in str(step.get("if", "")), prefix
    assert not any("steps.changes" in str(s.get("if", "")) for s in jobs["python"]["steps"])


def test_the_nightly_ci_call_grants_what_ci_asks() -> None:
    """D-3795, D-4281: a called workflow gets no more than its caller grants, so a missing grant stops the nightly."""
    nightly = yaml.safe_load((ROOT / ".github/workflows/nightly-main.yml").read_text(encoding="utf-8"))
    granted, asked = nightly["jobs"]["ci"]["permissions"], _ci()["permissions"]
    assert asked == {"contents": "read", "actions": "read"}
    assert all(granted.get(scope) == level for scope, level in asked.items())
