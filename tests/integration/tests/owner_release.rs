//! The owner's release button (T540; TD-PICK-41's integration half; PICK-022, PICK-023, SEC-006;
//! D-074, D-3890, D-3892).
//!
//! Driven through `portals::api::owner_release_resend` with a signed-in owner: a released pick's
//! press queues one `messaging.business_notice` task under the press's dedupe key, writes one
//! audit row and no release row, and the same press again queues nothing; a pick with no
//! `picks.release` row (paid or not, in safe mode or paused by the homeowner's CANCEL), an unknown
//! number, a malformed number or press, and a signed-out jar are refused and queue nothing; only
//! the job's newest release is re-sent. PostgreSQL through `#[sqlx::test]`; fake IDs only, and no
//! contact detail is seeded.

use askhava_ops::api::Actor;
use askhava_picks::api::{BUSINESS_NOTICE_KIND, RELEASE_RESEND_ACTION, resend_key};
use askhava_portals::api::{
    CookieJar, OwnerAuth, OwnerToolError, ReleaseForm, ReleaseScreen, SecondFactor,
    bootstrap_owner, owner_release_resend, owner_sign_in,
};
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const PASSWORD: &str = "fake owner pass phrase 2026";
const TOKEN: &str = "fake-bootstrap-token-0123456789abcdef";
const NOTICES: &str = "SELECT count(*) FROM core.task WHERE kind = $1";
const RELEASES: &str = "SELECT count(*) FROM picks.release";
const AUDITS: &str = "SELECT count(*) FROM ops.audit_log WHERE action = $1";

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_010).expect("time") + SignedDuration::from_secs(secs)
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `now`.
fn totp_code(secret: &[u8], now: Timestamp) -> String {
    let step = u64::try_from(now.as_second() / 30).expect("step");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[offset..offset + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

/// The bootstrapped owner signed in at 0 with a TOTP code; the session's jar.
async fn signed_in(db: &mut PgConnection) -> CookieJar {
    bootstrap_owner(db, Some(TOKEN), PASSWORD, at(0))
        .await
        .expect("bootstrap");
    let secret: Vec<u8> = sqlx::query_scalar("SELECT totp_secret FROM portals.owner_factor")
        .fetch_one(&mut *db)
        .await
        .expect("secret");
    let code = totp_code(&secret, at(0));
    let auth = OwnerAuth::new("askhava.test", "https://askhava.test").expect("relying party");
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    owner_sign_in(db, CookieJar::new(), factor, &auth, at(0))
        .await
        .expect("signed in")
}

/// A fake business with an owner whose mailbox gets pick-and-release notices; the business ID.
async fn business(db: &mut PgConnection) -> Uuid {
    let id = askhava_businesses::api::create(db, false, at(0))
        .await
        .expect("business");
    sqlx::query(
        "INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes, \
         confirm_and_billing, added_at) VALUES ($1, gen_random_uuid(), true, true, true, $2)",
    )
    .bind(id)
    .bind(at(0).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("owner member");
    id
}

/// A new job and its number.
async fn job(db: &mut PgConnection) -> (Uuid, i64) {
    let job = askhava_jobs::api::create(db, at(0), false, Actor::System)
        .await
        .expect("job");
    let number = askhava_jobs::api::job_number(db, job)
        .await
        .expect("number");
    (job, number)
}

/// A pick of `business` on `job` with `status`, released at `released` seconds when given.
async fn pick(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    status: &str,
    released: Option<i64>,
) -> Uuid {
    let pick: Uuid = sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', $3, $4) RETURNING id",
    )
    .bind(job)
    .bind(business)
    .bind(status)
    .bind(at(0).to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    if let Some(secs) = released {
        sqlx::query(
            "INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, \
             contact_id, mail_chosen) VALUES ($1, $2, $3, 'paid', $4, gen_random_uuid(), false)",
        )
        .bind(pick)
        .bind(job)
        .bind(business)
        .bind(at(secs).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("release");
    }
    pick
}

async fn count(pool: &PgPool, sql: &'static str, arg: Option<&str>) -> i64 {
    let query = sqlx::query_scalar(sql);
    let query = match arg {
        Some(arg) => query.bind(arg.to_owned()),
        None => query,
    };
    query.fetch_one(pool).await.expect("count")
}

async fn press(pool: &PgPool, jar: &CookieJar, job: &str, press: &str) -> ReleaseScreen {
    let mut db = pool.acquire().await.expect("conn");
    let form = ReleaseForm {
        job: job.to_owned(),
        press: press.to_owned(),
    };
    owner_release_resend(&mut db, jar, &form, at(60))
        .await
        .expect("answered")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_41_a_released_pick_is_resent_once_per_press_and_never_released_again(
    pool: PgPool,
) {
    let mut db = pool.acquire().await.expect("conn");
    let jar = signed_in(&mut db).await;
    let business = business(&mut db).await;
    let (job, number) = job(&mut db).await;
    let pick = pick(&mut db, (job, business), "paid", Some(10)).await;
    let first = Uuid::new_v4();
    let typed = number.to_string();
    let screen = press(&pool, &jar, &typed, &first.to_string()).await;
    assert_eq!(screen, ReleaseScreen::Sent(number));
    let key = resend_key(pick, first);
    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1 AND dedupe_key = $2")
            .bind(BUSINESS_NOTICE_KIND)
            .bind(&key)
            .fetch_one(&mut *db)
            .await
            .expect("one notice under the press's key");
    assert_eq!(payload["pick"], serde_json::json!(pick));
    assert_eq!(payload["values"], serde_json::json!({ "job": number }));
    assert!(!payload.to_string().contains("phone"), "PICK-023");
    assert_eq!(count(&pool, NOTICES, Some(BUSINESS_NOTICE_KIND)).await, 1);
    assert_eq!(count(&pool, RELEASES, None).await, 1, "no release row made");
    assert_eq!(count(&pool, AUDITS, Some(RELEASE_RESEND_ACTION)).await, 1);
    let again = press(&pool, &jar, &typed, &first.to_string()).await;
    assert_eq!(again, ReleaseScreen::Repeat);
    assert_eq!(count(&pool, NOTICES, Some(BUSINESS_NOTICE_KIND)).await, 1);
    assert_eq!(count(&pool, AUDITS, Some(RELEASE_RESEND_ACTION)).await, 1);
    let second = press(&pool, &jar, &typed, &Uuid::new_v4().to_string()).await;
    assert_eq!(second, ReleaseScreen::Sent(number));
    assert_eq!(count(&pool, NOTICES, Some(BUSINESS_NOTICE_KIND)).await, 2);
    assert_eq!(count(&pool, RELEASES, None).await, 1);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_41_a_pick_with_no_release_is_refused_in_every_state(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let jar = signed_in(&mut db).await;
    let business = business(&mut db).await;
    let mut numbers = Vec::new();
    for status in ["paid", "free", "awaiting_confirm"] {
        let (job, number) = job(&mut db).await;
        pick(&mut db, (job, business), status, None).await;
        numbers.push(number);
    }
    let (paused, number) = job(&mut db).await;
    pick(&mut db, (paused, business), "paid", None).await;
    sqlx::query(
        "WITH h AS (INSERT INTO intake.homeowner (id, contact_id, created_at) \
           VALUES (gen_random_uuid(), gen_random_uuid(), now()) RETURNING id), \
         c AS (INSERT INTO intake.conversation (id, homeowner_id, job_id, language, \
           first_inbox_id, consent_template, consent_sent_at, opened_at) \
           SELECT gen_random_uuid(), h.id, $1, 'en', gen_random_uuid(), 'consent', now(), now() \
           FROM h RETURNING id, job_id) \
         INSERT INTO intake.homeowner_pause (conversation_id, job_id, inbox_id, paused_at) \
           SELECT c.id, c.job_id, gen_random_uuid(), now() FROM c",
    )
    .bind(paused)
    .execute(&mut *db)
    .await
    .expect("CANCEL pause");
    numbers.push(number);
    sqlx::query(r#"UPDATE ops.safe_mode SET "on" = true, since = now()"#)
        .execute(&mut *db)
        .await
        .expect("safe mode on");
    for number in numbers {
        let screen = press(
            &pool,
            &jar,
            &number.to_string(),
            &Uuid::new_v4().to_string(),
        )
        .await;
        assert_eq!(screen, ReleaseScreen::NotReleased(number));
    }
    assert_eq!(count(&pool, NOTICES, Some(BUSINESS_NOTICE_KIND)).await, 0);
    assert_eq!(count(&pool, RELEASES, None).await, 0, "nothing released");
    assert_eq!(count(&pool, AUDITS, Some(RELEASE_RESEND_ACTION)).await, 0);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_41_bad_input_and_a_signed_out_jar_queue_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let jar = signed_in(&mut db).await;
    let business = business(&mut db).await;
    let (job, number) = job(&mut db).await;
    pick(&mut db, (job, business), "paid", Some(10)).await;
    let fresh = Uuid::new_v4().to_string();
    let unknown = number + 1000;
    let screen = press(&pool, &jar, &unknown.to_string(), &fresh).await;
    assert_eq!(screen, ReleaseScreen::NotReleased(unknown));
    assert_eq!(
        press(&pool, &jar, "abc", &fresh).await,
        ReleaseScreen::NotANumber
    );
    let typed = number.to_string();
    let stale = press(&pool, &jar, &typed, "not-a-press").await;
    assert_eq!(stale, ReleaseScreen::StaleForm);
    let form = ReleaseForm {
        job: typed,
        press: fresh,
    };
    let signed_out = owner_release_resend(&mut db, &CookieJar::new(), &form, at(60)).await;
    assert_eq!(signed_out, Err(OwnerToolError::Refused));
    assert_eq!(count(&pool, NOTICES, Some(BUSINESS_NOTICE_KIND)).await, 0);
    assert_eq!(count(&pool, AUDITS, Some(RELEASE_RESEND_ACTION)).await, 0);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_41_only_the_newest_release_of_a_job_is_resent(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let jar = signed_in(&mut db).await;
    let (older_business, newer_business) = (business(&mut db).await, business(&mut db).await);
    let (job, number) = job(&mut db).await;
    let older = pick(&mut db, (job, older_business), "no_show", Some(10)).await;
    let newer = pick(&mut db, (job, newer_business), "paid", Some(20)).await;
    let press_id = Uuid::new_v4();
    let screen = press(&pool, &jar, &number.to_string(), &press_id.to_string()).await;
    assert_eq!(screen, ReleaseScreen::Sent(number));
    let keys: Vec<String> = sqlx::query_scalar("SELECT dedupe_key FROM core.task WHERE kind = $1")
        .bind(BUSINESS_NOTICE_KIND)
        .fetch_all(&mut *db)
        .await
        .expect("keys");
    assert_eq!(keys, vec![resend_key(newer, press_id)]);
    assert_ne!(keys[0], resend_key(older, press_id));
}
