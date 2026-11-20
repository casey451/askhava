"""The kept work of a failed attempt (D-4720): the `claude/wip/<task>-a<n>` ref names, and the path rules and the
scan that decide whether a head may be pushed to one.

A failed attempt's unpushed commits are kept on a WIP ref only when nothing in them could harm a public repository
or the next attempt: no changed path under `kit/tasks.json` `protected_paths`, among the plan lock's locked files
(`scripts/*.py`, `scripts/*.sh`, `kit/runner.json`, ...), a `.gitattributes` or a `.gitleaks*` file (`blocked_paths`);
no binary or image (`binary_paths`); and a clean `scan`: no added `gitleaks:allow`, gitleaks over every commit of
`<main>..<head>` and over the head tree, and main's `scripts/pii_scan.py` over each commit's changed files, the head
tree and the commit messages. Every scanner is main's copy, written from `git show <main>:<path>` into a temporary
directory, never the checkout's (the worker's branch). The whole scan has one time bound (WIP_SCAN_SECONDS), and a
missing tool, a non-zero exit, a gitleaks exit 0 that logged an error or scanned too few commits
(`gitleaks_refusal`, D-4801), the bound or any exception is a refusal with a one-line reason: it fails closed.
The clone itself is the worker's: every git and scanner call runs with no system or global git config, no
replace objects or grafts, no hooks, no fsmonitor, no colour and no signature checks forced through the
environment (`git_env`), and the scan refuses a clone whose own `.git/config` sets any key outside
LOCAL_CONFIG_KEYS or whose `info/attributes` says anything (`repo_refusal`, D-4799), so a local setting can
neither hide a line from a scanner nor run worker code. Every read a `.gitattributes` could change takes its
attributes from main's tree (`attr_source`), never the working tree (D-4800); gitleaks reads a fresh worktree of the
head, which holds no untracked file.
`GitHubEnv.wip_check`, `push_wip`, `wip_refs`, `delete_wip_ref` and `prepare_branch` use this module; nothing here
touches the network. Standard library only.
"""

from __future__ import annotations

import ast
import fnmatch
import importlib.util
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from collections.abc import Callable, Iterable
from pathlib import Path, PurePosixPath
from types import ModuleType

WIP_REF = re.compile(r"claude/wip/(T[0-9]+[A-Za-z]?)-a([0-9]+)", re.ASCII)  # a WIP ref (D-4720)
WIP_SCAN_SECONDS = 120  # the one time bound of a whole scan; past it the head is refused (D-4720)
PII_FILES = ("scripts/pii_scan.py", "scripts/brand.py", "app/core/data/brand.json")  # main's PII scanner and its brand
GITLEAKS_FILES = (".gitleaks.toml", ".gitleaksignore")  # main's gitleaks config, when main has one
GITLEAKS_FLAGS = ("--no-banner", "--redact", "--exit-code", "1")
ALLOW_MARK = "gitleaks:allow"
IMAGE_SUFFIXES = (".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".ico", ".bmp", ".tif", ".tiff", ".heic", ".avif")
# Never handed to a scanner: a token, or a gitleaks config the environment could name.
DROPPED_ENV = ("RUNNER_PUSH_TOKEN", "RUNNER_CHECKS_TOKEN", "GH_TOKEN", "GITHUB_TOKEN", "GITLEAKS_CONFIG", "GITLEAKS_CONFIG_TOML")
# Forced on every git call of a scan, gitleaks' own included, above any config file (D-4799).
# The graft file is a path that cannot exist (under /dev/null): an existing one, even empty, makes git 2.43+ print
# a deprecation hint on stderr, and `gitleaks git` then scans 0 commits and still exits 0 (D-4801).
SAFE_GIT_ENV = {"GIT_NO_REPLACE_OBJECTS": "1", "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
                "GIT_ATTR_NOSYSTEM": "1", "GIT_GRAFT_FILE": os.path.join(os.devnull, "no-grafts")}
SAFE_GIT_CONFIG = (("core.hooksPath", os.devnull), ("core.fsmonitor", "false"), ("core.attributesFile", os.devnull),
                   ("core.commitGraph", "false"), ("color.ui", "never"), ("log.showSignature", "false"),
                   ("advice.graftFileDeprecated", "false"))
# The only keys the clone's own .git/config may set (an actions/checkout clone plus the runner's identity, D-4799).
LOCAL_CONFIG_KEYS = ("core.repositoryformatversion", "core.filemode", "core.bare", "core.logallrefupdates",
                     "core.ignorecase", "core.precomposeunicode", "remote.origin.url", "remote.origin.fetch", "gc.auto",
                     "branch.*.remote", "branch.*.merge", "user.name", "user.email")
NO_COLOR = ("--no-color", "--no-show-signature", "--no-textconv", "--no-ext-diff")  # on every git log (D-4799)
ANSI = re.compile(r"\x1b\[[0-9;]*m")
GITLEAKS_ERROR = re.compile(r"(?:^|\s)(?:ERR|FTL|error=)", re.MULTILINE)  # gitleaks' error and fatal log levels
GITLEAKS_SCANNED = re.compile(r"\bINF\s+([0-9]+) commits scanned\b")

Bounded = Callable[[list[str], Path, float], tuple[int, str, str]]


class Refused(Exception):
    """A scan's refusal; its message is the one-line reason."""


def wip_ref(task_id: str, attempt: int) -> str:
    """`claude/wip/<task>-a<attempt>`; ValueError when that is not a WIP_REF name."""
    name = f"claude/wip/{task_id}-a{int(attempt)}"
    if not WIP_REF.fullmatch(name):
        raise ValueError(f"not a WIP ref: {name[:80]!r}")
    return name


def git_env() -> dict[str, str]:
    """The environment of every scan call: this process's, without a token or any `GIT_*` variable, plus
    SAFE_GIT_ENV and SAFE_GIT_CONFIG (as command-line config, which beats every file; D-4799)."""
    env = {k: v for k, v in os.environ.items() if k not in DROPPED_ENV and not k.startswith("GIT_")}
    env.update(SAFE_GIT_ENV)
    env["GIT_CONFIG_COUNT"] = str(len(SAFE_GIT_CONFIG))
    for i, (key, value) in enumerate(SAFE_GIT_CONFIG):
        env[f"GIT_CONFIG_KEY_{i}"], env[f"GIT_CONFIG_VALUE_{i}"] = key, value
    return env


def run_bounded(args: list[str], cwd: Path, seconds: float) -> tuple[int, str, str]:
    """One scanner or git call with what is left of the bound, in `git_env()`; Refused when it outruns the bound."""
    try:
        done = subprocess.run(args, cwd=cwd, capture_output=True, timeout=seconds, check=False, env=git_env())
    except subprocess.TimeoutExpired as exc:
        raise Refused(f"the WIP scan passed its {WIP_SCAN_SECONDS}-second bound ({Path(args[0]).name})") from exc
    return done.returncode, done.stdout.decode("utf-8", "replace"), done.stderr.decode("utf-8", "replace")


def git_read(root: Path, *args: str, check: bool = True) -> str:
    """One git read in `root` in `git_env()` within 60 seconds, stripped; Refused when it fails and `check`
    ('' then otherwise). `GitHubEnv.wip_check` reads the clone only through this (D-4799)."""
    code, out, err = run_bounded(["git", *args], root, 60)
    if code != 0 and check:
        raise Refused(f"git {' '.join(args[:2])} failed (exit {code}): {err.strip()[:160]}")
    return out.strip() if code == 0 else ""


def repo_refusal(root: Path) -> str:
    """'' when the clone's own git state is one a scan can trust (`_repo`), else why (D-4799); fails closed."""
    try:
        _repo(lambda args, cwd: run_bounded(args, cwd, 60), root)
    except Refused as exc:
        return str(exc)[:300]
    except Exception as exc:  # noqa: BLE001  # fail closed (D-4720)
        return f"the WIP scan could not read the clone: {type(exc).__name__}: {str(exc)[:200]}"
    return ""


def _repo(bounded: Callable[[list[str], Path], tuple[int, str, str]], root: Path) -> None:
    """Refused unless the clone's `.git/config`, read without includes, sets only LOCAL_CONFIG_KEYS (so no colour,
    textconv, external diff, hook path, fsmonitor, signature program, include, worktree config or sparse
    checkout), it is not shallow (a planted `shallow` file cuts history) and its `info/attributes` says nothing
    (D-4799)."""
    listed = _git(bounded, root, "config", "--local", "--no-includes", "--name-only", "--list", "-z")
    odd = [k for k in listed.split("\0") if k and not any(fnmatch.fnmatchcase(k.lower(), p) for p in LOCAL_CONFIG_KEYS)]
    if odd:
        raise Refused(f"the clone's git config sets {odd[0][:80]!r}, which a WIP scan does not allow")
    if _git(bounded, root, "rev-parse", "--is-shallow-repository").strip() != "false":
        raise Refused("the clone is shallow, which a WIP scan does not allow")
    attributes = Path(root, _git(bounded, root, "rev-parse", "--git-common-dir").strip(), "info", "attributes")
    if attributes.is_file() and attributes.read_text(encoding="utf-8", errors="replace").strip():
        raise Refused("the clone has .git/info/attributes, which a WIP scan does not allow")


def locked_patterns(source: str) -> list[str]:
    """`LOCKED_FILES` and `LOCKED_GLOBS` of a `scripts/check_plan_lock.py` text, read as literals (never run).
    The globs cover a file main does not have yet too, so this is at least `locked_paths()`. ValueError when
    either is missing or not a literal."""
    found: dict[str, list[str]] = {}
    for node in ast.parse(source).body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
            if node.targets[0].id in ("LOCKED_FILES", "LOCKED_GLOBS"):
                found[node.targets[0].id] = [str(p) for p in ast.literal_eval(node.value)]
    if len(found) != 2:
        raise ValueError("check_plan_lock.py has no LOCKED_FILES or LOCKED_GLOBS literal")
    return found["LOCKED_FILES"] + found["LOCKED_GLOBS"]


def blocked_paths(paths: Iterable[str], protected: Iterable[str], locked: Iterable[str]) -> list[str]:
    """The paths a WIP ref may never carry (D-4720): under a `protected_paths` entry (a `/` entry is a
    directory), matching a locked file or glob, or named `.gitattributes` or `.gitleaks*` anywhere."""
    guarded, frozen = tuple(protected), tuple(locked)
    return [p for p in paths if _blocked(p, guarded, frozen)]


def _blocked(path: str, protected: tuple[str, ...], locked: tuple[str, ...]) -> bool:
    name = PurePosixPath(path).name
    if name == ".gitattributes" or name.startswith(".gitleaks"):
        return True
    if any(path == q or (q.endswith("/") and path.startswith(q)) for q in protected):
        return True
    return any(path == q or fnmatch.fnmatchcase(path, q) for q in locked)


def binary_paths(numstat: str) -> list[str]:
    """The paths `git diff --numstat` (rows split by newline or NUL) reports as binary (`-`), and any image by
    its name: a public repository never keeps a photo or a binary on a WIP ref (D-4720)."""
    found = []
    for row in re.split(r"[\0\n]", numstat):
        parts = row.split("\t", 2)
        if len(parts) == 3 and parts[2] and ("-" in parts[:2] or parts[2].lower().endswith(IMAGE_SUFFIXES)):
            found.append(parts[2])
    return found


def commit_paths(root: Path, main: str, head: str) -> tuple[list[str], list[str]]:
    """Every path any commit of `<main>..<head>` changes, and those of them it changes as a binary or an image
    (`binary_paths`), with attributes from main's tree. A merge counts only the paths its combined diff (`--cc`)
    shows, the ones it changes against every parent, so main's own changes merged in are not the branch's; a
    non-merge counts its diff against its parent. A file added in one commit and deleted in a later one is
    still in the WIP ref's history, so the diff `<main>...<head>` alone would miss it (D-4801)."""
    flags = (attr_source(main), "diff-tree", "--root", "-r", "--no-commit-id", "--no-renames", "--no-ext-diff",
             "--no-textconv", "-z")
    names: list[str] = []
    binary: list[str] = []
    for commit in git_read(root, "rev-list", f"{main}..{head}").split():
        changed = [p for p in git_read(root, *flags, "--cc", "--name-only", commit).split("\0") if p]
        numstat = git_read(root, *flags, "--numstat", "--diff-merges=first-parent", commit)
        names += changed
        binary += [p for p in binary_paths(numstat) if p in changed]
    return names, binary


def scan(root: Path, main: str, head: str, run: Bounded = run_bounded, seconds: float = WIP_SCAN_SECONDS,
         clock: Callable[[], float] = time.monotonic) -> str:
    """'' only when `<main>..<head>` passes every check in the module docstring, else the one-line reason.
    `root` is the clone (any checkout: only main's scanners and git objects are read)."""
    deadline = clock() + seconds

    def bounded(args: list[str], cwd: Path) -> tuple[int, str, str]:
        left = deadline - clock()
        if left <= 0:
            raise Refused(f"the WIP scan passed its {seconds:g}-second bound")
        return run(args, cwd, left)

    try:
        with tempfile.TemporaryDirectory() as tmp:
            return _scan(bounded, root, (main, head), Path(tmp))
    except Refused as exc:
        return str(exc)[:300]
    except Exception as exc:  # noqa: BLE001  # any failure is a refusal, never a pass (fail closed, D-4720)
        return f"the WIP scan failed: {type(exc).__name__}: {str(exc)[:200]}"


def _scan(bounded: Callable[[list[str], Path], tuple[int, str, str]], root: Path, span: tuple[str, str], tmp: Path) -> str:
    main, head = span
    rng = f"{main}..{head}"
    _repo(bounded, root)
    attr = attr_source(main)
    log = _git(bounded, root, attr, "log", "--cc", *NO_COLOR, "--format=", rng)
    if any(ALLOW_MARK in line for line in added_lines(log)):
        raise Refused(f"an added line in {rng[:40]} carries {ALLOW_MARK}")
    base = tmp / "main"
    _write_main(bounded, root, main, base)
    tree = tmp / "head"
    _git(bounded, root, "worktree", "add", "--detach", str(tree), head)
    try:
        _gitleaks(bounded, tree, (rng, adding_commits(_git(bounded, root, attr, "log", *NO_COLOR, "--numstat",
                                                          "--diff-merges=first-parent", "--format=%x00", rng))), base)
        _pii(bounded, root, (attr, rng), tree, base)
    finally:
        subprocess.run(["git", "worktree", "remove", "--force", str(tree)], cwd=root, capture_output=True, timeout=60,
                       check=False, env=git_env())
    return ""


def attr_source(main: str) -> str:
    """`--attr-source=<main>`, on every git read of a scan that a `.gitattributes` could change: git then reads
    attributes from main's tree only, never from the worker's working tree, where an untracked or ignored
    `.gitattributes` (`* diff` passes a binary as text, `* -diff` hides an added line) is no committed path that
    `blocked_paths` sees (D-4800). A git older than 2.40 rejects the option: a refusal, never a pass."""
    return f"--attr-source={main}"


def added_lines(log: str) -> Iterable[str]:
    """The lines a `git log --cc` patch adds against every parent: `+` in a plain diff, `++` in a merge's combined
    diff. A merge (`_merge_main` makes one on every retry) shows its own additions this way, and a line that came
    from main (`+ `) or from the branch's earlier commits (` +`) is not one (D-4798)."""
    width = 1
    for line in log.splitlines():
        if line.startswith("@@"):
            width = len(line) - len(line.lstrip("@")) - 1
        elif line[:width] == "+" * width:
            yield line


def _git(bounded: Callable[[list[str], Path], tuple[int, str, str]], cwd: Path, *args: str) -> str:
    code, out, err = bounded(["git", *args], cwd)
    if code != 0:
        raise Refused(f"git {' '.join(args[:2])} failed (exit {code}): {err.strip()[:160]}")
    return out


def _write_main(bounded: Callable[[list[str], Path], tuple[int, str, str]], root: Path, main: str, base: Path) -> None:
    """Main's PII scanner, its brand files and any gitleaks config, written under `base` at their own paths."""
    for path in (*PII_FILES, *GITLEAKS_FILES):
        code, out, _ = bounded(["git", "show", f"{main}:{path}"], root)
        if code != 0 and path in PII_FILES:
            raise Refused(f"main has no {path}")
        if code == 0:
            (base / path).parent.mkdir(parents=True, exist_ok=True)
            (base / path).write_text(out, encoding="utf-8")


def _gitleaks(bounded: Callable[[list[str], Path], tuple[int, str, str]], tree: Path, span: tuple[str, int],
              base: Path) -> None:
    """gitleaks over exactly the commits of `rng` and over the head tree, with main's config and ignore file. A
    merge's patch is its diff against its first parent: `git log -p` prints none for a merge by default (D-4798).
    `span` is (`rng`, the commits of it that add a text line); each run must pass `gitleaks_refusal` (D-4801)."""
    exe = shutil.which("gitleaks")
    if not exe:
        raise Refused("gitleaks is not installed")
    rng, least = span
    opts = [*GITLEAKS_FLAGS, "--gitleaks-ignore-path", str(base)]
    if (base / ".gitleaks.toml").is_file():
        opts += ["--config", str(base / ".gitleaks.toml")]
    for args in ([exe, "git", "--log-opts", " ".join((*NO_COLOR, "--diff-merges=first-parent", rng)), *opts, str(tree)], [exe, "dir", *opts, str(tree)]):
        code, out, err = bounded(args, base)
        why = gitleaks_refusal(args[1], code, out + "\n" + err, least)
        if why:
            raise Refused(why)


def gitleaks_refusal(kind: str, code: int, output: str, least: int) -> str:
    """'' when one gitleaks run (`kind` "git" or "dir") passed, else why. Exit 0 is not enough: gitleaks exits 0
    after logging an error (a `git log` stderr line, even a hint, gives `ERR error="stderr is not empty"` and "0
    commits scanned"), so any ERR or FTL line is a refusal, and a `git` run must report scanning at least `least`
    commits, the ones that add a text line (gitleaks skips the rest). Fails closed (D-4801)."""
    text = ANSI.sub("", output)
    if code != 0:
        return f"gitleaks {kind}: " + ("a leak was found" if code == 1 else f"exit {code}")
    if GITLEAKS_ERROR.search(text):
        return f"gitleaks {kind}: it logged an error, so its exit 0 is no pass"
    counts = GITLEAKS_SCANNED.findall(text)
    if kind == "git" and (len(counts) != 1 or int(counts[0]) < least):
        return f"gitleaks git: scanned {counts[0] if len(counts) == 1 else 'no count of'} commits, not the {least} that add a line"
    return ""


def adding_commits(numstat: str) -> int:
    """How many commits of a `git log --numstat --format=%x00` add at least one text line (binary rows are `-`)."""
    blocks = numstat.split("\0")[1:]
    return sum(1 for block in blocks if any(r.split("\t")[0].isdigit() and int(r.split("\t")[0]) > 0
                                             for r in block.splitlines() if r.count("\t") >= 2))


def _pii(bounded: Callable[[list[str], Path], tuple[int, str, str]], root: Path, span: tuple[str, str], tree: Path,
         base: Path) -> None:
    """Main's PII scanner over each commit's changed files as of that commit, the commit messages, then the head tree.
    `span` is (`attr_source(main)`, `<main>..<head>`)."""
    module = _load(base / "scripts" / "pii_scan.py")
    prefixes, files = tuple(getattr(module, "SKIP_PREFIXES", ())), tuple(getattr(module, "SKIP_FILES", ()))
    attr, rng = span
    for commit in _git(bounded, root, "rev-list", "--reverse", rng).split():
        names = _git(bounded, root, attr, "diff-tree", "--root", "--cc", "-r", "--no-commit-id", "--name-only", "-z",
                     "--no-renames", "--diff-filter=d", commit)
        for path in (p for p in names.split("\0") if p and not p.startswith(prefixes) and p not in files):
            if module.scan_text(path, _git(bounded, root, attr, "show", "--no-textconv", f"{commit}:{path}")):
                raise Refused(f"PII scan: {path} in {commit[:12]}")
    if module.scan_text("commit message", _git(bounded, root, "log", *NO_COLOR, "--format=%B", rng)):
        raise Refused(f"PII scan: a commit message in {rng[:40]}")
    code, out, _ = bounded([sys.executable, "-I", str(base / "scripts" / "pii_scan.py"), "--root", str(tree)], base)
    if code != 0:
        last = (out.strip().splitlines() or [""])[-1]
        raise Refused(f"PII scan of the head tree: exit {code}; {last[:120]}")


def _load(path: Path) -> ModuleType:
    """Main's pii_scan.py loaded from its temporary copy, with no cached `brand` module of the checkout's in the way."""
    spec = importlib.util.spec_from_file_location("wip_pii_scan", path)
    if spec is None or spec.loader is None:
        raise Refused("main's pii_scan.py cannot be loaded")
    saved = sys.modules.pop("brand", None)
    try:
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
    finally:
        sys.modules.pop("brand", None)
        if saved is not None:
            sys.modules["brand"] = saved
    return module
