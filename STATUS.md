# Ask Hava build status

Updated 2026-11-07T03:40:59Z by runner 5aac33d8a77e (actions).

**Idle since 03:36Z; 131 of 239 done; week 29% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 131 of 239 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** idle since 03:36Z (no task in progress)
- **Week:** 29% used, 138.0M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 03:40Z, 0 min before this update
- **Keeper:** last ran 03:40Z, next wake 04:00Z

## Needs attention
- Strict plan check red: 10 problem(s) on tasks still to run (+17 on DONE tasks); first: T084: stored context_pct 11% but it measures 15%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T084 | portals: business one-time sign-in link and 30-day sessions | check_failed | 1 | 29 min | 03:36Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
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
| T072 | Metamorphic prompt-injection tests over routing, fees and di | done | 1 | 30 min | 05:55Z |  |
| T071 | Stage 3 check: the ledger matches the Stripe mock on 1,000 s | done | 1 | 37 min | 05:09Z |  |
| T070 | picks: 'no longer active' once, Confirm link replies | done | 1 | 45 min | 04:18Z |  |

### Plan and harness merges on main
- 16:15Z Plan: dynamic SQL in migrations is a TD-SEC-10 finding (T252), T126 context budget, stored budgets refreshed (D-1677, D-1678) (#298)
- 16:05Z Plan: bump actix-http 3.20.2 -> 3.20.3 for RUSTSEC-2026-0455 (D-1666) (#297)
- 15:10Z Plan: bump actix-http 3.20.0 -> 3.20.2 for RUSTSEC-2026-0446 (D-1643) (#295)
- 14:27Z Plan: T250 becomes a harness task with a narrow TD-SEC-10 exception for default-privilege REVOKE migrations (D-1632) (#294)

## Progress (requirements verified)

**Requirements verified:** 215 of 731 (29.4%) · partial 240 · unstarted 276
**Tasks merged (throughput only):** 131 of 239

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 14 | 10 | 11 | 35 |
| ai | 14 | 9 | 5 | 28 |
| businesses | 5 | 27 | 26 | 58 |
| calls | 0 | 24 | 1 | 25 |
| consent | 15 | 14 | 6 | 35 |
| fees | 24 | 27 | 1 | 52 |
| intake | 19 | 15 | 9 | 43 |
| job-cards | 12 | 5 | 7 | 24 |
| licensing | 32 | 15 | 1 | 48 |
| lifecycle | 19 | 6 | 3 | 28 |
| messaging | 16 | 11 | 18 | 45 |
| ops-recovery | 1 | 5 | 48 | 54 |
| privacy | 2 | 14 | 15 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 16 | 27 | 13 | 56 |
| recurring | 7 | 1 | 3 | 11 |
| security | 10 | 22 | 22 | 54 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 1 | 27 | 28 |
| wording | 2 | 2 | 28 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-07T02:53:41Z (week)): five-hour: 3.0M used, 26.1M left, resets 2026-11-07T07:49:30Z; week: 56.0M used, 138.0M left, resets 2026-11-12T17:00:00Z; projected finish 2026-11-20T05:20:15Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 11 of 55 done
- stage 5: 0 of 42 done
- stage 6: 0 of 26 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T084", "at": "2026-11-07T03:36:53Z", "cache_creation_input_tokens": 440781, "output_tokens": 126165, "input_tokens": 326, "cache_read_input_tokens": 19327620}
