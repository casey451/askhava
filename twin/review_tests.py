"""
Regression tests for the problems found in the independent code review (round 2).
Each test replays one exact situation, step by step, against the app as written (v9.1) and with all fixes (v9.3).

    python review_tests.py
"""
import json, copy, os, re
import twin
from app import DAY

HERE = os.path.dirname(os.path.abspath(__file__))


class Scripted(twin.Homeowner):
    """A homeowner who does only what the test tells them to (records every text they get)."""
    def __init__(self, sim, idx):
        super().__init__(sim, idx, "normal")
        self.inbox = []

    def on_sms(self, rec):
        self.inbox.append(rec)

    def current_truth(self, body):
        trade = next((t for t, xs in twin.JOB_TEXT.items() if any(x.lower() in body.lower() for x in xs)), "plumbing")
        jt = next((v for k, v in twin.JOB_TYPE_OF.items() if k.lower() in body.lower()), f"{trade}_other")
        return dict(trade=trade, job_type=jt, raw_text=body, emergency="gas" in body.lower())

    def say(self, body):
        self.sim.sms.inbound(self.phone, body, [])


def make(hardening, **vendor):
    base = json.load(open(os.path.join(HERE, "twin_config.json")))
    sc = json.load(open(os.path.join(HERE, "twin_scenarios.json")))
    cfg = copy.deepcopy(base)
    if hardening:
        cfg = twin.deep_update(cfg, copy.deepcopy(sc["normal_fixed"]["overrides"]))
    else:
        cfg = twin.deep_update(cfg, copy.deepcopy(sc["normal_fixed"]["overrides"]))
        cfg["app"]["fixes"]["hardening"] = False          # v9.1: every earlier fix, none of the later ones
        cfg["app"]["fixes"]["v93"] = False
        cfg["app"]["fixes"]["owner_checklist"] = False
        cfg["app"]["license_recheck_mode"] = "daily"
    cfg.update(requests_per_day=0, fraud_signups=0, p_shared_entity=0.0, contractor_personas={"responsive": 1.0})
    cfg["vendors"].update(dict(dict(sms_fail_prob=0, sms_delay_prob=0, webhook_dup_prob=0, webhook_delay_prob=0, roc_down_prob=0,
                          roc_captcha_prob=0, roc_layout_change_prob=0, card_decline_prob=0), **vendor))
    sim = twin.build(cfg, 7)
    run_until(sim, 0.9 * DAY)
    for a in list(sim.contractors.values()):
        a.confirm_p = 0.0                                  # contractors confirm only when the test says so (after they've signed up)
    for coid, c in sim.app.contractors.items():           # paying customers (first free win already used, card on file)
        c["first_win_used"] = True
        sim.app.contractor_card_added(coid, dict(good=True, needs_3ds=False))
    return sim


def run_until(sim, t):
    while sim.q and sim.q[0][0] <= t:
        tt, _, _, fn, a = twin.heapq.heappop(sim.q)
        sim.now = tt
        fn(*a)
    sim.now = max(sim.now, t)


def to_quotes(sim, ho):
    """Day 1, 9 a.m.: homeowner asks for AC help and says OK; run until the quote link arrives."""
    run_until(sim, DAY + 9 * 60)
    ho.say("AC blowing warm air 86403")
    run_until(sim, sim.now + 5)
    ho.say("OK")
    run_until(sim, sim.now + 5)
    if ho.inbox and ho.inbox[-1]["kind"] == "intake_q" and "Reply" in ho.inbox[-1]["body"]:
        ho.say("1"); run_until(sim, sim.now + 5)
    run_until(sim, sim.now + 8 * 60)
    rid = sim.app.homeowners[sim.app.phone_to_ho[ho.phone]]["requests"][-1]
    return sim.app.requests[rid]


def pick(sim, ho, r):
    qid = sim.app.visible_quotes(r)[0]
    if sim.app.page_choose(r["token"], qid) == "code_sent":
        run_until(sim, sim.now + 5)
        code = [m for m in ho.inbox if m["kind"] == "pick_code"][-1]["body"].split("code is ")[1][:6]
        ho.authorized[r["id"]].append(sim.now)
        sim.app.page_enter_code(r["token"], code)
    else:
        ho.authorized[r["id"]].append(sim.now)
        ho.say("YES")
        run_until(sim, sim.now + 2)
    return sim.app.picks[r["pick"]]


def kept_charges(sim):
    return [pi for pi in sim.stripe.intents.values() if pi["status"] == "succeeded" and pi["refunded"] < pi["amount"]]


# ------------------------------------------------------------------------------------------------ tests
def t_cancel_during_stripe_outage(h):
    sim = make(h); ho = Scripted(sim, 1); r = to_quotes(sim, ho); p = pick(sim, ho, r)
    sim.stripe.down_until = sim.now + 60
    sim.app.contractor_confirm(p["co"], r["id"])            # Stripe is down: the app retries every 10 minutes
    run_until(sim, sim.now + 15)
    sim.app.cancel_request(r, "homeowner")                    # homeowner closes the request meanwhile
    run_until(sim, sim.now + 6 * 60)
    k = kept_charges(sim)
    return not k, f"request {r['status']}, contractor charged and kept: {['$%d' % pi['amount'] for pi in k] or 'nothing'}"


def t_bare_cancel_closes_request(h):
    sim = make(h); ho = Scripted(sim, 1)
    run_until(sim, DAY + 9 * 60); ho.say("Toilet keeps running 86403"); run_until(sim, sim.now + 5); ho.say("OK"); run_until(sim, sim.now + 5)
    rid = sim.app.homeowners[sim.app.phone_to_ho[ho.phone]]["requests"][-1]
    ho.say("CANCEL"); run_until(sim, sim.now + 3 * 60)
    r = sim.app.requests[rid]
    return r["status"] == "CANCELLED", f"after a bare CANCEL the request is {r['status']} and has {len(r['quotes'])} quotes nobody will see"


def t_gas_at_consent_during_ai_outage(h):
    sim = make(h); ho = Scripted(sim, 1)
    run_until(sim, DAY + 9 * 60)
    sim.ai_intake.down_until = sim.now + 6 * 60
    ho.say("Hi need help with water heater 86403"); run_until(sim, sim.now + 3)
    ho.say("I smell gas now"); run_until(sim, sim.now + 5)
    got = [m for m in ho.inbox if m["kind"] == "safety"]
    return bool(got), f"safety text within 5 minutes: {'yes' if got else 'no'}"


def t_yes_at_consent(h):
    sim = make(h); ho = Scripted(sim, 1)
    run_until(sim, DAY + 9 * 60); ho.say("Breaker keeps tripping 86403"); run_until(sim, sim.now + 5)
    ho.say("Yes"); run_until(sim, sim.now + 10)
    hh = sim.app.homeowners[sim.app.phone_to_ho[ho.phone]]
    return hh["consented"], f"'Yes' accepted as consent: {hh['consented']}; requests created: {len(hh['requests'])}"


def t_lost_stripe_webhook(h):
    sim = make(h, webhook_loss_prob=1.0); ho = Scripted(sim, 1); r = to_quotes(sim, ho); p = pick(sim, ho, r)
    sim.app.contractor_confirm(p["co"], r["id"])
    run_until(sim, sim.now + 2 * 60)
    ok = r["status"] == "CONNECTED"
    return ok, f"Stripe took the money but its notice never arrived: request is {r['status']}, contractor {'has' if ok else 'does NOT have'} the job"


def t_one_free_win_per_business(h):
    sim = make(h); app = sim.app
    a, b = [c for c in app.contractors.values() if c["trade"] == "hvac"][:2]
    b["entity"] = a["entity"]                                  # same business, two accounts, both signed up before either won
    a["first_win_used"] = b["first_win_used"] = False
    wins = 0
    for i, c in enumerate((a, b)):
        ho = Scripted(sim, 10 + i); r = to_quotes(sim, ho)
        qid = next((q for q in r["quotes"] if app.quotes[q]["co"] == c["id"]), None)
        if not qid:
            app.add_quote(r, c["id"], dict(type="firm", amount=240, high=None, original="firm 240"))
            if r["status"] == "OPEN":
                app.quotes_ready(r)
            run_until(sim, sim.now + 5)
            qid = next((q for q in r["quotes"] if app.quotes[q]["co"] == c["id"]), None)
        app.page_choose(r["token"], qid); run_until(sim, sim.now + 5)
        code = [m for m in ho.inbox if m["kind"] == "pick_code"][-1]["body"].split("code is ")[1][:6]
        app.page_enter_code(r["token"], code)
        app.contractor_confirm(c["id"], r["id"]); run_until(sim, sim.now + 30)
        wins += app.picks[r["pick"]]["free"]
    return wins <= 1, f"free first wins given to one business: {wins}"


def t_handyman_cap(h):
    sim = make(h); app = sim.app
    run_until(sim, DAY + 9 * 60)
    ho = Scripted(sim, 1); ho.say("Patch drywall in hallway 86403"); run_until(sim, sim.now + 5); ho.say("OK"); run_until(sim, sim.now + 5)
    rid = app.homeowners[app.phone_to_ho[ho.phone]]["requests"][-1]
    r = app.requests[rid]
    k = next((k for k in r["cards"] if app.contractors[app.cards[k]["co"]]["trade"] == "handyman"), None)
    if not k:
        return True, "couldn't arrange the test (no handyman card)"
    res = app.contractor_quote_portal(app.cards[k]["co"], rid, dict(type="firm", amount=1350, high=None, original="firm 1350"))
    shown = any(app.quotes[q]["amount"] == 1350 for q in r["quotes"])
    return not shown, f"a $1,350 handyman quote {'was shown to the homeowner' if shown else 'was blocked'}"


def t_database_restore(h):
    """A pick, payment and introduction happen; a new request arrives; then the database is restored to before all of it."""
    sim = make(h); app = sim.app
    ho = Scripted(sim, 1); r = to_quotes(sim, ho)
    t0 = sim.now; snap = app.snapshot()
    p = pick(sim, ho, r)
    app.contractor_confirm(p["co"], r["id"]); run_until(sim, sim.now + 20)
    ho2 = Scripted(sim, 2); ho2.say("Toilet keeps running 86403"); run_until(sim, sim.now + 3); ho2.say("OK"); run_until(sim, sim.now + 10)
    lost_number = app.homeowners[app.phone_to_ho[ho2.phone]]["requests"][-1]
    app.restore(snap, t0); run_until(sim, sim.now + 60)
    ho3 = Scripted(sim, 3); ho3.say("Breaker keeps tripping 86403"); run_until(sim, sim.now + 3); ho3.say("OK"); run_until(sim, sim.now + 10)
    new_numbers = app.homeowners[app.phone_to_ho[ho3.phone]]["requests"] if ho3.phone in app.phone_to_ho else []
    kept = [pi for pi in kept_charges(sim) if pi["meta"]["pick"] not in app.picks]
    reused = lost_number in new_numbers
    rr = app.requests.get(r["id"])
    ok = not kept and not reused and rr and rr["status"] == "CONNECTED"
    return ok, (f"after the restore: request {r['id']} is {rr['status'] if rr else 'missing'} (the contractor already has the details); "
                f"{len(kept)} charge(s) kept with no record; job number {lost_number} {'REUSED for a different homeowner' if reused else 'not reused'}")


# ------------------------------------------------------------------------------------- same-moment collisions
def capture_stripe(sim):
    """Hold Stripe's notices so the test decides exactly when (and in which order) they arrive."""
    held = []
    def _wh(typ, pid):
        ev = dict(id=f"evt_{len(sim.stripe.events)+1}", type=typ, pid=pid, created=sim.now)
        sim.stripe.events.append(ev); held.append(ev)
    sim.stripe._webhook = _wh
    return held


def money_ok(sim):
    """Every kept charge bought a real introduction; nothing refunded twice; the books match Stripe."""
    app = sim.app; problems = []
    for pi in sim.stripe.intents.values():
        if pi["status"] == "succeeded" and pi["refunded"] < pi["amount"]:
            p = app.picks.get(pi["meta"]["pick"])
            sent = any(m["kind"] == "connect_contractor" and m["meta"].get("ref") == pi["meta"]["req"] for m in sim.sms.log)
            if not p or not sent:
                problems.append(f"{pi['id']} kept without an introduction")
        if pi["refunded"] > pi["amount"]:
            problems.append(f"{pi['id']} over-refunded")
        booked = -sum(l["amount"] for l in app.ledger if l["type"] == "refund" and l.get("stripe") == pi["id"])
        if abs(booked - pi["refunded"]) > 0.01:
            problems.append(f"{pi['id']} refunds: books ${booked} vs Stripe ${pi['refunded']}")
    return problems


def both_orders(h, setup, a, b):
    out = []
    for order in ("AB", "BA"):
        sim, ctx = setup(h)
        (a if order == "AB" else b)(sim, ctx); (b if order == "AB" else a)(sim, ctx)
        run_until(sim, sim.now + 3 * DAY)
        probs = money_ok(sim)
        out.append(f"{order}: {'ok' if not probs else '; '.join(probs)}")
    return all(x.endswith("ok") for x in out), " | ".join(out)


def setup_charging(h):
    sim = make(h); held = capture_stripe(sim); ho = Scripted(sim, 1); r = to_quotes(sim, ho); p = pick(sim, ho, r)
    sim.app.contractor_confirm(p["co"], r["id"])
    return sim, dict(r=r, p=p, held=held, ho=ho)


def t_race_close_vs_payment(h):
    def A(sim, c): sim.app.cancel_request(c["r"], "homeowner")
    def B(sim, c):
        for ev in list(c["held"]): sim.app.on_stripe_webhook(ev)
    return both_orders(h, setup_charging, A, B)


def t_race_deadline_vs_confirm(h):
    def setup(h):
        sim = make(h); held = capture_stripe(sim); ho = Scripted(sim, 1); r = to_quotes(sim, ho); p = pick(sim, ho, r)
        return sim, dict(r=r, p=p, held=held)
    def A(sim, c): sim.app.t_confirm_deadline(c["r"]["pick"])
    def B(sim, c):
        sim.app.contractor_confirm(c["p"]["co"], c["r"]["id"])
        for ev in list(c["held"]): sim.app.on_stripe_webhook(ev)
    return both_orders(h, setup, A, B)


def t_race_auth_deadline_vs_approval(h):
    def setup(h):
        sim = make(h); held = capture_stripe(sim); ho = Scripted(sim, 1); r = to_quotes(sim, ho); p = pick(sim, ho, r)
        cust = sim.app.contractors[p["co"]]["stripe"]; sim.stripe.customers[cust]["pm"] = dict(good=True, needs_3ds=True)
        real = sim.rng.random; sim.rng.random = lambda: 0.1               # force 'bank approval needed'
        sim.app.contractor_confirm(p["co"], r["id"]); sim.rng.random = real
        for ev in list(held): sim.app.on_stripe_webhook(ev)
        held.clear()
        return sim, dict(r=r, p=sim.app.picks[r["pick"]], held=held)
    def A(sim, c): sim.app.t_payment_fix_deadline(c["p"]["id"])
    def B(sim, c):
        sim.stripe.complete_authentication(c["p"].get("pi") or c["p"]["charge"], True)
        for ev in list(c["held"]): sim.app.on_stripe_webhook(ev)
    return both_orders(h, setup, A, B)


def t_race_refund_decision_vs_call(h):
    def setup(h):
        sim, c = setup_charging(h)
        for ev in list(c["held"]): sim.app.on_stripe_webhook(ev)
        p = sim.app.picks[c["r"]["pick"]]
        p.setdefault("calls", []).extend([dict(sid=p.get("session"), at=p["released_at"] + 60, human=False, seconds=0, rejected=None),
                                          dict(sid=p.get("session"), at=p["released_at"] + 300, human=False, seconds=0, rejected=None)])
        return sim, dict(p=p)
    def A(sim, c): sim.app.t_refund_decision(c["p"]["id"]); sim.app.t_refund_decision(c["p"]["id"])
    def B(sim, c): sim.app.on_call_completed(dict(sid=c["p"].get("session"), at=sim.now, human=True, seconds=300, rejected=None))
    return both_orders(h, setup, A, B)


def t_race_notice_before_save(h):
    sim = make(h, card_decline_prob=1.0); ho = Scripted(sim, 1); r = to_quotes(sim, ho); p = pick(sim, ho, r)
    sim.stripe._webhook = lambda typ, pid: sim.app.on_stripe_webhook(dict(id=f"evt_now{pid}{typ}", type=typ, pid=pid, created=sim.now))
    sim.app.contractor_confirm(p["co"], r["id"])                 # Stripe's 'declined' notice lands before charge() returns
    run_until(sim, sim.now + 5)
    pp = sim.app.picks[r["pick"]]
    told = any(m["kind"] == "payment_failed" for m in sim.sms.log)
    return pp["status"] in ("payment_failed", "cancelled") and told, f"pick is '{pp['status']}', contractor told about the declined card: {told}"


# ------------------------------------------------------------------------------------- restore edge cases
def _restore(sim, snap, t0, stripe_down=False):
    if stripe_down:
        sim.stripe.down_until = sim.now + 30
    sim.app.restore(snap, t0)
    run_until(sim, sim.now + 3 * 60)


def t_restore_late_notice_stripe_down(h):
    sim, c = setup_charging(h); app = sim.app
    t0 = sim.now; snap = app.snapshot()                          # charge created before the restore point
    for ev in list(c["held"]): app.on_stripe_webhook(ev)          # ...its 'succeeded' notice lands in the lost window
    run_until(sim, sim.now + 10)
    sim.stripe._webhook = lambda typ, pid: None
    _restore(sim, snap, t0, stripe_down=True)
    r = app.requests[c["r"]["id"]]; free = [l for l in app.ledger if l["type"] == "free_first_win"]
    probs = money_ok(sim)
    ok = r["status"] == "CONNECTED" and not free and not probs and sum(1 for l in app.ledger if l["type"] == "charge") == 1
    return ok, f"request {r['status']}, free wins recorded {len(free)}, charges booked {sum(1 for l in app.ledger if l['type']=='charge')}, problems: {probs or 'none'}"


def t_restore_free_win_in_window(h):
    sim = make(h); app = sim.app; ho = Scripted(sim, 1); r = to_quotes(sim, ho); p = pick(sim, ho, r)
    app.contractors[p["co"]]["first_win_used"] = False
    t0 = sim.now; snap = app.snapshot()
    app.contractor_confirm(p["co"], r["id"]); run_until(sim, sim.now + 20)          # free first win, introduced in the window
    _restore(sim, snap, t0)
    rr = app.requests[r["id"]]
    told_couldnt = any(m["kind"] in ("reoffer", "reoffer_none") for m in ho.inbox)
    ok = rr["status"] == "CONNECTED" and app.contractors[p["co"]]["first_win_used"] and not told_couldnt and len([l for l in app.ledger if l["type"] == "free_first_win"]) == 1
    return ok, f"request {rr['status']}, first win used: {app.contractors[p['co']]['first_win_used']}, homeowner wrongly told 'couldn't take': {told_couldnt}"


def t_restore_call_then_close(h):
    sim, c = setup_charging(h); app = sim.app
    for ev in list(c["held"]): app.on_stripe_webhook(ev)
    run_until(sim, sim.now + 5)
    t0 = sim.now; snap = app.snapshot()
    p = app.picks[c["r"]["pick"]]; ho = c["ho"]; ho.answer_p = 1.0
    real = sim.rng.random; sim.rng.random = lambda: 0.5
    sim.voice.place_call(p["session"], sim.contractors[p["co"]].phone, ho); sim.rng.random = real    # they talked
    run_until(sim, sim.now + 30)
    ho.say(f"CLOSE {c['r']['id']}"); run_until(sim, sim.now + 10)
    _restore(sim, snap, t0)
    pi = sim.stripe.intents[p["charge"]]
    return pi["refunded"] == 0 and not money_ok(sim), f"contractor talked to the homeowner before the CLOSE; refunded ${pi['refunded']:.0f} (should be $0)"


def t_restore_refund_and_dispute_on_older_charge(h):
    sim, c = setup_charging(h); app = sim.app
    for ev in list(c["held"]): app.on_stripe_webhook(ev)
    run_until(sim, sim.now + 5)
    sim.stripe._webhook = lambda typ, pid: sim.stripe.events.append(dict(id=f"evt_{len(sim.stripe.events)+1}", type=typ, pid=pid, created=sim.now))
    t0 = sim.now; snap = app.snapshot()
    p = app.picks[c["r"]["pick"]]
    sim.stripe.refund(p["charge"], 10, idem_key="manual-partial")             # Casey gives a $10 courtesy refund in Stripe
    sim.stripe.maybe_dispute(p["charge"])                                       # and the contractor disputes the rest
    run_until(sim, sim.now + 10)
    _restore(sim, snap, t0)
    disp = [l for l in app.ledger if l["type"] == "dispute"]
    probs = money_ok(sim)
    return bool(disp) and not probs, f"dispute booked: {bool(disp)}; refund books match Stripe: {not probs} {probs or ''}"


def t_restore_quotes_and_links(h):
    sim = make(h); app = sim.app; ho = Scripted(sim, 1)
    run_until(sim, DAY + 9 * 60); ho.say("Toilet keeps running 86403"); run_until(sim, sim.now + 5); ho.say("OK"); run_until(sim, sim.now + 5)
    if ho.inbox and ho.inbox[-1]["kind"] == "intake_q" and "Reply" in ho.inbox[-1]["body"]:
        ho.say("1"); run_until(sim, sim.now + 5)
    rid = app.homeowners[app.phone_to_ho[ho.phone]]["requests"][-1]
    t0 = sim.now; snap = app.snapshot()
    run_until(sim, sim.now + 6 * 60)                                            # quotes come in on the website; the link is texted
    links = [m for m in ho.inbox if m["kind"] == "quote_link"]
    n_before = len(app.requests[rid]["quotes"])
    _restore(sim, snap, t0)
    n_after = len(app.requests[rid]["quotes"])
    token = re.search(r"/q/(\S+)", links[-1]["body"]).group(1) if links else None
    works = bool(token) and app.resolve_token(token) == rid
    return n_after >= n_before > 0 and works, f"quotes before restore {n_before}, after {n_after}; the texted link {'still works' if works else 'is dead'}"


def t_widen_respects_license(h):
    """A job in a neighboring town is offered to nearby businesses, but only those whose license covers it."""
    sim = make(h); app = sim.app; ho = Scripted(sim, 1); r = to_quotes(sim, ho)
    co = next(c for c in app.contractors.values() if c["trade"] == "plumbing")
    co["towns"] = {"Bullhead City"}; co["neighbor_towns"] = ["Fort Mohave"]; co["license_class"] = "R-37R-drains"; co["checklist"] = {"drain_sewer", "water_heater"}
    r2 = dict(r, id="9999", town="Fort Mohave", trade="plumbing", job_types=["water_heater"], cards=[], status="OPEN", history=[(sim.now, "OPEN")])
    app.requests["9999"] = r2
    app.send_job_cards(r2, widen=True)
    got = any(app.cards[k]["co"] == co["id"] for k in r2["cards"])
    return not got, f"a drains-only license {'GOT' if got else 'did not get'} a water-heater job in the neighboring town"


TESTS = [("Neighboring-town jobs respect the license", t_widen_respects_license),
         ("Restore: payment notice landed in the lost window, Stripe down at restore time", t_restore_late_notice_stripe_down),
         ("Restore: free first win confirmed in the lost window", t_restore_free_win_in_window),
         ("Restore: contractor talked to the homeowner, then the homeowner closed the job, in the lost window", t_restore_call_then_close),
         ("Restore: partial refund and a dispute on an older charge in the lost window", t_restore_refund_and_dispute_on_older_charge),
         ("Restore: quotes and quote links from the lost window", t_restore_quotes_and_links),
         ("Collision: homeowner closes the job as Stripe confirms payment (both orders)", t_race_close_vs_payment),
         ("Collision: Confirm deadline vs a late Confirm (both orders)", t_race_deadline_vs_confirm),
         ("Collision: bank-approval deadline vs the approval (both orders)", t_race_auth_deadline_vs_approval),
         ("Collision: refund decision vs a connected call (both orders)", t_race_refund_decision_vs_call),
         ("Collision: Stripe's notice arrives before the app saves the charge", t_race_notice_before_save),
         ("Database restored to before a paid introduction", t_database_restore),
         ("Cancelled during a Stripe outage, contractor charged anyway", t_cancel_during_stripe_outage),
         ("Homeowner texts CANCEL: request should close", t_bare_cancel_closes_request),
         ("'I smell gas' at the consent step during an AI outage", t_gas_at_consent_during_ai_outage),
         ("Homeowner answers the consent prompt with 'Yes'", t_yes_at_consent),
         ("Stripe's payment notice is lost", t_lost_stripe_webhook),
         ("Same business, two accounts: one free first win", t_one_free_win_per_business),
         ("Handyman quote over $1,000", t_handyman_cap)]

if __name__ == "__main__":
    out = []
    for name, fn in TESTS:
        row = {"test": name}
        for label, h in (("v9.1 as built", False), ("v9.3", True)):
            ok, detail = fn(h)
            row[label] = ("PASS" if ok else "FAIL") + " - " + detail
        out.append(row)
        print(f"\n{name}\n   v9.1:  {row['v9.1 as built']}\n   v9.3:  {row["v9.3"]}")
    os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
    json.dump(out, open(os.path.join(HERE, "results", "review_tests.json"), "w"), indent=1)
