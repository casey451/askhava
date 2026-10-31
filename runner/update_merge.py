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
   (the runner's copy could compute a different lock); otherwise the lock is a real conflict;
4. a conflicting docs/for-casey-review.md, docs/architecture.md or Cargo.toml is resolved only when
   every conflict in it is a pure insertion on both sides at the same place (git's diff3 view holds
   no base line between them, `insertions_after_main`): both inserted blocks are kept verbatim, main's
   first and the branch's after, as for docs/decisions.md; when main's block opens a section (a
   heading or a TOML table) the branch's must open its own. A resolved Cargo.toml must still parse as
   TOML (a key both sides added twice does not), and every locked section of a resolved
   docs/architecture.md (scripts/check_plan_lock.py) must be the branch's or main's (D-890).
Any other conflicting path is a real conflict: nothing is resolved by guesswork. The cycle then hands
the branch to a worker on the same attempt to merge main in itself (a resync, runner/cycle.py, D-890).

The commit's author and committer dates are main's commit date, so two runners that build the same
update make the same commit and the second push is a no-op instead of a rejected race.
"""

from __future__ import annotations

import importlib.util
import tempfile
import tomllib
from collections.abc import Callable
from pathlib import Path
from types import ModuleType

from runner import state as st

Runner = Callable[[list[str], "str | None", Path], tuple[int, str, str]]
DECISIONS = "docs/decisions.md"
PLAN_LOCK = "kit/plan.lock"
LOCK_TOOL = "scripts/check_plan_lock.py"
LOCK_TREES = ("docs/", "scripts/", "kit/")  # where locked files live; root files are checked out too
ARCHITECTURE = "docs/architecture.md"
# Shared files where two tasks finishing close together each insert lines at the same place (D-890).
INSERT_ONLY = ("docs/for-casey-review.md", ARCHITECTURE, "Cargo.toml")
MARK = 40  # conflict marker size for `git merge-file`: no line of these files starts with 40 "<"s


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


def opens_section(line: str) -> bool:
    """A Markdown heading or a TOML table header: what the lines after it belong to."""
    return line.startswith(("#", "["))


def joinable(branch: list[str], base: list[str], main: list[str]) -> bool:
    """Whether one conflict is two pure insertions that can stand one after the other: no base line
    between the markers, each block ending in a newline, and, when main's block opens a section, the
    branch's block opening its own (else the branch's lines would move into main's new section)."""
    if base or any(b and not b[-1].endswith("\n") for b in (branch, main)):
        return False
    first = next((line for line in branch if line.strip()), "")
    return not any(opens_section(line) for line in main) or opens_section(first)


def insertions_after_main(merged: str) -> str | None:
    """A `git merge-file -p --diff3 --marker-size=MARK <branch> <base> <main>` result with each conflict
    replaced by main's lines and then the branch's (D-890); None when any conflict is not `joinable` or
    the markers do not close."""
    out: list[str] = []
    part, block = "", {"ours": [], "base": [], "theirs": []}  # type: str, dict[str, list[str]]
    opens = {"<" * MARK: "ours", "|" * MARK: "base", "=" * MARK: "theirs"}
    for line in merged.splitlines(keepends=True):
        mark = line.rstrip("\n").split(" ", 1)[0]
        if mark in opens and (part or mark == "<" * MARK):
            part = opens[mark]
        elif mark == ">" * MARK and part == "theirs":
            if not joinable(block["ours"], block["base"], block["theirs"]):
                return None
            out += block["theirs"] + block["ours"]
            part, block = "", {"ours": [], "base": [], "theirs": []}
        elif part:
            block[part].append(line)
        else:
            out.append(line)
    return None if part else "".join(out)


def insert_only(path: str) -> bool:
    """Whether a conflict in `path` may be resolved as insertions on both sides (D-890)."""
    return path in INSERT_ONLY or path.endswith("/Cargo.toml")


def toml_ok(path: str, text: str) -> bool:
    """A resolved Cargo.toml must still parse (tomllib refuses a key defined twice); other files pass."""
    if not path.endswith("Cargo.toml"):
        return True
    try:
        tomllib.loads(text)
    except tomllib.TOMLDecodeError:
        return False
    return True


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
        for path in sorted(p for p in conflicts if insert_only(p)):
            text = self._insertions((base, reviewed, main), path)
            if text is not None:
                conflicts.discard(path)
                writes[path] = text
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

    def _insertions(self, commits: tuple[str, str, str], path: str) -> str | None:
        """`path` merged with both sides' insertions kept, main's first (D-890); None when its conflicts
        are not all pure insertions, a side lacks the file, a Cargo.toml no longer parses, or a locked
        section of docs/architecture.md would be neither side's (a resolution never writes locked text)."""
        base, reviewed, main = commits
        texts = [self._file(reviewed, path), self._file(base, path), self._file(main, path)]
        if any(t is None for t in texts):
            return None
        with tempfile.TemporaryDirectory() as tmp:
            names = [str(Path(tmp) / n) for n in ("branch", "base", "main")]
            for name, text in zip(names, texts):
                Path(name).write_text(str(text), encoding="utf-8")
            code, out, _ = self._run(["git", "merge-file", "-p", "--diff3", f"--marker-size={MARK}", *names], None, self.root)
        text = insertions_after_main(out) if 0 <= code < 128 else None
        if text is None or not toml_ok(path, text):
            return None
        if path == ARCHITECTURE and not self._sections_kept(text, str(texts[0]), str(texts[2])):
            return None
        return text

    def _sections_kept(self, merged: str, ours: str, theirs: str) -> bool:
        """Whether each locked section of docs/architecture.md (scripts/check_plan_lock.py) in the merged
        text is the branch's or main's, so the lock one side carries still holds for it (D-890)."""
        tool = self._lock_tool()
        if tool is None:
            return False
        for heading in tool.LOCKED_SECTIONS:
            found = tool.section_text(merged, heading)
            if found not in (tool.section_text(ours, heading), tool.section_text(theirs, heading)):
                return False
        return True

    def _file(self, commit: str, path: str) -> str | None:
        """The file's text in that commit, None when the commit has no such file."""
        code, out, _ = self._git("show", f"{commit}:{path}")
        return out if code == 0 else None

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
