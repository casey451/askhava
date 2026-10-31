//! TD-TXT-21 property and TD-TXT-06's gap edge (WORD-011, TXT-016, TXT-031, PICK-002, PICK-003,
//! TXT-017; rust brain R17): for any quote (firm, range, visit fee, recurring with any period,
//! tiers, hourly) the amounts and the period in the LIST text are exactly the stored ones, nothing
//! is converted or added, and an update is held until exactly 3 hours after the previous text.
//! Every name and amount here is made up.

use jiff::Timestamp;
use jiff::civil::date;
use proptest::prelude::*;

use crate::api::{
    DayPart, Listed, Notice, Period, Price, Reader, Sent, UPDATE_GAP, list_texts_from, notice,
};
use askhava_messaging::api::Language;

const READER: Reader = Reader {
    wanted: Language::En,
    spanish_on: false,
};
const PERIODS: [(Period, &str); 5] = [
    (Period::Visit, "per visit"),
    (Period::Week, "per week"),
    (Period::Month, "per month"),
    (Period::Quarter, "per quarter"),
    (Period::Year, "per year"),
];

fn price() -> impl Strategy<Value = Price> {
    let cents = 1_u64..5_000_000;
    prop_oneof![
        cents.clone().prop_map(Price::Firm),
        cents.clone().prop_map(Price::VisitFee),
        (cents.clone(), 1_u64..1000, any::<bool>()).prop_map(|(low, d, on_site)| Price::Range {
            low,
            high: low + d,
            on_site
        }),
        (
            cents.clone(),
            0..5_usize,
            proptest::option::of(cents.clone())
        )
            .prop_map(|(cents, p, setup)| Price::Monthly {
                cents,
                period: PERIODS[p].0,
                setup
            }),
        (cents.clone(), proptest::option::of((1_u32..9, 0_u32..4))).prop_map(|(rate, h)| {
            Price::Hourly {
                rate,
                hours: h.map(|(lo, d)| (lo, lo + d)),
            }
        }),
        proptest::collection::vec(cents, 1..=3).prop_map(|c| {
            Price::Tiers(
                c.into_iter()
                    .enumerate()
                    .map(|(i, v)| (format!("Plan{i}"), v))
                    .collect(),
            )
        }),
    ]
}

/// The dollar amounts in `text`, in cents, read back from the digits.
fn amounts(text: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find('$') {
        let tail = &rest[i + 1..];
        let end = tail
            .find(|c: char| !(c.is_ascii_digit() || c == ',' || c == '.'))
            .unwrap_or(tail.len());
        let num = tail[..end].trim_end_matches('.').replace(',', "");
        let (d, c) = num.split_once('.').unwrap_or((&num, "0"));
        let cents: u64 = if c.len() == 1 {
            c.parse::<u64>().unwrap_or(0) * 10
        } else {
            c.parse().unwrap_or(0)
        };
        out.push(d.parse::<u64>().unwrap_or(0) * 100 + cents);
        rest = &tail[end..];
    }
    out
}

/// The amounts a price holds, in the order the line states them.
fn expected(price: &Price) -> Vec<u64> {
    match price {
        Price::Firm(c) | Price::VisitFee(c) => vec![*c],
        Price::Range { low, high, .. } => vec![*low, *high],
        Price::Monthly { cents, setup, .. } => {
            [Some(*cents), *setup].into_iter().flatten().collect()
        }
        Price::Hourly { rate, .. } => vec![*rate],
        Price::Tiers(t) => t.iter().map(|(_, c)| *c).collect(),
    }
}

proptest! {
    /// TD-TXT-21: the amounts and the period in the LIST line equal the stored ones, and no other
    /// period or converted figure appears.
    #[test]
    fn the_list_line_states_the_stored_amount_and_period(p in price()) {
        let item = Listed {
            name: "Sample Business".to_owned(),
            price: p.clone(),
            earliest_visit: Some((date(2026, 11, 9), DayPart::Morning)),
        };
        let text = list_texts_from(READER, "1234", &[item]).expect("text");
        prop_assert_eq!(text.len(), 1);
        let line = text[0].lines().find(|l| l.starts_with("1. ")).expect("line").to_owned();
        prop_assert_eq!(amounts(&line), expected(&p));
        for (period, words) in PERIODS {
            let stated = matches!(&p, Price::Monthly { period: q, .. } if *q == period);
            prop_assert_eq!(line.contains(words), stated);
        }
    }

    /// TD-TXT-06: an update is held for any time before 3 hours and sent from exactly 3 hours.
    #[test]
    fn an_update_goes_from_exactly_three_hours(secs in 0_i64..40_000, more in 1_u32..20) {
        let last = Sent { at: Timestamp::UNIX_EPOCH, quoted: 1 };
        let now = Timestamp::from_second(secs).expect("time");
        let got = notice(1 + more, Some(last), now);
        if secs < UPDATE_GAP.as_secs() {
            let until = Timestamp::UNIX_EPOCH.checked_add(UPDATE_GAP).expect("time");
            prop_assert_eq!(got, Notice::Held { until });
        } else {
            prop_assert_eq!(got, Notice::Update { quoted: 1 + more });
        }
    }
}
