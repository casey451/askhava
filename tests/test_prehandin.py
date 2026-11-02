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
