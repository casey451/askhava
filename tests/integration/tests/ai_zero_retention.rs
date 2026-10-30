//! The AI-008 gate, keyword-only mode, off-database readings and weekly counts on a fresh
//! database (TD-AI-07, TD-AI-04; AI-001, AI-008, AI-013, AI-014).
//!
//! Proves that in live, with the "AI on scrubbed text" switch off, the adapter is called zero
//! times; that the switch refuses to turn on until the newest provider-terms row is complete
//! (training-free, flagged-content retention, a Console spend limit of at most 3 times the
//! expected cost) and the privacy-policy version in force today holds the AI-018 sentence; that
//! once on, the adapter gets only the scrubbed text and no image block; that a signed zero-retention
//! date opens the gate without the switch; that `ai.reading` has no text column and each reading is
//! queued for the off-database stream as code fields only; and that the weekly summary is counts.
//! All data is fake.

use std::sync::Mutex;

use askhava_ai::api::{
    AiConfig, AiMode, AiOff, Endpoint, Fault, Intake, IntakeCall, Missing, OpenTrades, Origin,
    Reply, SwitchError, Transport, WeeklyCounts, ai_mode, read_intake, set_ai_switch,
    weekly_counts,
};
use askhava_core::api::Environment;
use askhava_privacy::api::{CommonWords, Known, NameMemory, scrub};
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const OWNER: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0025);

/// A transport that answers every call with one valid reading and keeps each request body.
#[derive(Default)]
struct Counting(Mutex<Vec<Value>>);

impl Transport for Counting {
    async fn post(&self, _: Endpoint, _: &str, _: Uuid, body: &Value) -> Result<Reply, Fault> {
        self.0.lock().unwrap().push(body.clone());
        let fields = json!({"trade": "plumbing_repair", "trade_confidence": 0.9,
            "job_type": "leak repair", "job_type_confidence": 0.8, "emergency": false,
            "urgent": false, "language": "en", "major": false, "missing_info": []});
        let body = json!({"id": "msg_fake25", "stop_reason": "end_turn",
            "content": [{"type": "text", "text": fields.to_string()}]});
        Ok(Reply {
            body,
            request_id: Some("req_fake25".to_owned()),
        })
    }
}

/// One intake reading of a fake request in `env`, through the gate.
async fn intake(db: &mut PgConnection, transport: &Counting, env: Environment) -> Intake {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let text = scrub(
        "water leak under the sink, call 928-555-0125",
        &known,
        &mut NameMemory::default(),
    );
    let open = OpenTrades::new([("plumbing_repair".to_owned(), vec!["Leak repair".to_owned()])]);
    let call = IntakeCall {
        subject: OWNER,
        origin: Origin::Text,
        text: &text,
        open: &open,
    };
    let config = AiConfig::from_pairs([]).unwrap();
    read_intake(db, transport, &config, env, &call)
        .await
        .unwrap()
}

/// Records a provider-terms row as the owner's account checklist would.
async fn terms(
    db: &mut PgConnection,
    training_free: bool,
    flagged: Option<i32>,
    spend: Option<i64>,
) {
    sqlx::query(
        "INSERT INTO ai.provider_terms (terms_url, checked_on, inputs_not_used_for_training, \
         retention_days, flagged_retention_days, spend_limit_cents, expected_monthly_cents) \
         VALUES ('https://example.com/fake-terms', now()::date, $1, 30, $2, $3, 2000)",
    )
    .bind(training_free)
    .bind(flagged)
    .bind(spend)
    .execute(db)
    .await
    .unwrap();
}

/// Records privacy-policy `version`, in force from today plus `days`.
async fn policy(db: &mut PgConnection, version: &str, days: i32, sentence: bool) {
    sqlx::query(
        "INSERT INTO ai.privacy_policy_version (version, effective_on, has_ai018_sentence) \
         VALUES ($1, (now() AT TIME ZONE 'America/Phoenix')::date + $2, $3)",
    )
    .bind(version)
    .bind(days)
    .bind(sentence)
    .execute(db)
    .await
    .unwrap();
}

/// Records a zero-retention signing date, today plus `days`, on the newest provider-terms row.
async fn sign(db: &mut PgConnection, days: i32) {
    sqlx::query(
        "UPDATE ai.provider_terms \
         SET zero_retention_signed_on = (now() AT TIME ZONE 'America/Phoenix')::date + $1 \
         WHERE id = (SELECT max(id) FROM ai.provider_terms)",
    )
    .bind(days)
    .execute(db)
    .await
    .unwrap();
}

async fn mode(db: &mut PgConnection) -> AiMode {
    ai_mode(db, Environment::Live).await.unwrap()
}

async fn refused(db: &mut PgConnection) -> Option<Missing> {
    match set_ai_switch(db, true, OWNER).await {
        Ok(()) => None,
        Err(SwitchError::Refused(missing)) => Some(missing),
        Err(SwitchError::Database) => panic!("database error"),
    }
}

/// TD-AI-07 (AI-001, AI-008): in live with the switch off the adapter is called zero times and
/// intake runs keyword-only; outside live (build, `Ci`, the test project) the call runs.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn with_the_switch_off_the_adapter_is_called_zero_times(pool: PgPool) {
    let mut db = pool.acquire().await.unwrap();
    let transport = Counting::default();
    assert_eq!(
        ai_mode(&mut db, Environment::Live).await.unwrap(),
        AiMode::KeywordOnly
    );
    let off = intake(&mut db, &transport, Environment::Live).await;
    assert_eq!(off, Intake::AiOff(AiOff::SwitchOff));
    assert_eq!(transport.0.lock().unwrap().len(), 0, "no AI call in live");
    assert!(matches!(
        intake(&mut db, &transport, Environment::Test).await,
        Intake::Reading(_)
    ));
    assert_eq!(transport.0.lock().unwrap().len(), 1);
}

/// TD-AI-07, TD-AI-04 (AI-008, D-412, D-491, D-492): the switch refuses each missing record in
/// turn, including a policy version that lacks the AI-018 sentence or is not yet in force; once
/// on, the adapter gets only scrubbed text with no image block; a policy change that drops the
/// sentence closes the gate again; turning off always works and both flips are audited.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_switch_turns_on_only_once_every_record_holds(pool: PgPool) {
    let mut db = pool.acquire().await.unwrap();
    assert_eq!(refused(&mut db).await, Some(Missing::Terms));
    terms(&mut db, false, Some(30), Some(6000)).await;
    assert_eq!(refused(&mut db).await, Some(Missing::TrainingFree));
    terms(&mut db, true, None, Some(6000)).await;
    assert_eq!(refused(&mut db).await, Some(Missing::FlaggedRetention));
    terms(&mut db, true, Some(730), None).await;
    assert_eq!(refused(&mut db).await, Some(Missing::SpendLimit));
    terms(&mut db, true, Some(730), Some(6001)).await;
    assert_eq!(
        refused(&mut db).await,
        Some(Missing::SpendLimit),
        "over 3 times"
    );
    terms(&mut db, true, Some(730), Some(6000)).await;
    assert_eq!(refused(&mut db).await, Some(Missing::PolicyWording));
    policy(&mut db, "2026.10a", 0, false).await;
    policy(&mut db, "2026.11", 1, true).await;
    assert_eq!(refused(&mut db).await, Some(Missing::PolicyWording));
    policy(&mut db, "2026.10b", 0, true).await;
    assert_eq!(refused(&mut db).await, None);
    let transport = Counting::default();
    assert!(matches!(
        intake(&mut db, &transport, Environment::Live).await,
        Intake::Reading(_)
    ));
    let sent = transport.0.lock().unwrap().pop().unwrap();
    let content = sent["messages"][0]["content"].as_str().unwrap();
    assert!(
        !content.contains("555") && content.contains("water leak"),
        "scrubbed text only"
    );
    assert!(
        !sent.to_string().contains("\"image\""),
        "no photo before zero retention"
    );
    policy(&mut db, "2026.10c", 0, false).await;
    assert_eq!(mode(&mut db).await, AiMode::KeywordOnly);
    policy(&mut db, "2026.10d", 0, true).await;
    assert_eq!(mode(&mut db).await, AiMode::Ai, "every record holds");
    set_ai_switch(&mut db, false, OWNER).await.unwrap();
    assert_eq!(mode(&mut db).await, AiMode::KeywordOnly, "off closes it");
    let off = intake(&mut db, &transport, Environment::Live).await;
    assert_eq!(off, Intake::AiOff(AiOff::SwitchOff));
    assert!(transport.0.lock().unwrap().is_empty(), "no call once off");
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE action IN ('ai.switch_on', 'ai.switch_off')",
    )
    .fetch_one(&mut *db)
    .await
    .unwrap();
    assert_eq!(audited, 2);
}

/// TD-AI-07 (AI-008): a recorded zero-retention signing date opens the gate with the switch off,
/// but only once that date has come in Arizona.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_signed_zero_retention_agreement_opens_the_gate(pool: PgPool) {
    let mut db = pool.acquire().await.unwrap();
    terms(&mut db, true, Some(730), None).await;
    sign(&mut db, 1).await;
    assert_eq!(mode(&mut db).await, AiMode::KeywordOnly, "not signed yet");
    sign(&mut db, 0).await;
    assert_eq!(mode(&mut db).await, AiMode::Ai);
}

/// TD-AI-07 (AI-008, D-841): the gate reads only the newest provider-terms row, so a newer row
/// without a zero-retention date, or one missing a switch record, closes an open gate.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_newer_incomplete_terms_row_closes_the_gate(pool: PgPool) {
    let mut db = pool.acquire().await.unwrap();
    terms(&mut db, true, Some(730), Some(6000)).await;
    sign(&mut db, 0).await;
    assert_eq!(mode(&mut db).await, AiMode::Ai);
    terms(&mut db, true, Some(730), Some(6000)).await;
    assert_eq!(
        mode(&mut db).await,
        AiMode::KeywordOnly,
        "older date ignored"
    );
    policy(&mut db, "2026.10a", 0, true).await;
    assert_eq!(refused(&mut db).await, None);
    assert_eq!(mode(&mut db).await, AiMode::Ai);
    terms(&mut db, true, None, Some(6000)).await;
    assert_eq!(
        mode(&mut db).await,
        AiMode::KeywordOnly,
        "newer row incomplete"
    );
    let transport = Counting::default();
    let off = intake(&mut db, &transport, Environment::Live).await;
    assert_eq!(off, Intake::AiOff(AiOff::SwitchOff));
    assert!(transport.0.lock().unwrap().is_empty());
}

/// TD-AI-04 (AI-013, AI-014, PRIV-029): `ai.reading` has no text column; a reading is queued for
/// the off-database stream with code fields only, never the homeowner's words; the weekly
/// summary is counts only; the app role cannot rewrite the switch history.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn readings_go_off_database_as_codes_and_the_week_is_counts(pool: PgPool) {
    let mut db = pool.acquire().await.unwrap();
    let text_columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns WHERE table_schema = 'ai' \
         AND table_name = 'reading' AND data_type IN ('text', 'character varying', 'character', \
         'json', 'jsonb', 'bytea', 'ARRAY', 'USER-DEFINED')",
    )
    .fetch_one(&mut *db)
    .await
    .unwrap();
    assert_eq!(text_columns, 0);
    askhava_integration::set_app_role(&mut db).await.unwrap();
    intake(&mut db, &Counting::default(), Environment::Dev).await;
    let payload: Value =
        sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = 'ops.offdb_stream'")
            .fetch_one(&mut *db)
            .await
            .unwrap();
    assert_eq!(payload["stream"], "ai_reading");
    let fields = &payload["fields"];
    assert_eq!(
        (&fields["trade"], &fields["job_type"]),
        (&json!("plumbing_repair"), &json!("leak_repair"))
    );
    assert_eq!(
        (&fields["emergency"], &fields["language"]),
        (&json!("false"), &json!("en"))
    );
    assert!(
        !payload.to_string().contains("water"),
        "no homeowner words leave"
    );
    let week = weekly_counts(&mut db).await.unwrap();
    let expected = WeeklyCounts {
        calls: 1,
        ended: 0,
        readings: 1,
        emergencies: 0,
        spanish: 0,
    };
    assert_eq!(week, expected);
    let rewrite = sqlx::query("UPDATE ai.switch_history SET turned_on = true")
        .execute(&mut *db)
        .await;
    let code = rewrite
        .unwrap_err()
        .into_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned();
    assert_eq!(code, askhava_integration::PERMISSION_DENIED);
}
