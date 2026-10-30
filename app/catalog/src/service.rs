//! Storing the catalog and switching categories (LIC-001, LIC-002, LIC-003, LIC-004, AI-017).
//!
//! Every function takes the caller's connection. Loading never switches an existing category on:
//! the switch is the owner's setting, and the file only decides where a new category starts
//! (LIC-003). It does switch off, with a system audit row, a category that is now not offered, has
//! a ROC kind with no classes, or is gone from the file (LIC-003, LIC-004, AI-017). Switching writes the new state and an `ops` audit row in one transaction (LIC-002).

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::loader::normalize;
use crate::models::{Catalog, CatalogError, Category, CategoryDef, JobType};

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, CatalogError> {
    r.map_err(|_| CatalogError::Database)
}

/// Stores the catalog in one transaction: the not-offered list, then each category with its job
/// types, license rule and fee rule. A new category starts with the file's `enabled`; an existing
/// one keeps its switch (LIC-002, LIC-003).
pub(crate) async fn load(
    db_conn: &mut PgConnection,
    catalog: &Catalog,
) -> Result<(), CatalogError> {
    let mut tx = db(db_conn.begin().await)?;
    for name in &catalog.not_offered_at_launch {
        let name = normalize(name);
        db(sqlx::query!(
            "INSERT INTO catalog.not_offered (name) VALUES ($1) ON CONFLICT DO NOTHING",
            name
        )
        .execute(&mut *tx)
        .await)?;
    }
    for def in &catalog.categories {
        store_category(&mut tx, def).await?;
    }
    drop_stale_not_offered(&mut tx, catalog).await?;
    switch_off_unsafe(&mut tx, catalog).await?;
    db(tx.commit().await)
}

/// Removes not-offered names the file no longer lists, so the stored list is the file's list.
async fn drop_stale_not_offered(
    tx: &mut PgConnection,
    catalog: &Catalog,
) -> Result<(), CatalogError> {
    let names: Vec<String> = catalog
        .not_offered_at_launch
        .iter()
        .map(|n| normalize(n))
        .collect();
    db(sqlx::query!(
        "DELETE FROM catalog.not_offered WHERE name <> ALL($1)",
        &names
    )
    .execute(&mut *tx)
    .await)?;
    Ok(())
}

/// Switches off, with a system audit row each, every stored category that may not stay on after a
/// reload: one now on the not-offered list, a ROC kind with no classes, or one the file no longer
/// lists. A reload never switches anything on (LIC-002, LIC-003, LIC-004, AI-017).
async fn switch_off_unsafe(tx: &mut PgConnection, catalog: &Catalog) -> Result<(), CatalogError> {
    let blocked: Vec<String> = catalog
        .not_offered_at_launch
        .iter()
        .map(|n| normalize(n))
        .collect();
    let listed: Vec<String> = catalog.categories.iter().map(|d| d.id.clone()).collect();
    let ids = db(sqlx::query_scalar!(
        "UPDATE catalog.category c SET enabled = false FROM catalog.license_rule l \
         WHERE l.category_id = c.id AND c.enabled AND ( \
           c.code <> ALL($2) \
           OR replace(lower(btrim(c.code)), '_', ' ') = ANY($1) \
           OR replace(lower(btrim(c.name)), '_', ' ') = ANY($1) \
           OR (l.kind IN ('roc', 'roc_and_state_agency') AND cardinality(l.classes) = 0)) \
         RETURNING c.id",
        &blocked,
        &listed,
    )
    .fetch_all(&mut *tx)
    .await)?;
    for id in ids {
        let entry = AuditEntry {
            actor: Actor::System,
            action: "catalog.category_switched",
            subject_kind: "catalog.category",
            subject_id: id,
            change: Change::Status {
                from: "on",
                to: "off",
            },
        };
        audit(&mut *tx, &entry)
            .await
            .map_err(|_| CatalogError::Database)?;
    }
    Ok(())
}

async fn store_category(tx: &mut PgConnection, def: &CategoryDef) -> Result<(), CatalogError> {
    let id = db(sqlx::query_scalar!(
        "INSERT INTO catalog.category \
         (code, name, trade_group, enabled, recurring, after_hours_emergency, quote_format, keywords) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (code) DO UPDATE SET name = EXCLUDED.name, trade_group = EXCLUDED.trade_group, \
         recurring = EXCLUDED.recurring, after_hours_emergency = EXCLUDED.after_hours_emergency, \
         quote_format = EXCLUDED.quote_format, keywords = EXCLUDED.keywords RETURNING id",
        def.id,
        def.name,
        def.group,
        def.enabled,
        def.recurring,
        def.after_hours_emergency,
        def.quote_format,
        &def.keywords,
    )
    .fetch_one(&mut *tx)
    .await)?;
    db(sqlx::query!(
        "INSERT INTO catalog.license_rule \
         (category_id, kind, classes, agency, handyman_allowed, construction_type) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (category_id) DO UPDATE SET kind = EXCLUDED.kind, classes = EXCLUDED.classes, \
         agency = EXCLUDED.agency, handyman_allowed = EXCLUDED.handyman_allowed, \
         construction_type = EXCLUDED.construction_type",
        id,
        def.license.kind,
        &def.license.classes,
        def.license.agency,
        def.handyman_allowed,
        def.construction_type,
    )
    .execute(&mut *tx)
    .await)?;
    db(sqlx::query!(
        "INSERT INTO catalog.fee_rule (category_id, trip_fee_tier) VALUES ($1, $2) \
         ON CONFLICT (category_id) DO UPDATE SET trip_fee_tier = EXCLUDED.trip_fee_tier",
        id,
        def.trip_fee_tier,
    )
    .execute(&mut *tx)
    .await)?;
    store_job_types(tx, id, def).await
}

async fn store_job_types(
    tx: &mut PgConnection,
    category: Uuid,
    def: &CategoryDef,
) -> Result<(), CatalogError> {
    for name in &def.job_types {
        db(sqlx::query!(
            "INSERT INTO catalog.job_type (category_id, name, one_time, fee_free, kind, quote_format) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (category_id, name) DO UPDATE SET one_time = EXCLUDED.one_time, \
             fee_free = EXCLUDED.fee_free, kind = EXCLUDED.kind, quote_format = EXCLUDED.quote_format",
            category,
            name,
            def.one_time_job_types.contains(name),
            def.fee_free_job_types.contains(name),
            def.job_type_kinds.get(name),
            def.job_type_formats.get(name),
        )
        .execute(&mut *tx)
        .await)?;
    }
    db(sqlx::query!(
        "DELETE FROM catalog.job_type WHERE category_id = $1 AND name <> ALL($2)",
        category,
        &def.job_types,
    )
    .execute(&mut *tx)
    .await)?;
    Ok(())
}

/// The stored category with this code, if any (LIC-001).
pub(crate) async fn category(
    conn: &mut PgConnection,
    code: &str,
) -> Result<Option<Category>, CatalogError> {
    let row = db(sqlx::query!(
        "SELECT c.id, c.code, c.name, c.enabled, c.recurring, c.after_hours_emergency, \
         c.quote_format, c.keywords, l.kind AS license_kind, l.classes AS license_classes, \
         l.agency, l.handyman_allowed, l.construction_type, f.trip_fee_tier \
         FROM catalog.category c JOIN catalog.license_rule l ON l.category_id = c.id \
         JOIN catalog.fee_rule f ON f.category_id = c.id WHERE c.code = $1",
        code
    )
    .fetch_optional(&mut *conn)
    .await)?;
    let Some(r) = row else { return Ok(None) };
    let job_types = db(sqlx::query_as!(
        JobType,
        "SELECT name, one_time, fee_free, kind, quote_format FROM catalog.job_type \
         WHERE category_id = $1 ORDER BY name",
        r.id
    )
    .fetch_all(conn)
    .await)?;
    Ok(Some(Category {
        id: r.id,
        code: r.code,
        name: r.name,
        enabled: r.enabled,
        recurring: r.recurring,
        after_hours_emergency: r.after_hours_emergency,
        quote_format: r.quote_format,
        keywords: r.keywords,
        license_kind: r.license_kind,
        license_classes: r.license_classes,
        agency: r.agency,
        handyman_allowed: r.handyman_allowed,
        construction_type: r.construction_type,
        trip_fee_tier: r.trip_fee_tier,
        job_types,
    }))
}

/// The codes of the categories now switched on, sorted (LIC-003).
pub(crate) async fn enabled_codes(conn: &mut PgConnection) -> Result<Vec<String>, CatalogError> {
    db(
        sqlx::query_scalar!("SELECT code FROM catalog.category WHERE enabled ORDER BY code")
            .fetch_all(conn)
            .await,
    )
}

/// True when `name` (any case, `_` or space) is one of the kinds not offered at launch (LIC-004).
pub(crate) async fn is_not_offered(
    conn: &mut PgConnection,
    name: &str,
) -> Result<bool, CatalogError> {
    let name = normalize(name);
    let found = db(sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM catalog.not_offered WHERE name = $1)",
        name
    )
    .fetch_one(conn)
    .await)?;
    Ok(found.unwrap_or(false))
}

/// Switches a category on or off for the owner and logs it (LIC-002). Returns false, writing
/// nothing, when it was already in that state. A not-offered kind (LIC-004) and a ROC category with
/// no resolved class (LIC-003, AI-017) cannot be switched on.
pub(crate) async fn set_enabled(
    conn: &mut PgConnection,
    code: &str,
    on: bool,
    actor: Actor,
) -> Result<bool, CatalogError> {
    if !matches!(actor, Actor::Owner(_)) {
        return Err(CatalogError::OwnerOnly);
    }
    let cat = category(conn, code)
        .await?
        .ok_or(CatalogError::UnknownCategory)?;
    if cat.enabled == on {
        return Ok(false);
    }
    if on {
        if is_not_offered(conn, &cat.code).await? || is_not_offered(conn, &cat.name).await? {
            return Err(CatalogError::NotOffered);
        }
        let needs_class = cat.license_kind.starts_with("roc");
        if needs_class && cat.license_classes.is_empty() {
            return Err(CatalogError::LicenseUnresolved);
        }
    }
    let mut tx = db(conn.begin().await)?;
    let changed = db(sqlx::query!(
        "UPDATE catalog.category SET enabled = $2 WHERE id = $1 AND enabled = NOT $2",
        cat.id,
        on
    )
    .execute(&mut *tx)
    .await)?
    .rows_affected();
    if changed == 0 {
        return Ok(false);
    }
    let (from, to) = if on { ("off", "on") } else { ("on", "off") };
    let entry = AuditEntry {
        actor,
        action: "catalog.category_switched",
        subject_kind: "catalog.category",
        subject_id: cat.id,
        change: Change::Status { from, to },
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| CatalogError::Database)?;
    db(tx.commit().await)?;
    Ok(true)
}
