"""Event engine, clocks and text helpers (standard library only). All results simulated."""
import heapq, math, random, re

DAY = 1440
HOUR = 60
BH_START, BH_END = 8 * 60, 21 * 60   # G business clock 8 a.m.-9 p.m.

def day(t): return int(t // DAY)
def hod(t): return (t % DAY) / 60.0
def dow(t): return day(t) % 7         # 0 = Monday (relative, not a calendar date)

def in_quiet(t, offset_h=0.0):
    h = ((t + offset_h * 60) % DAY) / 60.0
    return h >= 21 or h < 8

def next_open(t, start=8 * 60):
    d = day(t); tod = t - d * DAY
    if tod < start: return d * DAY + start
    return (d + 1) * DAY + start

def add_bmin(t, m, weekdays_only=False):
    """Advance t by m business minutes on the 8 a.m.-9 p.m. clock."""
    while m > 1e-9:
        d = day(t); tod = t - d * DAY
        if weekdays_only and d % 7 >= 5:
            t = (d + 1) * DAY + BH_START; continue
        if tod < BH_START: t = d * DAY + BH_START; continue
        if tod >= BH_END: t = (d + 1) * DAY + BH_START; continue
        avail = d * DAY + BH_END - t
        if m <= avail: return t + m
        m -= avail; t = (d + 1) * DAY + BH_START
    return t

def add_clock_hours(t, h, weekdays_only=False):
    if not weekdays_only: return t + h * 60
    m = h * 60
    while m > 1e-9:
        d = day(t)
        if d % 7 >= 5: t = (d + 1) * DAY; continue
        avail = (d + 1) * DAY - t
        if m <= avail: return t + m
        m -= avail; t = (d + 1) * DAY
    return t

def fmt_t(t):
    d = day(t); h = hod(t)
    return f"day {d} {int(h):02d}:{int(round((h % 1) * 60)) % 60:02d}"

class Engine:
    def __init__(self, seed, collision=None):
        self.q = []; self.seq = 0; self.t = 0.0
        self.collision = collision
        self.tie_rng = random.Random(seed * 1000003 + 17)
    def at(self, t, fn, *args, pr=5):
        if t < self.t: t = self.t
        if self.collision:
            t = float(math.ceil(t - 1e-9))
            r = self.tie_rng.random()
            tie = r if self.collision == "fwd" else -r
        else:
            tie = 0.0
        self.seq += 1
        heapq.heappush(self.q, (t, pr, tie, self.seq, fn, args))
    def run(self, until):
        q = self.q
        while q:
            t, pr, tie, s, fn, args = q[0]
            if t > until: break
            heapq.heappop(q)
            self.t = t
            fn(*args)
        self.t = until

# ---- SMS segment counting (GSM-7 vs UCS-2) ----
GSM = set("@£$¥èéùìòÇ\nØø\rÅåΔ_ΦΓΛΩΠΨΣΘΞÆæßÉ !\"#¤%&'()*+,-./0123456789:;<=>?¡ABCDEFGHIJKLMNOPQRSTUVWXYZÄÖÑÜ§¿abcdefghijklmnopqrstuvwxyzäöñüà^{}\\[~]|€")
def segments(body):
    if all(c in GSM for c in body):
        n = len(body) + sum(1 for c in body if c in "^{}\\[~]|€")
        return 1 if n <= 160 else math.ceil(n / 153)
    n = len(body)
    return 1 if n <= 70 else math.ceil(n / 67)

DIGITS = re.compile(r"\d")
NUMWORDS = {"zero","one","two","three","four","five","six","seven","eight","nine","oh","cero","uno","dos","tres","cuatro","cinco","seis","siete","ocho","nueve"}
CONTACT_PHRASES = ["call me", "text me", "my number", "reach me", "my name is", "this is ", "i'm ", "i am ", "email", "llámame", "mi número", "me llamo", "soy "]
NAMESHAPE = re.compile(r"(?<![.!?]\s)(?<!^)\b[A-Z][a-z]+\s+[A-Z][a-z]+\b")

def code_last_pass(text):
    """Part 4.3 step 8: plain-code last pass. Anything shaped like a name, a contact phrase,
    a run of number words, or 7+ digits removes the rest of that sentence."""
    out = []
    for sent in re.split(r"(?<=[.!?])\s+", text):
        low = sent.lower(); cut = None
        for p in CONTACT_PHRASES:
            i = low.find(p)
            if i >= 0: cut = i if cut is None else min(cut, i)
        m = NAMESHAPE.search(sent)
        if m: cut = m.start() if cut is None else min(cut, m.start())
        toks = re.findall(r"[a-záéíóúñ]+|\d", low)
        run = 0; pos = 0
        for w in re.finditer(r"[a-záéíóúñ]+", low):
            if w.group() in NUMWORDS:
                run += 1
                if run >= 3:
                    cut = w.start() if cut is None else min(cut, w.start()); break
            else: run = 0
        if len(DIGITS.findall(sent)) >= 7:
            # cut at the first digit
            i = DIGITS.search(sent).start()
            cut = i if cut is None else min(cut, i)
        out.append(sent if cut is None else sent[:cut].rstrip())
    return " ".join(s for s in out if s)

STREET = re.compile(r"\b\d{2,6}\s+[A-Za-z]+(\s+[A-Za-z]+)?\s+(st|street|ave|avenue|dr|drive|rd|road|ln|lane|blvd|way|ct|court|pl|place|cir|circle|calle)\b", re.I)
PHONE_ANY = re.compile(r"(\d[\s\-.()]*){7,}")
def strong_scrub(text, vault_items):
    """Proposed fix: deterministic scrub BEFORE any AI call, using the contact vault."""
    t = PHONE_ANY.sub("[removed]", text)
    t = STREET.sub("[removed]", t)
    for v in vault_items:
        if v and len(v) >= 3:
            t = re.sub(re.escape(v), "[removed]", t, flags=re.I)
    return code_last_pass(t)
