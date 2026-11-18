# askhava-picks: the homeowner's pick and what follows it

## Purpose
The layer-6 crate that takes the homeowner's pick and carries it to the business's Confirm, the charge and the release (`docs/architecture.md` section 3).

## Never break
- Refused at exactly 10:00, cooldown after the fifth wrong try, paste reads: `src/tests/pick_code_rules.rs`, `tests/security/tests/pick_code.rs::td_pick_06_*`.
- Voice only on the tap and never at night from another session; every cap holds; one alert a day; no number stored: `pick_code.rs::pick_055_*`.
- No pick before the Terms line; one acceptance per text pick: `tests/integration/tests/text_pick.rs::td_pick_40_*`.
- Another phone gets nothing; two jobs ask which: `text_pick.rs::td_pick_07_*`.
- A second YES replaces and tells the first business, but never after the business confirmed; confirmed and status replies: `text_pick.rs::td_pick_14_*`.
- CANCEL withdraws the waiting pick, keeps the status, blocks new picks: `text_pick.rs::td_con_12_*`.
- A withdrawn pick tells its owner and "Confirm and billing" members once, never a cards-only member: `withdrawn_optout_safety.rs::td_pick_55_*`.
- Every new pick stores exactly one fee in its own transaction, and a pick whose fee can't be fixed stores nothing: `tests/integration/tests/fee_fixed_at_pick.rs` (TD-FEE-48).
- A page re-pick replaces the waiting one with its notice and no charge; the waiting quote again stores nothing; Guarded by `tests/integration/tests/page_pick.rs`. More: `(never 9)` in NOTES.md.
- The window, its stale-timer fence, a repeated send restarting nothing, a held notice expiring a dead pick and the license checks: `tests/integration/tests/confirm_window.rs::td_pick_09_*`; More: `(never 10)` in NOTES.md.
- Each other quoter told once, never an amount or a name; the 90% and 110% edges; Guarded by `tests/integration/tests/pick_aftermath.rs`. More: `(never 11)` in NOTES.md.
- The released view: the refusal rule over every status, seat and pairing, and on the database (crate-private `viewable`: owner and billing member only, the bound contact and email choice, a removed or demoted member refused) in `src/tests/released_view.rs`; More: `(never 12)` in NOTES.md.
- PICK-018, PRIV-001, D-3457: `picks.contact_choice` holds the choice word and IDs only, never an email; a pick with no row releases as a masked call: `tests/integration/tests/pick_contact_choice.rs`.
- Details only on a proof, once, never the phone, no detail in the notice: `src/tests/prop_release_only_paid.rs`, `tests/security/tests/no_phone_released.rs`. Never add `Field::Phone` to a release field list or a phone field to `ReleaseView`. More: `(never 14)` in NOTES.md.
- An emailed code keeps the limits, a quote-page address never gets one, an opted-out YES queues no text and picks only after the Terms line: `tests/integration/tests/email_pick_code.rs`.
- Refund and close once, never on a failed or blocked refund, a half-finished run finished; email YES stops them; Guarded by `tests/integration/tests/unreachable_refund.rs`. More: `(never 16)` in NOTES.md.
- Steps queued once per release, reminders cancelled by an earlier call (one between 24 and 48 hours too), no refund without a queued missed-call notice, a queued notice still counting after an opt-out, a NO to the question keeping the refund, one question per pick, an ended pick never acting on the job's next pick, no decision email on a job gone to Done, the missed-call text at 8 a.m. Guarded by `tests/integration/tests/unreachable_steps.rs`. More: `(never 17)` in NOTES.md.
- A lapse, refusal or payments end moves the pick to a still-available backup once, with its own notice and window (emergency too); no backup or an unavailable one is PICK-046; Guarded by `tests/integration/tests/backup_choice.rs`. More: `(never 18)` in NOTES.md.
- PICK-022, FEE-052, D-2627: the release claim and the fraud end lock the pick row first; Guarded by `tests/integration/tests/release_fraud_race.rs`. More: `(never 19)` in NOTES.md.
- FEE-050, D-2204: `picks.dispute_opened` marks a dispute only with a release and a business call attempt or a stored authenticated 3-D Secure result, waits for Casey otherwise, and marks nothing twice: `tests/integration/tests/dispute_submit.rs` (TD-FEE-51).
- FEE-045, D-2331: the fee's facts come from the job's latest `jobs::api::job_facts` row through `payments::api::resolve_fee_facts`, never the defaults; unsettled facts refuse a non-emergency pick and store nothing; an emergency pick proceeds on `JobFeeFacts::EMERGENCY_DEFAULT` with one High `FEE_FACTS_ALERT` naming the pick ID only: `tests/integration/tests/fee_facts_at_pick.rs` (TD-FEE-62).
- PICK-053: `tier_and_visit_pick`
- BIZ-025, D-284, D-3790: `pick_numbers` counts one business's own picks, confirms (`status::confirmed`) and lapsed picks by pick month, and lists only its own releases as `(job, connected_at)`: `tests/integration/tests/business_numbers.rs`.
- PICK-008, PRIV-005, D-2912: `place` ends the waiting pick, then takes the job's row lock (pick row before job row, D-3979), and `new_pick` re-reads the quote; a withdrawn or unavailable one rolls `place`'s savepoint back (page: `Taken`; text: the no-quote reply): `tests/integration/tests/pick_bound_business.rs::td_pick_60_a_withdrawal_committed_after_the_check_is_taken`.
- PICK-008, PRIV-005, D-2632, D-3999: a pick is bound to its business: `bound_pick` answers `Taken` when the shown number no longer maps to the business, text and opted-out picks resolve the shown number, and a waiting pick is compared by `business_id`, never the stored `quote_number`: `pick_bound_business.rs::td_pick_60_text_picks_compare_the_waiting_pick_by_business`, `td_pick_60_finishing_number_one_is_b_or_refused_never_a`.
- PICK-019, PICK-044, D-3750: the Confirm window's end (normal, emergency, after `jobs`' backstop or timer, a late worker never in the past) lapses with one PICK-047 or PICK-046 event; a restarted window or a pick no longer waiting does nothing: `tests/integration/tests/confirm_window_lapse.rs`.
- PICK-047, PICK-046, D-3750: "Can't take it", the business leaving and a `payments` end queue her one PICK-047 (`pick_moved`) or PICK-046 (`pick_lapsed`) event per ended pick, after their last job move, never inside `backup_choice`: `tests/integration/tests/pick_moved_text.rs`.

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/picks/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/picks/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
