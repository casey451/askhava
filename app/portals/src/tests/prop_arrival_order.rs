//! The quote page's arrival order and token life as properties (TD-PICK-04, TD-PICK-05; PICK-008,
//! PICK-009, PICK-011, PICK-012, D-084; rust-production R17). Fake IDs and wording only.
//!
//! Whatever order the rows come in, quotes are numbered 1, 2, 3 by time received; a later
//! arrival or a quote becoming "no longer available" never moves an earlier number; the rendered
//! page has no sort or filter control and puts "or reply <n> YES" beside every Pick button from
//! the first quote; the token works 29 days 23:59:59 after close and stops at 30 days.

use askama::Template;
use askhava_quotes::api::PageQuote;
use jiff::{SignedDuration, Timestamp};
use proptest::prelude::*;
use sqlx::types::Uuid;

use crate::quote_page::{Numbered, PageView, number_quotes, token_expired};

fn base() -> Timestamp {
    "2026-11-03T16:00:00Z".parse().expect("time")
}

fn quote(id: u128, minute: i64) -> PageQuote {
    PageQuote {
        business: Uuid::from_u128(id),
        received_at: base() + SignedDuration::from_mins(minute),
        unavailable: false,
        original: format!("Sample wording {id}"),
    }
}

fn numbers(rows: &[Numbered]) -> Vec<(Uuid, usize)> {
    rows.iter().map(|n| (n.quote.business, n.number)).collect()
}

fn arrivals() -> impl Strategy<Value = Vec<PageQuote>> {
    prop::collection::vec(0_i64..600, 1..12).prop_map(|minutes| {
        let ids = (1_u128..).zip(minutes);
        ids.map(|(id, m)| quote(id, m)).collect()
    })
}

proptest! {
    /// TD-PICK-04 (PICK-008): numbered from 1 by time received, whatever order the rows come in.
    #[test]
    fn td_pick_04_numbered_in_arrival_order(rows in arrivals(), seed in any::<u64>()) {
        let mut shuffled = rows.clone();
        shuffled.rotate_left(usize::try_from(seed % rows.len() as u64).unwrap_or(0));
        let numbered = number_quotes(shuffled);
        for (i, n) in numbered.iter().enumerate() {
            prop_assert_eq!(n.number, i + 1);
        }
        for pair in numbered.windows(2) {
            let (a, b) = (&pair[0].quote, &pair[1].quote);
            prop_assert!((a.received_at, a.business) < (b.received_at, b.business));
        }
    }

    /// TD-PICK-04 (PICK-008, PICK-009, D-084): a later quote arriving, or any quote becoming
    /// unavailable, never changes a number already shown.
    #[test]
    fn td_pick_04_numbers_never_change(rows in arrivals(), gone in any::<prop::sample::Index>()) {
        let before = numbers(&number_quotes(rows.clone()));
        let mut later = rows;
        later.push(quote(999, 601));
        let i = gone.index(later.len());
        later[i].unavailable = true;
        let after = number_quotes(later);
        prop_assert_eq!(&numbers(&after)[..before.len()], &before[..]);
        prop_assert_eq!(after.len(), before.len() + 1);
    }

    /// TD-PICK-05 (PICK-011): works before 30 days after close, refused from 30 days; an open job's
    /// token never stops on time.
    #[test]
    fn td_pick_05_token_life_after_close(secs in 0_i64..(60 * 86_400)) {
        let now = base() + SignedDuration::from_secs(secs);
        prop_assert_eq!(token_expired(true, base(), now), secs >= 30 * 86_400);
        prop_assert!(!token_expired(false, base(), now));
    }
}

/// TD-PICK-05 (PICK-011): the exact edges, 29 days 23:59:59 and 30 days.
#[test]
fn td_pick_05_token_edges() {
    let thirty = SignedDuration::from_hours(30 * 24);
    let edge = base() + thirty - SignedDuration::from_secs(1);
    assert!(!token_expired(true, base(), edge));
    assert!(token_expired(true, base(), base() + thirty));
}

/// TD-PICK-04 (PICK-012, PICK-015): with the job before a pick, every available quote from #1
/// has its Pick button and its own "or reply <n> YES"; an unavailable one has neither and says
/// "No longer available"; no sort or filter control exists.
#[test]
fn td_pick_04_pick_from_the_first_with_reply_line() {
    let mut rows = vec![quote(1, 0), quote(2, 5), quote(3, 9)];
    rows[1].unavailable = true;
    let view = PageView {
        request: 4821,
        still_open: false,
        live: true,
        pick_open: true,
        csrf: "fakecsrf".to_owned(),
        quotes: number_quotes(rows),
    };
    let html = view.render().expect("render");
    for n in [1, 3] {
        assert!(html.contains(&format!("Pick #{n}</button>")), "{n}");
        assert!(html.contains(&format!("or reply {n} YES")), "{n}");
    }
    assert!(!html.contains("Pick #2") && !html.contains("or reply 2 YES"));
    assert!(html.contains("No longer available"));
    for control in ["<select", "Sort", "Filter", "type=\"search\""] {
        assert!(!html.contains(control), "{control}");
    }
}
