# Decisions in force

Through D-366. Reader layer over the append-only `docs/decisions.md` (D-3856, D-4447), shaped by `docs/reviews/decision-digest-plan.md` and checked by `python3 scripts/check_decision_digest.py`. One line per rule in force; the log keeps the full entry (`grep -n '^- \*\*D-####\*\* ' docs/decisions.md`). "The brand" is Laftly (D-3347); older entries say Ask Hava.

## Product and scope

- D-018 (product) The directory-era /chat page goes; the request web chat is built as the fallback channel.
- D-028, D-189 (product) No weekly maximum, cap, rotation or "trade full" waitlist; volume is set by job types, towns and pause; OPS-027 only flags a crowded trade (7+ active businesses, median job 5+ quotes, 2 months running).
- D-031 (product) The catalog starts at 39 categories; garage doors and junk removal are listed and off, the "left out" kinds are listed as not offered, pest control is on but never advertised.
- D-082 (product) Any move that starts or ends outside Arizona is refused.
- D-086 (product) The service area is a towns list in settings, starting with Lake Havasu City; requests from other towns close early.
- D-172, D-268 (product) Pages take every number and offer from the spec, never the prototype (WEB-018); the pitch and flyers are fixed by hand outside the repo.
- D-181 (product) Founding businesses come from Casey's own short interest list, signed up in person in launch week; never built from the ROC or city lists.
- D-194 (product) No in-person ID check at launch; stale record contacts use the self-repair path and the review target.
- D-269, D-301, D-321, D-342, D-354 (product) Not adopted (Casey may revisit): tap-to-ask, multiple site-visit picks, tenant WAIT, storing lock codes, business texting, WhatsApp or an app at launch, open or click tracking, AI reading of HELP messages, and each entry's other listed items.

## Brand and wording

- D-007 (wording) The opt-out confirmation says to text START to use the service again.
- D-023 (wording) Quote-card insurance line: "Insurance (business-reported): Yes/No".
- D-029 (wording) The ban on claims and rankings covers businesses; the neutral job-type prompt to the homeowner is exempt from the word scan.
- D-064 (wording) Every text the app composes, English and Spanish, starts with the brand prefix; Twilio Verify uses the brand as its friendly name.
- D-083 (wording) Feedback is private to the service: never public, never on quote cards, never used to order quotes.
- D-175 (wording) The waitlist message says "Your join date is kept."; the free-win line says "on any job size".
- D-184 (wording) Business copy: "You quote once. No bidding rounds, no paid placement." and "Every card says how many businesses got it."; never "no bidding wars".
- D-245 (wording) WORD-019 bans the three phrases the entry names; WEB-018 covers homeowner pages; WORD-005: "#1 means first received, not first choice."
- D-300 (wording) WORD-020: never "lead(s)" (except lead paint, lead pipe, lead-based), "pre-vetted", "exclusive", "affiliate" (outside TERMS-009) or "partner(s)"; the only exclusivity claim is "Only the business you pick gets your contact info."
- D-349 (wording) No personal name in any template, page, email or document for users (WORD-021 scan); copy says "the owner"; internal requirements may say Casey.
- D-352 (wording) Always the full brand name, never a fragment of it; Spanish copy never says leads, "clientes potenciales" or "prospectos"; the site says it is not connected to similarly named businesses.
- D-356 (wording) One not-a-contractor statement in long and short forms, English and Spanish: site footer, help page, terms, privacy policy, quote page and consent text (WORD-002, WEB-015, TERMS-004).
- D-357 (wording) TERMS-026: businesses are independent; the service never directs, prices or negotiates; the introduction fee by job size ($25/$75/$199) is never a percentage and does not depend on the work being done or paid.
- D-358 (wording) Quote amounts and periods show exactly as the business wrote them; the monthly conversion is for the fee tier only; WORD-011: never edit, round, convert or suggest a price.
- D-359 (wording) Copy sweep: "chose your quote" not "picked you"; "Busy until / Busy for <n> days" not "Booked"; calls are "on a masked number", never "through" the service.
- D-360 (wording) WORD-022: one NOT_A_CONTRACTOR banned-phrase list in `scripts/trace_spec.py` (hiring, dispatching, scheduling, "our pros", "our network", "guaranteed", "our price", Spanish forms); any hit fails.

## Homeowner intake

- D-095 (intake) The 72-hour Intake close moves the job to Closed early ("no reply"); reminder and closing text only when consent was recorded, else a silent close.
- D-102 (intake) After a trade answer, keep that trade's plain-keyword job types; `<trade>_other` only when no reading or keyword gives one, then "Which fits best?".
- D-103, D-104 (intake) The trade question offers up to three distinct trades, numbered; re-asks repeat the options; after two unusable answers fall back to the single keyword trade, else the first reading.
- D-105 (intake) License-boundary keyword job types stay candidates whatever trade is read; candidates under different licenses trigger "Which fits best?" (INT-014).
- D-128 (intake) The safety text ends "Reply SAFE when you are safe."; safe words (English and Spanish) and, after consent, plain OK or Yes count; a consent reply while safety is open gets one "Are you safe now?" (INT-025).
- D-137, D-165, D-325, D-361 (intake) Emergency stems per INT-002 (English and Spanish; "hot" only with an electrical word; a chirping detector is a low-battery reply, no 911 hold); checked per message after address patterns are removed, never on the name-and-address answer.
- D-138 (intake) An emergency-keyword message opens a job whatever its length (INT-007).
- D-146 (intake) Scrubbing (AI-002) removes name intros, phone-shaped numbers, street numbers and emails; first name and address are asked last and never go to the AI (INT-027).
- D-155, D-246 (intake) Own-or-rent is asked when the reading is missing or unsure or there is no AI, with "Own the home, rent the lot" and "Second or winter home"; a message naming two trades starts the first and offers the second.
- D-157 (intake) The gas keyword list lives in the catalog (gas, propane, pilot, furnace, gas appliance words, Spanish gas, propano, piloto; AI-012).
- D-164 (intake) Emergency job cards go once consent, safe reply, urgent choice, ZIP and area check are done; own-or-rent and name-and-address follow; the release waits for the address (INT-029).
- D-208, D-274, D-292 (intake) INT-030 asks "How soon?" (today, few days, no rush, by a certain day) as a card field; INT-031 AC or cooler plus heat words sends the heat-safety text at any hour, before consent, and offers the emergency path.
- D-209 (intake) A gas or CO match adds the call-your-gas-company line (INT-004); the generic gas line stays in every safety text.
- D-216 (intake) TXT-028: after a RENT answer, one informational landlord-responsibility text; no WAIT option.
- D-238 (intake) INT-027 says only the picked business gets name and address after the pick and "We never ask about immigration status or ID."
- D-242 (intake) INT-016: the category check is the first step after consent; WEB-023 lists open and not-yet categories (never pest) and says Canadian numbers work.
- D-271, D-324 (intake) INT-033: at night, before the urgent choice, the count of after-hours businesses for the trade (singular form for 1); with none the choice is not offered; one "no quote yet" text after 60 minutes, day or night.
- D-272 (intake) INT-004: a CO or symptom match puts the get-outside-and-call-911 CO text first, then the gas-company line; a gas match adds "Don't flip switches or light anything."
- D-273 (intake) INT-034: electrical and water matches add their own safety lines and keep the generic line.
- D-276 (intake) CARD-007 adds the fixed field "Safety text sent: <kind>", never free text.
- D-326 (intake) INT-035: a gas match gets one "Has your gas company checked it?" reply that never holds the request; the answer is a fixed card field.
- D-350 (intake) INT-036: safety contact numbers live in `app/messaging/safety_contacts.toml` with source URL, verified date and review-by date; the PII scan allows only that file's numbers.
- D-364 (intake) INT-034's water line: home shutoff only if known and safe, never the meter; street or meter leaks go to the water provider from the INT-036 file, never guessing which serves the home.

## Businesses and licensing

- D-017 (licensing) Licenses are checked from the agencies' daily downloadable lists plus a live lookup at the pick; v9.3's "never guess" rules stay.
- D-032 (licensing) Category licenses: solar screens R-65; boat repair and detailing need no license; dock, lift and seawall go to ROC licensees; gas appliance repair is R-63 only; `handyman_allowed` per the catalog.
- D-035 (licensing) A construction-type quote under the handyman exemption or in a no-license construction category must be under $1,000.00 at the top of its range; never applied to non-construction categories.
- D-056 (licensing) Daily check of every license; a card goes only if the latest check showed Active; one live lookup at the pick; Confirm re-checks stored results with no second lookup; a failed pick lookup uses a daily result under 24 hours old.
- D-057 (licensing) License line: "<Agency> license #___ showed Active on the <agency> website when we checked on [date]. Check it yourself."
- D-058 (licensing) License coverage is judged per job type (CARD-001): R-39R only air-conditioning, R-37R only drain and sewer job types.
- D-059, D-254 (licensing) Identity codes and letters go only to a public-record contact, never one the business typed; before a letter, sign-up shows the town and ZIP and offers to fix the record first.
- D-061 (licensing) Insurance is a business-reported yes/no on no-license cards only; never verified, tracked or used to pause.
- D-063 (licensing) Business notices are English unless the business picks Spanish in its portal.
- D-081, D-186 (licensing) Handyman quotes use the portal form with the certification box on the price screen, ticked on every quote; overlapping scope is the same category or a shared job type at the same property within 30 days.
- D-145, D-256, D-281 (licensing) A sign-up with identity done and a license or city-list match and no flag activates automatically; flagged ones wait for Casey; the letter path stops being a flag once its code is entered (BIZ-003, BIZ-016).
- D-154 (licensing) The split-job check runs on the second quote and blocks only when the jobs share a job type or together reach $1,000 (LIC-023).
- D-027 (licensing) The split-job check's short window is 30 days, a setting.
- D-176, D-261 (licensing) BIZ-021: sign-up starts with categories marked "Open now" or "Coming later"; coming-later only joins the waitlist; when a trade opens each waitlisted business gets one account notice.
- D-177, D-255 (licensing) LIC-011: a city-list sign-up with no match waits as pending and activates when a later list matches its business, DBA or owner name; it may enter its city license number.
- D-179 (licensing) BIZ-024: anyone may fill in the sign-up; the code still goes only to the record contact.
- D-190, D-267 (licensing) Trust lines: "<Agency> license since <year>" from the record, the city-license year only if listed; a screened 140-character About line; no rating, review, hire count or ranking (WORD-016).
- D-191 (licensing) BIZ-023: team members have "Cards and quotes" or "Confirm and billing" roles (owner has both); sign-in, consent and language are per person.
- D-198, D-259 (licensing) BIZ-028: PAUSE, RESUME and BUSY <n> (1 to 60 days), with Spanish forms, only from the owner or a "Confirm and billing" member; never read as opt-outs and vice versa.
- D-199 (licensing) LIC-008: when status is unknown the business stays as it was up to 72 hours, warned at 24, then paused; it cannot Confirm meanwhile.
- D-250 (licensing) BIZ-025's "Your numbers" panel adds "Picked, then finished" from DONE, NO and OTHER replies.
- D-230, D-282 (licensing) LIC-039: a list failing 12 hours alerts; the throttled live lookup covers active businesses meanwhile; still failing after 24 hours joins the OPS-032 push, before LIC-008's pause.
- D-253 (licensing) BIZ-020: sign-up status messages (never the code) also go to the sign-up email and, with consent, the phone entered.
- D-257 (licensing) LIC-040: listed trades get multiple-choice card-field intake questions, never free-text model numbers; LIC-041: trades split job types into repair, replace and new install where the license allows.
- D-258, D-311 (licensing) BIZ-033: the owner is emailed when a non-owner confirms or links a phone; a member-linked phone works only after the owner approves; a STOP stops texts to that phone only, the business stays on email.
- D-260 (licensing) Removing a member ends their sessions and unlinks every phone they linked (SEC-004, CALL-012).
- D-285 (licensing) BIZ-034: "Away until <time>" up to 4 business hours delays a non-emergency pick's Confirm clock; never on emergency jobs; an Away-delayed pick cannot be held.
- D-307, D-308 (licensing) BIZ-036/037: each business chooses its notice channel (email by default and only, while business texting is off); cards go to "Cards and quotes", picks and money to "Confirm and billing", each by their own email.
- D-309, D-333 (licensing) BIZ-038: every former keyword is a no-JavaScript portal page one tap from a signed single-purpose email link; Confirm needs a session or a Verify code; email replies never act.
- D-312 (licensing) BIZ-040: the sign-up email is verified before activation; a bounce or complaint marks it unverified with a banner; a changed notice email is verified first and the old one told.
- D-328, D-362 (licensing) BIZ-042: onboarding sends a sample card email and shows per-mail-app steps until it lands in the inbox; the after-hours tick needs a Do Not Disturb test email, a Yes lasting 90 days.
- D-329 (licensing) BIZ-043: each person may add one verified backup notice address.
- D-330 (licensing) CARD-022: one daily open-cards email at the person's chosen time, only with unquoted cards with an hour left, never to a paused or busy business.
- D-331 (licensing) BIZ-044: a monthly email when at least 3 cards came and half ended unquoted, pointing to the deliverability steps.
- D-336 (licensing) FEE-022's decision email says which condition was met; BIZ-022: the public business name passes the same channel and link rules.
- D-346 (licensing) CARD-021: emergency card and pick emails carry an EMERGENCY subject and first line; at night only to people with "Emergency emails at any hour" at after-hours businesses, others at 8 a.m.

## Matching, quotes and picks

- D-011 (matching) The job moves to Quotes ready at the first quote; quotes are accepted until each card's window ends or a pick is confirmed.
- D-030 (matching) An Open job with no quote moves to "No quotes yet" at 6 p.m., with its text.
- D-042, D-347 (matching) Each business's 24-hour window starts when its own card is sent (after any quiet-hours hold); a newly eligible business gets the card with its own 24 hours; no open or click tracking (PICK-001).
- D-043 (matching) Quotes are accepted while the job is Open, No quotes yet, Quotes ready or Awaiting confirm; refused once the picked business confirms.
- D-044, D-150 (matching) When the pick's payment or free win is recorded, other quoting businesses get "picked another quote"; at the end the rest get "no longer active" (PICK-026).
- D-045, D-140 (matching) A failed pick's quote shows "no longer available"; the job closes only when no choosable quote is left and every card window has ended, else it waits with a fresh 3-day timer (JOB-025).
- D-046 (matching) With no reply 7 days after the day-14 check-in, the job moves to Closed, with no review request.
- D-049, D-143 (matching) For email contact the portal shows the preference and the masked line still opens; the day-3 check-in decides, NO records a no-show and returns to Quotes ready; for call contact NO closes the job.
- D-055 (matching) An emergency is never a reason to close early.
- D-068 (matching) A text pick works when exactly one job is in Quotes ready, else the service asks which job.
- D-084 (matching) Quotes show in arrival order only, no sorting or filtering; numbers never change.
- D-147 (matching) "<n> YES" for another quote before Confirm replaces the pick and tells the first business (PICK-032); after Confirm the reply says it is confirmed (PICK-036).
- D-149 (matching) A price read from a text is always shown back to the business to confirm before the homeowner sees it (PICK-006).
- D-185, D-262 (matching) PICK-039: the quote form shows the fee and its basis (midpoint, recurring months, cap) before the business submits.
- D-188 (matching) CARD-019: every card says "Sent to <N> businesses."; the portal job page shows sent and quotes so far.
- D-193, D-263 (matching) PICK-026: with 3+ quotes the loss text compares the picked quote (lower, about the same within 10%, higher), never disclosing amounts or names; with 4 or fewer recipients it says "Several businesses quoted".
- D-201 (matching) No business number sharing; the details text tips "on your first call, ask which number to text" (PICK-024).
- D-202 (matching) CARD-007: cards carry the category's multiple-choice intake answers as fixed fields; the final card gate still runs and photos stay off.
- D-212, D-294 (matching) PICK-025: "Who will let the business in?" (me, someone else, lockbox); an on-site name and number pass only with the release and are never texted; lock codes are never stored.
- D-233, D-283 (matching) PICK-040: every quote carries required quote facts in a fixed order, pre-filled from the business's last answers with "Same as last time"; earliest visit is a date plus part of day.
- D-234 (matching) PICK-041: each quote card shows the public business name and a fixed line that the service shows no reviews and does not rank; the page shows no homeowner contact or business phone or address.
- D-235 (matching) PICK-018: at the pick the homeowner chooses a call window; only attempts inside it count toward the unreachable refund (FEE-022).
- D-239 (matching) TXT-031: LIST (LISTA) sends the quotes by text in arrival order, up to 10 per text, ending with how to reply "<n> YES".
- D-240 (matching) JOB-028: LATER, ONGOING and OTHER check-in replies, Spanish forms; LISTO counts as DONE.
- D-241 (matching) TXT-032 says how many businesses got the request and the quote deadline; JOB-010 offers keep waiting, add details or close; JOB-011's close always says why.
- D-244 (matching) PICK-042: after Confirm the homeowner gets the accepted quote by email if on file; the quote page keeps it until the token expires.
- D-247 (matching) PICK-013's code text warns to type it only on the site and that the service never asks for it by phone; PICK-015 shows "or reply <n> YES" by every Choose button.
- D-252, D-284 (matching) PICK-043: HOLD adds 2 business hours once per pick, refused only on emergency jobs; OPS-034 flags businesses with 3+ lapsed picks in 30 days.
- D-264 (matching) FEE-041: a Confirm stopped by the business's monthly limit ends the pick and the homeowner may choose another quote.
- D-270 (matching) PICK-044: an emergency job's Confirm and card-fix windows run 60 minutes clock time at any hour; "Now" is the default call window; cards show "Takes after-hours calls".
- D-286 (matching) PICK-021 and PICK-037 show the contact choice, call window and refund condition; a delivery failure never counts against the business; BIZ-035 per-job call log.
- D-291 (matching) PICK-031: after a text pick, one "When can they call?" question; no reply in 15 minutes means any time 8 to 8 (Now on emergencies).
- D-332 (matching) The quote window stays 24 hours from send; a pick whose every notice email bounced or failed gets 1 more business hour once; BIZ-045 notice log per job.
- D-345 (matching) PICK-019: the non-emergency Confirm window is 4 business hours from the pick email, with reminders 1 hour after and 1 hour before the end; PICK-045 tells the homeowner at the pick.
- D-363 (matching) PICK-017's review step shows the Confirm end time before the pick, warns when it runs past 8 p.m., and may offer a switch to emergency; the window is unchanged.
- D-365 (matching) Confirm and pay are one on-session step with one "Confirm and pay" button; no off-session charge or background retry; details release only after the server sees the payment succeed, once (PICK-022).

## Money

- D-009 (money) Fee basis is the chosen quote's amount: under $500.00 is $25, $500.00 to $1,500.00 inclusive is $75, over $1,500.00 is $199; no AI flag changes the tier.
- D-010, D-159 (money) Recurring quotes convert to monthly first (weekly x 52/12, biweekly x 26/12, quarterly /3, yearly /12, per visit x visits; range midpoint first), then x 4; a recurring job type needs a period (REC-010).
- D-019, D-151, D-152, D-195 (money) Free first win: businesses joining within 90 days of launch or of their category opening; one per entity (ROC number, city-list entity, card fingerprint, record contact, owner name); join date is the form submit date.
- D-033 (money) Compute exactly, then round the fee basis down to the cent before choosing the tier.
- D-034 (money) Trip-fee tiers map small, medium, large to $25, $75, $199; new rows take $25 unless the typical job is clearly $500 to $1,500.
- D-037 (money) A quote with a period uses the 4-month rule; no period is one-time at face value, allowed in a recurring category only for its `one_time_job_types`.
- D-047 (money) A no-show gets no refund; the call line closes, the no-show is recorded and Casey alerted; no automatic pause (TERMS-005).
- D-069 (money) A repeat pick of the same business by the same homeowner costs $0 until the 180-day contact purge; it never uses up a free win.
- D-070 (money) Flag only: a business with 3 of its last 10 quotes within 5% under a tier edge (FEE-033).
- D-071 (money) Every day-3 PROBLEM reply is flagged to Casey; no automatic comparison or fee change.
- D-072 (money) Flat mode defaults each job type to its category's trip-fee tier; any fee-mode change needs a new authorization before the next charge.
- D-073 (money) Cards only at launch.
- D-089 (money) No card at sign-up; a card is given when confirming (FEE-020), now in one on-session step (D-365).
- D-135 (money) Payment timers stop while charges are paused by safe mode or the charging switch and resume with the time left (FEE-038).
- D-141 (money) A business "talked" to the homeowner when any call either way was answered by a person; the unreachable refund needs 2 attempts 3+ hours apart within 72 hours and no such call (FEE-022); the 45-second mark is for reporting only.
- D-142 (money) The unreachable refund closes the job, closes the call line and stops check-ins (JOB-027).
- D-153 (money) A timed-out charge is not retried until Stripe confirms it failed; the nightly reconcile refunds any second charge for one pick (FEE-037).
- D-173, D-205, D-248 (money) BIZ-030: a plain-words box on fees, refunds and card use at sign-up and on the business page ("It's an introduction fee for a homeowner who chose you."); no "didn't book" credit.
- D-196, D-265 (money) FEE-039: a recurring job type's fee is the tier for 4 months, capped at $75 (a setting); one-time jobs in recurring categories are not capped.
- D-197 (money) Fees stay $25 / $75 / $199 (FEE-001); $39 not adopted.
- D-204, D-223 (money) TERMS-022 and BIZ-027 promise replies within 4 calendar days (settings, blocking nothing); FEE-041 is an optional business-set monthly fee limit.
- D-225 (money) FEE-031: every receipt says "Tax included." and links a "Why was I charged?" page for that charge.
- D-229 (money) FEE-027: the dispute evidence pack goes to Stripe automatically 2 days before the due date unless Casey sent it.
- D-236 (money) TXT-030: after a first unanswered business call the homeowner gets one text; the unreachable refund fires only if it (or its email) went out.
- D-243 (money) For email contact, at 72 hours the homeowner is asked whether the business emailed; YES stops the refund and close.
- D-251 (money) OPS-031 flags a trade where over 30% of connected jobs end NO or OTHER for 2 months; any credit is Casey's decision.
- D-287 (money) BIZ-032: a business message about a charge gets its latest charge's details and "Why was I charged?" link; TERMS-022: a charge that was our mistake is refunded in full.
- D-288 (money) BIZ-030 says Casey may add a credit if many picks end without a hire; the FEE-040 lock starts no earlier than activation; BIZ-022 refuses directions to other channels; a non-owner-linked phone needs the owner's YES.

## Privacy and the contact vault

- D-013 (privacy) Retention: contact details 180 days after close (unless dispute), photos 30 days, consent and message logs 5 years (bodies to template ID after 30 days), ledger and audit 7 years, scrubbed job text 3 years.
- D-026 (privacy) The read-back on masked calls happens only from a phone linked to a business with a confirmed, paid pick (or free win) for that job, after a key press.
- D-074 (privacy) The owner's release only completes or re-sends a release for a confirmed paid pick or recorded free win; never otherwise.
- D-077, D-160, D-231 (privacy) Masked calls use a fixed number pool sized by Casey (starting at 10); the app never buys numbers; sharing uses PIN routing, with one pool alert open at a time.
- D-078 (privacy) A blocked caller hears the PIN prompt; the business is told how to link the phone (now by email, D-311).
- D-079 (privacy) At purge name, address, email and photos go; the phone stays encrypted only with its consent and opt-out records, for those uses only.
- D-093 (privacy) The vault holds homeowner message text only; raw text is seen only by `privacy::read_inbound`, which stores, matches emergency and keyword words and returns flags plus scrubbed text; business quotes stay in quotes.
- D-094 (privacy) Photos are stored encrypted in the privacy schema; exports go to object storage behind a `storage` adapter (default Cloudflare R2), separate test and live buckets.
- D-203, D-217 (privacy) TERMS-021: a business data page, English and Spanish: what is kept and how long, no call recordings, data to others only by legal demand, which the business is told of unless forbidden.
- D-214 (privacy) WORD-018: the homeowner's number is never given to a business; PRIV-011 delete wording; TERMS-008 says no AI reads messages while AI-008 holds; TERMS-023 homeowner data page.
- D-215 (privacy) TERMS-024: businesses use homeowner details only for this job; no marketing lists or resale; PROBLEM reports a breach.
- D-296 (privacy) CALL-021: masked calls use standard programmable voice, never a beta or no-SLA masking product.
- D-299 (privacy) TERMS-025: one plain sharing sentence: scrubbed description to businesses; first name and address only to the business that confirms and pays; never the phone number.
- D-306 (privacy) TXT-036: no text to anyone but the homeowner carries homeowner name, address, email, on-site contact or photo link; the release goes by portal and email only.

## Texting and email

- D-003, D-004, D-085 (texting) Non-urgent texts only 8 a.m. to 8 p.m. in both Arizona and area-code time (v9.3 exceptions stay); business hours are 8 a.m. to 8 p.m. Arizona daily; a business may narrow but never widen its window.
- D-005, D-129 (texting) An opt-out stops all texts and changes no job status; sole CANCEL or CANCELAR on a live job also pauses it ("paused by the homeowner") and tells them how to close (CON-028, CON-029).
- D-006 (texting) Natural-language opt-outs are honored; an opt-out word used in another sense is not an opt-out.
- D-008 (texting) The consent text is the first message on every path, except that an emergency's safety text goes first and consent right after.
- D-012, D-158 (texting) At most one quote-count update per job every 3 hours (a setting); held updates merge into one at the next window.
- D-020 (texting) Spanish single-word opt-outs are parar, alto, cancelar, baja, kept in settings.
- D-021 (texting) Consent replies accepted include OK, Yes, Sure and Si (any case); the exact reply is stored.
- D-022, D-289 (texting) HELP gets the help email and help page, says the texting number is text-only and gives no voice number; the next message after HELP is queued for the owner.
- D-051 (texting) Web chat shows the full consent text first; a phone entered there gets the consent text as its first text and nothing else until it replies OK.
- D-052 (texting) The consent text and request page both say "You must be 18 or older".
- D-053 (texting) No text goes to an opted-out number until it texts START; after START it is a new request starting with consent.
- D-054, D-136 (texting) On a homeowner-chosen emergency job, cards, confirm requests, pick, confirm, connect and quote texts may go at night, only to after-hours businesses; other texts keep quiet hours.
- D-065, D-066 (texting) Spanish when the first message reads as Spanish, else English; a sole AYUDA gets the Spanish HELP reply.
- D-088, D-305 (texting) Twilio registration: Customer Care, Low-Volume Standard; Casey first sends Twilio the written eligibility question and waits for the answer (TERMS-014).
- D-100, D-139 (texting) Only US and Canadian area codes in the quiet-hours table are texted or called; every code has a zone; other numbers are refused with a message to use email.
- D-127 (texting) Consent is a reply whose first word is an agree word (CON-005); other replies get the consent text once more; one last re-send at 24 hours within quiet hours.
- D-130 (texting) The provider's auto-reply handles its own opt-out keywords with CON-009 wording; the app confirms only other words, so one confirmation goes (CON-030).
- D-131 (texting) A first message that is only HELP, INFO or AYUDA gets the HELP reply; "help" inside a request gets the consent text; opted-out numbers still get HELP replies.
- D-132 (texting) A business-intent first message gets one text with the business sign-up link and opens no job; the AI second opinion runs only on would-be new requests.
- D-144 (texting) While texting is off, an emergency message raises a critical alert and gets the safety text by email and web chat; every business notice also shows in the portal.
- D-174, D-266, D-293 (texting) TXT-026: a business with a released job and no call attempt is reminded at 24 and 48 hours; calls only inside the homeowner's window; homeowner times say "Arizona time" (TXT-033); TIMES changes the window.
- D-206, D-210, D-275 (texting) TXT-024: one automated safety voice call for a homeowner's own emergency message during a suspension, behind a setting off by default until a TCPA attorney approves in writing.
- D-211 (texting) OPS-028: a critical alert is auto-acknowledged when the safety voice call was answered by a person; otherwise it pushes.
- D-213, D-339 (texting) TXT-027: the who's-calling text names the business, masked number and call window and advises a written price and no full upfront payment; an email-contact variant.
- D-226 (texting) Call-backs for homeowners unreachable by text or email are due within 24 hours (urgent) or 4 days; an opted-out homeowner writing without START is emailed or listed, no call-back.
- D-227 (texting) CALL-020: callers to the texting number hear a recorded greeting; no voicemail, recording or ring-through.
- D-228 (texting) BIZ-032: business replies get an automatic reply with the portal link; questions and charge words also go to Casey.
- D-237 (texting) CON-031: the consent text opens with who we are, "Free for you" and "Our texts are automated"; who-is-this replies get the consent text again; CON-005 adds more agree words.
- D-290, D-338 (texting) TXT-016: the first-quote text gives the LIST option before the quote-page link; PICK-015 accepts "<n> SI".
- D-297 (texting) TXT-021, TXT-034: every step can be done by web chat, quote page, portal and email alone; every journey has a zero-texts test; web chat always available.
- D-298 (texting) CON-032: before Gate C a build-time check confirms the opt-out rules meet the FCC rule, re-checked and logged.
- D-303 (texting) TXT-035 Option A: per-audience channels; homeowners SMS; business texting off by default until Casey files Twilio's written answer and a separate business campaign is registered.
- D-304 (texting) TERMS-011, TERMS-013: the homeowner campaign covers homeowner texts only; a test fails if any business template could be sent from it.
- D-313 (texting) TXT-038: every pick, identity and sign-in code goes only through Twilio Verify, never the outbox.
- D-315 (texting) CON-033: toll-free verification for the homeowner stream only with Casey's go and only after a refused registration.
- D-340 (texting) TXT-032: when the card went to 1 or 2 businesses, the first text warns quotes may be slow or may not come.
- D-344 (texting) No automated call is ever placed to a business phone (BIZ-039); CON-019 asks no call consent; the contact card lists only the masked pool numbers.
- D-366 (texting) No call of any kind to a business: Verify voice for businesses is withdrawn; codes go by text or the person's sign-in email; a homeowner gets a voice code only on request; HELP keeps "replies within 2 days".

## AI

- D-040 (ai) The real AI is tested in Phase 2 step 5 from the Railway test project, fake data only.
- D-080 (ai) Without a signed zero-retention agreement the live app makes no AI call of any kind; quotes use the form and intake uses keyword rules.

## Pages and accessibility

- D-041 (pages) The Stage 0.5 publication puts every public page live as static pages with "requests open soon"; no texting number shown and nothing takes requests before Gate C.
- D-180 (pages) WEB-019: the business page, sign-up and business-terms summary exist in Spanish, marked for native review.
- D-182 (pages) WEB-020: the business page shows a fake-data sample card per open trade, never pest control.
- D-249 (pages) WEB-020 adds the city-license guide to handyman and no-license sample cards, a solo-worker safety tip and a waiting count for coming-later trades.
- D-317 (pages) A11Y-023: action pages and their emails meet A11Y-001 to A11Y-019: link text names action and job, plain-text part, end times not countdowns, each action shows what it will do first.

## Security

- D-014 (security) Businesses sign in by single-use emailed link plus a code on a new device (30-day sessions); the owner by argon2id password plus passkey or TOTP (12 hours max); homeowners by phone code, 30 days per device.
- D-015 (security) The pick-code cooldown is 15 minutes.
- D-050 (security) Without texting the pick code goes by email only to an address the homeowner gave in their own conversation, never one typed on the quote page.
- D-098 (security) The owner registers passkeys or TOTP, at least two in total, plus offline recovery codes.
- D-099 (security) ASVS 5.0.0 at section level; cookies `__Host-`, Secure, HttpOnly, SameSite=Lax; accepted deviations R1, R2 and R11 in the threat model.
- D-125 (security) `scripts/pii_scan.py` fails on real-looking phones, unfaked street names, real names in name fields and vendor-key shapes; findings print path, line and rule, never the match.
- D-318, D-351 (security) SEC-025: business email from a dedicated sending subdomain via the email adapter, SPF and DKIM aligned, DMARC moving to reject before real mail, no tracking; SEC-024 address changes need a verified owner session.

## Operations

- D-016, D-200, D-222, D-280 (ops) Complaints: 3 in 90 days pauses; each shows in the portal with a 5-day reply box; digest-only unless 2+ in 90 days or a safety word, which alerts at once; TXT-029 PROBLEM reply.
- D-024 (ops) Hosting: Railway Pro if it has point-in-time recovery, else Render; no backup person.
- D-075, D-278 (ops) Safe mode starts only when a High alert that pushed is unacknowledged 24 hours after its push; delivery alerts and failed test restores never start it.
- D-087 (ops) The DNS record set keeps any existing google-site-verification record.
- D-133 (ops) Safe mode pauses charges and releases only; texts keep going; each paused item goes ahead after FEE-017's check (OPS-024).
- D-134, D-163 (ops) The delivery alert fires under 90% of the last 50 or 95% of the last 200 texts; one open at a time; recovery and auto-acknowledgement at 97% of 200.
- D-156 (ops) Before every send a replay checks Twilio's sent log for the message's derived key and skips texts already sent (OPS-017).
- D-207 (ops) OPS-026: an owner "away until <date>" setting tells businesses when reviews will be answered; no timer changes.
- D-218 (ops) OPS-029: Casey works a weekly window plus a mid-week check; only four things push to his phone; only critical alerts break Do Not Disturb.
- D-219 (ops) OPS-030: a weekly digest email of activations, auto-cleared alerts, pauses, complaints and the rest, never homeowner contact details.
- D-220 (ops) OPS-023: Stripe, webhook, health and ledger alerts clear and acknowledge automatically when the condition ends, logged in the digest.
- D-221 (ops) OPS-009: a monthly spot check of 10 random cards; flags shown in the monthly review; LIC-023 blocks alone.
- D-232 (ops) OPS-013: the test-restore result is in the digest; a failed restore raises a High alert.
- D-277 (ops) OPS-032: at 9 a.m. and 3 p.m. one push lists open urgent call-backs, urgent HELP, safety complaints, Twilio restored and long list failures; never breaks Do Not Disturb.
- D-279 (ops) OPS-033: the weekly window and mid-week check are at most 4 days apart; the host restarts the app on a failed health check.
- D-314 (ops) OPS-035: alerts to Casey go as email to his alert address, never a text; a critical safety alert also places one automated call to him.
- D-348 (ops) OPS-036: each help request sends one email notification to the alert address at once, with a link to an owner-portal page to fix and reply.

## Code and architecture

- D-036 (code) `scripts/check_catalog.py` restates the fee rules, recomputes the worked examples and scans the catalog for superseded values and banned words.
- D-090 (code) A shared kernel `core` owns only the task queue, timers and rate limits; everything else stays in the business modules.
- D-091 (code) One Postgres schema per module; modules use only lower layers' public API; cross-module signals are task events carrying IDs only.
- D-092 (code) Two database roles: a migration owner and an app role with no UPDATE or DELETE on append-only tables.
- D-096 (code) Pins: newest stable release 14 days before K4 unless an audit finding forces later.
- D-097 (code) The new-library entries of the Python plan (purpose and license each); see the log.

## Testing

- D-101 (testing) TD entries per behavior, boundary, failure or invariant, covering every requirement ID; time windows include the start and exclude the end; "within N" means less than N.
- D-106 (testing) The homeowner's own answer decides; the Twin gate waives such an instance only with per-instance evidence under a per-scenario ceiling.
- D-107 (testing) `twin/gate.py` runs every `_fixed` scenario with seeds 1 to 5 and fails on any check not waived in `twin/waivers.json`; v9.1 scenarios are a negative control only.
- D-108 (testing) Mutation testing: named mutants in `tests/mutation/mutants.toml` on every PR touching their file, plus nightly runs over the rules modules.
- D-109 (testing) One fake-data generator: 555-01xx phones in table area codes, example.com emails, fake streets and names, sentinel values; the PII scanner allows its patterns, never the path.
- D-110 (testing) `docs/catalog/matching_examples.yaml` is written and checked before routing code; app matching tests and the Twin's license table check against it.
- D-113 (testing) Named mutants and coverage floors come from one harness task per stage with mutation targets, Opus reviewed.
- D-124 (testing) Vendor stand-ins are in-process fakes from each vendor's public API (Twilio, Stripe and others) returning the vendor's envelope.
- D-162 (testing) Spec lines change first, then tests (TD-CON-05, TD-OPS-01, J3 and the new entries the log lists).
- D-167, D-168 (testing) Sessions have no Docker, so stripe-mock runs only in CI; Playwright uses the pre-installed Chromium; Ally runs in GitHub Actions.

## Runner and governance

- D-001 (runner) Requirements are one testable sentence each with a stable area-prefixed ID and `path:LINE` citations, checked by `scripts/trace_spec.py`.
- D-002 (runner) K2 was written in one session, with the traceability script as the check.
- D-038 (runner) Conflicts are decided and logged, not asked of Casey.
- D-039 (runner) Required checks use the brief's names and mechanism: Ci, Twin, Ally.
- D-067 (runner) Spanish native review is a Gate C account-checklist item, not a build stop.
- D-111 (runner) Every task in `kit/tasks.json` carries the full field set; the validator fails on malformed tasks, cycles and any uncovered requirement or TD entry.
- D-112 (runner) Stage 2 builds list imports, business records, portal security and the quote page; stage 4 keeps sign-up, portals and calls; the twin links to app rules at the end of stage 3.
- D-114 (runner) Each vendor-sandbox task and the ZAP task is a harness task with an Opus review.
- D-115 (runner) Every task whose review areas include money, privacy, security or recovery runs on Opus with an Opus reviewer; the validator fails otherwise.
- D-116 (runner) Owner gates are `gate` tasks: account checkpoint, Stage 0.5 go, conditional real texts, finished, Gate B, Gate C, first live charge.
- D-117 (runner) Coverage is checked against TD entries and journeys J1 to J4; task IDs have three digits.
- D-118 (runner) The decision, architecture and review files are shared paths outside the ownership check; the kit's own tests are protected.
- D-119 (runner) Task limits: 14 owned paths, 14 files and 800 changed lines, 25 requirement IDs, 6 test entries, context at most 20%.
- D-120 (runner) After Gate P shifted the brief, every later `BUILD_BRIEF.md` citation in `kit/tasks.json` moved by one line and prompts were regenerated.
- D-121 (runner) The runner is pure Python over the real `kit/state.json` shape; inbox answers are recorded and pushed before moving to done.
- D-122, D-170 (runner) The model check reads `message.model` from the worker transcript; the worker call takes short names and the transcript's full ID decides.
- D-123 (runner) A gate clears only on the quoted phrase in its resume text, else a whole-word go, yes, approved or done.
- D-126 (runner) A waiting owner gate holds only its dependents; Gate P holds every task.
- D-161 (runner) TERMS-017 is unused and never reused.
- D-166 (runner) The runner runs build prompts itself; practice tasks P1 to P3 proved pass, fail and resume paths.
- D-169 (runner) The hourly routine carries the chain; about one task an hour until a merge trigger is confirmed.
- D-171 (runner) One scheduler routine; a live lease naming another session means stop.
- D-302, D-322, D-343, D-355 (runner) Legal, tax, insurance, Twilio, TCPA, email-records and verification items go to Casey in `docs/for-casey-review.md`.
- D-320 (runner) T147 and T148 added to stage 4 for business notices and portal action links.
- D-341 (runner) T149 and T150 added to stage 4 for email reliability and emergency emails and link hardening.
- D-353 (runner) T148 and T150 retitled after business calls were removed; T151 added for help requests.

## Superseded

- D-025 -> D-139
- D-048 -> D-141
- D-060 -> D-145
- D-062 -> D-311
- D-076 -> D-218
- D-148 -> D-365
- D-178 -> D-344
- D-183 -> D-349
- D-187 -> D-312
- D-192 -> D-345
- D-224 -> D-349
- D-295 -> D-358
- D-310 -> D-344
- D-316 -> D-344
- D-319 -> D-344
- D-323 -> D-344
- D-327 -> D-344
- D-334 -> D-344
- D-335 -> D-344
- D-337 -> D-349

## Task-scoped
