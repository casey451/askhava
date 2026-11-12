# Route benchmark: first baseline (T417, November 12, 2026)

Performance track part 2 (D-3292, D-3319). The numbers are in `docs/audit/perf-baseline.json`, written by `python3 -m perf.bench --out <file> --baseline` at commit `bc343d81` in the task's session. No fix is made or claimed here; each fix is a later task with before and after numbers (D-3292).

## Method

Lab numbers on one build machine (4 CPUs, x86_64 Linux, PostgreSQL 16.15 on the same host), not production and not field data. The app is the debug, test-control build of `server` and `worker` with every adapter on its stand-in, pool maximum 10 (the default) and AI off. Each run uses its own fresh database on the CI Postgres server, seeded with fake data only: 1,500 businesses, 20,000 past jobs, 50,000 quotes and 11,000 paid picks (tables skipped are listed in the JSON's `skipped_tables`). Per route and concurrency: 20 warm-up requests (none for the final Pick), then 200 requests at 1, 10 and 50 concurrent users, each on a new connection. Percentiles are nearest rank. Queries per request are read from `GET /__test/perf/db` around each request at 1 user only. Acquire waits are bucket bounds from the snapshot difference around each phase, so "1,000" means "at most 1 ms". The error rate counts 5xx and timeouts (10 s); a 4xx is not an error.

## Results

| Route | Users | p50 ms | p95 ms | p99 ms | Queries | Acquire p95 / max (us, bound) | Median bytes | Errors |
|---|---|---|---|---|---|---|---|---|
| Intake acknowledgement (signed inbound text) | 1 / 10 / 50 | 10.4 / 64.0 / 290.7 | 20.8 / 229.2 / 462.2 | 23.7 / 391.0 / 510.6 | 29.7 | 1,000 / 5,000; 2,500 / 50,000; 500,000 / 500,000 | 0 | 0% |
| Quote page | 1 / 10 / 50 | 4.2 / 14.7 / 39.8 | 6.0 / 21.5 / 44.9 | 7.0 / 23.8 / 46.0 | 8.2 | 1,000 / 2,500; 5,000 / 10,000; 50,000 / 50,000 | 2,966 | 0% |
| Quote page polling refresh | 1 / 10 / 50 | 4.6 / 11.0 / 57.8 | 6.0 / 16.6 / 88.7 | 6.7 / 18.4 / 94.2 | 9.2 | 1,000 / 5,000; 5,000 / 10,000; 100,000 / 100,000 | 2,966 | 0% |
| Pick review step | 1 / 10 / 50 | 3.9 / 9.5 / 47.6 | 5.4 / 14.7 / 58.4 | 6.7 / 16.9 / 60.3 | 7.3 | 1,000 / 5,000; 5,000 / 5,000; 50,000 / 50,000 | 2,056 | 0% |
| Final Pick (code send to the Verify stand-in) | 1 / 10 / 50 | 13.0 / 60.3 / 276.4 | 27.4 / 69.3 / 315.1 | 38.7 / 73.8 / 320.1 | 33.7 | 1,000 / 5,000; 5,000 / 10,000; 250,000 / 250,000 | 2,418 | 0% |

Not exposed on `main`, so not measured: home (`GET /` answers 404), ZIP lookup (no HTTP route), matching (no results step; it runs in the worker), Confirm (not served, D-1480, so no paying request was made) and the business dashboard (`GET /b` answers 404). `perf_zip_scale` (41,000 made-up ZIPs over 29,000 towns, debug build): p95 0.3 us for the first ZIP, 805 us for the middle, 1,991 us for the last and 1,980 us for an unknown ZIP.

## D-3292 goals (met or missed, never a failure)

- ZIP lookup p95 under 100 ms: not measurable as a route (no route). The in-process lookup's worst p95 is 2.0 ms.
- Matching p95 under 250 ms: not measurable (no route).
- Error rate under 0.1%: met for every measured route and concurrency (0 of 200 each). 200 requests cannot show a rate below 0.5% with confidence, so this is weak evidence.

## Findings PERF-001 to PERF-005

- PERF-001 (full-page polling): confirmed. A poll costs what a first load costs: the whole 2,966-byte page and about 9 statements, every 30 seconds per open page (`POLL_MS = 30_000`). At 50 concurrent polls p95 is 88.7 ms with acquire waits up to 100 ms. Not measured: 100 to 10,000 viewers and the time for a new quote to show up.
- PERF-002 (sequential quote-page queries): confirmed. 8.2 statements per load on one connection. On this local database the load takes 6.0 ms at p95 with 1 user, so the cost of each round trip to a remote database is not yet measurable here.
- PERF-003 (linear ZIP scan): confirmed linear (first ZIP 0.3 us, last 1,991 us at national size). Rejected as a latency risk at today's sizes: the worst p95 is 2.0 ms against the 100 ms route goal. Today's catalog has one town.
- PERF-004 (pool acquire and contention): confirmed under load. At 50 users with a pool of 10, the acquire-wait p95 grows from at most 1 ms (1 user) to 50 ms (quote page, pick review), 100 ms (polling), 250 ms (final Pick) and 500 ms (intake), and intake p95 reaches 462 ms. No acquire timed out and there were no errors. Postgres connection limits and replicas are not measured.
- PERF-005 (request timeout vs. budget): not yet measurable. The slowest p99 was 511 ms, far inside the 60-second timeout. Slow-database, slow-AI and slow-SMS cases were not run.
