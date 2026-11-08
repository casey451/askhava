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
