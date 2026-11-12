"""Route benchmark over seeded fake data: lab numbers, judged against D-3292's goals (T417, D-3319).

`python3 -m perf.bench [--quick] --out <file> [--baseline]` starts the test-control `server`, the
`worker` and the stand-ins as tests/e2e/webserver.ts does (plus j1_homeowner.spec.ts's fake keys)
on a database of its own on DATABASE_URL's server (created, migrated, seeded by perf/seed.py,
dropped at the end), then times each route in ROUTES at 1, 10 and 50 users (`--quick`: 1 and 10,
fewer requests). Queries per request come from `GET /__test/perf/db` (T416) at 1 user; acquire
waits from snapshot bucket differences. An unexposed figure is "not exposed: <reason>"; a missed
goal never fails the run. The full run adds `perf_zip_scale`. Secrets are random per run and never
printed or written; the output passes scripts/pii_scan.py's rules before it is written
(TD-OPS-42); only `--baseline` writes under the repository.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import hmac
import importlib.util
import json
import math
import os
import platform
import re
import secrets
import socket
import statistics
import subprocess
import tempfile
import threading
import time
import urllib.parse
from concurrent.futures import ThreadPoolExecutor
from http.client import HTTPConnection
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "docs/audit/perf-baseline.json"
PUBLIC_BASE = "https://perf.example.test"
TWILIO_SID = "AC" + "0" * 32
AREA_CODES = ("602", "480", "623", "520", "928")
PICK_AREA_CODES = ("212", "305", "312", "415", "503", "617", "702", "801", "919", "971")
TIMEOUT_S = 10.0
GOALS = {"zip_lookup": ("p95_ms", 100.0), "matching": ("p95_ms", 250.0)}
ERROR_GOAL = 0.001
NOT_ROUTED = "not exposed: no HTTP route on main"
ROUTES = {
    "home": {"probe": "/"},
    "zip_lookup": {"reason": NOT_ROUTED + " (catalog::Markets::town_for_zip runs in process; see perf_zip_scale)"},
    "intake_ack": {"kind": "intake"},
    "matching": {"reason": NOT_ROUTED + " (matching runs in the worker after intake; no results step is served)"},
    "quote_page": {"kind": "page"},
    "quote_poll": {"kind": "poll"},
    "pick_review": {"kind": "review"},
    "pick_final": {"kind": "pick"},
    "confirm": {"reason": NOT_ROUTED + " (the Confirm route is not served yet, D-1480)"},
    "business_dashboard": {"probe": "/b"},
}


def nearest_rank(values: list[float], pct: float) -> float:
    """The nearest-rank percentile: the value at rank ceil(pct/100 * n) of the sorted values."""
    ordered = sorted(values)
    return ordered[max(1, math.ceil(pct / 100 * len(ordered))) - 1]


def bucket_p95(before: dict, after: dict) -> int | str:
    """The p95 acquire wait from two `/__test/perf/db` snapshots: the upper bound (us) of the
    bucket holding the nearest-rank 95th acquire, ">5000000" above the last bound."""
    diffs = [(a["le_us"], a["count"] - b["count"]) for a, b in zip(after["buckets"], before["buckets"])]
    total = sum(n for _, n in diffs)
    if total == 0:
        return "not exposed: no pool acquire in this phase"
    rank, seen = math.ceil(0.95 * total), 0
    for bound, n in diffs:
        seen += n
        if seen >= rank:
            return bound if bound is not None else ">5000000"
    raise AssertionError("bucket counts do not add up")


def bucket_max(before: dict, after: dict) -> int | str:
    """The upper bound (us) of the highest bucket an acquire in the phase landed in."""
    hit = [a["le_us"] for a, b in zip(after["buckets"], before["buckets"]) if a["count"] > b["count"]]
    if not hit:
        return "not exposed: no pool acquire in this phase"
    return hit[-1] if hit[-1] is not None else ">5000000"


def is_error(status: int | None) -> bool:
    """A 5xx or a timeout (status None) is an error; an expected 4xx is not (D-3292)."""
    return status is None or status >= 500


def error_rate(statuses: list[int | None]) -> float:
    return sum(map(is_error, statuses)) / len(statuses)


def verdict(value: object, goal: float) -> str:
    """"met" at or under the goal, "missed" over it; never a failure (D-3292)."""
    if not isinstance(value, (int, float)):
        return "not measurable"
    return "met" if value <= goal else "missed"


def _pii_rules():
    spec = importlib.util.spec_from_file_location("pii_scan", ROOT / "scripts/pii_scan.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.scan_text


def dumps(record: dict) -> str:
    """JSON with one line per top-level key and one per route, so a baseline diff stays readable."""
    one = lambda v: json.dumps(v, sort_keys=True)
    routes = ",\n".join(f"  {one(r)}: {one(v)}" for r, v in sorted(record.get("routes", {}).items()))
    lines = [f" {one(k)}: {one(v)}" for k, v in sorted(record.items()) if k != "routes"]
    lines += [f' "routes": {{\n{routes}\n }}'] if "routes" in record else []
    return "{\n" + ",\n".join(lines) + "\n}\n"


def write_record(path: Path, record: dict) -> None:
    """Writes `record` as JSON only if scripts/pii_scan.py's rules find nothing in it (TD-OPS-42)."""
    text = dumps(record)
    found = _pii_rules()(str(path), text)
    if found:
        raise ValueError("refused: not fake data (" + ", ".join(sorted({r for _, _, r in found})) + ")")
    path.write_text(text, encoding="utf-8")


def summarize(route: str, samples: list[tuple[float, int | None, int]], extra: dict) -> dict:
    """One route and concurrency: percentiles, payload, error rate and the goal verdicts."""
    times = [ms for ms, _, _ in samples]
    entry = {f"p{p}_ms": round(nearest_rank(times, p), 2) for p in (50, 95, 99)}
    entry["requests"] = len(samples)
    entry["median_bytes"] = statistics.median(b for _, _, b in samples)
    entry["statuses"] = {str(s): sum(1 for _, x, _ in samples if x == s) for s in {x for _, x, _ in samples}}
    entry["error_rate"] = error_rate([s for _, s, _ in samples])
    entry.update(extra)
    entry["verdicts"] = {"error_rate_under_0.1%": verdict(entry["error_rate"], ERROR_GOAL)}
    if route in GOALS:
        figure, goal = GOALS[route]
        entry["verdicts"][f"{figure}_goal_{goal:g}"] = verdict(entry[figure], goal)
    return entry


class App:
    """The stand-ins, `server` and `worker` of one run, with their per-run secrets in memory."""

    def __init__(self) -> None:
        self.scratch = tempfile.mkdtemp(prefix="askhava-perf-")
        self.web, self.stand_in = _free_port(), _free_port()
        self.control = secrets.token_hex(24)
        self.twilio_token = secrets.token_hex(20)
        self.children: list[subprocess.Popen] = []

    def env(self) -> dict[str, str]:
        db = os.environ["DATABASE_URL"]
        twilio = f"http://127.0.0.1:{self.stand_in}/twilio"
        key = lambda: "1:" + base64.b64encode(secrets.token_bytes(32)).decode()
        env = {k: v for k, v in os.environ.items() if not k.startswith("ASKHAVA_")}
        env.update(PORT=str(self.web), ASKHAVA_APP_ENV="ci", ASKHAVA_DATABASE_URL=db,
                   ASKHAVA_MIGRATION_DATABASE_URL=db, ASKHAVA_PUBLIC_BASE_URL=PUBLIC_BASE,
                   ASKHAVA_ADAPTER_MODE_TWILIO="stand_in", ASKHAVA_TWILIO_BASE_URL=twilio,
                   ASKHAVA_TWILIO_VERIFY_BASE_URL=twilio, ASKHAVA_TWILIO_ACCOUNT_SID=TWILIO_SID,
                   ASKHAVA_TWILIO_AUTH_TOKEN=self.twilio_token,
                   ASKHAVA_TWILIO_VERIFY_SERVICE_SID="VA" + "e" * 32, ASKHAVA_MASKED_POOL_SIZE="1",
                   ASKHAVA_MASKED_NUMBERS="+15005550100", ASKHAVA_ID_SECRET=secrets.token_hex(24),
                   ASKHAVA_STORAGE_BASE_URL="file://" + os.path.join(self.scratch, "storage"),
                   ASKHAVA_TEST_CONTROL_SECRET=self.control,
                   ASKHAVA_TEST_CONTROL_CLOCK_FILE=os.path.join(self.scratch, "clock"),
                   VAULT_KEY=key(), VAULT_INDEX_KEY=key())
        return env

    def start(self) -> None:
        env = self.env()
        build = ["cargo", "build", "-q", "-p", "askhava-server", "--bins", "--features", "askhava-core/test-control"]
        _run(build, {**env, "SQLX_OFFLINE": "true"})
        _run([str(ROOT / "target/debug/server"), "migrate"], env)
        for command in (["python3", "-m", "stand_ins.server", "--port", str(self.stand_in)],
                        [str(ROOT / "target/debug/worker")], [str(ROOT / "target/debug/server")]):
            self.children.append(subprocess.Popen(command, cwd=ROOT, env=env, stdout=subprocess.DEVNULL,
                                                  stderr=subprocess.DEVNULL))
        deadline = time.monotonic() + 60
        while not all(_listening(p) for p in (self.web, self.stand_in)):
            if time.monotonic() > deadline or any(c.poll() is not None for c in self.children):
                raise RuntimeError("the stand-ins, worker or web binary did not start")
            time.sleep(0.2)

    def stop(self) -> None:
        for child in self.children:
            child.terminate()
        for child in self.children:
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()

    def request(self, method: str, path: str, headers: dict | None = None, body: str = "") -> tuple[float, int | None, bytes, list]:
        """One request on a fresh connection: wall ms, status (None on timeout), body, Set-Cookie headers."""
        start = time.perf_counter()
        conn = HTTPConnection("127.0.0.1", self.web, timeout=TIMEOUT_S)
        try:
            conn.request(method, path, body=body or None, headers=headers or {})
            reply = conn.getresponse()
            data = reply.read()
            return (time.perf_counter() - start) * 1000, reply.status, data, reply.headers.get_all("set-cookie") or []
        except (TimeoutError, OSError):
            return (time.perf_counter() - start) * 1000, None, b"", []
        finally:
            conn.close()

    def db(self) -> dict:
        _, status, data, _ = self.request("GET", "/__test/perf/db", {"authorization": f"Bearer {self.control}"})
        if status != 200:
            raise RuntimeError(f"GET /__test/perf/db answered {status}")
        return json.loads(data)


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _listening(port: int) -> bool:
    with socket.socket() as s:
        return s.connect_ex(("127.0.0.1", port)) == 0


def _run(command: list[str], env: dict[str, str]) -> None:
    done = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True, check=False)
    if done.returncode != 0:
        raise RuntimeError(f"{command[0]} {command[1]} failed with status {done.returncode}")


class Requests:
    """Builds the n-th request of each measured route over the seeded objects."""

    def __init__(self, app: App, pages: list[str]) -> None:
        self.app, self.pages, self.picks, self.count, self.picked = app, pages, [], 0, 0
        self.lock = threading.Lock()
        self.cookie, self.csrf = self._pre_session(pages[0])

    def _pre_session(self, token: str) -> tuple[str, str]:
        _, _, body, cookies = self.app.request("GET", f"/q/{token}")
        found = re.search(rb'name="csrf" value="([^"]+)"', body)
        return "; ".join(c.split(";", 1)[0] for c in cookies), found.group(1).decode() if found else ""

    def _form(self, fields: dict) -> tuple[dict, str]:
        origin = {"origin": f"https://127.0.0.1:{self.app.web}", "sec-fetch-site": "same-origin"}
        headers = {**origin, "cookie": self.cookie, "content-type": "application/x-www-form-urlencoded"}
        return headers, urllib.parse.urlencode({"csrf": self.csrf, **fields})

    def homeowners(self, count: int) -> list[str]:
        """One homeowner request per pick, opened by a signed text from its own fake number, as
        J1 step 1 does; returns the conversation IDs (the phone stays in the app's vault)."""
        from perf.seed import psql
        opened = []
        for n in range(count):
            if self.app.request(*self._intake(n, PICK_AREA_CODES))[1] != 200:
                raise RuntimeError("an inbound text for a pick's homeowner was refused")
            opened.append(psql("SELECT id FROM intake.conversation ORDER BY opened_at DESC LIMIT 1")[0][0])
        return opened

    def _intake(self, n: int, areas: tuple[str, ...] = AREA_CODES) -> tuple[str, str, dict, str]:
        path = "/webhooks/twilio/sms"
        phone = f"+1{areas[n // 100 % len(areas)]}55501{n % 100:02d}"
        form = {"AccountSid": TWILIO_SID, "Body": "AC blowing warm air", "From": phone,
                "MessageSid": "SM" + secrets.token_hex(16), "NumMedia": "0", "To": "+15005550006"}
        signed = PUBLIC_BASE + path + "".join(k + form[k] for k in sorted(form))
        mac = hmac.new(self.app.twilio_token.encode(), signed.encode(), hashlib.sha1).digest()
        headers = {"content-type": "application/x-www-form-urlencoded", "x-twilio-signature": base64.b64encode(mac).decode()}
        return "POST", path, headers, urllib.parse.urlencode(form)

    def build(self, kind: str) -> tuple[str, str, dict, str]:
        with self.lock:
            n, self.count = self.count, self.count + 1
        page = self.pages[n % len(self.pages)]
        if kind.startswith("get:"):
            return "GET", kind[4:], {}, ""
        if kind == "intake":
            return self._intake(n)
        if kind == "page":
            return "GET", f"/q/{page}", {}, ""
        if kind == "poll":
            return "GET", f"/q/{page}", {"accept": "text/html", "cache-control": "no-store"}, ""
        if kind == "review":
            return ("POST", f"/q/{page}", *self._form({"quote": "1"}))
        with self.lock:
            n, self.picked = self.picked, self.picked + 1
        return ("POST", f"/q/{self.picks[n]}/pick", *self._form({"quote": "1", "share": "yes", "backup": ""}))


def _phase(app: App, requests: Requests, kind: str, users: int, count: int) -> dict:
    before = app.db()
    queries: list[int] = []

    def one(_: int) -> tuple[float, int | None, int]:
        method, path, headers, body = requests.build(kind)
        start = app.db() if users == 1 else None
        ms, status, data, _ = app.request(method, path, headers, body)
        if start is not None:
            queries.append(app.db()["statements"] - start["statements"])
        return ms, status, len(data)

    with ThreadPoolExecutor(max_workers=users) as pool:
        samples = list(pool.map(one, range(count)))
    after = app.db()
    extra = {"users": users, "acquire_wait_p95_us": bucket_p95(before, after),
             "acquire_wait_max_us_bucket": bucket_max(before, after)}
    extra["mean_queries"] = round(statistics.mean(queries), 2) if queries else (
        "not exposed: with several requests in flight a statement difference is not one request's")
    return extra | {"samples": samples}


def _measure(app: App, requests: Requests, plan: dict) -> dict:
    results: dict = {}
    for route, spec in ROUTES.items():
        if "reason" in spec or "probe" in spec and app.request("GET", spec["probe"])[1] == 404:
            results[route] = spec.get("reason") or f"{NOT_ROUTED} (GET {spec['probe']} answers 404)"
            continue
        kind = spec.get("kind") or "get:" + spec["probe"]
        for _ in range(plan["warmup"]):
            if kind != "pick":
                app.request(*requests.build(kind))
        results[route] = {}
        for users in plan["users"]:
            extra = _phase(app, requests, kind, users, plan["count"])
            results[route][str(users)] = summarize(route, extra.pop("samples"), extra)
    return results


def zip_scale() -> dict | str:
    """T416's `perf_zip_scale` at national size: its printed JSON line."""
    command = ["cargo", "test", "-q", "-p", "askhava-integration", "--test", "perf_zip_scale", "--", "--ignored", "--nocapture"]
    done = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, check=False, env={**os.environ, "SQLX_OFFLINE": "true"})
    for line in done.stdout.splitlines():
        if line.startswith("{") and '"zip_lookup"' in line:
            return json.loads(line)
    return f"not exposed: perf_zip_scale exited {done.returncode} without its line"


def _machine(seeded: dict) -> dict:
    sha = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=False)
    from perf.seed import psql
    return {"git_sha": sha.stdout.strip(), "python": platform.python_version(), "machine": platform.machine(),
            "system": platform.system(), "cpus": os.cpu_count(), "build": "debug, test-control",
            "postgres": psql("SHOW server_version")[0][0], "data": seeded}


def _scratch_database(base: str, action: str, name: str) -> str:
    """Creates or drops the run's own database on DATABASE_URL's server; returns its URL."""
    sql = f"CREATE DATABASE {name}" if action == "create" else f"DROP DATABASE IF EXISTS {name} WITH (FORCE)"
    done = subprocess.run(["psql", base, "-qc", sql], capture_output=True, text=True, check=False)
    if done.returncode != 0:
        raise RuntimeError(f"{action} of the run's database failed with status {done.returncode}")
    return urllib.parse.urlunsplit(urllib.parse.urlsplit(base)._replace(path="/" + name))


def run(quick: bool) -> dict:
    from perf import seed
    plan = {"users": [1, 10] if quick else [1, 10, 50], "count": 40 if quick else 200, "warmup": 5 if quick else 20}
    base, name = os.environ["DATABASE_URL"], "askhava_perf_" + secrets.token_hex(4)
    os.environ["DATABASE_URL"] = _scratch_database(base, "create", name)
    app = App()
    try:
        app.start()
        seeded = seed.seed_background()
        pick_count = len(plan["users"]) * plan["count"]
        requests = Requests(app, seed.quote_pages(20))
        requests.picks = seed.quote_pages(pick_count, requests.homeowners(pick_count))
        record = {"mode": "quick" if quick else "full", "plan": plan, "routes": _measure(app, requests, plan)}
        record["environment"] = _machine(seeded)
    finally:
        app.stop()
        _scratch_database(base, "drop", name)
        os.environ["DATABASE_URL"] = base
    record["perf_zip_scale"] = "not run in --quick" if quick else zip_scale()
    return record


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--quick", action="store_true", help="1 and 10 users, fewer requests, no perf_zip_scale")
    parser.add_argument("--out", type=Path, required=True, help="the JSON file to write")
    parser.add_argument("--baseline", action="store_true", help="also write docs/audit/perf-baseline.json")
    args = parser.parse_args(argv)
    record = run(args.quick)
    write_record(args.out, record)
    if args.baseline:
        write_record(BASELINE, record)
    print(f"perf: wrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
