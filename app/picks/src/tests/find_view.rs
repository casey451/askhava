//! TD-PICK-53 (T318, D-2206, fourth audit F23): `service::find` gives the view of the job asked
//! for or none, never another job's view (another job's request number), and an empty list gives
//! none without a panic. Fake IDs only.

use sqlx::types::Uuid;

use crate::rules::{JobView, Stage};
use crate::service::find;

/// A ready job's view with request number `number`.
fn view(job: Uuid, number: i64) -> JobView {
    JobView {
        job,
        number,
        stage: Stage::Ready,
        paused: false,
        terms_sent: true,
        quotes: vec![true],
        waiting: None,
    }
}

/// TD-PICK-53: the job asked for is found among several.
#[test]
fn td_pick_53_the_job_asked_for_is_found() {
    let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
    let views = [view(a, 1042), view(b, 1043)];
    assert_eq!(find(&views, b).map(|v| (v.job, v.number)), Some((b, 1043)));
    assert_eq!(find(&views, a).map(|v| v.number), Some(1042));
}

/// TD-PICK-53: a job not among the views gives none, never the first view's number.
#[test]
fn td_pick_53_a_missing_job_gives_no_view_never_another_jobs() {
    let views = [view(Uuid::from_u128(1), 1042)];
    assert!(find(&views, Uuid::from_u128(9)).is_none());
}

/// TD-PICK-53: an empty list gives none and does not panic.
#[test]
fn td_pick_53_an_empty_list_gives_no_view_without_a_panic() {
    assert!(find(&[], Uuid::from_u128(1)).is_none());
}
