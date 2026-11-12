# Ask Hava — Performance Optimization Review and Claude Implementation Brief (November 12, 2026)

Casey supplied this review in chat on November 12, 2026 (~16:15Z) for the build to act on. It is a targeted source review, not a load test; every figure in it is a proposal, not a measurement. The overseer checked on `main` (`c65dd22c`) that each cited file exists and that the cited patterns are present: `POLL_MS = 30_000` with a full-page `fetch` + `DOMParser` refresh (`app/portals/static/quote_page.ts:8`, `:77`, `:85`), the linear `town_for_zip` scan (`app/catalog/src/markets.rs:141-146`), and `ACQUIRE_TIMEOUT = 5s` (`app/core/src/db.rs:30`). The spec (`docs/spec/`) holds no latency or Core Web Vitals targets today; adopting the targets below is a "what" decision for the plan (decide and log, per the order of authority). The brief's text follows unchanged.

---

**Purpose:** Prevent the slow-feeling interaction patterns observed on larger home-service marketplaces while preserving correctness, privacy, and security.

## Scope and evidence standard

This is a **targeted source-code review**, not a completed line-by-line audit of every repository file and **not a production load test**. The findings below were verified against GitHub source in the current review, and additional audit targets are explicitly labeled *unverified*. No performance improvement percentages are claimed without benchmarks. Claude must inspect the current branch before changing anything, because development is ongoing. If an issue is already fixed, mark it resolved rather than implementing it twice.

### Executive priority
1. **P1: Full-page quote polling** — confirmed inefficient transfer/DOM work; optimize after measuring.
2. **P1: Sequential quote-page database round trips** — confirmed pattern; measure and reduce critical-path latency safely.
3. **P2: ZIP lookup linear scan** — confirmed algorithm; benchmark at realistic national catalog size before replacing.
4. **P2: Connection pool acquisition / contention** — confirmed configuration; instrument, do not blindly increase pool size.
5. **P2: Request timeouts versus user-perceived responsiveness** — confirmed timeout implementation; establish route-level latency budgets.
6. **P2/P3: Other hot paths** — audit matching, business portals, intake, frontend assets, and third-party calls; not yet established as defects.

---

## Finding PERF-001 — Quote page downloads and parses the entire page repeatedly
**Priority:** P1 | **Evidence:** Confirmed source pattern | **Impact:** Network transfer, server rendering, client DOM parsing, database read amplification.

**Source:** `app/portals/static/quote_page.ts`, around lines 76–115.

**Evidence:** `refresh()` calls `fetch(window.location.href, { credentials: "same-origin", cache: "no-store", headers: { Accept: "text/html" } })`, parses `response.text()` with `new DOMParser().parseFromString(..., "text/html")`, extracts `#quotes`, and merges into the existing list. The code schedules another refresh using `window.setTimeout(tick, POLL_MS)` while live. The prior review identified a 30-second polling interval; verify the current `POLL_MS` declaration.

**Why it matters:** Even when nothing changes, the system transfers and parses a complete HTML document and repeats the server-side work required to render it. This scales with the number of concurrently open quote pages and may use extra mobile battery and data.

**Recommended approach:** Benchmark first. Prefer a compact, authenticated, token-scoped status/changes endpoint (JSON or a narrowly rendered HTML fragment), with a version/ETag or `since` cursor, returning 304/no-change when appropriate. Preserve access checks, CSRF protections for mutations, privacy, ordering, quote visibility, accessibility announcements, and final-state behavior. Pause polling for hidden tabs; use capped exponential backoff with jitter for transient errors; avoid stopping forever on a single network failure. Consider SSE only if it is operationally justified—do not introduce WebSockets by default.

**Verification:** Compare transferred bytes/minute, server CPU, database queries per open page, and time to reflect a new quote at 1, 100, 1,000, and 10,000 *simulated* viewers. Ensure no quote data leaks across users and updates remain correct on reconnect.

## Finding PERF-002 — Quote-page rendering contains sequential database calls
**Priority:** P1 | **Evidence:** Confirmed source pattern | **Impact:** Tail latency and connection occupancy.

**Source:** `app/portals/src/quote_page.rs`, `load()`, around lines 225–263; `accepted()` around 267–278.

**Evidence:** The handler acquires a connection and awaits `resolve`, `current`, `job_number`, conditionally `paused_by_homeowner`, `page_quotes`, and `accepted`; `accepted` can then await `accepted_pick` and `accepted_quote`. The queries are sequential while one connection is held.

**Why it matters:** Several small round trips can add up, particularly with a remote database or p95/p99 contention. The same work may be triggered by repeated full-page polling.

**Recommended approach:** Capture per-query latency, query plans, row counts, and request spans. Consider a single read-only query/CTE or a small number of batched queries for logically related fields; use indexes only where `EXPLAIN (ANALYZE, BUFFERS)` shows benefit. Do **not** parallelize queries on one PostgreSQL connection. If splitting across pooled connections, account for consistency and extra pool pressure. Maintain authorization checks before exposing quote details, and preserve consistent snapshot semantics where needed.

**Verification:** Compare p50/p95/p99 response time and queries/request before and after under realistic data distributions. Test paused/final/expired jobs, replacement picks, and concurrent quote changes.

## Finding PERF-003 — ZIP-to-town lookup is a linear scan
**Priority:** P2 | **Evidence:** Confirmed source pattern; performance impact unmeasured.

**Source:** `app/catalog/src/markets.rs`, `Markets::town_for_zip()`, around lines 141–147.

**Evidence:** The function trims the ZIP and uses `self.pairs().find(...)`, checking each town's `zips` and `extra_zips` arrays until it finds a match.

**Why it matters:** Lookup cost increases with catalog size. For an Arizona-only catalog, this may be negligible; at national scale it can become unnecessary repeated work if invoked on every request.

**Recommended approach:** Benchmark against the expected number of ZIPs/towns and requests per second. If warranted, construct a validated `HashMap<ZIP, market/town>` once during startup or catalog reload, preserving duplicate validation and source-of-truth semantics. A compiled-in static map is another option. Avoid introducing a remote cache or search engine for a simple deterministic mapping.

**Verification:** Benchmark cold startup cost, lookup p95, memory usage, and equivalence across every known ZIP and unknown ZIP. Ensure data refresh is atomic.

## Finding PERF-004 — Pool acquisition and database contention require instrumentation
**Priority:** P2 | **Evidence:** Confirmed configuration; contention not established.

**Source:** `app/core/src/db.rs`, around lines 28–30 and 89–104.

**Evidence:** `ACQUIRE_TIMEOUT` is five seconds; `app_pool` uses `PgPoolOptions::new().max_connections(settings.db_pool_max()).acquire_timeout(ACQUIRE_TIMEOUT).connect_lazy_with(options)`.

**Why it matters:** A five-second acquire timeout is a failure limit, not a responsiveness target. If a connection is held during sequential reads, queueing can compound latency under load. Conversely, increasing pool size blindly can overload PostgreSQL.

**Recommended approach:** Instrument connection acquisition wait, active/idle pool connections, query duration, lock wait, transaction age, and timeout rate. Set pool sizes from measured concurrency and PostgreSQL capacity, including all replicas and workers. Audit transactions that hold a connection across network or AI calls; release connections before slow external operations. Consider startup readiness checks without exposing secrets.

**Verification:** Sustained load and burst tests; confirm p95 acquire wait and error rates remain within budgets, and PostgreSQL connection limits are respected.

## Finding PERF-005 — Request timeout is not a UX performance budget
**Priority:** P2 | **Evidence:** Confirmed implementation; no defect implied.

**Source:** `app/server/src/request_timeout.rs`, around lines 38 and 55.

**Evidence:** Request handling uses `tokio::time::timeout(limit.0, next.run(request))`, with a settings-controlled duration. The prior review noted a 60-second default; verify current settings.

**Why it matters:** A request can remain technically within timeout yet feel unusably slow. Timeouts also do not automatically cancel work running in separate jobs or external services.

**Recommended approach:** Define separate p95/p99 budgets for static pages, ZIP lookup, matching, quote pages, and intake acknowledgement. Add tracing spans and alerts for slow routes; use bounded provider timeouts and background processing for expensive tasks where the workflow permits. Do not shorten timeouts without considering payment and idempotency behavior.

**Verification:** Test slow database, slow AI provider, slow SMS provider, partial outages, and retries. User must receive immediate truthful status without duplicate jobs or charges.

---

## Additional codebase-wide audit targets — not yet verified defects

Claude should search **all relevant crates, handlers, migrations, templates, TypeScript, and background workers**, documenting concrete code evidence before proposing changes:

1. **N+1 database queries:** loops that call `fetch_one`, `fetch_optional`, or `fetch_all` per business, quote, photo, category, or notification.
2. **Unbounded result sets:** `fetch_all` without reasonable limits; missing pagination; large `SELECT *`; repeated hydration of entire business or job records.
3. **Matching:** repeated business service-area scans, category filtering, capability checks, distance calculations, duplicate database reads, and expensive matching performed synchronously during intake.
4. **Indexes and query plans:** service-area/ZIP, business eligibility, job status, quote-by-job, owner dashboards, token lookup, and polling queries. Check migration-defined indexes against actual predicates.
5. **External dependencies on critical path:** AI, Twilio, Stripe, email, image screening, geocoding. Separate required synchronous validation from background operations without breaking workflow guarantees.
6. **Frontend delivery:** HTML/CSS/JS payload sizes, render-blocking resources, compression, caching headers, font loading, layout shift, mobile INP, and image dimensions.
7. **Worker throughput:** per-job serial sends, concurrency limits, backpressure, retry storms, lock contention, and idempotency.
8. **Cache correctness:** static public data can be cached; never cache sensitive quote, token, payment, or contact-release responses across users.
9. **Operational scaling:** database connection limits per replica, queue depth, cold starts, regional network latency, and object-storage delivery.

For each finding, record: file path and line range, exact mechanism, whether observed or hypothetical, affected user flow, proposed fix, security/correctness risks, benchmark methodology, before/after result, and regression test.

## Performance targets (proposed, not current measurements)

| Experience | Target |
|---|---|
| User-visible feedback after click | <100 ms where feasible |
| Simple ZIP lookup endpoint | p95 <100 ms |
| Local matching/results endpoint (without AI) | p95 <250 ms |
| Mobile LCP | p75 <1.5 s aspirational; must meet Core Web Vitals good threshold of 2.5 s |
| Mobile INP | p75 <150 ms aspirational; good threshold 200 ms |
| CLS | p75 <0.1 |
| Error rate under defined normal load | <0.1% excluding expected client errors |

Measure with real-device RUM and repeatable synthetic tests; specify dataset size, deployment region, network, hardware, concurrency, and cache state. Do not turn aspirational targets into guarantees.

## Instructions to Claude — execution order

1. Inspect current `main` HEAD and compare these paths; mark resolved/changed findings. Do not duplicate work already completed.
2. Establish reproducible baseline and tracing, with representative seeded businesses, ZIPs, jobs, quotes, and concurrent sessions. Never use real customer PII in tests.
3. Profile critical user flows: homepage -> ZIP -> intake -> matching -> quote page -> pick/payment -> business dashboard.
4. Confirm or reject each finding using measurements and explain plans. Prioritize by **measured user impact × frequency × confidence**, not novelty.
5. Implement one optimization at a time, with tests preserving privacy, authorization, deterministic matching, quote order, business eligibility, idempotency, and payment correctness.
6. Run load tests before and after, including p95/p99, throughput, CPU, memory, database load, payload size, and error rates.
7. Keep architecture lean: no Elasticsearch, Redis, WebSockets, or distributed services unless measurements justify their complexity.
8. Produce a final `PERFORMANCE_FINDINGS.md` with a table of confirmed issues, rejected hypotheses, exact diffs, benchmarks, and remaining risks. Do not claim unmeasured speedups.

**Non-goal:** Rewrite Rust, Axum, PostgreSQL, or server-rendered templates merely because other marketplaces use different technologies. Optimize observed bottlenecks while preserving the project's security and audit-readiness goals.
