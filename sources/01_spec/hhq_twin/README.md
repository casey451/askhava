# Havasu Home Quotes: digital twin

This folder runs the whole business as if it were live, with fake versions of every outside service. Simulated homeowners, contractors and the owner (Casey) send texts, pick quotes, confirm, pay, call and complain. Automatic checks then look for every way the business could break a promise, leak a detail, charge wrongly or leave someone stuck.

Everything here is **simulated**. Nothing contacts a real person or service.

## What's in the folder

| File | What it is |
|---|---|
| `app.py` | The app's rules, written the way the real app will work. It talks to the outside world only through the vendor objects. |
| `vendors.py` | Fake Twilio texts and calls, fake Stripe, fake Claude AI, the fake ROC license site and fake email. Each has real prices and fails the way the real one can (delays, duplicate notices, declines, outages, CAPTCHAs, AI mistakes). |
| `twin.py` | The simulator, the simulated people, 56 automatic checks and the report. |
| `review_tests.py` | 19 step-by-step tests: bugs the two independent code reviews found, database-restore edge cases, and same-moment collisions run in both orders. Every one passes on v9.3; v9.1 fails 13 of them. |
| `templates_es.json` | Draft Spanish versions of every text a homeowner can get. A native speaker must review them before launch. |
| `twin_config.json` | Every setting: demand, personas, vendor failure rates, app rules. Edit any number. |
| `twin_scenarios.json` | 21 scenarios: 10 situations, each run with the v9 design as written and again with the v9.3 fixes, plus a crowded market (v9.3 only). |
| `results/` | `twin_results.json` (every number and problem), `console.txt` and `review_tests_console.txt`. |

## How to run it (step by step)

1. Install Python 3 from python.org (Macs may already have it).
2. Open Terminal (Mac) or Command Prompt (Windows).
3. Type `cd ` (with a space), drag this folder onto the window, and press Enter.
4. Type `python3 twin.py` and press Enter (on Windows, `python twin.py`). All 21 scenarios run, 3 times each, in about 40 seconds. `python3 twin.py all 5` runs each 5 times.
5. Read the printout. Each scenario shows requests, job cards, quotes, picks, paid introductions, money and owner minutes. Below that, every problem found, how many times per run, and one example.
6. To run one scenario: `python3 twin.py busy_fixed 5` (5 runs).
7. To run the step-by-step tests: `python3 review_tests.py` (about 1 minute).

## The situations tested

| Scenario | What happens |
|---|---|
| `normal` | 60 days in Lake Havasu City, 5 requests a day |
| `busy` | 16 requests a day, HVAC-heavy summer |
| `vendor_chaos` | 5% of texts fail, notices arrive twice or late, a Stripe outage, an AI outage, ROC CAPTCHAs |
| `adversarial` | People over-share details, try to trick the AI, forward their links, contractors try to bypass the fee, fake contractors sign up |
| `owner_away` | Casey unavailable for 2 weeks |
| `texting_suspended` | The texting provider suspends the number on day 30 |
| `disputes_wave` | 10% of charges disputed on day 40 |
| `shortcut_summary` | A tempting shortcut: pasting job descriptions into the AI weekly summary |
| `races` | 16 requests a day; every event snaps to the minute and same-minute events run in random order (a cancel and a Confirm, a Stripe notice and a deadline) |
| `db_restore` | Day 30: a bad change at 10 a.m. is found at 1 p.m. and the database is restored to 10 a.m. Stripe and Twilio keep everything from those 3 hours. |
| `crowded_fixed` | 15 businesses per trade; every qualified business that opted in gets each job card |

Add `_fixed` to any name to run it with the v9.3 fixes switched on: the v9.1 and v9.2 fixes, plus
job-type matching, sending to every qualified business, the texting-compliance rules, restore replay and the other v9.3 changes.

The simulated people answer the way real people do. They say "Yes!", "yes please", "the AC one", "my email is …", or "my tenant Sam will be there". The fake AI's second reading of the same message usually repeats the first reading's mistake, as a real model would.

## The automatic checks (each problem key in the printout)

The checks read only what the outside world saw: every text the fake carrier delivered, every Stripe payment, every call. They also use what each simulated person actually did. They never trust the app's own labels or records, so a bug in the app can't hide itself.

| Area | Checks | Rule it protects |
|---|---|---|
| Money | charge_without_homeowner_yes, charge_without_contractor_confirm, charged_after_confirm_window, charge_while_request_not_live, double_charge, over_refund, charged_but_not_introduced | A charge only after the homeowner's own phone said yes and the contractor confirmed in time, while the job was live, exactly once, and only for a real introduction |
| Money records | stripe_charge_missing_from_ledger, duplicate_ledger_rows, ledger_refund_mismatch, dispute_missing_from_ledger, contractor_statement_mismatch | Casey's books and each contractor's statement match Stripe, payment by payment |
| Refund rule | unreached_homeowner_no_refund | Judged from the call log against the published rule: 2+ attempts at least 3 hours apart within 72 hours, no 45-second conversation, then refund |
| Privacy | details_released_wrongly, details_released_to_unlicensed, identifying_text_in_job_card, identifying_photo_in_job_card, photo_location_not_stripped, personal_details_to_non_zero_retention_ai | Contact details reach only the confirmed, paid, licensed contractor. Job cards stay anonymous. The AI data rule holds. |
| Texting | text_in_quiet_hours, text_after_stop, english_text_to_spanish_speaker | No texts 9 p.m.–8 a.m. unless they texted us in the last 15 minutes. STOP is honored. Spanish speakers get Spanish. |
| Safety | emergency_without_safety_message, safety_message_late, job_card_sent_for_emergency | Emergencies get the safety text within 5 minutes and never a job card |
| Contractors | job_card_to_unlicensed_contractor, fraudulent_contractor_activated, legit_contractor_stuck_in_review, handyman_over_1000, free_first_win_reused, major_fee_on_small_job | Licensing, identity, the handyman limit, one free win per business, the right fee |
| Calls | call_to_wrong_homeowner, call_after_homeowner_closed, call_blocked_unlinked_staff_phone | Masked calls go to the right person, stop when the job is closed, and work for staff phones |
| Homeowner experience | promise_missed, quotes_never_reached_homeowner, pick_lost_to_failed_text, pick_not_intended_by_homeowner, rental_without_onsite_contact, request_left_open_after_stop, review_request_without_done, duplicate_open_requests | The 9 p.m. promise, failed texts don't lose jobs, a forwarded link can't pick, rentals have an on-site contact, CANCEL closes the job |
| Nothing stuck | request_stuck_waiting, pick_stuck, illegal_status_change | Every job and payment moves forward or ends |
| Matching | job_card_outside_license, qualified_business_left_out | A job card goes only to a license that covers the job, and to every qualified business that asked for that job type |
| Everyone hears back | texter_never_answered | Every new request hears back within 24 hours (text, email, or Casey personally) |
| After a restore | request_number_reused, charge_record_lost_or_mismatched | Job numbers already texted out are never reused; every kept charge matches a pick the app knows |
| Texting compliance | first_text_missing_disclosures, link_not_on_own_domain, promotional_content_in_text, text_without_consent, help_not_answered, natural_optout_ignored, more_than_one_optout_confirmation | The first text to anyone names the brand and says how to get HELP and how to STOP. Links stay on our own domain. No marketing. We text only people who contacted us or signed up. HELP is answered. "Please stop texting me" works like STOP, with one confirmation. |

## Extra numbers in each run's results

- `license_lookups_per_day`: visits to the state license site. About 35 a day with 4 businesses per trade, about 130 a day with 15 per trade. At 30–60 seconds each, that's roughly 20–35 minutes a day, or 1–2 hours in a crowded market, of automated browser time. It's Cowork's time, not Casey's.
- `questions_per_request`: how often a homeowner gets the extra "is this 1) or 2)?" question (about 6–7% of requests).
- `optout_rate_homeowners`: share of texted homeowners who opted out.

## Limits

- **People:** their behavior (how often they answer, quote, pick, confirm, which job types they tick) is made up. The hand-run test and the paid pilot replace those numbers.
- **Vendors:** the fake vendors behave the way the real vendors' documentation describes, which isn't always how the live systems act. Stage 0 of the build runs this twin against the real code, and the test environment runs against the real vendors in test mode.
- **The fake AI:** it's told the right answer so it can decide when to make a realistic mistake. The app never sees that answer; it gets only what the AI returns.
- **Guessed rates:**
  - on-server photo checks: 85% detection;
  - Casey's sign-up reviews: fooled 0.5% of the time with the mailed-code step;
  - homeowners answering the 1-or-2 questions correctly: 90–95%.
- **Same-moment collisions** are tested by running them in every order. That matches a real database only if the build locks each job's record while changing it, so every change runs whole, one at a time. The build must do that.
- **Not simulated or still to review:**
  - The web pages themselves and the attorney's wording aren't simulated.
  - The job-type-to-license table needs the attorney's review before launch.
  - The Spanish drafts need a native speaker's review.
  - The texting registration needs Twilio's acceptance.
