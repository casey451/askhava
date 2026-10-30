"""
The Ask Hava app as specified in Plan v9 / Technical Blueprint v9 - the system under test.
It talks to the outside world ONLY through the vendor objects (sim.sms, sim.ai_intake, sim.stripe, sim.voice, ...),
exactly as the real app would talk to Twilio, Stripe, the AI provider and the ROC site.
"""
import re, math, json, os

# homeowner texts that have an approved Spanish version (drafts in templates_es.json, for native-speaker review)
SPANISH_KINDS = {k for k in json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "templates_es.json"))) if not k.startswith("_")}


class CoRecord(dict):
    """A contractor record. Status changes are also written to an audit stream outside the database."""
    stream = None
    clock = None

    def __setitem__(self, k, v):
        if k == "status" and dict.get(self, "_live") and dict.get(self, "status") != v and CoRecord.stream is not None:
            CoRecord.stream.append((CoRecord.clock(), dict.get(self, "id"), v))
        dict.__setitem__(self, k, v)

DAY = 1440
OPEN_H, CLOSE_H = 8, 21

ZIP_TOWN = {"86403": "Lake Havasu City", "86404": "Lake Havasu City", "86405": "Lake Havasu City", "86406": "Lake Havasu City",
            "86429": "Bullhead City", "86442": "Bullhead City", "86426": "Fort Mohave", "86401": "Kingman", "86409": "Kingman"}

FEES = {"plumbing": 45, "electrical": 45, "hvac": 59, "handyman": 29, "pool": 45, "appliance": 29}
MAJOR_FEE = 149

# v9.3 job types and which Arizona residential license classes may do them (Ariz. Admin. Code R4-9-103).
# This table must be reviewed with the attorney / ROC before launch; it is deliberately conservative.
JOB_TYPES = {
    "plumbing": ["leak_repair", "toilet_fixture", "water_heater", "drain_sewer", "repipe", "plumbing_other"],
    "hvac": ["ac_repair", "heating_repair", "thermostat", "system_replacement", "hvac_other"],
    "electrical": ["outlets_breakers", "fixture_install", "panel_upgrade", "electrical_other"],
    "pool": ["pool_equipment", "pool_remodel", "pool_other"],
    "handyman": ["drywall", "mounting", "doors", "handyman_other"],
}
CLASS_ALLOWS = {
    "R-37": set(JOB_TYPES["plumbing"]),                      # plumbing
    "R-37R-drains": {"drain_sewer"},                         # sewers and drains only
    "R-39": set(JOB_TYPES["hvac"]),                          # air conditioning and heating
    "R-39R-ac": {"ac_repair", "thermostat", "system_replacement"},
    "R-11": set(JOB_TYPES["electrical"]),                    # electrical
    "R-6": {"pool_equipment", "pool_other"},                 # pool service and minor repair only
    "B-6": set(JOB_TYPES["pool"]),                           # pools and spas
    "HANDYMAN": set(JOB_TYPES["handyman"]),                  # no state license: small non-trade jobs under $1,000
}
KEYWORD_TYPES = [(r"\b(remodel|resurfac|replaster|re-plaster|new pool|pool build)", "pool_remodel"),
                 (r"\b(repipe|re-pipe|whole house (plumbing|pipes))", "repipe"),
                 (r"\b(panel upgrade|new panel|upgrade (the )?panel|200 ?amp)", "panel_upgrade"),
                 (r"\b(new (ac|a/c|unit|system|furnace)|replace (the )?(ac|a/c|unit|system|furnace)|whole new)", "system_replacement"),
                 (r"\b(drain|clog|sewer|backing up|backed up)", "drain_sewer"),
                 (r"\b(toilet|sink|faucet|shower|tub)\b", "toilet_fixture"), (r"\bwater heater|no hot water", "water_heater"),
                 (r"\b(leak|leaking|drip|dripping|burst)", "leak_repair"), (r"\b(blowing warm|not cooling|ac not|a/c not|no cool|ac blowing|warm air)", "ac_repair"),
                 (r"\b(heater|heating|furnace|no heat)\b", "heating_repair"), (r"\bthermostat", "thermostat"),
                 (r"\b(outlet|breaker|tripping|no power)", "outlets_breakers"), (r"\b(install|ceiling fan|light fixture|new light)", "fixture_install"),
                 (r"\b(pump|filter|heater|motor)\b", "pool_equipment"), (r"\bdrywall|patch", "drywall"), (r"\b(hang|mount|shel|mirror|tv)", "mounting"),
                 (r"\bdoor", "doors")]
JT_NAMES = {"leak_repair": "a leak", "toilet_fixture": "toilet, sink or faucet", "water_heater": "water heater", "drain_sewer": "clogged drain or sewer line",
            "repipe": "replacing the pipes", "plumbing_other": "other plumbing", "ac_repair": "AC repair", "heating_repair": "heating repair",
            "thermostat": "thermostat", "system_replacement": "replacing the whole AC/heating system", "hvac_other": "other heating/AC",
            "outlets_breakers": "outlets or breakers", "fixture_install": "installing a light or fan", "panel_upgrade": "electrical panel upgrade",
            "electrical_other": "other electrical", "pool_equipment": "pool pump, filter or equipment", "pool_remodel": "pool resurfacing or remodel",
            "pool_other": "other pool work", "drywall": "drywall", "mounting": "hanging or mounting things", "doors": "doors", "handyman_other": "other small repair"}
TRADE_WORDS = {"plumbing": r"plumb|pipe|repip|re-pip|leak|water|drain|toilet|faucet|fontaner|plomer", "hvac": r"\bac\b|a/c|air|heat|cool|hvac|furnace|aire|calefacc",
               "electrical": r"electric|outlet|breaker|wire|light|panel|ceiling fan", "pool": r"pool|spa|piscina",
               "handyman": r"handyman|drywall|\bdoor|shel(f|ves)|mirror|\bpatch|\bhang\b"}
CLASS_TRADE = {k: next(t for t, v in JOB_TYPES.items() if CLASS_ALLOWS[k] <= set(v)) for k in CLASS_ALLOWS}
JT_TRADE = {jt: t for t, v in JOB_TYPES.items() for jt in v}
TRADE_NAMES = {"hvac": "heating/AC", "plumbing": "plumbing", "electrical": "electrical", "pool": "pool", "handyman": "handyman (small repairs)"}


def keyword_trades(text):
    """AI-011: the trades whose plain keywords start a word of the text, so "repair" never reads as "air" (D-103)."""
    low = text.lower()
    return [t for t, pat in TRADE_WORDS.items() if re.search(rf"\b(?:{pat})", low)]


def boundary_types(text):
    """D-105, D-517: the keyword job types that mark a license boundary, kept whatever trade is read or answered. The
    twin's keyword patterns are trade-relative (a bare "heater" or "install"), so one counts when the homeowner's words
    also name its trade, the stand-in for the catalog's trade-specific keyword lists (LIC-001) until T007."""
    low, named = text.lower(), keyword_trades(text)
    return {jt for pat, jt in KEYWORD_TYPES if re.search(pat, low) and JT_TRADE[jt] in named}

# allowed request status changes (anything else is a bug)
EDGES = {
    "INTAKE": {"EMERGENCY", "INELIGIBLE", "DUPLICATE", "OPEN", "CANCELLED", "ABANDONED"},
    "OPEN": {"QUOTES_READY", "NO_QUOTES_YET", "CANCELLED", "NOT_COVERED"},
    "NO_QUOTES_YET": {"QUOTES_READY", "CLOSED_NO_QUOTES", "CANCELLED"},
    "QUOTES_READY": {"AWAITING_YES", "CLOSED_NONE", "CANCELLED", "CLOSED_NO_QUOTES"},
    "AWAITING_YES": {"AWAITING_CONFIRM", "QUOTES_READY", "CANCELLED", "CLOSED_NONE"},
    "AWAITING_CONFIRM": {"PAYMENT_PENDING", "QUOTES_READY", "CLOSED_NONE", "CANCELLED"},
    "PAYMENT_PENDING": {"CONNECTED", "AWAITING_CONFIRM", "QUOTES_READY", "CLOSED_NONE", "CANCELLED"},
    "CONNECTED": {"DONE", "QUOTES_READY", "CLOSED", "CANCELLED", "CLOSED_NONE"},
}
TERMINAL = {"NOT_COVERED", "EMERGENCY", "INELIGIBLE", "DUPLICATE", "CANCELLED", "ABANDONED", "CLOSED_NO_QUOTES", "CLOSED_NONE", "DONE", "CLOSED"}

# plain-code emergency words (checked on EVERY homeowner message, before anything else, no AI needed)
EMERGENCY_WORDS = ["gas", "carbon monoxide", "co alarm", "co detector", "sparks", "sparking", "smoke", "burning smell",
                   "smells like burning", "fire", "flooding", "water everywhere", "exposed wire", "electrocuted", "shock",
                   "fuga de gas", "huele a gas", "chispas", "inundacion"]
CONSERVATIVE_WORDS = ["smell", "burning", "alarm", "head hurts", "dizzy", "hot outlet", "buzzing panel"]
STOP_WORDS = ("STOP", "STOPALL", "UNSUBSCRIBE", "CANCEL", "END", "QUIT")   # carrier/Twilio opt-out words: we can't change these
STALE_IF_CLOSED = ("connect_contractor", "who_is_calling", "pick_confirm_request", "job_card", "nudge", "quote_link", "more_quotes",
                   "reoffer", "no_quotes_yet", "call_reminder", "needs_card", "payment_failed", "auth_needed", "onsite_q", "done_check")
def up_cmd_free(up):
    return not re.match(r"^(YES|NO|DONE|PROBLEM|CLOSE|CANCEL|STOP|LIST|LINK|START|HELP|OK|SI|ME)\b", up)


NATURAL_OPTOUT = re.compile(r"\b(stop (texting|messaging|sending|contacting)|(don'?t|do not) (text|message|contact)|no more (texts|messages|job cards)|"
                            r"unsubscribe|opt ?out|remove me|take me off|revoke|leave me alone|quit texting)\b", re.I)
LIVE_PICK = ("awaiting_confirm", "charge_retry", "needs_card", "payment_failed", "auth_needed", "charging", "held")

SPELLED_CONTACT_RE = re.compile(r"\b(call|text|reach|contact)\s+me\b|\b(zero|one|two|three|four|five|six|seven|eight|nine)(\s+(zero|one|two|three|four|five|six|seven|eight|nine)){2,}\b", re.I)
PHONE_RE = re.compile(r"(\+?1[\s.-]?)?\(?\d{3}\)?[\s.-]?\d{3}[\s.-]?\d{4}")
EMAIL_RE = re.compile(r"[\w.+-]+@[\w-]+\.[\w.]+")
ADDR_RE = re.compile(r"\b\d{2,6}\s+[A-Za-z]+(\s[A-Za-z]+)?\s(st|street|ave|avenue|dr|drive|rd|road|ln|lane|blvd|way|ct|court|pl|place)\b", re.I)


class App:
    def __init__(self, sim, cfg):
        self.sim, self.cfg = sim, cfg
        CoRecord.stream = sim.co_stream; CoRecord.clock = lambda: sim.now
        # private stores
        self.vault = {}                # contact id -> dict(name, phone, addresses)  (plain code only, never sent to AI except ZDR intake)
        self.phone_to_ho = {}          # routing index
        self.homeowners = {}
        self.requests = {}
        self.contractors = {}
        self.phone_to_co = {}
        self.cards = {}                # job cards
        self.quotes = {}
        self.picks = {}
        self.ledger = []               # app's own money records
        self.alerts = []
        self.audit = []
        self.msg_meta = {}             # sms id -> meta
        self.processed_events = set()  # webhook idempotency
        self.processed_inbound = set()
        self.stopped = set()
        self.paused = dict(all=False, towns=set(), trades=set(), charging=False, texting=False)
        self.safe_mode = False
        self.req_seq = 1040
        self.view_tokens = {}          # quote-page link token -> request id (view only)
        self.violations_inline = []
        self.last_inbound = {}         # phone -> time of their last text to us (for the 'direct reply' quiet-hours rule)
        self.timers = {}               # timer table (lives in the database): id -> (due time, action, args)
        self.replaying = False
        self.replay_sent = None
        self.timer_seq = 0
        self.epoch = 0                 # bumps on a database restore; timers from before the restore are ignored
        self.uid_n = 0

    # ------------------------------------------------------------------ timers live in the database
    def schedule_at(self, t, name, *args):
        self.timer_seq += 1
        tid = self.timer_seq
        self.timers[tid] = (t, name, args)
        self.sim.at(t, self._fire, tid, self.epoch)
        return tid

    def schedule_in(self, d, name, *args):
        return self.schedule_at(self.sim.now + d, name, *args)

    def _fire(self, tid, epoch):
        if epoch != self.epoch or tid not in self.timers:
            return
        t, name, args = self.timers.pop(tid)
        getattr(self, name)(*args)

    def stable_id(self, prefix, *parts):
        """v9.3: IDs derived from what they identify (plus a server secret), so a replay after a restore recreates the
        same job-card links, quote IDs and pick IDs, and Stripe's idempotency keys line up."""
        import hashlib
        return prefix + hashlib.sha256(("hhq-server-secret|" + "|".join(map(str, parts))).encode()).hexdigest()[:10]

    def token_for(self, rid):
        return f"tok{rid}x{self.stable_id('', 'quote-page', rid)}" if self.fx("v93") else f"tok{rid}x{self.sim.rng.randrange(10**8)}"

    def resolve_token(self, token):
        if self.fx("v93"):
            m = re.match(r"^tok(\d+)x([0-9a-f]{10})$", token or "")
            return m.group(1) if m and self.token_for(m.group(1)) == token and m.group(1) in self.requests else None
        return self.view_tokens.get(token)

    def web(self, name, *args):
        """Every website action is also written to the web server's request log, which lives outside the database."""
        if not self.replaying:
            self.sim.web_log.append((self.sim.now, name, args))

    def new_id(self, prefix, n_existing):
        """v9.3: IDs that can never repeat, even after a database restore rolls counters back."""
        if self.fx("v93"):
            return f"{prefix}{self.sim.rng.getrandbits(40):010x}"
        return f"{prefix}{n_existing + 1}"

    # wrappers so timers carry only ids (as a database row would)
    def run_intake_id(self, hid, phone, onsite_done=False):
        return self.run_intake(self.homeowners[hid], phone, onsite_done)

    def charge_id(self, pid):
        return self.charge(self.picks[pid])

    def do_refund_id(self, pid, reason, attempt=1):
        return self.do_refund(self.picks[pid], reason, attempt)

    def fx(self, k):
        return self.cfg.get("fixes", {}).get(k)

    def emergency_words(self, text):
        low = text.lower()
        words = EMERGENCY_WORDS + (CONSERVATIVE_WORDS if self.fx("conservative_emergency") else [])
        return any(w in low for w in words)

    # ------------------------------------------------------------------ helpers
    def log(self, what, **kw):
        self.audit.append(dict(t=self.sim.now, what=what, **kw))

    def hour(self, t=None):
        t = self.sim.now if t is None else t
        return (t % DAY) / 60

    def in_business(self, t=None):
        return OPEN_H <= self.hour(t) < CLOSE_H

    def next_open(self, t):
        h = (t % DAY) / 60; d0 = t - t % DAY
        if h < OPEN_H:
            return d0 + OPEN_H * 60
        if h >= CLOSE_H:
            return d0 + DAY + OPEN_H * 60
        return t

    def add_business(self, t, minutes):
        t = self.next_open(t)
        while minutes > 0:
            d0 = t - t % DAY
            left = d0 + CLOSE_H * 60 - t
            if minutes < left:
                return t + minutes
            minutes -= left
            t = d0 + DAY + OPEN_H * 60
        return t

    def set_status(self, r, new):
        old = r["status"]
        if old in TERMINAL or new not in EDGES.get(old, set()):
            self.violations_inline.append(("illegal_status_change", f"request {r['id']}: {old} -> {new}"))
            return False
        r["status"] = new
        r["history"].append((self.sim.now, new))
        self.sim.audit_stream.append((self.sim.now, r["id"], new))     # shipped off the database as it happens
        return True

    def alert(self, level, kind, text, ref=None):
        a = dict(id=len(self.alerts) + 1, t=self.sim.now, level=level, kind=kind, text=text, ref=ref, done=False)
        self.alerts.append(a)
        self.sim.owner_notify(a)
        return a

    # ------------------------------------------------------------------ outbound texting (all goes through here)
    def text(self, phone, body, kind, ref=None, conversational=False, media=None, tries=1):
        H = self.fx("hardening")
        if self.replaying and self.replay_sent is not None and self.replay_sent.get((phone, kind, ref), 0) > 0:
            self.replay_sent[(phone, kind, ref)] -= 1                     # already sent before the restore: don't send twice
            self.log("replay_text_already_sent", kind=kind, request=ref)
            return "already_sent"
        if self.fx("v93") and ref and kind in STALE_IF_CLOSED:
            r_ = self.requests.get(ref)
            if r_ is None or r_["status"] in ("CANCELLED", "CLOSED", "CLOSED_NONE", "CLOSED_NO_QUOTES", "DUPLICATE", "EMERGENCY"):
                self.log("stale_text_dropped", kind=kind, request=ref)      # a held or retried text about a job that has since closed
                return None
        if H and conversational and kind not in ("safety", "stop_confirm", "pick_code"):
            # only a direct reply (they texted us in the last 15 minutes) may go out at night
            if self.sim.now - self.last_inbound.get(phone, -1e9) > 15:
                conversational = False
        if phone in self.stopped and kind not in ("stop_confirm",):
            self.log("text_suppressed_stop", phone=phone, kind=kind)
            return None
        if self.paused["texting"] and kind not in ("safety",):
            self.log("text_suppressed_pause", kind=kind)
            hid = self.phone_to_ho.get(phone)
            if self.cfg.get("fixes", {}).get("email_fallback") and hid and self.homeowners[hid].get("email"):
                self.sim.email.send(self.homeowners[hid]["email"], "Ask Hava update", body)
                self.sim.email_deliveries.append(dict(t=self.sim.now, phone=phone, kind=kind, ref=ref))
                self.sim.homeowners[phone].on_email(dict(kind=kind, body=body, meta=dict(ref=ref)))
            return None
        if kind not in ("safety", "stop_confirm", "pick_code") and not conversational and not self.in_business():
            # quiet hours: hold until 8 a.m.
            self.schedule_at(self.next_open(self.sim.now) + self.sim.rng.uniform(0, 10), "text", phone, body, kind, ref, False, media, tries)
            return None
        lang = "en"
        hid = self.phone_to_ho.get(phone)
        if hid and self.cfg.get("fixes", {}).get("spanish_templates") and self.homeowners[hid].get("lang") == "es":
            lang = "es" if (not self.fx("v93") or kind in SPANISH_KINDS) else "en"      # v9.3: only kinds with a real Spanish template
        sid = self.sim.sms.send(phone, body, kind, meta=dict(ref=ref, conversational=conversational, lang=lang), media=media)
        self.sent_log = getattr(self, "sent_log", []); self.sent_log.append((self.sim.now, phone))
        self.msg_meta[sid] = dict(kind=kind, ref=ref, phone=phone, body=body, media=media, tries=tries, conversational=conversational)
        return sid

    def on_sms_status(self, sid, status, err):
        m = self.msg_meta.get(sid)
        if not m:
            return
        if status in ("failed", "undelivered"):
            m["failed"] = m.get("failed", 0) + 1
            fx = self.cfg.get("fixes", {})
            critical = self.fx("v93") or m["kind"] in ("quote_link", "pick_confirm_request", "connect_contractor", "who_is_calling", "yes_request", "needs_card", "payment_failed", "reoffer", "status", "no_quotes_yet")
            if self.fx("v93") and m["tries"] >= 3 and m["phone"] in self.phone_to_ho and not self.paused["texting"]:
                hid = self.phone_to_ho[m["phone"]]
                key_ = m.get("ref") or (self.homeowners[hid].get("convo") or {}).get("started")
                if not self.homeowners[hid].get("email") and self.homeowners[hid].get("_undeliverable_alerted") != key_:
                    self.homeowners[hid]["_undeliverable_alerted"] = key_
                    self.alert("normal", "call_back", "Texts to a homeowner keep failing: call them", m["phone"])
            if fx.get("retry_critical_texts") and critical and m["tries"] < 3 and err not in ("21610", "campaign suspended"):
                m["tries"] += 1
                self.schedule_in(10 * m["tries"], "_resend", sid)
            if fx.get("email_fallback") and critical:
                hid = self.phone_to_ho.get(m["phone"])
                if hid and self.homeowners[hid].get("email"):
                    self.sim.email.send(self.homeowners[hid]["email"], "Ask Hava update", m["body"])
                    self.sim.email_deliveries.append(dict(t=self.sim.now, phone=m["phone"], kind=m["kind"], ref=m["ref"]))
                    self.sim.homeowners[m["phone"]].on_email(m)
            if m["kind"] in ("pick_confirm_request", "connect_contractor") and m["failed"] == 1:
                r = self.requests.get(m["ref"])
                self.alert("high", "delivery_failed", f"Text to contractor for request {m['ref']} failed ({err}); portal shows it, email sent", m["ref"])
                co = self.phone_to_co.get(m["phone"])
                if co:
                    self.sim.email.send(self.contractors[co]["email"], "Action needed", f"Request {m['ref']}")
            if err == "campaign suspended" and not self.paused["texting"]:
                self.paused["texting"] = True
                self.alert("critical", "texting_suspended", "Texting provider suspended the number: all texting paused, email + portal fallback on")

    def _resend(self, sid):
        m = self.msg_meta[sid]
        if self.fx("hardening"):
            # retries go through the normal texting rules (STOP, pause, quiet hours)
            return self.text(m["phone"], m["body"], m["kind"], m["ref"], m.get("conversational", False), m["media"], tries=m["tries"])
        nsid = self.sim.sms.send(m["phone"], m["body"], m["kind"], meta=dict(ref=m["ref"], conversational=True, retry=True), media=m["media"])
        self.msg_meta[nsid] = dict(m, tries=m["tries"])

    # ------------------------------------------------------------------ inbound texts
    def on_inbound_sms(self, frm, body, media, dup=False, sid=None):
        key = sid if (self.fx("hardening") and sid) else (frm, body, round(self.sim.now, 0))
        if key in self.processed_inbound:
            self.log("inbound_duplicate_ignored", phone=frm)
            return
        self.processed_inbound.add(key)
        self.last_inbound[frm] = self.sim.now
        b = body.strip()
        up = b.upper()
        if up in STOP_WORDS:
            self.stopped.add(frm)
            self.log("stop", phone=frm)
            self.text(frm, "You're unsubscribed from Ask Hava texts. Reply START to resume.", "stop_confirm", conversational=True)
            if frm in self.phone_to_co:
                co = self.contractors[self.phone_to_co[frm]]
                if all(p in self.stopped for p in co["phones"]):
                    co["status"] = "paused"; co["pause_reason"] = "STOP"
            elif self.fx("hardening") and frm in self.phone_to_ho:
                self.homeowner_stopped(frm)
            return
        if up in ("START", "UNSTOP"):
            self.stopped.discard(frm)
            self.optout_natural.discard(frm) if hasattr(self, "optout_natural") else None
            return
        if self.fx("v93") and up in ("HELP", "INFO"):
            self.text(frm, self.help_text(), "help", None, conversational=True)
            return
        if self.fx("v93") and NATURAL_OPTOUT.search(b):
            # FCC: people may revoke consent by any reasonable means, not only the exact keywords
            self.optout_natural = getattr(self, "optout_natural", set()); self.optout_natural.add(frm)
            self.text(frm, "Ask Hava: you won't get more texts from us. Reply START to resume.", "optout_confirm", None, conversational=True)
            self.stopped.add(frm)
            self.log("stop", phone=frm, natural=True)
            if frm in self.phone_to_co:
                co = self.contractors[self.phone_to_co[frm]]
                co["status"] = "paused"; co["pause_reason"] = "STOP"
            elif frm in self.phone_to_ho:
                self.homeowner_stopped(frm)
            return
        if frm in self.phone_to_co:
            return self.contractor_sms(self.phone_to_co[frm], b)
        return self.homeowner_sms(frm, b, media)

    # ------------------------------------------------------------------ homeowner side
    def homeowner_sms(self, phone, b, media):
        up = b.upper()
        hid = self.phone_to_ho.get(phone)
        if hid is None:
            hid = f"H{len(self.homeowners)+1}"
            cid = f"C{len(self.vault)+1}"
            self.vault[cid] = dict(phone=phone, name=None, addresses=[])
            self.homeowners[hid] = dict(id=hid, contact=cid, convo=None, lang="en", consented=False, requests=[])
            self.phone_to_ho[phone] = hid
        h = self.homeowners[hid]
        cv = h["convo"]
        H = self.fx("hardening")
        if self.fx("v93") and phone in self.stopped:
            # they opted out earlier, so the carrier blocks every text until they send START. Tell them another way.
            if self.emergency_words(b):
                self.alert("critical", "call_back", f"Opted-out number texted what may be an emergency: call now", phone)
            elif h.get("email"):
                self.sim.email.send(h["email"], "Ask Hava", "You turned off our texts earlier. Text START to turn them back on, then send your request again.")
                self.sim.email_deliveries.append(dict(t=self.sim.now, phone=phone, kind="opted_out_notice", ref=None))
                self.sim.homeowners[phone].on_email(dict(kind="opted_out_notice", body="", meta={}))
            elif self.sim.now - h.get("_optout_callback_at", -1e9) > DAY:
                h["_optout_callback_at"] = self.sim.now
                self.alert("normal", "call_back", "An opted-out homeowner texted a request: call them back", phone)
            return
        if H and self.emergency_words(b):
            # plain-code safety check on every message, before any stage logic and whether or not the AI is up
            if self.sim.now - h.get("_safety_at", -1e9) > 30:
                self.safety(h, phone); h["_safety_at"] = self.sim.now
            h["_em_at"] = self.sim.now
            if cv:
                cv["emergency_sent"] = True
        if H and cv and cv["stage"] == "consent":
            if re.match(r"^(OK|OKAY|YES|Y|SI|SÍ|SURE)\b", up):
                h["consented"] = True
                h["consent_record"] = dict(at=self.sim.now, reply=b, prompt_version="v9.3")
                return self.run_intake(h, phone)
            cv["text"] += " " + b                     # extra details: keep them, ask once more
            if not cv.get("reprompted"):
                cv["reprompted"] = True
                self.text(phone, "Got it. Reply OK to start (it's free; terms: askhava.example/terms).", "consent_again", conversational=True)
            return
        em_ = EMAIL_RE.search(b) if self.fx("v93") and len(b.split()) <= 10 else None
        if em_:
            h["email"] = em_.group(0)
            self.text(phone, "Got it. We'll email you too if a text doesn't arrive.", "email_ok", None, conversational=True)
            return
        # quick commands
        onsite_like = up == "ME" or (self.fx("v93") and (re.match(r"^ME\b", up) or re.search(r"\b(tenant|renter|will be (there|home)|be home|i'?ll be|meet (them|him|her)|(he|she)'?s ok|ok with it|agreed)\b", b, re.I))) or (len(b.split()) <= 6 and re.search(r"\b(agreed?|tenant|renter|ok|okay)\b", b, re.I)) or \
            (len(b.split()) <= 4 and re.match(r"^[A-Z][a-z]+\b", b) and up_cmd_free(up))
        if not self.fx("v93"):
            onsite_like = up == "ME" or re.match(r"^[A-Za-z]+\b.*(agree|tenant|renter|ok)", b, re.I)
        if cv is None and h["requests"] and self.fx("hardening") and onsite_like:
            asked = [self.requests[x] for x in h["requests"] if self.requests[x].get("onsite_asked") and not self.requests[x].get("onsite")
                     and (not self.fx("v93") or self.requests[x]["status"] not in TERMINAL)]
            if asked:
                asked[-1]["onsite"] = b; return
        if up == "LINK":
            for rid in reversed(h["requests"]):
                r = self.requests[rid]
                if r["status"] not in TERMINAL:
                    self.send_quote_link(r, conversational=True); return
            return
        if up == "LIST":
            for rid in reversed(h["requests"]):
                r = self.requests[rid]
                if r["status"] in ("QUOTES_READY", "AWAITING_YES"):
                    lines = [f"{i+1}. {self.contractors[self.quotes[q]['co']]['business']}: {self.quote_label(self.quotes[q])}" for i, q in enumerate(self.visible_quotes(r))]
                    self.text(phone, f"Quotes for request {rid}:\n" + "\n".join(lines) + "\nReply the number + YES (e.g. 1 YES) to choose.", "quote_list", rid, conversational=True)
                    r["list_mode"] = True
                    return
            return
        if self.fx("v93"):
            up_n = re.sub(r"\s+", " ", re.sub(r"[^A-Z0-9 ]", "", up).replace("PLEASE", "")).strip()
            m = re.match(r"^(\d{1,2}) ?(YES|SI)$", up_n)
            is_yes = up_n in ("YES", "SI") and (up_n == "YES" or any(self.requests[x]["status"] == "AWAITING_YES" for x in h["requests"]))
        else:
            m = re.match(r"^(\d{1,2})\s+YES$", up)
            is_yes = up == "YES"
        if is_yes or m:
            return self.homeowner_yes(h, int(m.group(1)) if m else None, phone)
        if up in ("DONE", "NO") or up.startswith("PROBLEM"):
            return self.homeowner_outcome(h, up, phone)
        if up.startswith("CANCEL") or (H and re.match(r"^CLOSE\b", up)):
            m2 = re.search(r"(\d{4})", up)
            for rid in reversed(h["requests"]):
                r = self.requests[rid]
                if r["status"] not in TERMINAL and (not m2 or m2.group(1) == rid):
                    self.cancel_request(r, "homeowner"); return
            return
        if H and "cancel" in b.lower():
            act = [rid for rid in h["requests"] if self.requests[rid]["status"] not in TERMINAL]
            if act and self.fx("v93"):
                self.cancel_request(self.requests[act[-1]], "homeowner")
                self.text(phone, f"Closed request {act[-1]}. Text us anytime to start a new one.", "closed_ok", act[-1], conversational=True)
                return
            if act:
                self.text(phone, f"To close request {act[-1]}, reply CLOSE {act[-1]}.", "cancel_help", act[-1], conversational=True)
                return
        if cv and cv["stage"] == "consent":
            if up in ("OK", "YES", "SI", "SÍ", "Y"):
                h["consented"] = True
                return self.run_intake(h, phone)
            return
        if cv and cv["stage"] == "zip":
            z = re.search(r"\b(86\d{3})\b", b)
            if z:
                cv["zip"] = z.group(1)
                return self.run_intake(h, phone)
            if H and not cv.get("zip_reasked"):
                cv["zip_reasked"] = True
                self.text(phone, "Sorry, I need the 5-digit ZIP code where the work is (e.g. 86403).", "intake_q", conversational=True)
            return
        if cv and cv["stage"] == "jt_q":
            return self.jt_answer(h, phone, cv, b)
        if cv and cv["stage"] == "trade_q":
            return self.trade_answer(h, phone, cv, b)
        if cv and cv["stage"] == "onsite":
            cv["onsite"] = b
            return self.run_intake(h, phone, onsite_done=True)
        if self.fx("v93") and cv is None and len(re.findall(r"[A-Za-z]{2,}", b)) < 3:
            # one- or two-word replies never open a new job
            self.text(phone, "Sorry, I didn't catch that. To get quotes for a new job, describe the problem in a few words.", "help", None, conversational=True)
            return
        # a new request
        truth = self.sim.truth_for(phone, b)          # opaque: only the fake AI uses it, to decide when to make a mistake
        h["convo"] = dict(stage="consent" if not h["consented"] else "new", text=b, media=media, _sim_truth=truth, started=self.sim.now,
                          zip=(re.search(r"\b(86\d{3})\b", b).group(1) if re.search(r"\b(86\d{3})\b", b) else None))
        if H and h.get("_em_at") == self.sim.now:
            h["convo"]["emergency_sent"] = True
        if not h["consented"]:
            if H and not self.sim.ai_intake.available():
                return self.text(phone, self.consent_text(), "consent", conversational=True)
            if self.cfg.get("fixes", {}).get("spanish_templates") and h["lang"] == "en":
                h["lang"] = self.sim.ai_intake.classify_intake(b, truth)["lang"]
            self.text(phone, self.consent_text(), "consent", conversational=True)
            # safety check happens even before consent
            ai = self.sim.ai_intake.classify_intake(b, truth)
            if self.cfg.get("fixes", {}).get("spanish_templates"):
                h["lang"] = ai["lang"]
            if self.cfg.get("fixes", {}).get("conservative_emergency") and any(w in b.lower() for w in ("smell", "burning", "alarm", "head hurts", "dizzy", "smoke", "hot outlet", "buzzing panel")):
                ai["emergency"] = True
            if ai["emergency"] and not h["convo"].get("emergency_sent"):
                self.safety(h, phone)
            return
        return self.run_intake(h, phone)

    @staticmethod
    def numbered_answer(b, options):
        """INT-012: a reply that starts with one of the question's option numbers, as that option, else None."""
        m1 = re.match(r"^\s*(\d)\b", b)
        return options[int(m1.group(1)) - 1] if m1 and 1 <= int(m1.group(1)) <= len(options) else None

    @staticmethod
    def trade_question(options):
        """INT-013 (D-104): the numbered trade question; the re-ask repeats it whole (D-103)."""
        listed = [f"{i + 1}) {TRADE_NAMES.get(t, t)}" for i, t in enumerate(options)]
        return f"Quick question so the right people see it: is this mainly {', '.join(listed[:-1])} or {listed[-1]}? Reply with the number."

    @staticmethod
    def jt_question(options):
        """INT-014: the numbered "Which fits best?" question; the re-ask repeats it whole (D-103)."""
        opts = " ".join(f"{i + 1}) {JT_NAMES.get(jt, jt)}" for i, jt in enumerate(options))
        return f"Which fits best? {opts}. Reply with the number."

    def trade_answer(self, h, phone, cv, b):
        """INT-013, INT-015 (D-103): a number, or whole words of exactly one option's keywords or label. One re-ask that
        repeats the options; after a second unusable reply (or "Not sure"), the keyword trade if exactly one trade matched,
        otherwise the first reading. The license-boundary job types stay either way (D-105, `boundary_types`)."""
        options, low = cv["trade_options"], b.lower()
        words = set(re.findall(r"[a-z]+", low))
        hits = [t for t in options if re.search(rf"\b(?:{TRADE_WORDS.get(t, t)})\b", low)
                or words & set(re.findall(r"[a-z]+", TRADE_NAMES.get(t, t).lower()))]
        pick_ = self.numbered_answer(b, options) or (hits[0] if len(hits) == 1 else None)
        if pick_ is None and not cv.get("trade_reasked") and not re.search(r"\bnot sure\b", low):
            cv["trade_reasked"] = True
            self.text(phone, "Sorry, I didn't catch that. " + self.trade_question(options), "intake_q", conversational=True)
            return
        if pick_ is None:
            kw = keyword_trades(cv["text"])
            pick_ = kw[0] if len(kw) == 1 else options[0]
        cv["trade_choice"] = pick_; cv["stage"] = "new"
        return self.run_intake(h, phone, onsite_done=cv.get("onsite_done", False))

    def jt_answer(self, h, phone, cv, b):
        """INT-014, INT-015 (D-103): a number, or the whole words of exactly one option's label. One re-ask that repeats
        the options; after a second unusable reply (or "Not sure") every candidate stays (conservative)."""
        options, low = cv["jt_options"], b.lower()
        words_ = [jt for jt in options if re.search(rf"\b{re.escape(JT_NAMES.get(jt, jt).lower())}\b", low)]
        choice = self.numbered_answer(b, options) or (words_[0] if len(words_) == 1 else None)
        if choice is None and not cv.get("jt_reasked") and not re.search(r"\bnot sure\b", low):
            cv["jt_reasked"] = True
            self.text(phone, "Sorry, I didn't catch that. " + self.jt_question(options), "intake_q", conversational=True)
            return
        cv["jt_choice"] = choice; cv["stage"] = "new"
        return self.run_intake(h, phone, onsite_done=cv.get("onsite_done", False))

    def trade_options(self, cv, ai):
        """INT-013, AI-011 (D-104): ask when the two readings disagree or the keyword trades leave out the reading's;
        offer every distinct trade of the two readings and the keyword rules, up to three."""
        kw, second = keyword_trades(cv["text"]), cv["ai2"].get("trade")
        if (not second or second == ai["trade"]) and (not kw or ai["trade"] in kw):
            return []
        return list(dict.fromkeys(t for t in (ai["trade"], second, *kw) if t))[:3]

    @staticmethod
    def jt_options(cv, cands):
        """INT-014 (D-102, D-105): ask when the candidates fall under different licenses, or when a trade answer left
        only `<trade>_other` for that trade, whose job types are then listed in its place."""
        broad = f"{cv.get('trade_choice')}_other"
        if broad in cands:
            return list(dict.fromkeys(jt for c in cands for jt in (JOB_TYPES[cv["trade_choice"]] if c == broad else [c])))[:9]
        if len(cands) > 1 and len({frozenset(k for k, v in CLASS_ALLOWS.items() if jt in v) for jt in cands}) > 1:
            return cands[:9]
        return []

    def consent_text(self):
        base = "Hi, this is Ask Hava. We gather quotes so you don't have to make the calls. It's free, and we'll never ask you for money or card details. Reply OK to start (terms: askhava.example/terms)."
        if self.fx("v93"):
            # carrier (CTIA / 10DLC) opt-in disclosures: brand, frequency, rates, STOP and HELP, terms and privacy
            base += " Msg frequency varies by request. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Privacy: askhava.example/privacy"
        return base

    def help_text(self):
        return "Ask Hava: we gather home-repair quotes. Help: help@askhava.example or (928) 555-0100, askhava.example/help. Reply STOP to opt out."

    def safety(self, h, phone):
        self.text(phone, "If you smell gas, see sparks or smoke, or water is near electricity: leave the area and call 911, or UniSource Energy / your utility now. We can't send a contractor for emergencies.", "safety", conversational=True)
        self.log("safety_message", homeowner=h["id"])
        if h["convo"]:
            h["convo"]["emergency_sent"] = True

    def run_intake(self, h, phone, onsite_done=False):
        cv = h["convo"]
        if not cv:
            return
        if not self.sim.ai_intake.available():
            if not (self.fx("hardening") and cv.get("hold_sent")):
                self.text(phone, "Thanks, we have your message and will get back to you shortly.", "status", conversational=True)
                cv["hold_sent"] = True
            self.schedule_in(30, "run_intake_id", h["id"], phone, onsite_done)
            return
        key_ = (phone, cv["text"])
        if self.fx("v93") and cv.get("ai"):
            ai = dict(cv["ai"])                                   # already read; don't re-read on every step
        elif self.fx("v93") and key_ in self.sim.intake_log:
            ai = dict(self.sim.intake_log[key_]["ai"]); cv["ai"] = dict(ai)          # replay: reuse the reading made the first time
            if self.sim.intake_log[key_].get("ai2"):
                cv["ai2"] = dict(self.sim.intake_log[key_]["ai2"])
        else:
            ai = self.sim.ai_intake.classify_intake(cv["text"], cv["_sim_truth"])
            cv["ai"] = dict(ai)
        if self.fx("v93") and not ai.get("emergency"):
            if "ai2" not in cv:
                cv["ai2"] = self.sim.ai_intake.classify_intake(cv["text"], cv["_sim_truth"])      # second, independent reading
            # the structured readings (trade, job type, flags; no personal details) go to the off-database log
            self.sim.intake_log[key_] = dict(ai={k: v for k, v in cv["ai"].items() if k != "tool_request"}, ai2={k: v for k, v in cv["ai2"].items() if k != "tool_request"})
            if cv.get("trade_choice"):
                ai["trade"] = cv["trade_choice"]
            # a second opinion that doesn't come from the AI: plain keywords for each trade
            options = [] if cv.get("trade_choice") else self.trade_options(cv, ai)
            if options:
                # the readings disagree on the trade: ask instead of guessing
                cv["stage"] = "trade_q"; cv["trade_options"] = options
                self.text(phone, self.trade_question(options), "intake_q", conversational=True)
                return
        if self.cfg.get("fixes", {}).get("conservative_emergency") and any(w in cv["text"].lower() for w in ("smell", "burning", "alarm", "head hurts", "dizzy", "smoke", "hot outlet", "buzzing panel")):
            ai["emergency"] = True
        if self.fx("v93") and not ai["emergency"] and not cv.get("emergency_sent") and "jt_choice" not in cv:
            # if the possible job types fall under different licenses, ask the homeowner which fits (they know their job)
            options = self.jt_options(cv, self.job_types_for(cv, ai))
            if options:
                cv["stage"] = "jt_q"; cv["jt_options"] = options
                self.text(phone, self.jt_question(options), "intake_q", conversational=True)
                return
        if cv.get("jt_choice"):
            ai["trade"] = JT_TRADE.get(cv["jt_choice"], ai["trade"])         # the job type the homeowner chose names its trade, so its licenses get the card (D-105, CARD-001)
        if ai.get("tool_request"):
            self.log("ai_tool_request_ignored", request=ai["tool_request"])     # never acted on
        if ai["emergency"] or cv.get("emergency_sent"):
            if not cv.get("emergency_sent"):
                self.safety(h, phone)
            r = self.new_request(h, cv, ai, status="EMERGENCY")
            h["convo"] = None
            return
        if not cv.get("zip"):
            cv["stage"] = "zip"
            self.text(phone, "What's the ZIP code where the work is?", "intake_q", conversational=True)
            return
        if ai.get("rental") and not onsite_done and self.cfg["ask_onsite_contact"]:
            cv["stage"] = "onsite"
            self.text(phone, "Who will meet the contractor, and have they agreed to be contacted? Reply with their first name, or ME.", "intake_q", conversational=True)
            return
        town = ZIP_TOWN.get(cv["zip"])
        r = self.new_request(h, cv, ai, status="INTAKE")
        h["convo"] = None
        if not town:
            self.set_status(r, "INELIGIBLE")
            self.text(phone, "Sorry, we only cover Lake Havasu City, Bullhead City, Fort Mohave and Kingman right now.", "ineligible", r["id"], conversational=True)
            return
        if not self.area_open(town, r["trade"]):
            self.set_status(r, "INELIGIBLE")
            self.text(phone, f"Sorry, we don't have {r['trade']} contractors in {town} yet. We'll text you if that changes.", "ineligible", r["id"], conversational=True)
            return
        # duplicate check: same phone + trade + property, still open
        for rid in h["requests"][:-1]:
            o = self.requests[rid]
            same_job = o["trade"] == r["trade"] or (self.cfg.get("fixes", {}).get("dup_ignore_trade_2h") and self.sim.now - o["created"] < 120)
            if self.fx("v93") and same_job and self.sim.now - o["created"] >= 120:
                # past 2 hours, the same trade is only the same job if the words mostly match (a different leak is a new job)
                wa = set(re.findall(r"[a-z]{3,}", o["original_text"].lower())); wb = set(re.findall(r"[a-z]{3,}", r["original_text"].lower()))
                same_job = len(wa & wb) / max(1, min(len(wa), len(wb))) >= 0.6
            resent = self.fx("v93") and self.sim.now - o["created"] < 120 and o["original_text"] == r["original_text"]     # the same text sent twice
            if (same_job and o["zip"] == r["zip"] and o["status"] not in TERMINAL) or resent:
                self.set_status(r, "DUPLICATE")
                r["merged_into"] = rid
                self.text(phone, f"Looks like this is the same job as request {rid}, so we've added it there.", "duplicate", rid, conversational=True)
                return
        self.set_status(r, "OPEN")
        self.send_job_cards(r)

    def area_open(self, town, trade):
        if self.paused["all"] or town in self.paused["towns"] or trade in self.paused["trades"]:
            return False
        open_trades = self.cfg["open_trades"].get(town, [])
        if trade not in open_trades and trade != "general_licensed":
            return False
        return len(self.eligible(town, trade, check=False)) >= self.cfg["min_contractors_to_open"]

    def new_request(self, h, cv, ai, status):
        self.req_seq += 1
        rid = str(self.req_seq)
        if self.replaying or (getattr(self, "replay_rid_hints", None) and self.sim.now - getattr(self, "replay_ended", -1e9) < DAY):
            phone = self.vault[h["contact"]]["phone"]
            hints = self.replay_rid_hints.get(phone, [])
            if hints:
                rid = hints.pop(0); self.req_seq = max(self.req_seq - 1, int(rid))
            elif self.replaying:
                self.req_seq = max(self.req_seq, self.replay_max_rid + 1); rid = str(self.req_seq)
        desc = cv["text"]
        scrubbed = self.scrub(desc)
        if self.cfg.get("fixes", {}).get("ai_scrub"):
            scrubbed = self.sim.ai_intake.ai_scrub(scrubbed)
        if self.fx("v93"):
            scrubbed = self.final_text_gate(scrubbed, ai["trade"])
        photos = []
        for p in cv.get("media", []):
            p2 = dict(p); p2["gps"] = False                                 # plain code strips location data
            flags = self.sim.ai_photo.screen_photo(p)
            if self.cfg.get("fixes", {}).get("double_photo_screen") and not flags:
                flags = self.sim.ai_photo.screen_photo(p)
            if self.fx("v93") and not flags:
                flags = self.sim.local_screen.screen(p)          # OCR + face detector on our own server
            p2["screened_ok"] = not flags
            photos.append(dict(raw=p, card_version=p2 if not flags else None))
        r = dict(id=rid, ho=h["id"], status=status, history=[(self.sim.now, status)], trade=ai["trade"], urgent=ai["urgent"],
                 zip=cv.get("zip"), town=ZIP_TOWN.get(cv.get("zip") or ""), desc=scrubbed, photos=photos, created=self.sim.now,
                 cards=[], quotes=[], pick=None, picks=[], _sim_truth=cv["_sim_truth"], onsite=cv.get("onsite"), lang=ai["lang"],
                 major=ai.get("major", False), first_status_sent=None, status_msgs=[], original_text=cv["text"],
                 job_types=self.job_types_for(cv, ai))
        r["fee"] = MAJOR_FEE if r["major"] else FEES.get(r["trade"], 45)
        self.requests[rid] = r
        h["requests"].append(rid)
        self.log("request_created", request=rid, status=status)
        return r

    def job_types_for(self, cv, ai):
        if cv.get("jt_choice"):
            return [cv["jt_choice"]]
        types = self._job_types_for(cv, ai)
        specific = {JT_TRADE.get(t_) for t_ in types if not t_.endswith("_other")}
        # an 'other' reading only means 'not sure'; a specific reading of the same trade wins
        return [t_ for t_ in types if not (t_.endswith("_other") and JT_TRADE.get(t_) in specific)]

    def _job_types_for(self, cv, ai):
        """v9.3: the job type is read twice, independently. If the two readings disagree, or either is unsure,
        the job carries every candidate type, so only licenses covering all of them get the card."""
        if not ai.get("job_type"):
            return []
        types = {ai["job_type"]}
        low, boundary = cv["text"].lower(), boundary_types(cv["text"]) if self.fx("v93") else set()
        if self.fx("v93"):
            # plain-code guardrails for job types where a license boundary matters (added whatever the AI says)
            types |= {jt for pat, jt in KEYWORD_TYPES if re.search(pat, low) and jt in JOB_TYPES.get(ai["trade"], [])}
        if ai.get("job_type_conf", 1) < 0.8 and ai.get("alt_job_type"):
            types.add(ai["alt_job_type"])
        if self.fx("v93"):
            ai2 = cv.get("ai2") or self.sim.ai_intake.classify_intake(cv["text"], cv["_sim_truth"])
            if cv.get("trade_choice"):
                # the homeowner chose the trade: keep the readings and plain-keyword job types of that trade (D-102),
                # otherwise the broad type for it, whose job types "Which fits best?" then lists (jt_options)
                chosen = cv["trade_choice"]
                types = {rd["job_type"] for rd in (cv.get("ai") or {}, ai2) if rd.get("trade") == chosen and rd.get("job_type")}
                types |= {jt for pat, jt in KEYWORD_TYPES if re.search(pat, low) and JT_TRADE[jt] == chosen}
                types = types or {f"{chosen}_other"}
            if ai2.get("job_type") and ai2.get("trade") == ai["trade"]:
                types.add(ai2["job_type"])
                if ai2.get("job_type_conf", 1) < 0.8 and ai2.get("alt_job_type"):
                    types.add(ai2["alt_job_type"])
        return sorted(types | boundary)               # license-boundary keyword types stay whatever the trade (D-105)

    def final_text_gate(self, text, trade):
        """v9.3: last plain-code pass (no AI). Anything that still looks like a name, contact phrase or a run of
        number words removes the rest of that sentence. If little is left, the card uses a generic line."""
        t = re.sub(r"\b(ask for|my name is|this is|i'm|i am|it's)\s+[A-Z][a-z]+\d*\b[^.!?]*", "", text)
        t = re.sub(r"\b(my number|my cell|call me|text me|reach me|contact me|phone)\b[^.!?]*", "", t, flags=re.I)
        t = re.sub(r"[^.!?]*\b(zero|one|two|three|four|five|six|seven|eight|nine|oh)(\W+(zero|one|two|three|four|five|six|seven|eight|nine|oh)){2,}\b[^.!?]*", "", t, flags=re.I)
        t = re.sub(r"(\d\D{0,2}){7,}", "", t)                                      # 7+ digits in any spacing
        t = re.sub(r"\s{2,}", " ", t).strip(" ,;")
        if len(t) < 12:
            t = f"{trade.title()} job (details on the job page)"
        return t

    def scrub(self, text):
        t = PHONE_RE.sub("[phone removed]", text)
        t = EMAIL_RE.sub("[email removed]", t)
        t = ADDR_RE.sub("[address removed]", t)
        return t

    # ------------------------------------------------------------------ contractors: eligibility and job cards
    def license_ok(self, co):
        lc = co.get("license_checked_at")
        if lc is None or co.get("license_status") != "ACTIVE":
            return False
        if self.fx("v93"):
            for k in ("license_expires", "bond_cancel"):          # dates shown on the state record
                if co.get(k) is not None and self.sim.now >= co[k]:
                    return False
        return self.sim.now - lc <= self.cfg["license_recheck_days"] * DAY

    def matches(self, c, r):
        """v9.3: the license class allows at least one candidate job type, and the business opted in to it."""
        types = r.get("job_types") or []
        allowed = CLASS_ALLOWS.get(c.get("license_class") or "", set())
        # the license must cover EVERY job type the AI thinks it could be; the business must want at least one of them
        return bool(types) and all(jt in allowed for jt in types) and any(jt in c["checklist"] for jt in types)

    def eligible(self, town, trade, check=True):
        out = []
        for c in self.contractors.values():
            if c["status"] != "active":
                continue
            if trade == "general_licensed":
                if c["trade"] == "handyman":
                    continue
            elif c["trade"] != trade:
                continue
            if town not in c["towns"]:
                continue
            if check and not self.license_ok(c):
                continue
            if self.fx("v93") and self.sim.now < c.get("welcome_at", 0):
                continue                                          # no job cards before the welcome text (with STOP/HELP) has gone out
            out.append(c)
        return out

    def send_job_cards(self, r, widen=False):
        pool = self.eligible(r["town"], r["trade"])
        if self.fx("v93") and self.cfg.get("job_matching", True):
            full = pool
            pool = [c for c in full if self.matches(c, r)]
            if not pool and not widen and r.get("job_types"):
                # nobody's checklist covers it: offer it to licensed businesses that allow 'jobs nobody else covers'
                primary = r["job_types"]
                pool = [c for c in full if c.get("overflow", True) and all(jt in CLASS_ALLOWS.get(c.get("license_class") or "", set()) for jt in primary)]
                if pool:
                    r["overflow_sent"] = True
        if widen:
            pool = [c for c in self.contractors.values() if c["status"] == "active" and c["trade"] == r["trade"] and self.license_ok(c)
                    and r["town"] not in c["towns"] and r["town"] in c.get("neighbor_towns", [])]
            if self.fx("v93") and self.cfg.get("job_matching", True):
                pool = [c for c in pool if self.matches(c, r)]
        pool = [c for c in pool if c["id"] not in [self.cards[k]["co"] for k in r["cards"]]]
        pool.sort(key=lambda c: (c["cards_sent"], self.sim.rng.random()))
        if not self.fx("v93"):
            pool = pool[: self.cfg["max_cards_per_request"]]      # v9.3: every qualified business that opted in gets the card
        if not pool and not widen:
            self.log("no_eligible_contractors", request=r["id"])
            if self.fx("v93") and not any(self.contractors[self.cards[k]["co"]] for k in r["cards"]):
                # no licensed business in town covers this kind of job: say so now instead of waiting a day
                self.set_status(r, "NOT_COVERED")
                self.demand_log = getattr(self, "demand_log", []) + [(self.sim.now, r["trade"], tuple(r.get("job_types") or []))]
                self.text(self.phone_of(r), f"Sorry, none of our local businesses cover this kind of {r['trade']} work yet (request {r['id']}). We've noted it and will text you if that changes.", "not_covered", r["id"], conversational=True)
                return
        if self.fx("v93"):
            # a fresh state-record check (if the last one is over 2 hours old) for each contractor about to get a card
            for c in list(pool):
                if c["trade"] != "handyman" and self.sim.now - (c.get("license_checked_at") or -1e9) > 12 * 60:
                    self.check_license(c["id"])
            pool = [c for c in pool if c["status"] == "active" and self.license_ok(c)]
        for c in pool:
            kid = self.stable_id("K", r["id"], c["id"]) if self.fx("v93") else self.new_id("K", len(self.cards))
            card_photos = [p["card_version"] for p in r["photos"] if p["card_version"]]
            self.cards[kid] = dict(id=kid, req=r["id"], co=c["id"], at=self.sim.now, photos=card_photos, desc=r["desc"])
            r["cards"].append(kid)
            c["cards_sent"] += 1
            self.text(c["phones"][0], f"New {r['trade']} job {r['id']} in {r['town']} {r['zip']}{' (URGENT)' if r['urgent'] else ''}: {r['desc'][:120]} Quote: askhava.example/j/{kid}", "job_card", r["id"], media=card_photos)
        if not widen:
            start = self.next_open(self.sim.now)
            self.schedule_at(self.add_business(start, 60), "t_nudge", r["id"])
            self.schedule_at(self.add_business(start, 120), "t_widen", r["id"])
            r["cmp_deadline"] = self.add_business(start, 240)
            self.schedule_at(r["cmp_deadline"], "t_compare", r["id"])
            d0 = r["created"] - r["created"] % DAY
            # v9.3: the 'no quotes yet' check runs at 6 p.m., leaving 3 hours for retries and email before the 9 p.m. promise
            self.schedule_at(d0 + DAY + (18 * 60 if self.fx("v93") else CLOSE_H * 60 - 1), "t_promise", r["id"])
            n = len(pool)
            ask_email = self.fx("v93") and not self.homeowners[r["ho"]].get("email")
            self.text(self.phone_of(r), f"{n} local {r['trade']} contractor{'s have' if n != 1 else ' has'} your job. Quotes usually start within a few hours, 8 a.m.-9 p.m. Request {r['id']}." + (" Want updates by email too, in case a text doesn't arrive? Reply with your email." if ask_email else ""), "status", r["id"], conversational=True)
            r["first_status_sent"] = self.sim.now

    def phone_of(self, r):
        return self.vault[self.homeowners[r["ho"]]["contact"]]["phone"]

    def t_nudge(self, rid):
        r = self.requests[rid]
        if r["status"] == "OPEN" and not r["quotes"]:
            for k in r["cards"]:
                self.text(self.contractors[self.cards[k]["co"]]["phones"][0], f"Reminder: job {rid} is waiting for quotes.", "nudge", rid)

    def t_widen(self, rid):
        r = self.requests[rid]
        if r["status"] == "OPEN" and not r["quotes"]:
            self.send_job_cards(r, widen=True)

    def t_compare(self, rid):
        r = self.requests[rid]
        if r["status"] in ("OPEN", "NO_QUOTES_YET") and r["quotes"]:
            self.quotes_ready(r)

    def t_promise(self, rid):
        r = self.requests[rid]
        if r["status"] == "OPEN" and not r["quotes"]:
            self.set_status(r, "NO_QUOTES_YET")
            self.text(self.phone_of(r), f"No quotes yet for request {rid}. We're still asking local contractors and will text you as soon as one comes in. " + (f"Reply CLOSE {rid} to close it." if self.fx("hardening") else "Reply CANCEL to stop."), "no_quotes_yet", rid)
            self.schedule_in(5 * DAY, "t_close_no_quotes", rid)

    def t_close_no_quotes(self, rid):
        r = self.requests[rid]
        if r["status"] == "NO_QUOTES_YET":
            self.set_status(r, "CLOSED_NO_QUOTES")

    # ------------------------------------------------------------------ contractor actions
    def contractor_sms(self, coid, b):
        c = self.contractors[coid]
        up = b.upper()
        m0 = re.match(r"^LINK\s+(\d{4})$", up)
        if m0 and self.fx("v93") and m0.group(1) in c.get("pending_links", {}):
            ph = c["pending_links"].pop(m0.group(1))
            c["phones"].append(ph); self.phone_to_co[ph] = coid
            for p in self.picks.values():
                if p["co"] == coid and p.get("session"):
                    self.sim.voice.add_phone(p["session"], ph)
            self.text(c["phones"][0], f"Linked. Calls from the phone ending {m0.group(1)} now go through.", "link_done", None, conversational=True)
            return
        if up == "PAUSE":
            c["status"] = "paused"; c["pause_reason"] = "self"; return
        if up == "RESUME":
            if c.get("pause_reason") == "self":
                c["status"] = "active"
            return
        m = re.match(r"^CONFIRM\s+(\d+)$", up)
        if m:
            return self.contractor_confirm(coid, m.group(1), via="sms")
        m = re.match(r"^(\d{4})\s+OK\s+(FIRM|RANGE|TRIP)$", up)
        if m and (coid, m.group(1)) in getattr(self, "pending_quote_text", {}):
            text, p = self.pending_quote_text.pop((coid, m.group(1)))
            r = self.requests[m.group(1)]
            return self.add_quote(r, coid, dict(type=m.group(2).lower(), amount=p["amount"], high=p["high"], original=text, parsed_by_ai=True, confirmed_by_contractor=True))
        m = re.match(r"^(\d{4})\s+(.+)$", b)
        if m and m.group(1) in self.requests:
            return self.contractor_quote_text(coid, m.group(1), m.group(2))
        if "STOP" in up:
            self.log("possible_stop_intent", co=coid, text=b)      # natural-language 'stop' is not an opt-out keyword
            self.text(c["phones"][0], "To stop all texts reply STOP by itself. To pause job cards reply PAUSE.", "help", conversational=True)
            return
        self.text(c["phones"][0], "Reply with the job number and your quote, e.g. '1042 $150 Thu AM', or use the link.", "help", conversational=True)

    def contractor_quote_text(self, coid, rid, text):
        r = self.requests[rid]
        kid = next((k for k in r["cards"] if self.cards[k]["co"] == coid), None)
        if not kid:
            return
        if PHONE_RE.search(text) or "http" in text.lower() or "www" in text.lower() or (self.fx("hardening") and SPELLED_CONTACT_RE.search(text)):
            self.text(self.contractors[coid]["phones"][0], f"Please resend your quote for {rid} without phone numbers or links. The homeowner can call you after picking you.", "bounce", rid, conversational=True)
            self.log("quote_bounced_contact_info", request=rid, co=coid)
            return
        if not self.sim.ai_parse.available():
            self.schedule_in(30, "contractor_quote_text", coid, rid, text); return
        p = self.sim.ai_parse.parse_quote(text)
        if self.handyman_over_cap(coid, p.get("amount"), p.get("high")):
            return
        if p["confidence"] < 0.8:
            self.text(self.contractors[coid]["phones"][0], f"Is this right for {rid}? '{text}'. Reply {rid} OK FIRM, {rid} OK RANGE or {rid} OK TRIP, or use the link.", "quote_check", rid, conversational=True)
            self.pending_quote_text = getattr(self, "pending_quote_text", {})
            self.pending_quote_text[(coid, rid)] = (text, p)
            return
        self.add_quote(r, coid, dict(type=p["type"], amount=p["amount"], high=p["high"], original=text, parsed_by_ai=True, ai_conf=p["confidence"]))

    def contractor_quote_portal(self, coid, rid, fields):
        self.web("contractor_quote_portal", coid, rid, fields)
        r = self.requests.get(rid)
        if r is None:
            self.log("quote_for_unknown_job", co=coid, request=rid)
            if self.replaying:
                self.restore_lost.add(rid)
            return "unknown_job"
        if not any(self.cards[k]["co"] == coid for k in r["cards"]):
            self.violations_inline.append(("quote_without_card", f"{coid} quoted {rid} without a job card")); return
        if PHONE_RE.search(fields.get("original", "")):
            self.log("quote_bounced_contact_info", request=rid, co=coid)
            return "bounced"
        if self.handyman_over_cap(coid, fields.get("amount"), fields.get("high")):
            return "over_cap"
        self.add_quote(r, coid, dict(fields, parsed_by_ai=False))
        return "ok"

    def handyman_over_cap(self, coid, amount, high):
        c = self.contractors[coid]
        if self.fx("hardening") and c["trade"] == "handyman" and max(amount or 0, high or 0) >= 1000:
            self.text(c["phones"][0], "Handyman quotes must be under $1,000 total (Arizona handyman exemption). For bigger jobs a licensed contractor is needed.", "bounce_cap", None, conversational=True)
            self.log("handyman_quote_blocked", co=coid)
            return True
        return False

    def add_quote(self, r, coid, q):
        if r["status"] in TERMINAL or r["status"] in ("AWAITING_CONFIRM", "PAYMENT_PENDING", "CONNECTED"):
            self.log("late_quote", request=r["id"], co=coid)
            return
        if any(self.quotes[x]["co"] == coid for x in r["quotes"]):
            return
        qid = self.stable_id("Q", r["id"], coid) if self.fx("v93") else self.new_id("Q", len(self.quotes))
        self.quotes[qid] = dict(id=qid, req=r["id"], co=coid, at=self.sim.now, expires=self.sim.now + 3 * DAY, **q)
        r["quotes"].append(qid)
        self.contractors[coid]["quotes"] += 1
        if r["status"] in ("OPEN", "NO_QUOTES_YET"):
            if len(r["quotes"]) >= 3 or self.sim.now >= r.get("cmp_deadline", 1e18):
                self.quotes_ready(r)
        elif r["status"] in ("QUOTES_READY", "AWAITING_YES"):
            self.text(self.phone_of(r), f"Another quote was added to request {r['id']}: askhava.example/q/{r['token']}", "more_quotes", r["id"])

    def quote_label(self, q):
        if q["type"] == "trip":
            return f"TRIP FEE ${q['amount']} + estimate on site (not a repair price)"
        if q["type"] == "range":
            return f"PRICE RANGE ${q['amount']}-{q.get('high')}"
        return f"FIRM PRICE ${q['amount']}"

    def visible_quotes(self, r):
        out = [q for q in r["quotes"] if self.quotes[q]["expires"] > self.sim.now and q not in r.get("rejected_quotes", [])]
        if self.fx("v93"):
            # a quote from a contractor who is paused or whose license isn't currently verified isn't shown
            out = [q for q in out if self.contractors[self.quotes[q]["co"]]["status"] == "active" and self.license_ok(self.contractors[self.quotes[q]["co"]])]
        return out

    def quotes_ready(self, r):
        if r["status"] == "NO_QUOTES_YET" or r["status"] == "OPEN":
            self.set_status(r, "QUOTES_READY")
            r["token"] = self.token_for(r["id"])
            self.view_tokens[r["token"]] = r["id"]
            self.send_quote_link(r)
            self.schedule_in(3 * DAY, "t_quotes_stale", r["id"])

    def send_quote_link(self, r, conversational=False):
        self.text(self.phone_of(r), f"Your quotes for request {r['id']} are ready ({len(r['quotes'])} so far): askhava.example/q/{r['token']} Reply LIST to get them by text.", "quote_link", r["id"], conversational=conversational)

    def t_quotes_stale(self, rid):
        r = self.requests[rid]
        if r["status"] == "QUOTES_READY":
            self.set_status(r, "CLOSED_NONE")

    # ------------------------------------------------------------------ choosing (needs the homeowner's phone)
    def page_set_email(self, token, email):
        """Optional email box on the quote page (used if texting fails)."""
        self.web("page_set_email", token, email)
        rid = self.resolve_token(token)
        if rid and email:
            self.homeowners[self.requests[rid]["ho"]]["email"] = email

    def page_choose(self, token, qid):
        """Someone tapped Choose on a quote page. The page alone can't pick: the homeowner's phone must confirm."""
        self.web("page_choose", token, qid)
        rid = self.resolve_token(token)
        if not rid:
            return
        r = self.requests[rid]
        if r["status"] != "QUOTES_READY" or qid not in self.visible_quotes(r):
            return
        if self.fx("page_code"):
            # a one-time code goes to the homeowner's phone; only someone with the page AND the phone can finish
            if self.fx("v93"):
                r["pending_code"] = dict(code=None, qid=qid, at=self.sim.now, tries=0)
                if self.replaying:
                    return "code_sent"                              # the code was already sent before the restore
                code = self.sim.verify.start(self.phone_of(r), rid)
            else:
                self.sim.costs.add("otp", 0.05)
                code = f"{self.sim.rng.randrange(10**6):06d}"
                r["pending_code"] = dict(code=code, qid=qid, at=self.sim.now, tries=0)
            self.text(self.phone_of(r), f"Your Ask Hava code is {code}. Enter it on the quote page to choose. Don't share it.", "pick_code", rid)
            return "code_sent"
        r["pending_choice"] = dict(qid=qid, at=self.sim.now)
        self.set_status(r, "AWAITING_YES")
        co = self.contractors[self.quotes[qid]["co"]]
        self.text(self.phone_of(r), f"Reply YES to choose {co['business']} for request {rid}. We'll then ask them to confirm.", "yes_request", rid, conversational=True)
        self.schedule_in(24 * 60, "t_yes_timeout", rid, qid)

    def page_enter_code(self, token, code):
        self.web("page_enter_code", token, code)
        rid = self.resolve_token(token)
        r = self.requests.get(rid) if rid else None
        if not r:
            return "bad_link"
        pc = r.get("pending_code")
        if self.fx("v93"):
            if not pc or r["status"] != "QUOTES_READY":
                return "expired"
            if self.replaying:
                ok = bool(self.sim.verify.approved_since(self.phone_of(r), rid, self.replay_since))
            else:
                ok = self.sim.verify.check(self.phone_of(r), rid, code)
            if not ok:
                pc["tries"] += 1
                if pc["tries"] >= 5:
                    r["pending_code"] = None
                return "wrong"
            pc = dict(pc, code=code)
        if not pc or self.sim.now - pc["at"] > 15 or r["status"] != "QUOTES_READY":
            return "expired"
        if code != pc["code"]:
            pc["tries"] += 1
            if pc["tries"] >= 5:
                r["pending_code"] = None
            return "wrong"
        r["pending_code"] = None
        if pc["qid"] not in self.visible_quotes(r):
            return "expired"
        r["pending_choice"] = dict(qid=pc["qid"], at=self.sim.now)
        self.set_status(r, "AWAITING_YES")
        self.start_pick(r, pc["qid"], self.phone_of(r), auth="code")
        return "ok"

    def t_yes_timeout(self, rid, qid):
        r = self.requests[rid]
        if r["status"] == "AWAITING_YES" and r.get("pending_choice", {}).get("qid") == qid:
            self.set_status(r, "QUOTES_READY")
            r["pending_choice"] = None

    def homeowner_yes(self, h, n, phone):
        for rid in reversed(h["requests"]):
            r = self.requests[rid]
            if r["status"] == "AWAITING_YES" and n is None:
                return self.start_pick(r, r["pending_choice"]["qid"], phone)
            if r["status"] in ("QUOTES_READY", "AWAITING_YES") and n is not None and r.get("list_mode"):
                vq = self.visible_quotes(r)
                if 1 <= n <= len(vq):
                    if r["status"] == "QUOTES_READY":
                        self.set_status(r, "AWAITING_YES")
                    return self.start_pick(r, vq[n - 1], phone)
        self.log("yes_without_pending", homeowner=h["id"])

    def start_pick(self, r, qid, phone, auth="yes_text"):
        q = self.quotes[qid]
        if self.cfg.get("fixes", {}).get("license_check_at_pick"):
            self.check_license(q["co"])
        c0 = self.contractors[q["co"]]
        if self.fx("hardening") and (c0["status"] != "active" or not self.license_ok(c0)):
            # the contractor can't take jobs right now: don't start the pick
            r.setdefault("rejected_quotes", []).append(qid)
            rest = self.visible_quotes(r)
            self.set_status(r, "QUOTES_READY" if rest else "CLOSED_NONE")
            self.text(self.phone_of(r), f"Sorry, {c0['business']} can't take new jobs right now." + (f" Please choose another quote: askhava.example/q/{r['token']}" if rest else " Text us anytime to try again."), "reoffer" if rest else "reoffer_none", r["id"], conversational=True)
            return
        if self.fx("hardening"):
            # $149 tier only when the AI flagged a major job AND the chosen quote is $1,500+, or the quote itself is $3,000+
            top = (q.get("amount") or 0) if q.get("type") == "firm" else max(q.get("amount") or 0, q.get("high") or 0)
            r["fee"] = MAJOR_FEE if ((r["major"] and top >= 1500) or top >= 3000) else FEES.get(r["trade"], 45)
            if r["fee"] != MAJOR_FEE and r["major"]:
                self.log("major_fee_not_applied", request=r["id"], quote=top)
        n_prev = sum(1 for x in self.picks.values() if x["req"] == r["id"] and x["co"] == q["co"])
        pid = self.stable_id("P", r["id"], q["co"], n_prev) if self.fx("v93") else self.new_id("P", len(self.picks))
        deadline = self.add_business(self.sim.now, self.cfg["confirm_window_business_min"])
        self.picks[pid] = dict(id=pid, req=r["id"], qid=qid, co=q["co"], yes_from=phone, yes_at=self.sim.now, confirm_deadline=deadline,
                               status="awaiting_confirm", charge=None, released=False, fee=r["fee"], free=False, auth=auth)
        r["pick"] = pid; r["picks"].append(pid)
        self.set_status(r, "AWAITING_CONFIRM")
        if self.fx("hardening") and not r.get("onsite"):
            r["onsite_asked"] = True
            self.text(self.phone_of(r), f"Will you be the one meeting the contractor for request {r['id']}? Reply ME, or the first name of who will (they must agree to be called).", "onsite_q", r["id"], conversational=True)
        c = self.contractors[q["co"]]
        self.text(c["phones"][0], f"You were picked for job {r['id']} (${r['fee']}{', FREE first win' if not c['first_win_used'] else ''}). Tap Confirm within 2 business hours: askhava.example/c/{pid} or reply CONFIRM {r['id']}.", "pick_confirm_request", r["id"])
        self.schedule_at(deadline, "t_confirm_deadline", pid)

    def t_confirm_deadline(self, pid):
        p = self.picks[pid]
        if p["status"] == "awaiting_confirm":
            p["status"] = "expired"
            self.reoffer(self.requests[p["req"]], p, "no_confirm")

    def reoffer(self, r, p, reason):
        r.setdefault("rejected_quotes", []).append(p["qid"])
        rest = self.visible_quotes(r)
        if r["status"] in TERMINAL:
            return
        if rest:
            self.set_status(r, "QUOTES_READY")
            self.text(self.phone_of(r), f"{self.contractors[p['co']]['business']} couldn't take request {r['id']}. You can choose another quote: askhava.example/q/{r['token']}", "reoffer", r["id"])
        else:
            self.set_status(r, "CLOSED_NONE")
            self.text(self.phone_of(r), f"Sorry, the contractor couldn't take request {r['id']} and there are no other quotes. Text us anytime to try again.", "reoffer_none", r["id"])

    def contractor_confirm(self, coid, rid, via="portal"):
        if via == "portal":
            self.web("contractor_confirm", coid, rid, via)
        r = self.requests.get(rid)
        if not r or not r.get("pick"):
            return
        p = self.picks[r["pick"]]
        if p["co"] != coid:
            return
        if p["status"] != "awaiting_confirm":
            self.log("late_or_repeat_confirm", pick=p["id"], status=p["status"])
            if p["status"] == "expired":
                self.text(self.contractors[coid]["phones"][0], f"Sorry, the time to confirm job {rid} ran out and the homeowner was offered other quotes. You weren't charged.", "confirm_late", rid, conversational=True)
            return
        c = self.contractors[coid]
        if self.fx("hardening") and (c["status"] != "active" or not self.license_ok(c)):
            p["status"] = "refused"
            self.text(c["phones"][0], f"We can't send you job {rid} right now (account paused or license not verified). You weren't charged.", "confirm_refused", rid, conversational=(via == "sms"))
            return self.reoffer(r, p, "not_eligible")
        p["confirmed_at"] = self.sim.now
        if self.paused["charging"]:
            p["status"] = "held"; self.alert("high", "charging_paused", f"Pick {p['id']} confirmed while charging is paused"); return
        if self.fx("hardening") and not c["first_win_used"] and c["first_win_eligible"] and \
                any(o["entity"] == c["entity"] and o["id"] != coid and o["first_win_used"] for o in self.contractors.values()):
            c["first_win_eligible"] = False          # checked when the free win is used, not only at sign-up
        if not c["first_win_used"] and c["first_win_eligible"]:
            p["free"] = True; p["status"] = "paid"; c["first_win_used"] = True
            self.ledger.append(dict(t=self.sim.now, type="free_first_win", amount=0.0, waived=p["fee"], co=coid, req=rid, pick=p["id"]))
            self.set_status(r, "PAYMENT_PENDING")
            return self.release(p)
        if not c["card_on_file"]:
            p["status"] = "needs_card"
            self.text(c["phones"][0], f"Add a card to accept job {rid}: askhava.example/billing", "needs_card", rid, conversational=(via == "sms"))
            if self.cfg.get("fixes", {}).get("card_deadline"):
                self.schedule_at(self.add_business(self.sim.now, 60), "t_payment_fix_deadline", p["id"])
            return
        self.charge(p)

    def charge(self, p):
        r = self.requests[p["req"]]
        c = self.contractors[p["co"]]
        H = self.fx("hardening")
        if H and not (r["status"] in ("AWAITING_CONFIRM", "PAYMENT_PENDING") and r.get("pick") == p["id"] and p["status"] in LIVE_PICK):
            self.log("charge_skipped_not_live", pick=p["id"], req_status=r["status"], pick_status=p["status"])
            return
        p["status"] = "charging"
        if r["status"] == "AWAITING_CONFIRM":
            self.set_status(r, "PAYMENT_PENDING")
        if self.cfg.get("fixes", {}).get("idem_per_attempt") and p.get("attempt_failed"):
            p["attempt"] = p.get("attempt", 1) + 1; p["attempt_failed"] = False
        key = f"pick-{p['id']}-try{p.get('attempt', 1)}" if self.cfg.get("fixes", {}).get("idem_per_attempt") else f"pick-{p['id']}"
        res = self.sim.stripe.charge_off_session(c["stripe"], p["fee"], idem_key=key, meta=dict(pick=p["id"], req=r["id"]))
        if not res["ok"]:
            p["status"] = "charge_retry"
            if H:
                p.setdefault("retry_since", self.sim.now)
                if self.sim.now - p["retry_since"] > 240:
                    p["status"] = "cancelled"
                    self.alert("high", "charge_failed", f"Couldn't reach Stripe for 4 hours for job {r['id']}: pick released, homeowner re-offered", r["id"])
                    self.text(c["phones"][0], f"We couldn't process job {r['id']} because of a payment-system outage. You weren't charged.", "payment_outage", r["id"])
                    return self.reoffer(r, p, "stripe_outage")
            self.schedule_in(10, "charge_id", p["id"])          # Stripe unavailable: retry with the same idempotency key
            return
        p["charge"] = res["id"]
        if H:
            self.schedule_in(30, "t_reconcile", p["id"], res["id"])

    def t_reconcile(self, pid, pi_id):
        """If Stripe's notice never arrives, ask Stripe directly."""
        p = self.picks[pid]
        if p["status"] != "charging" or p.get("charge") != pi_id:
            return
        pi = self.sim.stripe.retrieve(pi_id)
        if pi is None:
            return self.schedule_in(30, "t_reconcile", pid, pi_id)
        self.log("reconciled_from_stripe", pick=pid, status=pi["status"])
        typ = {"succeeded": "payment_intent.succeeded", "failed": "payment_intent.payment_failed", "requires_action": "payment_intent.requires_action"}[pi["status"]]
        self.handle_payment_event(typ, pi)

    def on_stripe_webhook(self, ev):
        if ev["id"] in self.processed_events:
            self.log("webhook_duplicate_ignored", ev=ev["id"]); return
        self.processed_events.add(ev["id"])
        self.handle_payment_event(ev["type"], self.sim.stripe.intents[ev["pid"]])

    def record_charge(self, p, pi):
        if any(l["type"] == "charge" and l.get("stripe") == pi["id"] for l in self.ledger):
            return
        self.ledger.append(dict(t=self.sim.now, type="charge", amount=pi["amount"], co=p["co"], req=p["req"], pick=p["id"], stripe=pi["id"]))

    def handle_payment_event(self, typ, pi):
        pid = pi["meta"]["pick"]
        p = self.picks.get(pid)
        if p is None:
            # a payment for a pick this database doesn't know (lost in a restore)
            self.log("payment_for_unknown_pick", pick=pid, stripe=pi["id"])
            if self.fx("v93"):
                self.alert("high", "unknown_payment", f"Stripe payment {pi['id']} for an unknown pick: handled by the restore check", None)
            return
        r = self.requests[p["req"]]
        H = self.fx("hardening")
        if typ == "payment_intent.succeeded" and H and p["status"] in ("charging", "auth_needed", "payment_failed") and \
                (r["status"] != "PAYMENT_PENDING" or r.get("pick") != pid):
            # money arrived for a job that's no longer live: refund it and tell Casey
            self.record_charge(p, pi)
            p["charge"] = pi["id"]; p["status"] = "cancelled"
            self.do_refund(p, "job_no_longer_live")
            self.alert("normal", "late_charge_refunded", f"Charge for job {r['id']} arrived after it closed: refunded", r["id"])
            return
        ev = dict(type=typ)
        if ev["type"] == "payment_intent.succeeded":
            if p["status"] in ("charging", "auth_needed") or (H and p["status"] == "payment_failed" and p.get("charge") == pi["id"]):
                p["status"] = "paid"
                if H:
                    self.record_charge(p, pi); p["charge"] = pi["id"]
                else:
                    self.ledger.append(dict(t=self.sim.now, type="charge", amount=pi["amount"], co=p["co"], req=r["id"], pick=pid, stripe=pi["id"]))
                self.release(p)
            elif p["status"] in ("expired", "cancelled", "payment_failed", "refused"):
                # paid after we gave up: refund automatically
                if H:
                    self.record_charge(p, pi); p["charge"] = pi["id"]
                else:
                    self.ledger.append(dict(t=self.sim.now, type="charge", amount=pi["amount"], co=p["co"], req=r["id"], pick=pid, stripe=pi["id"]))
                self.do_refund(p, "late_payment_after_timeout")
        elif ev["type"] == "payment_intent.payment_failed":
            if p["status"] in ("charging", "auth_needed") and (not H or p.get("charge") in (None, pi["id"])):
                p["status"] = "payment_failed"; p["attempt_failed"] = True
                self.text(self.contractors[p["co"]]["phones"][0], f"Your card didn't go through for job {r['id']}. Update it within 1 business hour: askhava.example/billing", "payment_failed", r["id"])
                self.schedule_at(self.add_business(self.sim.now, 60), "t_payment_fix_deadline", pid)
        elif ev["type"] == "payment_intent.requires_action":
            if p["status"] == "charging":
                p["status"] = "auth_needed"
                self.text(self.contractors[p["co"]]["phones"][0], f"Your bank needs you to approve the charge for job {r['id']}: askhava.example/billing", "auth_needed", r["id"])
                p["pi"] = pi["id"]
                self.schedule_at(self.add_business(self.sim.now, 60), "t_payment_fix_deadline", pid)
        elif ev["type"] == "charge.refunded":
            pass
        elif ev["type"] == "charge.dispute.created":
            self.ledger.append(dict(t=self.sim.now, type="dispute", amount=-(pi["amount"] - pi["refunded"]), fee=self.cfg["dispute_fee"], co=p["co"], req=r["id"], pick=pid, stripe=pi["id"]))
            self.alert("high", "dispute", f"Contractor disputed the charge for job {r['id']}", r["id"])

    def t_payment_fix_deadline(self, pid):
        p = self.picks[pid]
        if self.fx("hardening") and p["status"] in ("payment_failed", "auth_needed") and p.get("charge"):
            pi = self.sim.stripe.retrieve(p["charge"])          # ask Stripe before giving up: the notice may just be late
            if pi and pi["status"] == "succeeded":
                return self.handle_payment_event("payment_intent.succeeded", pi)
        if p["status"] in ("payment_failed", "auth_needed", "needs_card"):
            p["status"] = "cancelled"
            self.reoffer(self.requests[p["req"]], p, "payment")

    def contractor_card_added(self, coid, card):
        self.web("contractor_card_added", coid, card)
        c = self.contractors[coid]
        res = self.sim.stripe.setup_card(c["stripe"], card)
        if not res["ok"] and self.cfg.get("fixes", {}).get("card_deadline"):
            self.text(c["phones"][0], "That card didn't work. Try another card or a bank account: askhava.example/billing", "card_setup_failed", None, conversational=True)
        if res["ok"]:
            c["card_on_file"] = True
            for p in self.picks.values():
                if p["co"] == coid and p["status"] in ("needs_card", "payment_failed"):
                    self.charge(p)
        return res["ok"]

    def release(self, p):
        """Only after: homeowner YES + contractor Confirm + payment succeeded (or free first win)."""
        r = self.requests[p["req"]]
        if p["released"]:
            self.log("release_repeat_blocked", pick=p["id"]); return
        if r["status"] != "PAYMENT_PENDING":
            self.log("release_wrong_state", pick=p["id"], status=r["status"])
            return
        p["released"] = True; p["released_at"] = self.sim.now
        c = self.contractors[p["co"]]
        h = self.homeowners[r["ho"]]
        v = self.vault[h["contact"]]
        sid = None
        if self.replaying:
            sid = next((k for k, s_ in self.sim.voice.sessions.items() if s_["request"] == r["id"] and s_["hphone"] == v["phone"] and s_["open"]
                        and c["phones"][0] in s_["cphones"]), None)
        sid = sid or self.sim.voice.open_session(r["id"], c["phones"], v["phone"])
        p["session"] = sid
        self.set_status(r, "CONNECTED")
        full_photos = [ph["raw"] for ph in r["photos"]]
        self.text(c["phones"][0], f"Confirmed: job {r['id']}. Homeowner: {v['name'] or 'the homeowner'}, address on your job page. Call them through (928) 555-0100.", "connect_contractor", r["id"], media=full_photos)
        self.sim.disclosures.append(dict(t=self.sim.now, to_co=c["id"], req=r["id"], pick=p["id"], kind="contact_and_address"))
        when = "by 4 p.m." if self.in_business() else "tomorrow morning"
        self.text(self.phone_of(r), f"{c['business']} confirmed and will call you from (928) 555-0100 {when}.", "who_is_calling", r["id"])
        self.schedule_in(24 * 60, "t_call_reminder", p["id"])
        self.schedule_in(72 * 60, "t_no_call_reoffer", p["id"])
        self.schedule_in(96 * 60, "t_refund_decision", p["id"])
        self.schedule_in(3 * DAY, "t_done_check", r["id"], p["id"])
        self.schedule_in(14 * DAY, "t_done_check", r["id"], p["id"])

    def on_call_completed(self, rec):
        for p in self.picks.values():
            if p.get("session") == rec["sid"]:
                p.setdefault("calls", []).append(rec)
                if rec["rejected"] == "unlinked_phone" and self.fx("v93"):
                    c = self.contractors[p["co"]]
                    last4 = rec["frm"][-4:]
                    c.setdefault("pending_links", {})[last4] = rec["frm"]
                    self.text(c["phones"][0], f"A call about job {p['req']} came from a phone ending {last4} that isn't on your account. Reply LINK {last4} to add it, then call again.", "link_phone", p["req"])
                    self.schedule_in(45, "t_call_followup", p["id"], rec["at"])
                    return
                if rec["rejected"] == "unlinked_phone" and self.cfg.get("fixes", {}).get("job_pin"):
                    return
                if rec["rejected"] == "unlinked_phone":
                    self.text(self.contractors[p["co"]]["phones"][0], "That phone isn't linked to your account. Add it in your portal (Profile > Staff phones) to call homeowners.", "unlinked_phone", p["req"], conversational=True)
                return

    def t_call_followup(self, pid, blocked_at):
        """45 minutes after a blocked call: if nobody has got through yet, ask the business to call from its main phone."""
        p = self.picks.get(pid)
        if p and p["status"] == "paid" and not any(not c_["rejected"] and c_["at"] > blocked_at for c_ in p.get("calls", [])):
            self.text(self.contractors[p["co"]]["phones"][0], f"Job {p['req']} still needs a call. Please call from this phone: (928) 555-0100.", "call_needed", p["req"])

    def connected(self, p):
        return any(c["human"] and c["seconds"] >= 45 for c in p.get("calls", []))

    def t_call_reminder(self, pid):
        p = self.picks[pid]
        if p["status"] == "paid" and not p.get("calls"):
            self.text(self.contractors[p["co"]]["phones"][0], f"Reminder: please call the homeowner for job {p['req']}.", "call_reminder", p["req"])

    def t_no_call_reoffer(self, pid):
        p = self.picks[pid]
        if p["status"] == "paid" and not p.get("calls"):
            p["status"] = "no_show"
            self.contractors[p["co"]]["no_shows"] += 1
            if self.contractors[p["co"]]["no_shows"] >= 2:
                self.contractors[p["co"]]["status"] = "paused"; self.contractors[p["co"]]["pause_reason"] = "no_shows"
                self.alert("normal", "contractor_no_shows", f"{self.contractors[p['co']]['business']} paused after 2 no-shows")
            r = self.requests[p["req"]]
            if self.fx("hardening"):
                self.close_session(p)
            if r["status"] == "CONNECTED":
                self.reoffer(r, p, "no_call")

    def t_refund_decision(self, pid):
        p = self.picks[pid]
        if p["status"] != "paid" or p["free"]:
            return
        calls = [c for c in p.get("calls", []) if not c["rejected"] and c["at"] - p["released_at"] <= 72 * 60]
        days = {int(c["at"] // DAY) for c in calls}
        gap_h = self.cfg.get("fixes", {}).get("refund_attempts_hours_apart")
        if gap_h and len(calls) >= 2:
            ts = sorted(c["at"] for c in calls)
            spread_ok = ts[-1] - ts[0] >= gap_h * 60
        else:
            spread_ok = len(days) >= 2
        if not self.connected(p) and spread_ok:
            self.do_refund(p, "unreachable")

    def do_refund(self, p, reason, attempt=1):
        if p.get("refunded"):
            self.log("refund_repeat_blocked", pick=p["id"]); return
        if not p.get("charge"):
            return
        amt = self.sim.stripe.intents[p["charge"]]["amount"] if self.fx("hardening") else p["fee"]
        res = self.sim.stripe.refund(p["charge"], amt, idem_key=f"refund-{p['id']}")
        if not res.get("ok") and self.fx("hardening"):
            if res.get("error") == "charge_disputed":
                self.alert("normal", "refund_blocked_dispute", f"Refund for job {p['req']} blocked: the charge is already disputed", p["req"]); return
            if attempt == 1:
                self.alert("high", "refund_failed", f"Refund for job {p['req']} failed ({res.get('error')}): retrying hourly", p["req"])
            if attempt < 24:
                self.schedule_in(60, "do_refund_id", p["id"], reason, attempt + 1)
            return
        if res.get("ok"):
            p["refunded"] = True
            self.ledger.append(dict(t=self.sim.now, type="refund", amount=-amt, co=p["co"], req=p["req"], pick=p["id"], reason=reason, stripe=p["charge"]))
            if reason != "late_payment_after_timeout":
                self.text(self.contractors[p["co"]]["phones"][0], f"Refunded ${p['fee']} for job {p['req']} ({reason}).", "refund_notice", p["req"])

    def t_done_check(self, rid, pid):
        r = self.requests[rid]
        p = self.picks[pid]
        if r["status"] == "CONNECTED" and r.get("pick") == pid:
            self.text(self.phone_of(r), f"Did {self.contractors[p['co']]['business']} do the work for request {rid}? Reply DONE, NO, or PROBLEM.", "done_check", rid)

    def homeowner_outcome(self, h, up, phone):
        for rid in reversed(h["requests"]):
            r = self.requests[rid]
            if r["status"] == "CONNECTED":
                p = self.picks[r["pick"]]
                if up == "DONE":
                    self.set_status(r, "DONE")
                    self.text(phone, f"Glad it's fixed. If you'd like, leave {self.contractors[p['co']]['business']} a review: askhava.example/review/{rid}" + ("" if self.fx("v93") else ". Save this number for next time."), "review_request", rid, conversational=True)
                    self.sim.reviews_requested.append(rid)
                elif up.startswith("PROBLEM"):
                    c = self.contractors[p["co"]]
                    c["status"] = "paused"; c["pause_reason"] = "complaint"
                    if self.fx("hardening"):
                        self.close_session(p)
                    self.alert("high", "complaint", f"Complaint on job {rid} about {c['business']}: contractor paused", rid)
                    self.set_status(r, "CLOSED")
                else:
                    self.set_status(r, "CLOSED")
                return

    def close_session(self, p):
        if p.get("session"):
            self.sim.voice.close_session(p["session"])

    def cancel_request(self, r, by):
        if r["status"] in TERMINAL:
            return
        H = self.fx("hardening")
        if r["status"] == "CONNECTED":
            p = self.picks[r["pick"]]
            if not p.get("calls") and not p["free"] and p["status"] == "paid":
                self.do_refund(p, "homeowner_cancelled_before_contact")
            if H:
                if p["status"] == "paid":
                    p["status"] = "closed_by_homeowner"
                self.close_session(p)
                self.text(self.contractors[p["co"]]["phones"][0], f"The homeowner closed job {r['id']}." + (" Your fee was refunded." if p.get("refunded") else ""), "homeowner_closed", r["id"])
        elif r.get("pick"):
            p = self.picks[r["pick"]]
            live = LIVE_PICK if H else ("awaiting_confirm", "needs_card", "payment_failed", "auth_needed", "charging")
            if p["status"] in live:
                p["status"] = "cancelled"
                if H:
                    self.text(self.contractors[p["co"]]["phones"][0], f"The homeowner closed job {r['id']}. You won't be charged.", "homeowner_closed", r["id"])
        self.set_status(r, "CANCELLED")

    def homeowner_stopped(self, phone):
        """STOP (or CANCEL, END...) from a homeowner: no more texts, and close what hasn't reached a contractor yet."""
        h = self.homeowners[self.phone_to_ho[phone]]
        for rid in h["requests"]:
            r = self.requests[rid]
            if r["status"] in TERMINAL:
                continue
            if r["status"] == "CONNECTED" and self.picks[r["pick"]].get("calls"):
                continue
            self.cancel_request(r, "homeowner_stop")

    # ------------------------------------------------------------------ contractor onboarding and checks
    def contractor_signup(self, actor):
        self.web("contractor_signup", actor)
        coid = f"B{len(self.contractors)+1}"
        cust = self.sim.stripe.create_customer(coid, idem_key=f"customer-{actor.phone}")
        self.contractors[coid] = CoRecord(id=coid, business=actor.business, trade=actor.trade, towns=set(actor.towns), neighbor_towns=actor.neighbor_towns,
                                      roc=actor.roc, phones=[actor.phone], email=actor.email, status="pending", stripe=cust, card_on_file=False,
                                      first_win_used=False, first_win_eligible=True, cards_sent=0, quotes=0, no_shows=0,
                                      license_status=None, license_checked_at=None, insurance_expiry=actor.insurance_expiry, entity=actor.entity,
                                      checklist=set(actor.checklist),            # job types they asked for, from the sign-up form
                                      overflow=actor.overflow,                   # 'also send me jobs nobody else covers' (on by default)
                                      license_class="HANDYMAN" if actor.trade == "handyman" else None)
        self.contractors[coid]["_live"] = True
        self.sim.co_stream.append((self.sim.now, coid, self.contractors[coid]["status"]))
        self.phone_to_co[actor.phone] = coid
        self.sim.contractors[coid] = actor            # (simulator bookkeeping: which simulated person owns this account)
        actor.coid = coid
        # one free first win per verified business entity
        if any(c["entity"] == actor.entity and c["id"] != coid and c["first_win_used"] for c in self.contractors.values()):
            self.contractors[coid]["first_win_eligible"] = False
        # cap per trade per town
        for town in actor.towns:
            n = sum(1 for c in self.contractors.values() if c["status"] == "active" and c["trade"] == actor.trade and town in c["towns"])
            if not self.fx("v93") and n >= self.cfg["cap_per_trade_town"]:      # v9.3: no cap; every qualified business can join
                self.contractors[coid]["status"] = "waitlist"
                return coid
        self.check_license(coid, first=True)
        return coid

    def check_license(self, coid, first=False):
        c = self.contractors[coid]
        if c["trade"] == "handyman":
            c["license_status"] = "ACTIVE"; c["license_checked_at"] = self.sim.now      # business license path (simplified)
            if first:
                self.verify_identity(coid, c.get("_record_phone_override"))
            return
        res = self.sim.roc.lookup(c["roc"])
        self.sim.costs.add("license_checks", 0)
        if not res["ok"]:
            c.setdefault("check_failures", 0); c["check_failures"] += 1
            self.log("license_check_failed", co=coid, error=res["error"])
            if c["status"] == "active" and self.sim.now - (c["license_checked_at"] or 0) > self.cfg["license_recheck_days"] * DAY:
                c["status"] = "paused"; c["pause_reason"] = "license_check_failed"
            self.schedule_in(60, "check_license", coid, first)                           # retry; never guess
            return
        rec = res["record"]
        cls_trade = CLASS_TRADE.get(rec["cls"], rec["cls"]) if rec else None
        if self.fx("v93") and rec and rec["status"] == "UNKNOWN":
            self.log("license_page_unreadable", co=coid)               # misread page: keep what we knew, try again
            self.schedule_in(60, "check_license", coid, first); return
        if rec is None or rec["status"] != "ACTIVE" or (rec["name"] and rec["name"] != c["business"]) or (rec["cls"] and cls_trade != c["trade"]):
            c["license_status"] = rec["status"] if rec else "NOT_FOUND"
            c["license_checked_at"] = self.sim.now
            if rec and rec["status"] == "UNKNOWN":
                self.schedule_in(60, "check_license", coid, first); return
            if c["status"] == "active":
                c["status"] = "paused"; c["pause_reason"] = "license"
                self.alert("high", "license_lapsed", f"{c['business']} license now {c['license_status']}: paused", coid)
            elif first:
                self.alert("normal", "signup_flagged", f"{c['business']} sign-up: license record doesn't match", coid)
            return
        c["license_status"] = "ACTIVE"; c["license_checked_at"] = self.sim.now
        if rec.get("cls"):
            c["license_class"] = rec["cls"]                    # read from the state record, never self-reported
        c["license_expires"] = rec.get("expires"); c["bond_cancel"] = rec.get("bond_cancel")
        if self.fx("v93") and not self.license_ok(c) and c["status"] == "active":
            c["status"] = "paused"; c["pause_reason"] = "license"
            self.alert("normal", "license_dates", f"{c['business']}: license or bond end date reached: paused", coid)
        if first:
            self.verify_identity(coid, rec["contact_phone"])

    def verify_identity(self, coid, record_phone):
        """Send a code to the phone on the state record, not the applicant's phone."""
        self.sim.costs.add("otp", 0.05)
        actor = self.sim.contractor_actor(coid)
        if record_phone and actor.controls_phone(record_phone):
            self.activate(coid)
        else:
            self.alert("normal", "identity_unverified", f"{self.contractors[coid]['business']}: couldn't verify through the state record contact", coid)
            self.contractors[coid]["status"] = "needs_review"

    def activate(self, coid):
        c = self.contractors[coid]
        c["status"] = "active"
        self.log("contractor_active", co=coid)
        if self.fx("v93") and not c.get("welcomed"):
            c["welcomed"] = True
            c["welcome_at"] = self.sim.now if self.in_business() else self.next_open(self.sim.now) + 11      # quiet hours hold it until 8 a.m.
            self.text(c["phones"][0], "Ask Hava: you're set up to get job cards for the job types you picked. Msg frequency varies. Msg & data rates may apply. "
                      "Reply PAUSE to pause job cards, STOP to opt out, HELP for help.", "co_welcome", None)

    def recheck_all(self):
        for coid, c in self.contractors.items():
            if c["status"] in ("active", "paused") and c.get("pause_reason") in (None, "license", "license_check_failed"):
                self.check_license(coid)
            if c["insurance_expiry"] is not None and c["insurance_expiry"] < self.sim.now and c["status"] == "active":
                c["status"] = "paused"; c["pause_reason"] = "insurance_expired"
                self.alert("normal", "insurance_expired", f"{c['business']} insurance expired: paused", coid)

    def hourly(self):
        if self.cfg.get("safe_mode_enabled", True):
            stale = [a for a in self.alerts if not a["done"] and a["level"] in ("high", "critical") and self.sim.now - a["t"] > 24 * 60]
            if stale and not self.safe_mode:
                self.safe_mode = True
                self.log("safe_mode_on", n=len(stale))
                self.sim.backup_notified.append(self.sim.now)
            if self.safe_mode and not stale:
                self.safe_mode = False
        self.schedule_in(60, "hourly")

    def daily(self):
        self.sim.stripe.payout()
        if self.fx("v93"):
            # compliance monitor: weekly opt-out rate per audience (internal alarm levels, set well below anything carriers act on)
            wk = self.sim.now - 7 * DAY
            texted = {p for t, p in getattr(self, "sent_log", []) if t >= wk}
            stops = {a["phone"] for a in self.audit if a["what"] == "stop" and a["t"] >= wk}
            for who, phones, limit in (("homeowners", {p for p in texted if p in self.phone_to_ho}, 0.03), ("contractors", {p for p in texted if p in self.phone_to_co}, 0.05)):
                rate = len(stops & phones) / max(1, len(phones))
                self.optout_rates = getattr(self, "optout_rates", []) + [(self.sim.now, who, round(rate, 4), len(phones))]
                if len(phones) >= 30 and rate > limit:
                    self.alert("high", "optout_rate", f"Opt-out rate for {who} is {rate:.1%} this week (alarm at {limit:.0%}): review message volume and wording", None)
        if self.cfg["license_recheck_mode"] in ("daily", "twice_daily") or int(self.sim.now // DAY) % 30 == 0:
            self.recheck_all()
        if self.cfg["license_recheck_mode"] == "twice_daily":
            self.schedule_in(19 * 60, "recheck_all")            # noon and 7 a.m.
        if int(self.sim.now // DAY) % 7 == 0:
            stats = f"week {int(self.sim.now // DAY)//7}: requests {len(self.requests)}, picks {len(self.picks)}, alerts {sum(1 for a in self.alerts if not a['done'])}"
            if self.cfg.get("summary_includes_examples") and not self.cfg.get("fixes", {}).get("summary_counts_only"):
                # a tempting shortcut: paste recent job descriptions into the summary prompt
                stats += " examples: " + " | ".join(r["original_text"] for r in list(self.requests.values())[-3:])
            self.sim.ai_summary.weekly_summary(stats)
        self.schedule_in(DAY, "daily")

    # ------------------------------------------------------------------ texting down: Casey's call list (person-to-person calls)
    def manual_call_list(self):
        """Owner portal list while texting is paused: who needs a personal call from Casey."""
        if not self.paused["texting"]:
            return []
        out = []
        answered = set()
        for m in self.sim.sms.log:                     # (the app's own message log; delivered replies)
            if m["status"] == "delivered":
                answered.add(m["to"])
        last_out = {}
        for m in self.sim.sms.log:
            if m["status"] == "delivered":
                last_out[m["to"]] = max(last_out.get(m["to"], -1e9), m["at"])
        for phone, hid in self.phone_to_ho.items():
            h = self.homeowners[hid]
            if phone in self.stopped:
                continue
            cv = h.get("convo")
            if cv and last_out.get(phone, -1e9) < cv.get("started", 0) and self.sim.now - h.get("_called_at", -1e9) > 2 * 60:
                out.append(("new_texter", phone, None))            # texted us for help; our reply never arrived
            if h.get("email"):
                continue
            for rid in h["requests"]:
                r = self.requests[rid]
                if r["status"] in ("OPEN", "NO_QUOTES_YET", "QUOTES_READY") and r.get("manual_contact_status") != r["status"]:
                    out.append(("update", phone, rid))
        return out

    def owner_set_email(self, phone, email):
        hid = self.phone_to_ho.get(phone)
        if hid and email:
            self.homeowners[hid]["email"] = email

    def owner_logged_call(self, kind, phone, rid, reached, info=None):
        hid = self.phone_to_ho.get(phone)
        h = self.homeowners.get(hid) if hid else None
        if kind == "new_texter" and h:
            h["_called_at"] = self.sim.now
            if reached:
                # Casey takes the request by phone: consent, ZIP and an optional email, then the normal flow continues
                h["consented"] = True
                if info and info.get("email"):
                    h["email"] = info["email"]
                if h.get("convo"):
                    if info and info.get("zip"):
                        h["convo"]["zip"] = info["zip"]
                    if info and info.get("onsite"):
                        h["convo"]["onsite"] = info["onsite"]
                    h["convo"]["stage"] = "new"
                    self.run_intake(h, phone, onsite_done=True)
        elif kind == "update" and rid and reached:
            r = self.requests[rid]
            r["manual_contact_status"] = r["status"]
            if info and info.get("email") and h:
                h["email"] = info["email"]
        self.log("owner_call", kind=kind, request=rid, reached=reached)

    # ------------------------------------------------------------------ database restore
    def snapshot(self):
        import copy
        return copy.deepcopy({k: v for k, v in self.__dict__.items() if k not in ("sim", "cfg", "epoch")})

    def restore(self, snap, restore_point):
        """Point-in-time restore: the database goes back to restore_point. Vendors (Stripe, Twilio) keep everything since."""
        import copy
        self.__dict__.update(copy.deepcopy(snap))
        self.epoch += 1
        for tid, (t, name, args) in self.timers.items():       # the timer table came back with the database
            self.sim.at(max(t, self.sim.now), self._fire, tid, self.epoch)
        self.log("database_restored", to=restore_point)
        if self.fx("v93"):
            self.reconcile_after_restore(restore_point)

    def reconcile_after_restore(self, t0):
        """v9.3: replay, in time order, everything that happened in the lost window, from records kept outside the database:
        Twilio (texts in and out, calls, Verify codes), Stripe's event feed, and the web server's request log.
        IDs are derived from content, so replays recreate the same IDs; texts already sent aren't sent again."""
        from collections import Counter
        evs = self.sim.stripe.list_events(since=t0)
        if evs is None:                                             # Stripe unreachable: try again shortly, never guess
            self.alert("high", "restore_waiting", "Restore check waiting for Stripe; will retry in 15 minutes", None)
            return self.schedule_in(15, "reconcile_after_restore", t0)
        out = self.sim.sms.list_outbound(since=t0)
        self.replay_sent = Counter((m["to"], m["kind"], m["meta"].get("ref")) for m in out)
        self.replay_rid_hints = {}
        refs = [m["meta"].get("ref") for m in out if str(m["meta"].get("ref") or "").isdigit()]
        self.replay_max_rid = max([int(x) for x in refs] + [self.req_seq])
        for m in out:
            ref = m["meta"].get("ref")
            if m["to"] in self.sim.homeowners and str(ref or "").isdigit() and ref not in self.requests:
                lst = self.replay_rid_hints.setdefault(m["to"], [])
                if ref not in lst:
                    lst.append(ref)
        timeline = []
        for frm, body, t, sid, media in self.sim.sms.list_inbound(since=t0):
            timeline.append((t, 0, "sms", (frm, body, media, sid)))
        for t, name, args in self.sim.web_log:
            if t >= t0:
                timeline.append((t, 1, "web", (name, args)))
        for e in evs:
            timeline.append((e["created"], 2, "stripe", e))
        for c in self.sim.voice.list_calls(since=t0):
            timeline.append((c["at"], 3, "call", c))
        timeline.sort(key=lambda x: (x[0], x[1]))
        self.replaying = True; self.replay_since = t0; self.restore_lost = set()
        try:
            for t, _, kind, item in timeline:
                self.run_due_timers(t)                              # let deadlines that fell before this event act first
                if kind == "sms":
                    frm, body, media, sid = item
                    self.on_inbound_sms(frm, body, media, False, sid)
                elif kind == "web":
                    name, args = item
                    try:
                        getattr(self, name)(*args)
                    except KeyError as e:                            # the action points at something the replay couldn't recreate
                        self.restore_lost.add(str(e).strip("'"))
                elif kind == "stripe":
                    self.replay_stripe_event(item)
                elif kind == "call":
                    self.on_call_completed(item)
        finally:
            self.replaying = False; self.replay_sent = None; self.replay_ended = self.sim.now
            self.req_seq = max(self.req_seq, self.replay_max_rid)
        # any kept payment still unknown after the replay is refunded and flagged
        for pi in self.sim.stripe.list_intents(since=t0) or []:
            if pi["status"] == "succeeded" and pi["meta"]["pick"] not in self.picks:
                co = next((cid for cid, c in self.contractors.items() if c["stripe"] == pi["cid"]), None)
                self.ledger.append(dict(t=self.sim.now, type="charge", amount=pi["amount"], co=co, req=pi["meta"].get("req"), pick=pi["meta"]["pick"], stripe=pi["id"], reconciled=True))
                owed = pi["amount"] - pi["refunded"]
                if owed > 0 and not pi["disputed"]:
                    res = self.sim.stripe.refund(pi["id"], owed, idem_key=f"refund-orphan-{pi['id']}")
                    if res.get("ok"):
                        self.ledger.append(dict(t=self.sim.now, type="refund", amount=-owed, co=co, req=pi["meta"].get("req"), pick=pi["meta"]["pick"], reason="lost_in_restore", stripe=pi["id"]))
                self.alert("high", "restore_orphan_payment", f"Payment {pi['id']} had no matching job after the restore: refunded", None)
        touched = {m["meta"].get("ref") for m in out if m["meta"].get("ref")}
        if self.restore_lost:
            self.alert("high", "restore_lost_jobs", f"Restore: {len(self.restore_lost)} job(s) from the lost window couldn't be rebuilt automatically: {sorted(self.restore_lost)[:5]}. Call those homeowners.", None)
        self.alert("high", "restore_review", f"Database restored; {len(touched)} jobs had activity in the lost window and were replayed. Spot-check them.", None)
        self.log("restore_replayed", events=len(timeline), jobs=len(touched))

    def run_due_timers(self, t):
        due = sorted((v[0], k) for k, v in self.timers.items() if v[0] <= t)
        for _, tid in due:
            if tid in self.timers:
                _, name, args = self.timers.pop(tid)
                getattr(self, name)(*args)

    def replay_stripe_event(self, e):
        if e["id"] in self.processed_events:
            return
        self.processed_events.add(e["id"])
        pi = self.sim.stripe.retrieve(e["pid"]) or dict(self.sim.stripe.intents[e["pid"]])
        if e["type"] == "charge.refunded":
            p = self.picks.get(pi["meta"]["pick"])
            booked = -sum(l["amount"] for l in self.ledger if l["type"] == "refund" and l.get("stripe") == pi["id"])
            if p and pi["refunded"] - booked > 0.01:
                self.ledger.append(dict(t=self.sim.now, type="refund", amount=-(pi["refunded"] - booked), co=p["co"], req=p["req"], pick=p["id"], reason="reconciled", stripe=pi["id"]))
                p["refunded"] = True
            return
        if e["type"] == "charge.dispute.created" and any(l["type"] == "dispute" and l.get("stripe") == pi["id"] for l in self.ledger):
            return
        self.handle_payment_event(e["type"], pi)

    # ------------------------------------------------------------------ owner actions
    def owner_resolve(self, alert, decision="approve"):
        alert["done"] = True; alert["done_at"] = self.sim.now
        if alert["kind"] in ("identity_unverified", "signup_flagged") and decision == "approve":
            self.activate(alert["ref"])
        elif alert["kind"] in ("identity_unverified", "signup_flagged"):
            self.contractors[alert["ref"]]["status"] = "removed"

    # ------------------------------------------------------------------ portals (what each person would see)
    def homeowner_portal(self, hid):
        h = self.homeowners[hid]
        return [dict(request=rid, status=self.requests[rid]["status"], quotes=len(self.requests[rid]["quotes"]),
                     picked=(self.contractors[self.picks[self.requests[rid]["pick"]]["co"]]["business"] if self.requests[rid].get("pick") else None))
                for rid in h["requests"]]

    def contractor_portal(self, coid):
        cards = [k for k in self.cards.values() if k["co"] == coid]
        charges = [l for l in self.ledger if l["co"] == coid]
        return dict(cards=len(cards), quotes=self.contractors[coid]["quotes"], wins=sum(1 for p in self.picks.values() if p["co"] == coid and p["status"] in ("paid", "no_show")),
                    net_billed=round(sum(l["amount"] for l in charges if l["type"] in ("charge", "refund")), 2),
                    statement_lines=len(charges), license=self.contractors[coid]["license_status"], status=self.contractors[coid]["status"])

    def owner_portal(self):
        charged = sum(l["amount"] for l in self.ledger if l["type"] == "charge")
        refunded = -sum(l["amount"] for l in self.ledger if l["type"] == "refund")
        disputes = -sum(l["amount"] for l in self.ledger if l["type"] == "dispute")
        return dict(requests=len(self.requests), charged=round(charged, 2), refunded=round(refunded, 2), disputes=round(disputes, 2),
                    open_alerts=sum(1 for a in self.alerts if not a["done"]),
                    active_contractors=sum(1 for c in self.contractors.values() if c["status"] == "active"))
