//! The "Auto-confirm and pay if picked" tick stored with a quote (T209; PICK-048, PICK-056,
//! FEE-014, FEE-042, SEC-038, D-401, D-480, D-491).
//!
//! [`tick_auto_confirm`] stores one tick of a quote's box in `quotes.auto_confirm`: the member who
//! ticked it, the fee shown, the authorization version ([`AUTO_CONFIRM_VERSION`], the wording of
//! [`auto_confirm_authorization`]) and the saved card it was ticked with; [`untick_auto_confirm`]
//! ends it (the business's own untick, or `picks`' void when the member, the card, the fee or the
//! version no longer match); [`auto_confirm_of`] is the live tick `picks` reads at the pick, only
//! one placed at or before the pick's own time;
//! [`stripe_answer_filed`] reads the owner's filed Stripe answer (D-2783). This
//! module never decides whether the box shows (the answer filed, a saved card and the member's
//! role are the portals route's), never checks the fresh Verify code (the route's
//! `require_step_up` first) and never charges anything.

use askhava_core::api::brand;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

/// The version of the PICK-048 authorization wording; a new wording takes a higher number, and a
/// tick stored under another version never fires (PICK-056).
pub const AUTO_CONFIRM_VERSION: i32 = 1;

/// PICK-048: the box's own authorization, with the fee shown in whole dollars (cents only when
/// the fee has them) and the legal name from the brand file.
#[must_use]
pub fn auto_confirm_authorization(fee_cents: u64) -> String {
    let (dollars, cents) = (fee_cents / 100, fee_cents % 100);
    let fee = if cents == 0 {
        format!("${dollars}")
    } else {
        format!("${dollars}.{cents:02}")
    };
    format!(
        "I authorize {} to charge my saved card {fee} for this quote's introduction if the \
homeowner picks it, without another tap. I can untick this until the homeowner picks.",
        brand().legal_name()
    )
}

/// One live tick of a quote's box (PICK-048, PICK-056). `Debug` shows the card's ID only, never
/// card data (FEE-014).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoConfirmTick {
    /// The team member who ticked the box after a fresh Verify code (SEC-038).
    pub member: Uuid,
    /// The introduction fee by job size shown beside the box, in cents.
    pub fee_cents: u64,
    /// The authorization version shown ([`AUTO_CONFIRM_VERSION`] at the tick).
    pub authorization_version: i32,
    /// The saved card, `pm_…`, the tick was given with.
    pub payment_method: String,
}

/// Why an untick happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Untick {
    /// The business unticked the box before the pick.
    Business,
    /// `picks` voided the tick: the member, the card, the fee or the version changed (PICK-056).
    Void,
}

impl Untick {
    const fn code(self) -> &'static str {
        match self {
            Self::Business => "business",
            Self::Void => "void",
        }
    }
}

/// Why a tick could not be stored or read. No variant carries a database message (R5).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AutoConfirmError {
    /// The business has no live quote on that job (none, withdrawn or no longer available).
    #[error("no live quote to tick")]
    NoQuote,
    /// A $0 fee, a version below 1 or a card ID that is not `pm_…`.
    #[error("an auto-confirm field has the wrong form")]
    BadField,
    /// A statement failed.
    #[error("auto-confirm database statement failed")]
    Database,
}

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, AutoConfirmError> {
    result.map_err(|_| AutoConfirmError::Database)
}

/// PICK-048, PICK-056, FEE-042: stores `tick` on the business's live quote for `job`, replacing
/// any earlier live tick (unticked as the business's), in one transaction. The caller has already
/// checked the role, the fresh Verify code, Stripe's written answer and the saved card.
///
/// # Errors
/// [`AutoConfirmError::BadField`], [`AutoConfirmError::NoQuote`] or
/// [`AutoConfirmError::Database`].
pub async fn tick_auto_confirm(
    db_conn: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    tick: &AutoConfirmTick,
    at: Timestamp,
) -> Result<(), AutoConfirmError> {
    let fee = i64::try_from(tick.fee_cents).map_err(|_| AutoConfirmError::BadField)?;
    if fee == 0 || tick.authorization_version < 1 || !tick.payment_method.starts_with("pm_") {
        return Err(AutoConfirmError::BadField);
    }
    let mut tx = db(sqlx::Connection::begin(&mut *db_conn).await)?;
    let quote = db(sqlx::query_scalar!(
        "SELECT id FROM quotes.quote WHERE job_id = $1 AND business_id = $2
           AND withdrawn_at IS NULL AND unavailable_at IS NULL FOR UPDATE",
        job,
        business
    )
    .fetch_optional(&mut *tx)
    .await)?
    .ok_or(AutoConfirmError::NoQuote)?;
    end_live(&mut tx, quote, Untick::Business, at).await?;
    db(sqlx::query!(
        "INSERT INTO quotes.auto_confirm (quote_id, member_id, fee_cents, authorization_version,
           payment_method, ticked_at) VALUES ($1, $2, $3, $4, $5, $6)",
        quote,
        tick.member,
        fee,
        tick.authorization_version,
        tick.payment_method,
        at.to_sqlx(),
    )
    .execute(&mut *tx)
    .await)?;
    db(tx.commit().await)
}

async fn end_live(
    tx: &mut PgConnection,
    quote: Uuid,
    why: Untick,
    at: Timestamp,
) -> Result<bool, AutoConfirmError> {
    let ended = db(sqlx::query!(
        "UPDATE quotes.auto_confirm SET unticked_at = greatest($2, ticked_at), untick_reason = $3
         WHERE quote_id = $1 AND unticked_at IS NULL",
        quote,
        at.to_sqlx(),
        why.code(),
    )
    .execute(&mut *tx)
    .await)?;
    Ok(ended.rows_affected() > 0)
}

/// PICK-048, PICK-056: ends the live tick on the business's quote for `job`, if any; `true` when
/// one ended. An untick before `picks` reads the tick stops the charge; one after it changes
/// nothing for that pick, whose charge was already decided.
///
/// # Errors
/// [`AutoConfirmError::Database`].
pub async fn untick_auto_confirm(
    db_conn: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    why: Untick,
    at: Timestamp,
) -> Result<bool, AutoConfirmError> {
    let quote = db(sqlx::query_scalar!(
        "SELECT id FROM quotes.quote WHERE job_id = $1 AND business_id = $2",
        job,
        business
    )
    .fetch_optional(&mut *db_conn)
    .await)?;
    match quote {
        Some(quote) => end_live(db_conn, quote, why, at).await,
        None => Ok(false),
    }
}

/// PICK-048, PICK-056: the live tick on the business's quote for `job`, as stored; `None` when the
/// box is unticked. With `picked_at` (the pick's own time, `picks` at the pick) only a tick placed
/// at or before the pick counts: the box authorizes a charge "if the homeowner picks it", so a
/// tick placed after the pick never fires for it. `None` (the quote form) reads any live tick.
/// `picks` compares the tick with the pick's fee, card, version and member.
///
/// # Errors
/// [`AutoConfirmError::Database`].
pub async fn auto_confirm_of(
    db_conn: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    picked_at: Option<Timestamp>,
) -> Result<Option<AutoConfirmTick>, AutoConfirmError> {
    let row = db(sqlx::query!(
        "SELECT a.member_id, a.fee_cents, a.authorization_version, a.payment_method
         FROM quotes.auto_confirm a JOIN quotes.quote q ON q.id = a.quote_id
         WHERE q.job_id = $1 AND q.business_id = $2 AND a.unticked_at IS NULL
           AND ($3::timestamptz IS NULL OR a.ticked_at <= $3)",
        job,
        business,
        picked_at.map(ToSqlx::to_sqlx) as _,
    )
    .fetch_optional(db_conn)
    .await)?;
    Ok(row.map(|r| AutoConfirmTick {
        member: r.member_id,
        fee_cents: u64::try_from(r.fee_cents).unwrap_or(0),
        authorization_version: r.authorization_version,
        payment_method: r.payment_method,
    }))
}

/// PICK-048, FEE-014, D-401, D-2783: whether the owner-portal record "Stripe written answer on
/// charges without the business present" is filed and allows the charge: the latest filed record
/// (date, reference and answer) in `quotes.stripe_written_answer` says yes. No record, or a latest
/// one saying no, is `false`: no box shows and no off-session charge is made. The owner portal's
/// settings page (T108) files the record.
///
/// # Errors
/// [`AutoConfirmError::Database`].
pub async fn stripe_answer_filed(db_conn: &mut PgConnection) -> Result<bool, AutoConfirmError> {
    let allows = db(sqlx::query_scalar!(
        "SELECT allows FROM quotes.stripe_written_answer ORDER BY filed_at DESC, id LIMIT 1"
    )
    .fetch_optional(db_conn)
    .await)?;
    Ok(allows == Some(true))
}
