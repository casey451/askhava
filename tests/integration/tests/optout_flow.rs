//! The opt-out flow on a fresh database (TD-CON-05, TD-CON-13, TD-CON-20; CON-009, CON-013 to
//! CON-017, CON-030, CON-035).
//!
//! Proves through `messaging::api` alone that a STOP by text lists the number at once with its
//! channel and time and nothing else goes to it, even when the carrier has no block, until START;
//! that "yes" re-subscribes nothing; that the app queues exactly one bare CON-009 confirmation,
//! only for words the provider did not answer, which goes within 5 minutes and never later; that
//! the HELP reply still goes; that a later message sends nothing and an emergency in it raises a
//! critical alert; that a 21610 refusal blocks the row and is never retried; and that while
//! texting is paused an opt-out is recorded at once and never confirmed by a later text. The
//! provider's own replies are the fake Twilio's (T167). All numbers and data are fake.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, IdempotencyKey, SentText, SmsAdapter};
use askhava_messaging::api::{
    InboundText, Language, NewText, OutboxError, Outcome, ProviderKeyword, Relation, To, enqueue,
    on_inbound_text, render, send, send_checked,
};
use askhava_ops::api::{Actor, Switch, set_switch};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const FROM: &str = "+19285550100";
const NOW: i64 = 1_793_000_000;

/// A fake carrier with no block of its own; with `opted_out` it refuses every send with 21610.
#[derive(Clone, Default)]
struct FakeCarrier {
    sent: Arc<Mutex<Vec<(String, String)>>>,
    opted_out: bool,
}

impl SmsAdapter for FakeCarrier {
    async fn send_text(
        &self,
        _key: &IdempotencyKey,
        to: &str,
        _from: &str,
        body: &str,
    ) -> Result<SentText, AdapterError> {
        let mut sent = self.sent.lock().expect("lock");
        sent.push((to.to_owned(), body.to_owned()));
        if self.opted_out {
            return Err(AdapterError::Vendor {
                status: 400,
                code: Some(21610),
            });
        }
        Ok(SentText {
            sid: format!("SM{:032}", sent.len()),
            status: "queued".to_owned(),
        })
    }
}

impl FakeCarrier {
    fn calls(&self) -> usize {
        self.sent.lock().expect("lock").len()
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x52), &key(0x62), &url).expect("vault")
}

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(NOW + secs).expect("time")
}

async fn contact(db: &mut PgConnection, vault: &Vault, phone: &str) -> Uuid {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    store_contact(db, vault, &new).await.expect("contact")
}

/// One inbound text from `phone` (contact `her`), as the inbox handler would pass it.
fn inbound<'a>(
    her: Uuid,
    phone: &'a str,
    text: &'a str,
    provider: ProviderKeyword,
) -> InboundText<'a> {
    InboundText {
        inbox_id: Uuid::from_u128(0x1b0),
        contact: her,
        relation: Relation::OpenRequest,
        phone,
        text,
        provider,
        emergency: false,
    }
}

async fn on(db: &mut PgConnection, vault: &Vault, msg: &InboundText<'_>, secs: i64) -> Outcome {
    on_inbound_text(db, (&secret(), vault), msg, at(secs))
        .await
        .expect("inbound")
}

/// Queues template `template` for contact `her` about `job`.
async fn queue(db: &mut PgConnection, her: Uuid, template: &str, job: u128) -> Uuid {
    let new = NewText {
        template_id: template,
        subject: &[Uuid::from_u128(job)],
        recipient: her,
        relation: Relation::OpenRequest,
        body: "Ask Hava: fake body",
    };
    enqueue(db, &secret(), &new).await.expect("queued").0
}

async fn try_send(
    db: &mut PgConnection,
    (carrier, vault): (&FakeCarrier, &Vault),
    id: Uuid,
    secs: i64,
) -> Result<Option<SentText>, OutboxError> {
    let to = To::Homeowner {
        vault,
        relation: Relation::OpenRequest,
    };
    send_checked(db, (&secret(), carrier, vault), id, (to, FROM), at(secs)).await
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// TD-CON-20, CON-035, CON-013, CON-015, CON-030: STOP by text lists the number at once with its
/// channel and time; nothing goes to it, though the carrier has no block; "yes" re-subscribes
/// nothing; START does, and texts go again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_20_stop_lists_the_number_and_nothing_goes_until_start(pool: PgPool) {
    let (vault, carrier) = (vault(), FakeCarrier::default());
    let mut db = pool.acquire().await.expect("connection");
    let phone = "+19285550142";
    let her = contact(&mut db, &vault, phone).await;
    let stop = inbound(her, phone, "STOP", ProviderKeyword::OptOut);
    let none = Outcome::OptedOut {
        confirmation: None,
        email: false,
    };
    assert_eq!(
        on(&mut db, &vault, &stop, 0).await,
        none,
        "the provider confirmed it"
    );
    let (channel, detected, effective): (String, String, i64) = sqlx::query_as(
        "SELECT channel, detected, extract(epoch FROM effective_at)::bigint FROM messaging.optout_event",
    )
    .fetch_one(&pool)
    .await
    .expect("event");
    assert_eq!(
        (channel.as_str(), detected.as_str(), effective),
        ("text", "word:stop", NOW)
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM messaging.outbox").await,
        0
    );
    let quote = queue(&mut db, her, "first_quote", 1).await;
    let refused = try_send(&mut db, (&carrier, &vault), quote, 10).await;
    assert_eq!(refused.err(), Some(OutboxError::Suppressed));
    let yes = inbound(her, phone, "yes", ProviderKeyword::None);
    assert_eq!(
        on(&mut db, &vault, &yes, 20).await,
        Outcome::Suppressed { alert: None }
    );
    let refused = try_send(&mut db, (&carrier, &vault), quote, 30).await;
    assert_eq!(refused.err(), Some(OutboxError::Suppressed));
    assert_eq!(carrier.calls(), 0, "nothing reached the carrier");
    let start = inbound(her, phone, "START", ProviderKeyword::Start);
    assert_eq!(on(&mut db, &vault, &start, 40).await, Outcome::Resubscribed);
    assert!(
        try_send(&mut db, (&carrier, &vault), quote, 50)
            .await
            .expect("sent")
            .is_some()
    );
    let events = "SELECT count(*) FROM messaging.optout_event WHERE action = 'start'";
    assert_eq!(count(&pool, events).await, 1);
}

/// TD-CON-05, CON-009, CON-013, CON-016, CON-017: a word the provider does not answer gets
/// exactly one bare CON-009 confirmation from the app within 5 minutes; afterwards only the HELP
/// reply goes; a later message sends nothing and an emergency in it raises a critical alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_05_one_confirmation_then_only_help_until_start(pool: PgPool) {
    let (vault, carrier) = (vault(), FakeCarrier::default());
    let mut db = pool.acquire().await.expect("connection");
    let phone = "+19285550143";
    let her = contact(&mut db, &vault, phone).await;
    let opt_out = inbound(her, phone, "Opt out", ProviderKeyword::None);
    let Outcome::OptedOut {
        confirmation: Some(confirmation),
        email: false,
    } = on(&mut db, &vault, &opt_out, 0).await
    else {
        panic!("the app confirms a word the provider does not answer");
    };
    let again = inbound(her, phone, "STOP", ProviderKeyword::OptOut);
    on(&mut db, &vault, &again, 30).await;
    let once = try_send(&mut db, (&carrier, &vault), confirmation, 60).await;
    assert!(once.expect("sent").is_some());
    let twice = try_send(&mut db, (&carrier, &vault), confirmation, 90).await;
    assert_eq!(twice, Ok(None), "sent once");
    let con_009 = render("opt_out_confirmation", Language::En, false, &[]).expect("text");
    let sent = carrier.sent.lock().expect("lock").clone();
    assert_eq!(
        sent,
        vec![(phone.to_owned(), con_009.body)],
        "one bare confirmation"
    );
    let help = queue(&mut db, her, "help_reply", 2).await;
    assert!(
        try_send(&mut db, (&carrier, &vault), help, 3_600)
            .await
            .expect("help")
            .is_some()
    );
    let other = queue(&mut db, her, "first_quote", 3).await;
    let refused = try_send(&mut db, (&carrier, &vault), other, 3_600).await;
    assert_eq!(refused.err(), Some(OutboxError::Suppressed));
    let mut gas = inbound(her, phone, "I smell gas", ProviderKeyword::None);
    gas.emergency = true;
    let Outcome::Suppressed { alert: Some(_) } = on(&mut db, &vault, &gas, 3_700).await else {
        panic!("an emergency from a listed number raises an alert");
    };
    let critical = "SELECT count(*) FROM ops.alert WHERE severity = 'critical'";
    assert_eq!(count(&pool, critical).await, 1);
    assert_eq!(
        carrier.calls(),
        2,
        "no text but the confirmation and the HELP reply"
    );
    let start = inbound(her, phone, "unstop", ProviderKeyword::None);
    assert_eq!(
        on(&mut db, &vault, &start, 3_800).await,
        Outcome::Resubscribed
    );
}

/// TD-CON-05, CON-009, CON-030: a confirmation not sent within 5 minutes never goes; a Spanish
/// phrase is confirmed in Spanish by the app; a Spanish provider word ("parar") gets none from
/// the app, the provider having replied in Spanish.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_13_spanish_and_late_confirmations(pool: PgPool) {
    let (vault, carrier) = (vault(), FakeCarrier::default());
    let mut db = pool.acquire().await.expect("connection");
    let (phone, other) = ("+19285550144", "+19285550145");
    let her = contact(&mut db, &vault, phone).await;
    let spanish = inbound(
        her,
        phone,
        "Ya no quiero mensajes, gracias",
        ProviderKeyword::None,
    );
    let Outcome::OptedOut {
        confirmation: Some(confirmation),
        ..
    } = on(&mut db, &vault, &spanish, 0).await
    else {
        panic!("the app confirms a Spanish phrase");
    };
    let body: String = sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE id = $1")
        .bind(confirmation)
        .fetch_one(&pool)
        .await
        .expect("row");
    let es = render("opt_out_confirmation", Language::Es, false, &[]).expect("text");
    assert_eq!(body, es.body);
    let late = try_send(&mut db, (&carrier, &vault), confirmation, 301).await;
    assert_eq!(
        late.err(),
        Some(OutboxError::Suppressed),
        "never a late confirmation"
    );
    let him = contact(&mut db, &vault, other).await;
    let parar = inbound(him, other, "PARAR", ProviderKeyword::OptOut);
    let none = Outcome::OptedOut {
        confirmation: None,
        email: false,
    };
    assert_eq!(on(&mut db, &vault, &parar, 0).await, none);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM messaging.outbox").await,
        1
    );
    assert_eq!(carrier.calls(), 0);
}

/// TD-CON-13, CON-030: while texting is paused an opt-out is recorded at once, the caller is told
/// to confirm by email, and no confirmation text is queued, so none goes when texting returns.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_13_an_opt_out_while_texting_is_paused_is_never_confirmed_by_text(pool: PgPool) {
    let vault = vault();
    let mut db = pool.acquire().await.expect("connection");
    let phone = "+19285550146";
    let her = contact(&mut db, &vault, phone).await;
    set_switch(&mut db, Switch::Texting, true, Actor::System)
        .await
        .expect("paused");
    let msg = inbound(her, phone, "remove me from this", ProviderKeyword::None);
    let email = Outcome::OptedOut {
        confirmation: None,
        email: true,
    };
    assert_eq!(on(&mut db, &vault, &msg, 0).await, email);
    set_switch(&mut db, Switch::Texting, false, Actor::System)
        .await
        .expect("on");
    assert_eq!(
        count(&pool, "SELECT count(*) FROM messaging.suppression").await,
        1
    );
    let detected =
        "SELECT count(*) FROM messaging.optout_event WHERE detected = 'phrase:en.remove_me'";
    assert_eq!(count(&pool, detected).await, 1);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM messaging.outbox").await,
        0
    );
}

/// TD-CON-05, CON-014: a send the carrier refuses with 21610 lists the number, blocks the row and
/// is never retried, by the checked send or the raw outbox send.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_05_a_21610_refusal_is_never_retried(pool: PgPool) {
    let vault = vault();
    let carrier = FakeCarrier {
        opted_out: true,
        ..FakeCarrier::default()
    };
    let mut db = pool.acquire().await.expect("connection");
    let her = contact(&mut db, &vault, "+19285550147").await;
    let id = queue(&mut db, her, "first_quote", 4).await;
    let blocked = try_send(&mut db, (&carrier, &vault), id, 0).await;
    assert_eq!(blocked.err(), Some(OutboxError::Blocked));
    let (status, body): (String, Option<String>) =
        sqlx::query_as("SELECT status, body FROM messaging.outbox WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("row");
    assert_eq!((status.as_str(), body), ("blocked", None));
    let carrier_events = "SELECT count(*) FROM messaging.optout_event WHERE channel = 'carrier'";
    assert_eq!(count(&pool, carrier_events).await, 1);
    let again = try_send(&mut db, (&carrier, &vault), id, 60).await;
    assert_eq!(again.err(), Some(OutboxError::Suppressed));
    let to = To::Homeowner {
        vault: &vault,
        relation: Relation::OpenRequest,
    };
    let raw = send(&mut db, (&secret(), &carrier), id, (to, FROM)).await;
    assert_eq!(raw, Ok(None), "a blocked row is never claimed");
    assert_eq!(carrier.calls(), 1, "never retried");
}
