//! The public face of `askhava-intake`: everything other crates may call (D-481).
//!
//! The first reply to a request (CON-001 to CON-004, CON-026, CON-027, CON-031): [`receive_text`]
//! opens a request with its consent text, after the 911 text on a 911-list match, and hands every
//! later reply to the conversation; its consent record and the gates on cards and texts (CON-005,
//! CON-007, CON-021); the homeowner terms acceptance at a pick (TERMS-001, TERMS-016, TERMS-033);
//! the row types of the `intake` tables (D-492, D-493); the one INT-004 911 text, sent at any hour
//! within [`SAFETY_WITHIN`] (D-436); the 911-list screen on every message, the SAFE hold, the
//! INT-037 question, "How soon?", the INT-033 night count and the emergency-or-regular choice and
//! release (INT-002 to INT-006, INT-024 to INT-026, INT-029, INT-030, INT-033, INT-037); the
//! keyword second opinion, the trade question and INT-032's two trades (AI-011, INT-012, INT-013,
//! INT-015, INT-032, T042); "Which fits best?" and its reply (INT-014, CARD-001, T183); the
//! no-reply reminder and close, the LIST reply and the call-back clock (INT-019, INT-021, INT-023,
//! TXT-031, T048); closing a job on CLOSE, the close button or a cancel-intent message, and
//! "Close which?" (JOB-018 to JOB-023, T049); an opt-out's live jobs: no status change, the CANCEL
//! pause and the CON-028 event (CON-012, CON-028, CON-029, JOB-020, T050).

pub use crate::closing::{
    CLOSE_MENTION_TEMPLATE, CLOSE_WHICH_TEMPLATE, Closing, JOB_CLOSED_TEMPLATE,
    JOBS_CLOSED_TEMPLATE, answer_close, close_from_page,
};
pub use crate::conversation::{
    Answer, JobStart, Opened, Recorded, Taken, ask_name_address, begin_job, hear, joining_job,
    record_answer, stop_if_under_18, take_name_address,
};
pub use crate::emergency_count::{
    CHOICE_TEMPLATE, NIGHT_MANY_TEMPLATE, NIGHT_NONE_TEMPLATE, NIGHT_ONE_TEMPLATE, Offer, count,
    is_night, offer,
};
pub use crate::handlers::{
    CALL_BACK_ALERT, CALL_BACK_DAYS, CALL_BACK_URGENT_ALERT, CLOSE_AFTER, CLOSED_TEMPLATE,
    CallBack, HEAR_BACK_WITHIN, ListAsk, NO_REPLY_KIND, NoReply, REMIND_AFTER, REMINDER_TEMPLATE,
    Wait, answer_list, call_back_due, counts_urgent, list_ask, no_reply_step, queue_call_back,
    run_no_reply, texting_window,
};
pub use crate::matching::{
    Candidates, FitStep, KeywordReading, Opinion, Reading, TradeAnswer, TradeQuestion, TradeStep,
    candidates, job_types_of, keyword_reading, second_opinion, starts_second, trade_label,
    two_trades,
};
pub use crate::models::{
    ConsentRecordRow, ConversationRow, EmergencyEventRow, HomeownerRow, IntakeAnswerRow,
    IntakeError, PropertyRow, RecruitingDemandRow, TermsAcceptanceRow,
};
pub use crate::routing::{
    COUNT_MANY_TEMPLATE, COUNT_ONE_TEMPLATE, NOT_COVERED_TEMPLATE, OUT_OF_AREA_TEMPLATE, Outcome,
    Request, Routing, SPLIT_TEMPLATE, Town, TownRefused, Why, apply as apply_routing,
    asks_property_management, check_towns, is_split_reply, move_refused, recruiting_demand, route,
    split_request, tell_coverage, trade_is_open,
};
pub use crate::safety_text::{SAFETY_WITHIN, safety_outgoing, safety_text, sent_in_time};
/// The first-reply step without the consent record, for this crate's own tests only.
#[cfg(test)]
pub(crate) use crate::service::on_text;
pub use crate::service::{
    Answered, Asked, BEEP_TEMPLATE, EMERGENCY_RELEASED_KIND, HOW_SOON_TEMPLATE, Handled,
    NEEDED_BY_TEMPLATE, OPTED_OUT_LIVE_KIND, OptOutJobs, Queued, Screened, TermsAcceptance,
    TermsChannel, TextIn, Trade, answer_choice, answer_how_soon, ask_how_soon, card_allowed,
    consent_records, conversation, disclosure, emergency_events, how_soon, may_text,
    offer_emergency_choice, on_opt_out, paused_by_homeowner, receive_answer, receive_text,
    record_terms_acceptance, release_emergency, safe_hold, sole_cancel, terms_acceptances,
};

/// The pure first-reply rules, for the rules bridge and other crates' property tests.
pub mod rules {
    pub use crate::emergency::{
        Check, Choice, Hazard, Urgency, beep_is_alarm, check, choice, is_safe_reply, urgency,
    };
    pub use crate::rules::{
        AgeStop, CONSENT_CALL_TEMPLATE, CONSENT_TEMPLATE, CloseReading, CloseTarget,
        DISCLOSURE_TEMPLATE, Inbound, JOIN_SHARE_PERCENT, JOIN_WINDOW, NAME_ADDRESS_TEMPLATE, Plan,
        SAFETY_TEMPLATE, Send, Settings, Then, UNDER_18_TEMPLATE, accepts_word_answer, age_stop,
        asks_who, cancel_like, close_reading, consent_language, consent_template, follow_up_joins,
        joins_open_job, language_code, language_from_code, number_named, opens_job, pick_option,
        plan, reads_spanish, shares_words, split_name_address, wanted_language,
    };
}
