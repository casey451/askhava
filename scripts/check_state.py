"""Validate kit/state.json against kit/state_schema.json before a runner cycle (D-486, docs/governance.md section 8).

The runner runs this at start (`python3 -m runner`); a state that fails is never written over: the
cycle stops and reports. The schema is a small subset of JSON Schema (type, enum, required,
properties, items, additionalProperties as a schema, pattern, minLength) checked here with the
standard library only. Unknown keys are allowed, because the runner keeps every key it does not
know (D-121). On top of the schema: task IDs are unique and a lease names a task in the list; with
the lane map `leases` (D-514), each lane lease names a task, no task holds two lanes, at most `lanes`
live leases sit outside the "fix" lane, and `lease` mirrors lane "1".

Usage: python3 scripts/check_state.py [--state PATH] [--schema PATH]
Exit status 0 when valid; otherwise each problem is printed as `path: problem` and the status is 1.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

SCHEMA = Path(__file__).resolve().parent.parent / "kit" / "state_schema.json"
TYPES: dict[str, tuple[type, ...]] = {
    "object": (dict,), "array": (list,), "string": (str,), "integer": (int,),
    "number": (int, float), "boolean": (bool,), "null": (type(None),),
}


def _type_ok(value: Any, wanted: str | list[str]) -> bool:
    names = [wanted] if isinstance(wanted, str) else wanted
    for name in names:
        if isinstance(value, bool) and name in ("integer", "number"):
            continue
        if isinstance(value, TYPES[name]):
            return True
    return False


def _scalar(value: Any, schema: dict[str, Any], where: str) -> list[str]:
    problems = []
    if "enum" in schema and value not in schema["enum"]:
        problems.append(f"{where}: {value!r} is not one of {schema['enum']}")
    if isinstance(value, str):
        if len(value) < schema.get("minLength", 0):
            problems.append(f"{where}: empty")
        if "pattern" in schema and not re.search(schema["pattern"], value):
            problems.append(f"{where}: {value!r} does not match {schema['pattern']}")
    return problems


def _object(value: dict[str, Any], schema: dict[str, Any], where: str) -> list[str]:
    problems = [f"{where}: missing {key}" for key in schema.get("required", []) if key not in value]
    props = schema.get("properties", {})
    extra = schema.get("additionalProperties")
    for key, item in value.items():
        if key in props:
            problems += validate(item, props[key], f"{where}.{key}")
        elif isinstance(extra, dict):
            problems += validate(item, extra, f"{where}.{key}")
    return problems


def validate(value: Any, schema: dict[str, Any], where: str = "state") -> list[str]:
    """Problems with `value` against the schema subset; empty when it fits."""
    if "type" in schema and not _type_ok(value, schema["type"]):
        return [f"{where}: expected {schema['type']}, got {type(value).__name__}"]
    problems = _scalar(value, schema, where)
    if isinstance(value, dict):
        problems += _object(value, schema, where)
    elif isinstance(value, list) and "items" in schema:
        for index, item in enumerate(value):
            problems += validate(item, schema["items"], f"{where}[{index}]")
    return problems


def _cross_checks(state: dict[str, Any]) -> list[str]:
    ids = [t.get("id") for t in state.get("tasks", []) if isinstance(t, dict)]
    problems = [f"state.tasks: {tid} appears more than once" for tid in sorted({i for i in ids if ids.count(i) > 1})]
    lease = state.get("lease")
    if isinstance(lease, dict) and lease.get("task") not in ids:
        problems.append(f"state.lease: names {lease.get('task')}, which is not in tasks")
    return problems


def _expires(lease: dict[str, Any]) -> datetime:
    return datetime.fromisoformat(str(lease["expires"]).replace("Z", "+00:00"))


def _lane_checks(state: dict[str, Any], lanes: int, now: datetime) -> list[str]:
    """The lane map (D-514): each lease names a real task, no task holds two lanes, at most `lanes`
    live leases outside the "fix" lane, and `lease` equal to lane "1" without its "lane" field."""
    leases = state.get("leases")
    if not isinstance(leases, dict):
        return []
    ids = {t.get("id") for t in state.get("tasks", []) if isinstance(t, dict)}
    held = {str(lane): lease for lane, lease in leases.items() if isinstance(lease, dict)}
    problems = [f"state.leases.{lane}: names {lease.get('task')}, which is not in tasks" for lane, lease in held.items() if lease.get("task") not in ids]
    tasks = [lease.get("task") for lease in held.values()]
    problems += [f"state.leases: {tid} holds more than one lane" for tid in sorted({str(t) for t in tasks if tasks.count(t) > 1})]
    live = [lane for lane, lease in held.items() if lane != "fix" and _expires(lease) > now]
    if len(live) > lanes:
        problems.append(f"state.leases: {len(live)} live leases ({', '.join(sorted(live))}) but lanes is {lanes}")
    one = {k: v for k, v in (held.get("1") or {}).items() if k != "lane"} or None
    if state.get("lease") != one:
        problems.append("state.lease: differs from state.leases.1 (lane 1 must mirror the top-level lease)")
    return problems


def check(state: Any, schema: dict[str, Any] | None = None, lanes: int | None = None, now: datetime | None = None) -> list[str]:
    """Every problem with a kit/state.json value (schema plus cross checks).

    `lanes` is the lane count (kit/runner.json, D-514); when not given, the state's own `lanes`, else 1.
    `now` decides which leases are live (default: the current UTC time)."""
    schema = schema or json.loads(SCHEMA.read_text(encoding="utf-8"))
    problems = validate(state, schema)
    if problems or not isinstance(state, dict):
        return problems
    count = lanes if lanes is not None else state.get("lanes", 1)
    return _cross_checks(state) + _lane_checks(state, int(count), now or datetime.now(timezone.utc))


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--state", type=Path, default=Path("kit/state.json"))
    ap.add_argument("--schema", type=Path, default=SCHEMA)
    args = ap.parse_args(argv)
    try:
        state = json.loads(args.state.read_text(encoding="utf-8"))
    except (OSError, ValueError) as err:
        print(f"{args.state}: unreadable ({err})")
        return 1
    problems = check(state, json.loads(args.schema.read_text(encoding="utf-8")))
    for line in problems:
        print(line)
    print(f"{args.state}: {'valid' if not problems else f'{len(problems)} problem(s)'}")
    return 0 if not problems else 1


if __name__ == "__main__":
    sys.exit(main())
