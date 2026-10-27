"""App part 4: business sign-up and identity chain, license batches, call routing, restore, Casey actions."""
import copy, re
from engine import DAY, HOUR, next_open, add_bmin, day
from app import AppBase, LIVE
from app_intake import IntakeMixin
from app_flow import FlowMixin

class BizMixin:
    def biz_signup(self, info):
        """info: name, phone, email, lic (or None), lic_kind ('ROC'|'PEST'|None), cats, after_hours, ... (web form)."""
        bid = info["bid"]
        b = {"id": bid, "name": info["name"], "phone": info["phone"], "email": info.get("email"), "lic": info.get("lic"),
             "lic_kind": info.get("lic_kind"), "classes": info.get("classes", ()), "cats": info["cats"], "active": False,
             "paused": None, "verified": False, "after_hours": info.get("after_hours", False), "skip_tenant": info.get("skip_tenant", False),
             "max_cards": info.get("max_cards"), "stripe_cust": None, "optout": False, "t_join": self.now(), "chain": [],
             "insured": info.get("insured", True), "staff": []}
        if self.cfg["insurance_required"] and not b["insured"]:
            self.audit("signup_rejected_insurance", bid=bid); return "rejected"
        self.db["biz"][bid] = b; self.db["phone_biz"][b["phone"]] = bid
        if b["lic_kind"] in ("ROC", "PEST"):
            st = self.sim.roc.check(self.now(), b["lic"])
            rec = self.sim.roc.records.get(b["lic"])
            if st == "not_found" or st == "inactive":
                b["chain"].append(("roc", st)); self.audit("signup_rejected_license", bid=bid, st=st); b["rejected"] = st; return "rejected"
            if st == "unknown" or rec is None:
                self.needs("approve_signup", prio=3, bid=bid, why="roc_unknown"); b["chain"].append(("roc", "unknown")); return "held"
            b["roc_phone"] = rec["phone"]; b["roc_email"] = rec["email"]
            cls = "/".join(rec["classes"]) if rec["classes"] else b["lic_kind"]
            self.sms(b["phone"], "lic_readback", f"Havasu Home Quotes: the state record for {b['lic']} shows {rec['holder']}, class {cls}, active. "
                     f"We'll send a code to the phone on that record.", aud="biz", important=False)
            return self.verify_step(bid, "sms")
        return self.verify_step(bid, "own_sms")
    def verify_step(self, bid, step):
        b = self.db["biz"][bid]; b["chain"].append((step, self.now()))
        if step in ("sms", "voice"):
            idx = self.sim.tw.verify_send(self.sim.eng.t, b["roc_phone"], "sms" if step == "sms" else "voice")
            b["vpend"] = (idx, step); self.timer(self.now() + 30, "verify_timeout", bid, step); return "verifying"
        if step == "own_sms":
            idx = self.sim.tw.verify_send(self.sim.eng.t, b["phone"], "sms")
            b["vpend"] = (idx, step); self.timer(self.now() + 30, "verify_timeout", bid, step); return "verifying"
        if step == "email":
            code = f"E{len(b['chain'])}{bid[-3:]}"
            b["email_code"] = code
            ok = self.sim.email.send(self.sim.eng.t, b.get("roc_email"), "Havasu Home Quotes sign-up code", f"Your code is {code}")
            self.timer(self.now() + 2 * DAY, "verify_timeout", bid, step); return "verifying"
        if step == "letter":
            code = f"L{bid[-4:]}"; b["letter_code"] = code
            self.sim.mail.send_letter(self.sim.eng.t, self.sim.world.biz_obj(bid).addr_owner(), code)
            self.timer(self.now() + 14 * DAY, "verify_timeout", bid, step); return "verifying"
        if step == "manual":
            self.needs("letter_manual", prio=3, bid=bid); return "manual"
    def t_verify_timeout(self, bid, step):
        b = self.db["biz"][bid]
        if b["verified"] or b.get("rejected"): return
        if b["chain"][-1][0] != step: return
        nxt = {"sms": "voice", "voice": "email", "email": "letter", "letter": "manual", "own_sms": "manual"}[step]
        self.verify_step(bid, nxt)
    def web_verify_code(self, bid, code, who):
        b = self.db["biz"].get(bid)
        if b is None or b["verified"]: return False
        ok = False
        if b.get("vpend"): ok = self.sim.tw.verify_check(self.sim.eng.t, b["vpend"][0], code, who)
        if not ok and code in (b.get("email_code"), b.get("letter_code")): ok = True
        if ok:
            b["verified_by"] = b["chain"][-1][0]; b["verified_who"] = who
            return self.city_check(bid)
        return False
    def city_check(self, bid):
        b = self.db["biz"][bid]
        if self.sim.world.city_list_has(b["name"]):
            return self.activate(bid)
        self.needs("city_mismatch", prio=3, bid=bid); return "held"
    def activate(self, bid):
        b = self.db["biz"][bid]
        if b["verified"]: return True
        if self.fix["casey_checklist"] and not b.get("casey_ok"):
            self.needs("approve_signup", prio=3, bid=bid, why="checklist"); return "held"
        b["verified"] = True; b["active"] = True; b["t_active"] = self.now()
        self.sim.world.biz_activated(bid)
        self.sms(b["phone"], "welcome", "You're live on Havasu Home Quotes. New jobs arrive by text. Reply STOP to stop.", aud="biz", important=False)
        return True
    def casey_decide(self, bid, approve):
        b = self.db["biz"][bid]
        if approve:
            b["casey_ok"] = True; b["verified"] = False
            return self.activate(bid)
        b["rejected"] = "casey"; return False
    def t_license_batch(self):
        now = self.now(); self.sim.roc.batch_tick(now)
        if self.cfg["roc_mode"] == "lists":
            for b in self.db["biz"].values():
                if b["active"] and b["lic_kind"] in ("ROC", "PEST") and not b["paused"]:
                    if not self.sim.roc.truth_active(b["lic"], now - 7 * DAY): self.pause_biz(b, "license_inactive")
        else:
            unknown = 0; n = 0
            for b in self.db["biz"].values():
                if not b["active"] or b["lic_kind"] not in ("ROC", "PEST"): continue
                n += 1
                st = self.sim.roc.check(now, b["lic"]); b["lic_checked"] = now
                if st == "inactive" and not b["paused"]: self.pause_biz(b, "license_inactive")
                elif st == "unknown":
                    unknown += 1
                    if b.get("unknown_since") is None: b["unknown_since"] = now
                    elif now - b["unknown_since"] > 24 * HOUR and not b["paused"]:
                        self.pause_biz(b, "license_unknown_48h")
                else:
                    b["unknown_since"] = None
                    if b["paused"] == "license_unknown_48h": b["paused"] = None
            if n and unknown / n > 0.5 and not self.db.get("scraper_alert"):
                self.db["scraper_alert"] = True; self.alert("scraper_broken", "high")
        self.timer(now + 12 * HOUR, "license_batch")
    # ---------------- calls (Part 5.2 masked line)
    def route_call(self, frm, ctx):
        bid = self.db["phone_biz"].get(frm)
        if bid is None:
            for b in self.db["biz"].values():
                if frm in b["staff"]: bid = b["id"]
        if bid is not None:
            jobs = [j for j in self.db["jobs"].values() if j["status"] == "Connected" and j["pick"]["bid"] == bid]
            if len(jobs) == 1: return {"to": jobs[0]["phone"], "job_num": jobs[0]["num"], "ivr": "direct"}
            if not jobs: return {"to": None, "job_num": None, "ivr": "no_job"}
            if self.fix["pin_when_multi_job"]:
                pin = ctx.get("pin")
                for j in jobs:
                    if j["pin"] == pin: return {"to": j["phone"], "job_num": j["num"], "ivr": "pin"}
                return {"to": None, "job_num": None, "ivr": "pin_fail"}
            j = max(jobs, key=lambda x: x["released"])
            return {"to": j["phone"], "job_num": j["num"], "ivr": "most_recent"}
        h = self.db["ho"].get(frm)
        if h is not None:
            for jn in reversed(h["jobs"]):
                j = self.db["jobs"][jn]
                if j["status"] == "Connected": return {"to": self.db["biz"][j["pick"]["bid"]]["phone"], "job_num": jn, "ivr": "ho_direct"}
            return {"to": None, "job_num": None, "ivr": "ho_no_job"}
        pin = ctx.get("pin")
        if pin:
            for j in self.db["jobs"].values():
                if j["status"] == "Connected" and j["pin"] == pin:
                    lk = f"LINK{j['num']}"
                    self.sms(self.db["biz"][j["pick"]["bid"]]["phone"], "link_offer", f"A call for #{j['num']} came from {frm}. Reply {lk} to link that phone.", job=j["num"], aud="biz", important=False)
                    self.db.setdefault("link_pending", {})[j["pick"]["bid"]] = frm
                    self.sim.world.link_offered(j["pick"]["bid"], frm)
                    return {"to": j["phone"], "job_num": j["num"], "ivr": "unknown_pin"}
        return {"to": None, "job_num": None, "ivr": "unknown_no_pin"}
    def link_staff(self, bid, phone):
        self.db["biz"][bid]["staff"].append(phone)
    def on_call_done(self, idx):
        c = self.sim.tw.calls[idx]
        if c["job_num"] is None: return
        d = "b2h" if c["ivr"] in ("direct", "pin", "most_recent", "unknown_pin") else "h2b"
        self.db["call_obs"].setdefault(c["job_num"], []).append({"t": c["t"], "dir": d, "answered": c["answered"], "dur": c["dur"]})
    # ---------------- restore (Part 5.6)
    def snapshot(self): return {"t": self.sim.eng.t, "db": copy.deepcopy(self.db)}
    def restore(self, snap, mode):
        now = self.sim.eng.t; t0 = snap["t"]
        self.db = copy.deepcopy(snap["db"]); self.reschedule_all()
        self.audit("restored", lost_from=t0, mode=mode)
        if mode != "spec": return
        evs = []
        for r in self.sim.tw.inbound:
            if t0 <= r["t"] < now: evs.append((r["t"], 0, "in", r))
        for sid, r in self.sim.tw.sms.items():
            if r["t_deliv"] and t0 <= r["t"] < now and r["status"] == "delivered": evs.append((r["t_deliv"] + 1, 1, "st", sid))
        for ch in self.sim.stripe.charges:
            if t0 <= ch["t"] < now and ch["status"] == "succeeded": evs.append((ch["t"] + 0.5, 2, "pay", ch["id"]))
        for w in self.sim.web_log:
            if t0 <= w[0] < now: evs.append((w[0], 3, "web", w))
        for i, c in enumerate(self.sim.tw.calls):
            if t0 <= c["t"] < now and c["job_num"] is not None: evs.append((c["t"], 4, "call", i))
        evs.sort(key=lambda e: (e[0], e[1]))
        self.replaying = True
        try:
            for t, _, k, x in evs:
                self._now = t
                if k == "in": self.on_inbound(x["frm"], x["body"])
                elif k == "st": self.on_sms_status(x, "delivered", None)
                elif k == "pay": self.on_stripe_event("payment_intent.succeeded", x)
                elif k == "web": self.sim.replay_web(x)
                elif k == "call": self.on_call_done(x)
        finally:
            self.replaying = False; self._now = None

class App(BizMixin, FlowMixin, IntakeMixin, AppBase):
    def start(self):
        self.timer(2 * HOUR, "license_batch")
        self.timer(2 * HOUR + 30, "reconcile")
        self.timer(7 * DAY + 9 * HOUR, "weekly")
