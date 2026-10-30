//! The public face of `askhava-adapters`: everything other crates may call (D-481).
//!
//! The Twilio settings and fixed masked pool (CALL-017), the derived
//! idempotency key (SEC-018), and the texting, voice and Verify traits with
//! their one HTTP client each (SEC-032); the payments, AI (stand-in only,
//! AI-009), email (with suppression removal, BIZ-056), letters, license-source
//! and storage adapters; the guarded fetch of untrusted URLs (SEC-041); and the inbound webhook
//! signature and Stripe source-address checks (SEC-016, SEC-039, SEC-049).

pub use crate::ai::{AiAdapter, StandInAi};
pub use crate::base::{
    AdapterError, FetchError, FetchPolicy, IdempotencyKey, MAX_REDIRECTS, TWILIO_MEDIA_HOSTS,
    fetch_untrusted, is_public_address, twilio_clients,
};
pub use crate::config::{AdapterMode, DEFAULT_POOL_SIZE, MaskedPool, PoolNumber, TwilioConfig};
pub use crate::email::{Email, EmailAdapter, ResendEmail};
pub use crate::letters::{LettersAdapter, LobLetters, SentLetter};
pub use crate::license_sources::{LicenseLists, LicenseSource};
pub use crate::payments::{Charge, IntentMetadata, PaymentIntent, PaymentsAdapter, StripePayments};
pub use crate::signatures::{
    Delivery, REJECTED_STATUS, SignatureError, VerifiedWebhook, WEBHOOK_TOLERANCE_SECS,
    WebhookVendor, check_webhook_source, refuse, verify_webhook, webhook_source_list,
};
pub use crate::sms::{SentText, SmsAdapter, TwilioSms};
pub use crate::storage::{LocalStorage, StorageAdapter};
pub use crate::verify::{CodeChannel, TwilioVerify, VerifyAdapter};
pub use crate::voice::{CallState, TwilioVoice, VoiceAdapter};
