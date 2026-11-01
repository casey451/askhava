//! Intake's reading of other modules' status enums (SEC-031): every `match` on one lives here and
//! names every variant, so a new status does not build until it is handled.
#![deny(
    clippy::wildcard_enum_match_arm,
    clippy::match_wildcard_for_single_variants
)]

use askhava_messaging::api::DeliveryStatus;

/// What a watching card timer does on the card text's delivery status (INT-041, D-1278):
/// `Some(true)` looks again (not yet sent), `Some(false)` starts the wait (sent), `None` stops
/// (the text failed or was not delivered).
pub(crate) fn card_text_watch(status: DeliveryStatus) -> Option<bool> {
    match status {
        DeliveryStatus::Queued | DeliveryStatus::Sending => Some(true),
        DeliveryStatus::Sent | DeliveryStatus::Delivered => Some(false),
        DeliveryStatus::Failed | DeliveryStatus::Undelivered => None,
    }
}
