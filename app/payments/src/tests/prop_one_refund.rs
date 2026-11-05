//! TD-FEE-14 (property layer; FEE-026, D-1542): for any order of refund triggers, hourly retries,
//! Stripe answers and a dispute, `RefundStatus::next` sends nothing once the pick is refunded or
//! its charge disputed, never sends while a try is unanswered except that try's own retry, blocks
//! (and alerts) once, and a run whose tries all succeed ends refunded with one ledger entry.
//! The database cases are `tests/integration/tests/disputes.rs`.

use proptest::prelude::*;

use crate::api::{RefundStatus, RefundStep};

#[derive(Debug, Clone)]
enum Event {
    Trigger,
    Retry(usize),
    Answer(usize, bool),
    Dispute,
}

fn event() -> impl Strategy<Value = Event> {
    prop_oneof![
        3 => Just(Event::Trigger),
        3 => any::<usize>().prop_map(Event::Retry),
        4 => (any::<usize>(), any::<bool>()).prop_map(|(i, ok)| Event::Answer(i, ok)),
        1 => Just(Event::Dispute),
    ]
}

/// The refund row, the queued retries and unanswered tries, and Stripe, which refunds a charge at
/// most once and refuses any later refund of it.
#[derive(Default)]
struct Model {
    row: Option<(RefundStatus, i32)>,
    disputed: bool,
    in_flight: Vec<i32>,
    retries: Vec<i32>,
    stripe_refunded: bool,
    ledger: u32,
    alerts: u32,
}

fn status_of(row: Option<(RefundStatus, i32)>) -> Option<&'static str> {
    row.map(|(s, _)| s.code())
}

impl Model {
    fn step(&mut self, retry_of: Option<i32>) {
        let before = self.row;
        match RefundStatus::next(self.row, self.disputed, retry_of) {
            RefundStep::Send(n) => {
                assert!(!self.disputed, "sent on a disputed charge");
                assert_ne!(
                    status_of(before),
                    Some("succeeded"),
                    "sent after the refund"
                );
                assert_ne!(status_of(before), Some("blocked"), "sent after the block");
                if status_of(before) == Some("sending") {
                    assert_eq!(
                        retry_of,
                        before.map(|(_, t)| t),
                        "a second try while one is unanswered"
                    );
                }
                assert_eq!(n, before.map_or(0, |(_, t)| t) + 1, "tries count up by one");
                self.row = Some((RefundStatus::Sending, n));
                self.in_flight.push(n);
                self.retries.push(n);
            }
            RefundStep::Block(new) => {
                self.alerts += u32::from(new);
                self.row = Some((RefundStatus::Blocked, before.map_or(0, |(_, t)| t)));
            }
            RefundStep::Nothing => {}
        }
    }

    fn answer(&mut self, try_no: i32, ok: bool) {
        if ok && !self.stripe_refunded {
            self.stripe_refunded = true;
            self.ledger = 1;
            self.row = self.row.map(|(_, t)| (RefundStatus::Succeeded, t));
        } else if status_of(self.row) == Some("sending") && self.row.map(|(_, t)| t) == Some(try_no)
        {
            self.row = self.row.map(|(_, t)| (RefundStatus::Failed, t));
        }
    }

    fn run(&mut self, event: Event) {
        match event {
            Event::Trigger => self.step(None),
            Event::Retry(i) if !self.retries.is_empty() => {
                let t = self.retries.remove(i % self.retries.len());
                self.step(Some(t));
            }
            Event::Answer(i, ok) if !self.in_flight.is_empty() => {
                let t = self.in_flight.remove(i % self.in_flight.len());
                self.answer(t, ok);
            }
            Event::Dispute => self.disputed = true,
            Event::Retry(_) | Event::Answer(..) => {}
        }
    }
}

proptest! {
    /// TD-FEE-14: any interleaving keeps one refund, no send after a refund or dispute, one alert.
    #[test]
    fn td_fee_14_one_refund_whatever_the_order(events in prop::collection::vec(event(), 0..60)) {
        let mut model = Model::default();
        for e in events {
            model.run(e);
        }
        prop_assert!(model.ledger <= 1);
        prop_assert!(model.alerts <= 1);
        for _ in 0..2 {
            while let Some(t) = model.in_flight.pop() {
                model.answer(t, true);
            }
            model.step(None);
        }
        if model.row.is_some() && !model.disputed {
            prop_assert_eq!(model.ledger, 1);
        }
    }

    /// TD-FEE-14: a trigger replayed any number of times sends one try.
    #[test]
    fn td_fee_14_replayed_trigger_sends_once(times in 1_usize..20) {
        let mut model = Model::default();
        for _ in 0..times {
            model.step(None);
        }
        prop_assert_eq!(model.in_flight.len(), 1);
    }
}

/// TD-FEE-14: a failed try is sent again only by its own retry, then by a replayed trigger.
#[test]
fn td_fee_14_failed_try_is_retried() {
    let next = RefundStatus::next;
    let failed = Some((RefundStatus::Failed, 2));
    let blocked = Some((RefundStatus::Blocked, 2));
    let cases = [
        (next(failed, false, Some(2)), RefundStep::Send(3)),
        (next(failed, false, Some(1)), RefundStep::Nothing),
        (next(failed, false, None), RefundStep::Send(3)),
        (next(None, false, Some(1)), RefundStep::Nothing),
        (next(failed, true, None), RefundStep::Block(true)),
        (next(blocked, true, Some(2)), RefundStep::Block(false)),
    ];
    for (got, want) in cases {
        assert_eq!(got, want);
    }
    let unknown = RefundStatus::from_code("pending");
    assert!(unknown.is_none());
}
