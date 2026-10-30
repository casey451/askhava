//! The public face of `askhava-adapters`: everything other crates may call (D-481).
//!
//! The Twilio settings and fixed masked pool (CALL-017), the derived
//! idempotency key (SEC-018), and the texting, voice and Verify traits with
//! their one HTTP client each (SEC-032).

pub use crate::base::{AdapterError, IdempotencyKey, twilio_clients};
pub use crate::config::{AdapterMode, DEFAULT_POOL_SIZE, MaskedPool, PoolNumber, TwilioConfig};
pub use crate::sms::{SentText, SmsAdapter, TwilioSms};
pub use crate::verify::{CodeChannel, TwilioVerify, VerifyAdapter};
pub use crate::voice::{CallState, TwilioVoice, VoiceAdapter};
