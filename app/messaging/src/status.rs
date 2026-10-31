//! A text's delivery status and how Twilio's status names fold into it (TXT-013, SEC-031).
//!
//! Every `match` on [`DeliveryStatus`] lives here and names every variant, so a new status does
//! not build until each place handles it (`docs/architecture.md` section 4 rule 5).
#![deny(
    clippy::wildcard_enum_match_arm,
    clippy::match_wildcard_for_single_variants
)]

/// A text's delivery status (TXT-013): the outbox's own `queued` and `sending` until the carrier
/// took it, then `sent` and whatever Twilio's status callbacks reported after that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryStatus {
    /// Waiting to be sent, or accepted or scheduled by Twilio.
    Queued,
    /// Being handed to the carrier, or Twilio is sending it.
    Sending,
    /// The carrier passed it on; no final word yet.
    Sent,
    /// The handset confirmed it (Twilio `delivered` or `read`).
    Delivered,
    /// The carrier could not deliver it.
    Undelivered,
    /// Twilio could not send it (`failed` or `canceled`).
    Failed,
}

impl DeliveryStatus {
    /// Folds a Twilio `MessageStatus` to a status; `None` for one that is not an outbound text's
    /// (such as `received`), which is ignored (TXT-013).
    #[must_use]
    pub fn from_twilio(name: &str) -> Option<Self> {
        Some(match name {
            "accepted" | "scheduled" | "queued" => Self::Queued,
            "sending" => Self::Sending,
            "sent" => Self::Sent,
            "delivered" | "read" => Self::Delivered,
            "undelivered" => Self::Undelivered,
            "failed" | "canceled" => Self::Failed,
            _ => return None,
        })
    }

    /// The stored name, one of `messaging.delivery_event.status`'s values.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Delivered => "delivered",
            Self::Undelivered => "undelivered",
            Self::Failed => "failed",
        }
    }

    /// How far along a text is: a callback that arrives late with a lower rank never moves the
    /// status back (TXT-013).
    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::Queued => 0,
            Self::Sending => 1,
            Self::Sent => 2,
            Self::Delivered | Self::Undelivered | Self::Failed => 3,
        }
    }
}
