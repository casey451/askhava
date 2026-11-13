//! How the homeowner chose to be reached at the page pick (PICK-018, PICK-042; T266, D-1264,
//! D-1761).
//!
//! At the page pick she chooses a call through the masked number or an email she shares.
//! [`record_contact_choice`] stores the choice in `picks.contact_choice`
//! (`migrations/0166_picks_contact_choice.sql`, one row per pick, append-only) in the transaction
//! that stores the pick, right after `page_pick`; the email itself goes only into her vault
//! contact (`privacy::api::store_shared_email`), never here. [`contact_choice`] answers the
//! release's caller: a pick with no row (a text pick, or a page pick from before this table) is a
//! masked call.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::PicksError;

/// How the business reaches the homeowner (PICK-018).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactChoice {
    /// A call through the masked number.
    Call,
    /// An email to the address she shared, sealed in her vault contact.
    Email,
}

impl ContactChoice {
    /// The stored word: `call` or `email`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::Email => "email",
        }
    }

    /// Whether the release opens her email (`email_chosen`, PICK-024, D-1761).
    #[must_use]
    pub const fn email_chosen(self) -> bool {
        matches!(self, Self::Email)
    }
}

/// PICK-018, D-1761: stores `choice` for page pick `pick` at `at`, inside the transaction that
/// stores (or keeps) the pick. `true` when this call stored it; `false` when the pick already had
/// a choice, which stays (a re-pick of the waiting quote keeps her first answer).
///
/// # Errors
///
/// [`PicksError::Database`]; the caller rolls the pick back.
pub async fn record_contact_choice(
    tx: &mut PgConnection,
    pick: Uuid,
    choice: ContactChoice,
    at: Timestamp,
) -> Result<bool, PicksError> {
    let stored = sqlx::query!(
        "INSERT INTO picks.contact_choice (pick_id, choice, decided_at) VALUES ($1, $2, $3) \
         ON CONFLICT (pick_id) DO NOTHING",
        pick,
        choice.code(),
        at.to_sqlx() as _,
    )
    .execute(tx)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(stored.rows_affected() == 1)
}

/// PICK-018, PICK-024, D-1761: pick `pick`'s contact choice for the release; `Call` when none was
/// stored (a text pick).
///
/// # Errors
///
/// [`PicksError::Database`].
pub async fn contact_choice(
    db: &mut PgConnection,
    pick: Uuid,
) -> Result<ContactChoice, PicksError> {
    let row = sqlx::query_scalar!(
        "SELECT choice FROM picks.contact_choice WHERE pick_id = $1",
        pick
    )
    .fetch_optional(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(match row.as_deref() {
        Some("email") => ContactChoice::Email,
        _ => ContactChoice::Call,
    })
}
