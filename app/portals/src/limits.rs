//! The portals' rate limits: sign-in links, code sends and code tries, owner password tries,
//! business sign-ups and quote submissions, per client IP, per business and for everyone
//! (SEC-014, SEC-015, SEC-020, SEC-040, PICK-055, D-491, D-1837, T089).
//!
//! Every limit runs through `intake::api::admit` (one atomic counter per bucket, narrowest
//! first, one High alert per scope and window on the first refusal). The client IP is the one
//! `core::api::ClientIpPolicy::client_ip` reads: the edge header `ASKHAVA_CLIENT_IP_HEADER` names,
//! `CF-Connecting-IP` only on a proxied hostname from Cloudflare's ranges, never
//! `X-Forwarded-For` (SEC-040). These limits sit in front of T272's per-member link cap and
//! per-link code-try cap in `business_auth.rs` and count requests, never links or tries (D-1837).
//! The owner's passkey path calls none of them, so it is never hard-locked (SEC-040, D-492).
//! Each portal route calls its `admit_*` first and, on a refusal, answers as for any failure.

use std::net::IpAddr;

use askhava_intake::api::{Admission, Key, Limit, LimitError, admit};
use jiff::Timestamp;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// Sign-in link requests per client IP per hour (SEC-014, SEC-020).
pub const LINKS_PER_IP: Limit = Limit::hourly("portals.signin_links_per_ip", 10);
/// Sign-in link requests per business per hour, all its members together (SEC-014, D-1837).
pub const LINKS_PER_BUSINESS: Limit = Limit::hourly("portals.signin_links_per_business", 10);
/// Code tries (pick, sign-in and data codes) per client IP per hour (SEC-020).
pub const CODE_TRIES_PER_IP: Limit = Limit::hourly("portals.code_tries_per_ip", 20);
/// Code tries per business per hour, all its links together (SEC-020, D-1837).
pub const CODE_TRIES_PER_BUSINESS: Limit = Limit::hourly("portals.code_tries_per_business", 20);
/// Code-send requests per client IP per hour, numbers on file or not (SEC-014, PICK-055).
pub const CODE_SENDS_PER_IP: Limit = Limit::hourly("portals.code_sends_per_ip", 10);
/// Portal Verify sends per destination (her vault contact) per Arizona day (PICK-055).
pub const CODE_SENDS_PER_NUMBER: Limit = Limit::daily("portals.code_sends_per_number", 5);
/// Owner password tries per client IP per hour (SEC-020, SEC-040).
pub const OWNER_TRIES_PER_IP: Limit = Limit::hourly("portals.owner_tries_per_ip", 5);
/// Owner password tries on the one owner account per hour, from any IP (SEC-040).
pub const OWNER_TRIES_ALL: Limit = Limit::hourly("portals.owner_tries_all", 20);
/// Business sign-ups per client IP per Arizona day (SEC-014).
pub const SIGNUPS_PER_IP: Limit = Limit::daily("portals.signups_per_ip", 5);
/// Business sign-ups for everyone per Arizona day (SEC-014).
pub const SIGNUPS_ALL: Limit = Limit::daily("portals.signups_all", 50);
/// Quote submissions per business per hour (SEC-014).
pub const QUOTES_PER_BUSINESS: Limit = Limit::hourly("portals.quotes_per_business", 30);
/// Quote submissions per client IP per hour (SEC-014).
pub const QUOTES_PER_IP: Limit = Limit::hourly("portals.quotes_per_ip", 60);

/// The portal Verify sends for everyone per Arizona day; past `threshold` sends stop and a High
/// alert is raised (PICK-055: the same configured threshold as the page pick's).
#[must_use]
pub const fn verify_all(threshold: i32) -> Limit {
    Limit::daily("portals.verify_daily_cap", threshold)
}

fn with_business(
    ip: IpAddr,
    ip_limit: Limit,
    business: Option<(Limit, Uuid)>,
) -> Vec<(Limit, Key)> {
    let mut checks = vec![(ip_limit, Key::Ip(ip))];
    checks.extend(business.map(|(limit, id)| (limit, Key::Record(id))));
    checks
}

/// SEC-014, SEC-020: a business sign-in link request from `ip` for `business` (when the email
/// is on file); T272's per-member link cap still applies after it (D-1837).
///
/// # Errors
///
/// [`LimitError`]: refuse the request.
pub async fn admit_sign_in_link(
    pool: &PgPool,
    (ip, business): (IpAddr, Option<Uuid>),
    now: Timestamp,
) -> Result<Admission, LimitError> {
    let business = business.map(|id| (LINKS_PER_BUSINESS, id));
    admit(pool, &with_business(ip, LINKS_PER_IP, business), now).await
}

/// SEC-020: one code try (a pick code, a sign-in code or a data-request code) from `ip`, and for
/// `business` on a business link; T272's per-link try cap still applies after it (D-1837).
///
/// # Errors
///
/// [`LimitError`]: refuse the try.
pub async fn admit_code_try(
    pool: &PgPool,
    (ip, business): (IpAddr, Option<Uuid>),
    now: Timestamp,
) -> Result<Admission, LimitError> {
    let business = business.map(|id| (CODE_TRIES_PER_BUSINESS, id));
    admit(pool, &with_business(ip, CODE_TRIES_PER_IP, business), now).await
}

/// The two counters an unknown number's code send spends in place of the per-number and global
/// counts, so both answers do the same database work (SEC-008, PICK-055, D-1954). Its ceiling is
/// never reached, so it never refuses or alerts.
pub const CODE_SENDS_UNKNOWN: Limit = Limit::daily("portals.code_sends_unknown", i32::MAX);

/// SEC-014, PICK-055, SEC-008: a homeowner-portal code send. Every request counts against `ip`
/// first, on file or not; a number on file (`contact`) then counts 5 a day and the global
/// `threshold`, whose first refusal raises the High alert. An unknown number spends two
/// [`CODE_SENDS_UNKNOWN`] counters instead, the same three upserts as a known one, and sends
/// nothing (identical work and timing, TD-PICK-36).
///
/// # Errors
///
/// [`LimitError`]: send nothing and answer as always.
pub async fn admit_code_send(
    pool: &PgPool,
    (ip, contact): (IpAddr, Option<Uuid>),
    (now, threshold): (Timestamp, i32),
) -> Result<Admission, LimitError> {
    let ip_check = (CODE_SENDS_PER_IP, Key::Ip(ip));
    let checks = match contact {
        Some(contact) => [
            ip_check,
            (CODE_SENDS_PER_NUMBER, Key::Record(contact)),
            (verify_all(threshold), Key::All),
        ],
        None => [
            ip_check,
            (CODE_SENDS_UNKNOWN, Key::Record(Uuid::nil())),
            (CODE_SENDS_UNKNOWN, Key::All),
        ],
    };
    admit(pool, &checks, now).await
}

/// SEC-020, SEC-040: one owner password try from `ip`, by IP then for the account. The passkey
/// path never calls this (never hard-locked, D-492).
///
/// # Errors
///
/// [`LimitError`]: refuse the try.
pub async fn admit_owner_password(
    pool: &PgPool,
    ip: IpAddr,
    now: Timestamp,
) -> Result<Admission, LimitError> {
    let checks = [
        (OWNER_TRIES_PER_IP, Key::Ip(ip)),
        (OWNER_TRIES_ALL, Key::All),
    ];
    admit(pool, &checks, now).await
}

/// SEC-014: one business sign-up from `ip`: 5 per IP and 50 for everyone per Arizona day.
///
/// # Errors
///
/// [`LimitError`]: refuse the sign-up.
pub async fn admit_sign_up(
    pool: &PgPool,
    ip: IpAddr,
    now: Timestamp,
) -> Result<Admission, LimitError> {
    let checks = [(SIGNUPS_PER_IP, Key::Ip(ip)), (SIGNUPS_ALL, Key::All)];
    admit(pool, &checks, now).await
}

/// SEC-014: one quote submission by `business`, from `ip` when it came by the web.
///
/// # Errors
///
/// [`LimitError`]: refuse the submission.
pub async fn admit_quote(
    pool: &PgPool,
    (business, ip): (Uuid, Option<IpAddr>),
    now: Timestamp,
) -> Result<Admission, LimitError> {
    let mut checks = vec![(QUOTES_PER_BUSINESS, Key::Record(business))];
    checks.extend(ip.map(|ip| (QUOTES_PER_IP, Key::Ip(ip))));
    admit(pool, &checks, now).await
}
