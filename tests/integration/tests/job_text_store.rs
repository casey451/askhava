//! The stored job text and cards on a fresh database (TD-PRIV-27, T411; PRIV-008, PRIV-011,
//! PRIV-006, CARD-007, CARD-023; D-3189).
//!
//! `quotes::api::keep_description` stores scrubbed words once per message and job (a SPLIT's two
//! jobs each keep a row; the `&str` compile-fail case is the doc test in `app/quotes/src/job_text.rs`);
//! `keep_card` stores a card only under its own digest, the value `intake::api::card_digest` gives;
//! `stored_card`, `unsent_cards`, `mark_card_sent` and `sent_card` read back only a card whose
//! text still has its digest; no column holds a contact detail and no type's `Debug` holds the
//! words; the PRIV-008 text stage and "Delete my data" (`portals::api::delete_her_data`) delete
//! the rows. Every write runs as `askhava_app`. Every name, phone, email and address is fake.

use askhava_core::api::Claimed;
use askhava_integration::{PERMISSION_DENIED, set_app_role};
use askhava_portals::api::delete_her_data;
use askhava_privacy::api::{
    CommonWords, Known, NameMemory, NewContact, PURGE_KIND, ScrubbedText, Vault, scrub,
    store_contact,
};
use askhava_quotes::api::{
    CardFacts, CardParts, DEFAULT_MIN_WORDS, JobCard, KeptCard, Place, build_card, card_digest,
    description, keep_card, keep_description, mark_card_sent, run_text_purge, sent_card,
    stored_card, unsent_cards,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

const REQUEST: Uuid = Uuid::from_u128(0x7411_0001);
const JOB_A: Uuid = Uuid::from_u128(0x7411_00a1);
const JOB_B: Uuid = Uuid::from_u128(0x7411_00b1);
const FAKE_CONTACT: [&str; 5] = [
    "Zelda",
    "Canary",
    "555-0142",
    "zelda@example.com",
    "12 Test Street",
];
const WITH_CONTACT: &str = "Call Zelda Canary at 928-555-0142 or zelda@example.com, \
     I live at 12 Test Street. The kitchen faucet drips all day and the cabinet is wet";

fn at(minutes: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000).expect("time") + SignedDuration::from_mins(minutes)
}

/// Her words through the scrubber, with her fake vault name known (AI-002, AI-003).
fn scrubbed(words: &str) -> ScrubbedText {
    let common = CommonWords::default();
    let name = SecretString::from("Zelda Canary".to_owned());
    let known = Known {
        common: &common,
        vault_name: Some(&name),
    };
    scrub(words, &known, &mut NameMemory::default())
}

fn card(words: &str) -> JobCard {
    let parts = CardParts {
        category: "Plumbing".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: words.to_owned(),
        answers: vec![("Where".to_owned(), "Kitchen".to_owned())],
        timing: None,
        danger: None,
        photos: vec![Uuid::from_u128(0x7411_00f0)],
        concept: None,
    };
    build_card(&parts, DEFAULT_MIN_WORDS)
}

fn facts(emergency: bool) -> CardFacts {
    CardFacts {
        category: "plumbing_repair".to_owned(),
        candidates: vec!["faucet_repair".to_owned(), "leak_repair".to_owned()],
        emergency,
    }
}

async fn app(pool: &PgPool) -> PgConnection {
    let mut conn = pool.acquire().await.expect("connection").detach();
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

async fn app_pool(pool: &PgPool) -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|conn, _| Box::pin(async move { set_app_role(conn).await }))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool")
}

async fn rows(pool: &PgPool, job: Uuid) -> (i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM quotes.job_description WHERE job_id = $1),
                (SELECT count(*) FROM quotes.shown_card WHERE job_id = $1)",
    )
    .bind(job)
    .fetch_one(pool)
    .await
    .expect("counts")
}

/// Stores one description and one card for `job` of `request`.
async fn keep_both(db: &mut PgConnection, (request, job): (Uuid, Uuid), words: &str) {
    let message = Uuid::new_v4();
    let kept = keep_description(db, (request, job), message, &scrubbed(words), at(0)).await;
    assert_eq!(kept, Ok(true));
    let shown = card(scrubbed(words).as_str());
    let digest = card_digest(shown.text());
    let stored = keep_card(db, (request, job), &digest, &shown, &facts(false), at(1)).await;
    assert_eq!(stored, Ok(KeptCard::Stored));
}

/// TD-PRIV-27 (PRIV-008): a repeat of one message and job stores one row; a SPLIT's two jobs from
/// one text each keep a row; the words come back in time order and `Debug` hides them.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_27_words_are_kept_once_per_message_and_job(pool: PgPool) {
    let mut db = app(&pool).await;
    let (first, second) = (Uuid::from_u128(0x7411_00e1), Uuid::from_u128(0x7411_00e2));
    let words = scrubbed("The kitchen faucet drips and the yard sprinkler leaks");
    for (job, expected) in [(JOB_A, true), (JOB_A, false), (JOB_B, true)] {
        let kept = keep_description(&mut db, (REQUEST, job), first, &words, at(0)).await;
        assert_eq!(kept, Ok(expected));
    }
    let later = scrubbed("It started after the freeze last week");
    let kept = keep_description(&mut db, (REQUEST, JOB_A), second, &later, at(5)).await;
    assert_eq!(kept, Ok(true));
    assert_eq!(rows(&pool, JOB_A).await, (2, 0));
    assert_eq!(rows(&pool, JOB_B).await, (1, 0));
    let read = description(&mut db, JOB_A)
        .await
        .expect("read")
        .expect("some");
    assert_eq!(read.parts(), [words.as_str(), later.as_str()]);
    assert_eq!(
        read.text(),
        format!("{}\n{}", words.as_str(), later.as_str())
    );
    assert!(!format!("{read:?}").contains("drips"));
    let none = description(&mut db, Uuid::from_u128(0x7411_0099)).await;
    assert_eq!(none, Ok(None));
}

/// TD-PRIV-27 (CARD-007): a card is stored only under its own digest, which equals
/// `intake::api::card_digest`; once, with its category and candidates; read back only for its
/// own job and digest, and never once its stored text no longer matches the digest.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_27_a_card_is_kept_and_read_only_under_its_own_digest(pool: PgPool) {
    let mut db = app(&pool).await;
    let shown = card("The kitchen faucet drips all day long");
    let digest = card_digest(shown.text());
    assert_eq!(digest, askhava_intake::api::card_digest(shown.text()));
    let other = card_digest("another card");
    let key = (REQUEST, JOB_A);
    let refused = keep_card(&mut db, key, &other, &shown, &facts(false), at(0)).await;
    assert_eq!(refused, Ok(KeptCard::WrongDigest));
    assert_eq!(rows(&pool, JOB_A).await, (0, 0));
    for expected in [KeptCard::Stored, KeptCard::Repeat] {
        let kept = keep_card(&mut db, key, &digest, &shown, &facts(false), at(0)).await;
        assert_eq!(kept, Ok(expected));
    }
    assert_eq!(rows(&pool, JOB_A).await, (0, 1));
    let read = stored_card(&mut db, JOB_A, &digest).await.expect("read");
    let read = read.expect("stored");
    assert_eq!(
        (read.card.clone(), read.facts.clone()),
        (shown.clone(), facts(false))
    );
    assert_eq!((read.request, read.job), (REQUEST, JOB_A));
    assert!(!format!("{read:?}").contains("drips") && !format!("{shown:?}").contains("drips"));
    assert_eq!(stored_card(&mut db, JOB_B, &digest).await, Ok(None));
    assert_eq!(stored_card(&mut db, JOB_A, &other).await, Ok(None));
    sqlx::query("UPDATE quotes.shown_card SET card_text = card_text || ' changed'")
        .execute(&pool)
        .await
        .expect("change as the owner");
    assert_eq!(stored_card(&mut db, JOB_A, &digest).await, Ok(None));
    assert_eq!(unsent_cards(&mut db, REQUEST).await, Ok(vec![]));
}

/// TD-PRIV-27 (INT-029, CARD-023): `unsent_cards` gives each unsent job's newest card with its
/// emergency mark, oldest job first; `mark_card_sent` sets `sent_at` once and `sent_card` then
/// returns that card, and the sent job leaves the unsent list.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_27_unsent_cards_then_one_sent_card_per_job(pool: PgPool) {
    let mut db = app(&pool).await;
    let (b_card, a_old, a_new) = (
        card("The yard sprinkler leaks near the gate"),
        card("The kitchen faucet drips all day long"),
        card("The kitchen faucet drips and the cabinet is wet"),
    );
    let stores = [
        (JOB_B, &b_card, true, 0),
        (JOB_A, &a_old, false, 1),
        (JOB_A, &a_new, false, 2),
    ];
    for (job, shown, emergency, minute) in stores {
        let digest = card_digest(shown.text());
        let key = (REQUEST, job);
        let kept = keep_card(&mut db, key, &digest, shown, &facts(emergency), at(minute)).await;
        assert_eq!(kept, Ok(KeptCard::Stored));
    }
    let unsent = unsent_cards(&mut db, REQUEST).await.expect("unsent");
    let got: Vec<_> = unsent
        .iter()
        .map(|c| (c.job, c.card.clone(), c.facts.emergency))
        .collect();
    assert_eq!(
        got,
        [(JOB_B, b_card.clone(), true), (JOB_A, a_new.clone(), false)]
    );
    assert_eq!(sent_card(&mut db, JOB_A).await, Ok(None));
    let digest = card_digest(a_new.text());
    assert_eq!(
        mark_card_sent(&mut db, JOB_A, &digest, at(10)).await,
        Ok(true)
    );
    assert_eq!(
        mark_card_sent(&mut db, JOB_A, &digest, at(11)).await,
        Ok(false)
    );
    let old = card_digest(a_old.text());
    assert_eq!(
        mark_card_sent(&mut db, JOB_A, &old, at(12)).await,
        Ok(false)
    );
    let sent = sent_card(&mut db, JOB_A)
        .await
        .expect("read")
        .expect("sent");
    assert_eq!((sent.card, sent.digest), (a_new, digest));
    let unsent = unsent_cards(&mut db, REQUEST).await.expect("unsent");
    assert_eq!(unsent.iter().map(|c| c.job).collect::<Vec<_>>(), [JOB_B]);
}

/// TD-PRIV-27 (PRIV-006, PRIV-008): words and a card made from a text holding a fictional name,
/// phone, email and street address leave none of them in any column, and nothing in the module
/// logs (it has no logging call at all).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_27_no_column_holds_a_contact_detail_and_nothing_logs(pool: PgPool) {
    let mut db = app(&pool).await;
    keep_both(&mut db, (REQUEST, JOB_A), WITH_CONTACT).await;
    let all: Vec<(String,)> = sqlx::query_as(
        "SELECT row_to_json(d)::text FROM quotes.job_description d
         UNION ALL SELECT row_to_json(s)::text FROM quotes.shown_card s",
    )
    .fetch_all(&pool)
    .await
    .expect("rows");
    assert_eq!(all.len(), 2);
    for (row,) in &all {
        assert!(row.contains("faucet"), "words kept");
        for detail in FAKE_CONTACT {
            assert!(!row.contains(detail), "{detail} stored");
        }
    }
    let source = include_str!("../../../app/quotes/src/job_text.rs");
    for logging in ["tracing", "println!", "eprintln!", "log::"] {
        assert!(!source.contains(logging), "{logging}");
    }
}

fn purge_task(job: Uuid, stage: &str) -> Claimed {
    Claimed {
        id: 1,
        kind: PURGE_KIND.to_owned(),
        payload: json!({ "job": job, "stage": stage }),
        attempts: 1,
    }
}

/// TD-PRIV-27 (PRIV-008): the text stage deletes the job's rows and leaves another job's; the
/// photo and contact stages delete none; a rerun is harmless.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_27_the_text_stage_deletes_only_its_own_job(pool: PgPool) {
    let mut db = app(&pool).await;
    keep_both(
        &mut db,
        (REQUEST, JOB_A),
        "The kitchen faucet drips all day long",
    )
    .await;
    keep_both(
        &mut db,
        (REQUEST, JOB_B),
        "The yard sprinkler leaks near the gate",
    )
    .await;
    let handlers = app_pool(&pool).await;
    for stage in ["photos", "contact"] {
        run_text_purge(handlers.clone(), purge_task(JOB_A, stage))
            .await
            .expect(stage);
        assert_eq!(rows(&pool, JOB_A).await, (1, 1));
    }
    for _ in 0..2 {
        run_text_purge(handlers.clone(), purge_task(JOB_A, "text"))
            .await
            .expect("text");
        assert_eq!(rows(&pool, JOB_A).await, (0, 0));
        assert_eq!(rows(&pool, JOB_B).await, (1, 1));
    }
    let bad = run_text_purge(handlers, purge_task(JOB_A, "everything")).await;
    assert!(bad.is_err());
}

fn vault() -> Vault {
    let key = |byte: u8| SecretString::from(format!("1:{}", STANDARD.encode([byte; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid:5432/askhava".to_owned());
    Vault::new(&key(0x37), &key(0x47), &url).expect("vault keys")
}

/// A homeowner with a fake vault contact; returns (contact, homeowner).
async fn homeowner(pool: &PgPool, phone: &str) -> (Uuid, Uuid) {
    let new = NewContact {
        name: Some(SecretString::from("Zelda Canary".to_owned())),
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let contact = store_contact(&mut app(pool).await, &vault(), &new)
        .await
        .expect("contact");
    let id = sqlx::query_scalar(
        "INSERT INTO intake.homeowner (contact_id, created_at) VALUES ($1, now()) RETURNING id",
    )
    .bind(contact)
    .fetch_one(pool)
    .await
    .expect("homeowner");
    (contact, id)
}

/// A request of `homeowner` whose first job is `job`, closed when `closed`; returns its ID.
async fn request(pool: &PgPool, homeowner: Uuid, job: Uuid, closed: bool) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO intake.conversation (homeowner_id, job_id, language, first_inbox_id,
           consent_template, consent_sent_at, opened_at, closed_at) VALUES ($1, $2, 'en',
           gen_random_uuid(), 'consent_first', now() - interval '2 days',
           now() - interval '2 days', CASE WHEN $3 THEN now() END) RETURNING id",
    )
    .bind(homeowner)
    .bind(job)
    .bind(closed)
    .fetch_one(pool)
    .await
    .expect("request")
}

async fn photo(pool: &PgPool, job: Uuid) {
    sqlx::query(
        "INSERT INTO privacy.photo (id, job_id, key_version, storage_key, stored_at)
         VALUES (gen_random_uuid(), $1, 1, 'photos/fake-' || $1::text, now())",
    )
    .bind(job)
    .execute(pool)
    .await
    .expect("photo");
}

async fn photos(pool: &PgPool, job: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM privacy.photo WHERE job_id = $1")
        .bind(job)
        .fetch_one(pool)
        .await
        .expect("photos")
}

/// TD-PRIV-27 (PRIV-011): "Delete my data" deletes the rows of her jobs only, in its transaction:
/// an open request with two jobs (the second linked only by its `job.%` answer) and a request
/// closed before the delete (no `job.%` links left), whose later job's photos go too; another
/// homeowner's rows stay; a rolled-back delete deletes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_27_delete_my_data_deletes_every_request_of_hers(pool: PgPool) {
    let (her, her_home) = homeowner(&pool, "(928) 555-0171").await;
    let (_, other_home) = homeowner(&pool, "(928) 555-0172").await;
    let (job_c, job_d, job_x) = (
        Uuid::from_u128(0xc1),
        Uuid::from_u128(0xd1),
        Uuid::from_u128(0xe1),
    );
    let open = request(&pool, her_home, JOB_A, false).await;
    let closed = request(&pool, her_home, job_c, true).await;
    let theirs = request(&pool, other_home, job_x, false).await;
    sqlx::query(
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at)
         VALUES ($1, 'job.plumbing.home', $2, now())",
    )
    .bind(open)
    .bind(JOB_B.simple().to_string())
    .execute(&pool)
    .await
    .expect("job link");
    let mut db = app(&pool).await;
    let words = "The kitchen faucet drips all day long";
    for key in [
        (open, JOB_A),
        (open, JOB_B),
        (closed, job_c),
        (closed, job_d),
        (theirs, job_x),
    ] {
        keep_both(&mut db, key, words).await;
    }
    for job in [JOB_B, job_d, job_x] {
        photo(&pool, job).await;
    }
    let mut tx = db.begin().await.expect("begin");
    delete_her_data(&mut tx, her).await.expect("delete");
    tx.rollback().await.expect("rollback");
    assert_eq!(rows(&pool, JOB_B).await, (1, 1));
    let mut tx = db.begin().await.expect("begin");
    delete_her_data(&mut tx, her).await.expect("delete");
    tx.commit().await.expect("commit");
    for job in [JOB_A, JOB_B, job_c, job_d] {
        assert_eq!(rows(&pool, job).await, (0, 0));
    }
    assert_eq!(
        (photos(&pool, JOB_B).await, photos(&pool, job_d).await),
        (0, 0)
    );
    assert_eq!(
        (rows(&pool, job_x).await, photos(&pool, job_x).await),
        ((1, 1), 1)
    );
}

/// TD-PRIV-27 (CARD-007): the app role cannot UPDATE any column but `shown_card.sent_at`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_27_the_app_role_updates_only_sent_at(pool: PgPool) {
    let mut db = app(&pool).await;
    keep_both(
        &mut db,
        (REQUEST, JOB_A),
        "The kitchen faucet drips all day long",
    )
    .await;
    for sql in [
        "UPDATE quotes.shown_card SET card_text = 'other words here'",
        "UPDATE quotes.shown_card SET digest = repeat('0', 32)",
        "UPDATE quotes.shown_card SET emergency = true",
        "UPDATE quotes.job_description SET words = 'other words here'",
    ] {
        let err = sqlx::query(sql).execute(&mut db).await.expect_err(sql);
        let code = err.as_database_error().and_then(|e| e.code());
        assert_eq!(code.as_deref(), Some(PERMISSION_DENIED), "{sql}");
    }
    sqlx::query("UPDATE quotes.shown_card SET sent_at = recorded_at")
        .execute(&mut db)
        .await
        .expect("sent_at");
}
