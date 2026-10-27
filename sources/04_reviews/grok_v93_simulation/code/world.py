"""The simulated outside world: demand, homeowners, businesses, Casey, licence records, city list.
Holds the TRUTH the checks compare vendor logs against. All rates Assumed unless the brief gives them."""
import math, random, re
from engine import DAY, HOUR, day, hod, add_bmin
from config import CATS, JOBTYPES, POOL, GROUP_CATS, GROUP_LIC, CAC, FEES
from world_people import Homeowner, DESC, DIRECT_EMER, INDIRECT_EMER, INDIRECT_TRUE, bt
from world_biz import Biz, Holder, Casey, p_confirm, biz_hours

KW_CAT = [("pool heater", "pool_repair"), ("pool pump", "pool_repair"), ("pool filter", "pool_repair"), ("losing water", "pool_repair"),
          ("pool cleaning", "pool_cleaning"), ("water heater", "plumbing"), ("leak", "plumbing"), ("pipe", "plumbing"), ("toilet", "plumbing"),
          ("drain", "plumbing"), ("sink", "plumbing"), ("gas line", "plumbing"), ("replace our", "hvac_replace"), ("ac ", "hvac_repair"),
          ("air conditioner", "hvac_repair"), ("furnace", "hvac_repair"), ("thermostat", "hvac_repair"), ("duct", "hvac_repair"),
          ("outlet", "electrical"), ("breaker", "electrical"), ("panel", "electrical"), ("ceiling fan", "electrical"), ("drywall", "handyman"),
          ("doors", "handyman"), ("shelves", "handyman"), ("fence", "handyman"), ("smoke alarm", "handyman"), ("dryer", "handyman"),
          ("wall", "handyman"), ("weed", "landscape"), ("yard", "landscape"), ("house cleaning", "house_cleaning"), ("pest", "pest"),
          ("scorpion", "pest"), ("dog waste", "dog_waste"), ("trash can", "trash_can"), ("turnover", "turnover"), ("junk", "junk"),
          ("car detail", "car_detail"), ("car ", "car_detail"), ("full detail", "boat_detail"), ("outboard", "boat_repair"),
          ("window cleaning", "window"), ("pressure wash", "pressure_wash"), ("solar screen", "solar_screens"), ("roof", "roof"),
          ("paint", "painting"), ("movers", "movers"), ("washer", "appliance"), ("range", "appliance"), ("garage door", "garage_door")]
KW_JT = {"plumbing": [("gas water heater", "water_heater_gas"), ("gas line", "gas_line"), ("electric water heater", "water_heater_elec")],
         "handyman": [("gas", "gas_dryer_hookup"), ("smoke alarm", "smoke_alarm_hardwired"), ("wall", "wall_removal")],
         "landscape": [("weed", "weed_control")], "appliance": [("gas", "gas_appliance")], "hvac_repair": [("gas furnace", "furnace_gas")],
         "pool_repair": [("heater", "heater_gas")], "pressure_wash": [("seal", "seal_paint")]}
TZ = {"206": -1, "503": -1, "415": -1, "612": 1, "312": 1, "218": 1, "303": 0, "720": 0}   # A: winter offsets vs Arizona

class World:
    def __init__(self, sim):
        self.sim = sim; self.cfg = sim.cfg; self.seed = sim.seed
        self.rng = random.Random(sim.seed * 7 + 3)
        self.end_t = self.cfg["days"] * DAY
        self.phones = {}; self.emails = {}; self.hos = []; self.biz = {}; self.holders = {}
        self.truth = {}           # num -> request truth
        self.reqs = []            # all request truths
        self.said = []; self.intent = []; self.shown = {}; self.picks_intended = []; self.cards = []
        self.details_to = []; self.casey_decisions = []; self.callbacks_done = []; self.charged = []
        self.biz_events = []; self.offplatform = []; self.fee_avoided = []; self.direct_contacts = []
        self.done_jobs = {}; self.problems = {}; self.optout_ho = 0; self.optout_biz = 0
        self.casey = Casey(self)
        self.ho_n = 0; self.biz_n = 0
    # ---------- registry
    def reg_phone(self, ph, obj): self.phones[ph] = obj
    def phone_owner(self, ph): return self.phones.get(ph)
    def email_owner(self, em): return self.emails.get(em)
    def tz_offset(self, ph): return 0 if ph.startswith("+1928") else TZ.get(ph[2:5], 0)
    def out_of_state(self, ph): return not ph.startswith("+1928")
    def web(self, kind, *args):
        """A web-page action by a person (outside event). Logged for restore replay."""
        t = self.sim.eng.t; self.sim.web_log.append((t, kind, args))
        return self.sim.do_web(kind, args)
    # ---------- setup
    def setup(self):
        c = self.cfg
        mult = c["adversarial_mult"]
        bp = dict(c["bpersonas"])
        for k in ("bypasser", "shader", "tripgamer", "noshow"): bp[k] *= mult
        tot = sum(bp.values()); self.bp = {k: v / tot for k, v in bp.items()}
        hp = dict(c["personas"])
        if c.get("spanish_share_override") is not None:
            sp = c["spanish_share_override"]; rest = 1 - hp["spanish"]
            hp = {k: (v * (1 - sp) / rest if k != "spanish" else sp) for k, v in hp.items()}
        for k in ("oversharer", "injector", "canceller", "stopreturn", "forwarder"): hp[k] *= mult
        tot = sum(hp.values()); self.hp = {k: v / tot for k, v in hp.items()}
        # businesses: pool per group; join decision by Part 9 join rule
        self.join_rule_p = {}
        idx = 0
        for g, (nlic, ntot) in POOL.items():
            if c["approved_scope"] and g not in ("plumbing", "hvac", "electrical"): continue
            if not any(cat in c["categories"] for cat in GROUP_CATS[g]): continue
            pj = self.join_prob(g)
            ov = (c.get("per_category_override") or {})
            for k in range(ntot):
                idx += 1
                joins = self.rng.random() < pj
                persona = self.rng.choices(list(self.bp), list(self.bp.values()))[0]
                b = Biz(self, f"B{idx:04d}", g, persona, idx=idx)
                b.joins = joins; b.join_t = None
                self.add_record(b)
                self.biz[b.bid] = b
            if g in ov:
                members = [b for b in self.biz.values() if b.group == g]
                need = ov[g]
                while len(members) < need:
                    idx += 1; persona = self.rng.choices(list(self.bp), list(self.bp.values()))[0]
                    b = Biz(self, f"B{idx:04d}", g, persona, idx=idx); self.add_record(b); self.biz[b.bid] = b; members.append(b)
                for i, b in enumerate(members): b.joins = i < need
        horizon = min(self.end_t, 365 * DAY)
        for b in self.biz.values():
            if not b.joins: continue
            if self.rng.random() < c["launch_fraction"]: b.join_t = self.rng.uniform(0, 3) * DAY + 9 * HOUR
            else: b.join_t = self.rng.uniform(3, 365) * DAY + self.rng.uniform(8, 18) * HOUR
            if c["growth"] and self.rng.random() < 0.5 and self.end_t > 365 * DAY:
                b.join2 = True   # A: year 2 adds a second wave equal to half of year 1 (new firms / late adopters)
            if b.join_t < self.end_t: self.sim.eng.at(b.join_t, self.biz_join, b.bid)
        if c["growth"] and self.end_t > 365 * DAY:
            extra = [b for b in self.biz.values() if not b.joins and self.rng.random() < 0.10]
            for b in extra:
                b.joins = True; b.join_t = self.rng.uniform(365, self.end_t / DAY) * DAY + 10 * HOUR
                self.sim.eng.at(b.join_t, self.biz_join, b.bid)
        # fakes (Part 8.2): half borrow a real licence, half claim an unlicensed category under a made-up name
        nf = int(round(c["fake_businesses"] * mult))
        for k in range(nf):
            idx += 1
            if k % 2 == 0:
                cands = [b for b in self.biz.values() if not b.joins and b.lic_kind == "ROC" and not b.fake]
                g = cands[k % len(cands)].group if cands else "plumbing"
                f = Biz(self, f"F{idx:04d}", g, "responsive", fake=True, idx=idx)
                if cands:
                    victim = cands[self.rng.randrange(len(cands))]
                    f.lic = victim.lic; f.classes = victim.classes; f.lic_kind = "ROC"; f.borrowed = victim
                f.cats = list(GROUP_CATS[g])
            else:
                g = self.rng.choice(["cleaning", "junk", "handyman"])
                f = Biz(self, f"F{idx:04d}", g, "responsive", fake=True, idx=idx); f.lic_kind = None; f.lic = None; f.classes = ()
                f.borrowed = None
            f.joins = True; f.join_t = self.rng.uniform(1, min(45, self.end_t / DAY - 1)) * DAY + 11 * HOUR
            self.biz[f.bid] = f
            self.reg_phone(f.phone, f)
            if f.email: self.emails[f.email] = f
            self.sim.eng.at(f.join_t, self.biz_join, f.bid)
        # monthly business decisions
        for m in range(1, int(self.end_t // (30 * DAY)) + 1):
            self.sim.eng.at(m * 30 * DAY + 6 * HOUR, self.month_tick, m)
        self.schedule_demand()
        self.casey.start(int(self.end_t // DAY))
        if c.get("dispute_wave"):
            d0, n = c["dispute_wave"]
            for k in range(n): self.sim.eng.at((d0 + k * 7 / n) * DAY + 12 * HOUR, self.force_dispute)
    def add_record(self, b):
        if b.lic_kind in ("ROC", "PEST"):
            lapse = None
            if b.persona == "liclapse": lapse = self.rng.uniform(5, max(6, self.end_t / DAY - 5)) * DAY
            self.sim.roc.records[b.lic] = {"holder": b.name, "classes": b.classes, "lapse_t": lapse, "phone": b.roc_phone,
                                          "email": b.email if self.rng.random() < 0.7 else None, "addr": b.bid}
        self.reg_phone(b.phone, b)
        if b.roc_phone != b.phone: self.reg_phone(b.roc_phone, b)
        for s in b.staff: self.reg_phone(s, b)
        if b.email: self.emails[b.email] = b
    def fee_typical(self, g, fees):
        F = FEES[fees]; vals = []
        from app_flow import fee_for
        for cat in GROUP_CATS[g]:
            v = CATS[cat][1]
            q = {"kind": "recurring", "monthly": v / 12} if CATS[cat][2] else {"kind": "fixed", "amount": v}
            vals.append(fee_for({"fees": fees}, cat, q))
        return sum(vals) / len(vals)
    def join_prob(self, g):
        """Part 9 joining rule (A): base U(15%,30%) (G range) scaled by how the typical fee compares with
        Part 6.3 cost-to-win-a-customer: factor = (1 - fee/CAC) / (1 - base_fee/CAC), capped 0..1.3."""
        base = self.rng.uniform(*self.cfg["launch_join_share"])
        cac = CAC.get(g, 120)
        f = self.fee_typical(g, self.cfg["fees"]); f0 = self.fee_typical(g, "base")
        fac = max(0.0, 1 - f / cac) / max(0.05, 1 - f0 / cac)
        if self.cfg["free_first_win"] == "first90": fac *= 0.95
        p = base * min(1.3, fac); self.join_rule_p[g] = p
        return p
    # ---------- demand
    def rate(self, t):
        c = self.cfg; d = t / DAY
        if c["growth"]:
            g = c["growth"]; m = d / 30
            if m <= 12: return g["m1"] + (g["m12"] - g["m1"]) * m / 12
            return g["m12"] + (g["m24"] - g["m12"]) * (m - 12) / 12
        return c["requests_per_day"]
    def month_of_year(self, t): return int((self.cfg["start_month"] - 1 + t / (30.4 * DAY)) % 12) + 1
    def cat_weights(self, t):
        c = self.cfg; moy = self.month_of_year(t); S = c["season"]
        w = {}
        for cat in c["categories"]:
            x = c["category_share_override"].get(cat, CATS[cat][0])
            if cat in ("hvac_repair", "hvac_replace") and moy in (6, 7, 8, 9): x *= S["hvac_summer_mult"]
            if cat in ("boat_detail", "boat_repair") and moy in (4, 5, 6, 7, 8, 9): x *= S["boat_season_mult"]
            if cat in ("house_cleaning", "window", "turnover", "landscape") and moy in (11, 12, 1, 2, 3): x *= S["snowbird_clean_mult"]
            hw = c.get("heat_wave")
            if hw and cat == "hvac_repair" and hw[0] * DAY <= t < (hw[0] + hw[1]) * DAY: x *= hw[2]
            w[cat] = x
        return w
    def schedule_demand(self):
        r = self.rng; t = 0.0
        days = int(self.end_t // DAY)
        for d in range(days):
            lam = self.rate(d * DAY + 12 * HOUR)
            hw = self.cfg.get("heat_wave")
            if hw and hw[0] <= d < hw[0] + hw[1]: lam *= 1.3
            n = self.poisson(lam)
            for _ in range(n):
                if r.random() < self.cfg["p_overnight"]: h = r.choice([r.uniform(21, 24), r.uniform(0, 7)])
                else: h = r.triangular(7, 21, 10)
                ta = d * DAY + h * 60
                if r.random() < self.cfg["p_business_text_in"]: self.sim.eng.at(ta, self.biz_text_in); continue
                self.sim.eng.at(ta, self.arrival)
    def poisson(self, lam):
        if lam > 30: return max(0, int(round(self.rng.gauss(lam, math.sqrt(lam)))))
        L = math.exp(-lam); k = 0; p = 1.0
        while True:
            p *= self.rng.random()
            if p < L: return k
            k += 1
    def biz_text_in(self):
        self.biz_n += 1; ph = f"+1928{6000000 + self.biz_n:07d}"
        class Asker:
            landline = False
            def __init__(s): s.got = []
            def receive(s, to, sid): s.got.append(sid)
            def mobile_for(s, p): return True
            def dead_number(s, p): return False
            def answers_robot(s, p): return True
        a = Asker(); self.reg_phone(ph, a)
        body = self.rng.choice(["How do I get on your list? I do pool service", "I'm a contractor, send me jobs", "hi, can my business sign up?",
                                "Do you take handymen? want work"])
        self.intent.append({"t": self.sim.eng.t, "phone": ph, "kind": "biz_join_text", "body": body})
        self.said.append({"t": self.sim.eng.t, "who": ph, "body": body, "kind": "biz_join", "job": None, "lang": "en"})
        self.sim.tw.receive_inbound(self.sim.eng.t, ph, body)
    def arrival(self):
        t = self.sim.eng.t; r = self.rng
        old = [h for h in self.hos if h.optout_t is None and h.persona != "stopreturn"]
        if old and r.random() < self.cfg["p_repeat"]:
            h = r.choice(old)
            sat = getattr(h, "satisfied", None)
            if sat is False and r.random() < 0.5: h = None
            elif sat is True or r.random() < 0.67: pass
        else: h = None
        if h is None:
            self.ho_n += 1
            persona = r.choices(list(self.hp), list(self.hp.values()))[0]
            if self.cfg.get("snowbird") and persona == "normal" and r.random() < 0.15: persona = "absent"
            h = Homeowner(self, self.ho_n, persona); self.hos.append(h); self.reg_phone(h.phone, h)
            if h.email: self.emails[h.email] = h
        truth = self.new_truth(h, t)
        h.start_request(t, truth)
    def new_truth(self, h, t):
        r = self.rng
        w = self.cat_weights(t)
        if h.persona == "emergency":
            if r.random() < 0.5:
                txt = r.choice(DIRECT_EMER); indirect = False
                cat, jt = ("plumbing", "water_heater_gas") if "gas" in txt else (("electrical", "breaker") if "spark" in txt else
                          (("hvac_repair", "ac_not_cooling") if "AC" in txt else (("plumbing", "leak") if "flood" in txt else ("hvac_repair", "furnace_gas"))))
            else:
                txt = r.choice(INDIRECT_EMER); indirect = True
                key = next(k for k in INDIRECT_TRUE if k in txt); cat, jt = INDIRECT_TRUE[key]
            if cat not in self.cfg["categories"]: cat = "plumbing"; jt = "leak"
            emer = True
        else:
            cat = r.choices(list(w), list(w.values()))[0]
            jts = JOBTYPES.get(cat, [(cat, 1, 0, 0, 0, None)])
            jt = r.choices([x[0] for x in jts], [x[1] for x in jts])[0]
            txt = None; indirect = False; emer = False
        d = DESC[cat]
        desc = r.choice(d[jt] if isinstance(d, dict) else d)
        if h.persona == "tenant": desc += ", I rent and my landlord said it's ok"
        v = CATS[cat][1] * math.exp(r.gauss(0, self.cfg["value_sigma"]))
        photos = []
        if r.random() < self.cfg["p_photo"]:
            for k in range(r.randint(1, 3)):
                photos.append({"id": f"P{h.idx}-{len(self.reqs)}-{k}", "identifying": r.random() < self.cfg["p_photo_identifying"]})
        tr = {"ho": h, "phone": h.phone, "t0": t, "cat": cat, "jt": jt, "value": v, "emergency": emer, "indirect": indirect,
              "emer_text": txt, "urgent_truth": emer and r.random() < 0.7, "desc": desc, "short": r.choice(["AC broken", "need plumber", "help"]),
              "photos": photos, "tenant": h.persona == "tenant", "spanish": h.lang == "es", "nums": [], "injection": False,
              "pii_in_raw": False, "idx": len(self.reqs)}
        self.reqs.append(tr)
        return tr
    # ---------- app hooks: intake
    def job_created(self, num, phone, t, body):
        h = self.phones.get(phone)
        tr = getattr(h, "cur", None) if isinstance(h, Homeowner) else None
        if tr is None: return
        if tr["nums"] and tr["nums"][-1] in self.truth and t - tr["t0"] > 0 and tr.get("short_first") is not True and len(tr["nums"]) >= 1:
            pass
        tr["nums"].append(num); self.truth[num] = tr
    def safety_sent(self, num):
        tr = self.truth.get(num)
        if tr is not None and "safety_t" not in tr: tr["safety_t"] = self.sim.eng.t
    def clarify_asked(self, num, opts, kind):
        tr = self.truth.get(num)
        if tr is not None: tr.setdefault("clarify", []).append((self.sim.eng.t, opts, kind))
    def keyword_category(self, text):
        low = " " + text.lower() + " "
        for k, cat in KW_CAT:
            if k in low and cat in self.cfg["categories"]: return cat
        return None
    def keyword_jobtype(self, cat, text):
        low = text.lower()
        for k, jt in KW_JT.get(cat, []):
            if k in low: return jt
        return None
    def pii_tokens(self, h):
        return [h.fname.lower(), h.lname.lower(), h.addr.lower(), h.addr.split()[0], h.phone[-7:], h.phone[-4:]]
    def pii_found(self, h, text):
        low = text.lower(); digits = re.sub(r"\D", "", text)
        hits = []
        if re.search(r"\b" + h.fname.lower() + r"\b", low): hits.append("first name")
        if re.search(r"\b" + h.lname.lower() + r"\b", low): hits.append("last name")
        if h.addr.split()[0] in text and h.addr.split()[1].lower() in low: hits.append("street address")
        if h.phone[-7:] in digits or h.phone[-4:] in re.sub(r"\D", "", text.replace(" ", "")) and len(digits) >= 4 and "phone" not in hits:
            if h.phone[-4:] in digits: hits.append("phone digits")
        return hits
    def truth_for_ai(self, num, text):
        tr = self.truth[num]; h = tr["ho"]
        pii = self.pii_found(h, text)
        frag = f"(ask for {h.fname})" if "first name" in pii else ("near " + h.addr if "street address" in pii else "")
        return {"cat": tr["cat"], "jobtype": tr["jt"], "emergency": tr["emergency"] or bool(tr.get("injection") and False),
                "indirect": tr["indirect"], "injection": tr.get("injection"), "injection_kind": "urgent", "spanish": tr["spanish"],
                "tenant": tr["tenant"], "card_core": DESC_EN(tr), "pii_in_text": bool(pii), "pii_fragment": frag}
    def photos_of(self, num):
        tr = self.truth.get(num)
        return list(tr["photos"]) if tr else []
    # ---------- cards and quotes
    def card_delivered_to(self, bid, num, card):
        if self.sim.app.replaying: self.replay_hooks = getattr(self, 'replay_hooks', 0) + 1; return
        b = self.biz[bid]; tr = self.truth.get(num); t = self.sim.eng.t
        self.cards.append({"t": t, "bid": bid, "num": num, "card": card, "fake": b.fake})
        if tr is None or b.left is not None: return
        tr.setdefault("cards_to", []).append(bid)
        if b.persona == "stopby" and self.rng.random() < 0.3:
            self.sim.eng.at(biz_hours(b.rng, t, 10, 120), self.biz_says, b, "can stop by at 3 to look")
        wr = (b.wins + 1) / (b.quotes + 4)
        pq = b.base_q * (0.5 + 0.5 * min(1.0, wr / 0.25))       # Part 9 quoting rule (A)
        if b.rng.random() > pq: return
        lo, hi = {"responsive": (15, 150), "busy": (240, 1800)}.get(b.persona, (60, 600))
        tq = biz_hours(b.rng, t, lo, hi)
        app = self.sim.app; j = app.db["jobs"].get(num)
        if j and tq > j.get("window_end", 1e18) and b.persona != "busy": tq = j["window_end"] - b.rng.uniform(5, 60)
        if j and tq > j.get("window_end", 1e18): return      # A: businesses respect the deadline shown on the card
        self.sim.eng.at(max(t + 5, tq), self.biz_quote, bid, num)
    def make_quote(self, b, tr):
        r = b.rng; v = tr["value"]; cat = tr["cat"]
        true_price = v * r.uniform(0.85, 1.2)
        q = {"true": true_price}
        if CATS[cat][2]:
            q.update(kind="recurring", monthly=round(true_price / 12, 0))
        else:
            x = r.random()
            if b.persona == "shader" and (500 <= true_price < 600 or 1500 < true_price <= 1750):
                q.update(kind="fixed", amount=(r.choice([475, 489, 495]) if true_price < 600 else r.choice([1450, 1475, 1495])), shaded=True)
            elif b.persona == "tripgamer" and true_price >= 500:
                q.update(kind="trip", amount=r.choice([79, 89, 99]), gamed=True)
            elif x < 0.70: q.update(kind="fixed", amount=round(true_price, 0))
            elif x < 0.90: q.update(kind="range", lo=round(true_price * 0.85), hi=round(true_price * 1.15))
            elif cat in ("plumbing", "hvac_repair", "electrical", "appliance", "garage_door", "pool_repair", "boat_repair", "roof"):
                q.update(kind="trip", amount=r.choice([79, 89, 99, 129]))
            else: q.update(kind="fixed", amount=round(true_price, 0))
        txt = ""
        if b.persona == "bypasser":
            txt = r.choice([f"call me direct {b.phone[2:5]} {b.phone[5:8]} {b.phone[8:]}", "look us up, " + b.name, "google " + b.name + " and call"])
        q["text"] = txt
        return q
    def biz_quote(self, bid, num):
        b = self.biz[bid]; tr = self.truth.get(num)
        if tr is None or b.left is not None: return
        q = self.make_quote(b, tr); b.quotes += 1
        tr.setdefault("quotes", {})[bid] = q
        via_sms = b.persona == "linkaverse" or b.rng.random() < 0.15
        qq = {k: v for k, v in q.items() if k not in ("true", "shaded", "gamed")}
        if via_sms:
            amt = qq.get("amount") or qq.get("monthly") or qq.get("lo")
            self.biz_says(b, f"{num} ${amt:.0f} {q['text']}".strip())
        else:
            res = self.web("quote", bid, num, qq)
            q["app_result"] = res
            if res == "bounced" and b.rng.random() < 0.7:
                q2 = dict(qq); q2["text"] = ""
                self.sim.eng.at(self.sim.eng.t + b.rng.uniform(10, 90), self.web, "quote", bid, num, q2)
    def biz_says(self, b, body):
        t = self.sim.eng.t
        self.said.append({"t": t, "who": b.phone, "body": body, "kind": "biz", "job": None, "lang": "en"})
        self.sim.tw.receive_inbound(t, b.phone, body)
    def biz_sms_quote_routed(self, bid): return True
    def biz_quote_truth(self, bid, num):
        tr = self.truth.get(num)
        if tr is None or bid not in tr.get("quotes", {}): return None
        q = tr["quotes"][bid]; return {k: v for k, v in q.items() if k not in ("true", "shaded", "gamed")}
    def quote_needs_confirm(self, bid, num):
        if self.sim.app.replaying: self.replay_hooks = getattr(self, 'replay_hooks', 0) + 1; return
        b = self.biz[bid]
        if b.rng.random() < 0.85:
            q = self.biz_quote_truth(bid, num)
            if q: q["via"] = "web"; self.sim.eng.at(biz_hours(b.rng, self.sim.eng.t, 10, 240), self.web, "quote", bid, num, q)
    def quotes_shown(self, num, order, sid):
        if sid and sid != "held": self.shown[sid] = list(order)
        else: self.shown.setdefault("held:" + str(num), []).append(list(order))
    # ---------- homeowner pick behaviour
    def on_quotes_seen(self, h, num, sid, tpl):
        tr = self.truth.get(num)
        if tr is None or tr.get("pick_sched") or tr.get("picked_by_ho") and tpl not in ("quotes_after_fail", "quotes_after_noshow"): return
        if tpl in ("quotes_after_fail", "quotes_after_noshow"): tr["picked_by_ho"] = False
        tr["pick_sched"] = True
        p = h.persona; r = h.rng
        lo, hi = {"impatient": (3, 40), "absent": (240, 2000)}.get(p, (20, 600))
        self.sim.eng.at(bt(r, self.sim.eng.t, lo, hi, offset_h=h.tz_off), self.ho_decide, h, num, sid)
    def ho_decide(self, h, num, trigger_sid):
        tr = self.truth[num]; tr["pick_sched"] = False; r = h.rng
        if h.optout_t is not None or tr.get("cancelled"): return
        app = self.sim.app; j = app.db["jobs"].get(num)
        # which list does the person have in mind? latest seen with p .6 (A), else the one that prompted them
        seen_t, latest = h.seen.get(num, (None, []))
        lst = latest if (r.random() < 0.6 or trigger_sid not in self.shown) else self.shown[trigger_sid]
        lst = [b for b in lst]
        if not lst: return
        nq = len(lst)
        pp = self.cfg["pick_base"] * (1 - self.cfg["pick_q_decay"] ** nq)
        if r.random() > pp:
            if r.random() < self.cfg["p_direct_contact"] * (1 + 4 * any(self.biz[b].persona == "bypasser" for b in lst)):
                b = next((b for b in lst if self.biz[b].persona == "bypasser"), lst[0])
                self.direct_contacts.append({"t": self.sim.eng.t, "num": num, "bid": b})
            return
        if h.persona == "impatient" or r.random() < 0.55:
            choice = lst[0]                 # cheapest / top of the list
        else:
            choice = r.choice(lst)
        n = lst.index(choice) + 1
        via = "text" if (h.persona == "nolinks" or r.random() < 0.55) else "web"
        rec = {"t": self.sim.eng.t, "num": num, "intended": choice, "n": n, "via": via, "ho": h.phone, "list": lst}
        self.picks_intended.append(rec); tr["picked_by_ho"] = True
        if via == "text":
            h.text(self.sim.eng.t, f"{n} YES" if h.lang == "en" else f"{n} SÍ", kind="pick", job=num)
        else:
            who = "spouse" if h.persona == "forwarder" and r.random() < 0.7 else "homeowner"
            rec["who"] = who
            vidx = self.web("pick", num, choice)
            if vidx is None: rec["web_failed"] = True; return
            self.pending_code = getattr(self, "pending_code", {}); self.pending_code[vidx] = (h, num, who, rec)
    def on_ho_code(self, h, vidx):
        pc = getattr(self, "pending_code", {})
        if vidx not in pc: return
        h2, num, who, rec = pc.pop(vidx)
        code = self.sim.tw.verify[vidx]["code"]
        if who == "spouse":
            if h.rng.random() < self.cfg["p_spouse_gets_code"]:
                rec["relayed"] = True
                self.sim.eng.at(self.sim.eng.t + h.rng.uniform(2, 15), self.web, "pick_code", num, code, "spouse_with_code")
            else:
                rec["spouse_blocked"] = True
            return
        self.sim.eng.at(self.sim.eng.t + h.rng.uniform(0.5, 4), self.web, "pick_code", num, code, "homeowner")
    # ---------- business confirm / pay
    def biz_picked(self, bid, num, dl):
        if self.sim.app.replaying: self.replay_hooks = getattr(self, 'replay_hooks', 0) + 1; return
        b = self.biz[bid]; tr = self.truth.get(num); app = self.sim.app; t = self.sim.eng.t
        j = app.db["jobs"][num]; q = j["quotes"][bid]
        ent = app.entity(app.db["biz"][bid]); free = app.free_eligible(ent)
        fee = 0.0 if free else q["fee"]
        val = tr["quotes"][bid]["true"] if tr and bid in tr.get("quotes", {}) else CATS[j["cat"]][1]
        if CATS[j["cat"]][2]: val = val          # first-year value for recurring
        s = fee / max(1.0, val)
        rel = {"responsive": .97, "busy": .85, "noshow": .92}.get(b.persona, .92)
        if b.persona == "smalldecliner" and val < 200: rel = 0.1
        if b.fake: rel = 1.0
        if b.left is not None: rel = 0.0
        pc = rel * (1.0 if fee == 0 else p_confirm(s, self.cfg["s50"]))
        ev = {"t": t, "bid": bid, "num": num, "fee": fee, "s": s, "p": pc, "free": free}
        go = b.rng.random() < pc; ev["confirm"] = go
        self.biz_events.append(("picked", ev))
        if b.rng.random() < self.cfg["p_bus_question"]:
            self.sim.eng.at(t + b.rng.uniform(2, 20), self.biz_says, b, b.rng.choice(["got the notice, what now?", "picked? what do I do"]))
        if not go: return
        lo, hi = (5, 60) if b.persona == "responsive" else ((30, 260) if b.persona == "busy" else (10, 150))
        tc = add_bmin(t, b.rng.uniform(lo, hi))
        if b.persona in ("linkaverse",) or b.rng.random() < 0.6:
            self.sim.eng.at(tc, self.biz_says, b, f"YES {num}")
        else:
            self.sim.eng.at(tc, self.web, "confirm", bid, num)
    def biz_needs_card(self, bid, num):
        if self.sim.app.replaying: self.replay_hooks = getattr(self, 'replay_hooks', 0) + 1; return
        b = self.biz[bid]
        if b.rng.random() < self.cfg["p_card_added"]:
            b.cust = {"id": f"cus_{bid}", "bid": bid, "ach": b.ach, "decline_mult": 1.0}
            self.sim.eng.at(add_bmin(self.sim.eng.t, b.rng.uniform(5, 70)), self.web, "card", bid, dict(b.cust))
    def biz_declined(self, bid, num):
        if self.sim.app.replaying: self.replay_hooks = getattr(self, 'replay_hooks', 0) + 1; return
        b = self.biz[bid]
        if b.rng.random() < 0.6:
            b.cust = {"id": f"cus_{bid}_2", "bid": bid, "ach": b.ach, "decline_mult": 0.3}
            self.sim.eng.at(add_bmin(self.sim.eng.t, b.rng.uniform(5, 70)), self.web, "card", bid, dict(b.cust))
    def biz_3ds(self, bid, num, cid):
        if self.sim.app.replaying: self.replay_hooks = getattr(self, 'replay_hooks', 0) + 1; return
        b = self.biz[bid]
        ok = b.rng.random() < 0.85
        if b.rng.random() < 0.9:
            self.sim.eng.at(add_bmin(self.sim.eng.t, b.rng.uniform(2, 50)), self.web, "3ds", bid, num, cid, ok)
    def on_charged(self, t, ch): self.charged.append((t, ch["id"]))
    def on_card_email(self, b, body): pass
    def biz_told_inactive(self, bid, num):
        self.biz_events.append(("told_inactive", {"t": self.sim.eng.t, "bid": bid, "num": num}))
    # ---------- details and calls
    def details_released(self, bid, num, info):
        b = self.biz[bid]; t = self.sim.eng.t; tr = self.truth.get(num)
        self.details_to.append({"t": t, "bid": bid, "num": num, "fake": b.fake, "info": info, "replay": self.sim.app.replaying})
        if self.sim.app.replaying: return
        b.wins += 1
        if tr is None: return
        tr.setdefault("details_to", []).append(bid)
        if b.persona == "noshow" and b.rng.random() < 0.8: return
        lo, hi = (20, 180) if b.persona == "responsive" else (120, 1440)
        self.sim.eng.at(biz_hours(b.rng, t, lo, hi), self.biz_call, bid, num, 1)
    def biz_call(self, bid, num, attempt):
        b = self.biz[bid]; tr = self.truth.get(num); app = self.sim.app
        j = app.db["jobs"].get(num)
        if tr is None or tr.get("reached") or tr.get("cancelled"): return
        frm = b.phone; pin = None
        linked = app.db["biz"].get(bid, {}).get("staff", [])
        if b.staff and b.rng.random() < self.cfg["p_unlinked_call"]:
            frm = b.rng.choice(b.staff)
            pin = j["pin"] if (j and b.rng.random() < self.cfg["p_has_pin"]) else None
        elif j: pin = j["pin"] if b.rng.random() < 0.5 else None
        ctx = {"pin": pin, "intended_job": num, "intended_phone": tr["phone"], "by": bid, "attempt": attempt}
        rec = self.sim.tw.call(self.sim.eng.t, frm, "+19285550199", ctx)
        if rec["routed_to"] == tr["phone"] and rec["answered"]:
            tr["reached"] = self.sim.eng.t; self.after_contact(b, tr, num)
        elif rec["routed_to"] is None and frm != b.phone:
            self.sim.eng.at(self.sim.eng.t + b.rng.uniform(5, 60), self.biz_call_main, bid, num, attempt)
        else:
            h = tr["ho"]
            if rec["routed_to"] == tr["phone"] and h.rng.random() < self.cfg["callback_prob"]:
                self.sim.eng.at(bt(h.rng, self.sim.eng.t, 20, 300, offset_h=h.tz_off), self.ho_callback, h, num, bid)
            if attempt < 4 and b.persona != "noshow":
                self.sim.eng.at(biz_hours(b.rng, self.sim.eng.t, 180, 600), self.biz_call, bid, num, attempt + 1)
    def biz_call_main(self, bid, num, attempt):
        b = self.biz[bid]; tr = self.truth.get(num)
        if tr is None or tr.get("reached"): return
        ctx = {"pin": None, "intended_job": num, "intended_phone": tr["phone"], "by": bid, "attempt": attempt, "retry_main": True}
        rec = self.sim.tw.call(self.sim.eng.t, b.phone, "+19285550199", ctx)
        if rec["routed_to"] == tr["phone"] and rec["answered"]: tr["reached"] = self.sim.eng.t; self.after_contact(b, tr, num)
        elif attempt < 4: self.sim.eng.at(biz_hours(b.rng, self.sim.eng.t, 180, 600), self.biz_call, bid, num, attempt + 1)
    def ho_callback(self, h, num, bid):
        tr = self.truth.get(num)
        if tr is None or tr.get("reached"): return
        ctx = {"intended_job": num, "intended_bid": bid, "intended_phone": self.biz[bid].phone, "by": h.phone}
        rec = self.sim.tw.call(self.sim.eng.t, h.phone, "+19285550199", ctx)
        if rec["routed_to"] == self.biz[bid].phone and rec["answered"]: tr["reached"] = self.sim.eng.t; self.after_contact(self.biz[bid], tr, num)
    def after_contact(self, b, tr, num):
        q = tr.get("quotes", {}).get(b.bid)
        if q is None: return
        prob = self.cfg["p_problem_if_shaded"] if (q.get("shaded") or q.get("gamed")) else self.cfg["p_problem_quality"]
        tr["problem"] = b.rng.random() < prob
        tr["done_by"] = self.sim.eng.t + b.rng.uniform(1, 5) * DAY
        app = self.sim.app; j = app.db["jobs"].get(num)
        if j and j["quotes"].get(b.bid):
            fee_paid = j["quotes"][b.bid]["fee"] if not j["free_win"] else 0.0
            from app_flow import fee_for
            if q.get("shaded") or q.get("gamed"):
                tq = {"kind": "fixed", "amount": q["true"]}
                true_fee = fee_for(self.cfg, tr["cat"], tq)
                self.fee_avoided.append({"t": self.sim.eng.t, "bid": b.bid, "num": num, "avoided": max(0, true_fee - j["quotes"][b.bid]["fee"]),
                                         "kind": "shade" if q.get("shaded") else "trip", "free": j["free_win"]})
            if fee_paid > 0 and fee_paid / max(1, q["true"]) > self.cfg["s50"] / 2: b.strikes += 1
        if CATS[tr["cat"]][2] and b.rng.random() < self.cfg["p_recurring_stop_30d"]:
            tr["recurring_stop"] = self.sim.eng.t + b.rng.uniform(10, 30) * DAY
            self.sim.eng.at(tr["recurring_stop"], self.recurring_stopped, num)
    def recurring_stopped(self, num):
        tr = self.truth[num]; tr["stopped"] = True
        self.sim.app.recurring_stop(num)
    def on_checkin(self, h, num, tpl):
        tr = self.truth.get(num)
        if tr is None or tr.get("checkin_replied"): return
        r = h.rng
        if h.persona == "absent" and r.random() < 0.5: return
        if r.random() < 0.15: return
        es = h.lang == "es"
        if tr.get("reached"):
            if tr.get("problem"): word = "PROBLEMA" if es else "PROBLEM"; h.satisfied = False
            elif self.sim.eng.t >= tr.get("done_by", 0): word = "LISTO" if es else "DONE"; h.satisfied = True
            else: return
        else:
            word = "NO"; h.satisfied = False
        tr["checkin_replied"] = word
        h.text(bt(r, self.sim.eng.t, 5, 300, offset_h=h.tz_off), word, kind="checkin", job=num)
    # ---------- business sign-up and life cycle
    def biz_join(self, bid):
        b = self.biz[bid]; t = self.sim.eng.t
        info = {"bid": bid, "name": b.name, "phone": b.phone, "email": b.email, "lic": b.lic, "lic_kind": b.lic_kind, "classes": b.classes,
                "cats": [c for c in b.cats if c in self.cfg["categories"]], "after_hours": b.after_hours, "skip_tenant": b.skip_tenant,
                "max_cards": b.max_cards, "insured": b.insured}
        b.join_t = t
        self.sim.web_log.append((t, "signup", (dict(info),)))
        res = self.sim.do_web("signup", (info,))
        self.biz_events.append(("signup", {"t": t, "bid": bid, "fake": b.fake, "res": res}))
    def biz_obj(self, bid): return self.biz[bid] if not getattr(self.biz[bid], "borrowed", None) else self.biz[bid].borrowed
    def city_list_has(self, name):
        b = next((x for x in self.biz.values() if x.name == name), None)
        if b is None or b.fake: return False
        if b.rng.random() < self.cfg["p_name_mismatch_city"]: return False
        return True
    def biz_activated(self, bid):
        b = self.biz[bid]; b.active_t = self.sim.eng.t
        self.biz_events.append(("activated", {"t": self.sim.eng.t, "bid": bid, "fake": b.fake}))
    def link_offered(self, bid, phone):
        if self.sim.app.replaying: self.replay_hooks = getattr(self, 'replay_hooks', 0) + 1; return
        b = self.biz[bid]
        if b.rng.random() < 0.7:
            num = next((j["num"] for j in self.sim.app.db["jobs"].values() if j["status"] == "Connected" and j["pick"]["bid"] == bid), 0)
            self.sim.eng.at(self.sim.eng.t + b.rng.uniform(5, 120), self.biz_says, b, f"LINK{num}")
    def month_tick(self, m):
        """Part 9 leaving rule (A): 3 strikes (fee > s50/2 of job) -> 50% leave; no wins in 2 months -> 15%;
        background churn 1.5%/month. Leavers stop quoting; half text STOP."""
        for b in list(self.biz.values()):
            if b.left is not None or b.active_t is None or b.fake: continue
            months = (self.sim.eng.t - b.active_t) / (30 * DAY)
            p = self.cfg["background_churn_month"]
            if b.strikes >= 3: p = max(p, 0.5); b.strikes = 0
            if months >= 2 and b.wins == 0: p = max(p, 0.15)
            if b.rng.random() < p:
                b.left = self.sim.eng.t; b.base_q = 0
                self.biz_events.append(("left", {"t": b.left, "bid": b.bid, "group": b.group, "wins": b.wins}))
                if b.rng.random() < 0.5: self.biz_says(b, "STOP")
            # DBA second free win (A rate), unlicensed groups only
            if b.lic is None and b.wins >= 1 and not getattr(b, "dba_done", False) and b.rng.random() < self.cfg["p_dba_second_account"]:
                b.dba_done = True
                self.biz_n += 1; nb = Biz(self, b.bid + "D", b.group, b.persona, idx=9000 + self.biz_n)
                nb.name = b.name.split()[0] + " Pro Services " + str(self.biz_n); nb.dba_of = b.bid; nb.joins = True
                self.biz[nb.bid] = nb; self.reg_phone(nb.phone, nb)
                if nb.email: self.emails[nb.email] = nb
                self.biz_join(nb.bid)
    def force_dispute(self):
        st = self.sim.stripe
        cands = [c for c in st.charges if c["status"] == "succeeded" and not c["disputed"] and c["refunded"] < c["amount"]]
        if cands:
            c = cands[-1 - int(self.rng.random() * min(5, len(cands)))]
            st.open_dispute(self.sim.eng.t, c["id"], "forced_wave")

def DESC_EN(tr):
    return tr["desc"].split(",")[0] if tr["cat"] else ""
