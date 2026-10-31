//! Row types and queries of the `intake` schema (migration `0053_intake_tables.sql`, D-492, D-493).
//!
//! One row type per table; T181 added the consent and terms records (`0054_intake_terms_acceptance.sql`);
//! T039, T040 and T043 add their queries here. Every row holds IDs, codes
//! and times only, never a contact value or homeowner text (PRIV-001, PRIV-028).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

/// Why an intake call failed. No variant carries a value from a message (rust brain R5, R6).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IntakeError {
    /// A database call failed.
    #[error("intake database call failed")]
    Database,
    /// A compiled-in text template is missing or broken.
    #[error("intake text template failed")]
    Template,
    /// The outbox refused or failed to queue a text.
    #[error("intake could not queue a text")]
    Outbox,
    /// The audit row could not be written (JOB-005).
    #[error("intake audit row failed")]
    Audit,
    /// No request has the given ID.
    #[error("intake request not found")]
    UnknownRequest,
}

/// `intake.homeowner`: one person by vault contact ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeownerRow {
    /// Row ID.
    pub id: Uuid,
    /// The vault contact (`privacy.contact`), never the number.
    pub contact_id: Uuid,
    /// When the first request made the row.
    pub created_at: Timestamp,
}

/// `intake.property`: only the vault contact ID and the town (PRIV-028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyRow {
    /// Row ID.
    pub id: Uuid,
    /// The homeowner.
    pub homeowner_id: Uuid,
    /// The vault contact holding the address.
    pub contact_id: Uuid,
    /// The catalog town code.
    pub town: String,
    /// When it was recorded.
    pub created_at: Timestamp,
}

/// `intake.conversation`: one request's conversation and its consent text (CON-026, CON-027).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationRow {
    /// Row ID, the subject of its consent text's outbox key.
    pub id: Uuid,
    /// The homeowner.
    pub homeowner_id: Uuid,
    /// The `jobs.job` once opened (T040).
    pub job_id: Option<Uuid>,
    /// `en` or `es`, the consent text's language (CON-026).
    pub language: String,
    /// The `messaging.inbox` row that opened it.
    pub first_inbox_id: Uuid,
    /// The consent template sent (CON-031); the consent-text version is in the consent record.
    pub consent_template: String,
    /// When the consent text was queued, in the transaction that opened the request.
    pub consent_sent_at: Timestamp,
    /// When the request opened.
    pub opened_at: Timestamp,
    /// When it closed; at most one open per homeowner.
    pub closed_at: Option<Timestamp>,
}

/// `intake.consent_record`: one consent (CON-005, CON-022), append-only; `receive_text` writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentRecordRow {
    /// Row ID.
    pub id: Uuid,
    /// The homeowner.
    pub homeowner_id: Uuid,
    /// The request.
    pub conversation_id: Uuid,
    /// `sms` or `web_chat`.
    pub channel: String,
    /// The inbound text (channel `sms`).
    pub inbox_id: Option<Uuid>,
    /// The web-chat session (channel `web_chat`, CON-025).
    pub web_chat_session: Option<Uuid>,
    /// The consent-text version shown.
    pub consent_version: String,
    /// When consent was given.
    pub consented_at: Timestamp,
}

/// `intake.terms_acceptance`: one homeowner terms acceptance at a pick (TERMS-016, TERMS-033).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermsAcceptanceRow {
    /// Row ID.
    pub id: Uuid,
    /// The homeowner.
    pub homeowner_id: Uuid,
    /// The request.
    pub conversation_id: Uuid,
    /// The pick that accepted; one acceptance per pick.
    pub pick_id: Uuid,
    /// The document code, such as `homeowner_terms`.
    pub document: String,
    /// The terms version.
    pub version: String,
    /// SHA256 of the exact published text, 64 lower-case hex digits.
    pub text_sha256: String,
    /// `en` or `es`.
    pub language: String,
    /// `quote_page` or `text_pick`.
    pub channel: String,
    /// The assent line shown beside the pick.
    pub assent_line: String,
    /// The inbound "<number> YES" text (channel `text_pick` only).
    pub inbox_id: Option<Uuid>,
    /// When the pick accepted.
    pub accepted_at: Timestamp,
}

/// `intake.intake_answer`: one answer to one question (T040).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeAnswerRow {
    /// Row ID.
    pub id: Uuid,
    /// The request.
    pub conversation_id: Uuid,
    /// The question bank ID.
    pub question_id: String,
    /// The option code chosen, if a choice.
    pub choice: Option<String>,
    /// The inbound text that answered.
    pub inbox_id: Option<Uuid>,
    /// When it was answered.
    pub answered_at: Timestamp,
}

/// `intake.emergency_event`: one 911-list match (INT-004, T039).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmergencyEventRow {
    /// Row ID.
    pub id: Uuid,
    /// The request, once known.
    pub conversation_id: Option<Uuid>,
    /// The inbound text that matched.
    pub inbox_id: Uuid,
    /// The list entry code that matched.
    pub matched: String,
    /// When the 911 text was queued.
    pub text_queued_at: Option<Timestamp>,
    /// When the person replied SAFE (INT-005).
    pub safe_at: Option<Timestamp>,
    /// When it was recorded.
    pub created_at: Timestamp,
}

/// `intake.recruiting_demand`: a request no business covers yet (T043, OPS-041).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecruitingDemandRow {
    /// Row ID.
    pub id: Uuid,
    /// The request.
    pub conversation_id: Uuid,
    /// The town code.
    pub town: String,
    /// The trade code.
    pub trade: String,
    /// The job type code, if known.
    pub job_type: Option<String>,
    /// When it was recorded.
    pub created_at: Timestamp,
}

/// The open conversation of the homeowner with vault contact `contact`, if any.
pub(crate) async fn open_conversation(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<Option<Uuid>, IntakeError> {
    sqlx::query_scalar!(
        "SELECT c.id FROM intake.conversation c JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE h.contact_id = $1 AND c.closed_at IS NULL",
        contact
    )
    .fetch_optional(db)
    .await
    .map_err(|_| IntakeError::Database)
}

/// The homeowner row for `contact`, made at `now` if new.
pub(crate) async fn homeowner_for(
    db: &mut PgConnection,
    contact: Uuid,
    now: Timestamp,
) -> Result<Uuid, IntakeError> {
    sqlx::query_scalar!(
        "INSERT INTO intake.homeowner (contact_id, created_at) VALUES ($1, $2) \
         ON CONFLICT (contact_id) DO UPDATE SET contact_id = EXCLUDED.contact_id RETURNING id",
        contact,
        now.to_sqlx() as _
    )
    .fetch_one(db)
    .await
    .map_err(|_| IntakeError::Database)
}

/// Opens a request with its consent text queued now; `None` when one is already open (CON-027).
pub(crate) async fn open(
    db: &mut PgConnection,
    (homeowner, inbox): (Uuid, Uuid),
    (language, template): (&str, &str),
    now: Timestamp,
) -> Result<Option<Uuid>, IntakeError> {
    sqlx::query_scalar!(
        "INSERT INTO intake.conversation \
         (homeowner_id, language, first_inbox_id, consent_template, consent_sent_at, opened_at) \
         VALUES ($1, $2, $3, $4, $5, $5) \
         ON CONFLICT (homeowner_id) WHERE closed_at IS NULL DO NOTHING RETURNING id",
        homeowner,
        language,
        inbox,
        template,
        now.to_sqlx() as _
    )
    .fetch_optional(db)
    .await
    .map_err(|_| IntakeError::Database)
}

/// One conversation row by ID.
pub(crate) async fn conversation(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Option<ConversationRow>, IntakeError> {
    let row = sqlx::query!(
        "SELECT id, homeowner_id, job_id, language, first_inbox_id, consent_template, \
         consent_sent_at, opened_at, closed_at FROM intake.conversation WHERE id = $1",
        id
    )
    .fetch_optional(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(row.map(|r| ConversationRow {
        id: r.id,
        homeowner_id: r.homeowner_id,
        job_id: r.job_id,
        language: r.language,
        first_inbox_id: r.first_inbox_id,
        consent_template: r.consent_template,
        consent_sent_at: r.consent_sent_at.to_jiff(),
        opened_at: r.opened_at.to_jiff(),
        closed_at: r.closed_at.map(|t| t.to_jiff()),
    }))
}

/// Stores the SMS consent of request `conversation`, given by inbound text `inbox` (CON-005).
pub(crate) async fn record_consent(
    db: &mut PgConnection,
    (homeowner, conversation, inbox): (Uuid, Uuid, Uuid),
    version: &str,
    at: Timestamp,
) -> Result<(), IntakeError> {
    sqlx::query!(
        "INSERT INTO intake.consent_record \
         (homeowner_id, conversation_id, channel, inbox_id, consent_version, consented_at) \
         VALUES ($1, $2, 'sms', $3, $4, $5)",
        homeowner,
        conversation,
        inbox,
        version,
        at.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| IntakeError::Database)
}

/// The consent records of request `conversation`, oldest first (CON-005, CON-007).
pub(crate) async fn consent_records(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Vec<ConsentRecordRow>, IntakeError> {
    let rows = sqlx::query!(
        "SELECT id, homeowner_id, conversation_id, channel, inbox_id, web_chat_session, \
         consent_version, consented_at FROM intake.consent_record WHERE conversation_id = $1 \
         ORDER BY consented_at, id",
        conversation
    )
    .fetch_all(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| ConsentRecordRow {
            id: r.id,
            homeowner_id: r.homeowner_id,
            conversation_id: r.conversation_id,
            channel: r.channel,
            inbox_id: r.inbox_id,
            web_chat_session: r.web_chat_session,
            consent_version: r.consent_version,
            consented_at: r.consented_at.to_jiff(),
        })
        .collect())
}

/// Whether vault contact `contact` is a homeowner with a consent record (CON-021).
pub(crate) async fn contact_consented(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<bool, IntakeError> {
    sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM intake.consent_record r \
         JOIN intake.homeowner h ON h.id = r.homeowner_id WHERE h.contact_id = $1) AS \"ok!\"",
        contact
    )
    .fetch_one(db)
    .await
    .map_err(|_| IntakeError::Database)
}

/// One terms acceptance's stored values, the request first (TERMS-033).
pub(crate) struct NewAcceptance<'a> {
    pub(crate) conversation: Uuid,
    pub(crate) pick: Uuid,
    pub(crate) document: &'a str,
    pub(crate) version: &'a str,
    pub(crate) text_sha256: &'a str,
    pub(crate) language: &'a str,
    pub(crate) channel: &'a str,
    pub(crate) assent_line: &'a str,
    pub(crate) inbox: Option<Uuid>,
    pub(crate) at: Timestamp,
}

/// Stores one acceptance; `false` when the pick's acceptance is already stored (TERMS-016).
pub(crate) async fn insert_acceptance(
    db: &mut PgConnection,
    a: &NewAcceptance<'_>,
) -> Result<bool, IntakeError> {
    sqlx::query_scalar!(
        "INSERT INTO intake.terms_acceptance (homeowner_id, conversation_id, pick_id, document, \
         version, text_sha256, language, channel, assent_line, inbox_id, accepted_at) \
         SELECT homeowner_id, id, $2, $3, $4, $5, $6, $7, $8, $9, $10 \
         FROM intake.conversation WHERE id = $1 ON CONFLICT (pick_id) DO NOTHING RETURNING id",
        a.conversation,
        a.pick,
        a.document,
        a.version,
        a.text_sha256,
        a.language,
        a.channel,
        a.assent_line,
        a.inbox,
        a.at.to_sqlx() as _
    )
    .fetch_optional(db)
    .await
    .map(|row| row.is_some())
    .map_err(|_| IntakeError::Database)
}

/// The terms acceptances of request `conversation`, oldest first (TERMS-016).
pub(crate) async fn acceptances(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Vec<TermsAcceptanceRow>, IntakeError> {
    let rows = sqlx::query!(
        "SELECT id, homeowner_id, conversation_id, pick_id, document, version, text_sha256, \
         language, channel, assent_line, inbox_id, accepted_at FROM intake.terms_acceptance \
         WHERE conversation_id = $1 ORDER BY accepted_at, id",
        conversation
    )
    .fetch_all(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| TermsAcceptanceRow {
            id: r.id,
            homeowner_id: r.homeowner_id,
            conversation_id: r.conversation_id,
            pick_id: r.pick_id,
            document: r.document,
            version: r.version,
            text_sha256: r.text_sha256,
            language: r.language,
            channel: r.channel,
            assent_line: r.assent_line,
            inbox_id: r.inbox_id,
            accepted_at: r.accepted_at.to_jiff(),
        })
        .collect())
}
