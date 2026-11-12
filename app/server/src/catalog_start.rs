//! The catalog file stored at start-up (LIC-001, LIC-002, LIC-003; D-3094, D-3100, IC-96).
//!
//! `catalog::api::load_embedded` parses the catalog file compiled into the build and stores it,
//! but nothing ran it, so on a fresh deployment `catalog.category` was empty and every reader
//! (intake's `is_enabled`, `ai`'s `enabled_categories`, the sign-up checklist) saw every category
//! as off. The `web` process calls [`store_catalog`] once after the app pool is built and before
//! it accepts a request; the `migrate` and `bootstrap-owner` commands never do. Loading is safe to
//! repeat and never switches an existing category on, so a restart keeps every switch the owner
//! set (LIC-002), and two web processes starting at once both succeed (each category is an
//! upsert). A failure stops the process, as bad settings do (SEC-002).

use std::io;

use sqlx::PgPool;

/// LIC-001: stores the compiled-in catalog over one connection from the app pool. A new category
/// starts with the file's switch and an existing one keeps its own (LIC-002, LIC-003).
///
/// # Errors
///
/// An [`io::Error`] that names no value: the database is unreachable, or the load failed.
pub async fn store_catalog(pool: &PgPool) -> io::Result<()> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|_| io::Error::other("the database is unreachable for the catalog load"))?;
    askhava_catalog::api::load_embedded(&mut conn)
        .await
        .map_err(|_| io::Error::other("the catalog file could not be stored"))
}
