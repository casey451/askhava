//! Not covered and out-of-area requests end to end in the database (TD-INT-14, TD-INT-19,
//! TD-LIC-01: INT-016, INT-017, INT-018, INT-022, LIC-018). Fake IDs and fake text only.
//!
//! A request for a trade that is not open moves its job from Intake to Not covered, gets the
//! INT-017 reply and one recruiting-demand row; a town outside the list moves it to Closed early
//! with no demand row; a later text from the same phone opens a new request (INT-022).

use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    JobStart, NOT_COVERED_TEMPLATE, OUT_OF_AREA_TEMPLATE, Opened, Outcome, Queued, Request,
    Routing, TextIn, Why, apply_routing, begin_job, receive_text, recruiting_demand, route,
    trade_is_open,
};
use askhava_jobs::api::current;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const TOWN: &str = "lake_havasu_city";
const AREA: [&str; 1] = [TOWN];

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn at() -> Timestamp {
    "2026-11-02T10:00:00-07:00".parse().expect("time")
}

fn text(contact: Uuid) -> TextIn<'static> {
    TextIn {
        inbox_id: Uuid::from_u128(0x4_3000 + contact.as_u128()),
        contact,
        inbound: Inbound {
            words: "my boat lift is stuck",
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(),
    }
}

/// An open request with one job for `trade`, and the job's ID.
async fn request_with_job(db: &mut Db, msg: &TextIn<'_>, trade: &str) -> (Uuid, Uuid) {
    let handled = receive_text(db, &secret(), msg, Settings::default())
        .await
        .expect("open");
    let conversation = handled.conversation.expect("request");
    let start = JobStart {
        conversation,
        words: msg.inbound.words,
        emergency: false,
        trade,
        at: at(),
    };
    let Opened::Created(job) = begin_job(db, &start).await.expect("job") else {
        panic!("a new job");
    };
    (conversation, job)
}

async fn run(
    db: &mut Db,
    msg: &TextIn<'_>,
    conversation: Uuid,
    town: &str,
    trade: &str,
) -> (Outcome, Option<Queued>) {
    let open = trade_is_open(db, trade).await.expect("open check");
    let req = Request {
        town,
        area: &AREA,
        trade,
        trade_open: open,
        words: msg.inbound.words,
        move_states: None,
        job_types: &[],
    };
    let outcome = route(&req);
    let routing = Routing {
        conversation,
        msg,
        language: askhava_messaging::api::Language::En,
        spanish_on: false,
        label: "boat lift",
        job_type: None,
    };
    let queued = apply_routing(db, &secret(), (&req, outcome), &routing)
        .await
        .expect("apply");
    let expected = match outcome {
        Outcome::Continue => None,
        Outcome::OutOfArea => Some(OUT_OF_AREA_TEMPLATE),
        Outcome::NotCovered(_) => Some(NOT_COVERED_TEMPLATE),
    };
    if let Some(sent) = &queued {
        assert_eq!(Some(sent.template), expected);
    }
    (outcome, queued)
}

/// TD-INT-14, TD-LIC-01 (INT-016, INT-017, INT-018): a boat-lift request moves to Not covered with
/// the INT-017 reply and one recruiting-demand row; an open trade goes on untouched.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_14_a_trade_not_open_moves_to_not_covered(mut db: Db) {
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let msg = text(Uuid::from_u128(0x7001));
    let (conversation, job) = request_with_job(&mut db, &msg, "boat_lift").await;
    let (outcome, first) = run(&mut db, &msg, conversation, TOWN, "boat_lift").await;
    assert!(first.is_some());
    assert_eq!(outcome, Outcome::NotCovered(Why::TradeNotOpen));
    assert_eq!(
        current(&mut db, job).await.expect("job").status.code(),
        "not_covered"
    );
    let rows = recruiting_demand(&mut db, conversation)
        .await
        .expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].town.as_str(), rows[0].trade.as_str()),
        (TOWN, "boat_lift")
    );
    let (again, queued) = run(&mut db, &msg, conversation, TOWN, "boat_lift").await;
    assert_eq!(again, outcome);
    assert!(queued.is_none(), "a second run queues nothing");
    let rows = recruiting_demand(&mut db, conversation)
        .await
        .expect("rows");
    assert_eq!(rows.len(), 1, "a second run adds no demand row");
}

/// TD-INT-19 (INT-022): a town outside the list moves the job to Closed early, logs no demand,
/// and a later text from the same phone opens a new request.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_19_out_of_area_closes_early_and_a_later_text_reopens(mut db: Db) {
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let msg = text(Uuid::from_u128(0x7002));
    let (conversation, job) = request_with_job(&mut db, &msg, "plumbing_repair").await;
    let (outcome, queued) = run(&mut db, &msg, conversation, "needles", "plumbing_repair").await;
    assert!(queued.is_some(), "the out-of-area reply was queued");
    assert_eq!(outcome, Outcome::OutOfArea);
    assert_eq!(
        current(&mut db, job).await.expect("job").status.code(),
        "closed_early"
    );
    assert!(
        recruiting_demand(&mut db, conversation)
            .await
            .expect("rows")
            .is_empty()
    );
    let later = TextIn {
        inbox_id: Uuid::from_u128(0x4_3999),
        ..text(Uuid::from_u128(0x7002))
    };
    let handled = receive_text(&mut db, &secret(), &later, Settings::default())
        .await
        .expect("later");
    let opened = handled.conversation.expect("a new request opened");
    assert_ne!(opened, conversation);
}

/// TD-LIC-01 (INT-017): an open trade is open, a trade not in the catalog is not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_01_open_and_unknown_trades(mut db: Db) {
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    assert!(
        trade_is_open(&mut db, "plumbing_repair")
            .await
            .expect("open")
    );
    assert!(!trade_is_open(&mut db, "boat_lift").await.expect("open"));
}
