# Ask Hava build status

Updated 2026-11-07T19:16:37Z by runner 49d6fbafca64 (actions).

**Idle since 19:16Z; 143 of 269 done; week 43% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 143 of 269 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** idle since 19:16Z (no task in progress)
- **Week:** 43% used, 108.0M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 19:03Z, 13 min before this update
- **Keeper:** last ran 18:54Z, next wake 19:14Z

## Needs attention
- Strict plan check red: 7 problem(s) on tasks still to run (+7 on DONE tasks); first: T149: stored context_pct 16% but it measures 20%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T252 | architecture test: dynamic SQL in a migration is a TD-SEC-10 | MERGE_PENDING | 1 | 19 min | 19:16Z |  |
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
| T080 | calls: relay-friendly bridging, identity call, code call | done | 1 | 49 min | 22:52Z |  |
| T081 | businesses: sign-up, identity chain from public records, act | done | 1 | 37 min | 21:42Z |  |
| T079 | calls: line closing, staff phones and LINK, email-preferred  | done | 1 | 43 min | 20:51Z |  |
| T078 | calls: never recorded, 45-second connection rule, call logs | done | 1 | 46 min | 19:53Z |  |

### Plan and harness merges on main
- 15:37Z Plan: second code audit — owners and acceptance for B01–B08 and known gaps (D-1835–D-1845) (#326)
- 05:10Z Plan: repo audit findings — owners and acceptance for A01–A06, A08–A12 (D-1751–D-1761, D-1788) (#316)
- 03:59Z Plan: tests/a11y/fixtures.ts is a shared page-state registry (D-1772) (#314)
- 03:48Z Plan: business-session read of released picks, release recovery tests, production release wiring (D-1739, D-1740) (#313)

## Progress (requirements verified)

**Requirements verified:** 224 of 731 (30.6%) · partial 275 · unstarted 232
**Tasks merged (throughput only):** 143 of 269

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 13 | 11 | 11 | 35 |
| ai | 13 | 11 | 4 | 28 |
| businesses | 8 | 28 | 22 | 58 |
| calls | 9 | 15 | 1 | 25 |
| consent | 13 | 18 | 4 | 35 |
| fees | 20 | 31 | 1 | 52 |
| intake | 19 | 15 | 9 | 43 |
| job-cards | 12 | 8 | 4 | 24 |
| licensing | 31 | 16 | 1 | 48 |
| lifecycle | 21 | 7 | 0 | 28 |
| messaging | 18 | 23 | 4 | 45 |
| ops-recovery | 2 | 4 | 48 | 54 |
| privacy | 2 | 23 | 6 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 18 | 25 | 13 | 56 |
| recurring | 7 | 1 | 3 | 11 |
| security | 9 | 31 | 14 | 54 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 1 | 27 | 28 |
| wording | 2 | 2 | 28 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-07T18:53:50Z (week)): five-hour: 10.1M used, 19.0M left, resets 2026-11-07T19:31:51Z; week: 86.0M used, 108.0M left, resets 2026-11-12T17:00:00Z; projected finish 2026-11-24T04:31:44Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 23 of 85 done
- stage 5: 0 of 42 done
- stage 6: 0 of 26 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T252", "at": "2026-11-07T19:16:25Z", "output_tokens": 60130, "cache_read_input_tokens": 3931333, "cache_creation_input_tokens": 219751, "input_tokens": 136}
