//! Every homeowner sign-in code is bound to its purpose by a local challenge with a try cap
//! (TD-SEC-60; SEC-055, SEC-008, PRIV-031; T346, D-2348, D-2349, AVAIL-01).
//!
//! Drives `portals`' homeowner sign-in against PostgreSQL and the Twilio stand-in unchanged
//! (`stand_ins/server.py`, which keys a pending code by number only, like the one shared Verify
//! service), through a counting wrapper that can also answer right without the stand-in or hang:
//! a pick code or an expired challenge is refused before Verify is called; 20 simultaneous wrong
//! checks reach Verify 5 times; a used challenge is refused; a re-send inside the code's life
//! reuses the challenge; two right checks start one session; a hanging Verify leaves a pool of 5
//! free. The copy and delete cases need her `OnFile`, which only a `SessionMint` holder can make,
//! so they run in `askhava-portals`' `data_download::tests` (D-3293). Every number is fake.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use askhava_adapters::api::{
    AdapterError, CodeChannel, IdempotencyKey, TwilioConfig, TwilioVerify, VerifyAdapter,
    twilio_clients,
};
use askhava_core::api::Settings;
use askhava_portals::api::{
    CookieJar, HomeownerError, Realm, current_session, finish_sign_in, send_sign_in_code,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::sync::Barrier;
use tokio::task::JoinSet;

const PHONE: &str = "+19285550100";
const WRONG: &str = "000001";

/// The stand-in server on a port it picked itself; killed on drop.
struct StandIn(Child);

impl Drop for StandIn {
    fn drop(&mut self) {
        let _killed = self.0.kill();
        let _reaped = self.0.wait();
    }
}

/// The stand-in and the real Verify client pointed at it.
fn stand_in() -> (StandIn, TwilioVerify) {
    let mut child = Command::new("python3")
        .args(["-m", "stand_ins.server", "--port", "0"])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .stdout(Stdio::piped())
        .spawn()
        .expect("python3 stand-in");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("listening line");
    std::thread::spawn(move || std::io::copy(&mut reader, &mut std::io::sink()));
    let port = line.trim_end().rsplit(':').next().unwrap_or_default();
    let base = format!("http://127.0.0.1:{port}/twilio");
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "ci"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_NUMBERS", "+19285550110,+19285550111"),
        ("ASKHAVA_MASKED_POOL_SIZE", "2"),
        ("ASKHAVA_TWILIO_BASE_URL", &base),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", &base),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let config = TwilioConfig::from_settings(&settings, pairs).expect("twilio config");
    let (_, _, verify) = twilio_clients(&config).expect("clients");
    (StandIn(child), verify)
}

/// The code the stand-in texts to `to` (`stand_ins/twilio.py` `start_verification`).
fn right_code(to: &str) -> String {
    let sum: u32 = to.chars().map(u32::from).sum();
    format!("{:06}", sum % 1_000_000)
}

/// What the wrapper does with a check: ask the stand-in, answer right once two arrive, or hang.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    StandIn,
    RightInPairs,
    Hang,
}

/// The stand-in's Verify client, counting every send and check that reaches it.
struct Counted {
    inner: TwilioVerify,
    mode: Mutex<Mode>,
    pair: Barrier,
    sends: AtomicUsize,
    checks: AtomicUsize,
}

impl Counted {
    fn mode(&self) -> Mode {
        *self.mode.lock().unwrap_or_else(PoisonError::into_inner)
    }
    fn set(&self, mode: Mode) {
        *self.mode.lock().unwrap_or_else(PoisonError::into_inner) = mode;
    }
    fn checks(&self) -> usize {
        self.checks.load(Ordering::SeqCst)
    }
}

impl VerifyAdapter for Counted {
    async fn send_code(
        &self,
        key: &IdempotencyKey,
        to: &str,
        channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        if self.mode() == Mode::Hang {
            std::future::pending::<()>().await;
        }
        self.inner.send_code(key, to, channel).await
    }
    async fn check_code(&self, to: &str, code: &str) -> Result<bool, AdapterError> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        match self.mode() {
            Mode::StandIn => self.inner.check_code(to, code).await,
            Mode::RightInPairs => {
                self.pair.wait().await;
                Ok(true)
            }
            Mode::Hang => std::future::pending().await,
        }
    }
}

struct World {
    _stand_in: StandIn,
    verify: Arc<Counted>,
    vault: Arc<Vault>,
    key: SecretString,
}

fn vault() -> Vault {
    let key = |k: &str| SecretString::from(format!("1:{k}"));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let (seal, index) = ("UlJS".repeat(10) + "UlI=", "YmJi".repeat(10) + "YmI=");
    Vault::new(&key(&seal), &key(&index), &url).expect("vault")
}

/// The stand-in, the counting client and her contact holding `PHONE`.
async fn world(pool: &PgPool) -> World {
    let (stand_in, inner) = stand_in();
    let vault = vault();
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        phone: Some(SecretString::from(PHONE.to_owned())),
        ..NewContact::default()
    };
    store_contact(&mut db, &vault, &new).await.expect("contact");
    let verify = Arc::new(Counted {
        inner,
        mode: Mutex::new(Mode::StandIn),
        pair: Barrier::new(2),
        sends: AtomicUsize::new(0),
        checks: AtomicUsize::new(0),
    });
    let key = SecretString::from("fake-key-for-tests-only".to_owned());
    World {
        _stand_in: stand_in,
        verify,
        vault: Arc::new(vault),
        key,
    }
}

fn t0() -> Timestamp {
    "2026-11-12T17:00:00Z".parse().expect("time")
}

async fn send(w: &World, pool: &PgPool, now: Timestamp) {
    let parts = (&*w.vault, &*w.verify, &w.key);
    let sent = send_sign_in_code(pool, parts, PHONE, now).await;
    assert_eq!(sent, Ok(()));
}

async fn check(
    w: &World,
    pool: &PgPool,
    code: &str,
    now: Timestamp,
) -> Result<CookieJar, HomeownerError> {
    let parts = (&*w.vault, &*w.verify);
    finish_sign_in(pool, parts, (CookieJar::new(), PHONE, code), now).await
}

/// Every challenge as (tries, used), oldest first.
async fn challenges(pool: &PgPool) -> Vec<(i32, bool)> {
    let sql = "SELECT tries, used_at IS NOT NULL FROM portals.code_challenge ORDER BY id";
    sqlx::query_as(sql)
        .fetch_all(pool)
        .await
        .expect("challenges")
}

async fn sessions(pool: &PgPool) -> i64 {
    let sql = "SELECT count(*) FROM portals.session WHERE realm = 'homeowner'";
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("sessions")
}

/// TD-SEC-60 (SEC-055, SEC-008): a pick code, or a sign-in code whose challenge expired, is
/// refused at sign-in before Verify is called.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_60_a_pick_code_or_an_expired_one_is_refused_before_verify(pool: PgPool) {
    let w = world(&pool).await;
    let pick = IdempotencyKey::derive(&w.key, "verify.page_pick", &["fake-pick"]);
    let sent = w
        .verify
        .inner
        .send_code(&pick, PHONE, CodeChannel::Sms)
        .await;
    assert!(sent.is_ok());
    let refused = check(&w, &pool, &right_code(PHONE), t0()).await;
    assert_eq!(refused.err(), Some(HomeownerError::Refused));
    send(&w, &pool, t0()).await;
    let late = t0() + SignedDuration::from_mins(10);
    let refused = check(&w, &pool, &right_code(PHONE), late).await;
    assert_eq!(refused.err(), Some(HomeownerError::Refused));
    assert_eq!(w.verify.checks(), 0);
    assert_eq!(
        (challenges(&pool).await, sessions(&pool).await),
        (vec![(0, false)], 0)
    );
}

/// TD-SEC-60 (SEC-055): 20 simultaneous wrong checks reach Verify 5 times; the sixth, even with
/// the right code, is refused before Verify.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_60_twenty_wrong_checks_reach_verify_five_times(pool: PgPool) {
    let w = Arc::new(world(&pool).await);
    send(&w, &pool, t0()).await;
    let mut all = JoinSet::new();
    for _ in 0..20 {
        let (w, pool) = (Arc::clone(&w), pool.clone());
        all.spawn(async move { check(&w, &pool, WRONG, t0()).await.err() });
    }
    while let Some(done) = all.join_next().await {
        assert_eq!(done.expect("task"), Some(HomeownerError::Refused));
    }
    assert_eq!(w.verify.checks(), 5);
    let sixth = check(&w, &pool, &right_code(PHONE), t0()).await;
    assert_eq!(sixth.err(), Some(HomeownerError::Refused));
    assert_eq!(w.verify.checks(), 5);
    assert_eq!(challenges(&pool).await, [(5, false)]);
}

/// TD-SEC-60 (SEC-055, SEC-008): a right code and the session commit together, the challenge is
/// used, and a used challenge is refused before Verify.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_60_a_right_code_signs_in_once_and_uses_its_challenge(pool: PgPool) {
    let w = world(&pool).await;
    send(&w, &pool, t0()).await;
    let jar = check(&w, &pool, &right_code(PHONE), t0())
        .await
        .expect("signed in");
    let mut db = pool.acquire().await.expect("db");
    let session = current_session(&mut db, &jar, t0()).await.expect("session");
    assert_eq!(session.map(|s| s.realm()), Some(Realm::Homeowner));
    assert_eq!(challenges(&pool).await, [(1, true)]);
    let again = check(&w, &pool, &right_code(PHONE), t0()).await;
    assert_eq!(again.err(), Some(HomeownerError::Refused));
    assert_eq!((w.verify.checks(), sessions(&pool).await), (1, 1));
}

/// TD-SEC-60 (SEC-055): a re-send inside the code's life reuses the challenge and gives no more
/// tries; one after expiry, or after use, opens a new one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_60_a_resend_reuses_the_live_challenge(pool: PgPool) {
    let w = world(&pool).await;
    send(&w, &pool, t0()).await;
    assert!(check(&w, &pool, WRONG, t0()).await.is_err());
    send(&w, &pool, t0() + SignedDuration::from_mins(5)).await;
    assert_eq!(challenges(&pool).await, [(1, false)]);
    let expired = t0() + SignedDuration::from_mins(10);
    send(&w, &pool, expired).await;
    assert_eq!(challenges(&pool).await, [(1, false), (0, false)]);
    assert!(check(&w, &pool, &right_code(PHONE), expired).await.is_ok());
    send(&w, &pool, expired).await;
    let all = [(1, false), (1, true), (0, false)];
    assert_eq!(challenges(&pool).await, all);
}

/// TD-SEC-60 (SEC-055): two simultaneous right checks of one challenge start one session.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_60_two_right_checks_act_once(pool: PgPool) {
    let w = Arc::new(world(&pool).await);
    send(&w, &pool, t0()).await;
    w.verify.set(Mode::RightInPairs);
    let mut both = JoinSet::new();
    for _ in 0..2 {
        let (w, pool) = (Arc::clone(&w), pool.clone());
        both.spawn(async move { check(&w, &pool, WRONG, t0()).await.is_ok() });
    }
    let signed = both.join_all().await;
    assert_eq!(signed.iter().filter(|ok| **ok).count(), 1);
    assert_eq!(
        (sessions(&pool).await, challenges(&pool).await),
        (1, vec![(2, true)])
    );
}

/// TD-SEC-60 (AVAIL-01, D-2349): with Verify hanging, 30 concurrent sends and checks leave a pool
/// of 5 connections able to serve another request.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_60_a_hanging_verify_leaves_the_pool_free(pool: PgPool) {
    let w = Arc::new(world(&pool).await);
    let small = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("pool");
    send(&w, &small, t0()).await;
    w.verify.set(Mode::Hang);
    let mut hanging = JoinSet::new();
    for n in 0..30 {
        let (w, small) = (Arc::clone(&w), small.clone());
        hanging.spawn(async move {
            if n % 2 == 0 {
                let parts = (&*w.vault, &*w.verify, &w.key);
                let _hung = send_sign_in_code(&small, parts, PHONE, t0()).await;
            } else {
                let _hung = check(&w, &small, WRONG, t0()).await;
            }
        });
    }
    let reached = || w.verify.sends.load(Ordering::SeqCst) + w.verify.checks();
    let waited = tokio::time::timeout(Duration::from_secs(20), async {
        while reached() < 1 + 15 + 5 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    assert!(waited.await.is_ok(), "the sends and checks reached Verify");
    let served = tokio::time::timeout(Duration::from_secs(2), challenges(&small)).await;
    assert_eq!(
        served.expect("the pool served another request"),
        [(5, false)]
    );
    hanging.abort_all();
}
