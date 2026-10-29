# Ask Hava build constitution

The principles every task in this build must follow. Written in kit step K1 (`BUILD_BRIEF.md:594`; `sources/02_build_plan/Kit_Plan_Before_Coding.md:29`).

**How to read the citations.** Every rule ends with its source as `path:LINE` (line numbers taken from `main` at commit `cb601df`, the latest commit touching `BUILD_BRIEF.md` and `docs/sources.md`; `sources/` is fingerprinted and read-only, `docs/sources.md:3`). This file restates rules; it never creates one. If this file and a cited source ever disagree, the source wins and this file is the bug.

---

## 1. Order of authority

1. **Rule zero.** `BUILD_BRIEF.md` decides *how* to build; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md` decides *what* to build. Anything else in the repo that contradicts them loses. (`BUILD_BRIEF.md:7`)
2. **The changes file beats every other document**, including the v9.3 blueprint. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:7`)
3. **Then the v9.3 blueprint** (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md`) is the base spec wherever the changes file is silent. (`docs/sources.md:30`)
4. **Then the twin and its review tests**, stale on brand, trades and fees until build Stage 1. (`docs/sources.md:31`)
5. **Then everything else** (prototype, business and design docs, reviews, legal record, history, build plan). (`docs/sources.md:32`)
6. **When BUILD_BRIEF.md and the changes file conflict:** the changes file wins on *what*, BUILD_BRIEF.md wins on *how*; Claude decides, logs both lines and the choice, and does not stop. (`BUILD_BRIEF.md:7`, `BUILD_BRIEF.md:539`)
   - Note: `docs/sources.md:29` says "stop and ask Casey" for this case. BUILD_BRIEF.md §16 is Casey's later standing instruction and overrides older text on questions (`BUILD_BRIEF.md:533`), and BUILD_BRIEF.md outranks `docs/sources.md` under rule zero. Follow §16: decide and log.
7. **Open questions** are decided by the order of authority (changes file, then v9.3, then other sources). Where all are silent, pick the option that is legally safest, then simplest to change later, then smallest in scope, and never one that breaks §2 of the brief. (`BUILD_BRIEF.md:537`)

## 2. Superseded rules (never build the old value)

The full table is `docs/sources.md:38`–`docs/sources.md:53`. The ones most likely to leak back in:

| Old value | Authoritative value | Source |
|---|---|---|
| Ongoing services count as 2 months | **4 months** ($150/month pool account counts as $600, fee $75) | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:60` |
| Fees $45 / $59 / $149 | **$25** under $500; **$75** for $500–$1,500; **$199** over $1,500; no percentage cap | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:59` |
| Brand "Havasu Home Quotes" | **Ask Hava** and **Ask Hava for Business**; legal entity Ask Hava LLC | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:24` |
| Directory plus quotes | Old directory **deleted**; quote service only | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:26` |
| Free first win for everyone | Only for businesses joining in the **first 90 days** | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:67` |
| Up to 4 contractors per trade | No cap: every qualified opted-in business gets every matching card | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:13` |
| Refund after 2 attempts on 2 different days | 2 attempts at least 3 hours apart within 72 hours | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:13` |
| Pick by replying YES | Pick by a code texted to the homeowner's phone (`2 YES` still works) | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:13` |
| "Every quote shown, never ranked"; "Insurance on file: Yes/No"; "skip tenant requests"; "cancel" closes a job | Section 7 wording and rules of the changes file | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:15` |
| No homeowner details to any AI | Zero-retention AI allowed **after** a plain-code scrub | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:18` |
| Profit goal per month | $20k–$50k **per year** | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:57` |
| Build in test mode alongside the hand-run test (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:70`) | The 4 to 6 week hand-run test (`docs/pilot/hand-run-test.md`) comes before Casey replies `approved to build` at Gate P; no new requirements until the re-plan pass after it | D-455, `docs/decisions.md:619` |
| Six launch-open pilot trades (D-418) | Every enabled category is launch-open; a category takes requests at 2 or more active businesses | D-459, `docs/decisions.md:623` |
| Price lock 12 months for early joiners (D-197) | **6 months** | D-465, `docs/decisions.md:629` |
| v9.3 host Render by default (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:66`) | Railway Pro if its Backups tab shows point-in-time recovery, otherwise Render | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:72` |
| v9.3 backup person (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:429`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:464`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:704`) | None for now; safe mode after 24 hours without Casey | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:73` |
| v9.3: describe the model to Twilio support before registering (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:253`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:696`) | Superseded: Casey sends the written eligibility question first, and registers only after Twilio's answer and once the full site is live (TERMS-014, D-305, D-477) | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:160`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:173` |
| "Insurance on file: Yes/No" (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:48`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:64`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:144`) | "Insurance (business-reported): Yes/No" | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:190`; `BUILD_BRIEF.md:452` |
| Gate A owner approval (`sources/02_build_plan/Kit_Plan_Before_Coding.md:39`) and owner approvals of K2b and K3 | Gate A removed; K2b and K3 decided by Claude and logged | `BUILD_BRIEF.md:533`, `BUILD_BRIEF.md:538` |
| Required checks `ci`, `twin`, `security`, `a11y` | `Ci`, `Twin`, `Ally` (security runs inside `Ci`) — a *how* item, so the brief wins | `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:296`; `BUILD_BRIEF.md:259` |

## 3. Casey's guardrails (the rules that can't bend)

1. **No sign-ups, purchases, outreach, publishing, live messages, live charges or production deployments without Casey's explicit go. Never create accounts or accept terms for him.** (`BUILD_BRIEF.md:61`, §2 rule 1; also `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:309`)
2. **Never paste, print, log, commit or store secrets.** Keys live only in network secrets (build), the app host's variables (running app) and the one Actions secret `CLAUDE_CODE_OAUTH_TOKEN` (backup runner). (`BUILD_BRIEF.md:62`–`BUILD_BRIEF.md:65`, §2 rule 2; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:310`)
   - No keys in chat, code, CLAUDE.md or plain environment variables. (`sources/02_build_plan/Build_Plan.md:165`)
   - Any key that lands in chat or a log is replaced at once. (`sources/02_build_plan/Build_Plan.md:176`)
   - Claude never holds a hosting key. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:305`)
3. **Never put homeowner names, phones, addresses or photos into an AI tool that keeps data.** The build uses fake data only; the app uses the zero-retention API with a code scrubber first. (`BUILD_BRIEF.md:66`, §2 rule 3; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:311`)
4. **Don't claim something was tested unless it was.** (`BUILD_BRIEF.md:67`, §2 rule 4; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:312`)
5. **Never use the live Railway project, live keys or live phone numbers before Gate C.** Deny rules enforce this. (`BUILD_BRIEF.md:68`, §2 rule 5)
6. **Never weaken a test to make it pass.** A wrong test changes only through a spec change logged in `docs/decisions.md`. (`BUILD_BRIEF.md:69`, §2 rule 6)
7. **Protected paths:** `.github/`, `.claude/`, `runner/`, `stand_ins/`, `sources/` and tests a task doesn't own may change only in a `harness` task with an Opus review; the runner rejects any other diff touching them. (`BUILD_BRIEF.md:70`, §2 rule 7)
8. **Only the runner writes state** on `claude/kit-state`; worker subagents never touch it. (`BUILD_BRIEF.md:223`)
9. **Casey's answers come only from `owner` GitHub issues.** Chat elsewhere is never read by the build. (`BUILD_BRIEF.md:33`, `BUILD_BRIEF.md:232`)

## 4. Honesty rules

1. Never report anything as done, tested or passing unless a tool run in this repository shows it. (`BUILD_BRIEF.md:57`; runner prompt `BUILD_BRIEF.md:585`)
2. Only the runner marks a task done, and only from GitHub's checks on the exact merged commit: `Ci`, `Twin`, `Ally` green on the head SHA and the squash commit's tree equal to that head's tree. (`BUILD_BRIEF.md:261`–`BUILD_BRIEF.md:263`)
3. A session saying it finished proves nothing. (`sources/02_build_plan/Build_Plan.md:255`)
4. The runner confirms which model actually ran from evidence it can inspect, not from the subagent's word; a substituted model fails the attempt. (`BUILD_BRIEF.md:247`)
5. Grok's simulation results are simulated, not real-world; never present them as real-world results. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:104`)
6. A flaky test is never deleted; a quarantined test keeps running and reporting. (`BUILD_BRIEF.md:272`)
7. Legal answers are research-based judgments, not legal advice; say so wherever they appear. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:233`)
8. Screen-reader, 200% zoom and Lighthouse checks on the prototype have **not** been run; don't imply they have. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:151`)

## 5. Approval gates and stop points

**Casey's standing instruction:** the build runs on its own. Claude asks only for things that truly need a human's hands; every other question is decided, logged and shown to Casey at the end. (`BUILD_BRIEF.md:533`)

**Claude stops and asks only at these points** (`BUILD_BRIEF.md:546`–`BUILD_BRIEF.md:551`, `BUILD_BRIEF.md:231`):

| # | Stop | What it requires | Source |
|---|---|---|---|
| 1 | Hands-on task only a human can do | One issue with exact taps, sent only after Claude checked it can't do it itself | `BUILD_BRIEF.md:547` |
| 2 | **Account checkpoint** (end of Phase 1) | Runner sets `WAITING_FOR_OWNER`, opens "Your turn: set up accounts" with `docs/account-checklist.md`, puts it atop STATUS.md, stops. Resumes on Casey's "accounts done". Claude never creates accounts or accepts terms | `BUILD_BRIEF.md:174`–`BUILD_BRIEF.md:178`, `BUILD_BRIEF.md:186`, `BUILD_BRIEF.md:548` |
| 3 | Real texts to Casey's own phone (Phase 2) | Only if Twilio test credentials can't be used, and only after he answers "yes" to that exact question in an `owner` issue. No other real phone receives anything before Gate C | `BUILD_BRIEF.md:190`, `BUILD_BRIEF.md:549` |
| 4 | **Stage 0.5, the askhava.com page** | Casey's separate go; it publishes something. Comes after the account checkpoint | `BUILD_BRIEF.md:201`, `BUILD_BRIEF.md:550`; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:267` |
| — | **Gate B** (not a stop; Casey's own review) | Casey's guided click-through plus his VoiceOver check; his alone, after "Finished: ready for your review". Nothing waits on it except Gate C | `BUILD_BRIEF.md:55`, `BUILD_BRIEF.md:199`, `BUILD_BRIEF.md:427` |
| 6 | **Gate C** | After the business accounts; Casey enters live keys and says "go live"; real-phone rehearsal on his phone | `BUILD_BRIEF.md:203`, `BUILD_BRIEF.md:551` |
| 7 | **First live charge** | Needs Casey's go, even after Gate C. No real charge before Gate C | `BUILD_BRIEF.md:203`; `sources/02_build_plan/Build_Plan.md:244` |

- **Gate A is removed**; on K9's pass the runner goes straight to build tasks. (`BUILD_BRIEF.md:538`)
- **Finished:** when Phase 2 is done (all of §1 except Gate B and Gate C), one outbox message "Finished: ready for your review" links the handover pack, `docs/for-casey-review.md` and the Gate B script. (`BUILD_BRIEF.md:553`)
- **Live deploys:** the live service has auto-deploy off and Casey presses Deploy at Gate C. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:305`)
- **First real AI key use** is from the Railway test project in Phase 2, never earlier. (`BUILD_BRIEF.md:194`; stand-in AI during the build, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:262`)
- **Decided by Claude, never asked:** K2b categories and worked examples (auto-approved when every worked example's fee matches the fee rules in a passing test and the Opus reviewer finds no conflict), K3 questions, conflicts, and `BLOCKED` tasks (up to 3 attempts, Opus last). (`BUILD_BRIEF.md:536`–`BUILD_BRIEF.md:540`)
- **Where decisions go:** one line each in `docs/decisions.md` (question, choice, reason, source line) and in plain language in `docs/for-casey-review.md`. Neither ever blocks the build. (`BUILD_BRIEF.md:542`)
- **Blocking is narrow:** a `BLOCKED` task holds only its dependents. (`BUILD_BRIEF.md:234`)
- **Usage limits:** wait for the reset; never pay per use. (`BUILD_BRIEF.md:509`; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:71`)

## 6. Legal gate and licensing

**The legal gate.** Every item in changes-file §7 is built in; items marked **gate** must also be cleared before any business signs, any live charge or any real homeowner request. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:155`) Every item in changes-file §3, §7 and §7.1 is required. (`BUILD_BRIEF.md:431`)

Gate items and other pre-launch requirements:
- **Business consent:** sign-up checkbox with written E-SIGN consent to job-card emails (and texts only while business texting is on, TXT-035); no consent to automated calls is asked for, because Ask Hava never calls businesses (D-344, D-477); version and timestamp stored. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:181`)
- **Business setup:** LLC via a commercial registered agent with a Maricopa County address; Lake Havasu City business license (home-occupation if run from home; never publish the home address); USPTO search for "Ask Hava" and "Hava" in Classes 35 and 42 before brand spending. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:226`–`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:229`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:254`)
- **Recruiting:** mostly self-serve from hand-written, disclosed Facebook group posts; a few visits; never texts or calls (BIZ-048, D-456, `docs/decisions.md:620`). D-181's in-person launch-week sign-ups are superseded.
- **Insurance:** later, after the business is fully working (Casey, D-426, `docs/decisions.md:575`); the changes file's "before Gate C" line (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:247`) is superseded by that decision.
- **Sales tax:** fees are tax-included; an 8.6% set-aside until an ADOR private taxpayer ruling is filed, requested after the LLC exists; no TPT license taken only "in case" (D-444, `docs/decisions.md:598`). This supersedes the changes file's TPT-license line (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:248`).
- **Before launch:** read the ROC website terms; file a commercial-purpose statement if required. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:250`)

**Not a contractor.** Ask Hava never prices, bids, negotiates, schedules, supervises or guarantees work; refusals are worded as eligibility rules. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:237`)

**License rules (per category).**
- Each category stores its license rule: none (city license list plus identity chain), ROC class, or other state agency. Adding a category is a settings change. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:35`–`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:39`)
- ROC classes are checked from the ROC's daily lists plus a live lookup at each pick. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:38`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:65`)
- ROC and city lists are for verification only, never recruiting. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:250`)
- License class comes from the state record at sign-up, never typed by the business, except a pest-management license, which the business attests with its number (LIC-047, D-417, D-477). (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:115`)
- Eligibility is re-checked at the pick and again at Confirm. (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:222`)
- A misread license page never changes a verified status; lookups never guess. (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:130`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:194`)
- A category that isn't confirmed stays switched off; the AI never guesses. (`sources/02_build_plan/Build_Plan.md:34`)
- Owner approval never activates a business on its own; the identity code is still required, and unlicensed businesses also need a city-license-list name match. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:115`)

**Routing that is fixed by law** (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:203`–`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:208`):
- pest control and pre-emergent or advertised weed control only to AZ Dept of Agriculture licensees; pest control is **never advertised** (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:239`);
- alarms only to Board-certified alarm businesses with an ROC low-voltage class; home inspections only to Board-certified inspectors;
- septic pumping only to ADEQ-licensed septage haulers;
- moves leaving Arizona refused;
- home watch and turnover cleaning never include rent collection, tenant-finding or leasing;
- garage doors and junk removal off at launch (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:240`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:249`; `BUILD_BRIEF.md:460`).

**Handyman** (A.R.S. 32-1121): quotes under $1,000 total, no permit, no job splitting; gas, hardwired smoke alarms and fire sprinklers never handyman work; permit-type jobs never sent to handymen; every card shows "Not a licensed contractor. Only for jobs under $1,000 total with no permit."; the cap applies only to construction-type work. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:40`–`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:45`) Split-job check, and handymen certify each quote. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:208`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:241`)

**Fees.** A fixed, published **introduction fee by job size**, never a percentage, never called a commission; charged at confirmation, not adjusted to the final bill; fallback is a flat-fee-per-job-type switch. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:238`; `BUILD_BRIEF.md:455`)

**Terms.** Homeowners must be 18 or older; no arbitration clause and no class waiver; acceptance at the pick only ("By picking, you agree to the Terms"), logged with time, terms version and request ID; texting in is the opt-in to texts, and no reply OK is asked for (Casey's decision D-419 and D-420 replace the source's "Reply OK" step). (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:213`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:244`) Business terms state refund triggers exactly as coded (a test checks they match), non-discrimination, and the card authorization. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:212`; `BUILD_BRIEF.md:465`)

**Copy and claims** (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:188`–`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:197`; `BUILD_BRIEF.md:451`–`BUILD_BRIEF.md:455`):
- "Ask Hava is not a contractor and does not perform, bid on or supervise work."
- "Every eligible quote shown, in the order received." "Insurance (business-reported): Yes/No."
- Never "vetted," "background-checked" or "best"; no rankings; say exactly what's checked.
- Hava is not a person; service voice only; truthful answer if asked whether it's a person (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:28`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:197`).
- Business-side claims without a cited source on the page are dropped (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:193`).
- The safety text stays generic and says Ask Hava is not an emergency service (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:196`).
- "Your number is never shared" plus "Your first name and address go only to the business you pick." (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:191`)
- License line: "ROC #___ showed Active on the ROC website when we checked on [date]. Check it yourself." (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:192`)
- Key terms (fee, STOP/HELP, privacy) also appear in Spanish wherever Spanish copy markets the service. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:195`)
- Stripe account described as "Quote-request service for homeowners. The business the homeowner chooses pays a set introduction fee." (WORD-015, D-377, D-477; the old "local service marketplace" wording is superseded) (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:224`)
- Terms include the independent-business statement and a warranty disclaimer, and limit liability to $100 for homeowners (TERMS-004, D-375, D-477). (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:247`)
- A receipt is emailed for every charge, and the charge description names the request number. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:252`)

## 7. Privacy

1. **Fake data only** in the build: no real names, phones, addresses or photos in code, tests or logs; a scanner fails the build on real-looking personal data or keys. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:263`; `sources/02_build_plan/Build_Plan.md:36`; environments `BUILD_BRIEF.md:516`–`BUILD_BRIEF.md:518`)
2. **AI and homeowner details:** allowed only to the Claude API (Haiku 5.5): under the zero-retention agreement once signed, and before that only on scrubbed text under the provider's recorded API terms, with the owner's switch and the privacy policy updated (AI-008, D-412, confirmed D-418). A plain-code scrubber runs before **every** AI call (names after "this is / my name is", street-number patterns, 7+ digit runs, emails); the final text gate still runs on every card. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:58`)
3. **Until the zero-retention agreement is signed:** photos go on job cards only after metadata stripping and Ask Hava's own on-server check, and are never sent to any AI (D-432, `docs/decisions.md:586`, which supersedes `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:58` and `BUILD_BRIEF.md:340` on photos), and, unless the D-412 switch is on, the app uses plain-code scrubbing, keyword rules and the question bank only (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:298`; D-412).
4. The build's Claude plan and the app's zero-retention Console organization stay separate accounts. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:307`)
5. The `ai` module never sees unscrubbed text. (`BUILD_BRIEF.md:133`)
6. The weekly AI summary takes counts only, never free text. (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:294`)
7. **Contact vault:** no homeowner contact detail leaves the vault except on a confirmed, paid pick. (`BUILD_BRIEF.md:151`) Details are released only after Stripe confirms payment (or a $0 fee decision is recorded: a free first win, a repeat customer under FEE-013 or a fee-free job type under FEE-045, D-477), and the homeowner's phone number is never given to the business. (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:218`–`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:225`)
8. **Privacy policy** must include the no-sharing statement, the CalOPPA plus CTIA contents, and the AI zero-retention disclosure; it never says numbers or opt-ins are shared, sold or transferred. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:180`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:214`–`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:221`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:176`)
9. **Photos:** location data stripped; optional; "avoid photos of people"; not sent to the AI unless needed. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:222`)
10. Never collect SSNs, driver's licenses or tax ID images. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:223`; `BUILD_BRIEF.md:361`)
11. Retention defaults and logging rules are in §10 below (`BUILD_BRIEF.md:350`–`BUILD_BRIEF.md:359`, `BUILD_BRIEF.md:373`).

## 8. Texting

**Consent**
- The full consent text is the first message to any new number, on every path; the only exceptions are the safety text (D-008), the business sign-up reply (D-132) and the bare-HELP reply (D-131). (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:113`; `BUILD_BRIEF.md:445`)
- Superseded: the first text carries no fee line (CON-003, D-419, D-477); the fee line is in the first-quote text and on the quote page. The old rule was: the first text also says the business you pick pays a small fee. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:81`) Since fix round 12 the wording is "a set fee of $25 to $199, by quote size", never "small fee" (CON-003, D-371, `docs/decisions.md:501`).
- Emergencies: the safety text goes **before** the consent text. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:86`) A plain keyword list runs before the AI at intake, on every message. (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:290`)
- Only people who contacted us or signed up are texted; never tenants or on-site contacts. (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:258`)
- Business consent by sign-up checkbox (E-SIGN) with version and timestamp. (`BUILD_BRIEF.md:448`)
- Homeowner texts are purely about their request: no promotions, no cross-selling. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:181`; `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:262`)
- Consent, opt-in wording version, STOP/HELP/START events and message logs kept 5 years after the last message to a number. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:183`; `BUILD_BRIEF.md:447`)

**Opt-out**
- A text that is only *stop, stopall, quit, end, revoke, opt out, optout, cancel* or *unsubscribe* (any case, D-477), or Spanish *parar, alto, cancelar, baja*, opts out of all texts; honored at once; the reply says "To use Ask Hava again, text START." (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:158`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:114`; `BUILD_BRIEF.md:443`)
- A message that merely contains "stop" is not an opt-out. (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:188`)
- One confirmation only. (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:261`)
- A STOP returner who texts again is treated as a new request. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:95`)
- Jobs close only with "CLOSE ####", the quote-page button or a longer cancel-intent message; "cancel" alone never closes a job. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:159`; `BUILD_BRIEF.md:444`)

**Quiet hours**
- No non-urgent texts outside 8 a.m.–8 p.m. in the **stricter** of Arizona time and the recipient's area-code zone (urgent safety texts excepted). (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:184`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:121`; `BUILD_BRIEF.md:446`) The v9.3 9 p.m. start is superseded.
- Other night exceptions from v9.3 still apply where the changes file is silent: STOP and HELP replies, pick codes, and direct replies within 15 minutes of the person's own text. (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:264`) Retries follow the same rules. (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:200`)
- Texts held overnight are dropped if the job closed. (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:212`)

**Registration and content**
- Register the Twilio campaign only when the full site is live; never use "leads," "lead generation," "referral network," "marketplace of providers" or "connect you with"; sample messages are real Ask Hava texts. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:173`–`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:175`)
- Never a second number to get around a suspension; fallback is the status banner, web chat and email (web-and-email mode behind a switch). (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:267`; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:75`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:177`)
- Links inside texts stay on askhava.com; never a public link shortener. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:29`; `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:263`)
- Homeowner job descriptions are screened for carrier-restricted content before going on a card. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:185`)
- Identity voice call opens "This is Ask Hava LLC" and gives a callback number. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:182`)
- Every text kind has a Spanish template. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:88`; `BUILD_BRIEF.md:416`)
- Plain words, no meaning carried only by emoji, full links. (`BUILD_BRIEF.md:415`)

## 9. Security (OWASP ASVS Level 2)

Full list in `BUILD_BRIEF.md:293`–`BUILD_BRIEF.md:382`. The non-negotiables:
- Deny rules cover live key patterns, Railway deploys, force pushes and pushes to `main`, reading `.env*`, `curl` to live vendor hosts with credentials, and deleting `claude/kit-state`. (`BUILD_BRIEF.md:300`–`BUILD_BRIEF.md:306`)
- gitleaks and the PII scanner run on every PR; any hit fails the build. (`BUILD_BRIEF.md:298`)
- Test and live are separate Railway projects with separate keys, data, webhooks and numbers; live auto-deploy off. (`BUILD_BRIEF.md:299`) The app refuses to start if a live key appears in a test environment, or the reverse. (`BUILD_BRIEF.md:521`)
- Homeowner quote page: unguessable 128-bit token, no contact details, pick only with a Twilio Verify code (6 digits, 10 minutes, 5 tries then cooldown); tokens die 30 days after close. (`BUILD_BRIEF.md:309`–`BUILD_BRIEF.md:314`)
- Business portal: single-use 15-minute email link plus SMS code on a new device. Owner portal: argon2id plus passkey or TOTP, no email-only recovery. (`BUILD_BRIEF.md:315`–`BUILD_BRIEF.md:317`)
- Cookies `Secure`, `HttpOnly`, `SameSite=Lax`; CSRF on every form. (`BUILD_BRIEF.md:318`)
- Every input parsed by serde into a typed struct; Askama autoescape always on; strict headers (CSP without inline script, HSTS, `frame-ancestors 'none'`, etc.); no trackers; self-hosted fonts; rate limits. (`BUILD_BRIEF.md:321`–`BUILD_BRIEF.md:331`)
- Webhooks: signature-verified (Stripe 5-minute tolerance), idempotent by event ID, unverified calls get 403 and are logged. (`BUILD_BRIEF.md:332`–`BUILD_BRIEF.md:335`)
- AI: scrubbed text in, schema-validated fields out, no tools, nothing it says is executed, can't add claims; prompt-injection tests must not change routing, fees or disclosures. (`BUILD_BRIEF.md:343`–`BUILD_BRIEF.md:345`; `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:192`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:296`) The quote card shows the business's own wording beside any AI summary. (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:200`)
- Vault: AES-GCM field-level encryption, key separate from DB credentials; encrypted backups; vault access audit-logged. (`BUILD_BRIEF.md:348`–`BUILD_BRIEF.md:349`)
- Card data never touches our servers (PCI SAQ A). (`BUILD_BRIEF.md:363`)
- Supply chain: committed `Cargo.lock` and `package-lock.json`, cargo-audit, cargo-deny and npm audit on every PR, Dependabot, base image pinned by digest. (`BUILD_BRIEF.md:365`–`BUILD_BRIEF.md:369`) New libraries need a `docs/decisions.md` entry with purpose, license and a cargo-audit (or npm audit) result, and must pass cargo-deny. (`BUILD_BRIEF.md:122`)
- Sentry `send_default_pii=False` plus scrubber; logs carry request IDs, never phone numbers; incident runbook includes Arizona's 45-day breach notice (A.R.S. 18-552); safe mode after 24 hours without acknowledgement of a High alert; what it pauses is narrowed to charges and releases by D-133, following the changes file's "pauses risky actions" (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:73`). (`BUILD_BRIEF.md:372`–`BUILD_BRIEF.md:375`)
- Architecture invariants: idempotency key on every outside action; audit row on every state change; money moves only through `payments` and the ledger reconciles to Stripe. (`BUILD_BRIEF.md:148`–`BUILD_BRIEF.md:152`)

## 10. Retention (decided defaults)

| Data | Kept for | Source |
|---|---|---|
| Homeowner contact details | Purged 180 days after the job closes, unless dispute or complaint | `BUILD_BRIEF.md:354` |
| Job text | Kept with contact details removed | `BUILD_BRIEF.md:355` |
| Consent, opt-out and message logs | 5 years after the last message to that number | `BUILD_BRIEF.md:356` |
| Ledger and payment records | 7 years | `BUILD_BRIEF.md:357` |
| Photos | Deleted 30 days after the job closes | `BUILD_BRIEF.md:358` |
| Audit log | 7 years | `BUILD_BRIEF.md:359` |

## 11. Accessibility (WCAG 2.2 Level AA)

Full list in `BUILD_BRIEF.md:384`–`BUILD_BRIEF.md:427`; target also in `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:151`. The non-negotiables:
- Semantic HTML first, ARIA only where needed; one `h1`, landmarks, skip link; `lang` on every page and inline passage. (`BUILD_BRIEF.md:387`–`BUILD_BRIEF.md:388`)
- Full keyboard use, visible unobscured focus (2.4.11), no traps; targets at least 24×24 px (2.5.8); no dragging. (`BUILD_BRIEF.md:389`–`BUILD_BRIEF.md:390`)
- Contrast 4.5:1 text, 3:1 large text and UI; reflow at 320px and 400% zoom; wide tables scroll in a labeled focusable region; text spacing overridable; reduced motion honored. (`BUILD_BRIEF.md:391`–`BUILD_BRIEF.md:393`; revision note `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:148`)
- Forms: visible labels, instructions first, errors in text with a linked summary, no re-asking (3.3.7), no memory or puzzle tests and paste/autofill work (3.3.8). (`BUILD_BRIEF.md:394`–`BUILD_BRIEF.md:399`)
- Time limits only where security needs them, each warns and offers a one-step extend, input kept; quote windows shown as plain text, not countdowns. (`BUILD_BRIEF.md:401`–`BUILD_BRIEF.md:405`)
- Review step with "Go back" before the homeowner's pick and the business's confirm-to-pay (3.3.4). (`BUILD_BRIEF.md:406`)
- Deaf and hard-of-hearing contact choice at the pick (masked call works with 711 relay, or email); no phone number ever shared. (`BUILD_BRIEF.md:407`–`BUILD_BRIEF.md:410`)
- Consistent help (3.2.6); `aria-live` polite for new quotes; `role="status"`/`role="alert"` used appropriately. (`BUILD_BRIEF.md:400`, `BUILD_BRIEF.md:411`–`BUILD_BRIEF.md:412`)
- Emails: real text, plain-text version, meaningful link text. Accessibility statement with a contact answered within 2 business days. (`BUILD_BRIEF.md:419`–`BUILD_BRIEF.md:421`)
- axe-core on every page and state at 320, 768 and 1280px; any violation fails `Ally`; keyboard-only Playwright journeys; Stripe's frame and VoiceOver checked by hand at Gate B. (`BUILD_BRIEF.md:424`–`BUILD_BRIEF.md:427`)

## 12. Quality rules (no AI-made mess)

1. A task touches only the files it owns; small PRs. (`BUILD_BRIEF.md:485`)
2. Functions under 50 lines; clippy complexity limits. (`BUILD_BRIEF.md:486`)
3. clippy clean with warnings denied, `cargo fmt --check` clean, no `unsafe`; no `#[allow(...)]` without a reason comment. (`BUILD_BRIEF.md:487`)
4. No commented-out code, TODOs or unused code on `main`; a cleanup task after every stage. (`BUILD_BRIEF.md:488`)
5. `docs/architecture.md` and `docs/decisions.md` updated in the same PR as the change. (`BUILD_BRIEF.md:489`)
6. If something breaks `main`, revert it; don't patch over it. (`BUILD_BRIEF.md:490`) A red `main` makes "fix main" the only task. (`BUILD_BRIEF.md:240`)
7. CLAUDE.md stays under 200 lines and points to BUILD_BRIEF.md. (`BUILD_BRIEF.md:491`)
8. Overall coverage never drops; money, privacy and licensing code stay at or above 95% branch coverage. (`BUILD_BRIEF.md:481`)
9. Mutation tests on `payments`, `catalog` and `licensing`: breaking the 4-month rule or the license filter must turn tests red. (`BUILD_BRIEF.md:472`)
10. The stack is decided; don't reopen it. (`BUILD_BRIEF.md:101`)
11. No module reaches into another's tables. (`BUILD_BRIEF.md:126`)
12. Every job status moves only along defined arrows; any other move is refused and logged; every waiting state has a timeout. (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:155`; `BUILD_BRIEF.md:136`)
13. Fresh-eyes review on every task checks for spec drift, duplicated logic, dead code and TODOs, secrets or PII, accessibility regressions, and AI smell: `scripts/check_ai_smell.py` must pass, and any customer copy gets a cold read aloud by a reviewer that sees only the copy (WORD-027, WORD-028, D-452). (`BUILD_BRIEF.md:249`–`BUILD_BRIEF.md:254`)

## 13. Legal holds: never build without a new legal review (fix round 12)

From the AI legal-research memos (Memo 1 Part V.B and R-14, Memo 4 Part F), adopted in fix round 12 (`docs/decisions.md:499`). Changing any of these needs a new logged decision citing a fresh legal review.

1. No payment handling for the work: no escrow, deposits, invoices or Ask Hava-branded receipts for the work.
2. No guarantee, protection, warranty, home warranty or service contract; no paying for or arranging redo work (LIC-042, D-376, `docs/decisions.md:506`).
3. No choosing for the homeowner: no auto-assign, ranking, "recommended" label, or sorting by price or score.
4. No price advice: no estimates, re-bid or "beat this price" requests, and no showing one business another's amount (PICK-026 stays coarse, or goes).
5. No scheduling of work, multi-trade bundling or materials selection.
6. No percentage fees and no fee tied to the final bill.
7. No requests from outside sources without disclosure; no buying or selling of request data.
8. No introduction fee on any inspection tied to a home sale, purchase or refinance: it is a fee-free job type (LIC-015, FEE-045, RESPA section 8, D-376, D-403, D-477).
9. Pest control only for businesses that attest to a Department of Agriculture license, with every pest card saying it isn't independently verified (LIC-012, LIC-047, LIC-048; Casey's D-417, `docs/decisions.md:559`, replaces the written-answer hold).
10. No recruiting texts or calls to businesses (BIZ-048, A.R.S. 44-1282 and 44-1272, D-374, `docs/decisions.md:504`).
11. No town outside Arizona (INT-022, D-382, `docs/decisions.md:512`).
12. AI-written words only inside AI-019: reworded intake questions and the job card and project file the homeowner sees and approves before they go; never a diagnosis, price, safety call or business recommendation; AI quote summaries stay off unless Casey turns them on with the label and the policy line (AI-019, PICK-003, D-373, D-430, `docs/decisions.md:584`, which rests on legal Memo 4's points: no diagnosing or pricing in Ask Hava's words, homeowner approval of the card).
