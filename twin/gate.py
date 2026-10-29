"""Twin gate: the hard-failing `Twin` check (D-107; docs/twin-warnings.md section 6).

Runs every gate scenario (names ending in `_fixed`) with seeds 1-5 in-process through the twin's
`build`, `run` and `check`, applies `twin/waivers.json` to every instance the checks report, runs
the negative control (`normal` seed 1 must report both matching checks) and prints a table per
scenario. Only an entry in the waiver file that names a decision in `docs/decisions.md` waives an
instance; nothing else does (no variable, no flag, no "known flaky" list).

Exit 0: no unwaived instance, the control fired, the waiver file is valid.
Exit 1: an unwaived instance, a class ceiling exceeded, or a negative control that did not fire.
Exit 2: PYTHONHASHSEED is not 0, a required check is missing from the twin, or a waiver-file error
        (malformed or unknown entry, unknown check key or evidence, decision ID not in the log,
        stale pinned entry).

Usage: PYTHONHASHSEED=0 python3 twin/gate.py [--seeds N] [--no-pinned] [--scenarios a,b] [--report PATH]
Standard library only. The license oracle check and its self-test are T007's (D-491).
"""

from __future__ import annotations

import argparse
import copy
import json
import os
import re
import sys
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
DECISIONS = HERE.parent / "docs" / "decisions.md"
GATE_SEEDS = 5  # pinned entries exist only for seeds 1-5 (section 6.1)
MATCHING = ("job_card_outside_license", "qualified_business_left_out")
NEGATIVE_CONTROL = ("normal", 1, MATCHING)  # a v9.1 scenario: its matching checks must fire (section 5)
CAUSES = frozenset({"C1", "C2a", "C2b", "C2c", "C2d", "C2e", "C3", "R-TRIAGE"})
PINNED_KEYS = frozenset({"id", "scenario", "seed", "check", "ref", "cause", "decision", "until"})
CLASS_KEYS = frozenset({"id", "checks", "evidence", "decision", "max_per_seed"})
# Frozen: the 57 checks of sources/01_spec/hhq_twin/README.md (its table; it says 56). Removing or renaming one exits 2.
REQUIRED_CHECKS = frozenset(
    [
        "call_after_homeowner_closed",
        "call_blocked_unlinked_staff_phone",
        "call_to_wrong_homeowner",
        "charge_record_lost_or_mismatched",
        "charge_while_request_not_live",
        "charge_without_contractor_confirm",
        "charge_without_homeowner_yes",
        "charged_after_confirm_window",
        "charged_but_not_introduced",
        "contractor_statement_mismatch",
        "details_released_to_unlicensed",
        "details_released_wrongly",
        "dispute_missing_from_ledger",
        "double_charge",
        "duplicate_ledger_rows",
        "duplicate_open_requests",
        "emergency_without_safety_message",
        "english_text_to_spanish_speaker",
        "first_text_missing_disclosures",
        "fraudulent_contractor_activated",
        "free_first_win_reused",
        "handyman_over_1000",
        "help_not_answered",
        "identifying_photo_in_job_card",
        "identifying_text_in_job_card",
        "illegal_status_change",
        "job_card_outside_license",
        "job_card_sent_for_emergency",
        "job_card_to_unlicensed_contractor",
        "ledger_refund_mismatch",
        "legit_contractor_stuck_in_review",
        "link_not_on_own_domain",
        "major_fee_on_small_job",
        "more_than_one_optout_confirmation",
        "natural_optout_ignored",
        "over_refund",
        "personal_details_to_non_zero_retention_ai",
        "photo_location_not_stripped",
        "pick_lost_to_failed_text",
        "pick_not_intended_by_homeowner",
        "pick_stuck",
        "promise_missed",
        "promotional_content_in_text",
        "qualified_business_left_out",
        "quotes_never_reached_homeowner",
        "rental_without_onsite_contact",
        "request_left_open_after_stop",
        "request_number_reused",
        "request_stuck_waiting",
        "review_request_without_done",
        "safety_message_late",
        "stripe_charge_missing_from_ledger",
        "text_after_stop",
        "text_in_quiet_hours",
        "text_without_consent",
        "texter_never_answered",
        "unreached_homeowner_no_refund",
    ]
)
DECLARED = re.compile(r"""(?:\bbad|violations_inline\.append)\(\(?["']([a-z0-9_]+)["']""")


class GateError(Exception):
    """A setup or waiver-file error: the gate exits 2."""


def declared_checks(twin_dir: Path) -> set[str]:
    """Check keys the twin's source declares (`bad("key", ...)` and the app's inline violations)."""
    text = "".join((twin_dir / name).read_text(encoding="utf-8") for name in ("twin.py", "app.py"))
    return set(DECLARED.findall(text))


def decision_ids(path: Path) -> set[str]:
    """Decision IDs logged in docs/decisions.md (`- **D-123** ...`)."""
    return set(re.findall(r"^- \*\*(D-\d+)\*\*", path.read_text(encoding="utf-8"), re.MULTILINE))


def _check_entry(entry: object, keys: frozenset[str], known: dict[str, set[str]]) -> str:
    """Validate the fields every waiver kind shares; returns the entry's ID."""
    if not isinstance(entry, dict) or set(entry) != keys:
        raise GateError(f"malformed waiver entry: {entry!r}")
    if entry["decision"] not in known["decisions"]:
        raise GateError(f"{entry['id']}: decision {entry['decision']} is not in docs/decisions.md")
    checks = entry["checks"] if "checks" in entry else [entry["check"]]
    unknown = [c for c in checks if c not in known["checks"]]
    if unknown or not checks:
        raise GateError(f"{entry['id']}: unknown check key {unknown}")
    return str(entry["id"])


def _check_pinned(entry: dict, known: dict[str, set[str]]) -> None:
    seed = entry["seed"]
    if entry["scenario"] not in known["scenarios"] or type(seed) is not int or not 1 <= seed <= GATE_SEEDS:
        raise GateError(f"{entry['id']}: unknown scenario or seed outside 1-{GATE_SEEDS}")
    if entry["cause"] not in CAUSES or not isinstance(entry["ref"], str):
        raise GateError(f"{entry['id']}: unknown cause {entry['cause']!r} or bad ref")


def _check_class(entry: dict, known: dict[str, set[str]]) -> None:
    if entry["evidence"] not in EVIDENCE:
        raise GateError(f"{entry['id']}: unknown evidence predicate {entry['evidence']!r}")
    ceilings = entry["max_per_seed"]
    if not isinstance(ceilings, dict) or any(
        s not in known["scenarios"] or type(n) is not int or n < 0 for s, n in ceilings.items()
    ):
        raise GateError(f"{entry['id']}: max_per_seed needs a non-negative integer per gate scenario")


def validate_waivers(data: object, known: dict[str, set[str]]) -> dict:
    """The waiver file, checked against the known decisions, check keys and scenarios (section 6.4)."""
    if not isinstance(data, dict) or set(data) != {"version", "class", "pinned"} or data["version"] != 1:
        raise GateError("waivers.json needs exactly version 1, class and pinned")
    ids = [_check_entry(e, CLASS_KEYS, known) for e in data["class"]]
    ids += [_check_entry(e, PINNED_KEYS, known) for e in data["pinned"]]
    for entry in data["class"]:
        _check_class(entry, known)
    for entry in data["pinned"]:
        _check_pinned(entry, known)
    duplicates = [i for i, n in Counter(ids).items() if n > 1]
    if duplicates:
        raise GateError(f"duplicate waiver IDs: {duplicates}")
    return data


def _homeowner_answered_wrong(sim: object, ref: str) -> bool:
    """D-106: the request's simulated homeowner recorded a wrong answer and the stored trade or job type
    is the option it sent. The twin records `wrong_answer` once T007 adds actor evidence (section 6.3)."""
    m = re.match(r"request (\S+)", ref)
    r = sim.app.requests.get(m.group(1)) if m else None  # type: ignore[attr-defined]
    sent = r["_sim_truth"].get("wrong_answer") if r else None
    return bool(sent) and (sent == r["trade"] or sent in r.get("job_types", []))


EVIDENCE = {"homeowner_answered_wrong": _homeowner_answered_wrong}


def run_scenario(twin: object, name: str, seed: int) -> list[dict]:
    """Every instance one scenario and seed reports, with the evidence predicates that hold for it."""
    base = json.loads((HERE / "twin_config.json").read_text(encoding="utf-8"))
    scenarios = json.loads((HERE / "twin_scenarios.json").read_text(encoding="utf-8"))
    cfg = twin.deep_update(copy.deepcopy(base), copy.deepcopy(scenarios[name]["overrides"]))  # type: ignore[attr-defined]
    sim = twin.build(cfg, seed)  # type: ignore[attr-defined]
    sim.run(cfg["days"])
    return [
        {
            "scenario": name,
            "seed": seed,
            "check": key,
            "ref": str(ref),
            "evidence": {e for e, f in EVIDENCE.items() if f(sim, str(ref))},
        }
        for key, ref in twin.check(sim)  # type: ignore[attr-defined]
    ]


def control_missing(twin: object, control: tuple = NEGATIVE_CONTROL) -> list[str]:
    """Checks the negative control should have reported but did not (section 5)."""
    name, seed, expected = control
    seen = {i["check"] for i in run_scenario(twin, name, seed)}
    return [key for key in expected if key not in seen]


def apply_waivers(instances: list[dict], waivers: dict, ran: set[tuple[str, int]], no_pinned: bool) -> dict:
    """Mark each instance waived (by entry ID) or not; list stale pinned entries and exceeded ceilings."""
    pool: dict[tuple, list[str]] = defaultdict(list)
    for e in [] if no_pinned else waivers["pinned"]:
        pool[(e["scenario"], e["seed"], e["check"], e["ref"])].append(e["id"])
    per_seed: Counter = Counter()
    for inst in instances:
        ids = pool.get((inst["scenario"], inst["seed"], inst["check"], inst["ref"]))
        inst["waived_by"] = ids.pop(0) if ids else None
        for entry in waivers["class"] if inst["waived_by"] is None else []:
            if inst["check"] in entry["checks"] and entry["evidence"] in inst["evidence"]:
                inst["waived_by"] = entry["id"]
                per_seed[(entry["id"], inst["scenario"], inst["seed"])] += 1
                break
    left = {k: ids for k, ids in pool.items() if ids and (k[0], k[1]) in ran}
    stale = sorted(i for ids in left.values() for i in ids)
    ceilings = {e["id"]: e["max_per_seed"] for e in waivers["class"]}
    over = [
        f"{cid} {sc} seed {sd}: {n} > {ceilings[cid].get(sc, 0)}"
        for (cid, sc, sd), n in sorted(per_seed.items())
        if n > ceilings[cid].get(sc, 0)
    ]
    return {"unwaived": [i for i in instances if i["waived_by"] is None], "stale": stale, "over_ceiling": over}


def verdict(result: dict, missing_control: list[str]) -> int:
    """Exit code from the waiver result and the negative control (docs/twin-warnings.md 6.3, item 5).
    A failing invariant outranks the stale entries it leaves behind (a moved random path), so a broken
    twin rule always exits 1 (section 6.5); stale entries alone exit 2."""
    if result["unwaived"] or result["over_ceiling"] or missing_control:
        return 1
    return 2 if result["stale"] else 0


def print_report(instances: list[dict], result: dict, missing_control: list[str]) -> None:
    """A table per scenario, then every unwaived instance, stale entry and exceeded ceiling."""
    print(f"{'scenario':<26}{'instances':>10}{'waived':>8}{'unwaived':>10}")
    for name in sorted({i["scenario"] for i in instances}):
        mine = [i for i in instances if i["scenario"] == name]
        open_ = sum(1 for i in mine if i["waived_by"] is None)
        print(f"{name:<26}{len(mine):>10}{len(mine) - open_:>8}{open_:>10}")
    for i in result["unwaived"]:
        print(f"UNWAIVED {i['scenario']} seed {i['seed']} {i['check']}: {i['ref']}")
    for entry_id in result["stale"]:
        print(f"STALE pinned waiver {entry_id}: its instance no longer occurs")
    for line in result["over_ceiling"]:
        print(f"CEILING exceeded {line}")
    for key in missing_control:
        print(f"NEGATIVE CONTROL did not fire: {key}")


def load_twin() -> object:
    sys.path.insert(0, str(HERE))
    import twin

    return twin


def setup(args: argparse.Namespace) -> tuple[list[str], dict]:
    """Checks that come before any run; each failure is a GateError (exit 2)."""
    if os.environ.get("PYTHONHASHSEED") != "0":
        raise GateError("PYTHONHASHSEED must be 0 (run: PYTHONHASHSEED=0 python3 twin/gate.py)")
    missing = sorted(REQUIRED_CHECKS - declared_checks(HERE))
    if missing:
        raise GateError(f"required checks missing from the twin: {missing}")
    all_fixed = [
        n for n in json.loads((HERE / "twin_scenarios.json").read_text(encoding="utf-8")) if n.endswith("_fixed")
    ]
    names = args.scenarios.split(",") if args.scenarios else all_fixed
    if set(names) - set(all_fixed):
        raise GateError(f"not gate scenarios (only *_fixed): {sorted(set(names) - set(all_fixed))}")
    known = {
        "decisions": decision_ids(DECISIONS),
        "checks": declared_checks(HERE),
        "scenarios": set(all_fixed),
    }
    waivers = validate_waivers(json.loads((HERE / "waivers.json").read_text(encoding="utf-8")), known)
    return names, waivers


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Twin gate (D-107)")
    parser.add_argument("--seeds", type=int, default=GATE_SEEDS)
    parser.add_argument("--no-pinned", action="store_true", help="ignore pinned waivers (stage 1 done check)")
    parser.add_argument("--scenarios", default="", help="comma-separated subset of the gate scenarios")
    parser.add_argument("--report", default=str(Path(tempfile.gettempdir()) / "twin-report.json"))
    args = parser.parse_args(argv)
    try:
        names, waivers = setup(args)
    except GateError as err:
        print(f"twin gate: {err}")
        return 2
    twin = load_twin()
    ran = {(n, s) for n in names for s in range(1, args.seeds + 1)}
    instances = [i for n, s in sorted(ran) for i in run_scenario(twin, n, s)]
    result = apply_waivers(instances, waivers, ran, args.no_pinned)
    missing_control = control_missing(twin)
    print_report(instances, result, missing_control)
    code = verdict(result, missing_control)
    for i in instances:
        i["evidence"] = sorted(i["evidence"])
    Path(args.report).write_text(json.dumps(dict(exit=code, instances=instances, **result), indent=1, default=sorted))
    print(
        f"twin gate: exit {code} ({len(instances)} instances, {len(result['unwaived'])} unwaived, report {args.report})"
    )
    return code


if __name__ == "__main__":
    sys.exit(main())
