"""Simulated businesses (Part 8.2) and Casey (Part 8.3). Rates Assumed unless the brief gives them."""
import math, random
from engine import DAY, HOUR, day, hod, add_bmin, next_open
from config import CATS, GROUP_CATS, GROUP_LIC, CAC, JOBTYPES

def p_confirm(s, s50): return 1.0 / (1.0 + (s / s50) ** 3)   # Part 9 (G formula, A parameters)

def biz_hours(rng, t, lo, hi):
    """t + U(lo,hi) minutes, moved into 7 a.m.-7 p.m. working hours."""
    x = t + rng.uniform(lo, hi)
    h = (x % DAY) / 60
    if h >= 19: x = (x // DAY + 1) * DAY + 7 * 60 + rng.uniform(0, 90)
    elif h < 7: x = (x // DAY) * DAY + 7 * 60 + rng.uniform(0, 90)
    return x

class Holder:
    """A licence holder in the state record who is not on the platform (fakes borrow these)."""
    def __init__(self, w, lic, phone): self.w = w; self.lic = lic; self.phone = phone; self.codes = 0; self.letters = 0
    def receive(self, to, sid): pass
    def receive_email(self, s, b): self.codes += 1
    def got_code(self, phone, idx): self.codes += 1
    def got_letter(self, code): self.letters += 1
    def answer_call(self, t, frm, rec): return False, 0.0
    def mobile_for(self, ph): return True
    def dead_number(self, ph): return False
    def answers_robot(self, ph): return False
    landline = False
    bid = None

class Biz:
    def __init__(self, w, bid, group, persona, fake=False, idx=0):
        self.w = w; self.bid = bid; self.group = group; self.persona = persona; self.fake = fake
        self.rng = random.Random(w.seed * 6007 + idx * 17 + 5)
        r = self.rng; c = w.cfg
        self.name = f"{['Desert','Lake','River','Mesa','Sun','Bridge','Island','Canyon','Palm','Mohave'][idx % 10]} {group.title()} {idx}"
        self.phone = f"+1928{7000000 + idx:07d}"
        self.email = f"b{idx}@biz.example" if r.random() < 0.85 else None
        self.cats = list(GROUP_CATS[group])
        self.lic_kind = None; self.lic = None; self.classes = ()
        gl = GROUP_LIC.get(group)
        if gl == ("PEST",): self.lic_kind = "PEST"; self.lic = f"PEST{idx:05d}"
        elif gl:
            self.lic_kind = "ROC"; self.lic = f"ROC{300000 + idx}"
            self.classes = gl if gl != ("ROC-any",) else ("ROC-any",)
            if group == "hvac": self.classes = ("R-39",)
            if group == "pool": pass
        if group == "pool" and r.random() < 0.5:
            self.lic_kind = "ROC"; self.lic = f"ROC{300000 + idx}"; self.classes = ("B-6",)
        if group == "handyman": self.lic_kind = None
        self.after_hours = r.random() < c["p_after_hours"]; self.spanish = r.random() < c["p_speaks_spanish"]
        self.skip_tenant = r.random() < c["p_skip_tenant"]; self.max_cards = r.choice([5, 10, 20]) if r.random() < c["p_max_cards"] else None
        self.ach = r.random() < c["p_bank_payment"]; self.insured = r.random() < 0.8
        self.staff = [f"+1928{8000000 + idx * 10 + k:07d}" for k in range(3)] if persona == "multistaff" else []
        self.roc_phone = self.phone; self.roc_landline = False; self.roc_dead = False
        if persona == "oldrecord":
            if r.random() < 0.5: self.roc_phone = f"+1928{9000000 + idx:07d}"; self.roc_dead = True
            else: self.roc_phone = f"+1928{9100000 + idx:07d}"; self.roc_landline = True
        self.landline = False
        self.wins = 0; self.quotes = 0; self.cards = 0; self.paid = []; self.strikes = 0; self.left = None
        self.active_t = None; self.cust = None; self.card_known = False
        self.fee_avoided = 0.0
        self.base_q = {"responsive": 0.80, "busy": 0.30, "linkaverse": 0.60, "bypasser": 0.6, "noshow": 0.6}.get(persona, 0.6)
        if fake: self.base_q = 0.9
        self.dba = None
    # ---- phone behaviour
    def mobile_for(self, ph):
        return not (ph == self.roc_phone and self.roc_landline)
    def dead_number(self, ph): return ph == self.roc_phone and self.roc_dead
    def answers_robot(self, ph):
        if ph == self.roc_phone and self.roc_landline: return self.rng.random() < 0.6
        return self.rng.random() < 0.8
    def addr_owner(self): return self
    def receive(self, to, sid):
        rec = self.w.sim.tw.sms[sid]; tpl = rec["meta"].get("tpl")
        if tpl == "card": self.cards += 1
    def receive_email(self, subject, body):
        if not getattr(self, "joins", False): self.unexpected = getattr(self, "unexpected", 0) + 1; return
        if "sign-up code" in subject:
            code = body.split()[-1]
            self.w.sim.eng.at(biz_hours(self.rng, self.w.sim.eng.t, 30, 24 * 60), self.w.web, "verify", self.bid, code, self.phone)
        elif "card" in subject or "New job" in body:
            self.w.on_card_email(self, body)
    def got_code(self, phone, idx):
        rec = self.w.sim.tw.verify[idx]
        if not getattr(self, "joins", False): self.unexpected = getattr(self, "unexpected", 0) + 1; return
        if self.fake and phone != self.phone: return
        self.w.sim.eng.at(self.w.sim.eng.t + self.rng.uniform(1, 8), self.w.web, "verify", self.bid, rec["code"], self.phone)
    def got_letter(self, code):
        if not getattr(self, "joins", False): self.unexpected = getattr(self, "unexpected", 0) + 1; return
        if self.fake: return
        self.w.sim.eng.at(biz_hours(self.rng, self.w.sim.eng.t, 60, 3 * DAY), self.w.web, "verify", self.bid, code, self.phone)
    def answer_call(self, t, frm, rec):
        if self.rng.random() < 0.6: return True, self.rng.uniform(1, 6)
        return False, 0.0

class Casey:
    """Casey's two 30-minute windows a day on a rotating shift (A), a backup person (G role)."""
    def __init__(self, w):
        self.w = w; self.c = w.cfg["casey"]; self.rng = random.Random(w.seed * 211 + 9)
        self.minutes = []      # (t, task, minutes, who, mode)
        self.backlog = []
        self.away = None
        if self.c.get("away"): a, n = self.c["away"]; self.away = (a * DAY, (a + n) * DAY)
        self.backup_minutes = 0.0
    def windows_for_day(self, d):
        wk = (d // 7) % 4      # A: 2 weeks day shift, 2 weeks night shift
        if wk < 2: return [6 * 60 + 15, 19 * 60 + 45]       # before and after a day shift
        return [12 * 60, 17 * 60]                               # after waking, before a night shift
    def is_away(self, t): return self.away is not None and self.away[0] <= t < self.away[1]
    def start(self, days):
        for d in range(days + 1):
            for m in self.windows_for_day(d):
                self.w.sim.eng.at(d * DAY + m + self.rng.uniform(-10, 10), self.window, pr=6)
    def window(self):
        t = self.w.sim.eng.t
        if self.is_away(t):
            self.backup_window(); return
        budget = self.c["window_min"]
        app = self.w.sim.app
        items = [n for n in app.db["needs_you"] if n["done"] is None]
        items.sort(key=lambda n: (n["prio"], n["t"]))
        for n in items:
            m = self.cost(n)
            if budget <= 0: break
            budget -= m
            self.do(n, t, "casey")
            self.minutes.append((t, n["kind"], m, "casey", "on"))
        if budget < 0: pass
    def backup_window(self):
        t = self.w.sim.eng.t; app = self.w.sim.app
        if not self.c.get("backup"): return
        for n in [n for n in app.db["needs_you"] if n["done"] is None]:
            if n["kind"] in ("alert", "approve_signup", "city_mismatch", "pause_review", "letter_manual"):
                if n["kind"] == "alert" or self.rng.random() < 0.5:
                    m = self.cost(n); self.do(n, t, "backup"); self.minutes.append((t, n["kind"], m, "backup", "away"))
    def backup_paged(self, t, i):
        self.w.sim.eng.at(t + self.rng.uniform(30, 240), self._backup_ack, i)
    def _backup_ack(self, i):
        app = self.w.sim.app; a = app.db["alerts"][i]
        if a["ack"] is None: a["ack"] = (self.w.sim.eng.t, "backup")
        app.db["safe_mode"] = False
        self.minutes.append((self.w.sim.eng.t, "backup_page", 10, "backup", "away" if self.is_away(self.w.sim.eng.t) else "on"))
    def cost(self, n):
        k = n["kind"]; M = self.c["minutes"]
        if k == "approve_signup" and self.w.cfg["fix"]["casey_checklist"]: return M["approve_signup_checklist"]
        if k == "alert" and n.get("alert_kind") == "scraper_broken": return M["scraper_fix"]
        if k == "contact_dispute": return 10
        if k == "biz_text": return M["misc"]
        return M.get(k, M["misc"])
    def do(self, n, t, who):
        app = self.w.sim.app; n["done"] = (t, who); k = n["kind"]; w = self.w
        if k == "alert":
            a = app.db["alerts"][n["alert_idx"]]
            if a["ack"] is None: a["ack"] = (t, who)
            ak = a["kind"]
            if ak == "scraper_broken" and who == "casey":
                w.sim.roc.fix_layout(t); app.db["scraper_alert"] = False
                for b in app.db["biz"].values():
                    if b["paused"] == "license_unknown_48h": b["paused"] = None
                    b["unknown_since"] = None
            elif ak == "unknown_charge":
                ch = w.sim.stripe.charges[a["cid"]]
                j = app.db["jobs"].get(ch["meta"]["job"])
                if not (j and a["cid"] in j["charges"]):
                    app._now = None; app.do_refund(a["cid"], ch["amount"], f"casey-refund-{a['cid']}", 0, ch["meta"]["job"], "unknown_charge")
            elif ak == "twilio_suspension":
                pass
            app.db["safe_mode"] = False
        elif k in ("approve_signup", "city_mismatch", "letter_manual"):
            b = w.biz.get(n["bid"])
            if b is None: return
            if b.fake: ok = self.rng.random() < (self.c["p_fooled_checklist"] if w.cfg["fix"]["casey_checklist"] else self.c["p_fooled"])
            else: ok = self.rng.random() < self.c["p_approve_legit"]
            w.casey_decisions.append((t, n["bid"], b.fake, ok, k))
            app.casey_decide(n["bid"], ok)
        elif k == "pause_review":
            b = app.db["biz"][n["bid"]]
            if b["paused"] in ("license_unknown_48h",) and w.sim.roc.truth_active(b["lic"], t): b["paused"] = None
        elif k == "refund_fail":
            pass
        elif k == "contact_dispute":
            j = app.db["jobs"][n["job"]]
            if j["status"] == "Connected": app.refund_job(j, "casey_contact_dispute"); app.set_status(j, "Closed"); j["close_reason"] = "casey"
        elif k == "dispute":
            d = w.sim.stripe.disputes[n["idx"]]
            if d["responded"] is None and t <= d["due"] and who == "casey": d["responded"] = t
        elif k == "callback_call":
            w.callbacks_done.append((t, n.get("phone"), n.get("job")))
