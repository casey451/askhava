//! TD-OPS-04 (OPS-015, OPS-016, OPS-017): the replay against the served stand-in (texts) and a
//! fake Stripe list (charges). A text sent before the restore whose outbox row lost its `sent`
//! state is found in Twilio's sent log by recipient, sender, body digest and send time, never by a
//! key, and marked sent, never resent; an ambiguous one is listed for Casey; an unreachable Stripe
//! is waited for; unmatched and refunded charges are flagged. Fake data only.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use askhava_adapters::api::{
    AdapterError, IdempotencyKey, SmsAdapter, SmsSentLog, TwilioConfig, TwilioSms, twilio_clients,
};
use askhava_core::api::Settings;
use askhava_messaging::api::{
    LaunchChannel, Line, Lines, NewText, Relation, SendingNumber, enqueue,
};
use askhava_payments::api::{ChargePage, StripeCharge};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::api::{ChargeSource, ItemKind, ReplayItem, ReplaySources, Retry, replay};
use crate::models::RecoveryError;

const FROM: &str = "+15005550006";
const HER: &str = "+15005550142";
const HIM: &str = "+15005550177";
const QUICK: Retry = Retry {
    tries: 3,
    wait: Duration::from_millis(5),
};

/// The stand-in server on a port it picked itself, killed on drop.
struct StandIn(Child, String);

impl Drop for StandIn {
    fn drop(&mut self) {
        let _killed = self.0.kill();
        let _reaped = self.0.wait();
    }
}

fn stand_in() -> StandIn {
    let mut child = Command::new("python3")
        .args(["-m", "stand_ins.server", "--port", "0"])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .stdout(Stdio::piped())
        .spawn()
        .expect("python3 stand-in");
    let mut line = String::new();
    BufReader::new(child.stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("listening line");
    let base = line.trim_end().strip_prefix("stand-ins listening on ");
    let base = base.expect("listening").to_owned();
    StandIn(child, base)
}

fn sms(base: &str) -> TwilioSms {
    let api = format!("{base}/twilio");
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
        ("ASKHAVA_TWILIO_BASE_URL", &api),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", &api),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let config = TwilioConfig::from_settings(&settings, pairs).expect("config");
    twilio_clients(&config).expect("clients").0
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests".to_owned())
}

fn vault() -> Vault {
    let key = |k: &str| SecretString::from(format!("1:{k}"));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let (a, b) = ("UVFR".repeat(10) + "UVE=", "YWFh".repeat(10) + "YWE=");
    Vault::new(&key(&a), &key(&b), &url).expect("vault")
}

fn lines() -> Lines {
    let line = |n: Option<&str>| {
        Line::at_launch(
            LaunchChannel::Sms,
            n.and_then(|n| SendingNumber::parse(n).ok()),
        )
    };
    Lines {
        homeowner: line(Some(FROM)),
        business: line(None),
    }
}

/// A homeowner with `phone`, stored in the vault.
async fn contact(pool: &PgPool, phone: &str) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from("Fake Homeowner".to_owned())),
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let mut db = pool.acquire().await.expect("connection");
    store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact")
}

/// One queued text to `who`, as the restored outbox holds it.
async fn queued(pool: &PgPool, (who, body): (Uuid, &str), n: u128) -> Uuid {
    let text = NewText {
        template_id: "first_quote",
        subject: &[Uuid::from_u128(n)],
        recipient: who,
        relation: Relation::OpenRequest,
        body,
    };
    let mut db = pool.acquire().await.expect("connection");
    enqueue(&mut db, &secret(), &text).await.expect("queued").0
}

/// Stripe's list: unreachable for the first `fails` calls, then the given charges.
struct FakeStripe(AtomicU32, u32, Vec<StripeCharge>);

impl ChargeSource for FakeStripe {
    async fn list_page(
        &self,
        _: Timestamp,
        _: Option<&str>,
        _: usize,
    ) -> Result<ChargePage, AdapterError> {
        if self.0.fetch_add(1, Ordering::SeqCst) < self.1 {
            return Err(AdapterError::NotSent);
        }
        Ok(ChargePage {
            charges: self.2.clone(),
            next: None,
        })
    }
}

fn charge(id: &str, refunded: i64) -> StripeCharge {
    StripeCharge {
        id: id.to_owned(),
        payment_intent: "pi_fake1".to_owned(),
        pick: Some(Uuid::from_u128(9)),
        amount_cents: 7500,
        refunded_cents: refunded,
        dispute: None,
        balance_transaction: String::new(),
        fee_cents: 0,
    }
}

async fn run(
    pool: &PgPool,
    (sms, stripe): (&TwilioSms, &FakeStripe),
) -> Result<crate::api::ReplayReport, RecoveryError> {
    let (vault, lines) = (vault(), lines());
    let from = ReplaySources {
        sms,
        stripe,
        vault: &vault,
        lines: &lines,
        charges_since: Timestamp::UNIX_EPOCH,
        retry: QUICK,
    };
    let mut db = pool.acquire().await.expect("connection");
    replay(&mut db, &from, Timestamp::now()).await
}

/// TD-OPS-04 (OPS-017, D-2764, D-3155): a text Twilio sent whose outbox row the restore lost as
/// `queued` is found without any key filter and marked sent, never resent; a second run changes
/// nothing; a row Twilio has no text for is left to the sender.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_04_a_text_sent_before_the_restore_is_found_and_not_resent(pool: PgPool) {
    let server = stand_in();
    let sms = sms(&server.1);
    let (her, him) = (contact(&pool, HER).await, contact(&pool, HIM).await);
    queued(&pool, (her, "Fake quote one"), 1).await;
    queued(&pool, (him, "Fake quote two"), 2).await;
    let key = IdempotencyKey::derive(&secret(), "sms.send", &["fake-row-1"]);
    let sent = sms.send_text(&key, HER, FROM, "Fake quote one").await;
    sent.expect("sent before the restore");
    let stripe = FakeStripe(AtomicU32::new(0), 0, Vec::new());
    let first = run(&pool, (&sms, &stripe)).await.expect("replay");
    assert_eq!((first.texts_marked, first.texts_unsent), (1, 1));
    assert!(first.items.is_empty());
    let again = run(&pool, (&sms, &stripe)).await.expect("second replay");
    assert_eq!((again.texts_marked, again.texts_unsent), (0, 1));
    assert_ne!(first.run, again.run);
    let day = SignedDuration::from_hours(24);
    let window = (Timestamp::now() - day, Timestamp::now() + day);
    let log = sms.list_sent(FROM, window).await.expect("sent log");
    assert_eq!(log.len(), 1, "nothing was sent twice");
}

/// TD-OPS-04 (OPS-017, D-2764): one listed text that fits two outbox rows is listed for Casey
/// for both and resends neither.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_04_an_ambiguous_match_is_listed_for_casey_and_not_resent(pool: PgPool) {
    let server = stand_in();
    let sms = sms(&server.1);
    let body = "Fake same body";
    let her = contact(&pool, HER).await;
    let a = queued(&pool, (her, body), 3).await;
    let key = IdempotencyKey::derive(&secret(), "sms.send", &["fake-row-3"]);
    sms.send_text(&key, HER, FROM, body).await.expect("sent");
    let b = queued(&pool, (her, body), 4).await;
    let stripe = FakeStripe(AtomicU32::new(0), 0, Vec::new());
    let report = run(&pool, (&sms, &stripe)).await.expect("replay");
    assert_eq!(report.texts_marked, 0);
    for row in [a, b] {
        let it = ReplayItem {
            kind: ItemKind::TextAmbiguous,
            subject: row.to_string(),
        };
        assert!(report.items.contains(&it));
    }
}

/// TD-OPS-04 (OPS-016, D-3889): Stripe unreachable is waited for and retried; an unmatched and a
/// refunded charge are booked nowhere and flagged in `recovery.replay_item`, which the app role
/// cannot change; Stripe unreachable through every retry fails the run.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_04_stripe_is_retried_and_unmatched_charges_are_flagged(pool: PgPool) {
    let sms = sms("http://127.0.0.1:1");
    let charges = vec![
        charge("ch_fakeunmatched", 0),
        charge("ch_fakerefunded", 7500),
    ];
    let stripe = FakeStripe(AtomicU32::new(0), 2, charges);
    let report = run(&pool, (&sms, &stripe)).await.expect("replay");
    assert_eq!((report.charges_booked, report.charges_already), (0, 0));
    let flagged = |kind, subject: &str| ReplayItem {
        kind,
        subject: subject.to_owned(),
    };
    assert_eq!(
        report.items,
        [
            flagged(ItemKind::ChargeUnmatched, "ch_fakeunmatched"),
            flagged(ItemKind::ChargeRefunded, "ch_fakerefunded")
        ]
    );
    let mut db = pool.acquire().await.expect("connection");
    sqlx::query!("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let kinds = sqlx::query_scalar!("SELECT kind FROM recovery.replay_item ORDER BY kind")
        .fetch_all(&mut *db)
        .await
        .expect("items");
    assert_eq!(kinds, ["charge_refunded", "charge_unmatched"]);
    let edit = sqlx::query!("UPDATE recovery.replay_item SET subject = 'ch_other'")
        .execute(&mut *db)
        .await;
    assert!(edit.is_err(), "append-only");
    let down = FakeStripe(AtomicU32::new(0), u32::MAX, Vec::new());
    let failed = run(&pool, (&sms, &down)).await;
    assert_eq!(failed.err(), Some(RecoveryError::Vendor));
}
