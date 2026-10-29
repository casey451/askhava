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
ledger. state.usage.source records which was used ("quota_cmd" or "ledger").

**Decision** (`decide`): claim when estimate <= budget * (1 - headroom) - used in both windows;
otherwise wait until the latest reset among the windows it does not fit; "defer" when it cannot fit
even an empty window (the runner then claims it anyway at the start of a window: waiting cannot
help, and a rejected run is RETRY_AFTER_RESET). A usage-limit rejection whose reset is still ahead
(state.usage.limit_hit) means wait until that reset whatever the ledger says.

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


def read_quota(command: str | None = None) -> dict[str, Any] | None:
    """Utilisation from PACING_QUOTA_CMD, or None when unset or unreadable (the ledger is used then)."""
    command = command if command is not None else os.environ.get("PACING_QUOTA_CMD", "")
    if not command:
        return None
    try:
        done = subprocess.run(shlex.split(command), capture_output=True, text=True, timeout=30, check=False)
        reading = json.loads(done.stdout)
    except (OSError, ValueError, subprocess.SubprocessError):
        return None
    ok = isinstance(reading, dict) and all(isinstance(reading.get(w), dict) and "utilization" in reading[w] for w in WINDOWS)
    return reading if ok else None


def standing(state: State, cfg: dict[str, Any], now: datetime, quota: dict[str, Any] | None) -> tuple[dict[str, dict[str, Any]], str]:
    """Per window: used, effective budget, left and reset time; plus the source used."""
    out: dict[str, dict[str, Any]] = {}
    ledger = windows(state, cfg, now)
    for w in WINDOWS:
        budget = float(cfg[f"budget_tokens_{w}"])
        if quota:
            used, reset = float(quota[w]["utilization"]) * budget, quota[w].get("resets_at")
        else:
            start = ledger[w]["start"]
            used = float(ledger[w]["tokens"])
            reset = iso(parse(start) + timedelta(hours=float(cfg["window_hours"][w]))) if start else None
        eff = budget * (1 - float(cfg["headroom"]))
        out[w] = {"used": int(used), "budget": int(eff), "left": int(eff - used), "resets_at": reset}
    return out, "quota_cmd" if quota else "ledger"


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


def assess(
    state: State, task: dict[str, Any], *, root: Path = ROOT, config: dict[str, Any] | None = None,
    now: datetime | None = None, quota: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """decide() with its working: estimate, bytes, basis, standing, source."""
    cfg, now = config or load_config(), now or datetime.now(timezone.utc)
    need, size, basis = estimate(task, state, cfg, root)
    stand, source = standing(state, cfg, now, quota)
    hit = ((state.get("usage") or {}).get("limit_hit") or {}).get("reset_at")
    decision = ("wait", hit) if hit and parse(hit) > now else _verdict(need, stand, now)
    return {"decision": decision, "estimate": need, "bytes": size, "basis": basis, "standing": stand, "source": source}


def _k(tokens: float) -> str:
    return f"{tokens / 1e6:.1f}M" if abs(tokens) >= 1e6 else f"{tokens / 1e3:.0f}k"


def line(tid: str, found: dict[str, Any]) -> str:
    """The PROGRESS.md line: 'T058 estimated 180k tokens; window has 1.2M left; claimed'."""
    stand, (verdict, detail) = found["standing"], found["decision"]
    left = f"five-hour window has {_k(stand['five_hour']['left'])} left, week {_k(stand['week']['left'])}"
    outcome = {"claim": "claimed", "wait": f"waiting until {detail}", "defer": f"claimed anyway ({detail})"}[verdict]
    return f"{tid} estimated {_k(found['estimate'])} tokens ({found['basis']}); {left} ({found['source']}); {outcome}"


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
