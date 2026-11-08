//! The daily open-cards email (TD-CARD-10; CARD-022). Fake trades, towns, links and times only.
//!
//! The default 5 p.m. email lists both unquoted cards (18:30 and 21:00, D-1993) with trade, town, end time and Quote link;
//! 59 minutes left is not listed, 60 is; nothing to list, switched off, paused or busy sends
//! nothing; noon sends at 12:00; 8 a.m. and 5 p.m. are two emails and a third time can't be
//! chosen; a waiting pick names the subject and comes first; no marketing word; the windows
//! are unchanged.

use askhava_businesses::api::{OpenCardsTime, OpenCardsTimes};
use askhava_messaging::api::{DigestFacts, Language, MIN_LEFT, digest_goes, listable, slot_due};
use askhava_picks::api::{OpenCard, WaitingPick, chosen_minutes, open_cards_email};
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;
use sqlx::types::Uuid;

/// Arizona wall-clock time on November 9, 2026 (UTC-7 all year).
fn az(hour: i8, minute: i8) -> Timestamp {
    date(2026, 11, 9)
        .at(hour, minute, 0, 0)
        .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(-7)))
        .expect("time")
        .timestamp()
}

fn card(trade: &'static str, ends: Timestamp) -> OpenCard<'static> {
    OpenCard {
        trade,
        town: "Lake Havasu City",
        ends,
        quote_link: "https://askhava.com/b/a/q1",
    }
}

fn facts(chosen: Vec<u16>, card_ends: Vec<Timestamp>) -> DigestFacts {
    DigestFacts {
        chosen,
        paused: false,
        busy: false,
        card_ends,
        picks_waiting: 0,
    }
}

/// TD-CARD-10 (CARD-022): the 5 p.m. default lists both cards with their facts.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_10_the_default_5_pm_email_lists_both_cards(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let chosen = chosen_minutes(&mut db, Uuid::new_v4())
        .await
        .expect("times");
    assert_eq!(chosen, vec![1020]);
    // TD-CARD-10 says 17:30, which CARD-022's 1-hour rule would drop; 18:30 keeps both (D-1993).
    let ends = vec![az(18, 30), az(21, 0)];
    assert!(!digest_goes(
        &facts(chosen.clone(), ends.clone()),
        az(16, 59),
        MIN_LEFT
    ));
    assert!(digest_goes(
        &facts(chosen, ends.clone()),
        az(17, 0),
        MIN_LEFT
    ));
    let cards = [card("Plumbing", ends[0]), card("Electrical", ends[1])];
    let email = open_cards_email((&cards, &[]), az(17, 0), MIN_LEFT, Language::En).expect("email");
    assert_eq!(email.subject, "Ask Hava: Your job requests still open");
    assert_eq!(
        email.lines,
        [
            "Plumbing in Lake Havasu City. Quote by 6:30 p.m.: https://askhava.com/b/a/q1",
            "Electrical in Lake Havasu City. Quote by 9 p.m.: https://askhava.com/b/a/q1",
        ]
    );
    assert_eq!(cards[0].ends, az(18, 30), "the window is unchanged");
    for word in ["free", "offer", "deal", "save", "promo", "upgrade", "Hurry"] {
        assert!(
            !email
                .lines
                .iter()
                .chain([&email.subject])
                .any(|l| l.contains(word))
        );
    }
}

/// TD-CARD-10 (CARD-022): the 60-minute edge and every "nothing" case.
#[test]
fn td_card_10_edges_and_nothing_cases() {
    let now = az(17, 0);
    assert!(listable(now + SignedDuration::from_mins(60), now, MIN_LEFT));
    assert!(!listable(
        now + SignedDuration::from_mins(59),
        now,
        MIN_LEFT
    ));
    let close = [card("Plumbing", now + SignedDuration::from_mins(59))];
    assert!(open_cards_email((&close, &[]), now, MIN_LEFT, Language::En).is_none());
    assert!(
        !digest_goes(&facts(vec![1020], vec![]), now, MIN_LEFT),
        "no cards"
    );
    assert!(
        !digest_goes(&facts(vec![], vec![az(21, 0)]), now, MIN_LEFT),
        "switched off"
    );
    let mut paused = facts(vec![1020], vec![az(21, 0)]);
    paused.paused = true;
    assert!(!digest_goes(&paused, now, MIN_LEFT));
    let mut busy = facts(vec![1020], vec![az(21, 0)]);
    busy.busy = true;
    assert!(!digest_goes(&busy, now, MIN_LEFT));
}

/// TD-CARD-10 (CARD-022): noon sends at 12:00; 8 a.m. and 5 p.m. are two emails; no third time.
#[test]
fn td_card_10_chosen_times() {
    let noon = OpenCardsTime::Noon.minute();
    assert_eq!(slot_due(&[noon], az(12, 0)), Some(noon));
    assert_eq!(slot_due(&[noon], az(17, 0)), None);
    let two = [
        OpenCardsTime::EightAm.minute(),
        OpenCardsTime::FivePm.minute(),
    ];
    let late = vec![az(23, 0)];
    assert!(digest_goes(
        &facts(two.to_vec(), late.clone()),
        az(8, 0),
        MIN_LEFT
    ));
    assert!(digest_goes(&facts(two.to_vec(), late), az(17, 0), MIN_LEFT));
    assert!(
        OpenCardsTimes::new(&OpenCardsTime::ALL).is_none(),
        "a third time"
    );
}

/// TD-CARD-10 (CARD-022, D-345, D-380): a waiting pick names the subject and is listed first,
/// with its end time and Confirm link, for a "Confirm and billing" member.
#[test]
fn td_card_10_a_waiting_pick_comes_first() {
    let pick = WaitingPick {
        job: 1042,
        ends: az(19, 0),
        confirm_link: "https://askhava.com/b/a/c1",
    };
    let cards = [card("Plumbing", az(21, 0))];
    let email =
        open_cards_email((&cards, &[pick]), az(17, 0), MIN_LEFT, Language::En).expect("email");
    assert_eq!(
        email.subject,
        "Ask Hava: Pick waiting: confirm job 1042 by 7 p.m."
    );
    assert_eq!(
        email.lines[0],
        "Job 1042: a homeowner chose your quote. Confirm by 7 p.m.: https://askhava.com/b/a/c1"
    );
    let mut f = facts(vec![1020], vec![]);
    f.picks_waiting = 1;
    assert!(digest_goes(&f, az(17, 0), MIN_LEFT));
    let es = open_cards_email((&cards, &[pick]), az(17, 0), MIN_LEFT, Language::Es).expect("es");
    assert!(es.subject.contains("confirme el trabajo 1042"));
}

/// Arizona wall-clock time `days` after November 9, 2026 (a Monday).
fn az_on(days: i8, hour: i8, minute: i8) -> Timestamp {
    az(hour, minute) + SignedDuration::from_hours(24 * i64::from(days))
}

/// CARD-022, D-345: an end beyond tomorrow names its weekday or date, never "tomorrow"; Spanish
/// says "la 1 p.m."; a pick whose Confirm window already ended is not listed or named.
#[test]
fn td_card_10_end_days_spanish_one_oclock_and_ended_picks() {
    let link = "https://askhava.com/b/a/c1";
    let pick = |job, ends| WaitingPick {
        job,
        ends,
        confirm_link: link,
    };
    let ended = pick(1041, az(16, 0));
    let picks = [
        ended,
        pick(1042, az_on(1, 13, 0)),
        pick(1043, az_on(2, 9, 0)),
    ];
    let cards = [
        card("Plumbing", az_on(2, 17, 0)),
        card("Electrical", az_on(11, 13, 30)),
    ];
    let en = open_cards_email((&cards, &picks), az(17, 0), MIN_LEFT, Language::En).expect("en");
    assert_eq!(
        en.subject,
        "Ask Hava: Pick waiting: confirm job 1042 by 1 p.m. tomorrow"
    );
    assert_eq!(
        en.lines,
        [
            format!("Job 1042: a homeowner chose your quote. Confirm by 1 p.m. tomorrow: {link}"),
            format!("Job 1043: a homeowner chose your quote. Confirm by 9 a.m. Wednesday: {link}"),
            "Plumbing in Lake Havasu City. Quote by 5 p.m. Wednesday: https://askhava.com/b/a/q1"
                .to_owned(),
            "Electrical in Lake Havasu City. Quote by 1:30 p.m. Nov 20: https://askhava.com/b/a/q1"
                .to_owned(),
        ]
    );
    assert!(
        !en.lines.iter().any(|l| l.contains("1041")),
        "an ended pick"
    );
    let es = open_cards_email((&cards, &picks), az(17, 0), MIN_LEFT, Language::Es).expect("es");
    assert_eq!(
        es.subject,
        "Ask Hava: Selección en espera: confirme el trabajo 1042 antes de la 1 p.m. de mañana"
    );
    assert!(es.lines[1].contains("antes de las 9 a.m. del miércoles"));
    assert!(es.lines[3].contains("antes de la 1:30 p.m. del 20 de noviembre"));
    let only_ended = open_cards_email((&[], &[ended]), az(17, 0), MIN_LEFT, Language::En);
    assert!(only_ended.is_none(), "an ended pick alone sends nothing");
}
