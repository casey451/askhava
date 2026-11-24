//! "Delete my data" also deletes her kept message words (PRIV-011, PRIV-006, PRIV-030; T757,
//! D-4765).
//!
//! On a fresh database, as `askhava_app`: a homeowner's problem words are kept under her job and
//! her help-request words under each request's own ID (`open_help` in `intake`). After
//! `delete_my_data` no `privacy.message_body` row of her jobs or her help requests remains,
//! another homeowner's rows and a business's request under the same ID stay, and the
//! confirmation is the PRIV-011 sentence. Every name, phone and word is fake.

use askhava_integration::set_app_role;
use askhava_ops::api::{Asker, HelpLimits, HelpOpened, open_help_request};
use askhava_privacy::api::{DELETE_CONFIRMATION, NewContact, Vault, delete_my_data, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const HER_JOB: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0757_0001);
const OTHER_JOB: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0757_0002);
const HOUR: SignedDuration = SignedDuration::from_hours(1);

fn vault() -> Vault {
    let key = |byte: u8| SecretString::from(format!("1:{}", STANDARD.encode([byte; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid:5432/askhava".to_owned());
    Vault::new(&key(0x57), &key(0x75), &url).expect("vault keys")
}

async fn app(pool: &PgPool) -> PgConnection {
    let mut conn = pool.acquire().await.expect("connection").detach();
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

async fn contact(pool: &PgPool, name: &str, phone: &str) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from(name.to_owned())),
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    store_contact(&mut app(pool).await, &vault(), &new)
        .await
        .expect("contact")
}

/// Opens a help request for `asker` at `at` and returns its ID.
async fn ask(pool: &PgPool, asker: Asker, at: Timestamp) -> Uuid {
    let key = (Uuid::new_v4(), at, at + HOUR * 24);
    let opened = open_help_request(&mut app(pool).await, asker, key, HelpLimits::default())
        .await
        .expect("help request");
    let HelpOpened::Opened { request, .. } = opened else {
        panic!("a new request was expected");
    };
    request
}

/// Keeps one fake word row under `key` (a job or a help request ID).
async fn keep(pool: &PgPool, key: Uuid) {
    sqlx::query(
        "INSERT INTO privacy.message_body (inbox_id, job_id, words, safety, created_at) \
         VALUES (gen_random_uuid(), $1, 'fake words about a leaky tap', false, now())",
    )
    .bind(key)
    .execute(pool)
    .await
    .expect("kept words");
}

async fn kept(pool: &PgPool, keys: &[Uuid]) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM privacy.message_body WHERE job_id = ANY($1)")
        .bind(keys)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// PRIV-011, PRIV-006, PRIV-030, D-4765: after "Delete my data" none of her kept words remain,
/// under her job or under any help request she asked, open or closed; another homeowner's words
/// and a business's request stay, and the confirmation is unchanged.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn delete_my_data_deletes_her_kept_words_and_no_one_elses(pool: PgPool) {
    let her = contact(&pool, "Fakey Testerson", "(928) 555-0157").await;
    let other = contact(&pool, "Mocky Placeholder", "(928) 555-0158").await;
    let start = Timestamp::now() - HOUR * 6;
    let first = ask(&pool, Asker::Homeowner(her), start).await;
    let second = ask(&pool, Asker::Homeowner(her), start + HOUR * 2).await;
    sqlx::query("UPDATE ops.help_request SET closed_at = now() WHERE id = $1")
        .bind(first)
        .execute(&pool)
        .await
        .expect("closed");
    let theirs = ask(&pool, Asker::Homeowner(other), start).await;
    let business = ask(&pool, Asker::Business(her), start).await;
    let hers = [HER_JOB, first, second];
    let others = [OTHER_JOB, theirs, business];
    for key in hers.iter().chain(&others) {
        keep(&pool, *key).await;
    }
    keep(&pool, first).await;
    let said = delete_my_data(&mut app(&pool).await, her, &[HER_JOB]).await;
    assert_eq!(said, Ok(DELETE_CONFIRMATION));
    assert_eq!(
        DELETE_CONFIRMATION,
        "We keep your phone number, locked, for 5 years only as proof of your texting permission and opt-out."
    );
    assert_eq!(kept(&pool, &hers).await, 0, "none of her words remain");
    assert_eq!(kept(&pool, &others).await, 3, "no one else's words go");
}

/// PRIV-011, D-4765: a homeowner with no help request and no job still gets the confirmation, and
/// another homeowner's words stay.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn delete_my_data_with_nothing_kept_leaves_the_rest(pool: PgPool) {
    let her = contact(&pool, "Fakey Testerson", "(928) 555-0157").await;
    let other = contact(&pool, "Mocky Placeholder", "(928) 555-0158").await;
    let theirs = ask(&pool, Asker::Homeowner(other), Timestamp::now()).await;
    keep(&pool, theirs).await;
    keep(&pool, OTHER_JOB).await;
    let said = delete_my_data(&mut app(&pool).await, her, &[]).await;
    assert_eq!(said, Ok(DELETE_CONFIRMATION));
    assert_eq!(kept(&pool, &[theirs, OTHER_JOB]).await, 2);
}
