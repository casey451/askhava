//! TD-INT-53 (T716, D-4441, D-4602, D-4618): the trade questions at step 5 of the intake order,
//! through the route's entry, `intake::api::receive_raw_reply`: "Which fits best?" (INT-014) before
//! the coverage line when the stored candidates call for it, read by number or whole words
//! (INT-012) and asked at most twice (INT-015); `trade_ask` when her words named no trade, asked
//! once more, then a wait, never INT-017's reply for want of a trade (INT-016, INT-043); START
//! (`ask_open_again`) resends the open question with no row; a two-word first text naming a job
//! type (INT-007) and a licensed-only job type ("water heater leak") counted from the stored pairs
//! (INT-017, CARD-001). Texts are asserted only up to the coverage line or the wait. PostgreSQL,
//! fake data and 555 numbers only.

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, request_welcome, save_business_name,
    set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    COUNT_MANY_TEMPLATE, COUNT_ONE_TEMPLATE, Handled, NOT_COVERED_TEMPLATE, RawReply, Reply,
    TRADE_ASK_TEMPLATE, TextIn, WHICH_FITS_TEMPLATE, receive_raw_reply, restart_after_start,
};
use askhava_integration::set_app_role;
use askhava_licensing::api::{ROC_HEADER, import_city_list, import_roc_list};
use askhava_ops::api::Actor;
use askhava_privacy::api::Vault;
use askhava_quotes::api::NOT_UPDATE_TEXTS;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff::civil::date;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const TOWN: &str = "lake_havasu_city";
/// A Lake Havasu City ZIP (INT-048).
const HOME_ZIP: &str = "86403";
/// A first text naming the handyman trade and none of its job types.
const HANDYMAN: &str = "I need a handyman for a few things";
/// A first text of more than two words that names no trade.
const NO_TRADE: &str = "can you help me out today";
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7301,Sample Hinge One LLC,,,2019-03-04\nCL-7302,Sample Hinge Two LLC,,,2019-03-04\n";
const ROC_NUMBER: &str = "ROC900137";
const LEAK: (&str, &str) = ("plumbing_repair", "leak repair");
const DOOR: (&str, &str) = ("handyman", "door repair");
const ASKS: &str = "SELECT count(*) FROM intake.intake_answer WHERE conversation_id = $1 \
                    AND question_id IN ('which_fits', 'trade_ask')";
const LINKS: &str = "SELECT count(*) FROM intake.intake_answer WHERE conversation_id = $1 \
                     AND question_id LIKE 'job.%'";
/// The step-5 questions queued in request `$1` (each text's subject IDs name its request).
const ASKED_TEXTS: &str = "SELECT count(*) FROM messaging.outbox WHERE $1 = ANY(subject_ids) \
                           AND template_id IN ('which_fits', 'trade_ask')";
const KEPT: &str = "SELECT count(*) FROM quotes.job_description WHERE request_id = $1";

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x73), &key(0x74), &url).expect("vault")
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_200_000 + minute * 60).expect("time")
}

fn text_in(contact: u128, words: &str, minute: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x73_0000 + contact * 100 + minute.unsigned_abs() as u128),
        contact: Uuid::from_u128(contact),
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    }
}

/// `words` from fake contact `contact` at `minute`: the queued texts' templates and outbox IDs.
async fn post(
    db: &mut PgConnection,
    contact: u128,
    words: &str,
    minute: i64,
) -> Vec<(&'static str, Uuid)> {
    let msg = text_in(contact, words, minute);
    let raw = SecretString::from(words.to_owned());
    let vault = vault();
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    let reply = receive_raw_reply(db, &secret(), (&msg, keys), (Settings::default(), &[])).await;
    match reply.expect("reply") {
        Reply::Handled(Handled { texts, .. }) | Reply::NameAddress(_, Handled { texts, .. }) => {
            texts.iter().map(|q| (q.template, q.outbox_id)).collect()
        }
        other => panic!("handled, got {other:?}"),
    }
}

/// The templates `words` queued, in order.
async fn sent(db: &mut PgConnection, contact: u128, words: &str, minute: i64) -> Vec<&'static str> {
    let got = post(db, contact, words, minute).await;
    got.into_iter().map(|(t, _)| t).collect()
}

/// The body of outbox row `id`.
async fn body(db: &mut PgConnection, id: Uuid) -> String {
    let sql = "SELECT body FROM messaging.outbox WHERE id = $1";
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(db)
        .await
        .expect(sql)
}

/// The catalog, the fake city and ROC lists, `businesses` (signed up as `handyman` or holding
/// R-37, each ticking its job type and serving the town), then the app role.
async fn setup(db: &mut PgConnection, businesses: &[(bool, (&str, &str))]) {
    load_embedded(db).await.expect("catalog");
    import_city_list(db, date(2026, 11, 1), CITY)
        .await
        .expect("city list");
    let roc = format!("{ROC_HEADER}\n{ROC_NUMBER},R-37,Active,2012-05-01\n");
    import_roc_list(db, date(2026, 11, 1), &roc)
        .await
        .expect("roc list");
    for (n, (handyman, ticked)) in businesses.iter().enumerate() {
        business(db, n, *handyman, *ticked).await;
    }
    set_app_role(db).await.expect("SET ROLE askhava_app");
}

/// The `n`th active, welcomed business serving the town, ticking `ticked`: a handyman matched on
/// the city list (LIC-031), else an R-37 holder with an Active check.
async fn business(db: &mut PgConnection, n: usize, handyman: bool, ticked: (&str, &str)) {
    let b = create(db, false, at(0)).await.expect("business");
    let signup = "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
                  VALUES ($1, $2, now(), ARRAY['handyman'])";
    let check = "INSERT INTO licensing.license_check (business_id, source, license_number, \
                 list_date, status, checked_at) VALUES ($1, 'roc', $2, '2026-11-01', 'active', now())";
    let (sql, value) = if handyman {
        (signup, Uuid::new_v4().to_string())
    } else {
        (check, ROC_NUMBER.to_owned())
    };
    let bound = sqlx::query(sql).bind(b);
    let bound = if handyman {
        bound.bind(Uuid::parse_str(&value).expect("uuid"))
    } else {
        bound.bind(value)
    };
    bound.execute(&mut *db).await.expect("license");
    add_owner(db, b, Uuid::new_v4(), at(0))
        .await
        .expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, b, Event::Checked(ok), Actor::System, at(0))
        .await
        .expect("active");
    request_welcome(db, b, at(0)).await.expect("welcome");
    welcome_sent(db, b, at(0)).await.expect("sent");
    let name = ["Sample Hinge One LLC", "Sample Hinge Two LLC"][n % 2];
    save_business_name(db, b, (name, None), None)
        .await
        .expect("name");
    let actor = Actor::Business(b);
    set_towns(db, b, &[TOWN], actor).await.expect("towns");
    let choice = JobTypeChoice::new(ticked.0, ticked.1);
    let allowed: BTreeSet<_> = [choice.clone()].into();
    set_job_types(db, b, &[choice], &allowed, actor)
        .await
        .expect("job types");
}

/// Fake contact `contact`'s newest request.
async fn request(db: &mut PgConnection, contact: u128) -> Uuid {
    sqlx::query_scalar(
        "SELECT c.id FROM intake.conversation c JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE h.contact_id = $1 ORDER BY c.opened_at DESC LIMIT 1",
    )
    .bind(Uuid::from_u128(contact))
    .fetch_one(db)
    .await
    .expect("request")
}

async fn count(db: &mut PgConnection, sql: &'static str, id: Uuid) -> i64 {
    let n = sqlx::query_scalar(sql).bind(id).fetch_one(db).await;
    n.expect(sql)
}

const TWO_HANDYMEN: [(bool, (&str, &str)); 2] = [(true, DOOR), (true, DOOR)];

/// TD-INT-53 (INT-014, INT-012, INT-017): a first text naming a trade but no job type gets "Which
/// fits best?" after the ZIP, before the coverage line; a number chooses, and so do the whole words
/// of one option, and the coverage line follows.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_53_which_fits_by_number_or_words_then_the_coverage_line(mut db: Db) {
    setup(&mut db, &TWO_HANDYMEN).await;
    for (contact, answer) in [(0x0555_0301_u128, "1"), (0x0555_0302, "the door one")] {
        sent(&mut db, contact, HANDYMAN, 0).await;
        let asked = sent(&mut db, contact, HOME_ZIP, 1).await;
        assert_eq!(asked, [WHICH_FITS_TEMPLATE], "before the coverage line");
        let got = sent(&mut db, contact, answer, 2).await;
        assert_eq!(got.first(), Some(&COUNT_MANY_TEMPLATE), "{answer}: {got:?}");
        assert!(!got.contains(&WHICH_FITS_TEMPLATE));
    }
}

/// TD-INT-53 (INT-015, D-103): an unusable reply asks once more with the same options in the same
/// order; a second unusable reply, or "Not sure", keeps every candidate and asks no more. The kept
/// candidates hold `handyman_other`, so nothing after that reply is asserted (TD-INT-57's).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_53_which_fits_is_asked_at_most_twice(mut db: Db) {
    setup(&mut db, &TWO_HANDYMEN).await;
    sent(&mut db, 0x0555_0303, HANDYMAN, 0).await;
    let first = post(&mut db, 0x0555_0303, HOME_ZIP, 1).await;
    let again = post(&mut db, 0x0555_0303, "purple", 2).await;
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].0, WHICH_FITS_TEMPLATE);
    let (one, two) = (
        body(&mut db, first[0].1).await,
        body(&mut db, again[0].1).await,
    );
    assert_eq!(one, two, "the same options in the same order");
    assert!(one.contains("1) door repair"), "{one}");
    let last = sent(&mut db, 0x0555_0303, "purple", 3).await;
    assert!(!last.contains(&WHICH_FITS_TEMPLATE), "{last:?}");
    sent(&mut db, 0x0555_0304, HANDYMAN, 0).await;
    sent(&mut db, 0x0555_0304, HOME_ZIP, 1).await;
    let unsure = sent(&mut db, 0x0555_0304, "not sure", 2).await;
    assert!(!unsure.contains(&WHICH_FITS_TEMPLATE), "{unsure:?}");
    let id = request(&mut db, 0x0555_0304).await;
    assert_eq!(count(&mut db, KEPT, id).await, 1, "only her first text");
}

/// TD-INT-53 (INT-016, INT-015, INT-043, D-4441): a first text naming no trade gets `trade_ask`
/// after the ZIP; a reply naming none is asked once more, then gets nothing; a later reply naming
/// a trade still opens one job, keeps no words and gets the coverage line; never INT-017's reply.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_53_no_trade_is_asked_twice_then_waits(mut db: Db) {
    setup(&mut db, &TWO_HANDYMEN).await;
    let contact = 0x0555_0305;
    sent(&mut db, contact, NO_TRADE, 0).await;
    let id = request(&mut db, contact).await;
    assert_eq!(
        sent(&mut db, contact, HOME_ZIP, 1).await,
        [TRADE_ASK_TEMPLATE]
    );
    assert_eq!(
        sent(&mut db, contact, "not sure", 2).await,
        [TRADE_ASK_TEMPLATE]
    );
    assert!(sent(&mut db, contact, "just something", 3).await.is_empty());
    assert_eq!(count(&mut db, LINKS, id).await, 0, "no job yet");
    let got = sent(&mut db, contact, "my front door is sticking", 4).await;
    assert_eq!(got.first(), Some(&COUNT_MANY_TEMPLATE), "{got:?}");
    assert!(!got.contains(&NOT_COVERED_TEMPLATE));
    assert_eq!(count(&mut db, LINKS, id).await, 1, "one job");
    assert_eq!(count(&mut db, KEPT, id).await, 0, "no reply kept as words");
}

/// TD-INT-53 (INT-014, INT-043, T691): START resends the open question (`ask_open_again`) and
/// writes no row; both templates are listed in `NOT_UPDATE_TEXTS` (D-1293).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_53_start_resends_the_open_question_with_no_row(mut db: Db) {
    setup(&mut db, &TWO_HANDYMEN).await;
    for (contact, first) in [(0x0555_0306_u128, NO_TRADE), (0x0555_0307, HANDYMAN)] {
        sent(&mut db, contact, first, 0).await;
        sent(&mut db, contact, HOME_ZIP, 1).await;
        let id = request(&mut db, contact).await;
        let rows = count(&mut db, ASKS, id).await;
        let before = count(&mut db, ASKED_TEXTS, id).await;
        let msg = text_in(contact, "START", 2);
        restart_after_start(&mut db, &secret(), &msg, Settings::default())
            .await
            .expect("start");
        assert_eq!(count(&mut db, ASKS, id).await, rows, "no row written");
        let after = count(&mut db, ASKED_TEXTS, id).await;
        assert_eq!(after, before + 1, "the open question again");
    }
    for template in [WHICH_FITS_TEMPLATE, TRADE_ASK_TEMPLATE] {
        assert!(NOT_UPDATE_TEXTS.contains(&template), "{template}");
    }
}

/// TD-INT-53 (INT-007, INT-017, D-4602): a two-word first text naming a job type, with two
/// covering businesses, gets after the ZIP the coverage line counting them, no question and no
/// INT-017 reply.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_53_a_two_word_job_type_gets_the_count(mut db: Db) {
    setup(&mut db, &TWO_HANDYMEN).await;
    sent(&mut db, 0x0555_0308, "sticky door", 0).await;
    let got = sent(&mut db, 0x0555_0308, HOME_ZIP, 1).await;
    assert_eq!(got.first(), Some(&COUNT_MANY_TEMPLATE), "{got:?}");
    assert!(!got.contains(&WHICH_FITS_TEMPLATE));
    assert!(!got.contains(&NOT_COVERED_TEMPLATE));
}

/// TD-INT-53 (D-4618, CARD-001, LIC-043): "water heater leak" keeps plumbing's "water heater
/// replacement" after read-back, shown by the count: of two businesses ticking leak repair, the
/// R-37 holder counts and the city-list handyman does not (1, not 2).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_53_a_licensed_only_job_type_survives_read_back(mut db: Db) {
    setup(&mut db, &[(false, LEAK), (true, LEAK)]).await;
    sent(&mut db, 0x0555_0309, "water heater leak", 0).await;
    let got = sent(&mut db, 0x0555_0309, HOME_ZIP, 1).await;
    assert_eq!(got.first(), Some(&COUNT_ONE_TEMPLATE), "{got:?}");
}

/// The newest stored set's `job_type.electrical` choices in request `$1` (D-4602).
const ELECTRICAL_PAIRS: &str = "SELECT count(*) FROM intake.intake_answer a \
    WHERE a.conversation_id = $1 AND a.question_id = 'job_type.electrical' \
    AND a.choice = 'outlet_repair' AND a.answered_at = (SELECT max(b.answered_at) \
    FROM intake.intake_answer b WHERE b.conversation_id = $1 \
    AND starts_with(b.question_id, 'job_type.'))";

/// TD-INT-53 (INT-014, INT-032, CARD-001, LIC-043): with two trades in the first text and ALSO,
/// choosing in "Which fits best?" for the handyman job keeps the electrical job's stored pair
/// ("outlet repair", never `electrical_other`) in the newest set its own job reads back.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_53_a_choice_keeps_the_other_jobs_pairs(mut db: Db) {
    setup(&mut db, &TWO_HANDYMEN).await;
    let contact = 0x0555_0310;
    let first = "I need a handyman and an outlet in the kitchen is dead";
    sent(&mut db, contact, first, 0).await;
    sent(&mut db, contact, "ALSO", 1).await;
    let asked = sent(&mut db, contact, HOME_ZIP, 2).await;
    assert!(asked.contains(&WHICH_FITS_TEMPLATE), "{asked:?}");
    let id = request(&mut db, contact).await;
    assert_eq!(count(&mut db, ELECTRICAL_PAIRS, id).await, 1, "before");
    let got = sent(&mut db, contact, "1", 3).await;
    assert!(!got.contains(&WHICH_FITS_TEMPLATE), "{got:?}");
    assert_eq!(count(&mut db, ELECTRICAL_PAIRS, id).await, 1, "kept");
}
