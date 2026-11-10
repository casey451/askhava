# Ask Hava build status

Updated 2026-11-10T20:15:40Z by runner 5ca79792dad1 (actions).

**Waiting for tokens until 17:00Z; 188 of 335 done; week 96% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 188 of 335 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** idle since 20:15Z (no task in progress)
- **Week:** 96% used, 0.4M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 20:05Z, 10 min before this update
- **Keeper:** last ran 20:05Z, next wake 22:20Z

## Needs attention
- Strict plan check red: 4 problem(s) on tasks still to run (+21 on DONE tasks); first: T155: stored context_pct 21% but it measures 29%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T155 | businesses: sign-up capability setup pre-ticked from the lic | MERGE_PENDING | 1 | 57 min | 20:15Z |  |
| T311 | calls and server: a voice read-back confirm connects only th | check_failed | 1 | 21 min | 18:54Z | Ci: failure; Coverage: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / test  |
| T294 | portals and site: every page takes the brand from the brand  | done | 1 | 42 min | 18:32Z |  |
| T231 | portals (harness): the homeowner check-in email buttons and  | done | 2 | 26 min | 17:08Z | T231 built and committed (b2f7b11e, 26074ad1) but smoke red: tests/security/tests/auth_matrix.rs (T088, protected, no se |
| T231 | portals (harness): the homeowner check-in email buttons and  | QUEUED | 1 | 28 min | 15:58Z |  |
| T199 | portals: no automated call to a business, voice codes only o | done | 1 | 64 min | 14:22Z |  |
| T293 | businesses and quotes: sign-up, consent, complaint and quote | done | 1 | 41 min | 12:35Z |  |
| T292 | messaging and calls: every text, email and call takes the br | done | 1 | 21 min | 11:16Z |  |
| T290 | worked examples: pin D-033 for one-cent-wide recurring range | done | 2 | 39 min | 10:21Z | T290 blocked: any new recurring worked-example row breaks tests::worked_examples (fixed RECURRING list of 13 ids, worked |
| T290 | worked examples: pin D-033 for one-cent-wide recurring range | QUEUED | 1 | 1 min | 09:07Z |  |
| T289 | jobs tests: the refused-move test counts its one bug log the | done | 1 | 44 min | 07:52Z |  |
| T288 | payments: a Stripe charge the nightly reconcile cannot settl | done | 1 | 60 min | 06:30Z |  |
| T286 | server: every request has a whole-request time limit, answer | done | 1 | 26 min | 04:57Z |  |
| T285 | runner: a task closes with a visible mark when a file it own | done | 1 | 19 min | 03:45Z |  |
| T284 | server: every route the architecture lists is wired in the r | done | 3 | 34 min | 03:02Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests / test tests::ssrf_g |
| T284 | server: every route the architecture lists is wired in the r | check_failed | 2 | 21 min | 01:51Z |  |
| T284 | server: every route the architecture lists is wired in the r | check_failed | 1 | 34 min | 00:32Z |  |
| T283 | Ci: a change to a document the app compiles in runs the Rust | done | 1 | 30 min | 23:27Z |  |
| T287 | payments and picks: the fee fixed at the pick is stored by p | done | 2 | 43 min | 22:15Z | T287 built and smoke green, but prehandin check_pr_change fails: kit/tasks.json T287 owns lacks app/payments/src/models. |
| T287 | payments and picks: the fee fixed at the pick is stored by p | QUEUED | 1 | 46 min | 21:18Z |  |

### Plan and harness merges on main
- 19:42Z Plan: second tiering pass — 13 more tasks on Sonnet, T159 on Haiku, reviewer counterexample duty (D-2506) (#427)
- 19:31Z Plan: PIN draws fail only when the OS source fails; one failed draw costs one draw (D-2451, T311) (#422)
- 19:17Z Plan: no Fable window on Thursday — the D-2225 window closes before it opens (D-2485) (#425)
- 18:21Z Plan: two lanes, and each queued task on the lowest model that can do it well (D-2462, D-2463) (#424)
- 16:53Z Plan: matrix doors before the serving tasks — public, owner and action-link doors, HTTP probe for served realm routes (D-2440, T348) (#421)
- 16:29Z Plan: T231 extends the authorization matrix to /c/{token}/{answer} as a harness task (D-2429) (#420)
- 13:07Z Plan: no handyman split-job check — each job judged by its own price (Casey's ruling; D-2407, D-2408, T347) (#416)
- 09:36Z Plan: T290 may update the two tests that pin recurring rows by data; nearest reachable bases (D-2386) (#413)
- 03:30Z Plan: pentest assessment — Actions pinned, purpose-bound codes, Verify sends outside transactions (D-2346–D-2350, T345–T346) (#407)
- 02:15Z Plan: seventh audit — fee facts from the job's category at the pick, tier and per-visit picks, whole-field quote gate, atomic Verify cap (D-2329–D-2335, 
- 01:33Z Plan: the refused-connection test holds its port (T338, D-2318) (#404)
- 00:54Z Plan: sixth audit — $0 release bound to its pick, refund amounts checked, reconcile catch-up and paging, real Stripe reads, rate-limit prune, inbound-tex
- 00:05Z Plan: fifth audit — every queued task kind gets a registered handler; late success on ended picks; unreadable Stripe success and unmatched fraud warning 
- 21:41Z Plan: the LINK notice names the stored PIN; job_pin deleted (T326, D-2247) (#395)
- 21:26Z Plan: T287 owns payments models.rs and T254's release_recovery test (D-2236) (#394)
- 20:44Z Harness: Fable model window from the Thu Nov 12 reset, run to the week limit, then back to the plan's models (D-2225) (#392)

## Progress (requirements verified)

**Requirements verified:** 236 of 733 (32.2%) · partial 304 · unstarted 193
**Tasks merged (throughput only):** 188 of 335

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 15 | 13 | 7 | 35 |
| ai | 11 | 13 | 5 | 29 |
| businesses | 17 | 32 | 9 | 58 |
| calls | 11 | 13 | 1 | 25 |
| consent | 14 | 17 | 4 | 35 |
| fees | 11 | 40 | 1 | 52 |
| intake | 20 | 14 | 9 | 43 |
| job-cards | 12 | 10 | 2 | 24 |
| licensing | 31 | 16 | 1 | 48 |
| lifecycle | 18 | 10 | 0 | 28 |
| messaging | 19 | 23 | 3 | 45 |
| ops-recovery | 2 | 5 | 47 | 54 |
| privacy | 7 | 21 | 3 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 28 | 18 | 10 | 56 |
| recurring | 5 | 3 | 3 | 11 |
| security | 7 | 42 | 6 | 55 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 1 | 6 | 25 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-10T19:39:42Z (week)): five-hour: 12.5M used, 16.6M left, resets 2026-11-10T20:58:09Z; week: 193.6M used, 399k left, resets 2026-11-12T17:00:00Z; projected finish 2026-12-01T06:05:01Z; waiting until 2026-11-12T17:00:00Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 68 of 137 done
- stage 5: 0 of 53 done
- stage 6: 0 of 30 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T155", "at": "2026-11-10T20:15:24Z", "output_tokens": 189359, "cache_read_input_tokens": 27115828, "cache_creation_input_tokens": 663166, "input_tokens": 479}
