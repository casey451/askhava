//! TD-INT-52 (T715, D-4441, D-4602): the corrections to T671's walker (`order_dispatch`), through
//! the route's entry, `intake::api::receive_raw_reply`. Kept as they were: the under-18 gate before
//! the first question (INT-038), no ZIP question on a 911-list or alarm-beep first text (INT-025,
//! INT-037), nothing while a SAFE hold waits (INT-005). Corrected: SAFE and an INT-037 answer are
//! never a ZIP answer and go to the open step, which asks the ZIP question's text again with no
//! `zip` row, so the one re-ask (INT-049) is neither used nor renewed; nothing after STOP
//! (CON-009); a two-word first text (INT-007) opens its one job on the ZIP's turn keyed with her
//! new property; a request whose words named no trade gets nothing at step 5 (no INT-016 reply,
//! no close, no step 11); a trade answer opens its job through `open_job`, never `begin_job`
//! (INT-009), the coverage line following (CON-031's first question stays the ZIP). PostgreSQL,
//! fake data and 555 numbers only (no phone number appears here at all).

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, request_welcome, save_business_name,
    set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_intake::api::rules::{
    CONSENT_TEMPLATE, Inbound, NAME_ADDRESS_TEMPLATE, SAFETY_TEMPLATE, Settings, UNDER_18_TEMPLATE,
};
use askhava_intake::api::{
    BEEP_TEMPLATE, COUNT_MANY_TEMPLATE, Handled, NOT_COVERED_TEMPLATE, RawReply, Reply, TextIn,
    ZIP_AGAIN_TEMPLATE, ZIP_QUESTION_TEMPLATE, receive_raw_reply,
};
use askhava_integration::set_app_role;
use askhava_licensing::api::import_city_list;
use askhava_messaging::api::{InboundText, Outcome, ProviderKeyword, Relation, on_inbound_text};
use askhava_ops::api::Actor;
use askhava_privacy::api::{NewContact, Vault, store_contact};
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
/// A first text naming one trade (`handyman`, job type `door repair`).
const DOOR: &str = "my front door is sticking badly";
/// A fake 555 number for the one test that goes through the opt-out list.
const PHONE: &str = "+19285550177";
/// A first text of more than two words that names no trade.
const NO_TRADE: &str = "can you help me out today";
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7201,Sample Hinge One LLC,,,2019-03-04\nCL-7202,Sample Hinge Two LLC,,,2019-03-04\n";
const NAMES: [&str; 2] = ["One", "Two"];

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x71), &key(0x72), &url).expect("vault")
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_100_000 + minute * 60).expect("time")
}

/// `words` from fake contact `contact` at `minute`, through the route's entry; `opt_out` marks a
/// STOP `messaging` already listed.
async fn post(
    db: &mut PgConnection,
    contact: u128,
    (words, opt_out): (&str, bool),
    minute: i64,
) -> Reply {
    let msg = TextIn {
        inbox_id: Uuid::from_u128(0x71_0000 + contact * 100 + minute.unsigned_abs() as u128),
        contact: Uuid::from_u128(contact),
        inbound: Inbound {
            words,
            opt_out,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    };
    let raw = SecretString::from(words.to_owned());
    let vault = vault();
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    receive_raw_reply(db, &secret(), (&msg, keys), (Settings::default(), &[]))
        .await
        .expect("reply")
}

/// The templates `words` queued, in order.
async fn sent(db: &mut PgConnection, contact: u128, words: &str, minute: i64) -> Vec<&'static str> {
    templates(post(db, contact, (words, false), minute).await)
}

fn templates(reply: Reply) -> Vec<&'static str> {
    match reply {
        Reply::Handled(Handled { texts, .. }) | Reply::NameAddress(_, Handled { texts, .. }) => {
            texts.iter().map(|q| q.template).collect()
        }
        other => panic!("handled, got {other:?}"),
    }
}

/// The catalog, the fake city list and the job-link watch, then the app role.
async fn setup(db: &mut PgConnection) {
    load_embedded(db).await.expect("catalog");
    for sql in WATCH {
        sqlx::query(sql).execute(&mut *db).await.expect(sql);
    }
    import_city_list(db, date(2026, 11, 1), CITY)
        .await
        .expect("city list");
    set_app_role(db).await.expect("SET ROLE askhava_app");
}

/// The `n`th (0 or 1) active, welcomed handyman business (LIC-031) serving the town, door repair.
async fn business(db: &mut PgConnection, n: usize) {
    let b = create(db, false, at(0)).await.expect("business");
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
         VALUES ($1, $2, now(), ARRAY['handyman'])",
    )
    .bind(b)
    .bind(Uuid::new_v4())
    .execute(&mut *db)
    .await
    .expect("signed up as a handyman");
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
    let name = format!("Sample Hinge {} LLC", NAMES[n]);
    save_business_name(db, b, (&name, None), None)
        .await
        .expect("name");
    let actor = Actor::Business(b);
    set_towns(db, b, &[TOWN], actor).await.expect("towns");
    let choice = JobTypeChoice::new("handyman", "door repair");
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

/// Test-only: every job link written is copied to `public.seen_link`, so a link that a later
/// not-covered close deletes (D-4602 leaves that close to T716 and T723) is still seen.
const WATCH: [&str; 4] = [
    "CREATE TABLE public.seen_link (conversation_id uuid NOT NULL, question_id text NOT NULL)",
    "GRANT SELECT ON public.seen_link TO askhava_app",
    "CREATE FUNCTION public.see_link() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$ \
     BEGIN INSERT INTO public.seen_link VALUES (NEW.conversation_id, NEW.question_id); \
     RETURN NEW; END $$",
    "CREATE TRIGGER see_link AFTER INSERT ON intake.intake_answer FOR EACH ROW \
     WHEN (NEW.question_id LIKE 'job.%') EXECUTE FUNCTION public.see_link()",
];
/// Every job link request `id` ever wrote.
const SEEN: &str = "SELECT question_id FROM public.seen_link WHERE conversation_id = $1 \
                    ORDER BY question_id";
const LINKS: &str = "SELECT question_id FROM intake.intake_answer WHERE conversation_id = $1 \
                     AND question_id LIKE 'job.%' ORDER BY question_id";
const ZIP_ROWS: &str = "SELECT count(*) FROM intake.intake_answer WHERE conversation_id = $1 \
                        AND question_id = 'zip'";
const PROPERTIES: &str = "SELECT count(*) FROM intake.property p JOIN intake.conversation c \
                          ON c.homeowner_id = p.homeowner_id WHERE c.id = $1";
const KEPT: &str = "SELECT count(*) FROM quotes.job_description WHERE request_id = $1";
const CLOSED: &str = "SELECT count(*) FROM intake.conversation WHERE id = $1 \
                      AND closed_at IS NOT NULL";

async fn strings(db: &mut PgConnection, sql: &'static str, id: Uuid) -> Vec<String> {
    let rows = sqlx::query_scalar(sql).bind(id).fetch_all(db).await;
    rows.expect(sql)
}

async fn count(db: &mut PgConnection, sql: &'static str, id: Uuid) -> i64 {
    let n = sqlx::query_scalar(sql).bind(id).fetch_one(db).await;
    n.expect(sql)
}

async fn first_job(db: &mut PgConnection, id: Uuid) -> Option<Uuid> {
    sqlx::query_scalar("SELECT job_id FROM intake.conversation WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
        .expect("job")
}

/// The job link `trade` gets once keyed with request `id`'s new property (INT-009).
async fn keyed_link(db: &mut PgConnection, id: Uuid, trade: &str) -> String {
    let property: Uuid = sqlx::query_scalar(
        "SELECT p.id FROM intake.property p JOIN intake.conversation c \
         ON c.homeowner_id = p.homeowner_id WHERE c.id = $1 ORDER BY p.created_at DESC LIMIT 1",
    )
    .bind(id)
    .fetch_one(db)
    .await
    .expect("property");
    format!("job.{trade}.{}", &property.simple().to_string()[..12])
}

/// TD-INT-52 (INT-038, INT-025, INT-037, CON-031): the first texts that never get the ZIP
/// question: an under-18 text gets consent then INT-038's text, a 911-list text its safety text,
/// an alarm-beep text the INT-037 question.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_52_first_texts_that_get_no_zip_question(mut db: Db) {
    setup(&mut db).await;
    let cases: [(u128, &str, &[&str]); 3] = [
        (
            0x0555_0201,
            "I'm 15 and my front door sticks",
            &[CONSENT_TEMPLATE, UNDER_18_TEMPLATE],
        ),
        (
            0x0555_0202,
            "I smell gas in the kitchen",
            &[SAFETY_TEMPLATE, CONSENT_TEMPLATE],
        ),
        (
            0x0555_0203,
            "my smoke alarm is beeping",
            &[CONSENT_TEMPLATE, BEEP_TEMPLATE],
        ),
    ];
    for (contact, words, expected) in cases {
        let got = sent(&mut db, contact, words, 0).await;
        assert_eq!(got, expected, "{words}");
        assert!(!got.contains(&ZIP_QUESTION_TEMPLATE), "{words}");
    }
}

/// TD-INT-52 (INT-005, INT-025, INT-049): on a 911-list first text a ZIP before SAFE answers
/// nothing (no text, no property), and SAFE then gets the ZIP question.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_52_a_zip_before_safe_answers_nothing(mut db: Db) {
    setup(&mut db).await;
    sent(&mut db, 0x0555_0204, "I smell gas in the kitchen", 0).await;
    let early = sent(&mut db, 0x0555_0204, HOME_ZIP, 1).await;
    assert!(early.is_empty(), "{early:?}");
    let id = request(&mut db, 0x0555_0204).await;
    assert_eq!(count(&mut db, PROPERTIES, id).await, 0, "no ZIP stored");
    assert_eq!(count(&mut db, ZIP_ROWS, id).await, 0, "not asked yet");
    assert_eq!(
        sent(&mut db, 0x0555_0204, "SAFE", 2).await,
        [ZIP_QUESTION_TEMPLATE]
    );
    assert_eq!(count(&mut db, ZIP_ROWS, id).await, 1, "asked once");
}

/// TD-INT-52 (INT-025, INT-049, D-4441 (d)): a 911-list match while the ZIP question is open gets
/// only the INT-004 text; SAFE gets the ZIP question's text again with no new `zip` row; the next
/// unusable reply still gets the one re-ask and a second gets nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_52_safe_asks_the_zip_again_with_no_row(mut db: Db) {
    setup(&mut db).await;
    let first = sent(&mut db, 0x0555_0205, DOOR, 0).await;
    assert_eq!(first, [CONSENT_TEMPLATE, ZIP_QUESTION_TEMPLATE]);
    let id = request(&mut db, 0x0555_0205).await;
    let gas = sent(&mut db, 0x0555_0205, "now I smell gas in here", 1).await;
    assert_eq!(gas, [SAFETY_TEMPLATE]);
    let safe = sent(&mut db, 0x0555_0205, "SAFE", 2).await;
    assert_eq!(safe, [ZIP_QUESTION_TEMPLATE]);
    assert_eq!(count(&mut db, ZIP_ROWS, id).await, 1, "no new zip row");
    let again = sent(&mut db, 0x0555_0205, "the blue house by the lake", 3).await;
    assert_eq!(again, [ZIP_AGAIN_TEMPLATE], "the re-ask is still unused");
    let waited = sent(&mut db, 0x0555_0205, "the one with the boat", 4).await;
    assert!(waited.is_empty(), "{waited:?}");
}

/// TD-INT-52 (INT-037, INT-025): after an alarm-beep first text "just chirping" gets the ZIP
/// question; "2" gets the INT-004 text and, after SAFE, the ZIP question.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_52_an_alarm_answer_goes_to_the_zip_question(mut db: Db) {
    setup(&mut db).await;
    let beep = "my smoke alarm is beeping";
    sent(&mut db, 0x0555_0206, beep, 0).await;
    let chirp = sent(&mut db, 0x0555_0206, "just chirping", 1).await;
    assert_eq!(chirp, [ZIP_QUESTION_TEMPLATE]);
    sent(&mut db, 0x0555_0207, beep, 0).await;
    assert_eq!(sent(&mut db, 0x0555_0207, "2", 1).await, [SAFETY_TEMPLATE]);
    assert_eq!(
        sent(&mut db, 0x0555_0207, "SAFE", 2).await,
        [ZIP_QUESTION_TEMPLATE]
    );
}

/// TD-INT-52 (INT-037, INT-049, D-4441 (c)): the INT-037 question asked while the ZIP question is
/// open, answered "1", gets the ZIP question's text again with no new `zip` row, and the re-ask
/// is still unused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_52_an_alarm_answer_is_never_a_zip_answer(mut db: Db) {
    setup(&mut db).await;
    sent(&mut db, 0x0555_0208, DOOR, 0).await;
    let id = request(&mut db, 0x0555_0208).await;
    let beep = sent(&mut db, 0x0555_0208, "also the smoke alarm is beeping", 1).await;
    assert_eq!(beep, [BEEP_TEMPLATE]);
    let one = sent(&mut db, 0x0555_0208, "1", 2).await;
    assert_eq!(one, [ZIP_QUESTION_TEMPLATE]);
    assert_eq!(count(&mut db, ZIP_ROWS, id).await, 1, "no new zip row");
    let again = sent(&mut db, 0x0555_0208, "the blue house by the lake", 3).await;
    assert_eq!(again, [ZIP_AGAIN_TEMPLATE], "the re-ask is still unused");
}

/// TD-INT-52 (CON-009, CON-013, CON-016, D-4529): after STOP a later text gets nothing. Both go
/// as the route sends them (`app/server/src/inbound_text.rs`): `messaging::api::on_inbound_text`
/// first, `receive_raw_reply` only while the number is not listed; nothing new is queued for her.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_52_after_stop_a_later_text_gets_nothing(mut db: Db) {
    setup(&mut db).await;
    let vault = vault();
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(PHONE.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(&mut db, &vault, &new).await.expect("contact");
    let first = routed(&mut db, (&vault, her), (DOOR, ProviderKeyword::None), 0).await;
    assert_eq!(first, [CONSENT_TEMPLATE, ZIP_QUESTION_TEMPLATE]);
    let stop = routed(&mut db, (&vault, her), ("STOP", ProviderKeyword::OptOut), 1).await;
    assert!(stop.is_empty(), "{stop:?}");
    let queued = "SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1";
    let before = count(&mut db, queued, her).await;
    let later = routed(&mut db, (&vault, her), (HOME_ZIP, ProviderKeyword::None), 2).await;
    assert!(later.is_empty(), "{later:?}");
    assert_eq!(
        count(&mut db, queued, her).await,
        before,
        "nothing queued after STOP"
    );
}

/// `words` from vault contact `her` (phone [`PHONE`]) at `minute`, as the route sends it: the
/// opt-out list first, intake only for a number it leaves open (CON-008, CON-016).
async fn routed(
    db: &mut PgConnection,
    (vault, her): (&Vault, Uuid),
    (words, provider): (&str, ProviderKeyword),
    minute: i64,
) -> Vec<&'static str> {
    let inbound = InboundText {
        inbox_id: Uuid::from_u128(0x71_9900 + minute.unsigned_abs() as u128),
        contact: her,
        relation: Relation::OpenRequest,
        phone: PHONE,
        text: words,
        provider,
        emergency: false,
    };
    let outcome = on_inbound_text(db, (&secret(), vault), &inbound, at(minute)).await;
    if outcome.expect("inbound") != Outcome::Open {
        return Vec::new();
    }
    let msg = TextIn {
        inbox_id: inbound.inbox_id,
        contact: her,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    };
    let raw = SecretString::from(words.to_owned());
    let keys = RawReply { raw: &raw, vault };
    let reply = receive_raw_reply(db, &secret(), (&msg, keys), (Settings::default(), &[])).await;
    templates(reply.expect("reply"))
}

/// TD-INT-52 (INT-007, INT-009, D-4602): a two-word first text naming a trade opens no job and
/// keeps no words; the ZIP's turn opens exactly one job, keyed with her new property. The text
/// that turn sends is TD-INT-53's and is not asserted here.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_52_a_two_word_trade_opens_its_job_on_the_zip_turn(mut db: Db) {
    setup(&mut db).await;
    let first = sent(&mut db, 0x0555_0210, "sticky door", 0).await;
    assert_eq!(first, [CONSENT_TEMPLATE, ZIP_QUESTION_TEMPLATE]);
    let id = request(&mut db, 0x0555_0210).await;
    assert!(strings(&mut db, SEEN, id).await.is_empty(), "no job yet");
    assert_eq!(first_job(&mut db, id).await, None);
    sent(&mut db, 0x0555_0210, HOME_ZIP, 1).await;
    let link = keyed_link(&mut db, id, "handyman").await;
    assert_eq!(strings(&mut db, SEEN, id).await, [link], "exactly one job");
    assert!(first_job(&mut db, id).await.is_some(), "her one job");
    assert_eq!(count(&mut db, KEPT, id).await, 0, "no words kept");
}

/// TD-INT-52 (INT-016, INT-043, D-4441 (5)): a request whose words named no trade gets, on its
/// ZIP's turn, no job, no INT-016 reply, no close and no name-and-address question.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_52_no_trade_gets_nothing_at_step_5(mut db: Db) {
    setup(&mut db).await;
    sent(&mut db, 0x0555_0211, NO_TRADE, 0).await;
    let id = request(&mut db, 0x0555_0211).await;
    let got = sent(&mut db, 0x0555_0211, HOME_ZIP, 1).await;
    assert!(got.is_empty(), "{got:?}");
    assert!(!got.contains(&NOT_COVERED_TEMPLATE));
    assert!(!got.contains(&NAME_ADDRESS_TEMPLATE));
    assert_eq!(count(&mut db, PROPERTIES, id).await, 1, "her ZIP stored");
    assert!(strings(&mut db, SEEN, id).await.is_empty(), "no job");
    assert_eq!(count(&mut db, CLOSED, id).await, 0, "still open");
}

/// TD-INT-52 (INT-049, INT-016, INT-009, D-4441 (4)): a ZIP reply that also names a trade stores
/// the property and opens the job through `open_job`, keyed with that property, with the
/// coverage line; its words are never kept (INT-049).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_52_a_zip_reply_naming_a_trade_opens_its_job(mut db: Db) {
    setup(&mut db).await;
    business(&mut db, 0).await;
    business(&mut db, 1).await;
    sent(&mut db, 0x0555_0212, NO_TRADE, 0).await;
    let id = request(&mut db, 0x0555_0212).await;
    let got = sent(&mut db, 0x0555_0212, "86403 my front door is sticking", 1).await;
    assert_eq!(got.first(), Some(&COUNT_MANY_TEMPLATE), "{got:?}");
    assert_eq!(count(&mut db, PROPERTIES, id).await, 1);
    let link = keyed_link(&mut db, id, "handyman").await;
    assert_eq!(strings(&mut db, LINKS, id).await, [link]);
    assert!(first_job(&mut db, id).await.is_some());
    assert_eq!(count(&mut db, KEPT, id).await, 0, "no ZIP reply kept");
}

/// TD-INT-52 (INT-009, INT-007, D-4441 (a)): a later one-word trade answer ("plumbing") opens its
/// job with no `begin_job`: never refused for its length, and never joined to the active
/// same-trade job of her earlier request.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_52_a_one_word_trade_answer_opens_a_new_job(mut db: Db) {
    setup(&mut db).await;
    sent(&mut db, 0x0555_0213, "my kitchen sink is leaking", 0).await;
    let earlier = request(&mut db, 0x0555_0213).await;
    let old_job = first_job(&mut db, earlier).await.expect("old job");
    let close =
        "UPDATE intake.conversation SET closed_at = now() WHERE id = $1 RETURNING 1::bigint";
    sqlx::query("RESET ROLE")
        .execute(&mut *db)
        .await
        .expect("reset");
    assert_eq!(
        count(&mut db, close, earlier).await,
        1,
        "the earlier request"
    );
    set_app_role(&mut db).await.expect("role");
    sent(&mut db, 0x0555_0213, NO_TRADE, 10).await;
    sent(&mut db, 0x0555_0213, HOME_ZIP, 11).await;
    let id = request(&mut db, 0x0555_0213).await;
    assert_ne!(id, earlier);
    sent(&mut db, 0x0555_0213, "plumbing", 12).await;
    let job = first_job(&mut db, id).await.expect("her new job");
    assert_ne!(job, old_job, "another request's job is never joined");
    let link = keyed_link(&mut db, id, "plumbing_repair").await;
    assert_eq!(strings(&mut db, SEEN, id).await, [link], "its own new job");
}
