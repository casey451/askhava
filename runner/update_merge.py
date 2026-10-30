"""Stage C of two lanes (D-620): keep a task's pull request current by merging main into its branch.

With strict up-to-date branches (ruleset protect-main), one task's merge leaves another task's pull
request behind or conflicted, and it can never merge. The runner merges main into the branch itself:
a normal two-parent merge commit (reviewed head first, main second), never a rebase or a force-push.

The merge's tree is one deterministic function of the reviewed commit and the main commit,
`UpdateMerge.tree`, so the same function both builds the merge and later checks any head that claims
to be "the reviewed work plus main" (`reviewed_plus_main`, D-584):

1. `git merge-tree --write-tree` of the two commits;
2. when both sides changed docs/decisions.md, its text is main's file followed by the branch's own
   new lines (`decisions_after_main`), whatever git's union driver made of it, so main's entries stay
   in main's order and the branch's come after (scripts/check_pr_change.py's append-only rule); a
   branch that did more than append, or appended a number main already has, is a real conflict;
3. when both sides changed kit/plan.lock, or it conflicted, the lock is recomputed from the merged
   tree with scripts/check_plan_lock.py, but only when each side's own lock matched its own tree, so
   a stale lock is never papered over, and never when either side changed check_plan_lock.py itself
   (the runner's copy could compute a different lock); otherwise the lock is a real conflict.
Any other conflicting path is a real conflict: nothing is resolved by guesswork.

The commit's author and committer dates are main's commit date, so two runners that build the same
update make the same commit and the second push is a no-op instead of a rejected race.
"""

from __future__ import annotations

import importlib.util
import tempfile
from collections.abc import Callable
from pathlib import Path
from types import ModuleType

from runner import state as st

Runner = Callable[[list[str], "str | None", Path], tuple[int, str, str]]
DECISIONS = "docs/decisions.md"
PLAN_LOCK = "kit/plan.lock"
LOCK_TOOL = "scripts/check_plan_lock.py"
LOCK_TREES = ("docs/", "scripts/", "kit/")  # where locked files live; root files are checked out too


def decisions_after_main(base: str, ours: str, theirs: str) -> str | None:
    """docs/decisions.md for a merge of main (`theirs`) into a branch (`ours`) from their merge base:
    main's text, then the lines the branch appended after the base. None when the branch changed more
    than its end, or appended an entry whose D-number main already uses (a real conflict, D-620)."""
    if not ours.startswith(base):
        return None
    added = ours[len(base):]
    if not added.strip():
        return theirs
    if set(st.decision_numbers(added)) & set(st.decision_numbers(theirs)):
        return None
    if not base.endswith("\n") and added.startswith("\n"):
        added = added[1:]
    return theirs + ("" if not theirs or theirs.endswith("\n") else "\n") + added


class UpdateMerge:
    """The update merge over git plumbing (no working tree is touched); `run` is the Env's command runner."""

    def __init__(self, run: Runner, root: Path, identity: Callable[[], dict[str, str]]) -> None:
        self._run, self.root, self.identity = run, root, identity
        self._lock_module: ModuleType | None = None

    def _git(self, *args: str, stdin: str | None = None, env: dict[str, str] | None = None) -> tuple[int, str, str]:
        prefix = ["env", *[f"{k}={v}" for k, v in env.items()]] if env else []
        return self._run([*prefix, "git", *args], stdin, self.root)

    def _ok(self, *args: str, stdin: str | None = None, env: dict[str, str] | None = None) -> str:
        code, out, err = self._git(*args, stdin=stdin, env=env)
        if code != 0:
            raise RuntimeError(f"git {' '.join(args[:3])} failed: {err.strip()[:200]}")
        return out.strip()

    def _blob(self, commit: str, path: str) -> str:
        code, out, _ = self._git("rev-parse", "--verify", "--quiet", f"{commit}:{path}")
        return out.strip() if code == 0 else ""

    def _text(self, commit: str, path: str) -> str:
        code, out, _ = self._git("show", f"{commit}:{path}")
        return out if code == 0 else ""

    def merge_base(self, first: str, second: str) -> str:
        return self._ok("merge-base", first, second)

    def tree(self, reviewed: str, main: str) -> tuple[str | None, list[str]]:
        """(the update merge's tree, []) or (None, the conflicting paths); RuntimeError when git fails."""
        base = self.merge_base(reviewed, main)
        code, out, err = self._git("merge-tree", "--write-tree", "--name-only", "--no-messages", reviewed, main)
        if code not in (0, 1):
            raise RuntimeError(f"git merge-tree failed: {err.strip()[:200]}")
        lines = out.splitlines()
        merged, conflicts = lines[0].strip(), set(lines[1:]) if code == 1 else set()
        both = {p for p in (DECISIONS, PLAN_LOCK) if self._changed(base, reviewed, p) and self._changed(base, main, p)}
        writes: dict[str, str] = {}
        if DECISIONS in both:
            text = decisions_after_main(self._text(base, DECISIONS), self._text(reviewed, DECISIONS), self._text(main, DECISIONS))
            if text is None:
                conflicts.add(DECISIONS)
            else:
                conflicts.discard(DECISIONS)
                writes[DECISIONS] = text
        if conflicts - {PLAN_LOCK}:
            return None, sorted(conflicts)
        merged = self._replace(merged, writes)
        if PLAN_LOCK in conflicts or PLAN_LOCK in both:
            if self._changed(base, reviewed, LOCK_TOOL) or self._changed(base, main, LOCK_TOOL):
                return None, [PLAN_LOCK]  # the lock tool itself changed: the runner's copy cannot speak for the merge
            lock = self._relock(merged, reviewed, main)
            if lock is None and PLAN_LOCK in conflicts:
                return None, [PLAN_LOCK]
            merged = self._replace(merged, {PLAN_LOCK: lock} if lock is not None else {})
        return merged, []

    def _changed(self, base: str, side: str, path: str) -> bool:
        return self._blob(base, path) != self._blob(side, path)

    def _replace(self, tree: str, writes: dict[str, str]) -> str:
        """`tree` with each path's content replaced, built in a private index."""
        if not writes:
            return tree
        with tempfile.TemporaryDirectory() as tmp:
            env = {"GIT_INDEX_FILE": str(Path(tmp) / "index")}
            self._ok("read-tree", tree, env=env)
            for path, text in sorted(writes.items()):
                blob = self._ok("hash-object", "-w", "--stdin", stdin=text)
                self._ok("update-index", "--add", "--cacheinfo", f"100644,{blob},{path}", env=env)
            return self._ok("write-tree", env=env)

    def _relock(self, merged: str, reviewed: str, main: str) -> str | None:
        """The merged tree's kit/plan.lock recomputed, when both sides' own locks matched their trees; else None."""
        tool = self._lock_tool()
        if tool is None or not all(self._lock_matches(tool, side) for side in (reviewed, main)):
            return None
        with tempfile.TemporaryDirectory() as tmp:
            self._checkout(merged, Path(tmp))
            tool.write_lock(Path(tmp))
            return (Path(tmp) / PLAN_LOCK).read_text(encoding="utf-8")

    def _lock_matches(self, tool: ModuleType, commit: str) -> bool:
        with tempfile.TemporaryDirectory() as tmp:
            self._checkout(commit, Path(tmp))
            return not tool.differences(Path(tmp))

    def _checkout(self, tree: str, dest: Path) -> None:
        """Write the files a plan lock can name (root files, docs/, scripts/, kit/) of `tree` under `dest`."""
        with tempfile.TemporaryDirectory() as tmp:
            env = {"GIT_INDEX_FILE": str(Path(tmp) / "index")}
            self._ok("read-tree", tree, env=env)
            names = [n for n in self._ok("ls-files", env=env).splitlines() if "/" not in n or n.startswith(LOCK_TREES)]
            if names:
                self._ok("checkout-index", "-f", f"--prefix={dest}/", "--stdin", stdin="\n".join(names) + "\n", env=env)

    def _lock_tool(self) -> ModuleType | None:
        """scripts/check_plan_lock.py of the runner's own checkout (main), loaded once; None when absent."""
        if self._lock_module is None:
            path = self.root / "scripts" / "check_plan_lock.py"
            spec = importlib.util.spec_from_file_location("runner_update_plan_lock", path)
            if spec is None or spec.loader is None or not path.is_file():
                return None
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            self._lock_module = module
        return self._lock_module

    def commit(self, reviewed: str, main: str, branch: str) -> tuple[str | None, list[str]]:
        """(the merge commit of main into the reviewed head, []) or (None, the conflicting paths)."""
        tree, conflicts = self.tree(reviewed, main)
        if tree is None:
            return None, conflicts
        stamp = f"@{self._ok('show', '-s', '--format=%ct', main)} +0000"
        env = {**self.identity(), "GIT_AUTHOR_DATE": stamp, "GIT_COMMITTER_DATE": stamp}
        message = f"Merge main into {branch} (runner update merge, D-620)\n\nReviewed: {reviewed}\nMain: {main}\n"
        return self._ok("commit-tree", tree, "-p", reviewed, "-p", main, "-m", message, env=env), []

    def parents(self, commit: str) -> list[str]:
        return self._ok("rev-list", "--parents", "-n", "1", commit).split()[1:]

    def is_update_merge(self, reviewed: str, head: str) -> tuple[bool, str]:
        """(True, its main commit) when `head` is a merge of exactly the reviewed commit and one other
        commit whose tree is `tree(reviewed, that commit)`; (False, '') otherwise. Whether that other
        commit is on main is the caller's check (GitHubEnv.in_main). RuntimeError when git fails."""
        parents = self.parents(head)
        if len(parents) != 2 or parents[0] != reviewed:
            return False, ""
        tree, _ = self.tree(reviewed, parents[1])
        return tree is not None and tree == self._ok("rev-parse", f"{head}^{{tree}}"), parents[1]
