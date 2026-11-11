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
//! when another member links a phone (BIZ-011, BIZ-012, BIZ-029, BIZ-033; T083); the sign-up inbox check,
//! the sample card and sample pick, and each person's backup notice address, which
//! `notice_addresses` adds (BIZ-042, BIZ-043; T149); the "Emergency emails at any hour" switch,
//! its test emergency email, the after-hours tick's conditions and the covered night count
//! (BIZ-007, BIZ-037, BIZ-042, INT-033; T150); the licensing holder read, [`holding`] (LIC-019,
//! BIZ-049, CARD-001; T374); the business's own names and its checked Google and Yelp links
//! (BIZ-022, BIZ-052, PICK-041, SEC-041; T165).

use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

pub use crate::about_us::{
    ABOUT_US_MAX, AboutUsRefusal, screen as screen_about_us, screen_public_name, set_about_us,
};
pub use crate::backup_address::{
    BACKUP_CHANGED_KIND, backup_changed_words, confirm_backup, notice_addresses, remove_backup,
    request_backup,
};
pub use crate::capability::{
    Capability, CapabilityAnswers, LARGEST_JOB_MAX_CENTS, LICENSE_CHANGE_KIND, LargestJob,
    OfferRow, Reapplied, TradeOffer, capabilities, checklist_offer, is_recurring, kept_ticks,
    license_change, license_change_words, pre_ticked, reapply_license, save_capability,
    whole_dollars,
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
    AUTO_REPLY_TEMPLATE, CONSENT_VERSION, CONTACT_CARD_PATH, ConsentBox, ConsentChannel,
    ConsentError, PHONE_STOPPED_KIND, RecruitingEmail, WELCOME_KIND, advertisement_line,
    auto_reply_due, consent_box, consent_wording, may_get_cards, may_recruit, may_text,
    no_calls_line, phone_stopped, record_consent, recruiting_email, request_welcome,
    stop_recruiting, stopped_email_line, welcome_card_line, welcome_lines, welcome_sent,
};
pub use crate::deliverability_check::{
    EmailCheck, InboxAnswer, PICK_ALERT_YES_LIFE, SAMPLE_CARD_KIND, SAMPLE_PICK_KIND, email_check,
    offer_sample_pick, queue_pick_retests, record_inbox_answer, record_ok_reply, record_pick_alert,
    sample_card_words, sample_pick_words, send_sample_card, send_sample_pick,
};
pub use crate::email_verification::{
    ACTED_BY_ACTION, CHANGED_KIND, CHECK_TRIES, CODE_LIFE, EmailCheckError, RETURNED_KIND,
    Returned, SOFT_SPAN_DAYS, STEP_UP_LIFE, confirm as confirm_mailbox, log_notice_change,
    mark_returned, may_get_notices, record_step_up, request_change as request_notice_change,
    returned_line, returned_mailboxes, start_signup_check, verified_by_link,
};
pub use crate::emergency_switch::{
    AFTER_HOURS_OFF_KIND, EMERGENCY_TEST_KIND, EMERGENCY_YES_LIFE, MISSED_IN_A_ROW, NightPerson,
    OFF_REMOVED, OFF_SUSPENDED, SwitchError, SwitchState, VISIT_WITHIN, covered,
    emergency_test_words, missed_in_a_row, night_count as covered_night_count, night_people,
    queue_emergency_retests, recheck_after_hours, record_alert_answer, send_emergency_test,
    set_after_hours, set_emergency_emails, suspend_after_hours, switch_off_warns, switch_state,
    yes_current,
};
pub use crate::holding::{HANDYMAN_CATEGORY, Known, holding};
pub use crate::identity::{
    Flag, IDENTITY_CODE_TRIES, RecordContacts, Step, approve, check_code, episode_letter_code,
    letter_code, next_step, record_match, send_code, send_code_seen, settle,
};
pub use crate::links::{
    BusinessNames, CITY_LICENSE_MAX, GOOGLE_HOSTS, GuardedOpener, LINK_MAX, LinkOpener,
    LinkRefusal, LinksError, NAME_MAX, NameRefusal, ReviewLinks, ReviewSite, YELP_HOSTS,
    business_names, check_links, on_site, review_links, save_business_name, save_review_links,
    screen_names,
};
pub use crate::models::{Business, BusinessesError, Insurance};
pub use crate::open_cards_times::{
    OpenCardsTime, OpenCardsTimes, open_cards_times, set_open_cards_times,
};
pub use crate::rules::JobTypeChoice;
pub use crate::rules::{Checks, Event, OwnHours, Widens};
pub use crate::service::{
    Ticks, apply, create, night_count, own_hours, pause_for_rule, pause_self, resume_self,
    set_job_types, set_own_hours, set_ticks, set_towns,
};
pub use crate::signup::{
    ContactKind, EntityValues, FeeNumbers, HeardFrom, LIST_DELAY_KEY, MaskedContact,
    NUMBER_OLD_BUTTON, RECORD_UPDATED_TEMPLATE, REPEAT_FREE_DAYS, REVIEW_TARGET_KEY, RecordSeen,
    SOMEONE_ELSE_LINE, STOP_FIX_BUTTON, SignUpForm, SignedUp, SignupError, SignupSettings,
    TermsShown, accept_terms, business_statement, category_list, claim_launch_notice, fee_summary,
    fix_record_lines, hand_entity_keys, number_is_old, resume_on_new_record, review_due,
    review_target, review_target_line, review_told, sign_up, trade_count, waiting_on_record,
    waitlist_line, waitlisted,
};
pub use crate::status::{AccountStatus, PauseReason, Refused};
pub use crate::team::{
    Acting, MEMBER_REMOVED_KIND, NoticePurpose, Roles, SenderLink, TeamError, TeamMember,
    add_member, add_owner, change_phone, members, remove_member, set_roles, unlink_phone,
};

/// SEC-038, D-480: whether `by`, a current member of `by.business`, passed a Verify code (to their
/// verified phone, or email without a texting phone) no more than [`STEP_UP_LIFE`] before `now`;
/// a time after `now` is not fresh (T085, D-1850).
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn has_fresh_step_up(
    conn: &mut PgConnection,
    by: Acting,
    now: Timestamp,
) -> Result<bool, BusinessesError> {
    let q = sqlx::query_scalar!(
        "SELECT step_up_at FROM businesses.team_member \
         WHERE id = $1 AND business_id = $2 AND removed_at IS NULL",
        by.member,
        by.business
    );
    let at = q
        .fetch_optional(conn)
        .await
        .map_err(|_| BusinessesError::Database)?
        .flatten()
        .map(|t| t.to_jiff());
    Ok(at.is_some_and(|at| at <= now && now.duration_since(at) <= STEP_UP_LIFE))
}

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
