//! TD-INT-22 (INT-040, AI-017, OPS-040, D-493): routing confidence end to end and the stored
//! golden runs. Fake request words and generated IDs only; ROC cover comes from the shipped map.
//!
//! An unsure plumbing job goes, marked "Job type not certain", only to a business whose license
//! covers every candidate job type, never to one covering part of it or outside the trade; with
//! none, nothing goes. A catalog version is run once and its share comes back through
//! `intake::api`.

use std::collections::BTreeSet;

use askhava_businesses::api::AccountStatus;
use askhava_intake::api::{
    Decision, NOT_CERTAIN, Reading, UnsureReach, after_fit, candidates, catalog_version, decide,
    golden_results, golden_rows, keyword_reading, run_golden, run_golden_for, unsure_reach,
};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_quotes::api::{CardBusiness, CardJob, OfferTown, OwnerPauses, Pool, Pooled, rules};
use jiff::Timestamp;
use sqlx::types::Uuid;

const TOWN: &str = "lake_havasu_city";
const WORDS: &str = "The drain is clogged and a pipe under it is leaking";

fn business(id: u128, class: &str) -> Pooled {
    let jobs = ["leak repair", "drain clearing", "AC not cooling"];
    Pooled {
        card: CardBusiness {
            id: Uuid::from_u128(id),
            status: AccountStatus::Active,
            verified: true,
            towns: BTreeSet::from([TOWN.to_owned()]),
            ticked: jobs
                .iter()
                .map(|j| {
                    (
                        if j.starts_with("AC") {
                            "hvac_repair"
                        } else {
                            "plumbing_repair"
                        },
                        *j,
                    )
                })
                .map(|(c, j)| (c.to_owned(), j.to_owned()))
                .collect(),
            classes: vec![class.to_owned()],
            licenses: Holder::default(),
        },
        overflow_on: false,
    }
}

fn card_job(candidates: &[(String, String)]) -> CardJob {
    CardJob {
        towns: vec![OfferTown {
            code: TOWN.to_owned(),
            state: "AZ".to_owned(),
        }],
        candidates: candidates
            .iter()
            .map(|(c, j)| Job {
                category: c.clone(),
                job_type: j.clone(),
                license: LicenseKind::Roc,
                handyman_allowed: true,
                listed: true,
            })
            .collect(),
        text: String::new(),
    }
}

fn reached(job: &CardJob, pool: &[Pooled]) -> Vec<u128> {
    let map = ClassMap::shipped().expect("the shipped license map loads");
    let pauses = OwnerPauses::default();
    let pool = Pool {
        businesses: pool,
        pauses: &pauses,
    };
    rules::match_job(map, job, &pool)
        .iter()
        .map(|r| r.business.as_u128())
        .collect()
}

/// TD-INT-22: two job types named, so the keywords are unsure and the trade question is asked;
/// still unsure after both, the card goes marked to the covering-all business only.
#[test]
fn td_int_22_unsure_job_goes_marked_only_to_a_business_covering_every_candidate() {
    let kw = keyword_reading(WORDS);
    assert!(matches!(decide(&kw, None, 0.8), Decision::Ask(_)));
    let plumbing = Reading {
        trade: "plumbing_repair",
        job_type: None,
    };
    let step = candidates("plumbing_repair", [plumbing, plumbing], &kw).fit_step("not sure", 1);
    let out = after_fit(&step).expect("nothing more is asked");
    assert!(out.not_certain, "the card is marked {NOT_CERTAIN}");
    assert_eq!(out.candidates.len(), 2);
    let job = card_job(&out.candidates);
    let pool = [
        business(1, "R-37"),
        business(2, "R-37R Sewers, Drains and Pipe Laying"),
        business(3, "R-39"),
    ];
    let got = reached(&job, &pool);
    assert_eq!(
        got,
        vec![1],
        "never part of the job, never outside the trade"
    );
    assert_eq!(unsure_reach(got.len()), UnsureReach::Cards(1));
    assert_eq!(
        unsure_reach(reached(&job, &pool[1..]).len()),
        UnsureReach::NoneCovers
    );
}

/// TD-INT-22, OPS-040, D-493: a catalog version is run once and stored; a changed version runs
/// again; `intake::api` returns the runs with their share.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_22_a_catalog_version_runs_the_golden_set_once_and_stores_its_share(
    pool: sqlx::PgPool,
) {
    let mut db = pool.acquire().await.expect("connection");
    let at: Timestamp = "2026-11-02T10:00:00-07:00".parse().expect("time");
    let first = run_golden(&mut db, at)
        .await
        .expect("run")
        .expect("a new version runs");
    assert_eq!(first.catalog_version, catalog_version());
    let rows = golden_rows().expect("golden rows");
    assert_eq!(usize::try_from(first.rows_total).ok(), Some(rows.len()));
    assert!(first.rows_right > 0 && first.rows_right <= first.rows_total);
    assert_eq!(
        run_golden(&mut db, at).await.expect("rerun"),
        None,
        "once per version"
    );
    let later = at
        .checked_add(jiff::SignedDuration::from_hours(1))
        .expect("time");
    let changed = run_golden_for(&mut db, "test-version-2", later)
        .await
        .expect("run");
    assert!(changed.is_some(), "a changed version runs again");
    let stored = golden_results(&mut db).await.expect("results");
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].catalog_version, "test-version-2");
    assert_eq!(
        stored[1].share_percent(),
        first.rows_right * 100 / first.rows_total
    );
}
