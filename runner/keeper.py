"""The keeper: the backup runner wakes itself up instead of waiting on GitHub's cron (D-1451).

GitHub's scheduled trigger fired 24 times in 123 hours (median gap 5 h, longest 7.9 h), so a build
that waited on it sat idle for hours. `backup-runner.yml` now ends every backup job by dispatching
itself with `wake_after` (minutes); that run's `keeper` job sleeps, then decides here, from
`kit/state.json` (claude/kit-state), `lease.json` (claude/lease) and the workflow's own run list:

- `stop`: no keeper may run (the flag is off, or main is red and FIX-MAIN waits for a person);
- `rearm`: sleep again (another backup-runner run is queued or in progress, a lease is still live, the
  pacer waits for a usage window to reset, the week's reading is at WEEK_STOP, or no task could be
  claimed, which includes the T### tasks under Gate P's `build_hold`); a wait past CAP_MINUTES, the
  weekly reset among them, is reached by chained sleeps;
- an unreadable `kit/state.json` or `lease.json` (the workflow writes {"unreadable": true} for any failed
  read but a 404) is never read as "nothing live": the keeper looks again in IDLE_MINUTES;
- `go`: dispatch an ordinary run, which goes through the gate, the lane pick, the backup job's
  concurrency group and the lease exactly as any other start;
- free lanes (D-4281): while a lane has no task in flight and `cycle.refill_lanes` (the chain step's own rule)
  names it, and the last cycle result is not a REFILL_STOP one, the keeper sleeps IDLE_MINUTES even while
  leases are live and wakes into `go` with the lanes to start, less the lanes live runs already serve.

The keeper holds no lock: the backup concurrency group and the lease stay the only locks on a cycle.
Standard library only; the workflow calls `python3 -m runner.keeper arm|wake|clamp`.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
import tempfile
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

from runner import cycle, lanes as lane_policy, model_window, state as st

ROOT = Path(__file__).resolve().parent.parent

CAP_MINUTES = 340  # 5 h 40 m: one sleep stays well under GitHub's 6-hour job limit
IDLE_MINUTES = 20  # nothing in flight and the pacer not waiting: look again soon
FLOOR_MINUTES = 10  # a computed sleep never ends before the run that armed it has finished
LEASE_MARGIN = 2  # minutes past a lease's expiry
RESET_MARGIN = 5  # minutes past the pacer's `waiting_until` (a usage window's reset)
WEEK_STOP = 0.97  # the week's reading at which the build stops for the reset (kit/pacing.json headroom 0.03)
LIMIT = 1.0  # inside a model window with `run_to_limit`, the stop is the plan's real limit instead (D-2225)
READING_HOURS = 6  # a reading this old says nothing any more (scripts/pacing.py SESSION_READING_HOURS)
NOTHING_CLAIMED = frozenset({"idle", "fix_main"})  # a cycle result after which a 20-minute look would find the same
QUIET = ("not_lease_holder", "renewed", "renew_refused", "usage_reading", "keeper_armed", "model_window")  # log rows that say nothing of the cycle
# "T057 is MERGE_PENDING": the answer of an `--after` or a renewal for a task that has already moved on, not a
# cycle result; the backup-runner chain step skips the same rows with the same pattern (D-723).
STATUS_ONLY = re.compile(r"\S+ is \S+")
# runner/__main__.py REFILL_STOP, copied because that module imports this one; a test keeps the two equal (D-4281).
REFILL_STOP = frozenset({"idle", "paced", "no_free_lane", "lease_live", "lost_race", "state_invalid", "smoke_pending",
                         "smoke_timeout", "smoke_env", "main_red", "fix_main", "fix_main_blocked", "requeue_refused"})
UNREAD = "log_unreadable"  # no readable kit/runner_log.jsonl: never the free-lane path (D-4281)
FREE = "free lanes: "  # the start of `wake_after`'s why on the free-lane path
LANE_JOB = re.compile(r"backup lane (\d+)")  # the backup job's name, `backup lane <n>` (D-4281)

Doc = dict[str, Any]


def blocker(state: Doc) -> str:
    """Why no keeper may run at all, or '' (D-1451): the flag is off, or main is red and a person must fix it.
    Gate P's `build_hold` is respected through `has_work` (runner.state.eligible holds the T### tasks)."""
    if state.get("backup_runner") != "on":
        return "backup_runner is not on"
    if state.get("main_red") and _fix_main(state).get("escalated_at"):
        return "main is red and FIX-MAIN waits for a person (D-1099)"
    return ""


def _fix_main(state: Doc) -> Doc:
    tasks = state.get("tasks") if isinstance(state.get("tasks"), list) else []
    return next((t for t in tasks if isinstance(t, dict) and t.get("id") == st.FIX_MAIN), {})


def live_leases(state: Doc, lease_doc: Doc, now: datetime) -> dict[str, Doc]:
    """{lane: lease} still live, read from claude/lease's document as the gate reads it (D-620)."""
    try:
        return st.live_leases(_merged(state, lease_doc), now)
    except (KeyError, TypeError, ValueError):
        return {}


def _merged(state: Doc, lease_doc: Doc) -> Doc:
    """kit/state.json with claude/lease's leases in it, as the gate reads them (D-620)."""
    merged = dict(state, lease=lease_doc.get("lease"))
    merged["leases"] = lease_doc.get("leases") if isinstance(lease_doc.get("leases"), dict) else {}
    return merged


def refill_config(root: Path = ROOT) -> cycle.Config | None:
    """The Config `cycle.refill_lanes` judges with, built read-only from this checkout's kit/tasks.json and
    kit/runner.json as runner/__main__.py `build` builds it (D-4281), or None (no free-lane path) when it cannot be.
    Two parts are lighter, both fail safe: no stage gate (eligibility then is a superset, as `has_work`'s) and a
    plan-health check that refuses every task a claim already marked `plan_gap` (D-3812)."""
    try:
        from runner import __main__ as entry  # here, not at the top: runner/__main__.py imports this module
        kit = json.loads((root / "kit" / "tasks.json").read_text(encoding="utf-8"))
        settings = root / "kit" / "runner.json"
        return cycle.Config(kit["tasks"], kit["models"], kit["protected_paths"], kit["shared_paths"],
                            Path(tempfile.gettempdir()), "keeper", lambda: datetime.now(timezone.utc),
                            lanes=entry.lanes(settings), proof_stages=frozenset(entry.proof.MACHINE_PROOFS),
                            locked=entry.locked_path, plan_health=lambda _state, _tid: ["marked plan_gap"],
                            claim_first=entry.claim_first(settings), order=entry.claim_order_setting(settings),
                            prefer_disjoint_crates=entry.prefer_disjoint_crates(settings),
                            exclusive_crates=entry.exclusive_crates(settings), crate_cap=entry.crate_cap(settings),
                            crate_full=entry.crate_doc_full(root))
    except Exception:  # noqa: BLE001  # reason: a kit this cannot read leaves today's keeper (D-4281)
        return None


def refill_lanes(state: Doc, lease_doc: Doc, now: datetime, cfg: cycle.Config | None) -> tuple[list[str], int]:
    """(the numbered lanes with no task in flight, the free slots: how many of them `cycle.refill_lanes` names),
    the chain step's own rule (free lanes, no red main, no pacer wait, eligibility, `lanes.compatible_ids`, the
    crate rule, `_plan_keep`; D-4281). ([], 0) without a Config or for a state it cannot read."""
    if cfg is None or state.get("unreadable") or lease_doc.get("unreadable"):
        return [], 0
    merged = _merged(state, lease_doc)
    try:
        return lane_policy.free_lanes(cfg.lanes, merged, now), len(cycle.refill_lanes(cfg, merged, now))
    except Exception:  # noqa: BLE001  # reason: a state this cannot read gives no free slot (D-4281)
        return [], 0


def free_slots(state: Doc, lease_doc: Doc, now: datetime, cfg: cycle.Config | None) -> int:
    """How many free lanes have claimable work (`refill_lanes`), 0 when that cannot be told (D-4281)."""
    return refill_lanes(state, lease_doc, now, cfg)[1]


def short_ok(last: str) -> bool:
    """Whether the last cycle result allows the free-lane path: a readable log and a result in neither
    NOTHING_CLAIMED nor REFILL_STOP, so a run that claimed nothing never leads to another short sleep (D-4281)."""
    return last != UNREAD and last not in NOTHING_CLAIMED and last.split(":", 1)[0] not in REFILL_STOP


def pacer_until(state: Doc, now: datetime) -> datetime | None:
    """The pacer's `waiting_until` while its last decision is a wait still ahead of `now`, else None."""
    pace = state.get("pacing") if isinstance(state.get("pacing"), dict) else {}
    until = pace.get("waiting_until")
    if pace.get("decision") != "wait" or not isinstance(until, str):
        return None
    try:
        moment = st.parse(until)
    except ValueError:
        return None
    return moment if moment > now else None


def _minutes(delta: timedelta, margin: int) -> int:
    return max(FLOOR_MINUTES, min(CAP_MINUTES, math.ceil(delta.total_seconds() / 60) + margin))


def wake_after(
    state: Doc, lease_doc: Doc, now: datetime, last: str = "", to_limit: bool | None = None, ended: bool | None = None,
    cfg: cycle.Config | None = None,
) -> tuple[int | None, str]:
    """(minutes, why) for the next keeper, or (None, why) when none may run (D-1451).

    A task in flight: the soonest live lease's remaining time + LEASE_MARGIN; the pacer waiting: its
    `waiting_until` - now + RESET_MARGIN; both capped at CAP_MINUTES (a longer wait, the weekly reset,
    is reached by chained sleeps) and floored at FLOOR_MINUTES; work a cycle could act on: IDLE_MINUTES;
    nothing to claim: the soonest `retry_after` + RESET_MARGIN, else CAP_MINUTES, so an idle build does
    not start a Claude session every IDLE_MINUTES. `last`, the result the arming cycle itself reported,
    overrides the state's guess: after `idle` (nothing it could claim) the short sleep is never used.
    `to_limit` (default: kit/runner.json's model window with `run_to_limit` active now and, with `end_on_limit`,
    not yet closed by the limit, D-2225) moves the week stop from WEEK_STOP to the real limit. With it, and
    once that window is recorded ended at the limit (`ended`, default from kit-state's `model_window_ended`),
    the week stop and the pacer's wait last no longer than the session reading's READING_HOURS expiry: a cycle
    then starts, refreshes the reading and claims once Casey has reset the week (D-2225). With `cfg`, free slots
    (`free_slots`) and a `last` that `short_ok` allows give IDLE_MINUTES before the lease branch (D-4281)."""
    if state.get("unreadable") or lease_doc.get("unreadable"):
        return IDLE_MINUTES, "claude/kit-state or claude/lease could not be read: look again"
    why = blocker(state)
    if why:
        return None, why
    limit, expire = window_terms(state, now, to_limit, ended)
    until = pacer_until(state, now)
    until = bounded(until, state, now) if expire and until is not None else until
    reset = week_stop(state, now, limit, expire)
    slots = free_slots(state, lease_doc, now, cfg) if until is None and reset is None and short_ok(last) else 0
    if slots:
        return IDLE_MINUTES, f"{FREE}{slots} with claimable work"
    held = live_leases(state, lease_doc, now)
    if held:
        soonest = min(st.parse(str(lease["expires"])) for lease in held.values())
        return _minutes(soonest - now, LEASE_MARGIN), f"a task is in flight; its lease ends {st.iso(soonest)}"
    if until is not None:
        return _minutes(until - now, RESET_MARGIN), f"the pacer waits until {st.iso(until)}"
    if reset is not None:
        return _minutes(reset - now, RESET_MARGIN), f"the week stopped at its limit; it resets {st.iso(reset)}"
    if has_work(state, now) and last not in NOTHING_CLAIMED:
        return IDLE_MINUTES, "idle"
    retry = next_retry(state, now)
    if retry is not None:
        return _minutes(retry - now, RESET_MARGIN), f"nothing to claim before a retry at {st.iso(retry)}"
    held = " (build_hold is set: the T### tasks wait for Gate P)" if state.get("build_hold") else ""
    return CAP_MINUTES, f"nothing to claim{held}; events and the hourly tick still start runs"


def window_terms(state: Doc, now: datetime, to_limit: bool | None, ended: bool | None) -> tuple[bool, bool]:
    """(run to the limit, readings expire) from kit/runner.json's model window and the state (D-2225): readings
    expire while the window runs to the limit and after it was recorded ended at the limit."""
    window = model_window.load() if to_limit is None or ended is None else None
    limit = model_window.run_to_limit(window, now, state) if to_limit is None else to_limit
    over = model_window.ended(window, state) if ended is None else ended
    return limit, limit or over


def reading_expiry(state: Doc) -> datetime | None:
    """When the session reading stops counting (its `at` + READING_HOURS), None without a readable one."""
    usage = state.get("usage") if isinstance(state.get("usage"), dict) else {}
    reading = usage.get("session_reading") if isinstance(usage.get("session_reading"), dict) else {}
    try:
        return st.parse(str(reading.get("at"))) + timedelta(hours=READING_HOURS)
    except ValueError:
        return None


def bounded(until: datetime, state: Doc, now: datetime) -> datetime | None:
    """The pacer's wait cut at the reading's expiry (D-2225); None once that has passed, as a stale reading no
    longer holds the pacer and its ledger was dropped when the window ended."""
    expiry = reading_expiry(state)
    cut = until if expiry is None else min(until, expiry)
    return cut if cut > now else None


def week_stop(state: Doc, now: datetime, to_limit: bool = False, expire: bool | None = None) -> datetime | None:
    """The week's `resets_at` while the plan's own reading (usage.session_reading) says the week is at or
    past WEEK_STOP, else None: the keeper then sleeps in chained steps to the reset (the A+ plan, item 5).

    With `to_limit` (a model window running to the limit, D-2225) the stop is a reading at LIMIT, and only
    while the reading is under READING_HOURS old, as the pacer reads it: the keeper then wakes when the
    reading expires (or at the reset, if sooner), so a week Casey resets by hand resumes without a person.
    `expire` (default `to_limit`) applies that expiry to the WEEK_STOP stop as well: after the window ended."""
    usage = state.get("usage") if isinstance(state.get("usage"), dict) else {}
    reading = usage.get("session_reading") if isinstance(usage.get("session_reading"), dict) else {}
    week = reading.get("week") if isinstance(reading.get("week"), dict) else {}
    try:
        used, reset = float(week.get("utilization")), st.parse(str(week.get("resets_at")))
        expires = st.parse(str(reading.get("at"))) + timedelta(hours=READING_HOURS) if (to_limit if expire is None else expire) else reset
    except (TypeError, ValueError):
        return None
    until = min(reset, expires)
    return until if used >= (LIMIT if to_limit else WEEK_STOP) and until > now else None


def has_work(state: Doc, now: datetime) -> bool:
    """A task a cycle could act on now: one eligible (stage exits not checked, so a superset), one
    MERGE_PENDING whose merge a cycle resolves, or FIX-MAIN while main is red."""
    tasks = state.get("tasks") if isinstance(state.get("tasks"), list) else []
    if any(isinstance(t, dict) and t.get("status") == st.MERGE_PENDING for t in tasks):
        return True
    try:
        return bool(st.eligible(dict(state, tasks=tasks), now))
    except (KeyError, TypeError, ValueError):
        return True  # a state this cannot read: let a cycle look


def next_retry(state: Doc, now: datetime) -> datetime | None:
    """The soonest `retry_after` still ahead among the tasks, or None."""
    soonest: datetime | None = None
    for task in state.get("tasks") or []:
        stamp = task.get("retry_after") if isinstance(task, dict) else None
        try:
            moment = st.parse(stamp) if isinstance(stamp, str) else None
        except ValueError:
            moment = None
        if moment is not None and moment > now and (soonest is None or moment < soonest):
            soonest = moment
    return soonest


def last_result(rows: list[Doc], session: str, since: str) -> str:
    """The result of this job's last cycle row in kit/runner_log.jsonl (its own session, any with "", from `since`),
    skipping rows that did nothing (a renewal, a usage reading, an `--after` holding no lease)."""
    mine = [str(r.get("result", "")) for r in rows if session in ("", r.get("session")) and str(r.get("at", "")) >= since]
    told = [r for r in mine if not r.startswith(QUIET) and not STATUS_ONLY.fullmatch(r)]
    return told[-1] if told else ""


def live_runs(runs: list[Doc], own_id: str) -> list[Doc]:
    """The workflow's runs other than this one that are not completed (queued, waiting, in progress)."""
    return [r for r in runs if str(r.get("id")) != str(own_id) and r.get("status") != "completed"]


def start_lanes(state: Doc, lease_doc: Doc, now: datetime, cfg: cycle.Config | None, live: list[Doc], jobs: list[Doc]) -> list[str]:
    """The free lanes to start a run for (D-4281): every lane with no task in flight, less each lane a live run's
    unfinished `backup lane <n>` job serves, less one more for each live run whose lane cannot be read (a gate
    still waiting), and never more than the free slots left once those live runs claim theirs."""
    free, slots = refill_lanes(state, lease_doc, now, cfg)
    taken: set[str] = set()
    unknown = 0
    for run in live:
        named = {m.group(1) for job in jobs if isinstance(job, dict) and str(job.get("run_id")) == str(run.get("id"))
                 and job.get("status") != "completed" for m in [LANE_JOB.fullmatch(str(job.get("name", "")))] if m}
        taken |= named
        unknown += 0 if named else 1
    open_lanes = [lane for lane in free if lane not in taken]
    return open_lanes[: max(0, slots - len(taken & set(free)) - unknown)]


def decide(
    state: Doc, lease_doc: Doc, runs: list[Doc] | None, own_id: str, now: datetime, last: str = UNREAD,
    jobs: list[Doc] | None = None, cfg: cycle.Config | None = None,
) -> tuple[str, int | None, str, list[str]]:
    """`on_wake`'s answer plus the lanes a `go` starts ([]: one ordinary dispatch). With the live runs' jobs read
    (`jobs`, else today's answer) and `wake_after` on the free-lane path for `last`, `go` with `start_lanes`
    even while other runs are live; none left falls through to today's answer (D-4281)."""
    why = blocker(state) if not state.get("unreadable") else ""
    if why:
        return "stop", None, why, []
    if runs is None:
        return "stop", None, "the run list could not be read; the hourly tick is the backstop", []
    live = live_runs(runs, own_id)
    short = wake_after(state, lease_doc, now, last, cfg=cfg)[1] if jobs is not None else ""
    lanes = start_lanes(state, lease_doc, now, cfg, live, jobs or []) if short.startswith(FREE) else []
    if lanes:
        return "go", None, f"{short}: start lanes {' '.join(lanes)}", lanes
    return (*on_wake(state, lease_doc, runs, own_id, now), [])


def on_wake(
    state: Doc, lease_doc: Doc, runs: list[Doc] | None, own_id: str, now: datetime, last: str = UNREAD,
    jobs: list[Doc] | None = None, cfg: cycle.Config | None = None,
) -> tuple[str, int | None, str]:
    """("stop" | "rearm" | "go", minutes for a rearm, why) when a keeper wakes (D-1451); with `cfg`, `jobs` and
    a `last` that allows it, `go` on free lanes as `decide` says (D-4281)."""
    if cfg is not None:
        return decide(state, lease_doc, runs, own_id, now, last, jobs, cfg)[:3]
    why = blocker(state) if not state.get("unreadable") else ""
    if why:
        return "stop", None, why
    if runs is None:
        return "stop", None, "the run list could not be read; the hourly tick is the backstop"
    minutes, why = wake_after(state, lease_doc, now)
    live = live_runs(runs, own_id)
    if live:
        # Never a cycle beside a live run; sleep again instead of stopping, because a run whose gate starts
        # no backup job (a closed pull request, the flag off at that moment) arms no keeper. The keeper's
        # concurrency group keeps one sleeper: a backup job that ends arms a newer keeper, which cancels this.
        return "rearm", minutes or IDLE_MINUTES, f"run {live[0].get('id')} is {live[0].get('status')}; {why}"
    if why == "idle":
        return "go", None, "nothing live, nothing to wait for and work to do: start a cycle"
    return "rearm", minutes, why


def record(state: Doc, minutes: int, now: datetime) -> Doc:
    """The state with `keeper` = {last_run_at: now, next_wake_at: now + minutes} (kit/state_schema.json, D-1462):
    the backup run that armed the keeper and when that keeper first wakes (a re-arm is not recorded)."""
    out: Doc = json.loads(json.dumps(state))
    wake = now + timedelta(minutes=clamp(str(minutes)))
    out["keeper"] = {"last_run_at": st.iso(now), "next_wake_at": st.iso(wake)}
    return out


def clamp(text: str) -> int:
    """A dispatched `wake_after` as whole minutes from 1 to CAP_MINUTES; anything else is IDLE_MINUTES."""
    try:
        value = int(str(text).strip())
    except ValueError:
        return IDLE_MINUTES
    return max(1, min(CAP_MINUTES, value))


def _load(path: Path, default: Any) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return default


def _rows(path: Path | None) -> list[Doc]:
    """The JSON rows of a log file, skipping any line that is not a JSON object; [] without one."""
    if path is None:
        return []
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError:
        return []
    rows: list[Doc] = []
    for line in lines:
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if isinstance(row, dict):
            rows.append(row)
    return rows


def _last(path: Path | None, session: str, since: str) -> str:
    """The last cycle result in a kit/runner_log.jsonl copy (`last_result`); UNREAD when there is no file or the
    workflow marked its read failed, so an unreadable log never opens the free-lane path (D-4281)."""
    if path is None or not path.is_file():
        return UNREAD
    rows = _rows(path)
    return UNREAD if any(row.get("unreadable") for row in rows) else last_result(rows, session, since)


def _out(**fields: object) -> None:
    """key=value lines, ready for $GITHUB_OUTPUT; None prints as empty."""
    for key, value in fields.items():
        print(f"{key}={'' if value is None else value}")


def main(argv: list[str] | None = None, now: datetime | None = None) -> int:
    ap = argparse.ArgumentParser(prog="python3 -m runner.keeper")
    ap.add_argument("command", choices=("arm", "wake", "clamp"))
    ap.add_argument("value", nargs="?", default="")
    ap.add_argument("--state", type=Path)
    ap.add_argument("--lease", type=Path)
    ap.add_argument("--runs", type=Path)
    ap.add_argument("--jobs", type=Path, help="the live runs' jobs; none: no free-lane `go` (D-4281)")
    ap.add_argument("--run-id", default="")
    ap.add_argument("--log", type=Path, help="kit/runner_log.jsonl, for the arming cycle's own result")
    ap.add_argument("--session", default="")
    ap.add_argument("--since", default="9999")
    args = ap.parse_args(argv)
    if args.command == "clamp":
        print(clamp(args.value))
        return 0
    moment = now or datetime.now(timezone.utc)
    state = _load(args.state, {}) if args.state else {}
    lease = _load(args.lease, {}) if args.lease else {}
    state, lease = (state if isinstance(state, dict) else {}), (lease if isinstance(lease, dict) else {})
    cfg = refill_config()
    if args.command == "arm":
        minutes, why = wake_after(state, lease, moment, _last(args.log, args.session, args.since), cfg=cfg)
        _out(wake_after=minutes, why=why)
        return 0
    runs = _load(args.runs, None) if args.runs else None
    jobs = _load(args.jobs, None) if args.jobs else None
    action, minutes, why, lanes = decide(state, lease, runs if isinstance(runs, list) else None, args.run_id, moment,
                                         _last(args.log, "", ""), jobs if isinstance(jobs, list) else None, cfg)
    _out(action=action, wake_after=minutes, why=why, lanes=" ".join(lanes))
    return 0


if __name__ == "__main__":
    sys.exit(main())
