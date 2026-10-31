//! Cards for late-eligible businesses and the coverage line, in the database (TD-CARD-05:
//! CARD-016, CARD-018; TD-INT-29: CARD-001, INT-017, INT-032; TD-INT-25: INT-032). Fake IDs and
//! fake text only; the towns are public place names.
//!
//! A paused business gets nothing; once it becomes eligible while the job is Open or No quotes
//! yet it gets its card then, sent at that time (its own 24 hours, D-042); after that nothing.
//! Intake tells the homeowner the count `quotes::api::coverage` gives, the SPLIT offer, or moves
//! the request to Not covered when nobody in town covers the job.

use std::collections::BTreeSet;

use askhava_businesses::api::AccountStatus;
use askhava_catalog::api::{Catalog, parse};
use askhava_core::api::Guarded;
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    COUNT_MANY_TEMPLATE, COUNT_ONE_TEMPLATE, JobStart, NOT_COVERED_TEMPLATE, Opened, Request,
    Routing, SPLIT_TEMPLATE, TextIn, begin_job, receive_text, recruiting_demand, split_request,
    tell_coverage,
};
use askhava_jobs::api::{JobStatus, create, current, move_to};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor;
use askhava_quotes::api::{
    CardBusiness, CardJob, OfferTown, OwnerPauses, Pool, Pooled, coverage, send_job_cards,
    send_late_cards,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const CATEGORIES: &str = include_str!("../../../docs/catalog/categories.yaml");
const TOWN: &str = "lake_havasu_city";
const AC: (&str, &str) = ("hvac_repair", "AC not cooling");
const PIPE: (&str, &str) = ("plumbing_repair", "faucet replacement");
const WIRE: (&str, &str) = ("electrical", "outlet repair");

fn at(hour: i8) -> Timestamp {
    format!("2026-11-02T{hour:02}:00:00-07:00")
        .parse()
        .expect("time")
}

fn job(catalog: &Catalog, (category, job_type): (&str, &str)) -> Job {
    let def = catalog
        .categories
        .iter()
        .find(|c| c.id == category)
        .expect("category");
    Job {
        category: category.to_owned(),
        job_type: job_type.to_owned(),
        license: LicenseKind::parse(&def.license.kind).expect("kind"),
        handyman_allowed: def.handyman_allowed,
        listed: def.job_types.iter().any(|j| j == job_type),
    }
}

fn card_job(candidates: &[(&str, &str)]) -> CardJob {
    let catalog = parse(CATEGORIES).expect("catalog");
    CardJob {
        towns: vec![OfferTown {
            code: TOWN.to_owned(),
            state: "AZ".to_owned(),
        }],
        candidates: candidates.iter().map(|c| job(&catalog, *c)).collect(),
        text: String::new(),
    }
}

/// A verified business in Lake Havasu City with `class`, ticking `ticked`.
fn business(id: u128, status: AccountStatus, class: &str, ticked: &[(&str, &str)]) -> Pooled {
    Pooled {
        card: CardBusiness {
            id: Uuid::from_u128(id),
            status,
            verified: true,
            towns: BTreeSet::from([TOWN.to_owned()]),
            ticked: ticked
                .iter()
                .map(|(c, j)| ((*c).to_owned(), (*j).to_owned()))
                .collect(),
            classes: vec![class.to_owned()],
            licenses: Holder::default(),
        },
        overflow_on: false,
    }
}

/// Moves the job along one arrow, which must apply.
async fn step(db: &mut Db, id: Uuid, arrow: (JobStatus, JobStatus), at: Timestamp) {
    let moved = move_to(db, id, arrow, at, Actor::System)
        .await
        .expect("move");
    assert!(matches!(moved, Guarded::Applied(())));
}

fn pool_of<'a>(businesses: &'a [Pooled], pauses: &'a OwnerPauses) -> Pool<'a> {
    Pool { businesses, pauses }
}

async fn sent_at(db: &mut Db, job: Uuid, business: u128) -> Timestamp {
    let at: String = sqlx::query_scalar(
        "SELECT to_char(sent_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
         FROM quotes.job_card WHERE job_id = $1 AND business_id = $2",
    )
    .bind(job)
    .bind(Uuid::from_u128(business))
    .fetch_one(&mut **db)
    .await
    .expect("card");
    at.parse().expect("time")
}

/// TD-CARD-05 (CARD-016, CARD-018): a paused business and one under an owner switch get nothing;
/// unpaused while Open, or newly ticked while No quotes yet, each gets its card then; in Intake or
/// once cancelled, nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_05_late_eligible_business_gets_its_card_then(mut db: Db) {
    let map = ClassMap::shipped().expect("map");
    let ac = card_job(&[AC]);
    let id = create(&mut db, at(9), false, Actor::System)
        .await
        .expect("job");
    let mut pool = vec![
        business(1, AccountStatus::Active, "R-39", &[AC]),
        business(2, AccountStatus::Paused, "R-39", &[AC]),
        business(3, AccountStatus::Active, "R-39", &[]),
    ];
    let none = OwnerPauses::default();
    let early = send_late_cards(&mut db, map, id, &ac, &pool_of(&pool, &none), at(9))
        .await
        .expect("intake");
    assert!(early.is_empty(), "no late card while the job is in Intake");
    step(&mut db, id, (JobStatus::Intake, JobStatus::Open), at(9)).await;
    let first = send_job_cards(&mut db, map, id, &ac, &pool_of(&pool, &none), at(9))
        .await
        .expect("send");
    assert_eq!(
        first
            .iter()
            .map(|r| r.business.as_u128())
            .collect::<Vec<_>>(),
        vec![1]
    );
    let trade = OwnerPauses {
        trades: BTreeSet::from([AC.0.to_owned()]),
        ..OwnerPauses::default()
    };
    pool[1].card.status = AccountStatus::Active;
    let switched = Pool {
        businesses: &pool,
        pauses: &trade,
    };
    let held = send_late_cards(&mut db, map, id, &ac, &switched, at(10))
        .await
        .expect("held");
    assert!(held.is_empty(), "an owner trade switch sends nothing");
    let unpaused = send_late_cards(&mut db, map, id, &ac, &pool_of(&pool, &none), at(11))
        .await
        .expect("late");
    assert_eq!(unpaused.len(), 1);
    assert_eq!(
        sent_at(&mut db, id, 2).await,
        at(11),
        "its own 24 hours start now"
    );
    assert_eq!(sent_at(&mut db, id, 1).await, at(9));
    step(
        &mut db,
        id,
        (JobStatus::Open, JobStatus::NoQuotesYet),
        at(12),
    )
    .await;
    pool[2]
        .card
        .ticked
        .insert((AC.0.to_owned(), AC.1.to_owned()));
    let ticked = send_late_cards(&mut db, map, id, &ac, &pool_of(&pool, &none), at(13))
        .await
        .expect("tick");
    assert_eq!(ticked.len(), 1);
    assert_eq!(sent_at(&mut db, id, 3).await, at(13));
    step(
        &mut db,
        id,
        (JobStatus::NoQuotesYet, JobStatus::Cancelled),
        at(14),
    )
    .await;
    pool.push(business(4, AccountStatus::Active, "R-39", &[AC]));
    let after = send_late_cards(&mut db, map, id, &ac, &pool_of(&pool, &none), at(15))
        .await
        .expect("final");
    assert!(after.is_empty(), "no card once the job is final");
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn text(contact: u128, words: &'static str) -> TextIn<'static> {
    TextIn {
        inbox_id: Uuid::from_u128(0x5_1840 + contact),
        contact: Uuid::from_u128(contact),
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(10),
    }
}

/// Opens a request with a job for `trade`; returns both IDs.
async fn open_request(db: &mut Db, msg: &TextIn<'_>, trade: &str) -> (Uuid, Uuid) {
    let handled = receive_text(db, &secret(), msg, Settings::default())
        .await
        .expect("open");
    let conversation = handled.conversation.expect("request");
    let start = JobStart {
        conversation,
        words: msg.inbound.words,
        emergency: false,
        trade,
        at: at(10),
    };
    let Opened::Created(job_id) = begin_job(db, &start).await.expect("job") else {
        panic!("a new job");
    };
    (conversation, job_id)
}

/// Opens a request with a job for `trade`, tells it the coverage of `job` and `pool`, and
/// returns the request, the job, the queued template and the queued body.
async fn told(
    db: &mut Db,
    (msg, trade): (&TextIn<'_>, &str),
    job: &CardJob,
    pool: &[Pooled],
) -> (Uuid, Uuid, Option<&'static str>, Option<String>) {
    let (conversation, job_id) = open_request(db, msg, trade).await;
    let none = OwnerPauses::default();
    let count = coverage(
        job,
        &Pool {
            businesses: pool,
            pauses: &none,
        },
    )
    .expect("coverage");
    let req = Request {
        town: TOWN,
        area: &[TOWN],
        trade,
        trade_open: true,
        words: msg.inbound.words,
        move_states: None,
        job_types: &[],
    };
    let routing = Routing {
        conversation,
        msg,
        language: askhava_messaging::api::Language::En,
        spanish_on: false,
        label: "AC",
        job_type: None,
    };
    let queued = tell_coverage(db, &secret(), (&req, count), &routing)
        .await
        .expect("tell");
    let body = match &queued {
        Some(q) => sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE id = $1")
            .bind(q.outbox_id)
            .fetch_one(&mut **db)
            .await
            .expect("body"),
        None => None,
    };
    (conversation, job_id, queued.map(|q| q.template), body)
}

/// TD-INT-29 (INT-017, CARD-001, INT-032): 5 businesses covering the job get the plural count, 1
/// the singular; nobody covering it in town moves the request to Not covered with a demand row;
/// a business licensed for only one of plumbing and electrical gets no card.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_29_the_count_line_is_the_match(mut db: Db) {
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let five: Vec<Pooled> = (1..=5)
        .map(|n| business(n, AccountStatus::Active, "R-39", &[AC]))
        .collect();
    let ac = card_job(&[AC]);
    let (_, _, template, body) = told(
        &mut db,
        (&text(1, "my AC is not cooling"), AC.0),
        &ac,
        &five,
    )
    .await;
    assert_eq!(template, Some(COUNT_MANY_TEMPLATE));
    assert_eq!(
        body.as_deref(),
        Some("Ask Hava: Got it. We'll send this to the 5 AC businesses on Ask Hava.")
    );
    let (_, _, template, body) = told(
        &mut db,
        (&text(2, "my AC is not cooling"), AC.0),
        &ac,
        &five[..1],
    )
    .await;
    assert_eq!(template, Some(COUNT_ONE_TEMPLATE));
    assert_eq!(
        body.as_deref(),
        Some("Ask Hava: Got it. We'll send this to the 1 AC business on Ask Hava.")
    );
    let (conversation, job_id, template, _) =
        told(&mut db, (&text(3, "my AC is not cooling"), AC.0), &ac, &[]).await;
    assert_eq!(template, Some(NOT_COVERED_TEMPLATE));
    assert_eq!(
        current(&mut db, job_id).await.expect("job").status.code(),
        "not_covered"
    );
    assert_eq!(
        recruiting_demand(&mut db, conversation)
            .await
            .expect("demand")
            .len(),
        1
    );
    let both = card_job(&[PIPE, WIRE]);
    let partial = [business(6, AccountStatus::Active, "R-37", &[PIPE])];
    let map = ClassMap::shipped().expect("map");
    let none = OwnerPauses::default();
    let pool = Pool {
        businesses: &partial,
        pauses: &none,
    };
    let sent = send_job_cards(&mut db, map, job_id, &both, &pool, at(10))
        .await
        .expect("send");
    assert!(
        sent.is_empty(),
        "no card to a business covering only part of the job"
    );
}

/// TD-INT-25 (INT-032): plumbing and electrical with no business licensed for both gets the SPLIT
/// offer; SPLIT (or DIVIDIR) opens the second trade's job, so each goes out as its own request.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_25_split_offer_and_split_makes_two_requests(mut db: Db) {
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let parts = [
        business(6, AccountStatus::Active, "R-37", &[PIPE]),
        business(7, AccountStatus::Active, "R-11", &[WIRE]),
    ];
    let words = "the kitchen faucet leaks and an outlet sparks";
    let msg = text(4, words);
    let (conversation, first, template, body) =
        told(&mut db, (&msg, PIPE.0), &card_job(&[PIPE, WIRE]), &parts).await;
    assert_eq!(template, Some(SPLIT_TEMPLATE));
    assert_eq!(
        body.as_deref(),
        Some(
            "Ask Hava: This needs 2 kinds of business. Reply SPLIT to send it as 2 requests, one per trade."
        )
    );
    let second = JobStart {
        conversation,
        words,
        emergency: false,
        trade: WIRE.0,
        at: at(11),
    };
    assert_eq!(
        split_request(&mut db, "ok", &second).await.expect("ok"),
        Opened::Refused
    );
    let Opened::Created(other) = split_request(&mut db, "SPLIT", &second)
        .await
        .expect("split")
    else {
        panic!("SPLIT opens the second request");
    };
    assert_ne!(other, first);
    let msg = text(5, words);
    let (conversation, _, _, _) =
        told(&mut db, (&msg, PIPE.0), &card_job(&[PIPE, WIRE]), &parts).await;
    let second = JobStart {
        conversation,
        words,
        emergency: false,
        trade: WIRE.0,
        at: at(11),
    };
    let opened = split_request(&mut db, "dividir", &second)
        .await
        .expect("dividir");
    assert!(matches!(opened, Opened::Created(_)), "DIVIDIR opens it too");
}
