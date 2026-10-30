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

/// One top-level statement of a migration: its SQL with comments removed, and everything it holds
/// inside dollar quotes (a `DO` block or function body), where a grant could hide behind an
/// `IF EXISTS` or a `pg_roles` lookup (D-491).
#[derive(Debug, Default)]
pub struct Statement {
    /// The statement as it runs, without its `--` comments or the closing `;`.
    pub text: String,
    /// The text inside the statement's dollar quotes, concatenated.
    pub quoted: String,
}

/// Splits `sql` into its top-level statements, honouring `--` comments, single quotes and
/// dollar quotes, so the grant check reads the migration the way PostgreSQL does (D-491).
#[must_use]
pub fn split_statements(sql: &str) -> Vec<Statement> {
    let mut out = Vec::new();
    let mut current = Statement::default();
    let mut rest = sql;
    while let Some(c) = rest.chars().next() {
        let taken = if rest.starts_with("--") {
            rest.find('\n').unwrap_or(rest.len())
        } else if c == '\'' {
            let end = rest[1..].find('\'').map_or(rest.len(), |i| i + 2);
            current.text.push_str(&rest[..end]);
            end
        } else if let Some(tag) = dollar_tag(rest) {
            let body = &rest[tag.len()..];
            let close = body.find(tag).unwrap_or(body.len());
            current.quoted.push_str(&body[..close]);
            let end = (tag.len() + close + tag.len()).min(rest.len());
            current.text.push_str(&rest[..end]);
            end
        } else if c == ';' {
            out.push(std::mem::take(&mut current));
            1
        } else {
            current.text.push(c);
            c.len_utf8()
        };
        rest = &rest[taken..];
    }
    out.push(current);
    out.retain(|s| !s.text.trim().is_empty());
    out
}

/// The dollar-quote opener (`$$` or `$tag$`) at the start of `sql`, if there is one.
fn dollar_tag(sql: &str) -> Option<&str> {
    let name_end = sql.strip_prefix('$')?.find('$')?;
    let name = &sql[1..=name_end];
    let valid = !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    valid.then(|| &sql[..name_end + 2])
}

/// True when `outcome` is the `insufficient_privilege` error (SQLSTATE 42501), the refusal an
/// append-only table gives the app role's `UPDATE`, `DELETE` and `TRUNCATE` (D-092).
pub fn is_permission_denied<T>(outcome: &sqlx::Result<T>) -> bool {
    let Err(sqlx::Error::Database(err)) = outcome else {
        return false;
    };
    err.code().as_deref() == Some(PERMISSION_DENIED)
}
