//! TD-FEE-07 (FEE-009): no flow creates a Stripe customer or charge for a homeowner, and no
//! homeowner-realm route reaches `payments`.
//!
//! Three checks. (1) The `payments` schema has no column that could hold a homeowner, and a Stripe
//! customer row always names a business (`business_id` is the key). (2) The code of `payments`
//! (comments and string literals removed) names no homeowner, so no `payments` function takes
//! one. (3) No file under `app/` that is homeowner-realm (it names the homeowner realm or proof,
//! or serves the homeowner's quote, pick or code route) names `payments`. Each source rule is
//! proved on a planted source. The SetupIntent property cases are
//! `app/payments/src/tests/prop_no_homeowner_charge.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use sqlx::PgPool;

/// Names that make a file homeowner-realm: the realm, its vault proof and the homeowner's routes.
const HOMEOWNER_MARKS: [&str; 6] = [
    "Realm::Homeowner",
    "HomeownerProof",
    ".homeowner(",
    "QUOTE_PAGE_ROUTE",
    "PICK_ROUTE",
    "CODE_ROUTE",
];

/// Names that reach the `payments` crate.
const PAYMENTS_MARKS: [&str; 2] = ["askhava_payments", "payments::api"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `src` with `//` comments dropped and string literal bodies blanked (plain and `r#"…"#`).
fn code_only(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(c) = rest.chars().next() {
        if rest.starts_with("'\"'") {
            out.push_str("' '");
            rest = &rest[3..];
        } else if rest.starts_with("//") {
            rest = rest.find('\n').map_or("", |n| &rest[n..]);
        } else if rest.starts_with("r#\"") {
            rest = rest[3..].find("\"#").map_or("", |n| &rest[3 + n + 2..]);
            out.push_str("\"\"");
        } else if c == '"' {
            rest = string_end(&rest[1..]);
            out.push_str("\"\"");
        } else {
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// The text after the closing quote of a string whose body starts `body`.
fn string_end(body: &str) -> &str {
    let mut escaped = false;
    for (i, c) in body.char_indices() {
        match (escaped, c) {
            (false, '\\') => escaped = true,
            (false, '"') => return &body[i + 1..],
            _ => escaped = false,
        }
    }
    ""
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Finding for a `payments` source that names a homeowner in code.
fn payments_finding(name: &str, src: &str) -> Option<String> {
    let code = code_only(src).to_lowercase();
    code.contains("homeowner")
        .then(|| format!("{name}: payments code names a homeowner"))
}

/// Finding for any other source that is homeowner-realm and names `payments`.
fn route_finding(name: &str, src: &str) -> Option<String> {
    let code = code_only(src);
    let homeowner = HOMEOWNER_MARKS.iter().any(|m| code.contains(m));
    let payments = PAYMENTS_MARKS.iter().any(|m| code.contains(m));
    (homeowner && payments).then(|| format!("{name}: homeowner-realm code reaches payments"))
}

fn scan() -> Vec<String> {
    let mut files = Vec::new();
    rust_files(&root().join("app"), &mut files);
    let payments_src = root().join("app/payments/src");
    let payments_tests = payments_src.join("tests");
    let mut findings = Vec::new();
    for path in files {
        let src = fs::read_to_string(&path).expect("read source");
        let name = path.display().to_string();
        let found = if path.starts_with(&payments_tests) {
            None
        } else if path.starts_with(&payments_src) {
            payments_finding(&name, &src)
        } else {
            route_finding(&name, &src)
        };
        findings.extend(found);
    }
    findings
}

/// TD-FEE-07 (FEE-009): `payments` code takes no homeowner, and no homeowner-realm file reaches
/// `payments`.
#[test]
fn td_fee_07_no_homeowner_route_reaches_payments() {
    let findings = scan();
    assert!(findings.is_empty(), "{findings:#?}");
}

/// TD-FEE-07: each rule fires on a planted source and stays quiet on prose and data.
#[test]
fn td_fee_07_planted_sources_are_found() {
    let planted = "pub fn charge(homeowner_id: Uuid) {}";
    assert!(payments_finding("p.rs", planted).is_some());
    let quiet = "/// a homeowner's pick\nconst T: &str = \"homeowner \\\" text\";\nfn f() {}";
    assert!(payments_finding("p.rs", quiet).is_none());
    assert!(payments_finding("p.rs", "const T: &str = r#\"homeowner\"#;").is_none());
    let route = "use askhava_payments::api::save_card;\nfn r() { Router::new().route(PICK_ROUTE, post(x)); }";
    assert!(route_finding("r.rs", route).is_some());
    assert!(route_finding("r.rs", "use askhava_payments::api::save_card;").is_none());
    assert!(route_finding("r.rs", "// askhava_payments\nfn r() { Realm::Homeowner; }").is_none());
}

/// TD-FEE-07 (FEE-009): no `payments` column can hold a homeowner, and every Stripe customer row is
/// keyed by its business.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_07_payments_tables_hold_no_homeowner(pool: PgPool) {
    let homeowner_columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns WHERE table_schema = 'payments' \
         AND (column_name ILIKE '%homeowner%' OR column_name ILIKE '%phone%' \
         OR column_name ILIKE '%address%' OR column_name ILIKE '%email%')",
    )
    .fetch_one(&pool)
    .await
    .expect("columns");
    assert_eq!(homeowner_columns, 0);
    let key: Vec<String> = sqlx::query_scalar(
        "SELECT a.attname::text FROM pg_index i JOIN pg_attribute a \
           ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) \
         WHERE i.indrelid = 'payments.customer'::regclass AND i.indisprimary",
    )
    .fetch_all(&pool)
    .await
    .expect("primary key");
    assert_eq!(key, ["business_id"]);
}
