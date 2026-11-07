//! The public face of `askhava-portals`: everything other crates may call (D-481).
//!
//! The shared web security (SEC-009, SEC-012, SEC-013, SEC-034, SEC-042, SEC-048): the header
//! and cross-site middleware, the CSRF form extractor, sessions with their `__Host-` cookies and
//! realm proofs for vault reads (SEC-046), and the security event log with its hourly alert.
//! The homeowner's quote page router and its pure numbering and 30-day rules (PICK-008 to
//! PICK-012, SEC-037, T052). The pick review step's router and the pick result page (PICK-016,
//! PICK-017, PICK-054, A11Y-014, T055). The business's Confirm review step and its press answers
//! (PICK-021, FEE-014, FEE-045, CON-029, T224). The "Why was I charged?" page of a charge (FEE-031,
//! T068). The business sign-up screens, their WEB-024 notice line, fee numbers and done-screen
//! facts (BIZ-024, BIZ-026, BIZ-030, BIZ-031, LIC-047, T193). Business sign-in by emailed link,
//! the 30-day session and the business principal with Confirm's scope (SEC-004, SEC-019, PICK-033,
//! T084); the new-device code by Verify text or email (SEC-004, TXT-038, T194).

/// The cookie jar the session functions take and give back (axum-extra's).
pub use axum_extra::extract::cookie::CookieJar;

pub use crate::business_auth::{
    AddressForm, BUSINESS_SESSION_LIFE, BusinessPrincipal, CodeAddress, CodeForm, CodeFuture,
    CodeTo, DEVICE_COOKIE, DEVICE_LIFE, DeviceCodes, LINK_LIFE, LINKS_PER_LIFE, Need,
    SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress, SignInMail, business_principal,
    run_member_removed, signin_form, signin_open, signin_post, signin_request,
};
pub use crate::business_signup::{
    DoneError, DoneFacts, NOTICE_OFF, NOTICE_ON, PEST_ATTESTATION, Parts, SIGNUP_ROUTE, Screen,
    done_facts, done_lines, fee_numbers, notice_line, signup_page,
};
pub use crate::confirm_pay::{
    ConfirmFacts, FEE_FREE_LINE, NoFee, PORTAL_LINK, PressReply, REFUND_LINE, confirm_page,
    may_confirm, press_reply,
};
pub use crate::pick_flow::{
    CODE_ROUTE, PICK_ROUTE, PageLanguage, PickCodeKeys, VERIFY_DAILY_THRESHOLD, pick_flow_routes,
    result_page,
};
pub use crate::quote_page::{
    Numbered, QUOTE_PAGE_ROUTE, TOKEN_LIFE_AFTER_CLOSE, number_quotes, quote_page_routes,
    token_expired,
};
pub use crate::receipts_page::{Basis, WHY_CHARGED_ROUTE, WhyCharged, why_charged_page};
pub use crate::security::{
    ButtonForm, CONTENT_SECURITY_POLICY, CsrfField, CsrfForm, EVENTS_PER_HOUR_ALERT,
    PERMISSIONS_POLICY, STRICT_TRANSPORT_SECURITY, SecurityEvent, SecurityEventKind, WebSecurity,
    is_cross_site, no_store, record_security_event, refuse_cross_site, security_headers,
};
pub use crate::session::{
    PRE_SESSION_COOKIE, PortalsError, Realm, SESSION_COOKIE, Session, csrf_matches, csrf_token,
    current_session, ensure_pre_session, start_session,
};
