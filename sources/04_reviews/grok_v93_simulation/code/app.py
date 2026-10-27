"""The Havasu Home Quotes app (system under test), built from the v9.3 brief + Oct 26 changes.
All app state lives in self.db so a database restore is a snapshot swap. The app uses no
randomness. Each fix switch (cfg['fix']) is OFF = spec as written, ON = proposed fix."""
import copy, re, hashlib
from engine import DAY, HOUR, in_quiet, add_bmin, add_clock_hours, next_open, code_last_pass, strong_scrub, fmt_t, day
from vendors import AIDown, CARRIER_STOP
from config import CATS, JOBTYPES, FEES

DOMAIN = "hhq.example"
STOP_WORDS = {"STOP", "CANCEL", "END", "QUIT", "UNSUBSCRIBE", "STOPALL", "REVOKE", "OPTOUT"}
NL_OPTOUT = ["stop texting", "stop messaging", "stop sending", "don't text", "dont text", "do not text", "remove me",
             "take me off", "unsubscribe me", "no more texts", "quit texting", "leave me alone"]
ES_OPTOUT = ["alto", "parar", "no más mensajes", "no mas mensajes", "ya no me escriban", "dejen de escribir", "baja"]
CONSENT = {"ok", "yes", "sure", "sí"}
CONSENT_LENIENT = CONSENT | {"si", "okay", "yeah", "yep", "y", "claro", "dale", "yes please", "k"}
SAFETY_KW = ["smell", "burning", "alarm", "smoke", "sparks", "flooding", "dizzy", "head hurts",
             "olor", "quemando", "alarma", "humo", "chispas", "inundación", "inundando", "mareado", "me duele la cabeza"]
SAFETY_KW_FIX = ["hot", "pilot", "gas", "sizzl", "buzzing", "melting", "caliente", "zumbido"]
BUSINESS_PHRASES = ["get on your list", "sign up", "join your", "send me work", "send me jobs", "my business", "i'm a contractor",
                    "list my company", "add my company", "how do i join"]
PICK_Q = ["what now", "got the notice", "what do i do", "how do i confirm", "picked", "confirm"]
SPANISH_HINT = ["hola", "necesito", "mi ", "aire", "fuga", "gracias", "casa", "por favor", "tengo", "baño", "agua", "piscina", "limpieza"]
ROC_LINK = "https://roc.az.gov/file-complaint"
REASONS = {"gas": "A rotten-egg smell can mean a gas leak. Leave the area, don't flip switches, call 911 or the gas company.",
           "elec": "Heat, sparks or burning smells can mean an electrical fire risk. Turn off the breaker if safe, leave if there is smoke, call 911.",
           "water": "Flooding near outlets is a shock risk. Stay out of standing water near power and shut the main water valve if safe.",
           "co": "Dizziness or headaches near a heater can mean carbon monoxide. Get to fresh air now and call 911.",
           "smoke": "Smoke or an alarm can mean a fire. Leave the building and call 911."}
REASONS_ES = {"gas": "Un olor a huevo podrido puede ser una fuga de gas. Salga, no toque interruptores, llame al 911.",
              "elec": "Calor, chispas u olor a quemado pueden ser riesgo de incendio. Apague el breaker si es seguro, llame al 911.",
              "water": "El agua cerca de enchufes es un riesgo de descarga. Cierre la llave principal si es seguro.",
              "co": "Mareo o dolor de cabeza cerca de un calentador puede ser monóxido de carbono. Salga al aire libre y llame al 911.",
              "smoke": "Humo o una alarma pueden ser un incendio. Salga y llame al 911."}

ALLOWED = {"Intake": {"Open", "Closed", "Cancelled", "Not covered"},
           "Open": {"Quotes ready", "No quotes yet", "Cancelled", "Closed"},
           "No quotes yet": {"Quotes ready", "Closed", "Cancelled"},
           "Quotes ready": {"Awaiting confirm", "Closed", "Cancelled"},
           "Awaiting confirm": {"Payment pending", "Quotes ready", "Cancelled", "Closed"},
           "Payment pending": {"Connected", "Quotes ready", "Cancelled", "Closed"},
           "Connected": {"Done", "Closed", "Quotes ready", "Cancelled"},
           "Done": set(), "Closed": set(), "Cancelled": set(), "Not covered": set()}
LIVE = {"Intake", "Open", "No quotes yet", "Quotes ready", "Awaiting confirm", "Payment pending", "Connected"}

def norm(s): return re.sub(r"[.!?,]+$", "", s.strip().lower())

class AppBase:
    def __init__(self, sim):
        self.sim = sim; self.cfg = sim.cfg; self.fix = sim.cfg["fix"]
        self.replaying = False; self._now = None
        self.db = {"seq": {"job": 1041, "pick": 0, "msg": {}}, "jobs": {}, "ho": {}, "biz": {}, "phone_biz": {},
                   "timers": {}, "tid": 0, "epoch": 0, "out": {}, "ledger": [], "needs_you": [], "alerts": [],
                   "free_win_entities": {}, "charge_keys": {}, "complaints": {}, "callback_list": [], "demand_log": [],
                   "bugs": [], "safe_mode": False, "pause_charging": False, "audit": [], "ai_cache": {}, "status_log": [],
                   "handled_events": set(), "known_charges": set(), "call_obs": {}, "refund_done": {}}
    def now(self): return self._now if self._now is not None else self.sim.eng.t
    def audit(self, kind, **kw): self.db["audit"].append((self.now(), kind, kw))
    def bug(self, kind, **kw): self.db["bugs"].append((self.now(), kind, kw))
    def timer(self, t, kind, *args):
        self.db["tid"] += 1; tid = self.db["tid"]
        self.db["timers"][tid] = {"t": t, "kind": kind, "args": args, "active": True}
        self.sim.eng.at(t, self._fire, tid, self.db["epoch"])
        return tid
    def _fire(self, tid, epoch):
        if epoch != self.db["epoch"]: return
        tm = self.db["timers"].get(tid)
        if not tm or not tm["active"]: return
        tm["active"] = False
        getattr(self, "t_" + tm["kind"])(*tm["args"])
    def reschedule_all(self):
        self.db["epoch"] += 1
        for tid, tm in self.db["timers"].items():
            if tm["active"]: self.sim.eng.at(max(tm["t"], self.sim.eng.t), self._fire, tid, self.db["epoch"])
    def set_status(self, job, new):
        old = job["status"]
        if new not in ALLOWED.get(old, set()):
            self.bug("bad_transition", job=job["num"], old=old, new=new); return False
        job["status"] = new; job["st_t"] = self.now(); job["gen"] += 1
        self.db["status_log"].append((self.now(), job["num"], old, new))
        return True
    def needs(self, kind, prio=2, **kw):
        self.db["needs_you"].append({"t": self.now(), "kind": kind, "prio": prio, "done": None, **kw})
    def alert(self, kind, level="high", **kw):
        a = {"t": self.now(), "kind": kind, "level": level, "ack": None, **kw}
        self.db["alerts"].append(a)
        self.needs("alert", prio=1 if level in ("high", "critical") else 3, alert_idx=len(self.db["alerts"]) - 1, alert_kind=kind)
        if level in ("high", "critical"):
            self.timer(self.now() + 24 * HOUR, "alert_escalate", len(self.db["alerts"]) - 1)
    def t_alert_escalate(self, i):
        a = self.db["alerts"][i]
        if a["ack"] is None:
            self.db["safe_mode"] = True; a["escalated"] = self.now()
            self.sim.world.casey.backup_paged(self.now(), i)
    # ---------------- texting
    def sms(self, to, tpl, body, job=None, exempt=False, important=True, direct=False, mms=0, es=None, aud="ho"):
        h = self.db["ho"].get(to)
        if aud == "ho" and h is not None:
            if h["optout"] and tpl != "optout_confirm": return None
            if h["lang"] == "es" and es is not None: body = es
        if aud == "biz":
            bid = self.db["phone_biz"].get(to)
            if bid and self.db["biz"][bid].get("optout"): return None
        k = (str(job) if job else to, tpl, to)
        n = self.db["seq"]["msg"].get(k, 0) + 1; self.db["seq"]["msg"][k] = n
        key = f"{k[0]}:{tpl}:{to}:{n}"
        return self._send(to, tpl, body, job, exempt, important, direct, mms, key, 0, aud)
    def _quiet_blocked(self, to, exempt, direct):
        now = self.now()
        if exempt: return False
        last = self.db["ho"][to]["last_in"] if to in self.db["ho"] else self.db.get("biz_last_in", {}).get(to)
        if direct and last is not None and now - last <= 15: return False
        if self.fix["quiet_cutoff_2030"]:
            h = (now % DAY) / 60
            return h >= 20.5 or h < 8 + (1.0 if self.sim.world.out_of_state(to) else 0)
        return in_quiet(now)
    def _send(self, to, tpl, body, job, exempt, important, direct, mms, key, attempt, aud):
        now = self.now()
        if self._quiet_blocked(to, exempt, direct):
            t8 = next_open(now) + (60 if self.fix["quiet_cutoff_2030"] and self.sim.world.out_of_state(to) else 0)
            self.timer(t8, "held_send", to, tpl, body, job, exempt, important, mms, key, attempt, aud)
            return "held"
        tw = self.sim.tw
        k2 = key if attempt == 0 else f"{key}#r{attempt}"
        if self.replaying and k2 in tw.keys:
            return tw.keys[k2]
        sid = tw.send(self.sim.eng.t, to, body, k2, mms=mms, meta={"tpl": tpl, "job": job, "aud": aud})
        self.db["out"][sid] = {"key": key, "to": to, "tpl": tpl, "body": body, "job": job, "attempt": attempt,
                               "important": important, "final": None, "aud": aud, "mms": mms, "exempt": exempt}
        if important and attempt < 3:
            self.timer(now + 10, "sms_check", sid)
        return sid
    def t_held_send(self, to, tpl, body, job, exempt, important, mms, key, attempt, aud):
        if job is not None:
            j = self.db["jobs"].get(job)
            if j and j["status"] in ("Closed", "Cancelled", "Done", "Not covered") and tpl not in ("closed_notice", "loser", "cancelled"):
                return
        h = self.db["ho"].get(to)
        if h and h["optout"]: return
        self._send(to, tpl, body, job, exempt, important, False, mms, key, attempt, aud)
    def on_sms_status(self, sid, status, err):
        o = self.db["out"].get(sid)
        if o is None or o["final"] is not None: return
        o["final"] = status
        if status in ("failed", "undelivered"):
            if err == 21610: return
            if err == 30007:
                if not self.db.get("tw_suspended_alerted"):
                    self.db["tw_suspended_alerted"] = True; self.alert("twilio_suspension", "critical")
                self._fallback(o); return
            self._maybe_retry(sid, o)
    def t_sms_check(self, sid):
        o = self.db["out"].get(sid)
        if o is None or o["final"] is not None: return
        if self.fix.get("retry_query_status"):
            st = self.sim.tw.query(sid)
            if st == "delivered": o["final"] = "delivered"; return
        o["final"] = "assumed_failed"
        self._maybe_retry(sid, o)
    def _maybe_retry(self, sid, o):
        if not o["important"]: return
        if o["attempt"] >= 3:
            self._fallback(o); return
        self.timer(self.now() + 1, "retry_send", sid)
    def t_retry_send(self, sid):
        o = self.db["out"][sid]
        j = self.db["jobs"].get(o["job"]) if o["job"] else None
        if j and j["status"] in ("Closed", "Cancelled", "Done") and o["tpl"] not in ("loser", "closed_notice"): return
        self._send(o["to"], o["tpl"], o["body"], o["job"], o["exempt"], o["important"], False, o["mms"], o["key"], o["attempt"] + 1, o["aud"])
    def _fallback(self, o):
        to = o["to"]
        if o["aud"] == "ho":
            h = self.db["ho"].get(to); email = h and h.get("email")
        else:
            bid = self.db["phone_biz"].get(to); email = bid and self.db["biz"][bid].get("email")
        if email:
            self.sim.email.send(self.sim.eng.t, email, f"Havasu Home Quotes {o['tpl']}", o["body"])
        elif o["aud"] == "ho":
            dkey = (to, int(self.now() // DAY))
            seen = self.db.setdefault("cb_seen", set())
            if dkey not in seen:
                seen.add(dkey)
                self.db["callback_list"].append({"t": self.now(), "phone": to, "job": o["job"], "tpl": o["tpl"], "done": None})
                self.needs("callback_call", prio=2, phone=to, job=o["job"], body=o["body"])
    def optout_confirm_text(self, phone):
        h = self.db["ho"].get(phone)
        if h and h["lang"] == "es":
            return "Havasu Home Quotes: no recibirá más mensajes. Para volver a usarnos, envíe START."
        return "Havasu Home Quotes: you're unsubscribed and won't get more texts. To use us again, text START."
    def consent_text(self):
        en = ("Hi, this is Havasu Home Quotes. We gather quotes so you don't have to make the calls. It's free for you, "
              "and the business you choose pays us a small fee. Msg frequency varies by request. Msg & data rates may apply. "
              f"Reply STOP to opt out, HELP for help. Terms and privacy: {DOMAIN}/terms. Reply OK to start.")
        es = ("Hola, somos Havasu Home Quotes. Reunimos cotizaciones para que no tenga que hacer las llamadas. Es gratis para usted "
              "y el negocio que elija nos paga una pequeña comisión. La frecuencia varía. Pueden aplicar tarifas de mensajes y datos. "
              f"Responda STOP para salir, HELP para ayuda. Términos y privacidad: {DOMAIN}/terms. Responda SÍ para empezar.")
        return en, es
