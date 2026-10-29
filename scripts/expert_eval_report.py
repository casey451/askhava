"""Aggregate intake test judgments into docs/catalog/expert/TEST-RESULTS.md.

Reads every docs/catalog/expert/eval_runs/<run>/<category>/judgement.yaml, recomputes the
per-variant numbers from the per-scenario scores (never trusting the judge's own summary),
applies the pass bar from docs/catalog/expert/judges/README.md, and writes the results table.

Usage: python3 scripts/expert_eval_report.py <run id> [--root REPO_ROOT] [--out PATH]
"""

from __future__ import annotations

import argparse
import statistics
import sys
from pathlib import Path
from typing import Any

import yaml

VARIANTS = ("playbook", "baseline_none", "baseline_human")
HIGH_VOLUME = {"plumbing_repair", "hvac_repair", "electrical", "pool_cleaning", "house_cleaning", "pest_control", "landscape_maintenance"}


def load(path: Path) -> Any:
    return yaml.safe_load(path.read_text(encoding="utf-8"))


def share(values: list[Any], ok: Any) -> float | None:
    kept = [v for v in values if v is not None]
    if not kept:
        return None
    return sum(1 for v in kept if ok(v)) / len(kept)


def variant_stats(scenarios: list[dict[str, Any]], variant: str) -> dict[str, Any]:
    quotable: list[int | None] = []
    questions: list[int] = []
    routing: list[bool | None] = []
    fire: list[bool | None] = []
    traps: list[bool | None] = []
    not_stated = 0
    for sc in scenarios:
        labels = sc.get("labels") or {}
        label = next((k for k, v in labels.items() if v == variant), None)
        score = (sc.get("scores") or {}).get(label) if label else None
        if not score:
            continue
        quotable.append(score.get("quotable"))
        if score.get("questions_asked") is not None:
            questions.append(int(score["questions_asked"]))
        routing.append(score.get("routing_correct"))
        fire.append(score.get("fire_dept_correct"))
        traps.append(score.get("trap_caught"))
        not_stated += len(score.get("facts_not_stated") or [])
    kept_q = [q for q in quotable if q is not None]
    return {
        "scenarios": len(quotable),
        "quotable_share": share(quotable, lambda q: q >= 2),
        "median_quotable": statistics.median(kept_q) if kept_q else None,
        "median_questions": statistics.median(questions) if questions else None,
        "routing_correct_share": share(routing, lambda r: r is True),
        "fire_dept_correct_share": share(fire, lambda f: f is True),
        "trap_caught_share": share(traps, lambda t: t is True),
        "facts_not_stated_count": not_stated,
    }


def passes(category: str, stats: dict[str, Any], compliance_count: int) -> tuple[bool, list[str]]:
    bar = 0.95 if category in HIGH_VOLUME else 0.90
    fails: list[str] = []
    if (stats["quotable_share"] or 0) < bar:
        fails.append(f"quotable {pct(stats['quotable_share'])} < {pct(bar)}")
    if stats["median_questions"] is not None and stats["median_questions"] > 5:
        fails.append(f"median questions {stats['median_questions']} > 5")
    if (stats["routing_correct_share"] or 0) < 1.0:
        fails.append(f"routing {pct(stats['routing_correct_share'])} < 100%")
    if stats["fire_dept_correct_share"] is not None and stats["fire_dept_correct_share"] < 1.0:
        fails.append(f"911 flag {pct(stats['fire_dept_correct_share'])} < 100%")
    if stats["trap_caught_share"] is not None and stats["trap_caught_share"] < 0.9:
        fails.append(f"traps caught {pct(stats['trap_caught_share'])} < 90%")
    if compliance_count:
        fails.append(f"{compliance_count} compliance violation(s)")
    return (not fails), fails


def pct(value: float | None) -> str:
    return "n/a" if value is None else f"{round(value * 100)}%"


def num(value: Any) -> str:
    return "n/a" if value is None else (str(int(value)) if float(value).is_integer() else f"{value:.1f}")


def beats(play: dict[str, Any], other: dict[str, Any], questions_matter: bool) -> str:
    pq, oq = play["quotable_share"] or 0, other["quotable_share"] or 0
    pm, om = play["median_questions"] or 0, other["median_questions"] or 0
    if pq > oq and (not questions_matter or pm <= om):
        return "yes"
    if pq >= oq and pm <= om:
        return "ties or better"
    return "no"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("run")
    parser.add_argument("--root", default=".")
    parser.add_argument("--out", default="docs/catalog/expert/TEST-RESULTS.md")
    args = parser.parse_args(argv)
    root = Path(args.root).resolve()
    run_dir = root / "docs/catalog/expert/eval_runs" / args.run
    rows: list[str] = []
    losses: list[str] = []
    violations: list[str] = []
    totals = {v: [] for v in VARIANTS}
    passed = 0
    categories = sorted(p.parent.name for p in run_dir.glob("*/judgement.yaml"))
    for category in categories:
        data = load(run_dir / category / "judgement.yaml")
        scenarios = data.get("scenarios") or []
        stats = {v: variant_stats(scenarios, v) for v in VARIANTS}
        comp = (data.get("compliance") or {}).get("violations") or []
        ok, fails = passes(category, stats["playbook"], len(comp))
        passed += ok
        p, n, h = stats["playbook"], stats["baseline_none"], stats["baseline_human"]
        for v in VARIANTS:
            totals[v].append(stats[v])
        rows.append(
            f"| {category} | {p['scenarios']} | {pct(p['quotable_share'])} | {num(p['median_questions'])} | {pct(p['routing_correct_share'])} | "
            f"{pct(p['fire_dept_correct_share'])} | {pct(p['trap_caught_share'])} | {len(comp)} | "
            f"{pct(n['quotable_share'])} / {num(n['median_questions'])} | {pct(h['quotable_share'])} / {num(h['median_questions'])} | "
            f"{beats(p, n, True)} / {beats(p, h, False)} | {'pass' if ok else 'FAIL: ' + '; '.join(fails)} |"
        )
        for item in data.get("where_playbook_lost_to_human") or []:
            losses.append(f"- {category}: {item}")
        for item in comp:
            violations.append(f"- {category} {item.get('scenario_id')}: {item.get('code')} at {item.get('where')}: \"{item.get('text')}\"")
    out = [
        f"# Intake test results: run `{args.run}`",
        "",
        "Generated by `scripts/expert_eval_report.py` from the judgments in `eval_runs/`; numbers are recomputed from per-scenario scores. Pass bar: `judges/README.md`. The homeowner in every scenario is played by a model from a script (fake data only); real-person validation is the hand-run test.",
        "",
        f"**Categories judged: {len(categories)}. Passing the bar: {passed}.**",
        "",
        "| Category | Scenarios | Quotable (playbook) | Median questions | Routing | 911 flag | Traps caught | Compliance violations | No-playbook baseline: quotable / questions | Human estimator: quotable / questions | Beats no-playbook / meets human | Result |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|",
        *rows,
        "",
        "## Where the playbook lost to the human estimator (fixes to propose)",
        "",
        *(losses or ["- none recorded"]),
        "",
        "## Compliance violations in the playbook run",
        "",
        *(violations or ["- none"]),
        "",
    ]
    if categories:
        agg = {v: {
            "quotable_share": statistics.mean([s["quotable_share"] for s in totals[v] if s["quotable_share"] is not None] or [0]),
            "median_questions": statistics.median([s["median_questions"] for s in totals[v] if s["median_questions"] is not None] or [0]),
        } for v in VARIANTS}
        out += [
            "## Across all categories",
            "",
            f"- Playbook: quotable {pct(agg['playbook']['quotable_share'])}, median questions {num(agg['playbook']['median_questions'])}",
            f"- No-playbook baseline: quotable {pct(agg['baseline_none']['quotable_share'])}, median questions {num(agg['baseline_none']['median_questions'])}",
            f"- Human estimator: quotable {pct(agg['baseline_human']['quotable_share'])}, median questions {num(agg['baseline_human']['median_questions'])}",
            "",
        ]
    (root / args.out).write_text("\n".join(out), encoding="utf-8")
    print(f"{len(categories)} categories, {passed} pass; wrote {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
