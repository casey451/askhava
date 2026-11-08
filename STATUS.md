# Ask Hava build status

Updated 2026-11-08T21:06:29Z by runner 195ecd29697f (actions).

**Idle since 20:51Z; 158 of 283 done; week 56% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 158 of 283 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** idle since 20:51Z (no task in progress)
- **Week:** 56% used, 86.0M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 21:05Z, 0 min before this update
- **Keeper:** last ran 20:52Z, next wake 21:12Z

## Needs attention
- Strict plan check red: 24 problem(s) on tasks still to run (+20 on DONE tasks); first: T255: stored context_pct 25% but it measures 32%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T098 | Journey J1 (homeowner) and its variants, keyboard-only, with | MERGE_PENDING | 1 | 50 min | 20:51Z |  |
| T254 | server and picks: the production release (paid or free, mint | done | 1 | 84 min | 19:24Z |  |
| T150 | picks and quotes: EMERGENCY card and pick emails with the em | done | 1 | 54 min | 16:04Z |  |
| T198 | messaging and portals: daily open-cards email, missed-card a | done | 1 | 71 min | 14:02Z |  |
| T149 | businesses: sign-up deliverability check with a sample card, | done | 1 | 43 min | 12:00Z |  |
| T197 | picks: Confirm by email alone with the 4-business-hour windo | done | 1 | 41 min | 08:17Z |  |
| T148 | portals: secure email action links for every former text key | done | 1 | 56 min | 06:57Z |  |
| T089 | Rate limits per phone, IP and business; brute-force stops | done | 1 | 44 min | 05:26Z |  |
| T088 | Authorization matrix over every route and actor; token isola | done | 1 | 45 min | 04:25Z |  |
| T291 | core and checks: one brand file the app and the build's chec | done | 1 | 44 min | 03:16Z |  |
| T087 | portals: homeowner portal (phone and code), own data, no for | done | 1 | 46 min | 02:04Z |  |
| T086 | portals: owner password, passkey or TOTP, recovery codes, 12 | done | 1 | 54 min | 00:56Z |  |
| T195 | portals: business billing and statements from the ledger | done | 1 | 48 min | 23:28Z |  |
| T085 | portals: business portal home (contact choice, your numbers, | done | 1 | 33 min | 21:43Z |  |

### Plan and harness merges on main
- 20:26Z Plan: 3-second AI reply budget, AI waits hold no DB connection, war-games latency (D-2038) (#362)
- 18:33Z Harness: run the start-routine smoke before the session (D-2028) (#359)
- 15:06Z Harness: warm the smoke build before the session (D-2007) (#356)
- 10:55Z Harness: retry release downloads; backup job's coverage tools best-effort (D-1986) (#352)
- 07:39Z harness: chain the next cycle at once after the first smoke timeout on a head (D-1970) (#349)
- 01:33Z Plan: brand as configuration (T291–T295, D-1932–D-1933) (#342)
- 22:37Z Harness: keep the smoke incremental across tasks that add queries; save its cache only from a smoking job (D-1911) (#338)

## Progress (requirements verified)

**Requirements verified:** 273 of 732 (37.3%) · partial 257 · unstarted 202
**Tasks merged (throughput only):** 158 of 283

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 13 | 13 | 9 | 35 |
| ai | 12 | 12 | 5 | 29 |
| businesses | 25 | 23 | 10 | 58 |
| calls | 12 | 12 | 1 | 25 |
| consent | 15 | 16 | 4 | 35 |
| fees | 17 | 34 | 1 | 52 |
| intake | 23 | 11 | 9 | 43 |
| job-cards | 14 | 8 | 2 | 24 |
| licensing | 33 | 14 | 1 | 48 |
| lifecycle | 20 | 8 | 0 | 28 |
| messaging | 18 | 23 | 4 | 45 |
| ops-recovery | 2 | 5 | 47 | 54 |
| privacy | 6 | 22 | 3 | 31 |
| quiet-hours | 8 | 1 | 0 | 9 |
| quotes-pick | 33 | 13 | 10 | 56 |
| recurring | 7 | 1 | 3 | 11 |
| security | 14 | 32 | 8 | 54 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 1 | 27 | 28 |
| wording | 1 | 5 | 26 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-08T19:36:24Z (week)): five-hour: 9.7M used, 19.4M left, resets 2026-11-09T00:24:45Z; week: 108.0M used, 86.0M left, resets 2026-11-12T17:00:00Z; projected finish 2026-11-26T03:20:42Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 38 of 98 done
- stage 5: 0 of 42 done
- stage 6: 0 of 27 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T098", "at": "2026-11-08T20:51:03Z", "cache_creation_input_tokens": 532995, "cache_read_input_tokens": 27890778, "input_tokens": 503, "output_tokens": 168512}
