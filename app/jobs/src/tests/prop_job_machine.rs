//! Property tests of the job state machine (TD-JOB-01: JOB-001, JOB-002, JOB-003, JOB-005, JOB-006).
//!
//! Random runs of quotes, picks, confirms, declines, calls, replies, cancels, closes and clock
//! advances against an oracle copied by hand from the diagram in `docs/architecture.md` section 6:
//! one status at a time, only the oracle's arrows, one audit entry per change, and every waiting
//! status times out along an arrow. The timer firing cases with a database are in `transitions.rs`;
//! the per-status timer values are TD-JOB-03's (D-492).

use jiff::{SignedDuration, Timestamp};
use proptest::prelude::*;

use crate::api::JobStatus;
use crate::api::rules::{check_move, timeout};

/// The section 6 arrows, by code, written out independently of `JobStatus::targets`.
const ORACLE: &[(&str, &str)] = &[
    ("intake", "open"),
    ("intake", "not_covered"),
    ("intake", "closed_early"),
    ("open", "quotes_ready"),
    ("open", "no_quotes_yet"),
    ("open", "not_covered"),
    ("no_quotes_yet", "quotes_ready"),
    ("no_quotes_yet", "closed"),
    ("quotes_ready", "awaiting_confirm"),
    ("quotes_ready", "closed"),
    ("awaiting_confirm", "payment_pending"),
    ("awaiting_confirm", "quotes_ready"),
    ("payment_pending", "connected"),
    ("payment_pending", "quotes_ready"),
    ("connected", "done"),
    ("connected", "quotes_ready"),
    ("connected", "closed"),
];
const FINAL: &[&str] = &["done", "closed", "closed_early", "not_covered", "cancelled"];

fn oracle_allows(from: JobStatus, to: JobStatus) -> bool {
    let cancel = to.code() == "cancelled" && !FINAL.contains(&from.code());
    cancel || ORACLE.contains(&(from.code(), to.code()))
}

/// One step of a run: an event aiming at a status, or a clock advance in minutes.
#[derive(Debug, Clone, Copy)]
enum Step {
    Event(&'static str),
    Advance(i64),
}

/// Each event and the status it asks the job to move to.
const EVENTS: &[(&str, JobStatus)] = &[
    ("intake_done", JobStatus::Open),
    ("not_covered", JobStatus::NotCovered),
    ("quote", JobStatus::QuotesReady),
    ("pick", JobStatus::AwaitingConfirm),
    ("confirm", JobStatus::PaymentPending),
    ("paid", JobStatus::Connected),
    ("decline", JobStatus::QuotesReady),
    ("no_call", JobStatus::QuotesReady),
    ("reply_done", JobStatus::Done),
    ("reply_no", JobStatus::Closed),
    ("close", JobStatus::Closed),
    ("cancel", JobStatus::Cancelled),
];

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => (0..EVENTS.len()).prop_map(|i| Step::Event(EVENTS[i].0)),
        1 => (1i64..10_000).prop_map(Step::Advance),
    ]
}

fn target(event: &str) -> JobStatus {
    EVENTS.iter().find(|(e, _)| *e == event).expect("event").1
}

/// The machine under test: status, entry time and the audit entries its changes wrote.
struct Machine {
    status: JobStatus,
    entered: Timestamp,
    now: Timestamp,
    audit: Vec<(JobStatus, JobStatus)>,
}

impl Machine {
    fn try_move(&mut self, to: JobStatus) -> bool {
        let applied = check_move(self.status, to).is_ok();
        if applied {
            self.audit.push((self.status, to));
            (self.status, self.entered) = (to, self.now);
        }
        applied
    }

    /// Moves the clock on `minutes`, firing every timer due by then at its own instant (JOB-006).
    fn advance(&mut self, minutes: i64) {
        let end = self.now + SignedDuration::from_mins(minutes);
        while let Some(due) = timeout(self.status, self.entered, false) {
            if due.at > end {
                break;
            }
            self.now = due.at;
            assert!(self.try_move(due.then), "a timeout follows an arrow");
        }
        self.now = end;
    }
}

proptest! {
    /// TD-JOB-01: one status at a time, only the oracle's arrows, one audit entry per change.
    #[test]
    fn only_the_arrows_move_a_job(steps in proptest::collection::vec(step(), 1..60)) {
        let start: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
        let mut m = Machine { status: JobStatus::Intake, entered: start, now: start, audit: vec![] };
        let mut changes = 0;
        for s in steps {
            let before = m.status;
            match s {
                Step::Event(e) => {
                    let to = target(e);
                    let applied = m.try_move(to);
                    prop_assert_eq!(applied, oracle_allows(before, to));
                    changes += usize::from(applied);
                }
                Step::Advance(min) => {
                    let n = m.audit.len();
                    m.advance(min);
                    changes += m.audit.len() - n;
                }
            }
            prop_assert_eq!(JobStatus::from_code(m.status.code()), Some(m.status));
        }
        prop_assert_eq!(m.audit.len(), changes);
        for pair in m.audit.windows(2) {
            prop_assert_eq!(pair[0].1, pair[1].0);
        }
    }

    /// TD-JOB-01: every waiting status has a timeout after its entry that follows an arrow; a
    /// final status has none (JOB-003).
    #[test]
    fn every_waiting_status_times_out(secs in 1_767_225_600_i64..1_798_761_600, emergency: bool) {
        let at = Timestamp::from_second(secs).expect("instant");
        for status in JobStatus::ALL {
            match timeout(status, at, emergency) {
                Some(due) => {
                    prop_assert!(due.at > at);
                    prop_assert!(oracle_allows(status, due.then));
                }
                None => prop_assert!(FINAL.contains(&status.code())),
            }
        }
    }
}

/// TD-JOB-01: the arrows are exactly the oracle's, for all 144 pairs (JOB-002).
#[test]
fn the_arrows_are_the_architectures() {
    for from in JobStatus::ALL {
        for to in JobStatus::ALL {
            assert_eq!(
                from.can_move_to(to),
                oracle_allows(from, to),
                "{from:?} to {to:?}"
            );
        }
    }
}
