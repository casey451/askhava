"""`python3 -m twin chaos --baseline twin/chaos_baseline.json`: the chaos run through the app's rules (T074, D-491).

Runs the fault scenarios (CHAOS: vendor outages and lost webhooks, races, a database restore) with seeds 1-5
through the `rules_bridge` binary (--bridge, default target/debug/rules_bridge; build it with
`SQLX_OFFLINE=true cargo build -p askhava-server --bin rules_bridge --locked`) and collects every problem the
twin's checks report, before any waiver. Exit 0 when each is in the baseline, 1 on any problem not in it (each
printed), 2 when the binary or the baseline is missing or PYTHONHASHSEED is not 0. Baseline problems that no
longer occur are listed but do not fail. `--write` records the run as the new baseline. T126 runs it.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CHAOS = ("vendor_chaos_fixed", "races_fixed", "db_restore_fixed")
SEEDS = 5
ABOUT = (
    "Written by `python3 -m twin chaos --write` through the app's rules (T074, D-491): every problem the twin's "
    "checks report for the chaos scenarios, seeds 1-5, before waivers. A new problem fails the chaos run."
)


def load_gate() -> object:
    """twin/gate.py, with `twin` meaning twin/twin.py rather than this package."""
    sys.path.insert(0, str(HERE))
    sys.modules.pop("twin", None)
    import gate  # noqa: PLC0415  # twin/gate.py, found through the path set above

    return gate


def problems(gate: object) -> list[str]:
    """`scenario | seed | check | ref` for every instance of every chaos run."""
    twin = gate.load_twin()  # type: ignore[attr-defined]
    found = {
        f"{i['scenario']} | {i['seed']} | {i['check']} | {i['ref']}"
        for name in CHAOS
        for seed in range(1, SEEDS + 1)
        for i in gate.run_scenario(twin, name, seed)  # type: ignore[attr-defined]
    }
    return sorted(found)


def chaos(args: argparse.Namespace) -> int:
    if os.environ.get("PYTHONHASHSEED") != "0":
        print("twin chaos: PYTHONHASHSEED must be 0")
        return 2
    gate = load_gate()
    try:
        gate.use_bridge(args.bridge or "target/debug/rules_bridge")  # type: ignore[attr-defined]
    except gate.GateError as err:  # type: ignore[attr-defined]
        print(f"twin chaos: {err}")
        return 2
    found = problems(gate)
    path = Path(args.baseline)
    if args.write:
        data = {"_about": ABOUT, "scenarios": list(CHAOS), "seeds": SEEDS, "problems": found}
        path.write_text(json.dumps(data, indent=1) + "\n", encoding="utf-8")
        print(f"twin chaos: wrote {len(found)} problems to {path}")
        return 0
    if not path.is_file():
        print(f"twin chaos: no baseline at {path}")
        return 2
    known = set(json.loads(path.read_text(encoding="utf-8"))["problems"])
    new = [p for p in found if p not in known]
    for p in new:
        print(f"NEW {p}")
    for p in sorted(known - set(found)):
        print(f"GONE {p}")
    print(f"twin chaos: {len(found)} problems, {len(new)} not in the baseline")
    return 1 if new else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python3 -m twin", description="Twin tools (T074)")
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("chaos", help="the chaos scenarios through the app's rules, against a baseline")
    run.add_argument("--baseline", default=str(HERE / "chaos_baseline.json"))
    run.add_argument("--bridge", default="", help="the rules_bridge binary (default target/debug/rules_bridge)")
    run.add_argument("--write", action="store_true", help="record this run as the baseline")
    return chaos(parser.parse_args(argv))


if __name__ == "__main__":
    sys.exit(main())
