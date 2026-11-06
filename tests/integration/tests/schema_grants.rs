//! TD-SEC-34 (OPS-010, CON-022; D-092, D-1520): after every migration, the app role's privileges
//! on every schema but `payments` (TD-SEC-33, `payments_grants.rs`) are exactly what the rules
//! allow. Every schema's default ACL gives `askhava_app` SELECT and INSERT on new tables and nothing
//! more (`migrations/0110`), a schema a later migration creates included (`calls`, T076;
//! `recovery`, T109); each pinned table's privileges equal its allow-list entry, every UPDATE or
//! DELETE named with what needs it; no table grants TRUNCATE, REFERENCES or TRIGGER; the
//! append-only tables the spec names have no UPDATE or DELETE; and every table, pinned or not,
//! holds no more than SELECT, INSERT and what an explicit `GRANT ... TO askhava_app` statement in
//! `migrations/` gives it, so a table a later task adds needs no edit here.
//!
//! Privileges are read from the catalogs (`pg_class.relacl`, `pg_attribute.attacl`,
//! `pg_default_acl`), a column grant written as `UPDATE(column)`. The GRANT parser reads the forms
//! T245's test reads: `--` comments dropped, statements split on `;`, whitespace and case folded,
//! comma lists of tables, statements over several lines and column lists.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use sqlx::{PgConnection, PgPool};

/// `schema.table` to the app role's privileges on it.
type Grants = BTreeMap<String, BTreeSet<String>>;

/// What every new table gets by default (D-1520).
const DEFAULT: [&str; 2] = ["SELECT", "INSERT"];

/// SELECT, INSERT and UPDATE: a row whose status or fields move, never deleted.
const UPDATES: [&str; 3] = ["SELECT", "INSERT", "UPDATE"];

/// All four: a row that is edited and removed.
const EDITS: [&str; 4] = ["SELECT", "INSERT", "UPDATE", "DELETE"];

/// Privileges no table may give the app role.
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

/// The schema TD-SEC-33 checks instead.
const PAYMENTS: &str = "payments";

/// The append-only tables the spec names: no UPDATE or DELETE, ever (OPS-010, CON-022, D-1015).
const APPEND_ONLY: [&str; 4] = [
    "ops.audit_log",
    "intake.consent_record",
    "messaging.optout_event",
    "quotes.job_card",
];

/// The thirteen schemas 0110 narrows (D-1520); `payments` is 0098's (D-1495).
const SCHEMAS: [&str; 13] = [
    "core",
    "ops",
    "catalog",
    "jobs",
    "privacy",
    "ai",
    "messaging",
    "licensing",
    "intake",
    "businesses",
    "portals",
    "quotes",
    "picks",
];

/// The explicit allow list as the migrations stand at T250: each table outside `payments` and why
/// it may hold more than SELECT, INSERT. "Default" is the four-privilege schema default its
/// migration ran under (0019 to 0079), which 0110 removes for tables created later; an upsert is an
/// `INSERT ... ON CONFLICT DO UPDATE`, which needs UPDATE.
#[rustfmt::skip]
const PINNED: [(&str, &[&str]); 66] = [
    // 0040, 0041: AI call counts, provider terms, policy versions, the switch history and readings
    // are append-only (AI-008, D-412).
    ("ai.call_stat", &DEFAULT),
    ("ai.privacy_policy_version", &DEFAULT),
    ("ai.provider_terms", &DEFAULT),
    ("ai.reading", &DEFAULT),
    ("ai.switch_history", &DEFAULT),
    // 0055: a business record is edited, never deleted (explicit GRANT).
    ("businesses.business", &UPDATES),
    // 0055: BIZ-010, a business edits and removes its towns and job-type choices (explicit GRANT).
    ("businesses.job_type_choice", &EDITS),
    ("businesses.town", &EDITS),
    // 0056: staff phones are added, changed and removed; team members are edited, never deleted
    // (explicit GRANTs).
    ("businesses.staff_phone", &EDITS),
    ("businesses.team_member", &UPDATES),
    // 0027 default: the catalog is settings data, upserted and pruned by the catalog loader.
    ("catalog.category", &EDITS),
    ("catalog.fee_rule", &EDITS),
    ("catalog.job_type", &EDITS),
    ("catalog.license_rule", &EDITS),
    ("catalog.not_offered", &EDITS),
    // 0020 default: the rate-limit upsert and prune (SEC-040); task claim, finish and lease moves
    // (JOB-006).
    ("core.rate_limit", &EDITS),
    ("core.task", &EDITS),
    // 0053: CON-022, the consent record is append-only.
    ("intake.consent_record", &DEFAULT),
    // 0053 default: the conversation, its answers, the homeowner, the property, emergency events and
    // recruiting demand move with the intake and are upserted or cleared by it.
    ("intake.conversation", &EDITS),
    ("intake.emergency_event", &EDITS),
    ("intake.homeowner", &EDITS),
    ("intake.intake_answer", &EDITS),
    ("intake.property", &EDITS),
    ("intake.recruiting_demand", &EDITS),
    // 0083: golden results are append-only.
    ("intake.golden_result", &DEFAULT),
    // 0076: a homeowner pause is lifted by UPDATE, never deleted (DELETE revoked).
    ("intake.homeowner_pause", &UPDATES),
    // 0054: CON-022, terms acceptances are kept as consent is.
    ("intake.terms_acceptance", &DEFAULT),
    // 0031 default: JOB-004, the guarded status move; 0032 default: a quote token is revoked.
    ("jobs.job", &EDITS),
    ("jobs.quote_token", &EDITS),
    // 0034: job numbers are append-only; the counter row only moves (INSERT revoked).
    ("jobs.job_number", &DEFAULT),
    ("jobs.job_number_next", &["SELECT", "UPDATE"]),
    // 0031: the status history is append-only.
    ("jobs.status_history", &DEFAULT),
    // 0050: imports, agency records, city entries and checks are append-only.
    ("licensing.agency_record", &DEFAULT),
    ("licensing.city_license_entry", &DEFAULT),
    ("licensing.license_check", &DEFAULT),
    ("licensing.list_import", &DEFAULT),
    // 0048: a business license is re-checked by UPDATE, never deleted (explicit GRANT).
    ("licensing.business_license", &UPDATES),
    // 0042, 0043, 0044, 0089 default: outbox and retry status moves, webhook and delivery upserts,
    // the sender directory sync and suppression list add and remove rows.
    ("messaging.delivery_event", &EDITS),
    ("messaging.email_event", &EDITS),
    ("messaging.email_outbox", &EDITS),
    ("messaging.inbox", &EDITS),
    ("messaging.outbox", &EDITS),
    ("messaging.portal_notice", &EDITS),
    ("messaging.sender_directory", &EDITS),
    ("messaging.suppression", &EDITS),
    ("messaging.text_retry", &EDITS),
    ("messaging.update_channel", &EDITS),
    ("messaging.webhook_event", &EDITS),
    // 0044: CON-022, the opt-out record is append-only.
    ("messaging.optout_event", &DEFAULT),
    // 0021 default: alerts are upserted and acknowledged; pause switches flip.
    ("ops.alert", &EDITS),
    ("ops.pause_switch", &EDITS),
    // 0021: OPS-010, the audit log and the switch history are append-only.
    ("ops.audit_log", &DEFAULT),
    ("ops.switch_history", &DEFAULT),
    // 0106: releases and address waits are append-only.
    ("picks.address_wait", &DEFAULT),
    ("picks.release", &DEFAULT),
    // 0079: a pick and a code attempt move status, never deleted (explicit GRANTs); the daily cap
    // and the terms line are append-only.
    ("picks.code_attempt", &UPDATES),
    ("picks.pick", &UPDATES),
    ("picks.terms_line", &DEFAULT),
    ("picks.verify_cap_day", &DEFAULT),
    // 0058: security events are append-only; 0058 default: sessions are renewed and ended.
    ("portals.security_event", &DEFAULT),
    ("portals.session", &EDITS),
    // 0036 default: the vault's contact, blind index and read counts are updated and erased
    // (PRIV-002).
    ("privacy.contact", &EDITS),
    ("privacy.contact_index", &EDITS),
    ("privacy.read_count", &EDITS),
    // 0067: D-1015, the job card is append-only.
    ("quotes.job_card", &DEFAULT),
    // 0070: a quote is confirmed and withdrawn by UPDATE, never deleted (explicit GRANT).
    ("quotes.quote", &UPDATES),
];

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// The table-level and column-level privileges the app role holds outside `payments`.
const HELD_SQL: &str = "SELECT n.nspname || '.' || c.relname, a.privilege_type FROM pg_class c \
     JOIN pg_namespace n ON n.oid = c.relnamespace CROSS JOIN LATERAL aclexplode(c.relacl) a \
     WHERE c.relkind IN ('r', 'p') AND a.grantee = 'askhava_app'::regrole AND n.nspname <> $1 \
     UNION ALL \
     SELECT n.nspname || '.' || c.relname, a.privilege_type || '(' || att.attname || ')' \
     FROM pg_attribute att JOIN pg_class c ON c.oid = att.attrelid \
     JOIN pg_namespace n ON n.oid = c.relnamespace CROSS JOIN LATERAL aclexplode(att.attacl) a \
     WHERE c.relkind IN ('r', 'p') AND att.attnum > 0 AND NOT att.attisdropped \
     AND a.grantee = 'askhava_app'::regrole AND n.nspname <> $1";

/// Every table in a module schema (one with a default ACL or a grant to the app role) outside
/// `payments`, with the app role's privileges on it.
async fn actual_grants(conn: &mut PgConnection) -> Grants {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT n.nspname || '.' || c.relname FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relkind IN ('r', 'p') AND n.nspname <> $1 \
         AND has_schema_privilege('askhava_app', n.oid, 'USAGE') \
         AND n.nspname NOT IN ('public', 'pg_catalog', 'information_schema')",
    )
    .bind(PAYMENTS)
    .fetch_all(&mut *conn)
    .await
    .expect("tables");
    let rows: Vec<(String, String)> = sqlx::query_as(HELD_SQL)
        .bind(PAYMENTS)
        .fetch_all(&mut *conn)
        .await
        .expect("privileges");
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

/// One whitespace-folded, lower-case statement to its privileges and `schema.table` names, when it
/// is a `GRANT ... ON [TABLE] s.x[, ...] TO askhava_app` (schema grants and defaults are skipped).
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
        .map(str::trim)
        .filter(|object| object.contains('.') && !object.contains(' '))
        .map(str::to_owned)
        .collect();
    (!tables.is_empty()).then(|| (privilege_list(privileges), tables))
}

/// Adds what the explicit `GRANT ... TO askhava_app` statements in `sql` give each table.
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

/// The problems with one table's privileges: a NEVER privilege, an append-only table that can be
/// changed, a pinned table off its list, or anything beyond the default and its explicit GRANTs.
fn table_problems(table: &str, held: &BTreeSet<String>, granted: &Grants) -> Vec<String> {
    let mut problems = Vec::new();
    for never in held.iter().filter(|p| NEVER.contains(&p.as_str())) {
        problems.push(format!("{table} grants the app role {never}"));
    }
    if APPEND_ONLY.contains(&table) && *held != set(&DEFAULT) {
        problems.push(format!("{table} is append-only but holds {held:?}"));
    }
    match PINNED.iter().find(|(name, _)| *name == table) {
        Some((_, allowed)) if *held != set(allowed) => problems.push(format!(
            "{table}: app role holds {held:?}, allow list {allowed:?}"
        )),
        Some(_) => {}
        None => {
            let mut allowed = set(&DEFAULT);
            allowed.extend(granted.get(table).cloned().unwrap_or_default());
            for extra in held.difference(&allowed) {
                problems.push(format!(
                    "{table}: {extra} is neither default nor an explicit GRANT"
                ));
            }
        }
    }
    problems
}

/// TD-SEC-34 (OPS-010, CON-022): every table's grants outside `payments` after all migrations.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn every_table_holds_only_its_allowed_privileges(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("conn");
    let held = actual_grants(&mut conn).await;
    for (table, _) in PINNED {
        assert!(held.contains_key(table), "pinned table {table} is missing");
    }
    for table in APPEND_ONLY {
        assert!(
            held.contains_key(table),
            "append-only table {table} is missing"
        );
    }
    let granted = migration_grants();
    let problems: Vec<String> = held
        .iter()
        .flat_map(|(table, privileges)| table_problems(table, privileges, &granted))
        .collect();
    assert!(problems.is_empty(), "grants:\n{}", problems.join("\n"));
}

/// The app role's privileges in each schema's default ACL for tables, `payments` aside.
async fn default_acls(pool: &PgPool) -> Grants {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT coalesce(n.nspname, '(every schema)'), a.privilege_type FROM pg_default_acl d \
         LEFT JOIN pg_namespace n ON n.oid = d.defaclnamespace \
         CROSS JOIN LATERAL aclexplode(d.defaclacl) a \
         WHERE d.defaclobjtype = 'r' AND a.grantee = 'askhava_app'::regrole \
         AND n.nspname IS DISTINCT FROM $1",
    )
    .bind(PAYMENTS)
    .fetch_all(pool)
    .await
    .expect("default acl");
    let mut grants = Grants::new();
    for (schema, privilege) in rows {
        grants.entry(schema).or_default().insert(privilege);
    }
    grants
}

/// TD-SEC-34 (D-1520): every schema's default ACL but `payments` gives the app role SELECT and
/// INSERT only, the thirteen 0110 narrows and any schema a later migration creates.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn every_schema_default_is_select_and_insert(pool: PgPool) {
    let defaults = default_acls(&pool).await;
    for schema in SCHEMAS {
        assert!(defaults.contains_key(schema), "{schema} has no default ACL");
    }
    for (schema, privileges) in &defaults {
        assert_eq!(privileges, &set(&DEFAULT), "default ACL of {schema}");
    }
}

/// Runs one statement as the app role and expects `permission denied`.
async fn assert_denied(conn: &mut PgConnection, statement: String) {
    let error = sqlx::raw_sql(sqlx::AssertSqlSafe(statement.clone()))
        .execute(&mut *conn)
        .await
        .expect_err(&statement);
    assert!(
        error.to_string().contains("permission denied"),
        "{statement}: {error}"
    );
}

/// TD-SEC-34 (OPS-010, CON-022, D-092): a table the migration role creates now in any of the
/// thirteen schemas gets SELECT and INSERT only, and the app role cannot update, delete from or
/// truncate it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_new_table_in_any_schema_is_append_only_for_the_app_role(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("conn");
    for schema in SCHEMAS {
        let sql = format!("CREATE TABLE {schema}.t250_probe (id bigint PRIMARY KEY, note text)");
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(&mut *conn)
            .await
            .expect("create probe");
    }
    let held = actual_grants(&mut conn).await;
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *conn)
        .await
        .expect("set role");
    for schema in SCHEMAS {
        let probe = format!("{schema}.t250_probe");
        assert_eq!(held.get(&probe), Some(&set(&DEFAULT)), "{probe}");
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {probe} VALUES (1, 'fake')"
        )))
        .execute(&mut *conn)
        .await
        .expect("app role inserts");
        assert_denied(&mut conn, format!("UPDATE {probe} SET note = 'changed'")).await;
        assert_denied(&mut conn, format!("DELETE FROM {probe}")).await;
        assert_denied(&mut conn, format!("TRUNCATE {probe}")).await;
    }
}

/// TD-SEC-34: a table outside the pinned list may hold only SELECT, INSERT and its explicit
/// GRANTs, and an append-only table that can be changed is reported even when pinned.
#[test]
fn an_unpinned_or_append_only_table_is_held_to_its_grants() {
    let mut granted = Grants::new();
    parse_grants(
        "GRANT SELECT, INSERT, UPDATE ON calls.call TO askhava_app;",
        &mut granted,
    );
    assert!(table_problems("calls.call", &set(&UPDATES), &granted).is_empty());
    let extra = table_problems("calls.leg", &set(&UPDATES), &granted);
    assert_eq!(
        extra,
        ["calls.leg: UPDATE is neither default nor an explicit GRANT"]
    );
    let truncate = table_problems("calls.leg", &set(&["SELECT", "TRUNCATE"]), &granted);
    assert!(
        truncate
            .iter()
            .any(|p| p == "calls.leg grants the app role TRUNCATE")
    );
    let audit = table_problems("ops.audit_log", &set(&UPDATES), &granted);
    assert!(
        audit
            .iter()
            .any(|p| p.starts_with("ops.audit_log is append-only"))
    );
    let pinned = table_problems("quotes.quote", &set(&EDITS), &granted);
    assert!(
        pinned
            .iter()
            .any(|p| p.starts_with("quotes.quote: app role holds"))
    );
}

/// TD-SEC-34: the parser reads comma lists over several schemas, statements over several lines and
/// column grants, and ignores comments, other grantees, schema grants and default privileges.
#[test]
fn the_grant_parser_reads_the_forms_migrations_use() {
    let mut grants = Grants::new();
    parse_grants(
        "-- GRANT DELETE ON jobs.a TO askhava_app;\n\
         GRANT SELECT, INSERT ON jobs.a, intake.b\n  TO askhava_app;\n\
         GRANT UPDATE (removed_at, note) ON TABLE calls.c TO askhava_app; -- trailing\n\
         GRANT ALL PRIVILEGES ON recovery.d TO askhava_app;\n\
         GRANT DELETE ON jobs.a TO someone_else;\n\
         GRANT USAGE ON SCHEMA jobs TO askhava_app;\n\
         ALTER DEFAULT PRIVILEGES IN SCHEMA jobs GRANT SELECT, INSERT ON TABLES TO askhava_app;\n\
         REVOKE INSERT ON intake.b FROM askhava_app;",
        &mut grants,
    );
    assert_eq!(grants.get("jobs.a"), Some(&set(&DEFAULT)));
    assert_eq!(grants.get("intake.b"), Some(&set(&DEFAULT)));
    assert_eq!(
        grants.get("calls.c"),
        Some(&set(&["UPDATE(note)", "UPDATE(removed_at)"]))
    );
    assert_eq!(grants.get("recovery.d"), Some(&set(&ALL)));
    assert_eq!(grants.len(), 4);
}

/// TD-SEC-34: the parser finds the real explicit grants of 0050, 0055 and 0079.
#[test]
fn the_grant_parser_reads_the_real_migrations() {
    let grants = migration_grants();
    assert_eq!(grants.get("businesses.town"), Some(&set(&EDITS)));
    assert_eq!(grants.get("businesses.job_type_choice"), Some(&set(&EDITS)));
    assert_eq!(grants.get("picks.pick"), Some(&set(&UPDATES)));
    assert_eq!(grants.get("licensing.list_import"), Some(&set(&DEFAULT)));
    assert!(!grants.contains_key("ops.audit_log"));
}
