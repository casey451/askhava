//! The consent guard for request texts (T856; CON-005, CON-007, CON-021, CON-001, INT-004,
//! D-5848, D-5918, D-5971).
//!
//! [`request_text`] takes a request and the kind of text about to be queued for it and answers
//! [`Delivery::Outbox`], or refuses with [`IntakeError::NoConsent`] (fail closed): a request text
//! goes only when the request has an `sms` consent record of its own
//! (`models::sms_consented`). Named exemptions only: the consent text of a carrier-inbound first
//! text ([`TextKind::CarrierConsent`]), the INT-004 safety text of a carrier-inbound request with
//! no consent record yet ([`TextKind::CarrierSafety`]; a web request's safety text is shown on
//! the page), the carrier keyword replies ([`TextKind::CarrierKeyword`]), the business sign-up
//! reply ([`TextKind::BusinessSignup`]), and the chat exemption: a request whose only consent
//! records are `web_chat` keeps outbox delivery whatever the caller, until T801 deletes this
//! branch. T856 calls it at its own call sites; T857 and T865 put every caller behind it.

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{self, IntakeError};

/// What a request text is, as the guard's exemptions name it (D-5848).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    /// The consent text answering a carrier-inbound first text (CON-001, CON-031).
    CarrierConsent,
    /// The INT-004 safety text answering a carrier-inbound text (INT-004, D-008).
    CarrierSafety,
    /// A carrier keyword reply (STOP, HELP, START), decided by the send path (CON-008, CON-018).
    CarrierKeyword,
    /// The business sign-up reply to a business-intent first text; it opens no request (D-132).
    BusinessSignup,
    /// Any other text about the request: it needs the request's `sms` record (CON-021).
    Request,
}

/// How a guarded request text goes (D-5848): today only through the outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Queue it in the outbox as before.
    Outbox,
}

/// The guard (CON-005, CON-007, CON-021, INT-004, D-5848): whether a text of `kind` about
/// `request` may be queued. Keyword and business sign-up replies and the consent text of a
/// carrier-inbound first text always go; a carrier safety text goes while the request has no
/// record yet; otherwise the request's `sms` record lets it go, and a request whose records are
/// all `web_chat` keeps the outbox under the chat exemption (deleted by T801).
///
/// # Errors
///
/// [`IntakeError::NoConsent`] when no record lets it go (and for no request on a request text);
/// [`IntakeError::Database`].
pub async fn request_text(
    db: &mut PgConnection,
    request: Option<Uuid>,
    kind: TextKind,
) -> Result<Delivery, IntakeError> {
    match kind {
        TextKind::CarrierKeyword | TextKind::BusinessSignup | TextKind::CarrierConsent => {
            return Ok(Delivery::Outbox);
        }
        TextKind::CarrierSafety | TextKind::Request => {}
    }
    let Some(request) = request else {
        return Err(IntakeError::NoConsent);
    };
    let records = models::consent_records(db, request).await?;
    if records.is_empty() && kind == TextKind::CarrierSafety {
        return Ok(Delivery::Outbox);
    }
    let sms = records.iter().any(|r| r.channel == "sms");
    let web_chat_only = !sms && records.iter().any(|r| r.channel == "web_chat");
    if sms || web_chat_only {
        Ok(Delivery::Outbox)
    } else {
        Err(IntakeError::NoConsent)
    }
}

/// [`request_text`] read as a yes or no: `false` on [`IntakeError::NoConsent`], other errors kept.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub(crate) async fn may_queue(
    db: &mut PgConnection,
    request: Uuid,
    kind: TextKind,
) -> Result<bool, IntakeError> {
    match request_text(db, Some(request), kind).await {
        Ok(Delivery::Outbox) => Ok(true),
        Err(IntakeError::NoConsent) => Ok(false),
        Err(e) => Err(e),
    }
}
