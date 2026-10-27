"""App part 3: quotes, picks, payment, release, check-ins, refunds, disputes, opt-outs, calls (Parts 3-6)."""
import re
from engine import DAY, HOUR, add_bmin, next_open, fmt_t, day, code_last_pass
from config import CATS, FEES
from app import DOMAIN, LIVE, ROC_LINK, norm

def fee_for(cfg, cat, q, biz_cat_group=None):
    """Part 6 fee rules. q = {'kind': fixed|range|trip|recurring, 'amount', 'lo', 'hi', 'monthly'}"""
    F = FEES[cfg["fees"]]
    if q["kind"] == "recurring": basis = q["monthly"] * F["rec_months"]
    elif q["kind"] == "range": basis = (q["lo"] + q["hi"]) / 2
    else: basis = q["amount"]
    def tier(v, t):
        return t[0] if v < 500 else (t[1] if v <= 1500 else t[2])
    k = F["kind"]
    if k == "flat": return F["f"]
    if k == "pct": return round(min(F["max"], max(F["min"], basis * F["pct"])), 2)
    if k == "approved":
        g = CATS[cat][5]
        if basis > 1500: return 149
        return 59 if g == "hvac" else 45
    if k == "catflat":
        return tier(CATS[cat][1] * (F["rec_months"] if CATS[cat][2] else 1) / (12 if CATS[cat][2] else 1), F["t"])
    if q["kind"] == "trip":
        tt = CATS[cat][6]; return {"small": F["t"][0], "medium": F["t"][1], "large": F["t"][2]}[tt]
    return tier(basis, F["t"])

class FlowMixin:
    # ---------------- quotes
    def web_quote(self, bid, num, q):
        j = self.db["jobs"].get(num); b = self.db["biz"].get(bid)
        if j is None or b is None or bid not in j["recipients"]: return "not_for_you"
        if j["status"] not in ("Open", "No quotes yet", "Quotes ready", "Awaiting confirm", "Payment pending"): return "closed"
        if self.now() > j["window_end"] and j["status"] != "No quotes yet" and not self.fix["window_matches_promise"]:
            if j["status"] == "Open": pass
        txt = q.get("text", "")
        if re.search(r"(\d[\s\-.()]*){7,}|https?://|www\.|\.com", txt) or re.search(r"\b(nine|eight|seven|six|five|four|three|two|one|zero)\b.*\b(nine|eight|seven|six|five|four|three|two|one|zero)\b.*\b(nine|eight|seven|six|five|four|three|two|one|zero)\b", txt.lower()):
            self.sms(b["phone"], "quote_bounce", f"Your quote for #{num} can't include phone numbers or links. Please resend it.", job=num, aud="biz")
            self.audit("quote_bounced", bid=bid, job=num); return "bounced"
        if b["lic_kind"] is None and (j["cat"] == "handyman" or self.lic_req(j["cat"], j["jt"]) is not None):
            fl = self.jt_flags(j["cat"], j["jt"])
            basis = q["monthly"] * 12 if q["kind"] == "recurring" else (q["hi"] if q["kind"] == "range" else q["amount"])
            if fl["gas"] or fl["fire"] or basis >= 1000:
                self.sms(b["phone"], "hm_refused", f"Quote for #{num} refused: without a contractor license, jobs must total under $1,000 with no gas, permit or fire-safety work.", job=num, aud="biz")
                self.audit("handyman_refused", bid=bid, job=num, basis=basis); return "refused"
        if q.get("via") == "sms" and q.get("ai_conf", 1) < 0.8:
            self.sms(b["phone"], "quote_confirm", f"Please confirm your #{num} quote on the page: {DOMAIN}/q/{j['token']}", job=num, aud="biz")
            self.sim.world.quote_needs_confirm(bid, num); return "confirm"
        first = bid not in j["quotes"]
        q = dict(q); q["t"] = self.now(); q["fee"] = fee_for(self.cfg, j["cat"], q)
        q["name"] = b["name"]
        j["quotes"][bid] = q
        if first: j["arrival"].append(bid)
        if j["first_quote_t"] is None: j["first_quote_t"] = self.now()
        if j["status"] in ("Open", "No quotes yet"):
            self.set_status(j, "Quotes ready"); self.send_quotes(j, first_time=True)
            self.timer(self.now() + 3 * DAY, "qr_close", num, j["gen"])
        elif j["status"] == "Quotes ready":
            self.queue_update(j)
        return "ok"
    def biz_inbound(self, frm, body, low):
        bid = self.db["phone_biz"][frm]; b = self.db["biz"][bid]
        up = body.strip().upper().rstrip(".!")
        if up in ("STOP", "CANCEL", "END", "QUIT", "UNSUBSCRIBE", "STOPALL", "REVOKE", "OPTOUT"):
            b["optout"] = True; b["optout_t"] = self.now(); self.pause_biz(b, "biz_optout"); return
        if up in ("START", "UNSTOP"): b["optout"] = False; return
        m = re.match(r"^(yes|confirm|si|sí)\s*#?(\d+)?$", low)
        if m:
            num = int(m.group(2)) if m.group(2) else None
            if num is None:
                pend = [j for j in self.db["jobs"].values() if j["status"] == "Awaiting confirm" and j["pick"] and j["pick"]["bid"] == bid]
                if len(pend) != 1:
                    self.sms(frm, "biz_which", "Which job? Reply YES and the job number, e.g. YES 1042.", aud="biz", direct=True); return
                num = pend[0]["num"]
            return self.biz_confirm(bid, num)
        m = re.match(r"^#?(\d{4})\s+(.*)$", body.strip())
        if m and self.sim.world.biz_sms_quote_routed(bid):
            num = int(m.group(1)); text = m.group(2)
            amt = self.sim.world.biz_quote_truth(bid, num)
            if amt is None: return
            try:
                r = self.sim.claude.read_quote(self.sim.eng.t, text, amt["amount"] if amt["kind"] != "recurring" else amt["monthly"])
            except Exception:
                return
            q = dict(amt)
            if q["kind"] == "recurring": q["monthly"] = r["amount"]
            elif q["kind"] == "range": pass
            else: q["amount"] = r["amount"]
            q["via"] = "sms"; q["ai_conf"] = r["conf"]
            return self.web_quote(bid, num, q)
        m = re.match(r"^link\s*(\d+)$", low)
        if m:
            p = self.db.get("link_pending", {}).get(bid)
            if p: self.link_staff(bid, p)
            return
        pend = [j for j in self.db["jobs"].values() if j["status"] in ("Awaiting confirm", "Payment pending") and j["pick"] and j["pick"]["bid"] == bid]
        if pend and any(w in low for w in ["what now", "notice", "what do i", "how do i", "confirm", "picked", "next"]):
            j = pend[-1]
            self.sms(frm, "biz_answer", f"For #{j['num']}: tap Confirm at {DOMAIN}/c/{j['token']} or reply YES {j['num']} before {fmt_t(j['pick']['deadline'])}. Then you get the details.", job=j["num"], aud="biz", direct=True)
            return
        self.needs("biz_text", prio=3, bid=bid, body=body[:80])
    def quote_line(self, q):
        if q["kind"] == "range": a = f"${q['lo']:.0f}-${q['hi']:.0f}"
        elif q["kind"] == "recurring": a = f"${q['monthly']:.0f}/mo"
        elif q["kind"] == "trip": a = f"${q['amount']:.0f} visit fee"
        else: a = f"${q['amount']:.0f}"
        return a
    def ordered(self, j):
        if self.fix["stable_list_numbers"]: return list(j["arrival"])
        def key(b):
            q = j["quotes"][b]
            return (q["monthly"] if q["kind"] == "recurring" else (q["lo"] + q["hi"]) / 2 if q["kind"] == "range" else q["amount"], j["arrival"].index(b))
        return sorted([b for b in j["arrival"] if b in j["quotes"]], key=key)
    def send_quotes(self, j, first_time=False, tpl=None):
        order = [b for b in self.ordered(j) if b not in j.get("excluded", [])]
        if not order: return
        j["list"] = order; j["last_update_t"] = self.now(); j["update_pending"] = False
        lines = " ".join(f"{i+1}) {j['quotes'][b]['name']} {self.quote_line(j['quotes'][b])}." for i, b in enumerate(order))
        hdr = "Your first quote is in" if first_time else "Updated quotes"
        body = f"{hdr} for #{j['num']}: {lines} Reply with the number and YES (e.g. 1 YES) to pick, or see {DOMAIN}/j/{j['token']}"
        es = f"Cotizaciones para #{j['num']}: {lines} Responda con el número y SÍ (ej. 1 SÍ) para elegir, o vea {DOMAIN}/j/{j['token']}"
        sid = self.sms(j["phone"], tpl or ("quotes_first" if first_time else "quotes_update"), body, job=j["num"], es=es)
        self.sim.world.quotes_shown(j["num"], list(order), sid)
    def t_qr_close(self, num, gen):
        j = self.db["jobs"][num]
        if j["status"] != "Quotes ready" or j["gen"] != gen: return
        if self.set_status(j, "Closed"):
            j["close_reason"] = "no_pick"; self.tell_losers(j, None)
            self.sms(j["phone"], "closed_notice", f"We closed #{num} since no quote was picked. Text us any time.", job=num, es=f"Cerramos #{num} porque no eligió una cotización.")
    def queue_update(self, j):
        if j["update_pending"]: return
        nxt = (j["last_update_t"] or -1e9) + 3 * HOUR
        j["update_pending"] = True
        self.timer(max(self.now(), nxt), "send_update", j["num"])
    def t_send_update(self, num):
        j = self.db["jobs"][num]
        if j["status"] != "Quotes ready": j["update_pending"] = False; return
        self.send_quotes(j)
    def send_list(self, h):
        j = self.newest_live(h)
        if j and j["status"] == "Quotes ready": self.send_quotes(j, tpl="list")
    # ---------------- picks
    def pick_by_text(self, h, n):
        j = None
        for jn in reversed(h["jobs"]):
            o = self.db["jobs"][jn]
            if o["status"] == "Quotes ready": j = o; break
        if j is None:
            self.sms(h["phone"], "pick_none", "We don't have quotes waiting for a pick right now.", direct=True, important=False,
                     es="No tenemos cotizaciones pendientes ahora."); return
        order = j.get("list", [])
        if not (1 <= n <= len(order)):
            self.sms(h["phone"], "pick_bad", f"Please reply with a number from 1 to {len(order)}.", job=j["num"], direct=True, es=f"Responda con un número del 1 al {len(order)}."); return
        self.do_pick(j, order[n - 1], via="sms")
    def web_pick(self, num, bid):
        j = self.db["jobs"][num]
        if j["status"] != "Quotes ready": return None
        vidx = self.sim.tw.verify_send(self.sim.eng.t, j["phone"], "sms")
        j["web_pick_pending"] = (bid, vidx); return vidx
    def web_pick_code(self, num, code, who):
        j = self.db["jobs"][num]
        if not j.get("web_pick_pending") or j["status"] != "Quotes ready": return False
        bid, vidx = j["web_pick_pending"]
        if self.sim.tw.verify_check(self.sim.eng.t, vidx, code, who):
            j["web_pick_pending"] = None; self.do_pick(j, bid, via="web"); return True
        return False
    def do_pick(self, j, bid, via):
        if bid in j.get("excluded", []): return
        self.db["seq"]["pick"] += 1
        j["pick"] = {"bid": bid, "k": self.db["seq"]["pick"], "t": self.now(), "via": via, "try": 0, "pay_t0": None}
        j["picks"].append(dict(j["pick"]))
        self.set_status(j, "Awaiting confirm")
        b = self.db["biz"][bid]; q = j["quotes"][bid]
        ent = self.entity(b)
        free = self.free_eligible(ent)
        fee_txt = "This first win is free." if free else f"Fee ${q['fee']:.0f}, charged when you confirm."
        dl = add_bmin(self.now(), 120)
        j["pick"]["deadline"] = dl
        self.sms(b["phone"], "picked", f"The homeowner picked you for #{j['num']} ({self.quote_line(q)}). {fee_txt} Reply YES {j['num']} by {fmt_t(dl)} to confirm and get their details.",
                 job=j["num"], aud="biz")
        self.sim.world.biz_picked(bid, j["num"], dl)
        self.sms(j["phone"], "pick_ack", f"Thanks. We've asked {b['name']} to confirm #{j['num']}. We'll text you when they do.", job=j["num"], direct=True,
                 es=f"Gracias. Le pedimos a {b['name']} que confirme #{j['num']}.")
        if not self.fix.get("losers_at_connect"): self.tell_losers(j, bid)
        self.timer(dl, "confirm_deadline", j["num"], j["pick"]["k"])
    def tell_losers(self, j, bid):
        for o in j["recipients"]:
            if o != bid and o not in j["losers_told"] and o in j["quotes"]:
                j["losers_told"].append(o)
                self.sms(self.db["biz"][o]["phone"], "loser", f"Job #{j['num']} is no longer active. Thanks for quoting.", job=j["num"], aud="biz", important=False)
                self.sim.world.biz_told_inactive(o, j["num"])
    def entity(self, b): return f"LIC:{b['lic']}" if b["lic"] else "NAME:" + re.sub(r"[^a-z]", "", b["name"].lower())
    def free_eligible(self, ent):
        if self.cfg["free_first_win"] == "none": return False
        if ent in self.db["free_win_entities"]: return False
        if self.cfg["free_first_win"] == "first90" and self.now() > 90 * DAY: return False
        return True
    def t_confirm_deadline(self, num, k):
        j = self.db["jobs"][num]
        if j["status"] != "Awaiting confirm" or j["pick"]["k"] != k: return
        self.release_pick(j, "no_confirm")
    def release_pick(self, j, why):
        bid = j["pick"]["bid"]
        j.setdefault("excluded", []).append(bid)
        self.audit("pick_released", job=j["num"], bid=bid, why=why)
        if not self.set_status(j, "Quotes ready"): return
        self.timer(self.now() + 3 * DAY, "qr_close", j["num"], j["gen"])
        rem = [b for b in self.ordered(j) if b not in j["excluded"]]
        if rem:
            self.sms(j["phone"], "pick_failed", f"{self.db['biz'][bid]['name']} couldn't take #{j['num']}. Here are your other quotes.", job=j["num"],
                     es=f"{self.db['biz'][bid]['name']} no pudo tomar #{j['num']}. Estas son sus otras cotizaciones.")
            self.send_quotes(j, tpl="quotes_after_fail")
        else:
            self.sms(j["phone"], "pick_failed", f"{self.db['biz'][bid]['name']} couldn't take #{j['num']}. We'll keep looking.", job=j["num"],
                     es=f"{self.db['biz'][bid]['name']} no pudo tomar #{j['num']}. Seguimos buscando.")
    def biz_confirm(self, bid, num):
        j = self.db["jobs"].get(num)
        if j is None or j["status"] != "Awaiting confirm" or j["pick"]["bid"] != bid: 
            self.audit("stray_confirm", bid=bid, job=num); return "not_pending"
        b = self.db["biz"][bid]
        if b["lic_kind"] in ("ROC", "PEST"):
            st = self.sim.roc.check(self.now(), b["lic"])
            if st == "inactive":
                self.pause_biz(b, "license_inactive"); self.release_pick(j, "license_inactive"); return "inactive"
        j["pick"]["confirm_t"] = self.now()
        ent = self.entity(b)
        if self.free_eligible(ent):
            self.db["free_win_entities"][ent] = num; j["free_win"] = True
            self.set_status(j, "Payment pending"); return self.connect(j)
        self.set_status(j, "Payment pending")
        j["pick"]["pay_t0"] = self.now()
        if not b.get("stripe_cust"):
            self.sms(b["phone"], "need_card", f"To get the details for #{num}, add a card or bank account: {DOMAIN}/pay/{bid} within 1 business hour.", job=num, aud="biz")
            self.sim.world.biz_needs_card(bid, num)
            self.timer(add_bmin(self.now(), 60), "pay_deadline", num, j["pick"]["k"], "card")
            return "need_card"
        return self.try_charge(j)
    def web_card_added(self, bid, cust):
        b = self.db["biz"][bid]; b["stripe_cust"] = cust; b["card_used"] = False
        for j in self.db["jobs"].values():
            if j["status"] == "Payment pending" and j["pick"]["bid"] == bid and not j["charges"]:
                self.try_charge(j)
    def charge_key(self, j):
        p = j["pick"]
        return f"pick-{j['num']}-{p['k']}" if self.fix["pick_idempotency_key"] else f"pick-{j['num']}-{p['k']}-try{p['try']}"
    def try_charge(self, j):
        p = j["pick"]; b = self.db["biz"][p["bid"]]; q = j["quotes"][p["bid"]]
        if self.db["pause_charging"] or self.db["safe_mode"]:
            self.timer(self.now() + 30, "pay_poll", j["num"], p["k"]); return "paused"
        key = self.charge_key(j)
        first = not b.get("card_used")
        r = self.sim.stripe.charge(self.sim.eng.t, b["stripe_cust"], q["fee"], key, {"job": j["num"], "bid": p["bid"], "k": p["k"]}, first_card_use=first)
        b["card_used"] = True
        res = r["result"]
        self.db["charge_keys"][key] = {"job": j["num"], "k": p["k"], "res": res, "cid": r.get("id")}
        if res == "succeeded":
            return self.charged(j, r["id"])
        if res == "timeout":
            p["try"] += 1
            if p["try"] <= 3: self.timer(self.now() + 1, "pay_retry", j["num"], p["k"])
            else: self.timer(self.now() + 30, "pay_poll", j["num"], p["k"])
            return "timeout"
        if res == "unreachable":
            self.timer(self.now() + 30, "pay_poll", j["num"], p["k"]); return "unreachable"
        if res == "declined":
            self.sms(b["phone"], "declined", f"Your card was declined for #{j['num']}. Update it at {DOMAIN}/pay/{p['bid']} within 1 business hour.", job=j["num"], aud="biz")
            self.sim.world.biz_declined(p["bid"], j["num"])
            p["try"] += 1
            self.timer(add_bmin(self.now(), 60), "pay_deadline", j["num"], p["k"], "declined"); return "declined"
        if res == "requires_action":
            p["action_cid"] = r["id"]
            self.sms(b["phone"], "tds", f"Your bank needs you to approve the #{j['num']} charge: {DOMAIN}/pay/{p['bid']}/approve", job=j["num"], aud="biz")
            self.sim.world.biz_3ds(p["bid"], j["num"], r["id"])
            self.timer(add_bmin(self.now(), 60), "pay_deadline", j["num"], p["k"], "3ds"); return "3ds"
    def t_pay_retry(self, num, k):
        j = self.db["jobs"][num]
        if j["status"] == "Payment pending" and j["pick"]["k"] == k and not j["charges"]: self.try_charge(j)
    def t_pay_poll(self, num, k):
        j = self.db["jobs"][num]
        if j["status"] != "Payment pending" or j["pick"]["k"] != k or j["charges"]: return
        if self.now() - (j["pick"]["pay_t0"] or self.now()) > 4 * HOUR:
            return self.release_pick(j, "stripe_unreachable")
        r = self.sim.stripe.retrieve_by_key(self.sim.eng.t, self.charge_key(j))
        if isinstance(r, dict) and r["status"] == "succeeded": return self.charged(j, r["id"])
        if r == "unreachable" or self.db["pause_charging"] or self.db["safe_mode"]:
            self.timer(self.now() + 30, "pay_poll", num, k); return
        self.try_charge(j)
    def t_pay_deadline(self, num, k, why):
        j = self.db["jobs"][num]
        if j["status"] != "Payment pending" or j["pick"]["k"] != k or j["charges"]: return
        self.release_pick(j, "pay_" + why)
    def web_retry_payment(self, bid, num):
        j = self.db["jobs"].get(num)
        if j and j["status"] == "Payment pending" and j["pick"]["bid"] == bid and not j["charges"]:
            j["pick"]["try"] += 1; self.try_charge(j)
    def web_3ds_done(self, bid, num, cid, ok):
        st = self.sim.stripe.complete_action(self.sim.eng.t, cid, ok)
        j = self.db["jobs"].get(num)
        if st == "succeeded" and j and j["status"] == "Payment pending" and j["pick"]["bid"] == bid and not j["charges"]:
            self.charged(j, cid)
    def on_stripe_event(self, typ, cid):
        if cid in self.db["handled_events"] and not self.replaying: return
        self.db["handled_events"].add(cid)
        ch = self.sim.stripe.charges[cid]
        if typ != "payment_intent.succeeded": return
        self.db["known_charges"].add(cid)
        j = self.db["jobs"].get(ch["meta"]["job"])
        if j and j["status"] == "Payment pending" and j["pick"]["k"] == ch["meta"]["k"] and not j["charges"]:
            self.charged(j, cid)
    def charged(self, j, cid):
        if cid in j["charges"]: return
        j["charges"].append(cid); self.db["known_charges"].add(cid)
        ch = self.sim.stripe.charges[cid]
        self.db["ledger"].append({"t": self.now(), "job": j["num"], "cid": cid, "amount": ch["amount"], "kind": "charge"})
        if j["status"] == "Payment pending": self.connect(j)
    def connect(self, j):
        if not self.set_status(j, "Connected"): return
        p = j["pick"]; b = self.db["biz"][p["bid"]]; h = self.db["ho"][j["phone"]]
        j["released"] = self.now(); j["calls_ok"] = 0
        mms = 0 if self.fix["no_photos_on_cards"] else len(j["ok_photos"])
        body = (f"Confirmed #{j['num']}: {h.get('fname') or 'Homeowner'}, {h.get('addr') or 'address on file'}. Call them through (928) 555-0199 "
                f"and enter PIN {j['pin']} if asked.")
        self.sms(b["phone"], "details", body, job=j["num"], mms=mms, aud="biz")
        self.sim.world.details_released(p["bid"], j["num"], {"fname": h.get("fname"), "addr": h.get("addr"), "free": j["free_win"]})
        self.sms(j["phone"], "connected", f"{b['name']} confirmed #{j['num']} and will call you from (928) 555-0199.", job=j["num"],
                 es=f"{b['name']} confirmó #{j['num']} y le llamará desde (928) 555-0199.")
        if self.fix.get("losers_at_connect"): self.tell_losers(j, p["bid"])
        self.timer(self.now() + 72 * HOUR, "never_called", j["num"], p["k"])
        self.timer(self.now() + 96 * HOUR, "unreach_check", j["num"], p["k"])
        self.timer(next_open(self.now() + 3 * DAY - 8 * HOUR) + 2 * HOUR, "checkin", j["num"], 3)
        self.timer(next_open(self.now() + 14 * DAY - 8 * HOUR) + 2 * HOUR, "checkin", j["num"], 14)
    def t_never_called(self, num, k):
        j = self.db["jobs"][num]
        if j["status"] != "Connected" or j["pick"]["k"] != k: return
        if self.call_stats(j)["attempts"] == 0:
            self.audit("never_called", job=num)
            j.setdefault("excluded", []).append(j["pick"]["bid"])
            self.set_status(j, "Quotes ready")
            self.timer(self.now() + 3 * DAY, "qr_close", num, j["gen"])
            rem = [b for b in self.ordered(j) if b not in j["excluded"]]
            if rem:
                self.sms(j["phone"], "noshow", f"{self.db['biz'][j['pick']['bid']]['name']} didn't call about #{num}. You can pick another quote.", job=num,
                         es=f"{self.db['biz'][j['pick']['bid']]['name']} no llamó por #{num}. Puede elegir otra cotización.")
                self.send_quotes(j, tpl="quotes_after_noshow")
            else:
                self.set_status(j, "Closed"); j["close_reason"] = "noshow_no_others"
    def call_stats(self, j):
        o = self.db["call_obs"].get(j["num"], [])
        att = [c for c in o if c["dir"] == "b2h"]
        return {"attempts": len(att), "answered": sum(1 for c in o if c["answered"]), "times": [c["t"] for c in att]}
    def t_unreach_check(self, num, k):
        j = self.db["jobs"][num]
        if j["status"] != "Connected" or j["pick"]["k"] != k: return
        s = self.call_stats(j)
        if s["answered"] == 0 and s["attempts"] >= 2 and max(s["times"]) - min(s["times"]) >= 3 * HOUR:
            self.refund_job(j, "unreachable")
            self.set_status(j, "Closed"); j["close_reason"] = "unreachable"
    def t_checkin(self, num, d):
        j = self.db["jobs"][num]
        if j["status"] != "Connected": return
        b = self.db["biz"][j["pick"]["bid"]]
        self.sms(j["phone"], f"checkin{d}", f"How did it go with {b['name']} on #{num}? Reply DONE if finished, NO if they never contacted you, or PROBLEM.",
                 job=num, es=f"¿Cómo le fue con {b['name']} en #{num}? Responda LISTO, NO si nunca le contactaron, o PROBLEMA.")
        if d == 14: self.timer(self.now() + 7 * DAY, "auto_done", num)
    def t_auto_done(self, num):
        j = self.db["jobs"][num]
        if j["status"] == "Connected": self.set_status(j, "Done"); j["close_reason"] = "auto_done"
    def checkin_reply(self, h, low):
        j = None
        for jn in reversed(h["jobs"]):
            if self.db["jobs"][jn]["status"] == "Connected": j = self.db["jobs"][jn]; break
        if j is None: return
        if low in ("done", "listo"):
            self.set_status(j, "Done"); j["close_reason"] = "done"; return
        if low == "no":
            s = self.call_stats(j)
            if s["answered"] == 0:
                self.refund_job(j, "never_contacted"); self.set_status(j, "Closed"); j["close_reason"] = "never_contacted"
                self.sms(j["phone"], "sorry", f"Sorry about #{j['num']}. We've closed it; text us any time for new quotes.", job=j["num"], direct=True, es=f"Lo sentimos por #{j['num']}. La cerramos.")
            else:
                self.needs("contact_dispute", prio=2, job=j["num"])
            return
        b = self.db["biz"][j["pick"]["bid"]]
        self.db["complaints"].setdefault(b["id"], []).append(self.now())
        self.sms(j["phone"], "complaint", f"Sorry to hear that. You can reach {b['name']} at {b['phone']}. For licensed-contractor problems you can file at {ROC_LINK}. We've noted it.",
                 job=j["num"], direct=True, es=f"Lo sentimos. Puede llamar a {b['name']} al {b['phone']}. Puede presentar una queja en {ROC_LINK}.")
        self.needs("complaint", prio=2, job=j["num"], bid=b["id"])
        recent = [t for t in self.db["complaints"][b["id"]] if self.now() - t <= 90 * DAY]
        if len(recent) >= 3: self.pause_biz(b, "complaints")
    # ---------------- cancel / opt-out
    def ho_cancel(self, h, num):
        js = [self.db["jobs"][n] for n in h["jobs"] if self.db["jobs"][n]["status"] in LIVE and (num is None or n == num)]
        if num is not None and not js:
            self.sms(h["phone"], "cancel_none", f"We couldn't find an open request #{num}.", direct=True, important=False, es=f"No encontramos la solicitud abierta #{num}."); return
        for j in js[-1:] if num is None else js: self.cancel_job(j, "homeowner")
    def cancel_job(self, j, why):
        if j["status"] == "Connected":
            if self.call_stats(j)["answered"] == 0: self.refund_job(j, "cancel_before_contact")
        st0 = j["status"]
        if not self.set_status(j, "Cancelled"): return
        j["close_reason"] = why
        if st0 in ("Awaiting confirm", "Payment pending", "Connected") and j["pick"]:
            self.sms(self.db["biz"][j["pick"]["bid"]]["phone"], "job_cancelled", f"Job #{j['num']} was cancelled by the homeowner.", job=j["num"], aud="biz")
        self.tell_losers(j, j["pick"]["bid"] if j["pick"] else None)
        if why == "homeowner":
            self.sms(j["phone"], "cancelled", f"Request #{j['num']} is closed.", job=j["num"], direct=True, important=False, es=f"La solicitud #{j['num']} está cerrada.")
    def do_optout(self, h, carrier, word):
        h["optout"] = True; h["optout_t"] = self.now(); h["await"] = None
        self.audit("optout", phone=h["phone"], carrier=carrier, word=word)
        for jn in h["jobs"]:
            j = self.db["jobs"][jn]
            if j["status"] in LIVE: self.cancel_job(j, "optout")
        if not carrier:
            self.sms(h["phone"], "optout_confirm", self.optout_confirm_text(h["phone"]), exempt=True, important=False)
    # ---------------- refunds, disputes, reconciliation
    def refund_job(self, j, why, frac=1.0):
        for cid in j["charges"]:
            key = f"refund-{cid}-{why}"
            if key in self.db["refund_done"]: continue
            amt = self.sim.stripe.charges[cid]["amount"] * frac
            self.db["refund_done"][key] = "pending"
            self.do_refund(cid, amt, key, 0, j["num"], why)
    def do_refund(self, cid, amt, key, n, num, why):
        r = self.sim.stripe.refund(self.sim.eng.t, cid, amt, key)
        if r == "ok":
            self.db["refund_done"][key] = "ok"
            self.db["ledger"].append({"t": self.now(), "job": num, "cid": cid, "amount": -amt, "kind": "refund", "why": why})
        elif r == "disputed":
            self.db["refund_done"][key] = "disputed"; self.needs("refund_fail", prio=2, cid=cid, why="disputed")
        else:
            if n < 6: self.timer(self.now() + HOUR, "refund_retry", cid, amt, key, n + 1, num, why)
            else:
                self.db["refund_done"][key] = "failed"; self.needs("refund_fail", prio=1, cid=cid, why=why)
    def t_refund_retry(self, cid, amt, key, n, num, why): self.do_refund(cid, amt, key, n, num, why)
    def on_dispute(self, i):
        d = self.sim.stripe.disputes[i]
        self.needs("dispute", prio=1, idx=i, due=d["due"])
    def t_reconcile(self):
        ch = self.sim.stripe.charges
        led = {e["cid"] for e in self.db["ledger"] if e["kind"] == "charge"}
        for c in ch:
            if c["status"] == "succeeded" and c["id"] not in led and not c.get("recon_flag") and c["t"] < self.now():
                c["recon_flag"] = True
                self.alert("unknown_charge", "high", cid=c["id"])
        self.timer(self.now() + DAY, "reconcile")
    def recurring_stop(self, num):
        j = self.db["jobs"][num]
        if self.cfg["recurring_partial_refund"] and j["charges"]:
            self.refund_job(j, "recurring_stopped", frac=0.5)
    def t_weekly(self):
        # Owner-portal flag for quotes that cluster just under $500 or $1,500 (Part 5.6)
        for bid, b in self.db["biz"].items():
            if b.get("shade_flagged"): continue
            n = 0
            for j in self.db["jobs"].values():
                q = j["quotes"].get(bid)
                if q and q["kind"] == "fixed" and (450 <= q["amount"] < 500 or 1400 <= q["amount"] < 1500): n += 1
            if n >= 3:
                b["shade_flagged"] = self.now(); self.needs("shade_flag", prio=3, bid=bid)
        self.needs("spot_check", prio=3); self.needs("ledger", prio=3)
        # Part 11 opt-out alarms: >3% of homeowners or >5% of businesses texted in the last 7 days opted out
        now = self.now(); ho_n = ho_o = bz_n = bz_o = 0
        for h in self.db["ho"].values():
            if h.get("last_in") is not None and now - h["last_in"] <= 7 * DAY or (h.get("optout_t") and now - h["optout_t"] <= 7 * DAY):
                ho_n += 1; ho_o += 1 if (h.get("optout_t") and now - h["optout_t"] <= 7 * DAY) else 0
        for b in self.db["biz"].values():
            if b["active"]:
                bz_n += 1; bz_o += 1 if (b.get("optout") and now - b.get("optout_t", 0) <= 7 * DAY) else 0
        if ho_n >= 10 and ho_o / ho_n > 0.03: self.alert("optout_rate_homeowners", "high", rate=ho_o / ho_n)
        if bz_n >= 10 and bz_o / bz_n > 0.05: self.alert("optout_rate_businesses", "high", rate=bz_o / bz_n)
        self.timer(self.now() + 7 * DAY, "weekly")
    # ---------------- businesses
    def pause_biz(self, b, why):
        if not b["paused"]:
            b["paused"] = why; b["paused_t"] = self.now()
            self.needs("pause_review", prio=2, bid=b["id"], why=why)
    def unpause(self, bid):
        b = self.db["biz"][bid]; b["paused"] = None
