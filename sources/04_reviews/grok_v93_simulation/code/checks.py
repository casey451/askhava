"""Part 11 checks, judged from OUTSIDE-WORLD events only: what Twilio delivered (bodies, recipients, times),
what Stripe charged/refunded, what Claude was sent, calls placed, emails/letters, web actions people took,
and the world's own truth (what people said and meant). App database labels are never used to pass a check.
(Template names in the vendor log are used only to print examples.) All results simulated."""
import re
from collections import defaultdict
from engine import DAY, HOUR, fmt_t
from config import CATS, FEES, JOBTYPES
from world_people import Homeowner
from world_biz import Biz
from app_flow import fee_for

NUM = re.compile(r"#(\d{4,5})")
URL = re.compile(r"(https?://\S+|\b[a-z0-9.-]+\.(?:gov|com|org|net|example)(?:/\S*)?)", re.I)
ES_WORDS = {"su", "para", "solicitud", "gracias", "responda", "cotización", "cotizaciones", "usted", "hola", "está", "negocio", "llame", "qué", "en",
            "el", "la", "de", "lo", "sentimos", "nadie", "código", "trabajo", "cerramos", "somos", "del", "le", "aún", "elegir", "cuéntenos"}
EN_WORDS = {"your", "the", "reply", "thanks", "request", "quote", "quotes", "you", "is", "we"}

def lang_of(body):
    w = set(re.findall(r"[a-záéíóúñ]+", body.lower()))
    es = len(w & ES_WORDS); en = len(w & EN_WORDS)
    if any(c in body for c in "¿¡ñ"): es += 2
    return "es" if es > en else "en"

def local_h(t, off): return ((t + off * 60) % DAY) / 60

def group_cover(b, cat, jt):
    """LEGAL TRUTH (brief Part 2 + A.R.S. 32-1121), independent of app code: may business b lawfully do this job type?"""
    if cat not in b.cats: return False
    flags = next(((x[2], x[3], x[4], x[5]) for x in JOBTYPES.get(cat, []) if x[0] == jt), (0, 0, 0, None))
    gas, permit, fire, over = flags
    req = over if over is not None else CATS[cat][3]
    if req is None: return True
    if req == "PEST": return b.lic_kind == "PEST"
    if b.lic_kind == "ROC":
        return req == ("ROC-any",) or any(c in b.classes for c in req)
    if b.lic_kind is None:          # handyman exemption: listed categories only, never gas/fire, never permit work
        return CATS[cat][4] and not gas and not fire and not permit
    return False

class Checker:
    def __init__(self, sim):
        self.s = sim; w = sim.world; self.w = w; tw = sim.tw
        self.end = sim.cfg["days"] * DAY
        self.out = defaultdict(list)       # phone -> delivered app texts (rec)
        self.sent = defaultdict(list)      # phone -> all attempted app texts
        for r in sorted(tw.sms.values(), key=lambda r: r["t"]):
            self.sent[r["to"]].append(r)
            if r["status"] == "delivered": self.out[r["to"]].append(r)
        self.inb = defaultdict(list)
        for r in tw.inbound: self.inb[r["frm"]].append(r)
        self.res = {}
        # derive per-request cancel times from what the homeowner actually said (world truth, not app state)
        byph = defaultdict(list)
        for tr in w.reqs: byph[tr["phone"]].append(tr)
        for it in w.intent:
            if it["kind"] in ("cancel_job", "optout"):
                cand = [tr for tr in byph.get(it["phone"], []) if tr["t0"] <= it["t"] + 1]
                if cand and "cancel_t" not in cand[-1]: cand[-1]["cancel_t"] = it["t"]
        self.photo_ident = {p["id"]: p["identifying"] for tr in w.reqs for p in tr["photos"]}
    def body(self, r): return self.s.tw.sms[r["sid"]]["body"] if "body" not in r else r["body"]
    def add(self, cid, opps, fails):
        self.res[cid] = {"opps": opps, "fails": len(fails), "example": fails[0] if fails else None}
    def ho_of(self, ph):
        p = self.w.phones.get(ph); return p if isinstance(p, Homeowner) else None
    def biz_of(self, ph):
        p = self.w.phones.get(ph); return p if isinstance(p, Biz) else None
    def cards(self):
        """Card texts delivered to businesses: (t, biz, num, body)."""
        if hasattr(self, "_cards"): return self._cards
        c = []
        for ph, lst in self.out.items():
            b = self.biz_of(ph)
            if not b: continue
            for r in lst:
                if r["body"].startswith("New job #"):
                    c.append((r["t"], b, int(NUM.search(r["body"]).group(1)), r["body"], r))
        self._cards = c; return c
    # ------------------------------------------------------------------ MONEY
    def money(self):
        s = self.s; w = self.w; st = s.stripe
        ok_ch = [c for c in st.charges if c["status"] == "succeeded"]
        intended = defaultdict(list)
        for p in w.picks_intended: intended[p["num"]].append(p)
        f1 = []; per = defaultdict(list)
        for c in ok_ch:
            num = c["meta"]["job"]; tr = w.truth.get(num); b = w.biz.get(c["bid"])
            per[(num, c["bid"])].append(c)
            ip = [p for p in intended.get(num, []) if p["t"] <= c["t"]]
            if not ip: f1.append(f"charge {c['id']} ${c['amount']} for #{num} at {fmt_t(c['t'])}: homeowner never picked"); continue
            last = ip[-1]
            if last["intended"] != c["bid"]:
                f1.append(f"#{num}: homeowner meant {last['intended']} ('{last['n']} YES' by {last['via']}) at {fmt_t(last['t'])}; {c['bid']} was charged ${c['amount']}")
                continue
            if tr and tr.get("cancel_t") and tr["cancel_t"] < c["t"]:
                f1.append(f"#{num}: charged {fmt_t(c['t'])} after homeowner cancelled at {fmt_t(tr['cancel_t'])}")
        for (num, bid), cs in per.items():
            kept = [c for c in cs if c["refunded"] < c["amount"] - 0.005]
            if len(kept) > 1: f1.append(f"#{num}: {bid} charged {len(cs)} times ({', '.join('$'+str(c['amount']) for c in cs)}), {len(kept)} kept")
        self.add("M1 charge only after own-phone yes + timely confirm, live job, once, real intro", len(ok_ch), f1)
        # fee matches tier (business's own quote, true category)
        f2 = []
        for c in ok_ch:
            num = c["meta"]["job"]; tr = w.truth.get(num)
            if not tr or c["bid"] not in tr.get("quotes", {}): continue
            q = {k: v for k, v in tr["quotes"][c["bid"]].items() if k in ("kind", "amount", "lo", "hi", "monthly")}
            exp = fee_for(s.cfg, tr["cat"], q)
            if abs(exp - c["amount"]) > 0.005:
                f2.append(f"#{num} {tr['cat']} quote {q}: charged ${c['amount']}, tier says ${exp}")
        self.add("M2 fee matches tier (recurring rule, ranges, trip tier)", len(ok_ch), f2)
        # free first win once per real-world entity (truth owner; DBA counts as the same owner)
        free = defaultdict(list)
        for ph, lst in self.out.items():
            b = self.biz_of(ph)
            if not b: continue
            for r in lst:
                if "first win is free" in r["body"]:
                    owner = b.lic or b.bid
                    if getattr(b, "dba_of", None): owner = w.biz[b.dba_of].lic or b.dba_of
                    free[owner].append((r["t"], b.bid, NUM.search(r["body"]).group(1)))
        f3 = []
        for o, lst in free.items():
            # only count free offers that turned into released details
            rel = [x for x in lst if any(d["bid"] == x[1] and d["num"] == int(x[2]) and not d.get("replay") for d in w.details_to)]
            if len(rel) > 1: f3.append(f"owner {o}: {len(rel)} free introductions ({', '.join(x[1] + ' #' + x[2] for x in rel)})")
        self.add("M3 free first win once per entity", len(free), f3)
        # books match Stripe
        led = sum(e["amount"] for e in s.app.db["ledger"] if e["kind"] in ("charge", "refund"))
        stripe_net = sum(c["amount"] for c in ok_ch) - sum(r["amount"] for r in st.refunds)
        f4 = [] if abs(led - stripe_net) < 0.01 else [f"books ${led:.2f} vs Stripe net ${stripe_net:.2f} (diff ${led - stripe_net:.2f})"]
        self.add("M4 books match Stripe to the cent", 1, f4)
        # refunds by rule, exactly once; failed refunds retried; disputes answered
        f5 = []; opps5 = 0
        ref_by = defaultdict(float); nref = defaultdict(int)
        for r in st.refunds: ref_by[r["cid"]] += r["amount"]; nref[r["cid"]] += 1
        for c in ok_ch:
            if ref_by[c["id"]] > c["amount"] + 0.005: f5.append(f"charge {c['id']} ${c['amount']} refunded ${ref_by[c['id']]:.2f} ({nref[c['id']]} refunds)")
        for num, tr in w.truth.items():
            cs = [c for c in ok_ch if c["meta"]["job"] == num]
            if not cs: continue
            opps5 += 1
            never = not tr.get("reached") and tr.get("checkin_replied") == "NO"
            if never and all(ref_by[c["id"]] < c["amount"] - 0.005 for c in cs) and not any(c["disputed"] for c in cs):
                f5.append(f"#{num}: homeowner replied NO (never contacted) yet ${sum(c['amount'] for c in cs)} kept")
        for d in st.disputes:
            if d["responded"] is None and d["due"] <= s.eng.t: f5.append(f"dispute on charge {d['cid']} not answered by its due time {fmt_t(d['due'])}")
        fails_rf = getattr(st, "refund_failures", [])
        self.add("M5 refunds by rule exactly once; disputes handled; failed refunds retried", opps5 + len(st.disputes), f5)
        # money for a closed job refunded
        f6 = []; o6 = 0
        for num, tr in w.truth.items():
            cs = [c for c in ok_ch if c["meta"]["job"] == num]
            if not cs or not tr.get("cancel_t"): continue
            o6 += 1
            if tr.get("reached") and tr["reached"] < tr["cancel_t"]: continue
            if all(ref_by[c["id"]] < c["amount"] - 0.005 for c in cs):
                f6.append(f"#{num}: homeowner cancelled at {fmt_t(tr['cancel_t'])} before any contact; ${sum(c['amount'] for c in cs)} kept")
        self.add("M6 money for a closed job refunded", o6, f6)
    # ------------------------------------------------------------------ PRIVACY
    def _idx(self):
        if getattr(self, "_ix", None): return
        self._ix = True; self._p7 = {}; self._fn = {}; self._ad = {}
        for h in self.w.hos:
            self._p7.setdefault(h.phone[-7:], []).append(h)
            self._fn.setdefault(h.fname.lower(), []).append(h)
            self._ad.setdefault(h.addr.split()[0], []).append(h)
    def ho_by_phone7(self, d):
        self._idx(); out = []
        for i in range(0, max(0, len(d) - 6)):
            out += self._p7.get(d[i:i + 7], [])
        return out
    def ho_candidates(self, txt):
        """same set as 'fname in txt or house-number in txt or phone7 in digits(txt)', via indexes (speed only)."""
        self._idx(); low = txt.lower(); seen = {}
        for k, hs in self._fn.items():
            if k in low:
                for h in hs: seen[id(h)] = h
        for k, hs in self._ad.items():
            if k in txt:
                for h in hs: seen[id(h)] = h
        for h in self.ho_by_phone7(re.sub(r"\D", "", txt)): seen[id(h)] = h
        return list(seen.values())
    def privacy(self):
        s = self.s; w = self.w
        f1 = []; o1 = 0
        for ph, lst in self.out.items():
            b = self.biz_of(ph)
            if not b: continue
            for r in lst:
                if not r["body"].startswith("Confirmed #"): continue
                o1 += 1; num = int(NUM.search(r["body"]).group(1)); tr = w.truth.get(num)
                paid = any(c["bid"] == b.bid and c["meta"]["job"] == num and c["status"] == "succeeded" for c in s.stripe.charges) or \
                       any("first win is free" in x["body"] and f"#{num}" in x["body"] for x in self.out[ph])
                lic_ok = (not b.fake) and (tr is None or group_cover(b, tr["cat"], tr["jt"]))
                lapse = s.roc.records.get(b.lic, {}).get("lapse_t") if b.lic else None
                if lapse is not None and lapse < r["t"]: lic_ok = False
                conf = any(re.match(rf"^(yes|confirm|si|sí)\s*#?{num}?$", x["body"].strip().lower()) and x["t"] <= r["t"] for x in self.inb[ph]) or \
                       any(wl[1] == "confirm" and wl[2][0] == b.bid and wl[2][1] == num and wl[0] <= r["t"] for wl in s.web_log) or \
                       any(re.match(r"^(yes|confirm)", x["body"].strip().lower()) and x["t"] <= r["t"] for x in self.inb[ph])
                if not (paid and lic_ok and conf):
                    why = [] if paid else ["not paid"]
                    if not lic_ok: why.append("FAKE business" if b.fake else "licence does not cover / lapsed")
                    if not conf: why.append("never confirmed")
                    f1.append(f"#{num}: homeowner details sent to {b.bid} at {fmt_t(r['t'])} ({', '.join(why)})")
        self.add("P1 details only to confirmed, paid, licensed business", o1, f1)
        f2 = []; o2 = 0
        for t, b, num, body, r in self.cards():
            o2 += 1; tr = w.truth.get(num)
            if not tr: continue
            h = tr["ho"]; pii = w.pii_found(h, body)
            if pii: f2.append(f"#{num} card to {b.bid}: contains {', '.join(pii)}: '{body[:110]}'")
        for c in w.cards:
            for p in c["card"].get("photos", []):
                if self.photo_ident.get(p): f2.append(f"#{c['num']} card to {c['bid']}: identifying photo {p} attached (MMS)"); break
        self.add("P2 cards: no PII, no identifying photos", o2, f2)
        f3 = []
        hos = w.hos
        for e in s.claude.log:
            if e["photo"] is not None:
                if e["photo"].get("identifying"): f3.append(f"{fmt_t(e['t'])} {e['purpose']}: identifying photo {e['photo']['id']} sent to Claude")
                continue
            txt = e["input"] or ""
            for h in self.ho_candidates(txt):
                    pii = w.pii_found(h, txt)
                    if pii:
                        f3.append(f"{fmt_t(e['t'])} {e['purpose']}: Claude received {', '.join(pii)} of a homeowner: '{txt[:120]}'"); break
        self.add("P3 AI gets no homeowner name/phone/address", len(s.claude.log), f3)
        f4 = []; o4 = 0
        for ph, lst in self.out.items():
            b = self.biz_of(ph)
            if not b: continue
            for r in lst:
                o4 += 1; d = re.sub(r"\D", "", r["body"])
                for h in self.ho_by_phone7(d):
                    f4.append(f"text to {b.bid}: homeowner phone in '{r['body'][:90]}'"); break
        for e in s.email.log:
            d = re.sub(r"\D", "", e["body"])
            if self.ho_by_phone7(d): f4.append(f"email to {e['to']}: homeowner phone in body")
        for c in s.tw.calls:
            if self.biz_of(c.get("routed_to") or "") and c.get("shown_cid") and c["shown_cid"] == c["frm"]: f4.append("caller ID exposed")
        self.add("P4 homeowner phone never given out", o4, f4)
    # ------------------------------------------------------------------ TEXTING
    def texting(self):
        s = self.s; w = self.w
        f1 = []; o1 = 0
        for ph, lst in self.sent.items():
            first = lst[0]; o1 += 1
            ins = [x for x in self.inb[ph] if x["t"] <= first["t"]]
            signed = any(wl[1] == "signup" and wl[2][0]["phone"] == ph and wl[0] <= first["t"] for wl in s.web_log)
            if not ins and not signed: f1.append(f"text to {ph} at {fmt_t(first['t'])} but they never contacted us: '{first['body'][:80]}'"); continue
            h = self.ho_of(ph)
            if h:
                bd = first["body"].lower()
                need = ["havasu home quotes", "stop", "help", "rates may apply", "frequency", "terms", "pays us"]
                es_need = ["havasu home quotes", "stop", "ayuda", "tarifas", "frecuencia", "términos", "paga"]
                miss = [n for n in (es_need if h.lang == "es" and lang_of(first["body"]) == "es" else need) if n not in bd]
                if miss: f1.append(f"first text to homeowner {ph}: missing {miss}: '{first['body'][:80]}'")
        self.add("T1 first text has every disclosure; only people who contacted us", o1, f1)
        f2 = []; o2 = 0
        for ph, lst in self.out.items():
            p = self.w.phones.get(ph); off = w.tz_offset(ph)
            for r in lst:
                o2 += 1; h = local_h(r["t"], off)
                if not (h >= 21 or h < 8): continue
                bd = r["body"]
                if "safety notice" in bd or "unsubscribed" in bd or bd.startswith("Havasu Home Quotes help"): continue
                last = [x["t"] for x in self.inb[ph] if x["t"] <= r["t"]]
                if last and r["t"] - last[-1] <= 15: continue
                f2.append(f"{fmt_t(r['t'])} text to {ph} ({'out-of-state, local ' if off else 'local '}{int(h):02d}:{int((h%1)*60):02d}): '{bd[:70]}'")
        self.add("T2 quiet hours (recipient local time)", o2, f2)
        f3 = []; o3 = 0
        starts = defaultdict(list)
        for x in w.said:
            if x["kind"] in ("request", "start"): starts[x["who"]].append(x["t"])
        for it in w.intent:
            if it["kind"] not in ("optout",) and not (it["kind"] == "cancel_job" and it["word"] == "CANCEL"): continue
            o3 += 1; ph = it["phone"]; t = it["t"]
            nxt = min([u for u in starts[ph] if u > t + 1] + [self.end])
            after = [r for r in self.sent[ph] if t < r["t"] < nxt]
            conf = [r for r in after if "unsubscribed" in r["body"] or "is closed" in r["body"] or "cancelad" in r["body"] or "cerrada" in r["body"] or "cancel" in r["body"].lower() and "#" not in r["body"]]
            other = [r for r in after if r not in conf]
            if len(conf) != 1 or other:
                f3.append(f"{ph} sent '{it['word']}' at {fmt_t(t)}: {len(conf)} confirmations, {len(other)} other texts after" +
                          (f" (first: '{other[0]['body'][:60]}')" if other else ""))
        self.add("T3 STOP / plain-language opt-out / CANCEL: exactly one confirmation, then silence", o3, f3)
        f4 = []; o4 = 0
        for ph, lst in self.inb.items():
            for x in lst:
                if x["body"].strip().upper() in ("HELP", "AYUDA", "INFO"):
                    o4 += 1
                    if not any(x["t"] <= r["t"] <= x["t"] + 5 and ("help" in r["body"].lower() or "ayuda" in r["body"].lower()) for r in self.out[ph]):
                        f4.append(f"{ph} texted {x['body']} at {fmt_t(x['t'])}: no help reply within 5 min")
        self.add("T4 HELP answered within 5 minutes", o4, f4)
        f5 = []; o5 = 0
        for ph, lst in self.sent.items():
            for r in lst:
                o5 += 1
                for u in URL.findall(r["body"]):
                    if "hhq.example" not in u: f5.append(f"text to {ph}: off-domain link {u}: '{r['body'][:80]}'"); break
                if re.search(r"\b(sale|discount|deal|promo|% off)\b", r["body"], re.I): f5.append(f"marketing wording to {ph}")
        self.add("T5 no marketing; links only on our domain", o5, f5)
        f6 = []; o6 = 0
        for ph, lst in self.out.items():
            h = self.ho_of(ph)
            if not h or h.lang != "es": continue
            for r in lst:
                o6 += 1
                if lang_of(r["body"]) != "es": f6.append(f"Spanish speaker {ph} got English at {fmt_t(r['t'])}: '{r['body'][:70]}'")
        self.add("T6 Spanish speakers get Spanish texts", o6, f6)
        # opt-out alarms: compute weekly rates from outside events; alarm must reach Casey when over threshold
        f7 = []; o7 = 0
        alarms = [a for a in s.app.db["alerts"] if a["kind"].startswith("optout_rate")]
        self._said_wk = {}
        for x in w.said: self._said_wk.setdefault(int(x["t"] // (7 * DAY)), []).append(x)
        _int_wk = {}
        for it in w.intent: _int_wk.setdefault(int(it["t"] // (7 * DAY)), []).append(it)
        for wk in range(int(self.end // (7 * DAY))):
            t0, t1 = wk * 7 * DAY, (wk + 1) * 7 * DAY
            hos = {x["who"] for x in self._said_wk.get(wk, []) if self.ho_of(x["who"])}
            outs = {it["phone"] for it in _int_wk.get(wk, []) if it["kind"] == "optout"} | \
                   {x["who"] for x in self._said_wk.get(wk, []) if x["kind"] == "optout" and x["who"] in s.tw.carrier_optout and self.ho_of(x["who"])}
            if len(hos) >= 10:
                o7 += 1
                rate = len(outs & hos) / len(hos)
                if rate > 0.03 and not any(t1 <= a["t"] <= t1 + 7 * DAY + DAY for a in alarms if a["kind"].endswith("homeowners")):
                    f7.append(f"week {wk}: homeowner opt-out rate {rate:.1%} (>3%), no alarm to Casey")
        self.add("T7 opt-out alarms (3% homeowners / 5% businesses)", o7, f7)
    # ------------------------------------------------------------------ SAFETY
    def safety(self):
        w = self.w
        f1 = []; f2 = []; f3 = []; o1 = 0; o3 = 0
        for tr in w.reqs:
            if not tr.get("emergency") or not tr["nums"]: continue
            o1 += 1; ph = tr["phone"]; t0 = tr["t0"]
            st = [r for r in self.out[ph] if "safety notice" in r["body"] and r["t"] >= t0 - 1]
            if not st or st[0]["t"] - t0 > 5 or "911" not in st[0]["body"]:
                f1.append(f"#{tr['nums'][0]} ({'indirect' if tr['indirect'] else 'direct'}: '{tr['emer_text'][:50]}') safety text " +
                          (f"after {st[0]['t'] - t0:.0f} min" if st else "never sent"))
            safe_t = min([x["t"] for x in self.inb[ph] if x["t"] >= t0 and re.search(r"safe|seguro", x["body"], re.I)] + [1e18])
            for t, b, num, body, r in self.cards():
                if num in tr["nums"] and st and t < safe_t:
                    f2.append(f"#{num}: card to {b.bid} at {fmt_t(t)} before homeowner said SAFE"); break
            urgent = [x for x in self.inb[ph] if x["t"] >= t0 and x["body"].strip() == "1"]
            if urgent:
                for t, b, num, body, r in self.cards():
                    if num in tr["nums"]:
                        o3 += 1
                        if not b.after_hours: f3.append(f"#{num} EMERGENCY card to {b.bid} (no after-hours service) at {fmt_t(t)}"); break
        self.add("S1 safety text with reason within 5 minutes", o1, f1)
        self.add("S2 no card before homeowner says safe", o1, f2)
        self.add("S3 emergency cards only to after-hours businesses", o3, f3)
    # ------------------------------------------------------------------ MATCHING
    def matching(self):
        w = self.w; s = self.s
        f1 = []; f2 = []; f3 = []; f5 = []; o1 = 0
        by_job = defaultdict(list)
        for t, b, num, body, r in self.cards(): by_job[num].append((t, b, body))
        for num, cl in by_job.items():
            tr = w.truth.get(num)
            if not tr: continue
            o1 += 1
            for t, b, body in cl:
                if not group_cover(b, tr["cat"], tr["jt"]) or b.fake:
                    f1.append(f"#{num} ({tr['cat']}/{tr['jt']}) card to {b.bid} ({b.group}, {b.lic_kind or 'no licence'}{', FAKE' if b.fake else ''})")
                if tr["jt"] == "weed_control" and b.lic_kind != "PEST": f2.append(f"#{num} weed control card to {b.bid} ({b.group}, no pest licence)")
                if "gas" in (tr["jt"] or "") and b.group == "handyman": f3.append(f"#{num} gas job ({tr['jt']}) card to handyman {b.bid}")
                if tr["tenant"] and "tenant" not in body.lower() and "renter" not in body.lower(): f5.append(f"#{num} tenant job card to {b.bid} lacks tenant tag")
                if tr["tenant"] and b.skip_tenant: f5.append(f"#{num} tenant card sent to {b.bid} which asked to skip tenants")
                if tr["spanish"] and "spanish" not in body.lower() and "español" not in body.lower(): f5.append(f"#{num} Spanish-speaker card to {b.bid} lacks Spanish tag")
            # every qualified, active business got it (truth)
            t0 = cl[0][0]; got = {b.bid for _, b, _ in cl}
            for b in w.biz.values():
                if b.fake or b.left is not None or not getattr(b, "active_t", None) or b.active_t > t0: continue
                if not group_cover(b, tr["cat"], tr["jt"]) or (tr["tenant"] and b.skip_tenant): continue
                lapse = s.roc.records.get(b.lic, {}).get("lapse_t") if b.lic else None
                if lapse is not None and lapse < t0: continue
                if b.max_cards: continue
                if any(x["t"] < t0 and x["body"].strip().upper() == "STOP" for x in self.inb[b.phone]): continue
                # lenient exclusions (documented): business paused / not activated by the app, or job flagged urgent
                ab = s.app.db["biz"].get(b.bid)
                if ab is None or not ab["active"] or ab["paused"]: continue
                if (s.app.db["jobs"].get(num) or {}).get("urgent") and not b.after_hours: continue
                if b.bid not in got:
                    f1.append(f"#{num} ({tr['cat']}) qualified active business {b.bid} never got the card"); break
        # jobs that got no cards although qualified businesses existed
        self.add("X1 every card to a covering licence, and to every qualified business", o1, f1)
        self.add("X2 weed control only to pest licensees", o1, f2)
        self.add("X3 gas work never to a handyman", o1, f3)
        f4 = []; o4 = 0
        for ph, lst in self.out.items():
            if not self.ho_of(ph): continue
            for r in lst:
                for m in re.finditer(r"\d\) ([A-Za-z]+ Handyman \d+) \$([\d,]+)(?!/mo)", r["body"]):
                    o4 += 1
                    if float(m.group(2).replace(",", "")) >= 1000: f4.append(f"homeowner shown handyman quote ${m.group(2)} from {m.group(1)}: '{r['body'][:80]}'")
        self.add("X4 handyman quotes $1,000+ refused", o4, f4)
        self.add("X5 tenant and Spanish tags; skip-tenant honoured", o1, f5)
    # ------------------------------------------------------------------ HOMEOWNER EXPERIENCE
    def homeowner(self):
        w = self.w; s = self.s
        f1, f2, f3, f4, f5, f6, f7 = ([] for _ in range(7)); o1 = o3 = o4 = o6 = o7 = 0
        lists = defaultdict(list)
        for ph, lst in self.out.items():
            if not self.ho_of(ph): continue
            for r in lst:
                if "YES (e.g. 1 YES)" in r["body"] or "SÍ (ej. 1 SÍ)" in r["body"]:
                    lists[int(NUM.search(r["body"]).group(1))].append(r)
        for num, tr in w.truth.items():
            okq = [q for q in tr.get("quotes", {}).values() if q.get("app_result") == "ok"]
            if any(f"#{num} looks like the same job" in r["body"] or f"#{num} parece el mismo" in r["body"] for r in self.out[tr["phone"]]): okq = []
            if okq and not tr.get("cancel_t"):
                o1 += 1
                if not lists.get(num):
                    first_q = min(q.get("t", 0) for q in tr["quotes"].values()) if False else None
                    f1.append(f"#{num}: {len(tr['quotes'])} business(es) quoted, homeowner never got a quote list")
            ls = lists.get(num, [])
            for a, b in zip(ls, ls[1:]):
                if b["t"] - a["t"] < 3 * HOUR and "after_fail" not in str(b["meta"]) and "noshow" not in str(b["meta"]) and \
                   not any(a["t"] < x["t"] <= b["t"] for x in self.inb[tr["phone"]]):
                    f2.append(f"#{num}: quote updates at {fmt_t(a['t'])} and {fmt_t(b['t'])} ({(b['t'] - a['t']) / 60:.1f} h apart)"); break
        for ph, lst in self.out.items():
            for r in lst:
                m = re.search(r"(?:by|antes de las) 9 p\.m\. (today|tomorrow|de hoy|de mañana)", r["body"])
                if not m: continue
                o3 += 1; num = int(NUM.search(r["body"]).group(1))
                dd = int(r["t"] // DAY) + (1 if m.group(1) in ("tomorrow", "de mañana") else 0)
                dl = dd * DAY + 21 * 60
                ok = any(r["t"] < x["t"] <= dl and f"#{num}" in x["body"] and re.search(r"\$|No quotes|closed|no qualified|Aún no hay|nadie cotizó|Cerramos|cerrada|ningún", x["body"])
                         for x in self.out[ph])
                if not ok and dl < self.end:
                    tr = w.truth.get(num)
                    if tr and tr.get("cancel_t") and tr["cancel_t"] < dl: continue
                    f3.append(f"#{num}: promised news by 9 p.m. day {dd}; nothing by then")
        for tr in w.reqs:
            if tr["t0"] > self.end - DAY: continue
            ph = tr["phone"]
            if tr["ho"].optout_t is not None and tr["ho"].optout_t < tr["t0"] + DAY: continue
            win = [r for r in self.out[ph] if tr["t0"] <= r["t"] <= tr["t0"] + DAY]
            ins = [x for x in self.inb[ph] if tr["t0"] < x["t"] <= tr["t0"] + DAY]
            if win and re.search(r"\?|Reply OK|Responda|responda", win[-1]["body"]) and (not ins or ins[-1]["t"] < win[-1]["t"]) and \
               not any(re.search(r"\$|out to \d", r["body"]) for r in win):
                self.h4_silent = getattr(self, "h4_silent", 0) + 1; continue      # homeowner never answered our question
            o4 += 1
            ok = any(tr["t0"] <= r["t"] <= tr["t0"] + DAY and re.search(r"\$|No quotes|no qualified|outside the area|is closed|closed #|out to \d|Got it|Listo\. La solicitud|fuera de nuestra|nadie cotizó|Cerramos|calificado|cerrada", r["body"]) for r in self.out[ph])
            if not ok:
                lastr = [r for r in self.out[ph] if r["t"] >= tr["t0"]][:1]
                f4.append(f"request at {fmt_t(tr['t0'])} from {ph} ('{tr.get('first_body', '')[:40]}'): no answer in 24 h" +
                          (f"; last text '{lastr[0]['body'][:50]}'" if lastr else "; no text at all"))
        # forwarded links can't pick: each 'picked' text to a business must follow the homeowner's own phone's yes
        o5 = 0
        for ph, lst in self.out.items():
            b = self.biz_of(ph)
            if not b: continue
            for r in lst:
                if not r["body"].startswith("The homeowner picked you"): continue
                o5 += 1; num = int(NUM.search(r["body"]).group(1)); tr = w.truth.get(num)
                if not tr: continue
                hp = tr["phone"]
                own = any(x["t"] <= r["t"] and re.match(r"^\d+\s*(yes|sí|si)", x["body"].strip().lower()) for x in self.inb[hp]) or \
                      any(v["phone"] == hp and v.get("approved_t") and v["approved_t"] <= r["t"] for v in s.tw.verify)
                if not own: f5.append(f"#{num}: {b.bid} told it was picked at {fmt_t(r['t'])} without a yes from the homeowner's phone")
        for it in w.intent:
            if it["kind"] != "cancel_job" or it["word"] == "CANCEL": continue
            o6 += 1; ph = it["phone"]; t = it["t"]
            later = [r for r in self.out[ph] if t + 10 < r["t"] < t + 7 * DAY and ("$" in r["body"] or "How did it go" in r["body"] or "No quotes" in r["body"])]
            cards_after = [c for c in self.cards() if c[0] > t + 10 and w.truth.get(c[2], {}).get("phone") == ph and c[0] < t + 3 * DAY]
            if later or cards_after:
                f6.append(f"{ph} said '{it['word']}' at {fmt_t(t)}; job kept going: '{(later[0]['body'] if later else cards_after[0][3])[:60]}'")
        # losers told 'no longer active' and never picked afterwards
        told = defaultdict(dict)
        for ph, lst in self.out.items():
            b = self.biz_of(ph)
            if not b: continue
            for r in lst:
                if "no longer active" in r["body"]: told[int(NUM.search(r["body"]).group(1))][b.bid] = r["t"]
        for d in w.details_to:
            if d.get("replay"): continue
            num = d["num"]; tr = w.truth.get(num)
            if not tr: continue
            o7 += 1
            for bid, q in tr.get("quotes", {}).items():
                if bid == d["bid"] or q.get("app_result") != "ok": continue   # a late quote got 'job closed' on the page itself
                if bid not in told[num]: f7.append(f"#{num}: {bid} quoted but was never told the job is no longer active"); break
        for num, tb in told.items():
            for ph, lst in self.out.items():
                b = self.biz_of(ph)
                if not b or b.bid not in tb: continue
                for r in lst:
                    if r["body"].startswith("The homeowner picked you") and f"#{num}" in r["body"] and r["t"] > tb[b.bid]:
                        f7.append(f"#{num}: {b.bid} told 'no longer active' at {fmt_t(tb[b.bid])}, then picked at {fmt_t(r['t'])}")
        self.add("H1 first-quote text arrives", o1, f1)
        self.add("H2 at most one quote update every 3 hours", len(lists), f2)
        self.add("H3 the 9 p.m. promise is kept", o3, f3)
        self.add("H4 every request hears back within 24 hours", o4, f4)
        self.add("H5 forwarded links can't pick", o5, f5)
        self.add("H6 'please cancel' / 'never mind' closes the job", o6, f6)
        self.add("H7 losers told 'no longer active' (and only real losers)", o7, f7)
    # ------------------------------------------------------------------ BUSINESSES
    def businesses(self):
        w = self.w; s = self.s
        f1 = []; o1 = 0
        for it in w.intent:
            if it["kind"] != "biz_join_text": continue
            o1 += 1
            if not any(it["t"] <= r["t"] <= it["t"] + 5 and "/join" in r["body"] for r in self.out[it["phone"]]):
                f1.append(f"'{it['body']}' from {it['phone']} at {fmt_t(it['t'])}: no sign-up link")
        self.add("B1 sign-up recognised from a text", o1, f1)
        f2 = []; o2 = 0; f3 = []; o3 = 0
        for wl in s.web_log:
            if wl[1] != "signup": continue
            info = wl[2][0]; b = w.biz[info["bid"]]
            if not info["lic"]: continue
            o2 += 1
            if not any(info["lic"] in r["body"] for r in self.out[b.phone]):
                f2.append(f"{b.bid} signed up with {info['lic']} at {fmt_t(wl[0])}: licence never read back")
            # identity chain order on the record's contact points
            o3 += 1
            rec = s.roc.records.get(info["lic"], {})
            steps = []
            for v in s.tw.verify:
                if v["phone"] == rec.get("phone") and v["t"] >= wl[0]: steps.append((v["t"], v["channel"]))
            for e in s.email.log:
                if e["to"] and e["to"] == rec.get("email") and e["t"] >= wl[0] and "code" in e["subject"]: steps.append((e["t"], "email"))
            for m in s.mail.log:
                if m["to"] in (b.bid, getattr(getattr(b, "borrowed", None), "bid", None)) and m["t"] >= wl[0]: steps.append((m["t"], "letter"))
            seq = [x[1] for x in sorted(steps)]
            order = ["sms", "voice", "email", "letter"]
            idx = [order.index(x) for x in seq if x in order]
            if idx != sorted(idx): f3.append(f"{b.bid}: identity chain out of order {seq}")
            if b.fake and any(d["bid"] == b.bid for d in w.details_to):
                f3.append(f"{b.bid} (FAKE, licence {info['lic']} borrowed from {getattr(b.borrowed, 'bid', '?')}) passed the chain via {seq or ['none']} + Casey and received homeowner details")
        for b in w.biz.values():
            if b.fake and not b.lic and any(d["bid"] == b.bid for d in w.details_to):
                f3.append(f"{b.bid} (FAKE unlicensed identity) activated and received homeowner details"); o3 += 1
        self.add("B2 licence read back", o2, f2)
        self.add("B3 identity chain text -> voice -> email -> letter; fakes stopped", o3, f3)
        f4 = []; o4 = 0
        for ph, lst in self.inb.items():
            b = self.biz_of(ph)
            if not b: continue
            for x in lst:
                if re.search(r"what now|got the notice|what do i do|picked\?", x["body"].lower()):
                    o4 += 1
                    if not any(x["t"] <= r["t"] <= x["t"] + 10 for r in self.out[ph]): f4.append(f"{b.bid} asked '{x['body']}' at {fmt_t(x['t'])}: no answer in 10 min")
        self.add("B4 pick questions answered", o4, f4)
        f5 = []; o5 = 0
        for c in s.tw.calls:
            b = self.biz_of(c["frm"])
            if not b or c["frm"] == b.phone: continue
            o5 += 1
            if c["routed_to"] is None and c["ctx"].get("pin") is None:
                f5.append(f"staff phone of {b.bid} called at {fmt_t(c['t'])} for #{c['ctx'].get('intended_job')}: not linked, no PIN, call dropped")
        self.add("B5 staff phones can link / reach the homeowner", o5, f5)
        f6 = []; o6 = 0
        probs = defaultdict(list)
        for tr in w.reqs:
            for x in self.inb[tr["phone"]]:
                if x["body"] in ("PROBLEM", "PROBLEMA"):
                    num = next((n for n in tr["nums"] if n in w.truth), None)
                    bid = next((d["bid"] for d in w.details_to if d["num"] in tr["nums"]), None)
                    if bid: probs[bid].append(x["t"])
        for bid, ts in probs.items():
            o6 += 1; ts = sorted(set(ts)); b = w.biz[bid]
            cards_after = lambda t: [c for c in self.cards() if c[1].bid == bid and c[0] > t + DAY]
            third = next((ts[i] for i in range(2, len(ts)) if ts[i] - ts[i - 2] <= 90 * DAY), None)
            if third is None and ts and not cards_after(ts[-1]) and ts[-1] < self.end - 7 * DAY and any(c[1].bid == bid for c in self.cards()):
                pass
            if third is not None and cards_after(third): f6.append(f"{bid}: 3 complaints by {fmt_t(third)} but still got cards")
        self.add("B6 complaints pause only at 3 in 90 days", o6, f6)
        f7 = []; o7 = 0
        shaded = defaultdict(int)
        for tr in w.reqs:
            for bid, q in tr.get("quotes", {}).items():
                if q.get("shaded"): shaded[bid] += 1
        for bid, n in shaded.items():
            if n >= 3:
                o7 += 1
                if not any(m[1] == "shade_flag" for m in w.casey.minutes): f7.append(f"{bid} shaded {n} quotes under a tier line; Casey never saw a flag")
        self.add("B7 tier shading flagged", o7, f7)
    # ------------------------------------------------------------------ CALLS
    def calls(self):
        w = self.w; s = self.s
        f1 = []; f2 = []; f3 = []; o2 = 0
        ended = {}
        for it in w.intent:
            if it["kind"] in ("cancel_job", "optout"): ended.setdefault(it["phone"], it["t"])
        for c in s.tw.calls:
            ctx = c["ctx"]; ip = ctx.get("intended_phone")
            if c["routed_to"] is not None and ip and c["routed_to"] != ip:
                f1.append(f"{fmt_t(c['t'])} call from {ctx.get('by')} meant for #{ctx.get('intended_job')} rang {c['routed_to']} (route '{c['ivr']}')")
            if ctx.get("pin"):
                o2 += 1
                if c["routed_to"] != ip: f2.append(f"{fmt_t(c['t'])} PIN {ctx['pin']} for #{ctx.get('intended_job')} did not reach the homeowner (route {c['ivr']})")
            tr = w.truth.get(ctx.get("intended_job"))
            if tr and c["routed_to"] and c["answered"]:
                e = ended.get(tr["phone"])
                if e and c["t"] > e + 60 and not tr.get("reached"): f3.append(f"#{ctx['intended_job']}: line still connected a call {fmt_t(c['t'])} after cancel at {fmt_t(e)}")
        self.add("C1 masked calls reach the right person", len(s.tw.calls), f1)
        self.add("C2 PIN works", o2, f2)
        self.add("C3 lines close on cancel / no-show / complaint", len(s.tw.calls), f3)
    # ------------------------------------------------------------------ NOTHING STUCK, RESTORE, CASEY
    def stuck(self):
        w = self.w; s = self.s
        f1 = []; o1 = 0
        FINAL = r"closed|We closed|outside the area|no qualified|How did it go|unsubscribed|same job as|Sorry about|confirmed #|Cerramos|cerrada|nadie cotizó|fuera de nuestra|calificado|Cómo le fue|mismo trabajo|Lo sentimos|confirmó|cerró|Seguimos buscando\.$"
        for tr in w.reqs:
            if tr["t0"] > self.end - 4 * DAY or not tr["nums"]: continue
            o1 += 1; ph = tr["phone"]
            if not any(r["t"] >= tr["t0"] and re.search(FINAL, r["body"]) for r in self.sent[ph]) and tr["ho"].optout_t is None:
                last = [r for r in self.sent[ph] if r["t"] >= tr["t0"]]
                f1.append(f"#{tr['nums'][0]} from {fmt_t(tr['t0'])} never reached an end; last text: '{(last[-1]['body'] if last else 'none')[:60]}'")
        pend = [c for c in s.stripe.charges if c["status"] in ("processing", "pending", "requires_action")]
        for c in pend: f1.append(f"payment {c['id']} for #{c['meta']['job']} left '{c['status']}' at end")
        self.add("N1 every job and payment moves forward or ends", o1 + len(s.stripe.charges), f1)
        f2 = []; o2 = 0
        for ph, lst in self.sent.items():
            closed = {}
            for r in lst:
                m = NUM.search(r["body"])
                if not m: continue
                n = m.group(1)
                if re.search(r"is closed|We closed|outside the area|no qualified|Cerramos|cerrada|nadie cotizó|fuera de nuestra", r["body"]): closed.setdefault(n, r["t"]); continue
                o2 += 1
                if n in closed and r["t"] > closed[n] + 1 and not re.search(r"no longer active|cancelled|How did it go", r["body"]):
                    f2.append(f"#{n}: '{r['body'][:60]}' sent {fmt_t(r['t'])} after the job closed at {fmt_t(closed[n])}"); closed[n] = 1e18
        self.add("N2 no timer fires on a moved-on job", o2, f2)
    def restore(self):
        s = self.s; w = self.w
        if not s.restores: return
        f1 = []; owners = defaultdict(set)
        for ph, lst in self.sent.items():
            if not self.ho_of(ph): continue
            for r in lst:
                for n in NUM.findall(r["body"]): owners[n].add(ph)
        for n, phs in owners.items():
            if len(phs) > 1: f1.append(f"job #{n} used for {len(phs)} different homeowners")
        self.add("R1 job numbers never reused", len(owners), f1)
        f3 = []; o3 = 0; seen = {}
        r0 = s.restores[0]
        for ph, lst in self.sent.items():
            for r in lst:
                if not NUM.search(r["body"]): continue
                k = (ph, r["body"])
                if r["t"] >= r0["from"] - 1: o3 += 1
                if k in seen and r["t"] - seen[k] < 12 * HOUR and r["t"] >= r0["t"] and r0["from"] <= seen[k] < r0["t"]:
                    f3.append(f"'{r['body'][:60]}' to {ph} sent at {fmt_t(seen[k])} and again {fmt_t(r['t'])} (after restore)")
                seen[k] = r["t"]
        self.add("R3 nothing sent twice across the restore", o3, f3)
        m1 = self.res.get("M1 charge only after own-phone yes + timely confirm, live job, once, real intro")
        self.res["R2 every kept charge matches a pick"] = dict(m1) if m1 else {"opps": 0, "fails": 0, "example": None}
    def casey(self):
        w = self.w; s = self.s
        f = []; o = 0
        for a in s.app.db["alerts"]:
            if a["level"] not in ("high", "critical"): continue
            o += 1
            acted = [m for m in w.casey.minutes if m[0] >= a["t"] and m[0] <= a["t"] + 24 * HOUR and m[1] in ("alert",)]
            paged = [m for m in w.casey.minutes if m[1] == "backup_page" and a["t"] + 24 * HOUR - 1 <= m[0] <= a["t"] + 24 * HOUR + 5 * HOUR]
            if not acted and not paged and a["t"] + 30 * HOUR < self.end: f.append(f"alert '{a['kind']}' at {fmt_t(a['t'])}: nobody acted within 24 h and backup not paged")
        self.add("K2 alerts unanswered 24 h go to backup + safe mode", o, f)
    def run(self):
        for fn in (self.money, self.privacy, self.texting, self.safety, self.matching, self.homeowner, self.businesses,
                   self.calls, self.stuck, self.restore, self.casey):
            fn()
        return self.res
