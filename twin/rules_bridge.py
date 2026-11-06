"""The twin's link to the app's own rules: the `rules_bridge` binary (D-453, D-481, D-491, T074).

`RulesBridge` runs `app/server/src/bin/rules_bridge.rs` as one long-lived process and speaks JSON lines
`{id, call, args}` to it (`app/server/rules_bridge.schema.json`). `active()` gives the one bridge of this
process when `ASKHAVA_RULES_BRIDGE` names the binary (twin/gate.py --bridge sets it), else None and the twin
keeps its own Python rules (the tests in tests/twin, which break those rules on purpose, run that way).

The adapters below put the twin's names into the app's: a quote into a fee request (FEE-001, REC-002), a
simulated minute into an Arizona instant (QH-001), the twin's request statuses into the app's job statuses
(JOB-002) and a business and its candidate job types into catalog terms (CARD-001). The catalog names come
from `twin_config.json`'s `catalog` section (D-541), the same table the license oracle is generated from.
"""
import atexit
import datetime as dt
import json
import os
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
# Sim minute 0 is midnight Arizona time on a winter Monday (MST, UTC-7, no daylight saving anywhere for 928).
EPOCH = dt.datetime(2026, 1, 5, 7, 0, tzinfo=dt.timezone.utc)
# Every simulated number is a 928 number, so one fake 928 number stands for the area code's zones (QH-003).
QH_PHONE = "+19285550100"
# The twin's request statuses as the app's job statuses (JOB-001). AWAITING_YES is the twin's own step inside
# quotes_ready (a pick waiting for the YES text); the twin's four intake endings are the app's closed_early.
STATUS = {"INTAKE": "intake", "OPEN": "open", "NO_QUOTES_YET": "no_quotes_yet", "QUOTES_READY": "quotes_ready",
          "AWAITING_YES": "quotes_ready", "AWAITING_CONFIRM": "awaiting_confirm", "PAYMENT_PENDING": "payment_pending",
          "CONNECTED": "connected", "DONE": "done", "CLOSED": "closed", "CLOSED_NONE": "closed",
          "CLOSED_NO_QUOTES": "closed", "CANCELLED": "cancelled", "NOT_COVERED": "not_covered",
          "EMERGENCY": "closed_early", "INELIGIBLE": "closed_early", "DUPLICATE": "closed_early",
          "ABANDONED": "closed_early"}
HANDYMAN_CLASS = "HANDYMAN"   # the twin's no-license handyman: on the city list, no ROC class (LIC-011, LIC-014)


class BridgeError(RuntimeError):
    """The bridge answered an error code, or did not answer."""


class RulesBridge:
    """One `rules_bridge` process; `call` writes one request line and reads one answer line."""

    def __init__(self, path):
        if not os.path.isfile(path):
            raise BridgeError(f"no rules_bridge binary at {path} (cargo build -p askhava-server --bin rules_bridge)")
        self.proc = subprocess.Popen([path], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
        self.n = 0
        self.cache = {}

    def call(self, call, **args):
        self.n += 1
        self.proc.stdin.write(json.dumps({"id": self.n, "call": call, "args": args}) + "\n")
        self.proc.stdin.flush()
        line = self.proc.stdout.readline()
        if not line:
            raise BridgeError(f"rules_bridge closed during {call}")
        answer = json.loads(line)
        if "ok" in answer and answer["ok"]["id"] == self.n:
            return answer["ok"]["result"]
        raise BridgeError(f"{call}: {answer.get('error', {}).get('code', answer)}")

    def close(self):
        if self.proc.poll() is None:
            self.proc.stdin.close()
            self.proc.wait(timeout=10)
        return self.proc.returncode


_ACTIVE = {}


def active():
    """The process's bridge when ASKHAVA_RULES_BRIDGE is set, else None (the twin's own rules)."""
    path = os.environ.get("ASKHAVA_RULES_BRIDGE")
    if not path:
        return None
    if path not in _ACTIVE:
        _ACTIVE[path] = RulesBridge(path)
        atexit.register(_ACTIVE[path].close)
    return _ACTIVE[path]


def fee(bridge, q):
    """FEE-001, FEE-003, FEE-004, REC-002: the app's introduction fee by job size, in dollars, for a twin quote.
    A monthly quote is a one-time job type in a recurring category: 4 months, never capped (REC-010, D-265)."""
    low = round(100 * (q.get("amount") or 0))
    high = round(100 * q["high"]) if q.get("type") == "range" and q.get("high") else low
    form = "visit" if q.get("type") == "trip" else ("range" if high != low else "fixed")
    monthly = q.get("period") == "month"
    got = bridge.call("fee", format=form, amount=low, high=high, period="monthly" if monthly else "none",
                      service="one_time" if monthly else "not_recurring")
    return got["cents"] // 100


def at(minute):
    """A simulated minute as an RFC 3339 instant."""
    return (EPOCH + dt.timedelta(minutes=minute)).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def quiet(bridge, minute):
    """QH-001, QH-006: (may a non-urgent text go at this minute, the minute the window next opens)."""
    got = bridge.call("quiet_hours", phone=QH_PHONE, at=at(minute))
    if got["send"]:
        return True, minute
    until = dt.datetime.fromisoformat(got["until"].replace("Z", "+00:00"))
    return False, (until - EPOCH).total_seconds() / 60


def move_allowed(bridge, old, new):
    """JOB-002: the app's arrows allow the twin's move; a move inside one app status is the twin's own."""
    a, b = STATUS[old], STATUS[new]
    return a == b or bridge.call("transition", **{"from": a, "to": b})["allowed"]


def _catalog():
    with open(os.path.join(HERE, "twin_config.json"), encoding="utf-8") as f:
        return json.load(f)["catalog"]


def _pairs(bridge, twin_types):
    """The catalog (category, job type) pairs of twin job types; '*' is every listed job type of the category."""
    names = bridge.cache.setdefault("catalog", _catalog())["job_types"]
    out = []
    for jt in twin_types:
        for cat, kind in names.get(jt, []):
            key = ("job_types", cat)
            if kind == "*" and key not in bridge.cache:
                bridge.cache[key] = bridge.call("job_types", category=cat)["job_types"]
            kinds = bridge.cache[key] if kind == "*" else [kind]
            out += [{"category": cat, "job_type": k} for k in kinds if {"category": cat, "job_type": k} not in out]
    return out


def card(bridge, co, twin_types):
    """CARD-001, LIC-043: the app's card decision for an active business in the job's town (the twin checked
    status, town and the fresh license itself) with its license class and ticks, for these candidates. A tick is
    the catalog pairs of a ticked candidate: the twin's own names say what was ticked, since a '*' pair (every job
    type of a category) would otherwise tick the category's named job types too."""
    cls = co.get("license_class") or ""
    handyman = cls == HANDYMAN_CLASS
    classes = [] if handyman else bridge.cache.setdefault("catalog", _catalog())["license_classes"].get(cls, [])
    ticked = _pairs(bridge, [jt for jt in twin_types if jt in co["checklist"]])
    got = bridge.call("card", classes=classes, handyman=handyman, ticked=ticked,
                      candidates=_pairs(bridge, twin_types), text="")
    return got["card"]
