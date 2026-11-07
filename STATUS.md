# Ask Hava build status

Updated 2026-11-07T23:30:46Z by runner d3d384547e6d (actions).

**Idle since 23:28Z; 146 of 277 done; week 45% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 146 of 277 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** idle since 23:28Z (no task in progress)
- **Week:** 45% used, 106.0M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 23:30Z, 0 min before this update
- **Keeper:** last ran 23:30Z, next wake 23:50Z

## Needs attention
- Strict plan check red: 5 problem(s) on tasks still to run (+4 on DONE tasks); first: T195: stored context_pct 10% but it measures 14%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T195 | portals: business billing and statements from the ledger | MERGE_PENDING | 1 | 48 min | 23:28Z |  |
| T085 | portals: business portal home (contact choice, your numbers, | done | 1 | 33 min | 21:43Z |  |
| T253 | privacy, picks and portals: the signed-in business reads a r | done | 1 | 54 min | 20:44Z |  |
| T252 | architecture test: dynamic SQL in a migration is a TD-SEC-10 | done | 1 | 19 min | 19:16Z |  |
| T196 | businesses: verified notice email, sender rules and the addr | done | 2 | 32 min | 18:33Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests / test tests::transi |
| T093 | messaging: vault references in the log, bodies cut at 30 day | done | 1 | 45 min | 17:05Z |  |
| T196 | businesses: verified notice email, sender rules and the addr | check_failed | 1 | 49 min | 15:54Z |  |
| T147 | messaging and businesses: business notices by chosen channel | done | 1 | 39 min | 14:31Z |  |
| T230 | portals: the private feedback page and its storage | done | 1 | 61 min | 13:25Z |  |
| T191 | payments: the unreachable-homeowner refund at 96 hours and i | done | 1 | 56 min | 12:07Z |  |
| T097 | jobs: day-3 and day-14 check-ins, PROBLEM flags, private fee | done | 1 | 51 min | 10:55Z |  |
| T096 | messaging: web-and-email mode, number suspension, one config | done | 1 | 43 min | 09:49Z |  |
| T092 | privacy: 180-day contact purge, holds, photo and message del | done | 1 | 54 min | 08:38Z |  |
| T091 | quotes: photos on cards only after zero retention and all th | done | 1 | 40 min | 07:24Z |  |
| T090 | privacy: photo checks by bytes, re-encode without EXIF or GP | done | 1 | 52 min | 06:25Z |  |
| T194 | portals: the new-device code by text or email and team sign- | done | 1 | 42 min | 05:12Z |  |
| T084 | portals: business one-time sign-in link and 30-day sessions | done | 2 | 24 min | 04:14Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T084 | portals: business one-time sign-in link and 30-day sessions | check_failed | 1 | 29 min | 03:36Z |  |
| T083 | businesses: complaint replies, 3 in 90 days pause, alerts | done | 1 | 76 min | 02:49Z |  |
| T082 | businesses: E-SIGN consent box, welcome text, business STOP | done | 1 | 53 min | 01:13Z |  |
| T193 | businesses: sign-up screens for someone else filling in, the | done | 1 | 55 min | 00:02Z |  |

### Plan and harness merges on main
- 22:37Z Harness: keep the smoke incremental across tasks that add queries; save its cache only from a smoking job (D-1911) (#338)
- 19:48Z Plan: third audit — CI filters, close-time owned-file check, request timeout, confirm money invariants, reconcile retry (D-1866–D-1873) (#331)
- 19:25Z Harness: incremental runner smoke build (stable tree, own target dir, mtime pinning) (D-1890) (#333)
- 15:37Z Plan: second code audit — owners and acceptance for B01–B08 and known gaps (D-1835–D-1845) (#326)
- 05:10Z Plan: repo audit findings — owners and acceptance for A01–A06, A08–A12 (D-1751–D-1761, D-1788) (#316)
- 03:59Z Plan: tests/a11y/fixtures.ts is a shared page-state registry (D-1772) (#314)
- 03:48Z Plan: business-session read of released picks, release recovery tests, production release wiring (D-1739, D-1740) (#313)

## Progress (requirements verified)

**Requirements verified:** 226 of 731 (30.9%) · partial 276 · unstarted 229
**Tasks merged (throughput only):** 146 of 277

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 13 | 11 | 11 | 35 |
| ai | 13 | 11 | 4 | 28 |
| businesses | 10 | 29 | 19 | 58 |
| calls | 10 | 14 | 1 | 25 |
| consent | 13 | 18 | 4 | 35 |
| fees | 18 | 33 | 1 | 52 |
| intake | 19 | 15 | 9 | 43 |
| job-cards | 12 | 8 | 4 | 24 |
| licensing | 31 | 16 | 1 | 48 |
| lifecycle | 20 | 8 | 0 | 28 |
| messaging | 18 | 23 | 4 | 45 |
| ops-recovery | 2 | 4 | 48 | 54 |
| privacy | 3 | 22 | 6 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 19 | 24 | 13 | 56 |
| recurring | 7 | 1 | 3 | 11 |
| security | 9 | 31 | 14 | 54 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 1 | 27 | 28 |
| wording | 2 | 2 | 28 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-07T22:23:43Z (week)): five-hour: 7.5M used, 21.6M left, resets 2026-11-08T01:44:26Z; week: 88.0M used, 106.0M left, resets 2026-11-12T17:00:00Z; projected finish 2026-11-24T19:38:01Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 26 of 93 done
- stage 5: 0 of 42 done
- stage 6: 0 of 26 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T195", "at": "2026-11-07T23:28:57Z", "cache_creation_input_tokens": 546189, "cache_read_input_tokens": 15243110, "output_tokens": 148057, "input_tokens": 352}
