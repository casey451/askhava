# Ask Hava test design

Written in kit step K5 (`BUILD_BRIEF.md:599`; `sources/02_build_plan/Kit_Plan_Before_Coding.md:34`). It says what is tested, at which layer, against which requirement, and who writes and runs it. Tests are designed per **behavior, boundary, failure and invariant**, never one per requirement (stress-test F17, `sources/04_reviews/ChatGPT_Build_StressTest_Report.md:76`). Every requirement ID in `docs/spec/` appears in at least one entry below. The twin's warnings and the hard-failing `Twin` check are in `docs/twin-warnings.md`. Decisions are D-101 to D-110 in `docs/decisions.md`.

---

## 1. Rules for every test

1. **Assert what the outside world sees.** A test passes on what the fake carrier delivered, what Stripe's mock recorded, what the page shows, what the database and audit log hold; never on "function X was called". This is how the twin's checks work (`sources/01_spec/hhq_twin/README.md:53`) and why they catch bugs the app's own labels hide.
2. **Name the rule.** Each test carries `@pytest.mark.td("TD-FEE-01")` and `@pytest.mark.spec("FEE-001", ...)`. The test name says the case ("exactly 500.00 is the 75 tier"), not the function.
3. **Boundaries both sides.** Every number in the spec is tested at, just under and just over the edge (amounts to the cent, times to the second).
4. **Failures are first-class.** Every outside call has at least one test where the vendor errors, times out, answers twice or answers late.
5. **Fake data only** (`BUILD_BRIEF.md:66`; PRIV-019). Fixtures come from one generator module: names from a fixed fake list, phones in the fictional `555-0100` to `555-0199` range under area codes in the quiet-hours table (for example `+1 928 555 0142`), emails at `example.com`, streets like "1 Test Way" (D-109). No real job text, photo or person is ever pasted in.
6. **Time is injected.** Tests use the `core` clock (`docs/architecture.md` section 5) and advance it; no test sleeps. Timers fire by running due tasks after advancing the clock.
7. **Deterministic in `Ci`.** Hypothesis runs derandomized in `Ci` and randomized with more examples in the nightly run; any failing example found at night is added as an explicit `@example` (D-101).
8. **Tests are owned.** A test changes only with its owning task or through a spec change logged in `docs/decisions.md` (`BUILD_BRIEF.md:69`). A wrong expected value is fixed in the spec first, then the test.
9. **Reviewers judge assertions, not counts.** The fresh-eyes reviewer (`BUILD_BRIEF.md:249`) checks that each test would fail if the rule it names were broken. A test with no assertion on an outside effect, or that passes with the rule deleted, is sent back.

## 2. Layers

Per `BUILD_BRIEF.md:467`–`BUILD_BRIEF.md:481`.

| Layer | Folder | What | Runs in | Gate |
|---|---|---|---|---|
| Unit | `tests/unit/<module>/` | Pure rules in each module's `rules.py`: fees, the 4-month rule, eligibility, quiet hours, keyword matching, scrubber, state transitions | Every PR | `Ci` |
| Property | `tests/property/` | Hypothesis: invariants over generated inputs; stateful machine for the job lifecycle | Every PR (derandomized); nightly randomized | `Ci` |
| Deliberate break (mutation) | `tests/mutation/` | Named mutants that must turn tests red (section 4), plus mutmut | Named mutants: every PR touching their files and nightly; mutmut: nightly | `Ci` |
| Integration | `tests/integration/` | Real PostgreSQL 16; job queue with two workers; outbox and inbox never double-send; migrations; module boundaries | Every PR | `Ci` |
| Contract | `tests/contract/<vendor>/` | Each stand-in against the vendor's published API (request and response shapes, errors, webhook signatures); in Phase 2 the same tests against the sandbox (`BUILD_BRIEF.md:191`) | Every PR; Phase 2 with `--sandbox` | `Ci` |
| Twin | `twin/gate.py` | The twin as oracle; any non-waived invariant failure fails (`docs/twin-warnings.md` section 6). Until stage 1 links the twin to the app's rules, `Twin` checks only the stand-alone twin, not `app/`; until K8 builds the gate it is the informational smoke run | Every PR; nightly | `Twin` |
| End to end | `tests/e2e/` | The four journeys (section 5) in Playwright against the app with stand-ins | Every PR | `Ci` |
| Security | `tests/security/` | Authorization matrix, token isolation, brute force, webhook forgery, replay, headers, injection; plus gitleaks, the PII scanner, bandit and pip-audit as `Ci` steps | Every PR | `Ci` |
| Accessibility | `tests/a11y/` | axe-core at 320, 768 and 1280px on every page and state; keyboard-only journeys; reflow, zoom, spacing (section 6) | Every PR | `Ally` |
| Documents | `tests/docs/` | Scans of templates, pages and drafted documents for required and banned wording; terms-match-code checks | Every PR | `Ci` |
| Recovery | `tests/recovery/` | Local restore and replay in Phase 1; the Railway drill with switchover in Phase 2 | Nightly; Phase 2 | Phase 2 |

The kit's own tests (`tests/test_trace_spec.py`, `tests/test_check_catalog.py`) stay at the top of `tests/`.

**Coverage** (`BUILD_BRIEF.md:481`): `Ci` measures branch coverage with pytest-cov. Floors live in `tests/coverage_floor.json`: the total, and 95% for `app/payments`, `app/catalog` fee rules, `app/privacy` and `app/licensing` (FEE-034, PRIV-022, LIC-033). A PR may raise a floor; lowering one needs a decision.

## 3. Who designs, writes and runs

"Opus designs, Sonnet writes, Haiku runs" (`sources/02_build_plan/Kit_Plan_Before_Coding.md:34`; `BUILD_BRIEF.md:496`–`BUILD_BRIEF.md:498`).

| Work | Model |
|---|---|
| This design; adding entries when a spec line changes; matching worked examples (section 7.2); mutant lists; reviewing test PRs for money, privacy, licensing, security and recovery; triaging surviving mutants and new twin failures | **Opus** |
| Writing the tests inside each build task, **before** its code (`sources/02_build_plan/Build_Plan.md:271`); fixtures and generators; stand-in contract tests; named mutants for the code the task writes; Playwright journeys | **Sonnet** |
| Running suites, the twin gate, the nightly mutmut and randomized Hypothesis runs, axe scans; returning a short summary of failures to the session (`sources/02_build_plan/Build_Plan.md:189`) | **Haiku** |

K6 copies each entry's ID into the task that owns its module, so every build task names the TD entries its tests must cover.

## 4. Deliberate-break tests (mutation)

`BUILD_BRIEF.md:472` requires that breaking the 4-month rule or the license filter turns tests red; the Kit Plan makes it K5's done criterion (`sources/02_build_plan/Kit_Plan_Before_Coding.md:34`). K5 extends the same treatment to fee tiers, consent and opt-out, and quiet hours (D-108).

**Two mechanisms (D-108):**

- **Named mutants** (`tests/mutation/mutants.toml`, protected): each entry gives a file, an exact text replacement and the test files that must fail. `scripts/run_named_mutants.py` (K8) applies each one to a temporary copy, runs those tests, and fails `Ci` if any mutant survives. Run on every PR that touches a listed file, and nightly in full. The task that writes the code writes its mutants; the list below is the minimum.
- **mutmut** (nightly) on `app/payments/rules.py`, `app/catalog/rules.py`, `app/licensing/rules.py`, `app/quotes/rules.py` (matching), `app/messaging/rules.py` (quiet hours, opt-out words) and `app/intake/rules.py` (consent, cancel intent). Every surviving mutant is either killed by a new test or listed in `tests/mutation/equivalent.toml` with the reason, after an Opus review.

| Rule | Named mutants (each must turn a test red) | Killed by |
|---|---|---|
| **4-month rule** | multiplier 4→3; 4→5; 4→2 (the superseded rule); recurring quote charged at face value; weekly converted ×4 instead of ×52÷12; every-two-weeks ×2 instead of ×26÷12; yearly ÷4 instead of ÷12; a one-time job in a recurring category charged at 4 months (D-037); a period accepted in a non-recurring category | TD-FEE-01, TD-FEE-03 (W11 to W20, W30, W32; REC-003's $150 → $600 → $75) |
| **Fee tiers** ($25 / $75 / $199) | `< 500` → `<= 500`; `<= 1500` → `< 1500`; any tier amount changed (including to a superseded fee); rounding down → rounding half up (D-033); range basis from the high or low end instead of the midpoint; trip-fee quote priced from its amount instead of the category's set tier; the AI "major" flag raising the tier; a repeat customer charged; a second free first win for the same entity key | TD-FEE-01, TD-FEE-02, TD-FEE-04, TD-FEE-08, TD-FEE-09 (W02 to W05, W08, W21, W22, W23, W24, W30, W31) |
| **License filter** | "covers **every** candidate job type" → "covers **any**"; the ticked-job-type check dropped; a sub-license treated as its full class (R-39R as R-39, LIC-035); `handyman_allowed` ignored; handyman limit `>= 1000` → `> 1000`; limit judged on the midpoint instead of the top; a card sent when the latest daily check is not Active; the license end date ignored; the gas keyword override removed (AI-012); a permit-type job allowed to a handyman; the license check at pick skipped; plain-keyword job types dropped after a trade answer (the twin's C1 bug, D-102) | TD-LIC-02, TD-LIC-05, TD-LIC-08, TD-LIC-09, TD-INT-03, TD-PICK-09; matching examples M01 to M16 |
| **Consent and opt-out** | one opt-out word removed from the list (each of the 11 in turn); sole-word match → contains-word ("can stop by at 3" opts out); an opt-out closing a job (D-005); bare "cancel" closing a job; the send path skipping the opt-out check; a second opt-out confirmation; a 21610 failure retried; a non-consent text allowed as the first message; a job card sent before consent | TD-CON-01, TD-CON-03 to TD-CON-06, TD-JOB-05 |
| **Quiet hours** | end 20:00 → 21:00 (the superseded v9.3 window); start 08:00 → 07:00; only Arizona time checked; only the recipient's zone checked; an area code not in the table texted instead of refused (D-100); the D-025 all-US-zones window replaced by Arizona time; a non-urgent kind added to the night exceptions; the 15-minute direct-reply window widened; retries skipping the check (QH-008); 20:00:00 allowed | TD-QH-01, TD-QH-02 |

The twin's own rules get the same treatment inside the gate's self-tests (`docs/twin-warnings.md` section 6.5), because the current twin cannot see a broken license table.

## 5. The four end-to-end journeys

Playwright (Python) against `web` and `worker` with every adapter on its stand-in, a real local PostgreSQL, and a test-only control router (advance the clock, read the fake carrier's delivered messages, read stripe-mock's records). The control router is mounted only when `APP_ENV` is `test` or `ci`, and TD-SEC-05 checks it is absent in live configuration. Each journey also runs keyboard-only and with axe at three widths (section 6), so the same steps feed `Ci` and `Ally` (A11Y-021). Every journey ends with the same **closing checks**: no duplicate outbound message or charge, the ledger reconciles to stripe-mock to the cent, every status change has an audit row, and no sentinel contact detail appears outside the vault (TD-PRIV-02).

**J1 Homeowner** (INT, CON, QH, CARD, PICK, CALL, JOB).
1. A fake homeowner texts "AC blowing warm air" from `+1 928 555 0142`; the first reply is the full consent text (CON-001); "ok" is consent (CON-005).
2. Both stand-in readings say electrical; the keyword rules say heating/AC; the trade question is asked and answered "2"; the stored job type is `ac_repair` (TD-INT-03, the twin's six warnings as a regression).
3. ZIP 86403; the job opens; cards go to every eligible business (five seeded: R-39, R-39R that ticked AC repair, R-39 that ticked only heating, one paused, one in Kingman): exactly the first two get cards (CARD-001).
4. Two quotes arrive; the first-quote text goes out (TXT-016); the quote page shows both in arrival order with "Every eligible quote shown, in the order received." (WORD-005), announced by `aria-live` (A11Y-014).
5. Choose quote 2 → review step → "Go back" → choose again → code: a wrong code, then the right one (PICK-013, PICK-017); contact choice: masked call (PICK-018); focus moves to the result heading.
6. The business confirms; stripe-mock succeeds; the webhook arrives twice (SEC-017); details go to the business once, without the phone number (PICK-022, PICK-023).
7. The business calls through the masked number for 50 seconds (CALL-007); day 3 "DONE" → Done → one private feedback request (JOB-017); the other quoter gets "This job is no longer active." once (PICK-026).

Variants: Spanish homeowner (every text from the Spanish template, TXT-002); web chat start (CON-025); STOP after quotes arrive: no further texts, the job continues, the pick code goes by email (CON-012, PICK-029); emergency words first ("I smell gas"): safety text, then consent, no card until "safe" (INT-005).

**J2 Business** (BIZ, LIC, CON, FEE, PICK).
1. Sign-up for an R-37 plumbing business on a fixture ROC record; E-SIGN box starts unticked (CON-019); license class read from the record and read back (LIC-006, LIC-027).
2. Identity code to the record's phone (BIZ-002); Casey approves in the owner portal; activation needs all three (BIZ-016); welcome text before the first card (CON-020).
3. A card arrives; a text quote containing a phone number bounces (PICK-007); a portal quote of $1,500.00 is accepted.
4. The homeowner picks it; Confirm shows a review step with the $75 fee and "Go back" (PICK-021, W04); card entry through the Stripe test element (keyboard only; the frame itself is a Gate B manual check, A11Y-022); authorization text and version stored (FEE-015).
5. Charged once; details appear in the portal; the statement CSV equals the ledger (BIZ-015, FEE-030).

Variants: a handyman quote of $1,000.00 is refused with the limit explanation (LIC-019, W26); a free first win records $0 with the waived $75 (FEE-012, W24); a business joining after day 90 has no free win (FEE-010).

**J3 Operator (Casey)** (SEC, OPS, FEE, LIC).
1. Owner sign-in with password and a test TOTP secret (SEC-005); a session over 12 hours is refused.
2. Pause the plumbing trade switch: a new plumbing request gets no card; unpause (OPS-003, CARD-016).
3. Refund a charge through the review step; one Stripe refund, one ledger row (FEE-026, FEE-028).
4. Switch fee mode to flat: the change is logged and the business must accept a new authorization before it can confirm (FEE-007, D-072).
5. Raise a High alert (the stand-in delivery rate below 95% for 30 minutes, OPS-004); advance 24 hours without acknowledging: safe mode pauses charges, releases and outbound texts (OPS-001); acknowledge, resume.
6. The audit log viewer shows every action above with its actor (SEC-006, OPS-010).

**J4 Vendor outage** (TXT, FEE, AI, OPS, WEB).
1. Fake Twilio starts failing sends: texts retry 3 times over 30 minutes, then the quote link goes by email and appears in the portal (TXT-014); with no email on file, Casey gets a call-back alert (TXT-015).
2. Fake Twilio reports the number suspended: all texting pauses; the site banner appears with `role="status"` (WEB-007, A11Y-018); a new request through web chat still works (WEB-009); no other number is used (TXT-022); Casey's call list fills (TXT-023).
3. stripe-mock is unreachable when the business confirms: per-attempt idempotency keys (FEE-016); after 4 hours the pick is released and the next quote offered (FEE-018); Stripe comes back and a late success arrives: refunded once, Casey told (FEE-021).
4. The stand-in AI is down: one "we have your message" text, retry every 30 minutes (INT-021); "sparks" in a message still gets the safety text within 5 minutes (INT-003).
5. Everything recovers; the closing checks pass, and the nightly reconcile finds nothing unmatched (FEE-029).

## 6. Accessibility checks

| Check | How | Covers | Gate |
|---|---|---|---|
| axe-core on every page and state (empty, filled, error, after pick, banner on, Spanish) at 320, 768 and 1280px; any violation fails | `tests/a11y/` injects the vendored axe-core 4.14.0 through Playwright; the page list is generated from the route table so a new page can't be skipped | A11Y-001, A11Y-002 | `Ally` |
| One `h1`, landmarks, skip link, `lang` on page and inline passages, ARIA only where needed | DOM assertions per page | A11Y-003, A11Y-004 | `Ally` |
| Keyboard-only journeys J1 to J4: tab order, visible focus not hidden behind the sticky header (focused element's box inside the viewport and not under the header), no traps | Playwright keyboard driver; focus-visible style asserted on each focused element | A11Y-005, A11Y-021 | `Ally` |
| Target size at least 24×24 CSS px; no drag-only control | Bounding boxes of every interactive element | A11Y-006 | `Ally` |
| Contrast | axe colour-contrast rule plus a token check of the CSS palette (4.5:1 text, 3:1 large text and UI) | A11Y-007 | `Ally` |
| Reflow at 320px and 400% zoom with no sideways scroll; data tables scroll in a labelled focusable region | `scrollWidth <= clientWidth` per page; table wrapper attributes | A11Y-008 | `Ally` |
| Text spacing override (WCAG 1.4.12 values) breaks nothing; `prefers-reduced-motion` stops motion | Inject the spacing stylesheet and re-check overflow and clipping; emulate reduced motion | A11Y-009 | `Ally` |
| Forms: visible labels, instructions before inputs, error summary linked and announced, nothing asked twice, paste and autofill allowed | DOM assertions; submit invalid forms; check `autocomplete` and that no `onpaste` blocks | A11Y-010, A11Y-011, A11Y-012, A11Y-013, PICK-014 | `Ally` |
| New quotes announced politely; focus to the result heading after a pick | `aria-live="polite"` region updates; `document.activeElement` | A11Y-014 | `Ally` |
| Time limits only on codes, sign-in links and owner sessions; warning before expiry; one-step renew; entered data kept; quote windows as plain text | Advance the clock to one minute before expiry; assert the warning and the renew control; no countdown element on the quote page | A11Y-015, A11Y-016 | `Ally` |
| Help in the same place on every page; status and alerts with the right role | Same landmark position across pages; role attributes | A11Y-017, A11Y-018 | `Ally` |
| Emails: real text, plain-text part, meaningful link text | Fake Resend payloads: a `text/plain` part exists; no image-only body; link text is not "click here" | A11Y-019 | `Ci` |
| Statement page content | Page assertions | A11Y-020 | `Ally` |
| Error prevention for money and agreements | Review steps with "Go back" before the final pick and Confirm | PICK-017, PICK-021 | `Ally` |
| Manual: VoiceOver on iPhone through the homeowner flow and the business card entry, including Stripe's frame | Script in `docs/a11y.md`; Casey at Gate B; Claude records the result | A11Y-022 | Gate B |

## 7. Test catalog

Kind: **B** behavior, **E** boundary (edge), **F** failure, **I** invariant. Layer: U unit, P property, M mutation, In integration, C contract, E2E, S security, A accessibility, D documents, R recovery. "W##" are rows of `docs/catalog/worked_examples.yaml`; "M##" are the matching examples in section 7.2.

### 7.1 Fees and the 4-month rule (`payments`, `catalog`)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-FEE-01 | E | U, P, M | FEE-001, FEE-002, FEE-008, FEE-034, REC-003 | Every W row is one parametrized test. Edges: 499.99 → $25, 500.00 → $75, 1,500.00 → $75, 1,500.01 → $199 (W02 to W05). Property: the fee is always 25, 75 or 199, never decreases as the basis rises, and the charged amount equals the tier exactly (no tax added, no percentage) |
| TD-FEE-02 | B | U, P | FEE-003, FEE-004, REC-005 | Range → midpoint (W08 to W10, W18); trip fee → the category's set tier (W21, W22); fractions of a cent rounded down before the tier (W30, W31; D-033). Property: the basis of a range lies between its ends |
| TD-FEE-03 | B | U, P, M, D | REC-001, REC-002, REC-004, REC-006 | Monthly ×4 (W11, W12); weekly ×52÷12 ×4 (W13); every two weeks (W14); quarterly (W15); yearly (W16, W17, W30); one-time job in a recurring category at face value (W32); a period in a non-recurring category is an error. Property: monthly *m* → basis is 4*m* rounded down to the cent. Document scan: no code, template, setting or copy uses another month multiplier for recurring services |
| TD-FEE-04 | B | U, In | FEE-005 | The tier is fixed at the pick from the chosen quote; the AI's major flag set or unset gives the same fee; a later edit to another quote changes nothing |
| TD-FEE-05 | I | In | FEE-006, JOB-016 | Charged at Confirm; a day-3 PROBLEM reply about a much higher final bill changes no ledger row |
| TD-FEE-06 | B | In | FEE-007 | Fee-mode switch is logged; flat mode defaults to the category's set tier; a business without the new authorization can't confirm (D-072) |
| TD-FEE-07 | I | P, S | FEE-009 | No flow creates a Stripe customer or charge for a homeowner; no homeowner-realm route reaches `payments` |
| TD-FEE-08 | E | U, In | FEE-010, FEE-011, FEE-012 | Joined 89 days 23:59 after launch → free win; exactly 90 days → none; a second account with the same ROC number, city-list entity or card fingerprint → charged; $0 ledger row with the waived amount (W24) |
| TD-FEE-09 | B | In | FEE-013 | Same homeowner, same business, another property → $0, free win not used (W23); after the 180-day purge → charged |
| TD-FEE-10 | B | In, C | FEE-014, FEE-015, FEE-036 | No card at sign-up; SetupIntent through stripe-mock; authorization text, version and time stored; Confirm needs a card and an accepted authorization; a bank-debit method is refused |
| TD-FEE-11 | F | In | FEE-016, FEE-017, FEE-018, FEE-019, FEE-020 | Keys `pick-<id>-try1`, `try2`; a retry after the job moved on charges nothing; Stripe down 4 hours → pick released; no webhook in 30 minutes → Stripe asked directly; a decline at 7:30 p.m. gets its 1 business hour as 30 minutes that evening plus 30 minutes from 8:00 a.m. |
| TD-FEE-12 | F | In | FEE-021 | Payment succeeds after the job moved on → refunded exactly once, Casey alerted |
| TD-FEE-13 | E | In | FEE-022, FEE-023, FEE-024, FEE-025, FEE-035 | Two attempts 3:00:00 apart within 72 hours, no 45-second call → refund decided at 96 hours; attempts 2:59:59 apart count as one; a 44-second answered call is not a connection, 45 seconds is; cancel before a connection → refund, after → none; a charge after a cancel → refunded; no call in 72 hours → no refund (D-047) |
| TD-FEE-14 | I | In, P | FEE-026 | Any refund trigger replayed or duplicated → one refund; a failed refund retries hourly with an alert; a disputed charge's refund is blocked and flagged |
| TD-FEE-15 | B | In | FEE-027 | A dispute webhook (twice) → one ledger dispute row of the amount plus $15, an alert with audit evidence |
| TD-FEE-16 | I | P, In | FEE-028, FEE-029, FEE-030, FEE-032 | Random sequences of charges, refunds and disputes against stripe-mock: the ledger matches Stripe to the cent; the app role can't update or delete a ledger row (D-092); an unknown Stripe charge is auto-matched or auto-refunded (always refunded on a closed job); statements and the money view come only from the ledger; only `payments` imports the payments adapter (architecture test) |
| TD-FEE-17 | B | In | FEE-031 | A receipt email per charge; the charge description names the request number |
| TD-FEE-18 | E | U | FEE-033 | 3 of the last 10 quotes with a basis from $475.00 to $499.99 (or $1,425.00 to $1,499.99) → flagged; 2 of 10 → not; $474.99 doesn't count; no automatic action |
| TD-FEE-19 | B | D | REC-007, REC-008, REC-009, TERMS-005, TERMS-019 | No partial-refund rule for recurring services exists in code or terms; pricing copy names four months with the $150 pool example in the $75 row; the business terms' refund triggers are compared with the refund rule constants (a change to one without the other fails); REC-008 is section 4 |

### 7.2 Matching and licensing (`quotes`, `licensing`, `catalog`)

**Matching worked examples (D-110).** The fee examples are an oracle the app didn't write (`sources/02_build_plan/Build_Plan.md:35`); matching needs the same, or the app and the twin can share one mistake (F07). An Opus task writes `docs/catalog/matching_examples.yaml` before any routing code, checked by `scripts/check_catalog.py`. The minimum set:

| # | Job (category, job type) | Business | Card? |
|---|---|---|---|
| M01 | hvac_repair, AC repair | R-39, ticked AC repair | Yes |
| M02 | hvac_repair, furnace repair | R-39R (AC only), ticked everything | No (LIC-035) |
| M03 | plumbing_repair, drain clearing | R-37R (drains only) | Yes |
| M04 | plumbing_repair, water heater replacement | handyman | No (permit-type, LIC-022) |
| M05 | plumbing_repair, faucet repair | handyman | Yes; quote must be under $1,000 |
| M06 | appliance_repair, gas dryer | handyman | No (gas, LIC-021) |
| M07 | electrical, outlet repair | handyman | No (`handyman_allowed: false`) |
| M08 | pool_repair, pump repair | R-6 | Yes |
| M09 | security_cameras, camera install | handyman | Yes, under $1,000 with no permit (LIC-014) |
| M10 | alarm_systems, install | R-67 without the alarm certificate | No (LIC-013) |
| M11 | pest_control, termites | ROC licensee without the pest license | No (LIC-012) |
| M12 | septic_pumping | ADEQ hauler | Yes (LIC-016) |
| M13 | local_moving, Lake Havasu City to Phoenix | city-list mover | Yes |
| M14 | local_moving, Lake Havasu City to Las Vegas | any | Refused at intake (LIC-017) |
| M15 | Candidates {AC repair, furnace repair} | R-39R that ticked AC repair | No: the license must cover every candidate |
| M16 | AC repair, business in Kingman only | R-39 | No (town) |

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-LIC-01 | B | U, In | LIC-001, LIC-002, LIC-003, LIC-004, LIC-005, AI-017 | The catalog loads each category's fields; an unconfirmed category and garage doors and junk removal are off; estate cleanout and donation pickup on; the eight not-offered kinds get the not-offered reply; turning a category off is a logged settings change |
| TD-LIC-02 | I | U, P, M | CARD-001, CARD-002, CARD-003, LIC-035 | M01 to M16 as parametrized tests. Property: for generated businesses and jobs, a card goes exactly when the business is active, verified, serves the town, its license covers every candidate job type and it ticked one; 50 qualified businesses → 50 cards; neighbouring towns follow the same rule |
| TD-LIC-03 | E | U | CARD-004 | Nobody ticked the job type → only licensed businesses with "jobs nobody else covers" on get it; if one business ticked it, overflow doesn't apply |
| TD-LIC-04 | B | In, C | LIC-006, LIC-007, LIC-010, LIC-027, CARD-015 | License class from the record; daily list import; exactly one throttled live lookup at the pick; list contacts are never messaged; the sign-up read-back lists what the license covers |
| TD-LIC-05 | F | In | LIC-008, LIC-009 | Lookup down, CAPTCHA, changed page → "unknown", status kept up to 24 hours then paused; a misread never changes a verified status; an end or bond-cancel date pauses at 00:00 Arizona time that day, not at 23:59 the day before |
| TD-LIC-06 | B | U | LIC-011, LIC-012, LIC-013, LIC-014, LIC-015, LIC-016 | Each agency-licensed category reaches only its licensees (M10 to M12); a pre-emergent weed job goes to pest licensees only; a no-license business must match the city list by name |
| TD-LIC-07 | B | U | LIC-017, LIC-018 | Moves starting or ending outside Arizona refused (M13, M14; D-082); home watch with rent collection or leasing refused |
| TD-LIC-08 | E | U, M | LIC-019, LIC-020, LIC-024, LIC-025, LIC-036 | $999.99 accepted, $1,000.00 refused (W25, W26); a range judged by its top (W27); non-construction work over $1,000 allowed (W29); the disclaimer line on every handyman card; the certification box required; a handyman's text quote answered with the form link |
| TD-LIC-09 | I | P, M | LIC-021, LIC-022, AI-012 | Property: any job whose candidates include a gas, smoke-alarm, sprinkler or permit-type job type never reaches a handyman, whatever the AI says |
| TD-LIC-10 | E | U | LIC-023 | A second handyman job at the same address with overlapping scope 29 days 23:59 later is blocked; 30 days later is allowed (D-027, D-081) |
| TD-LIC-11 | B | U | LIC-026, LIC-032, LIC-034 | ROC line and other-agency line with date and link (D-057); insurance shown on no-license cards only, never required |
| TD-LIC-12 | B | U | LIC-028, LIC-029, LIC-030, LIC-031 | Dryer vent cleaning vs rerouting; plug-in lights vs new outlet; freestanding appliance vs gas line; weekly pool cleaning vs equipment vs gas heater |
| TD-LIC-13 | I | M | LIC-033 | 95% branch coverage floor and section 4's license mutants |

### 7.3 Job cards (`quotes`, `privacy`)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-CARD-01 | I | P, U | CARD-005, CARD-006, CARD-007, CARD-008, CARD-009, PRIV-006 | Property: descriptions generated with fake names after "ask for", phone numbers in any spacing, spelled-out digits, emails, street numbers → the card holds none of them; only town and kind of place; a description emptied by the gate becomes "<Category> job (details on the job page)". Corpus also includes the twin's adversarial texts |
| TD-CARD-02 | B | U | CARD-010 | Carrier-restricted content is screened before the card |
| TD-CARD-03 | B | U, E2E | CARD-011, CARD-012 | Rental card says "Landlord approval may be needed."; the Spanish line only for businesses that ticked "We speak Spanish"; no setting exists to skip tenants or languages |
| TD-CARD-04 | B | In | CARD-013, CARD-014, AI-016 | Before zero retention, no photo on any card; afterwards a photo appears only after location stripping and all three screens pass; any flag keeps it off |
| TD-CARD-05 | B | In | CARD-016, CARD-018 | A paused business, or one under an owner switch for its town or trade, gets nothing; a business that becomes eligible while the job is Open or No quotes yet gets the card then, with its own 24 hours |
| TD-CARD-06 | I | P | CARD-017 | Card IDs are stable for the same inputs, differ across jobs and businesses, and can't be produced without the server secret |

### 7.4 Intake (`intake`)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-INT-01 | B | U, In | INT-002, INT-003, INT-004, INT-005, INT-006, INT-024 | Emergency stems match on every message, mid-conversation and during an AI outage, in English and Spanish; safety text within 5 minutes with the reason line; no card before "safe"; the urgent-or-regular question; an emergency never closes a request early |
| TD-INT-02 | E | U | INT-007, INT-008, INT-009, INT-010, INT-011, INT-012 | "ME", "86403", "ok sure" open nothing; a cancel-like message opens nothing; a second message within 1:59:59 joins even if read as another trade; after 2 hours a same-trade message joins only if mostly the same words; "ok" inside "broke" is not an answer; word answers ("the AC one", "Yes!", "my email is …") accepted |
| TD-INT-03 | B | U, In, M | INT-013, AI-011, CARD-001 | The six twin warnings as regression tests (`docs/twin-warnings.md` section 3): both readings name a wrong trade, the keyword trade differs, the homeowner chooses the right trade → the keyword job type of that trade is kept, never replaced by "other" (D-102); with no job type of that trade, "Which fits best?" lists the trade's job types |
| TD-INT-04 | E | U | INT-012, INT-015 | The re-ask repeats the numbered options; "just a small repair" chooses handyman; "repair" never matches heating/AC; two unusable answers apply the fallback in D-103 and intake moves on (twin causes C2b, C2c) |
| TD-INT-05 | B | U | INT-013, INT-014 | When the two readings and the keyword trade name three different trades, the question offers all three (D-104, twin cause C2d); when a license-boundary keyword job type belongs to another trade than the reading ("Water heater not getting hot" read as heating/AC), it stays a candidate and "Which fits best?" is asked (D-105, twin cause C2e) |
| TD-INT-06 | I | P | INT-014, CARD-001 | Property: a request whose words match a licensed job type's keyword never reaches a license that doesn't cover that job type, unless the homeowner chose another job type in "Which fits best?" |
| TD-INT-07 | B | U, In | INT-016, INT-017, INT-018, INT-022 | Category not open → immediate reply and demand log; no licensed business in town → told at once (Not covered); town not on the list → Closed early (D-086); candidate job types, answers and flags stored |
| TD-INT-08 | F | In | INT-019, INT-021, INT-023 | Consent recorded: reminder at 24 hours, closed at 72 hours with "Text us any time to restart"; consent pending: no reminder, silent close (D-095); AI down: one "we have your message", retried every 30 minutes; every request hears back within 24 hours by text, email or a call-back alert |
| TD-INT-09 | B | In, E2E | INT-001, INT-020 | Text and web chat both start requests; "get on your list" gets the business sign-up link |

### 7.5 Consent and opt-out (`intake`, `messaging`)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-CON-01 | I | P, M, E2E | CON-001, CON-002, CON-003, CON-004, CON-026 | Property over every entry path (text, web chat, START return, emergency): the first outbound message to a new number is the consent text, or the safety text then the consent text (D-008); the consent text contains every required phrase, the fee line, "18 or older" and the Terms link; Spanish when the first message reads as Spanish |
| TD-CON-02 | B | U | CON-005, CON-006, TERMS-001, TERMS-016 | OK, ok, Yes, SURE, Sí, SÍ accepted; reply, time and prompt version stored; "Yes" to the consent prompt never confirms a pending pick |
| TD-CON-03 | I | P | CON-007, CON-021 | No card before consent; every outbound text's recipient has consent or the text is the consent text; tenants and on-site contacts are never texted |
| TD-CON-04 | E | U, M | CON-008, CON-010, CON-011 | Each of stop, quit, end, revoke, opt out, cancel, unsubscribe, parar, alto, cancelar, baja alone, in any case, with surrounding spaces → opt-out; "please stop texting me" and its Spanish form → opt-out; "can stop by at 3", "stop sign is down", "please cancel my request" → not an opt-out |
| TD-CON-05 | I | P, In | CON-009, CON-013, CON-014, CON-015, CON-016, CON-017 | Exactly one confirmation within 5 minutes saying "To use Ask Hava again, text START."; afterwards no text of any kind until START (property over all later events); 21610 never retried; START → a new request from the consent text; a message without START → email or call-back alert; an emergency from that number → critical alert, no text |
| TD-CON-06 | I | P, M | CON-012, JOB-020 | An opt-out changes no job status; bare "cancel" opts out and closes nothing; the job carries on by email and the page (D-005) |
| TD-CON-07 | B | U, In | CON-018 | HELP, INFO, AYUDA at 11 p.m. → reply within 5 minutes naming Ask Hava, the help contacts and STOP; Spanish for AYUDA |
| TD-CON-08 | B | In | CON-019, CON-020, BIZ-017 | Business consent box starts unticked, wording version and time stored; welcome text before the first card; a business STOP pauses it with "texts stopped" and emails how to resume |
| TD-CON-09 | E | In | CON-022, CON-023, CON-024, TERMS-020 | Records kept 5 years after the last message; nothing exports numbers or opt-ins; weekly opt-out rate 2.99% → no alert, 3.00% → alert (homeowners), 5.00% (businesses) |
| TD-CON-10 | B | E2E | CON-025 | Web chat shows the consent text first; OK or the agree button; a number typed there gets the consent text first and nothing else until it replies OK (D-051) |

### 7.6 Quiet hours (`messaging`)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-QH-01 | E | U, P, M | QH-001, QH-002, QH-003, QH-009 | Allowed when the time is at least 08:00:00 and before 20:00:00 in both Arizona and the recipient's zone (07:59:59 no, 08:00:00 yes, 19:59:59 yes, 20:00:00 no; D-101). A Pacific number at 08:30 Arizona in January (07:30 Pacific) → held; the same in July → sent. An Eastern number at 18:00 Arizona in July (21:00 Eastern) → held. Both daylight-saving change days of the recipient's zone; Arizona never shifts. An area code not in the table is refused and never texted (D-100). For a table entry with no known zone, only the hours inside 08:00 to 20:00 in every US time zone are allowed (QH-003, D-025); a test asserts that window is not empty, since US zones from Guam to American Samoa narrow it to a few hours. Canadian zones such as Newfoundland's are not US zones and are not part of that window; a Canadian number is judged in its own zone. Property: allowed equals allowed-in-Arizona and allowed-in-zone |
| TD-QH-02 | B | U, In, M | QH-004, QH-005, QH-006, QH-007, QH-008 | Each night exception sent at 23:00 (safety, STOP/HELP/START replies, pick codes, a reply 14:59 after the person's text, High and critical alerts to Casey, emergency-job steps); a reply exactly 15:00 or 15:01 after → held (D-101); job cards and confirm requests held for businesses, except emergency cards to after-hours businesses (D-054); a held text sent at 08:00:00; dropped if the job closed meanwhile; retries obey the same rule |
| TD-QH-03 | B | U | BIZ-010 | A business's own quiet times narrow the window and can never widen it (D-085) |

### 7.7 Messaging (`messaging`)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-TXT-01 | I | U, D | TXT-001, TXT-002, TXT-003, TXT-004, BIZ-018, WORD-008 | Registry test: every text kind has English and Spanish, Spanish marked draft, every template begins "Ask Hava:"; a Spanish homeowner's journey gets only Spanish templates; business texts English unless the business picked Spanish |
| TD-TXT-02 | I | D | TXT-005, TXT-006, TXT-007, TXT-008, TXT-010 | Scan of every template: links only to askhava.com, no shortener hosts, the ROC link is askhava.com/help/roc-complaint, no promotion words, no "reply CANCEL", no meaning carried only by emoji |
| TD-TXT-03 | B | In | TXT-009 | No text to a past customer who hasn't written first |
| TD-TXT-04 | I | In | TXT-011, TXT-012, TXT-013 | Worker killed after the vendor accepted a send but before it was marked: the resend carries the same key and the fake carrier delivers once; the same inbound message ID three times → handled once; delivery statuses recorded |
| TD-TXT-05 | F | In | TXT-014, TXT-015 | Important texts retried 3 times over 30 minutes then emailed and shown in the portal; 3 failures with no email → call-back alert |
| TD-TXT-06 | E | In | TXT-016, TXT-017, TXT-018 | First-quote text; update texts 2:59:59 after the last update's delivery → held, 3:00:00 → sent (D-012); the first status text offers email updates |
| TD-TXT-07 | I | In | TXT-019, TXT-020 | Message log holds a vault reference, never a number; at 30 days outbound bodies become template IDs and inbound raw and scrubbed text is deleted (D-093) |
| TD-TXT-08 | F | In, E2E | TXT-021, TXT-022, TXT-023 | Web-and-email mode stops all texts; a suspension pauses texting, uses email and the portal, fills the call list; property: every outbound text uses the configured number |

### 7.8 Quotes and the pick (`quotes`, `picks`)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-PICK-01 | E | In | PICK-001 | A card held overnight and sent at 08:00 accepts a quote at 07:59:59 the next day and refuses one at exactly 08:00:00 and at 08:00:01 (D-101); quotes accepted in Awaiting confirm, refused after Confirm (D-042, D-043) |
| TD-PICK-02 | B | U | PICK-002, PICK-003, PICK-004, PICK-005, PICK-006 | Fields stored; original wording always beside the summary; a summary adding a price, scope or warranty is rejected; preview equals the homeowner view; confidence 0.79 → back to the business, 0.80 → shown |
| TD-PICK-03 | B | U | PICK-007 | Phone digits, a link, spelled-out numbers, "text me" → bounced |
| TD-PICK-04 | I | P, E2E | PICK-008, PICK-009, PICK-012, WORD-005 | Arrival order; numbers never change as quotes arrive or become "no longer available"; no sort or filter control exists; Choose on every quote from the first |
| TD-PICK-05 | I | S | PICK-010, PICK-011 | Token has 128 bits from a CSPRNG; `noindex`; no name, phone or address on the page; token works 29 days 23:59 after close, refused at 30 days |
| TD-PICK-06 | E | S, A | PICK-013, PICK-014 | Code valid at 9:59, refused at exactly 10:00 and at 10:01 (D-101); 5 wrong tries then a 15-minute cooldown (D-015); paste and `autocomplete="one-time-code"` work |
| TD-PICK-07 | B | In | PICK-015, PICK-031 | "2 YES", "2 yes please" from the homeowner's phone pick; from another phone nothing happens; with two jobs in Quotes ready, Ask Hava asks which (D-068) |
| TD-PICK-08 | B | E2E | PICK-016, PICK-017, PICK-018, PICK-021 | Terms line before any pick; review steps with "Go back"; contact choice masked call or a chosen email |
| TD-PICK-09 | F | In, M | PICK-019, PICK-020 | No Confirm within 2 business hours → Quotes ready; one live license lookup at the pick, stored results at Confirm; an ineligible business can't confirm; lookup down → a daily result under 24 hours old is used, otherwise the pick waits (D-056) |
| TD-PICK-10 | I | P, S | PICK-022, PICK-023, PICK-024, PRIV-005, PRIV-020, BIZ-014 | Stateful property: in every reachable state, a business sees first name, address and photos only on a paid pick or a recorded free win, the first name once in the details text, and never the homeowner's phone (portal HTML, texts, emails, caller ID); the owner's release only re-sends an existing paid release (D-074) |
| TD-PICK-11 | B | In | PICK-025, PICK-026, PICK-027 | Rental pick asks who will meet the business; every quoter gets "This job is no longer active." once at a final status; a business asking about a live pick gets the Confirm link |
| TD-PICK-12 | I | P | PICK-028 | Quote, pick and link IDs derived and stable, as TD-CARD-06 |
| TD-PICK-13 | B | In | PICK-029, PICK-030 | Opted-out or texting off: the code goes by email to an address from the homeowner's own conversation; an email typed on the quote page never gets a code; "2 YES" from the opted-out phone still picks (D-050) |

### 7.9 Job lifecycle (`jobs`)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-JOB-01 | I | P | JOB-001, JOB-002, JOB-003, JOB-005, JOB-006 | Hypothesis state machine over quotes, picks, confirms, declines, calls, replies, cancels, closes and clock advances: one status at a time; only the arrows in `docs/architecture.md` section 6; every non-final status has a pending timer; an audit row per change; a timer firing after the job moved does nothing |
| TD-JOB-02 | I | In | JOB-004 | Two workers race a cancel and a Confirm, and a Stripe notice and a deadline, in both orders: one wins, the other applies to nothing (the twin's `races` scenario as oracle) |
| TD-JOB-03 | E | U, In | JOB-007, JOB-008, JOB-009, JOB-010, JOB-011, JOB-012, JOB-013, JOB-014, JOB-015, JOB-025, JOB-026 | Business clock: a pick at 7:30 p.m. has its 2 hours end at 9:30 a.m.; Open → No quotes yet at 6:00 p.m. next day with the text; 5 days → Closed; 3 days restarted on every return to Quotes ready; the failed quote can't be chosen again; no call in 72 hours → Quotes ready; no reply 7 days after the day-14 check-in → Closed with no review request; Intake → Open only after consent, questions, safety, area and duplicate checks |
| TD-JOB-04 | B | In | JOB-016, JOB-017 | Every day-3 PROBLEM is flagged to Casey with its text; the review request only after Done, asking for private feedback |
| TD-JOB-05 | E | U, M | JOB-018, JOB-019, JOB-021, JOB-022, JOB-023 | "CLOSE 1042", the page button, "please cancel my request", "never mind", the Spanish forms close; "cancel" alone doesn't; two open jobs → "Close which?" with numbers and "all", nothing closes until answered; "I had to cancel the plumber" → the CLOSE explanation |
| TD-JOB-06 | I | In, R | JOB-024 | A job number texted out is never reused, including after a restore |

### 7.10 Calls (`calls`)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-CALL-01 | B | In, C | CALL-001, CALL-002, CALL-017 | Each connected job gets a pool number neither side's own; both sides see only that number; pool exhausted → shared with PIN routing and an alert; the voice adapter has no buy-number method |
| TD-CALL-02 | E | In | CALL-003, CALL-004, CALL-005 | Unlinked phone: wrong PIN refused; two jobs on one number → routed only by PIN; after 2 bad PINs a phone linked to the paid business hears first name and street and must confirm; any other phone hears nothing (D-026) |
| TD-CALL-03 | I | C | CALL-006 | No call request or TwiML ever asks for recording |
| TD-CALL-04 | E | U | CALL-007, CALL-008 | Person 44 seconds → not a connection; 45 → connection; machine 90 seconds → not; each call logged with who, answered by and length |
| TD-CALL-05 | B | In | CALL-009, CALL-010, CALL-011, CALL-012, CALL-018 | Line closes on cancel, no-show, complaint or 30 days; cancel after connection tells the business, no strike; blocked staff phone hears the PIN prompt and the LINK text goes to the main phone with the last 4 digits (D-078); only the account owner links staff phones; email-preferred homeowners still get a line |
| TD-CALL-06 | B | C, D | CALL-013, CALL-014, CALL-015, CALL-016 | Relay-compatible bridging (no tone-only prompts); identity call says "This is Ask Hava LLC" and a callback number; code call slow, code twice, "press 1 to hear it again"; no route lets a business text a homeowner through Ask Hava's number |

### 7.11 Privacy and AI (`privacy`, `ai`)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-PRIV-01 | I | In, S | PRIV-001, PRIV-002, PRIV-003, PRIV-004 | Architecture test: no column outside `privacy` holds contact fields; a ciphertext copied to another row fails to decrypt (associated data); the vault key is not the database credential; every plaintext read writes an audit row with its purpose; only the readers in `docs/architecture.md` section 8 import the read functions (D-093) |
| TD-PRIV-02 | I | E2E, S | PRIV-005, PRIV-006, PRIV-016, PRIV-017, PRIV-019 | Sentinel test: J1 to J4 run with sentinel fake details (a fake name like "Zelda Canary", `+1 928 555 0199`, `canary@example.com`); afterwards no non-vault table, log line, Sentry stub event, job card, AI stand-in input or pre-release business message contains them; the PII scanner and gitleaks run on the repo |
| TD-PRIV-03 | E | In | PRIV-007, PRIV-008, PRIV-009, PRIV-010, PRIV-011 | Contact purge at 180 days after close, not at 179; a dispute or complaint hold blocks it; the phone stays encrypted with consent records (D-079); job text kept 3 years without contact details; photos deleted at 30 days; ledger and audit kept 7 years; "delete my data" removes the vault entry and photos and keeps ledger rows without names |
| TD-PRIV-04 | B | U, In | PRIV-013, PRIV-014, PRIV-015 | A PDF renamed `.jpg` is refused by its bytes; oversize refused; a fixture photo with fake GPS comes back with no EXIF or GPS; signed links expire; the upload says "avoid photos of people" |
| TD-PRIV-05 | B | In, E2E | PRIV-012, PRIV-018, PRIV-021, PRIV-022 | No form field for SSN, driver's license or tax ID images; backups encrypted (export stand-in output is not readable without the key); homeowner portal edits properties, contacts, language and texts on or off; 95% branch floor |
| TD-AI-01 | I | U, P | AI-001, AI-002, AI-003, AI-004, AI-009 | `ai` accepts only `ScrubbedText` (mypy and a runtime check); property: generated names after "this is", street numbers, 7+ digit runs and emails never survive the scrubber; the vault name is removed; build and CI configuration select the stand-in and hold no AI key |
| TD-AI-02 | I | U, C | AI-005, AI-006, AI-010, AI-015 | Malformed or extra-field stand-in output discarded; requests carry no tools; output never sends, charges or releases; intake fields limited to the listed ones; instruction versions stored in code |
| TD-AI-03 | F | P | AI-007 | Metamorphic: a request plus an injection line ("ignore your instructions and …", from the twin's adversarial texts) routes, prices and discloses exactly like the request without it |
| TD-AI-04 | B | In | AI-008, AI-013, AI-014 | Zero-retention switch off: the adapter is called zero times over a full journey; keyword rules and questions route the job; businesses quote by form (D-080); structured readings are written off-database; the weekly summary is counts only |

### 7.12 Businesses (`businesses`)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-BIZ-01 | B | In | BIZ-001, BIZ-002, BIZ-003, BIZ-016 | Stripe customer at sign-up, no card; code to the record's phone, then voice, email, letter, in order, never to a typed-in number (D-059); approval alone, code alone or license alone never activates |
| TD-BIZ-02 | B | U, E2E | BIZ-004, BIZ-005, BIZ-006, BIZ-007, BIZ-008, BIZ-009, BIZ-010 | Checklist, overflow, Spanish and after-hours ticks; insurance yes/no never verified or used to pause; each account status reachable only by its rules; pause, towns, job types, own quiet times; no weekly card limit |
| TD-BIZ-03 | E | In | BIZ-011, BIZ-012 | Complaint reply content; 3 complaints within 90 days pause, the third on day 91 doesn't; every complaint alerts |
| TD-BIZ-04 | B | E2E | BIZ-013, BIZ-015 | Portal shows the homeowner's contact choice; billing pages and CSV and PDF statements equal the ledger |

### 7.13 Wording, website and terms (`site`, documents)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-WORD-01 | I | D | WORD-001, WORD-003, WORD-004, WORD-010, WORD-011, WORD-012, WORD-015, TERMS-010, TERMS-012, WEB-010 | Scan of every template, page and drafted document: brand "Ask Hava"; no "vetted", "background-checked", "best" (except the INT-014 prompt, D-029) or rankings; "introduction fee", never "commission"; refusals worded as eligibility; no feature that prices or schedules work; business-side claims only with a cited source; Stripe description; no sharing of numbers; no "leads" family in the campaign; no pest page or banner; superseded values via `trace_spec.SUPERSEDED` |
| TD-WORD-02 | B | D, E2E | WORD-002, WORD-005, WORD-006, WORD-007, WORD-009, WORD-013, WORD-014 | Each required line on each page that needs it: not-a-contractor line, quote-page line, insurance line, privacy lines, automated-texts disclosure, Spanish key terms, the independent-business line |
| TD-WEB-01 | B | E2E, A | WEB-001, WEB-002, WEB-003, WEB-004, WEB-005, WEB-006, WEB-007, WEB-008, WEB-009, WEB-011, WEB-012, WEB-014, WEB-015 | Pages render with JavaScript off; English and Spanish pages; no directory, events or old /chat links, footer column and tagline; launch-offer copy; Spanish consent draft above the English; banner hidden by default; "Is Hava a person?" answer; web chat; ROC complaint page links on; old directory URLs show an Ask Hava page (200); request page shows the consent text; licensed-trade copy form |
| TD-WEB-02 | B | D | WEB-013, WEB-016, WEB-017 | The Stage 0.5 build is the static page set with no texting number and nothing that takes requests; the DNS records file keeps any `google-site-verification` record. Publishing waits for Casey's go (`BUILD_BRIEF.md:550`) |
| TD-TERMS-01 | B | D | TERMS-002, TERMS-003, TERMS-004, TERMS-006, TERMS-007, TERMS-008, TERMS-009, TERMS-011, TERMS-013, TERMS-014, TERMS-015, TERMS-018 | Each drafted document holds its required clauses: 18 or older, Arizona law, no arbitration and no class waiver, not an emergency service, independent business, warranty disclaimer, liability limit, non-discrimination, card authorization, handyman certification, privacy coverage and the no-sharing sentence, campaign use case and samples taken from real templates, the registration order, the Spanish review item |

### 7.14 Security (`portals`, `adapters`, `core`)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-SEC-01 | I | S | SEC-004, SEC-005, SEC-006, SEC-008, SEC-019 | Authorization matrix generated from the route table: every route × anonymous, homeowner A, homeowner B, business X, business Y, owner → expected allow or deny; homeowner A's token never reads B's job; only the owner has pause, refund and release; sign-in rules per portal (link single use and 15 minutes, new-device code, 30-day sessions, owner 12 hours) |
| TD-SEC-02 | F | S | SEC-014, SEC-015, SEC-020 | 5 new requests per phone per hour accepted, the 6th refused; 100 fake requests in an hour blocked and alerted; brute force on pick codes, sign-in links and owner sign-in stops at the limits |
| TD-SEC-03 | F | S, C | SEC-016, SEC-017, SEC-018 | Forged Twilio or Stripe signature → 403 and a log line; Stripe timestamp 300 seconds old accepted, 301 refused; one event 100 times → one effect; every adapter method that acts outside requires an idempotency key (architecture test) |
| TD-SEC-04 | B | S | SEC-009, SEC-011, SEC-012, SEC-013 | Cookies `__Host-`, `Secure`, `HttpOnly`, `SameSite=Lax`, rotated on login (D-099); a form post without the CSRF token → 403; homeowner text `<script>` shows as text; every response's headers; no third-party script or font host on any page |
| TD-SEC-05 | B | U, S | SEC-001, SEC-002, SEC-003, SEC-010, SEC-021 | A live key in a test environment (and the reverse) stops startup; the test control router is absent when `APP_ENV=live`; every route input is a pydantic model (architecture test); gitleaks, bandit and pip-audit steps fail on a hit; the lock file is committed and the base image pinned by digest |
| TD-SEC-06 | B | S, D | SEC-007, SEC-022, SEC-023 | Owner recovery needs two factors registered plus recovery codes; no email-only path; the threat-model review and the Phase 2 ZAP scan are recorded |

### 7.15 Accessibility

Section 6 covers every accessibility requirement: A11Y-001, A11Y-002, A11Y-003, A11Y-004, A11Y-005, A11Y-006, A11Y-007, A11Y-008, A11Y-009, A11Y-010, A11Y-011, A11Y-012, A11Y-013, A11Y-014, A11Y-015, A11Y-016, A11Y-017, A11Y-018, A11Y-019, A11Y-020, A11Y-021, A11Y-022.

### 7.16 Operations and recovery (`ops`, `recovery`)

| ID | Kind | Layer | Covers | Cases |
|---|---|---|---|---|
| TD-OPS-01 | E | In, E2E | OPS-001, OPS-002, OPS-003, OPS-004, OPS-005, OPS-006 | Safe mode at 24:00:00 after an unacknowledged High alert, not at 23:59:59; a complaint alert never starts it (D-075); property: every action checks the pause switches; delivery 94.9% for 30 minutes alerts, 95.0% doesn't; fewer than 2 active businesses in a trade or town alerts |
| TD-OPS-02 | B | In | OPS-007, OPS-008, OPS-009 | `/healthz` reports database and worker; the health page shows its six figures; the weekly spot-check picks 10 random cards |
| TD-OPS-03 | I | In | OPS-010, OPS-011 | Audit log append-only and exportable; the audit stream, request log and AI readings reach the storage stand-in as each event happens |
| TD-OPS-04 | F | R | OPS-013, OPS-015, OPS-016, OPS-017, OPS-018, OPS-019 | Local drill: snapshot, run a busy hour, restore the snapshot, replay from the stand-ins' records: the same derived IDs, no text sent twice, no second charge, an open call line reused, Stripe unreachable during replay → wait and retry, unmatched payments refunded and flagged, unrebuildable jobs listed; the export restores |
| TD-OPS-05 | B | R, D | OPS-012, OPS-014, OPS-020, OPS-021, OPS-022 | Phase 2 Railway drill including the switch to the restored database; the runbook holds the 45-day breach notice and starts by pausing texting and charging; the live project deploys only by Casey's press |

## 8. Order of work and done criteria

- **K6** turns each TD entry into test work inside the build task that owns the module, and adds two tasks before any routing code: the matching examples file (section 7.2) and the catalog's per-job-type license scope (K3 follow-up 1).
- **K8** builds the runner for named mutants, the mutmut nightly job, the coverage floors file, the test control router guard and the Twin gate (`docs/twin-warnings.md` section 6.3).
- **Stage 1** (the twin task, a `harness` task with an Opus review because all of `twin/` is protected) fixes the intake causes C1 to C2e, re-measures the class-waiver ceilings and empties the pinned waivers.
- A build task is done only when its TD entries have tests carrying their markers, its named mutants are killed, coverage floors hold, and `Ci`, `Twin` and `Ally` pass (`BUILD_BRIEF.md:261`).
- **K5's own criterion** (`sources/02_build_plan/Kit_Plan_Before_Coding.md:34`): breaking the license filter or the 4-month rule turns tests red. Until the app exists this is shown on the existing code: `tests/test_check_catalog.py` already fails on a broken multiplier and a wrong tier (D-036), and the twin's broken sub-license was shown invisible to the current twin (`docs/twin-warnings.md` section 2), which the gate's self-tests will catch.
