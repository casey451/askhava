//! The texting provider's written answer on business texting, as Casey files it in the owner
//! portal (TXT-035, TERMS-014; D-303, D-305, D-3881, T527).
//!
//! `messaging.business_eligibility` (migration `0209_messaging_business_eligibility.sql`) is
//! append-only: [`file_business_eligibility`] adds one row and one `ops` audit row (IDs only,
//! never the answer or a reference) and the newest row is in force ([`business_eligibility`]).
//! `mode_state::switch_on` turns the business line on only while that row [`permits`] it: the
//! answer says allowed and a separate business campaign's registration reference is filed. A
//! filing that does not permit it also switches the business line off in the same transaction,
//! under `messaging.mode_state`'s lock, so the switch stays off unless the answer is yes. The
//! homeowner line never reads this table.
//!
//! [`permits`]: BusinessEligibility::permits

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::mode_state::{ModeStateError, lock, switch_off};
use crate::templates::Audience;

/// The `ops.audit_log` action of one filed provider answer, subject the new row.
pub const BUSINESS_ELIGIBILITY_ACTION: &str = "messaging.business_eligibility_filed";

/// One provider answer on business texting as filed (TXT-035).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusinessEligibility {
    /// The date of the provider's written answer.
    pub answered_on: Date,
    /// The provider's reference (case or ticket), 1 to 200 characters.
    pub reference: String,
    /// The answer text as received, 1 to 4,000 characters.
    pub answer: String,
    /// Whether the answer says business texting is allowed.
    pub allowed: bool,
    /// The separate business campaign's registration reference; empty while none is registered.
    pub campaign_reference: String,
}

impl BusinessEligibility {
    /// TXT-035, TERMS-014: whether this answer lets the business switch go on: it says allowed
    /// and names a business campaign.
    #[must_use]
    pub fn permits(&self) -> bool {
        self.allowed && !self.campaign_reference.trim().is_empty()
    }

    /// Whether every field is within its limits.
    fn complete(&self) -> bool {
        let within = |t: &str, max: usize| (1..=max).contains(&t.trim().chars().count());
        let campaign = self.campaign_reference.trim();
        within(&self.reference, 200)
            && within(&self.answer, 4000)
            && (campaign.is_empty() || within(campaign, 200))
    }
}

/// TXT-035, TERMS-014: files `answer` for `owner` at `now` (one row and one audit row); an answer
/// that does not [`BusinessEligibility::permits`] business texting also switches the business line
/// off. Returns the new row's ID.
///
/// # Errors
///
/// [`ModeStateError::BadFiling`] (nothing stored) or [`ModeStateError::Database`].
pub async fn file_business_eligibility(
    conn: &mut PgConnection,
    owner: Uuid,
    answer: &BusinessEligibility,
    now: Timestamp,
) -> Result<Uuid, ModeStateError> {
    if !answer.complete() {
        return Err(ModeStateError::BadFiling);
    }
    let mut tx = conn.begin().await.map_err(database)?;
    lock(&mut tx).await?;
    let campaign = Some(answer.campaign_reference.trim()).filter(|c| !c.is_empty());
    let id = sqlx::query_scalar!(
        "INSERT INTO messaging.business_eligibility (answered_on, reference, answer, allowed, \
         campaign_reference, filed_by, filed_at) VALUES ($1::text::date, $2, $3, $4, $5, $6, $7) \
         RETURNING id",
        answer.answered_on.to_string(),
        answer.reference.trim(),
        answer.answer.trim(),
        answer.allowed,
        campaign,
        owner,
        now.to_sqlx() as _,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(database)?;
    let entry = AuditEntry {
        actor: Actor::Owner(owner),
        action: BUSINESS_ELIGIBILITY_ACTION,
        subject_kind: "messaging.business_eligibility",
        subject_id: id,
        change: Change::Action,
    };
    audit(&mut tx, &entry).await.map_err(database)?;
    if !answer.permits() {
        switch_off(&mut tx, Audience::Business, (owner, now)).await?;
    }
    tx.commit().await.map_err(database)?;
    Ok(id)
}

fn database<E>(_: E) -> ModeStateError {
    ModeStateError::Database
}

/// TXT-035: the newest filed provider answer with when it was filed, or `None` with none.
///
/// # Errors
///
/// [`ModeStateError::Database`].
pub async fn business_eligibility(
    conn: &mut PgConnection,
) -> Result<Option<(BusinessEligibility, Timestamp)>, ModeStateError> {
    let row = sqlx::query!(
        r#"SELECT answered_on::text AS "answered_on!", reference, answer, allowed,
                  campaign_reference, filed_at
           FROM messaging.business_eligibility ORDER BY seq DESC LIMIT 1"#
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| ModeStateError::Database)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let answered_on = row
        .answered_on
        .parse::<Date>()
        .map_err(|_| ModeStateError::Database)?;
    let filed = BusinessEligibility {
        answered_on,
        reference: row.reference,
        answer: row.answer,
        allowed: row.allowed,
        campaign_reference: row.campaign_reference.unwrap_or_default(),
    };
    Ok(Some((filed, row.filed_at.to_jiff())))
}

/// TXT-035, TERMS-014: whether the newest filing permits business texting; none filed is no.
pub(crate) async fn business_texting_allowed(
    conn: &mut PgConnection,
) -> Result<bool, ModeStateError> {
    let filed = business_eligibility(conn).await?;
    Ok(filed.is_some_and(|(f, _)| f.permits()))
}
