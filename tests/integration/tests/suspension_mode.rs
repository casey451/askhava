//! The stored texting mode, the WEB-007 banner and an emergency while texting is off, through the
//! served inbound route on a real database (TD-TXT-33; TXT-021, TXT-022, TXT-023, TXT-024, INT-004,
//! WEB-007; T279, D-1843, D-2185 to D-2187).
//!
//! The route is `app/server/src/inbound_text.rs`, compiled here with `#[path]` and driven with
//! deliveries signed by a fake Twilio token, as in `inbound_text_route.rs`. Proves: the
//! homeowner-texting switch and a recorded suspension survive a restart (a new pool); while off or
//! suspended an emergency message raises one critical alert, turns the banner on, and its follow-up
//! queues the INT-004 email for a homeowner with an email and puts one without on Casey's call
//! list, and nothing is texted; a non-emergency message is handled as today; a recorded restore
//! keeps texting off until switched on; the banner clears only by Casey's action. All data is fake.

use std::sync::Arc;

use askhava_core::api::{Claimed, ManualClock};
use askhava_intake::api::SAFETY_CALL_KIND;
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_integration::{is_permission_denied, set_app_role};
use askhava_messaging::api::{
    Audience, LaunchChannel, MailboxKind, ModeError, ModeStateError, NumberState,
    PAUSED_SAFETY_KIND, SafetySender, SendingNumber, launch_channel, load_lines, record_restore,
    record_suspension, run_paused_safety, set_launch_channel, switch_off, switch_on,
};
use askhava_ops::api::{banner_shown, clear_banner};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::{INBOUND_PATH, Webhooks, routes};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

/// The route under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
mod inbound_text;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const HOME: &str = "+19285550142";
const OTHER: &str = "+19285550143";
const ASK_HAVA: &str = "+19285550100";
const CASEY: Uuid = Uuid::from_u128(0xCA5E);
const GAS: &str = "I smell gas in the kitchen";

fn at() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x53), &key(0x63), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn hooks(pool: &PgPool) -> Arc<Webhooks> {
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(at())),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: secret(),
        vault: vault(),
        names: Vec::new(),
        intake: IntakeSettings::default(),
    })
}

/// POSTs one signed inbound text with MessageSid `sid`; the route's status.
async fn text(hooks: &Arc<Webhooks>, sid: u32, from: &str, words: &str) -> StatusCode {
    let sid = format!("SM{sid:032}");
    let mut fields = [
        ("MessageSid", sid.as_str()),
        ("From", from),
        ("To", ASK_HAVA),
        ("Body", words),
    ];
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", encode(v)))
        .collect();
    fields.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{BASE}{INBOUND_PATH}").as_bytes());
    for (name, value) in fields {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let request = Request::post(INBOUND_PATH)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let answer = routes(Arc::clone(hooks)).oneshot(request).await;
    answer.expect("answer").status()
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn contact(db: &mut PgConnection, phone: &str, email: Option<&str>) -> Uuid {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.to_owned())),
        email: email.map(|e| SecretString::from(e.to_owned())),
        address: None,
        onsite_contact: None,
    };
    store_contact(db, &vault(), &new).await.expect("contact")
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn paused_alerts(pool: &PgPool) -> i64 {
    count(
        pool,
        "SELECT count(*) FROM ops.alert WHERE kind = 'messaging.paused_emergency' \
         AND severity = 'critical'",
    )
    .await
}

/// Runs every queued paused-safety follow-up once, as the worker would.
async fn run_follow_ups(pool: &PgPool) {
    let sender = SafetySender::new(
        (secret(), vault()),
        ("P.O. Box 9", MailboxKind::PoBox),
        "https://askhava.com/q/fake-token",
    )
    .expect("sender");
    let sender = Arc::new(sender);
    let rows: Vec<(i64, serde_json::Value)> =
        sqlx::query_as("SELECT id, payload FROM core.task WHERE kind = $1 ORDER BY id")
            .bind(PAUSED_SAFETY_KIND)
            .fetch_all(pool)
            .await
            .expect("tasks");
    for (id, payload) in rows {
        let task = Claimed {
            id,
            kind: PAUSED_SAFETY_KIND.to_owned(),
            payload,
            attempts: 1,
        };
        let done = run_paused_safety(pool.clone(), Arc::clone(&sender), task).await;
        done.expect("follow-up");
    }
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_33_the_switch_and_a_suspension_survive_a_restart(pool: PgPool) {
    let number = || Some(SendingNumber::parse(ASK_HAVA).expect("number"));
    let mut db = pool.acquire().await.expect("connection");
    set_app_role(&mut db).await.expect("role");
    let fresh = load_lines(&mut db, (number(), None)).await.expect("lines");
    assert!(fresh.homeowner.sender().is_some() && !fresh.business.texting_on);
    let casey = (CASEY, at());
    assert!(
        switch_off(&mut db, Audience::Homeowner, casey)
            .await
            .expect("off")
    );
    assert!(
        !switch_off(&mut db, Audience::Homeowner, casey)
            .await
            .expect("repeat")
    );
    let since = at() - SignedDuration::from_hours(1);
    assert!(
        record_suspension(&mut db, Audience::Homeowner, since, casey)
            .await
            .expect("held")
    );
    drop(db);
    let options = pool.connect_options();
    pool.close().await;
    let again = PgPool::connect_with((*options).clone())
        .await
        .expect("restart");
    let mut db = again.acquire().await.expect("connection");
    set_app_role(&mut db).await.expect("role");
    let lines = load_lines(&mut db, (number(), None)).await.expect("lines");
    assert_eq!(lines.homeowner.state, NumberState::Suspended { since });
    assert!(!lines.homeowner.texting_on && !lines.business.texting_on);
    let refused = switch_on(&mut db, Audience::Homeowner, casey).await;
    assert_eq!(
        refused,
        Err(ModeStateError::Refused(ModeError::NotRestored))
    );
    assert!(
        record_restore(&mut db, Audience::Homeowner, casey)
            .await
            .expect("restored")
    );
    let lines = load_lines(&mut db, (number(), None)).await.expect("lines");
    assert_eq!(lines.homeowner.state, NumberState::Restored { since });
    assert!(
        lines.homeowner.sender().is_none(),
        "texting stays off until Casey"
    );
    assert!(
        switch_on(&mut db, Audience::Homeowner, casey)
            .await
            .expect("on")
    );
    let lines = load_lines(&mut db, (number(), None)).await.expect("lines");
    assert!(lines.homeowner.sender().is_some() && !lines.business.texting_on);
    let web = LaunchChannel::WebAndEmail;
    assert!(
        set_launch_channel(&mut db, Audience::Homeowner, web, casey)
            .await
            .expect("set")
    );
    assert_eq!(launch_channel(&mut db, Audience::Homeowner).await, Ok(web));
    let lines = load_lines(&mut db, (number(), None)).await.expect("lines");
    assert!(!lines.homeowner.texting_on);
    let rows = count(&again, "SELECT count(*) FROM messaging.mode_state").await;
    let audited = "SELECT count(*) FROM ops.audit_log WHERE action = 'messaging.mode_changed' \
                   AND actor_id = '00000000-0000-0000-0000-00000000ca5e'";
    assert_eq!((rows, count(&again, audited).await), (5, 5));
    let rewrite = sqlx::query("UPDATE messaging.mode_state SET texting_on = true")
        .execute(&mut *db)
        .await;
    assert!(is_permission_denied(&rewrite), "history is append-only");
    drop(db);
    again.close().await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_33_switched_off_an_emergency_alerts_once_and_texts_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let with = contact(&mut db, HOME, Some("ada@example.test")).await;
    contact(&mut db, OTHER, None).await;
    switch_off(&mut db, Audience::Homeowner, (CASEY, at()))
        .await
        .expect("off");
    let hooks = hooks(&pool);
    assert_eq!(text(&hooks, 1, HOME, GAS).await, StatusCode::OK);
    assert_eq!(text(&hooks, 1, HOME, GAS).await, StatusCode::OK);
    assert_eq!(text(&hooks, 2, OTHER, GAS).await, StatusCode::OK);
    assert_eq!(
        paused_alerts(&pool).await,
        2,
        "one critical alert per message"
    );
    assert!(banner_shown(&mut db).await.expect("banner"));
    run_follow_ups(&pool).await;
    run_follow_ups(&pool).await;
    let emailed: Vec<Uuid> = sqlx::query_scalar(
        "SELECT recipient_id FROM messaging.email_outbox WHERE template_id = 'paused_safety_email'",
    )
    .fetch_all(&pool)
    .await
    .expect("emails");
    assert_eq!(emailed, [with]);
    let called = count(
        &pool,
        "SELECT count(*) FROM ops.alert a JOIN messaging.inbox i ON i.id = a.subject_id \
         WHERE a.kind = 'messaging.call_back_due' AND a.severity = 'high'",
    )
    .await;
    assert_eq!(
        called, 1,
        "the homeowner with no email is on Casey's call list"
    );
    let int_004 = "SELECT count(*) FROM intake.emergency_event";
    assert_eq!(
        count(&pool, int_004).await,
        2,
        "intake's event is still queued"
    );
    let sent = "SELECT count(*) FROM messaging.outbox WHERE status <> 'queued'";
    assert_eq!(count(&pool, sent).await, 0, "nothing is texted");
    let calls: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(SAFETY_CALL_KIND)
        .fetch_one(&pool)
        .await
        .expect("calls");
    assert_eq!(calls, 0, "the safety voice call setting is off");
    assert_eq!(
        text(&hooks, 3, HOME, "the kitchen sink drips").await,
        StatusCode::OK
    );
    assert_eq!(
        paused_alerts(&pool).await,
        2,
        "a plain message is handled as today"
    );
    let tasks = "SELECT count(*) FROM core.task WHERE kind = 'messaging.paused_safety'";
    assert_eq!(count(&pool, tasks).await, 2);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_33_a_restore_keeps_texting_off_and_only_casey_clears_the_banner(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    contact(&mut db, HOME, None).await;
    let casey = (CASEY, at());
    let since = at() - SignedDuration::from_hours(2);
    record_suspension(&mut db, Audience::Homeowner, since, casey)
        .await
        .expect("held");
    record_restore(&mut db, Audience::Homeowner, casey)
        .await
        .expect("restored");
    let hooks = hooks(&pool);
    assert_eq!(text(&hooks, 1, HOME, GAS).await, StatusCode::OK);
    assert_eq!(
        paused_alerts(&pool).await,
        1,
        "restored is still off until switched on"
    );
    switch_on(&mut db, Audience::Homeowner, casey)
        .await
        .expect("on");
    assert_eq!(text(&hooks, 2, HOME, GAS).await, StatusCode::OK);
    assert_eq!(
        paused_alerts(&pool).await,
        1,
        "an open line takes the normal INT-004 path"
    );
    assert!(
        banner_shown(&mut db).await.expect("banner"),
        "switching on leaves the banner"
    );
    set_app_role(&mut db).await.expect("role");
    let by_system = sqlx::query(
        "INSERT INTO ops.status_banner (shown, actor_kind, changed_at) VALUES (false, 'system', now())",
    )
    .execute(&mut *db)
    .await;
    assert!(by_system.is_err(), "only Casey clears the banner");
    let rewrite = sqlx::query("UPDATE ops.status_banner SET shown = false")
        .execute(&mut *db)
        .await;
    assert!(is_permission_denied(&rewrite));
    assert!(clear_banner(&mut db, CASEY, at()).await.expect("cleared"));
    assert!(!clear_banner(&mut db, CASEY, at()).await.expect("repeat"));
    assert!(!banner_shown(&mut db).await.expect("banner"));
}
