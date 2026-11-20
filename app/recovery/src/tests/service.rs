//! TD-OPS-04 (OPS-013, OPS-018, OPS-019; D-3889, D-4932): the drill's replay-and-record step
//! against a fake Stripe list and an empty outbox, and its restore step's refusals. Stripe
//! unreachable is waited for over whole replays and never guessed; what the replay lists raises
//! one High alert; every drill that restored is one `recovery.restore_drill` row. The restore of
//! a real export is `tests/recovery/tests/local_drill.rs`. Fake data only.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use askhava_adapters::api::{AdapterError, TwilioConfig, TwilioSms, twilio_clients};
use askhava_core::api::Settings;
use askhava_messaging::api::{LaunchChannel, Line, Lines};
use askhava_ops::api::{alert_subject, digest_alerts};
use askhava_payments::api::{ChargePage, StripeCharge};
use askhava_privacy::api::Vault;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::api::{
    ChargeSource, Drill, DrillError, DrillReport, ItemKind, REPLAY_LISTED_ALERT,
    RESTORE_FAILED_ALERT, ReplaySources, RestoreError, Retry, run_drill,
};
use crate::models::RecoveryError;
use crate::service::replay_recorded;

const QUICK: Retry = Retry {
    tries: 2,
    wait: Duration::from_millis(5),
};

/// Stripe's list: unreachable for the first `fails` calls, then one page of `charges`.
struct Stripe {
    calls: AtomicU32,
    fails: u32,
    charges: Vec<StripeCharge>,
}

impl ChargeSource for Stripe {
    async fn list_page(
        &self,
        _: Timestamp,
        _: Option<&str>,
        _: usize,
    ) -> Result<ChargePage, AdapterError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) < self.fails {
            return Err(AdapterError::NotSent);
        }
        let charges = self.charges.clone();
        Ok(ChargePage {
            charges,
            next: None,
        })
    }
}

/// A fake Stripe down for `fails` calls that then lists one charge no pick holds, or none.
fn stripe(fails: u32, unmatched: bool) -> Stripe {
    let charge = StripeCharge {
        id: "ch_fakeunmatched".to_owned(),
        payment_intent: "pi_fake1".to_owned(),
        pick: Some(Uuid::from_u128(9)),
        amount_cents: 7500,
        refunded_cents: 0,
        dispute: None,
        balance_transaction: String::new(),
        fee_cents: 0,
    };
    Stripe {
        calls: AtomicU32::new(0),
        fails,
        charges: unmatched.then_some(charge).into_iter().collect(),
    }
}

/// A texting client at a closed port: the outbox is empty, so the replay never calls it.
fn sms() -> TwilioSms {
    let pool: Vec<String> = (0..10).map(|i| format!("+1500555{:04}", 100 + i)).collect();
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
        ("ASKHAVA_MASKED_NUMBERS", &pool.join(",")),
        ("ASKHAVA_TWILIO_BASE_URL", "http://127.0.0.1:1/twilio"),
        (
            "ASKHAVA_TWILIO_VERIFY_BASE_URL",
            "http://127.0.0.1:1/twilio",
        ),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let config = TwilioConfig::from_settings(&settings, pairs).expect("config");
    twilio_clients(&config).expect("clients").0
}

fn drill(url: &str) -> Drill {
    let dump = PathBuf::from("/nonexistent/fake-export.dump");
    let url = SecretString::from(url.to_owned());
    Drill::new(dump, "exports/2026-11-01/7.age", url, QUICK).expect("drill")
}

/// What a replay reads besides Stripe: the texting client, a fake vault and both lines.
struct Parts(TwilioSms, Vault, Lines);

fn parts() -> Parts {
    let key = |k: &str| SecretString::from(format!("1:{k}"));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let (a, b) = ("UVFR".repeat(10) + "UVE=", "YWFh".repeat(10) + "YWE=");
    let vault = Vault::new(&key(&a), &key(&b), &url).expect("vault");
    let line = || Line::at_launch(LaunchChannel::Sms, None);
    let lines = Lines {
        homeowner: line(),
        business: line(),
    };
    Parts(sms(), vault, lines)
}

impl Parts {
    fn sources<'a>(&'a self, stripe: &'a Stripe) -> ReplaySources<'a, TwilioSms, Stripe> {
        ReplaySources {
            sms: &self.0,
            stripe,
            vault: &self.1,
            lines: &self.2,
            charges_since: Timestamp::now(),
            retry: QUICK,
        }
    }
}

/// The replay-and-record step of a drill whose restore took 3 seconds.
async fn drilled(pool: &PgPool, stripe: &Stripe) -> Result<DrillReport, DrillError> {
    let parts = parts();
    let mut db = pool.acquire().await.expect("connection");
    let fake = drill("postgres://fake@localhost/fake");
    replay_recorded(
        &mut db,
        (&fake, 3),
        &parts.sources(stripe),
        Timestamp::now(),
    )
    .await
}

/// The one drill row as (replay run, restore seconds, outcome), and the subjects of the alerts
/// of `kind` raised in the last hour.
async fn recorded(
    pool: &PgPool,
    kind: &'static str,
) -> ((Option<Uuid>, Option<i32>, String), Vec<(String, Uuid)>) {
    let mut db = pool.acquire().await.expect("connection");
    let row = sqlx::query!(
        "SELECT replay_run_id, restore_seconds, outcome FROM recovery.restore_drill \
         WHERE backup_object = 'exports/2026-11-01/7.age'"
    )
    .fetch_one(&mut *db)
    .await
    .expect("one drill row");
    let hour = SignedDuration::from_hours(1);
    let span = (Timestamp::now() - hour, Timestamp::now() + hour);
    let alerts = digest_alerts(&mut db, &[kind], span.0, span.1).await;
    let alerts = alerts.expect("alerts");
    for alert in &alerts {
        let found = alert_subject(&mut db, alert.id).await.expect("alert");
        assert_eq!(found.map(|a| a.severity).as_deref(), Some("high"));
    }
    let about = alerts.into_iter().filter_map(|a| a.subject).collect();
    ((row.replay_run_id, row.restore_seconds, row.outcome), about)
}

/// TD-OPS-04 (OPS-018, OPS-019): Stripe down through a whole replay is waited for and the replay
/// run again; the charge no pick holds is flagged, raises one High alert about the run, and the
/// drill is recorded `ok` with its restore time.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_04_stripe_down_is_waited_for_and_the_listed_charge_alerts_once(pool: PgPool) {
    let down_thrice = stripe(3, true);
    let report = drilled(&pool, &down_thrice).await.expect("drill");
    assert_eq!(down_thrice.calls.load(Ordering::SeqCst), 4, "two replays");
    assert_eq!((report.restore_seconds, report.rounds), (3, 2));
    let kinds: Vec<ItemKind> = report.replay.items.iter().map(|i| i.kind).collect();
    assert_eq!(kinds, [ItemKind::ChargeUnmatched]);
    assert_eq!(
        report.replay.charges_booked, 0,
        "the replay charges nothing"
    );
    let run = report.replay.run;
    let (row, listed) = recorded(&pool, REPLAY_LISTED_ALERT).await;
    assert_eq!(row, (Some(run), Some(3), "ok".to_owned()));
    assert_eq!(listed, [("recovery.replay_run".to_owned(), run)]);
    let (_, failed) = recorded(&pool, RESTORE_FAILED_ALERT).await;
    assert!(failed.is_empty(), "a drill that passed raises no failure");
}

/// TD-OPS-04 (OPS-018, OPS-013): Stripe down through every round is never guessed: no replay
/// run is written, and the drill is recorded `failed` with one High alert about it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_04_stripe_down_every_round_fails_the_drill_and_guesses_nothing(pool: PgPool) {
    let down = stripe(u32::MAX, true);
    let failed = drilled(&pool, &down).await;
    assert_eq!(failed, Err(DrillError::Replay(RecoveryError::Vendor)));
    assert_eq!(down.calls.load(Ordering::SeqCst), 4, "two rounds of two");
    let ((run, seconds, outcome), about) = recorded(&pool, RESTORE_FAILED_ALERT).await;
    assert_eq!((run, seconds, outcome.as_str()), (None, Some(3), "failed"));
    assert_eq!(about.len(), 1, "one alert");
    assert_eq!(about[0].0, "recovery.restore_drill");
    let runs = sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM recovery.replay_run"#)
        .fetch_one(&pool)
        .await
        .expect("runs");
    assert_eq!(runs, 0, "nothing was guessed");
}

/// TD-OPS-04 (OPS-019): a replay that lists nothing records the drill and raises no alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_04_a_clean_replay_records_the_drill_with_no_alert(pool: PgPool) {
    let report = drilled(&pool, &stripe(0, false)).await.expect("drill");
    assert!(report.replay.items.is_empty());
    assert_eq!(report.rounds, 1, "one replay when no vendor failed");
    let (row, listed) = recorded(&pool, REPLAY_LISTED_ALERT).await;
    assert_eq!(row, (Some(report.replay.run), Some(3), "ok".to_owned()));
    assert!(listed.is_empty(), "nothing listed, no alert");
}

/// TD-OPS-04 (OPS-013): a bad object name, a URL that is not PostgreSQL's or has a malformed
/// password, and a dump that does not restore each fail the restore step, before any replay.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_04_a_restore_that_fails_ends_the_drill_before_the_replay(pool: PgPool) {
    let url = || SecretString::from("postgres://fake@localhost/fake".to_owned());
    let settings = Err(DrillError::Restore(RestoreError::Settings));
    for object in ["", "Exports/7.age", "exports/7 .age", &"a".repeat(201)] {
        let refused = Drill::new(PathBuf::new(), object, url(), QUICK);
        assert_eq!(
            refused.err().map(Err::<DrillReport, _>),
            Some(settings.clone())
        );
    }
    let (parts, down) = (parts(), stripe(u32::MAX, false));
    let from = parts.sources(&down);
    let restore = RestoreError::Restore;
    for (url, why) in [
        ("mysql://fake@localhost/fake", RestoreError::Settings),
        (
            "postgres://fake:%zz@127.0.0.1:1/fake",
            RestoreError::Settings,
        ),
        ("postgres://fake:f%40ke@127.0.0.1:1/fake", restore),
    ] {
        let failed = run_drill(&drill(url), &from, Timestamp::now()).await;
        assert_eq!(failed, Err(DrillError::Restore(why)));
    }
    assert_eq!(down.calls.load(Ordering::SeqCst), 0, "no replay ran");
    let drills = sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM recovery.restore_drill"#)
        .fetch_one(&pool)
        .await
        .expect("drills");
    assert_eq!(drills, 0, "nothing restored, nothing recorded");
}
