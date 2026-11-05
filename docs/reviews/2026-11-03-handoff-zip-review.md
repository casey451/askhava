# Review of Casey's November 3 handoff package

**Reviewed:** November 5, 2026, by the overseer session, with Casey's go to act on it. **Decisions:** D-1494 to D-1499. **Package:** the zip "Ask_Hava_CLAUDE_MASTER_HANDOFF_2026-11-03" (86 files: a `00_CONTROL` folder, numbered files 00 to 61, manifests and two validation scripts). Nothing in it was executed; it was read as data.

## What the package is

A planning bundle Casey assembled from his November 3 voice calls and several outside reviews (ChatGPT audits, a national competitive brief, a SOC 2 gap analysis, war-game proposals). Its control folder says the live repository wins on any conflict, and that its draft task and decision numbers (T244 to T258, D-1492 to D-1502) are placeholders. This review maps each item to the repository as it stands on `main` and says what happens to it.

## Casey's own decisions on November 3 (voice calls, recorded in the package's file 23)

1. **No early-joiner price lock.** His words: "there is no early-joiner price lock. Don't advertise that signing up sooner locks in prices. If prices ever have to go up, early joiners could otherwise be locked in for years." Only the price-lock part of D-465 is cancelled; its refund wording (TERMS-026) and the save-card box at the free win (FEE-036) stay. Applied in this change (D-1494).
2. **National-ready design goal.** The code must work for any market, with Lake Havasu City and Arizona values held as data. The launch market stays open: Phoenix metro if a business partner is secured, Lake Havasu City as the fallback. Phoenix metro is the war-game stress market, Havasu the comparison. Not written as settled anywhere in the spec.
3. **Radius matching.** Each business types the distance it will travel; a job card goes to every business whose radius covers the job's location, across town lines, with licensing and category rules still applied.
4. **Rule 2 cold outreach parked** (his item 9 stayed open on the call; the overseer parks it, D-1496).
5. **Free first jobs stay open** (his item 3): the built FEE-036 rule stays until he picks a branch (D-1497).

The second plan PR (D-1515 to D-1520) adds the tasks for items 2 and 3: T248 (markets as data and each business's service radius) and T249 (radius matching), with T242's national-ready inventory; it also adds the page pick (T246), the first-quote move (T247) and the other schemas' default privileges (T250).

## Disposition

| Package item | Disposition | Reason | Repository evidence |
|---|---|---|---|
| Drop the early-joiner price lock (file 23 decision 3; file 03 approval 20 overridden) | Adopt now | Casey's decision | `docs/spec/fees.md:44` (FEE-040 withdrawn), `app/payments/src/rules.rs:411` (removed by T244) |
| No copy may say joining early locks in prices | Adopt now | Casey's decision | `docs/spec/wording.md:36` (WORD-032, owned by T218) |
| Payments default privileges grant UPDATE and DELETE to the app role (file 21 section 2, its draft "T246" and migration 0098) | Adopt now, as T245 with migration 0098 | Verified on `main` | `migrations/0090_payments_ledger.sql:15` |
| Same default in every other schema | Adopted in PR 2 as T250 (D-1520) | Found while verifying; fixed by a separate change | `migrations/0019_core_db_migrations.sql:17`, `migrations/0079_picks_code.sql:15` |
| National-ready design goal; Phoenix stress market; Havasu fallback (file 22, its draft T253 to T258) | Adopted in PR 2: D-1515, T248, the T242 inventory | Casey's decision; needs tasks | `docs/decisions.md:1054` (D-1339, the Havasu-only scope it replaces) |
| Per-business radius matching (file 23 decision 4) | Adopted in PR 2: T248, T249 (D-1518, D-1519) | Casey's decision; needs tasks | `docs/spec/quotes-pick.md` (matching rules) |
| Per-market launch date for the price lock (file 23 decision 1, draft D-1492) | Declined | Superseded by decision 3 | `docs/spec/fees.md:44` |
| Rule 2 cold-email outreach architecture (files 24, 32) | Declined for now (parked) | Item 9 open; no outreach without Casey's go; Resend's acceptable-use policy bars cold outreach from the transactional account | `BUILD_BRIEF.md:61` |
| Free first jobs, branches A, B and C (files 13, 21) | Input only | Item 3 open | `docs/spec/fees.md:40` (FEE-036 as built) |
| War-game scenarios: TX-086 to TX-105 (files 50, 51) and TX-106 to TX-115 (files 56, 57); TX-001 to TX-085 and the 14 SECWG cases are in package files not copied | Input only, copied | Proposals for T242's review, not tasks | `docs/reviews/war-games-inputs/` |
| 30% early-quote fee discount simulation (files 56, 57; TX-106 to TX-115) | Input only, copied | Proposal; any fee change needs Casey | `docs/reviews/war-games-inputs/`, `docs/spec/fees.md:5` (FEE-001) |
| War-game early-stop and cost control (file 52) | Input only | T243 already holds the war games for Casey's go | `CLAUDE.md:64` |
| ISO 27001, ISO 27701 and SOC 2 Type II certification readiness (files 15, 58 to 61) | Input only; post-launch program | Not a build gate; the build carries ASVS L2, gitleaks, the PII scan, ZAP (T138) and a restore drill (T139) | `BUILD_BRIEF.md:293`, `BUILD_BRIEF.md:298`, `docs/risk-register.md` (R-21) |
| Independent code review for national scale (files 53 to 55) | Input only | Its findings feed PR 2 and T242 | `docs/reviews/pre-war-games-review-brief.md` |
| Build hold while plans change (files 14, 27) | Declined | The runner continues; plan changes land before any task they touch is claimed; Gate P's `build_hold` already exists | `kit/state_schema.json:31` |
| GitHub settings for Casey (file 14: code-owner review, secret scanning, push protection) | Casey's own action | Settings, not code | `docs/governance.md` (E-2) |
| Draft task and decision numbers (T244 to T258, D-1492 to D-1502) | Declined as numbers | The repository numbers by the interim rule (D-1144) | `docs/decisions.md:969` |
| Flaky CSRF test note (file 12) | Input only | No failing run on record to act on | (none) |
| Sales tax follows the job's location (file 23, file 31) | Input only | A planning assumption until the CPA answers; not a legal tax rule | `docs/spec/ops-recovery.md` (money view) |
| The page pick, end to end (file 25 section 3, its draft "T245") | Adopted in PR 2 as T246 (D-1516) | Verified on `main`: the final Pick stores nothing and sends no code | `app/portals/src/pick_flow.rs:214` |
| The first-quote move to Quotes ready (file 25 section 2, its draft "T244") | Adopted in PR 2 as T247 (D-1517) | Verified on `main`: JOB-009 is not built | `app/quotes/CLAUDE.md:55` |

## Files copied into `docs/reviews/war-games-inputs/`

Copied without changes, except a one-line origin note at the top of each Markdown file. The two JSON files have no note inside (JSON has no comments); this table is their note. All four are proposals from the package, dated November 3, 2026, and none is an executed test.

| File | What it is |
|---|---|
| `50_WAR_GAMES_MARKETPLACE_REALITY_EXTENSION.md` | Marketplace stress scenarios (supply, timing, fraud, cross-city) for the war games |
| `51_WAR_GAME_SCENARIOS_MACHINE_READABLE.json` | The same scenarios as data (TX-086 to TX-105) |
| `56_EARLY_QUOTE_INCENTIVE_WAR_GAME_PROPOSAL.md` | WG-06: a 30% fee discount for a quote within 4 hours, as a simulation only |
| `57_EARLY_QUOTE_INCENTIVE_SCENARIOS.json` | WG-06's scenarios as data (TX-106 to TX-115) |

T242 lists this review and the folder under its reads, so the pre-war-games review sees them.

## Open after this review

- Casey: item 3 (free first jobs), item 9 (Rule 2 outreach), the launch market, and the CPA's sales-tax answer.
- The default-privileges pattern in the 13 other schemas (see D-1495): T250 (D-1520).
- PR 2: national-ready market data and radius matching: T248, T249 and T242's inventory (D-1515, D-1518, D-1519).

## Casey's confirmation, November 5

In chat on November 5, 2026, Casey confirmed the national-ready goal before the second plan PR was written:

> "I do want the product to be national ready. We will start in one market but it will expand later"

The launch market stays open (Phoenix metro if a business partner is secured, Lake Havasu City otherwise). D-1515 records the goal; T248 and T249 build the market data, the service radius and radius matching; T242's review lists every Lake Havasu City or Arizona rule still to be turned into per-market data, as approved-enhancement candidates before the war games.
