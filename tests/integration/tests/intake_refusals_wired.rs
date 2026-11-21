//! T776 (D-5081 finding F20, D-5160, D-5178): the LIC-017 and LIC-018 refusals wired into the
//! text order, through the route's entry `intake::api::receive_raw_reply`, with PostgreSQL, fake
//! data and 555 numbers only. A move leaving Arizona and a home watch request asking for rent
//! collection get the INT-017 not-covered reply at the trade step (INT-016), the job Not covered,
//! no card and no recruiting-demand row (D-1008); the same words arriving after EDIT are refused
//! before a new card is shown or sent, and words that reach a request any other way are refused
//! before a reply to a shown card is read and at the emergency-or-regular choice; in-Arizona moving and turnover cleaning texts still get
//! their coverage line and card. The keyword rules of D-5160 read the three trades.

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, request_welcome, save_business_name,
    set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    CARD_EDIT_TEMPLATE, CARD_SHOWN_TEMPLATE, CARDS_APPROVED_KIND, CHOICE_TEMPLATE,
    COUNT_ONE_TEMPLATE, Handled, NOT_COVERED_KIND, NOT_COVERED_TEMPLATE, Queued, RawReply, Reply,
    TextIn, keyword_reading, receive_raw_reply,
};
use askhava_integration::set_app_role;
use askhava_licensing::api::import_city_list;
use askhava_ops::api::Actor;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7321,Sample Fixit One LLC,,,2019-03-04\n";
/// Her fake answer to the name-and-address question.
const ANSWER: &str = "Testa 12 Sample Ave";
/// The three trades of D-5160 with the listed job type each keyword rule names (D-5178).
const RULES: [(&str, &str); 3] = [
    ("local_moving", "in-town move"),
    ("home_watch", "weekly house check"),
    ("turnover_cleaning", "turnover clean"),
];

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x75), &key(0x76), &url).expect("vault")
}

/// The texts `words` from fake homeowner `her` queued, in order, `minute` minutes after 11:00
/// Arizona on a fixed day, through the route's entry.
async fn sent(db: &mut PgConnection, her: Uuid, words: &str, minute: i64) -> Vec<Queued> {
    let start: Timestamp = "2026-11-02T11:00:00-07:00".parse().expect("time");
    let msg = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: her,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: start + SignedDuration::from_mins(minute),
    };
    let (raw, vault) = (SecretString::from(words.to_owned()), vault());
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    let reply = receive_raw_reply(db, &secret(), (&msg, keys), (Settings::default(), &[])).await;
    match reply.expect("reply") {
        Reply::Handled(Handled { texts, .. }) | Reply::NameAddress(_, Handled { texts, .. }) => {
            texts
        }
        other => panic!("handled, got {other:?}"),
    }
}

/// The catalog, the fake city list and one active, welcomed fake business serving the town that
/// signed up for the three trades and ticked each rule's job type; then the app role.
async fn setup(db: &mut PgConnection) {
    load_embedded(db).await.expect("catalog");
    let listed = import_city_list(db, date(2026, 11, 1), CITY).await;
    listed.expect("city list");
    let before: Timestamp = "2026-11-02T10:00:00-07:00".parse().expect("time");
    let b = create(db, false, before).await.expect("business");
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
         VALUES ($1, $2, now(), ARRAY['local_moving', 'home_watch', 'turnover_cleaning'])",
    )
    .bind(b)
    .bind(Uuid::new_v4())
    .execute(&mut *db)
    .await
    .expect("signed up for the three trades");
    let owner = add_owner(db, b, Uuid::new_v4(), before).await;
    owner.expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let checked = apply(db, b, Event::Checked(ok), Actor::System, before).await;
    checked.expect("active");
    request_welcome(db, b, before).await.expect("welcome");
    welcome_sent(db, b, before).await.expect("sent");
    let named = save_business_name(db, b, ("Sample Fixit One LLC", None), None).await;
    named.expect("name");
    let actor = Actor::Business(b);
    let towns = set_towns(db, b, &["lake_havasu_city"], actor).await;
    towns.expect("towns");
    let choices = RULES.map(|(trade, job)| JobTypeChoice::new(trade, job));
    let allowed: BTreeSet<_> = choices.iter().cloned().collect();
    let ticked = set_job_types(db, b, &choices, &allowed, actor).await;
    ticked.expect("job types");
    set_app_role(db).await.expect("SET ROLE askhava_app");
}

/// A new fake homeowner with 555 number `n`; returns her contact.
async fn homeowner(db: &mut PgConnection, n: u32) -> Uuid {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(format!("+1928555{n:04}"))),
        email: None,
        address: None,
        onsite_contact: None,
    };
    store_contact(db, &vault(), &new).await.expect("contact")
}

/// The templates of the texts each of `steps` got from `her`, one step a minute from `minute`.
async fn walk(db: &mut PgConnection, her: Uuid, steps: &[&str], minute: i64) -> Vec<&'static str> {
    let mut templates = Vec::new();
    for (at, words) in (minute..).zip(steps) {
        let texts = sent(db, her, words, at).await;
        templates.extend(texts.iter().map(|q| q.template));
    }
    templates
}

/// `first`, the ZIP, "In the next few days", `options` option answers and her name and address:
/// the templates of every text the walk got.
async fn to_card(
    db: &mut PgConnection,
    her: Uuid,
    first: &str,
    options: usize,
) -> Vec<&'static str> {
    let mut steps = vec![first, "86403", "2"];
    steps.extend(std::iter::repeat_n("1", options));
    steps.push(ANSWER);
    walk(db, her, &steps, 0).await
}

/// How many rows `sql` counts for `id`.
async fn rows(db: &mut PgConnection, sql: &'static str, id: Uuid) -> i64 {
    let n = sqlx::query_scalar(sql).bind(id).fetch_one(db).await;
    n.expect("count")
}

/// What `her` one request left behind.
#[derive(Debug, PartialEq, Eq)]
struct Left {
    /// Her job's status.
    status: String,
    /// `intake.not_covered` audit rows of the request.
    audits: i64,
    /// Recruiting-demand rows of the request.
    demand: i64,
    /// Cards shown to her for the job.
    shown: i64,
    /// Cards of the job sent to a business.
    cards: i64,
    /// Card approvals queued, for any job.
    approved: i64,
}

impl Left {
    /// A refused request after `shown` cards were shown to her: Not covered, one audit row, no
    /// demand row, nothing sent or approved.
    fn refused(shown: i64) -> Self {
        Self {
            status: "not_covered".to_owned(),
            audits: 1,
            demand: 0,
            shown,
            cards: 0,
            approved: 0,
        }
    }
}

/// Reads [`Left`] for `her` one request.
async fn left(db: &mut PgConnection, her: Uuid) -> Left {
    let (request, job): (Uuid, Uuid) = sqlx::query_as(
        "SELECT c.id, c.job_id FROM intake.conversation c \
         JOIN intake.homeowner h ON h.id = c.homeowner_id WHERE h.contact_id = $1",
    )
    .bind(her)
    .fetch_one(&mut *db)
    .await
    .expect("her one request");
    let status = sqlx::query_scalar("SELECT status FROM jobs.job WHERE id = $1")
        .bind(job)
        .fetch_one(&mut *db)
        .await;
    let audits = sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE action = $1 AND subject_id = $2",
    )
    .bind(NOT_COVERED_KIND)
    .bind(request)
    .fetch_one(&mut *db)
    .await;
    let demand = "SELECT count(*) FROM intake.recruiting_demand WHERE conversation_id = $1";
    let approved = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(CARDS_APPROVED_KIND)
        .fetch_one(&mut *db)
        .await;
    Left {
        status: status.expect("status"),
        audits: audits.expect("audits"),
        demand: rows(db, demand, request).await,
        shown: rows(
            db,
            "SELECT count(*) FROM quotes.shown_card WHERE job_id = $1",
            job,
        )
        .await,
        cards: rows(
            db,
            "SELECT count(*) FROM quotes.job_card WHERE job_id = $1",
            job,
        )
        .await,
        approved: approved.expect("approved"),
    }
}

/// LIC-017, LIC-018, INT-016, INT-017 (D-5081, D-1008): a move leaving Arizona and a home watch
/// request asking for rent collection, English and Spanish, get the one not-covered reply at the
/// trade step: the job Not covered, no card and no recruiting-demand row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn lic_017_lic_018_a_first_text_is_refused_at_the_trade_step(mut db: Db) {
    setup(&mut db).await;
    let texts = [
        "Need movers, Lake Havasu City to Las Vegas",
        "Necesito mudanza fuera del estado",
        "Home watch: check the house weekly and collect the rent",
        "Busco quien pueda cuidar la casa y cobrar la renta",
    ];
    for (n, first) in (301..).zip(texts) {
        let her = homeowner(&mut db, n).await;
        let before = walk(&mut db, her, &[first], 0).await;
        assert!(
            !before.contains(&NOT_COVERED_TEMPLATE),
            "{first}: {before:?}"
        );
        let told = walk(&mut db, her, &["86403"], 1).await;
        assert_eq!(told, [NOT_COVERED_TEMPLATE], "{first}");
        assert_eq!(left(&mut db, her).await, Left::refused(0), "{first}");
    }
}

/// LIC-017, LIC-018 (D-5081, D-5178): an out-of-state place or a property-management phrase that
/// first appears after EDIT gets the not-covered reply: the job Not covered, no further card
/// shown, nothing approved or sent and no demand row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn lic_017_lic_018_words_after_edit_are_refused_before_a_new_card(mut db: Db) {
    setup(&mut db).await;
    let cases = [
        (
            "Need movers next week",
            "actually the truck goes to Laughlin",
        ),
        (
            "Home watch while we are away",
            "and please find a tenant for it",
        ),
        ("Home watch while we are away", "tambien cobrar la renta"),
    ];
    for (n, (first, edit)) in (311..).zip(cases) {
        let her = homeowner(&mut db, n).await;
        let walked = to_card(&mut db, her, first, 4).await;
        assert_eq!(walked.last(), Some(&CARD_SHOWN_TEMPLATE), "{first}");
        assert!(
            !walked.contains(&NOT_COVERED_TEMPLATE),
            "{first}: {walked:?}"
        );
        let asked = walk(&mut db, her, &["EDIT"], 20).await;
        assert_eq!(asked, [CARD_EDIT_TEMPLATE], "{first}");
        let told = walk(&mut db, her, &[edit], 21).await;
        assert_eq!(told, [NOT_COVERED_TEMPLATE], "{edit}");
        assert_eq!(left(&mut db, her).await, Left::refused(1), "{edit}");
    }
}

/// Stores fake `words` as kept words of `her` one job without EDIT, as an earlier build's row or
/// another writer would leave them.
async fn add_words(db: &mut PgConnection, her: Uuid, words: &str) {
    let added = sqlx::query(
        "INSERT INTO quotes.job_description (request_id, job_id, message_id, words, recorded_at) \
         SELECT c.id, c.job_id, $2, $3, now() FROM intake.conversation c \
         JOIN intake.homeowner h ON h.id = c.homeowner_id WHERE h.contact_id = $1",
    )
    .bind(her)
    .bind(Uuid::new_v4())
    .bind(words)
    .execute(db)
    .await
    .expect("kept words");
    assert_eq!(added.rows_affected(), 1);
}

/// LIC-017, LIC-018 (D-5081, D-5143 point 4): a shown card whose request picked up refused words
/// without EDIT is not approved by SEND, OK, YES or ENVIAR: the not-covered reply, the job Not
/// covered, no `cards_approved` task and no card sent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn lic_017_lic_018_a_reply_to_a_shown_card_is_refused_before_it_is_read(mut db: Db) {
    setup(&mut db).await;
    let cases = [
        ("Need movers next week", "the truck goes to Vegas", "SEND"),
        (
            "Need movers next week",
            "nos mudamos a Nueva York",
            "ENVIAR",
        ),
        ("Home watch while we are away", "and collect the rent", "OK"),
        (
            "Home watch while we are away",
            "find a tenant for it",
            "YES",
        ),
    ];
    for (n, (first, extra, reply)) in (341..).zip(cases) {
        let her = homeowner(&mut db, n).await;
        let walked = to_card(&mut db, her, first, 4).await;
        assert_eq!(walked.last(), Some(&CARD_SHOWN_TEMPLATE), "{first}");
        add_words(&mut db, her, extra).await;
        let told = walk(&mut db, her, &[reply], 20).await;
        assert_eq!(told, [NOT_COVERED_TEMPLATE], "{extra}: {reply}");
        assert_eq!(left(&mut db, her).await, Left::refused(1), "{extra}");
    }
}

/// LIC-017 (D-5081): on the emergency path, a request whose kept words name a move out of Arizona
/// by the time the emergency-or-regular choice is answered is refused there: no card is stored,
/// shown or sent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn lic_017_the_emergency_or_regular_choice_is_refused(mut db: Db) {
    setup(&mut db).await;
    for (n, choice) in (351..).zip(["Emergency", "Regular"]) {
        let her = homeowner(&mut db, n).await;
        let walked = walk(&mut db, her, &["Need movers today", "86403", "1"], 0).await;
        assert_eq!(walked.last(), Some(&CHOICE_TEMPLATE), "{walked:?}");
        assert!(!walked.contains(&NOT_COVERED_TEMPLATE), "{walked:?}");
        add_words(&mut db, her, "the truck goes to Needles").await;
        let told = walk(&mut db, her, &[choice], 5).await;
        assert_eq!(told, [NOT_COVERED_TEMPLATE], "{choice}");
        assert_eq!(left(&mut db, her).await, Left::refused(0), "{choice}");
    }
}

/// LIC-017, INT-016 (D-5081): after a first text naming no trade, the trade answer "movers to Las
/// Vegas" is refused at the trade step.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn lic_017_a_trade_answer_naming_a_move_out_is_refused(mut db: Db) {
    setup(&mut db).await;
    let her = homeowner(&mut db, 321).await;
    let before = walk(&mut db, her, &["I need some help at my house", "86403"], 0).await;
    assert!(!before.contains(&NOT_COVERED_TEMPLATE), "{before:?}");
    let told = walk(&mut db, her, &["movers to Las Vegas"], 2).await;
    assert_eq!(told, [NOT_COVERED_TEMPLATE]);
    assert_eq!(left(&mut db, her).await, Left::refused(0));
}

/// LIC-017, LIC-018, INT-017 (D-1011, D-5178): a move inside Arizona, a bed size and turnover
/// cleaning "after the renters check out" still get the coverage line and their card.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn int_017_in_arizona_texts_still_get_the_coverage_line_and_a_card(mut db: Db) {
    setup(&mut db).await;
    let cases = [
        ("Need movers to Phoenix", 4),
        ("Need movers for a California king bed across town", 4),
        ("Turnover cleaning after the renters check out", 5),
    ];
    for (n, (first, options)) in (331..).zip(cases) {
        let her = homeowner(&mut db, n).await;
        let walked = to_card(&mut db, her, first, options).await;
        assert!(walked.contains(&COUNT_ONE_TEMPLATE), "{first}: {walked:?}");
        assert_eq!(walked.last(), Some(&CARD_SHOWN_TEMPLATE), "{first}");
        assert!(
            !walked.contains(&NOT_COVERED_TEMPLATE),
            "{first}: {walked:?}"
        );
        let after = left(&mut db, her).await;
        assert_eq!(
            (after.audits, after.demand, after.shown),
            (0, 0, 1),
            "{first}"
        );
        assert_ne!(after.status, "not_covered", "{first}");
    }
}

/// D-5160, D-5178: each new phrase reads its trade and its rule's job type, which the catalog
/// lists for the category; "moving an outlet" reads electrical only and house cleaning's
/// "limpieza de mudanza" does not read local moving.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn d_5160_the_keyword_rules_read_the_three_trades(mut db: Db) {
    load_embedded(&mut db).await.expect("catalog");
    let phrases: [&[&str]; 3] = [
        &[
            "movers",
            "moving help",
            "mudanza fuera",
            "necesito mudanza",
            "hacer una mudanza",
            "ayuda con mudanza",
        ],
        &[
            "home watch",
            "house check",
            "house checks",
            "cuidar la casa",
        ],
        &[
            "turnover clean",
            "turnover cleaning",
            "turnover cleaner",
            "airbnb cleaning",
            "vrbo cleaning",
        ],
    ];
    for ((trade, job), words) in RULES.into_iter().zip(phrases) {
        for phrase in words {
            let read = keyword_reading(&format!("Looking for {phrase} soon"));
            assert_eq!(read.trades, [trade], "{phrase}");
            assert_eq!(read.job_types, [(trade, job)], "{phrase}");
        }
        let listed: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM catalog.job_type j \
             JOIN catalog.category c ON c.id = j.category_id WHERE c.code = $1 AND j.name = $2)",
        )
        .bind(trade)
        .bind(job)
        .fetch_one(&mut *db)
        .await
        .expect("catalog job type");
        assert!(listed, "{trade}: {job}");
    }
    assert_eq!(keyword_reading("moving an outlet").trades, ["electrical"]);
    let cleaning = keyword_reading("limpieza de mudanza");
    assert!(!cleaning.trades.contains(&"local_moving"), "{cleaning:?}");
}
