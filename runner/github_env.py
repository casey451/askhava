"""The runner's real outside world: git plus the GitHub REST API through `gh api` (D-486, BUILD_BRIEF.md:205-292).

GraphQL is blocked from sessions (BUILD_BRIEF.md:255), so every GitHub call here is REST, with one
fallback: `enable_auto_merge` tries the ccr REST route first and, only if that route is absent, `gh pr
merge --auto`, which GitHub serves over GraphQL; under the Actions runner that fallback is reachable,
in a session it fails closed and the cycle reports it.

Where things live:
  - `claude/kit-state`: kit/state.json, STATUS.md, PROGRESS.md, outbox/, inbox/, kit/nightly.json and
    kit/owner_answers.jsonl (governance section 8);
  - `claude/lease`: one file, lease.json = {"lease": {task, session, expires} | null, "leases": {lane:
    {task, session, expires, lane} | null}, "seq": n}; `lease` is lane "1" (runner/state.py, D-515) and a
    file without `leases` (written before D-515) reads as lane "1" holding `lease`. Every
    state push writes a new lease commit and pushes both refs with `git push --atomic`, so the lease
    ref is the lock (BUILD_BRIEF.md:221): notify, inbox and nightly push only to claude/kit-state and
    never race it. When the atomic push is rejected, the runner fetches; if the lease ref and
    kit/state.json are unchanged (only inbox/outbox/nightly files moved), it rebuilds its commit on the
    new head and retries; otherwise it lost the race (K0 review N2).

Commits are built with git plumbing (a private index, `commit-tree`), so the working tree on `main`
is never touched. Every state commit carries `runner_sha` and `starter` in kit/state.json and as
trailers (BUILD_BRIEF.md:216-217).

The worker hook (`run_worker`): if RUNNER_WORKER_CMD is unset the runner hands the claimed task to
the session that started it (outcome `handed_off`; the session works it and calls
`python3 -m runner --after`). If it is set, it is run as a command (split like a shell, no shell)
with these variables:
  RUNNER_TASK_FILE    JSON of the task (kit/tasks.json entry merged with its state entry, plus
                      `gate_answers`: {gate id: "yes"|"no"} for each cleared conditional gate it
                      depends on, T170)
  RUNNER_MODEL        the full model ID the attempt must run on
  RUNNER_MODEL_ALIAS  haiku | sonnet | opus (the worker call takes only the alias, D-170), or the model
                      window's alias for an attempt claimed inside the window (D-2225)
  RUNNER_LOG_FILE     the previous attempt's failure log (may be empty)
  RUNNER_BRANCH       the task branch to commit to (local; the runner pushes it after review)
  RUNNER_RESULT_FILE  where the command writes JSON: {"outcome": ..., "transcripts": [paths],
                      "reviewer_transcripts": [paths], "reset_at": ISO | null, "log": str}
The runner takes only the outcome, reset time, log and transcript paths from that file. Changed
paths come from `git diff --no-renames`, the head from `git rev-parse`, and model evidence and token
usage from the transcripts themselves (D-170: ~/.claude/projects/<proj>/<session>/subagents/agent-<id>.jsonl).
"""

from __future__ import annotations

import dataclasses
import glob
import hashlib
import importlib.util
import json
import os
import random
import re
import shlex
import shutil
import signal
import subprocess
import tempfile
import time
from collections.abc import Callable, Iterable
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from runner import model_window, state as st, status, verify
from runner.cycle import DROP_MARKER, WorkerResult
from runner.update_merge import UpdateMerge
from runner.state import State

Runner = Callable[[list[str], str | None, Path], tuple[int, str, str]]
STATE_FILE = "kit/state.json"
LEASE_FILE = "lease.json"
NIGHTLY_FILE = "kit/nightly.json"
RUNNER_LOG = "kit/runner_log.jsonl"  # one JSON line per cycle, the last 200 kept (D-499)
ANSWERS_FILE = "kit/owner_answers.jsonl"
DECISIONS_FILE = "docs/decisions.md"  # read from main to reserve decision numbers (D-530)
TASKS_FILE = "kit/tasks.json"  # read from main for a plan-fix re-queue (D-677)
PUSH_RETRIES = 3
SMOKE_TIMEOUT = 540  # seconds for scripts/smoke.sh, under the session's 600-second Bash call so the runner answers first (D-1178)
SMOKE_TARGET = "smoke"  # the smoke's own CARGO_TARGET_DIR, a folder of the cached `target` (D-1890)
SMOKE_TREE = "askhava-smoke"  # the smoke worktree's fixed folder under RUNNER_TEMP, the same every run (D-1890)
SMOKE_MTIMES = "smoke-mtimes.json"  # in the smoke's target dir: each tracked file's blob and the mtime it gets (D-1890)
SMOKE_RAN = "smoke-ran"  # beside the smoke's target dir: this job ran the smoke, so backup-runner.yml saves its cache (D-1911)
WARM_TIMEOUT = 1200  # seconds for `--warm-smoke`, a workflow step before the session with no 600-second Bash call (D-2007)
PRE_SMOKE_TIMEOUT = 1500  # seconds for `--pre-smoke`'s scripts/smoke.sh, the start-routine smoke run before the session (D-2028)
SMOKE_PRE = "smoke-pre.json"  # beside the smoke's target dir: the pre-session smoke's result for `smoke_main` (D-2028)
SMOKE_PRE_LOG = "smoke-pre.log"  # beside it too: that smoke's output, printed to the step's job log (D-2028)
PRE_SESSION = "pre-session"  # `smoke_source` when smoke_main used the pre-session result; added to the run's `why` (D-2028)
# The Rust compile steps of scripts/smoke.sh word for word, `cargo test` with `--no-run`: the same flags and
# environment build the same artifacts, so the session's smoke finds them fresh (D-2007; the keeper tests hold
# each one to smoke.sh's text). `cargo sqlx prepare --check` is left out: it forces its own rebuild every run.
WARM_COMMANDS = (
    ("cargo", "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings", "-F", "clippy::allow_attributes_without_reason"),
    ("cargo", "build", "-p", "askhava-server", "--locked"),
    ("cargo", "test", "--workspace", "--locked", "--no-run"),
)
# Seconds of cargo compile time below which a warm-up counts as having built nothing (a head already built):
# it then leaves no SMOKE_RAN, so an idle or paced job saves no new multi-GB smoke entry that would evict the
# `rust-*` entry under GitHub's 10 GB cache limit (D-2007 review).
WARM_SAVE_SECONDS = 60
TOOLCHAIN = re.compile(r'^channel *= *"(.*)"', re.MULTILINE)  # smoke.sh's `sed` over rust-toolchain.toml
PREHANDIN = "scripts/prehandin.sh"  # the pre-hand-in gate; main's copy runs on a submitted head at --after (D-1300)
PREHANDIN_TIMEOUT = 240  # seconds for its --quick part (about 20 s measured), well inside --after's 600-second Bash call
PREHANDIN_RED = "pre-hand-in gate red"  # how a gate failure's log starts; runner/__main__.py `after` holds on it (D-1300)
PREHANDIN_LINES = 30  # the gate output's last lines kept in the failure log, each cut to EXCERPT_WIDTH,
PREHANDIN_CAP = 3000  # and at most this many characters of them (the end kept), so last_failure stays small
UPDATE_ATTEMPTS = 5  # update_state's read-transform-push tries (D-515)
UPDATE_ERRORS = (RuntimeError, ValueError, OSError, subprocess.SubprocessError)  # a failed read or push: one lost try
RUNNER_FILES = (STATE_FILE, NIGHTLY_FILE, ANSWERS_FILE, RUNNER_LOG, "STATUS.md", "PROGRESS.md", "inbox", "outbox")
HEARTBEAT_SECONDS = 60
HEAD_POLLS, HEAD_POLL_SECONDS = 6, 5.0  # enable_auto_merge waits up to 30 s for the PR to show a just-pushed head (D-620)
ALIASES = {"haiku": "haiku", "sonnet": "sonnet", "opus": "opus"}
FAILED_CONCLUSIONS = ("failure", "cancelled", "timed_out")
# The retry worker's failure log (D-582): at most this many characters in all, and per failed job the
# last EXCERPT_LINES lines of its failed step, each cut to EXCERPT_WIDTH characters.
FAILURE_LOG_CAP = 1500
EXCERPT_LINES = 15
EXCERPT_WIDTH = 200
LOG_RETRY_SECONDS = 10  # the wait before the one retry of a refused job-log download (D-1263)
JOB_URL = re.compile(r"/actions/runs/\d+/job/(\d+)")
LOG_STAMP = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z ")
# CSI sequences (private ones with `?` included), OSC strings (hyperlinks, titles) and two-byte
# escapes; then the C0 controls other than tab (D-582, D-1398: a log reaches the retry's prompt).
ANSI = re.compile(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)|[@-Z\\-_])")
CONTROL = re.compile(r"[\x00-\x08\x0b-\x1f\x7f]")
# Defence in depth only: Actions already masks secrets as *** and gitleaks runs with --redact.
TOKEN_LIKE = re.compile(r"gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|x-access-token:[^@\s]+@")
# A best-effort read or write (a failure detail, the branch preparation) that fails is left out, never raised.
BEST_EFFORT_ERRORS = (RuntimeError, ValueError, TypeError, AttributeError, KeyError, OSError, subprocess.SubprocessError)
NOT_BUILT_ON = ("the hand-in does not build on origin/{branch}: the remote task branch has commits this attempt "
                "did not build on, so its push cannot fast-forward (D-583)")
JOB_TOKEN_READS = ("/check-runs", "/actions/jobs/")  # REST reads that need the job token (D-502, D-582)
PLAN_MERGE = re.compile(r"^\S+ (?:[Pp]lan|[Hh]arness):")  # plan and harness merge subjects (D-1462)
RUNNER_WORKFLOW = "backup-runner.yml"  # the workflow a refill run is dispatched on, one run per lane (D-723)
# The comments on a pull request the runner closes while preparing a retry's branch (D-583). They are
# posted only once the pull request is closed. The marker lets a later attempt recognise a pull request
# the runner closed to drop auto-merge even when recording its number in the state failed (DROP_MARKER, in
# runner/cycle.py, which also puts it on a pull request closed for an unreviewed head; D-584).
AUTO_MERGE_NOTE = ("Closed by the runner before the next attempt: auto-merge was on for `{branch}`, so a push of "
                   "unreviewed work could have merged. The attempt continues on this branch; the runner opens a "
                   "fresh pull request after its review (D-583).\n\n" + DROP_MARKER)
CONFLICT_NOTE = ("Closed by the runner: merging main into `{branch}` conflicted, so the next attempt starts fresh "
                 "from main on `{new}` (D-583). This branch is kept for evidence.")


def run_command(args: list[str], stdin: str | None, cwd: Path) -> tuple[int, str, str]:
    """subprocess.run without a shell; returns (exit code, stdout, stderr).

    Remote git and gh calls go first with the long-lived token (D-519) and, if that is refused, once
    more exactly as before with the session's own credentials, so the change can never do worse."""
    long_args = _long_token_args(args)
    if long_args is not None:
        done = _run(long_args, stdin, cwd, _long_token_env(long_args))
        if done.returncode == 0:
            return done.returncode, done.stdout, done.stderr
    smoke = args[:2] == ["bash", "scripts/smoke.sh"]
    done = _run(args, stdin, cwd, _smoke_env() if smoke else _env_for(args), _timeout_for(args))
    return done.returncode, done.stdout, done.stderr


def _timeout_for(args: list[str]) -> int:
    """SMOKE_TIMEOUT for scripts/smoke.sh (D-1178), PREHANDIN_TIMEOUT for the pre-hand-in gate (D-1300), else 600."""
    if args[:2] == ["bash", "scripts/smoke.sh"]:
        return SMOKE_TIMEOUT
    return PREHANDIN_TIMEOUT if any(a.endswith("/prehandin.sh") or a == PREHANDIN for a in args[:4]) else 600


def _pii_patterns() -> tuple[re.Pattern[str], ...]:
    """scripts/pii_scan.py's key, phone and email patterns (standard library only, loaded by path), or ()
    when it cannot be loaded, so a missing scanner never stops a hand-in (D-1300)."""
    spec = importlib.util.spec_from_file_location("pii_scan_patterns", Path(__file__).resolve().parent.parent / "scripts" / "pii_scan.py")
    if spec is None or spec.loader is None:
        return ()
    try:
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        return (*mod.KEY_PATTERNS.values(), mod.PHONE, mod.PHONE_PLUS, mod.EMAIL)
    except (OSError, AttributeError, SyntaxError, ImportError):
        return ()


GATE_REDACT = _pii_patterns()


def _redact_gate_row(row: str) -> str:
    """One line of the gate's output for the failure log: ANSI removed, GitHub tokens (TOKEN_LIKE) and every
    key, phone and email shape scripts/pii_scan.py knows (GATE_REDACT) replaced, so a check that echoes what
    it found never carries it into kit/state.json (D-1300)."""
    row = TOKEN_LIKE.sub("[redacted]", ANSI.sub("", row))
    for pattern in GATE_REDACT:
        row = pattern.sub("[redacted]", row)
    return row


def _smoke_env() -> dict[str, str]:
    """The smoke test's environment: CARGO_TARGET_DIR is the smoke's own target dir (`smoke_target_dir`), and
    CARGO_INCREMENTAL is 0 as in `Ci`, since the cache keeps that dir and rustc's incremental data was about a
    third of it (4.7 of 15 GB measured, D-1890)."""
    return {**os.environ, "CARGO_TARGET_DIR": str(smoke_target_dir()), "CARGO_INCREMENTAL": "0"}


def smoke_target_dir() -> Path:
    """The smoke's own target dir, `smoke` inside the checkout's `target` (or inside an explicit
    CARGO_TARGET_DIR), so the job's Rust cache keeps it (D-1178) but the worker's builds never share it
    (D-1890): a member artifact built in the smoke worktree has that worktree's path compiled in through
    `env!("CARGO_MANIFEST_DIR")`, which is why D-1451 cleaned the members after every smoke."""
    base = os.environ.get("CARGO_TARGET_DIR") or str(Path(os.environ.get("GITHUB_WORKSPACE") or Path.cwd()) / "target")
    return Path(base) / SMOKE_TARGET


def smoke_tree_dir() -> Path:
    """The smoke worktree's path: SMOKE_TREE under RUNNER_TEMP (the system temp dir outside Actions). It is
    the same every run, so the paths compiled into the smoke's cached artifacts stay true (D-1890)."""
    return Path(os.environ.get("RUNNER_TEMP") or tempfile.gettempdir()) / SMOKE_TREE


def pin_smoke_mtimes(tree: Path, index: str, manifest: Path, now_ns: int) -> int:
    """Give every tracked file of the smoke worktree, and every folder holding one, a modification time Cargo
    can trust (D-1890); returns how many got the run's time.

    Cargo calls a workspace member's artifact fresh when no source file (nor any entry of a folder a build
    script watches, such as `askhava-core`'s `migrations`) is newer than its last build, and a fresh checkout
    makes everything new, so every smoke rebuilt every member. `manifest` keeps, per path, what it last held
    (a file's blob; a folder's `/`-suffixed key, the hash of its entry names) and the time it was given: an
    unchanged one keeps that time, which predates every build that read it; a new or changed one gets the
    run's time, later than `now_ns` and every recorded time, so after every build so far even with a clock
    that stepped back. A changed existing file under `.sqlx/` (offline query data no dep-info lists) gives
    every path the run's time; an added or removed one does not (D-1911): a new query file comes with the
    source change that names it, which rebuilds that crate, and a removed one is read by no build. The manifest is deleted before the first change and written back only when every
    path is pinned, so a failure part-way (see `stamp_tree_now`) or a red or timed-out run never leaves a
    record the tree does not match; an unreadable one makes everything new (a full rebuild). `index` is
    `git ls-files -s -z`."""
    old = _read_mtimes(manifest)
    manifest.unlink(missing_ok=True)
    held = _smoke_entries(tree, index)
    run_ns = max([now_ns, *(v[1] + 1 for v in old.values())])
    changed = {k for k, v in held.items() if old.get(k, [None])[0] != v} | (set(old) - set(held))
    query_data = any(".sqlx" in k.split("/") and not k.endswith("/") and k in old and k in held for k in changed)
    new = {}
    for path, what in held.items():
        stamp = run_ns if query_data or path in changed else old[path][1]
        os.utime(tree / path.rstrip("/") if path != "/" else tree, ns=(stamp, stamp))
        new[path] = [what, stamp]
    manifest.parent.mkdir(parents=True, exist_ok=True)
    part = manifest.with_suffix(".part")
    part.write_text(json.dumps(new), encoding="utf-8")
    part.replace(manifest)
    return sum(1 for v in new.values() if v[1] == run_ns)


def _mark_smoke_ran() -> None:
    """Leave SMOKE_RAN beside the smoke's target dir (outside every cache entry), so backup-runner.yml saves
    the smoke cache only from a job that ran the smoke (D-1911); best effort."""
    try:
        (smoke_target_dir().parent / SMOKE_RAN).write_text(time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), encoding="utf-8")
    except OSError:
        return


def run_warm(args: list[str], cwd: Path, timeout: float) -> int:
    """One `--warm-smoke` command (D-2007) in the smoke's environment (`_smoke_env`) plus smoke.sh's
    `SQLX_OFFLINE=true`, its output going to the job log. Past `timeout` its whole process group is killed, so no
    rustc is left writing into the smoke target while the session's smoke builds there, and TimeoutExpired is
    raised; returns the exit code."""
    proc = subprocess.Popen(args, cwd=cwd, env={**_smoke_env(), "SQLX_OFFLINE": "true"}, start_new_session=True)
    try:
        return proc.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        proc.wait()
        raise


def run_pre_smoke(args: list[str], cwd: Path, timeout: float, out: Path) -> int:
    """`--pre-smoke`'s scripts/smoke.sh (D-2028) in the smoke's environment (`_smoke_env`, as `run_command` gives
    the in-session smoke), its output going to the file `out` (no pipe a left-over child could hold open). Past
    `timeout` its whole process group is killed, as `run_warm`'s, and TimeoutExpired is raised; returns the exit code."""
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("wb") as sink:
        proc = subprocess.Popen(args, cwd=cwd, env=_smoke_env(), stdin=subprocess.DEVNULL, stdout=sink,
                                stderr=subprocess.STDOUT, start_new_session=True)
        try:
            return proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()
            raise


def job_id() -> str:
    """This Actions job's run, `<GITHUB_RUN_ID>-<GITHUB_RUN_ATTEMPT>`, or "" outside Actions: the pre-session
    smoke's result is used only by the job that wrote it (D-2028; one `backup` job per run)."""
    run = os.environ.get("GITHUB_RUN_ID", "")
    return f"{run}-{os.environ.get('GITHUB_RUN_ATTEMPT', '')}" if run else ""


def write_pre_smoke(result: dict[str, Any]) -> None:
    """Write SMOKE_PRE beside the smoke's target dir (outside every cache entry) whole or not at all (D-2028)."""
    path = smoke_target_dir().parent / SMOKE_PRE
    part = path.with_name(SMOKE_PRE + ".part")
    path.parent.mkdir(parents=True, exist_ok=True)
    part.write_text(json.dumps(result), encoding="utf-8")
    part.replace(path)


def warm_commands(tree: Path) -> list[list[str]]:
    """What `--warm-smoke` runs in the smoke tree (D-2007): smoke.sh's toolchain install for the channel in
    `rust-toolchain.toml` (when it names one), then WARM_COMMANDS; nothing without a `Cargo.toml`."""
    if not (tree / "Cargo.toml").is_file():
        return []
    try:
        found = TOOLCHAIN.search((tree / "rust-toolchain.toml").read_text(encoding="utf-8"))
    except OSError:
        found = None
    install = [["rustup", "toolchain", "install", found.group(1), "--profile", "minimal", "-c", "clippy", "-c", "rustfmt"]] if found else []
    return [*install, *(list(cmd) for cmd in WARM_COMMANDS)]


def stamp_tree_now(tree: Path, manifest: Path) -> None:
    """After a failed pin (D-1890): no manifest, and every file and folder of the tree at the current time, so
    nothing a half-done pin set back in time can make an old artifact look fresh (a full rebuild)."""
    manifest.unlink(missing_ok=True)
    stamp = time.time_ns()
    for top, dirs, files in os.walk(tree):
        for name in [*files, *dirs]:
            if not os.path.islink(os.path.join(top, name)):
                os.utime(os.path.join(top, name), ns=(stamp, stamp))
    os.utime(tree, ns=(stamp, stamp))


ARTIFACT = re.compile(r"^(.+?)-[0-9a-f]{16}(\..*)?$")  # a target-dir entry: a unit's name, its hash, an extension
SEED_DIRS = ("deps", "build", ".fingerprint")  # the folders of `debug` that hold dependency artifacts
SEED_MARGIN = 5 * 1024**3  # bytes of disk left free beyond the copy, or the smoke target is not seeded


def seed_files(debug: Path, members: set[str]) -> list[Path]:
    """The worker build's dependency artifacts under `debug` (its `debug` folder): every entry of SEED_DIRS
    whose unit name is not a workspace package or target (`members`, with `-` and `_` spellings; `lib`
    prefixes stripped). Member artifacts and anything unrecognised stay out: they only rebuild (D-1890)."""
    names = members | {m.replace("-", "_") for m in members}
    out: list[Path] = []
    for folder in SEED_DIRS:
        for entry in sorted((debug / folder).glob("*")) if (debug / folder).is_dir() else []:
            match = ARTIFACT.match(entry.name)
            unit = match.group(1) if match else ""
            if unit and unit not in names and not (unit.startswith("lib") and unit[3:] in names):
                out.append(entry)
    return out


def copy_seed(debug: Path, into: Path, entries: list[Path]) -> None:
    """Copy `entries` (real copies, never hard links: Cargo rewrites fingerprint and dep-info files in place,
    and a shared one would let the smoke's writes reach the worker's build, the T057 failure, D-1451)."""
    for entry in entries:
        dest = into / entry.relative_to(debug)
        dest.parent.mkdir(parents=True, exist_ok=True)
        if entry.is_dir():
            shutil.copytree(entry, dest, symlinks=True, dirs_exist_ok=True)
        else:
            shutil.copy2(entry, dest)


def _tree_size(entries: list[Path]) -> int:
    """The bytes under `entries` (files and folders, links not followed)."""
    total = 0
    for entry in entries:
        if entry.is_file():
            total += entry.stat().st_size
            continue
        for top, _, files in os.walk(entry):
            total += sum(os.lstat(os.path.join(top, f)).st_size for f in files)
    return total


def _smoke_entries(tree: Path, index: str) -> dict[str, str]:
    """What each tracked regular file holds (its blob) and, under its path plus `/` (the root as `/`), what
    each folder above one holds (a hash of its tracked entry names), files first and the deepest folder
    first. Submodules and links are left out: Cargo reads the file a link points at."""
    files: dict[str, str] = {}
    folders: dict[str, set[str]] = {}
    for row in filter(None, index.split("\0")):
        meta, _, path = row.partition("\t")
        mode, blob = (meta.split() + ["", ""])[:2]
        if mode == "160000" or (tree / path).is_symlink() or not (tree / path).is_file():
            continue
        files[path] = blob
        parts = path.split("/")
        for depth in range(len(parts)):
            folders.setdefault("/".join(parts[:depth]) + "/", set()).add(parts[depth])
    held = {k: hashlib.sha256("\n".join(sorted(v)).encode()).hexdigest() for k, v in folders.items()}
    ordered = sorted(held, key=lambda k: -k.count("/"))
    return {**files, **{k: held[k] for k in ordered}}


def _member_names(metadata: dict[str, Any]) -> set[str]:
    """Every workspace package's name and each of its targets' names, from `cargo metadata --no-deps`."""
    names: set[str] = set()
    for package in metadata["packages"]:
        names.add(package["name"])
        names.update(target["name"] for target in package["targets"])
    return names


def _read_mtimes(manifest: Path) -> dict[str, list[Any]]:
    """The smoke's mtime manifest, or {} when it is missing or unreadable (every file then counts as new)."""
    try:
        data = json.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    if not isinstance(data, dict):
        return {}
    return {k: v for k, v in data.items() if isinstance(v, list) and len(v) == 2 and isinstance(v[1], int)}


def _run(args: list[str], stdin: str | None, cwd: Path, env: dict[str, str] | None, timeout: int = 600) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, input=stdin, capture_output=True, text=True, cwd=cwd, timeout=timeout, check=False, env=env)


REMOTE_GIT = ("fetch", "push", "ls-remote")


def _long_token_args(args: list[str]) -> list[str] | None:
    """The call to make with RUNNER_PUSH_TOKEN, or None when it does not apply (D-519).

    The Claude App's token lasts one hour, so a session whose worker runs longer could no longer
    fetch, push or call the API at hand-in (T005, October 29, 2026). Remote git calls name the
    repository URL instead of `origin` so the credential helper, not the action's header, answers;
    gh calls other than the job-token calls (check runs and Actions jobs, RUNNER_CHECKS_TOKEN, D-502,
    D-582, and the refill dispatch, D-723) use the token as GH_TOKEN."""
    token, repo = os.environ.get("RUNNER_PUSH_TOKEN", ""), os.environ.get("GITHUB_REPOSITORY", "")
    if not token or not repo:
        return None
    if args[:1] == ["git"] and len(args) > 1 and args[1] in REMOTE_GIT:
        url = f"https://github.com/{repo}.git"
        return [url if a == "origin" else a for a in args]
    if args[:1] == ["gh"] and not _job_token_call(args):
        return list(args)
    return None


def _job_token_read(args: list[str]) -> bool:
    """A `gh api` read of check runs (D-502) or of an Actions job and its log (D-582)."""
    return args[:2] == ["gh", "api"] and any(part in args[-1] for part in JOB_TOKEN_READS)


def _job_token_call(args: list[str]) -> bool:
    """A call made with the job token: a job-token read, or a `gh workflow run` of the runner's own
    workflow, which the job token may start (`actions: write`, as the workflow's chain step does, D-723)."""
    return _job_token_read(args) or args[:4] == ["gh", "workflow", "run", RUNNER_WORKFLOW]


def _long_token_env(args: list[str]) -> dict[str, str]:
    if args[:1] == ["gh"]:
        return dict(os.environ, GH_TOKEN=os.environ["RUNNER_PUSH_TOKEN"])
    return _git_token_env()


def _env_for(args: list[str]) -> dict[str, str] | None:
    """Check-run and Actions-job reads use the job's own token (D-502, D-582).

    Inside the Claude Code action, GH_TOKEN is the Claude GitHub App's token, which has no Checks
    permission, so `commits/<sha>/check-runs` answers 403. The workflow passes the job token (which has
    `checks: read`, and `actions: write`, which includes the read of a job and its log) as
    RUNNER_CHECKS_TOKEN; it is used only for those reads and the refill dispatch (D-723), and never put
    in argv.
    """
    token = os.environ.get("RUNNER_CHECKS_TOKEN", "")
    if token and _job_token_call(args):
        return dict(os.environ, GH_TOKEN=token)
    return _push_env(args)


def _push_env(args: list[str]) -> dict[str, str] | None:
    """Task-branch pushes use RUNNER_PUSH_TOKEN when it is set (D-509).

    The Claude App's token cannot create or update files under `.github/workflows/`, so a task that
    owns one (T056, T074, T138, T169, T172, T173) could never be pushed. The workflow passes a
    repository-scoped token with Contents and Workflows write as RUNNER_PUSH_TOKEN; it reaches git
    through a credential helper reading the environment, so it is never in argv or a config file, and
    the action's own auth header is reset for this one command.
    """
    if not os.environ.get("RUNNER_PUSH_TOKEN") or args[:2] != ["git", "push"]:
        return None
    if not any(a.startswith("https://github.com/") for a in args):
        return None
    return _git_token_env()


def _git_token_env() -> dict[str, str]:
    """git config, through the environment only, that answers credentials with RUNNER_PUSH_TOKEN."""
    helper = '!f() { echo username=x-access-token; echo "password=$RUNNER_PUSH_TOKEN"; }; f'
    pairs = [("http.https://github.com/.extraheader", ""), ("credential.helper", ""), ("credential.helper", helper)]
    env = dict(os.environ, GIT_CONFIG_COUNT=str(len(pairs)))
    for i, (key, value) in enumerate(pairs):
        env[f"GIT_CONFIG_KEY_{i}"], env[f"GIT_CONFIG_VALUE_{i}"] = key, value
    return env


def parse_inbox_file(text: str) -> tuple[str, str]:
    """(outbox id, answer text) from an inbox file written by inbox.yml (kit/outbox_format.md)."""
    if not text.startswith("---\n"):
        return "", text.strip()
    head, _, body = text[4:].partition("\n---\n")
    outbox_id = ""
    for line in head.splitlines():
        key, _, value = line.partition(":")
        if key.strip() == "outbox_id" and value.strip() not in ("", "null"):
            outbox_id = str(json.loads(value.strip())) if value.strip().startswith('"') else value.strip()
    return outbox_id, body.strip()


def log_excerpt(text: str, lines: int = EXCERPT_LINES) -> list[str]:
    """The last lines of a job log's failed step, up to and including its first `##[error]` line (D-582).

    Timestamps and colour codes are dropped. Everything between `##[group]` and `##[endgroup]` is
    left out: that is where Actions prints the step's command and its `env:` block, so no environment
    dump reaches the excerpt. Token-shaped strings are replaced as a second guard. [] when the log
    holds no error line."""
    rows = [LOG_STAMP.sub("", CONTROL.sub("", ANSI.sub("", row))) for row in text.splitlines()]
    end = next((i for i, row in enumerate(rows) if row.startswith("##[error]")), None)
    if end is None:
        return []
    start = max((i for i in range(end) if rows[i].startswith("##[group]Run ")), default=0)
    kept: list[str] = []
    depth = 0  # a counter, not a toggle: a group opened inside a group never ends the outer one early
    for row in rows[start:end + 1]:
        if row.startswith(("##[group]", "##[endgroup]")):
            depth = depth + 1 if row.startswith("##[group]") else max(depth - 1, 0)
            continue
        if not depth and row.strip():
            kept.append(TOKEN_LIKE.sub("[redacted]", row)[:EXCERPT_WIDTH])
    return kept[-lines:]


def fit_failure_log(base: str, details: list[tuple[str, list[str]]], cap: int = FAILURE_LOG_CAP) -> str:
    """`base` (the "name: conclusion" list) and, per failed job, its header line and log excerpt, in at
    most `cap` characters (D-582). An excerpt that does not fit loses its oldest lines first, so the
    error line at its end survives longest."""
    parts = [base[:cap]]
    for header, excerpt in details:
        left = cap - len("\n".join(parts)) - 1
        if left <= 0:
            break
        rows = list(excerpt)
        while rows and len("\n".join([header, *rows])) > left:
            rows.pop(0)
        parts.append("\n".join([header, *rows])[:left])
    return "\n".join(parts)


def annotation_lines(notes: Any, lines: int = EXCERPT_LINES) -> list[str]:
    """The failure-level check-run annotations as excerpt lines, the last `lines` kept (D-1263).

    Each message line becomes one row, led by `path:start_line: ` (`path: ` without a start line) when
    the annotation names a file
    (not the bare `.github` that Actions gives its own "Process completed with exit code 1."). Rows
    are cut to EXCERPT_WIDTH and token-shaped strings replaced, as in `log_excerpt`. [] for anything
    that is not a list of annotations."""
    kept: list[str] = []
    for note in notes if isinstance(notes, list) else []:
        if not isinstance(note, dict) or note.get("annotation_level") != "failure":
            continue
        path = str(note.get("path") or "")
        line = note.get("start_line")
        where = (f"{path}:{line}: " if line is not None else f"{path}: ") if path and path != ".github" else ""
        for row in str(note.get("message") or "").splitlines():
            if row.strip():
                kept.append(TOKEN_LIKE.sub("[redacted]", where + row)[:EXCERPT_WIDTH])
    return kept[-lines:]


def _job_id(run: dict[str, Any]) -> str | None:
    """The Actions job behind a check run, from its details URL; None for a check from another app."""
    found = JOB_URL.search(str(run.get("details_url") or ""))
    return found.group(1) if found else None


def state_diff(before: State, after: State) -> dict[str, Any]:
    """What a cycle changed: task status moves and top-level keys (printed by `python3 -m runner`)."""
    old = {t["id"]: t.get("status") for t in before.get("tasks", [])}
    moves = {t["id"]: f"{old.get(t['id'])} -> {t.get('status')}" for t in after.get("tasks", []) if old.get(t["id"]) != t.get("status")}
    skip = {"tasks", "updated_at", "runner_sha", "starter"}
    keys = sorted(k for k in set(before) | set(after) if k not in skip and before.get(k) != after.get(k))
    return {"tasks": moves, "keys": {k: after.get(k) for k in keys}}


def _stage_counts(state: State) -> list[str]:
    stages: dict[str, list[int]] = {}
    for task in state.get("tasks", []):
        counts = stages.setdefault(str(task.get("stage", "kit")), [0, 0])
        counts[0] += task.get("status") == st.DONE
        counts[1] += 1
    return [f"- stage {s}: {d} of {n} done" for s, (d, n) in sorted(stages.items())]


def _waits_for(task: dict[str, Any]) -> str:
    """What a cleanup task's stage proof still waits for (runner.cycle.record_proofs, D-551), or ''."""
    waits = task.get("proof_waits_for") or []
    return f" (the proof waits for: {'; '.join(str(w) for w in waits)})" if waits else ""


def _plan_gap_line(task: dict[str, Any]) -> str:
    """A task held before its claim by the plan-health check (runner.cycle.plan_health_pass, D-1350)."""
    gap = task.get("plan_gap") or {}
    problems = "; ".join(str(p) for p in gap.get("problems") or [])
    return f"- PLAN GAP {task['id']} (since {gap.get('at')}, no attempt used; a plan change fixes it): {problems}"


def _attention(state: State, now: datetime) -> list[str]:
    lines = [f"- BLOCKED {t['id']}: {t.get('reason', '')}{_waits_for(t)}" for t in state.get("tasks", []) if t.get("status") == st.BLOCKED]
    lines += [f"- Waiting for Casey: {t['id']}" for t in state.get("tasks", []) if t.get("status") == st.WAITING_FOR_OWNER]
    lines += [_plan_gap_line(t) for t in state.get("tasks", []) if t.get("plan_gap") and t.get("status") not in st.SATISFIED]
    lines += [f"- Deviation {d.get('id')}: {d.get('rule')} ({d.get('owner')}, until {d.get('expiry')})" for d in st.open_deviations(state, now)]
    red = state.get("main_red")
    if red:
        lines.insert(0, f"- MAIN IS RED: {red.get('check') if isinstance(red, dict) else red}; only FIX-MAIN runs")
    return lines or ["- nothing"]


def _running(state: State) -> list[str]:
    """The "Running" lines: lane 1's lease as before, then every other lane's lease, named (D-620)."""
    lease = state.get("lease") or {}
    lines = [f"- {lease.get('task', 'nothing')}" + (f" (lease until {lease['expires']})" if lease else "")]
    for lane, held in sorted(st.leases_of(state).items()):
        if lane != st.LANE_ONE and held:
            lines.append(f"- lane {lane}: {held.get('task')} (lease until {held.get('expires')})")
    if state.get("lanes_fallback"):
        lines.append(f"- one lane only since {state['lanes_fallback'].get('at')} (lane fall back; `python3 -m runner --restore-lanes`)")
    return lines


@dataclasses.dataclass(frozen=True)
class Board:
    """What the owner's dashboard reads beside the state (runner/status.py, D-1462): the task graph, the
    time of the last cycle-log line, the strict plan check's problem lines (empty when green or not run)
    and recent plan and harness merges on `main` (subject lines)."""

    graph: tuple[dict[str, Any], ...] = ()
    last_run: datetime | None = None
    strict: tuple[str, ...] = ()
    merges: tuple[str, ...] = ()


def _redact(text: str) -> str:
    """A failure text for STATUS.md: as `_redact_gate_row` (tokens and every key, phone and email shape the
    PII scanner knows), with control characters removed too."""
    return _redact_gate_row(CONTROL.sub("", ANSI.sub("", text)))


def _safe(render: Callable[[], list[str]], what: str) -> list[str]:
    """A dashboard section, or one line naming the error: a malformed state field never stops a cycle (D-1462)."""
    try:
        return render()
    except Exception as exc:  # noqa: BLE001  # reason: STATUS.md is a view; any error in it must not fail the state push
        return [f"- {what} unavailable: {type(exc).__name__}"]


def _day_sections(state: State, board: Board, now: datetime) -> list[str]:
    merges = [f"- {m}" for m in board.merges] or ["- none"]
    rows = _safe(lambda: status.last_day(state, list(board.graph), now, _redact), "last 24 hours")
    return ["## Last 24 hours", *rows, "", "### Plan and harness merges on main", *merges, ""]


def _needs_attention(state: State, board: Board, now: datetime) -> list[str]:
    lines = [line for line in _safe(lambda: _attention(state, now), "attention list") if line != "- nothing"]
    lines += _safe(lambda: status.attention_extra(state, board.strict), "plan check flags")
    return lines or ["- nothing"]


def render_status(state: State, progress_md: str, now: datetime, pacing: str = "", board: Board | None = None) -> str:
    """STATUS.md: the owner's dashboard (runner/status.py, D-1462: verdict, done count, stage, lane, week,
    last run, keeper; the last 24 hours), then progress in verified requirements (governance 6), the
    pacing line (scripts/pacing.py), then BUILD_BRIEF.md:276-281. Each dashboard section that fails to
    render becomes one "unavailable" line instead."""
    board = board or Board()
    usage = (state.get("usage") or {}).get("last_reading")
    parts = [
        "# Ask Hava build status", "", f"Updated {st.iso(now)} by runner {str(state.get('runner_sha', ''))[:12]} ({state.get('starter', '')}).",
        "", *_safe(lambda: status.dashboard(state, list(board.graph), now, board.last_run), "dashboard"),
        "", "## Needs attention", *_needs_attention(state, board, now), "", *_day_sections(state, board, now),
        "## Progress (requirements verified)", "", progress_md.strip() or "Progress unavailable.", "", "## Pacing", f"- {pacing or 'no reading yet'}",
        "", "## Tasks done (throughput)", *_safe(lambda: _stage_counts(state), "throughput"),
        "", "## Running", *_safe(lambda: _running(state), "running list"),
        "", "## Last usage reading", f"- {json.dumps(usage) if usage else 'none yet'}", "",
    ]
    return "\n".join(parts)


def new_answers(base: State, new: State) -> list[dict[str, Any]]:
    """Owner answers recorded since base, for kit/owner_answers.jsonl."""
    seen = base.get("owner_answers", [])
    return [a for a in new.get("owner_answers", []) if a not in seen]


class GitHubEnv:
    """runner.cycle.Env over git and `gh api` (see the module docstring for the contract)."""

    def __init__(
        self, root: Path, graph: list[dict[str, Any]], outbox_dir: Path, *, repo: str, starter: str,
        runner_sha: str, state_ref: str = "claude/kit-state", lease_ref: str = "claude/lease",
        dry_run: bool = False, run: Runner = run_command, clock: Callable[[], datetime] | None = None,
        pacing_line: Callable[[State], str] | None = None,
    ) -> None:
        self.pacing_line = pacing_line
        self.root, self.graph, self.outbox_dir, self.repo = root, graph, outbox_dir, repo
        self.starter, self.runner_sha, self.dry_run, self._run = starter, runner_sha, dry_run, run
        self.state_ref, self.lease_ref = state_ref, lease_ref
        self.clock = clock or (lambda: datetime.now(timezone.utc))
        self.state_head: str | None = None
        self.lease_head: str | None = None
        self.state_blob = ""
        self.first_state: State | None = None
        self.last_state: State | None = None
        self.log: list[str] = []
        self._progress: dict[str, str] = {}
        self._strict: tuple[str, ...] | None = None  # strict_check's result, once per process (D-1462)
        self.sleep: Callable[[float], None] = time.sleep  # update_state's wait between tries; tests replace it
        self.prepared: dict[str, Any] = {}  # what prepare_branch made for this attempt, for the hand-off (D-583)
        self.last_rejection = ""  # the last rejected branch push's git output (push_rejection, D-583)
        self.updates = UpdateMerge(self._run, self.root, self._identity)  # stage C's update merge (D-620)
        self.warm_run: Callable[[list[str], Path, float], int] = run_warm  # `--warm-smoke`'s cargo runs; tests replace it (D-2007)
        self.pre_run: Callable[[list[str], Path, float, Path], int] = run_pre_smoke  # `--pre-smoke`'s smoke.sh; tests replace it (D-2028)
        self.smoke_timed_out = False  # the last smoke_main outran its budget (D-1178)
        self.smoke_source = ""  # PRE_SESSION when the last smoke_main used the pre-session result, else "" (D-2028)
        self.smoke_seconds: int | None = None  # how long the last smoke_main's smoke.sh ran, wherever it ran (D-2028)

    # ---- git plumbing

    def git(self, *args: str, stdin: str | None = None, check: bool = True, env: dict[str, str] | None = None) -> str:
        cmd = ["git", *args]
        if env:
            cmd = ["env", *[f"{k}={v}" for k, v in env.items()], *cmd]
        code, out, err = self._run(cmd, stdin, self.root)
        if check and code != 0:
            raise RuntimeError(f"git {' '.join(args[:3])} failed: {err.strip()[:300]}")
        return out.strip() if code == 0 else ""

    def _fetch(self, ref: str) -> str | None:
        local = f"refs/runner/{ref.replace('/', '-')}"
        code, _, _ = self._run(["git", "fetch", "--quiet", "origin", f"+refs/heads/{ref}:{local}"], None, self.root)
        return self.git("rev-parse", local) if code == 0 else None

    def _show(self, commit: str | None, path: str) -> str | None:
        if not commit:
            return None
        code, out, _ = self._run(["git", "show", f"{commit}:{path}"], None, self.root)
        return out if code == 0 else None

    def _commit(self, parent: str | None, writes: dict[str, str], removes: Iterable[str], message: str) -> str:
        """A commit of parent's tree with files written and removed, built in a private index."""
        with tempfile.TemporaryDirectory() as tmp:
            env = {"GIT_INDEX_FILE": str(Path(tmp) / "index"), **self._identity()}
            self.git("read-tree", *([parent] if parent else ["--empty"]), env=env)
            for path, text in sorted(writes.items()):
                blob = self.git("hash-object", "-w", "--stdin", stdin=text)
                self.git("update-index", "--add", "--cacheinfo", f"100644,{blob},{path}", env=env)
            for path in removes:
                self.git("update-index", "--force-remove", path, env=env)
            tree = self.git("write-tree", env=env)
            parents = ["-p", parent] if parent else []
            return self.git("commit-tree", tree, *parents, "-m", message, env=env)

    def _identity(self) -> dict[str, str]:
        name = self.git("config", "user.name", check=False) or "Ask Hava runner"
        email = self.git("config", "user.email", check=False) or "runner@users.noreply.github.com"
        return {"GIT_AUTHOR_NAME": name, "GIT_AUTHOR_EMAIL": email, "GIT_COMMITTER_NAME": name, "GIT_COMMITTER_EMAIL": email}

    # ---- state and lease

    def read_state(self) -> State:
        """kit/state.json from claude/kit-state, with the lease taken from claude/lease once that ref exists."""
        self.state_head = self._fetch(self.state_ref)
        self.lease_head = self._fetch(self.lease_ref)
        raw = self._show(self.state_head, STATE_FILE)
        if raw is None:
            raise RuntimeError(f"{STATE_FILE} not found on {self.state_ref}")
        self.state_blob = self.git("rev-parse", f"{self.state_head}:{STATE_FILE}")
        state: State = json.loads(raw)
        lease_raw = self._show(self.lease_head, LEASE_FILE)
        if lease_raw is not None:
            doc = json.loads(lease_raw)
            state["lease"] = doc.get("lease")
            state["leases"] = doc.get("leases") if isinstance(doc.get("leases"), dict) else {}
        state.pop("lease_on", None)
        state = st.with_leases_map(state)  # `leases` always present, lane "1" taken from `lease` (D-515)
        self.first_state = self.first_state or json.loads(json.dumps(state))
        self.last_state = state
        return state

    def _lease_seq(self) -> int:
        raw = self._show(self.lease_head, LEASE_FILE)
        return int(json.loads(raw).get("seq", 0)) if raw else 0

    def _state_files(self, new: State, base: State) -> tuple[dict[str, str], list[str]]:
        now = self.clock()
        stamped = dict(new, runner_sha=self.runner_sha, starter=self.starter, updated_at=st.iso(now), lease=None, lease_on=self.lease_ref)
        stamped.pop("leases", None)  # the lanes live in lease.json only, like the lease (D-515)
        pace = self.pacing_line(new) if self.pacing_line else ""
        status = render_status(stamped | {"lease": new.get("lease"), "leases": st.leases_of(new)}, self.progress_markdown(new), now, pace,
                               self.board(now))
        writes = {STATE_FILE: json.dumps(stamped, indent=1) + "\n", "STATUS.md": status}
        progress = self._show(self.state_head, "PROGRESS.md") or "# Progress\n"
        line = f"- {st.iso(now)} runner {self.runner_sha[:12]} ({self.starter}): {json.dumps(state_diff(base, new)['tasks'])}"
        paced = (new.get("pacing") or {}).get("last")
        line += (f"; {paced}" if paced and paced != (base.get("pacing") or {}).get("last") else "") + "\n"
        writes["PROGRESS.md"] = progress.rstrip("\n") + "\n" + line
        answers = new_answers(base, new)
        if answers:
            old = self._show(self.state_head, ANSWERS_FILE) or ""
            writes[ANSWERS_FILE] = old + "".join(json.dumps(a, sort_keys=True) + "\n" for a in answers)
        for path in sorted(self.outbox_dir.glob("*.md")) if self.outbox_dir.exists() else []:
            writes[f"outbox/{path.name}"] = path.read_text(encoding="utf-8")
        return writes, []

    def board(self, now: datetime) -> Board:
        """The dashboard's readings beside the state (D-1462); each one best effort, never raised."""
        return Board(tuple(self.graph), self._last_run(), self.strict_check(), self._plan_merges(now))

    def _last_run(self) -> datetime | None:
        """The time of the last kit/runner_log.jsonl line on the state branch (the last cycle that ended)."""
        try:
            lines = (self._show(self.state_head, RUNNER_LOG) or "").strip().splitlines()
            return st.parse(json.loads(lines[-1])["at"]) if lines else None
        except (*BEST_EFFORT_ERRORS, json.JSONDecodeError):
            return None

    def strict_check(self) -> tuple[str, ...]:
        """`gen_prompts.py --check --strict` (about 1.3 s), run once per runner process, which is every
        cycle and every --after: its problem lines when red, () when green or when it could not run (no
        script, a crash). STATUS.md judges them against the state (`status.strict_line`)."""
        if self._strict is None:
            code, out, _ = self._run(["python3", "scripts/gen_prompts.py", "--check", "--strict"], None, self.root)
            problems = tuple(line[6:] for line in out.splitlines() if line.startswith("ERROR "))
            self._strict = problems if code == 1 else ()
        return self._strict

    def _plan_merges(self, now: datetime) -> tuple[str, ...]:
        """Subjects of plan and harness merges on main in the last 24 hours (decision ids in brackets)."""
        since = st.iso(now - status.DAY)
        out = self.git("log", f"--since={since}", "--date=format-local:%H:%MZ", "--format=%cd %s", "refs/runner/main",
                       check=False, env={"TZ": "UTC"})
        return tuple(line[:160] for line in out.splitlines() if PLAN_MERGE.search(line))

    def _trailers(self, what: str) -> str:
        return f"runner: {what}\n\nRunner-Sha: {self.runner_sha}\nRunner-Starter: {self.starter}\n"

    def push_state(self, new: State, base: State) -> bool:
        """Atomic push of claude/kit-state and claude/lease; retry only when nothing but inbox/outbox moved."""
        for _ in range(PUSH_RETRIES):
            writes, removes = self._state_files(new, base)
            state_commit = self._commit(self.state_head, writes, removes, self._trailers("state"))
            lease_doc = {"lease": new.get("lease"), "leases": st.leases_of(new), "seq": self._lease_seq() + 1,
                         "runner_sha": self.runner_sha, "starter": self.starter}
            lease_commit = self._commit(self.lease_head, {LEASE_FILE: json.dumps(lease_doc, indent=1) + "\n"}, [], self._trailers("lease"))
            if self._push_atomic(state_commit, lease_commit):
                self.state_head, self.lease_head = state_commit, lease_commit
                self.state_blob = self.git("rev-parse", f"{state_commit}:{STATE_FILE}")
                self._clear_outbox()
                self.last_state = json.loads(json.dumps(new))
                return True
            if not self._only_side_files_moved():
                return False
        return False

    def update_state(self, transform: Callable[[State], State | None]) -> State | None:
        """Read fresh, transform, push atomically; on a lost race wait a moment and do it again (D-515).

        Up to UPDATE_ATTEMPTS tries with a short random wait between them, so two writers that collide
        do not collide again in step. `transform` gets a fresh copy each time and must re-check its own
        preconditions on it; None from it stops at once. Returns the state pushed (the fresh state when
        the transform changed nothing, with nothing pushed) or None.

        A failed read or push (a fetch or git error) counts as a lost try, never escapes. Unless it
        pushes, it puts the refs it read from back as they were (state and lease heads, blob, last
        state), so a later push_state against an older base is still rejected as a lost race; an
        exception from `transform` (LeaseError) is raised after that restore.
        """
        saved = (self.state_head, self.lease_head, self.state_blob, self.last_state)
        keep = False
        try:
            for attempt in range(UPDATE_ATTEMPTS):
                if attempt:
                    self.sleep(random.uniform(0.5, 2.0) * attempt)
                done, result = self._update_once(transform, attempt)
                if done:
                    keep = result is not None
                    return result
            return None
        finally:
            if not keep:
                self.state_head, self.lease_head, self.state_blob, self.last_state = saved

    def _update_once(self, transform: Callable[[State], State | None], attempt: int) -> tuple[bool, State | None]:
        """One update_state try: (True, pushed state or None) when finished, (False, None) to try again."""
        try:
            fresh = self.read_state()
        except UPDATE_ERRORS as exc:
            self.log.append(f"update_state: read failed on try {attempt + 1} of {UPDATE_ATTEMPTS}: {str(exc)[:200]}")
            return False, None
        new = transform(json.loads(json.dumps(fresh)))
        if new is None:
            return True, None
        if new == fresh:
            return True, fresh
        try:
            if self.push_state(new, fresh):
                return True, new
        except UPDATE_ERRORS as exc:
            self.log.append(f"update_state: push failed on try {attempt + 1} of {UPDATE_ATTEMPTS}: {str(exc)[:200]}")
            return False, None
        self.log.append(f"update_state: lost race {attempt + 1} of {UPDATE_ATTEMPTS}")
        return False, None

    def _push_atomic(self, state_commit: str, lease_commit: str) -> bool:
        if self.dry_run:
            self.log.append(f"dry-run: would push {state_commit[:12]} and {lease_commit[:12]}")
            return True
        refs = [f"{state_commit}:refs/heads/{self.state_ref}", f"{lease_commit}:refs/heads/{self.lease_ref}"]
        code, _, err = self._run(["git", "push", "--atomic", "--porcelain", "origin", *refs], None, self.root)
        if code != 0:
            self.log.append(f"push rejected: {err.strip()[:200]}")
        return code == 0

    def _only_side_files_moved(self) -> bool:
        """After a rejected push: True (rebase and retry) only if the lease ref and kit/state.json held still."""
        old_lease = self.lease_head
        new_state = self._fetch(self.state_ref)
        if self._fetch(self.lease_ref) != old_lease or new_state is None:
            return False
        if self.git("rev-parse", f"{new_state}:{STATE_FILE}", check=False) != self.state_blob:
            return False
        self.state_head = new_state
        return True

    def _clear_outbox(self) -> None:
        if self.dry_run or not self.outbox_dir.exists():
            return
        for path in self.outbox_dir.glob("*.md"):
            path.unlink()

    def progress_markdown(self, state: State) -> str:
        """`python3 scripts/progress.py --markdown` over this state and kit/nightly.json (governance 6)."""
        nightly = self.read_nightly() or {}
        key = json.dumps([[t.get("id"), t.get("status"), t.get("merged_sha")] for t in state.get("tasks", [])] + [nightly], sort_keys=True)
        if key not in self._progress:
            self._progress[key] = self._run_progress(state, nightly)
        return self._progress[key]

    def _run_progress(self, state: State, nightly: dict[str, Any]) -> str:
        with tempfile.TemporaryDirectory() as tmp:
            state_path, nightly_path = Path(tmp) / "state.json", Path(tmp) / "nightly.json"
            state_path.write_text(json.dumps(state), encoding="utf-8")
            nightly_path.write_text(json.dumps(nightly), encoding="utf-8")
            args = ["python3", "scripts/progress.py", "--state", str(state_path), "--nightly", str(nightly_path), "--markdown"]
            code, out, err = self._run(args, None, self.root)
        tail = (err or out).strip().splitlines()
        return out if code == 0 else f"Progress unavailable: {tail[-1] if tail else f'exit {code}'}"

    # ---- inbox

    def read_inbox(self) -> list[tuple[str, str, str]]:
        listing = self.git("ls-tree", "--name-only", f"{self.state_head}", "inbox/", check=False) if self.state_head else ""
        items = []
        for path in sorted(p for p in listing.splitlines() if p.endswith(".md")):
            outbox_id, text = parse_inbox_file(self._show(self.state_head, path) or "")
            items.append((path.rsplit("/", 1)[-1], outbox_id, text))
        return items

    def ack_inbox(self, file_ids: list[str]) -> None:
        """Move answered files to inbox/done/ on claude/kit-state only (never touches the lease ref)."""
        if not file_ids or self.dry_run:
            return
        for _ in range(PUSH_RETRIES):
            present = [f for f in file_ids if self._show(self.state_head, f"inbox/{f}") is not None]
            if not present:
                return
            writes = {f"inbox/done/{f}": self._show(self.state_head, f"inbox/{f}") or "" for f in present}
            commit = self._commit(self.state_head, writes, [f"inbox/{f}" for f in present], self._trailers("inbox read"))
            code, _, _ = self._run(["git", "push", "--porcelain", "origin", f"{commit}:refs/heads/{self.state_ref}"], None, self.root)
            if code == 0:
                self.state_head = commit
                return
            self.state_head = self._fetch(self.state_ref)

    def write_cycle_log(self, report: dict[str, Any]) -> None:
        """Append the cycle's report (result, log lines, changes) to kit/runner_log.jsonl on
        claude/kit-state, keeping the last 200 lines, so a cycle run where nobody watches the
        session's output (the Actions runner) can still be read afterwards (D-499)."""
        if self.dry_run:
            return
        line = json.dumps({"at": self.clock().isoformat(timespec="seconds"), **report}, sort_keys=True)
        for _ in range(PUSH_RETRIES):
            old = (self._show(self.state_head, RUNNER_LOG) or "").splitlines()
            text = "\n".join([*old[-199:], line]) + "\n"
            commit = self._commit(self.state_head, {RUNNER_LOG: text}, [], self._trailers("cycle log"))
            code, _, _ = self._run(["git", "push", "--porcelain", "origin", f"{commit}:refs/heads/{self.state_ref}"], None, self.root)
            if code == 0:
                self.state_head = commit
                return
            self.state_head = self._fetch(self.state_ref)

    # ---- main, nightly, smoke

    def read_nightly(self) -> dict[str, Any] | None:
        raw = self._show(self.state_head, NIGHTLY_FILE)
        return json.loads(raw) if raw else None

    def main_head(self) -> str:
        return self._fetch("main") or ""

    def in_main(self, sha: str) -> bool | None:
        """Whether the commit is main's head or one of its ancestors (`git merge-base --is-ancestor`, after a
        fresh fetch of main); None when git cannot say, as for a commit this clone does not have (D-551)."""
        main = self.main_head()
        if not main or not sha:
            return None
        code, _, _ = self._run(["git", "merge-base", "--is-ancestor", sha, main], None, self.root)
        return {0: True, 1: False}.get(code)

    def decision_numbers(self) -> set[int] | None:
        """The D-numbers on main's docs/decisions.md (fetched fresh), plus those on the head of every open
        pull request from a `claude/` branch that is not a task branch (a plan or harness change waiting
        to merge, D-1144); claims reserve decision numbers above them all (D-530). None when main's file
        cannot be read or holds no entry, so a bad read never makes a claim hand out D-001 to D-005."""
        numbers = set(st.decision_numbers(self._show(self.main_head() or None, DECISIONS_FILE) or ""))
        return (numbers | self.open_plan_decision_numbers()) if numbers else None

    def open_plan_decision_numbers(self) -> set[int]:
        """D-numbers on the heads of open plan and harness pull requests (`claude/` branches that are not
        task branches), read through the contents API; a request that fails contributes nothing (D-1144).
        T047's claim on October 31 took the block D-1127 to D-1131 while two such pull requests held
        D-1127 and D-1128, numbered above the counter as the interim rule asked; main's highest entry,
        not the counter, set the block, so a plan number only becomes safe once its pull request is open."""
        try:
            pulls = self.gh_json(f"repos/{self.repo}/pulls?state=open&per_page=100")
        except (RuntimeError, ValueError):
            return set()
        found: set[int] = set()
        for pull in pulls if isinstance(pulls, list) else []:
            ref = str((pull.get("head") or {}).get("ref") or "")
            sha = str((pull.get("head") or {}).get("sha") or "")
            if not ref.startswith("claude/") or verify.TASK_BRANCH.match(ref) or not sha:
                continue
            code, out, _ = self._run(["gh", "api", "-H", "Accept: application/vnd.github.raw",
                                      f"repos/{self.repo}/contents/{DECISIONS_FILE}?ref={sha}"], None, self.root)
            if code == 0:
                found |= set(st.decision_numbers(out or ""))
        return found

    def plan_evidence(self, task_id: str, prompt: str, since: str) -> dict[str, Any] | None:
        """What `--requeue --plan-fix` checks on main (D-677): main's head (fetched fresh) and main's
        commit at `since`, the start of the task's last attempt (cycle.plan_since; `git rev-list -1
        --first-parent --before`), with, at each, the task's plan fingerprint (verify.plan_fingerprint),
        the task's own `decisions` list (verify.task_decisions) and the D-numbers of docs/decisions.md.
        None when git cannot read main or finds no commit of main by then."""
        head = self.main_head()
        if not head:
            return None
        code, out, _ = self._run(["git", "rev-list", "-1", "--first-parent", f"--before={since}", head], None, self.root)
        base = out.strip() if code == 0 else ""
        if not base:
            return None
        then, now = self._plan_at(base, task_id, prompt), self._plan_at(head, task_id, prompt)
        return {"since": since, "base": base, "head": head, **{f"{k}_then": v for k, v in then.items()},
                **{f"{k}_now": v for k, v in now.items()}}

    def _plan_at(self, commit: str, task_id: str, prompt: str) -> dict[str, Any]:
        """At one commit of main (D-677): the task's plan fingerprint (`plan`), its `decisions` list in
        kit/tasks.json (`task_decisions`) and the D-numbers of docs/decisions.md (`decisions`)."""
        tasks = self._show(commit, TASKS_FILE)
        return {"plan": verify.plan_fingerprint(tasks, self._show(commit, prompt), task_id),
                "task_decisions": verify.task_decisions(tasks, task_id),
                "decisions": sorted(set(st.decision_numbers(self._show(commit, DECISIONS_FILE) or "")))}

    def smoke_main(self) -> tuple[bool, str]:
        """bash scripts/smoke.sh in a detached worktree of main's head (the runner's own tool run), at the
        fixed `smoke_tree_dir` and building into `smoke_target_dir` with pinned mtimes (D-1890). When this job's
        `--pre-smoke` step already ran it on this very head, its result is the smoke's instead (D-2028)."""
        head = self.main_head()
        if self.dry_run:
            self.log.append(f"dry-run: smoke test not run on {head[:12]}")
            return True, head
        self.smoke_timed_out, self.smoke_source, self.smoke_seconds = False, "", None
        pre = self._pre_smoke_for(head)
        if pre is not None:
            return self._use_pre_smoke(pre, head)
        tree, began = smoke_tree_dir(), time.monotonic()  # before the try: a seed or pin timeout still has a start
        self._drop_smoke_tree(tree)
        self.git("worktree", "add", "--detach", str(tree), head)
        try:
            self._seed_smoke_target(tree)
            self._pin_smoke_tree(tree)
            code, out, err = self._run(["bash", "scripts/smoke.sh"], None, tree)
        except subprocess.TimeoutExpired:
            # A smoke test that outruns the budget is not a crash and not a verdict on main: the
            # cycle notes it (never a D-498 strike) and the next run tries again (D-1178).
            code, out, err = 124, "", f"smoke timed out after {SMOKE_TIMEOUT} seconds"
            self.smoke_timed_out = True
        finally:
            self._drop_smoke_tree(tree)
            _mark_smoke_ran()
        self.smoke_seconds = round(time.monotonic() - began)
        tail = (out + err).strip().splitlines()
        self.log.append(f"smoke on {head[:12]}: exit {code}; {tail[-1] if tail else ''}")
        return code == 0, head

    def _pre_smoke_for(self, head: str) -> dict[str, Any] | None:
        """The pre-session smoke's result (SMOKE_PRE, D-2028) when it is for `head` and was written by this very
        job (`job_id`, never "" here), else None and, inside Actions, a log line saying why the smoke runs here."""
        job = job_id()
        try:
            pre = json.loads((smoke_target_dir().parent / SMOKE_PRE).read_text(encoding="utf-8"))
        except (OSError, ValueError):
            pre = None
        if not isinstance(pre, dict) or not isinstance(pre.get("exit"), int) or isinstance(pre.get("exit"), bool):
            why = "no pre-session smoke result"
        elif not job or pre.get("job") != job:
            why = "the pre-session smoke result is from another job"
        elif pre.get("sha") != head:
            why = f"the pre-session smoke ran on {str(pre.get('sha'))[:12]}, main is now {head[:12]}"
        else:
            return pre
        if job:
            self.log.append(f"{why}: the smoke runs here")
        return None

    def _use_pre_smoke(self, pre: dict[str, Any], head: str) -> tuple[bool, str]:
        """smoke_main's verdict from the pre-session result (D-2028): green only on exit 0; a timeout at its
        PRE_SMOKE_TIMEOUT sets `smoke_timed_out` as an in-session one does (D-1178, D-1970). Its `last` line is
        redacted again (`_redact_gate_row`) before it reaches the log."""
        code, timed_out, seconds = int(pre["exit"]), pre.get("timed_out") is True, pre.get("seconds")
        self.smoke_timed_out, self.smoke_source = timed_out, PRE_SESSION
        self.smoke_seconds = seconds if isinstance(seconds, int) and not isinstance(seconds, bool) else None
        self.log.append(f"smoke on {head[:12]}: exit {code}; {_redact_gate_row(str(pre.get('last') or ''))[:300]} "
                        f"({PRE_SESSION}, {self.smoke_seconds} s, this job's --pre-smoke step)")
        return code == 0 and not timed_out, head

    def pre_smoke(self) -> str:
        """`python3 -m runner --pre-smoke` (D-2028): the start routine's smoke itself, before the session where no
        600-second Bash call caps it: `_drop_smoke_tree`, the fixed `smoke_tree_dir` at main's head (the checkout's
        `origin/main`, as `warm_smoke`), `_seed_smoke_target`, `_pin_smoke_tree`, then `bash scripts/smoke.sh`
        through `pre_run` within PRE_SMOKE_TIMEOUT. Any old result is removed first; the new one (`write_pre_smoke`:
        sha, ok, timed_out, exit, seconds, at, job, last) is what `smoke_main` uses for the same head and job.
        Its output goes to SMOKE_PRE_LOG. Leaves SMOKE_RAN only on a timeout (the warm-up step before it judges
        a finished build, D-2007); writes no state, lease or log on claude/kit-state; returns the line to print."""
        head = self._checkout_main()
        if self.dry_run or not head or not job_id():
            why = "dry-run" if self.dry_run else "no main head" if not head else "no Actions job to tie it to"
            return f"pre-session smoke not run on {head[:12]}: {why}"
        tree, out = smoke_tree_dir(), smoke_target_dir().parent / SMOKE_PRE_LOG
        for old in (smoke_target_dir().parent / SMOKE_PRE, out):
            old.unlink(missing_ok=True)
        self._drop_smoke_tree(tree)
        self.git("worktree", "add", "--detach", str(tree), head)
        try:
            self._seed_smoke_target(tree)
            self._pin_smoke_tree(tree)
            code, timed_out, seconds = self._pre_smoke_run(tree, out)
        finally:
            self._drop_smoke_tree(tree)
        return self._record_pre_smoke(head, code, timed_out, seconds, out)

    def _pre_smoke_run(self, tree: Path, out: Path) -> tuple[int, bool, int]:
        """smoke.sh through `pre_run` in `tree`: (exit, timed out, seconds); a timeout is exit 124 as in-session."""
        began = time.monotonic()
        try:
            code, timed_out = self.pre_run(["bash", "scripts/smoke.sh"], tree, PRE_SMOKE_TIMEOUT, out), False
        except subprocess.TimeoutExpired:
            code, timed_out = 124, True
            _mark_smoke_ran()  # it may have been compiling: keep it, as D-2007 keeps a warm-up that timed out
        return code, timed_out, round(time.monotonic() - began)

    def _record_pre_smoke(self, head: str, code: int, timed_out: bool, seconds: int, out: Path) -> str:
        """Write the pre-session result (D-2028) with the output's last line, redacted as a gate row is
        (`_redact_gate_row`, since it reaches the runner log), and return the line to print."""
        try:
            rows = out.read_text(encoding="utf-8", errors="replace").strip().splitlines()
        except OSError:
            rows = []
        last = f"smoke timed out after {PRE_SMOKE_TIMEOUT} seconds" if timed_out else _redact_gate_row(rows[-1] if rows else "")
        write_pre_smoke({"sha": head, "ok": code == 0 and not timed_out, "timed_out": timed_out, "exit": code,
                         "seconds": seconds, "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                         "job": job_id(), "last": last[:300]})
        return f"pre-session smoke on {head[:12]}: exit {code}; {last[:300]}; {seconds} s"

    def _checkout_main(self) -> str:
        """main's head as the checkout has it (`origin/main`, else HEAD), with no fetch: the steps before the
        session have no token (D-2007, D-2028)."""
        return self.git("rev-parse", "--verify", "--quiet", "origin/main^{commit}", check=False) or self.git("rev-parse", "HEAD", check=False)

    def warm_smoke(self) -> str:
        """`python3 -m runner --warm-smoke` (D-2007): the smoke's own preparation (the fixed `smoke_tree_dir` at
        main's head, `_seed_smoke_target`, `_pin_smoke_tree` with its manifest) and then only the smoke's
        compile steps (`warm_commands`), within WARM_TIMEOUT, so the session's smoke, which pins the same
        content to the same times, finds the artifacts fresh. main's head is the checkout's `origin/main`
        (no fetch: the step has no token). Leaves SMOKE_RAN, so the cache is saved, only when the compile
        made progress (`_warm_build`); writes no state, lease or log on claude/kit-state; returns the line to print."""
        head = self._checkout_main()
        if self.dry_run or not head:
            return f"dry-run: smoke warm-up not run on {head[:12]}" if self.dry_run else "smoke warm-up skipped: no main head"
        tree, start, built = smoke_tree_dir(), time.monotonic(), False
        self._drop_smoke_tree(tree)
        self.git("worktree", "add", "--detach", str(tree), head)
        try:
            self._seed_smoke_target(tree)
            self._pin_smoke_tree(tree)
            done, built = self._warm_build(tree, start + WARM_TIMEOUT)
        finally:
            self._drop_smoke_tree(tree)
            if built:
                _mark_smoke_ran()
        saved = "cache marked for saving" if built else "nothing new to cache"
        return f"smoke warm-up on {head[:12]}: {done or 'nothing to build'}; {round(time.monotonic() - start)} s, {saved}"

    def _warm_build(self, tree: Path, deadline: float) -> tuple[str, bool]:
        """Each of `warm_commands` in turn, a red one not stopping the next (a clippy warning must not keep
        the test build cold); the first to outrun `deadline` ends the warm-up. Returns the `<step> <exit>`
        items and whether the build is worth saving: the cargo commands took at least WARM_SAVE_SECONDS and
        one of them exited 0 or ran out of time (a timeout that compiled for that long made progress)."""
        done: list[str] = []
        cargo_s, progress = 0.0, False
        for cmd in warm_commands(tree):
            left, began = deadline - time.monotonic(), time.monotonic()
            try:
                if left <= 0:
                    raise subprocess.TimeoutExpired(cmd, 0)
                code = self.warm_run(cmd, tree, left)
                done.append(f"{cmd[1]} exit {code}")
                progress = progress or (cmd[0] == "cargo" and code == 0)
            except subprocess.TimeoutExpired:
                done.append(f"{cmd[1]} timed out (budget {WARM_TIMEOUT} s)")
                progress = progress or cmd[0] == "cargo"
                break
            finally:
                cargo_s += time.monotonic() - began if cmd[0] == "cargo" else 0.0
        return ", ".join(done), progress and cargo_s >= WARM_SAVE_SECONDS

    def _drop_smoke_tree(self, tree: Path) -> None:
        """Remove the smoke worktree, and anything a crashed run left at its fixed path, so `worktree add` works."""
        self.git("worktree", "remove", "--force", str(tree), check=False)
        shutil.rmtree(tree, ignore_errors=True)
        self.git("worktree", "prune", check=False)

    def _pin_smoke_tree(self, tree: Path) -> None:
        """`pin_smoke_mtimes` on the smoke worktree; on any failure `stamp_tree_now` (no manifest, a full build)."""
        manifest = smoke_target_dir() / SMOKE_MTIMES
        self.log.append(f"smoke mtimes: manifest {manifest}, {len(_read_mtimes(manifest))} entries loaded")
        code, index, err = self._run(["git", "ls-files", "-s", "-z"], None, tree)
        try:
            if code != 0:
                raise OSError(err.strip()[-200:] or f"git ls-files exit {code}")
            fresh = pin_smoke_mtimes(tree, index, manifest, time.time_ns())
        except (OSError, ValueError) as exc:
            self.log.append(f"smoke mtimes not pinned, all new: {exc}")
            try:
                stamp_tree_now(tree, manifest)
            except OSError as again:
                self.log.append(f"smoke tree not stamped: {again}")
            return
        self.log.append(f"smoke mtimes: {fresh} new or changed paths")

    def _seed_smoke_target(self, tree: Path) -> None:
        """When the smoke target has no dependency builds yet (the first smoke after D-1890, or a cache entry
        without it), copy the worker build's dependency artifacts into it, so the smoke compiles only the
        members and fits SMOKE_TIMEOUT. Skipped, and logged, when the disk is short; never fails the smoke."""
        smoke, debug = smoke_target_dir(), smoke_target_dir().parent / "debug"
        if any((smoke / "debug" / "deps").glob("*")) or not (debug / "deps").is_dir():
            return
        code, out, _ = self._run(["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"], None, tree)
        try:
            members = _member_names(json.loads(out)) if code == 0 else set()
            entries = seed_files(debug, members) if members else []
            need, free = _tree_size(entries), shutil.disk_usage(smoke.parent).free
            self.log.append(f"smoke target seed: {len(entries)} entries, {need >> 20} MB, {free >> 20} MB free")
            if entries and free > need + SEED_MARGIN:
                (smoke / SMOKE_MTIMES).unlink(missing_ok=True)
                copy_seed(debug, smoke / "debug", entries)
        except (OSError, ValueError, KeyError, TypeError) as exc:
            self.log.append(f"smoke target not seeded: {exc}")

    def main_checks_green(self, head: str) -> bool | None:
        """True when `Ci`, `Twin` and `Ally` all concluded success for that commit of main; False when
        any of them failed; None when a required check is missing or still running (REST, read only).

        A squash commit on main carries no check runs of its own (the check workflows run on pushes to
        `claude/**` and `dependabot/**` only, D-1087), so the verdict is read from the merged pull
        request's head, which GitHub lists for the commit; a direct read of the commit comes first for a
        commit that has them."""
        verdict = self._required_checks(head)
        if verdict is not None:
            return verdict
        try:
            pulls = self.gh_json(f"repos/{self.repo}/commits/{head}/pulls")
        except RuntimeError as exc:
            self.log.append(f"pulls for {head[:12]} unreadable: {exc}")
            return None
        merged = [p for p in pulls if str(p.get("merge_commit_sha") or "") == head and p.get("merged_at")]
        if not merged:
            return None
        return self._required_checks(str(merged[0]["head"]["sha"]))

    def checks_on(self, sha: str) -> dict[str, str] | None:
        """{check name: conclusion, or "running"} for the required checks on a commit, each from its
        latest run (REST, read only, the job token); None when the check runs cannot be read (D-630)."""
        try:
            runs = self.gh_json(f"repos/{self.repo}/commits/{sha}/check-runs?per_page=100").get("check_runs", [])
        except RuntimeError as exc:
            self.log.append(f"check-runs on {sha[:12]} unreadable: {exc}")
            return None
        latest: dict[str, str] = {}
        for run in sorted(runs, key=lambda r: str(r.get("started_at") or "")):
            done = run.get("status") == "completed"
            latest[str(run.get("name"))] = str(run.get("conclusion") or "") if done else "running"
        return {name: latest[name] for name in verify.REQUIRED_CHECKS if name in latest}

    def _required_checks(self, sha: str) -> bool | None:
        latest = self.checks_on(sha)
        if latest is None:
            return None
        verdicts = [latest.get(name) for name in verify.REQUIRED_CHECKS]
        if any(v in ("failure", "cancelled", "timed_out") for v in verdicts):
            return False
        if all(v == "success" for v in verdicts):
            return True
        return None

    def dispatch_lane(self, lane: str) -> bool:
        """Start one backup-runner run for a lane (`gh workflow run backup-runner.yml -f lane=<n>`) with
        the job token, as the chain step does, so a free lane is refilled while this run's worker is still
        busy (D-723). False, with nothing started, in a dry run, without the job token (a routine or a
        local run) or when gh fails; GitHub keeps at most one waiting job per lane, so a second start
        replaces the first rather than adding one."""
        if self.dry_run or not lane.isdigit() or not os.environ.get("RUNNER_CHECKS_TOKEN"):
            return False
        args = ["gh", "workflow", "run", RUNNER_WORKFLOW, "--ref", "main", "--repo", self.repo, "-f", f"lane={lane}"]
        code, _, err = self._run(args, None, self.root)
        why = TOKEN_LIKE.sub("[redacted]", err.strip())[:200]
        self.log.append(f"refill: run started for lane {lane}" if code == 0 else f"refill: lane {lane} not started: {why}")
        return code == 0

    # ---- pull requests (REST only)

    def gh_json(self, path: str) -> Any:
        code, out, err = self._run(["gh", "api", "-H", "Accept: application/vnd.github+json", path], None, self.root)
        if code != 0:
            raise RuntimeError(f"gh api {path} failed: {err.strip()[:200]}")
        return json.loads(out or "null")

    def branch_for(self, task_id: str) -> str:
        known = next((t.get("branch") for t in self.graph if t.get("id") == task_id), None)
        entry = st.entry(self.last_state or {"tasks": []}, task_id)
        return str(entry.get("branch") or known or f"claude/task/{task_id}")

    def pr_for(self, task_id: str) -> dict[str, Any] | None:
        """The task's latest PR as runner.verify expects it: merged, head_sha, checks, trees, files."""
        owner = self.repo.split("/")[0]
        pulls = self.gh_json(f"repos/{self.repo}/pulls?head={owner}:{self.branch_for(task_id)}&state=all&per_page=20")
        if not pulls:
            return None
        pr = max(pulls, key=lambda p: int(p["number"]))
        head = str(pr["head"]["sha"])
        runs = self.gh_json(f"repos/{self.repo}/commits/{head}/check-runs?per_page=100").get("check_runs", [])
        checks = [{"name": r.get("name"), "sha": r.get("head_sha", head), "conclusion": r.get("conclusion")} for r in runs]
        out = {"number": int(pr["number"]), "merged": bool(pr.get("merged_at")), "head_sha": head, "checks": checks,
               "auto_merge": bool(pr.get("auto_merge")), "state": str(pr.get("state") or "open")}  # state: D-584
        out["failure_log"] = self._failure_log(runs)
        if out["merged"]:
            out.update(self._merge_facts(int(pr["number"]), head, str(pr.get("merge_commit_sha") or ""), str(pr["merged_at"])))
        return out

    def _failure_log(self, runs: list[dict[str, Any]]) -> str:
        """What failed on a PR head, as the retry worker gets it in `last_failure` (D-582).

        First the "name: conclusion" list of every failed check run, as before. Then, once per check
        name (its latest run), the Actions job's failed step names and the tail of that step's log
        (`log_excerpt`), or, when no log line could be had, the check run's failure annotations
        (D-1263), all within FAILURE_LOG_CAP characters. A detail that cannot be read is left out, so on
        any API error the log is exactly the plain list."""
        failed = [r for r in runs if r.get("conclusion") in FAILED_CONCLUSIONS]
        base = "; ".join(f"{r.get('name')}: {r.get('conclusion')}" for r in failed)
        latest: dict[str, dict[str, Any]] = {}
        for run in sorted(failed, key=lambda r: str(r.get("started_at") or "")):
            latest[str(run.get("name"))] = run
        details = [d for d in (self._job_detail(run) for run in latest.values()) if d is not None]
        return fit_failure_log(base, details)

    def _job_detail(self, run: dict[str, Any]) -> tuple[str, list[str]] | None:
        """(header naming the failed steps, excerpt) for one failed check run, or None (D-582).

        The excerpt is the tail of the failed step's log (`_log_lines`); when that is empty, the check
        run's failure annotations stand in (`_annotation_lines`, D-1263), so the retry worker still
        learns what failed when the log download is refused."""
        job = _job_id(run)
        if job is None:
            return None
        try:
            steps = self.gh_json(f"repos/{self.repo}/actions/jobs/{job}").get("steps") or []
            names = [str(s.get("name")) for s in steps if s.get("conclusion") in FAILED_CONCLUSIONS]
            if not names:
                return None
            excerpt = self._log_lines(job)
        except BEST_EFFORT_ERRORS as exc:
            self.log.append(f"job {job} detail unreadable: {TOKEN_LIKE.sub('[redacted]', str(exc))[:200]}")
            return None
        return f"{run.get('name')} failed at step: {', '.join(names)}", excerpt or self._annotation_lines(run)

    def _log_lines(self, job: str) -> list[str]:
        """`log_excerpt` of an Actions job's log, or [] with the reason in the cycle log (D-1263).

        A refused download is tried once more after LOG_RETRY_SECONDS, since a log read right after its
        check completed may not be ready. One still refused (`gh api` exit code and the first 200
        characters of its stderr, token-shaped strings redacted) and a log with no `##[error]` line are
        both written down, so the next failure shows why the log was not used instead of failing
        silently as every one did from D-582 on."""
        # `--allow-escape-sequences`: a log that carries terminal colour codes is otherwise refused by
        # `gh api` with "the response contains terminal escape sequences", which lost the excerpt of
        # T225's Coverage failure (D-1398); `log_excerpt` strips the codes itself. A `gh` too old for
        # the flag ("unknown flag") is asked again without it, so an older host loses nothing.
        path = f"repos/{self.repo}/actions/jobs/{job}/logs"
        args = ["gh", "api", "--allow-escape-sequences", path]
        code, text, err = self._run(args, None, self.root)
        if code != 0 and "unknown flag" in err:
            args = ["gh", "api", path]
            code, text, err = self._run(args, None, self.root)
        if code != 0:
            time.sleep(LOG_RETRY_SECONDS)
            code, text, err = self._run(args, None, self.root)
        if code != 0:
            self.log.append(f"job {job} log unreadable (exit {code}): {TOKEN_LIKE.sub('[redacted]', err.strip())[:200]}")
            return []
        excerpt = log_excerpt(text)
        if not excerpt:
            self.log.append(f"job {job} log has no error line ({len(text)} characters read)")
        return excerpt

    def _annotation_lines(self, run: dict[str, Any]) -> list[str]:
        """The failure annotations of a check run (`annotation_lines`), read with the job token
        (`checks: read`, D-502) and needing no log download (D-1263); [] when the run has no id or the
        read fails, with the reason in the cycle log."""
        check = run.get("id")
        if not check:
            return []
        try:
            notes = self.gh_json(f"repos/{self.repo}/check-runs/{check}/annotations?per_page=50")
        except BEST_EFFORT_ERRORS as exc:
            self.log.append(f"check run {check} annotations unreadable: {TOKEN_LIKE.sub('[redacted]', str(exc))[:200]}")
            return []
        return annotation_lines(notes)

    def _merge_facts(self, number: int, head: str, merged: str, merged_at: str) -> dict[str, Any]:
        """Trees of the tested head and the squash commit, and the merge's files (--no-renames)."""
        self._run(["git", "fetch", "--quiet", "origin", f"+refs/pull/{number}/head:refs/runner/pr-{number}", "+refs/heads/main:refs/runner/main"], None, self.root)
        files = self.git("diff-tree", "--no-commit-id", "--name-only", "-r", "--no-renames", f"{merged}^", merged, check=False)
        return {
            "merged_sha": merged, "merged_at": merged_at, "files": [f for f in files.splitlines() if f],
            "head_tree": self.git("rev-parse", f"{head}^{{tree}}", check=False),
            "merge_tree": self.git("rev-parse", f"{merged}^{{tree}}", check=False),
        }

    def _open_pr(self, branch: str) -> dict[str, Any] | None:
        """The branch's open PR, opening it through REST when auto-pr has not yet (BUILD_BRIEF.md:255)."""
        owner = self.repo.split("/")[0]
        found = self.gh_json(f"repos/{self.repo}/pulls?head={owner}:{branch}&state=open")
        if found:
            return dict(found[0])
        tid = verify.task_id_from_branch(branch)
        args = ["gh", "api", f"repos/{self.repo}/pulls", "-f", f"title={tid}: {branch.rsplit('/', 1)[-1]}",
                "-f", f"head={branch}", "-f", "base=main", "-f", f"body=Opened by the runner for {tid}."]
        code, out, _ = self._run(args, None, self.root)
        if code == 0:
            return dict(json.loads(out))
        again = self.gh_json(f"repos/{self.repo}/pulls?head={owner}:{branch}&state=open")  # auto-pr won the race
        return dict(again[0]) if again else None

    def enable_auto_merge(self, branch: str, head_sha: str) -> tuple[bool, str]:
        """Squash auto-merge as the runner's identity: the CCR route from a session, else `gh pr merge --auto`
        (the App token in Actions). Only for a PR whose head is the reviewed head."""
        if self.dry_run:
            return True, "dry-run"
        pr = self._open_pr(branch)
        for _ in range(HEAD_POLLS):  # GitHub may still list the head before a push it just took (D-620)
            if pr is None or str(pr["head"]["sha"]) == head_sha:
                break
            self.sleep(HEAD_POLL_SECONDS)
            pr = self._open_pr(branch)
        if pr is None:
            return False, "no open PR for the branch"
        if str(pr["head"]["sha"]) != head_sha:
            return False, f"PR head {str(pr['head']['sha'])[:12]} is not the reviewed head {head_sha[:12]}"
        number = str(pr["number"])
        ccr = ["gh", "api", "-X", "PUT", f"repos/{self.repo}/pulls/{number}/ccr/auto_merge", "-f", "merge_method=squash"]
        if self._run(ccr, None, self.root)[0] == 0:
            return True, f"auto-merge on #{number} (ccr route)"
        code, _, err = self._run(["gh", "pr", "merge", "--auto", "--squash", number, "--repo", self.repo], None, self.root)
        return code == 0, f"auto-merge on #{number} (gh pr merge)" if code == 0 else f"auto-merge refused: {err.strip()[:200]}"

    def merge_main(self) -> str:
        """Merge main into claude/kit-state with a normal two-parent merge commit (Appendix B, K0 step 4).

        The merged tree is main's tree plus the runner's own files from claude/kit-state (state, status,
        progress, inbox, outbox, nightly, owner answers), so workflow updates reach the branch and the
        runner's files are never overwritten. Skipped when main is already merged."""
        main, state = self._fetch("main"), self._fetch(self.state_ref)
        if not main or not state:
            return "skipped: a ref is missing"
        code, _, _ = self._run(["git", "merge-base", "--is-ancestor", main, state], None, self.root)
        if code == 0:
            return "up to date"
        commit = self._merge_commit(main, state)
        if self.dry_run:
            self.log.append(f"dry-run: would merge main {main[:12]} into {self.state_ref} as {commit[:12]}")
            return "dry-run"
        pushed = self._run(["git", "push", "--porcelain", "origin", f"{commit}:refs/heads/{self.state_ref}"], None, self.root)[0] == 0
        self.log.append(f"merge main {main[:12]} into {self.state_ref}: {'pushed' if pushed else 'rejected, next cycle retries'}")
        return "merged" if pushed else "rejected"

    def _merge_commit(self, main: str, state: str) -> str:
        with tempfile.TemporaryDirectory() as tmp:
            env = {"GIT_INDEX_FILE": str(Path(tmp) / "index"), **self._identity()}
            self.git("read-tree", main, env=env)
            for path in self.git("ls-files", "--", *RUNNER_FILES, env=env).splitlines():
                self.git("update-index", "--force-remove", path, env=env)
            entries = self.git("ls-tree", "-r", "--full-tree", state, "--", *RUNNER_FILES)
            if entries:
                self.git("update-index", "--index-info", stdin=entries + "\n", env=env)
            tree = self.git("write-tree", env=env)
            return self.git("commit-tree", tree, "-p", state, "-p", main, "-m", self._trailers(f"merge main {main[:12]}"), env=env)

    def gate_body(self, gate: dict[str, Any]) -> str:
        spec = gate.get("gate", {})
        lines = [f"**{gate.get('title') or spec.get('kind', gate['id'])}**", ""]
        lines += [f"- {step}" for step in spec.get("steps", [])]
        if spec.get("resume"):
            lines.append(f"To continue: {spec['resume']}.")
        if spec.get("conditional"):
            lines.append("Answering 'no' is a fine answer: the build then uses Twilio's test credentials and magic numbers only.")
        lines.append(f"Build status: https://github.com/{self.repo}/blob/{self.state_ref}/STATUS.md")
        return "\n".join(lines)

    # ---- worker hand-off and evidence

    def push_branch(self, branch: str, head_sha: str) -> bool:
        if not head_sha:
            self.log.append(f"branch push refused: no head for {branch}")  # `:refs/heads/x` would delete it (D-551)
            return False
        if self.dry_run:
            return True
        remote = f"https://github.com/{self.repo}.git" if os.environ.get("RUNNER_PUSH_TOKEN") else "origin"
        code, out, err = self._run(["git", "push", "--porcelain", remote, f"{head_sha}:refs/heads/{branch}"], None, self.root)
        if code != 0:
            # --porcelain puts the rejection reason on stdout; stderr only says "failed to push" (D-509).
            self.last_rejection = (out.strip() + " " + err.strip()).strip()[:400]
            self.log.append(f"branch push rejected: {self.last_rejection}")
        return code == 0

    def push_rejection(self) -> str:
        """The git output of the last rejected push_branch ('' when none), so the cycle can tell a
        non-fast-forward from a credential problem (D-583)."""
        return self.last_rejection

    # ---- the branch an attempt works on (D-583)

    def prepare_branch(self, task: dict[str, Any], attempt: int, session: str) -> dict[str, Any]:
        """Make the local task branch before the worker starts, so a retry builds on what an earlier
        attempt pushed and its push fast-forwards (the runner never force-pushes, BUILD_BRIEF.md:303).

        - origin has no task branch: the branch starts at main;
        - origin has it and its latest pull request is open, absent, or one the runner closed itself to
          drop auto-merge: the branch starts at origin's tip with main merged in (a normal merge commit;
          none when main is already in it). An open pull request with auto-merge on is closed with a
          comment first, so no push of unreviewed work can merge; the runner opens a fresh one after
          review (`_open_pr`), and the closed number is recorded in the task's `runner_closed_prs`;
          when that branch's docs/decisions.md has new entries filed before its base's last entry, a
          runner commit moves them to the end first (`_decisions_in_order`, D-1029);
        - a resync (the task's `resync`, D-891) whose reviewed head is still origin's tip, with a pull
          request that is open or one the runner closed: the branch starts at that head and main is NOT
          merged in, since the worker merges it and resolves the conflict itself ("resync": True);
        - its pull request was merged or closed by someone, its pull requests cannot be read, the
          auto-merge one cannot be closed, or merging main conflicts: a fresh branch from main under a
          new name (`<branch>-a<attempt>`); on a conflict the open pull request is closed with a
          comment, and the old branch stays for evidence.
        A pull request is closed only while `session` holds the task's live lease. Returns {"branch",
        "branch_ready": True, "base", "base_sha", ...}, also kept as `prepared` for the hand-off; {} in a
        dry run or when git fails (the attempt then goes on as before, and collect_result still refuses
        a hand-in that does not build on origin's branch)."""
        self.prepared = {}
        if self.dry_run:
            return {}
        branch = self.branch_for(task["id"])
        try:
            self.prepared = self._prepare(task, branch, attempt, session)
        except BEST_EFFORT_ERRORS as exc:
            self.log.append(f"branch {branch} not prepared: {str(exc)[:200]}")
        return self.prepared

    def _prepare(self, task: dict[str, Any], branch: str, attempt: int, session: str) -> dict[str, Any]:
        main = self.main_head()
        if not main:
            raise RuntimeError("main is unreadable")
        remote = self._remote_tip(branch)
        if remote is None:
            return self._fresh(branch, main)
        pr = self._latest_pr(branch)
        gone = "unreadable" if pr is None else self._pr_end(pr, task["id"])
        if gone:
            return self._fresh(self._new_name(task, attempt), main, renamed_from=branch, why=f"its pull request was {gone}")
        resync = task.get("resync") or {}
        extra: dict[str, Any] = {}
        if resync and remote == resync.get("from"):
            # A resync (D-891): the worker merges main into the reviewed head itself, so it is not merged here.
            tip: str | None = remote
            extra = {"resync": True, "main": main, "paths": [str(p) for p in resync.get("paths") or []]}
        else:
            start = self._decisions_in_order(branch, remote, main) or remote  # entries filed early move to the end (D-1029)
            tip = start if self._is_ancestor(main, start) else self._merge_main(branch, start, main)
        if tip is None:
            new = self._new_name(task, attempt)
            closed = self._close_open_pr(branch, CONFLICT_NOTE.format(branch=branch, new=new), task["id"], session)
            return self._fresh(new, main, renamed_from=branch, why="merging main into it conflicted", closed_pr=closed)
        if pr and pr.get("state") == "open" and pr.get("auto_merge"):
            dropped = self._close_open_pr(branch, AUTO_MERGE_NOTE.format(branch=branch), task["id"], session)
            if dropped is None:
                why = "its open pull request has auto-merge on and could not be closed"
                return self._fresh(self._new_name(task, attempt), main, renamed_from=branch, why=why)
            extra["dropped_pr"] = dropped
        self.git("branch", "-f", branch, tip)
        return {"branch": branch, "branch_ready": True, "base": f"origin/{branch}", "base_sha": tip, "remote_sha": remote, **extra}

    def _remote_has(self, branch: str) -> bool:
        """Whether origin has the branch (`git ls-remote --exit-code`: 0 yes, 2 no). Any other answer
        raises, so a branch git could not see is never taken for absent or for a free name (D-583)."""
        code, _, err = self._run(["git", "ls-remote", "--exit-code", "origin", f"refs/heads/{branch}"], None, self.root)
        if code not in (0, 2):
            raise RuntimeError(f"git ls-remote for {branch} failed (exit {code}): {err.strip()[:200]}")
        return code == 0

    def _remote_tip(self, branch: str) -> str | None:
        """origin's tip of the branch, fetched so git can merge it; None only when origin lacks it."""
        if not self._remote_has(branch):
            return None
        tip = self._fetch(branch)
        if tip is None:
            raise RuntimeError(f"git fetch of {branch} failed")
        return tip

    def _runner_closed(self, task_id: str) -> list[int]:
        """The pull requests the runner closed itself to drop auto-merge (`runner_closed_prs`, D-583)."""
        return [int(n) for n in st.entry(self.last_state or {"tasks": []}, task_id).get("runner_closed_prs") or []]

    def _pr_end(self, pr: dict[str, Any], task_id: str) -> str:
        """'merged' or 'closed' when the branch's latest pull request ended it, '' when the branch can be
        continued: no pull request, an open one, or one the runner closed itself to drop auto-merge,
        known from the task's `runner_closed_prs` or, when that record was lost, from the marker in the
        runner's closing comment (D-583, D-584)."""
        if pr.get("merged_at"):
            return "merged"
        if pr.get("state") != "closed":
            return ""
        number = int(pr.get("number", 0))
        return "" if number in self._runner_closed(task_id) or self._closed_by_runner(number) else "closed"

    def _closed_by_runner(self, number: int) -> bool:
        """Whether the pull request carries the runner's auto-merge closing comment (DROP_MARKER)."""
        try:
            comments = self.gh_json(f"repos/{self.repo}/issues/{number}/comments?per_page=100")
        except (RuntimeError, ValueError):
            return False  # unknown: the attempt starts a fresh branch, which is always safe
        return isinstance(comments, list) and any(DROP_MARKER in str(c.get("body", "")) for c in comments if isinstance(c, dict))

    def _holds_lease(self, task_id: str, session: str) -> bool:
        """Whether, on a fresh read, `session` holds a live lease on the task. The read leaves the cached
        heads, blob and states as they were (like update_state), so a later push_state still pushes onto
        the state this attempt read, and a race is still caught (D-584)."""
        saved = (self.state_head, self.lease_head, self.state_blob, self.last_state, self.first_state)
        try:
            fresh = self.read_state()
        finally:
            self.state_head, self.lease_head, self.state_blob, self.last_state, self.first_state = saved
        return any(l.get("task") == task_id and l.get("session") == session for l in st.live_leases(fresh, self.clock()).values())

    def _fresh(self, name: str, main: str, **extra: Any) -> dict[str, Any]:
        self.git("branch", "-f", name, main)
        return {"branch": name, "branch_ready": True, "base": "main", "base_sha": main, **extra}

    def _is_ancestor(self, older: str, newer: str) -> bool:
        return self._run(["git", "merge-base", "--is-ancestor", older, newer], None, self.root)[0] == 0

    def _merge_main(self, branch: str, remote: str, main: str) -> str | None:
        """The merge commit of main into origin's task branch, built without a working tree; None on a
        conflict. Its tree is the update merge's (`UpdateMerge.tree`, D-620), so a retry's branch also
        keeps main's docs/decisions.md entries first and a plan lock both sides rewrote is recomputed."""
        tree, _ = self.updates.tree(remote, main)
        if tree is None:
            return None
        message = f"Merge main into {branch} for the next attempt (D-583)"
        return self.git("commit-tree", tree, "-p", remote, "-p", main, "-m", message, env=self._identity())

    def _latest_pr(self, branch: str) -> dict[str, Any] | None:
        """The branch's latest pull request ({} when it has none); None when the API cannot say."""
        owner = self.repo.split("/")[0]
        try:
            pulls = self.gh_json(f"repos/{self.repo}/pulls?head={owner}:{branch}&state=all&per_page=20")
        except (RuntimeError, ValueError) as exc:
            self.log.append(f"pull requests for {branch} unreadable: {str(exc)[:200]}")
            return None
        return dict(max(pulls, key=lambda p: int(p["number"]))) if pulls else {}

    def _new_name(self, task: dict[str, Any], attempt: int) -> str:
        """`<the task's branch>-a<n>` for the first n from the attempt number that origin does not have."""
        known = next((t.get("branch") for t in self.graph if t.get("id") == task["id"]), None)
        base = str(known or f"claude/task/{task['id']}")
        first = max(attempt, 2)
        for n in range(first, first + 10):
            if not self._remote_has(f"{base}-a{n}"):
                return f"{base}-a{n}"
        raise RuntimeError(f"no free branch name up to {base}-a{first + 9}")

    def _close_open_pr(self, branch: str, body: str, task_id: str, session: str) -> int | None:
        """Close the branch's open pull request (REST), only while `session` still holds the task's live
        lease, and then comment on it; its number, or None when there was none or it could not be closed.
        The comment goes only on a pull request that did close, so it never says what did not happen (D-584)."""
        owner = self.repo.split("/")[0]
        try:
            pulls = self.gh_json(f"repos/{self.repo}/pulls?head={owner}:{branch}&state=open")
        except (RuntimeError, ValueError):
            return None
        if not pulls:
            return None
        number = int(pulls[0]["number"])
        if not self._holds_lease(task_id, session):
            self.log.append(f"pull request #{number} left open: {session} no longer holds the lease on {task_id}")
            return None
        return number if self.close_pr(number, body) else None

    def close_pr(self, number: int, body: str) -> bool:
        """Close a pull request through REST, then comment on it; the comment goes only on one that did
        close, so it never says what did not happen (D-584). Closing also clears its auto-merge."""
        closed = self._run(["gh", "api", "-X", "PATCH", f"repos/{self.repo}/pulls/{number}", "-f", "state=closed"], None, self.root)
        if closed[0] != 0:
            self.log.append(f"pull request #{number} not closed: {closed[2].strip()[:200]}")
            return False
        self._run(["gh", "api", f"repos/{self.repo}/issues/{number}/comments", "-f", f"body={body}"], None, self.root)
        return True

    def reviewed_plus_main(self, reviewed: str, head: str, number: int) -> bool | None:
        """Whether `head` holds exactly the reviewed work combined with main: its tree equals the clean
        automatic merge (`git merge-tree --write-tree`) of `reviewed` with the main commit `head` is based
        on (their merge base), as the runner's own update merge builds it (`UpdateMerge.tree`, D-620).
        That accepts the runner's update merge and GitHub's "Update branch" whether it merges main in or
        rebases onto it, and rejects a new commit, a merge of anything else, and a merge that adds content of its
        own (an "evil merge"). That base must be at or after the reviewed commit's fork point from main,
        so a head crafted on an older main commit is never trusted. The reviewed commit and the pull
        request's head are fetched first, since a fresh checkout may lack them; None when git still
        cannot say (D-584)."""
        if number:
            self._run(["git", "fetch", "--quiet", "origin", f"+refs/pull/{number}/head:refs/runner/pr-{number}"], None, self.root)
        if not self._has_commit(reviewed):
            self._run(["git", "fetch", "--quiet", "origin", reviewed], None, self.root)
        main = self.main_head()
        if not main or not self._has_commit(reviewed) or not self._has_commit(head):
            return None
        code, base, _ = self._run(["git", "merge-base", head, main], None, self.root)
        if code != 0:
            return False if code == 1 else None  # 1: no common history with main
        fresh = self._not_older_than_fork(reviewed, base.strip(), main)
        if fresh is not True:
            return fresh
        try:
            # The update merge's own tree (D-620): git's merge, with docs/decisions.md as main's entries then
            # the branch's, and kit/plan.lock recomputed when both sides rewrote it; None means a conflict.
            tree, _ = self.updates.tree(reviewed, base.strip())
        except RuntimeError:
            return None
        return tree is not None and tree == self.git("rev-parse", f"{head}^{{tree}}", check=False)

    def update_branch(self, branch: str, reviewed: str) -> dict[str, Any]:
        """Merge main into a task branch whose head is the reviewed head (stage C, D-620).

        {"status": "current"} when main is already in it; "moved" when origin's branch is not the
        reviewed head (someone pushed: the next cycle judges the new head); "conflict" with `paths`
        when a path other than docs/decisions.md and a lock both sides kept true conflicts (never
        resolved here); "pushed" with `sha` and `main` once the merge commit, checked against
        `UpdateMerge.tree` first, was pushed as a fast-forward (never forced); "push_failed" with the
        rejection; "error" when git cannot say. A dry run builds nothing."""
        if self.dry_run:
            return {"status": "current"}
        try:
            return self._update_branch(branch, reviewed)
        except BEST_EFFORT_ERRORS as exc:
            self.log.append(f"update merge of {branch} not made: {str(exc)[:200]}")
            return {"status": "error", "why": str(exc)[:200]}

    def _update_branch(self, branch: str, reviewed: str) -> dict[str, Any]:
        main = self.main_head()
        remote = self._remote_tip(branch) if main else None
        if not main or remote != reviewed:
            return {"status": "moved", "why": f"origin/{branch} is {str(remote)[:12]}, not the reviewed head {reviewed[:12]}"}
        if self._is_ancestor(main, reviewed):
            return {"status": "current"}
        sha, conflicts = self.updates.commit(reviewed, main, branch)
        if sha is None:
            return {"status": "conflict", "paths": conflicts, "main": main}
        if self.updates.is_update_merge(reviewed, sha) != (True, main):
            return {"status": "error", "why": f"the update merge {sha[:12]} failed its own tree check"}
        if not self.push_branch(branch, sha):
            return {"status": "push_failed", "why": self.last_rejection, "main": main}
        self.log.append(f"update merge: main {main[:12]} merged into {branch} as {sha[:12]}")
        return {"status": "pushed", "sha": sha, "main": main}

    def update_merge_of(self, reviewed: str, head: str, number: int) -> bool | None:
        """Whether `head` is an update merge of the reviewed head: its parents are exactly the reviewed
        commit and a commit on main, and its tree is `UpdateMerge.tree` of the two (D-620). The runner's
        own update merge whose record was lost, or GitHub's "Update branch" by merge. None when git
        cannot say."""
        if number:
            self._run(["git", "fetch", "--quiet", "origin", f"+refs/pull/{number}/head:refs/runner/pr-{number}"], None, self.root)
        tip = self.main_head()
        if not tip or not self._has_commit(reviewed) or not self._has_commit(head):
            return None
        try:
            merged, main = self.updates.is_update_merge(reviewed, head)
        except RuntimeError:
            return None
        if not merged or self.in_main(main) is not True:
            return False
        return self._not_older_than_fork(reviewed, main, tip)  # as reviewed_plus_main checks its base (D-584)

    def _not_older_than_fork(self, reviewed: str, base: str, main: str) -> bool | None:
        """Whether `base` (the main commit a head was built on) is at or after the reviewed commit's own
        fork point from main: a head built on an older main commit could carry back a change main has
        since reverted (D-584). None when git cannot say."""
        code, fork, _ = self._run(["git", "merge-base", reviewed, main], None, self.root)
        if code != 0:
            return False if code == 1 else None
        return self._is_ancestor(fork.strip(), base)

    def _has_commit(self, sha: str) -> bool:
        return bool(sha) and self._run(["git", "cat-file", "-e", f"{sha}^{{commit}}"], None, self.root)[0] == 0

    def collect_result(
        self, task: dict[str, Any], outcome: str, transcripts: list[str], reviewer_transcripts: list[str],
        log: str = "", reset_at: datetime | None = None,
    ) -> WorkerResult:
        """A WorkerResult from what the runner reads itself: git diff --no-renames and the transcripts.
        A submitted head whose docs/decisions.md has new entries filed before its base's last entry gets
        a runner commit on top that moves them to the end (`_hand_in_in_order`, D-1029)."""
        branch = self.branch_for(task["id"])
        base = self.main_head()
        head = self.git("rev-parse", f"refs/heads/{branch}", check=False)
        if outcome == "submitted" and not head:
            # No task branch: work left on a detached HEAD or another branch must never pass as an empty
            # hand-in that certifies main, so the attempt fails (D-551).
            return WorkerResult("tests_failed", log=f"no task branch {branch} to hand in", branch=branch)
        remote = self._fetch(branch) if outcome == "submitted" else None
        if remote and not self._is_ancestor(remote, head):
            # The push would be rejected as a non-fast-forward, and the runner never force-pushes (D-583).
            return WorkerResult("tests_failed", log=NOT_BUILT_ON.format(branch=branch), branch=branch, head_sha=head)
        if outcome == "submitted" and base:
            head = self._hand_in_in_order(branch, head, base)
            red = self.prehandin(branch, head, base)  # a red gate fails the hand-in before any review record or push (D-1300)
            outcome, log = ("tests_failed", red) if red else (outcome, log)
        changed = self.git("diff", "--no-renames", "--name-only", f"{base}...{head}", check=False) if head else ""
        evidence, usage = read_transcripts(transcripts)
        review, review_usage = read_transcripts(reviewer_transcripts)
        usage = {k: usage.get(k, 0) + review_usage.get(k, 0) for k in {*usage, *review_usage}}  # both count (pacing)
        return WorkerResult(outcome, [p for p in changed.splitlines() if p], evidence, reset_at, log, branch, head, review, usage,
                            self._commits_ahead(base, head))

    def prehandin(self, branch: str, head: str, main: str) -> str:
        """'' when main's scripts/prehandin.sh --quick passes on `head` against `main`, else the failure log
        (PREHANDIN_RED, then the gate's last lines, redacted) (D-1300).

        Main's copy of the gate runs (a task branch cannot weaken it) in a detached worktree of the head,
        so the checks run over exactly the tree that would be pushed, with the head's own scripts as `Ci`
        runs them. Only exit status 1 is red: a gate main does not have yet, a dry run, a worktree git
        cannot make, a timeout or any other exit (3 is the gate's own "cannot check": no base, no merge
        base) is logged and lets the hand-in through, so the gate never fails an attempt for a reason that
        is not the work's. A check script main added after the head was made is skipped by the gate."""
        script = self._show(main, PREHANDIN) if main and head and not self.dry_run else None
        if not script:
            return ""
        with tempfile.TemporaryDirectory() as tmp:
            tree, gate = Path(tmp) / "head", Path(tmp) / "prehandin.sh"
            gate.write_text(script, encoding="utf-8")
            if self._run(["git", "worktree", "add", "--detach", str(tree), head], None, self.root)[0] != 0:
                self.log.append(f"pre-hand-in gate not run: no worktree of {head[:12]}")
                return ""
            try:
                code, out, err = self._run(["env", f"PREHANDIN_ROOT={tree}", "bash", str(gate), "--quick", "--base", main,
                                            "--branch", branch], None, tree)
            except subprocess.TimeoutExpired:
                code, out, err = 124, "", f"timed out after {PREHANDIN_TIMEOUT} seconds"
            finally:
                self.git("worktree", "remove", "--force", str(tree), check=False)
        return self._gate_verdict(head, code, out + err)

    def _gate_verdict(self, head: str, code: int, output: str) -> str:
        """The gate's exit status and output as prehandin's answer, with one cycle-log line (D-1300)."""
        rows = [_redact_gate_row(r)[:EXCERPT_WIDTH] for r in output.strip().splitlines()]
        last = rows[-1] if rows else ""
        if code != 1:
            self.log.append(f"pre-hand-in gate on {head[:12]}: exit {code}; {last}" + ("" if code == 0 else " (not a verdict: let through)"))
            return ""
        self.log.append(f"pre-hand-in gate on {head[:12]}: red; {last}")
        tail = "\n".join(rows[-PREHANDIN_LINES:])[-PREHANDIN_CAP:]
        return f"{PREHANDIN_RED} on {head[:12]} (bash scripts/prehandin.sh, D-1300):\n{tail}"

    def _decisions_in_order(self, branch: str, head: str, main: str) -> str | None:
        """`UpdateMerge.in_order` of `head`: a commit on top of it whose docs/decisions.md has the new
        entries after main's last entry, or None when there is nothing to fix, nothing safe to do (a
        changed or removed entry stays a real failure for scripts/check_pr_change.py) or git fails (D-1029)."""
        try:
            fixed = self.updates.in_order(head, main, branch)
        except RuntimeError as exc:
            self.log.append(f"decision entries of {branch} not re-ordered: {str(exc)[:200]}")
            return None
        if fixed:
            self.log.append(f"decision entries of {branch} moved after main's last entry as {fixed[:12]} (D-1029)")
        return fixed

    def _hand_in_in_order(self, branch: str, head: str, main: str) -> str:
        """The hand-in's head after `_decisions_in_order`: the local task branch moves to the fixing commit
        (a fast-forward of the checkout when the branch is checked out, so the working tree follows; the
        old head when that fails), and the push that follows carries it (D-1029)."""
        fixed = self._decisions_in_order(branch, head, main)
        if not fixed:
            return head
        if self.git("symbolic-ref", "-q", "HEAD", check=False) == f"refs/heads/{branch}":
            code, _, err = self._run(["git", "merge", "--ff-only", "--quiet", fixed], None, self.root)
        else:
            code, _, err = self._run(["git", "update-ref", f"refs/heads/{branch}", fixed, head], None, self.root)
        if code != 0:
            self.log.append(f"decision entries of {branch} not re-ordered: {err.strip()[:200]}")
            return head
        return fixed

    def _commits_ahead(self, base: str, head: str) -> int | None:
        """The head's commits that main lacks (`git rev-list --count base..head`); 0 is an empty hand-in
        (only ever for a task branch that exists), None when git cannot say (D-551)."""
        out = self.git("rev-list", "--count", f"{base}..{head}", check=False) if base and head else ""
        return int(out) if out.isdigit() else None

    def run_worker(self, task: dict[str, Any], model: str, log: str, heartbeat: Callable[[], None]) -> WorkerResult:
        command = os.environ.get("RUNNER_WORKER_CMD", "")
        if self.dry_run or not command:
            return WorkerResult("handed_off")
        with tempfile.TemporaryDirectory() as tmp:
            files = {name: Path(tmp) / name for name in ("task.json", "log.txt", "result.json")}
            files["task.json"].write_text(json.dumps(task), encoding="utf-8")
            files["log.txt"].write_text(log, encoding="utf-8")
            mark = model_window.mark_of(task)
            alias = mark["alias"] if mark and mark["model"] == model else alias_for(model)
            env = dict(os.environ, RUNNER_TASK_FILE=str(files["task.json"]), RUNNER_MODEL=model,
                       RUNNER_MODEL_ALIAS=alias, RUNNER_LOG_FILE=str(files["log.txt"]),
                       RUNNER_BRANCH=self.branch_for(task["id"]), RUNNER_RESULT_FILE=str(files["result.json"]))
            code = wait_with_heartbeat(subprocess.Popen(shlex.split(command), cwd=self.root, env=env), heartbeat)
            report = json.loads(files["result.json"].read_text(encoding="utf-8")) if files["result.json"].exists() else {}
        if not report:
            return WorkerResult("tests_failed", log=f"worker command exited {code} without a result file")
        reset = st.parse(report["reset_at"]) if report.get("reset_at") else None
        return self.collect_result(task, str(report.get("outcome", "tests_failed")), list(report.get("transcripts", [])),
                                   list(report.get("reviewer_transcripts", [])), str(report.get("log", "")), reset)


def alias_for(model: str) -> str:
    """claude-sonnet-5-5 -> sonnet: the worker call accepts only the alias (D-170)."""
    return next((a for a in ALIASES if f"-{a}-" in f"-{model}-"), model)


def wait_with_heartbeat(proc: subprocess.Popen[bytes], heartbeat: Callable[[], None]) -> int:
    """Wait for the worker, calling heartbeat every minute so the lease is renewed when due."""
    while True:
        try:
            return proc.wait(timeout=HEARTBEAT_SECONDS)
        except subprocess.TimeoutExpired:
            heartbeat()


def read_transcripts(paths: Iterable[str]) -> tuple[list[str], dict[str, int]]:
    """Models (verify.models_in_transcript) and token totals (verify.usage_in_transcript) over transcripts."""
    models: list[str] = []
    totals: dict[str, int] = {}
    for path in paths:
        lines = Path(path).read_text(encoding="utf-8").splitlines()
        models += verify.models_in_transcript(lines)
        for key, value in verify.usage_in_transcript(lines).items():
            totals[key] = totals.get(key, 0) + value
    return models, totals


AGENT_ID = re.compile(r"^[0-9a-z]+$")


def agent_id_list(agent_ids: Iterable[str]) -> list[str]:
    """The agent IDs named by `--worker-agent` and `--reviewer-agent` values, one per flag or several
    joined by commas or spaces in one flag, each without an `agent-` prefix (D-1328: T241's first
    attempt passed `--worker-agent a,b`, no transcript matched, and a finished hand-in failed for lack
    of evidence). A value that is not a plain id is reported and dropped, never put in a glob."""
    ids: list[str] = []
    for value in agent_ids:
        for part in value.replace(",", " ").split():
            agent = part[len("agent-"):] if part.startswith("agent-") else part
            if not AGENT_ID.match(agent):
                print(f"runner: not an agent id, ignored: {part}")
            elif agent not in ids:
                ids.append(agent)
    return ids


def find_transcripts(agent_ids: Iterable[str], home: Path | None = None) -> list[str]:
    """Paths of subagent transcripts by agent ID (D-170 layout); an id with no transcript is
    reported in the job log, so a slip in the hand-in command shows up (D-1328)."""
    base = home or Path.home()
    found: list[str] = []
    for agent in agent_id_list(agent_ids):
        paths = sorted(glob.glob(str(base / ".claude" / "projects" / "*" / "*" / "subagents" / f"agent-{agent}.jsonl")))
        if not paths:
            print(f"runner: no transcript for agent {agent}")
        found += paths
    return found
