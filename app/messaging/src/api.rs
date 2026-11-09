//! The public face of `askhava-messaging`: everything other crates may call (D-481).
//!
//! Render a text from its template ID, pick the language it may use, count its segments, decide
//! when it may be sent (quiet hours), queue and send it through the outbox once (TXT-009,
//! TXT-011), keep the sender directory of business phones (D-492), record Twilio's inbound texts
//! and status callbacks once per message ID (TXT-012), read a text's delivery status (TXT-013),
//! keep the one suppression list and send only through its check (CON-008 to CON-017, CON-030,
//! CON-035), answer HELP, INFO and AYUDA (CON-018), and tell a module above it when a homeowner
//! text may go without handing it her number (`send_window`, QH-001, QH-006, D-1301); retry a
//! failed text and then email it or raise a call-back (TXT-014, TXT-015); queue and send every
//! email once in the SEC-028 layout, holding bulk email above the daily limit's share (OPS-038);
//! keep a homeowner's update channel (TXT-018); and decide web-and-email mode, a suspended
//! number's routes and the one configured number per audience (`mode`, TXT-021 to TXT-024, CON-033),
//! store each line's switch, suspension and launch channel and follow up an emergency while paused
//! (`mode_state`, T279);
//! read a message log entry and cut bodies kept 30 days (`retention`, TXT-019, TXT-020); keep each
//! job's notice log (`email_timing`, BIZ-045, OPS-037) and decide the open-cards and missed-cards
//! emails (`open_cards_digest`, CARD-022; `missed_cards`, BIZ-044).

pub use crate::business_notices::{
    BUSINESS_NOTICE_KIND, JOB_CARD_TEMPLATE, LINK_LIFE, NoticeOutcome, NoticeSender, SIGN_IN_URL,
    SendChecks, business_notice, card_notice_accepted, held_until, sign_in_link,
};
pub use crate::email::{
    COMPLAINT_ALERT, DailyLimit, EMAIL_ACTION, EmailClass, EmailEvent, EmailSent, EmailTo, FROM,
    MailboxKind, NewEmail, Notice, PostalAddress, REPLY_TO, Recipient, Rendered, UpdateChannel,
    WEBHOOK_GRACE, WEBHOOKS_SILENT_ALERT, check_webhook_silence, email_log, email_status,
    next_reset, queue_email, receive_email_event, render as render_email, send_email,
    set_update_channel, update_channel,
};
pub use crate::email_timing::{
    NewNoticeLog, NoticeEntry, NoticeKind, ProviderStatus, Sending, mark_sent, notice_log,
    record_acted, record_first_open, record_notice,
};
pub use crate::handlers::{Callback, receive_inbound, receive_status};
pub use crate::keywords::{
    HelpSettings, HelpSettingsError, Keyword, help_text, keyword, reply_to_help,
};
pub use crate::missed_cards::{MissedSettings, MonthCards, first_of_month, missed_email_due};
pub use crate::mode::{
    EmergencyFacts, EmergencyPlan, LaunchChannel, Line, LineSendError, Lines, ModeError,
    NumberState, PAUSED_EMERGENCY_ALERT, Raised, Refusal, SendingNumber, TollFree, UpdateRoute,
    emergency_plan, raise_emergency, send_on_line, update_route,
};
pub use crate::mode_state::{
    ModeStateError, PAUSED_SAFETY_EMAIL, PAUSED_SAFETY_KIND, SafetySender, launch_channel,
    load_lines, queue_paused_safety, record_restore, record_suspension, run_paused_safety,
    set_launch_channel, switch_off, switch_on,
};
pub use crate::models::{EmailError, EmailLog, InboxError, OutboxError};
pub use crate::open_cards_digest::{DigestFacts, MIN_LEFT, digest_goes, listable, slot_due};
pub use crate::optout::{
    InboundText, Outcome, RevokeChannel, on_inbound_text, send_checked, suppress,
};
pub use crate::purge::{
    BODY_DELETE_AFTER, DELETE_FAILED_ALERT as BODY_DELETE_FAILED_ALERT,
    DELETE_KIND as BODY_DELETE_KIND, SCAN_KIND as BODY_SCAN_KIND, queue_body_deletes,
    register_body_deletes, run_body_delete, schedule_body_scan,
};
pub use crate::receipt_email::{
    RECEIPT_EMAIL_KIND, RECEIPT_TEMPLATE, ReceiptFacts, ReceiptSender, WHY_CHARGED_PREFIX,
    render_receipt, run_receipt_email,
};
pub use crate::retention::{
    BODY_KEPT_FOR, CUT_KIND, LogEntry, cut_bodies, message_log, run_cut, schedule_cut,
};
pub use crate::retries::{
    CALL_BACK_ALERT, GaveUp, IMPORTANT_TEMPLATES, NO_RUSH_CALL_BACK, RETRY_AFTER, Reach, Step,
    TEXT_FAILED_EMAIL, Urgency, call_back_due, give_up, next_step, record_failure,
};
pub use crate::rules::{
    APP_OPT_OUT_WORDS, AreaCodes, CONFIRM_WITHIN, CONFIRMATION_TEMPLATE, Channel, Classified,
    Decision, Detected, HELP_TEMPLATE, Held, Kind, Listing, OPT_OUT_PHRASES, Outgoing,
    PROVIDER_OPT_OUT_WORDS, ProviderKeyword, QuietHoursError, RESUBSCRIBE_WORDS, Release,
    SPANISH_OPT_OUT_WORDS, classify, decide, detect_opt_out, may_send_to, plan_release, retry,
};
pub use crate::send_window::{SendWindow, send_window, window};
pub use crate::service::{
    NewText, Relation, SEND_ACTION, To, delivery_status, enqueue, may_text, send, sender_add,
    sender_business, sender_remove,
};
pub use crate::status::DeliveryStatus;
pub use crate::templates::{
    Audience, Encoding, Language, Registry, Segments, TemplateError, Text, count_segments,
};

/// Renders template `id` for a person whose wanted language is `wanted`, filling each `{name}` from
/// `values`. Spanish is used only while `spanish_conversations_on` is true (TXT-039, the owner-portal
/// setting) or the Spanish template is marked `spanish_stays_on`; a Spanish text is marked a draft
/// (TXT-003). The language is the person's own, so an owner phone in English and a team phone in
/// Spanish are rendered separately (BIZ-018, TXT-002).
///
/// # Errors
///
/// [`TemplateError::Unknown`] for an ID not in the registry, [`TemplateError::MissingValue`] when a
/// placeholder has no value, [`TemplateError::Invalid`] if the compiled-in files are broken.
pub fn render(
    id: &str,
    wanted: Language,
    spanish_conversations_on: bool,
    values: &[(&str, &str)],
) -> Result<Text, TemplateError> {
    Registry::embedded()?.render(id, wanted, spanish_conversations_on, values)
}

/// Every template ID in the compiled-in registry, sorted (TXT-001).
///
/// # Errors
///
/// [`TemplateError::Invalid`] if the compiled-in files are broken.
pub fn template_ids() -> Result<Vec<String>, TemplateError> {
    Ok(Registry::embedded()?.ids())
}
