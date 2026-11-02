//! When a non-urgent text to a homeowner may go, judged where her number may be read (QH-001,
//! QH-003, QH-006, CON-013, CON-035; D-1196, D-1301).
//!
//! A module above `messaging` that queues a homeowner text holds only her vault contact ID: the
//! vault grants the phone to `messaging` and `calls`, never to `intake` (PRIV-004). So
//! [`send_window`] reads the phone here under `messaging`'s send grant, checks the one suppression
//! list (an opted-out number gets no text, CON-013, CON-028), and answers with [`window`], the
//! pure part: send now when it is 8:00 a.m. to before 8:00 p.m. in both Arizona and every zone of
//! the area code, else the next instant both are open, by `rules::decide` (QH-001, QH-006). A
//! number outside the area-code table is never texted (QH-003). The phone never leaves this file.

use askhava_privacy::api::{
    Field, IndexPurpose, Purpose, Reader, Vault, normalize_phone, read_contact,
};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{self, OutboxError};
use crate::rules::{AreaCodes, Channel, Decision, Kind, Outgoing, QuietHoursError, decide};

/// When a non-urgent homeowner text may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendWindow {
    /// Inside the window in both zones: send now (QH-001).
    Now,
    /// Outside it: queue the text again for `until`, the next instant both zones are open (QH-006).
    Hold {
        /// The start of the next allowed window.
        until: Timestamp,
    },
    /// The number is on the suppression list: no text at all (CON-013, CON-028, CON-035).
    OptedOut,
    /// Not a `+1` number or its area code is not in the table: never texted (QH-003, D-139).
    NotTextable,
}

/// The window for an update text (`Kind::Update`, never a QH-004 night exception) to `phone` at
/// `now` (QH-001, QH-003, QH-006). Pure: no clock, no I/O.
///
/// # Errors
///
/// [`OutboxError::Template`] when the compiled-in area-code table or the zone data is broken.
pub fn window(table: &AreaCodes, phone: &str, now: Timestamp) -> Result<SendWindow, OutboxError> {
    let msg = Outgoing {
        phone,
        kind: Kind::Update,
        channel: Channel::Text,
        emergency_job: false,
        after_hours_business: false,
        emergency_emails: false,
    };
    match decide(table, &msg, now) {
        Ok(Decision::Send) => Ok(SendWindow::Now),
        Ok(Decision::Hold { until }) => Ok(SendWindow::Hold { until }),
        Err(QuietHoursError::NotNorthAmerican | QuietHoursError::UnknownAreaCode) => {
            Ok(SendWindow::NotTextable)
        }
        Err(QuietHoursError::InvalidTable | QuietHoursError::Zone) => Err(OutboxError::Template),
    }
}

/// The window for a non-urgent text to the homeowner with vault contact `contact` at `now`: her
/// phone is read from the vault here (`Reader::Messaging`, `Purpose::Send`, one audited read), an
/// opted-out number answers [`SendWindow::OptedOut`], else [`window`] (QH-001, QH-006, CON-013).
/// Call it outside a transaction: the vault refuses a read inside one.
///
/// # Errors
///
/// [`OutboxError::Recipient`] when the phone cannot be read, [`OutboxError::Database`] or
/// [`OutboxError::Template`].
pub async fn send_window(
    db: &mut PgConnection,
    vault: &Vault,
    contact: Uuid,
    now: Timestamp,
) -> Result<SendWindow, OutboxError> {
    let phone = phone_of(db, vault, contact).await?;
    let Ok(phone) = normalize_phone(phone.expose_secret()) else {
        return Ok(SendWindow::NotTextable);
    };
    let digests = vault.blind_index(IndexPurpose::HomeownerPhone, &phone);
    if digests.is_empty() {
        return Err(OutboxError::Database);
    }
    if models::listing(db, &digests).await?.is_some() {
        return Ok(SendWindow::OptedOut);
    }
    let table = AreaCodes::embedded().map_err(|_| OutboxError::Template)?;
    window(&table, &phone, now)
}

/// Her phone from the vault under `messaging`'s send grant (PRIV-003, PRIV-004).
async fn phone_of(
    db: &mut PgConnection,
    vault: &Vault,
    contact: Uuid,
) -> Result<SecretString, OutboxError> {
    let read = (Reader::Messaging, Purpose::Send);
    read_contact(db, vault, read, contact, &[Field::Phone])
        .await
        .map_err(|_| OutboxError::Recipient)?
        .phone
        .ok_or(OutboxError::Recipient)
}
