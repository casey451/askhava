//! Row types and queries of the `intake` schema (migration `0053_intake_tables.sql`, D-492, D-493).
//!
//! One row type per table; T181 added the consent and terms records (`0054_intake_terms_acceptance.sql`);
//! T039, T040 and T043 add their queries here. T040 keeps a request's jobs as `intake_answer` rows
//! (no new table, D-993). Every row holds IDs, codes
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
    /// A trade code is not a catalog code (lower case letters, digits and underscores).
    #[error("intake trade code is not valid")]
    BadTrade,
    /// A town code is not a catalog town code (lower case letters, digits and underscores).
    #[error("intake town code is not valid")]
    BadTown,
    /// The jobs module could not create the job.
    #[error("intake could not create a job")]
    Job,
    /// The vault could not store the name and address.
    #[error("intake could not store the name and address")]
    Vault,
    /// The quotes module could not read the quotes or build the LIST text (TXT-031).
    #[error("intake could not read the quotes")]
    Quotes,
    /// The `ops` alert could not be raised (INT-023's call-back).
    #[error("intake could not raise an alert")]
    Alert,
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

/// Records one emergency-list event of inbound text `inbox` (INT-004, INT-037): `matched` is the
/// 911-list code or `beep_question`, `queued` when the 911 text was queued. Once per inbound text.
pub(crate) async fn insert_event(
    db: &mut PgConnection,
    (conversation, inbox): (Uuid, Uuid),
    matched: &str,
    queued: Option<Timestamp>,
    at: Timestamp,
) -> Result<(), IntakeError> {
    sqlx::query!(
        "INSERT INTO intake.emergency_event \
         (conversation_id, inbox_id, matched, text_queued_at, created_at) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (inbox_id) DO NOTHING",
        conversation,
        inbox,
        matched,
        queued.map(|t| t.to_sqlx()) as _,
        at.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| IntakeError::Database)
}

/// The emergency events of request `conversation`, oldest first.
pub(crate) async fn events(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Vec<EmergencyEventRow>, IntakeError> {
    let rows = sqlx::query!(
        "SELECT id, conversation_id, inbox_id, matched, text_queued_at, safe_at, created_at \
         FROM intake.emergency_event WHERE conversation_id = $1 ORDER BY created_at, id",
        conversation
    )
    .fetch_all(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| EmergencyEventRow {
            id: r.id,
            conversation_id: r.conversation_id,
            inbox_id: r.inbox_id,
            matched: r.matched,
            text_queued_at: r.text_queued_at.map(|t| t.to_jiff()),
            safe_at: r.safe_at.map(|t| t.to_jiff()),
            created_at: r.created_at.to_jiff(),
        })
        .collect())
}

/// Marks every 911 text of `conversation` still waiting for SAFE as answered at `at` (INT-005);
/// `true` when one was waiting.
pub(crate) async fn mark_safe(
    db: &mut PgConnection,
    conversation: Uuid,
    at: Timestamp,
) -> Result<bool, IntakeError> {
    sqlx::query!(
        "UPDATE intake.emergency_event SET safe_at = $2 WHERE conversation_id = $1 \
         AND text_queued_at IS NOT NULL AND safe_at IS NULL",
        conversation,
        at.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|r| r.rows_affected() > 0)
    .map_err(|_| IntakeError::Database)
}

/// Stores one answer to question `question` of `conversation` (INT-030, INT-006, INT-037).
pub(crate) async fn insert_answer(
    db: &mut PgConnection,
    (conversation, inbox): (Uuid, Uuid),
    (question, choice): (&str, &str),
    at: Timestamp,
) -> Result<(), IntakeError> {
    sqlx::query!(
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, inbox_id, \
         answered_at) VALUES ($1, $2, $3, $4, $5)",
        conversation,
        question,
        choice,
        inbox,
        at.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| IntakeError::Database)
}

/// The latest stored answer to `question` in `conversation`, if any.
pub(crate) async fn latest_answer(
    db: &mut PgConnection,
    conversation: Uuid,
    question: &str,
) -> Result<Option<String>, IntakeError> {
    sqlx::query_scalar!(
        "SELECT choice FROM intake.intake_answer WHERE conversation_id = $1 AND question_id = $2 \
         ORDER BY answered_at DESC, id DESC LIMIT 1",
        conversation,
        question
    )
    .fetch_optional(db)
    .await
    .map(Option::flatten)
    .map_err(|_| IntakeError::Database)
}

/// What the release of an emergency job waits on (INT-029): consent recorded, no 911 text still
/// waiting for SAFE, the emergency choice, and the ZIP and area check (a property in a town).
pub(crate) struct ReleaseState {
    pub(crate) consented: bool,
    pub(crate) waiting_safe: bool,
    pub(crate) emergency: bool,
    pub(crate) area_done: bool,
}

/// The release state of `conversation` (INT-005, INT-029).
pub(crate) async fn release_state(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<ReleaseState, IntakeError> {
    let r = sqlx::query!(
        "SELECT EXISTS (SELECT 1 FROM intake.consent_record WHERE conversation_id = c.id) \
         AS \"consented!\", \
         EXISTS (SELECT 1 FROM intake.emergency_event WHERE conversation_id = c.id \
         AND text_queued_at IS NOT NULL AND safe_at IS NULL) AS \"waiting!\", \
         COALESCE((SELECT choice = 'emergency' FROM intake.intake_answer \
         WHERE conversation_id = c.id AND question_id = 'emergency_choice' \
         ORDER BY answered_at DESC, id DESC LIMIT 1), false) AS \"emergency!\", \
         EXISTS (SELECT 1 FROM intake.property WHERE homeowner_id = c.homeowner_id) AS \"area!\" \
         FROM intake.conversation c WHERE c.id = $1",
        conversation
    )
    .fetch_optional(db)
    .await
    .map_err(|_| IntakeError::Database)?
    .ok_or(IntakeError::UnknownRequest)?;
    Ok(ReleaseState {
        consented: r.consented,
        waiting_safe: r.waiting,
        emergency: r.emergency,
        area_done: r.area,
    })
}

/// Locks the homeowner row until the transaction ends, so two messages at once open one job
/// (INT-009).
pub(crate) async fn lock_homeowner(
    db: &mut PgConnection,
    homeowner: Uuid,
) -> Result<(), IntakeError> {
    sqlx::query_scalar!(
        "SELECT id FROM intake.homeowner WHERE id = $1 FOR UPDATE",
        homeowner
    )
    .fetch_one(db)
    .await
    .map(|_| ())
    .map_err(|_| IntakeError::Database)
}

/// The homeowner and open state of request `conversation`: `None` when it does not exist.
pub(crate) async fn conversation_state(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Option<(Uuid, bool)>, IntakeError> {
    sqlx::query!(
        "SELECT homeowner_id, closed_at IS NULL AS \"open!\" FROM intake.conversation WHERE id = $1",
        conversation
    )
    .fetch_optional(db)
    .await
    .map(|r| r.map(|r| (r.homeowner_id, r.open)))
    .map_err(|_| IntakeError::Database)
}

/// The active job stored under any of `kinds` (`job.<trade>.<property>`) in an open request of
/// the homeowner, if any (INT-009): a closed request never blocks a new job.
pub(crate) async fn active_job(
    db: &mut PgConnection,
    homeowner: Uuid,
    kinds: &[String],
) -> Result<Option<Uuid>, IntakeError> {
    let hex = sqlx::query_scalar!(
        "SELECT a.choice FROM intake.intake_answer a \
         JOIN intake.conversation c ON c.id = a.conversation_id \
         WHERE c.homeowner_id = $1 AND c.closed_at IS NULL AND a.question_id = ANY($2) LIMIT 1",
        homeowner,
        kinds
    )
    .fetch_optional(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(hex.flatten().and_then(|h| Uuid::parse_str(&h).ok()))
}

/// The homeowner's newest job and the time of the homeowner's last message to it (INT-010): the
/// later of the job's opening and the latest `heard` row of its request.
pub(crate) async fn newest_job(
    db: &mut PgConnection,
    homeowner: Uuid,
) -> Result<Option<(Uuid, Timestamp)>, IntakeError> {
    let row = sqlx::query!(
        "SELECT a.choice, GREATEST(a.answered_at, COALESCE((SELECT max(h.answered_at) \
         FROM intake.intake_answer h WHERE h.conversation_id = a.conversation_id \
         AND h.question_id = 'heard'), a.answered_at)) AS \"last!\" \
         FROM intake.intake_answer a JOIN intake.conversation c ON c.id = a.conversation_id \
         WHERE c.homeowner_id = $1 AND c.closed_at IS NULL AND a.question_id LIKE 'job.%' \
         ORDER BY a.answered_at DESC, a.id DESC LIMIT 1",
        homeowner
    )
    .fetch_optional(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(row.and_then(|r| Some((Uuid::parse_str(&r.choice?).ok()?, r.last.to_jiff()))))
}

/// Stores an answer row with no inbound text (a job link): question, choice and time.
pub(crate) async fn insert_plain(
    db: &mut PgConnection,
    conversation: Uuid,
    (question, choice): (&str, &str),
    at: Timestamp,
) -> Result<(), IntakeError> {
    sqlx::query!(
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) \
         VALUES ($1, $2, $3, $4)",
        conversation,
        question,
        choice,
        at.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| IntakeError::Database)
}

/// The IDs of every job the request opened: its job links and its first job (INT-038).
pub(crate) async fn request_jobs(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Vec<Uuid>, IntakeError> {
    let links = sqlx::query_scalar!(
        "SELECT choice FROM intake.intake_answer WHERE conversation_id = $1 AND question_id LIKE 'job.%' \
         UNION SELECT replace(job_id::text, '-', '') FROM intake.conversation WHERE id = $1",
        conversation
    )
    .fetch_all(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(links
        .into_iter()
        .flatten()
        .filter_map(|h| Uuid::parse_str(&h).ok())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect())
}

/// Closes request `conversation` at `at` and drops its job links (INT-038); `false` if it was
/// already closed.
pub(crate) async fn close_request(
    db: &mut PgConnection,
    conversation: Uuid,
    at: Timestamp,
) -> Result<bool, IntakeError> {
    let done = sqlx::query!(
        "UPDATE intake.conversation SET closed_at = GREATEST($2, opened_at) \
         WHERE id = $1 AND closed_at IS NULL",
        conversation,
        at.to_sqlx() as _
    )
    .execute(&mut *db)
    .await
    .map_err(|_| IntakeError::Database)?;
    sqlx::query!(
        "DELETE FROM intake.intake_answer WHERE conversation_id = $1 AND question_id LIKE 'job.%'",
        conversation
    )
    .execute(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(done.rows_affected() > 0)
}

/// Deletes the request's data except its consent record (INT-038, under 13): answers, 911-list
/// events, recruiting demand and the homeowner's property rows. The conversation row stays as
/// the consent record's anchor and holds only IDs and times.
pub(crate) async fn purge_request(
    db: &mut PgConnection,
    conversation: Uuid,
    homeowner: Uuid,
) -> Result<(), IntakeError> {
    let err = |_| IntakeError::Database;
    sqlx::query!(
        "DELETE FROM intake.intake_answer WHERE conversation_id = $1",
        conversation
    )
    .execute(&mut *db)
    .await
    .map_err(err)?;
    sqlx::query!(
        "DELETE FROM intake.emergency_event WHERE conversation_id = $1",
        conversation
    )
    .execute(&mut *db)
    .await
    .map_err(err)?;
    sqlx::query!(
        "DELETE FROM intake.recruiting_demand WHERE conversation_id = $1",
        conversation
    )
    .execute(&mut *db)
    .await
    .map_err(err)?;
    sqlx::query!(
        "DELETE FROM intake.property WHERE homeowner_id = $1",
        homeowner
    )
    .execute(db)
    .await
    .map_err(err)?;
    Ok(())
}

/// The homeowner's newest property row, if the ZIP and area check made one (INT-009).
pub(crate) async fn latest_property(
    db: &mut PgConnection,
    homeowner: Uuid,
) -> Result<Option<Uuid>, IntakeError> {
    sqlx::query_scalar!(
        "SELECT id FROM intake.property WHERE homeowner_id = $1 ORDER BY created_at DESC, id DESC LIMIT 1",
        homeowner
    )
    .fetch_optional(db)
    .await
    .map_err(|_| IntakeError::Database)
}

/// Sets request `conversation`'s first job if it has none (`conversation.job_id`).
pub(crate) async fn set_first_job(
    db: &mut PgConnection,
    conversation: Uuid,
    job: Uuid,
) -> Result<(), IntakeError> {
    sqlx::query!(
        "UPDATE intake.conversation SET job_id = COALESCE(job_id, $2) WHERE id = $1",
        conversation,
        job
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| IntakeError::Database)
}

/// Records one request no business covers yet (INT-017, OPS-041).
pub(crate) async fn insert_demand(
    db: &mut PgConnection,
    conversation: Uuid,
    (town, trade, job_type): (&str, &str, Option<&str>),
    at: Timestamp,
) -> Result<(), IntakeError> {
    sqlx::query!(
        "INSERT INTO intake.recruiting_demand (conversation_id, town, trade, job_type, created_at) \
         VALUES ($1, $2, $3, $4, $5)",
        conversation,
        town,
        trade,
        job_type,
        at.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| IntakeError::Database)
}

/// The recruiting-demand rows of request `conversation`, oldest first (INT-017).
pub(crate) async fn demand_rows(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Vec<RecruitingDemandRow>, IntakeError> {
    let rows = sqlx::query!(
        "SELECT id, conversation_id, town, trade, job_type, created_at \
         FROM intake.recruiting_demand WHERE conversation_id = $1 ORDER BY created_at, id",
        conversation
    )
    .fetch_all(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| RecruitingDemandRow {
            id: r.id,
            conversation_id: r.conversation_id,
            town: r.town,
            trade: r.trade,
            job_type: r.job_type,
            created_at: r.created_at.to_jiff(),
        })
        .collect())
}

/// The homeowner's vault contact ID and the language code of request `conversation`, the
/// recipient of its no-reply texts (INT-019); `None` when it does not exist.
pub(crate) async fn recipient(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Option<(Uuid, String)>, IntakeError> {
    sqlx::query!(
        "SELECT h.contact_id, c.language FROM intake.conversation c \
         JOIN intake.homeowner h ON h.id = c.homeowner_id WHERE c.id = $1",
        conversation
    )
    .fetch_optional(db)
    .await
    .map(|r| r.map(|r| (r.contact_id, r.language)))
    .map_err(|_| IntakeError::Database)
}
