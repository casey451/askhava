//! Owner sign-in (T086; TD-SEC-01 owner cases, TD-SEC-06, TD-SEC-22, TD-SEC-23; SEC-005 to
//! SEC-007, SEC-019, SEC-043, SEC-044): argon2id password plus a TOTP, passkey or recovery code,
//! 12-hour sessions ending after 30 minutes idle, the 5-minute step-up, logout and "Sign out
//! everywhere", the one-time bootstrap, and pause, refund and release for the owner alone.
//! Every password, token, code and ID is fake.

use askhava_portals::api::{
    CookieJar, OwnerAction, OwnerAuth, OwnerAuthError, OwnerPower, PasswordProblem, Realm,
    SecondFactor, add_owner_totp, bootstrap_owner, change_owner_password,
    finish_passkey_registration, owner_power, owner_principal, owner_sign_in, owner_sign_out,
    owner_signin_page, owner_step_up, passkey_registration_options, password_problem,
    require_owner_step_up, sign_out_everywhere, start_session,
};
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::ExposeSecret;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const PASSWORD: &str = "fake owner pass phrase 2026";
const TOKEN: &str = "fake-bootstrap-token-0123456789abcdef";
const ROUTE: &str = "/owner/settings";

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_010).expect("time") + SignedDuration::from_secs(secs)
}

fn auth() -> OwnerAuth {
    OwnerAuth::new("askhava.test", "https://askhava.test").expect("relying party")
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `now` (30-second steps).
fn totp_code(secret: &[u8], now: Timestamp) -> String {
    let step = u64::try_from(now.as_second() / 30).expect("step");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[offset..offset + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

async fn secrets(db: &mut PgConnection) -> Vec<Vec<u8>> {
    sqlx::query_scalar("SELECT totp_secret FROM portals.owner_factor ORDER BY created_at, id")
        .fetch_all(db)
        .await
        .expect("secrets")
}

/// The bootstrapped owner: the first authenticator's secret and the recovery codes.
async fn bootstrapped(db: &mut PgConnection) -> (Vec<u8>, Vec<String>) {
    let enrolment = bootstrap_owner(db, Some(TOKEN), PASSWORD, at(0)).await;
    let codes = enrolment.expect("bootstrap").recovery_codes;
    let codes = codes.iter().map(|c| c.expose_secret().to_owned()).collect();
    (secrets(db).await.remove(0), codes)
}

async fn sign_in(db: &mut PgConnection, secret: &[u8], t: i64) -> CookieJar {
    let code = totp_code(secret, at(t));
    let factor = SecondFactor::Totp(&code);
    let jar = owner_sign_in(db, CookieJar::new(), (PASSWORD, factor), &auth(), at(t)).await;
    jar.expect("signed in")
}

async fn events(db: &mut PgConnection, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM portals.security_event WHERE kind = $1")
        .bind(kind)
        .fetch_one(db)
        .await
        .expect("events")
}

/// TD-SEC-23 (SEC-044): no token or a short one creates nothing; the bootstrap creates the owner
/// once with 10 recovery codes and a second run fails; the app role cannot insert an owner, so
/// no web route can create one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_23_the_bootstrap_creates_the_owner_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let none = bootstrap_owner(&mut db, None, PASSWORD, at(0)).await;
    assert_eq!(none.err(), Some(OwnerAuthError::NoBootstrapToken));
    let short = bootstrap_owner(&mut db, Some("fake-short-token"), PASSWORD, at(0)).await;
    assert_eq!(short.err(), Some(OwnerAuthError::NoBootstrapToken));
    let (_, codes) = bootstrapped(&mut db).await;
    assert_eq!(codes.len(), 10);
    let again = bootstrap_owner(&mut db, Some(TOKEN), PASSWORD, at(5)).await;
    assert_eq!(again.err(), Some(OwnerAuthError::AlreadyBootstrapped));
    let owners: i64 = sqlx::query_scalar("SELECT count(*) FROM portals.owner")
        .fetch_one(&mut *db)
        .await
        .expect("count");
    assert_eq!(owners, 1);
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let insert = sqlx::query(
        "INSERT INTO portals.owner (singleton, password_hash, created_at) \
         VALUES (true, '$argon2id$fake', now())",
    )
    .execute(&mut *db)
    .await;
    assert!(insert.is_err(), "the app role cannot create an owner");
}

/// TD-SEC-22 (SEC-043): a 14-character password and a breached 20-character one are refused at
/// the bootstrap and at a password change; the stored hash is argon2id (SEC-005).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_22_short_and_breached_passwords_are_refused(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let short = "fakepass123456";
    let breached = "12345678901234567890";
    assert_eq!(password_problem(short), Some(PasswordProblem::TooShort));
    assert_eq!(password_problem(breached), Some(PasswordProblem::Breached));
    assert_eq!(
        password_problem("PasswordPassword"),
        Some(PasswordProblem::Breached)
    );
    assert_eq!(password_problem(PASSWORD), None);
    let weak = bootstrap_owner(&mut db, Some(TOKEN), breached, at(0)).await;
    assert_eq!(
        weak.err(),
        Some(OwnerAuthError::WeakPassword(PasswordProblem::Breached))
    );
    let (secret, _) = bootstrapped(&mut db).await;
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM portals.owner")
        .fetch_one(&mut *db)
        .await
        .expect("hash");
    assert!(hash.starts_with("$argon2id$"));
    let jar = sign_in(&mut db, &secret, 0).await;
    let owner = owner_principal(&mut db, &jar, at(10)).await.expect("read");
    let owner = owner.expect("owner");
    for (bad, problem) in [
        (short, PasswordProblem::TooShort),
        (breached, PasswordProblem::Breached),
    ] {
        let changed = change_owner_password(&mut db, &owner, (bad, ROUTE), at(20)).await;
        assert_eq!(changed, Err(OwnerAuthError::WeakPassword(problem)));
    }
    let good = change_owner_password(&mut db, &owner, ("another fake pass phrase", ROUTE), at(20));
    assert_eq!(good.await, Ok(()));
}

/// TD-SEC-01 owner sign-in rules (SEC-005): the password alone, a wrong code, a wrong password
/// with a good code and a replayed code are refused and logged; the session is at most 12 hours
/// however busy.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_01_owner_sign_in_needs_both_factors_and_lasts_twelve_hours(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (secret, _) = bootstrapped(&mut db).await;
    let code = totp_code(&secret, at(0));
    let tries = [
        ("fake wrong pass phrase", SecondFactor::Totp(&code)),
        (PASSWORD, SecondFactor::Totp("")),
        (PASSWORD, SecondFactor::Totp("000000x")),
    ];
    for attempt in tries {
        let refused = owner_sign_in(&mut db, CookieJar::new(), attempt, &auth(), at(0)).await;
        assert_eq!(refused.err(), Some(OwnerAuthError::Refused));
    }
    assert_eq!(events(&mut db, "auth_failed").await, 3);
    let jar = sign_in(&mut db, &secret, 0).await;
    let replay = (PASSWORD, SecondFactor::Totp(&code));
    let again = owner_sign_in(&mut db, CookieJar::new(), replay, &auth(), at(1)).await;
    assert_eq!(
        again.err(),
        Some(OwnerAuthError::Refused),
        "a code works once"
    );
    for t in (1200..43_200).step_by(1200) {
        let live = owner_principal(&mut db, &jar, at(t)).await.expect("read");
        assert!(live.is_some(), "live at {t} seconds");
    }
    let ended = owner_principal(&mut db, &jar, at(43_200))
        .await
        .expect("read");
    assert!(ended.is_none(), "gone at 12 hours");
}

/// TD-SEC-22 (SEC-043): idle 29:59 is live; idle 30:01 is gone and stays gone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_22_thirty_minutes_idle_ends_the_session(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (secret, _) = bootstrapped(&mut db).await;
    let jar = sign_in(&mut db, &secret, 0).await;
    let live = owner_principal(&mut db, &jar, at(1799))
        .await
        .expect("read");
    assert!(live.is_some(), "29:59 idle is live");
    let gone = owner_principal(&mut db, &jar, at(1799 + 1801))
        .await
        .expect("read");
    assert!(gone.is_none(), "30:01 idle is gone");
    let still = owner_principal(&mut db, &jar, at(1799 + 1802))
        .await
        .expect("read");
    assert!(still.is_none());
}

/// TD-SEC-22 (SEC-043): each listed action with no second factor in the last 5 minutes is refused
/// and logged; a fresh TOTP step-up passes until 5:00 and is refused at 5:01.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_22_each_listed_action_needs_a_factor_from_the_last_five_minutes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (secret, _) = bootstrapped(&mut db).await;
    let jar = sign_in(&mut db, &secret, 0).await;
    let mut owner = owner_principal(&mut db, &jar, at(301))
        .await
        .expect("read")
        .expect("owner");
    for action in OwnerAction::ALL {
        let stale = require_owner_step_up(&mut db, &owner, (action, ROUTE), at(301)).await;
        assert_eq!(stale, Err(OwnerAuthError::NeedsFreshFactor), "{action:?}");
    }
    assert_eq!(events(&mut db, "authz_denied").await, 7);
    let code = totp_code(&secret, at(600));
    let step = (&auth(), SecondFactor::Totp(&code), ROUTE);
    owner_step_up(&mut db, &mut owner, step, at(600))
        .await
        .expect("step-up");
    for action in OwnerAction::ALL {
        let edge = require_owner_step_up(&mut db, &owner, (action, ROUTE), at(900)).await;
        assert_eq!(edge, Ok(()), "{action:?} at 5:00");
        let late = require_owner_step_up(&mut db, &owner, (action, ROUTE), at(901)).await;
        assert_eq!(
            late,
            Err(OwnerAuthError::NeedsFreshFactor),
            "{action:?} at 5:01"
        );
    }
    let bad = (&auth(), SecondFactor::Totp("123456x"), ROUTE);
    let refused = owner_step_up(&mut db, &mut owner, bad, at(910)).await;
    assert_eq!(refused, Err(OwnerAuthError::Refused));
}

/// TD-SEC-22 (SEC-043): after logout the old cookie is refused (a 401 at the route); "Sign out
/// everywhere" ends a second device's session.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_22_logout_and_sign_out_everywhere_end_sessions(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (secret, _) = bootstrapped(&mut db).await;
    let first = sign_in(&mut db, &secret, 0).await;
    let old = first.clone();
    let cleared = owner_sign_out(&mut db, first).await.expect("logout");
    assert!(
        owner_principal(&mut db, &old, at(10))
            .await
            .expect("read")
            .is_none()
    );
    let cookie = cleared
        .get("__Host-askhava-session")
        .expect("cleared cookie");
    assert_eq!(cookie.value(), "");
    let laptop = sign_in(&mut db, &secret, 60).await;
    let phone = sign_in(&mut db, &secret, 120).await;
    let owner = owner_principal(&mut db, &laptop, at(130))
        .await
        .expect("read")
        .expect("owner");
    assert_eq!(sign_out_everywhere(&mut db, &owner).await, Ok(2));
    assert!(
        owner_principal(&mut db, &phone, at(140))
            .await
            .expect("read")
            .is_none()
    );
    assert!(
        owner_principal(&mut db, &laptop, at(140))
            .await
            .expect("read")
            .is_none()
    );
}

/// TD-SEC-06 (SEC-007): a recovery code signs in only once two factors are registered, and only
/// once per code; the sign-in page has no email field and no email path.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_06_recovery_needs_two_factors_and_offline_codes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (secret, codes) = bootstrapped(&mut db).await;
    let early = (PASSWORD, SecondFactor::RecoveryCode(&codes[0]));
    let refused = owner_sign_in(&mut db, CookieJar::new(), early, &auth(), at(0)).await;
    assert_eq!(
        refused.err(),
        Some(OwnerAuthError::Refused),
        "one factor is not enough"
    );
    let jar = sign_in(&mut db, &secret, 30).await;
    let owner = owner_principal(&mut db, &jar, at(40))
        .await
        .expect("read")
        .expect("owner");
    let second = add_owner_totp(&mut db, &owner, ROUTE, at(40))
        .await
        .expect("second factor");
    assert!(!second.expose_secret().is_empty());
    assert_eq!(secrets(&mut db).await.len(), 2);
    let spaced = format!(" {}-{} ", &codes[0][..10], &codes[0][10..]).to_uppercase();
    let recovered = (PASSWORD, SecondFactor::RecoveryCode(&spaced));
    let ok = owner_sign_in(&mut db, CookieJar::new(), recovered, &auth(), at(50)).await;
    assert!(ok.is_ok(), "two factors plus a paper code signs in");
    let reused = (PASSWORD, SecondFactor::RecoveryCode(&codes[0]));
    let again = owner_sign_in(&mut db, CookieJar::new(), reused, &auth(), at(60)).await;
    assert_eq!(
        again.err(),
        Some(OwnerAuthError::Refused),
        "each code works once"
    );
    let wrong = (
        "fake wrong pass phrase",
        SecondFactor::RecoveryCode(&codes[1]),
    );
    let no = owner_sign_in(&mut db, CookieJar::new(), wrong, &auth(), at(70)).await;
    assert_eq!(
        no.err(),
        Some(OwnerAuthError::Refused),
        "a code never replaces the password"
    );
    let page = owner_signin_page("fake-csrf", None, true)
        .expect("page")
        .to_lowercase();
    assert!(!page.contains("email"), "no email path");
    assert!(page.contains("autocomplete=\"one-time-code\""));
}

/// TD-SEC-01 (SEC-006, SEC-019): pause, refund and release refuse anonymous, homeowner and
/// business sessions and pass for the owner; refund also needs a fresh factor.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_01_only_the_owner_has_pause_refund_and_release(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (secret, _) = bootstrapped(&mut db).await;
    let mut others = vec![CookieJar::new()];
    for (realm, id) in [(Realm::Homeowner, 0xa1), (Realm::Business, 0xb1)] {
        let life = SignedDuration::from_hours(24);
        let login = (realm, Uuid::from_u128(id));
        let jar = start_session(&mut db, CookieJar::new(), login, life, at(0)).await;
        others.push(jar.expect("session"));
    }
    let owner = sign_in(&mut db, &secret, 0).await;
    for power in [OwnerPower::Pause, OwnerPower::Refund, OwnerPower::Release] {
        for jar in &others {
            let denied = owner_power(&mut db, jar, (power, ROUTE), at(10)).await;
            assert_eq!(denied.err(), Some(OwnerAuthError::NotOwner), "{power:?}");
        }
        assert!(
            owner_power(&mut db, &owner, (power, ROUTE), at(10))
                .await
                .is_ok()
        );
    }
    let stale = owner_power(&mut db, &owner, (OwnerPower::Refund, ROUTE), at(400)).await;
    assert_eq!(stale.err(), Some(OwnerAuthError::NeedsFreshFactor));
    assert!(
        owner_power(&mut db, &owner, (OwnerPower::Pause, ROUTE), at(400))
            .await
            .is_ok()
    );
}

/// SEC-005, D-098: a passkey answer that is not a real WebAuthn assertion, or names an unknown
/// challenge, is refused; a registration answer that is not real is refused and its challenge is
/// used up; a relying party on another origin is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn passkey_answers_must_be_real_and_fresh(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (secret, _) = bootstrapped(&mut db).await;
    let fake = SecondFactor::Passkey {
        challenge: Uuid::from_u128(0xc1),
        credential: "{\"id\":\"fake\"}",
    };
    let refused = owner_sign_in(&mut db, CookieJar::new(), (PASSWORD, fake), &auth(), at(0)).await;
    assert_eq!(refused.err(), Some(OwnerAuthError::Refused));
    let jar = sign_in(&mut db, &secret, 30).await;
    let owner = owner_principal(&mut db, &jar, at(40))
        .await
        .expect("read")
        .expect("owner");
    let started = passkey_registration_options(&mut db, &owner, (&auth(), ROUTE), at(40)).await;
    let (challenge, options) = started.expect("options");
    assert!(options.contains("challenge"));
    let answer = (&auth(), challenge, "{\"id\":\"fake\"}", ROUTE);
    let bad = finish_passkey_registration(&mut db, &owner, answer, at(41)).await;
    assert_eq!(bad, Err(OwnerAuthError::Passkey));
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM portals.owner_challenge")
        .fetch_one(&mut *db)
        .await
        .expect("count");
    assert_eq!(left, 0, "a challenge is used once");
    let other = OwnerAuth::new("askhava.test", "https://elsewhere.example");
    assert_eq!(other.err(), Some(OwnerAuthError::Passkey));
}
