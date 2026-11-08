"""The pre-hand-in gate (D-1300): scripts/prehandin.sh, the runner running main's copy of it on a handed-in
head (`GitHubEnv.prehandin` in `collect_result`), and `--after` holding a red hand-in for a fix round
(`hold_red_gate` in runner/__main__.py, `after_handoff`'s `hold`). Real git in tmp_path, no network, fake
data only.
"""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path
import pytest
import yaml

from runner import __main__ as entry, cycle, github_env as ge, state as st
from test_runner_live import (  # noqa: F401  # reason: `repo` is the shared fixture
    MODELS,
    NOW,
    TASK,
    git,
    live_config,
    make_env,
    repo,
    transcript,
    worker_commit,
)

ROOT = Path(__file__).resolve().parent.parent
GATE = ROOT / "scripts" / "prehandin.sh"

# A stand-in gate in the fake repository: red when the head holds RED, a non-verdict exit when it holds ODD.
FAKE_GATE = """#!/usr/bin/env bash
cd "${PREHANDIN_ROOT:-.}"
echo "args: $*"
if [ -f RED ]; then echo "check_pr_change: ERROR docs/k10/x.md: not owned"; echo "FAILED: check_pr_change"; exit 1; fi
if [ -f ODD ]; then echo "something broke"; exit 3; fi
echo "prehandin: green (quick)"
"""


def _gate_on_main(repo: dict[str, Path], text: str = FAKE_GATE) -> None:
    seed = repo["seed"]
    git(seed, "checkout", "-q", "main")
    git(seed, "pull", "-q", "origin", "main")
    (seed / "scripts").mkdir(exist_ok=True)
    (seed / "scripts" / "prehandin.sh").write_text(text)
    git(seed, "add", "-A")
    git(seed, "commit", "-qm", "gate")
    git(seed, "push", "-q", "origin", "main")


def _hand_in(repo: dict[str, Path], tmp_path: Path, marker: str | None) -> tuple[ge.GitHubEnv, cycle.WorkerResult]:
    env = make_env(repo["a"], tmp_path)
    env.read_state()
    worker_commit(repo["a"], TASK["branch"], "docs/k10/note.md")
    if marker:
        (repo["a"] / marker).write_text("x\n")
        git(repo["a"], "add", "-A")
        git(repo["a"], "commit", "-qm", marker)
    work, review = transcript(tmp_path, "w", MODELS["sonnet"]), transcript(tmp_path, "r", MODELS["opus"])
    return env, env.collect_result(TASK, "submitted", [work], [review])


# ---- the runner's hook


def test_a_green_gate_lets_the_hand_in_through_and_runs_mains_copy_on_the_head(repo: dict[str, Path], tmp_path: Path) -> None:
    _gate_on_main(repo)
    env, res = _hand_in(repo, tmp_path, None)
    assert res.outcome == "submitted" and res.log == ""
    line = next(row for row in env.log if row.startswith("pre-hand-in gate on"))
    assert line.endswith("exit 0; prehandin: green (quick)")
    assert git(repo["a"], "worktree", "list").count("\n") == 0, "the head's worktree is removed"


def test_a_red_gate_fails_the_hand_in_with_its_output_as_the_log(repo: dict[str, Path], tmp_path: Path) -> None:
    _gate_on_main(repo)
    env, res = _hand_in(repo, tmp_path, "RED")
    assert res.outcome == "tests_failed" and res.log.startswith(ge.PREHANDIN_RED)
    assert "FAILED: check_pr_change" in res.log and "--quick --base" in res.log and f"--branch {TASK['branch']}" in res.log
    assert res.head_sha and res.changed_paths, "the head and diff are still read, for the history entry"
    assert any(row.endswith("red; FAILED: check_pr_change") for row in env.log)


def test_a_gate_that_cannot_give_a_verdict_never_fails_the_work(repo: dict[str, Path], tmp_path: Path) -> None:
    """No gate on main yet, an exit other than 0 or 1, or a timeout: logged and let through."""
    env, res = _hand_in(repo, tmp_path, None)
    assert res.outcome == "submitted" and not any("pre-hand-in" in row for row in env.log)
    _gate_on_main(repo)
    env, res = _hand_in(repo, tmp_path, "ODD")
    assert res.outcome == "submitted" and any("exit 3; something broke (not a verdict: let through)" in r for r in env.log)

    def slow(args: list[str], stdin: str | None, cwd: Path) -> tuple[int, str, str]:
        if "--quick" in args:
            raise subprocess.TimeoutExpired(args, ge.PREHANDIN_TIMEOUT)
        return ge.run_command(args, stdin, cwd)

    env = make_env(repo["a"], tmp_path, run=slow)
    env.read_state()
    head = git(repo["a"], "rev-parse", "HEAD")
    assert env.prehandin(TASK["branch"], head, env.main_head()) == ""
    assert any(f"exit 124; timed out after {ge.PREHANDIN_TIMEOUT} seconds" in r for r in env.log)


def test_exit_3_from_the_gate_is_logged_and_let_through() -> None:
    env = ge.GitHubEnv(Path("."), [], Path("."), repo="o/r", starter="manual", runner_sha="x")
    assert env._gate_verdict("f" * 40, 3, "CANNOT CHECK: no merge base between origin/main and HEAD") == ""
    assert env.log[-1].endswith("exit 3; CANNOT CHECK: no merge base between origin/main and HEAD (not a verdict: let through)")


def test_the_gate_output_loses_every_key_phone_and_email_shape_pii_scan_knows() -> None:
    """Review NIT 4: pii_scan's patterns are applied to the captured output too, not only GitHub tokens."""
    env = ge.GitHubEnv(Path("."), [], Path("."), repo="o/r", starter="manual", runner_sha="x")
    # Assembled at run time, so this file itself holds no phone, email or key shape for pii_scan to find.
    phone, mail = "-".join(("928", "555", "2368")), "@".join(("jo.fake", "corp.invalid.zz"))
    found = f"docs/x.md:3: phone {phone} mail {mail} key sk_live_" + "A1" * 10 + " sk-ant-" + "b" * 24
    out = env._gate_verdict("f" * 40, 1, found + "\nFAILED: pii_scan")
    assert ge.GATE_REDACT and phone not in out and "jo.fake@" not in out and "sk_live_" not in out
    assert "sk-ant-" not in out and out.count("[redacted]") >= 4 and "docs/x.md:3" in out


def test_the_gate_output_is_redacted_and_capped() -> None:
    env = ge.GitHubEnv(Path("."), [], Path("."), repo="o/r", starter="manual", runner_sha="x")
    noisy = "\n".join([f"line {n} ghp_{'a' * 30}" for n in range(200)] + ["\x1b[31mFAILED: pii_scan\x1b[0m"])
    out = env._gate_verdict("f" * 40, 1, noisy)
    assert out.startswith(ge.PREHANDIN_RED) and out.endswith("FAILED: pii_scan") and "ghp_" not in out
    assert len(out) < ge.PREHANDIN_CAP + 100 and out.count("\n") <= ge.PREHANDIN_LINES
    assert env._gate_verdict("f" * 40, 0, "prehandin: green") == ""


def test_the_gate_gets_its_own_timeout_inside_the_after_call() -> None:
    assert ge._timeout_for(["env", "PREHANDIN_ROOT=/x", "bash", "/tmp/a/prehandin.sh", "--quick"]) == ge.PREHANDIN_TIMEOUT
    assert ge._timeout_for(["bash", "scripts/smoke.sh"]) == ge.SMOKE_TIMEOUT and ge._timeout_for(["git", "status"]) == 600
    assert ge.PREHANDIN_TIMEOUT + 120 < 600, "the session's Bash call is 600 seconds; --after must answer first"


# ---- --after holds a red hand-in for a fix round


def _red(log: str = f"{ge.PREHANDIN_RED} on abc: FAILED: pii_scan") -> cycle.WorkerResult:
    return cycle.WorkerResult("tests_failed", log=log)


def test_a_red_gate_is_held_twice_then_fails_and_a_held_run_writes_no_state(
    repo: dict[str, Path], tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(entry, "ROOT", tmp_path)
    (tmp_path / ".git").mkdir()
    (tmp_path / entry.HANDOFF).write_text(json.dumps({"session": "session-a", "task": "K10"}))
    assert [entry.hold_red_gate(_red()) for _ in range(3)] == [entry.PREHANDIN_HELD, entry.PREHANDIN_HELD, ""]
    kept = json.loads((tmp_path / entry.HANDOFF).read_text())
    assert kept["prehandin_holds"] == entry.PREHANDIN_HOLDS and "FAILED: pii_scan" in kept["prehandin_log"]
    assert entry.hold_red_gate(cycle.WorkerResult("tests_failed", log="worker said so")) == ""
    entry.close_handoff(entry.PREHANDIN_HELD)
    assert (tmp_path / entry.HANDOFF).exists(), "a held hand-in keeps the hand-off, so the safety step still frees the lease"

    env = make_env(repo["a"], tmp_path)
    env.update_state(lambda s: st.claim(st.with_task(s, "K10"), "K10", "session-a", NOW))
    before = git(repo["origin"], "rev-parse", "claude/kit-state")
    cfg = live_config("session-a", tmp_path, env)
    assert cycle.after_handoff(cfg, env, lambda t: _red(), lambda res: "prehandin_red") == "prehandin_red"
    assert git(repo["origin"], "rev-parse", "claude/kit-state") == before, "nothing written while held"
    assert st.status(env.read_state(), "K10") == st.IN_PROGRESS


def test_no_hand_off_file_means_no_hold(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """RUNNER_WORKER_CMD and the tests have no hand-off file: a red gate fails the attempt at once."""
    monkeypatch.setattr(entry, "ROOT", tmp_path)
    assert entry.hold_red_gate(_red()) == ""


# ---- the script itself


def test_the_gate_parses_runs_the_ci_checks_and_matches_cis_path_rules() -> None:
    text = GATE.read_text(encoding="utf-8")
    assert subprocess.run(["bash", "-n", str(GATE)], check=False).returncode == 0
    for needed in ("check_plan_lock", "pii_scan", "check_rust_rules", "check_crates", "check_code_docs", "check_ai_smell",
                   "check_copy", "trace_spec", "validate_tasks", "gen_prompts.py --check",
                   'check_pr_change.py --base "${BASE}" --branch "${BRANCH}"', "npm audit --audit-level=low",
                   "gitleaks dir . --no-banner --redact --verbose --exit-code 1"):
        assert needed in text, needed
    ci = yaml.safe_load((ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8"))
    steps = {s.get("name", ""): str(s.get("run", "")) for s in ci["jobs"]["checks"]["steps"]}
    touches = steps["What this change touches (compile only when code changed, D-489)"]
    ci_rules = re.findall(r"grep -Eq '([^']+)'", touches)
    gate_rules = [re.search(rf"^{name}='([^']+)'", text, re.MULTILINE).group(1) for name in ("RUST_PATHS", "NPM_PATHS")]  # type: ignore[union-attr]  # reason: asserted present above
    assert gate_rules == ci_rules, "the gate decides 'Rust changed' and 'npm changed' exactly as Ci does"
    assert "npm audit --audit-level=low" in steps["TypeScript checks and npm audit"]


def _git_repo(tmp_path: Path) -> Path:
    """A tiny repository carrying only what the quick part reads: origin/main plus one branch commit."""
    work = tmp_path / "w"
    work.mkdir()
    git(work, "init", "-q", "-b", "main")
    git(work, "config", "user.name", "Test")
    git(work, "config", "user.email", "test@example.com")
    (work / "a.txt").write_text("a\n")
    git(work, "add", "-A")
    git(work, "commit", "-qm", "main")
    git(work, "update-ref", "refs/remotes/origin/main", "HEAD")
    git(work, "switch", "-q", "-c", "claude/task/T999-fake")
    return work


def test_the_gate_stops_at_the_first_red_check_with_its_name(tmp_path: Path) -> None:
    work = _git_repo(tmp_path)
    (work / "kit").mkdir()
    (work / "sources").mkdir()
    (work / "sources" / "s.md").write_text("fake source\n")
    (work / "kit" / "sources.sha256").write_text(subprocess.run(["sha256sum", "sources/s.md"], cwd=work, capture_output=True,
                                                                 text=True, check=True).stdout)
    (work / "sources" / "s.md").write_text("changed\n")
    done = subprocess.run(["bash", str(GATE), "--quick", "--base", "origin/main"], cwd=work, capture_output=True, text=True,
                          env={"PATH": "/usr/bin:/bin", "PREHANDIN_ROOT": str(work), "HOME": str(tmp_path)}, check=False)
    assert done.returncode == 1 and "FAILED: sources/ differs from kit/sources.sha256" in done.stdout


def _run_gate(work: Path, tmp_path: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["bash", str(GATE), *args], cwd=work, capture_output=True, text=True,
                          env={"PATH": "/usr/bin:/bin", "PREHANDIN_ROOT": str(work), "HOME": str(tmp_path)}, check=False)


def test_a_git_problem_exits_3_never_the_red_1(tmp_path: Path) -> None:
    """Review NIT 5: no such base or no merge base is not a verdict on the work, so it is never exit 1."""
    work = _git_repo(tmp_path)
    bad = _run_gate(work, tmp_path, "--quick", "--base", "nowhere")
    assert bad.returncode == 3 and "CANNOT CHECK: base nowhere is not a commit here" in bad.stdout
    git(work, "checkout", "-q", "--orphan", "claude/task/T998-unrelated")
    git(work, "commit", "-qm", "unrelated history")
    lone = _run_gate(work, tmp_path, "--quick", "--base", "origin/main")
    assert lone.returncode == 3 and "CANNOT CHECK: no merge base" in lone.stdout


def test_a_check_script_the_head_lacks_is_skipped_not_failed(tmp_path: Path) -> None:
    """Review SHOULD-FIX 1: main's gate on a head made before main added a check script skips that check
    (as Ci on the branch would) instead of failing the work; the checks the head has still run."""
    work = _git_repo(tmp_path)
    (work / "kit").mkdir()
    (work / "sources").mkdir()
    (work / "sources" / "s.md").write_text("fake source\n")
    (work / "kit" / "sources.sha256").write_text(subprocess.run(["sha256sum", "sources/s.md"], cwd=work, capture_output=True,
                                                                 text=True, check=True).stdout)
    (work / "scripts").mkdir()
    for name in ("pii_scan", "trace_spec", "validate_tasks", "check_catalog", "check_legal_docs", "check_ai_smell",
                 "check_copy", "check_code_docs", "check_rust_rules", "check_plan_lock", "gen_prompts", "check_pr_change"):
        (work / "scripts" / f"{name}.py").write_text("print('fake check: green')\n")
    git(work, "add", "-A")
    git(work, "commit", "-qm", "head without check_crates")
    done = _run_gate(work, tmp_path, "--quick", "--base", "origin/main")
    assert done.returncode == 0, done.stdout
    assert "skipped: scripts/check_crates.py not on this head" in done.stdout and "ok  pii_scan" in done.stdout
    (work / "scripts" / "check_copy.py").write_text("raise SystemExit('fake copy problem')\n")
    red = _run_gate(work, tmp_path, "--quick", "--base", "origin/main")
    assert red.returncode == 1 and "FAILED: check_copy" in red.stdout and "fake copy problem" in red.stdout


CALLS = [("kit/prompts/TEMPLATE.md", "prehandin: green"), (".github/workflows/backup-runner.yml", "runner: prehandin_red")]


@pytest.mark.parametrize(("path", "marker"), CALLS)
def test_the_worker_is_told_to_run_the_gate_last(path: str, marker: str) -> None:
    text = (ROOT / path).read_text(encoding="utf-8")
    assert "bash scripts/prehandin.sh" in text and marker in text


# ---- per-crate coverage (D-1440)

COVERAGE = ROOT / "scripts" / "coverage_floor.py"


def _load_coverage_floor():  # type: ignore[no-untyped-def]  # reason: a module loaded by path
    import importlib.util

    spec = importlib.util.spec_from_file_location("coverage_floor", COVERAGE)
    module = importlib.util.module_from_spec(spec)  # type: ignore[arg-type]  # reason: the path exists
    spec.loader.exec_module(module)  # type: ignore[union-attr]  # reason: as above
    return module


def _fake_workspace(tmp_path: Path) -> Path:
    """Fake crates: core <- pay <- glue (tests), lic (floored, nobody depends on it), the excluded askhava-security."""
    root = tmp_path / "ws"
    crates = {"app/core": ("fake-core", []), "app/pay": ("fake-pay", ["fake-core"]), "app/lic": ("fake-lic", []),
              "tests/glue": ("fake-glue", ["fake-pay"]), "tests/security": ("askhava-security", ["fake-pay"])}
    for folder, (name, deps) in crates.items():
        (root / folder).mkdir(parents=True)
        lines = [f'[package]\nname = "{name}"\n', "[dev-dependencies]\n"] + [f"{d} = {{ workspace = true }}\n" for d in deps]
        (root / folder / "Cargo.toml").write_text("".join(lines) + 'serde = "1"\n')
    (root / "Cargo.toml").write_text('[workspace]\nmembers = ["app/*", "tests/glue", "tests/security"]\n')
    (root / "app" / "lic" / "src").mkdir()
    (root / "app" / "lic" / "src" / "rules.rs").write_text("//! fake\n")
    (root / "tests" / "coverage_floor.json").write_text(json.dumps({"total": 0.0, "floors": {
        "app/pay/": 95.0, "app/lic/src/rules.rs": 95.0, "app/core/src/fees.rs": 95.0}}))
    return root


def test_only_floored_crates_the_change_touches_are_measured_with_their_dependents(tmp_path: Path) -> None:
    cf, root = _load_coverage_floor(), _fake_workspace(tmp_path)
    assert cf.select(root, ["app/core/src/lib.rs", "docs/x.md"]) == ([], []), \
        "a floor with no file yet (app/core/src/fees.rs) is never measured, even when its crate changes"
    assert cf.select(root, ["app/pay/src/fee.rs"]) == (["fake-glue", "fake-pay"], ["app/pay/"]), \
        "a crate's dependents carry its coverage; Ci's excluded crates stay out"
    assert cf.select(root, ["app/lic/Cargo.toml"]) == (["fake-lic"], ["app/lic/src/rules.rs"])
    both = cf.select(root, ["tests/coverage_floor.json"])
    assert both == (["fake-glue", "fake-lic", "fake-pay"], ["app/pay/", "app/lic/src/rules.rs"]), "a floor change measures all"


def _report(tmp_path: Path, root: Path, files: dict[str, tuple[int, int]]) -> Path:
    rows = [{"filename": f"{root}/{name}", "summary": {"branches": {"count": count, "covered": covered},
                                                         "lines": {"count": 10, "covered": 10}}}
            for name, (covered, count) in files.items()]
    total = {"branches": {"count": sum(c for _, c in files.values()), "covered": sum(v for v, _ in files.values())},
             "lines": {"count": 10, "covered": 10}}
    path = tmp_path / "report.json"
    path.write_text(json.dumps({"data": [{"files": rows, "totals": total}]}))
    return path


def test_the_comparison_judges_floors_as_ci_did_and_only_the_named_ones_per_crate(tmp_path: Path) -> None:
    cf, root = _load_coverage_floor(), _fake_workspace(tmp_path)
    floors = json.loads((root / "tests" / "coverage_floor.json").read_text())
    data = json.loads(_report(tmp_path, root, {"app/pay/src/a.rs": (95, 100), "app/lic/src/rules.rs": (90, 100)})
                      .read_text())["data"][0]
    assert cf.judge(floors, data, f"{root}/") == ["app/lic/src/rules.rs 90.00 is below the floor 95.0"]
    assert cf.judge(floors, data, f"{root}/", ["app/pay/"]) == [], "a per-crate run judges only its own floors"
    shy = json.loads(_report(tmp_path, root, {"app/pay/src/a.rs": (94995, 100000)}).read_text())["data"][0]
    assert cf.judge(floors, shy, f"{root}/", ["app/pay/"]) == [], "Ci's 0.005 rounding slack is kept"
    lines_only = {"files": [{"filename": f"{root}/app/pay/src/a.rs", "summary": {
        "branches": {"count": 0, "covered": 0}, "lines": {"count": 10, "covered": 9}}}], "totals": data["totals"]}
    assert cf.judge(floors, lines_only, f"{root}/", ["app/pay/"]) == ["app/pay/ 90.00 is below the floor 95.0"]


def test_ci_and_the_gate_measure_and_compare_alike() -> None:
    """Ci's Coverage job calls scripts/coverage_floor.py (no inline copy left to drift); the gate uses the same
    nightly and flags, and backup-runner.yml installs Ci's pinned tools for it (D-1440)."""
    ci = yaml.safe_load((ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8"))["jobs"]["coverage"]
    steps = {s.get("name", ""): str(s.get("run", "")) for s in ci["steps"]}
    compare = steps["Compare with tests/coverage_floor.json and refuse a lowered floor (D-694)"]
    assert 'python3 scripts/coverage_floor.py compare "$COVERAGE_JSON"' in compare and "def percent" not in compare
    assert "scripts/coverage_floor\\.py" in steps["Does this change touch Rust? (same rule as Checks, D-489)"]
    measure = steps["Branch coverage (cargo llvm-cov on the pinned nightly)"]
    gate = GATE.read_text(encoding="utf-8")
    for flag in ("--locked --branch --json --summary-only", "--ignore-filename-regex '(/tests/|/bin/)'"):
        assert flag in measure and flag in gate, flag
    excluded = re.findall(r"--exclude (\S+)", measure)
    assert tuple(excluded) == _load_coverage_floor().EXCLUDED
    assert f'NIGHTLY="{ci["env"]["NIGHTLY"]}"' in gate
    backup = yaml.safe_load((ROOT / ".github/workflows/backup-runner.yml").read_text(encoding="utf-8"))["jobs"]["backup"]
    for pin in ("NIGHTLY", "LLVM_COV_URL", "LLVM_COV_SHA256"):
        assert backup["env"][pin] == ci["env"][pin], pin
    assert "coverage\n" in gate.split("extras() {", 1)[1].split("\n}\n", 1)[0], "full mode runs it; --quick does not"


def test_the_gate_skips_coverage_it_cannot_measure_and_never_calls_that_red(tmp_path: Path) -> None:
    """The gate's own `coverage` function on a branch touching a floored fake crate, with no cargo-llvm-cov on
    PATH: it names the floor, says it skipped and goes on (the quick checks are left out of this probe)."""
    work = _git_repo(tmp_path)
    root = _fake_workspace(tmp_path)
    for item in ("Cargo.toml", "app", "tests"):
        subprocess.run(["cp", "-r", str(root / item), str(work)], check=True)
    (work / "Cargo.lock").write_text("# fake\n")
    (work / "scripts").mkdir()
    subprocess.run(["cp", str(COVERAGE), str(work / "scripts")], check=True)
    git(work, "add", "-A")
    git(work, "commit", "-qm", "touch a floored crate")
    body = GATE.read_text(encoding="utf-8").split("\nquick_part\n", 1)[0]
    probe = tmp_path / "probe.sh"
    probe.write_text(body.replace("set -euo pipefail", "set -uo pipefail") + "\ncoverage\necho reached-end\n")
    done = subprocess.run(
        ["bash", str(probe), "--base", "origin/main"], cwd=work, capture_output=True, text=True,
        env={"PATH": "/usr/bin:/bin", "PREHANDIN_ROOT": str(work), "HOME": str(tmp_path)}, check=False)
    assert "branch coverage of app/pay/" in done.stdout and "skipped: needs cargo-llvm-cov" in done.stdout, done.stdout
    assert "reached-end" in done.stdout and done.returncode == 0


RETRY = "curl -sSfL --retry 5 --retry-all-errors --retry-delay 15 --connect-timeout 20 -o "


def _workflow_runs(name: str) -> dict[str, str]:
    jobs = yaml.safe_load((ROOT / ".github/workflows" / name).read_text(encoding="utf-8"))["jobs"]
    return {f"{job}: {s.get('name', '')}": str(s.get("run", "")) for job, body in jobs.items() for s in body["steps"]}


def test_every_release_download_retries_and_only_the_gates_coverage_tools_are_best_effort() -> None:
    """D-1986: a release-CDN 504 on one curl stopped every backup run for over an hour. Each release download in
    ci.yml and backup-runner.yml retries; the backup job's cargo-llvm-cov and nightly are best effort (the gate
    then says it skipped), while gitleaks there and every Ci download stay required."""
    runs = {**_workflow_runs("ci.yml"), **_workflow_runs("backup-runner.yml")}
    curls = [line.strip() for run in runs.values() for line in run.splitlines() if "curl -sSfL" in line]
    release = [c for c in curls if "gitleaks.tgz" in c or "llvm-cov.tgz" in c or "$tool.tgz" in c]
    assert len(release) == 5, release
    assert all(c.startswith(RETRY) or f"|| {RETRY}" in c for c in release), release
    prereq = runs["backup: Smoke-test prerequisites (what Ci installs for itself)"]
    gitleaks = prereq.split("done\n", 1)[1].split("# Ci's measuring", 1)[0]
    assert "gitleaks.tgz" in gitleaks and "sha256sum -c -" in gitleaks and "||" not in gitleaks, "gitleaks stays required"
    assert "prehandin coverage will report skipped (D-1986)" in prereq
    strict = [s for s, run in runs.items() if s.startswith(("checks:", "coverage:")) and ".tgz" in run]
    assert len(strict) == 3 and all("||" not in runs[s] for s in strict), "Ci's downloads stay strict"


@pytest.mark.parametrize("fail", ["curl", "sha256sum", ""])
def test_the_backup_jobs_coverage_install_is_skipped_not_fatal(tmp_path: Path, fail: str) -> None:
    """The prerequisites step's coverage part under Actions' `bash -e`, with stub tools: a failed download or
    checksum prints why, leaves cargo-llvm-cov off GITHUB_PATH and exits 0; a good one puts it there."""
    prereq = _workflow_runs("backup-runner.yml")["backup: Smoke-test prerequisites (what Ci installs for itself)"]
    part = "# Ci's measuring" + prereq.split("# Ci's measuring", 1)[1]
    stubs = tmp_path / "bin"
    stubs.mkdir()
    for tool in ("rustup", "curl", "sha256sum", "tar", "sudo", "df"):
        (stubs / tool).write_text(f"#!/bin/sh\n{'exit 22' if tool == fail else 'exit 0'}\n")
        (stubs / tool).chmod(0o755)
    (tmp_path / "tests").mkdir()
    (tmp_path / "tests" / "coverage_floor.json").write_text("{}\n")
    script = tmp_path / "step.sh"
    script.write_text(part)
    env = {"PATH": f"{stubs}:/usr/bin:/bin", "RUNNER_TEMP": str(tmp_path), "NIGHTLY": "nightly-2026-01-01",
           "LLVM_COV_URL": "https://example.invalid/x.tgz", "LLVM_COV_SHA256": "0" * 64,
           "GITHUB_PATH": str(tmp_path / "path"), "GITHUB_ENV": str(tmp_path / "env")}
    done = subprocess.run(["bash", "-e", str(script)], cwd=tmp_path, env=env, capture_output=True, text=True,
                          check=False)
    assert done.returncode == 0, done.stderr
    on_path = (tmp_path / "path").read_text() if (tmp_path / "path").exists() else ""
    if fail:
        assert "cargo-llvm-cov not installed: " in done.stdout and "prehandin coverage will report skipped" in done.stdout
        assert "llvm-cov" not in on_path
    else:
        assert "not installed" not in done.stdout and f"{tmp_path}/llvm-cov" in on_path


GITLEAKS_MODULE = "github.com/zricethezav/gitleaks/v8"
GITLEAKS_STEPS = {"ci.yml": "checks: Secret scan (gitleaks, working tree)",
                  "backup-runner.yml": "backup: Smoke-test prerequisites (what Ci installs for itself)"}


def _gitleaks_block(workflow: str) -> str:
    """The part of the workflow step that installs gitleaks, up to (not including) what runs after it."""
    run = _workflow_runs(workflow)[GITLEAKS_STEPS[workflow]]
    if workflow == "backup-runner.yml":
        return run.split("done\n", 1)[1].split("# Ci's measuring", 1)[0]
    return run.split("# --verbose", 1)[0]


@pytest.mark.parametrize("workflow", sorted(GITLEAKS_STEPS))
def test_gitleaks_falls_back_to_the_go_module_proxy_and_stays_required(workflow: str) -> None:
    """D-1986: the gitleaks release asset gave 504s for hours, so Ci and the backup job build the same pinned version
    from proxy.golang.org (go.sum checked by sum.golang.org) when the release download or its SHA-256 check fails.
    The checksum database is never switched off, the binary must report the pinned version, and the block is the
    same in both workflows."""
    block = _gitleaks_block(workflow)
    on_path = 'echo "$RUNNER_TEMP/gitleaks" >> "$GITHUB_PATH"\n'
    assert block.removesuffix(on_path) == _gitleaks_block("ci.yml"), "one install block in both workflows"
    assert f"{RETRY}\"$RUNNER_TEMP/gitleaks.tgz\"" in block
    assert '&& echo "${GITLEAKS_SHA256}  $RUNNER_TEMP/gitleaks.tgz" | sha256sum -c - \\\n' in block
    assert block.index("sha256sum -c -") < block.index("&& tar -xzf \"$RUNNER_TEMP/gitleaks.tgz\""), "checked, then extracted"
    go = block.split("else\n", 1)[1]
    for needed in ('GOBIN="$RUNNER_TEMP/gitleaks"', "GOPROXY=https://proxy.golang.org", "GOSUMDB=sum.golang.org",
                   f'go install -ldflags "-X={GITLEAKS_MODULE}/cmd.Version=${{GITLEAKS_VERSION}}"',
                   f'"{GITLEAKS_MODULE}@v${{GITLEAKS_VERSION}}"', 'go version -m "$RUNNER_TEMP/gitleaks/gitleaks"',
                   f'[ "$built" != "{GITLEAKS_MODULE}@v${{GITLEAKS_VERSION}}" ]'):
        assert needed in go, needed
    for banned in ("GONOSUMDB", "GONOSUMCHECK", "GOINSECURE", "GOPRIVATE", "GOSUMDB=off", "-insecure", "GOFLAGS=-insecure",
                   "|| true", "||"):
        assert banned not in block, banned
    assert '"$RUNNER_TEMP/gitleaks/gitleaks" version' in block and '*"${GITLEAKS_VERSION}"*)' in block
    assert "gitleaks source: the v${GITLEAKS_VERSION} release tarball" in block and "built from proxy.golang.org" in block


def _gitleaks_stubs(bin_dir: Path, case: str) -> None:
    """curl, sha256sum, tar and go stand-ins that log their calls; tar and `go install` leave a stub gitleaks."""
    fake_gitleaks = "#!/bin/sh\necho \"${FAKE_REPORTS:-8.28.0}\"\n"
    tools = {
        "curl": "exit 22" if case in ("curl", "both") else 'printf partial > "$RUNNER_TEMP/gitleaks.tgz"',
        "sha256sum": "cat >/dev/null; exit 1" if case == "sha" else "cat >/dev/null",
        "tar": f"printf '%s' '{fake_gitleaks}' > \"$RUNNER_TEMP/gitleaks/gitleaks\"; "
               "chmod +x \"$RUNNER_TEMP/gitleaks/gitleaks\"",
        "go": ('if [ "$1" = install ]; then echo "GOPROXY=$GOPROXY GOSUMDB=$GOSUMDB GOBIN=$GOBIN" >> "$RUNNER_TEMP/calls"; '
               + ("exit 1; " if case == "both" else "")
               + f"printf '%s' '{fake_gitleaks}' > \"$GOBIN/gitleaks\"; chmod +x \"$GOBIN/gitleaks\"; exit 0; fi\n"
               + 'printf "\\tpath\\t%s\\n\\tmod\\t%s\\tv%s\\th1:fake=\\n" '
               + f'{GITLEAKS_MODULE} {GITLEAKS_MODULE} "${{FAKE_BUILT:-8.28.0}}"'),
    }
    for tool, body in tools.items():
        (bin_dir / tool).write_text(f'#!/bin/sh\necho "{tool} $*" >> "$RUNNER_TEMP/calls"\n{body}\n')
        (bin_dir / tool).chmod(0o755)


@pytest.mark.parametrize("workflow", sorted(GITLEAKS_STEPS))
@pytest.mark.parametrize(("case", "env", "source"), [
    ("", {}, "release"), ("curl", {}, "go"), ("sha", {}, "go"), ("both", {}, None),
    ("", {"FAKE_REPORTS": "8.27.9"}, None), ("curl", {"FAKE_BUILT": "8.27.9"}, None)])
def test_the_gitleaks_install_runs_under_actions_bash(tmp_path: Path, workflow: str, case: str,
                                                      env: dict[str, str], source: str | None) -> None:
    """The install block under `bash -eo pipefail` with stub tools: a good release is used as is; a failed download or
    SHA-256 check falls back to `go install` (and never extracts the tarball); both sources failing, or a binary
    that is not the pinned version, fails the step."""
    stubs = tmp_path / "bin"
    stubs.mkdir()
    _gitleaks_stubs(stubs, case)
    script = tmp_path / "step.sh"
    script.write_text(_gitleaks_block(workflow))
    run_env = {"PATH": f"{stubs}:/usr/bin:/bin", "RUNNER_TEMP": str(tmp_path), "GITLEAKS_VERSION": "8.28.0",
               "GITLEAKS_SHA256": "0" * 64, "GITHUB_PATH": str(tmp_path / "path"), **env}
    done = subprocess.run(["bash", "-eo", "pipefail", str(script)], cwd=tmp_path, env=run_env, capture_output=True,
                          text=True, check=False)
    calls = (tmp_path / "calls").read_text()
    if source is None:
        assert done.returncode != 0, done.stdout
        assert "gitleaks reports version" not in done.stdout
        return
    assert done.returncode == 0, done.stdout + done.stderr
    assert "gitleaks reports version 8.28.0" in done.stdout
    if source == "release":
        assert "release tarball (SHA-256 verified)" in done.stdout and "go " not in calls
    else:
        assert f"{GITLEAKS_MODULE}@v8.28.0 built from proxy.golang.org" in done.stdout
        assert f"GOPROXY=https://proxy.golang.org GOSUMDB=sum.golang.org GOBIN={tmp_path}/gitleaks" in calls
        assert "tar " not in calls, "a tarball that failed its download or check is never extracted"
    if workflow == "backup-runner.yml":
        assert (tmp_path / "path").read_text() == f"{tmp_path}/gitleaks\n"
