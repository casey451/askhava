"""The update merge of stage C (D-620) over real local git repositories: runner/update_merge.py and
GitHubEnv.update_branch / update_merge_of / reviewed_plus_main, with a bare `origin` in tmp_path and no
network (the `repo` fixture and helpers of tests/test_runner_live.py). Fake data only.
"""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import check_plan_lock
import check_pr_change
import pytest

from runner import github_env as ge
from runner.github_env import GitHubEnv
from runner.update_merge import decisions_after_main
from test_runner_live import (  # noqa: F401  # reason: `repo` is the shared fixture
    SESSION, TASK, AutoMergeRunner, OfflineGh, am_env, claimed_env, commit_on, git, make_env, move_main, pushed_attempt, repo,
)

ROOT = Path(__file__).resolve().parent.parent
BRANCH = TASK["branch"]
BASE_LOG = "# Decision log\n\n- **D-001** Question: a fake first entry.\n"


def env_on(repo: dict[str, Path], tmp_path: Path, name: str = "b"):  # type: ignore[no-untyped-def]
    """A runner on clone `name`, its working tree on the latest main (the runner's checkout)."""
    git(repo[name], "fetch", "-q", "origin")
    git(repo[name], "checkout", "-q", "-B", "main", "origin/main")
    return make_env(repo[name], tmp_path, name, run=OfflineGh([]))


def parents(root: Path, sha: str) -> list[str]:
    return git(root, "rev-list", "--parents", "-n", "1", sha).split()[1:]


def push_main(repo: dict[str, Path], path: str, text: str) -> str:
    head = commit_on(repo["seed"], "main", "origin/main", path, text)
    git(repo["seed"], "push", "-q", "origin", "main")
    return head


def push_branch(repo: dict[str, Path], path: str, text: str, start: str = "origin/main") -> str:
    head = commit_on(repo["a"], BRANCH, start, path, text)
    git(repo["a"], "push", "-q", "origin", BRANCH)
    return head


# ---- docs/decisions.md: main's entries first, the branch's after (pure)


def test_decisions_after_main_appends_only_what_the_branch_appended() -> None:
    ours, theirs = BASE_LOG + "- **D-010** branch\n", BASE_LOG + "- **D-005** main\n"
    assert decisions_after_main(BASE_LOG, ours, theirs) == BASE_LOG + "- **D-005** main\n- **D-010** branch\n"
    assert decisions_after_main(BASE_LOG, BASE_LOG, theirs) == theirs
    assert decisions_after_main(BASE_LOG, BASE_LOG.replace("fake first", "edited") + "- **D-010** b\n", theirs) is None
    assert decisions_after_main(BASE_LOG, BASE_LOG + "- **D-005** also five\n", theirs) is None  # main took the number
    assert decisions_after_main("x", "x\n- **D-010** b\n", "x\n- **D-005** m") == "x\n- **D-005** m\n- **D-010** b\n"


# ---- update_branch over real git


def test_update_branch_merges_main_into_the_reviewed_head_and_pushes_a_fast_forward(repo: dict[str, Path], tmp_path: Path) -> None:
    reviewed = pushed_attempt(repo)
    main2 = move_main(repo, "fake repo, main moved\n")
    env = env_on(repo, tmp_path)
    done = env.update_branch(BRANCH, reviewed)
    sha = done["sha"]
    assert done == {"status": "pushed", "sha": sha, "main": main2}
    assert git(repo["origin"], "rev-parse", BRANCH) == sha and parents(repo["b"], sha) == [reviewed, main2]
    assert git(repo["b"], "rev-parse", f"{sha}^{{tree}}") == git(repo["b"], "merge-tree", "--write-tree", reviewed, main2).splitlines()[0]
    assert env.updates.is_update_merge(reviewed, sha) == (True, main2)
    assert env.reviewed_plus_main(reviewed, sha, 0) is True and env.update_merge_of(reviewed, sha, 0) is True
    assert env.update_branch(BRANCH, sha) == {"status": "current"}
    assert env.update_branch(BRANCH, reviewed)["status"] == "moved"  # origin's branch is no longer that head
    other = env_on(repo, tmp_path, "a")
    assert other.updates.commit(reviewed, main2, BRANCH)[0] == sha  # deterministic: a second runner makes the same commit


def test_update_merge_puts_mains_decisions_first_whatever_the_union_driver_does(repo: dict[str, Path], tmp_path: Path) -> None:
    push_main(repo, ".gitattributes", "docs/decisions.md merge=union\n")
    push_main(repo, "docs/decisions.md", BASE_LOG)
    reviewed = push_branch(repo, "docs/decisions.md", BASE_LOG + "- **D-010** Question: the branch's entry.\n")
    main2 = push_main(repo, "docs/decisions.md", BASE_LOG + "- **D-005** Question: main's entry.\n")
    env = env_on(repo, tmp_path)
    raw = subprocess.run(["git", "merge-tree", "--write-tree", reviewed, main2], cwd=repo["b"], capture_output=True, text=True, check=False)
    union = git(repo["b"], "show", f"{raw.stdout.splitlines()[0]}:docs/decisions.md")
    # git alone either conflicts (no union attribute seen) or, with the union driver, puts the branch first.
    assert raw.returncode == 1 or union.index("D-010") < union.index("D-005")
    done = env.update_branch(BRANCH, reviewed)
    merged = git(repo["b"], "show", f"{done['sha']}:docs/decisions.md") + "\n"
    assert merged == BASE_LOG + "- **D-005** Question: main's entry.\n- **D-010** Question: the branch's entry.\n"
    main_text = git(repo["b"], "show", f"{main2}:docs/decisions.md") + "\n"
    assert check_pr_change.decisions_append_only(main_text, merged) == [] and check_pr_change.decisions_unique(merged) == []
    assert env.reviewed_plus_main(reviewed, done["sha"], 0) is True
    a = repo["a"]  # a person's local merge with the union driver: the branch's entry before main's
    git(a, "fetch", "-q", "origin")
    git(a, "checkout", "-q", "-B", "union-merge", reviewed)
    git(a, "merge", "-q", "--no-edit", "origin/main")
    local = git(a, "show", "HEAD:docs/decisions.md")
    assert local.index("D-010") < local.index("D-005")
    git(a, "push", "-q", "origin", "union-merge")
    git(repo["b"], "fetch", "-q", "origin", "union-merge")
    assert env.reviewed_plus_main(reviewed, git(a, "rev-parse", "HEAD"), 0) is False


def test_a_reused_decision_number_or_a_real_conflict_is_reported_and_nothing_is_pushed(repo: dict[str, Path], tmp_path: Path) -> None:
    push_main(repo, "docs/decisions.md", BASE_LOG)
    reviewed = push_branch(repo, "docs/decisions.md", BASE_LOG + "- **D-005** Question: the branch took five.\n")
    push_main(repo, "docs/decisions.md", BASE_LOG + "- **D-005** Question: main took five.\n")
    env = env_on(repo, tmp_path)
    assert env.update_branch(BRANCH, reviewed)["paths"] == ["docs/decisions.md"]
    readme = push_branch(repo, "README.md", "the branch's readme\n", start=BRANCH)
    push_main(repo, "README.md", "main's readme\n")
    done = env.update_branch(BRANCH, readme)
    assert done["status"] == "conflict" and done["paths"] == ["README.md", "docs/decisions.md"]
    assert git(repo["origin"], "rev-parse", BRANCH) == readme


def test_an_evil_merge_or_a_merge_of_something_not_on_main_is_not_an_update_merge(repo: dict[str, Path], tmp_path: Path) -> None:
    reviewed = pushed_attempt(repo)
    move_main(repo, "fake repo, main moved\n")
    env = env_on(repo, tmp_path)
    a = repo["a"]
    git(a, "fetch", "-q", "origin")
    git(a, "checkout", "-q", "-B", "evil", reviewed)
    git(a, "merge", "-q", "--no-ff", "--no-commit", "origin/main")
    (a / "docs/k10/evil.md").write_text("added inside the merge commit\n")
    git(a, "add", "-A")
    git(a, "commit", "-qm", "Merge main (with an extra file)")
    git(a, "push", "-q", "origin", "evil")
    evil = git(a, "rev-parse", "HEAD")
    git(repo["b"], "fetch", "-q", "origin", "evil")
    assert env.update_merge_of(reviewed, evil, 0) is False and env.reviewed_plus_main(reviewed, evil, 0) is False
    side = commit_on(a, "side", reviewed, "docs/k10/side.md", "not on main\n")
    git(a, "checkout", "-q", "-B", "merged-side", reviewed)
    git(a, "merge", "-q", "--no-ff", "--no-edit", side)
    git(a, "push", "-q", "origin", "merged-side")
    git(repo["b"], "fetch", "-q", "origin", "merged-side")
    assert env.update_merge_of(reviewed, git(a, "rev-parse", "HEAD"), 0) is False  # its second parent is not on main
    assert env.update_merge_of(reviewed, "f" * 40, 0) is None


# ---- kit/plan.lock: recomputed when both sides rewrote it, never over a stale lock


def _locked_main(repo: dict[str, Path]) -> None:
    """main carries the real check_plan_lock.py, two locked files and a lock that matches them."""
    seed = repo["seed"]
    git(seed, "fetch", "-q", "origin")
    git(seed, "checkout", "-q", "-B", "main", "origin/main")
    (seed / "scripts").mkdir(exist_ok=True)
    (seed / "kit").mkdir(exist_ok=True)
    shutil.copy(ROOT / "scripts" / "check_plan_lock.py", seed / "scripts" / "check_plan_lock.py")
    (seed / "BUILD_BRIEF.md").write_text("brief, first version\n")
    (seed / "CLAUDE.md").write_text("rules, first version\n")
    check_plan_lock.write_lock(seed)
    git(seed, "add", "-A")
    git(seed, "commit", "-qm", "locked blueprint")
    git(seed, "push", "-q", "origin", "main")


def _relocked(root: Path, branch: str | None, path: str, text: str, lock: bool = True) -> str:
    git(root, "fetch", "-q", "origin")
    git(root, "checkout", "-q", "-B", branch or "main", "origin/main")
    (root / path).write_text(text)
    if lock:
        check_plan_lock.write_lock(root)
    git(root, "add", "-A")
    git(root, "commit", "-qm", f"change {path}")
    git(root, "push", "-q", "origin", branch or "main")
    return git(root, "rev-parse", "HEAD")


def _lock_ok(root: Path, sha: str, tmp: Path) -> list[str]:
    tree = tmp / f"tree-{sha[:8]}"
    git(root, "worktree", "add", "-q", "--detach", str(tree), sha)
    try:
        return check_plan_lock.differences(tree)
    finally:
        git(root, "worktree", "remove", "--force", str(tree))


def test_both_sides_rewrote_the_plan_lock_and_the_merge_recomputes_it(repo: dict[str, Path], tmp_path: Path) -> None:
    _locked_main(repo)
    reviewed = _relocked(repo["a"], BRANCH, "BUILD_BRIEF.md", "brief, the branch's version\n")
    main2 = _relocked(repo["seed"], None, "CLAUDE.md", "rules, main's version\n")
    env = env_on(repo, tmp_path)
    done = env.update_branch(BRANCH, reviewed)
    assert done["status"] == "pushed" and parents(repo["b"], done["sha"]) == [reviewed, main2]
    assert _lock_ok(repo["b"], done["sha"], tmp_path) == []
    assert env.reviewed_plus_main(reviewed, done["sha"], 0) is True


def test_a_stale_lock_on_either_side_is_never_papered_over(repo: dict[str, Path], tmp_path: Path) -> None:
    _locked_main(repo)
    reviewed = _relocked(repo["a"], BRANCH, "BUILD_BRIEF.md", "brief, the branch's version\n")
    stale = commit_on(repo["a"], BRANCH, BRANCH, "BUILD_BRIEF.md", "brief, changed again without a new lock\n")
    git(repo["a"], "push", "-q", "origin", BRANCH)
    _relocked(repo["seed"], None, "CLAUDE.md", "rules, main's version\n")
    env = env_on(repo, tmp_path)
    assert reviewed != stale
    assert env.update_branch(BRANCH, stale) == {"status": "conflict", "paths": ["kit/plan.lock"], "main": git(repo["b"], "rev-parse", "origin/main")}
    assert git(repo["origin"], "rev-parse", BRANCH) == stale


def test_a_changed_lock_tool_on_either_side_makes_a_rewritten_lock_a_real_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    """The runner's own check_plan_lock.py (main's, at its checkout) cannot speak for a merge in which
    either side changed that script, so the lock is not recomputed (D-620)."""
    _locked_main(repo)
    reviewed = _relocked(repo["a"], BRANCH, "BUILD_BRIEF.md", "brief, the branch's version\n")
    seed = repo["seed"]
    git(seed, "fetch", "-q", "origin")
    git(seed, "checkout", "-q", "-B", "main", "origin/main")
    tool = seed / "scripts" / "check_plan_lock.py"
    tool.write_text(tool.read_text(encoding="utf-8") + "\n# a harmless change to the lock tool\n")
    _relocked(seed, None, "CLAUDE.md", "rules, main's version\n")
    env = env_on(repo, tmp_path)
    done = env.update_branch(BRANCH, reviewed)
    assert done["status"] == "conflict" and done["paths"] == ["kit/plan.lock"]
    assert git(repo["origin"], "rev-parse", BRANCH) == reviewed


def test_without_the_lock_tool_a_conflicted_lock_stays_a_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    _locked_main(repo)
    reviewed = _relocked(repo["a"], BRANCH, "BUILD_BRIEF.md", "brief, the branch's version\n")
    _relocked(repo["seed"], None, "CLAUDE.md", "rules, main's version\n")
    env = env_on(repo, tmp_path)
    raw = subprocess.run(["git", "merge-tree", "--write-tree", "--name-only", "--no-messages", reviewed, "origin/main"],
                         cwd=repo["b"], capture_output=True, text=True, check=False)
    assert raw.returncode == 1 and raw.stdout.splitlines()[1:] == ["kit/plan.lock"]  # the two lock lines are adjacent
    (repo["b"] / "scripts" / "check_plan_lock.py").unlink()  # the runner's own checkout has no lock tool
    assert env.updates.tree(reviewed, git(repo["b"], "rev-parse", "origin/main")) == (None, ["kit/plan.lock"])


# ---- a retry's branch (prepare_branch) is merged the same way (D-620)


def test_a_retrys_branch_keeps_mains_decisions_first(repo: dict[str, Path], tmp_path: Path) -> None:
    push_main(repo, "docs/decisions.md", BASE_LOG)
    first = push_branch(repo, "docs/decisions.md", BASE_LOG + "- **D-010** Question: attempt one's entry.\n")
    main2 = push_main(repo, "docs/decisions.md", BASE_LOG + "- **D-005** Question: main's entry.\n")
    env, _ = claimed_env(repo, tmp_path, [{"number": 94, "state": "open", "merged_at": None, "auto_merge": None}])
    prep = env.prepare_branch(TASK, 2, SESSION)
    assert prep["base"] == f"origin/{BRANCH}" and parents(repo["b"], prep["base_sha"]) == [first, main2]
    merged = git(repo["b"], "show", f"{prep['base_sha']}:docs/decisions.md") + "\n"
    assert merged == BASE_LOG + "- **D-005** Question: main's entry.\n- **D-010** Question: attempt one's entry.\n"


# ---- update_merge_of: the same fork-point rule as reviewed_plus_main (D-584, D-620)


def test_an_update_merge_built_on_main_older_than_the_reviewed_fork_is_refused(repo: dict[str, Path], tmp_path: Path) -> None:
    seed = repo["seed"]
    change = commit_on(seed, "main", "origin/main", "docs/x.md", "a change main later reverts\n")
    git(seed, "revert", "--no-edit", "HEAD")
    git(seed, "push", "-q", "origin", "main")
    reviewed = pushed_attempt(repo)
    env = env_on(repo, tmp_path)
    tree, _ = env.updates.tree(reviewed, change)
    crafted = git(repo["b"], "commit-tree", str(tree), "-p", reviewed, "-p", change, "-m", "an update merge onto an older main")
    assert env.updates.is_update_merge(reviewed, crafted) == (True, change)  # shaped like one, and `change` is on main
    assert env.update_merge_of(reviewed, crafted, 0) is False


# ---- update pushes go out with the long-lived push token (workflow files from main need it, D-509)


class Recording:
    """A command runner that records every call and answers the few git reads update_branch makes."""

    def __init__(self) -> None:
        self.calls: list[list[str]] = []

    def __call__(self, args: list[str], stdin: str | None, cwd: Path) -> tuple[int, str, str]:
        self.calls.append(args)
        return 0, "", ""


def test_an_update_push_goes_through_the_push_token_path(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("RUNNER_PUSH_TOKEN", "fake-push-token")
    monkeypatch.setenv("GITHUB_REPOSITORY", "o/r")
    fake = Recording()
    env = GitHubEnv(tmp_path, [TASK], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=fake)
    env.main_head = lambda: "main2"  # type: ignore[method-assign]  # reason: no remote here
    env._remote_tip = lambda branch: "rev1"  # type: ignore[method-assign]  # reason: as above
    env._is_ancestor = lambda older, newer: False  # type: ignore[method-assign]  # reason: main is not in the head
    env.updates.commit = lambda reviewed, main, branch: ("merge1", [])  # type: ignore[method-assign]  # reason: the merge itself is tested over real git above
    env.updates.is_update_merge = lambda reviewed, head: (True, "main2")  # type: ignore[method-assign]  # reason: as above
    assert env.update_branch(BRANCH, "rev1") == {"status": "pushed", "sha": "merge1", "main": "main2"}
    push = next(c for c in fake.calls if c[:2] == ["git", "push"])
    assert push == ["git", "push", "--porcelain", "https://github.com/o/r.git", f"merge1:refs/heads/{BRANCH}"]
    helper = ge._env_for(push) or {}
    assert "RUNNER_PUSH_TOKEN" in helper.get("GIT_CONFIG_VALUE_2", "") and "fake-push-token" not in " ".join(push)
    assert ge._long_token_args(["git", "ls-remote", "--exit-code", "origin", f"refs/heads/{BRANCH}"])[3] == "https://github.com/o/r.git"


# ---- enable_auto_merge waits briefly for the PR to show a just-pushed head (D-620)


class LaggingPulls(AutoMergeRunner):
    """The PR lists the head before the update merge for the first `lag` reads."""

    def __init__(self, lag: int) -> None:
        super().__init__([{"number": 30, "head": {"sha": "old"}}], ccr_ok=True)
        self.lag = lag

    def __call__(self, args: list[str], stdin: str | None, cwd: Path) -> tuple[int, str, str]:
        if args[:2] == ["gh", "api"] and "/pulls?head=" in args[-1]:
            self.lag -= 1
            self.open_prs = [{"number": 30, "head": {"sha": "old" if self.lag >= 0 else "m1"}}]
        return super().__call__(args, stdin, cwd)


def test_auto_merge_waits_a_bounded_time_for_the_pushed_head(tmp_path: Path) -> None:
    waits: list[float] = []
    env = am_env(tmp_path, LaggingPulls(lag=2))
    env.sleep = waits.append  # type: ignore[assignment]  # reason: record the waits
    assert env.enable_auto_merge(BRANCH, "m1") == (True, "auto-merge on #30 (ccr route)") and len(waits) == 2
    never = am_env(tmp_path, LaggingPulls(lag=99))
    never.sleep = waits.append  # type: ignore[assignment]  # reason: as above
    waits.clear()
    ok, why = never.enable_auto_merge(BRANCH, "m1")
    assert not ok and "not the reviewed head" in why and len(waits) == ge.HEAD_POLLS
