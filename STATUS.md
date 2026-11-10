# Ask Hava build status

Updated 2026-11-10T05:30:07Z by runner 3976d91e2924 (actions).

**Building: T288 in progress since 05:29Z; 180 of 333 done; week 84% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 180 of 333 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 1: T288 claimed 05:29Z, lease until 08:29Z
- **Week:** 84% used, 26.0M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 05:09Z, 20 min before this update
- **Keeper:** last ran 05:09Z, next wake 05:29Z

## Needs attention
- nothing

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T286 | server: every request has a whole-request time limit, answer | done | 1 | 26 min | 04:57Z |  |
| T285 | runner: a task closes with a visible mark when a file it own | done | 1 | 19 min | 03:45Z |  |
| T284 | server: every route the architecture lists is wired in the r | done | 3 | 34 min | 03:02Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests / test tests::ssrf_g |
| T284 | server: every route the architecture lists is wired in the r | check_failed | 2 | 21 min | 01:51Z |  |
| T284 | server: every route the architecture lists is wired in the r | check_failed | 1 | 34 min | 00:32Z |  |
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

### Plan and harness merges on main
- 03:30Z Plan: pentest assessment — Actions pinned, purpose-bound codes, Verify sends outside transactions (D-2346–D-2350, T345–T346) (#407)
- 02:15Z Plan: seventh audit — fee facts from the job's category at the pick, tier and per-visit picks, whole-field quote gate, atomic Verify cap (D-2329–D-2335, 
- 01:33Z Plan: the refused-connection test holds its port (T338, D-2318) (#404)
- 00:54Z Plan: sixth audit — $0 release bound to its pick, refund amounts checked, reconcile catch-up and paging, real Stripe reads, rate-limit prune, inbound-tex
- 00:05Z Plan: fifth audit — every queued task kind gets a registered handler; late success on ended picks; unreadable Stripe success and unmatched fraud warning 
- 21:41Z Plan: the LINK notice names the stored PIN; job_pin deleted (T326, D-2247) (#395)
- 21:26Z Plan: T287 owns payments models.rs and T254's release_recovery test (D-2236) (#394)
- 20:44Z Harness: Fable model window from the Thu Nov 12 reset, run to the week limit, then back to the plan's models (D-2225) (#392)
- 19:01Z Plan: fourth audit — voice confirm line, emergency notices, dispute atomicity, feedback purge, route limits, alerts (D-2200–D-2209, T311–T325) (#388)

## Progress (requirements verified)

**Requirements verified:** 232 of 733 (31.7%) · partial 302 · unstarted 199
**Tasks merged (throughput only):** 180 of 333

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
| quotes-pick | 28 | 18 | 10 | 56 |
| recurring | 5 | 3 | 3 | 11 |
| security | 7 | 40 | 8 | 55 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 2 | 26 | 28 |
| wording | 1 | 5 | 26 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-10T05:09:20Z (week)): five-hour: 1.3M used, 27.8M left, resets 2026-11-10T09:57:17Z; week: 168.0M used, 26.0M left, resets 2026-11-12T17:00:00Z; projected finish 2026-12-01T10:41:26Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 60 of 134 done
- stage 5: 0 of 53 done
- stage 6: 0 of 30 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T288 (lease until 2026-11-10T08:29:54Z)

## Last usage reading
- {"task": "T286", "at": "2026-11-10T04:57:17Z", "cache_read_input_tokens": 9060026, "input_tokens": 220, "output_tokens": 106170, "cache_creation_input_tokens": 333980}
