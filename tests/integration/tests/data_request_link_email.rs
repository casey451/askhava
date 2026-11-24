//! PRIV-031, TXT-018, SEC-028, CON-035 (T816, D-5339, D-5340, D-5670): the data-request link
//! email goes only to a confirmed update address, once, never the conversation email, and is
//! dropped when the address is replaced before send; no stored row holds an address. PostgreSQL
//! with a recording email stand-in; fake data only.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, Email, EmailAdapter, IdempotencyKey};
use askhava_messaging::api::{
    DATA_REQUEST_TEMPLATE, DailyLimit, DataLinkKind, EmailError, EmailSent, EmailTo, Language,
    LinkEmail, MailboxKind, PostalAddress, queue_data_request_link, send_email,
};
use askhava_privacy::api::{
    NewContact, UpdateAddress, Vault, confirm_update_address, store_contact, store_update_address,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const ID_KEY: &str = "fake-id-secret-for-tests-00000816";
const UPDATE_ADDRESS: &str = "sam.update@mail.test";
const CHAT_ADDRESS: &str = "sam.chat@mail.test";
const LINK: &str = "https://site.test/h/data/confirm/fake-token-816";
const REQUEST: Uuid = Uuid::from_u128(0x816_0001);
/// 10:00 a.m. Arizona.
const DAY: &str = "2026-11-02T17:00:00Z";

fn at(secs: i64) -> Timestamp {
    DAY.parse::<Timestamp>().expect("day") + SignedDuration::from_secs(secs)
}

fn key() -> SecretString {
    SecretString::from(ID_KEY.to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x81), &key(0x16), &url).expect("vault")
}

fn postal() -> PostalAddress {
    PostalAddress::new("PO Box 0816, Sample City, AZ 86400", MailboxKind::PoBox).expect("postal")
}

/// Fake homeowner `n` with a fake 555 phone and a conversation email; with `update`, her update
/// address, confirmed when `confirmed`.
async fn homeowner(db: &mut PgConnection, n: u8, update: Option<bool>) -> Uuid {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(format!(
            "+1928555{:04}",
            816 + u16::from(n)
        ))),
        email: Some(SecretString::from(CHAT_ADDRESS.to_owned())),
        address: None,
        onsite_contact: None,
    };
    let contact = store_contact(db, &vault(), &new).await.expect("contact");
    if let Some(confirmed) = update {
        let address = UpdateAddress::parse(UPDATE_ADDRESS).expect("address");
        let row = store_update_address(db, &vault(), contact, &address, at(0))
            .await
            .expect("update address");
        if confirmed {
            let done = confirm_update_address(db, row, at(1)).await;
            assert!(done.expect("confirm"));
        }
    }
    contact
}

async fn queue(
    db: &mut PgConnection,
    contact: Uuid,
    language: Language,
    kind: DataLinkKind,
) -> LinkEmail {
    let postal = postal();
    let link = (kind, REQUEST, LINK);
    queue_data_request_link(db, (&key(), &postal), (contact, language), link, at(10))
        .await
        .expect("queue")
}

/// Her stored emails: template, subject line and both parts.
async fn emails(pool: &PgPool, contact: Uuid) -> Vec<(String, String, String, String)> {
    sqlx::query_as(
        "SELECT template_id, coalesce(subject, ''), coalesce(html, ''), coalesce(plain, '') \
         FROM messaging.email_outbox WHERE recipient_id = $1 ORDER BY id",
    )
    .bind(contact)
    .fetch_all(pool)
    .await
    .expect("emails")
}

/// The email provider stand-in: records each address and answers with a fake ID.
#[derive(Clone, Default)]
struct Rec(Arc<Mutex<Vec<String>>>);

impl EmailAdapter for Rec {
    async fn send_email(
        &self,
        _: &IdempotencyKey,
        email: &Email<'_>,
    ) -> Result<String, AdapterError> {
        let mut to = self.0.lock().expect("lock");
        to.push(email.to.to_owned());
        Ok(format!("re_t816_{}", to.len()))
    }
    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

/// Sends email row `id` through the stand-in at `now`, its queue time a minute before.
async fn send(
    db: &mut PgConnection,
    mail: &Rec,
    id: Uuid,
    now: Timestamp,
) -> Result<EmailSent, EmailError> {
    let queued = now - SignedDuration::from_mins(1);
    sqlx::query("UPDATE messaging.email_outbox SET queued_at = $1, not_before = $1 WHERE id = $2")
        .bind(queued.to_sqlx())
        .bind(id)
        .execute(&mut *db)
        .await
        .expect("fixture: the email's queue time");
    let limit = DailyLimit::new(100, 90).expect("limit");
    let vault = vault();
    send_email(
        db,
        (&key(), mail, limit),
        id,
        EmailTo::Homeowner(&vault),
        now,
    )
    .await
}

/// No stored email row holds either fake address (PRIV-016).
async fn no_address_stored(pool: &PgPool) {
    let rows: Vec<String> =
        sqlx::query_scalar("SELECT row_to_json(e)::text FROM messaging.email_outbox e")
            .fetch_all(pool)
            .await
            .expect("rows");
    for row in rows {
        assert!(!row.contains(UPDATE_ADDRESS) && !row.contains(CHAT_ADDRESS));
    }
}

/// PRIV-031, TXT-018: a confirmed update address gets one English email with the link, the
/// SEC-028 footer and no code, once for a repeat, sent to the update address and never the
/// conversation email.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_confirmed_address_gets_one_email_with_the_link(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let contact = homeowner(&mut db, 1, Some(true)).await;
    let LinkEmail::Queued(id) = queue(&mut db, contact, Language::En, DataLinkKind::Delete).await
    else {
        panic!("queued");
    };
    let again = queue(&mut db, contact, Language::En, DataLinkKind::Delete).await;
    assert_eq!(again, LinkEmail::Queued(id), "a repeat stores nothing");
    let stored = emails(&pool, contact).await;
    assert_eq!(stored.len(), 1);
    let (template, subject, html, plain) = &stored[0];
    assert_eq!(template, DATA_REQUEST_TEMPLATE);
    assert_eq!(subject, "Confirm your data request");
    assert!(plain.contains("You asked us to delete your data."));
    assert!(plain.contains(LINK) && html.contains(LINK));
    assert!(plain.contains("works once and runs out in 15 minutes"));
    assert!(plain.contains("If you didn't ask, just ignore this email."));
    assert!(plain.contains("because you asked") && plain.contains("PO Box 0816"));
    assert!(!plain.to_lowercase().contains("code"));
    let mail = Rec::default();
    let sent = send(&mut db, &mail, id, at(60)).await;
    assert!(matches!(sent, Ok(EmailSent::Sent(_))), "{sent:?}");
    assert_eq!(
        *mail.0.lock().expect("lock"),
        vec![UPDATE_ADDRESS.to_owned()]
    );
    no_address_stored(&pool).await;
}

/// PRIV-031: an unconfirmed or missing update address answers `NoConfirmedAddress` and stores
/// nothing; the conversation email is never a fallback.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_unconfirmed_or_missing_address_stores_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let unconfirmed = homeowner(&mut db, 3, Some(false)).await;
    let missing = homeowner(&mut db, 4, None).await;
    for contact in [unconfirmed, missing] {
        let queued = queue(&mut db, contact, Language::En, DataLinkKind::Copy).await;
        assert_eq!(queued, LinkEmail::NoConfirmedAddress);
        assert!(emails(&pool, contact).await.is_empty());
    }
}

/// TXT-003, TXT-039, D-5340: a Spanish copy link is in Spanish, and an address replaced between
/// queue and send (its `confirmed_at` cleared) drops the email unsent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_spanish_link_for_a_replaced_address_is_never_sent(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let contact = homeowner(&mut db, 6, Some(true)).await;
    let LinkEmail::Queued(id) = queue(&mut db, contact, Language::Es, DataLinkKind::Copy).await
    else {
        panic!("queued");
    };
    let (_, subject, _, plain) = &emails(&pool, contact).await[0];
    assert_eq!(subject, "Confirme su solicitud de datos");
    assert!(plain.contains("Usted pidió una copia de sus datos.") && plain.contains(LINK));
    let fresh = UpdateAddress::parse("sam.fresh@mail.test").expect("address");
    store_update_address(&mut db, &vault(), contact, &fresh, at(20))
        .await
        .expect("replaced");
    let mail = Rec::default();
    let dropped = send(&mut db, &mail, id, at(60)).await;
    assert_eq!(dropped, Err(EmailError::NoAddress));
    assert!(mail.0.lock().expect("lock").is_empty());
    no_address_stored(&pool).await;
}
