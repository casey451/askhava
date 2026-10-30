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

use askhava_core::api::{DbError, Guarded, MIGRATOR, Settings, SettingsError, app_pool, migrate};
use askhava_integration::{
    APP_ROLE, Statement, is_permission_denied, set_app_role, split_statements,
};
use secrecy::{ExposeSecret, SecretString};
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
    SecretString::from(with_database(&base, &name))
}

/// `base` with its database name replaced by `name`; the query string is set aside first, since
/// it may hold a `/` (for example `?host=/run/postgresql`).
fn with_database(base: &str, name: &str) -> String {
    let (path, query) = base.split_once('?').unwrap_or((base, ""));
    let (server, _) = path
        .rsplit_once('/')
        .expect("DATABASE_URL names a database");
    let sep = if query.is_empty() { "" } else { "?" };
    format!("{server}/{name}{sep}{query}")
}

#[test]
fn with_database_keeps_a_query_with_a_slash() {
    assert_eq!(
        with_database("postgres://u@h/db?host=/run/pg", "t"),
        "postgres://u@h/t?host=/run/pg"
    );
    assert_eq!(with_database("postgres://u@h/db", "t"), "postgres://u@h/t");
}

#[sqlx::test(migrations = false)]
async fn app_pool_is_lazy_and_capped(pool: PgPool) {
    let url = own_database_url(&pool).await;
    let settings = |db_url: &str| {
        Settings::from_pairs(
            [
                ("ASKHAVA_APP_ENV", "dev"),
                ("ASKHAVA_DATABASE_URL", db_url),
                ("ASKHAVA_DB_POOL_MAX", "3"),
            ]
            .map(|(k, v)| (k.to_string(), v.to_string())),
        )
        .expect("settings parse")
    };
    let unreachable = app_pool(&settings("postgres://askhava_app@127.0.0.1:1/fake"));
    let unreachable = unreachable.expect("a lazy pool does not connect at startup");
    assert_eq!(unreachable.size(), 0);
    let live = app_pool(&settings(url.expose_secret())).expect("app pool");
    assert_eq!(live.options().get_max_connections(), 3);
    assert_eq!(live.size(), 0, "the pool connected before first use");
    let one: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&live)
        .await
        .expect("first use connects");
    assert_eq!(one, 1);
    let bad = app_pool(&settings("not a url"));
    assert!(matches!(bad, Err(DbError::BadUrl)), "{bad:?}");
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

/// Runs `change` on the migration history, then `server migrate`, then undoes it with `undo`.
async fn migrate_after(pool: &PgPool, url: &SecretString, change: &str, undo: &str) -> DbError {
    sqlx::raw_sql(AssertSqlSafe(change.to_string()))
        .execute(pool)
        .await
        .expect("change the history");
    let outcome = migrate(url).await.expect_err("server migrate refuses");
    sqlx::raw_sql(AssertSqlSafe(undo.to_string()))
        .execute(pool)
        .await
        .expect("restore the history");
    outcome
}

#[sqlx::test(migrations = false)]
async fn server_migrate_names_the_migration_it_refuses(pool: PgPool) {
    let url = own_database_url(&pool).await;
    migrate(&url).await.expect("server migrate applies");
    let first = MIGRATOR.iter().next().expect("a migration").version;
    let set = |sql: &str| sql.replace("{v}", &first.to_string());
    let edited = migrate_after(
        &pool,
        &url,
        &set("UPDATE _sqlx_migrations SET checksum = '\\x00' WHERE version = {v}"),
        "SELECT 1",
    )
    .await;
    assert!(
        matches!(edited, DbError::Edited(v) if v == first),
        "{edited:?}"
    );
    sqlx::query("DELETE FROM _sqlx_migrations")
        .execute(&pool)
        .await
        .expect("clear the history");
    let removed = migrate_after(
        &pool,
        &url,
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (999999, 'gone', true, '\\x00', 0)",
        "DELETE FROM _sqlx_migrations WHERE version = 999999",
    )
    .await;
    assert!(matches!(removed, DbError::Removed(999_999)), "{removed:?}");
    let dirty = migrate_after(
        &pool,
        &url,
        &set("INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
              VALUES ({v}, 'half', false, '\\x00', 0)"),
        "SELECT 1",
    )
    .await;
    assert!(
        matches!(dirty, DbError::Dirty(v) if v == first),
        "{dirty:?}"
    );
    assert_eq!(
        dirty.to_string(),
        format!("migration {first} is partly applied")
    );
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
            checked += assert_each_grant_fails_alone(&pool, migration.version, sql).await;
        }
        MIGRATOR
            .run_to(migration.version, &pool)
            .await
            .expect("the real migration applies");
    }
    assert!(checked > 0, "no migration grants to the app role");
}

/// True when the text holds a `GRANT` keyword, in any case.
fn has_grant(text: &str) -> bool {
    text.to_ascii_uppercase().contains("GRANT")
}

/// Fails when a grant hides inside dollar quotes, where an `IF EXISTS` or a `pg_roles` lookup
/// could skip it, or when a grant statement looks a role up (D-491).
fn assert_grants_are_plain(version: i64, statements: &[Statement]) {
    for statement in statements {
        assert!(
            !has_grant(&statement.quoted),
            "migration {version} grants inside a DO block or function body; a grant must be a plain statement: {}",
            statement.text.trim()
        );
        let upper = statement.text.to_ascii_uppercase();
        assert!(
            !(has_grant(&upper) && (upper.contains("IF EXISTS") || upper.contains("PG_ROLES"))),
            "migration {version} guards a grant with a role lookup: {}",
            statement.text.trim()
        );
    }
}

/// Replays `sql` one statement at a time in a rolled-back transaction; each statement naming the
/// app role is first run alone with the role renamed to [`ABSENT_ROLE`] and must fail naming it.
/// Returns how many grant statements were proved.
async fn assert_each_grant_fails_alone(pool: &PgPool, version: i64, sql: &str) -> usize {
    let statements = split_statements(sql);
    assert_grants_are_plain(version, &statements);
    let mut tx = pool.begin().await.expect("begin");
    let mut grants = 0;
    for statement in &statements {
        if statement.text.contains(APP_ROLE) {
            sqlx::raw_sql("SAVEPOINT grant_probe")
                .execute(&mut *tx)
                .await
                .expect("savepoint");
            let renamed = statement.text.replace(APP_ROLE, ABSENT_ROLE);
            let outcome = sqlx::raw_sql(AssertSqlSafe(renamed))
                .execute(&mut *tx)
                .await;
            let message = outcome.map(|_| ()).err().map(|e| e.to_string());
            assert!(
                message.as_deref().is_some_and(|m| m.contains(ABSENT_ROLE)),
                "migration {version} statement did not report the missing role: {}: {message:?}",
                statement.text.trim()
            );
            sqlx::raw_sql("ROLLBACK TO SAVEPOINT grant_probe")
                .execute(&mut *tx)
                .await
                .expect("rollback to savepoint");
            grants += usize::from(has_grant(&statement.text));
        }
        sqlx::raw_sql(AssertSqlSafe(statement.text.clone()))
            .execute(&mut *tx)
            .await
            .expect("the real statement applies");
    }
    tx.rollback().await.expect("rollback");
    grants
}

#[test]
fn a_grant_behind_a_role_lookup_is_caught() {
    let guarded = "-- core (T999)\nCREATE SCHEMA x;\nDO $g$ BEGIN IF EXISTS (SELECT FROM pg_roles \
                   WHERE rolname = 'askhava_app') THEN GRANT USAGE ON SCHEMA x TO askhava_app; \
                   END IF; END $g$;\nGRANT USAGE ON SCHEMA x TO askhava_app;";
    let statements = split_statements(guarded);
    let texts: Vec<&str> = statements.iter().map(|s| s.text.trim()).collect();
    assert_eq!(texts.len(), 3, "{texts:?}");
    assert_eq!(texts[0], "CREATE SCHEMA x");
    assert!(texts[1].starts_with("DO $g$") && texts[1].ends_with("END $g$"));
    assert!(has_grant(&statements[1].quoted));
    assert!(!has_grant(&statements[2].quoted) && has_grant(&statements[2].text));
    let caught = std::panic::catch_unwind(|| assert_grants_are_plain(9999, &statements));
    assert!(
        caught.is_err(),
        "a grant inside a DO block passed the check"
    );
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

/// Polls `pg_stat_activity` for up to 10 seconds until a session in this test's database waits on a
/// heavyweight lock (`wait_event_type = 'Lock'`), the row lock the first writer holds.
async fn waits_on_a_lock(pool: &PgPool) -> bool {
    for _ in 0..200 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .expect("read pg_stat_activity");
        if waiting > 0 {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
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
    assert!(
        waits_on_a_lock(&pool).await,
        "the second writer never waited on the row lock"
    );
    assert!(
        !second.is_finished(),
        "the second writer finished while the row lock was held"
    );
    first.commit().await.expect("commit first");
    let lost = second.await.expect("second writer finishes");
    assert_eq!(lost, Guarded::MovedOn);
    assert_eq!(status(&pool).await, "quoted");
}
