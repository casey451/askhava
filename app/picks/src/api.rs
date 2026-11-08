//! The public face of `askhava-picks`: everything other crates may call (D-481).
//!
//! T054: [`send_code`] and [`check_code`] are the page pick's Verify code (PICK-013, PICK-014,
//! PICK-055); [`text_pick`] handles a "<number> YES" text inside `intake`'s transaction (PICK-015,
//! PICK-016, PICK-031, PICK-032, PICK-036); [`withdraw_paused`] withdraws a paused job's waiting
//! pick (CON-029); [`terms_line_sent`] and [`record_terms_line`] read and write the PICK-016 Terms
//! line record; [`terms_line`] is the assent line a text pick accepts.
//!
//! T062: [`run_card_fix_notice`] turns `payments`' card-fix event into the business's notice
//! (FEE-020, D-492); T068: [`run_receipt_notice`] turns its receipt event into the receipt email
//! request (FEE-031).
//!
//! T063: [`run_pick_notice`] queues the pick notice (PICK-037); [`notice_sent`] starts the Confirm
//! window from the notice's real send and [`run_confirm_ends`] ends it (PICK-019);
//! [`check_at_pick`] and [`check_at_confirm`] are the license and account re-checks (PICK-020);
//! [`fee_line`] the recurring fee line (PICK-034); [`read_business_word`] and [`business_reply`]
//! the answer to a business's YES or CONFIRM text (PICK-033, PICK-035).
//!
//! T190: [`hold`] and [`hold_allowed`] (PICK-043), [`away_until`], [`away_for`] and
//! [`window_start`] (BIZ-034), [`emergency_plan`] and [`emergency_fix_by`] (PICK-044), and
//! [`read_time_word`] for HOLD, ESPERA, AWAY and AUSENTE texts, with [`WindowRules`] lengths.
//!
//! T197: [`reminders`] and [`emergency_reminders`] time the Confirm reminders, sent once through
//! [`claim_reminder`] with [`reminder_lines`]; [`bounce_grace`] and [`extend_once`] extend a window
//! once (PICK-019, PICK-037, PICK-043); [`lapse_plan`] and [`lapse_text`] are the PICK-046 text;
//! [`owner_told`] and [`member_confirmed_line`] the BIZ-033 email; [`window_open`] the Confirm edge.
//! T150: [`emergency_pick_plan`] sends an emergency pick at night only to people with "Emergency
//! emails at any hour" on, under [`emergency_pick_subject`] (PICK-044, BIZ-037, CARD-021).
//!
//! T069: [`release`] shows a proven pick's details in the portal once and queues the
//! [`release_notice`] (PICK-022 to PICK-024, PRIV-005, BIZ-014); [`plan_release`] is its pure rule
//! (INT-029).
//!
//! T253: [`released_view`] shows a released pick's details again to its signed-in business owner
//! or "Confirm and billing" member, read from the vault at each view and stored nowhere; [`may_view`]
//! is its pure rule (PICK-024, PRIV-005, D-1739).
//!
//! T246: [`page_pick`] stores the quote page's pick on the text pick's core (PICK-012, PICK-032,
//! PICK-045); [`send_page_code`] and [`check_page_code`] send and check its Verify code to the
//! homeowner's vaulted phone, which they read themselves (PICK-013, PRIV-003, D-1516).
//!
//! T070: [`lost_notices`] queues the loss notice to every other quoter once the pick is paid or
//! free, worded by [`compare`] and [`loss_template`] (PICK-026, PICK-038); [`no_longer_active`]
//! and [`refused_after_confirm`] send "This job is no longer active." once; [`live_pick_reply`]
//! answers a business asking about its live pick (PICK-027).
//!
//! T095: [`code_route`] and [`send_email_code`]/[`check_email_code`] send and check the pick code
//! by email to a [`ConversationEmail`] when texting is off (PICK-029); [`opted_out_yes`] takes an
//! opted-out homeowner's "<number> YES" with no text back (PICK-030).
//!
//! T191: [`decide_at_96_hours`] refunds through `payments` and closes the job (FEE-022, JOB-027);
//! [`missed_call_text`] and [`queue_reminders`] send TXT-030 and TXT-026; [`no_call_at_72_hours`]
//! returns a call-contact job (JOB-013); [`dispute_auto_submit`] (FEE-050) and [`fraud_warning`]
//! (FEE-052).

pub use crate::aftermath::{
    Comparison, INACTIVE_TEMPLATE, LIVE_PICK_TEMPLATE, LOST_HIGHER_TEMPLATE, LOST_LOWER_TEMPLATE,
    LOST_OTHER_TYPE_TEMPLATE, LOST_SAME_TEMPLATE, LOST_SEVERAL_HIGHER_TEMPLATE,
    LOST_SEVERAL_LOWER_TEMPLATE, LOST_SEVERAL_SAME_TEMPLATE, LOST_TEMPLATE, LiveReply,
    LossSettings, compare, live_pick_reply, loss_template, lost_notices, no_longer_active,
    refused_after_confirm,
};
pub use crate::confirm::{
    BusinessWord, ConfirmReply, ConfirmSettings, EXPIRED_TEMPLATE, FEE_REPLY_TEMPLATE, FeeBasis,
    LicenseAtPick, PICK_NOTICE_EMERGENCY_TEMPLATE, PICK_NOTICE_TEMPLATE, Refusal, StoredFacts,
    WINDOW_KEY, business_reply, check_at_confirm, check_at_pick, confirm_by, fee_line,
    license_at_pick, read_business_word,
};
pub use crate::confirm::{EmailOutcome, bounce_grace};
pub use crate::confirm::{
    EmergencyPlan, HoldAsk, NightLine, TimeRefusal, TimeWord, WindowRules, away_for, away_until,
    emergency_fix_by, emergency_plan, hold, hold_allowed, read_time_word, window_start,
};
pub use crate::confirm_reminders::{
    Extension, LapsePlan, Reminder, claim_reminder, emergency_reminders, extend_once, lapse_plan,
    lapse_text, member_confirmed_line, owner_told, reminder_lines, reminders, window_open,
};
pub use crate::email_code::{
    CodeRoute, ConversationEmail, EmailFrom, OptedOutYes, Texting, check_email_code, code_route,
    opted_out_yes, send_email_code,
};
pub use crate::emergency_emails::{EmergencyPick, emergency_pick_plan, emergency_pick_subject};
pub use crate::handlers::{CONFIRM_ENDS_KIND, notice_sent, run_confirm_ends, run_pick_notice};
pub use crate::models::PicksError;
pub use crate::money_notices::{
    BUSINESS_NOTICE_KIND, CARD_APPROVAL_TEMPLATE, CARD_FIX_TEMPLATE, RECEIPT_NO_RECIPIENT_ALERT,
    run_card_fix_notice, run_receipt_notice,
};
pub use crate::open_cards_email::{
    ComposedEmail, OpenCard, WaitingPick, chosen_minutes, missed_cards_line, open_cards_email,
};
pub use crate::release::{
    ADDRESS_NEEDED_KIND, EMAIL_CHOSEN_FIELDS, RELEASE_FIELDS, RELEASE_LINE_FAILED_ALERT,
    RELEASE_NO_RECIPIENT_ALERT, RELEASE_TEMPLATE, RELEASE_TEXT_TEMPLATE, ReleaseFacts, ReleaseStep,
    ReleaseView, Released, plan_release, release, release_notice,
};
pub use crate::released_view::{Seat, ViewAnswer, ViewFacts, may_view, released_view};
pub use crate::service::{
    CONFIRMED_TEMPLATE, CodeCheck, CodeSent, CodeTo, DONE_TEMPLATE, NO_QUOTE_TEMPLATE,
    NONE_TEMPLATE, PICK_MADE_KIND, PICK_WITHDRAWN_KIND, PageCodeKeys, PagePick, TERMS_TEMPLATE,
    TextPickDone, TextPickIn, VERIFY_CAP_ALERT, WAITING_TEMPLATE, WHICH_TEMPLATE, check_code,
    check_page_code, page_pick, record_terms_line, send_code, send_page_code, terms_line,
    terms_line_sent, text_pick, waiting_quote, withdraw_paused,
};
pub use crate::unreachable_refund::{
    AtNinetySix, MISSED_CALL_TEMPLATE, REMINDER_EMAIL_TEMPLATE, REMINDER_TEMPLATE, REMINDERS_AFTER,
    ReleasedPick, auto_submits, call_record, decide_at_96_hours, decision_template,
    dispute_auto_submit, fraud_warning, missed_call_at, missed_call_text, no_call_at_72_hours,
    queue_reminders, reminder_template, reminder_times, send_at,
};

/// The pure pick rules, for other crates and property tests.
pub mod rules {
    pub use crate::rules::{
        CALL_QUIET_FROM, CALL_QUIET_UNTIL, CALLS_PER_NUMBER, CODE_TRIES, CODE_VALID, COOLDOWN,
        CodeRow, CodeState, CodeVia, JobView, PickCode, Plan, SENDS_PER_JOB, SENDS_PER_NUMBER,
        SendAsk, SendCounts, SendRefused, Stage, TextPick, code_state, parse_code, plan_text_pick,
        read_text_pick, send_allowed,
    };
    /// Every pick status.
    pub use crate::status::PickStatus;
    /// Whether a released pick's status still shows its details (D-1739).
    pub use crate::status::shows_release;
    /// A job status's text-pick stage; its Ready and Waiting equal `JobStatus::takes_pick`
    /// (TD-PICK-45, D-1516).
    pub use crate::status::stage;
}
