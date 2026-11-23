//! The texting switches as the layers above `messaging`'s callers read them (T526, T725; TXT-035,
//! BIZ-036, WEB-017, WEB-024, CON-005, D-3881, D-3882, D-4709).
//!
//! `site` may not call `messaging` (`kit/layers.json`), so its pages ask here. Both answers fail
//! closed. Business texting counts as on only while the stored switch (`messaging.mode_state`,
//! `messaging::api::load_lines`) is on and the business line's own sending number
//! (`ASKHAVA_BUSINESS_NUMBER`) is configured. The home page's texting number
//! ([`homeowner_texting_number`]) is answered only from the Gate C launch instant
//! (`ASKHAVA_LAUNCH_DATE`) on, while the homeowner line has an open sender (switch on, number
//! configured, not paused). The switch off, no number, no date or a failed read is off (the
//! caller logs the read error), so a page never offers a channel nothing can send on.

use askhava_messaging::api::{ModeStateError, SendingNumber, load_lines};
use jiff::Timestamp;
use sqlx::PgConnection;

/// TXT-035, BIZ-036, WEB-024, D-3881: whether business texting is on: the stored switch is on
/// and `business_number` is configured. The switch off, no number or a failed read is `false`.
pub async fn business_texting_on(
    db: &mut PgConnection,
    business_number: Option<SendingNumber>,
) -> bool {
    load_lines(db, (None, business_number))
        .await
        .is_ok_and(|lines| lines.business.texting_on && lines.business.number.is_some())
}

/// WEB-017, CON-005, D-4709, D-5279: the homeowner texting number the home page shows, as
/// configured (`+1` and ten digits): `Some` only when `launch` (the Gate C instant) is set and not
/// after `now`, and the homeowner line's `sender()` is open (switch on, `number` configured, not
/// paused). No date, a later date, no number, the switch off or a paused number is `None`; no
/// database read is made without a reached date and a number.
///
/// # Errors
///
/// `messaging`'s read error; the caller shows no number and logs one warning without values.
pub async fn homeowner_texting_number(
    db: &mut PgConnection,
    (number, launch): (Option<SendingNumber>, Option<Timestamp>),
    now: Timestamp,
) -> Result<Option<String>, ModeStateError> {
    if !launch.is_some_and(|launch| launch <= now) || number.is_none() {
        return Ok(None);
    }
    let lines = load_lines(db, (number, None)).await?;
    Ok(lines.homeowner.sender().map(|n| n.as_str().to_owned()))
}
