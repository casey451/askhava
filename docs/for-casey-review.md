# For Casey's review

These are decisions Claude made on its own and logged so the build never waits (`BUILD_BRIEF.md:542`). None of them blocks anything. Each item names its entry in `docs/decisions.md`, which has the exact source lines. The ones most worth a second look come first.

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
