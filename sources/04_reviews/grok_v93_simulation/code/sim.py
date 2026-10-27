"""Wiring: one simulated run of the whole business (all results simulated)."""
import random
from engine import Engine, DAY, HOUR
from vendors import Costs, Twilio, Stripe, Claude, ROC, Email, Mail, Render
from config import make, CATS
from world import World
from app_biz import App

class Sim:
    def __init__(self, cfg, seed):
        self.cfg = cfg; self.seed = seed; self.CATS = CATS
        ch = cfg.get("chaos", 1.0)
        if ch != 1.0:
            for k in ("fail", "delay", "dup_status", "lost_status", "carrier"): cfg["tw"][k] *= ch
            for k in ("decline", "tds", "wh_lost", "wh_late", "timeout", "refund_fail"): cfg["stripe"][k] *= ch
            cfg["stripe"]["random_outage_per_60d"] *= ch
            for k in ("cat_err", "type_err", "quote_err"): cfg["ai"][k] *= ch
            for k in ("down", "captcha", "layout"): cfg["roc"][k] *= ch
            cfg["email_bounce"] *= ch; cfg["letter_lost"] *= ch
        self.eng = Engine(seed, cfg.get("collision"))
        self.costs = Costs(); self.web_log = []; self.claude_cache = {}
        self.tw = Twilio(self); self.stripe = Stripe(self); self.claude = Claude(self); self.roc = ROC(self)
        self.email = Email(self); self.mail = Mail(self); self.render = Render(self)
        self.app = App(self)
        self.world = World(self)
        self.world.setup()
        self.app.start()
        self.restores = []
        r = cfg.get("restore")
        if r:
            t1 = r["day"] * DAY + 14 * HOUR; t0 = t1 - r["lost_h"] * HOUR
            self.eng.at(t0, self._backup, pr=0); self.eng.at(t1, self._restore, r.get("mode", "spec"), pr=0)
    def _backup(self): self._snap = self.app.snapshot()
    def _restore(self, mode):
        self.restores.append({"t": self.eng.t, "from": self._snap["t"], "mode": mode,
                              "next_job_before": self.app.db["seq"]["job"]})
        self.app.restore(self._snap, mode)
        self.restores[-1]["next_job_after"] = self.app.db["seq"]["job"]
    def do_web(self, kind, args):
        a = self.app
        f = {"verify": a.web_verify_code, "quote": a.web_quote, "pick": a.web_pick, "pick_code": a.web_pick_code,
             "confirm": a.biz_confirm, "card": a.web_card_added, "3ds": a.web_3ds_done, "retry": a.web_retry_payment,
             "signup": a.biz_signup}[kind]
        return f(*args)
    def replay_web(self, w):
        t, kind, args = w
        return self.do_web(kind, args)
    def run(self):
        self.eng.run(self.cfg["days"] * DAY)
        return self

def run(over, seed):
    return Sim(make(over), seed).run()
