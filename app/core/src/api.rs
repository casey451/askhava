//! The public face of `askhava-core`: everything other crates may call (D-481).
//!
//! Settings (SEC-001, SEC-002, SEC-003, SEC-036), the clock and business clock
//! (JOB-007), the database pool, migrations and the guarded-update outcome
//! (JOB-004, D-092), and logging with `Redacted` contact values and the Sentry
//! scrubber (PRIV-016, PRIV-017, PRIV-028).

pub use crate::clock::{
    BUSINESS_CLOSE_HOUR, BUSINESS_OPEN_HOUR, Clock, ClockError, ManualClock, SystemClock,
    add_business_minutes, arizona, arizona_time, is_business_hours, local_time, zone,
};
pub use crate::db::{DbError, Guarded, MIGRATOR, app_pool, migrate};
pub use crate::logging::{
    LogGuard, LoggingError, REDACTED, Redacted, init as init_logging, json_layer, request_span,
    scrub_breadcrumb, scrub_event, scrub_text, sentry_options,
};
pub use crate::settings::{Environment, Settings, SettingsError, Vendor, check_base_url};
