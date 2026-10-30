//! Migrations, the two roles and the guarded status change (JOB-004, D-092, D-491, D-492).
//!
//! Proves on fresh databases that the migrations apply in number order and a second run is a
//! no-op; that a migration naming a role that does not exist stops with an error naming it, so no
//! grant is ever skipped; that on an append-only table the app role may `INSERT` and `SELECT` but
//! its `UPDATE`, `DELETE` and `TRUNCATE` fail with SQLSTATE 42501; and that a status change whose
//! expected status is stale, alone or behind another writer's row lock, changes nothing and returns
//! none (JOB-004). The probe tables are created by the test, as the migration identity, so the
//! migration's default privileges are what grants the app role its rights.

use std::time::Duration;

use askhava_core::api::{DbError, Guarded, MIGRATOR, Settings, SettingsError, migrate};
use askhava_integration::{APP_ROLE, is_permission_denied, set_app_role};
use secrecy::SecretString;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

/// A role name no cluster has, put in place of the app role to prove a missing role is reported.
const ABSENT_ROLE: &str = "askhava_absent_t012";

/// The versions recorded in `_sqlx_migrations`, in the order they were installed.
async fn applied_versions(pool: &PgPool) -> Vec<i64> {
    sqlx::query_scalar(
        "SELECT version FROM _sqlx_migrations WHERE success ORDER BY installed_on, version",
    )
    .fetch_all(pool)
    .await
    .expect("read the migration history")
}

#[sqlx::test(migrations = false)]
async fn migrations_apply_in_order_on_an_empty_database(pool: PgPool) {
    let expected: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
    assert!(!expected.is_empty(), "migrations/ embeds no migration");
    assert!(expected.is_sorted(), "embedded migrations are out of order");
    MIGRATOR.run(&pool).await.expect("migrations apply");
    assert_eq!(applied_versions(&pool).await, expected);
    MIGRATOR.run(&pool).await.expect("a second run is a no-op");
    assert_eq!(applied_versions(&pool).await, expected);
}

/// `DATABASE_URL` pointed at this test's own database, as `server migrate` would receive it.
async fn own_database_url(pool: &PgPool) -> SecretString {
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(pool)
        .await
        .expect("current database");
    let base = std::env::var("DATABASE_URL").expect("DATABASE_URL is set for database tests");
    let (server, rest) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database");
    let query = rest.split_once('?').map_or("", |(_, q)| q);
    let sep = if query.is_empty() { "" } else { "?" };
    SecretString::from(format!("{server}/{name}{sep}{query}"))
}

#[sqlx::test(migrations = false)]
async fn server_migrate_applies_then_is_a_no_op(pool: PgPool) {
    let url = own_database_url(&pool).await;
    let expected: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
    migrate(&url).await.expect("server migrate applies");
    migrate(&url)
        .await
        .expect("server migrate again is a no-op");
    assert_eq!(applied_versions(&pool).await, expected);
    let bad = migrate(&SecretString::from("not a url".to_string())).await;
    assert!(matches!(bad, Err(DbError::BadUrl)), "{bad:?}");
}

#[test]
fn server_migrate_needs_its_own_url() {
    let base = [
        ("ASKHAVA_APP_ENV", "dev"),
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://askhava_app@localhost/fake",
        ),
    ];
    let pairs = |extra: &[(&str, &str)]| {
        base.iter()
            .chain(extra)
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect::<Vec<_>>()
    };
    let without = Settings::from_pairs(pairs(&[])).expect("settings parse");
    let missing = without.migration_database_url().err();
    assert_eq!(
        missing,
        Some(SettingsError::Missing("ASKHAVA_MIGRATION_DATABASE_URL"))
    );
    let with = Settings::from_pairs(pairs(&[(
        "ASKHAVA_MIGRATION_DATABASE_URL",
        "postgres://askhava_migrator@localhost/fake",
    )]));
    assert!(
        with.expect("settings parse")
            .migration_database_url()
            .is_ok()
    );
}

#[sqlx::test(migrations = false)]
async fn every_grant_reports_a_missing_role(pool: PgPool) {
    let mut checked = 0;
    for migration in MIGRATOR.iter() {
        let sql = migration.sql.as_str();
        if sql.contains(APP_ROLE) {
            assert!(
                !migration.no_tx,
                "migration {} grants in a `-- no-transaction` file; grants belong in a transactional migration",
                migration.version
            );
            assert_missing_role_reported(&pool, migration.version, sql).await;
            checked += 1;
        }
        MIGRATOR
            .run_to(migration.version, &pool)
            .await
            .expect("the real migration applies");
    }
    assert!(checked > 0, "no migration grants to the app role");
}

/// Runs `sql` with the app role renamed to [`ABSENT_ROLE`] in a rolled-back transaction and
/// expects it to fail with an error naming that role.
async fn assert_missing_role_reported(pool: &PgPool, version: i64, sql: &str) {
    let mut tx = pool.begin().await.expect("begin");
    let outcome = sqlx::raw_sql(AssertSqlSafe(sql.replace(APP_ROLE, ABSENT_ROLE)))
        .execute(&mut *tx)
        .await;
    let message = outcome.map(|_| ()).err().map(|e| e.to_string());
    assert!(
        message.as_deref().is_some_and(|m| m.contains(ABSENT_ROLE)),
        "migration {version} did not report the missing role: {message:?}"
    );
    tx.rollback().await.expect("rollback");
}

/// Creates an append-only probe table in `core` the way a module's migration must (postgres brain P5).
async fn create_append_only_probe(conn: &mut PgConnection) {
    sqlx::raw_sql(
        "CREATE TABLE core.append_only_probe (id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, note text NOT NULL);
         REVOKE UPDATE, DELETE, TRUNCATE ON core.append_only_probe FROM askhava_app;",
    )
    .execute(conn)
    .await
    .expect("create the probe table as the migration identity");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn app_role_cannot_rewrite_an_append_only_table(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    create_append_only_probe(&mut conn).await;
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    sqlx::query("INSERT INTO core.append_only_probe (note) VALUES ('fake note')")
        .execute(&mut *conn)
        .await
        .expect("the app role may insert");
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM core.append_only_probe")
        .fetch_one(&mut *conn)
        .await
        .expect("the app role may select");
    assert_eq!(rows, 1);
    let update = sqlx::query("UPDATE core.append_only_probe SET note = 'changed'")
        .execute(&mut *conn)
        .await;
    assert!(is_permission_denied(&update), "UPDATE: {update:?}");
    let delete = sqlx::query("DELETE FROM core.append_only_probe")
        .execute(&mut *conn)
        .await;
    assert!(is_permission_denied(&delete), "DELETE: {delete:?}");
    let truncate = sqlx::query("TRUNCATE core.append_only_probe")
        .execute(&mut *conn)
        .await;
    assert!(is_permission_denied(&truncate), "TRUNCATE: {truncate:?}");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn app_role_owns_nothing_and_creates_nothing(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    let create = sqlx::query("CREATE TABLE core.app_made (id bigint)")
        .execute(&mut *conn)
        .await;
    assert!(is_permission_denied(&create), "CREATE TABLE: {create:?}");
    let owned: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_class c JOIN pg_roles r ON r.oid = c.relowner WHERE r.rolname = 'askhava_app'",
    )
    .fetch_one(&mut *conn)
    .await
    .expect("count owned relations");
    assert_eq!(owned, 0, "the app role owns a relation");
}

/// The guarded status change every module writes for its own table (JOB-004).
async fn advance(conn: &mut PgConnection, from: &str, to: &str) -> Guarded<String> {
    let row = sqlx::query_scalar(
        "UPDATE core.status_probe SET status = $3 WHERE id = $1 AND status = $2 RETURNING status",
    )
    .bind(1_i64)
    .bind(from)
    .bind(to)
    .fetch_optional(conn)
    .await
    .expect("guarded update runs");
    Guarded::from_returning(row)
}

async fn status(pool: &PgPool) -> String {
    sqlx::query_scalar("SELECT status FROM core.status_probe WHERE id = 1")
        .fetch_one(pool)
        .await
        .expect("read status")
}

async fn create_status_probe(pool: &PgPool) {
    sqlx::raw_sql(
        "CREATE TABLE core.status_probe (id bigint PRIMARY KEY, status text NOT NULL);
         INSERT INTO core.status_probe VALUES (1, 'open');",
    )
    .execute(pool)
    .await
    .expect("create the status probe");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn stale_expected_status_changes_nothing(pool: PgPool) {
    create_status_probe(&pool).await;
    let mut conn = pool.acquire().await.expect("connection");
    let stale = advance(&mut conn, "quoted", "closed").await;
    assert_eq!(stale, Guarded::MovedOn);
    assert_eq!(stale.applied(), None);
    assert_eq!(status(&pool).await, "open");
    let fresh = advance(&mut conn, "open", "quoted").await;
    assert_eq!(fresh.applied().as_deref(), Some("quoted"));
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn second_writer_waits_for_the_row_lock_then_moves_on(pool: PgPool) {
    create_status_probe(&pool).await;
    let mut first = pool.begin().await.expect("begin first");
    let won = advance(&mut first, "open", "quoted").await;
    assert_eq!(won, Guarded::Applied("quoted".to_string()));
    let second_pool = pool.clone();
    let second = tokio::spawn(async move {
        let mut tx = second_pool.begin().await.expect("begin second");
        let outcome = advance(&mut tx, "open", "closed").await;
        tx.commit().await.expect("commit second");
        outcome
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !second.is_finished(),
        "the second writer did not wait for the row lock"
    );
    first.commit().await.expect("commit first");
    let lost = second.await.expect("second writer finishes");
    assert_eq!(lost, Guarded::MovedOn);
    assert_eq!(status(&pool).await, "quoted");
}
