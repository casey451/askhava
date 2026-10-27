"""Runs every scenario on seeds 1..50 (8 processes) and writes results/*.json. Pure standard library."""
import sys, os, json, time, statistics as st
from multiprocessing import Pool
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sim import run
from metrics import run_metrics
from scenarios import SCEN, FEE_STRUCTS, S50S, GROWTH

SEEDS = list(range(1, 51))
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "results")

def job(arg):
    name, over, seed, checks = arg
    t = time.time()
    try:
        sim = run(over, seed); m = run_metrics(sim, with_checks=checks)
    except Exception as e:
        import traceback
        return {"name": name, "seed": seed, "error": repr(e), "tb": traceback.format_exc()[-1500:]}
    m["name"] = name; m["seed"] = seed; m["secs"] = time.time() - t
    m.pop("join_rule_p", None)
    return m

def tasks():
    T = []
    for n, o in SCEN.items():
        if n == "S02_growth": continue
        for s in SEEDS: T.append((n, o, s, True))
    for f in FEE_STRUCTS:
        for s50 in S50S:
            for s in SEEDS: T.append((f"S16_{f}_s{int(s50*100)}", {"fees": f, "s50": s50}, s, False))
    for s in SEEDS: T.append(("S16_first90_s15", {"free_first_win": "first90"}, s, False))
    # long runs for Part 13 (365 days, 5/day, seeds 1-20)
    long = {"L_base": {}, "L_first90": {"free_first_win": "first90"}, "L_rec4": {"fees": "alt5"},
            "L_weekdays": {"quote_window": "weekdays"}, "L_rec_partial": {"recurring_partial_refund": True}}
    for n, o in long.items():
        for s in range(1, 21): T.append((n, dict(o, days=365), s, False))
    # demand sweep (120 days, flat rate, seeds 1-8), months 3-4 used as steady state
    for r in (2, 3, 5, 8, 12, 16, 25, 40, 60, 100):
        for s in range(1, 9): T.append((f"D_{r}", {"days": 120, "requests_per_day": r}, s, False))
    return T

def main():
    os.makedirs(OUT, exist_ok=True)
    t0 = time.time()
    T = [(("S02_growth", {"days": 730, "growth": GROWTH}, s, True)) for s in SEEDS] + tasks()
    res = {}
    with Pool(8) as p:
        for i, m in enumerate(p.imap_unordered(job, T, chunksize=1)):
            res.setdefault(m["name"], []).append(m)
            if i % 200 == 0: print(i, len(T), round(time.time() - t0), flush=True)
    for n, lst in res.items():
        lst.sort(key=lambda m: m["seed"])
        with open(os.path.join(OUT, f"raw_{n}.json"), "w") as f: json.dump(lst, f, default=str)
    errs = [(m["name"], m["seed"], m["error"]) for l in res.values() for m in l if "error" in m]
    print("done", len(T), "runs in", round(time.time() - t0), "s; errors:", len(errs), errs[:5])

if __name__ == "__main__":
    main()
