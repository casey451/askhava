# Ask Hava build status

Updated 2026-11-07T06:31:25Z by runner 609e0ed0c74f (actions).

**Idle since 06:25Z; 133 of 256 done; week 35% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 133 of 256 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** idle since 06:25Z (no task in progress)
- **Week:** 35% used, 126.0M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 06:31Z, 0 min before this update
- **Keeper:** last ran 06:31Z, next wake 06:51Z

## Needs attention
- Strict plan check red: 1 problem(s) on tasks still to run (+1 on DONE tasks); first: T269: stored context_pct 10% but it measures 14%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T090 | privacy: photo checks by bytes, re-encode without EXIF or GP | MERGE_PENDING | 1 | 52 min | 06:25Z |  |
| T194 | portals: the new-device code by text or email and team sign- | done | 1 | 42 min | 05:12Z |  |
| T084 | portals: business one-time sign-in link and 30-day sessions | done | 2 | 24 min | 04:14Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T084 | portals: business one-time sign-in link and 30-day sessions | check_failed | 1 | 29 min | 03:36Z |  |
| T083 | businesses: complaint replies, 3 in 90 days pause, alerts | done | 1 | 76 min | 02:49Z |  |
| T082 | businesses: E-SIGN consent box, welcome text, business STOP | done | 1 | 53 min | 01:13Z |  |
| T193 | businesses: sign-up screens for someone else filling in, the | done | 1 | 55 min | 00:02Z |  |
| T080 | calls: relay-friendly bridging, identity call, code call | done | 1 | 49 min | 22:52Z |  |
| T081 | businesses: sign-up, identity chain from public records, act | done | 1 | 37 min | 21:42Z |  |
| T079 | calls: line closing, staff phones and LINK, email-preferred  | done | 1 | 43 min | 20:51Z |  |
| T078 | calls: never recorded, 45-second connection rule, call logs | done | 1 | 46 min | 19:53Z |  |
| T077 | calls: PIN routing and the paid-business read-back | done | 1 | 60 min | 18:52Z |  |
| T076 | calls: masked numbers from a fixed pool, PIN sharing when ex | done | 1 | 41 min | 17:32Z |  |
| T250 | core: narrow the default privileges in every schema and prov | done | 5 | 19 min | 16:38Z | T250 blocked: migration 0110 names 12 other modules' schemas; TD-SEC-10 (architecture_migrations.rs, protected) rejects  |
| T250 | core: narrow the default privileges in every schema and prov | check_failed | 4 | 24 min | 15:37Z |  |
| T250 | core: narrow the default privileges in every schema and prov | check_failed | 3 | 26 min | 14:20Z |  |
| T250 | core: narrow the default privileges in every schema and prov | QUEUED | 2 | 9 min | 13:30Z |  |
| T250 | core: narrow the default privileges in every schema and prov | QUEUED | 1 | 26 min | 13:18Z |  |
| T095 | picks: pick code by email when texting is off; '2 YES' from  | done | 1 | 36 min | 12:39Z |  |
| T075 | Stage 3 cleanup: dead code, duplicates, docs up to date | proof_missing | 1 | 45 min | 11:07Z |  |
| T246 | picks, portals and server: the page pick end to end (one pic | done | 1 | 66 min | 10:04Z |  |
| T247 | jobs and quotes: the first-quote move to Quotes ready (JOB-0 | done | 1 | 24 min | 08:42Z |  |
| T074 | Twin: run the gate against the app's rules modules | done | 1 | 51 min | 08:05Z |  |
| T073 | Named mutants: the 4-month rule, the fee tiers and the card  | done | 1 | 30 min | 06:37Z |  |

### Plan and harness merges on main
- 05:10Z Plan: repo audit findings — owners and acceptance for A01–A06, A08–A12 (D-1751–D-1761, D-1788) (#316)
- 03:59Z Plan: tests/a11y/fixtures.ts is a shared page-state registry (D-1772) (#314)
- 03:48Z Plan: business-session read of released picks, release recovery tests, production release wiring (D-1739, D-1740) (#313)
- 16:15Z Plan: dynamic SQL in migrations is a TD-SEC-10 finding (T252), T126 context budget, stored budgets refreshed (D-1677, D-1678) (#298)
- 16:05Z Plan: bump actix-http 3.20.2 -> 3.20.3 for RUSTSEC-2026-0455 (D-1666) (#297)
- 15:10Z Plan: bump actix-http 3.20.0 -> 3.20.2 for RUSTSEC-2026-0446 (D-1643) (#295)
- 14:27Z Plan: T250 becomes a harness task with a narrow TD-SEC-10 exception for default-privilege REVOKE migrations (D-1632) (#294)

## Progress (requirements verified)

**Requirements verified:** 202 of 731 (27.6%) · partial 259 · unstarted 270
**Tasks merged (throughput only):** 133 of 256

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 13 | 11 | 11 | 35 |
| ai | 12 | 11 | 5 | 28 |
| businesses | 4 | 28 | 26 | 58 |
| calls | 0 | 24 | 1 | 25 |
| consent | 13 | 16 | 6 | 35 |
| fees | 20 | 31 | 1 | 52 |
| intake | 19 | 15 | 9 | 43 |
| job-cards | 12 | 5 | 7 | 24 |
| licensing | 31 | 16 | 1 | 48 |
| lifecycle | 19 | 6 | 3 | 28 |
| messaging | 16 | 12 | 17 | 45 |
| ops-recovery | 1 | 5 | 48 | 54 |
| privacy | 0 | 16 | 15 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 17 | 26 | 13 | 56 |
| recurring | 7 | 1 | 3 | 11 |
| security | 9 | 28 | 17 | 54 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 1 | 27 | 28 |
| wording | 2 | 2 | 28 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-07T05:17:27Z (week)): five-hour: 8.2M used, 20.9M left, resets 2026-11-07T07:49:30Z; week: 68.0M used, 126.0M left, resets 2026-11-12T17:00:00Z; projected finish 2026-11-21T15:58:31Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 13 of 72 done
- stage 5: 0 of 42 done
- stage 6: 0 of 26 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T090", "at": "2026-11-07T06:25:29Z", "cache_read_input_tokens": 22282214, "cache_creation_input_tokens": 566621, "output_tokens": 192604, "input_tokens": 444}
