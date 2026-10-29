//! Throwaway spike (D-476): proves the pinned Rust stack builds and tests in `Ci`. Never merged.

use askama::Template;

/// A one-line page, to prove Askama compiles inside the workspace.
#[derive(Template)]
#[template(source = "<p>{{ name }}</p>", ext = "html")]
pub struct Hello<'a> {
    /// The name shown on the page.
    pub name: &'a str,
}

/// Runs `SELECT 1` against Postgres, to prove SQLx reaches the CI database.
///
/// # Errors
/// Returns the SQLx error if the database can't be reached.
pub async fn ping(url: &str) -> Result<i32, sqlx::Error> {
    let pool = sqlx::PgPool::connect(url).await?;
    let one: (i32,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await?;
    Ok(one.0)
}

/// The router the server would serve, to prove Axum compiles.
pub fn router() -> axum::Router {
    axum::Router::new().route("/healthz", axum::routing::get(|| async { "ok" }))
}
