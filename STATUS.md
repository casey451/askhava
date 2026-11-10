# Ask Hava build status

Updated 2026-11-10T01:51:19Z by runner 3bb23bcaf87b (actions).

**Building: T284 in progress since 01:29Z; 177 of 324 done; week 77% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 177 of 324 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 1: T284 claimed 01:29Z, lease until 04:29Z
- **Week:** 77% used, 40.0M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 01:29Z, 21 min before this update
- **Keeper:** last ran 01:18Z, next wake 01:38Z

## Needs attention
- Strict plan check red: 1 problem(s) on tasks still to run (+1 on DONE tasks); first: T284: stored context_pct 10% but it measures 15%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T284 | server: every route the architecture lists is wired in the r | check_failed | 1 | 34 min | 00:32Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests / test tests::ssrf_g |
| T283 | Ci: a change to a document the app compiles in runs the Rust | done | 1 | 30 min | 23:27Z |  |
| T287 | payments and picks: the fee fixed at the pick is stored by p | done | 2 | 43 min | 22:15Z | T287 built and smoke green, but prehandin check_pr_change fails: kit/tasks.json T287 owns lacks app/payments/src/models. |
| T287 | payments and picks: the fee fixed at the pick is stored by p | QUEUED | 1 | 46 min | 21:18Z |  |
| T279 | messaging, ops and server: the texting switch and a number s | done | 1 | 47 min | 19:32Z |  |
| T276 | privacy and messaging: purged photos' sealed objects and orp | done | 1 | 53 min | 17:46Z |  |
| T275 | adapters and stand-ins: local write-once storage that a raci | done | 1 | 38 min | 16:08Z |  |
| T274 | intake and privacy: the 180-day contact purge decides and cl | done | 1 | 66 min | 14:56Z |  |
| T270 | picks and payments: the unreachable refund and the fraud war | done | 1 | 29 min | 13:17Z |  |
| T282 | calls and picks: each call's start time reaches the unreacha | done | 2 | 32 min | 12:06Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Date:      2026- |
| T282 | calls and picks: each call's start time reaches the unreacha | check_failed | 1 | 27 min | 10:49Z |  |
| T267 | Ally harness: page states move to a shared data file; the ch | done | 1 | 34 min | 09:42Z |  |
| T265 | stage exit: a security, money or privacy handoff in the inte | done | 1 | 25 min | 08:49Z |  |
| T264 | coverage checker: a missing measurement fails, and the total | done | 1 | 24 min | 07:58Z |  |
| T262 | calls and server: a random stored PIN per line, unique on it | done | 1 | 33 min | 06:56Z |  |
| T257 | privacy and server: the scrubber remembers names across a ho | done | 1 | 56 min | 05:54Z |  |
| T263 | payments: the monthly fee limit is a hard cap; capacity is r | done | 1 | 49 min | 04:26Z |  |
| T260 | stand-in and adapters: an unconfirmed PaymentIntent, its on- | done | 1 | 42 min | 03:09Z |  |
| T259 | payments: the Stripe webhook dispatches refund, dispute and  | done | 1 | 61 min | 01:52Z |  |

### Plan and harness merges on main
- 01:33Z Plan: the refused-connection test holds its port (T338, D-2318) (#404)
- 00:54Z Plan: sixth audit — $0 release bound to its pick, refund amounts checked, reconcile catch-up and paging, real Stripe reads, rate-limit prune, inbound-tex
- 00:05Z Plan: fifth audit — every queued task kind gets a registered handler; late success on ended picks; unreadable Stripe success and unmatched fraud warning 
- 21:41Z Plan: the LINK notice names the stored PIN; job_pin deleted (T326, D-2247) (#395)
- 21:26Z Plan: T287 owns payments models.rs and T254's release_recovery test (D-2236) (#394)
- 20:44Z Harness: Fable model window from the Thu Nov 12 reset, run to the week limit, then back to the plan's models (D-2225) (#392)
- 19:01Z Plan: fourth audit — voice confirm line, emergency notices, dispute atomicity, feedback purge, route limits, alerts (D-2200–D-2209, T311–T325) (#388)

## Progress (requirements verified)

**Requirements verified:** 237 of 732 (32.4%) · partial 297 · unstarted 198
**Tasks merged (throughput only):** 177 of 324

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 14 | 14 | 7 | 35 |
| ai | 11 | 13 | 5 | 29 |
| businesses | 17 | 31 | 10 | 58 |
| calls | 11 | 13 | 1 | 25 |
| consent | 14 | 17 | 4 | 35 |
| fees | 10 | 41 | 1 | 52 |
| intake | 20 | 14 | 9 | 43 |
| job-cards | 12 | 10 | 2 | 24 |
| licensing | 33 | 14 | 1 | 48 |
| lifecycle | 17 | 11 | 0 | 28 |
| messaging | 16 | 25 | 4 | 45 |
| ops-recovery | 2 | 5 | 47 | 54 |
| privacy | 7 | 21 | 3 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 31 | 15 | 10 | 56 |
| recurring | 7 | 1 | 3 | 11 |
| security | 7 | 40 | 7 | 54 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 2 | 26 | 28 |
| wording | 1 | 5 | 26 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-10T01:17:52Z (week)): five-hour: 2.6M used, 26.5M left, resets 2026-11-10T04:27:48Z; week: 154.0M used, 40.0M left, resets 2026-11-12T17:00:00Z; projected finish 2026-11-30T14:59:43Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 57 of 125 done
- stage 5: 0 of 53 done
- stage 6: 0 of 30 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T284 (lease until 2026-11-10T04:29:21Z)

## Last usage reading
- {"task": "T284", "at": "2026-11-10T00:32:29Z", "input_tokens": 291, "cache_read_input_tokens": 11820831, "cache_creation_input_tokens": 482032, "output_tokens": 95281}
