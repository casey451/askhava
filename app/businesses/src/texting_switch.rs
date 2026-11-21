//! The business-texting switch as the layers above `messaging`'s callers read it (T526, TXT-035,
//! BIZ-036, WEB-024, D-3881, D-3882).
//!
//! `site` may not call `messaging` (`kit/layers.json`), so its business page asks
//! [`business_texting_on`] here. The answer fails closed: texting counts as on only while the
//! stored switch (`messaging.mode_state`, `messaging::api::load_lines`) is on and the business
//! line's own sending number (`ASKHAVA_BUSINESS_NUMBER`) is configured; the switch off, no number
//! or a failed read is off, so a page never offers a channel nothing can send on.

use askhava_messaging::api::{SendingNumber, load_lines};
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
