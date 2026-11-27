# MONEY-1 review: Opus verification (main 32744d0f2, read-only)

## F1 (critical): polled success never released. WRONG
- The reviewer did not see T663 (DONE, D-4349). The Confirm press moves the job to Payment pending (`confirm_moves::press_move`, `connect.rs:133`). When `jobs`' Payment pending wait ends it queues `picks.payment_returned` (`app/jobs/src/payment_wait.rs:29`, `app/server/src/lib.rs:1188`). `payment_returned` (`app/picks/src/connect.rs:228`) asks `payments::payment_held` (`app/payments/src/payment_hold.rs:43`). A `succeeded` attempt, which `run_charge_check`/`ask_stripe` or the synchronous charge answer stored, gives `PaymentHold::Success`. `held()` (`connect.rs:262-280`) then queues `picks.held_success`. That task is registered at `app/server/src/lib.rs:1144,1423` to `release_hold::run_held_release`, which calls `payments::held_release` (`release_proof.rs:250`). That function re-fetches the intent from Stripe, runs `check_intent`, checks for an end, a refund or a dispute, and releases once. PICK-022 allows this source ("PaymentIntent fetched from Stripe with status succeeded").
- Tested: `tests/integration/tests/connect_moves.rs:830-833` `td_job_18_a_held_success_releases_once_and_connects` (TD-JOB-18: "a succeeded attempt whose webhook never came ... queues one picks.held_success task, whose run releases the pick once").
- Residual, not a defect today: the release comes only after about 1 business hour (overnight if pressed late), and a High `picks.payment_held_long` alert fires after 24h. PICK-048 auto-confirm charges with the job still in Awaiting confirm, so it would never reach this wait, but `auto_confirm` has no production caller until T689 (QUEUED). Its Confirm-window lapse would end the pick and refund through `payments`' pick end. Suggest T689's goal say that an auto-confirmed success with no webhook is covered (or that the job enters Payment pending).

## F2 (critical): partial refund blocks the remainder refund. PARTLY (minor)
- The code is as described (`models.rs:754-778`: `untouched` needs `refunded_cents == 0`). The app itself only sends full refunds (`adapters/payments.rs:755-766`, no amount). A partial refund of Laftly's own that Stripe reports is alerted (D-2303, TD-FEE-60). So the only way in is a manual partial dashboard refund (D-5158) on a closed-job or second charge. A human made that refund, so the remainder should go to Casey, not be refunded automatically.
- Already planned: T796 (QUEUED, D-5254 (4)) raises one High `payments.reconcile_partial_refund` alert for a partial refund on an ENDED pick. Gap: a partly refunded SECOND charge (`!own_intent`) on a pick that has not ended is not in T796's condition.
- Fix: amend T796's goal (a plan change with a decisions entry) so the alert condition is `pick.ended || !pick.own_intent`, i.e. the same predicate as the refund. Files: `app/payments/src/reconcile.rs` (T796 owns it). No DONE test or locked test-design row changes.

## F3 (major): fraud refund before release cannot end the pick or return the job. CONFIRMED (major; lifecycle, not money)
- `fraud_warning` (`app/picks/src/unreachable_refund.rs:546-562`) needs the job in PaymentPending AND the pick `paid` together. Since T663, `settle` sets the pick to `paid` and the job to Connected in one transaction (`settle.rs:113-123`; `connect.rs:133,138`; `settle_step` moves only from AwaitingConfirm, `status.rs:89`). So on main the pair can never hold:
  - Before settle (pick awaiting_confirm, job PaymentPending): the hold is written and the refund succeeds. `end_pick` then returns MovedOn and rolls back the job move. `run_fraud_warning` swallows MovedOn (`unreachable_steps.rs:685-687`), and the refund state is not InFlight, so the task is never rerun. The job stays in Payment pending. `payment_returned` keeps seeing the `succeeded` attempt (`payment_hold.rs:61`, which ignores refunds), restarts the wait and queues `held_success` hourly, and `held_release` refuses each time (refunded). The only outcome is the 24h `payment_held_long` alert. The homeowner is never returned to Quotes ready and gets no backup offer.
  - Between settle commit and release claim (paid + Connected): `move_to` gives MovedOn and returns Ok(false). The job stays Connected with no release, and the business is refunded.
- Money is right (refunded, nothing released). FEE-052's "pick ending as fraud-refunded and the job returning to Quotes ready" is broken.
- Tests pass only on an unreachable planted seed (`'paid'` pick + PaymentPending job): `tests/integration/tests/unreachable_refund.rs:246-253,472-505`, `release_fraud_race.rs:101-108,218-242`.
- Fix: under the pick lock, branch on the real state. (a) awaiting_confirm + PaymentPending: end the pick as FraudRefunded and move PaymentPending to QuotesReady with the backup offer (as `end_pending` does, with a FraudRefunded end). (b) paid + Connected and unreleased: move Connected to QuotesReady (an allowed arrow, `app/jobs/src/status.rs:100`) and end the pick as FraudRefunded. Also make `payment_held` treat a refunded charge as not Success, so the hourly held_success loop stops.
- Files: `app/picks/src/unreachable_refund.rs`, maybe `app/picks/src/connect.rs` (T744 IN_PROGRESS owns it) and `app/payments/src/payment_hold.rs`. DONE tests need their seeds moved to the real states: `unreachable_refund.rs` td_fee_39 and `release_fraud_race.rs` (TD-PICK-59), in a harness task with no assertion weakened. TD-FEE-39 and TD-PICK-59 rows need no change.
- Owner: NEW task (money, Opus, harness for the done tests), after T744.

## F4 (major): dispute before charge evidence is dropped. PARTLY (medium)
- True: `record_dispute` (`disputes.rs:97-106`) gives UnknownCharge, and `money()` maps that to Acted::Unknown, i.e. 200 with no Stripe resend (`routes.rs:470-474`). No dispute row, evidence task or `picks.dispute_opened` follows.
- Reach is narrow. Evidence is written on any recorded success: the webhook, the 30-min check, `held_release`'s `store_success`, or the reconcile rebuild. A dispute normally comes days later. The realistic case is a dispute on a charge Laftly never recorded (a second intent, or success unrecorded through a long Stripe/DB outage).
- Partial safety net: the nightly reconcile compares disputed cents (`reconcile.rs:396-412,888-896`), so a missing dispute raises MISMATCH_ALERT. Stripe itself refuses a refund on a disputed charge.
- Fix: in `record_dispute`, when a `pick_intent` names the intent's pick but no evidence exists yet, return a retryable error (Acted::Retry gives a 500 and Stripe resends for 3 days), mirroring the fraud route's `Found::NotYet`. A dispute on a truly unknown charge should raise a High alert, not a silent 200. Files: `app/payments/src/disputes.rs`, `routes.rs`.
- Owner: amend T381 (QUEUED, owns both files) or T785 (QUEUED, owns both). New test file. No DONE test change.

## F5 (major): matched() ignores failed writes. CONFIRMED (minor)
- `matched()` = `stripe == ledger && malformed == 0` (`reconcile.rs:245-246`), and the mismatch alert is cleared on that (`:309-316`). The failed charge still keeps CHARGE_FAILED_ALERT and OPEN_CHARGE_ALERT, so Casey is not left blind.
- Already planned exactly: T797 (QUEUED, D-5255 (7)). `matched()` will also need `failed == 0`, and the alert will clear on a new `cents_matched()`. No new task.

## F6 (major): later partial refunds not booked (row count vs cents). CONFIRMED (medium)
- `ledger_facts` `has_refund` is a row count (`reconcile.rs:526-540`), and `plan_charge` books a refund only when none exists (`models.rs:770-772`). A second dashboard partial refund is never booked, and the mismatch alert recurs nightly.
- Already planned exactly: T796 (QUEUED, D-5254 (4)): a top-up is booked as `refunded_cents` minus the net refund, under `reconcile.refund.{charge}.{refunded_cents}`. No new task.

## Real defects and owners
1. F3 fraud end unreachable on main: NEW task (picks + payments, Opus money, harness for 2 done tests' seeds; after T744).
2. F4 dispute-before-evidence silently acknowledged: amend T381 or T785.
3. F2 residual (second charge on a live pick, partly refunded): amend T796's alert condition.
4. F5: T797 as planned. F6: T796 as planned.
5. F1 not a defect. Note for T689: an auto-confirm success with no webhook must reach the held-success path.
