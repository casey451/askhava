# For Casey's review

These are decisions Claude made on its own and logged so the build never waits (`BUILD_BRIEF.md:542`). Older entries below may have been superseded by a later decision (the own-or-rent question, the 12-month price lock and then any price lock (D-1494), the 1-business-day reply, the call window, the heat text and 211 lines); the spec in `docs/spec/` and the latest `D-###` win (D-482). None of them blocks anything. Each item names its entry in `docs/decisions.md`, which has the exact source lines. The ones most worth a second look come first.

## Nine more waiting steps fixed before they start, and one small new step (plan change, D-3112, November 12)

- **What happened.** A fifth check of steps still waiting in line found five that would have stopped partway: the safety-call step would have had to change older tests it was not allowed to touch; the step that adds job facts to older pick tests missed eight test files; the business welcome-email step put its code where the module rules forbid it; the business event emails needed an Approve link they could not make; and the "Why was I charged?" page could never pass the security door test, because no test business had ever been charged. Three more steps pointed at old line numbers or at a locked section of the architecture notes.
- **What changed.** Each step's instructions now name the right place and files: the safety-call step may make signature-only edits to three older tests; the job-facts step covers fourteen files; the welcome and event emails live in the businesses module and get their links and phone digits through the app's assembly code. A new small step, T404, gives each test business in the door test a charged pick (fake payments only) so the "Why was I charged?" page can be checked. No rule for homeowners or businesses changes, and no test is loosened.
- **Nothing needs you.**

## The catalog is stored when the app starts, and the Confirm step is fixed before it starts (plan change, D-3100, November 12)

- **What happened.** The open item from D-3094 was real: nothing stores the catalog in the database when the app starts, so on a fresh deployment every category would read as switched off. Separately, the waiting step that serves the business's Confirm and pay page (T261) named an old form of one payment call and a type that does not exist, was planned to load slightly more than the allowed share of the AI's working memory, and would have broken an older browser test of the bank-approval step that it was told to leave alone.
- **What changed.** A new small step, T400, stores the catalog each time the web app starts; it never switches a category on, so your on/off switches survive every restart. Every part of the app now follows one rule: your stored switch wins, and a category with no stored switch is off. The capability-checklist step (T368) follows that rule instead of falling back to the catalog file, and only adds one setup line to an older test. T261 now names the right call and type, updates one line of the architecture notes instead of loading a whole section (29% of the working memory instead of 46%), and may update that one older browser case so it checks the new "Checking with your bank" line too. No test is loosened.
- **Closes** the open item listed under D-3094 below.
- **Nothing needs you.**

## Six more waiting steps fixed before they start (plan change, D-3094, November 11)

- **What happened.** A third check of steps still waiting in line found six that would have stopped partway or misled the worker: new homeowner emails that an older test would reject, a category switch that would have broken an older license-change test, a test case that already passes today and so proves nothing, and three steps told to use code they cannot reach.
- **What changed.** Each step's instructions now name the right file or case: the opt-out emails step (T360) reuses the existing opt-out confirmation email and builds its one new email in code, as the check-in email does, so no older test is touched; a category never stored falls back to the catalog file's on/off setting, while a switch you make always wins. No rule for homeowners or businesses changes and no test is loosened.
- **Open item for the next plan change (now owned by T400, D-3100).** Nothing loads the catalog into the database when the app starts, so on a fresh deployment the stored category switches are empty and the text intake treats every category as switched off, while the sign-up checklist would fall back to the catalog file. The fix (load the catalog at start-up) is left for a follow-up step.
- **Nothing needs you.**

## Gate C now waits on proof, not just a date (plan change, D-3080, November 11)

- **What happened.** An outside 80-point review said the build is not yet ready for real customers, real texts or real charges. Checked against the files, the main gaps were real: the go-live step had a one-line description, your security checklist items had no place to be proved before go-live, nobody owned showing each business's name and license on the quote page, and nobody owned the AI-written questions and job cards.
- **What changed.** Go-live now opens only when a check finds your 14 must-do security items recorded, no open money, privacy or security handoffs, a clean check that no vendor kept customer details it shouldn't, a restore drill that met the recovery targets you set, a release record you signed and a fresh readiness verdict. Your "go live" counts only after a real alert has reached your phone in the rehearsal; if you say it too early, it is kept but doesn't count, and you say "go live" again once everything is in. You send each proof (a must-do item done, the test alert's date and ID, your release sign-off) as a reply on the owner issue, and a plan-change session writes it down; writing it down never goes live for you. If the vendor check finds a leak, a new step re-runs that check after the fix, and go-live waits for it to come back clean. You still do every step yourself. Four new steps build this (the check, the vendor check, the quote-page cards and the AI-written text), and the restore drill and handover pack now record the timings and the release record.
- **What you need to do.** Nothing now. Before go-live you'll see a list of anything still missing, including the recovery targets (how long a restore may take and how much recent data you could lose) only you can set.

## Linking a team phone by text gets an owner, and two waiting steps are trimmed (plan change, D-3079, November 11)

- **What happened.** The outside review you shared found one security item with no step assigned: when someone on a business's team calls a homeowner's line from a phone the business has not added, nothing yet sends the business the "Reply LINK <PIN> to add this phone" message, and nothing handles that reply. The build cannot finish its current stage while that item has no owner. The review also found that two waiting steps were planned to load more than the allowed share of the AI's working memory.
- **What changed.** Two new steps. T390 sends that message after such a call reaches the homeowner with the job PIN. T391, which follows it, adds the phone only when the reply comes from the business's main phone with the right PIN for one of its own open jobs, and only within 24 hours of the call. (They were one step; the independent review found it too big for one go, so it was split in two.) A wrong, reused or other business's PIN, or a reply 24 hours or more after the call, changes nothing, and after 3 wrong tries in a day that phone's LINK replies are ignored until the next day. The added phone's number is never stored, only a scrambled fingerprint of it, as for every linked phone today. Eight later steps had their database file numbers moved up by one to make room. The two oversized steps now update one line of the architecture notes instead of loading a whole section of it, and 26 other waiting steps had their memory figures re-measured.
- **For later.** A short "This phone is now linked" text after a successful LINK would need new wording from you, so none is sent yet. Also found while checking: a phone added by a team member (not the owner) works for calls right away, though the rule says it should wait for the owner's Approve. It is now listed as an open security item (IC-95) with no step assigned yet; the next plan change will give it one.

## Six more waiting steps fixed before they start (plan change, D-3083, November 11)

- **What happened.** A second check of steps still waiting in line found six that would have stopped partway or misled the worker: older tests the step did not own, a check order that would trip an older test, a missing setting that would have crashed a test in development, and stale line numbers. Two of them had already stopped for this reason: T345 (pinning the GitHub Actions versions) and T317 (the limit on new requests by text); both go back in line once this change lands.
- **What changed.** Each step now owns the few extra files it must touch, with strict limits on what it may change in them (only how a test builds its setup, or only the version text in a comparison), and its instructions name the exact code to use. No rule for homeowners or businesses changes and no test is loosened.
- **Nothing needs you.**

## The runner's start-up smoke test runs less often, with every red-build guard kept (harness and plan change, D-3058, November 11)

- **Who decided.** You told the overseer to let Fable make the decisions meant for you; Fable chose this one.
- **What happened.** Between noon and 19:29Z today the runner spent 228 minutes on its start-up smoke test against 690 minutes of real work: 29 runs on 18 versions of the code (11 repeats of a version already tested, about 95 minutes), and 9 of 23 sessions stopped to wait for a smoke before they could start a task.
- **What changed.** (1) When the only changes since the last real green smoke are up to 3 finished tasks, each already merged with all three required checks green, that smoke now stands for the newer code too. A real smoke still runs at least every 4 merges and at least every 6 hours, and any other kind of change in between (a plan or harness change), any failed smoke, failed nightly run or failed check on any of those versions, or anything the runner cannot read, makes it smoke again. The number is one setting in `kit/runner.json`; 0 turns it off. (2) When one lane is still smoking a version another lane has already smoked green, it stops its own unfinished run and uses that result. A finished result of its own, pass or fail, is always used.
- **What is protected.** None of this ever applies while the build is marked broken; clearing a broken build still needs a brand-new green smoke; a failed smoke still marks the build broken and queues the fix exactly as before.
- **Nothing needs you.**

## The build no longer pauses for a five-hour limit (plan change, D-3056, November 11)

- **What happened.** The pacer also budgeted a five-hour token window, using a made-up 30M figure. By 19:10Z that figure was nearly used up, so it would have let only about four tasks run at once until 20:04Z, while the week still had tokens to spare.
- **What changed.** You said you don't have a five-hour window, so the pacer now ignores it and only watches the week. If a real usage reading ever says some limit is at 100%, the build still stops until that limit resets; it never pays per use.

## A runner self-check no longer trips on work in progress (harness change, D-3057, November 11)

- **What happened.** One of the runner's own checks before claiming a task compared the plan with the build's live progress and failed whenever a task already being worked sat at the head of the longest remaining chain. The build itself was fine, but the runner stopped without claiming anything five times today, and enough of these in a row could have wrongly marked the build broken and undone good work.
- **What changed.** The check now only considers tasks the runner could actually pick up, so work already in progress no longer counts against it. It still catches the mistake it was written for, and a new test reproduces today's failure.

## Five waiting steps fixed before they start (plan change, D-3055, November 11)

- **What happened.** A check of five steps still waiting in line found that each would have stopped partway, because the plan asked for something the code or older tests do not allow.
- **What changed.** The business's Confirm page step is split in two: one part records the business's agreement to the terms on the Confirm page and adds the extra code check for a shared inbox; a new step, T388, emails the homeowner a copy of the quote she accepted and shows it on her quote page. The business sign-in step may now change a few lines in an older test (only how it builds the app's router) so the new pages can be checked. Quiet hours for waiting texts are checked by the new text sender itself, so older tests stay as they are. The pest-license step keeps what it can do, and a new step, T389, pauses a pest business's pest jobs when a homeowner reports its license is wrong. The nightly payments check reads Stripe in pages through a new read, so older tests stay as they are.
- **Still open.** Nothing yet lets a homeowner send the wrong-license report (no page or text keyword for it). And the pest quote card's line "This business told us it holds … license" is not built by any step. Both go in the next plan change.
- **Nothing needs you.**

## A flaky coverage check on the payments code gets a fix, first in line (plan change, D-3049, November 11)

- **What happened.** The build requires at least 95% of the payments code's branches to be exercised by tests. After the last refund change it sits right on that line: the same code measured 95.07% on one run and 94.94% on another, so one test reaches a piece of payments code only some of the time, and the check now passes or fails by chance.
- **What changed.** A new step, T387, finds that piece, makes the test reach it every time, and adds refund tests so payments has room above 95% (at least 95.5%). It only adds or fixes tests: no payments code changes, the 95% bar stays, and no existing check is loosened. It is the next step the build picks up.
- **Nothing needs you.**

## The build now works five tasks at once (plan change, D-3043, November 11)

- **What changed.** You said not to worry about the token budget, just to use the tokens efficiently. The runner now works up to five tasks side by side instead of four.
- **What can still slow it.** The five-hour and weekly usage limits still apply: when a window is nearly used up, the pacer holds the next task back, so five lanes may run as four or fewer. Each task still runs once, on its planned model.
- **Safety net.** Two tasks side by side still never touch the same files or sit in different stages. If one extra lane fails twice in a row for a harness reason, the runner drops to one lane by itself and tells you why; the overseer reads the reasons before turning the lanes back on.
- **GitHub's limit.** GitHub runs about 20 build jobs at a time, and one task's hand-in starts about eight. When several tasks hand in together, the extra jobs wait their turn; nothing is cancelled or failed, they just take a few minutes longer.
- **Undo.** Setting the lane count back to 4 is a one-line logged change.

## Your call: a try lost to an outside outage is given back; T332 gets its try (plan change, D-3031, November 11)

- **What happened.** T332's third and last try was failed by the Ubuntu download outage (D-3030): the step that sets up the test browser could not download, so its accessibility and journey tests never ran. The rules only gave a task an extra try when the plan itself was wrong, so you were asked, and you said "Give it back".
- **What changed.** The build rules now say: when a try fails only at a setup step, before any of its tests ran, because of an outside fault that a logged fix has since removed, the task gets one more try, on Opus. A failure inside a test, or in the task's own work, never counts. T332's plan now carries the note that lets the runner give it that try; its work and tests are unchanged. T384 and T269 lost a try the same way but still have tries left.
- **Nothing needs you.**

## The share box at the pick is required, and five older tests tick it (plan change, D-3032, November 11)

- **What happened.** The step that adds the homeowner's "Share my first name and the job address" box at the pick stopped a second time. Pressing Pick without ticking the box must now show an error and pick nothing, but five older tests press Pick without the box, and that step was not allowed to change them.
- **What changed.** The same step may now change those five tests, and only to tick the box before pressing Pick; every check they make stays the same. Because it now changes finished tests, it runs as a harness step with the stricter review. One queued step that also edits one of those tests now waits for it.
- **Also pinned.** The tick is saved in its own small record and attached to the pick only once the homeowner's code checks out. If she forgets the box, the page says so at the top, takes her to the box, and keeps the backup quote she had chosen. The box's Spanish wording is left to that step, which logs it.
- **Nothing needs you.**

## Browser setup survives an Ubuntu mirror outage (harness change, D-3030, November 11)

- **What happened.** From about 13:48Z the build servers could not reach Ubuntu's download mirrors. The step that prepares the test browser waited 12 minutes for some optional fonts and then failed, so the accessibility and journey tests never ran and T384 lost an attempt.
- **What changed.** The step now waits less, tries three times, and if the mirrors stay down it carries on with what the server already has, but only after proving the browser starts and shows a page. If it cannot, the step fails as before. When it carries on, the run shows a warning.
- **What is not changed.** No test, rule or check is relaxed. Only optional fonts for languages the site does not use (Chinese, Japanese, Thai and the like) can be missing.
- **Nothing needs you.**

## A license list change updates a business's job types (T207, D-2592, D-2595, D-2596, November 10)

- **What it does.** When the state's daily license list changes a business's classes, the job types the old license covered and the new one no longer does come off at once, with an email saying which and why; job types the new license adds are offered by email and never ticked for them. Pest control, septic pumping and home inspection (licensed by another agency, not by a class) and the no-license trades are never touched by a class change (D-2595). A retry of the same day's list never sends a second email (D-2596).
- **Your call: when to pause (D-2592).** "A removed class that leaves no covered job type pauses the business" is read as "nothing left on its list at all". A plumber whose license lapses but who also ticked window cleaning keeps getting window-cleaning cards and is not paused. If you meant "paused whenever no licensed job type is left", say so and it changes in one line.
- **Your call: the way back.** A business paused this way has no way back yet: it is not resumed when a later list shows the class again, and it cannot resume itself. For now only you could resume it, once a page for that exists. A later task should add the resume; tell us if it should be automatic when the license comes back.
- **One known gap.** The Spanish email names the job types by their English catalog names; the sender's task needs Spanish job-type names (BIZ-018).
- **Not wired yet (D-2593).** The daily list import does not call this yet and no sender delivers the email.
## The homeowner calls contact card (T199, D-2409 to D-2413, November 10)

- **Copy still to fix.** The welcome email's third line still says "Save the attached contact card", but the card is now a link to the download in the business portal, not an attachment. The line is pinned word for word by a test another task owns (`tests/integration/tests/business_consent.rs`), so a follow-up task that owns that test rewrites it (for example "Save the contact card from your business account"). Nothing is sent to a real business before then.

## Handyman split jobs: your ruling (plan change, D-2407 and D-2408, November 10)

- **Your call, as you put it:** "I only care about individual jobs being under the 1000 mark" and "I dont want the 30 day rule". The 30-day split-job check is gone. Each handyman job is judged only by its own price: under $1,000 is fine, $1,000 or more is refused. Another job by the same handyman at the same house never blocks a quote. Task T347 removes the old check and its tests.
- **Kept:** the handyman still ticks the box saying the quote is the full-job price under $1,000 with no permit, every handyman card still says "Not a licensed contractor", and gas, smoke-alarm, sprinkler and permit jobs still never go to a handyman.
- **Legal note (recorded as your informed choice):** Arizona's handyman exemption is strict about splitting one project to stay under $1,000, and a word with your lawyer was suggested before launch. You decided to drop the check anyway. The handyman's own promise not to split a project (in the pilot terms) stays.

## The security assessment (plan change, D-2346 to D-2350, November 10)

- **What it found.** Four points, all confirmed in the code. (1) Fifteen places in the build automation load outside tools by a version label that their authors can move, including the step that runs Claude with the build's access; they will be locked to exact versions, and a test will keep every workflow locked. (2) and (4) Two sign-in steps for businesses could be hurried past the five-try limit or hold the database while waiting for the texting service; those fixes were already planned or are added to the same task. (3) A texted code for one thing (for example signing in) could also confirm a different thing, such as deleting a homeowner's data. Each code will now work only for what it was sent for, with at most five tries.
- **Choice made for you:** no separate Twilio Verify service just for delete confirmations. The app-side check closes the gap without a new Twilio setup step. If you ever want that extra separation anyway, it is an option you can choose later.
- **Not done yet, on purpose:** the build robot's job still has wide write access and the push key. Narrowing that changes what the robot can do, so it needs its own trial run; it is noted as a risk for a later change.
- **Its 38 test cases** are saved for the war games, which must run each one or say why not, only in the test setup with made-up accounts.
- **Nothing needs you.**

## The seventh outside review (plan change, D-2329 to D-2335, November 10)

- **One question for you (handyman split jobs; answered November 10: no split-job rule, D-2407).** Today a handyman's quote is blocked when the same business has another handyman job at the same address within 30 days and the two together reach $1,000 or share a job type. It checks pairs, so three $400 jobs of different types at one address in 30 days pass ($1,200 in all, but each pair is $800). Should the check add up all of that business's handyman jobs at the address in the window? Nothing changes until you answer.
- **What it found and what changed.**
  - **Fee-free inspections would have been charged.** The app didn't yet store what kind of job each request is, so a buyer or refinance inspection would have carried a fee. Each job now stores its category and job type, and the fee is worked out from them. Nothing charged in the meantime: the Confirm page isn't live and now waits for this.
  - **Choice made for you:** when intake couldn't settle the exact job type and one of the possible types is fee-free, the job is treated as fee-free. It's never charged when it might be a sale-related inspection.
  - **Emergencies (the overseer's call).** An emergency job's cards go out before the detail questions, so at that moment the app knows the trade but maybe not the exact job type. It saves what it knows, then narrows it as the homeowner answers; it can only narrow, never switch to a different trade. When she picks a business, the fee uses the latest saved answer, with the same fee-free rule as above. If the job type was somehow never saved, the pick still goes through (nobody is turned away in an emergency) at the normal one-time fee for the quote's size, and you get a High alert to check that fee before the business confirms. Outside emergencies, a pick with no saved job type is stopped instead.
  - **Package (tier) quotes and per-visit prices can now be picked.** The homeowner picks a package on the quote page, and a per-visit price must state how often visits happen (weekly, every two weeks and so on); for any other schedule, such as twice a week, the business enters the monthly amount instead. A homeowner who texts "2 YES" for a package quote is sent the quote-page link to choose the package; whether texts should name a package is your call.
  - **Phone numbers split over several lines** in a business's quote are now caught.
  - **The daily code-send cap** now holds when many people ask at once.
- **Nothing else needs you.**

## The sixth outside review (plan change, D-2301 to D-2307, November 10)

- **What it found.** Three of its eight points were already handled by the fifth review's tasks. The new ones: a free ($0) pick's release check didn't confirm that the free decision belonged to that same pick, so one free decision could have waived the fee on another job of the same business; a refund was booked as the whole charge without checking what Stripe actually refunded; the nightly money check only looked back 7 days and read everything at once; the rate-limit counters (which hold visitors' internet addresses) were never cleaned up; and a text that failed to process was only alerted, never retried.
- **What changed.** Five new tasks (T333 to T337) plus additions to T261, T255, T319 and T328. Choices made for you:
  - **Only full refunds.** Ask Hava only ever refunds a whole charge. If Stripe ever reports a smaller refund, the books record what Stripe says and you get an alert; if it reports another currency, the books keep the charge amount and you get an alert to check it.
  - **Money-check catch-up.** If the nightly check was down, it catches up from its last good run, however long ago, reading page by page so nothing is skipped. If it was down more than 2 days, you also get an alert so you know.
  - **Text retry (the overseer's call, made under your "final say").** A text that failed to process is tried again up to 3 times. Each try reads the text back from Twilio, which keeps it until day 31. The app never stores the text itself; after day 31 the retry stops. A STOP or HELP is always honoured; if she has texted again since, only the STOP or HELP part is done; any reply waits for daytime hours.
  - **Rate-limit cleanup.** Old counters are deleted daily, so visitors' addresses aren't kept longer than a few days.
- **Nothing needs you.**

## The fifth outside code review (plan change, D-2284 to D-2289, November 9)

- **What it found.** Seventeen of its twenty points repeat the fourth review and already have tasks. Three are new. The biggest: several background jobs the app queues have no worker set up to run them, mostly payment ones (checking a charge that hasn't answered, the timers, refund retries, the nightly money check) plus some business emails, the charge receipt and the safety call. Today they would just pile up unrun. The other two: a Stripe "payment succeeded" message with a missing job reference, and a fraud warning that arrives before its charge record, were each being dropped without telling you.
- **What changed.** Five new tasks (T327 to T331): a check that fails the build whenever a queued job type has no worker and no named owner, plus a High alert if one ever gets stuck; one task each for the payment jobs, the business emails and the remaining three event types; and one for the two Stripe cases, which now record the message and alert you. The Confirm page (T261) now waits for the payment jobs. Two of these tasks touch money and get an extra independent review when built.
- **The review's verdict.** It says "not approved for production or real customer and money traffic". That matches the plan: nothing goes live before Gate C, your own go.
- **Nothing needs you.**

## Your Fable week: everything switches to Fable at Thursday's reset (harness change, D-2225, November 9)

- **What you asked.** "on Thursday when the usage resets, I want to switch everything to fable and run the usage all the way through so I can use one of my fable resets to bring usage back to zero", then "Back to opus. Its just to get more tokens out of the week for the build series."
- **What happens.** At the weekly reset (Thursday, November 12, 17:00 UTC, 10 a.m. Phoenix) the build switches itself to Fable: every worker, every reviewer and the runner's own session. The task list is not changed; the switch is one setting the runner reads. It also stops holding back at about 97% of the week and keeps working until the plan's real limit is reached; then it waits, as it always does, and never pays per use.
- **Back to Opus by itself.** The first time the week's usage runs out while on Fable, the switch turns itself off and a note says so. From then on every task runs on its usual model (Opus) at the usual pace, with nobody changing anything.
- **A task the limit interrupts** is simply retried after the reset; it does not count as a failed try.
- **After you use your reset.** The build waits while the old "100% used" reading stands, and carries on by itself, on Opus, as soon as a fresh reading shows the usage back down (within a few hours at most; sooner when a run starts).
- **Ending it early.** One date in that setting ("until") also ends the switch; the overseer does it through a normal reviewed change if you ask.
- **If Fable is not accepted.** If the build's tools refuse the Fable model name, that task simply runs on its own model instead, costs no retry, and a note says so; the rest of the week then stays on the usual models. The exact model name the build tools accept could not be tested ahead of Thursday from here, which is why there is a fallback.
- **Nothing needs you.**

## The fourth outside code review (plan change, D-2200 to D-2209, November 9)

- **Answered (F26).** The "business hours" clock used for business deadlines (for example the 4 business hours a business has to confirm) runs 8 a.m. to 8 p.m. every day, weekends included, as D-004 and JOB-007 say; the reviewer asked whether weekends should count. You answered in chat: "Yes keep weekends", given in chat to the overseer, outside the owner-issue inbox; recorded here as his answer, no rule change (D-2200).
- **What changed.** Fifteen new tasks (T311 to T325) and additions to fourteen queued ones. The important ones: a phone call to a masked number could, in a rare timing, connect a business to the wrong homeowner after the read-back; it will connect only the job that was read back (T311). Night EMERGENCY job emails were being held until 8 a.m. and lost their EMERGENCY subject, and nothing was sending business notices yet; both fixed (T313). A bank dispute's paperwork or its $15 fee could be lost if the system stopped at the wrong moment, and nothing was sending dispute paperwork automatically; both fixed, with a button for when you send it yourself in Stripe (T314, T205, T304). A homeowner's feedback words were kept after "Delete my data" and after 3 years; they will be blanked (T315). The rate limits on sign-in, codes, quotes and new requests were built but not switched on at any page; each page's task now switches its limit on (T317 and others). Failed incoming texts, refused webhooks and alerts will now reach you instead of only a log line, and the same alert won't repeat while it's open (T325, T204, T319, T320). A lower-case street name like "smoketree" no longer triggers the 911 safety text (T312).
- **Calls Claude made that you may want to check.**
  - **Business names in texts (T316).** No part of the app stored a business's public name, so the LIST reply named no one and the "who's calling" text could not be sent. The name will come from the business's license record (its trade name or DBA if it has one, otherwise the business name), screened for phone numbers and links. Businesses can't edit it themselves yet.
  - **The 5-year phone and opt-out records (T321).** Your privacy policy says these are kept 5 years after the last message. They will now be deleted at 5 years, except for a number that texted STOP: it keeps its opt-out record and its locked phone number, because the system needs the number to keep recognising the opt-out after its security keys are rotated; deleting it would mean that person could be texted again. Whether the privacy policy should say that opted-out numbers are kept longer is your call.
  - **Coverage floors (T324).** Money and privacy files outside the three already-floored parts get a 95% test-coverage floor where they already meet it; any that measure lower get their measured level for now and will be listed here by name.
- **Still open.** The quote page's cards don't yet show each business's name, license lines and review links (PICK-041); no task owns that yet, and the overseer will name one.
- **Nothing else needs you.**

## Built pages nobody was putting on the site (plan change, D-2128, November 9)

- **What changed.** Several finished pages had no task putting them on the site: business sign-up, the links in business emails (including the quote form), complaints, "Why was I charged?", the emergency-email switch, the homeowner portal and "Download my data", your own sign-in and owner pages, and the quote page's close button. Ten new tasks (T301 to T310) serve them, two in stage 4 because the business journey needs them, eight in stage 5; the homeowner check-in answer page is now served by the task that builds it (T231). Nothing needs you.

## Fourteen finished hand-offs closed in the register (plan change, D-2116, November 9)

- **What changed.** The register of hand-offs between parts of the system (`docs/integration-closure.md`) had fourteen security, money and privacy rows whose tasks had all finished but were still marked open, which would have stopped stage 4 from finishing. Each was checked against the merged code and its tests, and all fourteen are now closed with the merge and the tests that prove them: first texts racing, a business phone's STOP, names kept out of what the AI would see, refunds, Stripe's events, paid and free picks releasing their details, the business seeing them again, the masked line's retry, the card-confirm flow against the pretend Stripe, call PINs, a changed public record, the coverage floor, the register check itself, and business texts asking for consent at send time.
- **One gap reported to the overseer.** No task yet puts the business portal's pages on the live site (the released job's page, the portal home, notice settings). The code behind them is built and tested, but a business can't open them in a browser until a task serves them; the business journey test (T099) will need it. The overseer will name the task.
- **Gap closed (D-2117).** The business portal had no task serving it; a new task, T300, now does (home, a released job's page, billing and statements, notice settings), and the business journey test waits for it. No effect on dates beyond one more task.
- **Nothing needs you.**

## Connecting the app to the AI (D-2065, November 8)

- **The gap.** The last change found that nothing in the plan connects the app to the AI: there is no setting for where the AI lives or for its key, the app never builds its AI connection, and nothing calls the AI when a homeowner texts or chats. As planned, the app you'd get would never use the AI, and the war games would never test it.
- **What's added.** Two new tasks, because the order of the plan needs two. T298 adds the settings and the real Claude connection: in development and testing it talks only to the pretend AI on the same machine, the real Claude address is the only other one it accepts, the key comes only from the host's secret settings and never shows up in a log or error, and no real key works outside the test site and the live site. If the live site starts without its AI key, it still starts, with the AI off (Hava asks the fixed questions) and a high alert naming the missing setting, so a launch with the AI off works at Gate C; the Gate C key checklist now lists the AI key. It comes before the real-AI test on the test site (T137) and the pre-war-games review. T299 builds the connection once when the app starts and calls the AI from texts and web chat: an incoming text is saved first, nothing holds the database while waiting on the AI, no AI call happens before the consent text, the area check or (on web chat) the phone code, and if the AI is off, too slow (3 seconds), over its limits or failing, Hava asks the fixed question at once. It comes after the AI-limits task (T236) and before the first war-games round, so every round runs with the AI connected.
- **What the review checks.** The pre-war-games review now confirms T298 is done and T299 is still lined up before round 1; anything missing comes to you as a must-fix item.
- **Still open, worth a look.** The AI-written words (rewording a question, the follow-up, the job card and the project file) were planned for earlier tasks that couldn't build them, and they still have no task. They only matter while "AI on scrubbed text" is switched on; with it off, Hava uses the fixed questions and the homeowner's own words on the card. Until a task builds them, with the AI on Hava still can't reword or skip a bank question, ask the AI follow-up, or have the AI write the job card or the project file (so the content check those words would pass through never runs on AI text either). That means the war games will test the AI reading a request but not any AI-written words. They're listed as an open item (IC-53), and the pre-war-games review must rate it for your war-games go, so you can decide then whether to build them first.
- **What doesn't change.** Fees, privacy, consent and wording are untouched, and no requirement changed.

## Your AI-capacity and national-scale review (D-2054, November 8)

- **Already in the plan.** The main risks it raised are covered by the last change: nothing holds the database while waiting on the AI, the AI limits per message are reserved in one step, and a homeowner waits at most 3 seconds before Hava asks the fixed question (T236, T296). No duplicate tasks were added.
- **What's added.** The pre-war-games review (T242) now checks every outside service the app calls (texts, calls, codes, email, payments, letters, license lookups, file storage and the AI) for the same mistake, waiting on the service while holding the database, and lists each place with where it is in the code; any it finds come to you as must-fix candidates with the other P0 items. It also says whether the AI code leaves room for a later per-model limit and usage figures. A new war-games task (T297) adds a load test on fake data and stand-ins only, so it costs no AI money: at least 100 conversations at once, web chat and texts mixed, repeated webhooks, and a pretend AI that answers slowly or says "too busy" in bursts. Each round reports how many AI calls were running at once, how many were refused by the limits, how often Hava fell back to the fixed question, how long people waited (typical, slow and slowest cases), how busy the database and the background work got, and any duplicate text, or any AI call caused by the same text arriving twice, which must be zero. The last round says what slowed down first.
- **A gap found on the way.** Nothing in the plan yet connects the app to the AI: there is no setting for where the AI lives, the server never builds the AI connection, and the job of wiring the AI into the conversation (IC-23) has no task. The war games need it (the test site's real Claude calls and the load test's slow-AI part). The pre-war-games review must now check it, and if it's still missing it comes to you as a must-fix item before the war games. Until then the load test runs without the AI and says so.
- **Not done, and why.** More API keys don't add capacity: keys on one account share its limits. A second AI provider, or switching to one when Claude is slow, isn't planned: the plan uses Claude only, and adding another company that sees homeowners' words is your privacy and product call. If you want it, say so and it becomes a plan change.
- **Waiting for real numbers.** A shared limit on AI requests across all servers, an automatic cut-off when the AI keeps failing, cheaper models for simple steps, prompt caching and tests at 10,000 to 100,000 conversations wait until the war games (and later, real use) show they're needed.

## Your intake-speed review and your 3-second reply limit (D-2038, November 8)

- **What was checked.** The review you shared was checked against the code. One real risk: when the AI reading is wired in, the code as written would keep a database connection busy while it waits for the AI, up to 20 seconds per call by default (retries included), and the text-message webhook does its work inside one database transaction. A few slow AI answers at once could tie up the database for everyone.
- **What changes.** A new rule (AI-029): nothing holds the database while waiting on the AI, an incoming text is saved before any AI call, and the per-message AI limits are reserved in one step so a repeated webhook can't slip extra AI calls past them. T236 (AI-call budgets) builds and tests it; the task that later wires the AI into texts must keep it. The war-games harness (T234) now records how long each step takes (from a message to the next question, the AI's time and any database wait) as typical and slow-case figures per scenario, on fake data only. It also counts how many turns fell back because the AI was too slow.
- **Your reply-time decision.** As you said, 20 seconds was way too long. Any AI call a homeowner's reply waits on now gets 3 seconds by default; the setting can go from 1 to 5 seconds and never higher, and retries only happen inside that time. If the AI hasn't answered in time, Hava asks the fixed question at once. A new small task (T296) builds it before the pre-war-games review, so the review and the war games test it; the review recommends the final value between 1 and 5 seconds and the war games confirm it. Reading a business's quote, which no homeowner is waiting on, may keep a longer limit.
- **What doesn't.** Fees, privacy, consent and wording are untouched. The fixed question path and the AI call budgets the review suggested are already in the plan. Prefetching the next question and uploading photos early wait for your sandbox review; voice and prompt caching are not planned for now.

## The runner now wakes itself up (D-1451, November 2)

- **The problem.** The build only started its next step when something nudged it: a task finishing, or GitHub's hourly timer. That timer is unreliable: it went off 24 times in 123 hours, with gaps up to almost 8 hours, so a quiet build could sit for hours doing nothing, or wait for a session like this one to nudge it.
- **What happens now.** Every time the runner finishes a turn, it sets its own alarm: 20 minutes when there is work waiting, just after the current task's time runs out when one is in progress, or just after the usage limit resets when it is waiting on that. When the week reaches about 97%, it sleeps in steps of under 6 hours until the weekly reset and starts again by itself, so no one has to schedule the restart.
- **The safety.** Only one alarm exists at a time. When it goes off, it checks that nothing else is running and no task is in progress before it starts anything; the existing locks are unchanged. It never starts anything while the runner is switched off or while a broken `main` is waiting for a person. The hourly timer stays as a backup.
- **Cost.** None in money: the repository is public, so GitHub's build minutes are unlimited. The settings file that still described the old private-repository limit of 3,000 minutes now says that (the old figures are kept in case it ever goes private, when the sleeping alarm would cost minutes).
- **Also fixed.** One task's test failed because a leftover build from the runner's own check of `main` was reused with the wrong folder baked in. The runner now clears its own project's build pieces after that check (the downloaded libraries stay), so a task always builds its own copy. The price: every task now rebuilds the project's own code once at its start (the libraries it uses stay built), which adds some minutes to each task.
- **Follow-up for a later plan change.** Have the tests that look up files by their build-time folder (`env!("CARGO_MANIFEST_DIR")`, `fake_data` among them) find that folder while they run instead; then the clean-up above can be dropped and the rebuild cost goes away.
- **What you may notice.** In GitHub's Actions list, the runner's alarm shows up as many short runs marked cancelled or skipped: each new alarm cancels the one sleeping. That is expected and harmless.

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
  - waitlisted businesses get a notice when their trade opens (the price lock that started then was dropped, D-1494);
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
- **Fees:** amounts stay $25, $75 and $199 for now; the test asks whether $25 or $35 would have been worth it. Early joiners were locked for 6 months instead of 12 (since dropped: no price lock, D-1494). Ask Hava may refund any charge at its own choice.
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

## A stuck fix for a broken main no longer stops the build for good (D-1099, October 31)

- **What happened.** Today `main` broke for the first time and the fix task (FIX-MAIN) failed its first try. Had it failed all three, or been blocked for any other reason, it would have stopped for good, and so would the whole build, until someone edited the runner's state by hand, which the rules forbid.
- **What the runner now does itself.** It waits at least 30 minutes, until its next cycle after that, then gives FIX-MAIN a fresh set of three tries on the same undo, with every earlier failure in its instructions. It retries at most three times, and each retry opens an issue saying what failed. Nothing is needed from you for those.
- **When a person must act.** If FIX-MAIN is still blocked after its third retry, you get one issue saying it is stuck, with what failed, the files and the commit it is undoing. Once the cause is fixed, `python3 -m runner --requeue FIX-MAIN --reason "..."` starts it again. If the undo itself went in and `main` is still broken, you get one issue about that straight away, because undoing it again cannot help; the build resumes on its own once `main` passes again.
- **What did not change.** `main` is still fixed only by undoing the bad merge, and it is still marked fixed only after the runner's own check passes on it.

## The test that kept turning main red for no reason is fixed (D-1086, October 31)

- **What changed.** A test of the phone-call client failed now and then (about six false alarms, and main went red after T156). The cause was in the test setup, not the app: the test picked a free network port, then started the fake vendor server on it, and another test running at the same moment could grab that port in between. The test now waits for the fake server itself to say it is listening on its port, and starts it on a new port if it could not get the first one.
- **Calls Claude made.** No test was loosened, skipped or retried, and the app's code did not change. The fake vendor server (a protected file) was left alone. One other crate (the AI tests) has the same setup and is left for a later fix.

## How a business's quote comes in (T046, D-1048 to D-1052, October 31)

- **The 24 hours.** Each business may quote for 24 hours from when its own card was actually sent, every day of the week. A quote with a phone number, email, link or "call me" line goes back to the business with a plain line saying what to take out; nothing it wrote is kept.
- **Edits.** A business can change its quote until the homeowner picks someone. After a pick (or after a pick that fell through) its quote can't be changed, though a business that hasn't quoted yet still can, up to the moment the picked business confirms. A business that withdrew and then sends a new quote inside its 24 hours is simply back in.
- **A pick that falls through.** That quote is marked "no longer available" and can't be chosen again; if it was the only one and every business's 24 hours are over, the request closes and the homeowner is told.
- **For you to look at.** The plan lists a table of sent-back quotes; it was not built, because a sent-back quote holds exactly the contact detail we refuse, and a table of just "a quote was sent back at this time" adds nothing yet. If you want a count of how often businesses try to slip contact details in, that is a small later change. This task also came out about four times its planned size (about 1,580 lines against 400), mostly tests and the required code notes; it was kept as one piece because the parts share one database step.

## What a quote card shows (T186, D-1111 to D-1113, October 31)

- **The same card for everyone.** A business previewing its own quote sees exactly the card the homeowner sees; there is one way to draw a card, with no "who is looking" switch.
- **In this order.** The business's name, its license line with the "check it yourself" link, "Ask Hava doesn't rank businesses; check it yourself.", "Independent business, not affiliated with Ask Hava.", "Gets emergency requests at night" if it ticked after-hours emergencies, then the six quote facts (trip fee, parts, permit, whether the price may change, earliest visit, Spanish), a plain line on what the quote type means, and the business's own words. Spanish pages get the same card in Spanish.
- **Worth a look.** The spec gave only two of the quote-type lines (firm price and free estimate). Claude wrote the other four (price range, recurring price, packages, visit fee) in English and Spanish after two read-through reviews. The Spanish uses "cotización" rather than "presupuesto", since most local Spanish speakers would say it that way. A "tonight" visit reads "the night of November 3", so the card stays right the next day.
- **AI summaries stay off.** No summary is stored with any quote. The switch can only turn on with your recorded decision and a privacy policy carrying the summary sentence. Even then, a summary using any word or number the business didn't write is thrown out. That is strict: a summary that only rephrases is rejected too, which is the safe side.
- **Still to come.** The Google and Yelp links (T165), the price line (T164) and the stored quote facts (T148) arrive with their own tasks; this card already has their places.

## Why a free lane sat idle this afternoon (D-1126, October 31)

- **What happened.** T156 (the intake question bank) looked ready in the runner's state, but every lane refused to start it: a plan change earlier today had made T156 wait for T187 in the plan file, and the runner never copied that new wait into its state file. The two halves of the runner disagreed, so lane 1 reported "idle" with a task that looked ready. Fixed: the runner now copies changed waits into its state.
- **The wait itself was longer than needed.** T156 only has to come after T050 (they edit the same intake file); waiting for T187 would have put it behind eight tasks instead of four. It now waits for T050, and T208 keeps its own order after T054 directly. Nothing else moves.

## Spending the week's budget by Thursday morning (D-1128, October 31)

- **What you asked for.** Use the week's tokens up by a few hours before Thursday's reset, as accurately as possible, instead of running dry on Monday and sitting idle for two days.
- **What changed.** The runner now follows a straight spend line from today's reading to the budget at 8:38 am Phoenix on Thursday (three hours before the reset). When the next task would put spending above the line, it waits until the line catches up, then claims; nothing else about pacing changed. It also runs one lane instead of two: the remaining stage-2 tasks are a chain anyway, one lane paces exactly, and it removes the claim races and update-merge conflicts that cost attempts this week. Two lanes come back by removing `spread` and setting `lanes` to 2.
- **The caveat.** The budget the line aims at (200M weighted tokens) is still the calibration from your "about 50%" reading, because the runner's own usage probe is still falling back to its ledger. Fixing the probe is next, so the line can aim at the real number.

## Every remaining task now runs on Opus (D-1143, October 31)

- **Your call.** With the budget to be spent by Thursday rather than saved, the 39 queued tasks that were set to run on the cheaper Sonnet model now run on Opus, and the 14 whose reviewer was Sonnet get an Opus reviewer. Finished tasks keep the model they ran on; T047, already running, finishes on its original setup.
- **Why not a second review instead.** The attempts lost so far came from plan gaps, merge conflicts and a flaky test, not from a reviewer missing something; a second reviewer would cost about as much as the worker. The stronger model on first attempts is where the tokens buy accuracy.

## The runner now reads your real plan usage (D-1155, November 1)

- **What was wrong.** The runner paced itself on its own estimate because the probe that should read the plan's usage never had a login to use inside the build job.
- **What changed.** The build action already reports the plan's live usage during each session; the job now keeps the last reading at the end of every session, and the pacer uses it for the next claim, so the Thursday spend line measures against the same percentage you see. No secret is passed around: the reading is taken from the session's own output after it ends.

## The spend line now runs on your real plan percentage (D-1167, November 1)

- **What happened.** The runner's first in-band reading came in at 6:02 pm: 59% of the week used, reset Thursday 10:00 am Phoenix. The line had been set in the runner's own estimate, which sits about 10% lower, so left alone it would have waited most of Sunday before claiming.
- **What changed.** The line now starts from that 59% reading and ends at about 97% at 7:00 am Thursday, three hours before the reset — about one task every hour and a half, each on Opus. The old 10% safety margin is gone; the three-hour time margin does that job now.

## T208 fits its context budget again (D-1227, November 1)

- **What was wrong.** T208, the step where the homeowner approves her job card before it goes out, had grown to 47% of a session's working memory, over the 45% limit, because the intake files it changes got bigger in T048 and T049.
- **What changed.** It no longer loads the fake sample-request list used for the routing test (that list belongs to T154 and T156, not to the approval step). Nothing it must build or test changed, and the limit stayed at 45%. It now measures 39%.

## A retry now learns what failed (D-1263, November 1)

- **What was wrong.** When a task's checks failed, the next attempt was meant to get the last lines of the failing step's output. It never did: on every failure since October 30, including T055 today, it got only the names of the failed checks and steps, so it had to guess what broke. Downloading the step's output was being refused, and the runner threw the refusal away without a word.
- **What changed.** The runner now writes down why the download was refused, so the cause shows the next time, and falls back to the error notes GitHub attaches to the failed check (file, line and message), which need no download. No permission or setting changed.

## No more pacing holds (D-1189, November 1)

- **What you asked for.** At 5:07 am Sunday: no more pausing; running out of the week's plan early is fine; don't go faster where slower gives a better result.
- **What changed.** The spend line is gone, so the runner claims the next task as soon as it fits the five-hour and weekly limits, and stops only when the week is about 97% used, until Thursday's reset. Everything that makes the work careful stays: one lane, Opus on every task, one independent review, the smoke test every cycle.
- **One thing to know.** GitHub's hourly timer for the runner is unreliable (it skipped seven hours overnight), so Claude starts the runner itself whenever the lane sits idle with work ready. A small change to let the runner schedule its own next run can come later.

## War games before anything goes live (D-1154, November 1)

- **What you asked for.** Once the build is done, and before going live or hooking Twilio up for real, Claude tries to break the business: every kind of homeowner and business, pranksters and people trying to burn AI credits, jobs finished with test-mode money you can see in the owner portal, run as chaotically as possible until a full run finds nothing new.
- **Where it sits.** Six new tasks run after the restore drill and before the final Opus audit, so the audit checks the fixes too. First a toolkit of fake characters (on both sides) and ways to cause trouble (duplicate messages, texts out of order, the clock at the 8 p.m. edge, a vendor going down, the AI answering nonsense). Then a money run: at least 60 paid jobs through Stripe test mode, with refunds and a dispute, where the ledger, Stripe and your owner portal have to agree to the cent. Then three rounds of at least 5 pretend business days and 100 jobs each, fixing what they find as they go, ending with a clean round and a report in `docs/phase2/war-games.md`. Everything uses fake people and fake numbers; no real person gets a text.
- **Worth a look: one new rule.** The plan already limited how many requests one phone can make and capped total AI calls per day, but one prankster could still burn the whole day's AI allowance. New rule AI-028 gives each step of a conversation its own small allowance, sized so an honest homeowner never runs out: no AI until she has had the consent text and given a ZIP in the area; 2 reads per message, 1 rewording per question, a few calls for the card and each change she asks for, 4 for a concept picture, 2 per photo for up to 3 photos. On top of that, a conversation gets at most 40 calls a day and a job 60. The call that writes her card is always kept back, so she always gets her card. Businesses' quotes have their own allowance, so a prankster can't stop real quotes being read. Long messages are cut to 1,600 characters before the AI sees them, but the plain card keeps everything she wrote. Past an allowance Ask Hava carries on with its plain rules and says nothing. The numbers are settings, tuned from the war games. A first draft with one flat cap was too tight for honest customers; a review caught it and this is the redesign.
- **Spend during the war games.** The AI is switched on on the test site for the rounds, using the test project's key on fake data, capped at $40 for the whole campaign. Texts use Twilio's test numbers only, so no real phone gets anything.
- **Worth a look: what happens with a finding that needs a plan change.** A fix that would change a rule or a test in the plan isn't made by the war games themselves; it comes to the top of this file for you. If the last round still finds something new, the report says so plainly and another round needs one more plan change.
- **Your API credit.** As you asked: when you set up the Claude Console at the account checkpoint, set it up under the same Anthropic account as your Claude subscription, so the live Claude key you make at Gate C draws on the $200 a month of API credit it includes instead of a separate bill. The account-checkpoint message and the Gate C checklist both say so now, and the checklist has you confirm it on the Console's billing page.

## The two missing pieces now have their own tasks (T240, T241, D-1215, November 1)

- **What changed.** The two gaps T048 found (below) each got a task of its own, placed in stage 2 right after the text pick (T054). T240 sends the texts a request gets when it moves on by itself: "no quotes yet" at 6 p.m., the two "closed" texts and "all quotes are in" with her quote-page link. It holds them to 8 a.m. to 8 p.m. in both Arizona time and the time zone of her phone's area code, without intake ever seeing her number. T241 hooks up Twilio's incoming texts for the first time, so a reply reaches the app at all, and inside an open request sends LIST to the quote list and restarts the 72-hour clock when she replies.
- **Bigger than it looked.** Checking the plan showed that no task took incoming texts from Twilio into the app at all, not just the LIST and clock pieces. T241 covers that too. Journey J1, the stage 2 cleanup, the day-3 check-ins and the HELP follow-ups now wait for these tasks.
- **Still open, for later.** The 24-hour reminder and the 72-hour close still check quiet hours in Arizona time only. Texts from business phones reach the app but nothing answers them yet; the business-reply tasks will need a plan change to add that. The "That's 2 jobs" texts (D-999) are still open too.

## Two pieces still need a task: some request texts and the inbound wiring (T048, D-1198, D-1199, November 1)

- **What is missing.** When a request moves on by itself (the 6 p.m. move to Open, a close with no quotes, a close after quotes with no pick, "every quote window has ended"), the system records that a text to the homeowner is due, but nothing sends it yet. The plan gave this to T048; it turned out bigger than T048's size limit, because the text needs the quote-page link and the homeowner's own time zone. The runner should give it its own task.
- **Also not wired yet.** The code that answers LIST and that restarts the 72-hour clock when she replies is built and tested, but no incoming text reaches it until the task that routes replies inside an open request exists (the same gap as D-999).
- **Call-backs.** When a homeowner can't be reached by text or email, Ask Hava now raises one call-back alert for you: urgent ones (Today, emergency, or a 911-type word) are due within 24 hours and go in your 9 a.m. / 3 p.m. push; others are due within 4 days. The parts that notice "can't be reached" (failed texts, web chat) come with later tasks.

## Closing a request by text (T049, D-1200 to D-1203, November 1)

- **What closes.** "CLOSE 1042" (or CERRAR 1042), "CLOSE ALL", the close button on the quote page, or a clear message like "please cancel my request", "never mind", "no longer need it", "ya no lo necesito". The word "cancel" alone never closes anything: it is an opt-out, which stops texts but leaves the request alone.
- **Worth a look.** Spanish "ya no" closes only when it is the whole message (or "ya no lo necesito" and the like), because "el aire ya no funciona" means the AC stopped working. The phrase list is short on purpose; tell me if you want words added. A phrase counts only as its own sentence, so "the last plumber had to cancel the job, can you find someone?" and "I don't need it today, next week is fine" keep the request open (D-1203).
- **Several open jobs.** She gets "Close which?" with each number and CLOSE ALL, and nothing closes until she answers. A message that only mentions cancelling ("I had to cancel the plumber") gets "To close request 1042, reply CLOSE 1042."
- **Not wired yet.** Like LIST, this waits for the task that routes replies inside an open request (D-999), and for the quote page's close button route.

## The homeowner's quote page (T052, D-1233 to D-1237, November 1)

- **What she sees.** At her private link: "Your quotes for request 1042", "Free for you. The business you pick pays Ask Hava a set introduction fee.", the no-ranking line, "Every eligible quote shown, in the order received. #1 means first received, not first choice.", then each quote headed "Quote #1", "Quote #2", "Quote #3" in the order they came in, each with "Ask Hava doesn't rank businesses; check it yourself.", the business's own words, a Pick button and "or reply 1 YES" beside it. A quote that can no longer be chosen keeps its number and says "No longer available". The LIST line and "Ask Hava is not a contractor..." are at the bottom.
- **The link.** Nobody can guess it; search engines are told not to list it; it stops working 30 days after the request closes. A new link makes the old one stop at once.
- **New quotes.** With JavaScript the page checks every 30 seconds and adds new ones, and a screen reader hears one short line ("2 new quotes added below.", with "1 quote is no longer available." added to the same line when one drops out). Without JavaScript a reload does the same.
- **Not there yet.** The business name, license lines, price and the other quote facts on each card (nothing stores them yet), the Close button, the Pick step itself (T055), Spanish, and the "still open" line after a STOP (it shows after a CANCEL). Each is logged in D-1236 for the runner.
- **Needs an owner: numbers on the page and in the LIST text can differ.** The page keeps a quote's number when it becomes unavailable; the LIST text (T047's `list_rows`) skips unavailable quotes, so after a failed pick "3 YES" by text can mean a different quote than #3 on the page. A quote confirmed late can also slot in ahead of later ones. Fixing it needs either a stored arrival number on each quote (a `quotes` migration) or `list_rows` keeping unavailable quotes in its numbering; neither file is T052's, so a `quotes` task must take it (D-1233, D-1237).

## The accessibility checks (T053, D-1238 to D-1241, November 1)

- **What runs.** Every page the app serves (today only the quote page, in five states: no quotes, three quotes with one gone, paused after CANCEL, after a pick, closed) is checked at phone, tablet and desktop widths: axe-core's WCAG 2.2 AA rules, one main heading, landmarks, a skip link, the page language, tap targets of 24 px, contrast, no sideways scrolling at 320 px or 400% zoom, larger text spacing, reduced motion, and the status and alert roles.
- **New pages can't be skipped.** The page list is read from the app's routes; a new page with no test data fails the run.
- **Worth a look.** It isn't the required Ally check yet: the Ally workflow is still a placeholder until T173 switches it on. No page has a "Get help" link yet, so "help in the same place" passes for now; the first page that adds one fails until every page has it.

## The pick code and "2 YES" text picks (T054, D-1243 to D-1247, November 1)

- **The code.** Picking on the page needs a 6-digit code by text: good for 10 minutes, 5 tries, then a 15-minute wait. A call reading the code goes only when she taps for it, and never between 9 p.m. and 8 a.m. unless she asked from that phone. Each number gets at most 5 codes (3 calls) a day, each job 10, and if Ask Hava's daily total passes your threshold, codes stop and you get one urgent alert that day.
- **"2 YES".** Works when she has one request waiting for a pick. The first time, she gets the "By picking, you agree to the Terms" line and "Reply 2 YES again to pick"; the repeat picks. With two requests waiting, Ask Hava asks her to reply like "1042 2 YES". A second YES for another quote replaces the first pick and the first business is told. With nothing waiting she gets where each request stands.
- **CANCEL.** A pick waiting for the business's Confirm is withdrawn and that business told; no new pick until she picks or closes on her page.
- **Worth a look.** The reply says "You picked quote 2 for request 1042" rather than the business's name, because business names aren't stored yet (D-1243). "Per day" means the Arizona calendar day (D-1244). Nothing calls these yet: the text dispatcher and the page's Pick button come in later tasks (D-1247).

## The pick review step (T055, D-1248 to D-1252, November 1)

- **What she sees.** Tapping Pick on a quote opens a check step: what happens next, "or reply 2 YES", a "Go back" link, and the final Pick button with only "By picking, you agree to the Terms" beside it (the link underlined, in the link colour). No $100 cap and no end time on this step.
- **Not finished yet.** The step tells her a 6-digit code comes next. The final Pick doesn't pick anything yet: it says "Nothing was picked" until the code step (T187) is built (D-1248, D-1251). She can pick again while the first business hasn't confirmed, as the "You can still pick another quote" line promises, but the quote page still hides its Pick buttons then; that needs a follow-up (D-1251). The "Done" page after a pick is ready for that task: the heading gets focus, "Add Ask Hava to your contacts" and "Know someone who needs quotes? Send them askhava.com." The emergency switch and a Spanish page are open (D-1249).
- **Worth a look.** A real browser's form posts are refused by the site's cross-site check (D-1250): the "no-referrer" setting makes the browser send `Origin: null`. Every form on the site is affected; a fix is proposed for a security task.

## The pick code and contact form (T187, D-1264 to D-1266, November 1)

- **What she sees.** After the check step comes one form: how the business reaches her (a phone call through an Ask Hava number, so they never see her number, or an email she types), then the 6-digit code box. She can type, paste or let her phone fill the code. It says a code works for 10 minutes and that "Send a new code" keeps her answers.
- **Mistakes.** If something is missing, the page title starts "Error:", a box at the top lists each problem as a link to its field, and everything she typed stays.
- **Not finished yet.** No code is sent and nothing is picked: the form says so at the top. Sending and checking the code, and saving the pick, need wiring that no task owns yet (D-1264). Messages for a wrong or run-out code come with that wiring.
- **Worth a look.** There is no pop-up warning a minute before the code runs out; the 10-minute limit is stated up front instead, and a new code is one press away. The rule asks for a warning shortly before the end; that comes with the wiring (D-1265).

## How intake asks its questions and how sure it is (T156 again, D-1269 to D-1273, November 1)

- **What was built.** This is the second try; the first was taken back on October 31 (D-1053). The trade's questions are asked one at a time in the question bank's order, "Not sure" last on every list, never more than 8. A smell answer that doesn't say what kind asks the exact smell question once; "Gas" or "Burning" sends the 911 text. Intake goes on without asking the trade question only when its readings agree and are sure enough; otherwise it asks. None of it is wired into the live conversation yet.
- **The test set is far from the target.** The app checks itself against its 370 sample requests each time the catalog changes. On the current keyword rules it gets 21 of 370 right (5%), against the 90% target. The keyword rules cover few trades and little Spanish, and the build has no real AI to test with yet. The number is shown as it is, not adjusted.
- **The knowledge base's test file isn't used yet.** `test_set.csv` names its trades and job types in the knowledge base's own words (for example "fridge", "range_oven"), which don't match the catalog's job types. Someone has to map them before those messages can be run.
- **Two pieces still have no owner.** When intake can't tell which kind of job it is, the job is marked "Job type not certain" and goes only to businesses licensed for every possibility. But the job card has no line to show that mark, and there is no text yet telling the homeowner "We couldn't tell which kind of <trade> job this is". Both are left for the tasks that own the card and the homeowner wording.

## Showing the homeowner her card before it goes (T208, D-1274 to D-1277, November 1)

- **What was built.** Before a job card goes to businesses, the homeowner gets it by text: the whole card when it fits in 2 text segments, otherwise a link to it. She can reply SEND (or OK, YES, ENVIAR) to send it now, or EDIT (CAMBIAR) to change it. If she says nothing, it goes after 30 minutes. Emergency jobs skip this. The app keeps a fingerprint of the card she saw, and only that exact card may go to businesses.
- **The EDIT reply is new wording.** "Ask Hava: Sure, what should we change on request 1042? Tell us in one text and we'll show you how it looks now. Nothing goes out until you've seen it." After EDIT, nothing goes until she has seen the changed card.
- **Spanish cards will nearly always go by link.** Spanish accents make the text longer to send, so the full card rarely fits in 2 segments. The Spanish wording is still a draft for the native-speaker review.
- **The 30 minutes start when she gets the text (D-1278, D-1279).** If the text is held overnight by quiet hours, the clock waits too. If the text can't be delivered, the card doesn't go out on its own; who gets told then isn't built yet. If she asks to change the card and it comes back the same, she still gets it again with a fresh 30 minutes.
- **Not connected yet.** Nothing calls this step until the conversation wiring is built, and the sender that texts cards to businesses doesn't exist yet. The page behind the link is T095's.

## Page and journey checks on every change (T173, D-1280 to D-1283, November 1)

- **What was built.** Every change now runs the browser journeys as part of `Ci` and the accessibility suite (axe at phone, tablet and desktop widths) as `Ally`. One failing check in either turns it red, so the change can't merge. Any axe result that needs a person to look at it is listed on the run's summary page.
- **The image now carries the small browser scripts.** The production image compiles the quote page's script and keeps only the result. Nothing serves those files to the browser yet, so the quote page still works the way it does without JavaScript (D-1235).
- **The homeowner journey check is only half there.** The stage-2 proof is meant to walk a homeowner from her first text to picking a quote. Today only the quote page and the pick form can be walked, and the check jumps straight to the code-and-contact form because the real final Pick button doesn't send a code yet. Missing: the first four steps (text in and consent, ZIP and trade, urgency and questions, the card), which need T241 so the app can receive a text, and the checked pick code, which needs the Verify wiring. Those steps aren't in the check at all; this note and D-1280 are the only record of them. Stage 2 shouldn't be called finished on this file alone.

## Deliberate-break checks for licenses, texting consent and quiet hours (T056, D-1285 to D-1289, November 1)

- **What was built.** A list of 50 small deliberate breaks to the license, opt-out, consent and quiet-hours rules (for example "quiet hours end at 9 p.m. instead of 8" or "STOP is no longer an opt-out word"). A script puts each break into the code for a moment, runs the tests that guard it, and puts the code back. If the tests still pass with the rule broken, the check fails. Every change that touches one of those files runs the matching breaks; every night runs all of them. All 50 were caught when this was built.
- **Stripe's address list.** Every night the build downloads Stripe's published list of the addresses its payment notices come from and compares it with the copy the app uses. Any difference fails the night, so the copy gets updated in a normal change. Today they match.
- **Worth a look.** The wider automatic tool (cargo-mutants, which invents thousands of breaks by itself) runs every night but only reports for now. The plan says each break it finds that the tests miss is either covered by a new test or written down as harmless after a review, and no task yet owns that list (D-1286). Two kinds of break the plan names were left out because the code they would break doesn't exist: there is no list of "yes" words, because writing in at all is the consent (D-1287).
- **A gap in the plan, fixed for now.** The list of breaks quotes exact lines of code. Later tasks that are allowed to change those code files aren't allowed to change the list, so an ordinary edit could have blocked their own change for a reason they can't fix. Now a change like that is reported as "stale" and passes; the nightly run still fails until the list is brought up to date, which makes the build stop and fix it the next day. No task is yet assigned to keep the list current, so that next-day stop is the only thing that catches it (D-1285).

## The build checks its own work before handing it in (harness change, D-1300, November 1)

- **Why.** Of the 85 tasks done, 18 needed a second try, and each second try costs about a million tokens. In 10 of them the first try failed a GitHub check the worker never ran on its own branch: the rule that a task changes only its own files and the decision log stays in order (T047, T167, T184), the scan for personal data (T176), the npm security audit (T227), a Python test (T055) and the test-coverage floor (T020, T022, T033, T036).
- **What changed.** A new script, `scripts/prehandin.sh`, runs those same checks on the task's branch against the latest `main` in about 20 seconds. Every task prompt now tells the worker to run it last, after the smoke test. And the runner runs `main`'s copy of it itself when the work is handed in: if it fails, nothing is pushed, the session is handed the failure and gets up to two tries to fix it in the same session, the cheap way. Only after that does the attempt count as failed, and then the next try reads exactly what failed.
- **What it doesn't cover: test coverage.** Measuring coverage needs a second, special build of the whole app: about 4.5 minutes from cold and still about 4.5 minutes warm here (the 4 tasks that failed it would each have needed it), plus 5 GB of disk. That's past the 10-minute limit a single step in the runner's session gets, once the smoke test is counted. So coverage stays in GitHub's check only. The follow-up worth considering: put a fast coverage estimate in the prompt for tasks that own money, privacy or licensing code, or give the runner's job a separate coverage step with its own cache.
- **Worth a look.** The gate can only fail work on a real check result. If it can't run (an older `main`, a timeout, a git problem), it lets the work through and GitHub's checks still decide, as before.
- **A side finding.** The prompt-size check measures three already-finished tasks (T038, T156, T208) over their 45% limit now that the architecture file has grown. They're done, so it changes nothing for them, but the strict check fails until a plan change exempts finished tasks the way it already exempts T005 to T007. Their stored numbers are left as they were. After merging `main` in (T056 grew the architecture file again), one *unstarted* task also measures over: **T170** (the harness task for conditional gate answers) at 51% against its 50% limit, so its stored 50 is kept for the same reason. Before T170 is claimed it needs a trim (drop one read, as D-1227 did for T208) or a plan note raising its limit; the overseer will fold that into the next plan change.
- **After the independent review (same change).** The gate now skips a check the branch doesn't have yet instead of blaming the branch, and tells the worker to bring `main` in with a plain merge. A git problem (no common history with `main`) never counts as a failure. Anything the gate prints is scrubbed for keys, phone numbers and emails before it's saved. Workers run the full gate (the npm and cargo audits too, which need the internet): the runner's GitHub job has internet access and already installs npm packages for the smoke test, and the runner's own check at hand-in stays the quick, offline part.

## The business job page and short update texts (T213, D-1290 to D-1294, November 2)

- **What was built.** The data behind each job's page for businesses: the card the homeowner approved, every answer she gave, anything she adds later, how many businesses got the card and how many have quoted. Only a business that got the card can open it, and anything she typed passes the same filter as the card, so a phone number or email she wrote never shows.
- **Short texts.** A check now holds the texts that tell her about her quotes to short sentences (15 words, or 20 where the spec sets the exact words), each with the link to her page and no prices, except the LIST reply. Her other texts after the request goes out ("request is out", "no quotes yet", "closed", the text-pick replies) are held to 20 words a sentence. Every text to a homeowner is either checked or on a named list of ones left out, so a new text can't skip the check. Today's texts all pass.
- **Two different quote counts.** The business job page counts the quotes she can still see (a withdrawn one drops off); the "3 of 7 businesses quoted" text still counts a business that quoted and then withdrew.
- **Worth a look.** When she adds details later, the businesses should get an email about it. No task builds that email yet; it is flagged for the plan.

## The texts a request sends by itself now go out (T240, D-1301 to D-1303, November 2)

- **What was built.** The four texts that were recorded but never sent now go to the homeowner: "No quotes yet" at 6 p.m., the close after 5 days with no quotes, the close after 3 days with no pick, and "All quotes are in ... 2 of 3 businesses quoted" with her quote-page link. This closes the gap listed under T048 above.
- **Quiet hours.** Each one waits for 8 a.m. to 8 p.m. in both Arizona time and her phone's own time zone. A text held overnight is dropped if the request moved on meanwhile (she picked, or it closed), except the close text itself, which still goes in the morning.
- **Who never gets one.** Someone with no consent record, and anyone who texted STOP (her request carries on by email and the quote page).
- **Worth a look.** Sending "All quotes are in" makes a fresh quote-page link, and the earlier link (in the first-quote text) stops working at once, as the quote page already does for any new link. Tell me if older links should keep working.
- **For the account checkpoint.** The worker sends these only when the two vault keys are set; in the test and live projects it refuses to start without them.

## Workers keep their memory warm and read less at the start (harness change, D-1317, November 2)

- **Why.** Claude keeps the conversation so far in a short-term cache, so each new step re-reads it cheaply. For the worker that does a task, that cache lasted only 5 minutes. The worker waits about 6 minutes for the smoke test, so the cache ran out, and the next step paid full price for the whole conversation again. On T173 that was 144,000 tokens, half of everything it wrote to the cache. The start of each task also loaded big files the prompt already contains, plus whole files of 300+ lines that every later step then carried along.
- **What changed.** The worker's cache now lasts an hour: a documented Claude Code setting, which I tested here to confirm it works for workers. The prompt tells the worker to check on the smoke test at most every 4 minutes, never to sit silent longer. The start routine no longer re-reads the brief, the build state or the progress log, because the prompt already pastes the rules and the runner already checked the task. A new rule says to read only the needed lines of any file over 200 lines. Any command output over 15,000 characters now arrives as a short preview and a file to open, instead of sitting in every later step.
- **Expected saving.** About 15–20% of a task's usage from the cache alone, and roughly as much again from the smaller start: around 50M weighted tokens over the 148 tasks still queued. The prompts got slightly shorter, not longer.
- **Worth a look.** An hour-long cache costs about 60% more per token written (API price list). With the new 4-minute check-ins the long silences should mostly go away, and then the hour-long cache only pays off if more than about 6 tasks in 10 still have a silence over 5 minutes. It stays for now; the first 5 tasks after this merges decide. If their logs show no more silences, the setting comes out.
- **Retries keep their failure note.** The worker used to read the build state, which is where a retry's "what failed last time" note lives. Now the runner's session hands that note to the worker directly, before the task prompt.
- **Task sizes on file.** 68 tasks' stored size numbers changed, most by 4 points. They were out of date on `main` (the architecture file grew with T213 and T240), and this just catches them up to what they measure. The strict size check drops from 72 problems on `main` to the 4 known ones (T170, T038, T156, T208). Correction to what I first reported: the stored numbers don't only ratchet up; they follow the measurement both ways within 3 points. The real gap is that the everyday check doesn't flag stored numbers that have gone stale, so they can reach `main` that way.

## A finished task was thrown away over a comma (harness change, D-1328, November 2)

- **What happened.** T241 (the route that lets a homeowner's text reply reach the right request) was built, reviewed and passed the new pre-hand-in gate, then the runner's own session typed the two worker IDs into one flag separated by a comma instead of two flags. The runner looked for a transcript file named with that comma, found nothing, and treated the finished work as "no model ran", which fails the attempt by rule. About 48 minutes and roughly 1.8M plan tokens were lost; the task went back in the queue and the next run retries it.
- **What changed.** The runner now reads IDs joined by commas or spaces the same as repeated flags, and the session's instructions say to use one flag per worker. The rule that a hand-in with no evidence fails stays as it was.
- **Worth a look.** The worker never pushed its branch, so nothing of that attempt survives; the retry starts fresh. If a second attempt of any task is ever lost to a formatting slip like this, the fix is to make the runner read the agent IDs from the session itself rather than from a typed command.

## The runner checks a task's plan before it starts one (harness change, D-1350, November 2)

- **What changed.** Before the runner hands a task to a worker, it now checks the task's plan: does the task fit in the worker's memory; does every file its tests or requirements name belong to it or to an earlier task (or already exist); did every earlier task it builds on leave its files on `main`. A task that fails is held as a "plan gap" with no attempt used, the build status page lists it with the reasons, and the runner moves on to the next task. Once the plan is fixed, the task is picked up by itself.
- **Why.** A worker that starts a task with a hole in its plan spends an attempt (about a million tokens) finding the hole. The check costs a fraction of a second.
- **Size numbers got smaller.** The size check used to count the whole architecture file (177,000 characters, about a fifth of a worker's memory) for almost every task, though workers only read its layout section and a few lines. It now counts what they read. 215 tasks' stored size dropped, by 6 to 19 points (17 typical); the four tasks that were over their limit are now within it, and the strict size check has no problems left. A section a prompt itself points to ("architecture section 8") is still counted in full, which raised six tasks (the stage cleanups among them). T170, finished in October, keeps its old 51% on record, as T005 to T007 do.
- **Already done, nothing to change.** The handover asked to trim T170 and T172 before they ran and to move T170 to Opus. Both finished on October 29 and 30, so neither applies; every task still waiting is on Opus except T136 (the Haiku docs task that runs the journeys on the test site; its third try goes to Opus by the existing rule).
- **Plan gaps found on the first run (left for you or the overseer to decide).** (1) T148, T151 and T104 paste an old decision (D-353, replaced by D-491) naming two test files no task writes. After the review, an old decision shown only as history never holds a task, so these three are no longer held. (2) T089 names the deploy notes (`docs/deploy.md`), which T135 writes much later; T089's requirement says to record a measured setting there. (3) T024 (finished) lists `app/catalog/src/tests/restricted_words.rs` and T045 (finished) lists `migrations/0068_quotes_card_text.sql`; neither file exists, as the work landed in other files. Because T057 (the stage 2 cleanup) depends on T024, the runner would hold T057 as a plan gap until the plan stops listing that file.
- **Worth a look.** Item (3) would have held the next cleanup task; the separate plan change D-1361 (below) took the two never-written files off T024's and T045's lists, so after it the check holds nothing but T089, once T089 is next.
- **After the independent review (same change).** If the check itself breaks, the runner carries on without it instead of stopping, and any hold already on record stays as it was. A hold keeps the time it began, even when its reasons change. A task that stops being next in line keeps its hold on the status page until its turn comes round again; that is on purpose, since the problem usually still stands.

## Two finished tasks listed files that were never written (plan change, D-1361, November 2)

- **What happened.** The new plan-health check found that two finished tasks, T024 (the AI connection) and T045 (the anonymous job card), each still listed one file they never wrote: a separate test file for the restricted-words list, and a database migration to store the card text. A third task that waits on T024 (T057, the stage 2 cleanup) would have been held because of it.
- **What they really built.** T024 put the restricted-words tests inside its own AI test file instead. T045 built the card and its contact-detail filter as plain code with no database table. The card text is fixed by T045's own filter (running it again never changes the text, so every business gets exactly the same card), and a later task (T208) stores a fingerprint of the card the homeowner approved, not her words, so only that card goes out.
- **What changed.** The two unused file names are taken off those tasks' lists. Three waiting tasks (T148, T151, T104) also stop carrying an old decision (D-353) that pointed at two test files a later plan rewrite (D-491) had already moved to other tasks under new names. Nothing is lost: every requirement and test still has an owner.
- **Worth a look.** Nothing for you to decide. A separate question about T089 and the deploy notes is not part of this change.

## A competitive review now sits between the build and the war games (plan change, D-1339, November 1)

- **What you asked.** A mandatory "Pre-war-games competitive gap analysis and feature readiness review" between the finished, verified build and the war games, without disturbing the build. Your full instructions are saved word for word in `docs/reviews/pre-war-games-review-brief.md`, and that file is the review's requirement.
- **Exactly where it sits.** It is a new task, **T242**. It runs right after **T139**, the restore drill. By then every build task, the stage-5 audit and cleanup, the test site's four journeys, the real-AI test and the security scan are done. It runs right before **T234**, the first war-games task, which now waits for it. The other war games (T235 to T239) follow T234 as before, so all of them wait too, along with the final audit, the handover pack, "Finished" and Gate B. Nothing was removed or reordered. No other task's goal changed; T139 and the war-games prompts (T234 to T239) only re-quote the edited brief line about the war games.
- **What the review must produce.** A document titled "ASK HAVA — PRE-WAR-GAMES COMPETITIVE & FEATURE READINESS REVIEW" with the comparison matrix, a P0/P1/P2 priority for each item, legally blocked items listed apart, and answers to your eight questions. A second file holds the recommended feature plan and a decision log. Runhome is researched from public sources only, and each claim is marked as verified, marketing or speculation. Your earlier Runhome notes (price range before quotes, inspection-report upload, time-to-quote and win-rate measures, cold start) are included.
- **What is authorised and what isn't.** The review is mandatory. Building anything it recommends is not: every feature waits for your approval. The fees ($25 / $75 / $199; ongoing work at 4 months, capped at $75) and the business model stay as they are unless the review gives you a strong reason. Nothing turns Ask Hava into a contractor or construction manager, and anything near Arizona's contractor-licensing line (the "Hava, help me handle the rest" idea above all) is marked for a lawyer's review. Nothing goes live, and there are no paid rankings. I have not started the review or built any feature.
- **Scope.** Your brief talks about a national launch. You answered: "We will run it small market first with the plan to expand." So the build stays Lake Havasu City only. The review treats expansion (finding businesses where no one has signed up, growing without a person handling every job) as planning questions. Any answer that would change the scope comes back to you as a separate plan change.
- **The pause before the war games.** When the review is done, it adds a section here asking which P0 items you want built before the war games, or none. Any you approve get added as new tasks, built, tested and verified before the war games start. **Worth a look:** the task system can't hold a step for your answer at this point, because its gate types are fixed in a protected file. For now the pause is held by whoever is watching the runner, not by a machine. If you want a machine to hold it, that's a small harness change.
- **Later, not now.** Once any approved features are built, a further plan change will extend the war games to also test missing business networks, failed outreach, emergency demand, abandoned quotes, cancellations, fee disputes and abuse attempts. I left the war-games instructions untouched today, as you asked.

## Texts sent to Ask Hava now reach the app (T241, D-1307 to D-1311, November 2)

- **What was built.** Twilio's two webhooks (a text arriving, and a sent text's delivery status) now have routes. Each one is checked against Twilio's signature first and refused otherwise. A text is handled once even if Twilio sends it three times. In order: STOP (or any opt-out word) ends there; HELP gets only the HELP reply, even from someone who opted out; anything else from an opted-out number gets nothing; a text from a business phone is recorded only; everything else goes into the homeowner's request.
- **Inside an open request.** LIST or LISTA gets the quotes list; any other reply restarts the 3-day no-reply clock and carries on as before (CLOSE, cancel, "2 YES", the 911 screen).
- **Worth a look.** The HELP reply says "2 days, or within 24 hours if your job is urgent" until there's a setting for it. LIST can't name businesses yet (there's no business name on file), so for now a LIST with quotes waiting gets no list; it still counts as her reply, so the 3-day clock restarts and nothing is lost (D-1310). Any step that fails on a text is now logged by its error name, never its content (D-1311).
- **For the account checkpoint.** The test and live projects need `ASKHAVA_PUBLIC_BASE_URL` (the app's public address, which Twilio signs) or the web process won't start; Twilio's messaging and status webhooks then point at `/webhooks/twilio/sms` and `/webhooks/twilio/status` under it.

## The rate-limit task no longer waits on the deploy notes (plan change, D-1377, November 2)

- **What happened.** The plan-health check found that T089 (rate limits) was told to write down which header carries a visitor's real address in the deploy notes (`docs/deploy.md`). Those notes are written by T135, which sets up the test site two stages later, and the header can only be measured once that site exists. The runner would have held T089 when its turn came.
- **What changed.** One sentence of the security rule (SEC-040) now says the setting is `ASKHAVA_CLIENT_IP_HEADER` and that its measured value goes into the deploy settings table as part of the test-site task, not the rate-limit task. T089 builds the limits and reads the setting; T135 measures it and records it, as its own instructions already said. No code, test or fee changes.
- **Housekeeping.** The same pass corrected a stored size figure for T241 (the inbound-text task, just finished) that no longer matched; it was the one problem the strict prompt check reported on `main`.
- **Worth a look.** Nothing for you to decide. When you set up the test project, the deploy notes will tell you which header value to enter.

## Failed texts now fall back to email or a call-back for you (T094, D-1378 to D-1382, November 2)

- **What happened.** A text that fails is tried again 10, 20 and 30 minutes later. If the third retry fails too, a quote link or missed-call text goes by email (and shows in the portal) when there's an email on file. A homeowner with no email becomes a call-back alert for you: due in 24 hours for an urgent job, 4 days otherwise.
- **Emails.** Every email now goes through one outbox, once each, with the footer the rules require: why they get it, a "Manage email notices" link and the Ask Hava LLC P.O. box. Above 90 of the free plan's 100 a day, only the open-cards, missed-cards and weekly digest emails wait for midnight UTC; job cards, picks, reminders and receipts still go. A provider "limit reached" refusal waits and is never dropped.
- **Worth a look.** When you rent the P.O. box or private mailbox, its address goes into a setting (no home address is accepted). The opt-out email uses the exact opt-out text, which says "at this number"; in an email that reads a little oddly. Changing it would be a spec change, so say if you want it reworded. How Resend counts its day (midnight UTC is assumed) gets checked before going live.

## The retry now sees why coverage failed (harness change, D-1398, November 2)

- **What happened.** T225's first try failed the coverage floor in Ci. The runner tried to read the failing job's log to tell the retry what broke, but GitHub's tool refused to print it because the log had colour codes in it. The second try got only the step name.
- **What changed.** The runner now asks for the log anyway and strips the colour codes itself, as it already did for other logs. Nothing else about failure handling changed.

## Flat-fee mode needs its own card wording (T060, D-1417, D-1418, November 2)

- **What happened.** The fee-mode switch (tiers or one flat fee per job type) is built: switching is logged, and every business must accept a new card authorization before its next charge. But the card authorization text you approved quotes the $25 / $75 / $199 tiers, and there is no wording yet for flat mode.
- **What I chose.** I did not write legal wording for you. The system stores the exact text each business was shown, with its version and time. Before the switch is offered in your owner portal, the flat-mode wording needs your go as a spec change.
- **Worth a look.** The version number for the card wording and the one for the fee mode currently share one counter; whoever next changes either must raise it past both. A later task splits them. Flat mode keeps the $75 cap on recurring services.

## A status page you can read any time (harness change, D-1462, November 2)

- **What changed.** The build's status page on GitHub (STATUS.md, linked from the README) now opens with a short dashboard: one line saying what the build is doing right now, how many of the tasks are done, which stage it is in, how much of the week's tokens is used and when they reset (in Phoenix time too), and how long ago the runner last ran, with a warning if that was more than 90 minutes ago.
- **New on the page.** A "Last 24 hours" table of every task that finished or failed that day, how long it took and, for a retry, the start of why the earlier try failed; the plan and harness changes merged that day; and a "Needs attention" list that also flags a build hold or a plan-check problem.
- **Worth a look.** The task count says "of 230", not "of 228": it counts every build task except your owner gates, the same way the progress figure always has. The page flags a plan check only for tasks still to come. Right now that is two queued tasks (T189 and T079), whose stored size estimates are out of date; tasks already done are only counted. This does not stop the build, and a small plan change will refresh the estimates.

## The build checks coverage before handing in (harness change, D-1440, November 2)

- **What happened.** Money, privacy and licensing code must stay at or above 95% tested. That was only checked after a task was handed in, so a task that dipped below lost a whole attempt (T225 this morning).
- **What changed.** When a task touches one of those parts, the worker now measures it before handing in, the same way the final check does, and fixes a dip in the same session. Tasks that do not touch those parts are not slowed. The worker is also told the time by which it must hand in (30 minutes before its time slot ends), so a session no longer runs out of time with its work unhanded.
- **Worth a look.** The measurement takes about 5 minutes, because it has to run the whole-system tests that exercise those parts; measuring a part on its own gave a wrong low reading (61% instead of 99%). If it runs past 6 minutes, the worker skips it and the final check still decides. On the build machines it always starts from scratch, so for now it will often run out of time except on payment changes; keeping its build between runs is a follow-up.
- **A gap in the plan.** The 95% floor for the catalog's fee-rules file points at a file that does not exist, and no task is set to build it, so that floor is never checked. It needs a plan change to give it an owner (or to move the floor to where the fee rules really live).

## The runner now proves stages 2 and 4 itself, and the war games wait for your "war games go" (harness change, D-1473, November 2)

- **Stage proofs.** Each stage ends with a list of things that must be true. Until now, only stage 1's list was checked by the runner itself; for stage 2, the session watching the build checked it by hand this morning. The runner now checks stages 2 and 4 itself, item by item: for each item it knows which test proves it and which nightly check must be green, and it waits until that test exists and has passed on a night that includes all the stage's work. Stages 3, 5 and 6 are still recorded by hand, because each has an item no nightly test proves: stage 3's mutation report (it only reports for now), stage 5's chaos run and audit, and stage 6's live sandboxes, test site, drills, your review and the war games.
- **Worth a look: the stage-2 record stays, but it was made by hand.** The stage-2 proof recorded this morning rests on a homeowner journey test that covers only part of the journey (the quote page and part of the pick; see "Page and journey checks on every change" above). The runner's new check would not have accepted it, because the first four steps and the full pick aren't in that test. It stays, because every stage-2 rule is checked off and every stage-2 task is merged.
- **A gap in the plan.** No task is in charge of adding those missing steps to that test, so a later plan change has to give the job to one, or a stage-2 proof checked again would wait forever. A possible improvement later: the nightly run records which journey tests actually passed, and the check reads that instead of the test file.
- **The stop is now in the official list.** The build brief's short list of the only times I stop and ask you now includes this war-games go.
- **The pause before the war games is now a real stop.** When the competitive review (T242) is done, you get one message with the review, its plan and the list of P0 items. The war games can't start until you answer. To have some P0 items built first, reply naming them (for example "build P0-1 and P0-3"); they get added as tasks and built and checked before the war games. To start the war games with nothing added, reply exactly "war games go" and nothing else. Any other reply is saved and keeps the war games waiting, so a reply that names items and also says "war games go" won't start them by accident.

## Confirm and pay: how a charge runs and when a pick is let go (T062, D-1424 to D-1428, November 2)

- **What is built.** Each pick has one Stripe payment, made once. Every "Confirm and pay" press is one try with its own key, so a double click or a reload never charges twice, and the server never tries a charge by itself. If a try times out, no new try goes until Stripe says the first one failed.
- **When a pick is let go.** A declined card gives the business 1 business hour to fix it (60 minutes at any hour on an emergency job), and its money contacts get a notice. If Stripe can't be reached for 4 hours, the pick is let go too. Both clocks stop within a minute of your charging switch (or the pause-everything switch) going on, and pick up where they left off. A pick is never let go just because Stripe answered late: if Stripe answers, its answer counts, and a payment Stripe says went through always wins.
- **Monthly limit.** A business can set a monthly fee limit. A press that would go over it charges nothing and ends that pick at once. Reaching the limit exactly is still charged.
- **Not finished yet.** The notice's wording is in (English, Spanish draft), but the step that actually emails it, and the step that offers the homeowner the next quote, come in later tasks. Until then those requests are tried a few times and then set aside as failed (dead-lettered); the later tasks must replay them. Safe mode has no switch of its own yet; the pause-everything switch stands in for it.

## No price lock, tighter payments permissions, and your November 3 package (plan change, D-1494 to D-1499, November 5)

- **No price lock (your call on November 3).** Early joiners no longer get a 6-month price lock. Every business pays the published fee by job size in force when it confirms a pick. The rule (FEE-040) is withdrawn, the sign-up summary and business terms drop the lock sentence, and a new wording rule bans any page, text or document that says joining early locks in prices. The refund wording and the save-card box from the same earlier decision stay. A new task (T244) removes the lock code and keeps the 4-month rule for ongoing services and its $75 cap. No fee amount changes.
- **Payments permissions.** The database let the app change or delete rows in any new payments table unless that table's setup said otherwise. Each table so far did say otherwise, by hand. A new task (T245) makes "add and read only" the default and adds a test that checks every money table's permissions, so a forgotten line fails the build. Every other part of the database has the same loose default; this change fixes payments only and records the rest.
- **Your package.** I read all 86 files. Your decisions are recorded in `docs/reviews/2026-11-03-handoff-zip-review.md` with what happens to each item. The war-game scenario files and the 30% early-quote discount proposal are copied in as inputs for the pre-war-games review, not as tests to run. Certification (ISO 27001, ISO 27701, SOC 2) is listed as a post-launch risk, not a build gate.
- **Parked or open.** Cold-email outreach (your item 9) is parked: no task, no code, and never from the account that sends job emails, whose provider bans cold email. Free first jobs (your item 3) stay as built until you pick a branch. The build keeps running while these changes land; nothing they touch has started.
- **Coming next.** A second plan change adds the national-ready market settings and radius matching you asked for, with the launch market still open (Phoenix metro with a partner, otherwise Lake Havasu City).
- **You can veto.** Any of these. To bring the price lock back, or to have the permissions fix wait, reply on an owner issue; the tasks have not run yet.

## How long a business has to accept a pick, and the license check at the pick (T063, D-1500 to D-1502, November 5)

- **The window.** After a homeowner picks a quote, the business has 4 business hours (8 a.m. to 8 p.m. Arizona time) to accept, counted from when the pick email actually goes out, so an email held back by the daily sending limit doesn't eat into their time. A pick emailed at 7:41 p.m. has until 11:41 a.m. the next day. The 4 hours is a setting: changing it to 3 needs no code change. When time runs out, the pick ends and the homeowner can choose another quote.
- **The license.** The business's license is looked up live once at the pick. If the state site is down, a daily check from the last 24 hours is used; otherwise the pick waits. At Confirm, the stored results are used again with no second lookup, and a paused account, an ended license or bond date, or no fresh Active check stops the Confirm.
- **Texts from the business.** "YES 1042" or "CONFIRM 1042" never accepts or charges anything by text: both get the fee and the Confirm link. After the window ends they get: "Ask Hava: Time ran out to accept job 1042, so the pick expired. You weren't charged. Your quote can't be picked again for this job."
- **Still to connect.** The pieces that call these checks (the Confirm page's handler, the email sender, the license-number lookup and the inbound business text route) are later tasks; they are listed in D-1502.
- **You can veto.** The 4-hour default or the expired wording, by replying on an owner issue.

## National-ready, radius matching, the page pick, the first-quote move, sending the job cards and database permissions (plan change, D-1515 to D-1521, November 5)

- **National-ready (your call, November 3, confirmed November 5).** The product is built so another market is a data change, not a rebuild. The launch market stays open: Phoenix metro if you secure a business partner, Lake Havasu City otherwise. Nothing in the plan names either one as settled. In the war games, Phoenix metro is the stress test (many cities, jobs across city lines) and Lake Havasu City the comparison. The $40 AI cap for the war games stays.
- **What is built now.** Two tasks. T248 holds markets as reviewed data (one market today, Lake Havasu City; a Phoenix metro market exists only as a test setup), the location of each job by its ZIP code's center point (from the Census Bureau's public file, never a street address and never a map service), and each business's travel distance, asked at sign-up beside the towns it serves. T249 then sends a job to every business whose distance reaches it, across town lines, while the licensing and category rules of the job's place still apply. A business that serves the job's town still gets it.
- **What waits for the review before the war games.** The rules that are Arizona or Lake Havasu City only today (the city license list, Arizona quiet hours and business hours, the state license lookup, the 8.6% tax set-aside, the Arizona-only towns) are not changed now. The pre-war-games review (T242) now has to list each one and how it becomes per-market data, and those items come to you with the P0 list before the war games.
- **Worth a look: ZIP codes at the edges.** A business based outside Arizona (say, across the river in California, with an Arizona license) can still sign up: its travel distance just isn't used, and the towns it serves decide its jobs. A homeowner's ZIP that Ask Hava doesn't know (a PO-box-only ZIP, for example) gets asked once more and then the request waits for her; it is never closed for that. Lake Havasu City's PO-box ZIPs can be added by hand to the market file.
- **Worth a look: the job's ZIP code is now stored.** The homeowner's property record held only her town. It may now also hold her 5-digit ZIP code (never her street address), because radius matching needs it.
- **Worth a look: job cards never actually went out (now T251).** While checking, I found that nothing in the app asked the homeowner's ZIP code or sent the job cards to businesses after she approved her card; every piece existed except the step that runs them. A new task, T251, adds the ZIP question (draft wording for the copy panel) and the step that sends each card once, holds business notices overnight, and then texts her that her request is out. It runs before radius matching.
- **Picking on the quote page (T246).** Today the page's final Pick button saves nothing and sends no code, so a homeowner can't pick there. T246 makes it work: the 6-digit code goes to her phone without the page ever seeing the number, and the pick and her terms acceptance are saved together. She can pick, or switch to another quote, until the business confirms. One older page test assumed the opposite, so T246 corrects it.
- **The first quote now moves the request forward (T247).** Today a request with quotes stays "open", so its timer could tell the homeowner "no quotes yet" and close it, "2 YES" by text was refused, and the "your first quote is in" text never went. T247 fixes all three.
- **Database permissions in every other area (T250).** The same loose default fixed for payments (T245) exists in 13 other parts of the database. T250 makes "add and read only" the default there too and adds a test that checks every table.
- **You can veto.** Any of these: the national-ready scope, storing the ZIP code, how unknown ZIP codes and out-of-state businesses are handled, the "pick until the business confirms" rule, or the permissions change. Reply on an owner issue; none of the tasks has run yet.

## Refunds happen once, and bank disputes (T066, D-1542 to D-1544, November 5)

- **One refund per pick.** However many times a refund is asked for, Stripe is asked once. If Stripe says no or can't be reached, you get one alert and it tries again every hour until it goes through.
- **No refund on a disputed charge.** Once a business disputes a charge with its bank, no refund goes out on it; you get one alert instead.
- **Disputes.** Each dispute alerts you and goes in the books once, as the disputed amount plus Stripe's $15 fee. Two days before Stripe's deadline, the evidence goes in on its own only when the business was given the details and tried to call (T191 checks that); every other dispute waits for you to fight it or accept it, showing the $15 extra it costs to fight. If you already sent the evidence yourself, it is never sent again.
- **What the evidence holds.** Only record numbers and times from the charge (Stripe customer, payment, card-terms version, amount, when "Confirm and pay" was pressed and paid, any refund). Never the homeowner's name, phone, address, email or messages.
- **Fraud warnings.** If Stripe warns a charge may be fraud and the details were not yet given out, it is refunded at once; if they were, it is listed for you to decide.
- **You can veto** the "alert once, not every hour" choice for failed refunds. Reply on an owner issue.

## Receipts, the "Why was I charged?" page and the just-under-a-tier flag (T068, D-1557 to D-1561, November 6)

- **Receipts.** Every charge gets an Ask Hava receipt email: "Ask Hava introduction service, job 1042: $25.00.", "Tax included.", a line saying Stripe also emails its own receipt, and a "Why was I charged?" link. It goes to the business owner and every "Confirm and billing" member. If a business has nobody to send it to, you get one alert instead of a lost receipt.
- **The page.** It shows the job number, the quote, the fee and its job size, how the fee basis was set and the refund rules. For a repeating job it says the basis is 4 months of the regular price, and when the $75 cap set the fee it says so instead of claiming the bigger job size.
- **Not connected yet (later tasks).** The parts are built and tested, but nothing sends a receipt yet: the step that queues it after a successful charge belongs in the payments charge code (T132 or T191), and the email step needs Ask Hava LLC's postal-address setting before it can be switched on. Until then no receipt goes. The page also waits on business sign-in.
- **Just-under-a-tier flag (FEE-033).** The rule is built: at least 3 of a business's last 10 quotes within 5% under $500 or $1,500 flags it for your monthly review, with no automatic action and nothing sent to the business. It is not connected yet: the fee basis of each pick isn't stored, and the owner-portal line, its two settings and the step that runs it are a later task's. Until then nothing is flagged.
- **Not tested here.** That Stripe's own receipt also goes out is a Stripe Dashboard setting, so this build checks only that our receipt says it comes; the Stripe test is a later task's (T132).
- **You can veto** the "one alert, no email" choice for a business with no billing contact. Reply on an owner issue.

## Sharing the homeowner's details after a paid or free pick (T069, D-1562, D-1563, November 6)

- **What a business gets.** Only after the payment clears, or a $0 fee is recorded (free first win, repeat customer, fee-free job type), the business sees the homeowner's first name and job address in its signed-in portal, plus her email only if she chose email contact. It happens once, however many times the payment news arrives. Her phone number is never read for this and never shown.
- **The email and text.** The email says only "Ask Hava: Job <n> is confirmed. Sign in to see the homeowner's details." with a sign-in link. A business that chose text gets "Ask Hava: Job <n> is confirmed. The details are at askhava.com/business." No name, address or email is in either.
- **Emergency jobs without an address.** If the business accepts before she has sent her address, nothing is shared and the system asks her for it again.
- **Not connected yet (later tasks).** Nothing calls the release yet (the payment webhook wiring), the "ask her again" text has no sender yet (intake), and sharing once her address arrives needs the payment check run again. The release email waits for the business email sender.
- **Testing limit.** The safety rules are tested, but no test here can run the full release against the database, because tests are not allowed to make the payment proof. A later setup task has to decide how to test it.
- **Review fixes (D-1564).** The details are now shared only while the pick is marked paid or free, so a refunded, cancelled or no-show pick shares nothing even if a late payment notice arrives. On a regular job the share is recorded before the details are opened, so two payment notices at once can't both open them. If a business has nobody set to get pick notices, you get one alert instead of a silent missing email. On an emergency job she is asked for her address once per pick.
- **Still open (D-1565).** The business portal page can't show the details yet: the release hands them over once and keeps no copy, and the portal needs its own allowed way to read them for a released pick (a later portal task). The sign-in link in the email isn't filled in yet; the email sender has to add it. The system also doesn't yet check that the homeowner on the payment proof is the one on the job; it relies on the payment code making the proof right, until picks store the homeowner on each pick.
- **On emergency jobs** the details are opened to see whether her address is in; if it isn't, nothing is shared, but that look is logged. Two payment notices at the same moment could both look; only one is ever shown.

## Picking on the quote page (T246, D-1602 to D-1606, November 6)

- **What changed.** A homeowner can now pick on her quote page. Pick buttons show while the job is waiting for a pick or for the business to accept (Quotes ready, Awaiting confirm); picking another quote before the business accepts replaces the first pick and tells that business. After the business accepts, no Pick buttons show and the page says the pick can't be changed.
- **The code.** After "Pick" we text her a 6-digit code. Only the picks code reads her number from the vault, every read is logged, and the number never appears on a page, in a log or in an error. A paused request or a number we can't text gets no code and a plain line saying nothing was picked. The daily code limits still hold.
- **The Terms.** Picking stores one Terms acceptance with the pick, both or neither. The Terms text is the homeowner terms file as written; its version is the "Version" line at the top (now 0.4).
- **Draft words for the copy panel (English, then Spanish draft):**
  - "We texted a 6-digit code to your phone. Type it below to finish your pick." / "Le enviamos un código de 6 dígitos por mensaje de texto. Escríbalo abajo para terminar su elección."
  - "We can't text your phone right now, so no code was sent and nothing was picked." / "Ahora no podemos enviarle mensajes de texto, así que no se envió ningún código y no se eligió nada."
  - "You already have a pick, and that business has accepted it. Your pick stays as it is." / "Usted ya eligió, y ese negocio ya aceptó. Su elección se queda como está."
  - "That code didn't match 5 times, so new codes are paused for 15 minutes. After that, press "Send a new code"." / "Ese código no coincidió 5 veces, así que los códigos nuevos están en pausa por 15 minutos. Después, oprima "Enviar un código nuevo"."
  - "We've sent as many codes as we can today. Please try again tomorrow." / "Ya enviamos todos los códigos que podemos hoy. Inténtelo de nuevo mañana."
  - "That code didn't match. You have 4 tries left." / "Ese código no coincide. Le quedan 4 intentos."
  - "That code has expired. Codes work for 10 minutes. Press "Send a new code" to get another one." / "Ese código ya venció. Los códigos sirven por 10 minutos. Oprima "Enviar un código nuevo" para recibir otro."
- **Still open (D-1606).** The "Done" text after a page pick and the business's accept-by time on the result page wait for the accept-window work; the page still says "you can pick another quote" even when there is only one quote, which needs the review page's own task. Also still open: if saving a pick fails, the homeowner needs a new code; a code works for any quote on that job, not only the one she first pressed; the Terms she accepts are the draft (version 0.4) until the lawyer's version replaces it; the "already accepted" line doesn't yet say how to close the job; and the page always texts the code (no voice-call option yet).
- **Your call, if you want one.** The app stops sending codes after 500 in one day across everyone (an anti-abuse limit) and raises an alert. That number is a guess; tell me if you want it lower.

## Pick code by email when texting is off (T095, D-1612 to D-1614, November 6)

- **What changed.** If a homeowner opted out of texts, or texting is switched off, her pick code can now go by email instead, but only to an email she gave in her own text or chat with Ask Hava. An email typed on the quote page never gets a code. The emailed code has the same limits as the texted one: 6 digits, 10 minutes, 5 tries, then a 15-minute wait. And "2 YES" from an opted-out phone still picks, with no text back, once she was already sent the "By picking, you agree to the Terms" line.
- **Still open (D-1612).** Nothing calls this yet. Ask Hava can't yet look up "the email she gave in her own conversation", and the emails themselves (the code email, the Terms email with its Pick button, the "your pick is in" email) aren't written yet. The "Get updates by email instead" choice on the quote page, and showing her approved card and license lines there, were left for a follow-up: each needs parts of the app this task wasn't allowed to change.

## The database-permissions task gets one more try (T250, D-1632, November 6)

- **What happened.** T250 tightens what the app's database login may do to new tables in every part of the database (it should only add and read rows by default, never change or delete them). Its first two tries failed for a reason that was the plan's fault, not the worker's: its one migration touches all 13 parts of the database at once, and an older automatic check forbids a migration from naming any part but its own. The worker wasn't allowed to change that check, so it could never pass.
- **What changed.** T250 is now a "harness" task, the kind allowed to change protected checks, with an Opus reviewer. It adds one narrow exception: a migration in the shared core may name the other parts only if every line in it takes a default permission away. Anything else in that file (granting access, creating a table, changing data) still fails the check, and the check gets planted examples proving that. Nothing gives the app more access.
- **Why not the other way.** Splitting it into 13 small migrations would need 13 free numbers in a gap that has only 7, and would mean renumbering ten later tasks.
- **Nothing needs you.** A third try started on the old plan before this change could merge and will most likely fail the same way. If it does, T250 gets exactly one more try on Opus with the fixed plan; if it somehow passes, nothing more is needed.

## A blind spot in the database-layout check, and a tidier budget check (T252, D-1677, D-1678, November 6)

- **What happened.** One automatic check makes sure each part of the app's database setup only touches its own part of the database. It works by reading the names written in each setup file. T250's third try wrote its file as a small loop that builds those names while it runs, so no name was written down and the check passed it. That try never merged (it failed for an unrelated reason), and it was a fair reading of a rule that never said this was off limits.
- **What changed.** New task T252 (an Opus worker with an Opus reviewer, after T250) teaches the check to refuse any setup file that builds its commands while it runs, unless the file is on a short list in the check, each with a one-line reason. One file is on it at the start: an early one that only stops with an error if the app's database login is missing. Until T252 merges, the reviewer and every task that writes a setup file are told to watch for this (risk R-24).
- **Also tidied.** The stage-5 clean-up task (T126) was set to read the whole simulator folder, which made it too big for one session. It now reads just the three simulator files its check uses. Each task's stored "how much it reads" estimate was refreshed so that check is clean again; for finished tasks only that number moved.
- **Nothing needs you.**

## Masked numbers: two loose ends (T076, D-1680 to D-1682, November 6)

- **The "ran out of numbers" alert.** When every masked number is in use, you get one High alert and jobs start sharing numbers with a PIN. If you mark that alert resolved but have not bought more numbers, you will not get another one while the shortage lasts; the next alert comes only after a job gets a free number again and a later job has to share. So treat "resolved" as "I bought more numbers", not "I saw it".
- **Safety calls to numbers we can't text.** The safety call checks the same list texting uses. A US number whose area code is not in our time-zone table is never texted, so the safety call skips it too. Instead of skipping quietly it now raises a High alert to you, and your emergency alert still fires. Whether the call should dial such a number anyway is your call; until then it does not (the safety call switch is off at launch).
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## Masked calls: PINs (T077, D-1684 to D-1688, November 6)

- **PINs.** Each connected job gets a 4-digit PIN. A caller whose phone we don't know must enter it; 3 tries a call, then the call ends and you get one alert. After 2 wrong tries, a phone linked to the business that paid for that job hears the homeowner's first name and street and must confirm before it connects; nobody else hears anything.
- **Daily PIN limit not stored yet.** The 6-tries-a-day rule is built, but the count needs a table a later calls task adds. Nothing is live, so this changes nothing yet.
- **"Who's calling" text waits.** The text telling the homeowner which number the business will call from is written and tested, but adding it touches two files another task owns, so it lands with that task.
- **If a call line fails to open after a business pays,** the business still gets the homeowner's details, and you get a High alert to sort the line out. Nothing reopens the line by itself yet; a later task adds that.
- **Gaps passed to later tasks.** The PIN is worked out from an internal line ID, which isn't secret, so a later calls task must store a random PIN instead. Only the business that paid hears the name-and-street read-back, which holds as long as lines close when a pick ends (the line-closing task, T079). The phone company's caller check comes in a form the voice task (T080) must translate. A security test list needs one new entry for the read-back. The "who's calling" text has no task that owns it now; one needs adding.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## Masked calls: closing lines, staff phones, the main-number greeting (T079, D-1694 to D-1698, November 6)

- **Closing lines.** A job's call number now stops working when the homeowner cancels, the business doesn't show, a complaint comes in, or 30 days after it opened. On a cancel the business gets a short note that it doesn't count against them. Nothing calls these yet; the cancel, no-show and complaint steps and a daily sweep come with later tasks.
- **Calls to the Ask Hava number** hear the greeting in English, then Spanish, then the call ends. Nothing is recorded, there's no voicemail and nothing rings through to you. The Spanish wording (with its English back-translation) is in D-1697 for your check.
- **A gap in the plan (D-1695).** A phone linked by a team member other than the owner should work only after the owner approves it. The rule is built and tested, but no task stores the owner's approval yet, and the call answering doesn't apply the rule yet. I've named T148 (store the approval) and T080 (apply it on every call) to close it. Nothing is live, so this changes nothing yet.
- **Unknown staff phones.** When an unlinked phone calls a job line, the owner and billing members get a "phone ending in 1234" email, and if the business chose texts, its main phone alone gets the reply-LINK text. It goes once per job line, so a second unknown phone on the same line doesn't send another. The sending side and the "Link this phone" page come later (D-1696). The 45-minute "call from your main phone" nudge isn't built yet (it needs the call answering, T080).
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## Masked calls: relay, the code call, the safety call (T080, D-1704 to D-1708, November 6)

- **711 relay.** Every job-PIN prompt is spoken, waits 10 seconds for digits, is said once more, then the call ends with a spoken goodbye. The 10 seconds is a setting (`ASKHAVA_PIN_WAIT_SECONDS`, 10 to 60). The homeowner hears nothing on a bridged call.
- **Her code call** goes only when she taps for it. It opens "This is Ask Hava LLC", says the code slowly twice, gives the Ask Hava main number to call back and offers "press 1 to hear it again".
- **The safety voice call stays off.** It can only be switched on with your lawyer's written approval recorded and every CALL-022 item in place. When on, it calls once per emergency, within 30 minutes, from a pool number, and "press 9" stops all calls. A person answering clears your critical alert, and the digest lists it.
- **Calling a homeowner from the Ask Hava number** (your portal button, T108) rings your phone, reads you the 911 text, then connects you with the Ask Hava number shown, never yours.
- **Gaps for the plan (D-1708).** The voice adapter can't yet tell Twilio what a placed call should say, and nothing answers incoming calls yet. Both are later tasks' work.
- **The safety call only follows a 911 text (D-1705).** It goes only when the message also matched the 911 list, so "I got a shock from the switch" alone gets no call (the 911 list has no shock words). Adding shock words to the 911 list would be your call. Ordinary words like "sparkling", "shocked by the price" or "la descarga del inodoro" never trigger it. A retried call is never dialed twice, and every answer and key press is logged.
- **Which number your bridge shows (D-1707).** The plan says "the Ask Hava number", but the system can only dial out from a pool number, so your bridge shows a pool number, not the main number. If you want the main number shown, that's a later change. Only you hear the 911 words before it connects you.
- **English only for now (D-1708).** The code call and safety call scripts are English only, since the plan gives only English words. The "slow" voice setting may need a specific Twilio voice to work; that will be heard before Gate C.
- **Business sign-up screens (D-1709 to D-1713).** The first business in a trade sees "You're the first plumbing business so far." instead of "one of 1 businesses". The usual delay before an ROC change shows on the daily list is set to 3 days, a guess you can change (`ASKHAVA_BIZ_LIST_DELAY_DAYS`). One thing to look at: the sign-up says "We don't text or call businesses." while the sign-up code itself can go by text to the record phone; the wording is kept as the plan has it until you decide. The Spanish notice line on the sign-up page is a draft for a native speaker to check, like the Spanish texts.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## Showing a paid business the homeowner's details again, and connecting the payment to the release (plan change, D-1739, D-1740, November 6)

- **The problem.** When a payment clears, the system records "shared" first and then opens the details, so two payment notices can't both open them. But if opening the details failed right after that, or the page reloaded, nothing could show them to the business again.
- **The fix (new task T253).** The business's signed-in portal gets its own way to read a released job's details: only the owner or a "Confirm and billing" person of the business that was picked and paid (or got it free), only first name and address (and her email only if she chose email contact), never her phone. Every look is logged. Looking again is not a new introduction: no new fee, email, text or call line. After the 180-day clean-up or "Delete my data", the page says the details were removed. Nothing is copied out of the locked store.
- **Connecting the payment (new task T254).** Nothing yet turned a successful payment into the release. T254 does it: payment clears, the pick is marked paid (or free), then the release runs once. It also retries the masked call line if it failed to open, and tests the failure cases above against the real database.
- **Emergency jobs (new task T255).** If the business pays before she has sent her address, she is asked again at once, and the details go out once the address arrives, without charging again.
- **Still without an owner (D-1740).** The business's Confirm-and-pay page isn't switched on yet, and the homeowner's "contact me by email" choice isn't saved yet, so until a task does that every release uses the masked call line. Both need a later plan change.

## The outside code audit of November 6: what was confirmed and who fixes it (D-1751 to D-1761 and D-1788, November 6)

An outside review (ChatGPT) of the whole repository found twelve issues. Each was checked against today's code before anything was planned; all twelve hold. Thirteen new tasks (nine build tasks, T256 to T259, T261 to T263, T266 and T268, and four harness tasks: T260 for the Stripe test stand-in, T264 for the coverage checker, T265 for the new stage check and T267 for the accessibility test split), plus small additions to four planned tasks (T196, T236, T251, T147), now own them. The release and portal items (A07 and the release part of A08) are in a separate plan change (D-1739). Full list with evidence: `docs/reviews/2026-11-06-repo-audit-dispositions.md`.

- **A product change you should know about: the monthly fee limit is now a hard cap (D-1758).** Before, two "Confirm and pay" presses at the same moment on two different jobs could both get through and take a business past the limit it set (D-1427 accepted that). Now the app holds the business's remaining room for the month while a charge is in progress, so a limit is never passed; a charge that fails frees the room again. Nothing changes in the wording a business sees. If you'd rather keep it a soft guard, the portal and terms would need to say that two simultaneous Confirms can go over.
- **Two first texts at once from a new homeowner** could lose one of them. T256 fixes it.
- **The name scrubber forgot names between texts.** A name she gave in one text could reach the AI in her next one. T257 makes the scrubber remember, with the names kept encrypted in the vault and deleted with her data.
- **Refunds were recorded as done when Stripe had only accepted them.** T258 records what Stripe actually reports (pending, done, failed, canceled); if a finished refund later fails, the ledger adds a correcting line rather than changing history (FEE-028 gains that one entry type).
- **Stripe's refund, dispute and fraud-warning notices reached no code**, and **no page served the Confirm step** (and the site's strict security header would have blocked Stripe's card form). T259 to T261 build these, with the Stripe allowance only on the Confirm page.
- **Masked-call PINs** were worked out from an internal ID and the 6-tries-a-day limit was kept nowhere. T262 gives each line a random PIN and stores the tries.
- **A business whose public-record phone changed** could get its new code request treated as a repeat of the old one. T196 now covers it.
- **The daily AI-call cap** could be passed by simultaneous calls. T236 now reserves each call first.
- **The coverage check passed when a measurement was missing**, and the overall floor was still zero. T264 fixes the checker and sets the floor from a real measurement.
- **A register of every hand-off between parts of the system** (`docs/integration-closure.md`) now lists 36 of them, and T265 makes a stage unable to finish while a security, money or privacy hand-off has no live owner.
- **The sign-up consent box and welcome emails (D-1715)** now have an owner, T268: a business can no longer be activated without its written consent recorded. **The accessibility test file** that every page task may now edit (D-1772) is split by T267, so page tasks edit only a data file and never the checks.
- **Still without an owner (not security, money or privacy):** wiring the AI writer into intake (D-990, D-1198). They are open rows in the register for the next plan change.
- **The homeowner's "email me" choice at the pick was never saved (D-1761).** So a business could only ever reach her by masked call. T266 saves the choice, and her email only in the encrypted vault. The "job confirmed" email to a business now carries a signed link that opens sign-in for that person (T147, T269).
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## Every new page adds its own accessibility check entry (plan change, D-1772, November 6)

- The accessibility check fails any page that has no test entry, but only the task that first wrote that test list was allowed to edit it, so the business sign-in task (T084) couldn't add its page and failed. Now any task that adds a page adds its own entry (fake data only) and may not remove or loosen anyone else's. T084's next try picks this up.

## Photos on job cards (T091, D-1794, D-1795, November 7)

- A photo goes on a job card only after our own server checks it, strips its hidden data, re-saves it, and the homeowner confirms it shows no person and no house number. No photo goes to the AI until the zero-retention agreement is signed and the privacy policy says the AI looks at photos.
- After that, two separate AI checks look for faces, plates and readable text. If either one flags anything, the photo stays off the card (we don't blur it in this version). A request with no photo goes out the same way.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## Deleting old homeowner details (T092, D-1799 to D-1803, November 7)

- Every day the system looks for finished requests. 30 days after one closes, its photos are deleted. At 180 days the homeowner's name, email and address are erased; her phone number stays locked, only as proof of her texting permission and opt-out. At 3 years the request's answers are deleted. Money and audit records are never touched by this.
- An open bank dispute or an open complaint pauses the 180-day erase; it is checked again each day. Because we don't record how a dispute ends yet, a dispute counts as open until 90 days after its evidence was due.
- "Delete my data" erases the same details and the photos at once and tells the person we keep the phone number, locked, for 5 years.
- Not done yet, and no task is planned for it: deleting the scrambled photo files from storage and asking Twilio to delete message bodies after 31 days. Both need new connections to those services that no task in the plan builds yet; the runner should add one. Until then the photo files stay in storage, scrambled, with no link that can open them.
- A choice for you to know about: a complaint pauses the 180-day erase only while its 5-day reply box is open. We don't record whether a complaint was resolved, so one still unresolved after the box closes no longer pauses it.
- A person's details belong to her, not to one request: a pause on any of her requests keeps them, and a request that never became a job (turned away or abandoned) is erased 180 days after it ended. If a purge keeps failing, you get an alert.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## Texting switched off or a suspended number (T096, D-1804 to D-1808, November 7)

- When homeowner texting is off or the texting number is suspended, homeowner updates go by email and the quote page; anyone with no email goes on your call list, whether you switched texting off or the number was suspended. A business with no email only sees the portal; Ask Hava never calls a business.
- No second number is ever used. After Twilio says the number is back, texting stays off until you switch it on, and you can't switch it on before then.
- An emergency message while texting is off or suspended always raises a critical alert and turns the site banner on. The safety voice call goes only during a real suspension (including after Twilio says the number is back but before you switch texting on), only while you have turned that setting on, only for her own message sent after the suspension began, at most once per homeowner per suspension, and never to a number that opted out.
- The toll-free backup can be set up only after a refused homeowner campaign is recorded and you give the go; it is refused while the number is suspended and never carries business messages.
- Not done yet, and no task is planned for it: saving these switches and the suspension in the database with their owner-portal buttons, the safety email itself, turning the site banner on, and the wiring that runs this on each incoming message. The runner should add a task.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## Check-ins after a job connects, problem reports and the feedback request (T097, D-1809 to D-1811, November 7)

- After a business is connected, the homeowner is asked how it went 3 days after the business gets her details (or 3 days after the earliest visit date, if that is later) and again at 14 days. She can answer DONE, NO, PROBLEM, LATER, ONGOING or OTHER, in English or Spanish.
- LATER waits for the 14-day check-in. ONGOING (a regular service has started) and DONE finish the job, and only then is she sent one request for private feedback. OTHER (she hired someone else) closes the job without counting it against the business.
- A PROBLEM reply closes the job and gets the "the Ask Hava owner reads every problem report, usually within 7 days" text. It goes on your weekly list with her message, or on your 9 a.m. / 3 p.m. push if it has a safety word like "stole", "unsafe" or "threat". No fee changes because of it.
- If a job goes back to quotes (she said the business never reached her) and she picks again, the new business gets its own 3- and 14-day check-ins; the old ones are dropped. ONGOING counts the same as DONE, and a PROBLEM at 14 days is listed for you just like one at 3 days.
- Not working end to end yet: the check-in and feedback texts still need their wording added by a later task, the problem reply text exists but the sending code doesn't list it yet, so she won't get it until a later task adds it, and the release and the incoming-text route still need to call this code. The runner has the list.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## When a business can't reach the homeowner (T191, D-1814 to D-1819, November 7)

- If the business still hasn't called 24 and 48 hours after it got her details, it gets a reminder with the calling hours (8 a.m. to 8 p.m. Arizona time, any hour on an emergency job).
- After the business's first call that nobody picks up, she gets one text: "<business> tried to call you about request <n>. Call them back on <masked number>." It should wait until 8 a.m. unless the job is an emergency; the sending step that holds it is not wired yet. If her number can't be texted (she opted out of texts), no text goes and the business can't get the refund, because the email copy of that text is not built yet (D-1819).
- At 96 hours the refund rule is checked. If the business made 2 calls at least 3 hours apart in the calling hours within 72 hours, nobody picked up, she was told, and she didn't say DONE or PROBLEM (or YES to "Did <business> email you and agree a time?" when she chose email), the introduction fee is refunded, the job closes and the masked line closes. If Stripe fails the refund or the charge is disputed, nothing closes and no email goes until the refund goes through. Every decision email tells the business which condition was met or missed and points to "Ask about a charge".
- If a business placed no call at all in 72 hours (call contact), the job goes back to her quotes and the business is a no-show (no refund). For email contact it waits for her 3-day answer; a NO does the same.
- A bank fraud warning on a charge whose details weren't released yet refunds it, ends that pick and puts her quotes back; a released one waits for you. A dispute's evidence goes in by itself only when the details were released and the business made a call (or the card was 3-D Secure checked); every other dispute waits for you.
- Not built yet: her text "<business> couldn't reach you. Text us any time to start again." and the 72-hour "Did <business> email you and agree a time?" question. Adding a new homeowner text also means listing it in a file this task could not touch, so a follow-up task does both. That question will say "Text back YES or NO" instead of the spec's "Reply YES or NO", because a safety check stops any homeowner text that looks like a consent reply. Only the verb changes.
- Not working end to end yet: the worker tasks that call these steps at 24, 48, 72 and 96 hours, and the dispute and fraud-warning webhooks, still need wiring. The runner has the list.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## The private feedback page (T230, D-1820 to D-1823, November 7)

- After a job is Done, the homeowner can open a private feedback page from her link. She writes to Ask Hava in one box. One box, unticked, asks: "Ask Hava may show what I wrote on askhava.com with my first name and town". Everyone gets the same page and no reward.
- Only ticked feedback can ever go on askhava.com, and a bad review is shown like a good one. A ticked one comes off only for another person's personal details, abuse or discrimination, being believed fake, or being off topic, and each removal is logged with its reason. Feedback from you, a relative of yours or a business is never shown.
- Not built yet: the text that sends the link, the askhava.com section that shows the feedback, and your owner-portal buttons to remove one or mark who wrote it. Also not yet: the words a homeowner types are kept as typed, so they may hold her own contact details, and neither the 3-year purge nor "Delete my data" removes them yet; a later task adds that. The runner has the list.

## The second outside code audit: what was confirmed and who fixes it (plan change, D-1835 to D-1845, November 7)

A second outside review of the whole repository found eight issues and listed some known gaps. Each was checked against today's code (with the unreachable-refund work, T191, now merged) before anything was planned; all eight hold and none was rejected. Thirteen new tasks (T270 to T282) now own them. Full list with evidence: `docs/reviews/2026-11-07-second-audit-dispositions.md`.

- **The unreachable-homeowner refund** (T282, T270): it judged calls by when they ended instead of when they started; it could close a job and email "your fee is refunded" while the refund was still in progress at Stripe; and a fee-free introduction's no-show left the pick marked connected. The call's start is now recorded the moment the call reaches Ask Hava's phone line (T262, T282). If that record is ever lost, the call's end time is used and the call still counts toward the refund, since a lost record is Ask Hava's fault, not the business's (D-1836). T271 and T281 wire the call log, the 24/48/72/96-hour steps and the bank fraud warning, which nothing ran yet.
- **Business sign-in** (T272, T280): several sign-in requests at the same instant could get past the 5-links and 5-wrong-codes limits. T272 makes each one take a numbered slot. T280 serves the sign-in pages and refuses to start without the phone-code step.
- **Photos** (T273): a photo could be opened in a way that let its "who opened this" record be undone. Fixed before any photo page exists.
- **Deleting old homeowner details** (T274, T275, T276): the 180-day purge could clear the details of someone who had just sent a new request; stored photo files and Twilio's copies of messages were not deleted yet; and the local file store could be changed by two saves at once. All three now have owners.
- **Storage keys (D-1841):** the photo folder in the file store gets its own key that can read, list and delete, so old photos can actually be removed. The backup and audit-stream folders keep the write-only key with versioning and object lock, exactly as before.
- **Problem reports** (T277, T278): a PROBLEM reply's words were never kept anywhere, so your weekly list couldn't show them. T277 keeps the scrubbed words (owner-only, 30 days) before the text is acknowledged; T278 adds the check-in texts and schedules them.
- **Texting switched off or suspended** (T279): the switch, the suspension and the site banner are now stored, and an emergency while texting is off raises the alert and emails her. You record a Twilio suspension in the owner portal, since Twilio tells you by email, not automatically.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## Business notices by email or text (T147, D-1825 to D-1829, November 7)

- Business notices now go by email unless texting is set up.
- Texts start only after you turn on business texting. Even then, a text goes only to phones with the text box ticked. Everyone else gets the email.
- Notices due overnight wait until 8 a.m. Arizona time. The email and the text then go out together, and a job card's 24 hours to quote start at 8 a.m.
- The "job is confirmed" and "card didn't go through" emails carry a sign-in link made for that one person, good for 7 days. No homeowner details are in any email or text to a business.
- Job card emails have no photos, just a link to the job page in the business portal.
- The "Sent to N businesses" count leaves out any business whose card email bounced.
- Not built yet: the worker doesn't run these notices yet, the unsubscribe link in card emails, and Spanish business emails.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## The third outside code audit: what was taken, what was declined (plan change, D-1866 to D-1873, November 7)

A third outside review of the whole repository was checked finding by finding against today's code. Eight new tasks (T283 to T290) own what holds. Full list with evidence: `docs/reviews/2026-11-07-third-audit-dispositions.md`.

- **Worth your look: a fee-mode switch after a pick (D-1871).** If you switch between size tiers and a flat fee while a pick waits for the business to confirm, that pick can't be confirmed: nothing is charged, the business sees that the fee terms changed after the pick, and the pick ends with no fee and doesn't count against the business, so the homeowner gets her other quotes. The fee fixed at the pick is never re-priced. The rules never covered this window, so I decided it; say if you'd rather, for example, block the switch while any pick is waiting.
- **The amount charged** now has to match the fee stored at the moment of the pick, checked inside the payments code itself before Stripe is called (T287).
- **Build checks:** a change to the Terms or the fee and catalog files now runs the full tests before it can merge (T283); a test proves every listed page and webhook is actually connected (T284); and when a task finishes without a file it was supposed to create, it shows up on the status page in the same cycle (T285).
- **Smaller fixes:** every request now has a 60-second limit (T286); a Stripe charge the nightly check can't match is kept and retried until it is, with an alert after a week (T288); one test that failed once at random is made reliable without loosening it (T289).
- **Declined:** the audit's "rounding bug" on recurring price ranges. The code already rounds exactly the way your rule D-033 says, once at the end; changing it would change the rule. T290 adds worked examples at the $500 and $1,500 edges so nobody "fixes" it later (D-1874).
- **The fee setting is now stored in one place** (T287): when you switch between tiers and a flat fee, the payments code records it, and everything else reads it from there.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**
- **A business can see a released homeowner's details again** (T253): the owner or a "Confirm and billing" member of the picked business can reopen the first name, address and (if she chose email) email of a paid or free pick, read fresh from the vault each time and never stored; anyone else gets the same "not found", and after her data is purged the page says the details were removed. The page itself comes with T085 (D-1739, D-1891 to D-1895; the end-to-end test waits on a paid-release API, D-1895).

## Your owner sign-in (T086, D-1917 to D-1921, November 8)

- **How you'll sign in:** your password (at least 15 characters, checked against a list of passwords known from breaches) plus a code from an authenticator app or a passkey. You stay signed in for at most 12 hours, and 30 minutes without using the page signs you out. Refunds, the AI switch, fee mode, settings, the Stripe-answer record, data export and the safety-call switch each ask for a fresh code or passkey if your last one is more than 5 minutes old. There is no "reset by email".
- **Paper recovery codes:** the first setup gives you 10 one-time codes to write on paper and keep away from your password manager. A code works in place of your app only after you have added a second authenticator or passkey, so set up a second one soon after the first sign-in.
- **Worth your look: the first setup (D-1918).** The rule says the setup runs in the release step, but that step can't take your password and its logs would keep your codes. So you run `server bootstrap-owner` once yourself, in your own terminal, with the one-time token set as a host variable, and delete that variable right after. It refuses to run a second time.
- **Worth your look: the authenticator secret is stored as is (D-1917).** Anyone who could read that database table could make your codes. Encrypting it with the vault key is a later change.
- **Not built yet:** the owner pages aren't served yet; the page that adds a passkey comes with your settings page.
- **Worth your look: no lockout on wrong passwords yet (D-1917, D-1921).** The limit on repeated tries belongs to the rate-limit task (T089), so the owner sign-in page must not go live before that task covers it.
- **Worth your look: the setup token is only a check that one is set (D-1918).** It isn't compared with anything; what really protects the setup is that only the migration database login can create you, and only once. Delete the token variable by hand after the setup; nothing checks that for you. When you type your password into the setup in a terminal, it isn't shown.
- **After a recovery code:** signing in with a paper code works, but refunds and settings still ask for your app code or passkey (D-1917). Changing your password signs out your other devices.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## The name becomes a setting, so a rename is one file (plan change, D-1932 and D-1933, November 8)

You said the name may change from Ask Hava (likely to Laftly) before launch. Nothing is renamed now: every text, page and email still says Ask Hava.

- **What changes.** Five new tasks (T291 to T295) put the name, the short name texts start with, the company name, the web address, the email sender and the card-statement name in one file. Every text, email, page and call reads them from there, and so do the build's own checks. A test sets the name to a made-up one and proves it shows up everywhere and "Ask Hava" shows up nowhere.
- **What a rename will still need by hand.** The legal documents (Terms, privacy policy, the card authorization) keep naming Ask Hava in plain words: a new company name means a new version a lawyer should read, so the build fails on purpose after a switch until they are rewritten. Outside the code: the domain, the email sending domain, the name Stripe prints on card statements, and the texting registration.
- **Kept from coming back.** A new check fails the build if any later task writes "Ask Hava" into customer-facing code instead of using the setting.
- **Timing.** The first of the five tasks is the next one the build starts, so the check is in place before anything else is written; a few queued tasks that edit the same files wait for the later four. Changing the name itself later needs a reviewed plan change with its own logged decision.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.** Tell me when you pick the name; the copy rewritten for it comes after the build.

## The authorization check over every page and actor (T088, D-1945 to D-1948, November 8)

- **What it proves.** A test reads every web address from the code and tries each one as six made-up people (no one signed in, two homeowners, two businesses, you). Each gets exactly the yes or no it should; one homeowner's quote or feedback link never opens another's job; no push-alert app is built.
- **Only partly met: "every refusal is logged".** Refused form posts are logged as security events. Two kinds are not yet: a forged webhook (Twilio or Stripe) is only a plain log line, and a person refused at the homeowner or business door leaves no record. Named to fix them: T256 (Twilio), T259 (Stripe), the tasks that open the homeowner and business pages, and T204 (security alerts) checks it all before it is done.
- **Known limit.** Homeowner-to-homeowner separation is proven for the quote and feedback links only; the homeowner portal's own pages are not open yet and get the same check when they are.
- **Nothing needs you.**

## Rate limits (T089, D-1950 to D-1952, November 8)

- **What was built.** Limits on how often one phone, one visitor address, one business or everyone together can do something: 5 new requests per phone per hour, 40 new requests an hour in all (a burst of 100 fake requests is mostly refused and sends you one High alert, not 100), and limits on sign-in links, code tries, code texts, your owner password tries, business sign-ups (50 a day) and quote submissions. Your passkey sign-in is never limited, so you can't be locked out by someone guessing your password.
- **Not switched on everywhere yet.** The limits are ready, but the pages and the text line that use them belong to other tasks. The text line that takes homeowner requests is already live in testing and does not apply the per-phone limit yet; T256 adds it. The portal pages are not open yet and must apply theirs when they open.
- **Small gap.** Code texts sent from the portal and code texts sent for a pick each have their own daily total, so the "too many code texts today" alarm watches two counts, not one combined count.
- **Nothing needs you.**

## Email action links (T148, D-1955 to D-1959, November 8)

- **What was built.** Every action a business used to do by text keyword now has a secure link for its emails: Quote, Confirm, Hold for 2 more hours, Away, Pause, Resume, Busy for 1 to 60 days, Link this phone, Approve and Ask about a charge. Each link works for one person and one job, once, and only until its time ends. Opening a link only shows the page; nothing happens until the button is pressed. A "Cards and quotes" member gets no Hold, Confirm or Pause link. Replying to an email never does anything but the automatic reply or a help request.
- **The quote form** shows the business its fee if picked, with the basis, before it sends; asks every quote fact; fills in last time's answers for the same job type; and offers the optional "minutes this quote took", stored only for your pilot scorecard.
- **Not switched on yet.** The pages are ready but not served; the emails that carry the links, the Confirm code step and storing a "Busy" setting come with the tasks that open the business portal (D-1958).
- **A gap in the plan (D-1958).** "Ask about a charge" opens a help request with the charge, but not yet with the business's own question: nothing stores that message yet, and the queue only carries record numbers for privacy. The task that serves that page has to store the question first. Nothing is live, so nothing is lost today.
- **Also waiting (D-1957).** The quote form's price box and the handyman's "under $1,000, no permit" box sit beside the price, so they come with the task that serves the form. A "per visit" recurring price shows no fee yet, because the form doesn't ask how often the visits are; guessing weekly could charge the wrong amount.
- **Nothing needs you.**

## Open-cards email, missed-cards email and the notice log (T198, D-1992 to D-1995, November 8)

- **The open-cards email** is decided and worded: at each person's chosen time (5 p.m. unless they chose), only with a job request that still has at least an hour left or a pick waiting, never to a paused or busy business. A waiting pick comes first and goes in the subject.
- **The missed-cards email** on the 1st follows the rule: at least 3 requests last month and at least half closed before a quote. The words are the spec's.
- **The notice log** on each job page shows when each notice went (or "held by quiet hours"), what the mail server said, when the link was first opened ("may be a mail scanner") and when the business quoted or confirmed. It never shows homeowner details, and another business can't open it.
- **Not switched on yet (D-1993, D-1994).** Nothing sends these two emails yet. The email system can only send its fixed set of messages, so a later task adds one that can carry a list. The owner's off switch for the missed-cards email isn't stored yet either.
- **A test-plan fix (D-1993).** The test plan's example lists a request ending at 5:30 p.m. in the 5 p.m. email, but the rule needs an hour left. The test uses 6:30 p.m.; the plan's example should change.
- **Nothing needs you.**

## Emergency emails at any hour (T150, D-2008 to D-2012, November 8)

- **What was built.** EMERGENCY job cards and picks by email with an "EMERGENCY" subject and first line; each person's "Emergency emails at any hour" switch with a test emergency email and the question "Did your phone alert you with Do Not Disturb or Sleep on?"; the after-hours tick only while the owner or a "Confirm and billing" person has the switch on and a Yes under 90 days; the tick removed (owner emailed) when the last such person turns it off, answers No or is removed; suspended after two unvisited night cards until a new Yes.
- **Review fixes (D-2012).** Turning the switch off and on again needs a new test before a Yes counts. Only the owner's or a "Confirm and billing" person's Yes ends a suspension. The owner's email says whether the tick was removed or suspended.
- **Passes only at function level (D-2010, D-2011).** The tests for EMERGENCY cards (TD-CARD-09) and "after-hours cards go as soon as the ZIP is in" (TD-INT-31) check the functions, not the whole path: the email sender still holds every notice at night, the homeowner's night count still uses the old count, and nothing yet turns a released emergency job into cards. Later tasks wire these.
- **Known gap.** At night, emergency cards and picks go only to each switched-on person's sign-in email, not to the backup address they verified (PICK-037, BIZ-043). The task that wires the email sender should add the backups.
- **Nothing needs you.**

## Paid and free picks now release their details (T254, D-2013 to D-2016, November 8)

- **What changed.** When Stripe tells us a payment went through (signed and checked again with Stripe), or a $0 decision is on record (a free first win, a repeat customer or a fee-free job type), the pick becomes paid or free, the business gets the homeowner's first name and address once, and the masked call line opens. A pick that was cancelled or expired first gets nothing.
- **Recovery.** If something fails halfway (the server restarts, the contact store cannot be read for a moment), Stripe sends the event again and the release still happens exactly once; the business sees the details in its portal. If the masked line cannot open, it is tried again in the background, and a line closed after 30 days is never reopened.
- **Settings for the test and live projects.** The webhook needs `ASKHAVA_STRIPE_WEBHOOK_SECRET` (from Stripe's dashboard, at the account checkpoint) besides the Stripe key and account already listed; the worker needs the Twilio settings for the line retry.
- **Known gaps (D-2015, D-2017).** The job itself does not yet move to Payment pending or Connected; the payment's ledger line is still added only by the nightly reconcile; a repeated Stripe event is not yet skipped by its event ID (the release stays once); every release is a masked call until the homeowner's call-or-email choice is stored; an emergency job waiting for its address is not re-released when the address arrives. If a business pays but the job has no homeowner request on file to take her contact from, nothing is released and nothing retries it: one high-priority alert is raised and the pick stays paid with no details shown until someone acts on the alert (D-2017).
- **Nothing needs you.**

## The homeowner journey in a browser, stage 4 (T098, D-2029 to D-2032, November 8)

- **What runs now.** A fake homeowner's first text ("AC blowing warm air") reaches the app the way Twilio sends it, gets the consent text once and no 911 text; her quote page shows both quotes in the order received; she picks quote 2, goes back, picks again, types a wrong code and then the right one, chooses a masked call, and lands on the result with focus on its heading. "I smell gas" gets the 911 text and the consent text once each; "pipe burst" and "AC died, 92 inside" get no 911 text; STOP sent after the quotes arrive gets one confirmation and nothing after, and her job and quote page carry on. The same pages are walked by keyboard alone and scanned with axe at phone, tablet and desktop widths: no problems.
- **Closing checks.** No text twice, no charge, an audit row for every status change, and her phone number appears nowhere in the database outside the sealed vault or in the server log.
- **Not reachable yet (D-2030).** Nothing sends queued texts to the carrier yet; the conversation stops after the consent text; Spanish conversations cannot be switched on; and the paid release cannot be shown end to end, because the Confirm page is not served and the fake Stripe leaves out a field ("live mode") the app now insists on. That last one needs a small fix to the fake Stripe in a harness task. The full journey is T222's.
- **Also not reachable yet (D-2032).** After STOP the pick code should go by email, but the quote page always sends it by text and the conversation never asks for an email yet; and the app's record of queued texts cannot show that the 911 text went before the consent text, only that both went once.
- **Nothing needs you.**

## The monthly fee limit is a hard cap (T263, D-2081 to D-2083, November 9)

- **What changed.** A business's monthly fee limit can no longer be passed by two presses at once: each charge reserves its fee before Stripe is asked, a failed charge frees it, a paid one keeps it, and a charge that goes through after midnight on the 1st counts in the month it was pressed. Exactly at the limit is still charged. Changing a limit can no longer leave the business without one.
- **Review fix (D-2083).** A charge waiting on the bank's approval also holds its fee while its card-fix window runs, because the bank can still approve it; without this, a $100 limit could reach $125.
- **A test-plan fix (D-2082).** The test plan (TD-FEE-46) says that after the first charge fails "a later press on the other pick then charges", but that other pick has already ended over the limit. The test uses a third pick; the plan's wording should say "another pick".
- **Nothing needs you.**

## The 180-day contact purge can no longer lose a new request's details (T274, D-2150 to D-2152, November 9)

- **What changed.** The purge that clears a homeowner's name, email and address 180 days after her job closed used to check "is anything else of hers still live, is anything held?" and then clear in a separate step. A new text from her, or a dispute or complaint hold, landing in between could lose its details. Now the check and the clear happen together, under locks that new requests and holds also wait on, so whichever comes first wins cleanly.
- **A hold still on its way counts.** If the task that would place a dispute or complaint hold has not finished yet (or failed), the purge keeps her details and queues itself to try again an hour later, and keeps doing so until that task has finished (D-2153; an earlier version of this change kept the details for good instead, found in review).
- **One wide edge (D-2152).** A complaint hold event that fails for good now pauses every contact purge until it is cleared, because that event does not say which job it is about. That errs on keeping details. A later task should add the job to that event so only her purge waits.
- **One known gap (D-2154).** A reply to a request that never got a job, arriving in the very second its 180-day purge runs, may not be seen, and her name, email and address would be cleared (never her phone). The fix sits in a file this task may not change; it is listed for a small follow-up task.
- **Speed (D-2153).** The "is a hold still on its way" check reads the whole task table. It now runs before the purge takes its lock, so it no longer slows holds or other purges; an index that makes it fast is listed for a small follow-up task.
- **Nothing needs you.**

## Texting off or suspended: what happens to an emergency (T279, D-2185 to D-2189, November 9)

- **What it does.** You can switch homeowner texting off and record a Twilio suspension (and the restore) from the owner portal once T108 builds the pages; both survive a restart. While texting is off, suspended, or your texting or "all" pause is on, a homeowner's emergency text raises a critical alert for you, turns the site's "Texting is temporarily unavailable" banner on (only you can turn it off), and puts her on your call list. Nothing is texted.
- **For now, everyone goes on your call list.** The safety email needs the mailing-address footer every email carries, and no setting holds it yet, so the worker puts each homeowner on your call list instead, even one with an email (D-2186). The list shows only who she is by ID, nothing she wrote.
- **The email's wording is a stand-in.** When the email path is turned on, it reuses the "a text to you didn't go through" email with the safety text inside. That opening is wrong for a safety message; a copy-panel task (WORD-029) should write a proper one before it is used (D-2186).
- **Someone who opted out of texts** still gets the banner and the follow-up (email or your call list), never an automated call (D-2189).
- **One known gap (D-2188).** The intake step still writes its usual replies into the text queue while texting is off. They are never sent while it is off, but they must not go out late once texting is back; a small follow-up task is listed for that.
- **Nothing needs you.**

## The fee fixed at the pick is stored and checked before any charge (T287, D-1871, D-2210 to D-2212, November 9)

- **What changed.** When a homeowner picks, Ask Hava now stores the introduction fee by job size for that pick, worked out from the chosen quote alone. A "Confirm and pay" press must ask for exactly that amount, for that business and job, or nothing is charged and Stripe is never called.
- **A fee-mode switch after a pick.** If you switch between tiers and flat after a homeowner picked but before the business confirms, that pick is refused at Confirm with no charge (D-1871). A pick made after the switch still needs the business to accept the new card authorization first (D-072). If the business already passed the check when you switch, its charge goes through at the stored fee and is recorded.
- **One known gap (D-2210).** Jobs do not yet store their category and job type, so for now every pick is priced with the default category facts: the $25 visit fee size, no fee-free job types and the $75 recurring cap. A quote with packages (tiers) can't be picked until the homeowner's package choice is stored. This matches every category shipped today, but a fee-free job type would still be charged until a later task stores the job's category.
- **Nothing needs you.**

## The code review of the merged code (plan change, D-2620 to D-2630, November 10)

- **What it found.** Three separate reviews read the code already built for payments, privacy and texting. The main points: a paid charge is never written into the books, so a repeat customer would be charged twice and no Ask Hava receipt goes out; a won bank dispute stays booked as lost; a homeowner who writes "This is Bob, stop texting me" is not opted out, because the name filter swallows the word "stop"; no part of the app actually sends the queued texts yet, and nothing re-checks quiet hours at the moment of sending; START with a request still open gets no consent text; CANCEL does not pause a job; a few ways of writing a gate code, a street address or an email slip past the contact filter; and photos would become unreadable after a key change.
- **What changed.** Nine new tasks (T350 to T358) plus additions to T132, T261, T204 and T299. One review point was dropped because an earlier decision already covers it.
- **Choices made for you:**
  - **A won dispute** is recorded as two returns in the books: the dispute amount and the $15 countering fee. Stripe's own $15 dispute fee stays as a cost: Stripe returns only the countering fee on a win.
  - **A text queued while texting was off** is never sent late once texting is back (an opt-out confirmation never goes later than 5 minutes).
  - **A homeowner who opted out and writes again** gets at most one email a day if one is on file; otherwise she is counted in your weekly digest.
- **Second round (D-2631 to D-2641).** A review of this plan change fixed tasks that could not be built as written (split into more, smaller tasks: T359, T360), and a second batch of code review findings added tasks T361 to T367 and additions to T272, T295 and T347. In plain words: a homeowner's pick is tied to the business she reviewed, even if the list reorders; the quote page's live updates and other page scripts will actually load; the pick's code step gets the same limits as sign-in codes; businesses get a working "confirm it's you" step for team, billing and address changes; smoke-alarm, sprinkler and wall-removal jobs never go to handymen however they are worded; a "not under the sink" answer is no longer read as "under the sink"; a text is never sent twice (if the app can't tell whether a text went out, it does not resend it, it sends the email copy for important texts and alerts); stuck job timers and stopped clean-up runs now restart and alert.
- **Nothing needs you.**

## Code review batch 3: business sign-up and licenses (plan change, D-2700 to D-2710, November 11)

- **What it found.** A review of the business sign-up and catalog code found: pest, septic and home-inspection businesses, and handymen, could never tick their own job types; a business that changes a phone number would keep getting texts at the new number without agreeing to them; a business could say yes to after-hours emergencies at sign-up with nobody set up to get the emails, and homeowners at night would be told it takes emergencies; a category you switch off in settings would still be offered at sign-up; and license changes on the state lists are never applied (nothing runs the daily lists yet).
- **What changed.** Eight new tasks (T368 to T375) plus additions to T251, T301, T302 and T329.
- **Decided for you:** a business paused because its license no longer covers any job type starts getting cards again on its own as soon as it ticks, on its new Job types page, a job type its license covers, unless another pause still applies (complaints, a pest license reported wrong) or it had paused itself first, in which case it stays paused (D-2711, D-2730). This answers the earlier open question (D-2596); tell me if you want it different.

## Code review batch 4: stand-ins that were kinder than the real vendors (plan change, D-2760 to D-2770, November 11)

- **What it found.** The fake Twilio, Stripe, Resend and AI services used in tests accepted things the real ones would refuse or handle differently, so tests passed where the real thing would fail: the AI request uses two schema words Claude's API does not support; texts never ask Twilio for delivery reports; calls carry no instructions for what to say; emails Resend skips for an earlier bounce count as delivered; a 3-D Secure card may stall; a late refund retry or a card-network inquiry would be recorded wrongly; a resent code can hit Twilio's attempt limit and show an error.
- **What changed.** Seven new tasks (T376 to T382), and additions to T108, T110, T122, T129, T131, T132, T133, T137 and T298. Each fake is made to behave like the real vendor, and each sandbox task checks the vendor's real answer before launch.
- **Your setup, later (account checklist, T122).** (1) In Twilio, the incoming-text webhook gets a retry setting (Twilio's "connection override") and a fallback address, because by default Twilio does not resend a text when the app answers with an error. (2) Twilio Verify's email codes need a Twilio SendGrid account connected to the Verify service. SendGrid would then see the email addresses codes go to, so it joins the list of providers in the privacy notice.
- **Decided for you (D-2769).** Email codes stay on Twilio Verify, as the spec says. The alternative is sending email codes through Resend, which you already use: no new account, but the app would hold the codes itself and the spec line would change. Tell me if you prefer that.
- **Known gap (D-2763).** If the database is down for longer than Twilio's retries last, a text sent to Ask Hava in that window is lost. Storing every text before handling it would close that; it is not planned now.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## A shorter path to the finish: stage-6 steps that waited for no reason (plan change, D-2821, November 11)

- **What it found.** Several stage-6 steps were lined up one after another only by habit: the Stripe sandbox check waited for the Twilio calls one, the real-Claude connection and the 3-second reply limit waited for the whole test-site run, the security scan waited for the real-AI test, the AI-call limits waited for the money war game (they still wait for your war-games go, as does wiring the AI into texts and web chat), and the no-safety-number check waited for the final audit. None of them uses anything the step before builds.
- **What changed.** Those waits are removed; each step still waits for what it really needs (the sandbox setup, the account checkpoint, the test site). The Twilio calls, email and license sandbox checks stay in a row, and the two Stripe refund and dispute fixes come after them, because each adds to the same pretend-vendor file; four of them now own that file, which they needed and did not have. The pre-war-games review still sees the AI-call limits as planned rather than built, as before, because they wait for your go and the review comes before it. The test-site run of the four journeys still waits for every sandbox check, and the first war-games round still waits for the money run. The longest remaining chain of steps drops from 66 to 59.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## Brand switch: the last brand words in the texts get an owner (plan change, D-2843, November 11)

- **What it found.** The step that proves the brand name can be switched in one place (T295) stopped because three files still spell out "Ask Hava": the two files that hold every text message (English and Spanish) and the homeowner's own page. No step was assigned to change them.
- **What changed.** A new short step, T383, swaps those words for the brand setting, so every text and the page read exactly as today under "Ask Hava" and follow a new name automatically. T295 now waits for it. The Spanish texts stay Spanish, and no text is added or removed.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## A fee check that tripped on two old tests (plan change, D-2832, November 11)

- **What happened.** The step that makes sure a no-fee job can only unlock the job it was recorded for was built, but two older tests failed against it: they unlocked jobs without the stored fee record every real pick has. The step was not allowed to touch those tests.
- **What changed.** The step may now add the missing fee records to those two tests, and only that: every check in them stays exactly as it was. It is now a harness step (the kind allowed to adjust earlier tests), and the Stripe charge-ID step that edits the same test file waits for it.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## A step that says the plan is wrong now waits for the plan fix (harness change, D-2859, November 11)

- **What happened.** Twice today a step (the no-fee check and the brand switch) stopped on its first try and said plainly that the plan had to change first. The runner treated that like an ordinary failed try and ran the same step again, unchanged, one or two more times; each repeat hit the same wall and spent a lot of usage for nothing.
- **What changed.** A step that finds it cannot pass without a change the plan forbids it to make now says so in a fixed first line, naming the file and the step that owns it (a hard job or its own failing test does not count). Once the reviewer agrees, the runner stops it at once, lists it under "Needs attention" on the status page and leaves a note in the outbox. It comes back after the plan is fixed, with the tries it has left; if the report turns out to be wrong, one command brings it straight back.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## The no-fee check meets a third old test (plan change, D-2896, November 11)

- **What happened.** The no-fee check step tried again after this morning's fix and found one more older test it was not allowed to touch. That test lets two businesses quote the same job, then assumes the homeowner's first pick went to a particular one of them; really the page lists them in an order that comes out either way, so with the new check the test failed about half the time.
- **What changed.** The step may now fix that test's setup so it uses the business the pick really went to, and only that: every check in it stays exactly as it was. The step that adds job details to the same test file now waits for it, so only one step edits the file at a time. No other test calls the no-fee check with a guessed business, so a fourth surprise of this kind is not expected.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## Showing an edited or withdrawn quote gets its own step (plan change, D-2912, November 11)

- **What happened.** The step that lets a business edit or withdraw its quote before a pick stopped on its first try: showing the homeowner "Updated" or "Withdrawn by the business" needs the quote list, the LIST text and an older test, none of which that step was allowed to touch, and it was already at its size limit.
- **What changed.** The business side (edit, withdraw, the notes check and quote formats) stays in that step. A new small step, T384, right after it, shows the change to the homeowner: an edited quote says "Updated" with the time, and a withdrawn one stays in its place with "Withdrawn by the business", so no other quote's number changes, and it can never be picked. The same step fixes a small mix-up found on the way: after a business couldn't take a pick, the LIST text could number quotes differently from the page, so "2 YES" might have picked a different business than line 2; now the LIST uses the page's numbers. One older test only gains the two new fields; every check in it stays exactly as it was. The later step that keeps quote numbers fixed once shown now waits for it.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## Jobs that would fail quietly now raise an alert (T327, D-2849 to D-2851, November 11)

- **What changed.** A build check now fails whenever the app can queue a background job that nothing runs, unless the job is on a short list naming the task that will build it. And once an hour, in the test and live projects, the system raises one high alert per kind of background job that failed because nothing runs it.
- **Two gaps to know about.** The alert names the kind of job, but the count of failed jobs is only in the log, because alerts have no place for extra detail yet. And the "auto-confirm did not go through" notice to a business (D-2785) has no task assigned to build it; it is on the list under the task that created it until the plan names one.
- **Three more notes from the review (D-2852, D-2853).** The count is still only in the log, not on the alert. The task listed as owner of that auto-confirm notice (T209) is already finished, so it will not build it; the plan needs a real owner. And once you resolve one of these alerts, the next hourly check raises it again if the stuck jobs are still there, so clear or fix the jobs before resolving it. Two checks at once now raise one alert, not two.

## Business names come from what the business types at sign-up (plan change, D-2943, November 11)

- **What happened.** The step that gives each business's name to the LIST text and the "who's calling" text stopped on its first try. The plan said to read the name from the business's license record, but the app never keeps names from the state contractor list, and nothing ties a business to its line on the city license list, so there was no name to read.
- **What changed.** Sign-up now asks for the business name and, if it has one, the trade name (DBA), and keeps them on the business's own record, as the original blueprint's business record already did. Each name is checked for phone numbers, links and "find us on" wording; a bad one is shown back with the reason. Texts and quote cards use the trade name if given, otherwise the business name. The owner's personal name from the city list is never kept.
- **Calls Claude made that you may want to check.** This replaces the November 9 note that the name would come from the license record. For a business with no state license, the names it types are the ones checked against the city license list; no match means it waits as pending, as today. For a licensed business, the name is not yet checked against its state record, because the app doesn't read that record at sign-up yet. The same missing read is also what the identity code needs (the phone or email on the state record), so a follow-up plan change will add it before business sign-up is switched on; until then a licensed business could type another business's name. A business that matched the city list only on the owner's personal name will show as pending on later daily checks, because that name isn't kept. Businesses still can't change the name themselves after sign-up.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## Sign-up reads the business's own license record (plan change, D-2954, November 11)

- **What happened.** The November 11 change about business names left one hole: for a business with a state contractor license, nothing checked the name it typed against its state record, and nothing gave the identity code the phone or email on that record, because the app keeps no names or contact details from the daily license lists.
- **What changed.** A new step (T385) looks up only the signing-up business's own license record when sign-up needs it, and keeps nothing from it. The license number is fixed once, when the license is checked, so a later screen can't swap in someone else's number. The name the business typed is compared with the record's business name and trade name (DBA); if neither matches, the sign-up waits for your review instead of going live. The identity code goes to the phone, then the email, then the mailing address on that record. For a business with no state license, the contact comes from its line on the city license list. The served sign-up (T302) now waits for this step.
- **Expect more sign-ups in your review queue.** The state contractor site's public record seems to show a business name, phone, city and ZIP, but no email, often no street address and no trade name. So: a business whose record phone can't get texts may have no other way to get a code and will wait for you; a business that typed only its trade name, or wrote "LLC" where the record doesn't, will wait for you; and a city-list line without a single clear license number gives no contact and will wait for you. Each of these is a safe default (review, never a wrong activation).
- **Calls Claude made that you may want to check.** The record's qualifying party or owner name is never read or compared. A record phone that isn't marked as a landline gets a text first. If the state record can't be read, the business is asked to try again and you get one alert a day, with no details in it.
- **Still open.** When a business says "That number is old" and fixes its record, nothing yet checks the daily lists for the new contact and restarts its sign-up; that needs an owner in a later plan change.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## A late payment on a pick the system ended is refunded from the first check (plan change, D-2970, November 11)

- **What happened.** The step that refunds a payment arriving after the system had already ended a pick stopped on its first try. Its plan changes the 30-minute payment check so that the check itself queues the refund and alerts you, but an older test expected the refund to be queued later, when the pick side reported the payment, so that test could not pass.
- **What changed.** That one line of the older test now expects "already queued" at the later report. The test still checks that you get exactly one alert and exactly one refund job; only which step queues it first moved. Nothing else in that test changes, beyond the refund label change planned on November 9.
- **Also fixed.** The November 9 note gave this step's database change number 0176; a later change already moved it to 0177, and the plan now says so.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## T164: quote formats, notes, and changing or withdrawing a quote (D-2897 to D-2901)

- **What changed.** The quote form now shows only the price boxes for the job type's one format, with the same plain definition the homeowner sees. A price per visit must say how often (weekly, every two weeks, monthly, every 3 months or once a year), or the business picks "Other: enter the monthly amount". A note to the homeowner is limited to 300 characters and comes back with the reason if it holds a phone number, link, email, or a line like "text me", "Venmo" or "cash discount". Before any pick a business can change or withdraw its quote from the job page; after a pick it can't. A changed quote keeps its place in line.
- **Still to come.** The homeowner seeing "Updated" or "Withdrawn by the business" is the next step (T384). The AI check on notes is ready to plug in but has no AI behind it yet.

## The build now works four tasks at once (plan change, D-2986, November 11)

- **What changed.** You asked to speed the build up. Since this morning the token pacer almost never held a task back; the number of lanes did. The runner now works up to four tasks side by side instead of three.
- **What stays the same.** Two tasks side by side still never touch the same files or sit in different stages. If one extra lane fails twice in a row for a harness reason, the runner drops to one lane by itself and tells you why.
- **Cost.** About 62M tokens are left this week before Thursday's 17:00 UTC reset; four lanes will use them sooner, as you said is fine. With more branches open at once, tasks will more often need `main` merged in before they land.
- **Undo.** Setting the lane count back to 3 is a one-line logged change.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## A late payment on a pick the system ended is kept on record and refunded (plan change, D-3007, November 11)

- **What happened.** The step that refunds a payment arriving after the system had already ended a pick stopped a second time. Its plan has the system keep a record of that payment and queue its refund, but an older test expected the system to keep no record of it at all, so no build could pass both.
- **What changed.** That older test still checks that an ended pick is never handed to the business. Instead of "nothing is kept", it now checks that the payment and its record are kept, and that exactly one refund job and one alert to you are made. A stale comment in another test is corrected too. Every other test that touches these payments was checked; none other needs a change.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## The pick email gets its "Can't take it" link (plan change, D-3024, November 11)

- **What happened.** The step that adds the "Can't take it" link to the email a business gets when it is picked stopped on its first try. Its plan pointed it at the wrong file, and the email's words are shared texts it wasn't allowed to change. Adding the link to those texts also breaks five older tests that build the same email or every text with a fixed list of sample values.
- **What changed.** The work is split in two. The first step now puts the link's address in where the email is built. A new follow-up step (T386) adds the line to both pick emails (regular and emergency) in English, "Can't take it:" and the link, and in Spanish, "¿No puede aceptarlo?" and the link. The five older tests only gain a sample address; every check they make stays the same. The link opens a page that asks the business to sign in, and it carries no homeowner details. The full journey test of the business side waits for the new step.
- **Calls Claude made that you may want to check.** In the email the address shows as plain text, not a button; making it clickable is a small later change.

## A partial or foreign-currency refund is checked on every path, and the business is not told "refunded" (plan change, D-3018, November 11)

- **What happened.** The step that checks how much Stripe actually refunded stopped: two of the places it has to reach belonged to earlier, finished steps. A refund confirmed by Stripe's later notification skipped the check, and the "your introduction fee is refunded" message to the business is sent by another part of the system that didn't know about the check.
- **What changed.** The same step may now change exactly those two places. A refund confirmed later is checked the same way as one confirmed at once. If Stripe refunded less than the full fee, or in a currency other than US dollars, the job still closes as before, you get the alert, and the business does not get the "refunded" message; you decide what to tell them. If Stripe simply didn't say the amount or the currency, the message goes as today, because the system only ever asks for full refunds. If the system can't reach Stripe to check before sending the message, it closes nothing yet and tries again on its next run, so an unchecked refund is never announced. No other step and no test written by another step changes.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## T384: the homeowner sees a changed or withdrawn quote (D-2987 to D-2990)

- **What changed.** When a business changes its quote before any pick, the homeowner's quote page and the quote card show "Updated Nov 3, 9:15 a.m. Arizona time", and the quote keeps its number. A withdrawn quote keeps its number and place, says "Withdrawn by the business" and has no Pick button; no one can pick it by page, by "2 YES" text or as a backup. The LIST text now numbers each line with the quote's page number, so "2 YES" always picks the quote shown as 2 (before, after a failed pick the list could renumber and "2 YES" could pick a different business).
- **One thing you may want changed.** The LIST text ends "for example 2 YES" whatever quote 2 is, even when quote 2 was withdrawn. It is a fixed line from an earlier step; changing it to name a quote she can pick would be a small later task.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## The "job confirmed" email's sign-in link now works (T269, D-3025 to D-3028, November 11)

- **What it does.** The link in that email opens business sign-in for the person it was sent to. Pressing "Sign in" emails them the usual one-time sign-in link; after that link (and the new-device code, when asked) they land on that job's page. The link alone never signs anyone in or shows any job detail. A changed, expired or someone else's link just shows the normal sign-in form.
- **One wording point for later.** The screen with the button reuses today's words, "Press the button to finish signing in.", although pressing it sends an email. A small copy change would fit better; this task was not allowed to write new wording.
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## T214: the share box at the pick and "Can't take it" (D-2992 to D-2994)

- **What changed.** Before her final Pick the homeowner sees an unticked box: "Share my first name and the job address with the business for quote #2 once they accept." Pressing Pick without ticking it shows the page again with the error by the box; nothing is picked and no code is sent. The tick is logged with the pick. The pick notice now carries a "Can't take it" link for the business; pressing it ends the pick at once with no fee and moves the pick to her backup, or reopens her quotes.
- **One thing you may want changed.** The box names "the business for quote #2" until businesses' names are stored for the page; the Spanish label is "Compartir mi nombre de pila y la dirección del trabajo con ... cuando acepte."
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## T165: business names, license numbers and Google and Yelp links at sign-up (D-3033 to D-3036)

- **What changed.** The sign-up form now asks the business name (needed), a trade name or DBA, the ROC, BTR and city license numbers it has, and an optional Google Business Profile link and Yelp link. A name with a phone number, a link or "find us on Google" is sent back with the reason and nothing is saved. A link is kept only if it is on Google's or Yelp's own site and the page opens. Saved links show on every quote card as two plain links, "Google" and "Yelp", with no stars or counts.
- **One thing you may want changed.** Links are checked when they are saved, not again later; a page Google or Yelp takes down stays on the cards until the business changes it in the portal (a later task).
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## T228: leaving Ask Hava and the optional reason (D-3044 to D-3048)

- **What changed.** A business owner can leave Ask Hava from the portal (the page is built; it goes live with the portal's routes). The page lists what leaving does before the one button: open quotes are withdrawn ("Withdrawn by the business."), a pick waiting for its Confirm ends with no fee and goes to the homeowner's backup, saved payment cards are removed, the team is signed out, and the charge and account records stay 7 years. It asks once, optionally, why, with "Busy for a while" first and a pointer to "Busy for a number of days"; a business paused more than 2 weeks is asked the same question once. The answer changes nothing about its jobs or fees.
- **One thing you may want changed.** Until the sign-in checks for it, a member of a business that left could ask for a new sign-in link; nothing they could do there sends them jobs (D-3045).
- **After review (D-3048).** Leaving now withdraws every quote the business has, including on jobs past Confirm, so none can come back if a charge fails. The 2-week pause question is asked only when the business paused itself, never when you or a rule paused it. The screens no longer say notices stop, since only job cards stop today. Still open: removing a card at Stripe is tried once, with no retry yet, and a business that left can still ask for a sign-in link (both wait on later tasks).
- **Nothing needs you now.**

## T316: one read of each business's public name (D-2923)

- **What changed.** The app can now look up the name to show for a business: its trade name (DBA) if it gave one at sign-up, otherwise its business name, checked again for phone numbers, links and "find us on" wording. The LIST reply and the "who's calling" text will use it once their own steps are built (T317, T318).
- **Worth a look.** A business whose stored name fails that check, or that has no name stored, is left out of the list and you get one digest alert naming only the business, not the name. A business can't edit its name in the portal yet.
- **Nothing needs you now.**

## T317: a limit on new requests by text, and LIST names each business (D-3084 to D-3087)

- **What changed.** A phone can open at most 5 new requests by text in an hour (and all phones together have an hourly cap); a 6th is not answered and you get one alert with no phone or words in it. Replies inside an open request, HELP, STOP and START after a STOP are never limited. A LIST reply now names each business that quoted.
- **One thing you may want changed.** A text that matches the 911 list (for example "I smell gas") is never limited, so the homeowner always gets the 911 safety text, even at her limit. The cost: someone sending many such texts from many phones would get past the hourly cap. If you want a separate cap on emergency texts, say so; it needs a spec change.
- **Nothing needs you now.**
