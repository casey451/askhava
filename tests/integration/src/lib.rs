//! Helpers the database tests share (D-092, D-481, D-491; JOB-004).
//!
//! Every database test is `#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]`: one fresh
//! database per test, created and migrated by the role in `DATABASE_URL`, the database's migration
//! identity (`askhava_ci` in smoke and `Ci`, D-492). Test database names are a hash of the test path
//! shared across the cluster, so every run on one cluster uses that one role; a database another role
//! left behind fails with `must be owner of database _sqlx_test_…` (D-491). A test that checks what
//! the app may do runs [`set_app_role`] on its connection first, so the rule is exercised, not assumed.

use sqlx::PgConnection;

/// The role `web` and `worker` connect as; it owns nothing (D-092, postgres brain P3).
pub const APP_ROLE: &str = "askhava_app";

/// PostgreSQL's `insufficient_privilege` code, the error a forbidden statement gives (D-491).
pub const PERMISSION_DENIED: &str = "42501";

/// Makes the rest of this connection run as [`APP_ROLE`]. The migration identity must be a member
/// of it with `SET` (smoke.sh, `Ci`, or `scripts/bootstrap_roles.sql` with `test_cluster=1`).
///
/// # Errors
///
/// The database's error, for example when the current role may not set the app role.
pub async fn set_app_role(conn: &mut PgConnection) -> sqlx::Result<()> {
    sqlx::query("SET ROLE askhava_app").execute(conn).await?;
    Ok(())
}

/// True when `outcome` is the `insufficient_privilege` error (SQLSTATE 42501), the refusal an
/// append-only table gives the app role's `UPDATE`, `DELETE` and `TRUNCATE` (D-092).
pub fn is_permission_denied<T>(outcome: &sqlx::Result<T>) -> bool {
    let Err(sqlx::Error::Database(err)) = outcome else {
        return false;
    };
    err.code().as_deref() == Some(PERMISSION_DENIED)
}
