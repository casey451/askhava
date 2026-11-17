//! The weekly digest's `ops` readers and writers (T528; TD-OPS-09's digest-row half; OPS-030,
//! D-219, D-3882, D-3883, D-4044, D-4202). Fake IDs, kinds and trade codes only.
//!
//! Alerts of given kinds raised or resolved in a `[from, to)` window with the self-acknowledged
//! flag, the last active-business count per trade, the system actor's status-change audit rows of
//! one subject kind, and the digest row with its sent, opened and reminder writers; every result
//! holds IDs, code names, counts and times only, never a contact detail.

use askhava_ops::api::{
    CoverageCount, CoverageScope, DigestAlert, StatusChangeRow, digest_alerts, digest_opened,
    digest_reminded, digest_sent, latest_digest, record_business_counts, system_status_changes,
    trade_active_counts,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const FAILING: &str = "licensing.list_failing";
const STILL: &str = "licensing.list_still_failing";
const SUBJECT: &str = "businesses.business";
const FROM: &str = "2026-11-09T07:00:00Z";
const TO: &str = "2026-11-16T07:00:00Z";

fn at(text: &str) -> Timestamp {
    text.parse().expect("time")
}

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0000_7e57_0528_0000_0000_0000_0000_0000 + n)
}

/// Plants alert `n` of `kind` raised at `raised`, resolved at `resolved` when given.
async fn alert(db: &mut Db, n: u128, kind: &str, raised: &str, resolved: Option<&str>) {
    sqlx::query(
        "INSERT INTO ops.alert (id, kind, severity, subject_kind, subject_id, raised_at, \
         resolved_at) VALUES ($1, $2, 'normal', 'licensing.list', $3, $4, $5)",
    )
    .bind(id(n))
    .bind(kind)
    .bind(id(100 + n))
    .bind(at(raised).to_sqlx())
    .bind(resolved.map(|r| at(r).to_sqlx()))
    .execute(&mut **db)
    .await
    .expect("alert");
}

async fn ack(db: &mut Db, alert: Uuid, by: &str, when: &str) {
    sqlx::query("INSERT INTO ops.alert_ack (alert_id, acknowledged_by, at) VALUES ($1, $2, $3)")
        .bind(alert)
        .bind(by)
        .bind(at(when).to_sqlx())
        .execute(&mut **db)
        .await
        .expect("ack");
}

/// One planted audit row: subject number, by a business (else the system), subject kind, the
/// status move as `from>to` or a field name (empty for an action-only row), and its time.
type Planted = (u128, bool, &'static str, &'static str, &'static str);

const AUDIT_ROWS: [Planted; 8] = [
    (1, false, SUBJECT, "pending>active", FROM),
    (2, false, SUBJECT, "active>paused", "2026-11-12T16:00:00Z"),
    (3, false, SUBJECT, "pause_reason", "2026-11-13T16:00:00Z"),
    (4, false, SUBJECT, "active>paused", TO),
    (5, false, SUBJECT, "pending>active", "2026-11-09T06:59:59Z"),
    (6, true, SUBJECT, "active>paused", "2026-11-12T16:00:00Z"),
    (7, false, "jobs.job", "open>closed", "2026-11-12T16:00:00Z"),
    (8, false, SUBJECT, "", "2026-11-12T16:00:00Z"),
];

async fn audit(db: &mut Db, (n, by_business, kind, change, when): Planted) {
    let (actor, actor_id) = if by_business {
        ("business", Some(id(n)))
    } else {
        ("system", None)
    };
    let (from, to, field) = match change.split_once('>') {
        Some((from, to)) => (Some(from), Some(to), None),
        None => (None, None, Some(change).filter(|f| !f.is_empty())),
    };
    sqlx::query(
        "INSERT INTO ops.audit_log (at, actor_kind, actor_id, action, subject_kind, subject_id, \
         from_status, to_status, field) VALUES ($1, $2, $3, 'businesses.status_changed', $4, $5, \
         $6, $7, $8)",
    )
    .bind(at(when).to_sqlx())
    .bind(actor)
    .bind(actor_id)
    .bind(kind)
    .bind(id(n))
    .bind(from)
    .bind(to)
    .bind(field)
    .execute(&mut **db)
    .await
    .expect("audit");
}

/// Every reader's results as text: no `@`, no run of 7 or more digits outside an ID or time.
fn assert_no_contact(text: &str) {
    assert!(!text.contains('@'), "no email address: {text}");
    let stripped: String = text
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != ':')
        .filter(|w| Uuid::parse_str(w).is_err() && w.parse::<Timestamp>().is_err())
        .collect::<Vec<_>>()
        .join(" ");
    let longest = stripped
        .split(|c: char| !c.is_ascii_digit())
        .map(str::len)
        .max()
        .unwrap_or(0);
    assert!(longest < 7, "no phone-like digits: {stripped}");
}

/// Planted alerts: number, kind, raised, resolved. 1 is raised at `from`, 2 resolved in the
/// window, 3 inside it; 4 is raised at `to`, 5 just before `from`, 6 resolved at `to`, 7 another kind.
const ALERTS: [(u128, &str, &str, Option<&str>); 7] = [
    (1, FAILING, FROM, None),
    (
        2,
        STILL,
        "2026-11-01T00:00:00Z",
        Some("2026-11-15T23:59:59Z"),
    ),
    (
        3,
        FAILING,
        "2026-11-12T16:00:00Z",
        Some("2026-11-12T17:00:00Z"),
    ),
    (4, FAILING, TO, None),
    (5, FAILING, "2026-11-09T06:59:59Z", None),
    (6, STILL, "2026-11-02T00:00:00Z", Some(TO)),
    (7, "ops.volume_high", "2026-11-12T16:00:00Z", None),
];

/// TD-OPS-09, OPS-030, D-4044: alerts of the asked kinds raised or resolved in `[from, to)`, with
/// a system acknowledgement as self-acknowledged and Casey's not; other kinds and edges left out.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_alerts_by_kind_in_the_window(mut db: Db) {
    for (n, kind, raised, resolved) in ALERTS {
        alert(&mut db, n, kind, raised, resolved).await;
    }
    let (at_from, cleared, by_casey) = (id(1), id(2), id(3));
    ack(&mut db, cleared, "system", "2026-11-15T23:59:59Z").await;
    ack(&mut db, by_casey, "casey", "2026-11-12T17:00:00Z").await;

    let rows = digest_alerts(&mut db, &[FAILING, STILL], at(FROM), at(TO))
        .await
        .expect("read");
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![cleared, at_from, by_casey]);
    assert_eq!(
        rows[0],
        DigestAlert {
            id: cleared,
            kind: STILL.to_owned(),
            subject: Some(("licensing.list".to_owned(), id(102))),
            raised_at: at("2026-11-01T00:00:00Z"),
            resolved_at: Some(at("2026-11-15T23:59:59Z")),
            self_acknowledged: true,
        }
    );
    assert!(!rows[1].self_acknowledged && rows[1].resolved_at.is_none());
    assert!(
        !rows[2].self_acknowledged,
        "Casey's acknowledgement is not the system's"
    );
    let none = digest_alerts(&mut db, &[], at(FROM), at(TO))
        .await
        .expect("read");
    assert!(none.is_empty());
    assert_no_contact(&format!("{rows:?}"));
}

/// TD-OPS-09, OPS-030: the last reported active count of every trade, by code; towns left out and
/// a later report replaces the earlier one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_active_businesses_per_trade(mut db: Db) {
    assert!(trade_active_counts(&mut db).await.expect("read").is_empty());
    let count = |scope, code: &str, active| CoverageCount {
        scope,
        code: code.to_owned(),
        active,
    };
    let first = [
        count(CoverageScope::Trade, "plumbing", 1),
        count(CoverageScope::Trade, "electrical", 4),
        count(CoverageScope::Town, "lake_havasu_city", 9),
    ];
    record_business_counts(&mut db, &first, at("2026-11-10T16:00:00Z"))
        .await
        .expect("record");
    let later = [count(CoverageScope::Trade, "plumbing", 3)];
    record_business_counts(&mut db, &later, at("2026-11-11T16:00:00Z"))
        .await
        .expect("record");

    let rows = trade_active_counts(&mut db).await.expect("read");
    let got: Vec<(&str, u32, Timestamp)> = rows
        .iter()
        .map(|r| (r.trade.as_str(), r.active, r.counted_at))
        .collect();
    assert_eq!(
        got,
        vec![
            ("electrical", 4, at("2026-11-10T16:00:00Z")),
            ("plumbing", 3, at("2026-11-11T16:00:00Z")),
        ]
    );
    assert_no_contact(&format!("{rows:?}"));
}

/// TD-OPS-09, OPS-030, D-3883: the system's status and field rows about one subject kind in
/// `[from, to)`, oldest first; a person's row, another kind, an action-only row and the edges'
/// outside are left out.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_system_status_changes_in_the_window(mut db: Db) {
    for row in AUDIT_ROWS {
        audit(&mut db, row).await;
    }
    let rows = system_status_changes(&mut db, SUBJECT, at(FROM), at(TO))
        .await
        .expect("read");
    let row =
        |n, from: Option<&str>, to: Option<&str>, field: Option<&str>, when| StatusChangeRow {
            subject_id: id(n),
            from_status: from.map(str::to_owned),
            to_status: to.map(str::to_owned),
            field: field.map(str::to_owned),
            at: at(when),
        };
    assert_eq!(
        rows,
        vec![
            row(1, Some("pending"), Some("active"), None, FROM),
            row(
                2,
                Some("active"),
                Some("paused"),
                None,
                "2026-11-12T16:00:00Z"
            ),
            row(3, None, None, Some("pause_reason"), "2026-11-13T16:00:00Z"),
        ]
    );
    assert_no_contact(&format!("{rows:?}"));
}

/// TD-OPS-09, OPS-030: the digest row's writers keep the first open, give an unopened digest one
/// reminder and an opened one none, and the reader returns the newest digest.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_digest_row_sent_opened_and_reminded(mut db: Db) {
    assert_eq!(latest_digest(&mut db).await.expect("read"), None);
    let first = digest_sent(&mut db, at("2026-11-02T15:00:00Z"))
        .await
        .expect("sent");
    assert!(
        digest_opened(&mut db, first, at("2026-11-03T15:00:00Z"))
            .await
            .expect("open")
    );
    assert!(
        !digest_opened(&mut db, first, at("2026-11-04T15:00:00Z"))
            .await
            .expect("open")
    );
    assert!(
        !digest_reminded(&mut db, first, at("2026-11-06T15:00:00Z"))
            .await
            .expect("remind")
    );

    let second = digest_sent(&mut db, at("2026-11-09T15:00:00Z"))
        .await
        .expect("sent");
    assert!(
        digest_reminded(&mut db, second, at("2026-11-13T15:00:00Z"))
            .await
            .expect("remind")
    );
    assert!(
        !digest_reminded(&mut db, second, at("2026-11-14T15:00:00Z"))
            .await
            .expect("remind")
    );
    let row = latest_digest(&mut db).await.expect("read").expect("row");
    assert_eq!(row.id, second);
    assert_eq!(row.sent_at, at("2026-11-09T15:00:00Z"));
    assert_eq!(row.opened_at, None);
    assert_eq!(row.reminded_at, Some(at("2026-11-13T15:00:00Z")));

    assert!(
        digest_opened(&mut db, second, at("2026-11-15T15:00:00Z"))
            .await
            .expect("open")
    );
    let row = latest_digest(&mut db).await.expect("read").expect("row");
    assert_eq!(row.opened_at, Some(at("2026-11-15T15:00:00Z")));
    assert!(
        !digest_opened(&mut db, id(99), at(TO))
            .await
            .expect("unknown")
    );
    assert_no_contact(&format!("{row:?}"));
}
