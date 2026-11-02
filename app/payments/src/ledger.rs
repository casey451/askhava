//! The append-only money ledger, `payments.ledger_entry` (FEE-028, FEE-032, D-092, D-491).
//!
//! [`ledger_append`] is the only code that writes a ledger row: the free-win, refund, dispute,
//! charge and reconcile tasks each build a checked [`LedgerEntry`] and append it inside their own
//! transaction. The app role can insert and select, never update or delete, so a mistake is fixed
//! by a new entry, never an edit. Amounts are whole cents, never negative; the [`EntryKind`] says
//! which way the money went. The caller's idempotency key makes a repeated task append once.

use sqlx::PgConnection;
use sqlx::types::Uuid;

/// The FEE-028 entry types, with the code stored in `payments.ledger_entry.kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// An introduction fee by job size charged on a confirmed pick.
    Charge,
    /// A full refund of a charge (FEE-026).
    Refund,
    /// A credit given to a business.
    Credit,
    /// A free first win: $0, the business's first introduction.
    FreeFirstWin,
    /// Repeat work from the same customer: $0 (FEE-013).
    RepeatCustomer,
    /// A pick of a fee-free job type: $0 (FEE-045).
    FeeFreeJobType,
    /// A dispute's withdrawn amount.
    Dispute,
    /// The dispute countering fee, recorded when evidence is submitted (FEE-050).
    CounteringFee,
    /// The countering fee's return on a won dispute (FEE-050).
    CounteringFeeReturn,
    /// Stripe's processing fee on a balance transaction (FEE-051).
    ProcessingFee,
    /// A Stripe payout to Ask Hava's bank; it names no business and no job.
    Payout,
}

impl EntryKind {
    /// Every entry type, in FEE-028's order.
    pub const ALL: [Self; 11] = [
        Self::Charge,
        Self::Refund,
        Self::Credit,
        Self::FreeFirstWin,
        Self::RepeatCustomer,
        Self::FeeFreeJobType,
        Self::Dispute,
        Self::CounteringFee,
        Self::CounteringFeeReturn,
        Self::ProcessingFee,
        Self::Payout,
    ];

    /// The stored code, matching the migration's `kind` check (FEE-028).
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Charge => "charge",
            Self::Refund => "refund",
            Self::Credit => "credit",
            Self::FreeFirstWin => "free-first-win",
            Self::RepeatCustomer => "repeat-customer",
            Self::FeeFreeJobType => "fee-free-job-type",
            Self::Dispute => "dispute",
            Self::CounteringFee => "countering-fee",
            Self::CounteringFeeReturn => "countering-fee-return",
            Self::ProcessingFee => "processing-fee",
            Self::Payout => "payout",
        }
    }

    /// True for the three kinds that always record $0 (FEE-013, FEE-045, FEE-028).
    #[must_use]
    pub const fn is_zero(self) -> bool {
        matches!(
            self,
            Self::FreeFirstWin | Self::RepeatCustomer | Self::FeeFreeJobType
        )
    }

    const fn needs_job(self) -> bool {
        !matches!(self, Self::Payout | Self::Credit)
    }
}

/// Why a ledger entry was refused or could not be written.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LedgerError {
    /// The amount is negative, or not 0 for a $0 kind, or 0 for any other kind.
    #[error("the amount does not fit the entry kind")]
    Amount,
    /// The business or job is missing where the kind needs it, or present on a payout.
    #[error("the business or job does not fit the entry kind")]
    Parties,
    /// The Stripe ID is malformed, or present on a $0 entry.
    #[error("the Stripe ID is malformed or not allowed for this kind")]
    StripeId,
    /// The idempotency key is empty, too long or has a character outside `A-Za-z0-9._:-`.
    #[error("the idempotency key is malformed")]
    Key,
    /// The key was already used by an entry with different content.
    #[error("the idempotency key was already used for a different entry")]
    KeyReused,
    /// The database refused or dropped the statement.
    #[error("the ledger database statement failed")]
    Database,
}

/// The raw fields of a ledger entry, before [`LedgerEntry::new`] checks them.
#[derive(Clone, Debug)]
pub struct NewEntry<'a> {
    /// The entry type.
    pub kind: EntryKind,
    /// The business the money concerns; `None` only for a payout.
    pub business: Option<Uuid>,
    /// The job; required for every kind but payout and credit, never on a payout.
    pub job: Option<Uuid>,
    /// Whole cents, never negative.
    pub amount_cents: i64,
    /// The Stripe object ID (`ch_…`, `re_…`, `dp_…`, `txn_…`, `po_…`); never on a $0 entry.
    pub stripe_id: Option<&'a str>,
    /// The caller's derived key, no personal data (SEC-018, JOB-006).
    pub idempotency_key: &'a str,
}

/// A checked ledger entry: only [`LedgerEntry::new`] makes one, so [`ledger_append`] never
/// sends the database a row its checks would refuse (R2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    kind: EntryKind,
    business: Option<Uuid>,
    job: Option<Uuid>,
    amount_cents: i64,
    stripe_id: Option<String>,
    key: String,
}

impl LedgerEntry {
    /// Checks the fields against the FEE-028 rules the migration also enforces.
    ///
    /// # Errors
    /// [`LedgerError::Amount`], [`LedgerError::Parties`], [`LedgerError::StripeId`] or
    /// [`LedgerError::Key`] when that field does not fit the kind.
    pub fn new(raw: NewEntry<'_>) -> Result<Self, LedgerError> {
        let kind = raw.kind;
        let amount_ok = if kind.is_zero() {
            raw.amount_cents == 0
        } else {
            raw.amount_cents > 0
        };
        if !amount_ok {
            return Err(LedgerError::Amount);
        }
        let is_payout = kind == EntryKind::Payout;
        let parties_ok = raw.business.is_none() == is_payout
            && (raw.job.is_some() || !kind.needs_job())
            && !(is_payout && raw.job.is_some());
        if !parties_ok {
            return Err(LedgerError::Parties);
        }
        if raw
            .stripe_id
            .is_some_and(|id| kind.is_zero() || !stripe_id_ok(id))
        {
            return Err(LedgerError::StripeId);
        }
        if !key_ok(raw.idempotency_key) {
            return Err(LedgerError::Key);
        }
        Ok(Self {
            kind,
            business: raw.business,
            job: raw.job,
            amount_cents: raw.amount_cents,
            stripe_id: raw.stripe_id.map(str::to_owned),
            key: raw.idempotency_key.to_owned(),
        })
    }

    /// The entry type.
    #[must_use]
    pub const fn kind(&self) -> EntryKind {
        self.kind
    }

    /// The amount in whole cents.
    #[must_use]
    pub const fn amount_cents(&self) -> i64 {
        self.amount_cents
    }
}

fn stripe_id_ok(id: &str) -> bool {
    let Some((prefix, rest)) = id.split_once('_') else {
        return false;
    };
    (2..=8).contains(&prefix.len())
        && prefix.bytes().all(|b| b.is_ascii_lowercase())
        && (1..=200).contains(&rest.len())
        && rest.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn key_ok(key: &str) -> bool {
    (1..=200).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".:_-".contains(&b))
}

/// What [`ledger_append`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appended {
    /// A new row was written; its ID.
    New(Uuid),
    /// The same entry was already written under this key (a repeated task, JOB-006); its ID.
    Already(Uuid),
}

/// Appends one entry to `payments.ledger_entry`, the only writer of the ledger (FEE-028, FEE-032).
///
/// Runs on the caller's connection so the entry commits with the caller's state change (R11).
/// A key already used by the identical entry returns [`Appended::Already`] and writes nothing.
///
/// # Errors
/// [`LedgerError::KeyReused`] when the key holds a different entry; [`LedgerError::Database`]
/// when a statement fails.
pub async fn ledger_append(
    conn: &mut PgConnection,
    entry: &LedgerEntry,
) -> Result<Appended, LedgerError> {
    let inserted = sqlx::query_scalar!(
        "INSERT INTO payments.ledger_entry
           (kind, business_id, job_id, amount_cents, stripe_id, idempotency_key)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (idempotency_key) DO NOTHING
         RETURNING id",
        entry.kind.code(),
        entry.business,
        entry.job,
        entry.amount_cents,
        entry.stripe_id.as_deref(),
        entry.key,
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| LedgerError::Database)?;
    if let Some(id) = inserted {
        return Ok(Appended::New(id));
    }
    let row = sqlx::query!(
        "SELECT id, kind, business_id, job_id, amount_cents, stripe_id
           FROM payments.ledger_entry WHERE idempotency_key = $1",
        entry.key,
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|_| LedgerError::Database)?;
    let same = row.kind == entry.kind.code()
        && row.business_id == entry.business
        && row.job_id == entry.job
        && row.amount_cents == entry.amount_cents
        && row.stripe_id == entry.stripe_id;
    if same {
        Ok(Appended::Already(row.id))
    } else {
        Err(LedgerError::KeyReused)
    }
}
