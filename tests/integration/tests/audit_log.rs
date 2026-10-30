//! The ops audit log, alerts and pause switches on a fresh database (TD-OPS-03, TD-PRIV-13; JOB-005,
//! OPS-003, OPS-010, OPS-011, PRIV-028).
//!
//! Proves, as the app role, that `ops.audit_log` and `ops.switch_history` take `INSERT` and `SELECT`
//! but refuse `UPDATE`, `DELETE` and `TRUNCATE`; that a status change's audit row commits or rolls
//! back with the change's own transaction; that the export returns every row once, in order, by
//! cursor, as JSON lines; that an audit row can hold no contact value, in Rust and in the table; and
//! that a pause switch logs each real change with its actor, in its history and in the audit log,
//! atomically. Reaching the storage stand-in as each
//! event happens is TD-OPS-03's off-database case, T017's (D-729). All data is fake.

use askhava_integration::{is_permission_denied, set_app_role};
use askhava_ops::api::{
    Actor, AuditEntry, Change, NewAlert, OpsError, Severity, Switch, audit, export_audit,
    is_paused, raise_alert, set_switch,
};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const JOB: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0001);
const OWNER: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0002);
const HOMEOWNER: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0003);
const TOWN: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0004);

/// A pooled connection already running as the app role.
async fn app_conn(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

async fn audit_rows(conn: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.audit_log")
        .fetch_one(conn)
        .await
        .expect("count audit rows")
}

/// The app role's `UPDATE`, `DELETE` and `TRUNCATE` on `table` all fail with 42501.
async fn assert_append_only(conn: &mut PgConnection, table: &str, column: &str) {
    for sql in [
        format!("UPDATE {table} SET {column} = {column}"),
        format!("DELETE FROM {table}"),
        format!("TRUNCATE {table}"),
    ] {
        let outcome = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
            .execute(&mut *conn)
            .await;
        assert!(is_permission_denied(&outcome), "{sql}: {outcome:?}");
    }
}

/// TD-OPS-03: append-only as the app role (OPS-010, postgres brain P5).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn app_role_cannot_rewrite_the_audit_log_or_switch_history(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let entry = AuditEntry::status_change(Actor::System, ("jobs.job", JOB), "open", "picked");
    audit(&mut conn, &entry)
        .await
        .expect("the app role may insert");
    set_switch(&mut conn, Switch::Texting, true, Actor::Owner(OWNER))
        .await
        .expect("switch");
    assert_eq!(audit_rows(&mut conn).await, 2, "the app role may select");
    assert_append_only(&mut conn, "ops.audit_log", "action").await;
    assert_append_only(&mut conn, "ops.switch_history", "paused").await;
}

/// TD-OPS-03, JOB-005: the audit row commits and rolls back with the status change.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_status_change_audit_row_shares_the_transaction(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let entry = AuditEntry::status_change(
        Actor::Homeowner(HOMEOWNER),
        ("jobs.job", JOB),
        "open",
        "picked",
    );
    let mut tx = sqlx::Connection::begin(&mut *conn).await.expect("begin");
    audit(&mut tx, &entry).await.expect("audit");
    tx.rollback().await.expect("rollback");
    assert_eq!(
        audit_rows(&mut conn).await,
        0,
        "a rolled-back change leaves no row"
    );
    let mut tx = sqlx::Connection::begin(&mut *conn).await.expect("begin");
    let id = audit(&mut tx, &entry).await.expect("audit");
    tx.commit().await.expect("commit");
    let rows = export_audit(&mut conn, 0, 10).await.expect("export");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(
        (row.id, row.subject_id, row.actor_id),
        (id, JOB, Some(HOMEOWNER))
    );
    assert_eq!(
        (row.from_status.as_deref(), row.to_status.as_deref()),
        (Some("open"), Some("picked"))
    );
    assert_eq!(
        (row.actor_kind.as_str(), row.action.as_str()),
        ("homeowner", "status_changed")
    );
}

/// TD-OPS-03, OPS-010: the export pages through every row once, oldest first, as JSON lines.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_export_returns_every_row_once_in_order(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let edit = AuditEntry {
        actor: Actor::Owner(OWNER),
        action: "portal.edit",
        subject_kind: "intake.property",
        subject_id: JOB,
        change: Change::Field("address"),
    };
    for _ in 0..5 {
        audit(&mut conn, &edit).await.expect("audit");
    }
    let mut seen = Vec::new();
    let mut cursor = 0;
    loop {
        let page = export_audit(&mut conn, cursor, 2).await.expect("export");
        let Some(last) = page.last() else { break };
        cursor = last.id;
        seen.extend(page.iter().map(|r| r.id));
        let line = page[0].to_json().to_string();
        assert!(
            !line.contains('\n') && line.contains("\"field\":\"address\""),
            "{line}"
        );
    }
    let mut sorted = seen.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!((seen.len(), seen), (5, sorted));
}

/// TD-PRIV-13, PRIV-028: an audit row holds IDs and code names only; a contact value is refused
/// by the Rust check before any write and by the table's check if written by hand.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_audit_row_cannot_hold_a_contact_value(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let fake_contact = [
        "(928) 555-0100",
        "+19285550100",
        "pat@example.com",
        "1 Fake St",
    ];
    for value in fake_contact {
        let leaked: &'static str = Box::leak(value.to_owned().into_boxed_str());
        let entry = AuditEntry {
            change: Change::Field(leaked),
            ..AuditEntry::status_change(Actor::System, ("jobs.job", JOB), "open", "picked")
        };
        assert!(matches!(
            audit(&mut conn, &entry).await,
            Err(OpsError::NotACodeName("field"))
        ));
        let by_hand = sqlx::query(
            "INSERT INTO ops.audit_log (actor_kind, action, subject_kind, subject_id, field) \
             VALUES ('system', 'portal.edit', 'intake.property', $1, $2)",
        )
        .bind(JOB)
        .bind(value)
        .execute(&mut *conn)
        .await;
        assert!(by_hand.is_err(), "the table took a contact-shaped field");
    }
    assert_eq!(audit_rows(&mut conn).await, 0);
    let columns: Vec<(String, String)> = sqlx::query_as(
        "SELECT column_name::text, data_type::text FROM information_schema.columns \
         WHERE table_schema = 'ops' AND table_name = 'audit_log' ORDER BY ordinal_position",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("columns");
    let expected = [
        ("id", "bigint"),
        ("at", "timestamp with time zone"),
        ("actor_kind", "text"),
        ("actor_id", "uuid"),
        ("action", "text"),
        ("subject_kind", "text"),
        ("subject_id", "uuid"),
        ("from_status", "text"),
        ("to_status", "text"),
        ("field", "text"),
    ];
    let expected: Vec<(String, String)> = expected
        .iter()
        .map(|(n, t)| ((*n).to_owned(), (*t).to_owned()))
        .collect();
    assert_eq!(
        columns, expected,
        "ops.audit_log holds IDs and code names only"
    );
}

/// OPS-003: a pause switch logs each real change with its actor and ignores a repeat.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_pause_switch_logs_each_change_once(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let town = Switch::Town(TOWN);
    assert!(!is_paused(&mut conn, town).await.expect("read"));
    assert!(
        !set_switch(&mut conn, town, false, Actor::Owner(OWNER))
            .await
            .expect("off")
    );
    assert!(
        set_switch(&mut conn, town, true, Actor::Owner(OWNER))
            .await
            .expect("on")
    );
    assert!(
        !set_switch(&mut conn, town, true, Actor::Owner(OWNER))
            .await
            .expect("again")
    );
    assert!(is_paused(&mut conn, town).await.expect("read"));
    assert!(!is_paused(&mut conn, Switch::All).await.expect("read"));
    assert!(
        set_switch(&mut conn, town, false, Actor::System)
            .await
            .expect("off")
    );
    let history: Vec<(bool, String)> = sqlx::query_as(
        "SELECT paused, actor_kind FROM ops.switch_history WHERE scope_id = $1 ORDER BY id",
    )
    .bind(TOWN)
    .fetch_all(&mut *conn)
    .await
    .expect("history");
    assert_eq!(
        history,
        [(true, "owner".to_owned()), (false, "system".to_owned())]
    );
    let switch_id: Uuid = sqlx::query_scalar("SELECT id FROM ops.pause_switch WHERE scope_id = $1")
        .bind(TOWN)
        .fetch_one(&mut *conn)
        .await
        .expect("switch row");
    let audited: Vec<(String, String, Uuid, String)> = sqlx::query_as(
        "SELECT action, subject_kind, subject_id, actor_kind FROM ops.audit_log ORDER BY id",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("audit rows");
    let row = |action: &str, actor: &str| {
        (
            action.to_owned(),
            "ops.pause_switch".to_owned(),
            switch_id,
            actor.to_owned(),
        )
    };
    assert_eq!(
        audited,
        [row("switch_on", "owner"), row("switch_off", "system")],
        "SEC-006: every switch change is audit-logged"
    );
}

/// OPS-003: setting a switch to the state it is already in returns false and writes no second
/// history or audit row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_repeat_switch_change_writes_nothing(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let owner = Actor::Owner(OWNER);
    assert!(
        set_switch(&mut conn, Switch::Charging, true, owner)
            .await
            .expect("on")
    );
    assert!(
        !set_switch(&mut conn, Switch::Charging, true, owner)
            .await
            .expect("again")
    );
    let history: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ops.switch_history WHERE name = 'charging'")
            .fetch_one(&mut *conn)
            .await
            .expect("history");
    assert_eq!((history, audit_rows(&mut conn).await), (1, 1));
}

/// An alert holds the record it is about by ID only (PRIV-028).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_alert_holds_ids_only(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let alert = NewAlert {
        kind: "ops.health_check_failed",
        severity: Severity::High,
        subject: Some(("jobs.job", JOB)),
    };
    let id = raise_alert(&mut conn, &alert).await.expect("raise");
    let stored: (String, Option<Uuid>) =
        sqlx::query_as("SELECT severity, subject_id FROM ops.alert WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *conn)
            .await
            .expect("read");
    assert_eq!(stored, ("high".to_owned(), Some(JOB)));
    let bad = NewAlert {
        kind: "Call 928-555-0100",
        ..alert
    };
    assert!(matches!(
        raise_alert(&mut conn, &bad).await,
        Err(OpsError::NotACodeName("kind"))
    ));
}
