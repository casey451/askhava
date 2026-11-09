//! The retention purge on a fresh database (TD-PRIV-03; PRIV-007 to PRIV-011; D-013, D-079,
//! D-491, D-492).
//!
//! A closed job's three purge stages come due at 30, 180 and 1,096 days and not a second before,
//! however long after the close the sweep first runs;
//! the contact purge clears name, email and address but keeps the sealed phone, its index and the
//! consent records; an open dispute or complaint holds it, and the hold's release lets it go;
//! photos go at 30 days; the job text stage deletes the intake answers; ledger, audit and terms
//! records stay; "Delete my data" clears the vault entry and photos and confirms what is kept.
//! A hold on any of the homeowner's jobs keeps her contact; a request that never got a job is
//! purged by `intake`'s own sweep; a re-check that just queued complaint events waits for them.
//! Every handler runs as `askhava_app` ([`app_pool`]). Every name, phone, email and address is fake.
//!
//! TD-PRIV-12's day-31 Twilio body delete and PRIV-009's delete of the sealed photo objects in
//! storage (with the orphan sweep) are T276's, tested in `retention_deletes.rs` (TD-PRIV-20, D-1841).

use askhava_businesses::api::{NewComplaint, create, queue_complaint_holds, record_complaint};
use askhava_core::api::{Claimed, WorkerId, claim};
use askhava_intake::api::{run_purge, run_request_purge, sweep_requests};
use askhava_integration::set_app_role;
use askhava_jobs::api::{PURGE_DUE_KIND, sweep_closed};
use askhava_payments::api::run_dispute_holds;
use askhava_privacy::api::{
    DELETE_CONFIRMATION, HoldSource, NewContact, PURGE_KIND, RECHECK_KIND, Vault, delete_my_data,
    find_by_phone, is_held, place_hold, purge_photos, release_hold, run_complaint_hold,
    run_purge_due, run_purge_due_after, store_contact,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const JOB: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0092);
const PHONE: &str = "(928) 555-0192";
const DAY: SignedDuration = SignedDuration::from_hours(24);

fn vault() -> Vault {
    let key = |byte: u8| SecretString::from(format!("1:{}", STANDARD.encode([byte; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid:5432/askhava".to_owned());
    Vault::new(&key(0x31), &key(0x41), &url).expect("vault keys")
}

async fn app(pool: &PgPool) -> PgConnection {
    let mut conn = pool.acquire().await.expect("connection").detach();
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

/// A pool whose every connection runs as `askhava_app`, for the handlers (R9).
async fn app_pool(pool: &PgPool) -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|conn, _| Box::pin(async move { set_app_role(conn).await }))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool")
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(JOB)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn run(pool: &PgPool, sql: &'static str) {
    sqlx::query(sql)
        .bind(JOB)
        .execute(pool)
        .await
        .expect("seed");
}

/// A charged pick on [`JOB`]: its intent, attempt and evidence rows (IDs only, PRIV-010).
const LEDGER: [&str; 3] = [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ('00000000-0000-0000-0000-00000000092a',
           gen_random_uuid(), $1, 7500, 'pi_FakeT092', false, now())",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at) SELECT pick_id, 1, gen_random_uuid(), 'succeeded', now(), now() FROM payments.pick_intent WHERE job_id = $1",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at) SELECT p.pick_id, p.business_id, $1, a.press_id, 1,
           'pick-' || p.pick_id::text || '-try1', 'cus_FakeT092', 'pi_FakeT092', 1, 7500, now(), now()
           FROM payments.pick_intent p JOIN payments.charge_attempt a USING (pick_id)",
];

fn task(kind: &str, stage: &str) -> Claimed {
    let payload = json!({ "job": JOB, "stage": stage });
    Claimed {
        id: 1,
        kind: kind.to_owned(),
        payload,
        attempts: 1,
    }
}

/// A homeowner with a fake contact, one closed request for [`JOB`] with a consent record, a
/// terms acceptance, one intake answer and one photo with a live link; returns the contact.
async fn seeded(pool: &PgPool) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from("Zelda Canary".to_owned())),
        phone: Some(SecretString::from(PHONE.to_owned())),
        email: Some(SecretString::from("zelda@example.com".to_owned())),
        address: Some(SecretString::from("12 Test Street".to_owned())),
        onsite_contact: None,
    };
    let contact = store_contact(&mut app(pool).await, &vault(), &new)
        .await
        .expect("contact");
    sqlx::query("INSERT INTO intake.homeowner (id, contact_id, created_at) VALUES ($1, $2, now())")
        .bind(Uuid::from_u128(0x92b))
        .bind(contact)
        .execute(pool)
        .await
        .expect("homeowner");
    for sql in [
        "INSERT INTO intake.conversation (id, homeowner_id, job_id, language, first_inbox_id,
           consent_template, consent_sent_at, opened_at, closed_at) VALUES ('00000000-0000-0000-0000-00000000092c',
           '00000000-0000-0000-0000-00000000092b', $1, 'en', gen_random_uuid(), 'consent_first',
           now() - interval '200 days', now() - interval '200 days', now() - interval '181 days')",
        "INSERT INTO intake.consent_record (homeowner_id, conversation_id, channel, inbox_id,
           consent_version, consented_at) SELECT homeowner_id, id, 'sms', first_inbox_id, 'v1',
           opened_at FROM intake.conversation WHERE job_id = $1",
        "INSERT INTO intake.terms_acceptance (homeowner_id, conversation_id, pick_id, document,
           version, text_sha256, language, channel, assent_line, accepted_at) SELECT homeowner_id,
           id, '00000000-0000-0000-0000-00000000092a', 'homeowner_terms', '2026.1', repeat('a', 64),
           'en', 'quote_page', 'I agree to the terms.', opened_at FROM intake.conversation WHERE job_id = $1",
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at)
           SELECT id, 'leak.where', 'kitchen', opened_at FROM intake.conversation WHERE job_id = $1",
        "INSERT INTO privacy.photo (id, job_id, key_version, storage_key, stored_at)
           VALUES (gen_random_uuid(), $1, 1, 'photos/fake-092', now())",
        "INSERT INTO privacy.photo_link (token_hash, photo_id, issued_at, expires_at)
           SELECT sha256('fake-token'), id, now(), now() + interval '10 minutes' FROM privacy.photo",
    ] {
        run(pool, sql).await;
    }
    contact
}

async fn fields(pool: &PgPool, contact: Uuid) -> (bool, bool, bool, bool, i64) {
    sqlx::query_as(
        "SELECT name IS NULL, email IS NULL, address IS NULL, phone IS NULL,
           (SELECT count(*) FROM privacy.contact_index WHERE contact_id = $1
            AND purpose = 'homeowner-phone') FROM privacy.contact WHERE id = $1",
    )
    .bind(contact)
    .fetch_one(pool)
    .await
    .expect("contact row")
}

/// TD-PRIV-03 (PRIV-007 to PRIV-009): the three stages are queued once from the close date and
/// come due at 30, 180 and 1,096 days, even when first swept 40 days after the close (no look-back
/// limit); at 179 days the contact stage is not yet claimable.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_03_stages_come_due_at_30_180_and_1096_days(pool: PgPool) {
    let closed = Timestamp::from_second(1_790_000_000).expect("time");
    sqlx::query(
        "INSERT INTO jobs.job (id, status, created_at, status_at, timer_from)
         VALUES ($1, 'closed', $2, $2, $2)",
    )
    .bind(JOB)
    .bind(jiff_sqlx::Timestamp::from(closed))
    .execute(&pool)
    .await
    .expect("job");
    let mut conn = app(&pool).await;
    assert_eq!(sweep_closed(&mut conn, closed + DAY * 40).await, Ok(3));
    assert_eq!(sweep_closed(&mut conn, closed + DAY * 41).await, Ok(0));
    let owner = WorkerId::new(&pool).await.expect("worker");
    let due = |days: i32| closed + DAY * days;
    let lease = DAY * 2000;
    let early = claim(
        &pool,
        owner,
        due(30) - SignedDuration::from_secs(1),
        lease,
        10,
    );
    assert!(early.await.expect("claim").is_empty());
    let at_179 = claim(&pool, owner, due(179), lease, 10)
        .await
        .expect("claim");
    assert_eq!(at_179.len(), 1);
    assert_eq!(at_179[0].payload["stage"], "photos");
    let at_180 = claim(&pool, owner, due(180), lease, 10)
        .await
        .expect("claim");
    assert_eq!(at_180.len(), 1);
    assert_eq!(at_180[0].payload["stage"], "contact");
    assert!(
        claim(&pool, owner, due(1095), lease, 10)
            .await
            .expect("claim")
            .is_empty()
    );
    let text = claim(&pool, owner, due(1096), lease, 10)
        .await
        .expect("claim");
    assert_eq!(
        (text[0].kind.as_str(), &text[0].payload["stage"]),
        (PURGE_DUE_KIND, &json!("text"))
    );
}

/// TD-PRIV-03 (PRIV-007, PRIV-010, D-079, D-492): the contact stage waits an hour, then hands on;
/// `intake` clears name, email and address and keeps the sealed phone, its index, the consent
/// record and the terms acceptance (no contact field); a second run changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_03_contact_purge_keeps_the_phone_with_its_consent(pool: PgPool) {
    let contact = seeded(&pool).await;
    run_purge_due(app_pool(&pool).await, task(PURGE_DUE_KIND, "contact"))
        .await
        .expect("first");
    let rechecks = "SELECT count(*) FROM core.task WHERE kind = 'privacy.purge_recheck'
       AND payload->>'job' = $1::text AND run_at > now() + interval '59 minutes'";
    assert_eq!(count(&pool, rechecks).await, 1);
    run_purge_due(app_pool(&pool).await, task(RECHECK_KIND, "contact"))
        .await
        .expect("recheck");
    let handed = "SELECT count(*) FROM core.task WHERE kind = 'privacy.purge' AND payload->>'job' = $1::text";
    assert_eq!(count(&pool, handed).await, 1);
    for _ in 0..2 {
        run_purge(app_pool(&pool).await, task(PURGE_KIND, "contact"))
            .await
            .expect("purge");
    }
    assert_eq!(fields(&pool, contact).await, (true, true, true, false, 1));
    let found = find_by_phone(&mut app(&pool).await, &vault(), PHONE).await;
    assert_eq!(found, Ok(Some(contact)));
    let audits = "SELECT count(*) FROM ops.audit_log WHERE action = 'contact_purged' AND $1::uuid IS NOT NULL";
    assert_eq!(count(&pool, audits).await, 1);
    let kept = "SELECT (SELECT count(*) FROM intake.consent_record) + (SELECT count(*) FROM intake.terms_acceptance) + 0 * count($1::uuid)";
    assert_eq!(count(&pool, kept).await, 2);
}

/// TD-PRIV-03 (PRIV-007, D-491): an open dispute on the job's charge holds the contact purge,
/// so the re-check comes back daily and nothing is handed on; past its window the hold is released.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_03_a_dispute_hold_blocks_the_contact_purge(pool: PgPool) {
    let contact = seeded(&pool).await;
    for sql in LEDGER.into_iter().chain([
        "INSERT INTO payments.dispute (stripe_dispute_id, pick_id, amount_cents, due_at, opened_at)
           SELECT 'dp_FakeT092', pick_id, 7500, now() + interval '7 days', now() FROM payments.charge_evidence WHERE job_id = $1",
    ]) {
        run(&pool, sql).await;
    }
    let step = || async {
        let t = task(RECHECK_KIND, "contact");
        run_dispute_holds(app_pool(&pool).await, t.clone())
            .await
            .expect("dispute step");
        run_purge_due(app_pool(&pool).await, t)
            .await
            .expect("privacy step");
    };
    step().await;
    assert_eq!(is_held(&mut app(&pool).await, JOB).await, Ok(true));
    let handed = "SELECT count(*) FROM core.task WHERE kind = 'privacy.purge' AND payload->>'job' = $1::text";
    assert_eq!(count(&pool, handed).await, 0);
    let daily = "SELECT count(*) FROM core.task WHERE kind = 'privacy.purge_recheck' AND payload->>'job' = $1::text";
    assert_eq!(count(&pool, daily).await, 1);
    assert!(!fields(&pool, contact).await.0);
    run(&pool, "UPDATE payments.dispute SET due_at = now() - interval '91 days' WHERE $1::uuid IS NOT NULL").await;
    step().await;
    assert_eq!(is_held(&mut app(&pool).await, JOB).await, Ok(false));
    assert_eq!(count(&pool, handed).await, 1);
}

/// TD-PRIV-03 (PRIV-007, D-491): an open complaint's hold event holds the job; once its box has
/// closed the release event lets it go, and a repeated event changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_03_a_complaint_hold_blocks_until_its_box_closes(pool: PgPool) {
    let mut conn = app(&pool).await;
    let business = create(&mut conn, false, Timestamp::now())
        .await
        .expect("business");
    let new = NewComplaint {
        business,
        job: JOB,
        kind: "no_show",
        words: "never came",
    };
    record_complaint(&mut conn, &new, Timestamp::now())
        .await
        .expect("complaint");
    let apply_events = |pool: PgPool| async move {
        let rows: Vec<serde_json::Value> = sqlx::query_scalar(
            "SELECT payload FROM core.task WHERE kind = 'businesses.complaint_hold' ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .expect("events");
        for payload in rows {
            let event = Claimed {
                id: 1,
                kind: String::new(),
                payload,
                attempts: 1,
            };
            run_complaint_hold(app_pool(&pool).await, event)
                .await
                .expect("apply");
        }
    };
    assert_eq!(
        queue_complaint_holds(&mut conn, JOB, Timestamp::now()).await,
        Ok(1)
    );
    assert_eq!(
        queue_complaint_holds(&mut conn, JOB, Timestamp::now()).await,
        Ok(0)
    );
    apply_events(pool.clone()).await;
    assert_eq!(is_held(&mut conn, JOB).await, Ok(true));
    run(&pool, "UPDATE businesses.complaint SET listed_at = received_at + interval '121 hours' WHERE job_id = $1").await;
    assert_eq!(
        queue_complaint_holds(&mut conn, JOB, Timestamp::now()).await,
        Ok(1)
    );
    apply_events(pool.clone()).await;
    assert_eq!(is_held(&mut conn, JOB).await, Ok(false));
}

/// TD-PRIV-03 (PRIV-009, PRIV-008, PRIV-010): the photo stage deletes the job's photos and their
/// links as the app role; the text stage deletes the intake answers; the conversation, consent,
/// terms and audit records stay.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_03_photos_at_30_days_and_job_text_at_3_years(pool: PgPool) {
    seeded(&pool).await;
    let audits = "SELECT count(*) + 0 * count($1::uuid) FROM ops.audit_log";
    let audit_before = count(&pool, audits).await;
    run_purge_due(app_pool(&pool).await, task(PURGE_DUE_KIND, "photos"))
        .await
        .expect("photos");
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM privacy.photo WHERE job_id = $1"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) + 0 * count($1::uuid) FROM privacy.photo_link"
        )
        .await,
        0
    );
    assert_eq!(purge_photos(&mut app(&pool).await, JOB).await, Ok(0));
    run_purge(app_pool(&pool).await, task(PURGE_KIND, "text"))
        .await
        .expect("text");
    let answers = "SELECT count(*) + 0 * count($1::uuid) FROM intake.intake_answer";
    assert_eq!(count(&pool, answers).await, 0);
    let conversations = "SELECT count(*) FROM intake.conversation WHERE job_id = $1";
    assert_eq!(count(&pool, conversations).await, 1);
    assert_eq!(count(&pool, audits).await, audit_before);
}

/// TD-PRIV-03 (PRIV-011, D-214): "Delete my data" clears the vault entry and the photos, keeps the
/// phone locked for the consent records, leaves the ledger rows (IDs only) as they were and
/// confirms with the PRIV-011 sentence.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_03_delete_my_data_clears_the_vault_entry_and_photos(pool: PgPool) {
    let contact = seeded(&pool).await;
    for sql in LEDGER {
        run(&pool, sql).await;
    }
    let ledger = "SELECT (SELECT count(*) FROM payments.pick_intent) + (SELECT count(*) FROM payments.charge_attempt) + (SELECT count(*) FROM payments.charge_evidence WHERE job_id = $1)";
    let said = delete_my_data(&mut app(&pool).await, contact, &[JOB]).await;
    assert_eq!(said, Ok(DELETE_CONFIRMATION));
    assert!(DELETE_CONFIRMATION.contains("locked, for 5 years"));
    assert_eq!(fields(&pool, contact).await, (true, true, true, false, 1));
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM privacy.photo WHERE job_id = $1"
        )
        .await,
        0
    );
    assert_eq!(count(&pool, ledger).await, 3);
}

const JOB_B: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_092b);

/// Adds a request of the seeded homeowner for `job` (none when `None`), opened and closed the given
/// days ago (open when `closed` is `None`); returns its ID.
async fn request(pool: &PgPool, job: Option<Uuid>, opened: i32, closed: Option<i32>) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO intake.conversation (homeowner_id, job_id, language, first_inbox_id,
           consent_template, consent_sent_at, opened_at, closed_at)
         VALUES ('00000000-0000-0000-0000-00000000092b', $1, 'en', gen_random_uuid(),
           'consent_first', now() - make_interval(days => $2), now() - make_interval(days => $2),
           now() - make_interval(days => $3)) RETURNING id",
    )
    .bind(job)
    .bind(opened)
    .bind(closed)
    .fetch_one(pool)
    .await
    .expect("request")
}

/// TD-PRIV-03 (PRIV-007, D-1799): holds are per job but the contact is the homeowner's, so a hold
/// on her other job (closed 200 days ago, open dispute) keeps the contact when this job's purge
/// runs; once released, the purge clears it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_03_a_hold_on_another_job_of_hers_keeps_the_contact(pool: PgPool) {
    let contact = seeded(&pool).await;
    request(&pool, Some(JOB_B), 230, Some(200)).await;
    let dispute = Uuid::from_u128(0xd15);
    let now = Timestamp::now();
    place_hold(
        &mut app(&pool).await,
        HoldSource::Dispute,
        dispute,
        JOB_B,
        now,
    )
    .await
    .expect("hold");
    run_purge(app_pool(&pool).await, task(PURGE_KIND, "contact"))
        .await
        .expect("held purge");
    assert_eq!(
        fields(&pool, contact).await,
        (false, false, false, false, 1)
    );
    release_hold(
        &mut app(&pool).await,
        HoldSource::Dispute,
        dispute,
        JOB_B,
        now,
    )
    .await
    .expect("release");
    run_purge(app_pool(&pool).await, task(PURGE_KIND, "contact"))
        .await
        .expect("purge");
    assert_eq!(fields(&pool, contact).await, (true, true, true, false, 1));
}

/// TD-PRIV-03 (PRIV-007, D-1799): a request that never got a job is queued by `intake`'s sweep 180
/// days after it closed, and its purge clears the contact; an open request with no job blocks a
/// job's purge only while it was answered in the last 180 days.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_03_a_request_with_no_job_is_purged_and_an_abandoned_one_blocks_nothing(
    pool: PgPool,
) {
    let contact = seeded(&pool).await;
    let fresh = request(&pool, None, 10, None).await;
    run_purge(app_pool(&pool).await, task(PURGE_KIND, "contact"))
        .await
        .expect("blocked purge");
    assert!(!fields(&pool, contact).await.0);
    sqlx::query("UPDATE intake.conversation SET opened_at = now() - interval '300 days', consent_sent_at = now() - interval '300 days' WHERE id = $1")
        .bind(fresh)
        .execute(&pool)
        .await
        .expect("abandon");
    let mut conn = app(&pool).await;
    assert_eq!(sweep_requests(&mut conn, Timestamp::now()).await, Ok(1));
    assert_eq!(sweep_requests(&mut conn, Timestamp::now()).await, Ok(0));
    let payload = json!({ "conversation": fresh });
    let purge = Claimed {
        id: 1,
        kind: "intake.request_purge".to_owned(),
        payload,
        attempts: 1,
    };
    run_request_purge(app_pool(&pool).await, purge)
        .await
        .expect("request purge");
    assert_eq!(fields(&pool, contact).await, (true, true, true, false, 1));
}

/// TD-PRIV-03 (PRIV-007, D-1799): a closed request with no job is queued due 180 days after its
/// close, so it is not claimable before then.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_03_a_closed_request_with_no_job_comes_due_at_180_days(pool: PgPool) {
    seeded(&pool).await;
    request(&pool, None, 12, Some(10)).await;
    assert_eq!(
        sweep_requests(&mut app(&pool).await, Timestamp::now()).await,
        Ok(1)
    );
    let due = "SELECT count(*) + 0 * count($1::uuid) FROM core.task WHERE kind = 'intake.request_purge'
       AND run_at BETWEEN now() + interval '169 days 23 hours' AND now() + interval '170 days 1 hour'";
    assert_eq!(count(&pool, due).await, 1);
}

/// TD-PRIV-03 (PRIV-007, D-1799): a re-check on which the complaint step just queued hold events
/// does not decide: nothing is handed on and it comes back in an hour.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_03_a_recheck_with_new_complaint_events_waits_for_them(pool: PgPool) {
    seeded(&pool).await;
    run_purge_due_after(app_pool(&pool).await, task(RECHECK_KIND, "contact"), 1)
        .await
        .expect("deferred");
    let handed = "SELECT count(*) FROM core.task WHERE kind = 'privacy.purge' AND payload->>'job' = $1::text";
    assert_eq!(count(&pool, handed).await, 0);
    let later = "SELECT count(*) FROM core.task WHERE kind = 'privacy.purge_recheck'
       AND payload->>'job' = $1::text AND run_at > now() + interval '59 minutes'";
    assert_eq!(count(&pool, later).await, 1);
    run_purge_due_after(app_pool(&pool).await, task(RECHECK_KIND, "contact"), 0)
        .await
        .expect("decided");
    assert_eq!(count(&pool, handed).await, 1);
}
