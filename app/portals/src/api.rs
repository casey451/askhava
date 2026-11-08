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
//! T084); the new-device code by Verify text or email (SEC-004, TXT-038, T194). The private
//! feedback page's router after Done (JOB-017, T230). The business portal home with "Your numbers",
//! the open-cards times and the SEC-038 step-up gate, and the released job's page with its call
//! log (BIZ-013, BIZ-025, BIZ-035, CARD-022, PICK-024, SEC-038, T085). Owner sign-in: the
//! argon2id password plus a passkey, TOTP or recovery code, the 12-hour session ending after 30
//! minutes idle, the 5-minute step-up, logout and "Sign out everywhere", the one-time bootstrap
//! and the owner-only pause, refund and release gate (SEC-005 to SEC-007, SEC-043, SEC-044, T086).
//! The homeowner portal: sign-in by phone and texted code, her own record through her session
//! proof, the texts-off switch, "Download my data" and the code-checked delete or copy request
//! (SEC-008, PRIV-012, PRIV-021, PRIV-031, CON-035, T087). The rate limits in front of sign-in,
//! codes, sign-ups and quotes (SEC-014, SEC-020, SEC-040, PICK-055, T089). The email action links
//! for every former text keyword, the action pages, the quote form's fee lines and quote facts,
//! "Ask about a charge" and email replies that never act (BIZ-028, BIZ-032, BIZ-038, PICK-006,
//! PICK-039, PICK-040, SEC-025, SEC-026, A11Y-023, T148).

/// The cookie jar the session functions take and give back (axum-extra's).
pub use axum_extra::extract::cookie::CookieJar;

pub use crate::action_links::{
    ACTION_ROUTE, ActionGrant, ActionKind, ActionLinkError, ActionTarget, EmailReply,
    OPEN_IN_BROWSER_LINE, UseBy, email_reply, mint_action_link, open_action_link, session_may,
    use_action_link,
};
pub use crate::business_actions::{
    ActionFacts, BUSY_DAYS_ERROR, BUSY_DAYS_MAX, ChargeAsked, FactProblem, FactsForm,
    HELP_REQUEST_KIND, QUOTE_MINUTES_MAX, QuoteFormFacts, action_email_text, action_page,
    ask_about_charge, busy_days, busy_until_line, charge_answer, charge_reply_line, does_line,
    fee_lines, parse_facts, prefill, quote_form_page, quote_minutes,
};
pub use crate::business_auth::{
    AddressForm, BUSINESS_SESSION_LIFE, BusinessPrincipal, CODE_TRIES, CodeAddress, CodeForm,
    CodeFuture, CodeTo, DEVICE_COOKIE, DEVICE_LIFE, DeviceCodes, LINK_LIFE, LINKS_PER_LIFE, Need,
    SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress, SignInMail, business_principal,
    run_member_removed, signin_form, signin_open, signin_post, signin_request,
};
pub use crate::business_portal::{
    BILLING_ROUTE, BUSINESS_HOME_ROUTE, BillingChangeError, COMPLAINT_REPLY_ROUTE,
    COMPLAINTS_ROUTE, OPEN_CARDS_ROUTE, PaymentMethod, RefusedReply, STATEMENT_ROUTE,
    SensitiveChange, StepUpError, YourNumbers, billing_page, business_home_page, change_fee_limit,
    change_saved_card, complaints_page, reply_to_complaint, require_step_up, save_open_cards_times,
    statement_csv,
};
pub use crate::business_signup::{
    DoneError, DoneFacts, NOTICE_OFF, NOTICE_ON, PEST_ATTESTATION, Parts, SIGNUP_ROUTE, Screen,
    done_facts, done_lines, fee_numbers, notice_line, signup_page,
};
pub use crate::confirm_pay::{
    ConfirmFacts, FEE_FREE_LINE, NoFee, PORTAL_LINK, PressReply, REFUND_LINE, confirm_page,
    may_confirm, press_reply,
};
pub use crate::data_download::{
    ANSWER_WITHIN, Acted, DATA_CODE_ROUTE, DATA_REQUEST_ROUTE, DOWNLOAD_ROUTE, DataRequest,
    DataRequestKind, HeldItem, RequestError, Section, confirm_request, download_file, open_request,
};
pub use crate::feedback::{FEEDBACK_ROUTE, SHARE_LABEL, feedback_routes};
pub use crate::homeowner_portal::{
    HOMEOWNER_CODE_ROUTE, HOMEOWNER_HOME_ROUTE, HOMEOWNER_SESSION_LIFE, HOMEOWNER_SIGNIN_ROUTE,
    HomeFacts, HomeownerError, OnFile, PortalScreen, TEXTS_OFF_LINE, TEXTS_OFF_ROUTE,
    finish_sign_in, homeowner_page, send_sign_in_code, turn_texts_off,
};
pub use crate::job_release::{
    CallAttempt, EMAIL_CHOSEN_LINE, JOB_RELEASE_ROUTE, JobFacts, JobReleaseError, MissedText,
    TIP_LINE, job_release_page,
};
pub use crate::limits::{
    CODE_SENDS_PER_IP, CODE_SENDS_PER_NUMBER, CODE_SENDS_UNKNOWN, CODE_TRIES_PER_BUSINESS,
    CODE_TRIES_PER_IP, LINKS_PER_BUSINESS, LINKS_PER_IP, OWNER_TRIES_ALL, OWNER_TRIES_PER_IP,
    QUOTES_PER_BUSINESS, QUOTES_PER_IP, SIGNUPS_ALL, SIGNUPS_PER_IP, admit_code_send,
    admit_code_try, admit_owner_password, admit_quote, admit_sign_in_link, admit_sign_up,
    verify_all,
};
pub use crate::notice_settings::{
    ChannelChangeError, ChannelFacts, NOTICE_SETTINGS_ROUTE, NoticeAddress, change_channel,
    channel_choices, notice_settings_page,
};
pub use crate::owner_auth::{
    BOOTSTRAP_TOKEN_VAR, MAX_LIVE_SIGN_IN_CHALLENGES, MIN_OWNER_FACTORS, MIN_OWNER_PASSWORD_CHARS,
    OWNER_IDLE_LIMIT, OWNER_PASSKEY_ROUTE, OWNER_SESSION_LIFE, OWNER_SIGNIN_ROUTE,
    OWNER_STEP_UP_WINDOW, OwnerAction, OwnerAuth, OwnerAuthError, OwnerEnrolment, OwnerPower,
    OwnerPrincipal, PasswordProblem, RECOVERY_CODE_COUNT, SecondFactor, add_owner_totp,
    bootstrap_owner, change_owner_password, factor_fresh, finish_passkey_registration, idle_ended,
    owner_power, owner_principal, owner_sign_in, owner_sign_out, owner_signin_page, owner_step_up,
    passkey_registered, passkey_registration_options, passkey_sign_in_options, password_problem,
    require_owner_step_up, sign_out_everywhere,
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
