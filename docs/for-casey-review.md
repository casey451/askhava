# For Casey's review

These are decisions Claude made on its own and logged so the build never waits (`BUILD_BRIEF.md:542`). Older entries below may have been superseded by a later decision (the own-or-rent question, the 12-month price lock, the 1-business-day reply, the call window, the heat text and 211 lines); the spec in `docs/spec/` and the latest `D-###` win (D-482). None of them blocks anything. Each item names its entry in `docs/decisions.md`, which has the exact source lines. The ones most worth a second look come first.

## Two tasks finishing together no longer cost a try (D-891, October 31)

- **What happened.** On October 30 two tasks (T024 and T028) each lost one of their three tries only because another task merged into the same shared files just before them (a list of code libraries, the architecture notes, this file). The work itself was fine.
- **What the runner now does itself.** When both sides only added lines at the same spot in those shared files, the runner keeps both, the newer `main` lines first. Both of October 30's clashes would have been fixed this way with no one involved. Anything else, such as two edits to the same line, is never guessed.
- **What it hands back.** For any other clash, the task keeps its try. A worker merges `main` in, sorts out the clash keeping both sides, runs the tests, and the reviewer checks the result again before it is pushed. This can happen twice per try; a third clash, a failed test or a blocking review counts as a failed try, as before.
- **The rule that changed.** The build brief said a real clash "fails the attempt". That line (`BUILD_BRIEF.md:224`) now says the runner hands it to a worker on the same attempt first.

## Worth a look first: two tasks at once is ready, but still switched off (D-620, October 30)

- **What's new.** The runner can now work on two tasks at the same time, which should roughly double how fast the build moves. It stays off: `kit/runner.json` still says `"lanes": 1`. Turning it on means changing that one value to `"lanes": 2`, made separately once you've looked this over. That file is now part of the locked plan, so the same small change also refreshes the plan's fingerprint and adds a one-line decision entry saying why; that way the switch is always on record.
- **Safety rules when it's on.** Two tasks only run side by side when they don't touch the same files, are in the same stage, and everything they depend on is finished. A stage's cleanup task always runs alone. If `main` breaks, both lanes stop and only the fix runs. Usage pacing counts both tasks, so two at once can't overshoot your plan's limits.
- **Keeping pull requests current.** When one task merges, the other task's pull request falls behind `main` and GitHub won't merge it. Until now someone had to update it by hand. The runner now does it itself, with a normal merge, and only accepts the result if it's exactly the reviewed work plus `main`. If the two tasks really clash, it doesn't guess: that try fails and the next try starts from there. This also helps with one lane, whenever `main` moves under a waiting task.
- **If two at once misbehaves** (only failures that really come from running two at once count, not a task's own mistakes), the runner drops back to one task at a time after two such failures in a row, tells you why in an issue, and stays there until someone runs `python3 -m runner --restore-lanes`. It won't flip back and forth on its own.
- **The rule that changed.** The build brief said "one task per session, one session at a time" and "nothing new is claimed while any task is waiting to merge". Those lines (`BUILD_BRIEF.md:209`, `:210`, `:222`, `:224`) now allow up to `lanes` tasks at once under the rules above. The decision log quotes the old and new wording.

## A build rule changed so retries can push (D-583, October 30)

- **What happened.** T167's first try pushed its work. The second try started a brand-new branch from `main`, as the task's instructions said, so its push clashed with the first try's work. The runner never overwrites a branch, so the push was refused, and the runner wrongly said the push key was at fault and blocked the task.
- **The rule that changed.** The build brief said every task branch is "created from the latest `main`", and it also forbids overwriting a branch. On a retry both can't be true. The brief (`BUILD_BRIEF.md:246`) now says a retry carries on from its earlier branch with the latest `main` merged in, and starts clean on a new branch name (ending `-a2`, `-a3`, and so on) when that can't be done. This is the change most worth your second look; the decision log quotes the old and new lines.
- **How it works now.** Before each try, the runner prepares the branch itself. If the old pull request was set to merge on its own, the runner closes it first, with a note, so nothing unreviewed can slip in; it opens a fresh one after the review. If a merged pull request ever turns out not to be the reviewed one, the task is blocked for you to look at instead of being marked done. A refused push now says what really happened.
- **Unblocking.** A task blocked by something outside its own work (a refused push, a permission, a missing proof) goes back in line with `python3 -m runner --requeue T### --reason "..."`. It never adds tries beyond the usual three, and it waits while any task is running.

## Follow-up: unreviewed work can't sneak in, and nothing waits forever (D-584, October 30)

- **If a merge ever brings in work nobody reviewed**, the build now treats `main` as broken: it undoes that merge automatically, then puts the task back in line. STATUS.md shows it at the top. Before, the task just sat blocked. A merge that only brings the branch up to date with `main` (GitHub's "Update branch" button) is fine and isn't flagged.
- **If a try dies halfway**, the runner only picks up its pull request when it's exactly what was reviewed. Otherwise the task simply gets its next try. Before, it could pick up the wrong pull request, or wait forever on one it would never merge.
- **If someone pushes to a pull request after its review** while it's set to merge on its own, the runner closes that pull request first, so the unreviewed version can't merge, and the task gets another try.
- **Nothing for you to do**, unless the runner sends you a note that it couldn't check a merge after two hours of trying. That note says what to look at.

## Two loose ends from the task queue and rate limits (D-708 to D-710, October 30)

- **The visitor's address on Railway is a guess for now.** Rate limits count requests per visitor address. The app reads that address from a header Railway's front door sets, assumed to be `X-Real-IP`. Nobody has checked this on the test project yet. If it's wrong, every visitor shares one count and gets limited sooner. That's the safer way to be wrong, but it still needs fixing. It gets checked and written down in `docs/deploy.md` when the test project is set up (OPS-050). Nothing for you to do.
- **Old rate-limit counts aren't cleaned up yet.** The cleanup code exists, but nothing runs it yet. The task that adds the first nightly job will hook it up. Until then the old counts just take a little database space.
- **A stop or deploy doesn't use up a task's tries.** When the worker shuts down cleanly, any task it was still running goes back in line as if it never started. If a task was cut off halfway, it runs again from the start. Every task is built to be safe to run twice.

## How legal, tax and insurance items are handled now (October 28, 2026)

You've decided not to hire a lawyer, a CPA or an insurance broker for now, to rely on AI research instead, and to accept that risk yourself. So, throughout this file (D-367):

- **Every "ask a lawyer", "TCPA attorney", "CPA" or "trademark attorney" item** now reads "AI legal-research memo + your decision". The four memos are finished and are in `/home/claude/hhq/outputs/legal/` (Memo 1 contractor status and consumer law; Memo 2 messaging, privacy and data; Memo 3 business setup, tax and trademark; Memo 4, an attack review of the other three, which wins where they disagree). Older items below still say "(in progress)"; read that as "finished". They are research, not legal advice, and no lawyer checked them.
- **Fix round 12 (D-371 to D-388) put the memos' must-fix items into the spec.** What's left for you is in the fix round 12 section at the end of this file.
- **Insurance stays a recommended purchase.** It's cover for when something goes wrong, not advice. Memo 3 and Memo 4 lean to a tech E&O policy with cyber, bound before the first introduction. *(Oct 28: you chose to handle insurance after the business is fully working, D-426.)*
- **Government filings and registrations stay your actions:** the LLC, the Lake Havasu City business license, a TPT license if you choose that path, a trademark filing if you want one, and the Twilio and Stripe accounts.
- **Nothing in the spec got looser.** The homeowner safety voice call stays off: its switch still needs a recorded attorney approval (CALL-019), and now also Memo 2's six conditions built and tested first (CALL-022). If you want it on under your own recorded decision instead, ask for that as a spec change.

## Pre-build engineering audit (October 28, 2026, D-479 to D-483): three things only your hands can do

You asked for the decisions to be made on best practice rather than brought to you, so they were (D-483). The audit report is `docs/audit/prebuild-audit-2026-10-28.md`; the verdict is `docs/build-readiness.md` (**not ready to build yet**, on purpose). What is left is three settings only you can change, each sent as its own GitHub issue when the time comes:

1. **Give the build its own identity.** Run `claude setup-token` at a computer and paste the token into the repository secret `CLAUDE_CODE_OAUTH_TOKEN` (never into chat). The build then pushes and comments as the Claude app instead of as you, so a comment that says "approved to build" is provably yours. In the `protect-main` ruleset, turn on "Require review from Code Owners": only pull requests that change the rules themselves will wait for your tap.
2. **Four ruleset settings:** add `Governance` as a required check; set GitHub Actions as the source of all four checks; turn on "Require branches to be up to date"; add a ruleset on `claude/kit-state` that blocks deletion and force-pushes.
3. **One number:** the GitHub Actions minutes your plan includes (repository Settings, Billing). Reply with it so the schedule is paced to fit.

Decided for you: Sonnet writes by default and Opus writes the money, privacy, security and recovery code with Opus reviewing all of it; the live site only ever deploys a commit you name after an independent security review; at launch photos must be JPEG or PNG (phones send those by default), HELP notifications reach your inbox scrubbed, and a pick made by text never triggers an automatic charge.

**What the audit changed without you:** governance rules and an operating manual for every session; a fingerprint of the plan that fails the build if any plan file changes without a logged decision; progress counted in verified requirements instead of finished tasks; 28 new security and privacy requirements with their tests and owning tasks; two new harness tasks (the twin gate, which the `Twin` check never actually ran; the end-to-end journeys in CI, which no workflow ran); coverage floors moved ahead of the vault and licensing code; owner replies like "don't go live yet" no longer open a gate; a risk register with an owner and a resolution test for every open risk.

## Your first item: ask Twilio in writing before registering (October 28, 2026)

This comes before everything else in this file. Texting registration is the one step that can't be undone: if Twilio refuses the campaign as "lead generation" (error 30951), it can't be resubmitted. So ask first, in writing. Nothing has been sent; you send it. (D-305, D-322)

- **Sign as "Ask Hava (LLC being formed)"**, with no personal name (WORD-021). Once the LLC exists, sign "Ask Hava LLC".
- **Send the Twilio question** as a Twilio Support ticket. Save the case number and the written answer, and file the answer in the owner portal when it's built.
- **Send the Telnyx question only if Twilio's answer is unclear.**
- **What happens with each answer:**
  - "Yes": register the homeowner campaign, with the homeowner's first text as the opt-in (D-419).
  - "No", or a wording change asked for: change the wording they name, log it, then register; until then the service can run in web-and-email mode (TXT-021).
  - Businesses are email-only, so the question no longer asks about business texts; business texting stays off (TXT-035). (Shortened Oct 28, D-424.)

### To Twilio (send as a Support ticket before registering)

The same text is saved at `/home/claude/hhq/outputs/send_drafts/Twilio_question.md`.

> **Subject:** Eligibility check before A2P 10DLC registration – customer care texts
>
> Ask Hava (askhava.com) is a local quote-request service in Lake Havasu City, AZ. A homeowner texts us first to ask for quotes on a home job; our first reply includes our name, msg frequency, rates, HELP/STOP and Terms/Privacy links. We only text that homeowner about their own request: a few questions about the job (some worded with help from an AI service, from what the homeowner already told us), a request for optional photos, confirmations, "your first quote is in" with a link to their private quote page, and their pick confirmation. The homeowner may send photos by MMS. We send no marketing texts and never text businesses; businesses get job requests by email only. When the homeowner picks a business and agrees to share, that one business receives the homeowner's first name and address so it can do the job. The homeowner's phone number is never shared, and no consent or data is sold or transferred.
>
> Questions:
> 1) Is this eligible as a Customer Care (or conversational) campaign with the homeowner's inbound text as the opt-in?
> 2) Will the privacy-policy sentence "Your first name and address go only to the one business you choose" cause a rejection under errors 30871 or 30951? If so, what wording would you accept?
>
> Ask Hava (LLC being formed)

### To Telnyx (send only if Twilio's answer is unclear)

> **Subject:** Pre-registration question: is this 10DLC use case eligible? (code 708)
>
> Hello. I'm considering Telnyx for SMS and voice. Your 10DLC error-code page lists 708, "Lead Gen/Affiliate Marketing Prohibited." Before I open a brand, I'd like to know whether my use case would be treated that way.
>
> *[Paste the same body as in the Twilio question, from "Ask Hava (askhava.com)" to "sold or transferred."]*
>
> **Questions:**
> 1. Would texts between us and the homeowner about their own request, with the homeowner's inbound text as the opt-in, pass vetting as Customer Care (or conversational)?
> 2. If a campaign is rejected with 708, can it be appealed or corrected?
>
> A short written answer is fine. Thank you.
> Ask Hava (LLC being formed)

## From K2 (the spec), October 27, 2026

**Worth a second look**

1. **Texting STOP (or a bare "cancel") no longer closes a homeowner's open jobs.** The older blueprint closed them; the newer changes file says a one-word opt-out only stops texts. The job carries on through the quote page and email, and ends by "CLOSE 1042", the close button or its normal timeouts. (D-005)
2. **Quiet hours are 8 a.m. to 8 p.m.**, in both Arizona time and the time zone of the person's area code. Replies the person just asked for (STOP/HELP answers, pick codes, a reply within 15 minutes of their text) and urgent safety texts still go out at night. If an area code's time zone is unknown, only hours that are daytime everywhere in the US are used. (D-003, D-025)
3. **"Business hours" for deadlines are 8 a.m. to 8 p.m. Arizona time, every day.** The 2-hour Confirm window and the 1-hour card fix only count during those hours. (D-004)
4. **Fee tiers at the exact edges:** a $500.00 quote is $75 and a $1,500.00 quote is $75; $1,500.01 is $199. The old "major job" flag no longer changes the fee. (D-009)
5. **Recurring prices that aren't monthly** (weekly, every two weeks, quarterly, yearly) are turned into a monthly price first, then counted as 4 months. (D-010)
6. **Masked calls, after 2 wrong PINs:** the line reads back the homeowner's first name and street only to a phone already linked to the business that paid for that job. Anyone else hears nothing and is refused. (D-026)
7. **Quote-count update texts** go out at most once every 3 hours per job. (D-012)
8. **The free first win** counts "first 90 days" from the day you go live (Gate C). (D-019)
9. **Split-job check window for handymen:** 30 days. (D-027)

**Routine (filling a gap or taking the newer rule)**

- Homeowners see quotes from the first one, and businesses have 24 hours (every day) to quote. (D-011)
- Data is kept per the brief's retention table: contact details 180 days after a job closes, photos 30 days, texting records 5 years, money and audit records 7 years. (D-013)
- Sign-in: businesses by emailed link plus a text code on new devices; you by password plus passkey or authenticator, sessions up to 12 hours; homeowners by phone plus a texted code. (D-014, D-015)
- Businesses are paused after 3 complaints in 90 days, not on the first one; every complaint still alerts you. (D-016)
- Licenses are checked against the ROC's daily lists plus a live lookup at each pick; a failed lookup never changes a verified status. (D-017)
- Natural "please stop texting me" counts as an opt-out; the STOP reply says "To use Ask Hava again, text START." (D-006, D-007)
- In an emergency, the safety text goes before the consent text. (D-008)
- Consent replies OK, Yes, Sure and Sí are all accepted. (D-021) *(Replaced Oct 28, D-419: texting in is the opt-in; no reply OK is asked for.)*
- No weekly card limit for businesses; they control volume with their job types, towns and pause. (D-028)
- Spec format: one testable sentence per requirement with its source line, checked by `scripts/trace_spec.py`; written in one session. (D-001, D-002)
- Smaller wording and naming items: D-018, D-020, D-022, D-023, D-024.

## From K2b (category list and fee examples), October 27, 2026

The list is `docs/catalog/categories.yaml`; the fee examples are `docs/catalog/worked_examples.yaml`. A check script recomputed every example from the fee rules and all 32 matched.

**Worth a second look**

1. **39 categories, 37 on.** The 25 from the money model, plus the jobs the legal decisions named (dryer vents, plug-in holiday lights, estate cleanouts, donation pickup, septic pumping, alarms, cameras, home inspection, home watch) and Grok's license-class trades (water softeners and misting, gutters, insulation, pavers and irrigation, epoxy floors). Garage doors and junk removal are listed but off. Pest control is on but never advertised. (D-031)
2. **Judgment calls on licenses:** solar screens need the window-film license (R-65); boat repair and boat detailing need no license; dock, boat-lift and seawall work goes only to licensed contractors; handymen get cards only in handyman, plumbing, pool repair, appliance repair and cameras, and never for gas appliances. Each row has a note saying where its rule came from and what a lawyer might still check. (D-032)
3. **Fractions of a cent round down.** This only ever matters right at $1,500: a $4,500.01-a-year service works out to $1,500.0033 over 4 months, and a $1,500.00–$1,500.01 range has a midpoint of $1,500.005. Both count as $1,500.00, so the fee is $75, not $199. At $500 it never changes anything. (D-033)
4. **One-time jobs from recurring businesses are not counted as 4 months.** A $300 move-out clean from a house cleaner is a $300 job, so the fee is $25. Only the job types listed as one-time for that category (deep or move-out cleaning, green-pool cleanup, yard cleanup, termites, storm checks) can be quoted this way. (D-037)
5. **A handyman quote of exactly $1,000 is refused** (the law says "under $1,000"), and a range is judged by its top: $700–$1,100 is refused. A $1,200 car detail is fine, because the limit is only for construction work. (D-035)

**Routine**

- Trip-fee quotes use a set fee per category: $25 for most, $75 for bigger jobs like boat repair, moving, painting and roofing, $199 for AC replacement. (D-034)
- Examples cover the tier edges ($499.99 is $25; $500 and $1,500 are $75; $1,500.01 is $199), the $150-a-month pool account ($600, so $75), weekly, every-two-weeks, quarterly and yearly prices, ranges, repeat customers ($0) and the free first win ($0, with $75 recorded as waived). (D-033)
- The check uses one new library, PyYAML, to read the list; it is MIT-licensed and the vulnerability scan found nothing. (D-036)

**Follow-ups (not done in K2b)**

- The K2b reviewer raised four non-blocking points (review items 4, 5, 8 and 9) that the coordinator deferred to later tasks rather than fixing here; the review record has the details.

## From K3 (conflict check and open questions), October 27, 2026

Claude re-read the spec, the category list, the changes file, the build brief and the v9.3 blueprint against each other and settled 52 open points itself (D-038 to D-089). No question was sent to you. Where a choice changed a spec line, that line now names the decision.

**Worth a second look**

1. **The "full site" Twilio wants to see goes up at Stage 0.5, as static pages.** Twilio says register only once the full site is live, but the full app can't go live before Gate C, and Gate C needs the registration. So the Stage 0.5 page you approve will be every public page of the final site (including the request page showing the exact consent text), with a "requests open soon" note. No texting number is shown until Gate C; the page says it will appear when requests open. Nothing takes requests, sends texts or charges cards until Gate C. Your Stage 0.5 go still comes first. (D-041)
2. **Emergency jobs can text at night.** If a homeowner chooses "emergency", that job's cards go out at any hour, but only to businesses that ticked "after-hours emergencies". Other businesses get the card at 8 a.m. At night the homeowner gets only the pick, confirm and connect texts for that job (their pick code, "the business confirmed", who is calling); everything else waits for 8 a.m. The rules allow this because only non-urgent texts are barred at night, and the homeowner chose emergency service after the safety text. (D-054)
3. **When Ask Hava can't text someone** (they texted STOP, or texting is switched off), their pick code goes by email instead. It only goes to an email they gave in their own conversation with us, never one typed on the quote page, so a forwarded link still can't be used to pick. (D-050)
4. **After the 180-day purge the phone number is kept, and only the number.** Name, address, email and photos are deleted. The number stays, encrypted, next to its consent and STOP records for the 5-year texting-records period, so a STOP keeps being honored and consent can be proved. The privacy policy says this. (D-079)
5. **Refund rules that go in the business terms:**
   - a homeowner who cancels gets the business refunded unless a real call already happened (someone answered and talked for 45 seconds or more) (D-048);
   - a business that pays and then never calls is not refunded; it's recorded as a no-show and you're alerted, and the business terms say so (D-047);
   - the "couldn't reach the homeowner" refund uses the call log even when the homeowner chose email contact. The business sees "homeowner prefers email" and is told to email, but the masked line still opens so its call attempts can count. That's simplest, but a business could email, get the job, and still be refunded after two unanswered calls. (D-049)
6. **Repeat customers are free until the 180-day contact purge.** After that we can't recognise them. Rather than keep a fingerprint of their number, we let the match lapse. If a business shows you a missed repeat, refund it from the owner portal. (D-069)
7. **Every sign-up still needs your approval,** as in v9.3, together with the identity code and the license or city-list check; none of the three is enough alone. Flagged sign-ups are marked in your queue. Identity codes and letters always go to contacts on a public record (the state record, or Lake Havasu City's business-license list for unlicensed trades), never to a number the business typed in. (D-059, D-060)
8. **License checks:** every license is checked once a day, plus one live lookup when a homeowner picks. Confirm uses those results rather than a second lookup. (D-056)
9. **Sub-licenses:** an AC-only HVAC license (R-39R) gets only AC jobs, and a drains-only plumbing license (R-37R) only drain jobs. The category list doesn't record this yet, so a later task must add it before any matching code is written. (D-058)
10. **Moves starting or ending outside Arizona are refused**, not just moves leaving the state. (D-082)
11. **The "how did it go?" message after Done asks for private feedback to Ask Hava only.** It's never shown publicly or used to order quotes, because rankings are off-limits. (D-083)
12. **Switching to flat fees** sets each job type's fee to its category's trip-fee tier by default. Each business must accept a new card authorization before it can be charged under the new mode. (D-072)
13. **Losing quoters hear "This job is no longer active." when the job actually ends**, not at the pick, because a no-show can reopen the job. (D-044)
14. **Your "release" button can't release contact details on its own.** It only completes or re-sends a release for a pick that's already paid (or a recorded free win). (D-074)
15. **Someone who texted STOP gets no text until they send START,** even if they text us again. We reach them by email or put them on your call-back list. (D-053)

**Routine (filling a gap or taking the newer or higher-ranked rule)**

- Process: no K3 questions went to you, and the brief's names for checks and branches win (D-038, D-039). The real AI is first tested in Phase 2 from the test site (D-040).
- Quote timing: each business gets 24 hours from when its own card actually went out. A business that qualifies later still gets the card. Quotes stop once the picked business taps Confirm (D-042, D-043).
- No job waits forever: a failed pick's quote is withdrawn and the 3-day clock restarts. A connected job with no answer closes 7 days after the day-14 check-in (D-045, D-046).
- Emergencies no longer close a request early (D-055).
- Web chat shows the consent text first. A phone number typed there gets no texts beyond the consent text until that phone replies OK. The consent text and web form say users must be 18 or older (D-051, D-052). *(Replaced Oct 28, D-419: starting the chat is the opt-in, and a typed number gets only a code until the code is typed into the chat.)*
- Language: the first consent text goes in Spanish when the first message reads as Spanish. AYUDA works like HELP. Businesses can pick Spanish texts. Every text the app writes starts "Ask Hava:", and Twilio's code texts carry the name "Ask Hava" (D-063 to D-066). Native-speaker review of the Spanish texts is a Gate C checklist item (D-067). *(Changed Oct 28, D-427: Spanish conversations start switched off; only the Spanish safety texts stay on, checked by two-AI back-translation before launch; a paid review comes before Spanish is switched on.)*
- "2 YES" with several open jobs asks which job first. A text pick defaults to a masked call (D-068).
- Money details: near-tier-line flags (3 of the last 10 quotes within 5% under $500 or $1,500), PROBLEM replies flagged to you, cards only (no bank debits), and a card saved at the first paid Confirm for businesses with no free win (D-070, D-071, D-073, D-089).
- Operations: which alerts count as High for safe mode, High alerts can text you at night, and a fixed masked-number pool that the app never adds to by itself (D-075 to D-077).
- Insurance is a yes/no the business reports, never verified or tracked (D-061). A business that texts STOP is paused (D-062). The "Reply LINK" text goes to the business's main phone, not the unlinked one (D-078).
- No AI calls at all on real requests until zero retention is signed; the Phase 2 test of the real AI on fake data is still allowed (D-080). Handymen certify each quote on the portal form, and the split-job check covers the same category or job type (D-081).
- The quote page has no sorting, and quotes from businesses that become ineligible show as "no longer available" (D-084). Businesses can set their own quiet hours, but only narrower than 8 a.m. to 8 p.m. (D-085). The service area is a towns list starting with Lake Havasu City (D-086).
- Google Search Console verification is kept (D-087). Twilio registration follows the changes file: Customer Care, no pre-clearance, and the new privacy sentence (D-088).

**Follow-ups (files K3 doesn't own)**

- `docs/sources.md` still says "stop and ask Casey" for brief-vs-changes conflicts. The brief overrides it (D-038).
- `docs/catalog/categories.yaml` needs per-job-type scope for R-37R and R-39R; it's listed in `docs/decisions.md` for K6 to queue (D-058).

## From K4 (architecture, threat model and versions), October 27, 2026

Claude drew up how the app is split into parts, wrote the security threat model, and chose exact library versions. Eleven choices were made on its own (D-090 to D-100). None needs anything from you now.

**Worth a second look**

1. **Who can see homeowners' private details.** Names, phone numbers, emails, addresses, photos and the homeowner's own message text are kept locked (encrypted). Only five parts of the app may unlock them, and every unlock is logged: sending texts and emails, releasing details to a business after it has paid, connecting a masked call, the homeowner's own page, and your owner portal when you open a "call this person back" alert. The alert itself never holds the number; you see it only when you open the alert. The part of the app that reads requests sees a cleaned copy with names, numbers and addresses removed. A business's quote is the business's own words and is stored with the quote. (D-093)
2. **Where backups and records outside the database go.** Photos are stored, encrypted, in the database. The nightly backup copy and the running log copies need storage somewhere else, which the sources don't name. The plan is a storage bucket you'd create at the account checkpoint. The default is Cloudflare R2, since askhava.com's DNS is already on Cloudflare. It will be on the account checklist. (D-094)
3. **Texts and calls go only to US and Canadian numbers.** Fraudsters type in numbers from some Caribbean area codes (for example Jamaica's 876) to run up texting bills. Those, and every non-North American number, are refused and told to use email. Canadian numbers stay because many visitors use them. Texting Canadian numbers also brings in Canada's anti-spam law, so it goes on the attorney's review list. (D-100)
4. **Security standard gaps we accept.** The security checklist (OWASP ASVS 5.0.0) prefers more than a texted code for sign-in. Homeowners still sign in and pick with a texted code, as the plan says. Businesses on a device they've used before sign in with just the emailed link. Keys are kept in Railway's settings rather than a separate key safe. All three are listed as known risks. (D-099)
5. **Your second factor:** you can use passkeys, an authenticator app (TOTP) or both. You need at least two in total, plus recovery codes kept offline. Having one of each survives losing your phone. (D-098)
6. **A request that never gets an OK** to the consent text closes on its own after 72 hours, with no reminder and no closing text, because nothing but the consent text may go to a number that hasn't agreed. Once someone has said OK, they get the 24-hour reminder and the "Text us any time to restart" message as usual. (D-095) *(Replaced Oct 28, D-419: consent is recorded with the first text, so every request gets the reminder and the closing text.)*

**Routine (how the app is built)**

- One shared base layer holds settings, the clock, the job timers and rate limits. (D-090)
- Each part keeps its own tables, and a test fails the build if one part reaches into another's. (D-091)
- The app can't edit or delete money and audit records once written; only database upgrades can. (D-092)
- Library versions are the newest that had been out for at least two weeks. One exception: the photo library is newer because the older version had known security holes. A security scan of the full set found no known problems. (D-096, D-097)

## From K5 (test design and the twin), October 27, 2026

**Worth a second look**

1. **The six "business left out" warnings were one bug, and it's fixable.** When both AI readings guessed the wrong trade and the homeowner corrected it ("it's the AC"), the twin forgot the words that said what the job was ("AC blowing warm air") and filed it as "other heating/AC". AC-only businesses and those who hadn't ticked "other" then never got the card; three of those requests got no quotes. The build keeps those words. A scratch copy of the twin with the fix had no warnings left in that run; the first build stage proves it with a real test. (D-102)
2. **The twin couldn't see a broken license rule.** It checks licenses against the same table the app uses, so when we broke the table on purpose (AC-only licenses also getting furnace jobs), 81 wrong cards went out and the twin flagged none of them (seen in a scratch copy; the new check's own tests will prove it). The new check uses its own table, and a written list of matching examples (like the fee examples) comes before any matching code. (D-107, D-110)
3. **The `Twin` check will fail on any problem once K8 builds the gate** (until then it is still the old informational run, because those files are protected and only the harness step may change them). It allows only two kinds of exception. A known problem from the old twin is listed one by one until the twin is updated in the first build stage, which must end with none left. The one permanent exception: a simulated homeowner who deliberately answers a question wrongly, and only when the record shows that, up to a set number per run. The whole twin folder is protected, so its checks can't be quietly weakened. (D-106, D-107)
4. **Questions to homeowners get clearer.** A repeated question repeats the choices; "just a small repair" is read as handyman, not heating; up to three trades can be offered; and words like "water heater" always count, so a job isn't sent to the wrong kind of license on one guess or one wrong answer. (D-103, D-104, D-105)

**Routine (how testing works)**

- Tests are written per behavior, edge and failure, each naming the requirements it covers; every requirement is covered by at least one. Edges: texts may go from 8:00:00 a.m. up to but not including 8:00 p.m.; anything that lasts a set time ends exactly at that time (a 10-minute code is refused at 10:00; "within 90 days" means less than 90 full days). (D-101)
- Deliberate-break tests: the 4-month rule, the fee tiers, the license filter, opt-out and quiet hours are each broken on purpose in many small ways, and the tests must catch every one. (D-108)
- Test data uses fictional phone numbers (555-0100 to 555-0199) and example.com emails only. (D-109)

## From K6 (task graph), October 27, 2026

**Worth a second look**

1. **The build is 146 steps: 139 pieces of work and 7 stops.** Phase 1 has 126 work steps: stage 1 fixes the twin and the catalog follow-ups (8), stage 2 the core (49), stage 3 money (18), stage 4 calls, checks and portals (25), stage 5 the website, owner tools and documents (26). Phase 2, with real test accounts, has 13 work steps. The 7 stops are the account checkpoint, the askhava.com page go, the real-texts question, "Finished", Gate B, Gate C and the first live charge. A script checks that every one of the 431 requirements and every planned test is covered by some step, that nothing depends on itself in a loop, and that no two steps that could run in either order change the same file. (D-111, D-119)
2. **Much more Opus than the build plan guessed.** The brief says money, privacy, security and recovery work is done by Opus, so every step touching those (including the clean-ups, the end-to-end journeys and the real-vendor adapters) runs on Opus: 105 of 139 work steps, against the plan's guess of 1 in 10. That uses the plan's limits faster. The rehearsal (K9) measures real usage; if it's too slow, a logged decision can let Sonnet write test-only steps under Opus review. (D-115)
3. **Your stops are steps in the list:** accounts after Phase 1; the askhava.com page go (nothing waits on it, and after your go you get one message with the exact steps to publish it, since Claude can't deploy to the live site); real texts to your phone only if Twilio's test credentials don't work; "Finished"; your click-through (Gate B); going live (Gate C); and the first live charge. Each can only come after the steps before it. (D-116)
4. **Some checks moved earlier than the build plan's stages.** License list checks and business records come in stage 2, because job cards can't go out without them. (D-112)

**Routine**

- Changes to protected files (the twin, stand-ins, workflows, the deliberate-break list, the task-graph checker itself) happen only in steps marked "harness" with an Opus review. (D-113, D-114, D-118)
- The v9.3 blueprint's "build tests T01–T24" are never defined anywhere, so coverage is checked against the test design instead. (D-117)

## From the round-2 simulation fixes ("Fix it all"), October 27, 2026

You said "Fix it all" on the round-2 simulation findings. Every fix is in the spec now, logged as D-127 to D-162. Nothing is being built: Gate P still holds until you say "approved to build". The items below change something you had decided or been shown before, so they come first.

**Changes to earlier decisions: please look at these**

1. **Clean business sign-ups now go live on their own (replaces D-060).** Before, every business waited for your approval. Now a business whose identity code is done and whose license or city-list check passes, with nothing flagged, is switched on automatically, and you see a list of who went live. Anything odd (a name that doesn't match the record, the mailed-letter path, a fraud signal) still waits for you, and your approval alone still never switches anyone on. You can pause any business at any time. Why: in the simulation, approvals were most of your weekly time, and in a crowded launch half the businesses waited more than 18 days. If you'd rather keep approving every one by hand, say so and it goes back. (D-145)
2. **Safe mode no longer stops texts (changes D-075's effect and the brief's wording).** When you're away and a serious alert isn't acknowledged for 24 hours, the app now stops only charging cards and handing out homeowner details. Texts keep going, so safety texts, STOP confirmations and quotes are never held. (D-133)
3. **Backup contact for safe mode: your decision, still open.** The spec still says there is no backup person. Nobody has been named, and Claude won't name one. The working answer: a texting-delivery alert that clears up by itself (the next 50 texts go through fine) is acknowledged automatically and listed for you, so it can't start safe mode while you're away. Every other serious alert still waits for you. If you want someone to be able to acknowledge alerts while you're gone, tell us who and what they may do. (D-134)
4. **More replies count as "yes" to the first text (changes D-021), and people who answer something else get the first text once more (changes D-095).** "okay", "yes please", "Y", "yep" and Spanish "si" (without the accent) now count. Any other answer gets the consent text once more, and someone who never answers gets it one last time at 24 hours. Before, they heard nothing, ever. (D-127) *(Replaced Oct 28, D-419: texting in is the opt-in; no reply OK is asked for.)*
5. **A bare "CANCEL" from someone with an open request now pauses that request (refines D-005).** It still stops all texts, as the law requires, and it still doesn't close the job. But no business can confirm, be charged or get the address until the homeowner picks or closes on their quote page or by email, and they're told the request is still open. (D-129)
6. **Short answered calls now count as talking (changes v9.3's 45-second rule).** A business whose call was answered by a person, even briefly, no longer gets the automatic "couldn't reach the homeowner" refund. In the simulation about 6 in 10 of those refunds went to businesses that had actually spoken to the homeowner. (D-141)
7. **Losing businesses hear sooner (refines D-044).** When the chosen business pays, the others get "The homeowner picked another quote." right away, and "This job is no longer active." still goes when the job ends. The winning business no longer gets that second text. (D-150)
8. **The handyman split-job check is narrower (refines D-081).** The same handyman can come back to the same house within 30 days for a different small job, unless the two jobs share a job type or together reach $1,000. (D-154)
9. **A business owner who texts the main number gets the sign-up link first**, not the homeowner consent text. And someone whose first text is just "HELP" gets the help reply first. These are the only exceptions to "consent text first", besides the emergency safety text. (D-131, D-132)

**Fixes that don't change an earlier decision**

- Night emergencies: a business's quote for an emergency the homeowner chose is texted to them at night instead of waiting until 8 a.m. (D-136)
- More danger words get the safety text at once: buzzing, melting, sizzling, propane, carbon monoxide, CO alarm, and "hot" when an outlet, breaker or similar is named. Plain "hot water heater" doesn't trigger it. (D-137, D-138)
- A failed pick no longer closes the job while other businesses still have time to quote. (D-140)
- A homeowner who chose email contact isn't treated as a no-show because the business emailed instead of calling. (D-143)
- During a texting suspension, emergency messages raise a critical alert, the site banner turns on by itself, and businesses get their notices by email. (D-144)
- The privacy scrubber catches "im steve" and dashed phone numbers, and name-and-address answers never go to the AI. (D-146)
- A homeowner can change their pick before the business confirms; businesses can confirm by texting back "CONFIRM 1042" after seeing the fee; the 2-hour confirm window starts when the pick text arrives. (D-147, D-148)
- A price the AI read from a text is always shown back to the business to confirm. (D-149)
- Smaller items: second free win through a DBA closed (D-151); "join in the first 90 days" means the sign-up date (D-152); no double card charge after a timeout (D-153); "own or rent?" is asked when unsure (D-155); no double texts after a restore (D-156); wider gas word list (D-157); overnight updates merged into one text (D-158); recurring quotes must say per month, week, etc. (D-159); the masked-number pool starts at 10 numbers (D-160); card timers stop while charging is paused (D-135); opt-outs during a texting outage (D-130); quiet-hours rule for unknown area codes dropped (D-139); numbering note (D-161); two pairs of planned tests that couldn't both pass were fixed (D-162).

## From the round-3 simulation fixes, October 27, 2026

The round-3 simulation showed that two of the round-2 fixes caused new problems. Both are now fixed (D-163, D-164). Nothing is being built: Gate P still holds.

**Changes to something you saw in the round-2 fixes**

1. **Texting-delivery alerts are quieter (changes D-134).** The round-2 rule ("fewer than 95% of the last 50 texts") went off on normal noise several times a month, and right away again after you acknowledged one. Now it alerts when fewer than 90% of the last 50, or fewer than 95% of the last 200, texts get through. Only one such alert is open at a time, and it clears itself once 97% of the last 200 get through. The backup-contact question above is still yours to decide. (D-163)
2. **Emergencies reach businesses first, and the questions come after (refines D-146 and D-155).** When someone with an emergency says they're safe and wants urgent service, the job goes to after-hours businesses as soon as the ZIP code is in. "Do you own or rent?" and the name-and-address question come right after. Nothing is released to a business until the address is in. (D-164)

## From the round-4 simulation fix, October 27, 2026

1. **Street names no longer set off the emergency text.** The danger-word list matched "Smoketree Ave", so a homeowner who just typed their address got the "if you smell gas, leave now and call 911" text. Addresses are now skipped by that check. (D-165)

**Still your decision (not changed by any fix):** during a texting suspension, homeowners with no email on file can't get the safety text. The changes file rejected a backup texting number. One option is an automated voice call that reads the safety text, but that has its own consent and cost questions, so it's yours to decide. Until then, emergencies during a suspension raise a critical alert to you (D-144).

## From K9 (the rehearsal), October 27, 2026

The runner tried itself out on three harmless practice tasks before any real building. The full proof is `docs/rehearsal.md`. In short: the passing task merged, the failing task was blocked and never merged, and the task that hit a pretend usage limit was finished after the reset. Nothing is being built: Gate P still holds until you say "approved to build".

**Worth a second look**

1. **Two schedulers are starting runners, and only one should.** (a) "Ask Hava build runner (hourly)", set up through the API, wakes one long-lived session (on Opus) at 7 past each hour. (b) "Ask Hava build runner", the one you made on the routines page, starts a fresh Sonnet session at 50 past each hour. The long-lived session (...W8Ex...) held the lock on the rehearsal, but two fresh sessions from (b) worked anyway: ...W9x5... finished a practice task at 8:11 p.m. UTC and ...W6ng... marked it done at 8:14. The lock holder tried the same task at 8:12, lost the race and stood down. Git stopped a double merge, so no harm was done. The fix: keep (b), delete (a), and treat a lock held by another session as "stop", always. (D-171)
2. **Please check one setting on routine (b), and delete (a).** On the claude.ai routines page, open "Ask Hava build runner" and confirm it has the trigger "Pull request → closed, merged into main"; add it if it's missing. Then delete "Ask Hava build runner (hourly)". Claude couldn't see GitHub triggers from here. The fresh sessions starting right after merges suggest the trigger is there, but that isn't proof. Until it's confirmed, the build counts on the hourly run, about one task an hour, around the clock while limits allow. With the trigger, tasks run back to back. Nothing waits on this. (D-169, D-171)
3. **How fast it goes.** Each kit step took about 10 to 30 minutes, so one task an hour is the expected pace without the trigger. How much of your plan's limit that uses can't be seen from inside a session; the rehearsal measured tokens per model instead, mostly from K0 (`kit/schedule.json`). (D-169)

**Routine**

- The model check works: each worker's record shows exactly which model ran. The call that starts a worker takes "haiku", "sonnet" or "opus" rather than the full model name, so the runner checks the full name in the record. (D-170)
- Stripe's test server can't run inside a session (no Docker there), so it runs only in GitHub's checks; sessions use the build's own fake Stripe. (D-167)
- Browser tests can run inside a session; the browsers are already installed. (D-168)
- The runner ran the rehearsal itself, without a separate task prompt. Two practice tasks skipped the second-reviewer step (each was a 6-line note), the failing one wasn't retried (it fails on purpose, and the retry path is already tested offline), and two practice tasks were open at once (safe, because the failing one could never merge). The pickup after the pretend usage limit was done by sessions that ignored the lock, so the runner's own pickup is proven only by offline tests. This report had one round of Opus review. (D-166)

## From the business-side fix plan ("Make all the changes you recommend"), October 27, 2026

You approved every recommendation in the business-side fix plan. All 16 quick fixes (section 3A) and all 20 yes/no answers (section 3B) are now in the spec, with a test design and task for each. Nothing is built, and Gate P still holds. The plan numbered its decisions from D-166, but the rehearsal had already used D-166 to D-171, so these are D-172 to D-207. The frictions came from 29 AI-simulated businesses, not real people.

**Open items for you**

1. **The safety voice call (B19): now the AI legal-research memo (in progress, `/home/claude/hhq/outputs/legal/`) + your decision (D-367).** During a texting suspension, a homeowner with no email whose own message matched a danger word gets one automated call from the Ask Hava number that reads the safety text. It rests on the homeowner's own just-sent request, not on the narrow "emergency purposes" exemption. That reading is a research-based judgment, not legal advice. Treat it as unconfirmed; the call stays off (CALL-019) unless you later ask for a spec change after reading the memo. Your critical alert still fires either way. (D-206)
2. **Fix the pitch and flyers by hand before any flyer goes out (A1, A13).** The prototype and the go-to-market pitch live outside this repository, so only the spec could be changed. The spec now stops the build from copying the errors (WEB-018, WORD-017). By hand, change these: ongoing services count as 4 months; remove the "What a lead can cost elsewhere" table, or keep only IRS, SEC or FTC figures with the source on the page; remove the pest row; use "Launch offer: your first win is free, on any job size, when you join in the first 90 days or within 90 days after your trade opens."; remove "Skip tenant requests" and the identity "Email me" choice; replace "no bidding wars" with "You quote once. No bidding rounds, no paid placement."; add "Every card says how many businesses got it." and the five objection answers from friction report section 8.2 that match your answers. (D-172, D-184)
3. **Recruit about 2 to 4 businesses per trade** until a trade averages about 1 confirmed pick per business per month. Open small trades later, or with 1 or 2 businesses. Put this pace in the go-to-market plan, section 3.3. There is still no cap, and a business that finds Ask Hava on its own can still join. If a trade ever has 7 or more businesses and its typical job gets 5 or more quotes two months in a row, the owner portal flags it, and a 5-quote limit comes back to you as a decision. (D-189)
4. **Start the founding interest list yourself (A10).** Note each business, its trade, how it would like to be reached and its OK to be contacted once at launch. Then sign each one up in person during launch week. Nobody can sign up before Gate C, and the list must never be built from the ROC or city lists. (D-181)
5. **The business data page (B16): now the AI legal-research memo (in progress, `/home/claude/hhq/outputs/legal/`) + your decision (D-367).** Every line on it must be exactly true. It covers what's kept and for how long, who sees it, no call recordings, never asking about workers, police or others getting data only when a law requires it (such as a subpoena or warrant), and that you don't use Ask Hava data in any other job. (D-203)
6. **Whether to mention your day job (C8).** The "Who runs Ask Hava" block leaves it out until the interviews answer this. (D-183)
7. **Checked at build time:** whether the city license list has a date field. If it doesn't, no-license cards say "City business license on file" with no year. (D-190)

**Pending validation: needs real contractor interviews (section 3C, not applied)**

None of these blocks the spec. Each touches a number that is now a setting, or a wording choice, so interview answers change numbers, not code. The "Q" numbers refer to the interview script in friction report section 10. Interview rules: research, not selling; ask about past behavior; no recording without permission; notes by business type only in any sheet near an AI tool; never use the ROC or city lists to choose whom to interview.

- **C1. How stale are ROC and city-list contacts, and does the ROC's online update fix them?** Could reopen the in-person check (B7), and must be answered before the first sign-up. Q14, plus new: "If the code went to an old number, would you update your ROC record online?"
- **C2. Reaction to "Sent to __ businesses": what number makes them skip, and what feels fair by trade?** Sets the pacing target and the revisit trigger (B1, B2). Q6, Q5. Must be in before launch.
- **C3. Will they sign up on the spot with you during launch week?** This is the funnel's biggest assumption (A10). Q15. Must be in before launch.
- **C4. Does "licensed since" matter, and what would they write in "About us"?** (B3) Q8.
- **C5. Who reads the pick text, and is 2 business hours workable with every phone notified?** Could reopen B4 or B5. Q4, plus new: "If the pick text went to you and your office at once, could one of you confirm within 2 hours?" Must be in before launch.
- **C6. What is a new recurring client worth, and is $75 acceptable for an ongoing service?** Could reopen B9. Q12.
- **C7. What Spanish-first owners need in Spanish, and what they want promised about data.** (A9, B16) Q18.
- **C8. Is your having a full-time day job a plus, a worry or nothing?** (A12, B16 wording) New: "The person running this has a full-time day job. Does that matter to you, either way?"
- **C9. Would BUSY <days> keep them signed up through the busy season?** (B11) New: "When you're booked out, what do you do about new requests today?"
- **C10. Minutes per quote, and how many unanswered quotes before they stop.** (B1, B2 pacing; B6) Q7, Q9. Must be in before launch.
- **C11. Would the loss text ("4 quoted; the picked quote was lower") make them quote again, or quit?** (B6) Q9; show the loss text on paper.
- **C12. Movers and others who price by the hour: what quote format works?** (post-launch item L4) New, movers only: "How do you price a local move in writing?"
- **C13. $25 locked for a year against a possible later $39: does the lock matter?** (B10) Q11.

**Worth a second look**

1. **Every job card now says "Sent to N businesses."** The business portal also shows how many got the job and how many quoted so far. There is still no cap. (D-188, D-189)
2. **Losing businesses learn a little more.** When 3 or more businesses quoted, the "picked another quote" text adds how many quoted and whether the picked quote was lower than, about the same as (within 10%) or higher than theirs. It never gives amounts or names. A "Your numbers" panel shows each business its own month. (D-193)
3. **Team members.** An owner can add people with "Cards and quotes" and/or "Confirm and billing". The pick text goes to the owner and every "Confirm and billing" phone and email, with one reminder after 60 minutes. A texted CONFIRM counts only from those phones. Each person signs in separately, and each team phone gives its own texting consent and has its own STOP. (D-191)
4. **The Confirm window stays 2 business hours,** now as a setting. If late answers stay above 5% after the plan's validation run, it moves to 3 hours for non-emergency jobs. (D-192)
5. **Ongoing services are capped at a $75 fee.** A weekly cleaning that used to cost $199 is now $75; a $150-a-month pool account stays $75. The worked examples changed to match: the $400-a-month turnover cleaning (W20) is now $75, and a new $160-weekly cleaning (W33) shows the cap. Businesses accept a new card-authorization version before their next charge. (D-196)
6. **The fee stays $25 / $75 / $199, with no move to $39.** Businesses that join in the first 90 days after launch keep their join-date amounts for 12 months. Revisit $39 after 6 months of real data. (D-197)
7. **The free first win also counts from when a trade opens.** A business qualifies if it joins within 90 days after one of its trades is switched on. That includes a waitlisted business that joined earlier. (D-195)
8. **A failed license lookup now waits 72 hours, not 24, before a pause.** The business is warned at 24 hours and can't Confirm while its status is unknown. A pause that clears itself needs no review from you. (D-199)
9. **No in-person ID check (B7: no), and no sharing of the homeowner's number (B14: no).** Instead, sign-up shows the masked record contact first and calls landlines by voice first. "That number is old" walks the business through updating its ROC or city record and resumes on its own. The homeowner's details text adds "Tip: on your first call, ask which number to text." (D-178, D-194, D-201)
10. **You are named as the contact for charge questions and sign-up reviews,** with a 1-business-day reply target. An "away until <date>" setting tells businesses when you'll answer, and it changes no timer. Businesses can set a monthly fee limit. There is still no backup person. (D-204, D-207)

**Routine**

- Sign-up starts with the trade list marked "Open now" or "Coming later". Coming-later trades join the waitlist and are told "Your join date is kept." (D-175, D-176)
- A no-license business not yet on the city list waits as pending and activates on its own when the list shows it. (D-177)
- Anyone can fill in the sign-up form, and the owner just reads them the code. (D-179)
- A plain-words "How you're charged" box appears on sign-up and on the business page. It says "It's an introduction fee for a homeowner who chose you." and "A new fee never applies until you accept it." The fee still stands once the business has talked to the homeowner. (D-173, D-205)
- Businesses get call reminders 24 and 48 hours after a release when they haven't called, so nobody loses a refund without warning. (D-174)
- Spanish business page, sign-up and terms summary, marked for native-speaker review. (D-180)
- A sample job card for each open trade (never pest), and a "Who runs Ask Hava" block that uses the LLC mailing address, never your home. (D-182, D-183)
- The quote form shows the trip-fee tier before a business submits. The handyman certification box sits next to the price. (D-185, D-186)
- An honest done screen: "You're one of <n> businesses in <trade> so far." (D-187)
- Quote cards show "ROC license since <year>" and a screened 140-character "About us" line. There are no ratings or hire counts. (D-190)
- Intake answers, such as tank type or window count, show as fields on job cards. (D-202)
- PAUSE, RESUME and BUSY <days> text keywords. STOP from a team phone stops only that phone. (D-198)
- Businesses see each complaint, with no homeowner details, and have 5 days to reply. (D-200)
- A business data page, in English and Spanish. (D-203)

## From the three-angle fix plan ("do whatever research you need"), October 28, 2026

You asked for research and the most ideal changes before the simulations run again. The business, homeowner and owner reports are now one plan, `/home/claude/hhq/outputs/sim_v7/Three_Angle_Fix_Plan.md`, and its changes are in the spec as D-208 to D-269. Nothing is built, and Gate P and build_hold are unchanged. The order used was: safety and legal fit first, then your time, then homeowners, then businesses, then profit. The frictions came from AI-simulated people, not real ones. Nothing in this round spends money, adds a vendor or signs an agreement. Those items are listed below for you to decide.

**To buy, sign or decide (the spec already works without each one)**

1. **An answering service for call-backs (recommended).** Example: Ruby, at about $250 a month for 50 minutes or $395 for 100 minutes. Those are published prices, reported by a competitor in August 2026, so check them first. It would save about 7 to 16 minutes a week, and almost all the work during a texting suspension. The trade-off: it becomes a data processor that sees homeowner phone numbers, so it needs a privacy-policy line and a confidentiality agreement. **Without it:** you make call-backs within 4 days, or within 24 hours for a job marked urgent or emergency. (D-226)
2. **Sign the zero-retention AI agreement.** This is the one change most simulated businesses asked for, because it lets photos go on job cards after screening. **Without it:** cards have no photos (CARD-013).
3. **A maintainer on retainer** for the license-list reader and updates. The cost is unknown. **Without it:** while a list fails, a live lookup runs for each business, so statuses stay current, and you get an alert at 12 hours. (D-230)
4. **5 more masked numbers** (10 to 15), at a few dollars a month. **Without them:** numbers are shared with PIN routing in busy weeks. (D-231)
5. **Register the masked numbers** with the Free Caller Registry and the carriers' portals, and optionally buy branded caller ID (paid). The aim is to stop the first business call showing as "Spam Likely". The registry appears to be free, but it's an account you'd create yourself. Test it first: call 5 to 10 friends on different carriers and ask what their screen showed.
6. **Now the AI legal-research memo (in progress, `/home/claude/hhq/outputs/legal/`) + your decision (D-367).** It covers the safety voice call, which now also goes when an email is on file (D-206, D-210), and whether a safety call to a number that replied STOP could ever be allowed (it is not done now). It would also cover the heat text (D-208) and the tenant text (D-216). The wording is generic information either way.
7. **Your department's outside-employment rules.** The plan assumes no Ask Hava work on duty beyond a 1 to 3 minute acknowledgement at a break.
8. **Before launch week:** ask each founding-list business to look up its own ROC or city record on its own phone and fix the phone and email. Never use the lists to do this for them.

**Fix the pitch by hand (D-268).** The pitch file isn't in this repository: `/home/claude/hhq/outputs/gtm/Ask_Hava_GoToMarket_Plan_REVIEWED.md`.
- **"No bidding wars"** is still on lines 159, 194, 312 and 391. Use "You quote once. No bidding rounds, no paid placement."
- **Line 159** ends "Can I leave this card and check back next week?" Ask instead to add them to the founding list and sign them up together in launch week.
- **The ongoing-services line** on lines 129, 177 and 398 needs "capped at $75".
- **The free-win line** on lines 159, 180, 195, 234, 235, 416 and 559 should read "Launch offer: your first win is free, on any job size, when you join in the first 90 days or within 90 days after your trade opens."
- **The pest answer** on lines 218 and 223 says "not at launch". The spec lets licensed pest operators join as a neutral item in the sign-up list, and pest control is never advertised. Say "Licensed pest operators can sign up; Ask Hava doesn't advertise pest control."
- **Worth adding:** "The homeowner sees every quote and picks one business: you."

**Worth a second look**

1. **Your week.** You work in one 30-minute window plus a 10-minute mid-week check. Your phone only buzzes for four things:
   - a safety alert where the automatic call reached no one;
   - the app still down 15 minutes after its automatic restart;
   - a Twilio suspension;
   - a money mismatch that is still there the next night.

   Everything else comes in a weekly digest email. The owner simulation puts this at about 20 minutes a week with an answering service, and about 25 to 35 without one [sim]. (D-218, D-219)
2. **Sign-up reviews and charge questions now have a 4-day target**, not 1 business day, counted in calendar days. Fewer sign-ups get flagged now, because a city-list match on the trade name or owner name counts, and the waitlist flag is gone. Businesses get status updates at their own email and phone. (D-223, D-253, D-255, D-256)
3. **Heat.** Every request asks "How soon do you need this?" A broken AC or cooler in the heat gets a heat-safety text at any hour, then the emergency path. That text says to call 911 for confusion, weakness or fainting, and to get somewhere with AC. It never points to 211, which lost its live operators in August 2026. (D-208)
4. **HOLD.** On a non-emergency job, the owner or a "Confirm and billing" phone can reply HOLD once to add 2 business hours. The homeowner is told when they'll hear, and can still choose another quote. (D-252)
5. **Quote facts.** Every quote now states the same short list:
   - whether the trip fee counts toward the repair;
   - whether parts are included;
   - whether a permit is included;
   - the earliest visit;
   - whether they speak Spanish on the phone or on site.

   The business name shows on each quote, with the same "check it yourself" line on every card. Homeowners can't send free-text questions; this replaces that idea. (D-233, D-234)
6. **Call windows and missed calls.** The homeowner picks a call window. Only call attempts inside it count toward the "couldn't reach them" refund. After the first missed call, the homeowner gets a text with the number to call back. (D-235, D-236)
7. **Complaints** come to you once, after the business's 5-day reply window, unless they mention a safety word. A PROBLEM reply tells the homeowner a person reads it within 7 days. (D-222)
8. **The owner is told** whenever a team member confirms a pick or links a phone. Only the owner and "Confirm and billing" phones can PAUSE, BUSY or HOLD. Removing a member unlinks their phones. (D-258, D-259, D-260)
9. **No "hired no one" credit yet.** Instead, a trade is flagged for your monthly look if more than 30% of its connected jobs end with "didn't hire" for 2 months running. (D-251)

**Routine**

- Gas texts say to call the gas company before getting quotes. (D-209)
- Every pick asks who will let the business in. Ask Hava never stores lock codes. (D-212)
- The "who's calling" text adds "get changes in writing, don't pay in full up front." (D-213)
- Privacy:
  - plainer privacy lines and a homeowner data page;
  - the business terms limit the homeowner's details to that job;
  - the business data page adds notice of legal demands and retention in days. (D-214, D-215, D-217)
- Tenants get a "your landlord may be responsible" note. (D-216)
- Fewer alerts and checks:
  - alerts acknowledge themselves when they clear;
  - the spot check is monthly;
  - receipts link a "Why was I charged?" page;
  - dispute evidence is filed for you 2 days before it's due;
  - unreadable business texts get an automatic reply. (D-220, D-221, D-225, D-228, D-229)
- Calls to the Ask Hava number get a short recorded greeting, with no voicemail and nothing forwarded to you. (D-227)
- Homeowner texts:
  - a plainer first text that says texts are automated and HELP reaches a person;
  - more agreement words accepted;
  - LIST sends the quotes by text;
  - expectation texts ("Sent to 6 businesses…");
  - check-in replies LATER, ONGOING and OTHER. (D-237, D-239, D-240, D-241)
- Uncovered jobs are refused right after consent, and a "What we cover now" page lists what's covered. (D-242)
- Businesses:
  - the fee shows on the quote form for every quote;
  - "Several businesses quoted" replaces the count in small trades;
  - waitlisted businesses get a notice when their trade opens, and their price lock starts then;
  - "Your numbers" adds picked → finished;
  - a 3-line fee summary sits on top of the box. (D-248, D-250, D-261, D-262, D-263)
- Not adopted: see plan section 3 (D-269).

**New things to test with real people**

- **Homeowners:**
  - Do the quote facts answer what they'd want to ask?
  - Do they answer the first call after a "who's calling" text?
  - Would LIST help someone who doesn't open links?
- **Businesses:**
  - Would they use HOLD?
  - Do they want the owner alert?
  - Is 4 days for a sign-up review acceptable?

## From fix round 8 ("Ok fix everything we need to"), October 28, 2026

You asked to fix everything the round-7 simulations and the ChatGPT and Grok research found. The plan is `/home/claude/hhq/outputs/sim_v8/Fix_Round_8_Plan.md`, and its changes are in the spec as D-270 to D-302. Nothing is built, and Gate P and build_hold are unchanged. The order used was: safety, your time, businesses, homeowners, then legal fit from the research. The texting channel itself (which messages go by text, the Twilio campaign wording) is left for the separate messaging review. The frictions came from AI-simulated people, not real ones. Nothing here spends money or signs anything.

**To decide, ask or buy (the spec already works without each one)**

1. **Ask Twilio in writing whether Ask Hava's flow is allowed.** (Now drafted at the top of this file; see "Your first item", D-305.) Describe it exactly: anonymous job cards go to several opted-in businesses, quotes are shown unranked, the homeowner picks, and the picked business pays a fixed introduction fee before it gets the first name and address. Save the case number and their answer. The research's biggest risk is that this gets classed as "third-party lead generation". (D-302)
2. **TCPA questions: now the AI legal-research memo (in progress, `/home/claude/hhq/outputs/legal/`) + your decision (D-367).** The automatic safety voice call stays **off**: its switch still needs a recorded attorney approval, and since you're not hiring one now, it stays off unless you ask for a spec change. Every emergency message during a texting outage pushes to your phone. Questions for the memo:
   - Does STOP also cancel calls?
   - What does the FCC opt-out rule reported to start **Nov 25, 2026** mean for Ask Hava (as reported by research; verify the date)?
   - Can you call after a texting suspension?
   - Is keeping a scrambled (hashed) phone number enough proof of texting permission?
   (D-275, D-298)
3. **Sales tax (TPT): now the AI legal-research memo (in progress, `/home/claude/hhq/outputs/legal/`) + your decision (D-367).** The introduction fee may not be taxable at all, so don't get a TPT license just in case. You can still send ADOR (the state, not a paid adviser) the exact question in the research (Part 4 §3); a TPT license, if needed, is your filing. If they say it isn't taxable, the receipt line "Tax included." (D-225) should change; tell Claude and it will. (D-302)
4. **Stripe's written OK** for the introduction-fee flow and merchant category, before the first charge. (D-302)
5. **Insurance quotes (still recommended; insurance is cover, not legal advice, D-367).** Get general liability and tech E&O quotes, and maybe cyber. The research estimates about **$1,300 to $5,450 a year** in total; that's a planning range, not a quote. Tell the insurer the whole model (AI intake, quotes from several businesses, masked calls, addresses, the safety texts), and ask in writing what is excluded. (D-302)
6. **Setting up the LLC.**
   - A statutory agent hides only the agent's address. Member or manager addresses may still be public, so check what address you can lawfully use before filing.
   - An agent in Maricopa or Pima County avoids the Mohave newspaper notice (three publications within 60 days).
   (D-302)
7. **City business license.** $196 new and $140 renewal on the 2026 schedule. The file is still labeled "proposed", so verify with the city. File the home-occupation form too. (D-302)
8. **FinCEN beneficial-ownership (BOI) report.** Not required for a US-formed LLC under the Aug 2026 rule; verify before you skip it. (D-302)
9. **Trademark: now the AI legal-research memo (in progress, `/home/claude/hhq/outputs/legal/`) + your decision (D-367).** Bring Grok's list to the memo; filing a trademark, if you choose to, is your own filing.
   - Several local businesses use "Hava" (Hava Plumber LLC, Hava Handy Services LLC, Hava Pool LLC, Hava Landscape LLC and others). Homeowners could think Ask Hava is one of them, or the reverse. Ask how much that local confusion matters.
   - Also ask about the HAVA (diet app), HAVAWORKS (pending marketplace filing) and ASKAVA records.
   - Always write "Ask Hava" as one phrase.
   (D-302)
10. **Pilot trades (your call).** Grok's research suggests starting with HVAC, electrical and a licensed handyman, then adding pool when it's warm. The spec's launch list hasn't changed; decide it yourself. (D-301)
11. **A fire-safety desk check** of the new carbon-monoxide, electrical and water lines, alongside the heat text. (D-272, D-273)
12. **PROBLEM reports (your call).** Should a PROBLEM report count as a complaint and go to the business for a reply? It would mean more of your time and affect the 3-in-90 pause. (D-301)
13. **Founding list.** Contact each business twice: a "check your ROC phone and email" reminder about 2 weeks before launch, then the launch notice. Get their OK for both when they join the list.

**Worth a second look**

1. **Night emergencies.** After a homeowner picks "emergency" at night:
   - The call window defaults to "Now".
   - The business has 60 clock minutes to confirm, at any hour.
   - Missed-call texts go at any hour.
   - Before the choice, the homeowner is told how many businesses in that trade take after-hours calls. If none do, they're told so plainly, with "call 911" if anyone feels unwell.
   (D-270, D-271)
2. **Carbon monoxide.** On any CO alarm or symptoms, the text now says to get outside to fresh air and call 911 from outside, because CO has no smell. Gas texts add "Don't flip switches or light anything." Electrical and water problems get their own short lines. (D-272, D-273)
3. **Your phone.** At most two daytime pushes, at 9 a.m. and 3 p.m., and only when something is open:
   - urgent call-backs;
   - urgent HELP messages;
   - safety-word complaints;
   - "texting restored";
   - a license list still failing after a day.

   They never break Do Not Disturb. Safe mode now starts only from alerts that actually reached your phone. The portal keeps your two windows no more than 4 days apart. (D-277, D-278, D-279)
4. **Complaints** need your decision only when a business has 2 in 90 days, or when a safety word appears. Letter-path sign-ups are no longer flagged once the letter's code is entered. (D-280, D-281)
5. **The HELP promise is now exact:** "A person reads HELP messages within 4 days, or within 24 hours if your job is urgent." The HELP reply gives the help email and page, and never the texting number as a phone line. (D-289)
6. **"Away until".** A business can set itself away for up to 4 business hours, so a non-emergency pick waits for it. The pick text now names HOLD. (D-284, D-285)
7. **Refund rules before paying.** Before Confirm, the business now sees the homeowner's call window and the refund rule. A missed-call text that was attempted counts, even if the carrier failed to deliver it. (D-286)
8. **Web-and-email mode** can now run the whole service with no texts at all. A launch setting can start it that way, and every main journey has a test that sends zero texts. (D-297)
9. **Masked calls** use standard programmable voice, never Twilio Proxy (a beta with no service guarantee). (D-296)

**Routine**

- Quote facts:
  - pre-filled from the business's last answers, with a one-tap "Same as last time";
  - earliest visit now includes the time of day;
  - a new "price may change after seeing the job" fact;
  - a free-estimate definition.
  (D-283)
- Homeowner texts:
  - LIST is mentioned in the first-quote text and the HELP reply;
  - "2 SÍ" works;
  - a text pick asks for the call window;
  - "How soon?" has "By a certain day";
  - times show the homeowner's own time zone;
  - TIMES changes the call window;
  - FIRST lets the on-site person be called first;
  - the check-in text is shorter.
  (D-290 to D-295)
- Businesses:
  - charge questions get the charge details in the reply;
  - About-us lines can't say "find us on Google";
  - a staff-linked phone needs the owner's YES;
  - the waitlist says "no priority";
  - a per-job call log.
  (D-287, D-288)
- Privacy policy:
  - says exactly what goes on a job card (no name, phone, email or street);
  - says the picked business gets the first name and address only after it confirms and pays.
  (D-299)
- Banned words:
  - "lead", "leads", "pre-vetted" and "exclusive" are now banned, alongside "vetted" and "concierge";
  - the only exclusivity line allowed is "Only the business you pick gets your contact info.";
  - Grok's suggested "home-help concierge" and "pre-vetted, exclusive intros" pitch lines can't be used.
  (D-300)
- Not adopted: see plan section 3 (D-301).

**New things to test with real people**

- **Homeowners:**
  - At 10 p.m., what do they expect after choosing "emergency"?
  - Do they notice and use LIST?
  - Is the shorter check-in text clear to people over 70?
- **Businesses:**
  - Would they use "Away until"?
  - Is a one-tap "Same as last time" safe for their trade, or do the answers change job to job (interview C22)?

## From the messaging review ("If Twilio is going to be a problem look for other options"), October 28, 2026

You asked to fix everything needed and to look at other options if Twilio could be a problem. Then you added: "we also want email as an option. I'd like the business to choose email, text or both, but if text isn't possible we can do email and then later maybe an app." The research is `/home/claude/hhq/outputs/messaging_options/Messaging_Risk_and_Options.md`.

Its answer: Twilio could be a problem for **texts to businesses**, and switching providers doesn't help, because every US provider goes through the same registry and carrier review. The spec now uses its **Option A, "split by audience"**, with your email change, as D-303 to D-322. Nothing is built, nothing was sent or signed up for, and Gate P and build_hold are unchanged.

**What changes, in short**

- **Homeowners:** no change. They text Ask Hava exactly as before.
- **Businesses get email at launch.** Job cards, picks and reminders come by email. Each email has secure links to the right page on askhava.com. (D-307, D-308)
- **The pick call.** When a homeowner picks a business, Ask Hava also **calls its phone** with a recorded message. That way the 2-hour Confirm window still works for someone on a ladder. (D-310)
- **Email, text or both.** Each business picks one at sign-up and can change it later. Text and "both" appear only after you switch business texting on, and you can switch it on only after Twilio says yes in writing. It then runs on a separate campaign and number. Until then, everyone gets email. (D-303, D-307)
- **The app comes later, if at all.** An installable business app with phone notifications is written down as an optional later phase. It isn't built for launch and blocks nothing. If it's added, "app" becomes a fourth choice next to email and text. (D-307)
- **No homeowner name or address is ever texted to anyone.** The picked business sees them only on askhava.com and in email. (D-306)
- **The campaign wording is fixed.** It used to say businesses send no messages on the campaign, while the spec texted them from the same number. Now it's true, and it adds "Businesses get no texts on this number; they are notified by email." (D-304)

**To decide, ask or do (the spec already works without each one)**

1. **Send the Twilio question** at the top of this file. Send the Telnyx one only if Twilio's answer is unclear. Sign as "Casey Solomon, Ask Hava (LLC being formed)" *(now "Ask Hava (LLC being formed)", no personal name, D-424)*. This reverses the earlier "don't ask Twilio first" choice (D-088). (D-305)
2. **Email setup at the account checkpoint.** Set up the askhava.com email records (SPF, DKIM, DMARC) with the email provider (Resend), so business emails don't land in spam. (D-318, D-322)
3. **No longer needed:** this asked whether the sign-up checkbox covered recorded pick calls. There are no business calls now (D-344), and legal questions go to the AI legal-research memo (in progress, `/home/claude/hhq/outputs/legal/`) + your decision. (D-312, D-322, D-367)
4. **Later, your call:**
   - whether to ask for business texting at all;
   - whether to build the app.

   Email plus the pick call covers everything without either. (D-322)

**Worth a second look**

1. **Every old text command now has an email link** to a page on askhava.com:
   - Quote and Confirm;
   - Hold for 2 more hours;
   - Away until;
   - Pause and Resume, and Booked for a number of days;
   - Link this phone, and Approve a phone;
   - Ask about a charge.

   The same team roles apply, and Confirm still needs the business to be signed in. The Hold link works once, for that pick only. Replying to an email never does anything, because email senders can be faked. A reply is read as a question, like an unclear text. (D-309)
2. **The pick call** says: "This is Ask Hava LLC. You were picked for job 1042. Check your email or askhava.com to confirm by 2 p.m. Press 1 to hear this again. Press 2 for 2 more hours."
   - It goes to the owner's phone, and to "Confirm and billing" team phones that turn it on.
   - A second call goes with the 60-minute reminder.
   - It never names the homeowner, the address or the fee.
   - Quiet hours apply, except for emergency jobs as before.

   (D-310)
3. **The 2-hour Confirm window** now counts from when the pick notice goes out (email, call and any text together), not from when a text was delivered. (D-310)
4. **Verified email.** A business must confirm its email address before it's activated. If its emails bounce, the portal shows a warning and you see it in the weekly digest. The pick calls keep going. (D-312)
5. **Your own alerts** used to be texts. Now they come by email to an alert address that the owner guide sets to notify you on your phone. A critical safety alert also calls your phone from the Ask Hava number. The owner guide tells you to let that number through Do Not Disturb, so safety alerts still break through. (D-314)
6. **STOP from a business phone** now stops texts and pick calls to that phone. The business stays active on email, and the owner is emailed how to turn calls back on. (D-311)
7. **If Twilio refuses the homeowner campaign,** the fallback is a toll-free number for homeowner texts only, with your go. It's never used to get around a suspension. Until then, the site runs in web-and-email mode. (D-315)

**Routine**

- Codes (pick, identity, sign-in) go only through Twilio Verify, which needs no campaign. (D-313)
- "Affiliate" (outside the required privacy sentence) and "partner" are now banned words too. By default, the business page says "Job requests come by email, and we call you when a homeowner picks you. We don't text businesses." (D-316)
- Accessibility covers the email links, the pages they open and the pick call. Your Gate B VoiceOver script adds confirming from a pick email. (D-317)
- New security rules for business emails and address changes, plus new rows in the threat model. (D-318)
- New alerts in your portal (never pushed to your phone) if business emails bounce or pick calls fail. (D-319)
- Two new build tasks (T147 and T148) build the email notices and the links and pick call. (D-320)
- Not adopted now:
  - the app at launch;
  - switching providers;
  - a short code;
  - RCS;
  - WhatsApp;
  - Apple Messages;
  - a native app.

  (D-321)

**New things to test with real people**

- **Businesses:**
  - Is email fast enough for job cards, or do they miss them during the workday?
  - Do they notice the pick call, and is "press 2 for more time" clear?
  - Would they pay attention to texts more than email, if business texting is approved later?

## From fix round 9 ("Ok fix everything we need to"), October 28, 2026

You asked to fix everything the round-9 simulations found and to do whatever research the changes need. The plan is `/home/claude/hhq/outputs/sim_v9/Fix_Round_9_Plan.md`, and its changes are in the spec as D-323 to D-343. The order used was: safety, then the business email frictions, then homeowners. The owner-angle round-9 simulation was still running, so its results come in a later round. The frictions came from AI-simulated people, not real ones. Nothing is built, nothing was bought, sent or signed, and Gate P and build_hold are unchanged. Business texting stays off until Twilio says yes in writing.

**To decide, ask or buy (the spec already works without each one)**

1. **Superseded by fix round 10 (no business calls, D-344) and round 11 (the AI legal-research memo (in progress, `/home/claude/hhq/outputs/legal/`) + your decision, D-367).** Kept for the record: business automated calls never needed a lawyer switch. The safety voice call to homeowners is the only call behind one. So the new calls go on the same footing as the pick call, and the question already on your list (D-322) now covers all of them:
   - recorded pick calls;
   - the new **emergency-job calls**, including at night to businesses that ticked after-hours. These have their own optional, unticked box that says "Not required to join.";
   - the **test call** at activation;
   - the **account call** when a business's emails stop working.

   Also ask whether you may phone a homeowner yourself, live, during a texting suspension. The owner guide would add that step only if the lawyer says yes.
   (D-323, D-334, D-335, D-343)
2. **Branded caller ID (costs money, your call).** Registered caller name (CNAM) or Twilio's branded calling for the Ask Hava number. Without it, the "Save as Ask Hava" contact card still shows the name on phones that saved it. (D-334, D-343)
3. **Inbox placement test, before the first sign-up.** Send the real card, pick and verification emails from the production sending address to test inboxes you own: Gmail, Outlook.com, Yahoo and iCloud. Add a Microsoft 365 business inbox only if you already have one. Note whether each one lands in the inbox, a tab such as Promotions, or Spam. Use free accounts only, and no real business addresses. (D-343)
4. **Launch week.** Walk each founding business through the new "Did the sample card arrive in your inbox?" step in person. (D-328)
5. **When to build the app with phone alerts (your call).** Suggested trigger: build it first after launch if, in the first 60 days, more than 25% of job cards to active businesses end without a quote, or more than 3 of the first 10 businesses say they missed emails. The new missed-card numbers supply the data. (D-331, D-343)
6. **The pitch file** (`/home/claude/hhq/outputs/gtm/Ask_Hava_GoToMarket_Plan_REVIEWED.md`, outside the repo) still says:
   - "no bidding wars";
   - "a text-based quote service";
   - "check back next week".

   Use the business page's line instead: "Job requests come by email, and we call you when a homeowner picks you. We don't text businesses." When someone asks "Can I get jobs by text?", the honest answer is "Not yet; you get an email and a phone call." (D-343)
7. **Fire and gas desk check.** Add these to the review:
   - the new smoke-alarm line;
   - the "Has your gas company checked it?" question;
   - the wider keyword list.

   Also ask 10 or more people, in English and Spanish, how they'd text about a gas leak, a burst pipe, a CO alarm and a dead AC, and check each phrase against the list. (D-325, D-326)

**Worth a second look**

1. **Emergency job cards no longer rely on email alone.** For a job the homeowner marked as an emergency:
   - The email subject starts "Ask Hava: EMERGENCY".
   - Ask Hava also places one recorded call: "This is Ask Hava LLC. New emergency HVAC job in Lake Havasu City, job 1042. The homeowner chose emergency. Check your email or askhava.com to quote."
   - At night only businesses that take after-hours calls get the email and the call.
   - A business can tick "after-hours emergencies" only while at least one of its phones has these calls turned on.

   (D-323)
2. **More safety words are caught**, in English and Spanish:
   - gas leaks and hissing gas lines ("huele a gas", "fuga de gas");
   - burst or broken pipes ("se reventó la tubería");
   - "detector going off" and "CO is beeping";
   - fire and flames;
   - electric shocks;
   - headaches and "dolor de cabeza".

   A sounding alarm gets the get-outside lines. A detector that only chirps is a low battery and isn't treated as an alarm. After SAFE on a gas problem, one question asks whether the gas company has checked it, and the card shows the answer. (D-325, D-326)
3. **Emergency picks.**
   - The 30-minute reminder also calls.
   - "You can choose another quote" now appears only when there is one.
   - A pick made just before 8 p.m. of a business without after-hours calls warns the homeowner.
   - With a single after-hours business, the night count now reads "1 HVAC business ... takes".
   - The 60-minute "no quote yet" text now also goes in the daytime.

   (D-324, D-327)
4. **Missed card emails.**
   - At sign-up, a sample card email asks "Did it arrive in your inbox?" and shows how to fix Gmail, Outlook, Yahoo or iPhone Mail.
   - Each person can add a backup email address.
   - A daily "Your job requests still open" email goes out (5 p.m. by default, and it can be switched off).
   - A monthly note goes out only when at least half of a business's cards ended without a quote.
   - The 24-hour quote window still counts from when the card is sent. Counting from when the email is "opened" can't be measured honestly: Apple Mail opens tracking pixels on its own, and mail scanners open links.

   (D-328 to D-332)
5. **Fewer sign-ins.**
   - The Quote link in a card email now opens the quote form without signing in. It's tied to one person and one job and ends with the card's window.
   - Confirm from your own pick email now needs only a phone code.
   - Opening a link never does anything by itself; you always press a button on the page. This stops office mail scanners from using links up or triggering a Hold.
   - A "This inbox is shared" switch asks for a phone code on every Confirm.

   (D-333)
6. **The pick call.**
   - It comes from the Ask Hava number.
   - Sign-up offers a "Save as Ask Hava" contact card and places a test call.
   - Setup asks who should get pick calls and emergency-job calls.
   - The call names the trade.
   - If a business's only email starts bouncing or is marked as spam, one call tells the owner.

   (D-334, D-335)
7. **The HELP line** now reads: "Casey, the local owner, reads HELP messages twice a week, and within 24 hours if your job is urgent." It replaces "A person reads HELP messages within 4 days". Your actual deadlines don't change: your two weekly windows already sit at most 4 days apart. (D-337)

**Routine**

- Each job page shows businesses a notice log: what was sent and when, whether their mail server accepted it, and how each call went. It shows no opens or clicks. If all of a pick's emails bounced and its call failed, the business gets one extra business hour. (D-332)
- Refund decisions now say why. Public business names are screened like the "About us" line. (D-336)
- Homeowner texts:
  - the first-quote text puts "Reply LIST" before the link;
  - homeowners who chose email are told the business will email them;
  - "Today" jobs are told the first quote can take a few hours;
  - a trade with only 1 or 2 businesses says so.

  (D-324, D-338 to D-340)
- Two new build tasks: T149 (email reliability) and T150 (business calls and link hardening). (D-341)
- Not adopted: see plan section 3 (D-342):
  - counting the quote window from first open, or extending it;
  - email open tracking;
  - "Reply CALL ME";
  - a weekly Away schedule;
  - owner approval of staff Confirms;
  - water keywords without the SAFE step;
  - a deadline time-of-day question;
  - a 7-day trade-open reminder.

**New things to test with real people**

- **Businesses:**
  - When do you see a supplier email (C27)?
  - Would you answer an automated call from an unknown number, and would you save it (C28)?
  - Spanish-first owners: who reads your business email (C29)?
  - Office managers: does IT quarantine new senders, and who should get pick calls (C30)?
  - Text versus email card on a job site (C31).
- **Homeowners:**
  - "Twice a week" versus "within 4 days": which feels more trustworthy?
  - At 10 p.m., what would you do after the "no quote yet" text?

## From fix round 10 ("No calls to businesses" and the HELP notification), October 28, 2026

You said at 07:20: "No calls to businesses. We need to set up a help function that sends me a notification so I can fix it. No name attached." The plan is `/home/claude/hhq/outputs/sim_v10/Fix_Round_10_Plan.md`, and the changes are in the spec as D-344 to D-355. Two research reports from today (ChatGPT and Grok, round 2) fed the safety and email parts. Nothing is built, nothing was bought, sent or signed, and Gate P and build_hold are unchanged. Business texting stays off until Twilio says yes in writing.

**To decide, ask or buy (the spec already works without each one)**

1. **The TCPA question is smaller now, and goes to the AI legal-research memo (in progress, `/home/claude/hhq/outputs/legal/`) + your decision (D-367).** Ask Hava never phones a business, so the questions about pick calls, emergency-job calls, test calls and account calls are gone. Two questions are left:
   - the homeowner safety voice call (stays off; its switch needs a recorded attorney approval);
   - whether you may phone a homeowner yourself during a texting outage.

   (D-344, D-355)
2. **Branded caller ID is withdrawn.** There are no business calls left to brand. (D-355)
3. **Verify the safety phone numbers before launch.** They now live in one file with their sources:
   - UniSource's 24-hour gas and electric emergency number. It's marked "verify before launch", and the texts leave it out until you confirm it.
   - Lake Havasu City's water and sewer number and its after-hours dispatch number. The texts use them only as "if you're on city water", because not every Havasu address is on city water.

   The numbers themselves are in the plan, section 2.5. They stay out of the repository for now because the privacy scanner rejects any real phone number; the build task that adds the file also adds an exact allowance for those numbers only.
   - 211 Arizona, live help 9 a.m. to 7 p.m. Its funding was reported through early 2027 (KJZZ, July 14, 2026), so re-check it by January 15, 2027.

   Also ask the city how it wants homeowners told to shut off their own main valve, and where the city's side of the pipe ends. (D-350, D-355)
4. **Can you keep "within 2 days"?** HELP messages now reach your phone right away, so the promise changed from "twice a week" to "The Ask Hava owner gets HELP messages right away and replies within 2 days, or within 24 hours if your job is urgent." If 2 days is too tight, change the setting back to 4 and the text follows. (D-348)
5. **Set your phone to notify on the alert address.** HELP notifications use the same alert email as your other alerts. They never break Do Not Disturb. (D-348)
6. **The email provider's free plan.** Resend's free plan allows 100 emails a day and 3,000 a month. You get a portal alert at 80% of either. Upgrading costs money, so it's your call when it comes. (D-351)
7. **The pitch file** (`/home/claude/hhq/outputs/gtm/Ask_Hava_GoToMarket_Plan_REVIEWED.md`) says "we call you when a homeowner picks you". Use the business page's line instead: "Job requests and picks come by email. We don't text or call businesses." (D-355)
8. **Trademark (now the AI legal-research memo (in progress, `/home/claude/hhq/outputs/legal/`) + your decision, D-367).** Add Hav-A-Handyman LLC and Hava-Guy Handyman to the list of local "Hava" names. (D-352, D-355)
9. **Two ideas from the Grok report. Your call; neither is adopted.**
   - **A "call and leave a message" way for homeowners to start a request.** It would help people who don't text, but someone has to listen to the messages, and that's your time. Today a call to the number gets a recorded "we work by text" greeting.
   - **Keeping an askhava.com/seniors page.** A plain page for older homeowners: how it works, "we never ask for your card", "you only get texts about your own request". It would be marketing-free, but it's one more page to keep current.

   (D-354, D-355)

**Worth a second look**

1. **No calls to businesses, anywhere.** These are all gone:
   - the pick call and its "press 2 for 2 more hours";
   - the emergency-job call;
   - the test call at sign-up;
   - the call when a business's emails stop working.

   The sign-up checkbox now says "Ask Hava never places recorded or automated calls to you." What stays:
   - calls between a homeowner and the business they picked, through the masked number (person to person);
   - a code read out by phone, but only when the person taps "Call me with the code" on the screen;
   - the alert call to your own phone.

   The contact card stays only as "Ask Hava homeowner calls", listing the masked numbers, so a business recognizes a homeowner calling back. (D-344)
2. **Confirm now works by email alone.**
   - A business has **4 business hours** to confirm a pick (was 2, because the call is gone).
   - It gets a reminder after 1 hour and a "Last reminder" 1 hour before the end, at its main and backup addresses.
   - The daily "still open" email also lists picks waiting to be confirmed.
   - "Hold for 2 more hours" (by the email link) and "Away" are unchanged.
   - The homeowner is told the real end time: "<Business> gets your pick by email and has until 2 p.m. to confirm. You can still choose another quote before then."
   - If the pick runs out, the homeowner gets the other quotes in one text and can reply "2 YES" to pick again at once.

   (D-345)
3. **Emergencies without calls.**
   - Emergency cards and picks have an "EMERGENCY" subject line.
   - At night they go only to people who turned on "Emergency emails at any hour".
   - A business counts as "takes after-hours calls" only after someone who can confirm sends themselves a test emergency email and answers "Yes, my phone alerted me".
   - The homeowner is told the request goes "by email marked EMERGENCY".
   - The 60-minute window, the "no quote yet" text and the 911 lines stay.

   (D-346)
4. **HELP reaches you.**
   - After HELP, four common questions get an automatic answer first: where are my quotes, how do I close this, does it cost anything, and is this a person.
   - Anything else sends you one notification right away, with the job, its status and a link to a page where you can fix things or reply.
   - More messages within an hour join the same notification.
   - After 10 notifications in a day, the rest wait for your 9 a.m. or 3 p.m. summary.
   - Businesses get a "Get help" form on every portal page, and "Ask about a charge" now notifies you the same way.

   (D-348)
5. **No name attached.** Every text, email and page now says "the Ask Hava owner", never your name:
   - the "Who runs Ask Hava" block is "Ask Hava is run by its local owner, who lives in Havasu", with no name or photo;
   - the help page asks "Is Ask Hava a person?" (never "Hava" alone);
   - a wording check fails the build if a personal name or a bare "Hava" shows up.

   (D-349, D-352)
6. **Safety texts use the local numbers and official wording.**
   - Gas, CO and electrical lines follow UniSource and CPSC wording.
   - The heat text says a fan alone isn't enough above 90°F inside, and brings back 211 with its hours. It never names a cooling center or says one is open.
   - The water text asks about your shutoff only "if you know where it is and can reach it safely".

   Add these to the fire and gas desk check. (D-350)

**Routine**

- Emails come from "Ask Hava" <jobs@notify.askhava.com>, with replies going to the monitored help address. The email setup starts DMARC at "report only" and tightens it before the first real business email. Open and click tracking is off. No logo certificate and no dedicated IP. (D-351)
- Opening any emailed link, for homeowners too, never does anything by itself. Pressing the button twice acts once. (D-351)
- The last sign-up step asks businesses to "reply OK, star this email and add us to your contacts", with pictures for each mail app. Gmail only alerts for its Primary tab. (D-352)
- From day one, the app records how long it takes from card email sent, to the business first opening its own link, to its quote. You'll see weekly medians on the health page. No tracking pixels are used. (D-347)
- The FAQ says "Ask Hava isn't connected to any business with "Hava" in its name." Spanish copy never says "leads". (D-352)
- Two new build tasks: T151 (HELP notifications) and T152 (the safety contacts file). T148 and T150 are retitled because the calls are gone. (D-353)

**New things to test with real people**

- Businesses: is 4 business hours to confirm by email fair? Did the test emergency email make your phone buzz?
- Homeowners: does "replies within 2 days" feel trustworthy? Is the lapse text with the other quotes clear?

## From fix round 11 ("Ask Hava is not a contractor"), October 28, 2026

You said at 08:01: "Ask Hava is not a contractor. All we are doing is taking the quotes from contractors and relaying that information to the customers. We are not subcontracting them..." The plan is `/home/claude/hhq/outputs/sim_v10/Fix_Round_11_Plan.md`, and the changes are in the spec as D-356 to D-370. Nothing is built, nothing was bought, sent or signed, and Gate P and build_hold are unchanged.

**The statement, everywhere (D-356)**

- **Site, help page, homeowner terms, privacy policy, quote page:** "Ask Hava is not a contractor and does not perform, bid on or supervise work. We pass along your request and show you quotes from independent local businesses, as they wrote them. You choose, and you hire the business directly."
- **First text (inside the consent text):** "Ask Hava LLC is a local quote service in Lake Havasu City, not a contractor. We pass your request to local businesses, and you hire the one you choose directly." *(Shortened Oct 28, D-419: "Ask Hava is not a contractor; you hire the business you pick directly.")*
- **Every job card, the business page, sign-up and business terms:** "Ask Hava is not a contractor. It passes along this request and the homeowner's choice. If the homeowner picks you, they hire you directly; Ask Hava doesn't direct, schedule, supervise or pay for the work."
- **The pick review step:** "You hire the business directly. Ask Hava isn't part of your agreement with it."

Spanish versions are drafts for a native speaker to check.

**To decide, ask or send (the spec already works without each one)**

1. **Send Stripe this question before you take real payments.** Nothing has been sent. Payments now work one way only: the business presses "Confirm and pay" on its pick page, and Stripe charges the card right then, with the business there. Saving a card is optional. Ask Hava never charges a card in the background. Homeowner details go out only after the payment succeeds. (D-365)

   > **Subject:** Prelaunch eligibility review: fixed introduction fee paid by local businesses
   >
   > Hello Stripe Support,
   >
   > Ask Hava LLC (being formed in Arizona), Lake Havasu City, Arizona. Business description: "Quote-request service for homeowners. The business the homeowner chooses pays a set introduction fee." We'd like to confirm our model is eligible for Stripe before we accept payments.
   >
   > Homeowners send us a repair or service request by text or web chat, free. We pass the request, without their contact details, to independent local businesses by email. Businesses that want the job send us their own quote, and we show the homeowner every quote unchanged, in the order received, with no ranking. The homeowner picks one business. That business then opens a secure page that shows the job number and our fee, and presses "Confirm and pay". Only then do we charge our own fixed introduction fee: $25 for jobs under $500, $75 for $500 to $1,500, or $199 over $1,500 (ongoing services count four months, capped at $75). The charge happens on that page with the business present; a business may optionally save its card for next time, and we never charge a saved card in the background. Only after the payment succeeds do we give that one business the homeowner's first name and job address; never the homeowner's phone number.
   >
   > We are not a contractor. We don't perform, schedule, supervise, price or guarantee any work. The homeowner and the business make their own agreement, and the homeowner pays the business directly. We never collect money from homeowners or pay businesses.
   >
   > Could you confirm:
   >
   > 1. Is this fixed-fee introduction model eligible for Stripe, or does it need a restricted-business review or extra approval?
   > 2. Is it fine to use Stripe Elements with PaymentIntents confirmed on-session from that page, with an optional saved card (SetupIntent or setup for future use) that is only ever charged when the business presses the button again?
   > 3. Which business description, merchant category, documents or terms should we provide before going live?
   > 4. Optional for businesses: may a business tick "Auto-confirm and pay if picked" on one quote, with a separate saved-card authorization naming that quote's fee, so we charge that fee off-session the moment the homeowner picks it? If the charge fails or needs authentication, the business confirms on-session as usual. What mandate wording and setup (for example a SetupIntent with usage off_session) do you require?
   >
   > Thank you,
   > Ask Hava LLC

2. **Verify the water numbers before launch.** The burst-pipe text now lists the providers, "it's on your water bill": Lake Havasu City (and its after-hours number), EPCOR (24 hours) and Havasu Heights (after hours). It also says to use your own shutoff only if you know where it is and can reach it safely, not to use the water company's meter as a shutoff (Havasu Heights asks this), and to call 911 if anyone is in danger. Each number is marked "verify before launch", and the texts leave it out until you confirm it. The digits are in the plan, section 7, not in the repository: as in round 10, the privacy scanner rejects real phone numbers, and the build task that adds the file adds an exact allowance for those numbers only. Check UniSource and 211 (from round 10) at the same time. (D-364)
3. **Your name on the LLC record.** The site, texts and emails never name you; where trust needs it they say "Ask Hava LLC, a Lake Havasu City business". But Arizona LLC filings list members or managers by name (A.R.S. 29-3201), so the state record can show your name. Decide the filing setup (statutory agent, mailing address, member or manager) knowing that. The research's Part 3 has the details. (D-366)
4. **"No calls of any kind" to businesses.** Done: Ask Hava never calls a business, not even to read a code. A business whose record phone is a landline gets its code by email or a mailed letter. Sign-in and Confirm codes go by text, or by email to someone with no phone that gets texts. One thing stays, unless you say otherwise: **the masked calls between a homeowner and the business they chose.** Those are the two people calling each other on a masked number, not Ask Hava calling, and the refund for an unreachable homeowner counts those call attempts. (D-366)
5. **Homeowners can ask for their code by phone.** A homeowner gets a voice call reading the code only if they tap "Call me with the code instead" on the screen. (D-366)
6. **The 4-business-hour Confirm window on urgent jobs.** A pick at 8:10 p.m. waits until the next morning. The homeowner is now told the end time before picking, and on an urgent job they're offered "Switch to emergency (60-minute confirm)" when a business takes night emergencies. A shorter window for businesses on urgent jobs is your call (the business simulation should weigh in). (D-363, D-370)
7. **Native-speaker review** of the new Spanish statements, with the other Spanish templates. (D-356)
8. **The pitch file** (`/home/claude/hhq/outputs/gtm/Ask_Hava_GoToMarket_Plan_REVIEWED.md`, outside the repository, not changed) fails the new phrase list in two places. Suggested fixes:
   - H14: "Already used a business through Ask Hava? ... Repeat work never goes through us." → "Already hired a business you chose on Ask Hava? Next time, call them directly."
   - The yard-sign card: "Quotes through Ask Hava" → "Chosen on Ask Hava".

   Also fix round 10's line ("we call you when a homeowner picks you"). (D-359, D-360)

**Worth a second look**

1. **Business terms say it plainly (D-357).** "Businesses on Ask Hava are independent. Ask Hava never directs, controls, schedules, supervises, inspects, warrants or pays for your work, never sets or changes your prices, and never negotiates for you or the homeowner. There is no subcontract, employment, agency, partnership or joint venture between you and Ask Hava. ... The introduction fee pays only for the introduction service: a fixed amount by job size ($25, $75 or $199), never a percentage or share of the job's price, and it doesn't depend on whether the work is done, finished or paid for."
2. **Quotes are shown exactly as the business wrote them (D-358).** The texts used to say "per month" for every ongoing service, which turned a weekly price into Ask Hava's own monthly number. Now they show "$40 per visit, weekly" as the business said it. The monthly number is only used to work out the fee.
3. **Words that are gone (D-359):** "You were picked" (now "A homeowner chose your quote"), "Booked until" (now "Busy until"), "connected through Ask Hava", "takes after-hours calls" (it's email; now "gets emergency requests at night"). The help answer about cost now says "The business you pick pays Ask Hava a fixed introduction fee. You pay the business directly for the work."
4. **The build now enforces it (D-360).** One list of banned phrases ("our pros", "our network", "your contractor", "we'll send a plumber", "book", "hire through us", "through Ask Hava", "service provider", "we found you", "assigned", "subcontract", "guaranteed", "our price", and Spanish ones). The spec check, the category check and the app's wording scan all fail on any of them.
5. **Safety text fixes (D-361).**
   - Gas: "If you smell gas or hear gas hissing, leave now" (it said "hear a leak", which told people with a burst pipe to leave).
   - A smoke detector that chirps once a minute gets "usually needs a new battery", with the 911 line only if it's sounding a loud alarm. "Beeping" alone gets asked "chirp or loud alarm?".
   - Propane points to "your propane supplier", not UniSource.
   - The heat text leaves out 211 when 211 is closed (7 p.m. to 9 a.m.).

   Add these to the fire and gas desk check.
6. **Night emergency emails must be tested with Do Not Disturb or Sleep on (D-362).** The test asks "Did your phone alert you with Do Not Disturb or Sleep on?", lasts 90 days, then asks again. A business that misses 2 night emergency cards in a row stops being counted for night emergencies until it re-tests.

**Routine**

- Payments: one "Confirm and pay" button, no background charges, no automatic retries, details released once after the payment succeeds, Stripe's receipt plus ours, and an evidence record for each charge. (D-365)
- The test for each change is listed in D-368; the build tasks that own them are updated and their prompts regenerated.
- The professional-services stance is at the top of this file. (D-367)

## From fix round 12 ("Go ahead and fix everything that needs to be fixed"), October 28, 2026

Your words, 10:14: "Go ahead and fix everything that needs to be fixed. Just don't run any simulations right now." No simulation was run. This round put the legal memos' must-fix items into the plan (status report section 5A and Memo 4's "Must change before launch" list). For your own decisions (section 5B), only the cautious defaults went in; everything else is listed below with the memos' leans. The plan is `/home/claude/hhq/outputs/legal/Fix_Round_12_Plan.md`. Decisions D-371 to D-388.

**What changed, in short**

1. **One consent text, one HELP reply, one help page (D-371).** The first text keeps "not a contractor" and now says "the business you choose pays Ask Hava a set fee of $25 to $199, by quote size" (not "a small fee") and "Reply OK to agree to the Terms (askhava.com/terms) and to get automated texts about your request." Every reply-time promise says "usually". The help page no longer says "a real person reviews anything flagged", and the site no longer says "Safety texts go out at any hour"; both now say to call 911 and not to rely on Ask Hava in an emergency. The English first text is 5 text segments; the Spanish one is 13 (accents force the costlier encoding). Budget for it. *(Replaced Oct 28, D-419: the first text no longer asks for OK or carries the fee line; it is 2 segments in each language.)*
2. **Opt-outs, ready for the FCC rule of November 25, 2026 (D-372).** A "stop" by text, by email, on the help form, with the portal's texts-off switch, or one you record from a call or conversation all go on one stop list at once. The STOP confirmation is a bare confirmation. Twilio's setting that treats "YES" as re-subscribe gets switched off (you do that when you set up Twilio; the checklist says so).
3. **AI quote summaries are off at launch (D-373).** Homeowners see only the business's own words. The AI is described one way everywhere, and the privacy policy says "No AI reads your messages today" until the zero-retention agreement is signed. Business quote text is scrubbed before any AI call.
4. **Ask Hava never texts or calls a business to recruit it (D-374).** Arizona's do-not-call law covers texts. Recruit by posts, the website, email and in person. Recruiting emails carry the LLC's postal address and a way to stop them.
5. **Homeowner terms (D-375):** liability limited to $100, not $0, with exceptions for gross negligence and intentional misconduct, and no release of injury claims. "Reply OK" and the "By choosing" step are electronic signatures under Arizona's electronic-transactions law. The terms line sits right beside the choose button with a three-line summary. *(Replaced Oct 28, D-420 and D-421: picking is the only acceptance, and only "By picking, you agree to the Terms" sits beside the Pick button; the $100 cap stays in the terms under "Limit of liability".)*
6. **Pest control is off** until the Arizona Department of Agriculture answers in writing; **home inspections for a sale, purchase or refinance are declined**; **no warranties of any kind (D-376).**
7. **One honest description everywhere (D-377):** "a quote-request service; the business the homeowner chooses pays Ask Hava a set introduction fee." "Connect you with" and "referral network" now fail the build's wording check.
8. **The fee is never called a "license" or "data access" (D-378).** It pays for the introduction service. This matters for the sales-tax question.
9. **Accessibility page:** text, email or the help form, no phone promise (D-379).
10. **Stronger pick nudges, still no calls (D-380):** a "send a sample pick" test for everyone who can confirm, with Do Not Disturb or Sleep on; a reminder every business hour with the Hold link in each; up to two open-cards emails a day, with a waiting pick first in the subject.
11. **Email limit (D-381):** you get a phone alert at 70% of the free plan's 100-a-day limit; above 90%, only the non-urgent emails wait until the next day; no email is ever dropped.
12. **Smaller fixes:** Arizona towns only unless reviewed (D-382); photo location data stripped at upload (D-383); under-18 messages stopped politely (D-385); every email has the LLC postal address and a "Manage email notices" link (D-385); the gas card field now reads "Homeowner says gas company checked" (D-385).

**Your decisions (the spec works without each one; nothing was sent, filed or bought) (D-388)**

1. **Arbitration and class-action waiver for homeowners.** It would shield the one risk insurance usually excludes (texting lawsuits), but "Reply OK" as a contract is untested (since Oct 28 there is no reply OK; the Terms are accepted at the pick, D-420) and mass arbitration can cost more than it saves. **Memo 4's lean:** if you add it, a short, separate disputes line at the pick step, small claims allowed, Ask Hava pays the filing fees, plus a batching clause for mass filings. The draft homeowner terms (next update) will carry it as an optional section, switched off.
2. **Regulator letters** to the contractor board (ROC), the Department of Agriculture (pest control) and the Department of Revenue (ADOR). **Lean:** yes to the ADOR ruling; the others only if you want certainty. Pest control stays off until the Department of Agriculture answers, so that letter is what would turn it on.
3. **Sales tax (TPT):** Path A, a free private ruling request to ADOR plus setting aside 7.6% of each fee until it answers, or Path B, a $17 license and filing returns. **Memo 4: do one of them before the first fee, not neither.** Memo 3 leans to Path A.
4. **Trademark timing and wording:** file "ASK HAVA" in Class 35 now as intent-to-use ($500) or right after launch ($350). **Lean:** after launch unless launch is far off. Pick an ID Manual description without "referral" or "matching" if an accurate one exists (D-377); if none fits, accept that risk knowingly.
5. **Pilot trades. Lean:** HVAC, electrical, licensed handyman, then pool when it's warm.
6. **Insurance. Lean:** get at least a quote for tech E&O with cyber, bound before the first introduction; describe the texting and the AI accurately on the application (a wrong answer can void the policy). Assume texting lawsuits are not covered.
7. **Which outside-employment law covers your job.** Memo 3 assumed an Arizona public employer; Memo 4 says confirm that first, then get written approval either way.
8. **PICK-026** (telling losing businesses whether the picked quote was lower, about the same or higher): keep as is or drop. Memo 1: never expand it.
9. **The safety voice call's gate:** keep the attorney approval, or replace it with Memo 2's six conditions plus your own recorded decision. It stays off either way until you ask.
10. **Email plan upgrade:** buy the paid plan (Resend Pro, about $20 a month when researched; check the price) when the 70% alert starts arriving, before about month 7.
11. **Optional:** a "call and leave a message" option for homeowners who don't text; keeping the askhava.com seniors page.

**Fix the pitch by hand (D-386).** The pitch isn't in this repository: `/home/claude/hhq/outputs/gtm/Ask_Hava_GoToMarket_Plan_REVIEWED.md`. Exact replacements:

- **Line 159 (the walk-in):** replace "No monthly fee, no bidding wars, nobody can pay to rank higher." with "No monthly fee. You quote once. No bidding rounds, no paid placement." Replace "Your first win is free if you join in the first 90 days after launch. Can I leave this card and check back next week?" with "Launch offer: your first win is free, on any job size, when you join in the first 90 days or within 90 days after your trade opens. Can I add you to the founding list so we can sign you up together in launch week?" Optionally add, before the fee sentence: "Ask Hava is a quote-request service; the business the homeowner chooses pays Ask Hava a set introduction fee."
- **Line 194 (the card):** replace "- No bidding wars. No paid rankings." with "- You quote once. No bidding rounds, no paid placement." Add a line: "- The homeowner sees every quote and picks one business: you."
- **Line 195:** replace "- First win free in the first 90 days after launch." with "- Launch offer: your first win is free, on any job size, when you join in the first 90 days or within 90 days after your trade opens."
- **Line 218 (pest):** keep "Not at launch." and replace the rest with "Ask Hava doesn't take pest control requests until the state Department of Agriculture confirms how its rules apply." (This replaces the round-11 suggestion, which assumed pest operators could sign up; they can't while pest control is off.)
- **Line 312 (H6):** replace "No paid rankings, no bidding wars." with "No paid rankings, no bidding rounds."
- **Line 391 (B2):** replace "No bidding wars. No paid rankings." with "You quote once. No bidding rounds, no paid placement."
- **Never in any post, card or conversation:** "connect you with", "referral", "leads", "vetted", "best", "#1", "top-rated", "exclusive", "concierge", or anything about pest control. And never text or call a business to recruit it (D-374).
- The other round-11 items (the 4-month line "capped at $75" on lines 129, 177 and 398; the free-win line on lines 180, 234, 235, 416 and 559) still apply.

**Worth a second look**

1. **The consent text is long:** 5 segments in English, 13 in Spanish. *(Resolved Oct 28, D-419: 2 segments each.)* Every part is there for a legal reason (D-371). If cost matters later, the first thing to drop would be "Msg frequency varies by request" only if carriers stop requiring it; don't drop anything now.
2. **"Usually replies within 2 days"** is softer than before. That's deliberate: a promise you can't always keep is the main consumer-fraud risk the memos found. The away setting still changes it automatically.
3. **Home inspections** now take only general condition checks. Most inspection requests are for a purchase, so this category will be small. Switching it off entirely is the other option; say so if you'd rather.
4. **Reminders every business hour** mean up to 3 reminder emails per pick. The sim said businesses missed picks because reminders were too few and too quiet; if real businesses find this too much, it's one setting.

**Routine**

- The FCC rule's text and date were checked by the memos against the Federal Register (91 FR 67214, effective November 25, 2026), and that check is now recorded in CON-032 (D-372).
- The phone steps for Do Not Disturb and Sleep (iPhone VIP alerts, Gmail label notifications) are checked on real phones in T1 before they're written into the app (D-380).
- New wording checks: "connect you with" and "referral network/service" fail the spec and catalog checks; the catalog check fails if pest control is switched on without a logged written answer, if home inspections take a sale-related job, or if any category mentions a warranty (D-376, D-377).

**New things to test with real people**

- Whether the "send a sample pick" test with Do Not Disturb on actually makes picks reach businesses at the counter or on a job.
- Whether homeowners read "a set fee of $25 to $199, by quote size" as reassuring or off-putting.

## Legal document drafts, October 28, 2026

Status report item 13: the documents are drafted, in `docs/legal/` (D-389). Each starts with "Draft prepared with AI research; not reviewed by a lawyer." None is published.

- **Homeowner terms** (`homeowner-terms.md`): what Ask Hava is, how it works, your agreement is with the business, what we check, not an emergency service, electronic signatures, no warranties, the $100 limit, Arizona law and Mohave County courts with small claims. The arbitration section is there but marked **OPTIONAL and OFF**, with Memo 4's safeguards, for your decision.
- **Business terms** (`business-terms.md`): independence and the fixed introduction fee (TERMS-026), joining, quotes, picks and Confirm, the fee schedule, the card authorization for the one-step Confirm and pay, every refund rule as coded, the homeowner's details as part of the introduction (never a "license" or "data access"), no passing the job or details to another business, non-discrimination, indemnity, complaints and pauses.
- **Privacy policy** (`privacy-policy.md`): who sees what and when (a table), the AI wording for now and for later, retention, opt-outs by every channel, breach notice by email or text, no sale of lists, phone numbers or texting consent.
- **SMS terms** (`sms-terms.md`), **business email notice terms** (`business-email-notices.md`) and the **accessibility statement** (`accessibility-statement.md`).
- **Written information security program** and **incident response plan** (internal; the "GLBA-light" paperwork Memo 2 recommends).

A new check (`scripts/check_legal_docs.py`) fails the build if a draft's key promises stop matching the spec word for word, or if any banned wording appears.

**For you (D-390)**

1. Read the eight drafts.
2. Fill in the bracketed items: the LLC mailing address (P.O. box or registered agent), the effective date, and the business data retention days.
3. Decide arbitration: turn the optional section on (a spec change to TERMS-002) or delete it.
4. Spanish versions come after the English is settled, then native-speaker review.
5. Publishing waits for your go.

## From fix round 13 ("compose all of the fixes we need"), October 28, 2026

Your words, 10:53: "compose all of the fixes we need, and then put together a list of where we're at and what we need to do before we start building." No simulation, persona pass or rehearsal was run. **The one to-do list is now `docs/PREBUILD.md`** (D-416); this section only lists what changed and what's yours.

**What changed, in short (D-391 to D-415)**

1. **License matching (D-392):** a license map covers dual (CR-), general (B, B-3, KB-1, KB-2), R-61 and R-62, pool (B-5, B-6, KA-5, KA-6) and subclassification licenses, from the ROC's own class rules. Commercial-only licenses are told why they get no home jobs.
2. **Business setup (D-393):** the checklist arrives pre-ticked from the license; the owner unticks what he doesn't do and answers towns, largest job, recurring work and after-hours.
3. **Expert intake (D-394, D-395):** 3 to 6 questions per trade (boats, RVs, pools and vacation homes included), a "Here's what we'll send" check before cards go out, and a monthly accuracy report.
4. **Pilot cut (D-396):** only the launch-open trades take requests, each once it has 2 businesses; a pilot scorecard page.
5. **Honest counts (D-397), short consent texts (D-398), quote types and $25 site visits (D-399), backup choice (D-400), auto-confirm after Stripe says yes and the push web app (D-401), voice guide (D-406), request and job numbering (D-407), money set-aside (D-410), safety numbers checked by the build (D-411), caller ID steps (D-413).**
6. **The Twilio question** no longer mentions a pick call (D-391). **The Stripe question** uses the one honest description, says details go out only after payment, and asks about auto-confirm (D-408). Both are above in this file.
7. **The pitch file** (outside the repo) was fixed: "Text Ask Hava", no "bidding wars", no phone help line, email-only picks, a pest answer and "How will I know I've been picked?" (D-405).

**Defaults pending your OK (each is one setting):** pilot trades (B2); pest control for Department licensees only (B3); sale and refinance inspections with no fee (B4); AI on scrubbed text once the provider's terms are recorded (B5); backup choice, auto-confirm after Stripe's yes and the push app (B6); $25 site visits, never above $75 (B7); fees unchanged (B8); no arbitration (B9); no owner name online plus record links (B12); keep PICK-026 (B14); safety call off (B15); short consent texts (B16). (D-414) *(B16 replaced Oct 28 by the 2-segment first reply with no reply OK, D-419.)*

**Still open:** Gate P (B1), sales tax path (B10), trademark timing (B11), insurance (B13), Spanish review (B17). Recommended answers are in `docs/PREBUILD.md` section 4.

**Dropped (D-417): no letter goes to the Department of Agriculture.** Pest businesses attest to their license at sign-up and cards say it isn't independently verified. The old draft is kept below for reference only.

> **Subject:** Question about a quote-request service and pest-management licensing
>
> Hello. I'm starting Ask Hava, a quote-request service in Lake Havasu City. A homeowner asks for quotes; we pass the request, without contact details, to businesses that hold a current Department pest-management business license; the businesses send their own quotes; the homeowner picks one and hires that business directly. The business the homeowner chooses pays Ask Hava a set introduction fee. Ask Hava applies no pesticide, inspects nothing, writes no estimates, and runs no pest-control advertising; the license number of each business is shown with its quote.
>
> Does relaying licensed businesses' quotes this way require Ask Hava to hold a pest-management business license under A.R.S. 3-3615? If it does, we will stop taking pest-control requests.
>
> Thank you,
> Casey, Ask Hava (LLC being formed)

**Desk research results (D-404):** no ROC class names garage or overhead doors, so garage doors stays off; the city's solid-waste chapter (8.04) couldn't be read online, so junk removal stays off until the city clerk answers whether a private business may haul junk for pay.

## Consent by texting in ("their reaching out is their consent"), October 28, 2026

You said: "I think if they text us, that's implied that they want to talk to us... So I think their reaching out is their consent." Done in one pull request, decisions D-419 to D-428. Nothing was simulated.

1. **No "Reply OK" step anymore (D-419).** A homeowner's first text, or starting a web chat, is the opt-in for texts about their own request. Our first reply answers and moves straight on, with the disclosures carriers need: "Ask Hava: Got it. Next, a few quick questions. Ask Hava is not a contractor; you hire the business you pick directly. Automated texts about your request. Msg frequency varies. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms: askhava.com/terms Privacy: askhava.com/privacy" (2 text segments). The emergency check still runs first on every message. The "Here's what we'll send" summary now says it goes out in 2 hours on its own, so nothing waits for a reply there either.
2. **The Terms are accepted when the homeowner picks (D-420).** Next to the Pick button: "By picking, you agree to the Terms", with a link. We log the time, the terms version and the request number.
3. **The $100 cap isn't shown on the pick page (D-421).** It stays in the Terms, section 10, under "Limit of liability".
4. **One question after the request goes out (D-422):** "How should we send quote updates? 1) Text 2) Email 3) Both." Text if they don't answer. Email asks for the address once and never shares it with businesses. Every update links to the private quote page.
5. **Stopping texts is unchanged (D-423):** the first reply already says STOP; one stop list for text, email and the page.
6. **Twilio question shortened (D-424):** it's at the top of this file and in `send_drafts/Twilio_question.md`. Signed "Ask Hava (LLC being formed)".
7. **Your answers today:** trademark skipped for now (D-425); insurance after the business is fully working (D-426); Spanish: launch in English, with the Spanish safety texts on and checked by ChatGPT and Grok translating them back, and full Spanish after a paid review of about $100 (D-427). Sales tax (B10) stays open.


## From fix round 14 ("as well packaged as possible"), October 28, 2026

Your four answers (photos now, AI later; she OKs sharing at the pick; the tax ruling on the later list; the fire-truck list only) and "Do everything that you recommend" are in D-429 to D-449. The plan is `docs/handoff/rework-plan.md`. Nothing was sent anywhere, no simulation was run, and nothing is built.

**What changed, in short**
- The homeowner gets pro-level questions per job type, photos, design pictures and (for design jobs, once you switch it on) a labeled concept image; she approves the real job card before it goes; texts are short and link to her page, which holds the quotes, license numbers and Google and Yelp links (D-430 to D-435, D-438).
- One "call 911" text, for gas, a CO alarm, smoke or fire, a sparking or burning outlet or panel, and a downed power line. No heat, water, health or utility-number advice anywhere. The safety-numbers file and its verify task are gone (D-436).
- Own-or-rent and the landlord text are gone (D-437). No call window, no lockbox question, no scheduling (D-440).
- Quotes line up: one format per job type; site-visit fees only on repairs that need a look; notes screened; "Can't take it" for businesses; edit or withdraw before a pick (D-441, D-442, D-446).
- Fee wording is "introduction fee", never "lead"; the homeowner ticks "Share my first name and the job address" at the pick (D-443).

**Sales tax (ChatGPT report, AI research, not tax advice)**
- Best answer: the introduction fee is probably not subject to Arizona TPT (about 80% confidence), because it pays for an introduction, not for a product, software or data. The two real risks are the state calling it a sale of customer data (ADOR ruling LR 21-003; *ADP v. ADOR*, 2023) and the city calling it advertising.
- The set-aside is now **8.6%**, not 7.6%: Arizona 5.6% plus Mohave County 1.0% (October 1, 2026 table) plus Lake Havasu City 2.0% (D-444).
- On the company-setup list: ask ADOR for a private taxpayer ruling (A.R.S. 42-2101) after the LLC exists and before real money builds up. A ruling binds the state and, under the city tax code, the city too. Don't take a TPT license just in case.
- Keep the introduction fee separate from anything like paid placement or software subscriptions, if those ever exist.

**Owner time, re-estimated without a simulation (G11, D-446)**
These are assumptions to check in the pilot, not measurements. At 60 requests over 8 to 12 weeks (about 6 a week) and about 3 paid introductions a week:
| What | Per week (assumed) | Minutes each | Minutes a week |
|---|---|---|---|
| HELP messages (about 1 in 5 requests, plus businesses) | 2 | 5 | 10 |
| Flagged sign-ups (early weeks) | 2 | 10 | 20 |
| Not-covered emails (read, note for recruiting) | 3 | 2 | 6 |
| Weekly digest and scorecard | 1 | 10 | 10 |
| Refund or charge questions | 0.5 | 10 | 5 |
| **Total** | | | **about 50** |
That is roughly 15 to 20 minutes per paid introduction, inside the 20-minute go line but close to it in the first weeks, when sign-ups are heaviest. The HELP promises (2 days, 24 hours if urgent) stay as they are.

**Still yours, unchanged:** Gate P, the Twilio question (updated: it now mentions AI-worded questions, photos by MMS and "agrees to share"), the Stripe question, and the pilot steps in `docs/PREBUILD.md`.

## The look: dark plaster and brass (D-451, October 28)

You picked the darker plaster and brass homepage. It is now written down in `docs/design/DESIGN_SPEC.md`, so every page the build makes looks the same: dark plaster walls for headlines and menus, cream paper for quotes and forms, one brushed-brass button per step. Four mock-ups in `docs/design/mockups/` show the home page on a phone and a computer, her quote page and the pick step. Every color pair was measured for readability (all pass WCAG AA). Nothing for you to do; if you later get real close-up photos of plaster and brass (with permission), they can replace the generated textures.

## No AI smell (D-452, October 28)

You noticed the mock-up wording sounded like AI. The build now checks for that three ways:
- **A list** of words and habits that give AI away (`docs/ai_smell.yaml`): "seamless," "elevate," "peace of mind," "seasoned pro," "side by side," slogans in threes, "not just X but Y," dashes, exclamation marks, emoji, and design habits like color gradients and sticker-style badges.
- **An automatic scan** (`scripts/check_ai_smell.py`) that runs on every change. Any hit fails the build, and the fix is always a rewrite.
- **A cold read.** For every task that writes words people see, a second AI reads only the finished words aloud, as the homeowner or business owner would, and flags anything that doesn't sound like a local person talking to a neighbour.

The home page mock-ups were rewritten that way. "Text us the job. Local businesses quote. You pick." is now "Get local quotes by text." The paragraph under it now says what actually happens.

## Built in Rust (D-453, October 28)

You chose Rust for the whole server, with the server building the pages. What changed in the plan:
- **The stack:** Rust replaces Python for everything that runs Ask Hava. Pages are still plain HTML built on the server, so they work with JavaScript off. A little TypeScript handles small extras in the browser.
- **What the compiler now guarantees:** a homeowner's address can only be released with a "proof of payment" that only the payments part of the code can create, and a test proves nothing else can fake one. Every job and pick status has to be handled everywhere, or the code won't build. And each part of the app can only call the parts below it.
- **From your directive:** two more security tests. Changing a price or ID in the browser changes nothing, and no secret or contact detail ever appears in a page, script or web address.
- **What stays Python:** the build tools, the fake Twilio, Stripe and email services used for testing, and the twin. None of it runs in the live service. The fake services now answer over the network, so the real Rust code talks to them exactly as it will talk to Twilio and Stripe.
- **Checked here:** a trial Rust project with the chosen versions built cleanly. It blocked a planted script on a page, refused to compile a faked payment proof, and applied a database update added later even when it carried a lower number.

Every build task's files were re-pointed to the Rust layout, and three new tasks were added (169 in total): the fake services answering over the network, the Rust test folders, and the automatic checks for Rust and the browser tests. An independent reviewer checked the whole change, and every problem it found was fixed. Nothing for you to do.

## Customer details encrypted end to end (D-454, October 28)

You asked for customer details to be encrypted on the way to the business and everywhere we keep them. Most of that was already in the plan: every name, phone number, email and address sits in a locked vault, encrypted field by field; every read is logged; backups are encrypted; business sign-in is per person with a code on a new device. What changed:
- **No details in email any more.** The business you're picked by gets "Job 1042 is confirmed. Sign in to see the homeowner's details." The name and address appear only after they sign in to their Ask Hava account. Email isn't encrypted end to end and gets forwarded, so the details stay off it.
- **Photos encrypted before they're stored.** The storage bucket only ever holds scrambled photos, and the app hands each one out itself, behind a short-lived link.
- **Pages with details aren't saved** by phones or browsers, and carry no outside scripts. No web address, email, text or alert ever carries a homeowner's details.
- **The privacy policy says it plainly:** texts and ordinary email aren't encrypted end to end, so share sensitive details on askhava.com.

At the account checkpoint you'll confirm two things: the host encrypts the database and its backups (Render says so; Railway needs a written yes), and the Claude account has zero data retention switched on. An outside key service (AWS or Google, about $1 a month for the key) is an option to decide there, together with Railway or Render. It isn't needed to launch.

One thing this can't do: a deleted job's details stay inside encrypted backups until those backups expire. The privacy policy already allows for that. Nothing for you to do now.

## Fix round 15: the Hormozi, Martell and Koerner review (D-455 to D-467, October 28)

You said "do what you recommend". What changed:
- **Prove it by hand first.** Before you reply `approved to build`, run the 4 to 6 week hand-run test in `docs/pilot/hand-run-test.md`: two free web forms, one inbox, one sheet, no fees, no texts. It answers whether homeowners send jobs, whether businesses quote by email, and whether a picked business says the introduction was worth the fee. The build stays held at Gate P until then.
- **All service businesses open,** as you asked. Posts push one or two trades that fit the season, so those get several quotes a job.
- **Recruiting is mostly self-serve:** your own hand-written Facebook group posts, each saying you run Ask Hava, plus a few visits. Still no texts or calls to businesses.
- **The scorecard counts what matters for the goal:** requests a week, where people came from, paid introductions against 15, 50 and 115 a month, what's left from each fee, where intake loses people, and your recruiting minutes. About 50 paid introductions a month is the $20,000 a year pace in Havasu; $50,000 needs Bullhead City and Kingman too.
- **Faster and shorter for homeowners:** the town is asked right after the coverage line, the card goes out after 30 minutes instead of 2 hours, the "text, email or both" question is gone, and one text says when all the quotes are in.
- **Better first days for businesses:** open jobs on the welcome screen, the sample pick test at their first real card, a reason asked when they leave.
- **Fees:** amounts stay $25, $75 and $199 for now; the test asks whether $25 or $35 would have been worth it. Early joiners are locked for 6 months instead of 12. Ask Hava may refund any charge at its own choice.
- **Proof:** homeowners can tick a box to let their words show on askhava.com; the home page shows real numbers once 20 requests have gone out.
- **Cut:** the AI receptionist plan and the Ruby answering service. Fixed costs stay near $150 a month or less.

Held for one re-plan pass after the test, so the plan changes once on real numbers: the scope cut (team members, the monthly fee limit, the live license lookup, the full restore replay, the switched-off features) and the fee level. Your before-build list is `docs/PREBUILD.md` section 3.

## Fix round 16: ChatGPT's stress test of the hand-run test (D-468, October 28)

ChatGPT found the test's weak spot: a business saying "it would have been worth $25" isn't the same as paying $25. What changed in `docs/pilot/hand-run-test.md`:
- **Real money in weeks 4 to 6.** In the trades you push, a picked business pays the real fee by payment link before getting the homeowner's details. No free first win during the test. Gate P needs at least 5 different businesses to have paid.
- **The LLC, EIN and Stripe start now,** with the test, so the paid weeks can happen.
- **Honest demand.** Jobs from people you know are marked Seeded and used for practice; only Organic requests (strangers who found Ask Hava) count as demand. Partner requests (property managers, hosts, realtors) are their own channel.
- **Choice, not just a reply.** The scorecard looks at requests with 2 or more quotes, the slowest quarter of response times, and businesses that actually quote, not just sign up.
- **Does Ask Hava work, or do you?** Weeks 1 to 3 you may nudge businesses; weeks 4 to 6 you don't, and the two are compared. Every nudge is logged.
- **Your time** is counted per request, and should settle near 5 hours a week.
- **A winter test.** The result says "winter Havasu, in these trades", not the whole year.
- **Privacy.** Homeowner details are deleted from the sheet and inbox 90 days after the test.

In the build: the scorecard adds the same measures, the crowding flag shows quotes per win, and the home page waits for 50 requests before showing any numbers. All trades stay open, as you decided; the pushed trades are the ones judged.

## Dry run with fake businesses (D-469, October 28)

You wanted everything proven before going live. Two new files:
- **`docs/pilot/hand-run-tracker.xlsx`**: the test's spreadsheet, ready to upload to Google Sheets. Blue cells are yours to type; everything else fills in, including the scorecard with green, amber and red lines. It comes loaded with fake businesses, homeowners, quotes and payments so you can see it working; one step clears them.
- **`docs/pilot/dry-run.md`**: a 90-minute rehearsal. Your personal Gmail plays every fake business and homeowner with "+" addresses, and Stripe's test mode takes fake card payments. Six scenarios cover sign-ups, a pick and payment, a too-big handyman quote, a late quote, a job nobody covers, a homeowner who doesn't pick, and a business that won't pay.

The software build already has its own fake test setup (stand-in Twilio, Stripe and email, plus the twin), so this rehearses the hand-run test only.

## ChatGPT's legal research (D-470, October 28)

Bottom line: no law found blocks the paid test, as long as Ask Hava stays a neutral introduction service. A fixed introduction fee is likely lawful in Arizona, but nothing settles it outright; sales tax on the fee is likely not owed, but also unsettled. What changed:
- **Before the paid weeks** (playbook section 9, checklist item 8a): a one-page **paid-pilot terms** (`docs/pilot/paid-pilot-terms.md`) that every business accepts on a form before it gets a paid-week card; a short **pilot privacy notice** (`docs/pilot/pilot-privacy-notice.md`) linked from the request form, describing only what the test really uses; an unsubscribe footer with a postal address on every job card; the **Lake Havasu City business license** by week 4 at the latest; refund wording that fits email-only contact; and permit-heavy work going only to licensed businesses, with no splitting of a project to fit the handyman limit.
- **Before launch:** starting a web chat is no longer by itself consent to texts; typing a mobile number and its code is. The privacy policy's breach line now follows the law. The business terms say any tax owed is included. The security program now explains how the FTC Safeguards Rule applies and lists where data lives.

Your call, when you're ready: ask Arizona's Department of Revenue for a private ruling on whether the fee is taxable (the report's advice), or keep researching it yourself as you'd planned. Until then, 8.6% of every fee is set aside.

## Making sure the code comes out right (D-472, October 28)

Every change the build makes must pass three automatic checks before it can join the main code: Ci (tests, strict code rules, security and privacy scans), Twin (the independent rule checker) and Ally (accessibility). GitHub enforces this; nobody, including the build, can skip it. Each change also gets a separate reviewer that sees only the change and the spec, and two full fresh audits come near the end.

I ran the build runner's rehearsal (fake GitHub, fake workers, the real task list) and checked when each safety check switches on. Three fixes:
- **Coverage checking now starts before the money code is written.** It was scheduled about halfway through, after the fee code and after the 95% floors were set, so those floors would have gone unchecked for a while.
- **The deliberate-bug tests now run every night.** They break the 4-month rule, the fee tiers and the license filter on purpose and make sure the tests catch it. Before, they were checked once and never again.
- **Answering "no" to "May Ask Hava text your own phone?" no longer freezes the build.** Before, only "yes" let it continue. A new small task fixes that.

With the fixes, the rehearsal reaches "Finished" with only your planned stops, and a task that keeps failing holds up only the tasks that need it.

## Code that a new AI can pick up cold (D-473, October 28)

You asked for the code to be documented well enough that an AI that has never seen it can understand and change it. The plan already kept the architecture and decision files current with every change, but nothing required the code itself to be explained, and nothing tested it. Now:
- **A guide in every part of the code.** Each of the 19 modules (fees, texting, privacy and so on) has its own short guide file: what it does, which rules it enforces, what must never break and which test guards it, and step-by-step recipes for common changes. Claude Code reads it on its own when it works in that folder.
- **Comments everywhere they help.** Every code file starts with a note on what it does and which rules it implements; every function others use has a description; every rule in the code names its requirement number, so searching "FEE-039" finds the spec, the code and the test together.
- **Enforced, not hoped for.** A new automatic check fails the build if any of this is missing, the same way a failing test does.
- **A start-here map** for the whole project, with the common changes (prices, trades, texts, settings, legal pages) and the files each one touches.
- **A real test at the end.** Before the build is called finished, a fresh AI session with no history makes three real changes (a fee limit, a new job type, a reworded text) using only the docs, and writes down every place it had to guess. Each gap gets fixed before "Finished".

## Getting to "confident" before the build (D-476, D-477, October 28)

You asked what the project needs before I'm fully confident. Two things had never been tested, so I tested them.
- **The Rust build itself.** The switch to Rust came after the build runner's rehearsal, so no build session or GitHub check had ever compiled the code. A throwaway test branch with a small piece of the real setup, at the exact pinned versions, passed every GitHub check: formatting, the strict lint, the server build, tests against a real database, the security audit (no vulnerabilities) and the license check. It also caught three problems, now fixed: the "smoke test" every build step runs first was never written down; the project's ignore list let Rust's build folder be committed and scanned (I made that exact mistake on the test branch myself); and the documentation check from earlier today would have failed the very first real build task.
- **The spec contradicting itself.** After many rounds of fixes, a fresh reviewer read all 646 requirements and found 16 real contradictions, plus 11 smaller wording mismatches. The important ones: free picks (repeat customers, sale inspections) couldn't release the homeowner's details; a forwarded quote link could be used to get a pick code by email; AI was both banned from and allowed to trigger the 911 text; a "job type not certain" card could reach a license that doesn't cover the job; and photos on job cards clashed with the privacy rules. Each was settled by the later decision and fixed everywhere it appeared, including the legal drafts.

One thing for you: the test branch, `claude/spike-rust-ci`, couldn't be deleted from here. It's harmless, but you can delete it on GitHub (Branches, then the trash icon).

## The twin's intake fixes (T005, D-517, October 30)

- **The six "business left out" warnings are gone.** The twin now keeps the words that name the job after the homeowner answers the trade question, repeats the numbered options when it asks again, offers up to three trades, and keeps any job type the homeowner's words name that needs a different license, asking "Which fits best?" when they do. The run that showed the six warnings now shows none, and the 535 waivers for those causes are removed.
- **What's left is listed one by one.** Most remaining items are the simulated homeowner deliberately answering a question wrongly (which the homeowner's own answer decides). Another 185 come from one rare event the simulation now happens to reach: the simulated owner approving a fake business in review (a 1-in-200 chance). That is not an intake problem; the next twin step (T006) looks at it first, because it is about licenses and contact details.

## The twin's other failures, each traced (T006, D-525, October 30)

- **Money and contact details first.** The twin found four real gaps in its own rules, now fixed: a business whose license ran out in the minute between pressing Confirm and the payment still got the homeowner's details (now it gets nothing and its fee is refunded); a business charged while texting was down never got the details and kept paying (now refunded after 72 hours); a homeowner who closed the job after the business tried calling cancelled the business's "couldn't reach them" refund (now still decided at 96 hours); and replying "1 YES" could choose the wrong business if a quote had dropped out of the list since it was sent (now the number means the quote as listed).
- **Texting rules.** Someone who had opted out and then texted HELP got no answer (now they get the HELP reply); a person who never replied OK and texted again a day or more later heard nothing (now their request goes ahead, with their first message kept and no second welcome text, since no reply is ever needed); and a late second answer opened a duplicate job (now it joins the open job when it comes within 2 hours of that job's own last message, as the 2-hour rule says).
- **What can't be prevented is written down.** The rest come from things no rule can stop: the simulated owner approving a fake business in review despite the mailed code (1 in 200), a photo all three checks missed, and the days when the texting provider has suspended the number and the homeowner gave no email and doesn't answer the owner's calls. These stay listed one by one, each naming D-525.
- **One open question for the build, not for you.** When texting is down and a business has already paid and seen the homeowner's details on its job page, the twin refunds it after 72 hours because the confirmation text never went. That conflicts with the rule that the fee isn't refunded once the introduction is made. The app's own build settles it (count the job page as the introduction, or hold the details until the text goes), and the twin follows.

## Why the build stopped after T007, and how it restarts on its own (D-551, October 30)

- **What happened.** The stage-1 cleanup task (T008) looked over stage 1 and found nothing to clean, so it handed in no changes. GitHub won't open a pull request with nothing in it, and the runner was waiting for one, so every hourly run stopped at the same place.
- **The fix.** A cleanup task that finds nothing to change now counts as finished straight away. Any other task that hands in nothing has failed that try and gets the normal retries. The stuck T008 sorts itself out on the next run; nobody edits the build's records by hand.
- **Leaving stage 1.** Stage 1's finish line includes the stand-ins answering over the web, which is T167's job, and T167 was set to run after the cleanup. So the cleanup now waits only for the proof, and lets the rest of stage 1 (T167, T170) go ahead. Once those are merged and the next nightly test run on `main` passes with them in it, the runner records the proof itself from that run and stage 2 starts. Nothing is recorded that the tests didn't show.

## Why T167's retry would have failed the same way, and what changed (D-582, October 30)

- **What happened.** T167's pull request failed the secret scan in `Ci`: the scanner mistook two harmless test arguments for passwords. Its log said only "leaks found: 2", the runner passed the retry just "Ci: failure", and the local smoke test doesn't run the scanner, so the next try had nothing to go on.
- **The fix.** The scanner now names the rule, file and line of each finding (the value itself stays hidden). When a check fails, the runner tells the retry which step failed and adds the last few lines of that step's log, kept short and with no settings or keys in it. The smoke test runs the same scan when the scanner is installed, and the runner's Actions job now installs it, so a worker catches this before handing in.
- **Nothing for you to do.** No check got weaker; the scanner and its pinned version are the same as before.

## The wall between modules is now tested (T175, D-595 to D-597, October 30)

- **What it does.** A new test reads every part of the code and fails if a part reaches up to a part it should not, skips the shared safety settings, adds an unapproved library, or names the "release contact details" key outside the three places allowed to hold it. It reads the allowed-calls table from the build's own file, not from the written description, so the two can't drift apart quietly.
- **Money and duplicates.** Only the payments part may talk to the card processor, and every adapter action that sends or charges something must carry a repeat-proof key, so a retry never sends or charges twice.
- **Nothing for you to do.** These are build rules; no customer sees them.

## The app checks its own settings before it starts (T011, D-688, D-689, October 30)

- **What it does.** When the website or the background worker starts, it reads its settings and stops at once if one is missing, misspelled, a live payment key in the test site (or a test key in the live site), or an account that belongs to the other site. Error messages name the setting, never its value.
- **Logs.** Every log line is one tidy line that Railway can read, carrying a request number and the page pattern (such as `/q/{token}`), never the full web address, a phone number or an email. Error reports to Sentry have personal details stripped first.
- **Business hours.** Timers count only 8 a.m. to 8 p.m. Arizona time, every day of the week.
- **Nothing for you to do now.** At the account checkpoint each Railway site gets a short list of its own account names (Twilio, Stripe, Resend, storage bucket); the deploy notes will say exactly what to paste.

## The database has two separate keys (T012, D-698 to D-700, October 30)

- **What it does.** The app's database now has two logins: one that sets up the tables when a new version ships, and one the website and worker use every day. The everyday login can add records and read them, but it cannot change or erase the money and history records, and it owns nothing it could rewrite. A test checks this on every change.
- **Two people editing one job.** When two changes to the same job arrive at once, the second waits, sees the job has already moved on, and changes nothing.
- **Nothing for you to do now.** At the account checkpoint the deploy notes will say how to run the one-time setup script on each Railway database and which two passwords to paste.

## A test-only remote control that cannot ship (T020, D-739 to D-741, October 30)

- **What it does.** The automated website tests can now fast-forward the app's clock (to test timers such as reminders) and read the texts the fake phone carrier "received". That remote control is only built into test copies of the app; the copy that runs the real site does not contain it at all, and every remote-control address there answers "page not found".
- **Locked even in tests.** A test copy accepts remote-control requests only with a long random password made fresh for each test run, and it refuses to start the remote control on the Railway test or live sites.
- **Nothing for you to do.** This is build plumbing; no customer sees it.
- **Follow-up for the build checks (D-739, D-742).** The automatic checks on every change (`Ci`) still run these remote-control tests only in the shipped form of the app. A later build-system task should add one line to `Ci` that also runs them in the test form (`cargo test -p askhava-security --test control_router_absent --features askhava-core/test-control`); until then they run when someone runs that command by hand.

## Homeowner contact details are locked in a vault (T022, D-749 to D-751, October 30)

- **What it does.** Every homeowner's name, phone, email, street address and on-site contact is now stored scrambled, each piece on its own, under a key that is kept apart from the database password. Someone who copied the whole database would see only scrambled bytes, and a scrambled value moved to another person's record will not unscramble.
- **Who can look.** Only the parts of the app that need a detail can read it, and only the pieces they need: texting and email, the call connector, the release to a paid business (which never includes the homeowner's phone), the homeowner's own page and your owner pages. Every read is written to the record log with who read it and why.
- **An alarm for unusual reading.** If any part of the app reads contact details at more than three times its usual hourly rate, or tries to read for a reason that is not on the list, you get a High alert. The hourly check is built; a later task switches on its hourly run.
- **Changing the key.** The key can be replaced without downtime: old and new keys both work while the app moves every record over, and phone lookups keep working throughout.
- **Nothing for you to do now.** At the account checkpoint the deploy notes will say how to make the two vault keys and where to keep the sealed paper copy.

## Check the phone area-code table before going live (T027, D-765, October 30)

- **What it does.** Texts and business emails go only between 8 a.m. and 8 p.m. in Arizona and in the zone of the person's phone area code. A number whose area code is not in our table is never texted.
- **What to check.** The table was written from memory of the public plan. Before Gate C, compare `app/messaging/area_codes.json` with the official NANPA area-code list (new codes such as 382, 683, 327, 861, 357 and 728 were left out because the zone was not certain, so those numbers are refused), and check codes that span time zones (928, 850, 208, 541 and the other split codes in D-765). A missing code only means no text; a wrong zone could mean a text at a bad hour.

## Names, numbers and emails are cut before any AI sees a message (T023, D-800 to D-802, October 30)

- **What it does.** Before any message or business quote goes to the AI, plain code removes names people give ("I'm Steve", "me llamo Ana", "Thanks, Steve"), phone numbers however they are written (with dashes, spelled out, in Spanish, in odd digit styles), emails (also "steve at gmail dot com"), social handles, web addresses, street numbers and gate codes. A name given earlier in the chat is removed from later messages too. On a test set of 228 tricky English and Spanish messages it catches every contact detail; the build fails if that drops below 99%.
- **Possible over-cutting.** After "I'm" or "it's", an unusual word may be cut ("it's flooding" loses "flooding"). That costs the AI a word, never a homeowner's privacy.
- **Emails.** Inbound email is accepted only with the email provider's signature. A sender whose email fails the standard sender checks can only unsubscribe, never act on a job or change details.
- **One gap for the plan.** Saving the raw message text in its locked store needs a database table that no task builds yet; the runner has the note (D-801).

## Links and IDs cannot be guessed, and a vendor message counts once (T014, D-815 to D-817, D-819, October 30)

- **What it does.** Every job card, quote, pick and quote-page link gets an ID made from a secret key plus what it is about, so nobody can guess one, and a rebuild from backups gives the same IDs. A quote-page link also mixes in a random value saved with the job, so one link can be replaced or switched off without touching anything else; an old or switched-off link shows "not found". When Stripe, Twilio or another service sends the same notice twice (or 100 times), only the first one does anything.
- **One gap for the plan.** The random value behind each link is saved in the database but not yet copied to the off-site record, because that record has no slot for it yet; the runner has the note (D-817).
- **If the secret key is ever changed.** IDs already made keep their old value; only new records get IDs under the new key, and a rebuild from backups uses whichever key was in force when each record was made, so old keys are kept until nothing made under them can be rebuilt (D-819).

## How the app asks the AI, and what it refuses back (T024, D-810 to D-813, October 30)

- **What it does.** The AI only ever gets the already-cleaned message, wrapped so it is read as data, never as orders, and it has no tools: it can only fill in fixed boxes (trade, job type, language and a few yes/no flags). Anything that does not fit those boxes, or names a trade that is switched off, is thrown away and the plain no-AI path runs instead.
- **Limits you can change in settings.** Each AI call gets 20 seconds; the app stops calling the AI after 2,000 calls in one Arizona day and alerts you once; a rejected key or a spending limit also alerts you once a day.
- **Words it will not send.** Any AI-written question or card holding a carrier-banned word (sex, hate, alcohol, guns, tobacco, cannabis and drugs, English and Spanish), a link, or too many characters is replaced by the fixed wording. Everyday trade words like "weed", "nail gun" and "wine fridge" are deliberately not on the list (D-813).
- **Two notes for the plan.** The AI connector does not yet pass back the "try again in N seconds" signal, so for now the app treats every rate-limit as a spending problem and alerts you (D-810); and the practice AI uses trade names that do not match the catalog yet (D-812).

## The AI stays off in the live app until you switch it on (T025, D-841 to D-843, October 30)

- **What it does.** In the live app no message goes to the AI until either the zero-retention agreement is signed or you turn on "AI on scrubbed text". With it off, requests are sorted by keyword rules and the fixed questions, and nothing is sent to the AI at all.
- **What the switch needs first.** The switch won't turn on until the account checklist holds the AI provider's terms: the link, the date you checked them, that they don't train on what we send, how long they keep it, how long they may keep text their safety systems flag, and a monthly spend limit set in their Console at no more than 3 times the expected AI cost. The privacy policy in force that day must also carry the matching AI sentence. If a later policy version drops that sentence, the AI stops the day it takes effect.
- **What gets kept.** For each AI reading the database keeps only the confidence numbers and yes/no flags; the trade and job type go to the outside record copy. The weekly summary gets counts only.
- **One note for the plan.** Until the owner page for the checklist is built, those records are entered directly in the database (D-841).

## A text is never sent twice, and past customers are left alone (T028, D-846 to D-850, October 30)

- **What it does.** Every text waits in one outbox with a tag made from what it is about (which message, which job, which person), never from a phone number. If the app crashes just after the texting service took a text, the retry carries the same tag, so the service sends it only once.
- **Past customers.** Ask Hava never texts someone whose request is finished unless she has written to Ask Hava since then.
- **Business phones.** Each business's main and staff phones are kept only in a scrambled form that can be matched but not read, so an incoming text can be tied to its business.
- **One note for the plan.** For now the code that queues a text tells the outbox whether the person's request is still open and when she last wrote; the later inbox task should take that over (D-846).
- **Phone numbers and message text.** A homeowner's number is looked up in the locked contact store at the moment of sending, never handed in by other code, and the text of a message is wiped from the outbox once it has gone (D-849, D-850).
- **Check with Twilio before going live.** Two things rely on Twilio sending a text only once when it gets the same tag twice: the retry after the app crashed just after Twilio took the text, and two workers sending the same text at once. That needs confirming with Twilio before Gate C; if Twilio doesn't honour the tag, the crash retry could send a text twice and needs another guard (D-847).

## STOP means stop, in English or Spanish (T029, D-866 to D-870, October 31)

- **What it does.** A text that is only STOP, QUIT, END, CANCEL, UNSUBSCRIBE, OPT OUT or the Spanish PARAR, ALTO, CANCELAR or BAJA, in any capitals, or a message like "please stop texting me" or "dejen de escribirme", puts that number on one do-not-text list at once. "Can stop by at 3" or "please cancel my request" doesn't.
- **One confirmation, never two.** Twilio sends the confirmation for its own words; Ask Hava sends it only for the ones Twilio doesn't catch (like "opt out" with a space, or a sentence). If it can't go within 5 minutes, it never goes. If texting is paused, the opt-out is still recorded and the confirmation goes by email instead.
- **Nothing after.** After an opt-out the only texts that can reach that number are the one confirmation and the HELP reply, until the person texts START exactly as Twilio recognizes it (START or UNSTOP on its own); "Yes" or "Start." with a period doesn't count, because Twilio keeps its block for those too (D-870). If they write again, with or without another STOP, and mention something like gas, you get a critical alert.
- **Every way in, one list.** The same list will take opt-outs from email, the help form, the portal switch and requests you record; the app checks it before every text, even if Twilio hasn't blocked the number. A text Twilio refuses because the person opted out (error 21610) is never tried again.
- **One note for the plan.** Adding a new opt-out word now means changing it in three places together: the app, the Twilio settings file and the live Twilio Advanced Opt-Out, so they never disagree (D-868).

## The first reply to a new request (T038, D-907 to D-909, October 31)

- **What a new homeowner gets first.** The first text back to any new request is the consent text, word for word: "Got it. Next, a few quick questions." plus the not-a-contractor line, the automated-texts, frequency, rates and STOP/HELP lines and the Terms and Privacy links. No "Reply OK", no fee line. If the first message mentions gas, smoke, fire, a CO alarm or a downed power line, the 911 text goes first and the consent text right after it.
- **Only once.** The consent text goes once per request, even if the same message arrives twice or two texts arrive at the same moment. A text that is only STOP or HELP never gets it; HELP first, then a request, gets the HELP reply and then the consent text.
- **Spanish.** It goes in Spanish only when the first message reads as Spanish and you have turned Spanish conversations on; until then everyone gets English.
- **"Is this a real person?"** That reply gets "Ask Hava is a local service. Our texts are automated. The Ask Hava owner gets HELP messages right away and usually replies within 2 days, or within 24 hours if your job is urgent." before the next question. The "2 days" and "24 hours" are the spec's fixed words; if you change the HELP reply times in settings, this line does not follow them (D-907).
- **Two new Spanish drafts** wait for the native-speaker review: the "who is this" line and the consent text with the recorded-call sentence, which only goes once the safety voice call is switched on (it is off).

## How a business account moves between statuses (T041, D-953 to D-955, October 31)

- **Six statuses.** A business is pending, active, paused (always with a reason), waitlist, needs review or removed.
- **Activation.** A sign-up goes active by itself only with its identity code and its license or city-list match and nothing flagged. Anything flagged waits in needs review for you, and your approval still needs the code and the match. A removed business stays removed.
- **Insurance.** What a business says about insurance is only stored and shown as it reported it; it never pauses or activates anyone.
- **Night count.** The "after-hours emergencies" tick is stored now and counted for the night message, but the rule that someone on the team must have emergency emails on and pass the test email comes in a later task; nothing sends night cards yet.

## Which licenses get which jobs (T153, D-983 to D-985, October 31)

- **License map.** Which ROC classes get which job types now comes only from the license map. A dual (CR-) class gets its residential class's jobs, KB-1 and KB-2 get the B jobs, and a row marked unclear (for example plumbing R-37R and gas line work) is never passed on.
- **Commercial-only.** A business whose licenses are all commercial-only (A, B-1, B-2, any C-) is stopped at sign-up; so is one whose licenses reach no home job at all.
- **Project limits.** R-62 quotes over $5,000 and R-61 quotes over $50,000 (top of the range) are refused when that is the business's only license for the job. The dual CR-61 has no limit in the map; please confirm whether it should carry R-61's $50,000 limit.

## Danger words, SAFE, and the emergency choice (T039, D-978 to D-981, October 31)

- **Every message is checked.** Each text, not just the first, is checked for the 911 list: gas or propane smell or hiss, a CO or other alarm going off, smoke or fire, a sparking or burning outlet or panel, a downed power line. A match gets the one 911 text and no business sees the request until she replies SAFE (or OK or Yes). The AI's reading never sends or stops that text.
- **Not on the list.** A burst pipe, an AC out at 92 degrees, no heat, feeling dizzy, a gas water heater that won't light, a fire pit, a detector that chirps: no safety text, a normal request. A street name like Smoketree Ave never counts, and her address answer is never checked.
- **A beeping alarm.** "Smoke detector beeping" gets one question: "Is it 1) a chirp every minute or so, or 2) an alarm that keeps going off?" Answer 2 gets the 911 text; anything else is a normal request.
- **Emergency or regular.** The choice text adds "Is this an emergency?" before the spec's words and "Reply 1 for emergency or 2 for regular." after them, so she knows how to answer (D-979). At night she first gets the count of businesses that take emergency requests at night; with none, she gets "Your request goes out at 8 a.m." and no choice.
- **For you to look at.** One test-design line says the "no business at night" text "holds the 911 line"; the spec's own wording for that text has no 911 line and the D-436 rule allows no other safety text, so the spec's wording was built (D-978).

## The trade question and two-trade messages (T042, D-998, D-999, October 31)

- **A second opinion.** Each request is read by the AI and also by plain keyword rules. When they disagree, she gets a numbered question offering every trade any of them named, up to three.
- **Her answer.** A number, "Not sure", or words of one option ("the AC one", "just a small repair") count; "repair" never reads as heating/AC. If her reply fits nothing, the same numbered question comes once more; after a second miss, Ask Hava goes with the keyword trade (if exactly one trade matched) or the AI's first reading, and moves on.
- **The job keeps its words.** Once she picks a trade, the job keeps the job type her own words named ("AC blowing warm air" stays "AC not cooling"), so AC-only businesses still get the card. That fixes the twin's six warnings.
- **Later messages.** More than 2 hours after her last message, a message about the same trade joins the open job only if at least half of its words (leaving out words like "the" and "my") are in the job's description.
- **For you to look at.** The "That's 2 jobs" and SPLIT texts and the keyword list living in the catalog file are not built yet; the rules that detect two trades and the ALSO / TAMBIÉN reply are, and the texts go to the task that owns the intake texts (D-999).

## Requests we turn away (T043, D-1009, D-1010, D-1011, October 31)

- **Not in our towns.** A request from a town outside the towns list gets "Ask Hava: We don't cover your town yet, so we can't take this request. Text us any time." and the request closes. The spec fixes no wording for this one, so please read it (D-1009).
- **A trade we do not offer.** She gets the spec's "We don't cover <trade> jobs yet" text and a line is noted so we can recruit.
- **Refused work.** A move that leaves Arizona, or a home watch or turnover cleaning request asking for rent collection, finding or screening tenants, showing the home to renters or leasing, gets the same text but is not counted as recruiting demand. Plain words like "renters check out" do not trigger it (D-1011).
- **For you to look at.** The owner email for new demand is not sent yet; the demand row is there for whichever task builds that email (D-1010).

## Who gets a card, pauses and the count line (T184, D-1018 to D-1021, October 31)

- **Jobs nobody else covers.** When no business that ticked the job type can get the card, it goes to every licensed business that left "also send me jobs nobody else covers" on. If even one ticked business gets it, the switch is not used.
- **Pauses.** A paused business gets no cards, and neither does one under your "all" switch, its trade's switch, or the switch of every town it would get the job through. Pausing the job's own town stops every business for that job, even one that also serves a neighbouring town.
- **Late cards.** A business that becomes able to take a job while it is still Open or No quotes yet gets the card then, with its own 24 hours from that moment. After that, nothing.
- **What she hears.** "Ask Hava: Got it. We'll send this to the 5 <trade> businesses on Ask Hava." (or "the 1 <trade> business"), the number being exactly who would get the card. If nobody in her town covers it, she gets the "We don't cover <trade> jobs yet" text and it is noted for recruiting.
- **SPLIT.** A request needing two kinds of business with nobody licensed for both gets "Ask Hava: This needs 2 kinds of business. Reply SPLIT to send it as 2 requests, one per trade." SPLIT (or DIVIDIR) opens the second job; each goes out alone.
- **For you to look at.** The Spanish versions of these texts are drafts for your translator. Nothing yet calls the late-card step when a business unpauses or ticks a new job type, and nothing yet reads your pause switches into matching: both are wiring for a later task (D-1021).

## What a business sees on a job card (T045, D-1024 to D-1027, October 31)

- **What's on it.** The town, the kind of place (house, marina, storage lot or ramp), the homeowner's approved description, her multiple-choice answers, "How soon: <her answer>" ("Needed by Wed" when she gave a day), "911 text sent: gas" (or CO alarm and so on) when that text went out, and checked card photos. There is no place on the card for a name, phone, email, street address or on-site contact.
- **The last check.** Every line goes through a final plain-code check. In any sentence with something that looks like a contact ("ask for Pat", "text me", "my number", an email, a street number, a phone number in any spacing or spelled out), that word and the rest of the sentence are removed. It leans toward removing too much, so now and then an ordinary sentence loses its end. If fewer than 5 words of her description are left, the card says "Plumbing job (details on the job page)".
- **For you to look at.** Both reviewers thought a contractor might read "911 text sent: gas" as "911 was called." That wording is fixed by the spec (D-436), so it wasn't changed. If you'd like it clearer, something like "We sent the homeowner our 911 safety text (gas)" would be a spec change.

## Task size numbers re-measured (D-1036, October 31)

- **What you approved.** Each task tells its worker roughly how much of its reading space it will use. Those numbers were stale (about half the real size), and nearly every task was over the old 20% limit. They are now measured fresh for every task, and the limits are 45% for ordinary tasks and 50% for tooling tasks.
- **Two calls Claude made.** A clean-up task that sweeps the whole app folder is no longer counted as reading every file in it, since it only searches it. The one clean-up task that also runs the full twin check (T126) measures 59%, so clean-up tasks got their own limit of 65%. Three finished twin tasks (T005 to T007) measure 63 to 69%; their numbers are kept as measured under a limit only they can use.
- **Keeping the numbers honest.** Every plan change now re-checks the numbers strictly before it is committed. Ordinary task merges still only note small drift, because every merge moves some numbers by a point and a task cannot fix the task list itself.

## Task size numbers no longer churn every plan change (D-1075, October 31)

- **What changed.** Each task's size number moved by a point whenever the shared architecture notes grew (almost every task reads them), so even a one-line plan fix had to rewrite about 90 task prompts. A stored number now stays as long as it is within 3 points of the real measurement and under the task's limit. The limit itself still binds the real measurement: a plan change fails if any task's reading actually goes over it.
- **Calls Claude made.** 3 points (about 24 KB of reading) was picked so roughly a dozen task merges pass between refreshes. The decisions log was suspected but turned out not to be the cause: prompts only paste the decisions they name. Separately, the PII scanner now prints a warning once the decisions log passes 75% of its size limit; it warns at 75% rather than fails, so a growing log never turns the build red by surprise; past 90% the plan-change check itself refuses to pass until the limit is raised or the file split, so the session that can act must. One task (T156, the intake question bank) sits exactly at its size limit, so the next plan change after it lands will have to give it the same "keep the budget it was built with" allowance three stage-1 tasks already have.

## T156 was reverted (FIX-MAIN, D-1053, October 31)

- **What happened.** The intake question bank (T156) was reverted because main went red after its merge. Its decisions D-1037 to D-1041 stay in the log but are void until T156 is re-run.

## The test that kept turning main red for no reason is fixed (D-1086, October 31)

- **What changed.** A test of the phone-call client failed now and then (about six false alarms, and main went red after T156). The cause was in the test setup, not the app: the test picked a free network port, then started the fake vendor server on it, and another test running at the same moment could grab that port in between. The test now waits for the fake server itself to say it is listening on its port, and starts it on a new port if it could not get the first one.
- **Calls Claude made.** No test was loosened, skipped or retried, and the app's code did not change. The fake vendor server (a protected file) was left alone. One other crate (the AI tests) has the same setup and is left for a later fix.
