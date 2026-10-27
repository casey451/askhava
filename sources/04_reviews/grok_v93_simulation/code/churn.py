"""Part 9 churn by category: growth scenario (730 days) seeds 1-50, no checks. Writes results/churn.json."""
import sys, os, json
from multiprocessing import Pool
from collections import Counter
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sim import run
from scenarios import GROWTH
def one(seed):
    s = run({"days": 730, "growth": GROWTH}, seed); w = s.world
    act = Counter(); left = Counter(); nowin = Counter()
    for k, e in w.biz_events:
        if k == "activated" and not e["fake"]: act[w.biz[e["bid"]].group] += 1
        if k == "left": left[e["group"]] += 1; nowin[e["group"]] += (e["wins"] == 0)
    return act, left, nowin
if __name__ == "__main__":
    with Pool(8) as p: R = p.map(one, range(1, 51))
    A = Counter(); L = Counter(); N = Counter()
    for a, l, n in R: A.update(a); L.update(l); N.update(n)
    out = {g: {"activated_per_run": A[g] / 50, "left_per_run": L[g] / 50, "left_share": L[g] / max(1, A[g]), "left_with_0_wins_share": N[g] / max(1, L[g])} for g in A}
    json.dump(out, open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "results", "churn.json"), "w"), indent=1)
    for g, v in sorted(out.items(), key=lambda x: -x[1]["left_share"]): print(g, {k: round(x, 2) for k, x in v.items()})
