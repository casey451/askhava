"""Pacing: keep the build working continuously and pause only before the plan's usage limits (D-486).

Casey's rule: the runner claims work whenever the next task fits the usage windows, and waits only
when it would not, resuming when the window resets (BUILD_BRIEF.md:509: never pay per use).

**Estimate.** tokens ~= bytes / bytes_per_token * model multiplier * turn factor, where bytes are
the task's owned files that exist, its `reads` and its prompt (kit/prompts/T###.md); multipliers
(haiku 1.0, sonnet 1.3, opus 1.6) and the turn factor (3.0: read, write, test loops) are defaults
in kit/pacing.json. **Calibration:** once usage.by_task holds `calibrate_after` (3) DONE tasks
of one model with an estimate recorded, that model's tokens-per-byte becomes the median of
measured / estimated_bytes, replacing bytes_per_token, multiplier and turn factor together.

**Weighted tokens (an assumption, stated):** the plan's limits are not published in tokens. The
ledger counts input + output + cache writes at weight 1 and cache reads at 0.1 (kit/pacing.json
`weights`), because cache reads are far cheaper; the budgets are in the same unit. Worker and
reviewer transcripts both count.

**Ledger.** state.usage.windows = {"five_hour": {"start", "tokens"}, "week": {"start", "tokens"}};
a window starts at its first use and rolls over (start = now, tokens = 0) once its length passed.

**Real quota.** When PACING_QUOTA_CMD is set, it is run (no shell) and must print JSON
{"five_hour": {"utilization": 0..1, "resets_at": ISO}, "week": {...}}; that reading wins over the
ledger. Next comes the reading the build action itself reported at the end of its last session
(state.usage.session_reading, from its `rate_limit_event` stream messages, D-1155), used while it is
under SESSION_READING_HOURS old for the windows it holds. state.usage.source records which was used
("quota_cmd", "session_reading <at> (<windows>)" or "ledger", with the probe's reason when it failed).

**Decision** (`decide`): claim when estimate <= budget * (1 - headroom) - used in both windows;
otherwise wait until the latest reset among the windows it does not fit; "defer" when it cannot fit
even an empty window (the runner then claims it anyway at the start of a window: waiting cannot
help, and a rejected run is RETRY_AFTER_RESET). A usage-limit rejection whose reset is still ahead
(state.usage.limit_hit) means wait until that reset whatever the ledger says.

**Lanes** (D-620): with two lanes another attempt may be running at a claim; its usage reaches the
ledger only at its hand-in, so `used` also counts the estimate recorded at its claim for every other
IN_PROGRESS task (`in_flight_tokens`). With one lane that is always 0.

**Spend line** (D-1128, Casey's rule of October 31: the week's budget is to be spent by a few hours
before its reset, as accurately as possible, with no idle days after an early stop). When
kit/pacing.json has `spread` ({"anchor_at": ISO, "anchor_used_week": tokens,
"finish_hours_before_reset": hours}), the week window's spend is held to a straight line from the
anchor reading to the effective week budget at `resets_at` minus the margin: a claim that would put
`used + estimate` above the line now waits until the line reaches it (`spend_line`), never past the
reset; a task that fits the line claims as before. The line is for the anchor's week only: once the
window's start (its reset minus the week's hours) is later than the anchor, from the ledger's
roll-over or a quota reading, there is no line until a new anchor is set.

Usage: python3 scripts/pacing.py [--state PATH] [--task ID]
"""

from __future__ import annotations

import argparse
import json
import os
import shlex
import statistics
import subprocess
import sys
from collections.abc import Callable
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
CONFIG = ROOT / "kit" / "pacing.json"
WINDOWS = ("five_hour", "week")
DONE = ("DONE", "SKIPPED")
State = dict[str, Any]
Pacer = Callable[[State, dict[str, Any], datetime], tuple[str, "str | None", State]]


def iso(moment: datetime) -> str:
    return moment.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def parse(stamp: str) -> datetime:
    moment = datetime.fromisoformat(stamp.replace("Z", "+00:00"))
    return moment if moment.tzinfo else moment.replace(tzinfo=timezone.utc)


def load_config(path: Path = CONFIG) -> dict[str, Any]:
    found: dict[str, Any] = json.loads(path.read_text(encoding="utf-8"))
    return found


def alias(model: str) -> str:
    return next((a for a in ("haiku", "sonnet", "opus") if f"-{a}-" in f"-{model}-"), model)


def weighted(usage: dict[str, Any], cfg: dict[str, Any]) -> int:
    """Weighted tokens of one usage record (see the module docstring for the assumption)."""
    return int(sum(float(w) * int(usage.get(k, 0) or 0) for k, w in cfg["weights"].items()))


def _size(path: Path) -> int:
    if path.is_file():
        return path.stat().st_size
    if path.is_dir():
        return sum(p.stat().st_size for p in path.rglob("*") if p.is_file() and ".git" not in p.parts)
    return 0


def task_bytes(task: dict[str, Any], root: Path = ROOT) -> int:
    """Bytes of the owned files that exist, the reads and the prompt."""
    paths = {*task.get("owns", []), *task.get("reads", []), task.get("prompt") or f"kit/prompts/{task['id']}.md"}
    return sum(_size(root / p) for p in paths if p and not p.startswith("/") and ".." not in p.split("/"))


def calibrated(state: State, cfg: dict[str, Any]) -> dict[str, float]:
    """Model alias -> measured tokens per estimated byte (median), once enough DONE tasks exist."""
    status = {t["id"]: t.get("status") for t in state.get("tasks", [])}
    ratios: dict[str, list[float]] = {}
    for tid, entry in ((state.get("usage") or {}).get("by_task") or {}).items():
        size = int(entry.get("estimated_bytes", 0) or 0)
        if size > 0 and status.get(tid) == "DONE" and entry.get("last_model"):
            ratios.setdefault(alias(str(entry["last_model"])), []).append(weighted(entry, cfg) / size)
    return {m: statistics.median(r) for m, r in ratios.items() if len(r) >= int(cfg["calibrate_after"])}


def estimate(task: dict[str, Any], state: State, cfg: dict[str, Any], root: Path = ROOT) -> tuple[int, int, str]:
    """(estimated weighted tokens, bytes, 'calibrated' or 'default') for one attempt of the task."""
    size = max(task_bytes(task, root), 1)
    model = alias(str(task.get("model") or "sonnet"))
    factor = calibrated(state, cfg).get(model)
    if factor is not None:
        return int(size * factor), size, "calibrated"
    mult = float(cfg["model_multiplier"].get(model, max(cfg["model_multiplier"].values())))
    return int(size / float(cfg["bytes_per_token"]) * mult * float(cfg["turn_factor"])), size, "default"


def _rolled(window: dict[str, Any] | None, hours: float, now: datetime) -> dict[str, Any]:
    if not window or not window.get("start") or parse(window["start"]) + timedelta(hours=hours) <= now:
        return {"start": None, "tokens": 0}
    return dict(window)


def windows(state: State, cfg: dict[str, Any], now: datetime) -> dict[str, dict[str, Any]]:
    """The ledger windows rolled over to now."""
    held = (state.get("usage") or {}).get("windows") or {}
    return {w: _rolled(held.get(w), float(cfg["window_hours"][w]), now) for w in WINDOWS}


def record_usage(state: State, usage: dict[str, Any], now: datetime, cfg: dict[str, Any]) -> State:
    """Add an attempt's transcript usage (worker plus reviewer) to both windows."""
    spent = weighted(usage, cfg)
    rolled = windows(state, cfg, now)
    for window in rolled.values():
        window["start"] = window["start"] or iso(now)
        window["tokens"] = int(window["tokens"]) + spent
    out = json.loads(json.dumps(state))
    out.setdefault("usage", {})["windows"] = rolled
    return out


PROBE_NOTE: list[str] = []  # the last probe failure's stderr line, for the pacing row (D-1128)


def read_quota(command: str | None = None) -> dict[str, Any] | None:
    """Utilisation from PACING_QUOTA_CMD, or None when unset or unreadable (the ledger is used then).

    A probe that fails leaves its last stderr line in PROBE_NOTE, so the pacing row says why the
    ledger was used (D-1128); the command's output is never logged beyond that line."""
    command = command if command is not None else os.environ.get("PACING_QUOTA_CMD", "")
    PROBE_NOTE.clear()
    if not command:
        return None
    try:
        done = subprocess.run(shlex.split(command), capture_output=True, text=True, timeout=30, check=False)
        PROBE_NOTE[:] = [line for line in done.stderr.splitlines() if line.strip()][-1:]
        reading = json.loads(done.stdout)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        PROBE_NOTE[:] = PROBE_NOTE or [f"{type(error).__name__}: {' '.join(str(error).split())[:120]}"]
        return None
    ok = isinstance(reading, dict) and all(isinstance(reading.get(w), dict) and "utilization" in reading[w] for w in WINDOWS)
    return reading if ok else None


def in_flight_tokens(state: State, task_id: str = "") -> int:
    """Estimated weighted tokens of the attempts still running (D-620): every IN_PROGRESS task but
    `task_id`, at the estimate recorded when it was claimed (usage.by_task[tid].estimated_tokens). A
    running attempt's usage reaches the ledger only at its hand-in, so with two lanes the other lane's
    work is counted against the windows as if spent; a quota reading already holds part of it, so this
    errs on the side of waiting. With one lane nothing else is running at a claim, so it is 0."""
    by_task = (state.get("usage") or {}).get("by_task") or {}
    running = [t["id"] for t in state.get("tasks", []) if t.get("status") == "IN_PROGRESS" and t["id"] != task_id]
    return sum(int((by_task.get(tid) or {}).get("estimated_tokens", 0) or 0) for tid in running)


def standing(
    state: State, cfg: dict[str, Any], now: datetime, quota: dict[str, Any] | None, reserved: int = 0
) -> tuple[dict[str, dict[str, Any]], str]:
    """Per window: used (plus `reserved`, the attempts still running), effective budget, left and reset
    time; plus the source used."""
    out: dict[str, dict[str, Any]] = {}
    ledger = windows(state, cfg, now)
    reading = None if quota else session_reading(state, now)
    for w in WINDOWS:
        budget = float(cfg[f"budget_tokens_{w}"])
        if quota:
            used, reset = float(quota[w]["utilization"]) * budget, quota[w].get("resets_at")
        elif reading and isinstance(reading.get(w), dict):
            used, reset = float(reading[w]["utilization"]) * budget, reading[w].get("resets_at")
        else:
            start = ledger[w]["start"]
            used = float(ledger[w]["tokens"])
            reset = iso(parse(start) + timedelta(hours=float(cfg["window_hours"][w]))) if start else None
        used += reserved
        eff = budget * (1 - float(cfg["headroom"]))
        out[w] = {"used": int(used), "budget": int(eff), "left": int(eff - used), "resets_at": reset}
    if quota:
        return out, "quota_cmd"
    if reading:
        return out, "session_reading " + str(reading.get("at")) + (" (" + ", ".join(w for w in WINDOWS if isinstance(reading.get(w), dict)) + ")")
    return out, "ledger; " + PROBE_NOTE[0][:160] if PROBE_NOTE else "ledger"


SESSION_READING_HOURS = 6  # a usage reading the action reported this long ago still beats the ledger (D-1155)


def session_reading(state: State, now: datetime) -> dict[str, Any] | None:
    """The plan usage the build action itself reported at the end of a session (`rate_limit_event` in its
    execution file, recorded by `python3 -m runner --usage-reading`, D-1155): state.usage.session_reading
    = {"at": ISO, "week": {"utilization": 0..1, "resets_at": ISO}, "five_hour": {...} when reported}.
    None when absent, malformed, or older than SESSION_READING_HOURS; a window it lacks uses the ledger."""
    found = (state.get("usage") or {}).get("session_reading")
    if not isinstance(found, dict) or not isinstance(found.get("at"), str):
        return None
    try:
        age = now - parse(found["at"])
    except ValueError:
        return None
    if age < timedelta(0) or age > timedelta(hours=SESSION_READING_HOURS):
        return None
    good = {}
    for w in WINDOWS:
        window = found.get(w)
        if not isinstance(window, dict) or "utilization" not in window:
            continue
        reset = window.get("resets_at")
        try:
            if isinstance(reset, str) and parse(reset) <= now:
                continue  # a window already reset says nothing about the current one
        except ValueError:
            continue
        good[w] = window
    return {"at": found["at"], **good} if good else None


def _verdict(need: int, stand: dict[str, dict[str, Any]], now: datetime) -> tuple[str, str | None]:
    failing = [w for w in WINDOWS if need > stand[w]["left"]]
    waitable = [w for w in failing if stand[w]["used"] > 0]
    if waitable:
        return "wait", max(stand[w]["resets_at"] or iso(now) for w in waitable)
    if failing:
        return "defer", f"estimate {need} exceeds an empty {failing[0]} window ({stand[failing[0]]['budget']})"
    return "claim", None


def decide(
    state: State, task: dict[str, Any], *, root: Path = ROOT, config: dict[str, Any] | None = None,
    now: datetime | None = None, quota: dict[str, Any] | None = None,
) -> tuple[str, str | None]:
    """("claim", None) | ("wait", until ISO) | ("defer", reason) for claiming `task` now."""
    return assess(state, task, root=root, config=config, now=now, quota=quota)["decision"]


def spend_line(cfg: dict[str, Any], week: dict[str, Any], need: int, now: datetime) -> str | None:
    """When `used + need` sits above the week's straight spend line (D-1128), the ISO time the line
    reaches it (capped at the reset); None when the claim fits the line or no line applies."""
    spread = cfg.get("spread")
    reset = week.get("resets_at")
    if not spread or not reset:
        return None
    start, end = parse(str(spread["anchor_at"])), parse(str(reset)) - timedelta(hours=float(spread["finish_hours_before_reset"]))
    window_start = parse(str(reset)) - timedelta(hours=float(cfg["window_hours"]["week"]))
    if window_start > start:
        return None  # the anchor's window has rolled over: the line was for that week only
    if now < start or now >= end or end <= start:
        return None
    base, top = float(spread["anchor_used_week"]), float(week["budget"])
    if top <= base:
        return None
    target = float(week["used"]) + need
    released = base + (top - base) * (now - start).total_seconds() / (end - start).total_seconds()
    if target <= released:
        return None
    when = start + (end - start) * min((target - base) / (top - base), 1.0)
    return iso(max(when, now + timedelta(minutes=1)))


def assess(
    state: State, task: dict[str, Any], *, root: Path = ROOT, config: dict[str, Any] | None = None,
    now: datetime | None = None, quota: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """decide() with its working: estimate, bytes, basis, standing, source."""
    cfg, now = config or load_config(), now or datetime.now(timezone.utc)
    need, size, basis = estimate(task, state, cfg, root)
    reserved = in_flight_tokens(state, str(task.get("id", "")))
    stand, source = standing(state, cfg, now, quota, reserved)
    hit = ((state.get("usage") or {}).get("limit_hit") or {}).get("reset_at")
    decision = ("wait", hit) if hit and parse(hit) > now else _verdict(need, stand, now)
    line_at = spend_line(cfg, stand["week"], need, now) if decision[0] == "claim" else None
    if line_at:
        decision = ("wait", line_at)
    found = {"decision": decision, "estimate": need, "bytes": size, "basis": basis, "standing": stand, "source": source}
    found |= {"spend_line": line_at} if line_at else {}
    return found | ({"in_flight": reserved} if reserved else {})


def _k(tokens: float) -> str:
    return f"{tokens / 1e6:.1f}M" if abs(tokens) >= 1e6 else f"{tokens / 1e3:.0f}k"


def line(tid: str, found: dict[str, Any]) -> str:
    """The PROGRESS.md line: 'T058 estimated 180k tokens; window has 1.2M left; claimed'."""
    stand, (verdict, detail) = found["standing"], found["decision"]
    left = f"five-hour window has {_k(stand['five_hour']['left'])} left, week {_k(stand['week']['left'])}"
    outcome = {"claim": "claimed", "wait": f"waiting until {detail}", "defer": f"claimed anyway ({detail})"}[verdict]
    if found.get("spend_line"):
        outcome = f"on the spend line (D-1128), {outcome}"
    running = f" after {_k(found['in_flight'])} still running in another lane" if found.get("in_flight") else ""
    return f"{tid} estimated {_k(found['estimate'])} tokens ({found['basis']}); {left}{running} ({found['source']}); {outcome}"


def projected_finish(state: State, graph: list[dict[str, Any]], cfg: dict[str, Any], now: datetime, root: Path = ROOT) -> tuple[int, str]:
    """(remaining estimated tokens, projected finish ISO) at the budgets' sustained rate."""
    status = {t["id"]: t.get("status") for t in state.get("tasks", [])}
    remaining = sum(estimate(t, state, cfg, root)[0] for t in graph if t.get("type") != "gate" and status.get(t["id"]) not in DONE)
    rates = [float(cfg[f"budget_tokens_{w}"]) * (1 - float(cfg["headroom"])) / float(cfg["window_hours"][w]) for w in WINDOWS]
    return remaining, iso(now + timedelta(hours=remaining / min(rates)))


def remember(state: State, tid: str, found: dict[str, Any], now: datetime, finish: tuple[int, str]) -> State:
    """Write the decision into state: usage.source, the task's estimate, and pacing (for PROGRESS and STATUS)."""
    out = json.loads(json.dumps(state))
    usage = out.setdefault("usage", {})
    usage["source"] = found["source"]
    entry = usage.setdefault("by_task", {}).setdefault(tid, {})
    entry.update(estimated_bytes=found["bytes"], estimated_tokens=found["estimate"])
    verdict, detail = found["decision"]
    out["pacing"] = {
        "last": line(tid, found), "at": iso(now), "decision": verdict, "waiting_until": detail if verdict == "wait" else None,
        "standing": found["standing"], "remaining_estimate": finish[0], "projected_finish": finish[1],
    }
    return out


def status_line(state: State) -> str:
    """The STATUS.md pacing line."""
    pace = state.get("pacing") or {}
    stand = pace.get("standing") or {}
    if not stand:
        return "Pacing: no reading yet."
    parts = [f"{w.replace('_', '-')}: {_k(s['used'])} used, {_k(s['left'])} left, resets {s['resets_at'] or 'on first use'}" for w, s in stand.items()]
    tail = f"; waiting until {pace['waiting_until']}" if pace.get("waiting_until") else ""
    return f"Pacing ({(state.get('usage') or {}).get('source', 'ledger')}): " + "; ".join(parts) + f"; projected finish {pace.get('projected_finish')}{tail}."


def make_pacer(graph: list[dict[str, Any]], root: Path = ROOT, config: dict[str, Any] | None = None) -> Pacer:
    """The hook the runner calls before claiming (runner.cycle.Config.pacer)."""
    cfg = config or load_config(root / "kit" / "pacing.json")

    def pace(state: State, task: dict[str, Any], now: datetime) -> tuple[str, str | None, State]:
        command = os.environ.get("PACING_QUOTA_CMD") or cfg.get("quota_cmd_default", "")
        found = assess(state, task, root=root, config=cfg, now=now, quota=read_quota(command))
        verdict, detail = found["decision"]
        return verdict, detail, remember(state, task["id"], found, now, projected_finish(state, graph, cfg, now, root))

    return pace


def usage_recorder(root: Path = ROOT, config: dict[str, Any] | None = None) -> Callable[[State, dict[str, Any], datetime], State]:
    """The hook the runner calls with each attempt's usage (runner.cycle.Config.on_usage)."""
    cfg = config or load_config(root / "kit" / "pacing.json")
    return lambda state, usage, now: record_usage(state, usage, now, cfg)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--state", type=Path, default=ROOT / "kit" / "state.json")
    ap.add_argument("--task")
    args = ap.parse_args(argv)
    graph = json.loads((ROOT / "kit" / "tasks.json").read_text(encoding="utf-8"))["tasks"]
    state = json.loads(args.state.read_text(encoding="utf-8")) if args.state.exists() else {"tasks": []}
    now, cfg = datetime.now(timezone.utc), load_config()
    for task in [t for t in graph if t["id"] == args.task] if args.task else []:
        print(line(task["id"], assess(state, task, config=cfg, now=now, quota=read_quota())))
    remaining, finish = projected_finish(state, graph, cfg, now)
    print(f"remaining estimate {_k(remaining)} tokens; projected finish {finish} (budgets: {cfg['budget_source']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
