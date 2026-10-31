//! The public face of `askhava-portals`: everything other crates may call (D-481).
//!
//! The shared web security (SEC-009, SEC-012, SEC-013, SEC-034, SEC-042, SEC-048): the header
//! and cross-site middleware, the CSRF form extractor, sessions with their `__Host-` cookies and
//! realm proofs for vault reads (SEC-046), and the security event log with its hourly alert.

/// The cookie jar the session functions take and give back (axum-extra's).
pub use axum_extra::extract::cookie::CookieJar;

pub use crate::security::{
    ButtonForm, CONTENT_SECURITY_POLICY, CsrfField, CsrfForm, EVENTS_PER_HOUR_ALERT,
    PERMISSIONS_POLICY, STRICT_TRANSPORT_SECURITY, SecurityEvent, SecurityEventKind, WebSecurity,
    is_cross_site, no_store, record_security_event, refuse_cross_site, security_headers,
};
pub use crate::session::{
    PRE_SESSION_COOKIE, PortalsError, Realm, SESSION_COOKIE, Session, csrf_matches, csrf_token,
    current_session, ensure_pre_session, start_session,
};
