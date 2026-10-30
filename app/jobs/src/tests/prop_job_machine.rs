//! Property tests of the job state machine (TD-JOB-01: JOB-001, JOB-002, JOB-003, JOB-005, JOB-006).
//!
//! Random runs of quotes, picks, confirms, declines, calls, replies, cancels, closes and clock
//! advances against an oracle copied by hand from the diagram in `docs/architecture.md` section 6:
//! one status at a time, only the oracle's arrows, and every waiting status times out along an
//! arrow. The same random event runs also drive `move_to` against the database, which must write
//! exactly one history row and one audit row per applied change. The timer firing cases with a
//! database are in `transitions.rs`; the per-status timer values are TD-JOB-03's (D-492).

use askhava_core::api::Guarded;
use askhava_ops::api::Actor;
use jiff::{SignedDuration, Timestamp};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use sqlx::{PgConnection, PgPool};

use super::transitions::{audited_moves, history};
use crate::api::rules::{check_move, timeout};
use crate::api::{JobStatus, JobsError, create, current, move_to};

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

/// Drives one run of `events` through `move_to` from Intake and returns the changes the oracle
/// expects, checking each result and the stored status on the way (JOB-001, JOB-002).
async fn drive(
    db: &mut PgConnection,
    events: Vec<usize>,
) -> (sqlx::types::Uuid, Vec<(String, String)>) {
    let at: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
    let id = create(db, at, false, Actor::System).await.expect("job");
    let (mut status, mut changes) = (JobStatus::Intake, vec![]);
    for i in events {
        let to = EVENTS[i].1;
        let result = move_to(db, id, (status, to), at, Actor::System).await;
        if oracle_allows(status, to) {
            assert_eq!(result, Ok(Guarded::Applied(())));
            changes.push((status.code().to_owned(), to.code().to_owned()));
            status = to;
        } else {
            assert_eq!(
                result,
                Err(JobsError::RefusedMove(status.code(), to.code()))
            );
        }
        assert_eq!(
            current(db, id).await.expect("job").status.code(),
            status.code()
        );
    }
    (id, changes)
}

/// TD-JOB-01 against the database: random event runs through `move_to` write exactly one
/// `jobs.status_history` row and one `status_changed` audit row per applied change, in order, and
/// nothing for a refused one (JOB-002, JOB-005).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn every_applied_move_writes_one_audit_row(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let mut runner = TestRunner::deterministic();
    let runs = proptest::collection::vec(0..EVENTS.len(), 1..40);
    for _ in 0..24 {
        let events = runs.new_tree(&mut runner).expect("case").current();
        let (id, changes) = drive(&mut db, events).await;
        assert_eq!(audited_moves(&mut db, id).await, changes);
        assert_eq!(history(&mut db, id).await, changes);
    }
}
