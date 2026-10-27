"""Aggregates results/raw_*.json into results/summary.json (all numbers Measured, simulated)."""
import json, os, glob, statistics as st
R = os.path.join(os.path.dirname(os.path.abspath(__file__)), "results")
def load(n): return json.load(open(os.path.join(R, f"raw_{n}.json")))
def mean(xs): xs = list(xs); return sum(xs) / len(xs) if xs else 0.0
def pct(xs, p): xs = sorted(xs); return xs[min(len(xs) - 1, int(p * len(xs)))] if xs else 0
RATIO = ("cost_per_request", "avg_fee")
def norm_last(L):
    """metrics puts days beyond the last full 30-day bin into the last bin (730 days -> month 24 has 40 days); scale it to 30 days."""
    for m in L:
        if m.get("_normed"): continue
        m["_normed"] = True; nm = len(m["months"]); days = m.get("days") or None
        extra = m.get("_days", None)
    return L
def scen_summary(L, days=None):
    if days and days > 30 * len(L[0]["months"]) - 0.5:
        f = 30.0 / (days - 30 * (len(L[0]["months"]) - 1))
        for m in L:
            if m.get("_normed"): continue
            m["_normed"] = True; x = m["months"][-1]
            for k in list(x):
                if k not in RATIO: x[k] = x[k] * f
    s = {k: mean(m[k] for m in L) for k in ("profit", "low_cash", "biz_joined", "biz_kept", "fakes_active", "picks_confirmed_share",
          "quotes_per_job", "ho_pick_rate", "casey_min_week", "backup_min_week_away", "fee_avoided", "n_gamed", "gamed_caught", "casey_backlog_end")}
    s["profit_p10"] = pct([m["profit"] for m in L], .1); s["profit_p90"] = pct([m["profit"] for m in L], .9)
    s["low_cash_min"] = min(m["low_cash"] for m in L); s["low_cash_seed"] = min(L, key=lambda m: m["low_cash"])["seed"]
    tot = {}
    for m in L:
        for k, v in m["total"].items(): tot[k] = tot.get(k, 0) + v / len(L)
    s["total"] = tot
    if "checks" in L[0]:
        s["broken_promises"] = mean(m["broken_promises"] for m in L)
        s["refunds_n"] = tot.get("n_refunds", 0)
    nm = len(L[0]["months"]); s["months"] = []
    for i in range(nm):
        s["months"].append({k: mean(m["months"][i].get(k, 0) for m in L) for k in L[0]["months"][i]})
    tasks = {}
    for m in L:
        for k, v in m["casey_tasks_week"].items(): tasks[k] = tasks.get(k, 0) + v / len(L)
    s["casey_tasks"] = tasks
    bt = {}
    for m in L:
        for k, v in m["backup_tasks"].items(): bt[k] = bt.get(k, 0) + v / len(L)
    s["backup_tasks"] = bt
    vc = {}
    for m in L:
        for k, v in m["vendor_calls"].items(): vc[k] = vc.get(k, 0) + v / len(L)
    s["vendor_calls"] = vc
    return s
def check_summary(L):
    out = {}
    for k in L[0]["checks"]:
        opps = sum(int(m["checks"][k]["opps"]) for m in L); fails = [int(m["checks"][k]["fails"]) for m in L]
        ex = next(((m["seed"], m["checks"][k]["example"]) for m in L if int(m["checks"][k]["fails"]) and m["checks"][k]["example"]), None)
        out[k] = {"opps": opps, "fails_total": sum(fails), "fails_per_run": sum(fails) / len(L), "runs_failing": sum(1 for f in fails if f), "example": ex}
    return out
def main():
    names = sorted(os.path.basename(p)[4:-5] for p in glob.glob(os.path.join(R, "raw_*.json")))
    S = {"scen": {}, "checks": {}, "fees": {}, "long": {}, "demand": {}}
    for n in names:
        L = load(n)
        if n.startswith("S16_"): S["fees"][n] = scen_summary(L); continue
        if n.startswith("L_"): S["long"][n] = scen_summary(L, 365); continue
        if n.startswith("D_"):
            r = int(n[2:]); ss = scen_summary(L)
            steady = mean([ss["months"][2]["net"], ss["months"][3]["net"]])
            S["demand"][r] = {"steady_month_net": steady, "annual": steady * 365 / 30, "monthly": steady * 30.4 / 30, "biz_kept": ss["biz_kept"]}
            continue
        S["scen"][n] = scen_summary(L, 730 if n == "S02_growth" else None)
        if "checks" in L[0]: S["checks"][n] = check_summary(L)
    # growth: month reaching $20k/yr and $50k/yr pace (monthly net >= 20000/12, 50000/12), mean over seeds
    G = load("S02_growth"); scen_summary(G, 730)
    gm = S["scen"]["S02_growth"]["months"]
    def first(th):
        for i, x in enumerate(gm):
            if x["net"] >= th: return i + 1
        return None
    S["growth_month"] = {"20k_yr": first(20000 / 12), "50k_yr": first(50000 / 12), "20k_mo": first(20000), "50k_mo": first(50000)}
    per_seed = {}
    for th, key in ((20000 / 12, "20k_yr"), (50000 / 12, "50k_yr")):
        ms = []
        for m in G:
            ms.append(next((i + 1 for i, x in enumerate(m["months"]) if x["net"] >= th), None))
        per_seed[key] = ms
    S["growth_month_per_seed"] = per_seed
    # demand interpolation
    D = sorted((int(r), v["annual"]) for r, v in S["demand"].items())
    def need(target):
        for (r0, a0), (r1, a1) in zip(D, D[1:]):
            if a0 < target <= a1: return r0 + (target - a0) * (r1 - r0) / (a1 - a0)
        if D[0][1] >= target: return D[0][0]
        (r0, a0), (r1, a1) = D[-2], D[-1]
        return ("extrapolated", r1 + (target - a1) * (r1 - r0) / (a1 - a0))
    S["need"] = {"20k_yr": need(20000), "50k_yr": need(50000), "20k_mo": need(240000), "50k_mo": need(600000)}
    s1, s7 = S["scen"]["S01_normal"], S["scen"]["S07_twilio_suspend"]
    S["suspension"] = {"profit_loss": s1["profit"] - s7["profit"],
                       "casey_extra_min_week": s7["casey_min_week"] - s1["casey_min_week"],
                       "casey_extra_hours_total": (s7["casey_min_week"] - s1["casey_min_week"]) * 60 / 7 / 60}
    json.dump(S, open(os.path.join(R, "summary.json"), "w"), indent=1, default=str)
    return S
if __name__ == "__main__":
    S = main()
    for n, s in S["scen"].items():
        print(f"{n:22s} profit {s['profit']:8.0f} [{s['profit_p10']:.0f},{s['profit_p90']:.0f}] BP {s.get('broken_promises',0):5.1f} ref {s['total'].get('n_refunds',0):5.1f}/${s['total'].get('refunds',0):5.0f} casey {s['casey_min_week']:5.0f} bk {s['backup_min_week_away']:4.0f} low {s['low_cash']:6.0f}/{s['low_cash_min']:.0f} conf {s['picks_confirmed_share']:.2f} qpj {s['quotes_per_job']:.2f} pick {s['ho_pick_rate']:.2f} biz {s['biz_joined']:.0f}/{s['biz_kept']:.0f} fakes {s['fakes_active']:.2f}")
    for n, s in S["fees"].items():
        print(f"{n:22s} profit {s['profit']:8.0f} conf {s['picks_confirmed_share']:.3f} biz {s['biz_joined']:.1f}/{s['biz_kept']:.1f} qpj {s['quotes_per_job']:.2f} pick {s['ho_pick_rate']:.3f} avgfee {s['total'].get('revenue',0)/max(1,s['total'].get('paid_intros',1)):.0f} gamed {s['fee_avoided']:.0f}/{s['n_gamed']:.1f} caught {s['gamed_caught']:.1f}")
    for n, s in S["long"].items():
        print(f"{n:22s} profit {s['profit']:8.0f} conf {s['picks_confirmed_share']:.3f} biz {s['biz_joined']:.1f}/{s['biz_kept']:.1f} qpj {s['quotes_per_job']:.2f} pick {s['ho_pick_rate']:.3f} casey {s['casey_min_week']:.0f} refunds {s['total'].get('refunds',0):.0f}")
    for r, v in sorted(S["demand"].items(), key=lambda x: int(x[0])): print("demand", r, {k: round(x) for k, x in v.items()})
    print("need", S["need"]); print("growth", S["growth_month"]); print("susp", S["suspension"])
    print([ (i+1, round(x["requests"]), round(x["net"])) for i, x in enumerate(S["scen"]["S02_growth"]["months"])])
