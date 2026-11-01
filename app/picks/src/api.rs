//! The public face of `askhava-picks`: everything other crates may call (D-481).
//!
//! T054: [`send_code`] and [`check_code`] are the page pick's Verify code (PICK-013, PICK-014,
//! PICK-055); [`text_pick`] handles a "<number> YES" text inside `intake`'s transaction (PICK-015,
//! PICK-016, PICK-031, PICK-032, PICK-036); [`withdraw_paused`] withdraws a paused job's waiting
//! pick (CON-029); [`terms_line_sent`] and [`record_terms_line`] read and write the PICK-016 Terms
//! line record; [`terms_line`] is the assent line a text pick accepts.

pub use crate::models::PicksError;
pub use crate::service::{
    CONFIRMED_TEMPLATE, CodeCheck, CodeSent, CodeTo, DONE_TEMPLATE, NO_QUOTE_TEMPLATE,
    NONE_TEMPLATE, PICK_MADE_KIND, PICK_WITHDRAWN_KIND, TERMS_TEMPLATE, TextPickDone, TextPickIn,
    VERIFY_CAP_ALERT, WAITING_TEMPLATE, WHICH_TEMPLATE, check_code, record_terms_line, send_code,
    terms_line, terms_line_sent, text_pick, withdraw_paused,
};

/// The pure pick rules, for other crates and property tests.
pub mod rules {
    pub use crate::rules::{
        CALL_QUIET_FROM, CALL_QUIET_UNTIL, CALLS_PER_NUMBER, CODE_TRIES, CODE_VALID, COOLDOWN,
        CodeRow, CodeState, CodeVia, JobView, PickCode, Plan, SENDS_PER_JOB, SENDS_PER_NUMBER,
        SendAsk, SendCounts, SendRefused, Stage, TextPick, code_state, parse_code, plan_text_pick,
        read_text_pick, send_allowed,
    };
}
