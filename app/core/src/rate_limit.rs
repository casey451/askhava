//! Rate-limit counters and the client IP they are keyed on (SEC-014, SEC-015, SEC-040, D-480, D-492).
//!
//! [`hit`] raises one `core.rate_limit` counter per bucket and fixed window with a single atomic
//! `INSERT … ON CONFLICT … DO UPDATE … RETURNING`, so concurrent requests on any number of web
//! processes count exactly (SEC-040); [`hit_on`] runs the same upsert on the caller's connection,
//! so the count commits or rolls back with the caller's transaction (D-2206). A bucket is a scope
//! plus an IP ([`ip_bucket`]) or a blind-index hash a module computes; never a plaintext phone or
//! email (postgres brain P12).
//!
//! [`ClientIpPolicy::client_ip`] takes the IP from the one header the host's edge sets and
//! overwrites, named by `ASKHAVA_CLIENT_IP_HEADER` (default `x-real-ip`, to be measured on the test
//! project, OPS-050), never from a client-supplied `X-Forwarded-For`. `CF-Connecting-IP` counts
//! only on a hostname listed in `ASKHAVA_PROXIED_HOSTS` (empty at launch: every hostname is
//! DNS-only, SEC-053) and only when the edge saw the request come from Cloudflare's published
//! ranges ([`CLOUDFLARE_RANGES`]).
//!
//! A bucket name can hold a client address, so rows are not kept: the daily [`PRUNE_KIND`] task
//! ([`run_prune_at`]) deletes windows that started more than [`LONGEST_WINDOW`] plus a day ago,
//! [`PRUNE_BATCH`] rows per statement ([`prune_batch`]), and queues the next day's run first
//! (SEC-014, SEC-040, D-2306). No index serves it: a table pruned daily holds a few days of rows.

use std::net::IpAddr;

use axum::http::{HeaderMap, HeaderName, header};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::{PgConnection, PgPool};
use thiserror::Error;

use crate::tasks::{Claimed, HandlerError, NewTask, TaskError, enqueue};

/// The daily task that deletes old rate-limit windows (SEC-014, SEC-040, D-2306).
pub const PRUNE_KIND: &str = "core.rate_limit_prune";
/// The most rows one prune statement deletes (D-2306).
pub const PRUNE_BATCH: i64 = 5_000;
/// The longest window any caller counts in today: `intake`'s Arizona day (`intake::limits`). A
/// caller with a longer window raises this first, or the daily prune deletes live counts (D-2306).
pub const LONGEST_WINDOW: SignedDuration = SignedDuration::from_hours(24);
const DAY: SignedDuration = SignedDuration::from_hours(24);

const CLIENT_IP_HEADER: &str = "ASKHAVA_CLIENT_IP_HEADER";
const PROXIED_HOSTS: &str = "ASKHAVA_PROXIED_HOSTS";
/// The edge header read when `ASKHAVA_CLIENT_IP_HEADER` is unset (Railway's edge, OPS-050).
pub const DEFAULT_CLIENT_IP_HEADER: &str = "x-real-ip";
/// Cloudflare's header with the visitor's IP, honoured only as [`ClientIpPolicy`] allows.
pub const CF_CONNECTING_IP: &str = "cf-connecting-ip";
/// Cloudflare's published ranges (<https://www.cloudflare.com/ips/>); re-check them before any
/// hostname is proxied (SEC-040, SEC-053).
pub const CLOUDFLARE_RANGES: &[&str] = &[
    "173.245.48.0/20",
    "103.21.244.0/22",
    "103.22.200.0/22",
    "103.31.4.0/22",
    "141.101.64.0/18",
    "108.162.192.0/18",
    "190.93.240.0/20",
    "188.114.96.0/20",
    "197.234.240.0/22",
    "198.41.128.0/17",
    "162.158.0.0/15",
    "104.16.0.0/13",
    "104.24.0.0/14",
    "172.64.0.0/13",
    "131.0.72.0/22",
    "2400:cb00::/32",
    "2606:4700::/32",
    "2803:f800::/32",
    "2405:b500::/32",
    "2405:8100::/32",
    "2a06:98c0::/29",
    "2c0f:f248::/32",
];

/// Why a rate-limit call or its settings failed; no variant echoes a value (SEC-001).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RateLimitError {
    /// A setting does not parse.
    #[error("setting {0} has an invalid value")]
    Invalid(&'static str),
    /// The database refused or dropped the statement.
    #[error("the rate-limit counter could not be reached")]
    Db,
}

/// An IP network such as `198.51.100.0/24`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cidr {
    net: IpAddr,
    prefix: u32,
}

impl Cidr {
    fn parse(text: &str) -> Option<Self> {
        let (net, prefix) = text.trim().split_once('/')?;
        let (net, prefix): (IpAddr, u32) = (net.parse().ok()?, prefix.parse().ok()?);
        let max = if net.is_ipv4() { 32 } else { 128 };
        (prefix <= max).then_some(Self { net, prefix })
    }

    fn contains(self, ip: IpAddr) -> bool {
        match (self.net, ip) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX.checked_shl(32 - self.prefix).unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX.checked_shl(128 - self.prefix).unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            (IpAddr::V4(_), IpAddr::V6(_)) | (IpAddr::V6(_), IpAddr::V4(_)) => false,
        }
    }
}

/// Where the client IP for rate limits comes from (SEC-040).
#[derive(Debug, Clone)]
pub struct ClientIpPolicy {
    edge_header: HeaderName,
    proxied_hosts: Vec<String>,
    cloudflare: Vec<Cidr>,
}

impl ClientIpPolicy {
    /// Reads `ASKHAVA_CLIENT_IP_HEADER` and `ASKHAVA_PROXIED_HOSTS` (comma-separated hostnames)
    /// from the same pairs `Settings::from_pairs` takes (SEC-040).
    ///
    /// # Errors
    ///
    /// [`RateLimitError::Invalid`] when the header name is not a valid header name.
    pub fn from_pairs<I>(pairs: I) -> Result<Self, RateLimitError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let (mut header, mut hosts) = (DEFAULT_CLIENT_IP_HEADER.to_owned(), String::new());
        for (key, value) in pairs {
            match key.as_str() {
                CLIENT_IP_HEADER => header = value,
                PROXIED_HOSTS => hosts = value,
                _ => {}
            }
        }
        let edge_header = HeaderName::try_from(header.trim().to_ascii_lowercase())
            .map_err(|_| RateLimitError::Invalid(CLIENT_IP_HEADER))?;
        let proxied_hosts = hosts
            .split(',')
            .map(|h| h.trim().to_ascii_lowercase())
            .filter(|h| !h.is_empty())
            .collect();
        let cloudflare = CLOUDFLARE_RANGES
            .iter()
            .filter_map(|r| Cidr::parse(r))
            .collect();
        Ok(Self {
            edge_header,
            proxied_hosts,
            cloudflare,
        })
    }

    /// The client IP: the edge header's address (the socket peer when the edge sent none), or
    /// `CF-Connecting-IP` on a proxied hostname when that address is Cloudflare's (SEC-040).
    /// `X-Forwarded-For` is never read.
    #[must_use]
    pub fn client_ip(&self, headers: &HeaderMap, peer: IpAddr) -> IpAddr {
        let edge = header_ip(headers, &self.edge_header).unwrap_or(peer);
        let host = headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .map(host_name);
        let proxied = host.is_some_and(|h| self.proxied_hosts.contains(&h));
        if proxied && self.cloudflare.iter().any(|range| range.contains(edge)) {
            let cf = HeaderName::from_static(CF_CONNECTING_IP);
            return header_ip(headers, &cf).unwrap_or(edge);
        }
        edge
    }
}

fn header_ip(headers: &HeaderMap, name: &HeaderName) -> Option<IpAddr> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

/// The `Host` header's hostname, lower-cased, without a port.
fn host_name(host: &str) -> String {
    let name = host.rsplit_once(':').map_or(host, |(name, port)| {
        if port.bytes().all(|b| b.is_ascii_digit()) {
            name
        } else {
            host
        }
    });
    name.to_ascii_lowercase()
}

/// The bucket for one scope's per-IP limit, for example `intake:ip:198.51.100.7` (SEC-014).
#[must_use]
pub fn ip_bucket(scope: &str, ip: IpAddr) -> String {
    format!("{scope}:ip:{ip}")
}

/// Whether a request may go ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateDecision {
    /// Within the limit; `hits` counts this request.
    Allowed {
        /// Requests in this window so far, this one included.
        hits: i32,
    },
    /// Over the limit for this window.
    Limited,
}

/// Counts one request in `bucket`'s fixed window of `window` containing `now`, atomically, and
/// allows it while the count is at most `limit` (SEC-014, SEC-040).
///
/// # Errors
///
/// [`RateLimitError::Db`] when the upsert fails.
pub async fn hit(
    pool: &PgPool,
    bucket: &str,
    now: Timestamp,
    window: SignedDuration,
    limit: i32,
) -> Result<RateDecision, RateLimitError> {
    let mut conn = pool.acquire().await.map_err(|_| RateLimitError::Db)?;
    hit_on(&mut conn, bucket, now, window, limit).await
}

/// [`hit`] on the caller's connection, so the count joins the caller's transaction: a rolled-back
/// transaction counts nothing, and the counter row stays locked until it ends (SEC-014, SEC-040,
/// D-2206, D-3083). Hold no network call between this and the commit.
///
/// # Errors
///
/// [`RateLimitError::Db`] when the upsert fails.
pub async fn hit_on(
    conn: &mut PgConnection,
    bucket: &str,
    now: Timestamp,
    window: SignedDuration,
    limit: i32,
) -> Result<RateDecision, RateLimitError> {
    let hits = sqlx::query_scalar!(
        "INSERT INTO core.rate_limit (bucket, window_start, hits) VALUES ($1, $2, 1) \
         ON CONFLICT (bucket, window_start) DO UPDATE SET hits = core.rate_limit.hits + 1 \
         RETURNING hits",
        bucket,
        window_start(now, window).to_sqlx() as _,
    )
    .fetch_one(conn)
    .await
    .map_err(|_| RateLimitError::Db)?;
    Ok(if hits <= limit {
        RateDecision::Allowed { hits }
    } else {
        RateDecision::Limited
    })
}

/// Deletes counters whose window started before `before`; returns how many.
///
/// # Errors
///
/// [`RateLimitError::Db`] when the delete fails.
pub async fn prune(pool: &PgPool, before: Timestamp) -> Result<u64, RateLimitError> {
    let done = sqlx::query!(
        "DELETE FROM core.rate_limit WHERE window_start < $1",
        before.to_sqlx() as _
    )
    .execute(pool)
    .await
    .map_err(|_| RateLimitError::Db)?;
    Ok(done.rows_affected())
}

/// Deletes at most `most` counters whose window started before `before`, in one short statement
/// on its own (no transaction held across batches), and returns how many (SEC-014, SEC-040,
/// D-2306).
///
/// # Errors
///
/// [`RateLimitError::Db`] when the delete fails.
pub async fn prune_batch(
    pool: &PgPool,
    before: Timestamp,
    most: i64,
) -> Result<u64, RateLimitError> {
    let done = sqlx::query!(
        "DELETE FROM core.rate_limit WHERE (bucket, window_start) IN \
         (SELECT bucket, window_start FROM core.rate_limit WHERE window_start < $1 LIMIT $2)",
        before.to_sqlx() as _,
        most,
    )
    .execute(pool)
    .await
    .map_err(|_| RateLimitError::Db)?;
    Ok(done.rows_affected())
}

/// The cut-off of the daily prune at `now`: windows that started before it are deleted. It is
/// [`LONGEST_WINDOW`] plus one day back, so no window still counting is ever touched (an Arizona
/// day's row starts at a UTC midnight and counts until 31 hours later; SEC-014, D-2306).
#[must_use]
pub fn prune_cutoff(now: Timestamp) -> Timestamp {
    now - LONGEST_WINDOW - DAY
}

/// Queues [`PRUNE_KIND`] for the day after `now`, once per day (dedupe key by day number,
/// SEC-014, SEC-040, D-2306).
///
/// # Errors
///
/// [`TaskError::Db`].
pub async fn schedule_prune(db: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    let run_at = now + DAY;
    let day = run_at.as_second() / 86_400;
    let mut task = NewTask::new(PRUNE_KIND, serde_json::json!({}), run_at);
    task.dedupe_key = Some(format!("{PRUNE_KIND}:{day}"));
    enqueue(db, &task).await?;
    Ok(())
}

/// SEC-014, SEC-040, D-2306: the [`PRUNE_KIND`] handler at `now` (the worker's clock): queues the
/// next day's run first, so a run that fails for good still leaves it, then deletes every window
/// older than [`prune_cutoff`] in batches of [`PRUNE_BATCH`] until a batch comes back short. Safe
/// to run twice: a repeat finds nothing left to delete.
///
/// # Errors
///
/// A fixed [`HandlerError`] when a database call fails.
pub async fn run_prune_at(
    pool: PgPool,
    _task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let failed = HandlerError("rate-limit prune database call failed");
    {
        let mut conn = pool.acquire().await.map_err(|_| failed)?;
        schedule_prune(&mut conn, now).await.map_err(|_| failed)?;
    }
    let before = prune_cutoff(now);
    let full = u64::try_from(PRUNE_BATCH).unwrap_or(u64::MAX);
    loop {
        let deleted = prune_batch(&pool, before, PRUNE_BATCH)
            .await
            .map_err(|_| failed)?;
        if deleted < full {
            return Ok(());
        }
    }
}

/// The start of the fixed window of `window` (at least one second) that holds `now`.
fn window_start(now: Timestamp, window: SignedDuration) -> Timestamp {
    let (secs, size) = (now.as_second(), window.as_secs().max(1));
    Timestamp::from_second(secs - secs.rem_euclid(size)).unwrap_or(now)
}
