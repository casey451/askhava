"""
Fake versions of every outside service Ask Hava would use.
Each one has the same shape as the real service (send/charge/lookup/webhook), charges realistic prices,
and fails the way the real one can (delays, duplicates, declines, outages, CAPTCHAs, misreads).
Nothing here contacts any real service.
"""
import math, re

PRICES = {  # USD, checked on vendor pricing pages Oct 26, 2026 (Twilio SMS/voice/Verify, Stripe); AI per-call costs are estimates from Claude Haiku 5.5 token prices
    "sms_segment": 0.0083, "sms_carrier_fee": 0.0042, "mms": 0.0300, "number_month": 1.15,
    "voice_min": 0.01125, "verify_otp": 0.05, "ai_intake_call": 0.004, "ai_parse_call": 0.002,
    "ai_screen_photo": 0.003, "stripe_pct": 0.029, "stripe_fixed": 0.30, "stripe_dispute": 15.0,
    "license_check": 0.0, "email": 0.001,
}


class Costs:
    def __init__(self):
        self.items = {}

    def add(self, k, v):
        self.items[k] = self.items.get(k, 0.0) + v


# ----------------------------------------------------------------------------------- texting
class MockSMS:
    """Like Twilio Messaging: send(), inbound webhooks, delivery-status callbacks, carrier STOP handling."""
    def __init__(self, sim, cfg):
        self.sim, self.cfg = sim, cfg
        self.carrier_stopped = set()      # numbers that texted STOP (carrier-level block on long codes)
        self.suspended = False
        self.log = []                     # every outbound message, as sent
        self.handsets = {}                # phone -> actor callback (the 'phone' of each simulated person)
        self.inbound_log = []             # every text people sent us: (phone, body, time, MessageSid)
        self.inbound_media = {}

    def list_outbound(self, since):
        """Like GET /Messages from our number since a time (the app matches its own message templates in the bodies)."""
        return [dict(m) for m in self.log if m["at"] >= since]

    def list_inbound(self, since):
        """Like GET /Messages (filtered to inbound, DateSent >= that day, then by time): what a restore replays."""
        return [(f, b, t, sid, self.inbound_media.get(sid, [])) for f, b, t, sid in self.inbound_log if t >= since]

    def segments(self, body):
        return max(1, math.ceil(len(body) / 153)) if len(body) > 160 else 1

    def send(self, to, body, kind, meta=None, media=None):
        t = self.sim.now
        rec = dict(id=f"SM{len(self.log)+1}", to=to, body=body, kind=kind, meta=meta or {}, at=t, status="queued", media=media or [])
        self.log.append(rec)
        segs = self.segments(body)
        self.sim.costs.add("texting", segs * (PRICES["sms_segment"] + PRICES["sms_carrier_fee"]) + (PRICES["mms"] if media else 0))
        if self.suspended:
            rec["status"] = "failed"; rec["error"] = "campaign suspended"
            self.sim.later(1, self.sim.app.on_sms_status, rec["id"], "failed", "campaign suspended")
            return rec["id"]
        if to in self.carrier_stopped:
            rec["status"] = "failed"; rec["error"] = "21610 unsubscribed recipient"
            self.sim.later(1, self.sim.app.on_sms_status, rec["id"], "failed", "21610")
            return rec["id"]
        rng = self.sim.rng
        if rng.random() < self.cfg["sms_fail_prob"]:
            rec["status"] = "undelivered"
            self.sim.later(rng.uniform(1, 30), self.sim.app.on_sms_status, rec["id"], "undelivered", "30003")
            return rec["id"]
        delay = rng.lognormvariate(math.log(0.2), 1.0)        # minutes
        if rng.random() < self.cfg["sms_delay_prob"]:
            delay += rng.uniform(30, 240)                        # carrier queueing
        self.sim.later(delay, self._deliver, rec)
        return rec["id"]

    def _deliver(self, rec):
        rec["status"] = "delivered"; rec["delivered_at"] = self.sim.now
        h = self.handsets.get(rec["to"])
        if h:
            h(rec)
        self.sim.app.on_sms_status(rec["id"], "delivered", None)
        if self.sim.rng.random() < self.cfg["webhook_dup_prob"]:
            self.sim.later(self.sim.rng.uniform(0, 5), self.sim.app.on_sms_status, rec["id"], "delivered", None)

    def inbound(self, frm, body, media=None):
        """A person texts the business number."""
        self.sim.costs.add("texting", PRICES["sms_segment"] + PRICES["sms_carrier_fee"] + (PRICES["mms"] if media else 0))
        if body.strip().upper() in ("STOP", "STOPALL", "UNSUBSCRIBE", "CANCEL", "END", "QUIT"):
            self.carrier_stopped.add(frm)
        if body.strip().upper() in ("START", "UNSTOP"):
            self.carrier_stopped.discard(frm)
        sid = f"MM{len(self.inbound_log)+1}"
        self.inbound_log.append((frm, body, self.sim.now, sid))
        self.inbound_media[sid] = media or []
        d = self.sim.rng.lognormvariate(math.log(0.1), 0.8)
        self.sim.later(d, self.sim.app.on_inbound_sms, frm, body, media or [], False, sid)
        if self.sim.rng.random() < self.cfg["webhook_dup_prob"]:
            self.sim.later(d + self.sim.rng.uniform(0, 2), self.sim.app.on_inbound_sms, frm, body, media or [], True, sid)


# ----------------------------------------------------------------------------------- AI
MAJOR_WORDS = ["whole new", "new ac system", "replace the whole", "repipe", "repiped", "panel upgrade", "new water heater and", "whole house"]
RENTAL_WORDS = ["rental", "tenant", "renter"]
EMERGENCY_WORDS = ["gas", "smell gas", "carbon monoxide", "co alarm", "co detector", "sparks", "sparking", "smoke",
                   "burning smell", "smells like burning", "fire", "flooding", "water everywhere", "exposed wire",
                   "electrocuted", "shock", "fuga de gas", "huele a gas", "chispas", "inundacion"]


class MockAI:
    """Like a hosted language model with a signed zero-retention agreement (zdr=True). Makes realistic mistakes."""
    def __init__(self, sim, cfg, name, zdr):
        self.sim, self.cfg, self.name, self.zdr = sim, cfg, name, zdr
        self.inputs = []          # every input it ever saw (to check the data rule)
        self.down_until = 0

    def available(self):
        return self.sim.now >= self.down_until

    def _seen(self, text):
        self.inputs.append((self.sim.now, text))

    def classify_intake(self, text, truth):
        """Returns trade, emergency, urgent, lang, rental, major. 'truth' only decides when the fake model makes a mistake."""
        if not self.available():
            return dict(error="unavailable", trade=None, emergency=False, urgent=False, lang="en", injection=False, tool_request=None, rental=False, major=False)
        cache = self.__dict__.setdefault("_last_reading", {})
        if text in cache and self.sim.rng.random() < self.cfg.get("ai_reread_correlation", 0.6):
            self._seen(text); self.sim.costs.add("ai", PRICES["ai_intake_call"])
            return dict(cache[text])                    # same model, same text: usually the same answer, mistakes included
        out = self._classify(text, truth)
        cache[text] = dict(out)
        return out

    def _classify(self, text, truth):
        self._seen(text)
        self.sim.costs.add("ai", PRICES["ai_intake_call"])
        rng, low = self.sim.rng, text.lower()
        spanish = re.search(r"\b(necesito|ayuda|hoy|fuga|agua|tengo|por favor)\b", low) is not None
        rental = any(w in low for w in RENTAL_WORDS) or (truth.get("rental") and rng.random() < 0.3)   # hinted, not stated: often missed
        major = (any(w in low for w in MAJOR_WORDS) and rng.random() < 0.95) or rng.random() < 0.01
        out = dict(trade=truth["trade"], emergency=False, urgent="today" in low or "asap" in low or "hoy" in low,
                   lang="es" if spanish else "en", injection=False, tool_request=None, rental=bool(rental), major=bool(major))
        if any(w in low for w in EMERGENCY_WORDS):
            out["emergency"] = True
        elif truth.get("emergency") and rng.random() > self.cfg["ai_emergency_recall_implicit"]:
            out["emergency"] = False          # implicit wording missed (e.g. 'pilot light smells funny')
        elif truth.get("emergency"):
            out["emergency"] = True
        if rng.random() < self.cfg["ai_trade_error"]:
            out["trade"] = rng.choice(["plumbing", "hvac", "electrical", "handyman", "pool"])
        # job type within the trade (e.g. water heater vs drain): sometimes wrong; when wrong it is usually unsure
        from app import JOB_TYPES
        jt = truth.get("job_type") if truth.get("trade") == out["trade"] else None
        choices = JOB_TYPES.get(out["trade"], [])
        out.update(job_type=jt or (choices[-1] if choices else None), alt_job_type=None, job_type_conf=0.95)
        if choices and rng.random() < self.cfg.get("ai_jobtype_error", 0.06):
            wrong = rng.choice([c for c in choices if c != out["job_type"]] or choices)
            unsure = rng.random() < 0.7
            out.update(job_type=wrong, alt_job_type=(jt if unsure else None), job_type_conf=(0.6 if unsure else 0.9))
        if "ignore" in low and "instruction" in low:
            out["injection"] = True
            if rng.random() < self.cfg["ai_injection_compliance"]:
                out["tool_request"] = "send_all_phone_numbers"   # the model 'tries'; the app must never act on it
        return out

    def ai_scrub(self, text):
        """Zero-retention model rewrites the description without names, spelled-out numbers or addresses."""
        self._seen(text)
        self.sim.costs.add("ai", PRICES["ai_parse_call"])
        if self.sim.rng.random() < self.cfg.get("ai_scrub_recall", 0.98):
            t = re.sub(r"(ask for|my name is|this is)\s+\w+", "[name removed]", text, flags=re.I)
            t = re.sub(r"(my number is|call me at|text me)[^.,]*", "[contact removed]", t, flags=re.I)
            t = re.sub(r"\b(zero|one|two|three|four|five|six|seven|eight|nine)(\s+(zero|one|two|three|four|five|six|seven|eight|nine)){3,}\b", "[number removed]", t, flags=re.I)
            return t
        return text

    def parse_quote(self, text):
        """Reads a free-text contractor quote into the standard format; returns confidence."""
        self._seen(text)
        self.sim.costs.add("ai", PRICES["ai_parse_call"])
        low = text.lower()
        nums = [int(x) for x in re.findall(r"\$?(\d{2,5})", low)]
        typ = "trip" if ("trip" in low or "diag" in low or "service call" in low) else ("range" if "-" in low and len(nums) >= 2 else "firm")
        conf = 0.95
        if "plus parts" in low or "+ parts" in low or "depends" in low or "maybe" in low or not nums:
            conf = 0.5
        if self.sim.rng.random() < self.cfg["ai_parse_error"]:
            conf = 0.9; typ = "firm"                               # confident but wrong (the dangerous case)
        return dict(type=typ, amount=nums[0] if nums else None, high=nums[1] if len(nums) > 1 else None, confidence=conf)

    def screen_photo(self, photo):
        """Detects house numbers, faces, documents. Location data is stripped by plain code, not by AI."""
        if not self.available():
            return ["unavailable"]                 # can't screen: the app must treat the photo as not safe to share
        self.sim.costs.add("ai", PRICES["ai_screen_photo"])
        found = []
        for k in ("house_number", "face", "document"):
            if photo.get(k) and self.sim.rng.random() < self.cfg["ai_photo_recall"]:
                found.append(k)
        return found

    def weekly_summary(self, stats_text):
        self._seen(stats_text)
        self.sim.costs.add("ai", PRICES["ai_intake_call"] * 3)
        return "summary"


# ----------------------------------------------------------------------------------- payments
class MockStripe:
    """Like Stripe: customers, SetupIntents (save card), off-session PaymentIntents, refunds, disputes, webhooks, payouts."""
    def __init__(self, sim, cfg):
        self.sim, self.cfg = sim, cfg
        self.customers = {}
        self.intents = {}           # id -> dict
        self.idem = {}              # idempotency key -> intent id
        self.balance = 0.0
        self.paid_out = 0.0
        self.fees = 0.0
        self.events = []
        self.down_until = 0

    def available(self):
        return self.sim.now >= self.down_until

    def create_customer(self, ref, idem_key=None):
        if idem_key and idem_key in self.idem:
            return self.idem[idem_key]
        cid = f"cus_{len(self.customers)+1}"
        self.customers[cid] = dict(ref=ref, pm=None)
        if idem_key:
            self.idem[idem_key] = cid
        return cid

    def setup_card(self, cid, card):
        """card: dict(good=bool, needs_3ds=bool)"""
        if not self.available():
            return dict(ok=False, error="api_unavailable")
        if not card["good"]:
            return dict(ok=False, error="card_declined")
        self.customers[cid]["pm"] = card
        return dict(ok=True)

    def charge_off_session(self, cid, amount, idem_key, meta):
        if not self.available():
            return dict(ok=False, error="api_unavailable", id=None)
        if idem_key in self.idem:
            return dict(ok=True, id=self.idem[idem_key], replayed=True)
        pid = f"pi_{len(self.intents)+1}"
        self.idem[idem_key] = pid
        pm = self.customers[cid]["pm"]
        rng = self.sim.rng
        if pm is None:
            status = "failed"; err = "no_payment_method"
        elif pm.get("needs_3ds") and rng.random() < 0.7:
            status = "requires_action"; err = "authentication_required"
        elif rng.random() < self.cfg["card_decline_prob"]:
            status = "failed"; err = "card_declined"
        else:
            status = "succeeded"; err = None
        self.intents[pid] = dict(id=pid, cid=cid, amount=amount, status=status, error=err, meta=meta, refunded=0.0, disputed=False, at=self.sim.now)
        if status == "succeeded":
            fee = amount * self.cfg["stripe_pct"] + self.cfg["stripe_fixed"]
            self.balance += amount - fee; self.fees += fee
            self.sim.costs.add("card_fees", fee)
        self._webhook(f"payment_intent.{ 'succeeded' if status=='succeeded' else ('requires_action' if status=='requires_action' else 'payment_failed')}", pid)
        return dict(ok=True, id=pid, status_now="processing")

    def complete_authentication(self, pid, success=True):
        pi = self.intents[pid]
        if pi["status"] != "requires_action":
            return
        pi["changed_at"] = self.sim.now
        if success:
            pi["status"] = "succeeded"
            fee = pi["amount"] * self.cfg["stripe_pct"] + self.cfg["stripe_fixed"]
            self.balance += pi["amount"] - fee; self.fees += fee
            self.sim.costs.add("card_fees", fee)
            self._webhook("payment_intent.succeeded", pid)
        else:
            pi["status"] = "failed"; self._webhook("payment_intent.payment_failed", pid)

    def list_intents(self, since):
        """Like GET /v1/payment_intents?created[gte]=..."""
        if not self.available():
            return None
        return [dict(pi) for pi in self.intents.values() if pi["at"] >= since]          # created[gte] only, like the real API

    def list_events(self, since):
        """Like GET /v1/events?created[gte]=... (kept 30 days): includes events whose webhook was never delivered."""
        if not self.available():
            return None
        return [dict(e) for e in self.events if e["created"] >= since]

    def retrieve(self, pid):
        """Like GET /v1/payment_intents/:id."""
        if not self.available():
            return None
        return dict(self.intents[pid])

    def refund(self, pid, amount, idem_key):
        if not self.available():
            return dict(ok=False, error="api_unavailable")
        if idem_key in self.idem:
            return dict(ok=True, replayed=True)
        pi = self.intents[pid]
        if pi["status"] != "succeeded":
            return dict(ok=False, error="charge_not_succeeded")
        if pi["disputed"]:
            return dict(ok=False, error="charge_disputed")
        if pi["refunded"] + amount > pi["amount"] + 1e-9:
            return dict(ok=False, error="amount_too_large")
        self.idem[idem_key] = pid
        pi["refunded"] += amount; pi["changed_at"] = self.sim.now
        self.balance -= amount                        # Stripe keeps its fee
        self._webhook("charge.refunded", pid)
        return dict(ok=True)

    def maybe_dispute(self, pid):
        pi = self.intents[pid]
        if pi["status"] == "succeeded" and not pi["disputed"]:
            pi["disputed"] = True; pi["changed_at"] = self.sim.now
            self.balance -= pi["amount"] - pi["refunded"] + self.cfg["dispute_fee"]
            self.fees += self.cfg["dispute_fee"]
            self.sim.costs.add("dispute_fees", self.cfg["dispute_fee"])
            self._webhook("charge.dispute.created", pid)

    def payout(self):
        reserve = self.cfg["reserve"]
        amt = max(0.0, self.balance - reserve)
        self.balance -= amt; self.paid_out += amt
        return amt

    def _webhook(self, typ, pid):
        rng = self.sim.rng
        ev = dict(id=f"evt_{len(self.events)+1}", type=typ, pid=pid, created=self.sim.now)
        self.events.append(ev)
        if rng.random() < self.cfg.get("webhook_loss_prob", 0.0):
            ev["lost"] = True                         # never delivered (endpoint down past Stripe's retry window, misconfig)
            return
        d = rng.lognormvariate(math.log(0.1), 1.0)
        if rng.random() < self.cfg["webhook_delay_prob"]:
            d += rng.uniform(60, 600)                 # delayed delivery (minutes)
        self.sim.later(d, self.sim.app.on_stripe_webhook, ev)
        if rng.random() < self.cfg["webhook_dup_prob"]:
            self.sim.later(d + rng.uniform(0, 30), self.sim.app.on_stripe_webhook, ev)


# ----------------------------------------------------------------------------------- masked calling
class MockVoice:
    """Like a number-masking call service: one session per confirmed job; calls only from linked phones."""
    def __init__(self, sim, cfg):
        self.sim, self.cfg = sim, cfg
        self.sessions = {}          # session id -> dict(request, allowed_contractor_phones, homeowner_phone, open)
        self.calls = []

    def open_session(self, request_id, contractor_phones, homeowner_phone, days=30):
        sid = f"KS{len(self.sessions)+1}"
        self.sessions[sid] = dict(request=request_id, cphones=set(contractor_phones), hphone=homeowner_phone, open=True,
                                  until=self.sim.now + days * 1440, opened=self.sim.now, closed_at=None)
        return sid

    def add_phone(self, sid, phone):
        s = self.sessions.get(sid)
        if s and s["open"]:
            s["cphones"].add(phone)

    def list_calls(self, since):
        return [dict(c) for c in self.calls if c["at"] >= since]

    def close_session(self, sid):
        s = self.sessions.get(sid)
        if s and s["open"]:
            s["open"] = False; s["closed_at"] = self.sim.now

    def place_call(self, sid, from_phone, homeowner_actor):
        s = self.sessions.get(sid)
        rec = dict(sid=sid, frm=from_phone, at=self.sim.now, answered=False, human=False, seconds=0, connected_to=None, rejected=None)
        self.calls.append(rec)
        if not s or not s["open"] or self.sim.now > s["until"]:
            rec["rejected"] = "session_closed"
        elif from_phone not in s["cphones"]:
            rec["rejected"] = "unlinked_phone"
        else:
            ans, human, secs = homeowner_actor.answer_call()
            rec.update(answered=ans, human=human, seconds=secs, connected_to=s["hphone"])
            self.sim.costs.add("calls", 2 * max(1, math.ceil(secs / 60)) * PRICES["voice_min"])
        self.sim.later(self.sim.rng.uniform(0.1, 2), self.sim.app.on_call_completed, dict(rec))
        return rec


# ----------------------------------------------------------------------------------- license / identity checks
class MockLicenseSite:
    """Like the Arizona ROC public lookup used through a browser (Cowork). Can be down, show a CAPTCHA, or change layout."""
    def __init__(self, sim, cfg):
        self.sim, self.cfg = sim, cfg
        self.records = {}           # roc -> dict(name, status, cls, contact_phone, contact_email)

    def lookup(self, roc):
        self.lookups = getattr(self, "lookups", 0) + 1            # each one is a browser visit to the state site (about 30-60 seconds)
        r = self.sim.rng.random()
        if r < self.cfg["roc_down_prob"]:
            return dict(ok=False, error="site_down")
        if r < self.cfg["roc_down_prob"] + self.cfg["roc_captcha_prob"]:
            return dict(ok=False, error="captcha")
        if r < self.cfg["roc_down_prob"] + self.cfg["roc_captcha_prob"] + self.cfg["roc_layout_change_prob"]:
            return dict(ok=True, record=dict(name="", status="UNKNOWN", cls="", contact_phone=None))   # misread page
        rec = self.records.get(roc)
        if not rec:
            return dict(ok=True, record=None)
        out = dict(rec)
        if out.get("bond_cancel_posted", 0) > self.sim.now:
            out["bond_cancel"] = None                 # the bond company hasn't filed the cancellation yet
        out.pop("bond_cancel_posted", None)
        return dict(ok=True, record=out)


class MockEmail:
    def __init__(self, sim):
        self.sim = sim
        self.log = []

    def send(self, to, subject, body):
        self.log.append(dict(to=to, subject=subject, at=self.sim.now))
        self.sim.costs.add("email", PRICES["email"])


class MockLocalScreen:
    """Plain image checks on our own server (an OCR library for digits and text, a face detector). Nothing leaves the server.
    Its misses are independent of the AI's, because it works a different way. Recall numbers are assumptions."""
    def __init__(self, sim, cfg):
        self.sim, self.cfg = sim, cfg

    def screen(self, photo):
        rng, found = self.sim.rng, []
        if (photo.get("house_number") or photo.get("document")) and rng.random() < self.cfg.get("local_ocr_recall", 0.85):
            found.append("text_or_digits")
        if photo.get("face") and rng.random() < self.cfg.get("local_face_recall", 0.85):
            found.append("face")
        return found


class MockVerify:
    """Like Twilio Verify: sends a one-time code and checks it. Verification records live at Twilio, not in our database."""
    def __init__(self, sim):
        self.sim = sim
        self.records = []      # dict(phone, ref, code, created, status)

    def start(self, phone, ref):
        code = f"{self.sim.rng.randrange(10**6):06d}"
        self.records.append(dict(phone=phone, ref=ref, code=code, created=self.sim.now, status="pending"))
        self.sim.costs.add("otp", PRICES["verify_otp"])
        return code

    def check(self, phone, ref, code, max_age=15):
        for v in reversed(self.records):
            if v["phone"] == phone and v["ref"] == ref and v["status"] == "pending" and self.sim.now - v["created"] <= max_age:
                if v["code"] == code:
                    v["status"] = "approved"; v["approved_at"] = self.sim.now
                    return True
                return False
        return False

    def approved_since(self, phone, ref, since):
        return [dict(v) for v in self.records if v["phone"] == phone and v["ref"] == ref and v["status"] == "approved" and v["created"] >= since]
