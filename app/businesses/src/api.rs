//! The public face of `askhava-businesses`: everything other crates may call (D-481).
//!
//! The business account and its status moves (BIZ-009, BIZ-003, BIZ-016), the towns and the
//! job-type checklist within the license (BIZ-004, BIZ-010), the overflow switch, the Spanish
//! tick, the business-reported insurance answer and the after-hours tick (BIZ-005 to BIZ-008), and
//! the night count for INT-033 (D-491); the self-pause and own quiet times (BIZ-010, D-085); team
//! members with their roles, the main and staff phones kept in `messaging`'s sender directory, and
//! `notice_addresses` (BIZ-023, CALL-012, D-491, D-492); sign-up, the identity chain and activation,
//! the About us screen and the business terms acceptance (BIZ-001 to BIZ-003, BIZ-016, BIZ-019 to
//! BIZ-022, BIZ-053, FEE-011, TERMS-016, TERMS-033; T081); the sign-up screens' records: the
//! masked record contact, the "That number is old" wait and resume, the review target, the fee
//! summary, the trade count and the launch notice's sent-once claim (BIZ-024, BIZ-026, BIZ-027,
//! BIZ-030, BIZ-031; T193); the E-SIGN consent box and records, the welcome request and gate, the
//! business STOP email request, the auto-reply rule and the recruiting-email rules (CON-019,
//! CON-020, BIZ-017, BIZ-032, BIZ-048; T082); complaints with the homeowner's reply, the business's
//! 5-day reply box, the 3-in-90-days pause and the digest listing, and the owner's email request
//! when another member links a phone (BIZ-011, BIZ-012, BIZ-029, BIZ-033; T083).

use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

pub use crate::about_us::{
    ABOUT_US_MAX, AboutUsRefusal, screen as screen_about_us, screen_public_name, set_about_us,
};
pub use crate::channel_preference::{
    ChannelError, NoticeChannel, NoticeRecipient, notice_channel, notice_recipients,
    set_notice_channel,
};
pub use crate::complaint_holds::{COMPLAINT_HOLD_KIND, queue_complaint_holds, run_complaint_holds};
pub use crate::complaints::{
    BOX_CLOSED_KIND, CaseyView, Complaint, ComplaintError, DIGEST_ALERT, NewComplaint, PAUSE_COUNT,
    PAUSE_WINDOW_DAYS, PHONE_LINKED_KIND, REPLY_BOX, REPLY_MAX, Recorded, ReplyFacts, SAFETY_ALERT,
    complaints, for_casey, has_safety_word, list_closed, phone_linked_line, record_complaint,
    reply, run_box_closed,
};
pub use crate::consent::{
    ADVERTISEMENT_LINE, AUTO_REPLY_TEMPLATE, CONSENT_VERSION, ConsentBox, ConsentChannel,
    ConsentError, NO_CALLS_LINE, PHONE_STOPPED_KIND, RecruitingEmail, WELCOME_KIND, auto_reply_due,
    consent_box, consent_wording, may_get_cards, may_recruit, may_text, phone_stopped,
    record_consent, recruiting_email, request_welcome, stop_recruiting, stopped_email_line,
    welcome_lines, welcome_sent,
};
pub use crate::identity::{
    Flag, RecordContacts, Step, approve, check_code, letter_code, next_step, record_match,
    send_code, settle,
};
pub use crate::models::{Business, BusinessesError, Insurance};
pub use crate::rules::JobTypeChoice;
pub use crate::rules::{Checks, Event, OwnHours, Widens};
pub use crate::service::{
    Ticks, apply, create, night_count, own_hours, pause_for_rule, pause_self, resume_self,
    set_job_types, set_own_hours, set_ticks, set_towns,
};
pub use crate::signup::{
    BUSINESS_STATEMENT, ContactKind, EntityValues, FeeNumbers, HeardFrom, LIST_DELAY_KEY,
    MaskedContact, NUMBER_OLD_BUTTON, RECORD_UPDATED_TEMPLATE, REPEAT_FREE_DAYS, REVIEW_TARGET_KEY,
    RecordSeen, SOMEONE_ELSE_LINE, STOP_FIX_BUTTON, SignUpForm, SignedUp, SignupError,
    SignupSettings, TermsShown, WAITLIST_LINE, accept_terms, category_list, claim_launch_notice,
    fee_summary, fix_record_lines, hand_entity_keys, number_is_old, resume_on_new_record,
    review_due, review_target, review_target_line, review_told, sign_up, trade_count,
    waiting_on_record, waitlisted,
};
pub use crate::status::{AccountStatus, PauseReason, Refused};
pub use crate::team::{
    Acting, MEMBER_REMOVED_KIND, NoticePurpose, Roles, SenderLink, TeamError, TeamMember,
    add_member, add_owner, change_phone, members, notice_addresses, remove_member, set_roles,
    unlink_phone,
};

/// The pure status and checklist rules, for the rules bridge and other crates' property tests.
pub mod rules {
    pub use crate::rules::{
        APP_CLOSE_MINUTE, APP_OPEN_MINUTE, checklist, may_notify, may_resume_itself, towns,
    };
    pub use crate::status::next;
}

/// One business's stored record (BIZ-005 to BIZ-009).
///
/// # Errors
///
/// [`BusinessesError::UnknownBusiness`] or [`BusinessesError::Database`].
pub async fn business(db: &mut PgConnection, id: Uuid) -> Result<Business, BusinessesError> {
    crate::models::read(db, id).await
}

/// The towns a business serves, sorted (BIZ-010).
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn towns(db: &mut PgConnection, id: Uuid) -> Result<Vec<String>, BusinessesError> {
    crate::models::towns(db, id).await
}

/// The job types a business ticked, sorted by category then name (BIZ-004).
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn job_types(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Vec<JobTypeChoice>, BusinessesError> {
    crate::models::job_types(db, id).await
}

/// Links a phone as `team::link_phone` does (CALL-012) and, in the same transaction, queues the
/// owner's [`PHONE_LINKED_KIND`] email when the linking member is not the owner (BIZ-033, D-258,
/// D-311). The Approve gate and the owner text are T148's (D-493).
///
/// # Errors
///
/// As `team::link_phone`; [`TeamError::Database`] or `Audit` when the email request fails.
pub async fn link_phone(
    conn: &mut PgConnection,
    dir: &impl SenderLink,
    (by, main): (Acting, bool),
    number: &SecretString,
    now: Timestamp,
) -> Result<Uuid, TeamError> {
    let mut tx = conn.begin().await.map_err(|_| TeamError::Database)?;
    let line = crate::team::link_phone(&mut tx, dir, (by, main), number, now).await?;
    crate::complaints::phone_link_notice(&mut tx, by, line, now)
        .await
        .map_err(|e| match e {
            ComplaintError::Audit => TeamError::Audit,
            _ => TeamError::Database,
        })?;
    tx.commit().await.map_err(|_| TeamError::Database)?;
    Ok(line)
}
