//! The replay's view of the outbox (OPS-016, OPS-017; D-156, D-2764, D-3753, D-3877, D-3889).
//!
//! T110's replay in `recovery` may read neither `messaging.outbox` nor the vault, and no recipient
//! number ever leaves `messaging`. [`replay_window`] gives it the sending numbers and the time span
//! of the rows not yet `sent` (`queued`, `sending`, `unknown`, `failed`), so it can list Twilio's
//! sent log; [`replay_match`] matches each such row to the listed messages ([`ListedText`]) inside
//! `messaging`, by recipient (read under the send grant), sender, the body's SHA256 digest and
//! send time (a body redacted or dropped on either side matches on recipient, sender and a send
//! time near the row's claim, [`NO_BODY_WINDOW`]), answering row IDs only; [`mark_replayed_sent`]
//! marks one row sent with the SID found. Nothing here sends a text. These are exported through
//! `messaging::api`, so any higher crate could call them, but their one intended caller is T110.
//! The sending numbers are configuration, not stored, so both take the caller's [`Lines`]
//! (D-4005).

use askhava_ops::api::{Actor, AuditEntry, audit};
use askhava_privacy::api::{Field, Purpose, Reader, Vault, VaultError, read_contact};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::mode::Lines;

/// How far Twilio's clock may run behind the database's when a row was queued (D-4005).
pub const CLOCK_SKEW: SignedDuration = SignedDuration::from_mins(5);
/// A redacted or dropped body matches only a send from [`CLOCK_SKEW`] before the row's claim (its
/// queue time when never claimed) to this long after it (D-4005).
pub const NO_BODY_WINDOW: SignedDuration = SignedDuration::from_mins(10);
const UNSENT: [&str; 4] = ["queued", "sending", "unknown", "failed"];

/// One message from Twilio's sent log, as T110 listed it: its SID, `To`, `From`, body (none or
/// empty once redacted) and send time. `To` and the body are secrets, never logged (PRIV-016).
#[derive(Debug)]
pub struct ListedText {
    sid: String,
    to: SecretString,
    from: String,
    body: Option<SecretString>,
    sent_at: Timestamp,
}

impl ListedText {
    /// A listed message; `None` when `sid` is not a Twilio message SID (`SM` or `MM` and 32
    /// letters or digits), the form `messaging.outbox` stores.
    #[must_use]
    pub fn new(
        (sid, to, from): (&str, SecretString, &str),
        body: Option<SecretString>,
        sent_at: Timestamp,
    ) -> Option<Self> {
        let tail = sid.strip_prefix("SM").or_else(|| sid.strip_prefix("MM"))?;
        (tail.len() == 32 && tail.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| Self {
            sid: sid.to_owned(),
            to,
            from: from.to_owned(),
            body,
            sent_at,
        })
    }

    /// The message SID.
    #[must_use]
    pub fn sid(&self) -> &str {
        &self.sid
    }

    fn digest(&self) -> Option<[u8; 32]> {
        let body = self.body.as_ref().map(ExposeSecret::expose_secret);
        body.filter(|b| !b.is_empty()).map(sha256)
    }
}

/// What [`mark_replayed_sent`] did (OPS-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaySent {
    /// The row is now `sent` with the listed SID, its body cleared.
    Marked,
    /// The row was already `sent`; nothing changed.
    AlreadySent,
    /// Nothing changed, for the reason given.
    Refused(ReplayRefusal),
}

/// Why [`mark_replayed_sent`] changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayRefusal {
    /// No such outbox row.
    NotFound,
    /// The row is `blocked` or `expired`: it must never count as sent.
    Closed,
    /// The SID is already stored on another outbox row.
    SidTaken,
    /// The listed `To` is not the row's recipient, or the recipient cannot be read.
    WrongRecipient,
    /// The listed `From` is not one of Laftly's sending numbers.
    WrongSender,
}

/// [`replay_match`]'s answer for one unsent row; it names no number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayMatch {
    /// Exactly one listed message is this row's: its SID.
    Matched(String),
    /// More than one could be, or its recipient cannot be read: listed for Casey, never resent.
    Ambiguous,
    /// No listed message is this row's.
    Unmatched,
}

/// The rows not yet `sent`: the sending numbers to list from and the span to list (D-3889).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayWindow {
    /// Laftly's configured sending numbers (no recipient).
    pub numbers: Vec<String>,
    /// The earliest queue time, less [`CLOCK_SKEW`].
    pub earliest: Timestamp,
    /// The latest queue or claim time; T110 lists from `earliest` through its own run time.
    pub latest: Timestamp,
}

/// The replay helpers' one error; it carries no value (D-698).
#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    /// The database (or the vault's audit write) failed; nothing was changed.
    #[error("replay database call failed")]
    Database,
}

fn sha256(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

fn db<T, E>(r: Result<T, E>) -> Result<T, ReplayError> {
    r.map_err(|_| ReplayError::Database)
}

fn numbers(lines: &Lines) -> Vec<String> {
    [&lines.homeowner.number, &lines.business.number]
        .into_iter()
        .flatten()
        .map(|n| n.as_str().to_owned())
        .collect()
}

/// OPS-017, D-3889: the span and sending numbers of the unsent rows; `None` when there are none.
///
/// # Errors
/// [`ReplayError::Database`].
pub async fn replay_window(
    conn: &mut PgConnection,
    lines: &Lines,
) -> Result<Option<ReplayWindow>, ReplayError> {
    let row = db(sqlx::query!(
        "SELECT min(created_at) AS earliest, max(coalesce(sending_since, created_at)) AS latest \
         FROM messaging.outbox WHERE status = ANY($1)",
        &UNSENT.map(str::to_owned)[..]
    )
    .fetch_one(conn)
    .await)?;
    Ok(row.earliest.zip(row.latest).map(|(e, l)| ReplayWindow {
        numbers: numbers(lines),
        earliest: e
            .to_jiff()
            .saturating_sub(CLOCK_SKEW)
            .unwrap_or(e.to_jiff()),
        latest: l.to_jiff(),
    }))
}

/// D-3149, D-3877: a homeowner row's recipient phone, read as the sender reads it; `None` for a
/// business row or a contact whose phone can never be read.
async fn recipient_phone(
    conn: &mut PgConnection,
    vault: &Vault,
    (kind, recipient): (&str, Uuid),
) -> Result<Option<SecretString>, ReplayError> {
    if kind != "homeowner" {
        return Ok(None);
    }
    let grant = (Reader::Messaging, Purpose::Send);
    match read_contact(conn, vault, grant, recipient, &[Field::Phone]).await {
        Ok(contact) => Ok(contact.phone),
        Err(VaultError::NotFound | VaultError::BadPhone | VaultError::Sealed) => Ok(None),
        Err(_) => Err(ReplayError::Database),
    }
}

/// One unsent row as [`replay_match`] weighs it.
struct Unsent {
    id: Uuid,
    phone: Option<SecretString>,
    digest: Option<[u8; 32]>,
    queued: Timestamp,
    claimed: Timestamp,
}

impl Unsent {
    /// D-2764: whether `m`, from a sending number, could be this row's text.
    fn fits(&self, m: &ListedText) -> bool {
        let to = self.phone.as_ref().map(ExposeSecret::expose_secret);
        let after = self
            .queued
            .saturating_sub(CLOCK_SKEW)
            .unwrap_or(self.queued);
        let open = self
            .claimed
            .saturating_sub(CLOCK_SKEW)
            .unwrap_or(self.claimed);
        let close = self
            .claimed
            .saturating_add(NO_BODY_WINDOW)
            .unwrap_or(self.claimed);
        let body = match (self.digest, m.digest()) {
            (Some(ours), Some(theirs)) => ours == theirs,
            _ => open <= m.sent_at && m.sent_at <= close,
        };
        to == Some(m.to.expose_secret()) && m.sent_at >= after && body
    }
}

/// OPS-017, D-2764, D-3889: each unsent row's match among `listed`, by row ID. A message whose SID
/// an outbox row already holds, or not from a sending number, matches nothing; a message that
/// fits more than one row makes each of them ambiguous. No number is returned or logged.
///
/// # Errors
/// [`ReplayError::Database`].
pub async fn replay_match(
    conn: &mut PgConnection,
    (vault, lines): (&Vault, &Lines),
    listed: &[ListedText],
) -> Result<Vec<(Uuid, ReplayMatch)>, ReplayError> {
    let sids: Vec<String> = listed.iter().map(|m| m.sid.clone()).collect();
    let taken = db(sqlx::query_scalar!(
        r#"SELECT provider_sid AS "sid!" FROM messaging.outbox WHERE provider_sid = ANY($1)"#,
        &sids[..]
    )
    .fetch_all(&mut *conn)
    .await)?;
    let from = numbers(lines);
    let open: Vec<&ListedText> = listed
        .iter()
        .filter(|m| !taken.contains(&m.sid) && from.contains(&m.from))
        .collect();
    let rows = unsent_rows(conn, vault).await?;
    let fits = |r: &Unsent| -> Vec<&str> {
        let found = open.iter().filter(|m| r.fits(m));
        found.map(|m| m.sid.as_str()).collect()
    };
    let all: Vec<Vec<&str>> = rows.iter().map(fits).collect();
    let shared = |sid: &str| all.iter().filter(|f| f.contains(&sid)).count() > 1;
    Ok(rows
        .iter()
        .zip(&all)
        .map(|(r, f)| match f.as_slice() {
            _ if r.phone.is_none() => (r.id, ReplayMatch::Ambiguous),
            [] => (r.id, ReplayMatch::Unmatched),
            [sid] if !shared(sid) => (r.id, ReplayMatch::Matched((*sid).to_owned())),
            _ => (r.id, ReplayMatch::Ambiguous),
        })
        .collect())
}

/// The unsent rows, each with its recipient's phone read under the send grant.
async fn unsent_rows(conn: &mut PgConnection, vault: &Vault) -> Result<Vec<Unsent>, ReplayError> {
    let rows = db(sqlx::query!(
        "SELECT id, recipient_kind, recipient_id, body, created_at, sending_since \
         FROM messaging.outbox WHERE status = ANY($1) ORDER BY created_at, id",
        &UNSENT.map(str::to_owned)[..]
    )
    .fetch_all(&mut *conn)
    .await)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let queued = r.created_at.to_jiff();
        out.push(Unsent {
            id: r.id,
            phone: recipient_phone(conn, vault, (&r.recipient_kind, r.recipient_id)).await?,
            digest: r.body.as_deref().map(sha256),
            queued,
            claimed: r.sending_since.map_or(queued, |t| t.to_jiff()),
        });
    }
    Ok(out)
}

/// OPS-017, D-3753, D-3877: marks outbox row `row` sent with `listed`'s SID at `sent_at`, its body
/// cleared, under the row's lock, once, with one `ops` audit row of IDs only; it sends nothing.
/// Only a `queued`, `sending`, `unknown` or `failed` row moves; a sent row answers
/// [`ReplaySent::AlreadySent`]. Refused: a `blocked` or `expired` row, a SID another row holds, a
/// `To` that is not the row's recipient (read through `privacy::api` as the sender reads it), a
/// `From` that is not a sending number of `lines`. Its one intended caller is T110's replay.
///
/// # Errors
/// [`ReplayError::Database`]; nothing was changed.
pub async fn mark_replayed_sent(
    conn: &mut PgConnection,
    (vault, lines): (&Vault, &Lines),
    row: Uuid,
    listed: &ListedText,
    sent_at: Timestamp,
) -> Result<ReplaySent, ReplayError> {
    let found = db(sqlx::query!(
        "SELECT recipient_kind, recipient_id, status FROM messaging.outbox WHERE id = $1",
        row
    )
    .fetch_optional(&mut *conn)
    .await)?;
    let Some(found) = found else {
        return Ok(ReplaySent::Refused(ReplayRefusal::NotFound));
    };
    if let Err(done) = unsent(&found.status) {
        return Ok(done);
    }
    if !numbers(lines).contains(&listed.from) {
        return Ok(ReplaySent::Refused(ReplayRefusal::WrongSender));
    }
    let phone = recipient_phone(conn, vault, (&found.recipient_kind, found.recipient_id)).await?;
    if phone.as_ref().map(ExposeSecret::expose_secret) != Some(listed.to.expose_secret()) {
        return Ok(ReplaySent::Refused(ReplayRefusal::WrongRecipient));
    }
    mark_locked(conn, row, &listed.sid, sent_at).await
}

/// An unsent row's status, or the answer for a row whose status already decides it: sent, or
/// never to count as sent. The stored text is parsed here, at the edge, and never kept as text (SEC-031).
fn unsent(stored: &str) -> Result<&'static str, ReplaySent> {
    match UNSENT.into_iter().find(|s| *s == stored) {
        Some(s) => Ok(s),
        None if stored == "sent" => Err(ReplaySent::AlreadySent),
        None => Err(ReplaySent::Refused(ReplayRefusal::Closed)),
    }
}

/// The locked half of [`mark_replayed_sent`]: the SID's advisory lock, then the row's, the checks
/// again, the update and its audit row in one transaction.
async fn mark_locked(
    conn: &mut PgConnection,
    row: Uuid,
    sid: &str,
    sent_at: Timestamp,
) -> Result<ReplaySent, ReplayError> {
    let mut tx = db(conn.begin().await)?;
    db(sqlx::query!(
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 481))",
        sid
    )
    .execute(&mut *tx)
    .await)?;
    let status = db(sqlx::query_scalar!(
        "SELECT status FROM messaging.outbox WHERE id = $1 FOR UPDATE",
        row
    )
    .fetch_one(&mut *tx)
    .await)?;
    let from = match unsent(&status) {
        Ok(from) => from,
        Err(done) => return Ok(done),
    };
    let taken = db(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM messaging.outbox WHERE provider_sid = $1) AS "taken!""#,
        sid
    )
    .fetch_one(&mut *tx)
    .await)?;
    if taken {
        return Ok(ReplaySent::Refused(ReplayRefusal::SidTaken));
    }
    db(sqlx::query!(
        "UPDATE messaging.outbox SET status = 'sent', provider_sid = $2, sent_at = $3, body = NULL \
         WHERE id = $1",
        row,
        sid,
        sent_at.to_sqlx() as _
    )
    .execute(&mut *tx)
    .await)?;
    let entry = AuditEntry::status_change(Actor::System, ("messaging.outbox", row), from, "sent");
    db(audit(&mut tx, &entry).await)?;
    db(tx.commit().await)?;
    Ok(ReplaySent::Marked)
}
