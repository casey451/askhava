//! `calls`' masked-number pool on a fresh database (TD-CALL-01: CALL-001, CALL-002, CALL-017,
//! CALL-021; TD-CON-28's T076 case: CON-035, CALL-014; D-077, D-160, D-231, D-1679 to D-1681).
//! Fake numbers (555-01xx and the 500-555 test range), IDs and words only.
//!
//! Each case builds the pool the way the app does, from settings pairs through
//! `TwilioConfig::from_settings`, and gives connected jobs their numbers with
//! `calls::api::assign_number`, then reads `calls.number_pool`, `ops.alert` and `ops.audit_log`.

use askhava_adapters::api::{DEFAULT_POOL_SIZE, MaskedPool, TwilioConfig};
use askhava_calls::api::{
    ASSIGNED_ACTION, AutomatedCall, CallGate, CallsError, Connected, Line, POOL_ALERT,
    SAFETY_NOT_DIALABLE_ALERT, assign_number, automated_call_gate,
};
use askhava_core::api::Settings;
use askhava_messaging::api::{RevokeChannel, suppress};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The homeowner's and the business's own fake phones: never a pool number.
const HER_PHONE: &str = "+19285550141";
const BUSINESS_PHONE: &str = "+19285550142";
/// A fake `+1` phone whose area code (999) is not in the quiet-hours table, so never texted.
const UNTABLED_PHONE: &str = "+19995550143";
/// 10:00 Arizona.
const DAY: &str = "2026-11-02T17:00:00Z";

fn ts() -> Timestamp {
    DAY.parse().expect("instant")
}

/// `n` fake pool numbers in the 500-555 test range.
fn numbers(n: usize) -> String {
    (0..n)
        .map(|i| format!("+1500555{:04}", 100 + i))
        .collect::<Vec<_>>()
        .join(",")
}

/// The Twilio settings pairs with `extra` added; the pool size is unset unless `extra` sets it.
fn pairs(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut all = vec![
        ("ASKHAVA_APP_ENV", "dev"),
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
    ];
    all.extend_from_slice(extra);
    all.into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// The pool settings hold for `extra`, or why they were refused.
fn config(extra: &[(&str, &str)]) -> Option<MaskedPool> {
    let pairs = pairs(extra);
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    TwilioConfig::from_settings(&settings, pairs)
        .ok()
        .map(|c| c.pool().clone())
}

/// A pool of `n` numbers, sized by the setting.
fn pool(n: usize) -> MaskedPool {
    let size = n.to_string();
    let list = numbers(n);
    config(&[
        ("ASKHAVA_MASKED_POOL_SIZE", size.as_str()),
        ("ASKHAVA_MASKED_NUMBERS", list.as_str()),
    ])
    .expect("pool")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x54), &key(0x64), &url).expect("vault")
}

/// A vault contact holding only the fake `phone`.
async fn contact(db: &mut PgConnection, phone: &str) -> Uuid {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    store_contact(db, &vault(), &new).await.expect("contact")
}

fn job(homeowner: Uuid, business: Uuid) -> Connected {
    Connected {
        job: Uuid::new_v4(),
        homeowner,
        business,
    }
}

async fn give(db: &mut PgConnection, pool: &MaskedPool, job: Connected) -> Line {
    assign_number(db, pool, job, ts()).await.expect("assign")
}

async fn pool_alerts(db: &mut PgConnection) -> i64 {
    alerts(db, POOL_ALERT).await
}

async fn alerts(db: &mut PgConnection, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1")
        .bind(kind)
        .fetch_one(db)
        .await
        .expect("alerts")
}

/// TD-CALL-01 (CALL-017, D-160): the pool setting starts at 10 numbers, a list of another length
/// is refused, and a set size takes exactly that many.
#[test]
fn td_call_01_the_pool_setting_starts_at_10() {
    assert_eq!(DEFAULT_POOL_SIZE, 10);
    let ten = numbers(10);
    let default = config(&[("ASKHAVA_MASKED_NUMBERS", ten.as_str())]).expect("ten");
    assert_eq!(default.numbers().len(), 10);
    let eleven = numbers(11);
    assert!(config(&[("ASKHAVA_MASKED_NUMBERS", eleven.as_str())]).is_none());
    assert_eq!(pool(3).numbers().len(), 3);
}

/// TD-CALL-01 (CALL-017, CALL-021, D-077, D-296): the app never buys a number: the voice adapter
/// has no buy, provision or release method, and nothing in `calls` names Twilio's number-buying
/// or Proxy endpoints.
#[test]
fn td_call_01_the_voice_adapter_has_no_buy_number_method() {
    let sources = [
        include_str!("../../../app/adapters/src/voice.rs"),
        include_str!("../../../app/calls/src/service.rs"),
        include_str!("../../../app/calls/src/models.rs"),
        include_str!("../../../app/calls/src/api.rs"),
    ];
    let banned = [
        "incomingphonenumbers",
        "availablephonenumbers",
        "proxy.twilio.com",
        "fn buy",
        "fn purchase",
        "fn provision",
    ];
    for source in sources.map(str::to_lowercase) {
        for word in banned {
            assert!(
                !source.contains(word),
                "a calls or voice source names {word}"
            );
        }
    }
}

/// TD-CALL-01 (CALL-001, CALL-002): a connected job gets a pool number that is neither side's own
/// phone, the same line again on a repeat call, and one audit row; a repeat call naming another
/// business or another homeowner is refused, never handed the open line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_01_each_connected_job_gets_a_pool_number_neither_sides_own(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let pool = pool(10);
    let connected = job(Uuid::new_v4(), Uuid::new_v4());
    let line = give(&mut db, &pool, connected).await;
    assert!(pool.numbers().contains(line.number()));
    for own in [HER_PHONE, BUSINESS_PHONE] {
        assert_ne!(line.number().as_str(), own);
    }
    assert!(!line.shared());
    assert_eq!(give(&mut db, &pool, connected).await, line);
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE action = $1 AND subject_id = $2",
    )
    .bind(ASSIGNED_ACTION)
    .bind(line.id())
    .fetch_one(&mut *db)
    .await
    .expect("audit");
    assert_eq!(audits, 1);
    let other = Connected {
        business: Uuid::new_v4(),
        ..connected
    };
    let refused = assign_number(&mut db, &pool, other, ts()).await;
    assert_eq!(refused, Err(CallsError::LineOpen));
    let stranger = Connected {
        homeowner: Uuid::new_v4(),
        ..connected
    };
    let refused = assign_number(&mut db, &pool, stranger, ts()).await;
    assert_eq!(refused, Err(CallsError::LineOpen));
}

/// TD-CALL-01 (CALL-017, D-077): a job gets a number no other open line of the same homeowner or
/// of the same business uses while one is free.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_01_same_homeowner_or_business_gets_another_number(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let pool = pool(3);
    let (her, b1, b2) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let first = give(&mut db, &pool, job(her, b1)).await;
    let second = give(&mut db, &pool, job(her, b2)).await;
    let third = give(&mut db, &pool, job(Uuid::new_v4(), b1)).await;
    assert_ne!(first.number(), second.number());
    assert_ne!(first.number(), third.number());
    let unrelated = give(&mut db, &pool, job(Uuid::new_v4(), Uuid::new_v4())).await;
    assert!(!unrelated.shared());
    for line in [&first, &second, &third] {
        assert!(!line.shared());
    }
    assert_eq!(pool_alerts(&mut db).await, 0);
}

/// TD-CALL-01 (CALL-017, CALL-004, D-077, D-231): with no free number the job shares one, marked
/// for PIN routing, and one pool alert is raised; while it is open no second one is; a later
/// assignment that finds a free number ends it, so the next shortage alerts again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_01_an_exhausted_pool_shares_with_pin_routing_and_alerts_once(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let pool = pool(2);
    let her = Uuid::new_v4();
    for _ in 0..2 {
        assert!(
            !give(&mut db, &pool, job(her, Uuid::new_v4()))
                .await
                .shared()
        );
    }
    let shared = give(&mut db, &pool, job(her, Uuid::new_v4())).await;
    assert!(shared.shared());
    assert!(pool.numbers().contains(shared.number()));
    assert_eq!(pool_alerts(&mut db).await, 1);
    assert!(
        give(&mut db, &pool, job(her, Uuid::new_v4()))
            .await
            .shared()
    );
    assert_eq!(pool_alerts(&mut db).await, 1, "at most one pool alert open");
    let free = give(&mut db, &pool, job(Uuid::new_v4(), Uuid::new_v4())).await;
    assert!(!free.shared());
    assert!(
        give(&mut db, &pool, job(her, Uuid::new_v4()))
            .await
            .shared()
    );
    assert_eq!(pool_alerts(&mut db).await, 2);
}

/// TD-CALL-01 (CALL-017): two jobs of one homeowner connected at once never take the same number
/// while two are free (the assignment holds one lock).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_01_two_jobs_at_once_get_two_numbers(db: PgPool) {
    let pool = pool(2);
    let her = Uuid::new_v4();
    let (mut a, mut b) = (
        db.acquire().await.expect("a"),
        db.acquire().await.expect("b"),
    );
    let (one, two) = tokio::join!(
        give(&mut a, &pool, job(her, Uuid::new_v4())),
        give(&mut b, &pool, job(her, Uuid::new_v4())),
    );
    assert_ne!(one.number(), two.number());
    assert!(!one.shared() && !two.shared());
}

/// D-1520, D-1679 (TD-SEC-34's calls case): the app role may close a line but never change its
/// number or remove it, and a pool alert row is append-only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_01_lines_keep_their_number_and_alerts_are_append_only(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let pool = pool(1);
    let her = Uuid::new_v4();
    give(&mut db, &pool, job(her, Uuid::new_v4())).await;
    give(&mut db, &pool, job(her, Uuid::new_v4())).await;
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    sqlx::query("UPDATE calls.number_pool SET closed_at = assigned_at")
        .execute(&mut *db)
        .await
        .expect("close");
    for denied in [
        "UPDATE calls.number_pool SET pool_number = '+15005550199'",
        "DELETE FROM calls.number_pool",
        "UPDATE calls.pool_alert SET line_seq = 0",
        "DELETE FROM calls.pool_alert",
    ] {
        let error = sqlx::query(denied)
            .execute(&mut *db)
            .await
            .expect_err(denied);
        assert!(error.to_string().contains("permission denied"), "{denied}");
    }
}

/// TD-CON-28's T076 case (CON-035, CALL-014, CALL-022, D-1681): after an opt-out no safety call
/// and no code call she did not ask for on screen may dial her number; one she just asked for may.
/// A gate that skipped the list check would answer `Dial` and turn this red.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_28_no_automated_call_after_an_opt_out(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let her = contact(&mut db, HER_PHONE).await;
    let not_asked = AutomatedCall::Code {
        asked_on_screen: false,
    };
    let asked = AutomatedCall::Code {
        asked_on_screen: true,
    };
    for call in [AutomatedCall::Safety, not_asked] {
        let gate = automated_call_gate(&mut db, &vault(), (her, call), ts()).await;
        assert_eq!(gate, Ok(CallGate::Dial));
    }
    suppress(
        &mut db,
        &vault(),
        HER_PHONE,
        (RevokeChannel::Portal, "portal"),
        ts(),
    )
    .await
    .expect("suppress");
    for call in [AutomatedCall::Safety, not_asked] {
        let gate = automated_call_gate(&mut db, &vault(), (her, call), ts()).await;
        assert_eq!(gate, Ok(CallGate::OptedOut));
    }
    let gate = automated_call_gate(&mut db, &vault(), (her, asked), ts()).await;
    assert_eq!(gate, Ok(CallGate::Dial));
}

/// D-1681, D-1682 (CALL-019, QH-003): a `+1` number whose area code is not in the quiet-hours
/// table is one `messaging` never texts, so no automated call dials it; a safety call answered
/// `NotDialable` raises one High alert about the contact instead of failing silently, a code call
/// raises none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_28_an_untextable_number_is_not_dialled_and_a_safety_call_alerts(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let her = contact(&mut db, UNTABLED_PHONE).await;
    let not_asked = AutomatedCall::Code {
        asked_on_screen: false,
    };
    let gate = automated_call_gate(&mut db, &vault(), (her, not_asked), ts()).await;
    assert_eq!(gate, Ok(CallGate::NotDialable));
    assert_eq!(alerts(&mut db, SAFETY_NOT_DIALABLE_ALERT).await, 0);
    let gate = automated_call_gate(&mut db, &vault(), (her, AutomatedCall::Safety), ts()).await;
    assert_eq!(gate, Ok(CallGate::NotDialable));
    let subject: (String, Uuid, String) =
        sqlx::query_as("SELECT subject_kind, subject_id, severity FROM ops.alert WHERE kind = $1")
            .bind(SAFETY_NOT_DIALABLE_ALERT)
            .fetch_one(&mut *db)
            .await
            .expect("one alert");
    assert_eq!(
        subject,
        ("privacy.contact".to_owned(), her, "high".to_owned())
    );
}
