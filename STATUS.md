# Ask Hava build status

Updated 2026-11-06T17:37:25Z by runner 48d8e99c20f7 (actions).

**Idle since 17:32Z; 122 of 239 done; week 23% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 122 of 239 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** idle since 17:32Z (no task in progress)
- **Week:** 23% used, 150.0M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 17:37Z, 0 min before this update
- **Keeper:** last ran 17:37Z, next wake 17:57Z

## Needs attention
- Strict plan check red: 1 problem(s) on tasks still to run (+6 on DONE tasks); first: T249: stored context_pct 22% but it measures 26%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T076 | calls: masked numbers from a fixed pool, PIN sharing when ex | MERGE_PENDING | 1 | 41 min | 17:32Z |  |
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
| T192 | picks: compile-fail proofs that no crate but payments can bu | done | 1 | 26 min | 03:17Z |  |
| T069 | picks: release only on paid or free; never the homeowner's p | done | 1 | 48 min | 02:36Z |  |
| T068 | payments: receipt emails, charge description, just-under-a-t | done | 1 | 42 min | 01:35Z |  |
| T067 | payments: nightly reconcile, auto-match or auto-refund, ledg | done | 1 | 49 min | 00:38Z |  |
| T064 | payments: success after the job moved on is refunded once an | done | 1 | 27 min | 23:35Z |  |
| T066 | payments: one refund per trigger, retries, disputes with evi | done | 1 | 46 min | 22:54Z |  |
| T245 | payments: narrow the default privileges in schema payments t | done | 1 | 21 min | 21:56Z |  |
| T244 | payments: remove the early-joiner price lock (FEE-040 withdr | done | 1 | 19 min | 21:20Z |  |
| T065 | payments: refund rules for cancels and no-shows | done | 1 | 38 min | 20:38Z |  |
| T190 | picks: Hold for 2 more hours, the emergency 60-minute window | done | 1 | 32 min | 19:45Z |  |
| T063 | picks: Confirm within 4 business hours, license at pick and  | done | 1 | 47 min | 19:01Z |  |
| T224 | portals: the business Confirm-and-pay page with Stripe Eleme | done | 2 | 50 min | 17:59Z | session ended without handing in |

### Plan and harness merges on main
- 16:15Z Plan: dynamic SQL in migrations is a TD-SEC-10 finding (T252), T126 context budget, stored budgets refreshed (D-1677, D-1678) (#298)
- 16:05Z Plan: bump actix-http 3.20.2 -> 3.20.3 for RUSTSEC-2026-0455 (D-1666) (#297)
- 15:10Z Plan: bump actix-http 3.20.0 -> 3.20.2 for RUSTSEC-2026-0446 (D-1643) (#295)
- 14:27Z Plan: T250 becomes a harness task with a narrow TD-SEC-10 exception for default-privilege REVOKE migrations (D-1632) (#294)
- 19:30Z Plan: page pick end to end, first-quote move, markets and service radius, schema default privileges, card sender (D-1515–D-1521) (#272)
- 17:59Z Plan: drop the early-joiner price lock, payments default privileges, Nov 3 handoff review (D-1494–D-1499) (#269)

## Progress (requirements verified)

**Requirements verified:** 211 of 731 (28.9%) · partial 196 · unstarted 324
**Tasks merged (throughput only):** 122 of 239

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 14 | 10 | 11 | 35 |
| ai | 14 | 9 | 5 | 28 |
| businesses | 5 | 6 | 47 | 58 |
| calls | 0 | 2 | 23 | 25 |
| consent | 15 | 12 | 8 | 35 |
| fees | 21 | 30 | 1 | 52 |
| intake | 19 | 15 | 9 | 43 |
| job-cards | 12 | 5 | 7 | 24 |
| licensing | 31 | 16 | 1 | 48 |
| lifecycle | 19 | 6 | 3 | 28 |
| messaging | 16 | 10 | 19 | 45 |
| ops-recovery | 1 | 4 | 49 | 54 |
| privacy | 2 | 14 | 15 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 16 | 27 | 13 | 56 |
| recurring | 7 | 1 | 3 | 11 |
| security | 10 | 22 | 22 | 54 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 0 | 28 | 28 |
| wording | 2 | 2 | 28 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-06T16:39:03Z (week)): five-hour: 413k used, 28.7M left, resets 2026-11-06T21:38:35Z; week: 44.0M used, 150.0M left, resets 2026-11-12T17:00:00Z; projected finish 2026-11-19T02:07:48Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 2 of 55 done
- stage 5: 0 of 42 done
- stage 6: 0 of 26 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T076", "at": "2026-11-06T17:32:53Z", "cache_creation_input_tokens": 499042, "output_tokens": 136410, "input_tokens": 398, "cache_read_input_tokens": 16620609}
