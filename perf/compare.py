"""Ci's performance report: lab figures against the stored baseline and D-3292's goals (T418).

`python3 perf/compare.py --bench <perf/bench.py --quick JSON> --vitals <web_vitals.spec.ts JSON>
[--baseline docs/audit/perf-baseline-ci.json] [--write-baseline <file>]` appends a Markdown table to
`$GITHUB_STEP_SUMMARY` (stdout when unset): each route's p95 and queries per request per user
count, each lab Core Web Vital, and the change from the baseline. It prints one `::warning::` line
for each p95 more than 25% above its baseline and each goal missed, and always exits 0: no measure
fails a build on time (D-3292). A missing or empty baseline compares nothing and says so; an
unreadable input gives one warning and its section reads "not available". The summary and the
warnings hold only figures, route and page names and goals (TD-OPS-43): a text value from an input
(a "not exposed" reason) is shown as "not measured", and a name outside NAME is left out.
`--write-baseline` writes the figures alone in the baseline's shape, for the first green `main` run.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
from pathlib import Path

LABEL = "lab, not field"
GOALS = {"lcp_ms": 2500.0, "inp_ms": 200.0, "cls": 0.1}
ASPIRATIONAL = {"lcp_ms": 1500.0, "inp_ms": 150.0}
VITAL_NAMES = {"lcp_ms": "LCP ms", "inp_ms": "INP ms (lab stand-in)", "cls": "CLS"}
P95_SLACK = 0.25
NAME = re.compile(r"^[a-z0-9_.%]{1,48}$")
BASELINE = Path(__file__).resolve().parent.parent / "docs/audit/perf-baseline-ci.json"


def figure(value: object) -> float | None:
    """A finite number, else None (a text reason, a bool or a missing value)."""
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        return None
    return float(value)


def show(value: float | None) -> str:
    return "not measured" if value is None else f"{value:g}"


def show_base(value: float | None) -> str:
    return "none" if value is None else f"{value:g}"


def change(value: float | None, base: float | None) -> str:
    if value is None or base is None or base <= 0:
        return "no baseline"
    return f"{(value - base) / base * 100:+.1f}%"


def above_slack(value: float | None, base: float | None) -> bool:
    """More than 25% above the baseline; exactly 25% is not (a tolerance absorbs float error)."""
    return value is not None and base is not None and base > 0 and value > base * (1 + P95_SLACK) * (1 + 1e-9)


def names(table: object) -> list[str]:
    return sorted(k for k in table if isinstance(k, str) and NAME.match(k)) if isinstance(table, dict) else []


def load(path: Path | None) -> dict:
    """The JSON object at `path`; raises OSError or ValueError when it is missing or not an object."""
    if path is None:
        raise ValueError("not given")
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError("not a JSON object")
    return data


def route_rows(routes: object, base: object, warnings: list[str]) -> list[str]:
    rows = []
    for route in names(routes):
        runs = routes[route]
        if not isinstance(runs, dict):
            rows.append(f"| {route} | - | not measured | none | no baseline | not measured |")
            continue
        for users in sorted((u for u in runs if isinstance(u, str) and u.isdigit()), key=int):
            entry = runs[users] if isinstance(runs[users], dict) else {}
            p95, queries = figure(entry.get("p95_ms")), figure(entry.get("mean_queries"))
            old = base.get(route) if isinstance(base, dict) else None
            old_entry = old.get(users) if isinstance(old, dict) else None
            old_p95 = figure(old_entry.get("p95_ms")) if isinstance(old_entry, dict) else None
            rows.append(f"| {route} | {users} | {show(p95)} | {show_base(old_p95)} | {change(p95, old_p95)} | {show(queries)} |")
            if above_slack(p95, old_p95):
                warnings.append(f"{route} at {users} users: p95 {show(p95)} ms is more than 25% above its baseline {show(old_p95)} ms")
            verdicts = entry.get("verdicts") if isinstance(entry.get("verdicts"), dict) else {}
            for goal in names(verdicts):
                if verdicts[goal] == "missed":
                    warnings.append(f"{route} at {users} users: goal {goal} missed")
    return rows


def vital_rows(pages: object, base: object, warnings: list[str]) -> list[str]:
    rows = []
    for page in names(pages):
        found = pages[page] if isinstance(pages[page], dict) else {}
        old = base.get(page) if isinstance(base, dict) and isinstance(base.get(page), dict) else {}
        for vital, goal in GOALS.items():
            value, old_value = figure(found.get(vital)), figure(old.get(vital))
            aim = show(ASPIRATIONAL.get(vital)) if vital in ASPIRATIONAL else "-"
            rows.append(f"| {page} | {VITAL_NAMES[vital]} ({LABEL}) | {show(value)} | {goal:g} | {aim} | {show_base(old_value)} | {change(value, old_value)} |")
            if value is not None and value > goal:
                warnings.append(f"{page}: {VITAL_NAMES[vital]} {show(value)} ({LABEL}) misses the goal {goal:g}")
    return rows


def read_input(path: Path | None, what: str, warnings: list[str]) -> dict | None:
    try:
        return load(path)
    except (OSError, ValueError):
        warnings.append(f"the {what} input is missing or not readable JSON; its section is not available")
        return None


def read_baseline(path: Path, warnings: list[str]) -> tuple[dict, dict, str]:
    """(routes, vitals, note): empty tables and a note when there is nothing to compare with."""
    if not path.exists():
        return {}, {}, "No stored baseline: nothing compared."
    data = read_input(path, "baseline", warnings) or {}
    routes = data.get("routes") if isinstance(data.get("routes"), dict) else {}
    vitals = data.get("vitals") if isinstance(data.get("vitals"), dict) else {}
    note = "" if routes or vitals else "The stored baseline is empty: nothing compared."
    return routes, vitals, note


def report(bench: dict | None, vitals: dict | None, baseline: tuple[dict, dict, str], warnings: list[str]) -> str:
    """The step summary's Markdown; adds a warning line for each slow route and each missed goal."""
    base_routes, base_vitals, note = baseline
    lines = [f"## Performance ({LABEL}; warns only, never fails a build)", ""]
    lines += [note, ""] if note else []
    lines += ["### Routes (perf/bench.py --quick)", ""]
    if bench is None:
        lines.append("not available")
    else:
        lines += ["| Route | Users | p95 ms | Baseline p95 ms | Change | Queries per request |", "|---|---|---|---|---|---|"]
        lines += route_rows(bench.get("routes"), base_routes, warnings)
    lines += ["", f"### Core Web Vitals ({LABEL})", ""]
    if vitals is None:
        lines.append("not available")
    else:
        lines += ["| Page | Vital | Figure | Goal | Aspirational | Baseline | Change |", "|---|---|---|---|---|---|---|"]
        lines += vital_rows(vitals.get("pages"), base_vitals, warnings)
    return "\n".join(lines) + "\n"


def baseline_record(bench: dict | None, vitals: dict | None) -> dict:
    """The figures alone, in the stored baseline's shape."""
    routes: dict = {}
    for route in names((bench or {}).get("routes")):
        runs = bench["routes"][route]
        for users in names(runs) if isinstance(runs, dict) else []:
            entry = runs[users] if isinstance(runs[users], dict) else {}
            kept = {k: figure(entry.get(k)) for k in ("p95_ms", "mean_queries") if figure(entry.get(k)) is not None}
            routes.setdefault(route, {})[users] = kept
    pages = (vitals or {}).get("pages")
    kept_vitals = {p: {v: figure(pages[p].get(v)) for v in GOALS if figure(pages[p].get(v)) is not None}
                   for p in names(pages) if isinstance(pages[p], dict)}
    return {"label": LABEL, "routes": routes, "vitals": kept_vitals}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--bench", type=Path, help="perf/bench.py --quick's JSON")
    parser.add_argument("--vitals", type=Path, help="tests/perf/web_vitals.spec.ts's JSON")
    parser.add_argument("--baseline", type=Path, default=BASELINE, help="the stored Ci baseline")
    parser.add_argument("--write-baseline", type=Path, help="also write these figures as a baseline here")
    args = parser.parse_args(argv)
    warnings: list[str] = []
    bench, vitals = read_input(args.bench, "benchmark", warnings), read_input(args.vitals, "web vitals", warnings)
    text = report(bench, vitals, read_baseline(args.baseline, warnings), warnings)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as out:
            out.write(text)
    else:
        print(text)
    for line in warnings:
        print(f"::warning title=Performance ({LABEL})::{line}")
    if args.write_baseline is not None:
        args.write_baseline.write_text(json.dumps(baseline_record(bench, vitals), indent=1, sort_keys=True) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:  # the report never fails a build (D-3292); the type alone, no input text
        print(f"::warning title=Performance ({LABEL})::the report stopped early ({type(error).__name__})")
        raise SystemExit(0) from None
