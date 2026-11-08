# Ask Hava build status

Updated 2026-11-08T12:00:33Z by runner 0893cb8c3043 (actions).

**Idle since 12:00Z; 154 of 282 done; week 50% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 154 of 282 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** idle since 12:00Z (no task in progress)
- **Week:** 50% used, 94.0M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 11:24Z, 36 min before this update
- **Keeper:** last ran 11:09Z, next wake 11:29Z

## Needs attention
- Strict plan check red: 16 problem(s) on tasks still to run (+12 on DONE tasks); first: T255: stored context_pct 25% but it measures 30%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T149 | businesses: sign-up deliverability check with a sample card, | MERGE_PENDING | 1 | 43 min | 12:00Z |  |
| T197 | picks: Confirm by email alone with the 4-business-hour windo | done | 1 | 41 min | 08:17Z |  |
| T148 | portals: secure email action links for every former text key | done | 1 | 56 min | 06:57Z |  |
| T089 | Rate limits per phone, IP and business; brute-force stops | done | 1 | 44 min | 05:26Z |  |
| T088 | Authorization matrix over every route and actor; token isola | done | 1 | 45 min | 04:25Z |  |
| T291 | core and checks: one brand file the app and the build's chec | done | 1 | 44 min | 03:16Z |  |
| T087 | portals: homeowner portal (phone and code), own data, no for | done | 1 | 46 min | 02:04Z |  |
| T086 | portals: owner password, passkey or TOTP, recovery codes, 12 | done | 1 | 54 min | 00:56Z |  |
| T195 | portals: business billing and statements from the ledger | done | 1 | 48 min | 23:28Z |  |
| T085 | portals: business portal home (contact choice, your numbers, | done | 1 | 33 min | 21:43Z |  |
| T253 | privacy, picks and portals: the signed-in business reads a r | done | 1 | 54 min | 20:44Z |  |
| T252 | architecture test: dynamic SQL in a migration is a TD-SEC-10 | done | 1 | 19 min | 19:16Z |  |
| T196 | businesses: verified notice email, sender rules and the addr | done | 2 | 32 min | 18:33Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests / test tests::transi |
| T093 | messaging: vault references in the log, bodies cut at 30 day | done | 1 | 45 min | 17:05Z |  |
| T196 | businesses: verified notice email, sender rules and the addr | check_failed | 1 | 49 min | 15:54Z |  |
| T147 | messaging and businesses: business notices by chosen channel | done | 1 | 39 min | 14:31Z |  |
| T230 | portals: the private feedback page and its storage | done | 1 | 61 min | 13:25Z |  |
| T191 | payments: the unreachable-homeowner refund at 96 hours and i | done | 1 | 56 min | 12:07Z |  |

### Plan and harness merges on main
- 10:55Z Harness: retry release downloads; backup job's coverage tools best-effort (D-1986) (#352)
- 07:39Z harness: chain the next cycle at once after the first smoke timeout on a head (D-1970) (#349)
- 01:33Z Plan: brand as configuration (T291–T295, D-1932–D-1933) (#342)
- 22:37Z Harness: keep the smoke incremental across tasks that add queries; save its cache only from a smoking job (D-1911) (#338)
- 19:48Z Plan: third audit — CI filters, close-time owned-file check, request timeout, confirm money invariants, reconcile retry (D-1866–D-1873) (#331)
- 19:25Z Harness: incremental runner smoke build (stable tree, own target dir, mtime pinning) (D-1890) (#333)
- 15:37Z Plan: second code audit — owners and acceptance for B01–B08 and known gaps (D-1835–D-1845) (#326)

## Progress (requirements verified)

**Requirements verified:** 234 of 731 (32.0%) · partial 289 · unstarted 208
**Tasks merged (throughput only):** 154 of 282

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 13 | 13 | 9 | 35 |
| ai | 14 | 10 | 4 | 28 |
| businesses | 13 | 31 | 14 | 58 |
| calls | 11 | 13 | 1 | 25 |
| consent | 14 | 17 | 4 | 35 |
| fees | 17 | 34 | 1 | 52 |
| intake | 19 | 15 | 9 | 43 |
| job-cards | 12 | 8 | 4 | 24 |
| licensing | 33 | 14 | 1 | 48 |
| lifecycle | 20 | 8 | 0 | 28 |
| messaging | 16 | 25 | 4 | 45 |
| ops-recovery | 2 | 4 | 48 | 54 |
| privacy | 3 | 25 | 3 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 23 | 23 | 10 | 56 |
| recurring | 7 | 1 | 3 | 11 |
| security | 9 | 37 | 8 | 54 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 1 | 27 | 28 |
| wording | 1 | 5 | 26 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-08T11:09:38Z (week)): five-hour: 1.9M used, 27.2M left, resets 2026-11-08T13:17:02Z; week: 100.0M used, 94.0M left, resets 2026-11-12T17:00:00Z; projected finish 2026-11-25T07:03:30Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 34 of 98 done
- stage 5: 0 of 42 done
- stage 6: 0 of 26 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T149", "at": "2026-11-08T12:00:21Z", "input_tokens": 618, "cache_creation_input_tokens": 770063, "cache_read_input_tokens": 38803131, "output_tokens": 216622}
