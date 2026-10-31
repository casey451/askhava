//! The homeowner's quote texts in the database (TD-TXT-06, TD-TXT-11, TD-TXT-14, TD-TXT-20,
//! TD-TXT-21; TXT-016, TXT-017, TXT-031 to TXT-033, WORD-011, PICK-002, PICK-003). Fake IDs, names,
//! links and quotes only.
//!
//! The first-quote text and the 3-hour update gap (to the second), the LIST text in pages of 10
//! quotes in arrival order with its fixed ending in English and Spanish, amounts and periods as the
//! business stored them, the cards-out text with its Arizona-time clocks, and a stored amount that
//! only the business's own edit changes.

use askhava_jobs::api::{JobStatus, create, move_to};
use askhava_licensing::api::LicenseKind;
use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use askhava_quotes::api::{
    CardsOut, Counts, DayPart, Format, Notice, Period, Price, QuoteForm, QuoteRules, Reader,
    SLOW_AT_MOST, Sent, Submitted, arizona_time, cards_out_text, first_quote_text, list_texts,
    notice, notice_for, parse_quote, submit_quote, update_text, which_job_text,
};
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const LINK: &str = "https://askhava.com/q/sample-token";
const EN: Reader = Reader {
    wanted: Language::En,
    spanish_on: false,
};
const ES: Reader = Reader {
    wanted: Language::Es,
    spanish_on: true,
};

fn at(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

fn business(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

async fn open_job(db: &mut Db) -> Uuid {
    let job = create(db, at("02T21:00:00"), false, Actor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let _ = move_to(db, job, open, at("02T21:00:00"), Actor::System)
        .await
        .expect("open");
    job
}

/// Business `n` quotes `price` on `job` `minute` minutes after 9 a.m.
async fn quote(db: &mut Db, job: Uuid, (n, minute): (u128, u32), price: Price) {
    let format = match price {
        Price::Monthly { .. } => Format::Monthly,
        Price::Range { .. } => Format::Range,
        _ => Format::Firm,
    };
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3) ON CONFLICT DO NOTHING")
        .bind(job)
        .bind(business(n))
        .bind(at("03T08:00:00").to_sqlx())
        .execute(&mut **db)
        .await
        .expect("card");
    let form = QuoteForm {
        price,
        notes: String::new(),
        original: "Sample wording".to_owned(),
        ai_read: false,
        earliest_visit: Some((date(2026, 11, 9), DayPart::Evening)),
    };
    let rules = QuoteRules {
        format,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    };
    let checked = parse_quote(form, &rules).expect("quote");
    let when = at(&format!("03T09:{minute:02}:00"));
    let saved = submit_quote(db, (job, business(n)), &checked, when).await;
    assert_eq!(saved, Ok(Submitted::Saved));
}

fn names(count: u128) -> Vec<(Uuid, String)> {
    (1..=count)
        .map(|n| (business(n), format!("Sample Business {n}")))
        .collect()
}

/// TD-TXT-06 (TXT-016, TXT-017): the first quote gets the first-quote text; an update 2:59:59 after
/// the previous text's delivery is held until 3:00:00, which sends it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_06_first_quote_then_updates_at_most_every_three_hours(mut db: Db) {
    let job = open_job(&mut db).await;
    let now = at("03T10:00:00");
    assert_eq!(
        notice_for(&mut db, job, None, now).await,
        Ok(Notice::Nothing)
    );
    quote(&mut db, job, (1, 0), Price::Firm(19_000)).await;
    assert_eq!(notice_for(&mut db, job, None, now).await, Ok(Notice::First));
    quote(&mut db, job, (2, 1), Price::Firm(21_000)).await;
    let first = Sent {
        at: at("03T10:00:00"),
        quoted: 1,
    };
    let early = at("03T12:59:59");
    let held = Notice::Held {
        until: at("03T13:00:00"),
    };
    assert_eq!(notice_for(&mut db, job, Some(first), early).await, Ok(held));
    let on_time = at("03T13:00:00");
    let due = Notice::Update { quoted: 2 };
    assert_eq!(
        notice_for(&mut db, job, Some(first), on_time).await,
        Ok(due)
    );
    let told = Sent { quoted: 2, ..first };
    let again = notice_for(&mut db, job, Some(told), at("03T23:00:00")).await;
    assert_eq!(again, Ok(Notice::Nothing));
    assert_eq!(notice(0, None, now), Notice::Nothing);
}

/// TD-TXT-11, TD-TXT-14 (TXT-016, TXT-017, TXT-031): the first-quote and update texts hold their
/// fixed lines, with the LIST line before the link, in both languages.
#[test]
fn td_txt_11_first_quote_and_update_texts() {
    let en = first_quote_text(EN, "1234", LINK).expect("text");
    assert!(
        en.starts_with("Ask Hava: Your first quote for request 1234 is in, and more may come.")
    );
    assert!(en.contains("Free for you."));
    let (list, link) = (
        en.find("Reply LIST").expect("list"),
        en.find(LINK).expect("link"),
    );
    assert!(list < link);
    assert!(en.ends_with("Terms (18+): askhava.com/terms"));
    assert!(!en.contains('$'));
    let es = first_quote_text(ES, "1234", LINK).expect("text");
    assert!(es.contains("Responda LISTA"));
    assert!(es.find("Responda LISTA") < es.find(LINK));
    let counts = Counts {
        job: "1234",
        quoted: 4,
        sent: 6,
    };
    let up = update_text(EN, counts, LINK).expect("text");
    assert!(up.contains("You now have 4 quotes"));
    assert!(up.contains("4 of 6 businesses have quoted so far"));
}

/// TD-TXT-11, TD-TXT-14: 12 quotes make two texts of at most 10 lines in arrival order, each ending
/// with the "2 YES" line, the share sentence and the Terms line; LISTA is Spanish; two jobs ask which.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_11_list_pages_in_arrival_order(mut db: Db) {
    let job = open_job(&mut db).await;
    assert_eq!(
        list_texts(&mut db, (job, "1234"), EN, &names(12)).await,
        Ok(vec![])
    );
    for n in (1..=12_u128).rev() {
        let minute = u32::try_from(13 - n).expect("minute");
        quote(
            &mut db,
            job,
            (n, minute),
            Price::Firm(10_000 + u64::from(minute) * 100),
        )
        .await;
    }
    let texts = list_texts(&mut db, (job, "1234"), EN, &names(12))
        .await
        .expect("texts");
    assert_eq!(texts.len(), 2);
    let numbered = |t: &str| {
        t.lines()
            .filter(|l| l.starts_with(char::is_numeric))
            .count()
    };
    assert_eq!((numbered(&texts[0]), numbered(&texts[1])), (10, 2));
    assert!(texts[0].contains("1. Sample Business 12: $101."));
    assert!(texts[1].contains("11. Sample Business 2:"));
    let tail = "To pick, reply the number and YES, for example 2 YES. Picking shares your first name and job address with that business once it accepts.\nBy picking, you agree to the Terms: askhava.com/terms";
    assert!(texts.iter().all(|t| t.ends_with(tail)));
    let es = list_texts(&mut db, (job, "1234"), ES, &names(12))
        .await
        .expect("texts");
    assert!(es[0].contains("por ejemplo 2 SÍ."));
    assert!(es[0].contains("Primera visita: 9 nov, noche."));
    let which = which_job_text(EN, "1234").expect("text");
    assert!(which.contains("LIST 1234"));
}

/// TD-TXT-14, TD-TXT-21 (WORD-011, D-358): a weekly price stays weekly and a range stays a range,
/// character for character, and a stored amount changes only when the business edits it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_21_amounts_are_stored_words_until_the_business_edits(mut db: Db) {
    let job = open_job(&mut db).await;
    let weekly = Price::Monthly {
        cents: 4_000,
        period: Period::Week,
        setup: Some(2_500),
    };
    quote(&mut db, job, (1, 0), weekly).await;
    let range = Price::Range {
        low: 15_000,
        high: 25_050,
        on_site: true,
    };
    quote(&mut db, job, (2, 1), range).await;
    let read = |texts: Vec<String>| texts.concat();
    let before = read(
        list_texts(&mut db, (job, "1234"), EN, &names(2))
            .await
            .expect("texts"),
    );
    assert!(before.contains("1. Sample Business 1: $40 per week, plus $25 one-time setup."));
    assert!(
        before.contains("2. Sample Business 2: $150 to $250.50, Range to be confirmed on site.")
    );
    assert!(!before.contains("173") && !before.contains("per month"));
    let spanish = read(
        list_texts(&mut db, (job, "1234"), ES, &names(2))
            .await
            .expect("texts"),
    );
    assert!(spanish.contains("$40 por semana, más $25 de instalación única"));
    let _ = first_quote_text(EN, "1234", LINK).expect("text");
    let _ = notice_for(&mut db, job, None, at("03T10:00:00")).await;
    let again = read(
        list_texts(&mut db, (job, "1234"), EN, &names(2))
            .await
            .expect("texts"),
    );
    assert_eq!(before, again);
    quote(
        &mut db,
        job,
        (1, 5),
        Price::Monthly {
            cents: 4_500,
            period: Period::Week,
            setup: None,
        },
    )
    .await;
    let edited = read(
        list_texts(&mut db, (job, "1234"), EN, &names(2))
            .await
            .expect("texts"),
    );
    assert!(edited.contains("1. Sample Business 1: $45 per week."));
    let missing = list_texts(&mut db, (job, "1234"), EN, &names(1)).await;
    assert!(missing.is_err());
}

/// TD-TXT-11, TD-TXT-20, TD-TXT-14 (TXT-032, TXT-033): the cards-out text, held overnight, for a
/// "Today" job and for a small count, with every time in Arizona time.
#[test]
fn td_txt_20_cards_out_text() {
    let out = CardsOut {
        job: "1234",
        count: 2,
        trade: "plumbing",
        held_until: None,
        today: false,
        slow_at_most: SLOW_AT_MOST,
    };
    let plain = cards_out_text(EN, &out, &[]).expect("text");
    assert!(plain.starts_with("Ask Hava: Request 1234 is out to 2 plumbing businesses. Not every business answers. We'll text you when the first quote comes in."));
    assert!(plain.ends_with("Quotes may be slow with only 2 on Ask Hava so far."));
    let three = cards_out_text(EN, &CardsOut { count: 3, ..out }, &[]).expect("text");
    assert!(!three.contains("slow"));
    let held = CardsOut {
        held_until: Some("2026-11-04T08:00:00-07:00".parse().expect("time")),
        today: true,
        count: 5,
        ..out
    };
    let text = cards_out_text(EN, &held, &["America/Chicago".to_owned()]).expect("text");
    assert!(text.contains("It goes out at 8 a.m. Arizona time (9 a.m. Central)."));
    assert!(text.contains("You can pick as soon as the first quote arrives. If it can't wait, it's fine to call a company you know too."));
    assert_eq!(text.matches("Ask Hava:").count(), 1);
    let es = cards_out_text(ES, &held, &[]).expect("text");
    assert!(es.contains("Sale a las 8 a.m. hora de Arizona."));
}

/// TD-TXT-14 (TXT-033, D-293): a 928 number sees Arizona time only, a 612 number sees the Central
/// time too, and a zone showing the same clock adds nothing.
#[test]
fn td_txt_14_arizona_time_and_the_area_code_zone() {
    let winter: Timestamp = "2026-11-04T17:30:00-07:00".parse().expect("time");
    let phoenix = ["America/Phoenix".to_owned()];
    assert_eq!(
        arizona_time(winter, &phoenix, false).as_deref(),
        Ok("5:30 p.m. Arizona time")
    );
    let chicago = ["America/Chicago".to_owned()];
    assert_eq!(
        arizona_time(winter, &chicago, false).as_deref(),
        Ok("5:30 p.m. Arizona time (6:30 p.m. Central)")
    );
    let denver = ["America/Denver".to_owned()];
    assert_eq!(
        arizona_time(winter, &denver, false).as_deref(),
        Ok("5:30 p.m. Arizona time")
    );
    let summer: Timestamp = "2026-07-04T12:00:00-07:00".parse().expect("time");
    assert_eq!(
        arizona_time(summer, &denver, false).as_deref(),
        Ok("12 p.m. Arizona time (1 p.m. Mountain)")
    );
    assert!(arizona_time(winter, &["Nowhere/None".to_owned()], false).is_err());
}
