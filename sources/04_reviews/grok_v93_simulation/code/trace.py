"""Print an outside-world timeline for one job: python3 trace.py '<override dict>' SEED JOBNUM [BID]"""
import sys, os, ast, re
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sim import run
from engine import fmt_t
over = ast.literal_eval(sys.argv[1]); seed = int(sys.argv[2]); num = sys.argv[3]; bid = sys.argv[4] if len(sys.argv) > 4 else None
s = run(over, seed); w = s.world
tr = w.truth.get(int(num)) if num.isdigit() else None
phones = set()
if tr: phones.add(tr["ho"].phone); print("truth:", {k: tr[k] for k in ("cat", "jt") if k in tr}, "homeowner", tr["ho"].phone, tr["ho"].persona if hasattr(tr["ho"], "persona") else "")
if bid and bid in w.biz: phones.add(w.biz[bid].phone)
ev = []
for r in s.tw.sms.values():
    if f"#{num}" in r["body"] or r["to"] in phones and (not tr or abs(r["t"] - tr["t0"]) < 5 * 1440): ev.append((r["t"], "OUT", r["to"], r["status"], r["body"]))
for r in s.tw.inbound:
    if r["frm"] in phones and (not tr or abs(r["t"] - tr["t0"]) < 5 * 1440): ev.append((r["t"], "IN ", r["frm"], "", r["body"]))
for c in s.stripe.charges:
    if str(c["meta"].get("job")) == num: ev.append((c["t"], "CHG", c["bid"], c["status"], f"${c['amount']}"))
for c in s.stripe.refunds: ev.append((c["t"], "REF", c["cid"], "", f"${c['amount']}")) if any(x["id"] == c["cid"] and str(x["meta"].get("job")) == num for x in s.stripe.charges) else None
for c in s.tw.calls:
    if str(c.get("ctx", {}).get("intended_job")) == num: ev.append((c["t"], "CALL", c.get("frm"), c.get("routed_to"), str(c.get("ctx"))))
for e in sorted(ev, key=lambda x: x[0])[:60]: print(fmt_t(e[0]), *e[1:4], "|", e[4][:150])
