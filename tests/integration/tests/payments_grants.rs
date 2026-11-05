//! TD-SEC-33 (FEE-028, FEE-042; D-092, D-1495): after every migration, the app role's privileges
//! on the `payments` schema are exactly what the money rules allow. The schema's default ACL gives
//! `askhava_app` SELECT and INSERT on new tables and nothing more (`migrations/0098`); each pinned
//! table's privileges equal its allow-list entry; no table grants TRUNCATE, REFERENCES or TRIGGER;
//! and every table, pinned or not, holds no more than SELECT, INSERT and what an explicit
//! `GRANT ... TO askhava_app` statement in `migrations/` gives it. A table a later task adds
//! (T066, T067, T210) needs no edit here: a forgotten REVOKE or an extra privilege its migration
//! does not grant by name fails `Ci`.
//!
//! Privileges are read from the catalogs (`pg_class.relacl`, `pg_attribute.attacl`,
//! `pg_default_acl`), a column grant written as `UPDATE(column)`. The GRANT parser drops `--`
//! comments, splits on `;`, folds whitespace and case, and reads comma lists of tables, statements
//! over several lines and column lists as 0093, 0095 and 0096 write them.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use sqlx::{PgConnection, PgPool};

/// Table name (without `payments.`) to the app role's privileges on it.
type Grants = BTreeMap<String, BTreeSet<String>>;

/// What every new payments table gets by default (D-1495).
const DEFAULT: [&str; 2] = ["SELECT", "INSERT"];

/// Privileges no payments table may give the app role.
const NEVER: [&str; 3] = ["TRUNCATE", "REFERENCES", "TRIGGER"];

/// What `ALL [PRIVILEGES]` on a table expands to.
const ALL: [&str; 7] = [
    "SELECT",
    "INSERT",
    "UPDATE",
    "DELETE",
    "TRUNCATE",
    "REFERENCES",
    "TRIGGER",
];

/// The explicit allow list: each table and why it may hold more than SELECT, INSERT.
#[rustfmt::skip]
const PINNED: [(&str, &[&str]); 12] = [
    // FEE-028: the append-only ledger.
    ("ledger_entry", &DEFAULT),
    ("customer", &DEFAULT),
    // FEE-015: the card-authorization record is append-only.
    ("card_authorization", &DEFAULT),
    // FEE-014: a saved card is only ever marked removed.
    ("payment_method", &["SELECT", "INSERT", "UPDATE(removed_at)"]),
    ("entity_key", &DEFAULT),
    ("free_win", &DEFAULT),
    ("free_win_claim", &DEFAULT),
    // FEE-016 to FEE-020: pick, attempt and timer status moves.
    ("pick_intent", &["SELECT", "INSERT", "UPDATE"]),
    ("charge_attempt", &["SELECT", "INSERT", "UPDATE"]),
    ("pay_timer", &["SELECT", "INSERT", "UPDATE"]),
    // FEE-041: the monthly limit is a setting a business changes or clears.
    ("fee_limit", &["SELECT", "INSERT", "UPDATE", "DELETE"]),
    // FEE-042: the evidence record is kept as the ledger is.
    ("charge_evidence", &DEFAULT),
];

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// Every `payments` table with the app role's table-level and column-level privileges on it.
async fn actual_grants(conn: &mut PgConnection) -> Grants {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'payments' AND c.relkind IN ('r', 'p')",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("payments tables");
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT c.relname::text, a.privilege_type FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace CROSS JOIN LATERAL aclexplode(c.relacl) a \
         WHERE n.nspname = 'payments' AND c.relkind IN ('r', 'p') \
         AND a.grantee = 'askhava_app'::regrole \
         UNION ALL \
         SELECT c.relname::text, a.privilege_type || '(' || att.attname || ')' FROM pg_attribute att \
         JOIN pg_class c ON c.oid = att.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
         CROSS JOIN LATERAL aclexplode(att.attacl) a \
         WHERE n.nspname = 'payments' AND c.relkind IN ('r', 'p') AND att.attnum > 0 \
         AND NOT att.attisdropped AND a.grantee = 'askhava_app'::regrole",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("payments privileges");
    let mut grants: Grants = tables.into_iter().map(|t| (t, BTreeSet::new())).collect();
    for (table, privilege) in rows {
        grants.entry(table).or_default().insert(privilege);
    }
    grants
}

/// Splits a privilege list on the commas outside parentheses.
fn split_top_level(list: &str) -> Vec<String> {
    let (mut items, mut current, mut depth) = (Vec::new(), String::new(), 0_u32);
    for ch in list.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                items.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    items.push(current);
    items
}

/// `select, update (a, b)` to `SELECT`, `UPDATE(a)`, `UPDATE(b)`; `all` to every table privilege.
fn privilege_list(list: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for item in split_top_level(list) {
        let item = item.trim();
        if let Some((kind, columns)) = item.split_once('(') {
            let kind = kind.trim().to_ascii_uppercase();
            for column in columns.trim_end_matches(')').split(',') {
                out.insert(format!("{kind}({})", column.trim()));
            }
        } else if item == "all" || item == "all privileges" {
            out.extend(set(&ALL));
        } else {
            out.insert(item.to_ascii_uppercase());
        }
    }
    out
}

/// One whitespace-folded, lower-case statement to its privileges and `payments` tables, when it is
/// a `GRANT ... ON [TABLE] payments.x[, ...] TO askhava_app`.
fn grant_parts(statement: &str) -> Option<(BTreeSet<String>, Vec<String>)> {
    let rest = statement.strip_prefix("grant ")?;
    let (privileges, rest) = rest.split_once(" on ")?;
    let (objects, grantee) = rest.rsplit_once(" to ")?;
    if grantee.split_whitespace().next() != Some("askhava_app") {
        return None;
    }
    let objects = objects.strip_prefix("table ").unwrap_or(objects);
    let tables: Vec<String> = objects
        .split(',')
        .filter_map(|object| object.trim().strip_prefix("payments."))
        .map(str::to_owned)
        .collect();
    (!tables.is_empty()).then(|| (privilege_list(privileges), tables))
}

/// Adds what the explicit `GRANT ... TO askhava_app` statements in `sql` give each payments table.
fn parse_grants(sql: &str, into: &mut Grants) {
    let text: Vec<&str> = sql
        .lines()
        .map(|line| line.split("--").next().unwrap_or(""))
        .collect();
    for statement in text.join(" ").split(';') {
        let folded = statement.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some((privileges, tables)) = grant_parts(&folded.to_ascii_lowercase()) {
            for table in tables {
                into.entry(table).or_default().extend(privileges.clone());
            }
        }
    }
}

/// The explicit grants of every file in the root `migrations/`.
fn migration_grants() -> Grants {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    let mut grants = Grants::new();
    for entry in fs::read_dir(&dir).expect("migrations dir") {
        let path = entry.expect("migration entry").path();
        if path.extension().is_some_and(|ext| ext == "sql") {
            parse_grants(&fs::read_to_string(&path).expect("migration"), &mut grants);
        }
    }
    grants
}

/// The problems with one table's privileges: a NEVER privilege, a pinned table off its list, or
/// anything beyond the default and the table's explicit GRANTs.
fn table_problems(table: &str, held: &BTreeSet<String>, granted: &Grants) -> Vec<String> {
    let mut problems = Vec::new();
    for never in held.iter().filter(|p| NEVER.contains(&p.as_str())) {
        problems.push(format!("payments.{table} grants the app role {never}"));
    }
    if let Some((_, allowed)) = PINNED.iter().find(|(name, _)| *name == table)
        && *held != set(allowed)
    {
        problems.push(format!(
            "payments.{table}: app role holds {held:?}, allow list {allowed:?}"
        ));
    }
    let mut allowed = set(&DEFAULT);
    allowed.extend(granted.get(table).cloned().unwrap_or_default());
    for extra in held.difference(&allowed) {
        problems.push(format!(
            "payments.{table}: {extra} is neither default nor an explicit GRANT"
        ));
    }
    problems
}

/// TD-SEC-33 (FEE-028, FEE-042): every payments table's grants after all migrations.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn every_payments_table_holds_only_its_allowed_privileges(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("conn");
    let held = actual_grants(&mut conn).await;
    for (table, _) in PINNED {
        assert!(
            held.contains_key(table),
            "pinned table payments.{table} is missing"
        );
    }
    let granted = migration_grants();
    let problems: Vec<String> = held
        .iter()
        .flat_map(|(table, privileges)| table_problems(table, privileges, &granted))
        .collect();
    assert!(
        problems.is_empty(),
        "payments grants:\n{}",
        problems.join("\n")
    );
}

/// TD-SEC-33 (D-1495): the schema's default ACL gives the app role SELECT and INSERT only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn payments_default_privileges_are_select_and_insert(pool: PgPool) {
    let privileges: Vec<String> = sqlx::query_scalar(
        "SELECT a.privilege_type FROM pg_default_acl d \
         JOIN pg_namespace n ON n.oid = d.defaclnamespace \
         CROSS JOIN LATERAL aclexplode(d.defaclacl) a \
         WHERE n.nspname = 'payments' AND d.defaclobjtype = 'r' \
         AND a.grantee = 'askhava_app'::regrole",
    )
    .fetch_all(&pool)
    .await
    .expect("default acl");
    assert_eq!(
        privileges.into_iter().collect::<BTreeSet<_>>(),
        set(&DEFAULT)
    );
}

/// TD-SEC-33 (FEE-028, D-092): a table the migration role creates now gets SELECT and INSERT only,
/// and the app role cannot update, delete from or truncate it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_new_payments_table_is_append_only_for_the_app_role(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("conn");
    sqlx::query("CREATE TABLE payments.t245_probe (id bigint PRIMARY KEY, note text)")
        .execute(&mut *conn)
        .await
        .expect("create probe");
    let held = actual_grants(&mut conn).await;
    assert_eq!(held.get("t245_probe"), Some(&set(&DEFAULT)));
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *conn)
        .await
        .expect("set role");
    sqlx::query("INSERT INTO payments.t245_probe VALUES (1, 'fake')")
        .execute(&mut *conn)
        .await
        .expect("app role inserts");
    for denied in [
        "UPDATE payments.t245_probe SET note = 'changed'",
        "DELETE FROM payments.t245_probe",
        "TRUNCATE payments.t245_probe",
    ] {
        let error = sqlx::query(denied)
            .execute(&mut *conn)
            .await
            .expect_err(denied);
        assert!(
            error.to_string().contains("permission denied"),
            "{denied}: {error}"
        );
    }
}

/// TD-SEC-33: the parser reads comma lists, statements over several lines and column grants, and
/// ignores comments, other grantees and other objects.
#[test]
fn the_grant_parser_reads_the_forms_migrations_use() {
    let mut grants = Grants::new();
    parse_grants(
        "-- GRANT DELETE ON payments.a TO askhava_app;\n\
         GRANT SELECT, INSERT ON payments.a, payments.b\n  TO askhava_app;\n\
         GRANT UPDATE (removed_at, note) ON payments.c TO askhava_app; -- trailing\n\
         GRANT ALL ON TABLE payments.d TO askhava_app;\n\
         GRANT DELETE ON payments.a TO someone_else;\n\
         GRANT USAGE ON SCHEMA payments TO askhava_app;\n\
         REVOKE INSERT ON payments.b FROM askhava_app;",
        &mut grants,
    );
    assert_eq!(grants.get("a"), Some(&set(&DEFAULT)));
    assert_eq!(grants.get("b"), Some(&set(&DEFAULT)));
    assert_eq!(
        grants.get("c"),
        Some(&set(&["UPDATE(note)", "UPDATE(removed_at)"]))
    );
    assert_eq!(grants.get("d"), Some(&set(&ALL)));
    assert_eq!(grants.len(), 4);
}

/// TD-SEC-33: the parser finds the real grants of 0093, 0095 and 0096.
#[test]
fn the_grant_parser_reads_the_payments_migrations() {
    let grants = migration_grants();
    let payment_method = grants.get("payment_method").expect("0093");
    assert!(payment_method.contains("UPDATE(removed_at)"));
    assert!(!payment_method.contains("UPDATE"));
    for table in ["entity_key", "free_win", "free_win_claim"] {
        assert_eq!(grants.get(table), Some(&set(&DEFAULT)), "0095 {table}");
    }
    for table in ["pick_intent", "charge_attempt", "pay_timer"] {
        assert_eq!(
            grants.get(table),
            Some(&set(&["SELECT", "INSERT", "UPDATE"])),
            "0096 {table}"
        );
    }
    let fee_limit = set(&["SELECT", "INSERT", "UPDATE", "DELETE"]);
    assert_eq!(grants.get("fee_limit"), Some(&fee_limit));
}
