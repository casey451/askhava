"""The shape and behaviour of `.github/workflows/flake-run.yml` (D-5996 part b, T879): manual only, read-only,
every input checked by pattern before any build and passed to steps only through env:, ci.yml's setup reused,
and a loop that runs every repeat and reports pass and fail counts. The bash steps and the Playwright counter
are run here with fake `cargo` and fake reports; no network, no build.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
from pathlib import Path
from typing import Any

import pytest
import yaml

ROOT = Path(__file__).resolve().parent.parent
FLAKE = ROOT / ".github/workflows/flake-run.yml"
CI = ROOT / ".github/workflows/ci.yml"


def _load(path: Path) -> dict[str, Any]:
    return yaml.safe_load(path.read_text(encoding="utf-8"))


def _step(job: str, prefix: str) -> dict[str, Any]:
    steps = _load(FLAKE)["jobs"][job]["steps"]
    return next(s for s in steps if str(s.get("name", "")).startswith(prefix))


def _bash(script: str, env: dict[str, str], tmp: Path) -> subprocess.CompletedProcess[str]:
    """Runs a step as GitHub's default `bash -e {0}` does, with a summary file and a temp dir."""
    full = {"PATH": os.environ["PATH"], "RUNNER_TEMP": str(tmp), "GITHUB_STEP_SUMMARY": str(tmp / "summary.md")}
    return subprocess.run(["bash", "-e", "-c", script], env={**full, **env}, capture_output=True, text=True, check=False)


def test_the_trigger_is_only_workflow_dispatch_with_repeat_default_10() -> None:
    on = _load(FLAKE)[True]
    assert list(on) == ["workflow_dispatch"]
    inputs = on["workflow_dispatch"]["inputs"]
    assert set(inputs) == {"kind", "crate", "test_binary", "filter", "spec", "repeat"}
    assert inputs["repeat"]["default"] == "10"


def test_inputs_reach_steps_only_through_env() -> None:
    text = FLAKE.read_text(encoding="utf-8")
    runs = [str(s.get("run", "")) for job in _load(FLAKE)["jobs"].values() for s in job["steps"]]
    assert runs and not any("${{" in run for run in runs)
    # Every inputs.* use outside env: is a job-level `if:` choosing the kind.
    for line in text.splitlines():
        if "inputs." in line and "${{ inputs." not in line:
            assert re.fullmatch(r"    if: inputs\.kind == '(rust|playwright)'", line), line


def test_validation_runs_first_and_every_build_job_needs_it() -> None:
    jobs = _load(FLAKE)["jobs"]
    assert len(jobs["validate"]["steps"]) == 1
    assert jobs["rust"]["needs"] == "validate" and jobs["playwright"]["needs"] == "validate"


RUST_OK = {"KIND": "rust", "CRATE": "askhava-core", "TEST_BINARY": "fake_bin", "FILTER": "a::b_1", "REPEAT": "10"}
PW_OK = {"KIND": "playwright", "SPEC": "tests/e2e/sub/fake_one.spec.ts", "REPEAT": "10"}


@pytest.mark.parametrize(
    ("env", "ok"),
    [
        (RUST_OK, True),
        ({**RUST_OK, "FILTER": ""}, True),
        ({**RUST_OK, "REPEAT": "1"}, True),
        ({**RUST_OK, "REPEAT": "30"}, True),
        (PW_OK, True),
        ({**RUST_OK, "REPEAT": "31"}, False),
        ({**RUST_OK, "REPEAT": "0"}, False),
        ({**RUST_OK, "REPEAT": "05"}, False),
        ({**RUST_OK, "REPEAT": "1a"}, False),
        ({**RUST_OK, "CRATE": "Askhava"}, False),
        ({**RUST_OK, "CRATE": "a;b"}, False),
        ({**RUST_OK, "TEST_BINARY": "fake-bin"}, False),
        ({**RUST_OK, "FILTER": "a b"}, False),
        ({**RUST_OK, "FILTER": "a$(id)"}, False),
        ({**PW_OK, "SPEC": "tests/e2e/../../fake.spec.ts"}, False),
        ({**PW_OK, "SPEC": "app/fake.spec.ts"}, False),
        ({**PW_OK, "SPEC": "tests/e2e/fake.spec.js"}, False),
        ({**PW_OK, "SPEC": "tests/e2e/fake one.spec.ts"}, False),
        ({**PW_OK, "KIND": "other"}, False),
    ],
)
def test_each_input_is_checked_against_its_pattern(env: dict[str, str], ok: bool, tmp_path: Path) -> None:
    result = _bash(_step("validate", "Check every input")["run"], env, tmp_path)
    assert (result.returncode == 0) is ok, result.stdout


def test_read_only_no_push_no_state_no_secret_no_save() -> None:
    wf = _load(FLAKE)
    assert wf["permissions"] == {"contents": "read"}
    assert all("permissions" not in job for job in wf["jobs"].values())
    text = FLAKE.read_text(encoding="utf-8")
    for banned in ("git push", "secrets.", "github.token", "upload-artifact", "cache/save", "kit-state", "git tag"):
        assert banned not in text, banned
    caches = [s for j in wf["jobs"].values() for s in j["steps"] if "rust-cache" in str(s.get("uses", ""))]
    assert len(caches) == 2 and all(s["with"]["save-if"] is False for s in caches)


def test_actions_are_pinned_by_sha_as_ci_pins_them() -> None:
    ci_text = CI.read_text(encoding="utf-8")
    uses = re.findall(r"uses: (\S+)", FLAKE.read_text(encoding="utf-8"))
    assert uses
    for use in uses:
        assert re.fullmatch(r"[\w./-]+@[0-9a-f]{40}", use) and f"uses: {use}" in ci_text, use


def _same(ours: dict[str, Any], theirs: dict[str, Any]) -> None:
    theirs = {k: v for k, v in theirs.items() if k not in ("if", "id")}
    ours = {k: v for k, v in ours.items() if k not in ("if", "id")}
    if "save-if" in theirs.get("with", {}):
        theirs["with"] = {**theirs["with"], "save-if": False}
    assert {k: v for k, v in ours.items() if k != "name"} == {k: v for k, v in theirs.items() if k != "name"}


@pytest.mark.parametrize(("ours", "theirs"), [("rust", "checks"), ("playwright", "e2e")])
def test_setup_matches_ci(ours: str, theirs: str) -> None:
    flake, ci = _load(FLAKE)["jobs"][ours], _load(CI)["jobs"][theirs]
    assert flake["services"] == ci["services"]
    assert flake["env"].items() <= ci["env"].items()
    by_key = {s.get("name", s.get("uses")): s for s in ci["steps"]}
    by_uses = {s["uses"]: s for s in ci["steps"] if "uses" in s}
    setup = [s for s in flake["steps"] if not str(s.get("name", "")).startswith(("The ", "Migrations"))]
    assert len(setup) >= 7
    for step in setup:
        match = by_key.get(step.get("name", step.get("uses"))) or by_uses[step["uses"]]
        _same(step, match)


FAKE_CARGO = """#!/bin/bash
for a in "$@"; do [ "$a" = --list ] && exit 0; done
n=$(( $(cat "$COUNT" 2>/dev/null || echo 0) + 1 )); echo "$n" > "$COUNT"
case " $FAILS " in *" $n "*) echo "test result: FAILED. 0 passed; 1 failed"; exit 101;; esac
[ "$n" = "$EMPTY" ] && { echo "test result: ok. 0 passed; 0 failed"; exit 0; }
echo "test result: ok. 2 passed; 0 failed"
"""


@pytest.mark.parametrize(
    ("fails", "empty", "summary"),
    [("", "", "passed: 5, failed: 0"), ("2 4", "", "passed: 3, failed: 2"), ("", "5", "passed: 4, failed: 1")],
)
def test_rust_loop_runs_every_repeat_and_counts(fails: str, empty: str, summary: str, tmp_path: Path) -> None:
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    (bin_dir / "cargo").write_text(FAKE_CARGO, encoding="utf-8")
    (bin_dir / "cargo").chmod(0o755)
    env = {**RUST_OK, "REPEAT": "5", "COUNT": str(tmp_path / "n"), "FAILS": fails, "EMPTY": empty}
    env["PATH"] = f"{bin_dir}:{os.environ['PATH']}"
    result = _bash(_step("rust", "The named tests")["run"], env, tmp_path)
    assert (tmp_path / "n").read_text().strip() == "5"
    text = (tmp_path / "summary.md").read_text(encoding="utf-8")
    assert summary in text
    want = " ".join(sorted((fails + " " + empty).split())) or "none"
    assert f"Failing runs: {want}" in text
    assert (result.returncode == 0) is (want == "none")


def _counter() -> str:
    run = _step("playwright", "The spec")["run"]
    return run.split("<<'PY'\n", 1)[1].split("\nPY", 1)[0]


def _report(statuses: list[list[str]], errors: list[Any] | None = None) -> dict[str, Any]:
    """One spec per fake test; its `tests` list holds one entry per repeat, in run order."""
    specs = [
        {"file": "fake.spec.ts", "line": i, "title": f"t{i}", "tests": [{"projectName": "chromium", "status": s} for s in runs]}
        for i, runs in enumerate(statuses)
    ]
    return {"suites": [{"suites": [{"specs": specs}]}], "errors": errors or []}


@pytest.mark.parametrize(
    ("report", "rc", "ok", "failing"),
    [
        (_report([["expected"] * 3, ["expected"] * 3]), 0, True, "none"),
        (_report([["expected"] * 3]), 1, False, "none"),
        (_report([["expected"] * 3], errors=[{"message": "webserver teardown failed"}]), 0, False, "none"),
        (_report([["expected", "skipped", "expected"]]), 0, False, "2"),
        (_report([["expected", "expected", "unexpected"], ["flaky", "expected", "expected"]]), 1, False, "1 3"),
        (_report([["expected", "expected"]]), 0, False, "3"),
        (_report([]), 0, False, "none"),
        (None, 1, False, "none"),
    ],
)
def test_playwright_counter(report: Any, rc: int, ok: bool, failing: str, tmp_path: Path) -> None:
    path = tmp_path / "report.json"
    if report is not None:
        path.write_text(json.dumps(report), encoding="utf-8")
    env = {"PATH": os.environ["PATH"], "SPEC": PW_OK["SPEC"], "GITHUB_STEP_SUMMARY": str(tmp_path / "s.md")}
    result = subprocess.run(
        ["python3", "-", str(path), str(rc), "3"], input=_counter(), env=env, capture_output=True, text=True, check=False
    )
    text = (tmp_path / "s.md").read_text(encoding="utf-8")
    assert (result.returncode == 0) is ok, text
    assert ("PASSED" in text) is ok and f"Failing runs: {failing}" in text


def test_playwright_step_keeps_the_exit_code() -> None:
    run = _step("playwright", "The spec")["run"]
    assert "|| true" not in run
    assert re.search(r"rc=0\n.*npx playwright test [^\n]*--repeat-each \"\$REPEAT\" --retries 0[^\n]*\|\| rc=\$\?", run)
    assert 'python3 - "$PLAYWRIGHT_JSON_OUTPUT_NAME" "$rc" "$REPEAT"' in run
