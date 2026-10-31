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
  python3 -m runner --requeue T### --plan-fix D-### --reason TEXT
                                    give a task that used all its attempts exactly one more (on Opus)
                                    after a plan fix: since its last attempt started, D-### is new on
                                    main in docs/decisions.md and in the task's own `decisions` list, and
                                    its kit/tasks.json entry or prompt changed on main; once per decision,
                                    never under a lease that matters (D-677)
  python3 -m runner --reserve-decision --reason TEXT
                                    take ONE D-number from the live counter for a change made outside
                                    the runner (a plan change, a human fix): the next free number at or
                                    above `next_decision`, above main's highest entry and every reserved
                                    number, kept in `plan_reservations` with `next_decision` moved past
                                    it, and printed as `reserved D-####` (D-1058)
  python3 -m runner --restore-lanes end a fall back to one lane (`lanes_fallback`, D-620)
  RUNNER_LANE                       the lane a backup-runner run serves (its gate picks it): the run
                                    claims only on that lane; unset, the first free lane (D-620);
                                    with two or more lanes a cycle that hands off starts one run
                                    per refill lane at once, with the job token (D-723)
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
import fnmatch
import json
import os
import random
import subprocess
import sys
import tempfile
import uuid
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath
from typing import Any

from runner import cycle, lanes as lane_policy, proof, state as st
from runner.github_env import GitHubEnv, find_transcripts, state_diff

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import check_plan_lock  # scripts/, standard library only
import check_state
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
    ap.add_argument("--plan-fix", default="")
    ap.add_argument("--reason", default="")
    ap.add_argument("--restore-lanes", action="store_true")
    ap.add_argument("--reserve-decision", action="store_true")
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
                       proof_stages=frozenset(proof.MACHINE_PROOFS), stage_proof=stage_proof,
                       lane_hint=lane_hint(os.environ.get("RUNNER_LANE", "")), locked=locked_path)
    return cfg, env


def lane_hint(raw: str) -> str | None:
    """RUNNER_LANE from backup-runner.yml's gate: the lane this run serves (D-620); None when unset or not a lane."""
    value = raw.strip()
    return value if value.isdigit() and int(value) >= 1 else None


def locked_path(path: str) -> bool:
    """Whether an owned path (a file, or a folder ending "/") holds a locked blueprint file
    (scripts/check_plan_lock.py's lists), so its task also rewrites kit/plan.lock (D-620)."""
    pure = PurePosixPath(path.rstrip("/"))
    if path.endswith("/"):
        return any(name.startswith(path) for name in (*check_plan_lock.LOCKED_FILES, *check_plan_lock.LOCKED_GLOBS))
    if path in check_plan_lock.LOCKED_FILES:
        return True
    return any(pure.parent == PurePosixPath(glob).parent and fnmatch.fnmatch(pure.name, PurePosixPath(glob).name)
               for glob in check_plan_lock.LOCKED_GLOBS)


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
    """How many tasks may be in flight at once (D-515, D-620): kit/runner.json "lanes", 1 when unset or not
    a whole number of at least 1. Changing it is the whole switch to more lanes (D-620)."""
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
    if args.plan_fix:
        return plan_fix_requeue(tid, args.plan_fix.strip(), reason, cfg, env)
    why = cycle.requeue_refusal(env.read_state(), tid, now_utc(), cfg)
    if why:
        return f"requeue_refused: {why}"
    done = env.update_state(lambda fresh: cycle.requeue(fresh, tid, reason, now_utc(), cfg.session, cfg))
    if done is None:
        later = cycle.requeue_refusal(env.read_state(), tid, now_utc(), cfg)
        return f"requeue_refused: {later}" if later else "lost_race"
    return "requeued"


def plan_fix_requeue(tid: str, decision: str, reason: str, cfg: cycle.Config, env: GitHubEnv) -> str:
    """`--requeue T### --plan-fix D-### --reason TEXT`: one more attempt for a task that used all its
    attempts, after a plan fix on main (D-677). The state-only checks run first; then main is read for
    the start of the task's last attempt (cycle.plan_since, GitHubEnv.plan_evidence), and the move goes
    through update_state, which re-checks everything against the fresh state."""
    state = env.read_state()
    why = cycle.plan_fix_precheck(state, tid, decision, now_utc(), cfg)
    if why:
        return f"requeue_refused: {why}"
    task = {**st.entry(state, tid), **next((t for t in cfg.graph if t["id"] == tid), {})}
    since = str(cycle.plan_since(st.entry(state, tid)))
    evidence = env.plan_evidence(tid, str(task.get("prompt") or f"kit/prompts/{tid}.md"), since)
    why = cycle.plan_fix_refusal(state, tid, decision, evidence, now_utc(), cfg)
    if why:
        return f"requeue_refused: {why}"
    done = env.update_state(lambda fresh: cycle.requeue_plan_fix(fresh, tid, reason, decision, evidence, now_utc(), cfg.session, cfg))
    if done is None:
        later = cycle.plan_fix_refusal(env.read_state(), tid, decision, evidence, now_utc(), cfg)
        return f"requeue_refused: {later}" if later else "lost_race"
    return "requeued_after_plan_fix"


def reserve_decision(args: argparse.Namespace, cfg: cycle.Config, env: GitHubEnv) -> str:
    """`--reserve-decision --reason TEXT`: one D-number for a plan change from the live counter (D-1058).

    Main's docs/decisions.md is read fresh (GitHubEnv.decision_numbers); the reservation goes through
    update_state, which re-reads claude/kit-state and takes the number again from the fresh state after
    a lost race, so two sessions never get the same number. Refused with an empty reason or when main's
    file cannot be read (a bad read would otherwise hand out a number main already has)."""
    reason = args.reason.strip()
    if not reason:
        return "reserve_refused: --reason is empty"
    taken = env.decision_numbers()
    if taken is None:
        return "reserve_refused: main's docs/decisions.md could not be read"
    got: dict[str, int] = {}

    def take(fresh: st.State) -> st.State:
        out, got["number"] = st.reserve_plan_number(fresh, taken, reason, cfg.session, now_utc())
        return out

    if env.update_state(take) is None:
        return "lost_race"
    return f"reserved D-{got['number']:03d}"


def restore_lanes(env: GitHubEnv) -> str:
    """`--restore-lanes`: end a fall back to one lane through update_state, so every lane in
    kit/runner.json is used again (D-620); the fall back is kept in `lanes_fallback_history`."""
    if not env.read_state().get("lanes_fallback"):
        return "restore_refused: no lane fall back is set"
    return "lanes_restored" if env.update_state(lane_policy.restore) is not None else "lost_race"


def branch_note(prep: dict[str, Any]) -> str:
    """What the session must do with the branch prepare_branch made (D-583), '' when it made none."""
    if not prep.get("branch_ready"):
        return ""
    name = prep["branch"]
    if prep.get("resync"):
        return (f"The runner made the local branch {name} at origin/{name}, the reviewed work, WITHOUT main merged in: this is "
                f"a resync (D-891). Run `git switch {name}`, then `git merge {str(prep.get('main') or 'origin/main')}` (a normal "
                "merge commit), resolve the conflicts keeping both sides, and commit there; never create, reset or rebase "
                "the branch, whatever step 6 of the prompt's start routine says (D-583).")
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
    mine = next((lease for lease in st.leases_of(state).values() if lease and lease.get("session") == cfg.session), None)
    tid = str((mine or state.get("lease") or {}).get("task", ""))  # the lane this session claimed (D-620)
    task = {**st.entry(state, tid), **next((t for t in cfg.graph if t["id"] == tid), {})}
    info = {
        "session": cfg.session, "task": tid, "branch": env.branch_for(tid), "prompt": _prompt_for(task, tid),
        "model": cycle.attempt_model(task, int(task.get("attempts", 0)), cfg.models) if task.get("model") else None,
        "reviewer_model": task.get("reviewer_model"), "last_failure": task.get("last_failure"), "goal": task.get("goal"),
        # Non-empty only when the branch was made at the reviewed head: merge main in, resolve, nothing else (D-891).
        "resync": cycle.resync_log(task) if env.prepared.get("resync") else "",
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
    if args.restore_lanes:
        return restore_lanes(env)
    if args.reserve_decision:
        return reserve_decision(args, cfg, env)
    return cycle_with_retries(cfg, env)


LOST_RACE_RETRIES = 2  # extra cycles after a lost race, with two lanes and a handed-off worker (D-620)


def cycle_with_retries(cfg: cycle.Config, env: GitHubEnv) -> str:
    """One cycle; with two or more lanes a lost race (another lane's run wrote first, the usual way two
    runs started together collide) is tried again from a fresh read, up to LOST_RACE_RETRIES times after
    a short random wait, so the second lane still claims. Only when the worker is handed off
    (RUNNER_WORKER_CMD unset, as in backup-runner.yml), where a cycle never runs a worker, so a retry can
    never start a second task in one session. A storm that outlasts the retries is a strike on this
    run's lane (D-620)."""
    result = cycle.run_cycle(cfg, env, now_utc())
    if cfg.lanes < 2 or os.environ.get("RUNNER_WORKER_CMD"):
        return result
    for _ in range(LOST_RACE_RETRIES):
        if result != "lost_race":
            return result
        env.sleep(random.uniform(1.0, 5.0))
        result = cycle.run_cycle(cfg, env, now_utc())
    if result == "lost_race":
        cycle.record_strike(cfg, env, cfg.lane_hint, f"lane {cfg.lane_hint}: {LOST_RACE_RETRIES + 1} lost races in a row in one run")
    return result


def refill(cfg: cycle.Config, env: GitHubEnv) -> list[str]:
    """The lanes the backup-runner chain step should start a run for (D-620), from the last state this
    run saw; [] with one lane (the chain then works exactly as before) or when it cannot be worked out."""
    if cfg.lanes < 2 or not env.last_state:
        return []
    try:
        return cycle.refill_lanes(cfg, env.last_state, now_utc())
    except Exception as exc:  # noqa: BLE001  # reason: a failed outlook must never change the cycle's result (D-620)
        print(f"runner: refill lanes not worked out: {str(exc)[:200]}")
        return []


def start_refill(lanes: list[str], env: GitHubEnv) -> list[str]:
    """Start one backup-runner run per refill lane now, when this cycle handed a task off (D-723).

    The chain step starts refill runs only when its job ends, and a handed-off job ends only after its
    worker's whole session, so a lane freed by this cycle's resolved merges (or left free beside the
    claim) would sit idle that long. Only a hand-off starts runs here: every other result ends the job
    within minutes and the chain step refills. The refill lanes never include this run's own lane (its
    new lease holds it), and a started run that claims nothing reports a result that starts nothing,
    so this cannot loop. Returns the lanes started; a failed start never changes the result."""
    started = []
    for lane in lanes:
        try:
            if env.dispatch_lane(lane):
                started.append(lane)
        except Exception as exc:  # noqa: BLE001  # reason: a refill that could not start must never change the cycle's result (D-723)
            print(f"runner: refill run for lane {lane} not started: {str(exc)[:200]}")
    return started


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    with tempfile.TemporaryDirectory() as outbox:
        cfg, env = build(args, Path(outbox))
        result = run(args, cfg, env)
        report: dict[str, Any] = {"result": result, "session": cfg.session, "starter": env.starter, "runner_sha": env.runner_sha,
                                  "changed": cycle_diff(env), "log": env.log}
        if result == "handed_off":
            report["handoff"] = write_handoff(cfg, env)
        if args.after and not args.dry_run:
            close_handoff(result)
        if cfg.lanes >= 2:
            report["lane"], report["refill"] = cfg.lane_hint, refill(cfg, env)
            if result == "handed_off" and not args.dry_run:
                report["dispatched"] = start_refill(report["refill"], env)
        try:
            env.write_cycle_log({k: v for k, v in report.items() if k != "handoff"})
        except Exception as exc:  # noqa: BLE001  # reason: a lost log line must never change the cycle's result (D-499)
            print(f"runner: cycle log not written: {exc}")
    print(f"runner: {result}" + (" (dry run)" if args.dry_run else ""))
    print(json.dumps(report, sort_keys=True))
    return 2 if result == "state_invalid" else 0


AFTER_KEEPS_HANDOFF = ("not_lease_holder", "lost_race", "state_invalid")


def close_handoff(result: str) -> None:
    """Remove the hand-off file once `--after` finished the task (D-620), so backup-runner.yml's safety
    step sees no hand-off and writes no second, empty `--after` row after the real one; the chain step
    reads that real row. A hand-off whose `--after` did not finish (not this session's lease, a lost
    race, an invalid state) is kept, so the safety step still frees the lease."""
    if result not in AFTER_KEEPS_HANDOFF:
        (ROOT / HANDOFF).unlink(missing_ok=True)


def cycle_diff(env: GitHubEnv) -> dict[str, Any]:
    return state_diff(env.first_state or {}, env.last_state or {})


if __name__ == "__main__":
    sys.exit(main())
