"""Measured (simulated) outcomes of one run, from outside-world events: Stripe, Twilio/vendor bills, world truth, Casey's log."""
from collections import defaultdict
from engine import DAY, HOUR
from checks import Checker

MONTH = 30 * DAY
VENDOR_KEYS = ["twilio_sms", "twilio_voice", "twilio_verify", "claude", "email", "letters", "render"]

def run_metrics(sim, with_checks=True):
    w = sim.world; st = sim.stripe; cfg = sim.cfg; end = cfg["days"] * DAY
    nmon = max(1, int(round(cfg["days"] / 30)))
    mon = lambda t: min(nmon - 1, int(t // MONTH))
    M = [defaultdict(float) for _ in range(nmon)]
    for tr in w.reqs: M[mon(tr["t0"])]["requests"] += 1
    for p in w.picks_intended: M[mon(p["t"])]["picks"] += 1
    ok = [c for c in st.charges if c["status"] == "succeeded"]
    for c in ok:
        M[mon(c["t"])]["revenue"] += c["amount"]; M[mon(c["t"])]["paid_intros"] += 1
    for r in st.refunds:
        M[mon(r["t"])]["refunds"] += r["amount"]; M[mon(r["t"])]["n_refunds"] += 1
    for d in w.details_to:
        if d.get("replay"): continue
        paid = any(c["meta"]["job"] == d["num"] and c["bid"] == d["bid"] for c in ok)
        if not paid: M[mon(d["t"])]["free_wins"] += 1
    for dd, items in sim.costs.by_day.items():
        m = min(nmon - 1, int(dd // 30))
        for k, v in items.items(): M[m][k] += v
    for m in range(nmon):
        x = M[m]
        x["vendor"] = sum(x[k] for k in VENDOR_KEYS)
        x["fixed"] = cfg["fixed_month"]
        x["net"] = x["revenue"] - x["refunds"] - x["stripe_fees"] - x["dispute_fees"] - x["vendor"] - x["fixed"]
        x["cost_per_request"] = (x["vendor"] + x["fixed"] + x["stripe_fees"] + x["dispute_fees"]) / max(1, x["requests"])
        x["avg_fee"] = x["revenue"] / max(1, x["paid_intros"])
    # cash curve: start cash + reserve held by Stripe
    cash = cfg["start_cash"]; low = cash; low_day = 0
    days = defaultdict(float)
    for c in ok: days[int(c["t"] // DAY) + 2] += c["amount"] - c["fee"]     # A: 2-day payout
    for r in st.refunds: days[int(r["t"] // DAY)] -= r["amount"]
    for dd, items in sim.costs.by_day.items():
        days[dd] -= sum(v for k, v in items.items() if k in VENDOR_KEYS or k == "dispute_fees")
    for d in range(cfg["days"] + 1):
        if d % 30 == 0: days[d] -= cfg["fixed_month"]
    for d in range(cfg["days"] + 1):
        cash += days[d]
        if cash < low: low = cash; low_day = d
    # businesses
    ev = defaultdict(list)
    for k, e in w.biz_events: ev[k].append(e)
    real_act = {e["bid"] for e in ev["activated"] if not e["fake"]}
    left = {e["bid"] for e in ev["left"]}
    picked = ev["picked"]
    conf = [e for e in picked if e.get("confirm")]
    jobs_q = [tr for tr in w.reqs if tr.get("cards_to")]
    nq = [sum(1 for q in tr.get("quotes", {}).values() if q.get("app_result") == "ok") for tr in jobs_q]
    jobs_with_q = [tr for tr, n in zip(jobs_q, nq) if n]
    picked_jobs = sum(1 for tr in jobs_with_q if tr.get("picked_by_ho"))
    cm = defaultdict(float); cm_backup = defaultdict(float)
    weeks = cfg["days"] / 7
    for t, task, m, who, mode in w.casey.minutes:
        (cm if who == "casey" else cm_backup)[task] += m
    away = w.casey.away
    on_w = weeks - ((away[1] - away[0]) / (7 * DAY) if away else 0)
    tot = {k: sum(x[k] for x in M) for k in set().union(*[set(x) for x in M])}
    out = {
        "months": [dict(x) for x in M], "total": tot,
        "profit": tot.get("net", 0.0), "low_cash": low, "low_cash_day": low_day,
        "biz_joined": len(real_act), "biz_kept": len(real_act - left), "biz_left": len(left & real_act),
        "fakes_active": sum(1 for e in ev["activated"] if e["fake"]),
        "picks_confirmed_share": len(conf) / max(1, len(picked)),
        "quotes_per_job": sum(nq) / max(1, len(nq)), "ho_pick_rate": picked_jobs / max(1, len(jobs_with_q)),
        "casey_min_week": sum(cm.values()) / max(0.1, on_w), "casey_tasks_week": {k: v / max(0.1, on_w) for k, v in cm.items()},
        "backup_min_week_away": sum(cm_backup.values()) / max(0.1, (away[1] - away[0]) / (7 * DAY)) if away else sum(cm_backup.values()) / weeks,
        "backup_tasks": dict(cm_backup),
        "casey_backlog_end": sum(1 for n in sim.app.db["needs_you"] if n["done"] is None),
        "fee_avoided": sum(f["avoided"] for f in w.fee_avoided), "n_gamed": len(w.fee_avoided),
        "gamed_caught": 0, "join_rule_p": dict(w.join_rule_p),
        "vendor_calls": {"sms": len(sim.tw.sms), "sms_failed": sum(1 for r in sim.tw.sms.values() if r["status"] != "delivered"),
                         "verify": len(sim.tw.verify), "calls": len(sim.tw.calls), "claude": len(sim.claude.log),
                         "claude_errors": getattr(sim.claude, "errors", 0), "emails": len(sim.email.log), "letters": len(sim.mail.log),
                         "charges": len(st.charges), "charges_failed": sum(1 for c in st.charges if c["status"] != "succeeded"),
                         "disputes": len(st.disputes), "refunds": len(st.refunds),
                         "roc_checks": getattr(sim.roc, "n_checks", 0)},
        "restores": sim.restores,
    }
    # gaming caught: a PROBLEM reply on the gamed job or a shade flag for that business
    flagged = {b for b, x in sim.app.db["biz"].items() if x.get("shade_flagged")}
    for f in w.fee_avoided:
        tr = w.truth.get(f["num"])
        if f["bid"] in flagged or (tr and tr.get("checkin_replied") in ("PROBLEM", "PROBLEMA")): out["gamed_caught"] += 1
    if with_checks:
        ck = Checker(sim); res = ck.run()
        out["checks"] = res
        out["broken_promises"] = sum(res[k]["fails"] for k in res if k[:2] in ("H1", "H3", "H4", "H6"))
        out["h4_silent"] = getattr(ck, "h4_silent", 0)
    return out
