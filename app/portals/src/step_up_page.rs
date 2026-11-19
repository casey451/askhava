//! The business step-up code page and its code (SEC-038, SEC-004, SEC-020; T364, D-2636,
//! D-3744, D-4045).
//!
//! A guarded change (`business_portal::require_step_up`) passes only within 5 minutes of
//! `businesses::api::record_step_up`; this page is where a member earns it. [`send_step_up_code`]
//! sends one Verify code to the member's own phone or email (the composition's [`CodeAddress`]);
//! [`check_step_up_code`] checks it. `server::business_step_up` serves both behind the business
//! session, CSRF and the SEC-014 limits; [`business_step_up_page`] renders
//! `templates/business_step_up.html`.
//!
//! The step-up record is a `portals.login_link` row whose stored hash is the plain SHA256 of
//! `step_up:<member>:<floor(unix seconds / 300)>` (no random part), so reuse is decided on the
//! server per member and 5-minute bucket, never from the request: a member has at most one record
//! per bucket, it lives [`CODE_LIFE`] and its `created_at` is backdated by `LINK_LIFE`, as action
//! links are, so it never counts toward the sign-in link limits. A send reuses a live unused record
//! of this bucket or the two before it (its tries continue); a live record used up by
//! [`CODE_TRIES`] wrong tries (no `right` try row) refuses until it expires (Verify still holds
//! that code, D-2348); this bucket's record used by a right code means the step-up is still fresh;
//! otherwise it mints this bucket's record with `ON CONFLICT DO NOTHING` and reads again, so of two
//! racing sends the loser reuses the winner's record and its one send (`portals.code_send`).
//! A check reaches Verify only for a live unused record whose code was sent, reserves one of
//! [`CODE_TRIES`] tries first, uses the record up at the fifth wrong one, and uses it for a right
//! code in the same transaction as `record_step_up`, so a code confirms one step-up once.

use askama::Template;
use askhava_adapters::api::{CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_businesses::api::{Acting, record_step_up};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::business_auth::{
    BusinessPrincipal, CODE_TRIES, CodeAddress, CodeFuture, CodeTo, LINK_LIFE,
};
use crate::business_portal::BUSINESS_HOME_ROUTE;
use crate::models;
use crate::session::PortalsError;

/// `GET`: the step-up page (SEC-038).
pub const BUSINESS_STEP_UP_ROUTE: &str = "/b/step-up";
/// `POST`: send the member a step-up code (SEC-004, SEC-014).
pub const BUSINESS_STEP_UP_SEND_ROUTE: &str = "/b/step-up/send";
/// `POST`: check the member's step-up code (SEC-020, SEC-038).
pub const BUSINESS_STEP_UP_CHECK_ROUTE: &str = "/b/step-up/check";
/// A step-up record lives as long as the Verify code it carries.
pub const CODE_LIFE: SignedDuration = SignedDuration::from_mins(10);
/// A reservation a crash left `pending` is retaken after this (as business sign-in's).
const SEND_STALE: SignedDuration = SignedDuration::from_secs(75);
/// The reuse bucket, as long as a step-up lasts.
const BUCKET_SECONDS: i64 = 300;

/// Which state of the step-up page to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessStepUpScreen {
    /// The "Send me a code" button.
    Ask,
    /// A code went (or had gone): the code field.
    Sent,
    /// The code was wrong: the code field with its error summary.
    Wrong,
    /// A code was passed in the last few minutes; nothing was sent.
    Fresh,
    /// The code was right: the change can go ahead now.
    Done,
    /// Too many codes or tries: nothing was sent or checked.
    Refused,
    /// Codes can't be sent right now (no Verify, or it failed).
    Unavailable,
}

/// A same-site path to go back to after the step-up (R1: parsed once at the edge).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnPath(String);

impl ReturnPath {
    /// SEC-038: `raw` when it is a same-site path (`/` then letters, digits, `/`, `-`, `_`, `.`;
    /// never `//`), else the portal home.
    #[must_use]
    pub fn parse(raw: Option<&str>) -> Self {
        let safe = |p: &&str| {
            p.len() <= 200
                && p.starts_with('/')
                && !p.starts_with("//")
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
        };
        Self(raw.filter(safe).unwrap_or(BUSINESS_HOME_ROUTE).to_owned())
    }

    /// The path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Template)]
#[template(path = "business_step_up.html")]
struct StepUpView<'a> {
    csrf: &'a str,
    back: &'a str,
    screen: BusinessStepUpScreen,
    code_form: bool,
    send_route: &'a str,
    check_route: &'a str,
}

/// The step-up page for `screen`, its forms carrying `csrf` and `back` (SEC-038, SEC-042).
///
/// # Errors
///
/// The template's render error.
pub fn business_step_up_page(
    csrf: &str,
    screen: BusinessStepUpScreen,
    back: &ReturnPath,
) -> Result<String, askama::Error> {
    StepUpView {
        csrf,
        back: back.as_str(),
        screen,
        code_form: matches!(
            screen,
            BusinessStepUpScreen::Sent | BusinessStepUpScreen::Wrong
        ),
        send_route: BUSINESS_STEP_UP_SEND_ROUTE,
        check_route: BUSINESS_STEP_UP_CHECK_ROUTE,
    }
    .render()
}

/// Verify behind `dyn`: send (`false` when it failed) and check (`None` when it failed).
trait StepUpVerify: Send + Sync {
    fn send<'a>(&'a self, key: &'a IdempotencyKey, to: &'a CodeTo) -> CodeFuture<'a, bool>;
    fn check<'a>(&'a self, to: &'a CodeTo, code: &'a str) -> CodeFuture<'a, Option<bool>>;
}

fn address(to: &CodeTo) -> &str {
    match to {
        CodeTo::Phone(a) | CodeTo::Email(a) => a.expose_secret(),
    }
}

impl<V: VerifyAdapter + Send + Sync + 'static> StepUpVerify for V {
    fn send<'a>(&'a self, key: &'a IdempotencyKey, to: &'a CodeTo) -> CodeFuture<'a, bool> {
        let channel: CodeChannel = to.channel();
        Box::pin(async move { self.send_code(key, address(to), channel).await.is_ok() })
    }

    fn check<'a>(&'a self, to: &'a CodeTo, code: &'a str) -> CodeFuture<'a, Option<bool>> {
        Box::pin(async move { self.check_code(address(to), code).await.ok() })
    }
}

/// The step-up code's parts: Verify, where a member's code goes, and the idempotency key secret
/// (SEC-004, SEC-018).
pub struct StepUpCodes {
    verify: Box<dyn StepUpVerify>,
    to: Box<dyn CodeAddress>,
    key: SecretString,
}

impl StepUpCodes {
    /// Codes through `verify` to the address `to` finds, keyed with `key` (`ASKHAVA_ID_SECRET`).
    #[must_use]
    pub fn new<V, A>(verify: V, to: A, key: SecretString) -> Self
    where
        V: VerifyAdapter + Send + Sync + 'static,
        A: CodeAddress,
    {
        Self {
            verify: Box::new(verify),
            to: Box::new(to),
            key,
        }
    }
}

/// One live step-up record of the member.
struct Record {
    id: Uuid,
    bucket: i64,
    used: bool,
    right: bool,
}

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, PortalsError> {
    result.map_err(|_| PortalsError::Database)
}

fn bucket(now: Timestamp) -> i64 {
    now.as_second().div_euclid(BUCKET_SECONDS)
}

fn token(member: Uuid, bucket: i64) -> String {
    format!("step_up:{member}:{bucket}")
}

fn digest(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// D-3744 (a): `member`'s live records of this bucket and the two before it, newest first.
async fn records(pool: &PgPool, member: Uuid, now: Timestamp) -> Result<Vec<Record>, PortalsError> {
    let current = bucket(now);
    let buckets = [current, current - 1, current - 2];
    let hashes: Vec<Vec<u8>> = buckets.iter().map(|b| digest(&token(member, *b))).collect();
    let rows = db(sqlx::query!(
        r#"SELECT l.id, l.token_sha256, l.used_at IS NOT NULL AS "used!",
           EXISTS (SELECT 1 FROM portals.code_try t
                   WHERE t.link_id = l.id AND t.outcome = 'right') AS "right!"
           FROM portals.login_link l
           WHERE l.token_sha256 = ANY($1) AND l.member_id = $2 AND l.expires_at > $3"#,
        &hashes,
        member,
        now.to_sqlx(),
    )
    .fetch_all(pool)
    .await)?;
    let mut found: Vec<Record> = rows
        .into_iter()
        .filter_map(|r| {
            let at = hashes.iter().position(|h| *h == r.token_sha256)?;
            Some(Record {
                id: r.id,
                bucket: buckets[at],
                used: r.used,
                right: r.right,
            })
        })
        .collect();
    found.sort_by_key(|r| -r.bucket);
    Ok(found)
}

/// What a send finds.
enum Found {
    Fresh,
    Reuse(Uuid),
    Locked,
    Nothing,
}

fn decide(found: &[Record], now: Timestamp) -> Found {
    if found.iter().any(|r| r.bucket == bucket(now) && r.right) {
        return Found::Fresh;
    }
    if let Some(r) = found.iter().find(|r| !r.used) {
        return Found::Reuse(r.id);
    }
    if found.iter().any(|r| r.used && !r.right) {
        return Found::Locked;
    }
    Found::Nothing
}

/// D-3744 (a): mints this bucket's record for `who`, a racing mint changing nothing.
async fn mint(pool: &PgPool, who: &BusinessPrincipal, now: Timestamp) -> Result<(), PortalsError> {
    let made = now
        .checked_sub(LINK_LIFE)
        .map_err(|_| PortalsError::BadLifetime)?;
    let ends = now
        .checked_add(CODE_LIFE)
        .map_err(|_| PortalsError::BadLifetime)?;
    db(sqlx::query!(
        "INSERT INTO portals.login_link \
         (token_sha256, member_id, business_id, created_at, expires_at) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (token_sha256) DO NOTHING",
        digest(&token(who.member, bucket(now))),
        who.member,
        who.business,
        made.to_sqlx(),
        ends.to_sqlx(),
    )
    .execute(pool)
    .await)
    .map(|_| ())
}

/// SEC-004, SEC-038, D-3744 (a): sends `who` one step-up code, reusing a live record (and its one
/// send and its tries) or minting this bucket's. The caller has passed the SEC-014 limits. Without
/// `codes` nothing is minted or sent.
///
/// # Errors
///
/// [`PortalsError`] from a database call or the time.
pub async fn send_step_up_code(
    pool: &PgPool,
    codes: Option<&StepUpCodes>,
    who: &BusinessPrincipal,
    now: Timestamp,
) -> Result<BusinessStepUpScreen, PortalsError> {
    let Some(codes) = codes else {
        return Ok(BusinessStepUpScreen::Unavailable);
    };
    let mut found = decide(&records(pool, who.member, now).await?, now);
    if matches!(found, Found::Nothing) {
        mint(pool, who, now).await?;
        found = decide(&records(pool, who.member, now).await?, now);
    }
    match found {
        Found::Fresh => Ok(BusinessStepUpScreen::Fresh),
        Found::Locked => Ok(BusinessStepUpScreen::Refused),
        Found::Nothing => Err(PortalsError::Database),
        Found::Reuse(link) => send_once(pool, codes, (link, who.member), now).await,
    }
}

/// SEC-004, SEC-018: `link`'s one Verify send, reserved first, so a reused or racing send whose
/// code went sends nothing again; no connection is held across Verify.
async fn send_once(
    pool: &PgPool,
    codes: &StepUpCodes,
    (link, member): (Uuid, Uuid),
    now: Timestamp,
) -> Result<BusinessStepUpScreen, PortalsError> {
    let stale = now
        .checked_sub(SEND_STALE)
        .map_err(|_| PortalsError::BadLifetime)?;
    if !models::reserve_code_send(pool, link, (now, stale)).await? {
        return Ok(BusinessStepUpScreen::Sent);
    }
    let to = {
        let mut conn = db(pool.acquire().await)?;
        codes.to.code_to(&mut conn, member).await
    };
    let sent = match &to {
        Ok(to) => {
            let key =
                IdempotencyKey::derive(&codes.key, "verify.step_up_code", &[&link.to_string()]);
            codes.verify.send(&key, to).await
        }
        Err(_) => false,
    };
    models::finish_code_send(pool, (link, now), sent).await?;
    Ok(if sent {
        BusinessStepUpScreen::Sent
    } else {
        BusinessStepUpScreen::Unavailable
    })
}

/// SEC-020, SEC-038, D-3744 (b) to (d): checks `code` for `who`'s live unused record whose code
/// was sent (else no Verify call), after reserving one of [`CODE_TRIES`] tries; the fifth wrong try
/// uses the record up; a right code uses it in the same transaction as `record_step_up`. The
/// caller has passed the SEC-014 try limit.
///
/// # Errors
///
/// [`PortalsError`] from a database call.
pub async fn check_step_up_code(
    pool: &PgPool,
    codes: Option<&StepUpCodes>,
    who: &BusinessPrincipal,
    code: &str,
    now: Timestamp,
) -> Result<BusinessStepUpScreen, PortalsError> {
    let found = records(pool, who.member, now).await?;
    let Some(record) = found.iter().find(|r| !r.used) else {
        let locked = found.iter().any(|r| r.used && !r.right);
        return Ok(if locked {
            BusinessStepUpScreen::Refused
        } else {
            BusinessStepUpScreen::Ask
        });
    };
    let Some(codes) = codes else {
        return Ok(BusinessStepUpScreen::Unavailable);
    };
    if !models::code_sent(pool, record.id).await? {
        return Ok(BusinessStepUpScreen::Ask);
    }
    let Some(try_no) = models::reserve_try(pool, record.id, now, CODE_TRIES).await? else {
        models::use_up_link(pool, record.id, now).await?;
        return Ok(BusinessStepUpScreen::Refused);
    };
    let checked = verify_try(pool, codes, who.member, code.trim()).await?;
    let outcome = match checked {
        Some(true) => "right",
        Some(false) => "wrong",
        None => "abandoned",
    };
    models::finish_try(pool, (record.id, try_no), outcome).await?;
    if checked == Some(true) {
        return confirm(pool, who, record.bucket, now).await;
    }
    if i64::from(try_no) >= CODE_TRIES {
        models::use_up_link(pool, record.id, now).await?;
        return Ok(BusinessStepUpScreen::Refused);
    }
    Ok(BusinessStepUpScreen::Wrong)
}

/// Verify's answer for a 6-digit `code` to `member`'s address; any other shape is wrong unasked.
async fn verify_try(
    pool: &PgPool,
    codes: &StepUpCodes,
    member: Uuid,
    code: &str,
) -> Result<Option<bool>, PortalsError> {
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(Some(false));
    }
    let to = {
        let mut conn = db(pool.acquire().await)?;
        codes.to.code_to(&mut conn, member).await
    };
    Ok(match to {
        Ok(to) => codes.verify.check(&to, code).await,
        Err(_) => None,
    })
}

/// D-3744 (d): uses the record and records the step-up in one transaction, once.
async fn confirm(
    pool: &PgPool,
    who: &BusinessPrincipal,
    record_bucket: i64,
    now: Timestamp,
) -> Result<BusinessStepUpScreen, PortalsError> {
    let mut tx = db(pool.begin().await)?;
    let used = models::use_link(&mut tx, &token(who.member, record_bucket), now).await?;
    if used != Some((who.member, who.business)) {
        return Ok(BusinessStepUpScreen::Refused);
    }
    let by = Acting {
        business: who.business,
        member: who.member,
    };
    record_step_up(&mut tx, by, now)
        .await
        .map_err(|_| PortalsError::Database)?;
    db(tx.commit().await)?;
    Ok(BusinessStepUpScreen::Done)
}

#[cfg(test)]
mod tests {
    //! The page's screens and the return path's parse (SEC-038).

    use super::*;

    #[test]
    fn only_a_same_site_path_is_kept() {
        for bad in [
            "//evil.example",
            "https://evil.example",
            "/a b",
            "/x?y=1",
            "b/x",
        ] {
            assert_eq!(
                ReturnPath::parse(Some(bad)).as_str(),
                BUSINESS_HOME_ROUTE,
                "{bad}"
            );
        }
        assert_eq!(ReturnPath::parse(Some("/b/team")).as_str(), "/b/team");
        assert_eq!(ReturnPath::parse(None).as_str(), BUSINESS_HOME_ROUTE);
    }

    #[test]
    fn each_screen_renders_with_the_token_and_the_way_back() {
        use BusinessStepUpScreen as S;
        let back = ReturnPath::parse(Some("/b/team"));
        for screen in [
            S::Ask,
            S::Sent,
            S::Wrong,
            S::Fresh,
            S::Done,
            S::Refused,
            S::Unavailable,
        ] {
            let page = business_step_up_page("tok123", screen, &back).expect("page");
            assert!(page.contains("/b/team"), "{screen:?}");
            assert!(page.contains("data-get-help"), "{screen:?}");
        }
        let wrong = business_step_up_page("t", S::Wrong, &back).expect("page");
        assert!(wrong.contains("error-summary") && wrong.contains("aria-invalid"));
        let sent = business_step_up_page("t", S::Sent, &back).expect("page");
        assert!(sent.contains("one-time-code") && !sent.contains("error-summary"));
    }
}
