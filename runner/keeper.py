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
- `go`: dispatch an ordinary run, which goes through the gate, the lane pick, the backup job's
  concurrency group and the lease exactly as any other start.

The keeper holds no lock: the backup concurrency group and the lease stay the only locks on a cycle.
Standard library only; the workflow calls `python3 -m runner.keeper arm|wake|clamp`.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

from runner import state as st

CAP_MINUTES = 340  # 5 h 40 m: one sleep stays well under GitHub's 6-hour job limit
IDLE_MINUTES = 20  # nothing in flight and the pacer not waiting: look again soon
FLOOR_MINUTES = 10  # a computed sleep never ends before the run that armed it has finished
LEASE_MARGIN = 2  # minutes past a lease's expiry
RESET_MARGIN = 5  # minutes past the pacer's `waiting_until` (a usage window's reset)
WEEK_STOP = 0.97  # the week's reading at which the build stops for the reset (kit/pacing.json headroom 0.03)
NOTHING_CLAIMED = frozenset({"idle", "fix_main"})  # a cycle result after which a 20-minute look would find the same
QUIET = ("not_lease_holder", "renewed", "renew_refused", "usage_reading")  # log rows that say nothing of the cycle

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
    merged = dict(state, lease=lease_doc.get("lease"))
    merged["leases"] = lease_doc.get("leases") if isinstance(lease_doc.get("leases"), dict) else {}
    try:
        return st.live_leases(merged, now)
    except (KeyError, TypeError, ValueError):
        return {}


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


def wake_after(state: Doc, lease_doc: Doc, now: datetime, last: str = "") -> tuple[int | None, str]:
    """(minutes, why) for the next keeper, or (None, why) when none may run (D-1451).

    A task in flight: the soonest live lease's remaining time + LEASE_MARGIN; the pacer waiting: its
    `waiting_until` - now + RESET_MARGIN; both capped at CAP_MINUTES (a longer wait, the weekly reset,
    is reached by chained sleeps) and floored at FLOOR_MINUTES; work a cycle could act on: IDLE_MINUTES;
    nothing to claim: the soonest `retry_after` + RESET_MARGIN, else CAP_MINUTES, so an idle build does
    not start a Claude session every IDLE_MINUTES. `last`, the result the arming cycle itself reported,
    overrides the state's guess: after `idle` (nothing it could claim) the short sleep is never used."""
    why = blocker(state)
    if why:
        return None, why
    held = live_leases(state, lease_doc, now)
    if held:
        soonest = min(st.parse(str(lease["expires"])) for lease in held.values())
        return _minutes(soonest - now, LEASE_MARGIN), f"a task is in flight; its lease ends {st.iso(soonest)}"
    until = pacer_until(state, now)
    if until is not None:
        return _minutes(until - now, RESET_MARGIN), f"the pacer waits until {st.iso(until)}"
    reset = week_stop(state, now)
    if reset is not None:
        return _minutes(reset - now, RESET_MARGIN), f"the week stopped at its limit; it resets {st.iso(reset)}"
    if has_work(state, now) and last not in NOTHING_CLAIMED:
        return IDLE_MINUTES, "idle"
    retry = next_retry(state, now)
    if retry is not None:
        return _minutes(retry - now, RESET_MARGIN), f"nothing to claim before a retry at {st.iso(retry)}"
    held = " (build_hold is set: the T### tasks wait for Gate P)" if state.get("build_hold") else ""
    return CAP_MINUTES, f"nothing to claim{held}; events and the hourly tick still start runs"


def week_stop(state: Doc, now: datetime) -> datetime | None:
    """The week's `resets_at` while the plan's own reading (usage.session_reading) says the week is at or
    past WEEK_STOP, else None: the keeper then sleeps in chained steps to the reset (the A+ plan, item 5)."""
    usage = state.get("usage") if isinstance(state.get("usage"), dict) else {}
    reading = usage.get("session_reading") if isinstance(usage.get("session_reading"), dict) else {}
    week = reading.get("week") if isinstance(reading.get("week"), dict) else {}
    try:
        used, reset = float(week.get("utilization")), st.parse(str(week.get("resets_at")))
    except (TypeError, ValueError):
        return None
    return reset if used >= WEEK_STOP and reset > now else None


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
    """The result of this job's last cycle row in kit/runner_log.jsonl (its own session, from `since`),
    skipping rows that did nothing (a renewal, a usage reading, an `--after` holding no lease)."""
    mine = [str(r.get("result", "")) for r in rows if r.get("session") == session and str(r.get("at", "")) >= since]
    told = [r for r in mine if not r.startswith(QUIET) and not (" is " in r and len(r.split()) == 3)]
    return told[-1] if told else ""


def live_runs(runs: list[Doc], own_id: str) -> list[Doc]:
    """The workflow's runs other than this one that are not completed (queued, waiting, in progress)."""
    return [r for r in runs if str(r.get("id")) != str(own_id) and r.get("status") != "completed"]


def on_wake(state: Doc, lease_doc: Doc, runs: list[Doc] | None, own_id: str, now: datetime) -> tuple[str, int | None, str]:
    """("stop" | "rearm" | "go", minutes for a rearm, why) when a keeper wakes (D-1451)."""
    why = blocker(state)
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
    if args.command == "arm":
        minutes, why = wake_after(state, lease, moment, last_result(_rows(args.log), args.session, args.since))
        _out(wake_after=minutes, why=why)
        return 0
    runs = _load(args.runs, None) if args.runs else None
    action, minutes, why = on_wake(state, lease, runs if isinstance(runs, list) else None, args.run_id, moment)
    _out(action=action, wake_after=minutes, why=why)
    return 0


if __name__ == "__main__":
    sys.exit(main())
