"""The runner's real outside world: git plus the GitHub REST API through `gh api` (D-486, BUILD_BRIEF.md:205-292).

GraphQL is blocked from sessions (BUILD_BRIEF.md:255), so every GitHub call here is REST, with one
fallback: `enable_auto_merge` tries the ccr REST route first and, only if that route is absent, `gh pr
merge --auto`, which GitHub serves over GraphQL; under the Actions runner that fallback is reachable,
in a session it fails closed and the cycle reports it.

Where things live:
  - `claude/kit-state`: kit/state.json, STATUS.md, PROGRESS.md, outbox/, inbox/, kit/nightly.json and
    kit/owner_answers.jsonl (governance section 8);
  - `claude/lease`: one file, lease.json = {"lease": {task, session, expires} | null, "seq": n}. Every
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
  RUNNER_TASK_FILE    JSON of the task (kit/tasks.json entry merged with its state entry)
  RUNNER_MODEL        the full model ID the attempt must run on
  RUNNER_MODEL_ALIAS  haiku | sonnet | opus (the worker call takes only the alias, D-170)
  RUNNER_LOG_FILE     the previous attempt's failure log (may be empty)
  RUNNER_BRANCH       the task branch to commit to (local; the runner pushes it after review)
  RUNNER_RESULT_FILE  where the command writes JSON: {"outcome": ..., "transcripts": [paths],
                      "reviewer_transcripts": [paths], "reset_at": ISO | null, "log": str}
The runner takes only the outcome, reset time, log and transcript paths from that file. Changed
paths come from `git diff --no-renames`, the head from `git rev-parse`, and model evidence and token
usage from the transcripts themselves (D-170: ~/.claude/projects/<proj>/<session>/subagents/agent-<id>.jsonl).
"""

from __future__ import annotations

import glob
import json
import os
import shlex
import subprocess
import tempfile
from collections.abc import Callable, Iterable
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from runner import state as st, verify
from runner.cycle import WorkerResult
from runner.state import State

Runner = Callable[[list[str], str | None, Path], tuple[int, str, str]]
STATE_FILE = "kit/state.json"
LEASE_FILE = "lease.json"
NIGHTLY_FILE = "kit/nightly.json"
RUNNER_LOG = "kit/runner_log.jsonl"  # one JSON line per cycle, the last 200 kept (D-499)
ANSWERS_FILE = "kit/owner_answers.jsonl"
PUSH_RETRIES = 3
RUNNER_FILES = (STATE_FILE, NIGHTLY_FILE, ANSWERS_FILE, RUNNER_LOG, "STATUS.md", "PROGRESS.md", "inbox", "outbox")
HEARTBEAT_SECONDS = 60
ALIASES = {"haiku": "haiku", "sonnet": "sonnet", "opus": "opus"}


def run_command(args: list[str], stdin: str | None, cwd: Path) -> tuple[int, str, str]:
    """subprocess.run without a shell; returns (exit code, stdout, stderr)."""
    done = subprocess.run(args, input=stdin, capture_output=True, text=True, cwd=cwd, timeout=600, check=False, env=_env_for(args))
    return done.returncode, done.stdout, done.stderr


def _env_for(args: list[str]) -> dict[str, str] | None:
    """Check-run reads use the job's own token (D-502).

    Inside the Claude Code action, GH_TOKEN is the Claude GitHub App's token, which has no Checks
    permission, so `commits/<sha>/check-runs` answers 403. The workflow passes the job token (which has
    `checks: read`) as RUNNER_CHECKS_TOKEN; it is used only for those reads and never put in argv.
    """
    token = os.environ.get("RUNNER_CHECKS_TOKEN", "")
    if token and args[:2] == ["gh", "api"] and "/check-runs" in args[-1]:
        return dict(os.environ, GH_TOKEN=token)
    return None


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


def _attention(state: State, now: datetime) -> list[str]:
    lines = [f"- BLOCKED {t['id']}: {t.get('reason', '')}" for t in state.get("tasks", []) if t.get("status") == st.BLOCKED]
    lines += [f"- Waiting for Casey: {t['id']}" for t in state.get("tasks", []) if t.get("status") == st.WAITING_FOR_OWNER]
    lines += [f"- Deviation {d.get('id')}: {d.get('rule')} ({d.get('owner')}, until {d.get('expiry')})" for d in st.open_deviations(state, now)]
    red = state.get("main_red")
    if red:
        lines.insert(0, f"- MAIN IS RED: {red.get('check') if isinstance(red, dict) else red}; only FIX-MAIN runs")
    return lines or ["- nothing"]


def render_status(state: State, progress_md: str, now: datetime, pacing: str = "") -> str:
    """STATUS.md: progress in verified requirements first (governance 6), the pacing line (window used and
    left, next reset, projected finish; scripts/pacing.py), then BUILD_BRIEF.md:276-281."""
    lease = state.get("lease") or {}
    usage = (state.get("usage") or {}).get("last_reading")
    parts = [
        "# Ask Hava build status", "", f"Updated {st.iso(now)} by runner {str(state.get('runner_sha', ''))[:12]} ({state.get('starter', '')}).",
        "", progress_md.strip() or "Progress unavailable.", "", "## Pacing", f"- {pacing or 'no reading yet'}",
        "", "## Tasks done (throughput)", *_stage_counts(state),
        "", "## Running", f"- {lease.get('task', 'nothing')}" + (f" (lease until {lease['expires']})" if lease else ""),
        "", "## Needs attention", *_attention(state, now), "", "## Last usage reading", f"- {json.dumps(usage) if usage else 'none yet'}", "",
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
            state["lease"] = json.loads(lease_raw).get("lease")
        state.pop("lease_on", None)
        self.first_state = self.first_state or json.loads(json.dumps(state))
        self.last_state = state
        return state

    def _lease_seq(self) -> int:
        raw = self._show(self.lease_head, LEASE_FILE)
        return int(json.loads(raw).get("seq", 0)) if raw else 0

    def _state_files(self, new: State, base: State) -> tuple[dict[str, str], list[str]]:
        now = self.clock()
        stamped = dict(new, runner_sha=self.runner_sha, starter=self.starter, updated_at=st.iso(now), lease=None, lease_on=self.lease_ref)
        pace = self.pacing_line(new) if self.pacing_line else ""
        status = render_status(stamped | {"lease": new.get("lease")}, self.progress_markdown(new), now, pace)
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

    def _trailers(self, what: str) -> str:
        return f"runner: {what}\n\nRunner-Sha: {self.runner_sha}\nRunner-Starter: {self.starter}\n"

    def push_state(self, new: State, base: State) -> bool:
        """Atomic push of claude/kit-state and claude/lease; retry only when nothing but inbox/outbox moved."""
        for _ in range(PUSH_RETRIES):
            writes, removes = self._state_files(new, base)
            state_commit = self._commit(self.state_head, writes, removes, self._trailers("state"))
            lease_doc = {"lease": new.get("lease"), "seq": self._lease_seq() + 1, "runner_sha": self.runner_sha, "starter": self.starter}
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

    def smoke_main(self) -> tuple[bool, str]:
        """bash scripts/smoke.sh in a detached worktree of main's head (the runner's own tool run)."""
        head = self.main_head()
        if self.dry_run:
            self.log.append(f"dry-run: smoke test not run on {head[:12]}")
            return True, head
        with tempfile.TemporaryDirectory() as tmp:
            tree = Path(tmp) / "main"
            self.git("worktree", "add", "--detach", str(tree), head)
            try:
                code, out, err = self._run(["bash", "scripts/smoke.sh"], None, tree)
            finally:
                self.git("worktree", "remove", "--force", str(tree), check=False)
        tail = (out + err).strip().splitlines()
        self.log.append(f"smoke on {head[:12]}: exit {code}; {tail[-1] if tail else ''}")
        return code == 0, head

    def main_checks_green(self, head: str) -> bool | None:
        """True when `Ci`, `Twin` and `Ally` all concluded success for that commit of main; False when
        any of them failed; None when a required check is missing or still running (REST, read only).

        A squash commit on main carries no check runs of its own (the check workflows run on `claude/**`
        pushes and pull requests), so the verdict is read from the merged pull request's head, which
        GitHub lists for the commit; a direct read of the commit comes first for a commit that has them."""
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

    def _required_checks(self, sha: str) -> bool | None:
        try:
            runs = self.gh_json(f"repos/{self.repo}/commits/{sha}/check-runs?per_page=100").get("check_runs", [])
        except RuntimeError as exc:
            self.log.append(f"check-runs on {sha[:12]} unreadable: {exc}")
            return None
        latest: dict[str, str] = {}
        for run in sorted(runs, key=lambda r: str(r.get("started_at") or "")):
            done = run.get("status") == "completed"
            latest[str(run.get("name"))] = str(run.get("conclusion") or "") if done else "running"
        verdicts = [latest.get(name) for name in verify.REQUIRED_CHECKS]
        if any(v in ("failure", "cancelled", "timed_out") for v in verdicts):
            return False
        if all(v == "success" for v in verdicts):
            return True
        return None

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
               "auto_merge": bool(pr.get("auto_merge"))}
        failed = [f"{c['name']}: {c['conclusion']}" for c in checks if c["conclusion"] in ("failure", "cancelled", "timed_out")]
        out["failure_log"] = "; ".join(failed)
        if out["merged"]:
            out.update(self._merge_facts(int(pr["number"]), head, str(pr.get("merge_commit_sha") or ""), str(pr["merged_at"])))
        return out

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
        lines.append(f"Build status: https://github.com/{self.repo}/blob/{self.state_ref}/STATUS.md")
        return "\n".join(lines)

    # ---- worker hand-off and evidence

    def push_branch(self, branch: str, head_sha: str) -> bool:
        if self.dry_run:
            return True
        code, _, err = self._run(["git", "push", "--porcelain", "origin", f"{head_sha}:refs/heads/{branch}"], None, self.root)
        if code != 0:
            self.log.append(f"branch push rejected: {err.strip()[:200]}")
        return code == 0

    def collect_result(
        self, task: dict[str, Any], outcome: str, transcripts: list[str], reviewer_transcripts: list[str],
        log: str = "", reset_at: datetime | None = None,
    ) -> WorkerResult:
        """A WorkerResult from what the runner reads itself: git diff --no-renames and the transcripts."""
        branch = self.branch_for(task["id"])
        base = self.main_head()
        head = self.git("rev-parse", f"refs/heads/{branch}", check=False)
        changed = self.git("diff", "--no-renames", "--name-only", f"{base}...{head}", check=False) if head else ""
        evidence, usage = read_transcripts(transcripts)
        review, review_usage = read_transcripts(reviewer_transcripts)
        usage = {k: usage.get(k, 0) + review_usage.get(k, 0) for k in {*usage, *review_usage}}  # both count (pacing)
        return WorkerResult(outcome, [p for p in changed.splitlines() if p], evidence, reset_at, log, branch, head, review, usage)

    def run_worker(self, task: dict[str, Any], model: str, log: str, heartbeat: Callable[[], None]) -> WorkerResult:
        command = os.environ.get("RUNNER_WORKER_CMD", "")
        if self.dry_run or not command:
            return WorkerResult("handed_off")
        with tempfile.TemporaryDirectory() as tmp:
            files = {name: Path(tmp) / name for name in ("task.json", "log.txt", "result.json")}
            files["task.json"].write_text(json.dumps(task), encoding="utf-8")
            files["log.txt"].write_text(log, encoding="utf-8")
            env = dict(os.environ, RUNNER_TASK_FILE=str(files["task.json"]), RUNNER_MODEL=model,
                       RUNNER_MODEL_ALIAS=alias_for(model), RUNNER_LOG_FILE=str(files["log.txt"]),
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


def find_transcripts(agent_ids: Iterable[str], home: Path | None = None) -> list[str]:
    """Paths of subagent transcripts by agent ID (D-170 layout); a missing one is simply not found."""
    base = home or Path.home()
    found: list[str] = []
    for agent in agent_ids:
        found += sorted(glob.glob(str(base / ".claude" / "projects" / "*" / "*" / "subagents" / f"agent-{agent}.jsonl")))
    return found
