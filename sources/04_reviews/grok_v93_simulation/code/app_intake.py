"""App part 2: inbound texts, intake, matching (Part 3, 4.3, 5.1, 5.3)."""
import re, hashlib
from engine import DAY, HOUR, add_clock_hours, code_last_pass, strong_scrub, fmt_t, day
from vendors import AIDown
from config import CATS, JOBTYPES
from app import (AppBase, norm, DOMAIN, STOP_WORDS, NL_OPTOUT, ES_OPTOUT, CONSENT, CONSENT_LENIENT, SAFETY_KW, SAFETY_KW_FIX,
                 BUSINESS_PHRASES, SPANISH_HINT, REASONS, REASONS_ES, LIVE)

class IntakeMixin:
    def on_inbound(self, frm, body):
        now = self.now()
        low = norm(body); up = body.strip().upper().rstrip(".!")
        if frm in self.db["phone_biz"]:
            self.db.setdefault("biz_last_in", {})[frm] = now
            return self.biz_inbound(frm, body, low)
        h = self.db["ho"].get(frm)
        if h is None:
            h = {"phone": frm, "consent": None, "optout": False, "lang": self.guess_lang(body), "email": None, "fname": None,
                 "addr": None, "jobs": [], "last_in": now, "await": None, "reprompted": 0, "first_t": now, "list_order": {}}
            self.db["ho"][frm] = h
        h["last_in"] = now
        kw = self.safety_hit(low)
        if up in ("START", "UNSTOP"):
            h["optout"] = False; self.audit("opt_in", phone=frm); return
        if h["optout"]:
            if kw: self.alert("optout_emergency", "critical", phone=frm)
            if h.get("email"):
                self.sim.email.send(self.sim.eng.t, h["email"], "Havasu Home Quotes", "You texted us after opting out. Text START to use us again.")
            else:
                self.db["callback_list"].append({"t": now, "phone": frm, "job": None, "tpl": "optout_return", "done": None})
                self.needs("callback_call", prio=3, phone=frm, job=None, body="opted-out texted again")
            return
        if up in STOP_WORDS:
            return self.do_optout(h, carrier=True, word=up)
        es_hit = self.fix["spanish_optout"] and (low in ES_OPTOUT or any(p in low for p in ES_OPTOUT[2:]))
        if any(p in low for p in NL_OPTOUT) or es_hit:
            return self.do_optout(h, carrier=False, word=low)
        if up in ("HELP", "INFO", "AYUDA"):
            self.sms(frm, "help", "Havasu Home Quotes help: help@hhq.example, (928) 555-0100. Reply STOP to opt out.", exempt=True,
                     important=False, es="Ayuda de Havasu Home Quotes: help@hhq.example, (928) 555-0100. Responda STOP para salir.")
            return
        if any(p in low for p in BUSINESS_PHRASES):
            self.sms(frm, "signup_link", f"Thanks! Businesses sign up here: {DOMAIN}/join", direct=True, important=False, es=f"¡Gracias! Los negocios se registran aquí: {DOMAIN}/join")
            self.audit("biz_signup_text", phone=frm); return
        mclose = re.match(r"close\s+#?(\d+)$", low)
        if mclose: return self.ho_cancel(h, int(mclose.group(1)))
        nm = self.fix["never_mind_cancel"] and ("never mind" in low or "nevermind" in low or "ya no" in low or "no longer need" in low)
        if "please cancel" in low or "cancel my request" in low or "cancelar" in low or nm:
            return self.ho_cancel(h, None)
        if h["await"]:
            return self.answer(h, body, low, kw)
        m = re.match(r"^(\d{1,2})\s*(yes|sí|si)$", low)
        if m: return self.pick_by_text(h, int(m.group(1)))
        if low in ("list", "lista"): return self.send_list(h)
        if low in ("done", "no", "problem", "listo", "problema"): return self.checkin_reply(h, low)
        if "any quotes" in low or "quotes yet" in low or "cotizaciones" in low:
            j = self.newest_live(h)
            if j:
                n = len(j["quotes"])
                self.sms(frm, "status", f"Request #{j['num']}: {n} quote(s) so far. See them at {DOMAIN}/j/{j['token']}", job=j["num"],
                         direct=True, important=False, es=f"Solicitud #{j['num']}: {n} cotización(es). Véalas en {DOMAIN}/j/{j['token']}")
            return
        j = self.newest_live(h)
        if kw and j is not None and j["status"] != "Intake" and not j.get("safety_t"):
            self.safety_text(h, j, kw); return
        if len(low.split()) <= 2:
            if not h["jobs"]:
                self.sms(frm, "more", "Hi, this is Havasu Home Quotes. Tell us a bit more about the job you need done.", direct=True,
                         es="Hola, somos Havasu Home Quotes. Cuéntenos un poco más del trabajo que necesita.")
            return
        for jn in reversed(h["jobs"]):
            o = self.db["jobs"][jn]
            if o["status"] in LIVE and (now - o["t0"] <= 120 or self.mostly_matches(o["desc_raw"], body)):
                o["desc_raw"] += " " + body; o["dup_msgs"] = o.get("dup_msgs", 0) + 1
                if kw and not o.get("safety_t"): self.safety_text(h, o, kw)
                return
        self.new_job(h, body, kw)
    def guess_lang(self, body):
        low = body.lower()
        return "es" if sum(1 for w in SPANISH_HINT if w in low) >= 2 or any(c in low for c in "ñ¿¡") else "en"
    def mostly_matches(self, a, b):
        wa = set(re.findall(r"[a-záéíóúñ]+", a.lower())); wb = set(re.findall(r"[a-záéíóúñ]+", b.lower()))
        return len(wa & wb) / max(1, len(wb)) >= 0.5
    def safety_hit(self, low):
        for k in SAFETY_KW + (SAFETY_KW_FIX if self.fix["emergency_indirect_keywords"] else []):
            if re.search(r"\b" + re.escape(k), low): return k
        return None
    def newest_live(self, h):
        for jn in reversed(h["jobs"]):
            j = self.db["jobs"][jn]
            if j["status"] in LIVE: return j
        return None
    def new_job(self, h, body, kw):
        now = self.now()
        self.db["seq"]["job"] += 1; num = self.db["seq"]["job"]
        j = {"num": num, "phone": h["phone"], "t0": now, "status": "Intake", "st_t": now, "gen": 0, "desc_raw": body,
             "token": hashlib.sha1(f"job-{num}".encode()).hexdigest()[:10], "cat": None, "jt": None, "lang": h["lang"],
             "tenant": False, "emer": False, "urgent": None, "card": None, "ok_photos": [], "recipients": [], "quotes": {},
             "arrival": [], "pick": None, "picks": [], "losers_told": [], "charges": [], "released": None, "free_win": False,
             "safety_t": None, "safe": None, "last_update_t": None, "update_pending": False, "clar": 0, "lines_open": False,
             "pin": f"{(num * 7919) % 10000:04d}", "zip": None, "first_quote_t": None, "close_reason": None, "refunded": [],
             "ai": None, "promise_t": None, "seen_by_ai": []}
        self.db["jobs"][num] = j; h["jobs"].append(num)
        self.sim.world.job_created(num, h["phone"], now, body)
        if kw: self.safety_text(h, j, kw)
        if h["consent"] is None:
            en, es = self.consent_text()
            h["await"] = "consent"; h["await_job"] = num
            self.sms(h["phone"], "consent", en, job=num, direct=True, es=es)
        else:
            self.intake_continue(j)
    def safety_text(self, h, j, reason_kw):
        rk = self.reason_kind(j, reason_kw)
        j["safety_t"] = self.now(); j["safe"] = False; j["emer"] = True
        en = f"Havasu Home Quotes safety notice for #{j['num']}: {REASONS[rk]} Reply SAFE when you are safe."
        es = f"Aviso de seguridad de Havasu Home Quotes para #{j['num']}: {REASONS_ES[rk]} Responda SEGURO cuando esté a salvo."
        self.sms(h["phone"], "safety", en, job=j["num"], exempt=True, es=es)
        if h["await"] != "consent":
            h["await"] = "safe"; h["await_job"] = j["num"]
        self.sim.world.safety_sent(j["num"])
        if self.fix["safety_followup"]:
            self.timer(self.now() + 60, "safety_follow", j["num"], 1)
    def t_safety_follow(self, num, n):
        j = self.db["jobs"][num]
        if j["safe"] or j["status"] not in LIVE: return
        if n == 1:
            self.sms(j["phone"], "safety_follow", f"Havasu Home Quotes: checking on #{num}. Reply SAFE when you are safe, or call 911.",
                     job=num, exempt=True, es=f"Havasu Home Quotes: ¿está bien? Responda SEGURO, o llame al 911 (#{num}).")
            self.timer(self.now() + 240, "safety_follow", num, 2)
        else:
            self.alert("safety_no_reply", "high", job=num)
            if self.set_status(j, "Closed"):
                j["close_reason"] = "emergency_no_reply"
                self.sms(j["phone"], "closed_notice", f"Havasu Home Quotes closed #{num} because we couldn't confirm you're safe. Text us again any time.",
                         job=num, es=f"Havasu Home Quotes cerró #{num}. Escríbanos de nuevo cuando quiera.")
    def reason_kind(self, j, kw):
        d = (j["desc_raw"] + " " + str(kw)).lower()
        if any(w in d for w in ["gas", "rotten", "pilot", "egg", "propane"]): return "gas"
        if any(w in d for w in ["dizzy", "head hurts", "mareado", "cabeza"]): return "co"
        if any(w in d for w in ["flood", "inund"]): return "water"
        if any(w in d for w in ["spark", "breaker", "outlet", "burning", "hot", "chispas", "quemando"]): return "elec"
        return "smoke"
    def answer(self, h, body, low, kw):
        st = h["await"]; j = self.db["jobs"].get(h.get("await_job"))
        if j is None or j["status"] not in LIVE:
            h["await"] = None
            if len(low.split()) > 2: self.new_job(h, body, kw)
            return
        accept = CONSENT_LENIENT if self.fix["consent_lenient"] else CONSENT
        if st == "consent":
            if low in accept:
                h["consent"] = {"t": self.now(), "reply": body, "prompt_v": "9.3-oct26"}
                h["await"] = None
                return self.intake_continue(j)
            if h["reprompted"] < 2:
                h["reprompted"] += 1
                self.sms(h["phone"], "consent_reprompt", f"Havasu Home Quotes: to start #{j['num']}, please reply OK (or STOP to opt out).",
                         job=j["num"], direct=True, es=f"Havasu Home Quotes: para empezar #{j['num']}, responda SÍ (o STOP para salir).")
            return
        if st == "safe":
            if "safe" in low or "seguro" in low or low in ("ok", "yes", "sí", "si", "fine", "estoy bien", "we're ok", "im ok"):
                j["safe"] = True; h["await"] = "urgent"; h["await_job"] = j["num"]
                self.sms(h["phone"], "urgent_q", f"Glad you're safe. For #{j['num']}: after-hours work usually costs more. Reply 1 for EMERGENCY (tonight) or 2 for REGULAR.",
                         job=j["num"], direct=True, es=f"Qué bueno que está a salvo. #{j['num']}: el trabajo fuera de horario cuesta más. Responda 1 EMERGENCIA o 2 NORMAL.")
            return
        if st == "urgent":
            j["urgent"] = low.startswith("1") or "emerg" in low
            h["await"] = None
            if j["status"] == "Intake": return self.intake_continue(j)
            return
        if st == "clarify":
            opts = h.get("clar_opts", []); pick = None
            if low.startswith("1"): pick = opts[0]
            elif low.startswith("2") and len(opts) > 1: pick = opts[1]
            else:
                for o in opts:
                    if o.split("_")[0] in low: pick = o
            if pick is None:
                j["clar"] += 1
                if j["clar"] >= 2:
                    h["await"] = None; j["clar_fallback"] = True; return self.intake_continue(j)
                return self.ask_clarify(h, j, opts, h.get("clar_kind", "cat"))
            if h.get("clar_kind") == "cat":
                j["cat"] = pick
                jts = [x[0] for x in JOBTYPES.get(pick, [(pick,)])]
                if j["jt"] not in jts: j["jt"] = jts[0]
                j["clarified_cat"] = True
            else:
                j["jt"] = pick; j["clarified_jt"] = True
            h["await"] = None; return self.intake_continue(j)
        if st == "zip":
            m = re.search(r"\b(\d{5})\b", body)
            if m: j["zip"] = m.group(1); h["await"] = None; return self.intake_continue(j)
            return
        if st == "addr":
            h["fname"] = body.split(",")[0].strip()[:20]; h["addr"] = body
            j["addr_given"] = True; h["await"] = None
            return self.intake_continue(j)
    def ask_clarify(self, h, j, opts, kind="cat"):
        h["await"] = "clarify"; h["await_job"] = j["num"]; h["clar_opts"] = opts; h["clar_kind"] = kind
        lab = [o.replace("_", " ") for o in opts]
        self.sms(h["phone"], "clarify", f"For #{j['num']}: is this mainly 1) {lab[0]} or 2) {lab[1]}?", job=j["num"], direct=True,
                 es=f"Para #{j['num']}: ¿es principalmente 1) {lab[0]} o 2) {lab[1]}?")
        self.sim.world.clarify_asked(j["num"], opts, kind)
    def intake_continue(self, j):
        h = self.db["ho"][j["phone"]]
        if j["status"] != "Intake": return
        if h["consent"] is None: return
        if j.get("safety_t") and not j["safe"]:
            h["await"] = "safe"; h["await_job"] = j["num"]; return
        if j.get("safety_t") and j["urgent"] is None:
            h["await"] = "urgent"; h["await_job"] = j["num"]; return
        if j["ai"] is None:
            try:
                j["ai"] = self.ai_read(j)
            except AIDown:
                if not j.get("ai_down_t"):
                    j["ai_down_t"] = self.now()
                    if not self.db.get("ai_alert_open"):
                        self.db["ai_alert_open"] = True; self.alert("ai_down", "high")
                self.timer(self.now() + 15, "intake_retry", j["num"]); return
            r = j["ai"]
            j["cat"] = r["cat"]; j["jt"] = r["jobtype"]; j["tenant"] = r["rental"]; j["card"] = r["card_text"]
            if r["lang"] == "es": j["lang"] = "es"; h["lang"] = "es"
            if r["emergency"] and not j.get("safety_t"):
                self.safety_text(h, j, None); return
        if not j.get("clarified_cat") and not j.get("clar_fallback"):
            kc = self.sim.world.keyword_category(j["desc_raw"])
            if kc and kc != j["cat"]:
                return self.ask_clarify(h, j, [kc, j["cat"]], "cat")
        if not j.get("clarified_jt") and not j.get("clar_fallback"):
            kt = self.sim.world.keyword_jobtype(j["cat"], j["desc_raw"])
            if kt and kt != j["jt"] and self.lic_req(j["cat"], kt) != self.lic_req(j["cat"], j["jt"]):
                return self.ask_clarify(h, j, [kt, j["jt"]], "jt")
        if j["zip"] is None:
            m = re.search(r"\b(86\d{3})\b", j["desc_raw"])
            if m: j["zip"] = m.group(1)
            else:
                h["await"] = "zip"; h["await_job"] = j["num"]
                self.sms(h["phone"], "zip_q", f"What ZIP code is the job in (#{j['num']})?", job=j["num"], direct=True,
                         es=f"¿En qué código postal es el trabajo (#{j['num']})?")
                return
        if j["zip"] not in ("86403", "86404", "86405", "86406"):
            if self.set_status(j, "Closed"):
                j["close_reason"] = "out_of_area"
                self.sms(h["phone"], "out_of_area", f"Sorry, #{j['num']} is outside the area we cover right now.", job=j["num"], direct=True,
                         es=f"Lo sentimos, #{j['num']} está fuera de nuestra área.")
            return
        if not j.get("addr_given") and h.get("addr") is None:
            h["await"] = "addr"; h["await_job"] = j["num"]
            self.sms(h["phone"], "addr_q", f"For #{j['num']}: what's the street address and your first name? We share it only with the business you choose, after it confirms.",
                     job=j["num"], direct=True, es=f"Para #{j['num']}: ¿cuál es la dirección y su nombre? Solo lo compartimos con el negocio que elija.")
            return
        for jn in h["jobs"]:
            o = self.db["jobs"][jn]
            if o["num"] != j["num"] and o["status"] in LIVE and o["status"] != "Intake" and o["cat"] == j["cat"]:
                if self.set_status(j, "Closed"):
                    j["close_reason"] = "duplicate"
                    self.sms(h["phone"], "dup", f"#{j['num']} looks like the same job as #{o['num']}, so we added it there.", job=j["num"], direct=True,
                             es=f"#{j['num']} parece el mismo trabajo que #{o['num']}; lo agregamos ahí.")
                return
        j["ok_photos"] = []
        if not self.fix["no_photos_on_cards"]:
            for ph in self.sim.world.photos_of(j["num"]):
                try:
                    ident = self.sim.claude.screen_photo(self.sim.eng.t, ph); j["seen_by_ai"].append(("photo", ph["id"]))
                except AIDown:
                    ident = True
                if not ident: j["ok_photos"].append(ph["id"])
        self.match_and_send(j)
    def t_intake_retry(self, num):
        j = self.db["jobs"][num]
        if j["status"] == "Intake":
            self.intake_continue(j)
            if j["ai"] is not None: self.db["ai_alert_open"] = False
    def ai_read(self, j):
        h = self.db["ho"][j["phone"]]
        text = j["desc_raw"]
        if self.fix["code_scrub_before_ai"]:
            text = strong_scrub(text, [h.get("fname"), h.get("addr")])
        cached = self.sim.claude_cache.get(j["num"])
        if self.replaying and cached is not None:
            r = cached
        else:
            truth = self.sim.world.truth_for_ai(j["num"], text)
            r = self.sim.claude.read_request(self.sim.eng.t, text, truth)
            self.sim.claude_cache[j["num"]] = r
        j["seen_by_ai"].append(("text", text))
        return r
    def lic_req(self, cat, jt):
        for x in JOBTYPES.get(cat, []):
            if x[0] == jt and x[5] is not None: return x[5]
        return CATS[cat][3]
    def jt_flags(self, cat, jt):
        for x in JOBTYPES.get(cat, []):
            if x[0] == jt: return {"gas": x[2], "permit": x[3], "fire": x[4]}
        return {"gas": 0, "permit": 0, "fire": 0}
    def covers(self, b, cat, jt):
        req = self.lic_req(cat, jt); fl = self.jt_flags(cat, jt)
        if cat not in b["cats"]: return False
        if req is None: return b["lic_kind"] != "PEST" or cat == "landscape"
        if req == "PEST": return b["lic_kind"] == "PEST"
        if b["lic_kind"] == "PEST": return False
        if b["lic_kind"] is None:
            if not CATS[cat][4] and CATS[cat][3] is not None: return False
            if fl["gas"] or fl["fire"]: return False
            if fl["permit"] and self.fix["permit_question"]: return False
            return True
        if req == ("ROC-any",): return True
        return any(c in b["classes"] for c in req)
    def match_and_send(self, j):
        now = self.now(); out = []
        for bid, b in self.db["biz"].items():
            if not b["active"] or b["paused"] or not b["verified"]: continue
            if not self.covers(b, j["cat"], j["jt"]): continue
            if j["tenant"] and b.get("skip_tenant"): continue
            if j.get("urgent") and not b.get("after_hours"): continue
            if b.get("max_cards") and self.cards_this_week(b) >= b["max_cards"]: continue
            if b["lic_kind"] in ("ROC", "PEST") and now - b.get("lic_checked", -1e9) > 12 * HOUR:
                st = self.sim.roc.check(now, b["lic"]); b["lic_checked"] = now
                if st == "inactive": self.pause_biz(b, "license_inactive"); continue
                if st == "unknown":
                    if b.get("unknown_since") is None: b["unknown_since"] = now
                else: b["unknown_since"] = None
            out.append(bid)
        if not out:
            if self.set_status(j, "Not covered"):
                self.db["demand_log"].append((now, j["cat"], j["jt"]))
                self.sms(j["phone"], "not_covered", f"Sorry, no qualified local business covers #{j['num']} yet. We've logged it so we can recruit one.",
                         job=j["num"], direct=True, es=f"Lo sentimos, aún no hay un negocio calificado para #{j['num']}.")
            return
        self.set_status(j, "Open")
        j["open_t"] = now; j["recipients"] = out
        j["window_end"] = add_clock_hours(now, 24, weekdays_only=self.cfg["quote_window"] == "weekdays")
        tags = []
        if j["lang"] == "es": tags.append("Homeowner prefers Spanish.")
        if j["tenant"]: tags.append("Requester is a tenant.")
        if j.get("urgent"): tags.append("EMERGENCY, after-hours.")
        if self.fix["handyman_disclosure"]: pass
        card = code_last_pass(j["card"] or "")
        for bid in out:
            b = self.db["biz"][bid]
            b.setdefault("cards", []).append(now)
            body = (f"New job #{j['num']}: {j['cat'].replace('_', ' ')} / {j['jt'].replace('_', ' ')}. {card} {' '.join(tags)} "
                    f"Quote by {fmt_t(j['window_end'])}: {DOMAIN}/q/{j['token']}")
            self.sms(b["phone"], "card", body, job=j["num"], mms=len(j["ok_photos"]), aud="biz")
            self.sim.world.card_delivered_to(bid, j["num"], {"photos": list(j["ok_photos"]), "body": body, "tags": list(tags)})
        tomorrow = (day(now) + 1) * DAY
        self.timer(tomorrow + 18 * HOUR, "noq_text", j["num"])
        self.timer(tomorrow + 21 * HOUR, "noq_status", j["num"])
        j["promise_t"] = tomorrow + 21 * HOUR
        self.sms(j["phone"], "opened", f"Got it. Request #{j['num']} is out to {len(out)} local business(es). You'll hear from us by 9 p.m. tomorrow. Reply CLOSE {j['num']} to close it.",
                 job=j["num"], direct=True, es=f"Listo. La solicitud #{j['num']} se envió a {len(out)} negocio(s). Tendrá noticias antes de las 9 p.m. de mañana. Responda CLOSE {j['num']} para cerrarla.")
    def cards_this_week(self, b):
        now = self.now(); return sum(1 for t in b.get("cards", []) if now - t < 7 * DAY)
    def t_noq_text(self, num):
        j = self.db["jobs"][num]
        if j["status"] != "Open" or j["quotes"]: return
        self.sms(j["phone"], "noq", f"No quotes yet for #{num}. We'll keep looking and text you when one arrives.", job=num,
                 es=f"Aún no hay cotizaciones para #{num}. Seguimos buscando.")
    def t_noq_status(self, num):
        j = self.db["jobs"][num]
        if j["status"] != "Open": return
        self.set_status(j, "No quotes yet")
        self.timer(self.now() + 5 * DAY, "noq_close", num)
    def t_noq_close(self, num):
        j = self.db["jobs"][num]
        if j["status"] != "No quotes yet": return
        if self.set_status(j, "Closed"):
            j["close_reason"] = "no_quotes"
            self.sms(j["phone"], "closed_notice", f"Sorry, no business quoted #{num}, so we've closed it. Text us any time.", job=num,
                     es=f"Lo sentimos, nadie cotizó #{num}; la cerramos.")
