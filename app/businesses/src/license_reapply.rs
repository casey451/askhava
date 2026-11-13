//! The daily license-map re-apply and the way back from a `no_covered_job_type` pause (T372,
//! D-2705, D-2596, D-2711, D-2730; BIZ-050, LIC-043, BIZ-009, BIZ-012, LIC-047, CARD-015).
//!
//! [`REAPPLY_KIND`] runs once a day at [`RUN_HOUR`] Arizona time, after the daily lists are
//! recorded; each run first queues the next day's run ([`schedule_reapply`]), then
//! [`apply_lists`] applies every recorded ROC list it has not applied yet, oldest first, so a class
//! removed on a day the task did not run is never lost. A list is marked applied by one
//! [`LIST_APPLIED`] audit row on its `licensing.list_import` row (no new table); the first list
//! ever recorded has nothing before it and is only marked, and so is a list recorded late with a
//! date older than one already applied (D-3356). For each list, `licensing::api::class_changes`
//! gives, [`BATCH`] license numbers at a time, each number whose Active classes differ from the
//! ones on the last earlier list it had any on (D-3355; a number missing from the newer list or
//! lapsed is no change, and is compared with its last Active classes when it returns); every
//! business whose latest daily check is under a changed number gets
//! `capability::reapply_license` with the old and new classes and the list date, whose dedupe key
//! (D-2596) makes a re-run for the same list queue nothing new. Who records the lists (T371) does
//! not matter here.
//!
//! [`lift_no_cover`] is the only way back from a `no_covered_job_type` pause (D-2711 as narrowed by
//! D-2730): called by `save_job_types` and `save_capability` in their transaction, it lifts only
//! when the rebuilt offer and the stored ticks share a licensed job type, and re-runs the other
//! pause rules first: the complaint count, a pest license reported wrong and the latest daily
//! check; the first that holds keeps the business paused with its reason. A license pause that
//! replaced the owner's own pause returns the business to that pause; otherwise it becomes active.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use askhava_catalog::api::{Catalog, parse};
use askhava_core::api::{HandlerError, NewTask, enqueue};
use askhava_licensing::api::{
    ClassMap, HolderParts, RocList, bound_businesses, class_changes, latest_check_active, roc_lists,
};
use askhava_ops::api::{Actor, AuditEntry, Change, audit, audit_once, audited, field_changed_last};
use jiff::Timestamp;
use jiff::tz::{TimeZone, offset};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::capability::{TradeOffer, reapply_license};
use crate::complaints::{PAUSE_COUNT, PAUSE_WINDOW_DAYS};
use crate::models::{self, BusinessesError, db};
use crate::rules::Event;
use crate::service::apply;
use crate::status::{AccountStatus, PauseReason};

/// The `core.task` kind of the daily re-apply (BIZ-050).
pub const REAPPLY_KIND: &str = "businesses.license_reapply";
/// The audit action marking one recorded ROC list applied; its subject is the list's import row.
pub const LIST_APPLIED: &str = "businesses.license_list_applied";
/// The hour (Arizona time) the daily run is due, after the daily lists are recorded.
pub const RUN_HOUR: i8 = 7;
/// How many changed license numbers one batch reads.
pub const BATCH: i64 = 200;
/// The audit subject kind of a recorded list.
const LIST_SUBJECT: &str = "licensing.list_import";
/// The handler's failure sentence (D-491).
const FAILED: HandlerError = HandlerError("the license re-apply database call failed");
/// The category catalog compiled into the build, the one `catalog::api::load_embedded` stores.
const CATEGORIES: &str = include_str!("../../../docs/catalog/categories.yaml");

/// The compiled-in catalog and the shipped license map, read once (LIC-043, LIC-002).
pub(crate) fn shipped() -> Result<(&'static ClassMap, &'static Catalog), BusinessesError> {
    static CATALOG: OnceLock<Option<Catalog>> = OnceLock::new();
    let catalog = CATALOG.get_or_init(|| parse(CATEGORIES).ok()).as_ref();
    let map = ClassMap::shipped().ok();
    map.zip(catalog).ok_or(BusinessesError::Database)
}

/// BIZ-050, JOB-006: queues the run due at the first [`RUN_HOUR`] Arizona time after `now`, once
/// per day (dedupe key by its date).
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn schedule_reapply(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<(), BusinessesError> {
    let arizona = TimeZone::fixed(offset(-7));
    let mut day = now.to_zoned(arizona.clone()).date();
    let due = loop {
        let at = day.at(RUN_HOUR, 0, 0, 0).to_zoned(arizona.clone());
        let at = at.map_err(|_| BusinessesError::Database)?.timestamp();
        if at > now {
            break at;
        }
        day = day.tomorrow().map_err(|_| BusinessesError::Database)?;
    };
    let mut task = NewTask::new(REAPPLY_KIND, json!({}), due);
    task.dedupe_key = Some(format!("{REAPPLY_KIND}.{day}"));
    enqueue(db, &task)
        .await
        .map(|_| ())
        .map_err(|_| BusinessesError::Database)
}

/// The [`REAPPLY_KIND`] handler (BIZ-050, JOB-006): queues tomorrow's run first, then
/// [`apply_lists`] with the shipped map and catalog at `now`.
///
/// # Errors
///
/// A fixed [`HandlerError`] on any failure; the task retries and re-applies nothing twice.
pub async fn run_reapply(pool: &PgPool, now: Timestamp) -> Result<(), HandlerError> {
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    schedule_reapply(&mut conn, now).await.map_err(|_| FAILED)?;
    let maps = shipped().map_err(|_| FAILED)?;
    apply_lists(&mut conn, maps, now)
        .await
        .map_err(|_| FAILED)?;
    Ok(())
}

/// BIZ-050, LIC-043, D-2705, D-3356: applies every recorded ROC list not yet marked
/// [`LIST_APPLIED`], oldest first; returns how many lists it marked. The first list ever recorded,
/// and a list recorded late with a date older than one already applied, are only marked: the later
/// list already compared each number with the classes it had then, so applying the older one now
/// would re-apply stale classes.
///
/// # Errors
///
/// `Database` or `Audit`, or any error of `reapply_license`; lists applied before it stay marked.
pub async fn apply_lists(
    conn: &mut PgConnection,
    maps: (&ClassMap, &Catalog),
    now: Timestamp,
) -> Result<usize, BusinessesError> {
    let lists = roc_lists(conn)
        .await
        .map_err(|_| BusinessesError::Database)?;
    let mut done = Vec::with_capacity(lists.len());
    for list in &lists {
        let applied = audited(conn, LIST_APPLIED, (LIST_SUBJECT, list.id)).await;
        done.push(applied.map_err(|_| BusinessesError::Database)?);
    }
    let newest_done = done.iter().rposition(|d| *d);
    let mut marked = 0;
    for (i, list) in lists.into_iter().enumerate() {
        if done[i] {
            continue;
        }
        if i > 0 && newest_done.is_none_or(|n| n < i) {
            apply_list(conn, maps, list, now).await?;
        }
        mark_applied(conn, list.id).await?;
        marked += 1;
    }
    Ok(marked)
}

/// Re-applies the map to every business bound to a number whose classes on `new` changed from
/// the ones it last had (D-3355), in batches of [`BATCH`] numbers (BIZ-050).
async fn apply_list(
    conn: &mut PgConnection,
    maps: (&ClassMap, &Catalog),
    new: RocList,
    now: Timestamp,
) -> Result<(), BusinessesError> {
    let mut after = String::new();
    loop {
        let read = class_changes(conn, new.id, (&after, BATCH)).await;
        let changes = read.map_err(|_| BusinessesError::Database)?;
        let Some(last) = changes.last() else {
            return Ok(());
        };
        after.clone_from(&last.license_number);
        let numbers: Vec<String> = changes.iter().map(|c| c.license_number.clone()).collect();
        let bound = bound_businesses(conn, &numbers).await;
        for (business, number) in bound.map_err(|_| BusinessesError::Database)? {
            let found = changes.iter().find(|c| c.license_number == number);
            let Some(change) = found else { continue };
            let old_classes: Vec<&str> = change.old.iter().map(String::as_str).collect();
            let new_classes: Vec<&str> = change.new.iter().map(String::as_str).collect();
            let classes = (old_classes.as_slice(), new_classes.as_slice());
            reapply_license(conn, business, maps, classes, (new.list_date, now)).await?;
        }
    }
}

/// Writes the [`LIST_APPLIED`] row on list `id`, once.
async fn mark_applied(conn: &mut PgConnection, id: Uuid) -> Result<(), BusinessesError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action: LIST_APPLIED,
        subject_kind: LIST_SUBJECT,
        subject_id: id,
        change: Change::Action,
    };
    let once = audit_once(conn, &entry).await;
    once.map(|_| ()).map_err(|_| BusinessesError::Audit)
}

/// BIZ-050, BIZ-009, BIZ-012, LIC-047, CARD-015 (D-2596, D-2711, D-2730): in the caller's
/// transaction, with the business row already locked, lifts a `no_covered_job_type` pause of
/// business `id` when the stored ticks hold a job type `offer` (rebuilt from `parts`) marks
/// licensed. Before lifting, the other rules run in turn and the first that holds keeps it paused
/// with its reason (audited): three complaints within the window, a pest license reported wrong,
/// the latest daily check not Active. A pause that replaced the owner's own pause returns to it
/// (audited); otherwise the business becomes active (status audit row). Any other status or
/// reason, or no covered tick, is left as it is. Returns the status it moved to, if any.
///
/// # Errors
///
/// `Database`, `Audit`, `Refused` or `MovedOn`.
pub(crate) async fn lift_no_cover(
    tx: &mut PgConnection,
    id: Uuid,
    (offer, parts): (&[TradeOffer], &HolderParts),
    now: Timestamp,
) -> Result<Option<AccountStatus>, BusinessesError> {
    if !paused_bare(tx, id).await? || !covers_a_tick(tx, id, offer).await? {
        return Ok(None);
    }
    let held = if complaints_hold(tx, id, now).await? {
        Some(PauseReason::Complaints)
    } else if pest_reported(parts) || !check_active(tx, id).await? {
        Some(PauseReason::License)
    } else if replaced_self(tx, id).await? {
        Some(PauseReason::SelfPaused)
    } else {
        None
    };
    match held {
        Some(reason) => {
            set_reason(tx, id, reason).await?;
            Ok(Some(models::read(tx, id).await?.status))
        }
        None => apply(tx, id, Event::Resume, Actor::System, now)
            .await
            .map(Some),
    }
}

/// Whether the business is paused for `no_covered_job_type` now.
async fn paused_bare(tx: &mut PgConnection, id: Uuid) -> Result<bool, BusinessesError> {
    let bare = sqlx::query_scalar!(
        r#"SELECT COALESCE(status = 'paused' AND pause_reason = 'no_covered_job_type', false)
           AS "bare!" FROM businesses.business WHERE id = $1"#,
        id
    );
    let row = db(bare.fetch_optional(&mut *tx).await)?;
    row.ok_or(BusinessesError::UnknownBusiness)
}

/// Whether a stored tick is a row `offer` marks licensed (the license covers it, BIZ-050).
async fn covers_a_tick(
    tx: &mut PgConnection,
    id: Uuid,
    offer: &[TradeOffer],
) -> Result<bool, BusinessesError> {
    let rows = offer.iter().flat_map(|t| &t.rows);
    let licensed: BTreeSet<_> = rows.filter(|r| r.licensed).map(|r| &r.choice).collect();
    let ticks = models::job_types(tx, id).await?;
    Ok(ticks.iter().any(|t| licensed.contains(t)))
}

/// BIZ-012, D-016: [`PAUSE_COUNT`] or more complaints within [`PAUSE_WINDOW_DAYS`] up to `now`,
/// the count `complaints::record_complaint` pauses on.
async fn complaints_hold(
    tx: &mut PgConnection,
    id: Uuid,
    now: Timestamp,
) -> Result<bool, BusinessesError> {
    use jiff_sqlx::ToSqlx;
    let count = sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM businesses.complaint WHERE business_id = $1 \
         AND received_at <= $2 AND received_at >= $2 - make_interval(days => $3)",
        id,
        now.to_sqlx() as _,
        PAUSE_WINDOW_DAYS
    );
    Ok(db(count.fetch_one(&mut *tx).await)? >= PAUSE_COUNT)
}

/// LIC-047: the business's pest license was reported wrong and no found number replaced it.
fn pest_reported(parts: &HolderParts) -> bool {
    parts.holder.pest.as_ref().is_some_and(|p| p.is_paused())
}

/// CARD-015: false only when the business's latest daily check was not Active; a business never
/// checked (no ROC license) passes.
async fn check_active(tx: &mut PgConnection, id: Uuid) -> Result<bool, BusinessesError> {
    let check = latest_check_active(tx, id).await;
    Ok(check.map_err(|_| BusinessesError::Database)? != Some(false))
}

/// D-969, D-970: the pause began as the owner's own: the business's latest status or pause-reason
/// audit row is the `pause_reason` field change `pause_for_rule` writes when a rule's reason
/// replaces `self` (a pause from active writes a status row instead).
async fn replaced_self(tx: &mut PgConnection, id: Uuid) -> Result<bool, BusinessesError> {
    let subject = ("businesses.business", id);
    let last = field_changed_last(tx, subject, "pause_reason").await;
    last.map_err(|_| BusinessesError::Database)
}

/// Replaces the `no_covered_job_type` reason with `reason`, the business staying paused, with one
/// `pause_reason` field audit row by the system (BIZ-009, PRIV-028).
async fn set_reason(
    tx: &mut PgConnection,
    id: Uuid,
    reason: PauseReason,
) -> Result<(), BusinessesError> {
    let done = db(sqlx::query!(
        "UPDATE businesses.business SET pause_reason = $2 \
         WHERE id = $1 AND status = 'paused' AND pause_reason = 'no_covered_job_type'",
        id,
        reason.code()
    )
    .execute(&mut *tx)
    .await)?;
    if done.rows_affected() != 1 {
        return Err(BusinessesError::MovedOn);
    }
    let entry = AuditEntry {
        actor: Actor::System,
        action: "businesses.field_changed",
        subject_kind: "businesses.business",
        subject_id: id,
        change: Change::Field("pause_reason"),
    };
    audit(tx, &entry)
        .await
        .map(|_| ())
        .map_err(|_| BusinessesError::Audit)
}
