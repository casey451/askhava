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

from runner import __main__ as entry, github_env as ge
from runner.github_env import GitHubEnv
from runner.update_merge import decisions_after_main, decisions_appended
from test_runner_live import (  # noqa: F401  # reason: `repo` is the shared fixture
    SESSION, TASK, AutoMergeRunner, OfflineGh, am_env, claimed_env, commit_on, git, make_env, move_main, pushed_attempt, repo,
)
from test_runner_live import worker_commit  # noqa: E402

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


# ---- entries filed before main's last entry move to the end (T184's PR #187, D-1029)

PLAN_LOG = BASE_LOG + "- **D-023** Question: a fake plan change, numbered above the reserved block.\n"
EARLY = "- **D-018** Question: a fake reserved entry.\n- **D-019** Question: another fake reserved entry.\n"
INSERTED = BASE_LOG + EARLY + "- **D-023** Question: a fake plan change, numbered above the reserved block.\n"


def test_decisions_appended_moves_inserted_entries_to_the_end_and_refuses_anything_else() -> None:
    assert decisions_appended(PLAN_LOG, INSERTED) == PLAN_LOG + EARLY
    assert decisions_appended(PLAN_LOG, PLAN_LOG + EARLY) is None  # already in order: nothing to fix
    assert decisions_appended(PLAN_LOG, INSERTED.replace("fake first", "edited")) is None  # a changed entry
    assert decisions_appended(PLAN_LOG, BASE_LOG + EARLY) is None  # a removed entry
    assert decisions_appended(PLAN_LOG, BASE_LOG + "## A heading\n" + EARLY + PLAN_LOG[len(BASE_LOG):]) is None
    assert decisions_appended(PLAN_LOG, BASE_LOG + "- **D-001** again\n" + PLAN_LOG[len(BASE_LOG):]) is None
    assert decisions_appended(PLAN_LOG, BASE_LOG + EARLY + "\n" + PLAN_LOG[len(BASE_LOG):]) == PLAN_LOG + EARLY
    assert decisions_appended("", INSERTED) is None


def _append_only(root: Path, main: str, sha: str) -> list[str]:
    main_text = git(root, "show", f"{main}:docs/decisions.md") + "\n"
    return check_pr_change.decisions_append_only(main_text, git(root, "show", f"{sha}:docs/decisions.md") + "\n")


def test_a_retrys_branch_with_entries_filed_before_mains_last_gets_them_moved_to_the_end(repo: dict[str, Path], tmp_path: Path) -> None:
    main1 = push_main(repo, "docs/decisions.md", PLAN_LOG)
    first = push_branch(repo, "docs/decisions.md", INSERTED)
    assert _append_only(repo["a"], main1, first) != []  # what failed Governance on PR #187
    main2 = move_main(repo, "fake repo, main moved\n")
    env, _ = claimed_env(repo, tmp_path, [{"number": 94, "state": "open", "merged_at": None, "auto_merge": None}])
    prep = env.prepare_branch(TASK, 2, SESSION)
    fix, merged_main = parents(repo["b"], prep["base_sha"])
    assert merged_main == main2 and parents(repo["b"], fix) == [first]  # a commit on attempt one's work, then main merged in
    assert git(repo["b"], "show", f"{prep['base_sha']}:docs/decisions.md") + "\n" == PLAN_LOG + EARLY
    assert _append_only(repo["b"], main2, prep["base_sha"]) == []


def test_a_retrys_branch_with_a_changed_main_entry_is_left_to_fail(repo: dict[str, Path], tmp_path: Path) -> None:
    main1 = push_main(repo, "docs/decisions.md", PLAN_LOG)
    first = push_branch(repo, "docs/decisions.md", INSERTED.replace("fake first", "edited"))
    env, _ = claimed_env(repo, tmp_path, [{"number": 94, "state": "open", "merged_at": None, "auto_merge": None}])
    prep = env.prepare_branch(TASK, 2, SESSION)
    assert prep["base_sha"] == first and _append_only(repo["b"], main1, first) != []


def test_a_hand_in_with_entries_filed_before_mains_last_is_fixed_before_the_push(repo: dict[str, Path], tmp_path: Path) -> None:
    main1 = push_main(repo, "docs/decisions.md", PLAN_LOG)
    a = repo["a"]
    worker_commit(a, BRANCH, "docs/k10/note.md")
    (a / "docs/decisions.md").write_text(INSERTED)
    git(a, "commit", "-qam", "the worker's entries, in numeric order")
    worked = git(a, "rev-parse", "HEAD")
    env = make_env(a, tmp_path)
    env.read_state()
    res = env.collect_result(TASK, "submitted", [], [])
    assert parents(a, res.head_sha) == [worked] and git(a, "rev-parse", BRANCH) == res.head_sha
    assert (a / "docs/decisions.md").read_text() == PLAN_LOG + EARLY  # the checkout followed the branch
    assert _append_only(a, main1, res.head_sha) == [] and res.changed_paths == ["docs/decisions.md", "docs/k10/note.md"]
    assert env.collect_result(TASK, "submitted", [], []).head_sha == res.head_sha  # nothing left to fix


def test_a_hand_in_with_a_changed_main_entry_is_not_rewritten(repo: dict[str, Path], tmp_path: Path) -> None:
    main1 = push_main(repo, "docs/decisions.md", PLAN_LOG)
    a = repo["a"]
    worker_commit(a, BRANCH, "docs/k10/note.md")
    (a / "docs/decisions.md").write_text(INSERTED.replace("fake first", "edited"))
    git(a, "commit", "-qam", "the worker edited an old entry")
    git(a, "checkout", "-q", "--detach")  # the branch is not checked out: the ref path
    worked = git(a, "rev-parse", BRANCH)
    env = make_env(a, tmp_path)
    env.read_state()
    res = env.collect_result(TASK, "submitted", [], [])
    assert res.head_sha == worked and _append_only(a, main1, worked) != []


def test_a_hand_in_on_a_branch_not_checked_out_moves_the_ref(repo: dict[str, Path], tmp_path: Path) -> None:
    main1 = push_main(repo, "docs/decisions.md", PLAN_LOG)
    a = repo["a"]
    worker_commit(a, BRANCH, "docs/k10/note.md")
    (a / "docs/decisions.md").write_text(INSERTED)
    git(a, "commit", "-qam", "the worker's entries, in numeric order")
    git(a, "checkout", "-q", "--detach")
    env = make_env(a, tmp_path)
    env.read_state()
    res = env.collect_result(TASK, "submitted", [], [])
    assert git(a, "rev-parse", BRANCH) == res.head_sha and _append_only(a, main1, res.head_sha) == []


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


# ---- pure insertions in shared files are resolved, main's first (D-891)

REVIEW = "# For Casey's review\n\n## First (T001)\n\n- one\n"
CARGO = '[workspace]\nmembers = ["app/*"]\n\n[workspace.dependencies]\naskhava-core = { path = "app/core" }\naskhava-fixtures = { path = "tests/fixtures" }\naxum = "=0.8.9"\n'


def _ours_base_main(tmp: Path, base: str, ours: str, main: str) -> str:
    """`git merge-file -p --diff3` of the three texts with the runner's marker size, as `_insertions` runs it."""
    names = []
    for name, text in (("ours", ours), ("base", base), ("main", main)):
        (tmp / name).write_text(text)
        names.append(str(tmp / name))
    from runner import update_merge  # noqa: PLC0415  # reason: the constant under test
    return subprocess.run(["git", "merge-file", "-p", "--diff3", f"--marker-size={update_merge.MARK}", *names],
                          capture_output=True, text=True, check=False).stdout


def test_insertions_after_main_keeps_both_blocks_and_refuses_anything_else(tmp_path: Path) -> None:
    from runner.update_merge import insertions_after_main  # noqa: PLC0415  # reason: beside its sibling helpers
    ours, main = REVIEW + "\n## Branch (T003)\n\n- three\n", REVIEW + "\n## Main (T002)\n\n- two\n"
    assert insertions_after_main(_ours_base_main(tmp_path, REVIEW, ours, main)) == main + "\n## Branch (T003)\n\n- three\n"
    edited = REVIEW.replace("- one", "- one, edited") + "\n## Branch (T003)\n\n- three\n"
    assert insertions_after_main(_ours_base_main(tmp_path, REVIEW, edited, REVIEW.replace("- one", "- one, main's"))) is None
    loose = REVIEW + "- branch bullet\n"  # a bullet, not a section, after main's new section would move into it
    assert insertions_after_main(_ours_base_main(tmp_path, REVIEW, loose, main)) is None
    assert insertions_after_main("x\n" + "<" * 40 + " a\ny\n") is None  # markers that never close
    assert insertions_after_main("no conflict\n") == "no conflict\n"


def test_cargo_toml_and_casey_review_insertions_merge_without_a_worker(repo: dict[str, Path], tmp_path: Path) -> None:
    """T024's PR #142 (Cargo.toml and docs/for-casey-review.md, October 30) as a drill: both sides added one
    workspace dependency and one review section at the same place."""
    push_main(repo, "Cargo.toml", CARGO)
    push_main(repo, "docs/for-casey-review.md", REVIEW)
    ai = CARGO.replace('askhava-fixtures', 'askhava-ai = { path = "app/ai" }\naskhava-fixtures')
    push_branch(repo, "Cargo.toml", ai)
    reviewed = push_branch(repo, "docs/for-casey-review.md", REVIEW + "\n## The AI (T024)\n\n- ai\n", start=BRANCH)
    push_main(repo, "Cargo.toml", CARGO.replace('askhava-fixtures', 'askhava-jobs = { path = "app/jobs" }\naskhava-fixtures'))
    main2 = push_main(repo, "docs/for-casey-review.md", REVIEW + "\n## Links (T014)\n\n- ids\n")
    env = env_on(repo, tmp_path)
    done = env.update_branch(BRANCH, reviewed)
    assert done["status"] == "pushed" and parents(repo["b"], done["sha"]) == [reviewed, main2]
    cargo = git(repo["b"], "show", f"{done['sha']}:Cargo.toml")
    assert 'askhava-jobs = { path = "app/jobs" }\naskhava-ai = { path = "app/ai" }\naskhava-fixtures' in cargo
    assert git(repo["b"], "show", f"{done['sha']}:docs/for-casey-review.md").endswith("## Links (T014)\n\n- ids\n\n## The AI (T024)\n\n- ai")
    assert env.reviewed_plus_main(reviewed, done["sha"], 0) is True and env.update_merge_of(reviewed, done["sha"], 0) is True


def test_a_doubled_cargo_key_or_an_edited_line_stays_a_real_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    push_main(repo, "Cargo.toml", CARGO)
    twice = CARGO.replace('askhava-fixtures', 'askhava-ai = { path = "app/ai" }\naskhava-fixtures')
    reviewed = push_branch(repo, "Cargo.toml", twice)
    push_main(repo, "Cargo.toml", CARGO.replace('askhava-fixtures', 'askhava-ai = { path = "app/ai2" }\naskhava-fixtures'))
    env = env_on(repo, tmp_path)
    assert env.update_branch(BRANCH, reviewed)["paths"] == ["Cargo.toml"]  # the same key twice no longer parses
    edited = push_branch(repo, "Cargo.toml", twice.replace("=0.8.9", "=0.8.8"), start=BRANCH)
    push_main(repo, "Cargo.toml", CARGO.replace('askhava-fixtures', 'askhava-ai = { path = "app/ai2" }\naskhava-fixtures').replace("=0.8.9", "=0.8.7"))
    assert env.update_branch(BRANCH, edited)["status"] == "conflict"
    assert git(repo["origin"], "rev-parse", BRANCH) == edited


def test_an_architecture_insertion_into_a_locked_section_stays_a_real_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    """A resolution never writes locked text neither side had (scripts/check_plan_lock.py LOCKED_SECTIONS)."""
    arch = "# Architecture\n\n## 2. Layout\n\n- a\n\n## 3. Modules, what each owns, and its tables\n\n- core\n\n## 4. Dependency rules\n\n- none\n"
    push_main(repo, "docs/architecture.md", arch)
    in_two = push_branch(repo, "docs/architecture.md", arch.replace("- a\n", "- a\n- branch\n"))
    main_two = arch.replace("- a\n", "- a\n- main\n")
    push_main(repo, "docs/architecture.md", main_two)
    env = _with_lock_tool(env_on(repo, tmp_path), repo["b"])
    done = env.update_branch(BRANCH, in_two)
    assert done["status"] == "pushed"  # section 2 is not locked
    merged = git(repo["b"], "show", f"{done['sha']}:docs/architecture.md") + "\n"
    in_three = push_branch(repo, "docs/architecture.md", merged.replace("- core\n", "- core\n- branch\n"), start=f"origin/{BRANCH}")
    push_main(repo, "docs/architecture.md", main_two.replace("- core\n", "- core\n- main\n"))
    env = _with_lock_tool(env_on(repo, tmp_path), repo["b"])
    assert env.update_branch(BRANCH, in_three)["paths"] == ["docs/architecture.md"]
    assert git(repo["origin"], "rev-parse", BRANCH) == in_three


def _with_lock_tool(env: ge.GitHubEnv, root: Path) -> ge.GitHubEnv:
    """The runner's checkout with the real scripts/check_plan_lock.py, whose LOCKED_SECTIONS the resolution reads."""
    (root / "scripts").mkdir(exist_ok=True)
    shutil.copy(ROOT / "scripts" / "check_plan_lock.py", root / "scripts" / "check_plan_lock.py")
    return env


# ---- a resync's branch is the reviewed head, main not merged in (D-891)


def test_a_resync_branch_starts_at_the_reviewed_head_without_main(repo: dict[str, Path], tmp_path: Path) -> None:
    reviewed = pushed_attempt(repo, "README.md", "the branch's readme\n")
    main2 = move_main(repo, "main's readme\n")
    env, gh = claimed_env(repo, tmp_path, [{"number": 94, "state": "closed", "merged_at": None, "auto_merge": None}],
                          runner_closed_prs=[94])
    note = {"attempt": 1, "from": reviewed, "main": main2, "paths": ["README.md"], "at": "t"}
    prep = env.prepare_branch(dict(TASK, resync=note), 1, SESSION)
    assert prep["resync"] is True and prep["branch"] == BRANCH and prep["base_sha"] == reviewed and prep["paths"] == ["README.md"]
    assert git(repo["b"], "rev-parse", BRANCH) == reviewed and not any("PATCH" in c for c in gh.calls)
    assert "WITHOUT main merged in" in entry.branch_note(prep) and f"git merge {main2}" in entry.branch_note(prep)
    moved = dict(note, **{"from": "0" * 40})  # origin's tip is not the reviewed head any more: the D-583 path
    later = env.prepare_branch(dict(TASK, resync=moved), 1, SESSION)
    assert not later.get("resync") and later["branch"] == f"{BRANCH}-a2" and later["why"] == "merging main into it conflicted"


def test_a_resync_on_a_branch_whose_pull_request_still_has_auto_merge_closes_it_first(repo: dict[str, Path], tmp_path: Path) -> None:
    reviewed = pushed_attempt(repo, "README.md", "the branch's readme\n")
    main2 = move_main(repo, "main's readme\n")
    env, gh = claimed_env(repo, tmp_path, [{"number": 94, "state": "open", "merged_at": None, "auto_merge": {"merge_method": "squash"}}])
    prep = env.prepare_branch(dict(TASK, resync={"attempt": 1, "from": reviewed, "main": main2, "paths": ["README.md"], "at": "t"}), 1, SESSION)
    assert prep["resync"] is True and prep["dropped_pr"] == 94 and git(repo["b"], "rev-parse", BRANCH) == reviewed
    assert ["gh", "api", "-X", "PATCH", "repos/fake-owner/fake-repo/pulls/94", "-f", "state=closed"] in gh.calls


# ---- marker-shaped lines and dotted TOML keys are refused (D-891 review)

SETEXT = "Notes\n" + "=" * 40 + "\n"  # a Markdown heading underlined with exactly 40 "="s


@pytest.mark.parametrize("char", ["<", "|", "=", ">"])
@pytest.mark.parametrize("side", ["base", "ours", "main"])
def test_a_marker_shaped_line_on_any_side_is_never_resolved(tmp_path: Path, char: str, side: str) -> None:
    """A line whose first word is one of the four 40-character markers could be read as git's own, so
    `insertions_after_main` refuses when any of the three merged texts holds one."""
    from runner.update_merge import insertions_after_main  # noqa: PLC0415  # reason: beside its sibling helpers
    texts = {"base": REVIEW, "ours": REVIEW + "\n## Branch (T003)\n\n- three\n", "main": REVIEW + "\n## Main (T002)\n\n- two\n"}
    clean = _ours_base_main(tmp_path, texts["base"], texts["ours"], texts["main"])
    assert insertions_after_main(clean, (texts["ours"], texts["base"], texts["main"])) is not None
    shaped = "=" * 40 + "\n" if char == "=" else char * 40 + " heading\n"
    if side == "base":  # every side keeps the base's line, so the conflict is still a pure insertion
        texts = {k: v.replace(REVIEW, REVIEW + shaped) for k, v in texts.items()}
    else:
        texts[side] += shaped
    merged = _ours_base_main(tmp_path, texts["base"], texts["ours"], texts["main"])
    assert insertions_after_main(merged, (texts["ours"], texts["base"], texts["main"])) is None


def test_without_the_refusal_a_forty_equals_underline_in_mains_block_is_misread(tmp_path: Path) -> None:
    """Why the refusal exists: read without its sources, a 40-"=" underline in main's block is taken for git's
    separator and the join comes out wrong; with the sources it is refused."""
    from runner.update_merge import insertions_after_main  # noqa: PLC0415  # reason: beside its sibling helpers
    ours, main = REVIEW + "\n## Branch (T003)\n\n- three\n", REVIEW + "\n" + SETEXT + "\n- two\n"
    merged = _ours_base_main(tmp_path, REVIEW, ours, main)
    assert insertions_after_main(merged) != main + "\n## Branch (T003)\n\n- three\n"
    assert insertions_after_main(merged, (ours, REVIEW, main)) is None


def test_dotted_keys_are_found_outside_quotes_only() -> None:
    from runner.update_merge import dotted_key, toml_ok  # noqa: PLC0415  # reason: beside its sibling helpers
    assert dotted_key('serde.version = "1"\n') and dotted_key("version.workspace = true\n") and dotted_key("'x'.y = 2\n")
    assert not dotted_key('serde = { version = "1.0" }\n') and not dotted_key('"a.b" = 1\n') and not dotted_key("[a.b]\n")
    assert not dotted_key("# a.b = 1\n") and not dotted_key("\n") and not dotted_key("plain line\n")
    assert toml_ok("README.md", "not toml", ["a.b = 1\n"]) and not toml_ok("Cargo.toml", "a.b = 1\n", ["a.b = 1\n"])
    assert toml_ok("Cargo.toml", "a.b = 1\n", []) and not toml_ok("Cargo.toml", "a = [\n", [])


def test_dotted_keys_from_both_sides_stay_a_real_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    """The reviewer's case 14: branch `serde.version` and main `serde.features` parse with tomllib into one
    dependency neither side wrote, so the Cargo.toml guard refuses any inserted dotted key."""
    push_main(repo, "Cargo.toml", CARGO)
    reviewed = push_branch(repo, "Cargo.toml", CARGO.replace("axum", 'serde.version = "1.0.0"\naxum'))
    push_main(repo, "Cargo.toml", CARGO.replace("axum", 'serde.features = ["derive"]\naxum'))
    env = env_on(repo, tmp_path)
    assert env.update_branch(BRANCH, reviewed)["paths"] == ["Cargo.toml"]
    assert git(repo["origin"], "rev-parse", BRANCH) == reviewed


def test_a_setext_heading_inside_an_insertion_stays_a_real_conflict_end_to_end(repo: dict[str, Path], tmp_path: Path) -> None:
    """The reviewer's reproduction: a 40-"=" underline inside main's inserted block. Before the refusal it was
    misread and `is_update_merge` accepted the corrupted tree; now no tree is made, so nothing is pushed or adopted."""
    push_main(repo, "docs/for-casey-review.md", REVIEW)
    reviewed = push_branch(repo, "docs/for-casey-review.md", REVIEW + "\n## Branch (T003)\n\n- three\n")
    main2 = push_main(repo, "docs/for-casey-review.md", REVIEW + "\n" + SETEXT + "\n- two\n")
    env = env_on(repo, tmp_path)
    assert env.updates.commit(reviewed, main2, BRANCH) == (None, ["docs/for-casey-review.md"])
    forged = git(repo["b"], "commit-tree", f"{main2}^{{tree}}", "-p", reviewed, "-p", main2, "-m", "a hand-made merge")
    assert env.updates.is_update_merge(reviewed, forged) == (False, main2)
    assert env.update_branch(BRANCH, reviewed)["status"] == "conflict" and git(repo["origin"], "rev-parse", BRANCH) == reviewed
