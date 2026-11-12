//! A homeowner's wrong-license report pauses a pest business's pest jobs until a number that can
//! be found (TD-LIC-22's report case; T389, D-3055; LIC-047, D-417, D-2742).
//!
//! As the app role, on a fresh database: a pest attester's holder gives a general pest card; a
//! report pauses it (no pest card, general or termite) with one audit row by field name and one
//! owner-inbox alert on the business; a second report changes nothing; a malformed number is
//! refused and leaves it paused; a found number clears it and the card comes back; a business with
//! no pest row is untouched and gets no alert. All IDs and numbers are fake.

use askhava_businesses::api::create;
use askhava_integration::set_app_role;
use askhava_licensing::api::{
    Eligibility, HolderError, HolderInputs, Job, LicenseKind, NoCardReason,
    PEST_LICENSE_REPORTED_ALERT, PestAttestation, PestReplacement, PestReport, eligibility,
    holder_for, record_pest_attestation, replace_pest_number, report_pest_license_wrong,
};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

fn now() -> Timestamp {
    "2026-11-12T17:00:00Z".parse().expect("fake time")
}

async fn app(pool: &sqlx::PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

fn pest_job(job_type: &str) -> Job {
    Job {
        category: "pest_control".to_owned(),
        job_type: job_type.to_owned(),
        license: LicenseKind::StateAgency,
        handyman_allowed: false,
        listed: true,
    }
}

/// The eligibility of `business` for a general pest job and a termite job.
async fn cards(conn: &mut PgConnection, business: Uuid) -> [Eligibility; 2] {
    let parts = holder_for(conn, business, HolderInputs::default())
        .await
        .expect("holder");
    ["general pest service", "termites"].map(|t| eligibility(&pest_job(t), &parts.holder))
}

/// The pest row's `(number, paused)`.
async fn row(conn: &mut PgConnection, business: Uuid) -> (String, bool) {
    sqlx::query_as(
        "SELECT license_number, paused_at IS NOT NULL FROM licensing.business_license \
         WHERE business_id = $1",
    )
    .bind(business)
    .fetch_one(&mut *conn)
    .await
    .expect("pest row")
}

/// The `(action, field)` of every audit row on the business's licenses, oldest first.
async fn audits(conn: &mut PgConnection, business: Uuid) -> Vec<(String, Option<String>)> {
    sqlx::query_as(
        "SELECT a.action, a.field FROM ops.audit_log a \
         JOIN licensing.business_license l ON l.id = a.subject_id \
         WHERE l.business_id = $1 ORDER BY a.id",
    )
    .bind(business)
    .fetch_all(&mut *conn)
    .await
    .expect("audit rows")
}

async fn alerts(conn: &mut PgConnection, business: Uuid) -> Vec<(String, Option<String>)> {
    sqlx::query_as("SELECT kind, subject_kind FROM ops.alert WHERE subject_id = $1")
        .bind(business)
        .fetch_all(&mut *conn)
        .await
        .expect("alerts")
}

/// TD-LIC-22 (LIC-047, D-3055): a report pauses the attester's pest jobs, general and termite, with
/// one audit row and one owner-inbox alert; a second report changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_22_a_wrong_license_report_pauses_pest_jobs(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = create(&mut conn, false, now()).await.expect("business");
    let attested = PestAttestation::new(true, "PM-4242", true, false).expect("attestation");
    record_pest_attestation(&mut conn, id, &attested, Actor::Business(id))
        .await
        .expect("recorded");
    assert_eq!(cards(&mut conn, id).await, [Eligibility::Card; 2]);

    let report = report_pest_license_wrong(&mut conn, id, now()).await;
    assert_eq!(report, Ok(PestReport::Paused));
    let parts = holder_for(&mut conn, id, HolderInputs::default())
        .await
        .expect("holder");
    assert!(parts.holder.pest.expect("still attested").is_paused());
    let paused = Eligibility::NoCard(NoCardReason::PestPaused);
    assert_eq!(cards(&mut conn, id).await, [paused; 2]);
    let alerted = vec![(
        PEST_LICENSE_REPORTED_ALERT.to_owned(),
        Some("businesses.business".to_owned()),
    )];
    assert_eq!(alerts(&mut conn, id).await, alerted);

    let again = report_pest_license_wrong(&mut conn, id, now()).await;
    assert_eq!(again, Ok(PestReport::AlreadyPaused));
    assert_eq!(alerts(&mut conn, id).await, alerted, "no second alert");
    let logged = audits(&mut conn, id).await;
    assert_eq!(logged.len(), 2, "the sign-up row and one report row");
    assert_eq!(
        logged[1],
        (
            "licensing.pest_license_paused".to_owned(),
            Some("paused_at".to_owned())
        )
    );
}

/// TD-LIC-22 (LIC-047, D-3055, D-2742): a malformed number is refused and leaves the business
/// paused; a found number clears the pause and the card comes back, audited by field name only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_22_a_found_number_clears_the_pause(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = create(&mut conn, false, now()).await.expect("business");
    let attested = PestAttestation::new(true, "PM-4242", true, false).expect("attestation");
    record_pest_attestation(&mut conn, id, &attested, Actor::Business(id))
        .await
        .expect("recorded");
    let early = replace_pest_number(&mut conn, id, "PM-5151", now()).await;
    assert_eq!(
        early,
        Ok(PestReplacement::NotPaused),
        "no report, no change"
    );
    assert_eq!(row(&mut conn, id).await, ("PM-4242".to_owned(), false));
    report_pest_license_wrong(&mut conn, id, now())
        .await
        .expect("reported");

    for bad in ["", "PM 5151", "PM-5151-0000-0000-0000"] {
        let refused = replace_pest_number(&mut conn, id, bad, now()).await;
        assert_eq!(refused, Err(HolderError::BadRecord));
        assert_eq!(row(&mut conn, id).await, ("PM-4242".to_owned(), true));
    }

    let replaced = replace_pest_number(&mut conn, id, " PM-5151 ", now()).await;
    assert_eq!(replaced, Ok(PestReplacement::Replaced));
    assert_eq!(row(&mut conn, id).await, ("PM-5151".to_owned(), false));
    assert_eq!(cards(&mut conn, id).await, [Eligibility::Card; 2]);
    let logged = audits(&mut conn, id).await;
    let last = (
        "licensing.pest_number_replaced".to_owned(),
        Some("license_number".to_owned()),
    );
    assert_eq!(logged.last(), Some(&last));
    assert_eq!(logged.len(), 3, "sign-up, report, replacement");
    let leaked: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ops.audit_log WHERE field LIKE '%PM-%'")
            .fetch_one(&mut *conn)
            .await
            .expect("count");
    assert_eq!(leaked, 0, "no audit row holds a number");
}

/// TD-LIC-22 (LIC-047): a business with no pest row is untouched by a report or a number and
/// gets no alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_22_a_business_with_no_pest_row_is_untouched(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = create(&mut conn, false, now()).await.expect("business");
    let report = report_pest_license_wrong(&mut conn, id, now()).await;
    assert_eq!(report, Ok(PestReport::NoPestLicense));
    let replaced = replace_pest_number(&mut conn, id, "PM-5151", now()).await;
    assert_eq!(replaced, Ok(PestReplacement::NoPestLicense));
    assert!(alerts(&mut conn, id).await.is_empty(), "no alert");
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM licensing.business_license WHERE business_id = $1",
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await
    .expect("count");
    assert_eq!(rows, 0, "no row was made");
    let missing = Eligibility::NoCard(NoCardReason::AgencyMissing(
        askhava_licensing::api::Agency::PestManagement,
    ));
    assert_eq!(cards(&mut conn, id).await, [missing; 2]);
}
