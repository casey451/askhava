//! The text outbox and the sender directory (TD-TXT-03, TD-TXT-04; TXT-009, TXT-011, D-492).
//!
//! Proves on fresh databases that a worker killed after the carrier accepted a text but before
//! the row was marked leaves it unsent, and the resend carries the same key, so the fake carrier
//! delivers it once; that the same text queued twice is one row and a sent row never reaches the
//! carrier again; that no text is queued to a past customer unless she wrote after her request
//! closed, nor sent to one; that a homeowner's number comes from the vault and a sent row keeps no
//! body (D-849, D-850); that a row goes only to the recipient kind it was queued for and the
//! order of its subject IDs does not change its key; and that the sender directory matches a
//! linked business phone by its blind index only.
//! The fake carrier keeps Twilio's rule for a repeated `Idempotency-Key`. All data is fake.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, IdempotencyKey, SentText, SmsAdapter};
use askhava_messaging::api::{
    NewText, OutboxError, Relation, To, enqueue, send, sender_add, sender_business, sender_remove,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const TO: &str = "+19285550142";
const FROM: &str = "+19285550100";

/// A fake carrier: one delivery per idempotency key, a repeat gets the first SID back. With
/// `die_after_accept` it accepts, then the worker dies before the answer is used.
#[derive(Clone, Default)]
struct FakeCarrier {
    delivered: Arc<Mutex<HashMap<String, String>>>,
    calls: Arc<Mutex<Vec<String>>>,
    numbers: Arc<Mutex<Vec<String>>>,
    die_after_accept: bool,
}

impl SmsAdapter for FakeCarrier {
    async fn send_text(
        &self,
        key: &IdempotencyKey,
        to: &str,
        _from: &str,
        _body: &str,
    ) -> Result<SentText, AdapterError> {
        self.numbers.lock().expect("lock").push(to.to_owned());
        self.calls
            .lock()
            .expect("lock")
            .push(key.as_str().to_owned());
        let sid = {
            let mut delivered = self.delivered.lock().expect("lock");
            let next = format!("SM{:032}", delivered.len() + 1);
            delivered
                .entry(key.as_str().to_owned())
                .or_insert(next)
                .clone()
        };
        assert!(
            !self.die_after_accept,
            "worker killed after the carrier accepted"
        );
        Ok(SentText {
            sid,
            status: "queued".to_owned(),
        })
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x51), &key(0x61), &url).expect("vault")
}

async fn queue(pool: &PgPool, relation: Relation, job: Uuid) -> Result<(Uuid, bool), OutboxError> {
    queue_to(pool, (Uuid::from_u128(0x0c0), relation), &[job]).await
}

async fn queue_to(
    pool: &PgPool,
    (recipient, relation): (Uuid, Relation),
    subject: &[Uuid],
) -> Result<(Uuid, bool), OutboxError> {
    let new = NewText {
        template_id: "first_quote",
        subject,
        recipient,
        relation,
        body: "Ask Hava: fake body",
    };
    let mut db = pool.acquire().await.expect("connection");
    enqueue(&mut db, &secret(), &new).await
}

async fn status(pool: &PgPool, id: Uuid) -> (String, i32) {
    sqlx::query_as("SELECT status, attempts FROM messaging.outbox WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("row")
}

/// TD-TXT-04, TXT-011: a worker killed after the carrier accepted, before the mark; the resend
/// carries the same key and the fake carrier delivers once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_04_a_killed_worker_resends_under_the_same_key_and_delivers_once(pool: PgPool) {
    let (id, _) = queue(&pool, Relation::Business, Uuid::from_u128(1))
        .await
        .expect("queued");
    let dying = FakeCarrier {
        die_after_accept: true,
        ..FakeCarrier::default()
    };
    let (worker_pool, carrier) = (pool.clone(), dying.clone());
    let killed = tokio::spawn(async move {
        let mut db = worker_pool.acquire().await.expect("connection");
        send(&mut db, (&secret(), &carrier), id, (To::Business(TO), FROM)).await
    });
    assert!(killed.await.is_err(), "the first worker dies mid-send");
    assert_eq!(status(&pool, id).await, ("sending".to_owned(), 1));
    let carrier = FakeCarrier {
        die_after_accept: false,
        ..dying
    };
    let mut db = pool.acquire().await.expect("connection");
    let sent = send(&mut db, (&secret(), &carrier), id, (To::Business(TO), FROM))
        .await
        .expect("resend");
    assert_eq!(sent.map(|s| s.sid), Some(format!("SM{:032}", 1)));
    let calls = carrier.calls.lock().expect("lock").clone();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0], calls[1], "the resend carries the same key");
    assert_eq!(
        carrier.delivered.lock().expect("lock").len(),
        1,
        "delivered once"
    );
    assert_eq!(status(&pool, id).await, ("sent".to_owned(), 2));
}

/// TD-TXT-04, TXT-011: the same text queued twice is one row; a sent row never reaches the carrier.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_04_a_text_is_queued_once_and_never_sent_twice(pool: PgPool) {
    let job = Uuid::from_u128(2);
    let (id, new) = queue(&pool, Relation::Business, job).await.expect("first");
    assert!(new);
    assert_eq!(queue(&pool, Relation::Business, job).await, Ok((id, false)));
    let (other, _) = queue(&pool, Relation::Business, Uuid::from_u128(3))
        .await
        .expect("other");
    assert_ne!(other, id, "another job is another text");
    let carrier = FakeCarrier::default();
    let mut db = pool.acquire().await.expect("connection");
    for _ in 0..3 {
        send(&mut db, (&secret(), &carrier), id, (To::Business(TO), FROM))
            .await
            .expect("send");
    }
    assert_eq!(
        carrier.calls.lock().expect("lock").len(),
        1,
        "sent rows are skipped"
    );
    let missing = send(
        &mut db,
        (&secret(), &carrier),
        Uuid::max(),
        (To::Business(TO), FROM),
    )
    .await;
    assert_eq!(missing.err(), Some(OutboxError::NotFound));
}

/// TD-TXT-03, TXT-009: no text to a past customer who has not written since her request closed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_03_no_text_to_a_past_customer_who_has_not_written_first(pool: PgPool) {
    let closed_at = Timestamp::from_second(1_790_000_000).expect("time");
    let past = |last_inbound| Relation::PastCustomer {
        closed_at,
        last_inbound,
    };
    let second = SignedDuration::from_secs(1);
    for (n, inbound) in [None, Some(closed_at - second), Some(closed_at)]
        .into_iter()
        .enumerate()
    {
        let refused = queue(&pool, past(inbound), Uuid::from_u128(10 + n as u128)).await;
        assert_eq!(refused, Err(OutboxError::PastCustomer));
    }
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM messaging.outbox")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(rows, 0, "nothing queued for a past customer");
    let wrote = queue(&pool, past(Some(closed_at + second)), Uuid::from_u128(20)).await;
    assert!(wrote.is_ok(), "she wrote first");
    for relation in [Relation::OpenRequest, Relation::Business] {
        assert!(queue(&pool, relation, Uuid::from_u128(21)).await.is_ok());
    }
}

/// TD-TXT-03, TXT-009, D-849, D-850: a homeowner is re-checked at send time, her number is read
/// from the vault for the row's recipient, and a sent row keeps no body.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_03_a_homeowner_is_rechecked_and_texted_at_her_vault_number(pool: PgPool) {
    let vault = vault();
    let mut db = pool.acquire().await.expect("connection");
    let contact = NewContact {
        name: Some(SecretString::from("Zelda Canary".to_owned())),
        phone: Some(SecretString::from("928-555-0142".to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(&mut db, &vault, &contact)
        .await
        .expect("contact");
    let (id, _) = queue_to(&pool, (her, Relation::OpenRequest), &[Uuid::from_u128(30)])
        .await
        .expect("queued");
    let closed_at = Timestamp::from_second(1_790_000_000).expect("time");
    let carrier = FakeCarrier::default();
    let past = Relation::PastCustomer {
        closed_at,
        last_inbound: None,
    };
    let to = |relation| To::Homeowner {
        vault: &vault,
        relation,
    };
    let refused = send(&mut db, (&secret(), &carrier), id, (to(past), FROM)).await;
    assert_eq!(refused.err(), Some(OutboxError::PastCustomer));
    assert_eq!(status(&pool, id).await, ("queued".to_owned(), 0));
    let open = to(Relation::OpenRequest);
    send(&mut db, (&secret(), &carrier), id, (open, FROM))
        .await
        .expect("send");
    assert_eq!(*carrier.numbers.lock().expect("lock"), vec![TO.to_owned()]);
    let body: Option<String> =
        sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("row");
    assert_eq!(body, None, "a sent row keeps no body");
    let (stranger, _) = queue(&pool, Relation::OpenRequest, Uuid::from_u128(31))
        .await
        .expect("queued");
    let unknown = send(&mut db, (&secret(), &carrier), stranger, (open, FROM)).await;
    assert_eq!(unknown.err(), Some(OutboxError::Recipient));
}

/// D-849, TXT-011: a row goes only to the recipient kind it was queued for, a refused send leaves
/// it queued and untouched, and the order of the subject IDs does not change the text's key.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_row_goes_only_to_its_recipient_kind_and_subject_order_is_one_text(pool: PgPool) {
    let vault = vault();
    let carrier = FakeCarrier::default();
    let (homeowner_row, _) = queue(&pool, Relation::OpenRequest, Uuid::from_u128(40))
        .await
        .expect("queued");
    let (business_row, _) = queue(&pool, Relation::Business, Uuid::from_u128(41))
        .await
        .expect("queued");
    let mut db = pool.acquire().await.expect("connection");
    let to_business = (To::Business(TO), FROM);
    let wrong = send(&mut db, (&secret(), &carrier), homeowner_row, to_business).await;
    assert_eq!(wrong.err(), Some(OutboxError::RecipientKind));
    let homeowner = To::Homeowner {
        vault: &vault,
        relation: Relation::OpenRequest,
    };
    let wrong = send(
        &mut db,
        (&secret(), &carrier),
        business_row,
        (homeowner, FROM),
    )
    .await;
    assert_eq!(wrong.err(), Some(OutboxError::RecipientKind));
    assert!(
        carrier.calls.lock().expect("lock").is_empty(),
        "nothing sent"
    );
    for row in [homeowner_row, business_row] {
        assert_eq!(status(&pool, row).await, ("queued".to_owned(), 0));
    }
    let (a, b) = (Uuid::from_u128(42), Uuid::from_u128(43));
    let to = (Uuid::from_u128(0x0c0), Relation::Business);
    let (first, _) = queue_to(&pool, to, &[a, b]).await.expect("first");
    let second = queue_to(&pool, to, &[b, a]).await;
    assert_eq!(second, Ok((first, false)), "one text whatever the order");
}

/// D-492, PRIV-024: a linked phone matches its business by blind index only; a change, a
/// removal and another business's claim behave.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_sender_directory_matches_linked_phones_only(pool: PgPool) {
    let vault = vault();
    let mut db = pool.acquire().await.expect("connection");
    let (business, other, phone) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
    sender_add(&mut db, &vault, (business, phone), "928-555-0150")
        .await
        .expect("add");
    let found = sender_business(&mut db, &vault, "(928) 555-0150").await;
    assert_eq!(found, Ok(Some(business)));
    let taken = sender_add(&mut db, &vault, (other, Uuid::from_u128(4)), "9285550150").await;
    assert_eq!(taken, Err(OutboxError::SenderTaken));
    sender_add(&mut db, &vault, (business, phone), "928-555-0151")
        .await
        .expect("change");
    assert_eq!(
        sender_business(&mut db, &vault, "928-555-0150").await,
        Ok(None)
    );
    let stored: String =
        sqlx::query_scalar("SELECT string_agg(d::text, ',') FROM messaging.sender_directory d")
            .fetch_one(&mut *db)
            .await
            .expect("rows");
    assert!(!stored.contains("5550151"), "no plaintext number is stored");
    sender_remove(&mut db, phone).await.expect("remove");
    assert_eq!(
        sender_business(&mut db, &vault, "928-555-0151").await,
        Ok(None)
    );
    assert_eq!(
        sender_business(&mut db, &vault, "555").await,
        Err(OutboxError::BadPhone)
    );
}
