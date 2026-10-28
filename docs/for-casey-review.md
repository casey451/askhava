# For Casey's review

These are decisions Claude made on its own and logged so the build never waits (`BUILD_BRIEF.md:542`). None of them blocks anything. Each item names its entry in `docs/decisions.md`, which has the exact source lines. The ones most worth a second look come first.

## Your first item: ask Twilio in writing before registering (October 28, 2026)

This comes before everything else in this file. Texting registration is the one step that can't be undone: if Twilio refuses the campaign as "lead generation" (error 30951), it can't be resubmitted. So ask first, in writing. Nothing has been sent; you send it. (D-305, D-322)

- **Sign as "Casey Solomon, Ask Hava (LLC being formed)"** until the LLC exists. Once it's formed, use "Casey Solomon, Ask Hava LLC".
- **Send the Twilio question** as a Twilio Support ticket. Save the case number and the written answer, and file the answer in the owner portal when it's built.
- **Send the Telnyx question only if Twilio's answer is unclear.**
- **What happens with each answer:**
  - Homeowner texts "yes": register the homeowner campaign.
  - Business texts "no", or no clear answer: nothing changes. Businesses get everything by email, plus a phone call when they're picked.
  - Business texts "yes": you may later switch business texting on, on its own campaign and its own number. Each business then chooses email, text or both.

### To Twilio (send as a Support ticket before registering)

> **Subject:** Eligibility question before A2P 10DLC registration – local quote service
>
> Hello. Before I register, I'd like a written answer on whether my use case is eligible. I don't want to submit a campaign that gets rejected and can't be resubmitted.
>
> **How it works:** Ask Hava (askhava.com; the LLC is being formed) is a local service in Lake Havasu City, Arizona. A homeowner texts our number, or starts on our website, to ask for quotes on a home job, and replies OK to our consent message (frequency, rates, HELP, STOP, Terms and Privacy links). We send an anonymous description of the job (town, job type, details; no name, phone number, email or address) to every local business that signed up with us and opted in to receive job requests. Businesses send their prices to us. We show all of them to the homeowner, unranked, on a private page, and the homeowner picks one. The picked business pays us a fixed introduction fee ($25, $75 or $199 by job size), and only then do we give that one business the homeowner's first name and address. The business calls the homeowner through a masked number using Programmable Voice. The homeowner's phone number is never given to any business, businesses never text homeowners, and no consent or phone number is shared, sold or transferred. Homeowners pay nothing. We send no marketing texts. At launch, businesses get job requests and picks by email, plus an automated phone call when they are picked; we send no texts to businesses unless you tell us that is allowed, and then each business would choose email, text or both.
>
> **Questions:**
> 1. Are texts between Ask Hava and the homeowner about their own request (confirmations, "your first quote is in," the pick confirmation) eligible as Customer Care?
> 2. Are job-alert texts to businesses that signed up and opted in (anonymous job details, "reply with your price") eligible? If so, should they be a separate campaign on a separate number?
> 3. Are texts telling a picked business it was picked and showing the fee eligible? We plan never to text the homeowner's name or address; those appear only on our website.
> 4. If question 2 or 3 is "no," is question 1 eligible on its own while businesses are notified by email?
> 5. Will our privacy policy sentence "Your first name and address go only to the one business you choose" be treated as "sharing consumer information" under errors 30871 or 30951?
> 6. Do masked voice calls and Twilio Verify codes need anything beyond Trust Hub and SHAKEN/STIR?
>
> Please reply in writing, question by question, and tell me any changes that would make a "no" into a "yes." Thank you.
> Casey Solomon, Ask Hava (LLC being formed)

### To Telnyx (send only if Twilio's answer is unclear)

> **Subject:** Pre-registration question: is this 10DLC use case eligible? (code 708)
>
> Hello. I'm considering Telnyx for SMS and voice. Your 10DLC error-code page lists 708, "Lead Gen/Affiliate Marketing Prohibited." Before I open a brand, I'd like to know whether my use case would be treated that way.
>
> *[Paste the same "How it works" paragraph as in the Twilio question, with "a masked number on your Voice API" in place of "a masked number using Programmable Voice".]*
>
> **Questions:**
> 1. Would texts between us and the homeowner about their own request pass vetting as Customer Care?
> 2. Would opted-in job alerts to businesses pass, and as which use case?
> 3. If only question 1 passes, can we run it alone while businesses are notified by email?
> 4. If a campaign is rejected with 708, can it be appealed or corrected?
>
> A short written answer is fine. Thank you.
> Casey Solomon, Ask Hava (LLC being formed)

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
- Consent replies OK, Yes, Sure and Sí are all accepted. (D-021)
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
- Web chat shows the consent text first. A phone number typed there gets no texts beyond the consent text until that phone replies OK. The consent text and web form say users must be 18 or older (D-051, D-052).
- Language: the first consent text goes in Spanish when the first message reads as Spanish. AYUDA works like HELP. Businesses can pick Spanish texts. Every text the app writes starts "Ask Hava:", and Twilio's code texts carry the name "Ask Hava" (D-063 to D-066). Native-speaker review of the Spanish texts is a Gate C checklist item (D-067).
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
6. **A request that never gets an OK** to the consent text closes on its own after 72 hours, with no reminder and no closing text, because nothing but the consent text may go to a number that hasn't agreed. Once someone has said OK, they get the 24-hour reminder and the "Text us any time to restart" message as usual. (D-095)

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
4. **More replies count as "yes" to the first text (changes D-021), and people who answer something else get the first text once more (changes D-095).** "okay", "yes please", "Y", "yep" and Spanish "si" (without the accent) now count. Any other answer gets the consent text once more, and someone who never answers gets it one last time at 24 hours. Before, they heard nothing, ever. (D-127)
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

1. **Have a lawyer check the safety voice call (B19).** During a texting suspension, a homeowner with no email whose own message matched a danger word gets one automated call from the Ask Hava number that reads the safety text. It rests on the homeowner's own just-sent request, not on the narrow "emergency purposes" exemption. That reading is a research-based judgment, not legal advice. Until a lawyer has looked at it, treat it as unconfirmed. Your critical alert still fires either way. (D-206)
2. **Fix the pitch and flyers by hand before any flyer goes out (A1, A13).** The prototype and the go-to-market pitch live outside this repository, so only the spec could be changed. The spec now stops the build from copying the errors (WEB-018, WORD-017). By hand, change these: ongoing services count as 4 months; remove the "What a lead can cost elsewhere" table, or keep only IRS, SEC or FTC figures with the source on the page; remove the pest row; use "Launch offer: your first win is free, on any job size, when you join in the first 90 days or within 90 days after your trade opens."; remove "Skip tenant requests" and the identity "Email me" choice; replace "no bidding wars" with "You quote once. No bidding rounds, no paid placement."; add "Every card says how many businesses got it." and the five objection answers from friction report section 8.2 that match your answers. (D-172, D-184)
3. **Recruit about 2 to 4 businesses per trade** until a trade averages about 1 confirmed pick per business per month. Open small trades later, or with 1 or 2 businesses. Put this pace in the go-to-market plan, section 3.3. There is still no cap, and a business that finds Ask Hava on its own can still join. If a trade ever has 7 or more businesses and its typical job gets 5 or more quotes two months in a row, the owner portal flags it, and a 5-quote limit comes back to you as a decision. (D-189)
4. **Start the founding interest list yourself (A10).** Note each business, its trade, how it would like to be reached and its OK to be contacted once at launch. Then sign each one up in person during launch week. Nobody can sign up before Gate C, and the list must never be built from the ROC or city lists. (D-181)
5. **An attorney review of the business data page is optional but worth paying for (B16).** Every line on it must be exactly true. It covers what's kept and for how long, who sees it, no call recordings, never asking about workers, police or others getting data only when a law requires it (such as a subpoena or warrant), and that you don't use Ask Hava data in any other job. (D-203)
6. **Whether to mention police work (C8).** The "Who runs Ask Hava" block leaves it out until the interviews answer this. (D-183)
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
- **C8. Is your being a police officer a plus, a worry or nothing?** (A12, B16 wording) New: "The person running this is a local police officer. Does that matter to you, either way?"
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
6. **A lawyer's check (optional).** It would cover the safety voice call, which now also goes when an email is on file (D-206, D-210), and whether a safety call to a number that replied STOP could ever be allowed (it is not done now). It would also cover the heat text (D-208) and the tenant text (D-216). The wording is generic information either way.
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
2. **A TCPA attorney's written opinion.** The automatic safety voice call is now **off** until you record that lawyer's written approval in the portal. Until then, every emergency message during a texting outage pushes to your phone. Other questions for the lawyer:
   - Does STOP also cancel calls?
   - What does the FCC opt-out rule reported to start **Nov 25, 2026** mean for Ask Hava (as reported by research; verify the date)?
   - Can you call after a texting suspension?
   - Is keeping a scrambled (hashed) phone number enough proof of texting permission?
   (D-275, D-298)
3. **A CPA or ADOR on sales tax (TPT).** The introduction fee may not be taxable at all, so don't get a TPT license just in case. Send them the exact question in the research (Part 4 §3). If they say it isn't taxable, the receipt line "Tax included." (D-225) should change; tell Claude and it will. (D-302)
4. **Stripe's written OK** for the introduction-fee flow and merchant category, before the first charge. (D-302)
5. **Insurance quotes.** Get general liability and tech E&O quotes, and maybe cyber. The research estimates about **$1,300 to $5,450 a year** in total; that's a planning range, not a quote. Tell the insurer the whole model (AI intake, quotes from several businesses, masked calls, addresses, the safety texts), and ask in writing what is excluded. (D-302)
6. **Setting up the LLC.**
   - A statutory agent hides only the agent's address. Member or manager addresses may still be public, so check what address you can lawfully use before filing.
   - An agent in Maricopa or Pima County avoids the Mohave newspaper notice (three publications within 60 days).
   (D-302)
7. **City business license.** $196 new and $140 renewal on the 2026 schedule. The file is still labeled "proposed", so verify with the city. File the home-occupation form too. (D-302)
8. **FinCEN beneficial-ownership (BOI) report.** Not required for a US-formed LLC under the Aug 2026 rule; verify before you skip it. (D-302)
9. **The trademark attorney.** Bring Grok's list.
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

1. **Send the Twilio question** at the top of this file. Send the Telnyx one only if Twilio's answer is unclear. Sign as "Casey Solomon, Ask Hava (LLC being formed)". This reverses the earlier "don't ask Twilio first" choice (D-088). (D-305)
2. **Email setup at the account checkpoint.** Set up the askhava.com email records (SPF, DKIM, DMARC) with the email provider (Resend), so business emails don't land in spam. (D-318, D-322)
3. **Ask the TCPA attorney one more thing:** does the business sign-up checkbox cover the recorded pick calls? It already asks for consent to automated calls, and the new wording names each channel. (D-312, D-322)
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
