"""Seeded fake-data background and route objects for the route benchmark (T417, D-3292, D-453).

`seed_background` writes, in one transaction through psql on DATABASE_URL (after `server
migrate`), a deterministic Phoenix-metro-size background: BUSINESSES active businesses spread
over the catalog's towns (docs/catalog/markets.yaml) and launch-open trades
(docs/catalog/categories.yaml), JOBS past jobs with their numbers and facts, QUOTES quotes and
one paid page pick per Done job. IDs are md5 of a fixed label, times are fixed, so two runs
write the same rows; a database that already holds them is left as it is. Rows follow the
migrations on `main`; the tables in SKIPPED are not seeded and the report names them.

`quote_pages` makes the objects the measured routes need the way tests/e2e/pick_flow.spec.ts
does: a job in Quotes ready with three quotes and a quote-page link token (for the final Pick
also linked to a homeowner request, as j1_homeowner.spec.ts does), whose raw token is
kept in memory only. Fake data only (BUILD_BRIEF.md:66, PRIV-019): no name, phone, email or
street is written at all. The database URL is never printed.
"""

from __future__ import annotations

import hashlib
import os
import re
import secrets
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUSINESSES = 1500
JOBS = 20000
QUOTES = 50000
SKIPPED = {
    "intake.* and privacy.* (vault rows)": "sealed with the run's vault keys; SQL cannot seal them faithfully",
    "payments.*": "written only by the Stripe stand-in flows; a hand-made ledger row would be a guess",
    "jobs.status_history, ops.audit_log": "written by each status change, not by a background insert",
    "quotes matching and card tables": "written by the worker's matching; not reproduced by SQL",
    "businesses.business_name, team_member, service_area": "hold names or contact rows the routes measured do not read",
}


def _catalog() -> tuple[list[str], list[tuple[str, list[str]]]]:
    """The catalog's town codes and launch-open trades with their job types."""
    markets = (ROOT / "docs/catalog/markets.yaml").read_text(encoding="utf-8")
    towns = re.findall(r"^\s*- code: ([a-z][a-z0-9_]*)\s*$", markets, re.MULTILINE)
    text = (ROOT / "docs/catalog/categories.yaml").read_text(encoding="utf-8")
    trades = []
    for block in re.split(r"\n  - id: ", text)[1:]:
        types = re.search(r"^    job_types: \[(.*)\]$", block, re.MULTILINE)
        if "\n    launch_open: true" in block and types:
            trades.append((block.split("\n", 1)[0].strip(), [t.strip() for t in types.group(1).split(",")]))
    return towns, trades


def psql(sql: str) -> list[list[str]]:
    """Runs `sql` in one transaction on DATABASE_URL; rows as lists of columns."""
    url = os.environ.get("DATABASE_URL", "")
    if not url:
        raise SystemExit("DATABASE_URL must name the CI Postgres")
    done = subprocess.run(
        ["psql", url, "-1", "-v", "ON_ERROR_STOP=1", "-AtqF", "\t", "-f", "-"],
        input=sql, capture_output=True, text=True, check=False,
    )
    if done.returncode != 0:
        raise RuntimeError(f"psql failed with status {done.returncode}: {done.stderr.strip()[:400]}")
    return [line.split("\t") for line in done.stdout.splitlines() if line]


def _quote(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def _business_sql(towns: list[str], trades: list[tuple[str, list[str]]]) -> str:
    town_list = "ARRAY[" + ",".join(map(_quote, towns)) + "]"
    trade_rows = ",".join(f"({i},{_quote(c)},{_quote(t)})" for i, (c, ts) in enumerate(trades) for t in ts)
    return f"""
CREATE TEMP TABLE perf_trade (n int, category text, job_type text) ON COMMIT DROP;
INSERT INTO perf_trade VALUES {trade_rows};
INSERT INTO businesses.business (id, status, created_at, status_changed_at)
  SELECT md5('perf-business-' || g)::uuid, 'active', timestamptz '2025-06-01' + g * interval '1 hour',
    timestamptz '2025-06-01' + g * interval '1 hour' FROM generate_series(1, {BUSINESSES}) g;
INSERT INTO businesses.town (business_id, town)
  SELECT md5('perf-business-' || g)::uuid, ({town_list})[1 + g % {len(towns)}] FROM generate_series(1, {BUSINESSES}) g;
INSERT INTO businesses.job_type_choice (business_id, category, job_type)
  SELECT md5('perf-business-' || g)::uuid, t.category, t.job_type
  FROM generate_series(1, {BUSINESSES}) g JOIN perf_trade t ON t.n = g % {len(trades)};
"""


def _job_sql(trades: list[tuple[str, list[str]]]) -> str:
    status = "CASE WHEN g % 20 < 11 THEN 'done' WHEN g % 20 < 17 THEN 'closed' ELSE 'closed_early' END"
    at = "timestamptz '2026-01-01' + g * interval '20 minutes'"
    return f"""
INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
  SELECT md5('perf-job-' || g)::uuid, {status}, g % 50 = 0, {at}, {at}, {at} FROM generate_series(1, {JOBS}) g;
INSERT INTO jobs.job_number (number, job_id, issued_at)
  SELECT n.next + g - 1, md5('perf-job-' || g)::uuid, {at} FROM jobs.job_number_next n, generate_series(1, {JOBS}) g;
UPDATE jobs.job_number_next SET next = next + {JOBS};
INSERT INTO jobs.job_facts (job_id, seq, category, job_types, set_at)
  SELECT md5('perf-job-' || g)::uuid, 1, t.category, ARRAY[t.job_type], {at}
  FROM generate_series(1, {JOBS}) g JOIN perf_trade t ON t.n = g % {len(trades)}
  AND t.job_type = (SELECT min(job_type) FROM perf_trade u WHERE u.n = t.n);
"""


def _quote_sql() -> str:
    job = f"1 + (q - 1) % {JOBS}"
    business = f"1 + (q * 7919) % {BUSINESSES}"
    return f"""
INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, notes, original,
    ai_read, confirmed, received_at)
  SELECT md5('perf-job-' || ({job}))::uuid, md5('perf-business-' || ({business}))::uuid, 'firm',
    5000 + (q * 37) % 200000, false, '', 'Sample wording ' || q, false, true,
    timestamptz '2026-01-01' + ({job}) * interval '20 minutes' + (1 + (q - 1) / {JOBS}) * interval '1 hour'
  FROM generate_series(1, {QUOTES}) q;
INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at)
  SELECT md5('perf-job-' || g)::uuid, md5('perf-business-' || (1 + (g * 7919) % {BUSINESSES}))::uuid, 1, 'page',
    'paid', timestamptz '2026-01-01' + g * interval '20 minutes' + interval '2 hours'
  FROM generate_series(1, {JOBS}) g WHERE g % 20 < 11;
"""


def seed_background() -> dict[str, object]:
    """Seeds the background once; returns the counts the report prints."""
    if psql("SELECT count(*) FROM businesses.business WHERE id = md5('perf-business-1')::uuid")[0][0] == "0":
        towns, trades = _catalog()
        psql(_business_sql(towns, trades) + _job_sql(trades) + _quote_sql())
    counts = psql(
        "SELECT (SELECT count(*) FROM businesses.business), (SELECT count(*) FROM jobs.job),"
        " (SELECT count(*) FROM quotes.quote), (SELECT count(*) FROM picks.pick)"
    )[0]
    names = ("businesses", "jobs", "quotes", "picks")
    return {
        "constants": {"businesses": BUSINESSES, "jobs": JOBS, "quotes": QUOTES},
        "rows_in_database": dict(zip(names, map(int, counts))),
        "skipped_tables": SKIPPED,
    }


def _homeowner_link(conversation: str | None) -> str:
    """The CTEs that make the job hers, as tests/e2e/j1_homeowner.spec.ts's seedQuotes does."""
    if conversation is None:
        return ""
    return f"""f AS (INSERT INTO jobs.job_facts (job_id, seq, category, job_types, set_at)
      SELECT id, 1, 'hvac_repair', ARRAY['AC not cooling'], now() FROM j),
    a AS (INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at)
      SELECT '{conversation}', 'job.hvac.home', replace(j.id::text, '-', ''), now() FROM j),
    """


def quote_pages(count: int, conversations: list[str] | None = None) -> list[str]:
    """`count` jobs in Quotes ready with three quotes each, the i-th linked to `conversations[i]`
    when given (so the final Pick reaches the code send); returns their raw link tokens."""
    tokens = [secrets.token_hex(32) for _ in range(count)]
    statements = []
    for i, token in enumerate(tokens):
        digest = hashlib.sha256(token.encode()).hexdigest()
        link = _homeowner_link(conversations[i] if conversations else None)
        statements.append(f"""WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('quotes_ready', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    {link}t AS (INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode('{digest}', 'hex'), now() FROM j)
    INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, notes,
      original, ai_read, confirmed, received_at)
    SELECT j.id, md5('perf-business-' || (1 + ({i} * 3 + q.n) % {BUSINESSES}))::uuid, 'firm', 19000, false, '',
      'Sample wording from business ' || q.n, false, true, now() + q.n * interval '1 minute'
    FROM j, generate_series(1, 3) AS q(n);""")
    psql("\n".join(statements))
    return tokens
