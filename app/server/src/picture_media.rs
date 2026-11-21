//! The production picture-text media at Twilio (INT-044, SEC-018, SEC-032, SEC-041, PRIV-014;
//! T794, D-5251): [`TwilioPictureMedia`] is the `privacy::api::PictureTextMedia` the composition
//! hands to `take_picture_text` (registered by T675). The fetch is `privacy`'s guarded
//! `fetch_twilio_media` (Twilio's hosts only) and the delete only delegates to the adapter's
//! `SmsMediaDelete::delete_media_at`, so the URL parse and its tests live in `adapters`.

use askhava_adapters::api::{AdapterError, FetchError, IdempotencyKey, SmsMediaDelete, TwilioSms};
use askhava_privacy::api::{PictureTextMedia, fetch_twilio_media};

/// A picture text's media at Twilio, reached through the one texting client (D-453, SEC-032).
#[derive(Debug, Clone)]
pub struct TwilioPictureMedia {
    sms: TwilioSms,
}

impl TwilioPictureMedia {
    /// The media of picture texts sent to the account `sms` talks to.
    #[must_use]
    pub fn new(sms: TwilioSms) -> Self {
        Self { sms }
    }
}

impl PictureTextMedia for TwilioPictureMedia {
    /// SEC-041: the guarded fetch from Twilio's hosts only, with its size cap and timeout.
    async fn fetch_media(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        fetch_twilio_media(url).await
    }

    /// INT-044, PRIV-014, SEC-018, SEC-032: deletes the media at Twilio once stored; a URL of
    /// another shape or account is `InvalidId` and nothing is sent.
    async fn delete_media(
        &self,
        idempotency: &IdempotencyKey,
        url: &str,
    ) -> Result<(), AdapterError> {
        self.sms.delete_media_at(idempotency, url).await
    }
}
