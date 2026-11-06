# Ask Hava build status

Updated 2026-11-06T11:26:57Z by runner ea22387162b9 (actions).

**Idle since 11:21Z; 119 of 238 done; week 15% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 119 of 238 build tasks (owner gates not counted)
- **Stage:** stage 3; stage proofs recorded: 1, 2
- **Lane:** idle since 11:21Z (no task in progress)
- **Week:** 15% used, 166.0M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 11:26Z, 0 min before this update
- **Keeper:** last ran 11:23Z, next wake 17:03Z

## Needs attention
- BLOCKED T075: stage proof missing (the proof waits for: stage 3's proof is recorded by hand (`python3 -m runner --record-proof 3 --evidence TEXT`) (its cargo-mutants clause: nightly-main's cargo-mutants job is report only until tests/mutation/equivalent.toml holds reviewed reasons (D-1286), kit/nightly.json records no result for it, and app/catalog/ has no rules.rs to mutate))
- Strict plan check red: 10 problem(s) on tasks still to run (+23 on DONE tasks); first: T191: stored context_pct 10% but it measures 14%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
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
- 19:30Z Plan: page pick end to end, first-quote move, markets and service radius, schema default privileges, card sender (D-1515–D-1521) (#272)
- 17:59Z Plan: drop the early-joiner price lock, payments default privileges, Nov 3 handoff review (D-1494–D-1499) (#269)

## Progress (requirements verified)

**Requirements verified:** 181 of 731 (24.8%) · partial 222 · unstarted 328
**Tasks merged (throughput only):** 119 of 238

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 14 | 10 | 11 | 35 |
| ai | 14 | 9 | 5 | 28 |
| businesses | 4 | 7 | 47 | 58 |
| calls | 0 | 2 | 23 | 25 |
| consent | 14 | 12 | 9 | 35 |
| fees | 11 | 40 | 1 | 52 |
| intake | 19 | 15 | 9 | 43 |
| job-cards | 12 | 5 | 7 | 24 |
| licensing | 31 | 16 | 1 | 48 |
| lifecycle | 14 | 11 | 3 | 28 |
| messaging | 15 | 10 | 20 | 45 |
| ops-recovery | 1 | 4 | 49 | 54 |
| privacy | 1 | 15 | 15 | 31 |
| quiet-hours | 6 | 3 | 0 | 9 |
| quotes-pick | 8 | 33 | 15 | 56 |
| recurring | 6 | 2 | 3 | 11 |
| security | 9 | 23 | 22 | 54 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 0 | 28 | 28 |
| wording | 2 | 2 | 28 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-06T10:08:05Z (week)): five-hour: 0k used, 29.1M left, resets on first use; week: 28.0M used, 166.0M left, resets 2026-11-12T17:00:00Z; projected finish 2026-11-19T14:10:42Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 28 of 29 done
- stage 4: 0 of 54 done
- stage 5: 0 of 42 done
- stage 6: 0 of 26 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T075", "at": "2026-11-06T11:07:06Z", "cache_read_input_tokens": 13773071, "output_tokens": 86468, "cache_creation_input_tokens": 401509, "input_tokens": 356}
