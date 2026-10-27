"""Fake vendors. Each keeps its own outside-world log that the checks read.
No real service is contacted. All behaviour simulated with seeded RNGs."""
import random
from collections import defaultdict
from engine import DAY, HOUR, segments, in_quiet

CARRIER_STOP = {"STOP","STOPALL","UNSUBSCRIBE","CANCEL","END","QUIT","REVOKE","OPTOUT"}   # Twilio default opt-out words
CARRIER_START = {"START","UNSTOP"}

class Costs:
    def __init__(self): self.items = defaultdict(float); self.by_day = defaultdict(lambda: defaultdict(float))
    def add(self, t, k, v):
        self.items[k] += v; self.by_day[int(t // DAY)][k] += v

class Twilio:
    def __init__(self, sim):
        self.sim = sim; c = sim.cfg["tw"]; self.c = c
        self.rng = random.Random(sim.seed * 101 + 1)
        self.sms = {}            # sid -> record (outside-world truth)
        self.keys = {}           # idempotency-ish message key -> sid (what the app asked to send)
        self.inbound = []
        self.calls = []
        self.verify = []
        self.carrier_optout = set()
        self.n = 0
        self.suspended = (None, None)
        sd = c.get("suspend_day")
        if sd is not None: self.suspended = (sd * DAY, (sd + c["suspend_days"]) * DAY)
    def is_suspended(self, t):
        a, b = self.suspended
        return a is not None and a <= t < b
    def send(self, t, to, body, key, mms=0, meta=None):
        self.n += 1; sid = f"SM{self.n:07d}"
        segs = segments(body)
        rec = {"sid": sid, "t": t, "to": to, "body": body, "segs": segs, "mms": mms, "key": key, "meta": meta or {},
               "status": None, "t_deliv": None, "err": None}
        self.sms[sid] = rec; self.keys.setdefault(key, sid)
        sim = self.sim
        if self.is_suspended(t):
            rec["status"] = "failed"; rec["err"] = 30007
            sim.eng.at(t + 0.5, sim.app.on_sms_status, sid, "failed", 30007); return sid
        if to in self.carrier_optout:
            rec["status"] = "failed"; rec["err"] = 21610
            sim.eng.at(t + 0.1, sim.app.on_sms_status, sid, "failed", 21610); return sid
        cost = (self.c["mms"] if mms else self.c["seg"] * segs) + self.c["carrier"] * (1 if mms else segs)
        sim.costs.add(t, "twilio_sms", cost)
        person = sim.world.phone_owner(to)
        if person is not None and getattr(person, "landline", False) and not person.mobile_for(to):
            rec["status"] = "undelivered"; rec["err"] = 30006
            sim.eng.at(t + 1, sim.app.on_sms_status, sid, "undelivered", 30006); return sid
        if person is not None and person.dead_number(to):
            rec["status"] = "undelivered"; rec["err"] = 30005
            sim.eng.at(t + 1, sim.app.on_sms_status, sid, "undelivered", 30005); return sid
        r = self.rng.random()
        ch = sim.cfg["chaos"]
        if r < self.c["fail"] * ch:
            rec["status"] = "failed"; rec["err"] = 30003
            sim.eng.at(t + 1, sim.app.on_sms_status, sid, "failed", 30003); return sid
        lat = self.rng.uniform(0.05, 0.6)
        if self.rng.random() < self.c["delay"] * ch: lat = self.rng.uniform(5, 120)
        td = t + lat
        rec["status"] = "delivered"; rec["t_deliv"] = td
        if person is not None: sim.eng.at(td, person.receive, to, sid)
        r2 = self.rng.random()
        if r2 < self.c["lost_status"] * ch:
            pass
        else:
            sim.eng.at(td + self.rng.uniform(0.1, 2), sim.app.on_sms_status, sid, "delivered", None)
            if r2 < (self.c["lost_status"] + self.c["dup_status"]) * ch:
                sim.eng.at(td + self.rng.uniform(1, 30), sim.app.on_sms_status, sid, "delivered", None)
        return sid
    def query(self, sid):
        r = self.sms.get(sid)
        return None if r is None else r["status"]
    def receive_inbound(self, t, frm, body):
        """Carrier layer first: handles STOP/START words exactly like Twilio's default opt-out."""
        w = body.strip().upper().rstrip(".!")
        rec = {"t": t, "frm": frm, "body": body}
        self.inbound.append(rec)
        sim = self.sim
        if w in CARRIER_STOP:
            self.carrier_optout.add(frm)
            # Twilio Advanced Opt-Out reply (text configured by the app; one message, not billed to app logic)
            self.n += 1; sid = f"SM{self.n:07d}"
            body2 = sim.app.optout_confirm_text(frm)
            self.sms[sid] = {"sid": sid, "t": t + 0.1, "to": frm, "body": body2, "segs": segments(body2), "mms": 0,
                             "key": f"carrier-stop-{sid}", "meta": {"kind": "carrier_optout"}, "status": "delivered",
                             "t_deliv": t + 0.3, "err": None}
            sim.costs.add(t, "twilio_sms", (self.c["seg"] + self.c["carrier"]) * segments(body2))
            p = sim.world.phone_owner(frm)
            if p: sim.eng.at(t + 0.3, p.receive, frm, sid)
        elif w in CARRIER_START:
            self.carrier_optout.discard(frm)
        sim.eng.at(t + 0.05, sim.app.on_inbound, frm, body)
    # ---- voice ----
    def call(self, t, frm, to_number, ctx):
        """frm dials the business line. App decides the route; Twilio connects; callee may answer."""
        sim = self.sim
        sim.costs.add(t, "twilio_voice", self.c["call_in"] * 3)
        route = sim.app.route_call(frm, ctx)
        rec = {"t": t, "frm": frm, "to_line": to_number, "routed_to": route.get("to"), "job_num": route.get("job_num"),
               "ivr": route.get("ivr"), "answered": False, "human": False, "dur": 0.0, "ctx": ctx}
        self.calls.append(rec)
        if route.get("to"):
            callee = sim.world.phone_owner(route["to"])
            ans, dur = (False, 0.0)
            if callee is not None: ans, dur = callee.answer_call(t, frm, rec)
            rec["answered"] = ans; rec["human"] = ans; rec["dur"] = dur
            sim.costs.add(t, "twilio_voice", (self.c["call_in"] + self.c["call_out"]) * max(1, dur))
        sim.eng.at(t + max(rec["dur"], 0.5), sim.app.on_call_done, len(self.calls) - 1)
        return rec
    def outbound_call(self, t, to, purpose):
        """App-originated voice (verify by voice); returns answered."""
        sim = self.sim
        p = sim.world.phone_owner(to)
        ans = p is not None and not p.dead_number(to) and p.answers_robot(to)
        self.calls.append({"t": t, "frm": "APP", "to_line": to, "routed_to": to, "job_num": None, "ivr": purpose,
                           "answered": ans, "human": ans, "dur": 1.0 if ans else 0.3, "ctx": {"purpose": purpose}})
        sim.costs.add(t, "twilio_voice", self.c["call_out"] * 1.0)
        return ans
    def verify_send(self, t, phone, channel):
        sim = self.sim
        code = f"{self.rng.randint(0, 999999):06d}"
        sim.costs.add(t, "twilio_verify", self.c["verify_ok"] + (self.c["verify_sms"] if channel == "sms" else self.c["verify_voice"]))
        p = sim.world.phone_owner(phone)
        rec = {"t": t, "phone": phone, "channel": channel, "code": code, "delivered": False, "approved_t": None, "approved_by": None}
        self.verify.append(rec)
        if p is None or p.dead_number(phone): return len(self.verify) - 1
        if channel == "sms":
            if getattr(p, "landline", False) and not p.mobile_for(phone): return len(self.verify) - 1
            if self.is_suspended(t): return len(self.verify) - 1    # A: Verify shares the sender pool
            rec["delivered"] = True
        else:
            rec["delivered"] = p.answers_robot(phone)
        if rec["delivered"]: sim.eng.at(t + 0.5, p.got_code, phone, len(self.verify) - 1)
        return len(self.verify) - 1
    def verify_check(self, t, idx, code, who):
        rec = self.verify[idx]
        if code == rec["code"] and rec["approved_t"] is None and t - rec["t"] <= 10:
            rec["approved_t"] = t; rec["approved_by"] = who; return True
        return False

class Stripe:
    def __init__(self, sim):
        self.sim = sim; self.c = sim.cfg["stripe"]
        self.rng = random.Random(sim.seed * 103 + 2)
        self.charges = []; self.by_key = {}; self.refunds = []; self.disputes = []; self.events = []
        self.outages = list(self.c["outages"])
        days = sim.cfg["days"]
        n_out = int(self.c["random_outage_per_60d"] * days / 60 + self.rng.random())
        for _ in range(n_out):
            s = self.rng.uniform(1, days - 1) * DAY
            self.outages.append((s, s + self.rng.uniform(20, 300)))
        if sim.cfg.get("stripe_outage"):
            a, h = sim.cfg["stripe_outage"]; self.outages.append((a * DAY + 10 * HOUR, a * DAY + 10 * HOUR + h * 60))
    def down(self, t): return any(a <= t < b for a, b in self.outages)
    def fee(self, amount, ach):
        if ach: return min(self.c["ach_cap"], amount * self.c["ach_pct"])
        return amount * self.c["pct"] + self.c["fix"]
    def charge(self, t, cust, amount, key, meta, first_card_use=False):
        sim = self.sim
        if self.down(t): return {"result": "unreachable"}
        if key in self.by_key:
            ch = self.charges[self.by_key[key]]
            return {"result": ch["status"], "id": ch["id"]}
        if self.rng.random() < self.c["timeout"] * sim.cfg["chaos"]:
            if self.rng.random() < self.c["timeout_processed"]:
                ch = self._mk(t, cust, amount, key, meta, "succeeded")
                self._webhook(t, "payment_intent.succeeded", ch)
            return {"result": "timeout"}
        if self.rng.random() < self.c["decline"] * sim.cfg["chaos"] * cust.get("decline_mult", 1):
            ch = self._mk(t, cust, amount, key, meta, "failed")
            self._webhook(t, "payment_intent.payment_failed", ch)
            return {"result": "declined", "id": ch["id"]}
        if first_card_use and self.rng.random() < self.c["tds"]:
            ch = self._mk(t, cust, amount, key, meta, "requires_action")
            return {"result": "requires_action", "id": ch["id"]}
        ch = self._mk(t, cust, amount, key, meta, "succeeded")
        self._webhook(t, "payment_intent.succeeded", ch)
        return {"result": "succeeded", "id": ch["id"]}
    def complete_action(self, t, cid, ok):
        ch = self.charges[cid]
        if ch["status"] != "requires_action": return ch["status"]
        ch["status"] = "succeeded" if ok else "failed"
        if ok:
            ch["t"] = t; self.sim.costs.add(t, "stripe_fees", ch["fee"]); ch["fee_booked"] = True
            self._webhook(t, "payment_intent.succeeded", ch)
        return ch["status"]
    def _mk(self, t, cust, amount, key, meta, status):
        cid = len(self.charges)
        ach = cust.get("ach", False)
        ch = {"id": cid, "t": t, "cust": cust["id"], "bid": cust["bid"], "amount": amount, "key": key, "meta": dict(meta),
              "status": status, "refunded": 0.0, "disputed": False, "ach": ach, "fee": self.fee(amount, ach), "fee_booked": False}
        self.charges.append(ch); self.by_key[key] = cid
        if status == "succeeded":
            self.sim.costs.add(t, "stripe_fees", ch["fee"]); ch["fee_booked"] = True
            self.sim.world.on_charged(t, ch)
        return ch
    def _webhook(self, t, typ, ch):
        r = self.rng.random()
        if r < self.c["wh_lost"]: return
        d = self.rng.uniform(0.05, 1)
        if r < self.c["wh_lost"] + self.c["wh_late"]: d = self.rng.uniform(30, 180)
        self.sim.eng.at(t + d, self.sim.app.on_stripe_event, typ, ch["id"])
    def retrieve_by_key(self, t, key):
        if self.down(t): return "unreachable"
        if key not in self.by_key: return None
        return self.charges[self.by_key[key]]
    def refund(self, t, cid, amount, key):
        if self.down(t): return "unreachable"
        for r in self.refunds:
            if r["key"] == key: return "ok"
        ch = self.charges[cid]
        if ch["disputed"]: return "disputed"
        if self.rng.random() < self.c["refund_fail"] * self.sim.cfg["chaos"]: return "failed"
        amount = min(amount, ch["amount"] - ch["refunded"])
        if amount <= 0: return "ok"
        ch["refunded"] += amount
        self.refunds.append({"t": t, "cid": cid, "amount": amount, "key": key})
        return "ok"
    def open_dispute(self, t, cid, reason):
        ch = self.charges[cid]
        if ch["disputed"] or ch["status"] != "succeeded": return
        ch["disputed"] = True
        d = {"t": t, "cid": cid, "amount": ch["amount"] - ch["refunded"], "reason": reason, "due": t + self.sim.cfg["dispute_respond_days"] * DAY,
             "responded": None, "outcome": None}
        self.disputes.append(d)
        self.sim.costs.add(t, "dispute_fees", self.c["dispute_fee"])
        self.sim.eng.at(t + 1, self.sim.app.on_dispute, len(self.disputes) - 1)
        self.sim.eng.at(d["due"] + 1, self._dispute_close, len(self.disputes) - 1)
    def _dispute_close(self, i):
        d = self.disputes[i]
        if d["responded"] is not None and d["responded"] <= d["due"]:
            d["outcome"] = "won" if self.rng.random() < self.sim.cfg["dispute_win"] else "lost"
        else:
            d["outcome"] = "lost"
        if d["outcome"] == "won":
            self.sim.costs.add(d["due"], "dispute_fees", -self.c["dispute_fee"] * 0)  # fee kept by Stripe (A)

class AIDown(Exception): pass

class Claude:
    def __init__(self, sim):
        self.sim = sim; self.c = sim.cfg["ai"]; self.rng = random.Random(sim.seed * 107 + 3)
        self.log = []
        self.outages = list(self.c["outages"])
        if sim.cfg.get("ai_outage"):
            a, h = sim.cfg["ai_outage"]; self.outages.append((a * DAY + 9 * HOUR, a * DAY + 9 * HOUR + h * 60))
    def down(self, t): return any(a <= t < b for a, b in self.outages)
    def _rec(self, t, purpose, text, photo=None):
        if self.down(t): raise AIDown()
        self.log.append({"t": t, "purpose": purpose, "input": text, "photo": photo})
    def read_request(self, t, text, truth):
        self._rec(t, "read_request", text)
        self.sim.costs.add(t, "claude", self.c["cost_request"])
        r = self.rng; ch = self.sim.cfg["chaos"]
        cat = truth["cat"]
        if r.random() < self.c["cat_err"] * ch:
            cat = r.choice(list(self.sim.CATS))
        jt = truth["jobtype"]
        if cat != truth["cat"] or r.random() < self.c["type_err"] * ch:
            from config import JOBTYPES
            opts = [x[0] for x in JOBTYPES.get(cat, [(cat,)])]
            jt = r.choice(opts)
        emer = truth["emergency"]
        if emer and truth.get("indirect") and r.random() < self.c["miss_indirect"] * ch: emer = False
        inj = truth.get("injection") and r.random() < self.c["injection"] * ch
        if inj:
            emer = True if truth.get("injection_kind") == "urgent" else emer
        lang = "es" if truth.get("spanish") and r.random() > 0.02 else "en"
        rental = truth.get("tenant", False) and r.random() > 0.15
        return {"cat": cat, "jobtype": jt, "emergency": emer, "rental": rental, "lang": lang, "injected": inj,
                "card_text": self._card(truth, text)}
    def _card(self, truth, text):
        # The AI writes the short card description; a scrub miss keeps a fragment of personal detail.
        base = truth["card_core"]
        if truth.get("pii_in_text") and self.rng.random() < self.c["scrub_miss"] * self.sim.cfg["chaos"]:
            return base + " " + truth["pii_fragment"]
        return base
    def read_quote(self, t, text, truth_amount):
        self._rec(t, "read_quote", text)
        self.sim.costs.add(t, "claude", self.c["cost_quote"])
        if self.rng.random() < self.c["quote_err"] * self.sim.cfg["chaos"]:
            return {"amount": truth_amount * self.rng.choice([0.1, 10, 1.5]), "conf": self.rng.uniform(0.6, 0.95)}
        return {"amount": truth_amount, "conf": self.rng.uniform(0.85, 0.99)}
    def screen_photo(self, t, photo):
        self._rec(t, "screen_photo", "[photo]", photo)
        self.sim.costs.add(t, "claude", self.c["cost_photo"])
        if photo["identifying"] and self.rng.random() < self.c["photo_miss"] * self.sim.cfg["chaos"]:
            return False
        return photo["identifying"]

class ROC:
    def __init__(self, sim):
        self.sim = sim; self.c = sim.cfg["roc"]; self.rng = random.Random(sim.seed * 109 + 4)
        self.records = {}     # lic -> {"holder", "classes", "lapse_t", "phone", "email", "addr"}
        self.layout_broken_since = None
        self.queries = []
        fl = self.c.get("forced_layout_day")
        if fl is not None:
            sim.eng.at(fl * DAY + 3 * HOUR, self._break)
    def _break(self):
        if self.layout_broken_since is None: self.layout_broken_since = self.sim.eng.t
    def fix_layout(self, t): self.layout_broken_since = None
    def batch_tick(self, t):
        if self.layout_broken_since is None and self.rng.random() < self.c["layout"] * 0.5:   # ~1% per day, 2 batches/day
            self.layout_broken_since = t
    def check(self, t, lic):
        self.queries.append(t)
        if self.layout_broken_since is not None: return "unknown"
        r = self.rng.random()
        if r < self.c["down"] + self.c["captcha"]: return "unknown"
        rec = self.records.get(lic)
        if rec is None: return "not_found"
        if rec["lapse_t"] is not None and t >= rec["lapse_t"]: return "inactive"
        return "active"
    def truth_active(self, lic, t):
        rec = self.records.get(lic)
        return rec is not None and (rec["lapse_t"] is None or t < rec["lapse_t"])

class Email:
    def __init__(self, sim):
        self.sim = sim; self.rng = random.Random(sim.seed * 113 + 5); self.log = []
    def send(self, t, to, subject, body):
        b = to is None or self.rng.random() < self.sim.cfg["email_bounce"]
        self.log.append({"t": t, "to": to, "subject": subject, "body": body, "bounced": b})
        if not b and to:
            p = self.sim.world.email_owner(to)
            if p: self.sim.eng.at(t + self.rng.uniform(1, 30), p.receive_email, subject, body)
        return not b

class Mail:
    def __init__(self, sim):
        self.sim = sim; self.rng = random.Random(sim.seed * 127 + 6); self.log = []
    def send_letter(self, t, addr_owner, code):
        self.sim.costs.add(t, "letters", self.sim.cfg["letter_cost"])
        lost = self.rng.random() < self.sim.cfg["letter_lost"]
        arr = t + self.rng.uniform(3, 7) * DAY
        self.log.append({"t": t, "to": addr_owner.bid, "arrive": arr, "lost": lost})
        if not lost: self.sim.eng.at(arr, addr_owner.got_letter, code)

class Render:
    """Hosting: $7/month (inside fixed cost). Holds the database; the app can be restored from a backup."""
    def __init__(self, sim): self.sim = sim; self.backups = []
