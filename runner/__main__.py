"""`python3 -m runner`: one runner cycle from code (D-486, BUILD_BRIEF.md:205-292, D-479, D-483).

Both starters run this: the Actions runner (`backup-runner.yml`, RUNNER_STARTER=actions) and the
scheduled routine (Appendix A, RUNNER_STARTER=routine). It prints one line with the result and one
line of JSON with what changed.

  python3 -m runner                 one cycle; a claimed task is handed to this session (`handed_off`)
                                    unless RUNNER_WORKER_CMD runs it (runner/github_env.py)
  python3 -m runner --after [--transcript P ... | --worker-agent ID ...] [--reviewer-transcript P ...
                    | --reviewer-agent ID ...] [--outcome submitted|tests_failed|usage_limit|...]
                                    finish the handed-off task: path check (git diff --no-renames),
                                    model check from the transcripts, review record (reviewed_sha
                                    and lease) pushed, branch pushed, MERGE_PENDING
  python3 -m runner --renew         renew this session's lease (every 30 minutes while working)
  python3 -m runner --record-proof N --evidence TEXT
                                    record stage N's named proof for scripts/stage_exit.py
  python3 -m runner --requeue T### --reason TEXT
                                    move a task BLOCKED outside its own work (push, no head,
                                    permission, stage proof) back to QUEUED, attempts unchanged and
                                    the reason as its last_failure (refused for FIX-MAIN, a task out of
                                    attempts, or while any lease is live; recorded in `requeues`, D-583)
  --dry-run                         compute everything, push nothing, run no smoke test or worker
  --session ID                      the lease's session (default: the hand-off file, RUNNER_SESSION,
                                    or <starter>-<GITHUB_RUN_ID or random>)
  --state-ref REF                   the state branch (default claude/kit-state)

The state is validated by scripts/check_state.py first; an invalid state stops the cycle (exit 2).
Before claiming, scripts/pacing.py decides whether the task fits the usage windows (result `paced`
when nothing fits; the next cycle after the reset claims it). Each cycle first merges main into
claude/kit-state.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from runner import cycle, proof, state as st
from runner.github_env import GitHubEnv, find_transcripts, state_diff

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import check_state  # scripts/, standard library only
import pacing
import progress
import stage_exit

STARTERS = ("actions", "routine", "manual")
HANDOFF = ".git/runner_handoff.json"  # local only: the session that claimed, for --after and --renew


def now_utc() -> datetime:
    return datetime.now(timezone.utc)


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    ap = argparse.ArgumentParser(prog="python3 -m runner", description="One Ask Hava runner cycle.")
    ap.add_argument("--session")
    ap.add_argument("--after", action="store_true")
    ap.add_argument("--renew", action="store_true")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--state-ref", default="claude/kit-state")
    ap.add_argument("--lease-ref", default="claude/lease")
    ap.add_argument("--outcome", default="submitted")
    ap.add_argument("--log", default="")
    ap.add_argument("--transcript", action="append", default=[])
    ap.add_argument("--worker-agent", action="append", default=[])
    ap.add_argument("--reviewer-transcript", action="append", default=[])
    ap.add_argument("--reviewer-agent", action="append", default=[])
    ap.add_argument("--record-proof")
    ap.add_argument("--evidence", default="")
    ap.add_argument("--requeue")
    ap.add_argument("--reason", default="")
    return ap.parse_args(argv)


def git_out(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=False).stdout.strip()


def repo_name() -> str:
    """owner/repo from GITHUB_REPOSITORY or the origin URL."""
    if os.environ.get("GITHUB_REPOSITORY"):
        return os.environ["GITHUB_REPOSITORY"]
    url = git_out("remote", "get-url", "origin").removesuffix(".git")
    return "/".join(url.replace(":", "/").split("/")[-2:])


def starter_name() -> str:
    """RUNNER_STARTER: 'actions' (backup-runner.yml) or 'routine' (Appendix A); anything else is 'manual'."""
    name = os.environ.get("RUNNER_STARTER", "manual")
    return name if name in STARTERS else "manual"


def read_handoff() -> dict[str, Any]:
    try:
        found: dict[str, Any] = json.loads((ROOT / HANDOFF).read_text(encoding="utf-8"))
        return found
    except (OSError, ValueError):
        return {}


def session_id(args: argparse.Namespace, starter: str) -> str:
    if args.session:
        return str(args.session)
    if (args.after or args.renew) and read_handoff().get("session"):
        return str(read_handoff()["session"])
    if os.environ.get("RUNNER_SESSION"):
        return os.environ["RUNNER_SESSION"]
    return f"{starter}-{os.environ.get('GITHUB_RUN_ID') or uuid.uuid4().hex[:12]}"


def build(args: argparse.Namespace, outbox_dir: Path) -> tuple[cycle.Config, GitHubEnv]:
    """The Config from kit/tasks.json and the real Env, with stage gating by scripts/stage_exit.py."""
    kit = json.loads((ROOT / "kit" / "tasks.json").read_text(encoding="utf-8"))
    starter = starter_name()
    env = GitHubEnv(ROOT, kit["tasks"], outbox_dir, repo=repo_name(), starter=starter, runner_sha=git_out("rev-parse", "HEAD"),
                    state_ref=args.state_ref, lease_ref=args.lease_ref, dry_run=args.dry_run, clock=now_utc,
                    pacing_line=pacing.status_line)

    def gate(state: st.State) -> set[str]:
        env.main_head()  # merges must be local for the ancestry check
        return stage_exit.passed_stages(kit["tasks"], state, env.read_nightly(), {}, stage_exit.git_is_ancestor(ROOT))

    def stage_proof(state: st.State, stage: str) -> tuple[dict[str, Any] | None, list[str]]:
        env.main_head()  # the nightly commit and the merges must be local for git to answer
        nightly = env.read_nightly()
        world = proof.World(nightly, stage_exit.git_is_ancestor(ROOT), git_file,
                            lambda s: progress.compute(ROOT, s, nightly))
        return proof.machine_proof(stage, kit["tasks"], state, world)

    cfg = cycle.Config(kit["tasks"], kit["models"], kit["protected_paths"], kit["shared_paths"], outbox_dir,
                       session_id(args, starter), now_utc, gate, pacing.make_pacer(kit["tasks"], ROOT), pacing.usage_recorder(ROOT),
                       owner_paths(ROOT / ".github" / "CODEOWNERS") if owner_review(ROOT / "kit" / "runner.json") else (),
                       lanes(ROOT / "kit" / "runner.json"), env.decision_numbers,
                       proof_stages=frozenset(proof.MACHINE_PROOFS), stage_proof=stage_proof)
    return cfg, env


def git_file(sha: str, path: str) -> str | None:
    """The file's text in that commit (`git show sha:path`), None when absent, for runner/proof.py (D-551)."""
    done = subprocess.run(["git", "show", f"{sha}:{path}"], cwd=ROOT, capture_output=True, text=True, check=False)
    return done.stdout if done.returncode == 0 else None


def owner_review(settings: Path) -> bool:
    """False once Casey turned code-owner review off on the ruleset (D-512); true when unset."""
    if not settings.is_file():
        return True
    return bool(json.loads(settings.read_text(encoding="utf-8")).get("owner_review", True))


def lanes(settings: Path) -> int:
    """How many tasks may be in flight at once (D-515): kit/runner.json "lanes", 1 when unset or not a
    whole number of at least 1. Only lane "1" is claimed until a later stage uses more."""
    if not settings.is_file():
        return 1
    value = json.loads(settings.read_text(encoding="utf-8")).get("lanes", 1)
    return value if isinstance(value, int) and not isinstance(value, bool) and value >= 1 else 1


def owner_paths(codeowners: Path) -> tuple[str, ...]:
    """The paths in CODEOWNERS, without the leading "/" (D-511); none when the file is missing."""
    if not codeowners.is_file():
        return ()
    lines = codeowners.read_text(encoding="utf-8").splitlines()
    return tuple(line.split()[0].lstrip("/") for line in lines if line.strip() and not line.lstrip().startswith("#"))


def after(args: argparse.Namespace, cfg: cycle.Config, env: GitHubEnv) -> str:
    transcripts = list(args.transcript) + find_transcripts(args.worker_agent)
    reviews = list(args.reviewer_transcript) + find_transcripts(args.reviewer_agent)
    return cycle.after_handoff(cfg, env, lambda task: env.collect_result(task, args.outcome, transcripts, reviews, args.log))


def renew(cfg: cycle.Config, env: GitHubEnv) -> str:
    """Renew this session's lease on the fresh state (update_state, D-515): the lane it holds, lane "1"
    when it holds none (which st.renew then refuses)."""
    state = env.read_state()
    held = st.leases_of(state)
    lane = next((n for n, lease in held.items() if lease and lease.get("session") == cfg.session), st.LANE_ONE)
    task = str((held.get(lane) or {}).get("task"))
    try:
        renewed = env.update_state(lambda fresh: st.renew(fresh, task, cfg.session, cfg.clock(), lane))
    except st.LeaseError as err:
        return f"renew_refused: {err}"
    return "renewed" if renewed is not None else "lost_race"


def record_proof(args: argparse.Namespace, env: GitHubEnv) -> str:
    """Record stage N's named proof (docs/build-readiness.md section 3) with its evidence and main's head."""
    if not args.evidence.strip():
        return "proof_refused: --evidence is empty"
    state = env.read_state()
    proofs = dict(state.get("stage_proofs") or {})
    proofs[str(args.record_proof)] = {"evidence": args.evidence.strip(), "main_sha": env.main_head(), "at": st.iso(now_utc())}
    return "proof_recorded" if env.push_state(dict(state, stage_proofs=proofs), state) else "lost_race"


def requeue_task(args: argparse.Namespace, cfg: cycle.Config, env: GitHubEnv) -> str:
    """`--requeue T### --reason TEXT`: BLOCKED to QUEUED through update_state, which re-reads and
    re-checks on a lost race, so nobody edits claude/kit-state by hand (D-583)."""
    tid, reason = str(args.requeue), args.reason.strip()
    if not reason:
        return "requeue_refused: --reason is empty"
    why = cycle.requeue_refusal(env.read_state(), tid, now_utc())
    if why:
        return f"requeue_refused: {why}"
    done = env.update_state(lambda fresh: cycle.requeue(fresh, tid, reason, now_utc(), cfg.session))
    if done is None:
        later = cycle.requeue_refusal(env.read_state(), tid, now_utc())
        return f"requeue_refused: {later}" if later else "lost_race"
    return "requeued"


def branch_note(prep: dict[str, Any]) -> str:
    """What the session must do with the branch prepare_branch made (D-583), '' when it made none."""
    if not prep.get("branch_ready"):
        return ""
    name = prep["branch"]
    where = "the latest main" if prep.get("base") == "main" else f"origin/{name} (the earlier attempt's work) with the latest main merged in"
    moved = f" The old branch {prep['renamed_from']} is kept for evidence: {prep.get('why')}." if prep.get("renamed_from") else ""
    return (f"The runner made the local branch {name} from {where}.{moved} Run `git switch {name}` and commit there; "
            "never create, reset or rebase it, whatever step 6 of the prompt's start routine says (D-583).")


def _prompt_for(task: dict[str, Any], tid: str) -> str:
    """The prompt file, or for FIX-MAIN (which has no file under kit/prompts) its goal text itself (D-501)."""
    path = str(task.get("prompt", f"kit/prompts/{tid}.md"))
    return path if (ROOT / path).is_file() or not task.get("goal") else str(task["goal"])


def write_handoff(cfg: cycle.Config, env: GitHubEnv) -> dict[str, Any]:
    """Tell the session what it was handed (task, branch, prompt, models, its reserved `decision_ids`),
    and remember the session for --after."""
    state = env.last_state or {}
    tid = str((state.get("lease") or {}).get("task", ""))
    task = {**st.entry(state, tid), **next((t for t in cfg.graph if t["id"] == tid), {})}
    info = {
        "session": cfg.session, "task": tid, "branch": env.branch_for(tid), "prompt": _prompt_for(task, tid),
        "model": cycle.attempt_model(task, int(task.get("attempts", 0)), cfg.models) if task.get("model") else None,
        "reviewer_model": task.get("reviewer_model"), "last_failure": task.get("last_failure"), "goal": task.get("goal"),
        "decision_ids": list(task.get("decision_ids") or []),  # number new decision entries from these, in order (D-530)
        # The branch is already made, on top of an earlier pushed attempt when there is one: never recreate it (D-583).
        "branch_ready": bool(env.prepared.get("branch_ready")), "base": env.prepared.get("base"),
        "branch_note": branch_note(env.prepared),
    }
    if not env.dry_run:
        (ROOT / HANDOFF).write_text(json.dumps(info), encoding="utf-8")
    return info


def run(args: argparse.Namespace, cfg: cycle.Config, env: GitHubEnv) -> str:
    problems = check_state.check(env.read_state(), lanes=cfg.lanes)
    if problems:
        print("state_invalid: " + "; ".join(problems[:10]))
        return "state_invalid"
    if args.after:
        return after(args, cfg, env)
    if args.renew:
        return renew(cfg, env)
    if args.record_proof:
        return record_proof(args, env)
    if args.requeue:
        return requeue_task(args, cfg, env)
    return cycle.run_cycle(cfg, env, now_utc())


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    with tempfile.TemporaryDirectory() as outbox:
        cfg, env = build(args, Path(outbox))
        result = run(args, cfg, env)
        report: dict[str, Any] = {"result": result, "session": cfg.session, "starter": env.starter, "runner_sha": env.runner_sha,
                                  "changed": cycle_diff(env), "log": env.log}
        if result == "handed_off":
            report["handoff"] = write_handoff(cfg, env)
        try:
            env.write_cycle_log({k: v for k, v in report.items() if k != "handoff"})
        except Exception as exc:  # noqa: BLE001  # reason: a lost log line must never change the cycle's result (D-499)
            print(f"runner: cycle log not written: {exc}")
    print(f"runner: {result}" + (" (dry run)" if args.dry_run else ""))
    print(json.dumps(report, sort_keys=True))
    return 2 if result == "state_invalid" else 0


def cycle_diff(env: GitHubEnv) -> dict[str, Any]:
    return state_diff(env.first_state or {}, env.last_state or {})


if __name__ == "__main__":
    sys.exit(main())
