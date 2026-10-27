"""Paired (same-seed) differences for the Part 13 comparisons; writes results/paired.json."""
import json, os, statistics as st
R = os.path.join(os.path.dirname(os.path.abspath(__file__)), "results")
def L(n): return {m["seed"]: m for m in json.load(open(os.path.join(R, f"raw_{n}.json")))}
PAIRS = [("L_weekdays", "L_base"), ("L_first90", "L_base"), ("L_rec4", "L_base"), ("L_rec_partial", "L_base"),
         ("S17c_weekday_window", "S01_normal"), ("S17d_roc_lists", "S01_normal"), ("S17e_insurance_req", "S01_normal"),
         ("S17f_rec_partial", "S01_normal"), ("S17g_roc_layout", "S01_normal"), ("S17b_approved_scope", "S01_normal"),
         ("S17a_all_fixes", "S01_normal"), ("S07_twilio_suspend", "S01_normal"), ("S16_alt2_s15", "S16_base_s15"),
         ("S16_alt5_s15", "S16_base_s15"), ("S16_alt1_s15", "S16_base_s15"), ("S16_first90_s15", "S16_base_s15")]
out = {}
for a, b in PAIRS:
    A, B = L(a), L(b); seeds = sorted(set(A) & set(B))
    res = {}
    for k in ("profit", "biz_kept", "casey_min_week", "picks_confirmed_share", "fakes_active", "broken_promises"):
        if k not in A[seeds[0]] or k not in B[seeds[0]]: continue
        d = [A[s][k] - B[s][k] for s in seeds]
        res[k] = {"mean_diff": st.mean(d), "se": st.stdev(d) / len(d) ** 0.5, "n": len(d)}
    out[f"{a} - {b}"] = res
    print(f"{a} - {b}: " + "; ".join(f"{k} {v['mean_diff']:+.2f}±{v['se']:.2f}" for k, v in res.items()))
json.dump(out, open(os.path.join(R, "paired.json"), "w"), indent=1)
