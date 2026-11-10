//! Every documented route is served, and every route still to come names the task that wires it
//! (TD-SEC-41, T284, D-1868, D-2269 to D-2271).
//!
//! `docs/architecture.md` section 13 is compiled in: one row per method and path, `Wired` either
//! `yes` or the ID of the queued task in `kit/tasks.json` that will wire it. The router is the one
//! `build_router` builds with every optional part configured from fake values over a lazy pool
//! (no database is reached). Each row is probed with an extension method no route takes, sent as
//! a same-origin request: a matched path answers 405 with an `Allow` header naming the methods it
//! takes, the fallback answers 404, so a row reaches its route only when the answer is 405 and
//! `Allow` names the row's method. Route layers run before that 405, so a task row counts as
//! reached on any other answer than the fallback 404 or a 405 whose `Allow` lacks its method (a
//! wired route whose layer answers 4xx still fails its row). A `yes` row must reach its route and
//! a task row must not, so a route never merged into `build_router` fails here naming its row, and
//! the task that wires a route flips its own row. A malformed row, a duplicate row or an unknown task ID fails with the
//! row quoted, and a header other than `| Method | Path | Module | Wired |` or a missing separator
//! fails with that line quoted. Fake values only.

use std::collections::HashSet;
use std::path::Path;

use askhava_core::api::{Settings, TestControl};
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

/// The architecture file, whose section 13 is the route table.
const ARCHITECTURE: &str = include_str!("../../../docs/architecture.md");
/// The heading that opens the route table's section.
const SECTION: &str = "## 13. Routes";
/// The methods a row may name.
const METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];
/// The fake host every probe names, so a same-origin request passes the cross-site refusal.
const HOST: &str = "askhava.example";

/// What a row says about its route.
#[derive(Debug, PartialEq, Eq)]
enum Wired {
    /// `build_router` serves it on `main` today.
    Yes,
    /// The queued task that will wire it.
    Task(String),
}

/// One checked row of the table.
#[derive(Debug)]
struct Row {
    line: String,
    method: Method,
    path: String,
    wired: Wired,
}

/// The text of section 13, up to the next `## ` heading or the end of the file.
fn section(doc: &str) -> &str {
    let start = doc
        .find(&format!("\n{SECTION}\n"))
        .expect("docs/architecture.md has section 13, Routes");
    let body = &doc[start + SECTION.len() + 2..];
    body.find("\n## ").map_or(body, |end| &body[..end])
}

/// The task graph, read at run time: `kit/tasks.json` is no Rust path rule's file, so it is not
/// compiled in (D-2271).
fn tasks_json() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../kit/tasks.json");
    std::fs::read_to_string(path).expect("kit/tasks.json is readable")
}

/// Every task ID in `kit/tasks.json`.
fn task_ids(json: &str) -> HashSet<String> {
    let graph: serde_json::Value = serde_json::from_str(json).expect("kit/tasks.json parses");
    graph["tasks"]
        .as_array()
        .expect("kit/tasks.json has a tasks list")
        .iter()
        .filter_map(|task| task["id"].as_str().map(str::to_owned))
        .collect()
}

/// A path the router could hold: a `/`, then segments with no space, no empty segment.
fn path_ok(path: &str) -> bool {
    path == "/"
        || (path.starts_with('/')
            && !path.ends_with('/')
            && path[1..]
                .split('/')
                .all(|seg| !seg.is_empty() && !seg.chars().any(|c| c.is_whitespace() || c == '|')))
}

/// A task ID: `T` and three or more digits.
fn task_shaped(cell: &str) -> bool {
    cell.len() >= 4 && cell.starts_with('T') && cell[1..].chars().all(|c| c.is_ascii_digit())
}

/// One body line of the table checked against the row rules (the header and separator are
/// checked by `body_lines`).
fn parse_line(line: &str, tasks: &HashSet<String>) -> Result<Row, String> {
    let bad = |why: &str| Err(format!("{why}: {line:?}"));
    let Some(inner) = line
        .trim()
        .strip_prefix('|')
        .and_then(|l| l.strip_suffix('|'))
    else {
        return bad("malformed row (not opened and closed by |)");
    };
    let cells: Vec<&str> = inner.split('|').map(str::trim).collect();
    let [method, path, module, wired] = cells[..] else {
        return bad("malformed row (not 4 cells)");
    };
    let module_ok =
        !module.is_empty() && module.chars().all(|c| c.is_ascii_lowercase() || c == '_');
    let wired = match wired {
        "yes" => Wired::Yes,
        id if task_shaped(id) && tasks.contains(id) => Wired::Task(id.to_owned()),
        id if task_shaped(id) => {
            return bad(&format!("unknown task id {id} (not in kit/tasks.json)"));
        }
        _ => return bad("malformed row (Wired is yes or a task id)"),
    };
    if !METHODS.contains(&method) || !path_ok(path) || !module_ok {
        return bad("malformed row (method, path or module)");
    }
    let method = Method::from_bytes(method.as_bytes()).expect("a listed method");
    Ok(Row {
        line: line.to_owned(),
        method,
        path: path.to_owned(),
        wired,
    })
}

/// Whether `line` is the table's separator: `|`-bounded cells, each non-empty `-` and `:` only.
fn separator(line: &str) -> bool {
    let inner = line
        .trim()
        .strip_prefix('|')
        .and_then(|l| l.strip_suffix('|'));
    inner.is_some_and(|inner| {
        inner
            .split('|')
            .map(str::trim)
            .all(|c| !c.is_empty() && c.chars().all(|ch| "-:".contains(ch)))
    })
}

/// The table's header line, exactly.
const HEADER: &str = "| Method | Path | Module | Wired |";

/// The table's lines after its header and separator, or the problem with the line quoted: the
/// table opens at the first line opening with `|`, which must be `HEADER`, then a separator.
fn body_lines(table: &str) -> Result<std::str::Lines<'_>, String> {
    let start = match table.find("\n|") {
        _ if table.starts_with('|') => 0,
        found => found.map_or(table.len(), |at| at + 1),
    };
    let mut lines = table[start..].trim_end().lines();
    let header = lines.next().unwrap_or_default();
    if header.trim() != HEADER {
        return Err(format!("table header is not {HEADER:?}: {header:?}"));
    }
    let sep = lines.next().unwrap_or_default();
    if !separator(sep) {
        return Err(format!("table separator missing after the header: {sep:?}"));
    }
    Ok(lines)
}

/// Every row of the table, or every problem with the row quoted (a duplicate method and path too).
/// The table runs from the first line opening with `|` to the end of the section: its first line
/// is `HEADER`, then one separator (each checked, quoted when wrong); every later line, blank or
/// not, is a row.
fn parse_table(table: &str, tasks: &HashSet<String>) -> Result<Vec<Row>, Vec<String>> {
    let (mut rows, mut problems) = (Vec::new(), Vec::new());
    let mut seen = HashSet::new();
    for line in body_lines(table).map_err(|problem| vec![problem])? {
        match parse_line(line, tasks) {
            Err(problem) => problems.push(problem),
            Ok(row) if !seen.insert((row.method.clone(), row.path.clone())) => {
                problems.push(format!("duplicate row: {:?}", row.line));
            }
            Ok(row) => rows.push(row),
        }
    }
    if problems.is_empty() {
        Ok(rows)
    } else {
        Err(problems)
    }
}

/// A fake vault key pair value (`<version>:<base64 of 32 bytes>`).
fn vault_key(byte: u8) -> String {
    format!("1:{}", STANDARD.encode([byte; 32]))
}

/// Dev settings with every optional part configured: Twilio (text, voice, Verify, the masked
/// pool), the public base URL, the vault keys, `ID_SECRET` and the Stripe webhook. Fake values.
fn pairs() -> Vec<(String, String)> {
    let fixed = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_PUBLIC_BASE_URL", "https://askhava.example"),
        (
            "ASKHAVA_ID_SECRET",
            "test-only-id-secret-not-a-real-key-0001",
        ),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake_twilio_auth_token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
        ("ASKHAVA_STRIPE_WEBHOOK_SECRET", "whsec_fake_value_0001"),
        ("ASKHAVA_STRIPE_SECRET_KEY", "sk_test_fake_value_0001"),
        ("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_testfake"),
    ];
    let keys = [
        ("VAULT_KEY", vault_key(0x41)),
        ("VAULT_INDEX_KEY", vault_key(0x42)),
    ];
    fixed
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .chain(keys.into_iter().map(|(k, v)| (k.to_owned(), v)))
        .collect()
}

/// The app's router as `main` builds it, every optional part `Some` (asserted), over a lazy pool.
fn router() -> Router {
    let pairs = pairs();
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://fake@127.0.0.1:1/none")
        .expect("a lazy pool");
    let control = TestControl::from_settings(&settings, pairs.clone()).expect("test control");
    let webhooks = askhava_server::webhooks(&settings, pool.clone(), pairs.clone()).expect("hooks");
    let voice = askhava_server::voice_hooks(&settings, webhooks.as_ref(), pairs.clone())
        .expect("voice hooks");
    let pick_keys = askhava_server::pick_code_keys(&settings, pairs.clone()).expect("pick keys");
    let stripe = askhava_server::release_sink::stripe_webhook(&settings, pool.clone(), pairs)
        .expect("stripe hooks");
    let all = webhooks.is_some() && voice.is_some() && pick_keys.is_some() && stripe.is_some();
    assert!(all, "every optional part of build_router is configured");
    askhava_server::build_router(pool, control.router(), (webhooks, voice, pick_keys, stripe))
}

/// `path` with each `{name}` segment filled with a fake value.
fn concrete(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if seg.starts_with('{') {
                "probe-0001"
            } else {
                seg
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// What a same-origin `PROBE` request on `path` says about `method` there.
#[derive(Debug, PartialEq, Eq)]
enum Probe {
    /// 405 with `Allow` naming `method`: the route is served.
    Served,
    /// The fallback 404, or a 405 whose `Allow` lacks `method`: not served for `method`.
    Absent,
    /// Any other answer, such as a route layer's 4xx: the path is matched, the method unproven.
    Other,
}

/// Probes `method` on `path` with a same-origin `PROBE` request. Route layers run before the 405,
/// so a layer that answers first shows as `Other`; the fallback answers 404.
async fn probe(app: &Router, method: &Method, path: &str) -> Probe {
    let request = Request::builder()
        .method(Method::from_bytes(b"PROBE").expect("an extension method"))
        .uri(concrete(path))
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .body(Body::empty())
        .expect("a probe request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    let allow = response
        .headers()
        .get(header::ALLOW)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let named = allow.split(',').any(|m| m.trim() == method.as_str());
    match response.status() {
        StatusCode::METHOD_NOT_ALLOWED if named => Probe::Served,
        StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_FOUND => Probe::Absent,
        _ => Probe::Other,
    }
}

/// TD-SEC-41: section 13 parses (no malformed or duplicate row, no unknown task ID), every `yes`
/// row reaches a route of the assembled router and every row naming a queued task does not; each
/// failure names its row.
#[tokio::test]
async fn td_sec_41_yes_rows_are_served_and_task_rows_are_not() {
    let rows = parse_table(section(ARCHITECTURE), &task_ids(&tasks_json()))
        .unwrap_or_else(|problems| panic!("section 13 rows:\n{}", problems.join("\n")));
    assert!(
        rows.iter().any(|r| r.wired == Wired::Yes),
        "no row is marked yes"
    );
    let app = router();
    let mut problems = Vec::new();
    for row in &rows {
        let found = probe(&app, &row.method, &row.path).await;
        match (&row.wired, found) {
            (Wired::Yes, Probe::Absent | Probe::Other) => {
                problems.push(format!("marked yes, not served: {:?}", row.line));
            }
            (Wired::Task(id), Probe::Served | Probe::Other) => problems.push(format!(
                "served, still names {id} (mark it yes): {:?}",
                row.line
            )),
            _ => {}
        }
    }
    assert!(
        problems.is_empty(),
        "route wiring:\n{}",
        problems.join("\n")
    );
}

/// TD-SEC-41: the probe tells a matched route from the fallback: an unknown path, and a known
/// path under a method it does not take, are not reached.
#[tokio::test]
async fn td_sec_41_the_probe_tells_a_route_from_the_fallback() {
    let app = router();
    assert_eq!(probe(&app, &Method::GET, "/q/{token}").await, Probe::Served);
    assert_eq!(
        probe(&app, &Method::GET, "/no-such-route").await,
        Probe::Absent
    );
    assert_eq!(
        probe(&app, &Method::GET, "/webhooks/stripe").await,
        Probe::Absent
    );
}

/// The header and separator a test table opens with.
const HEAD: &str = "| Method | Path | Module | Wired |\n|---|---|---|---|\n";

/// TD-SEC-41: a table whose header is not `| Method | Path | Module | Wired |`, or whose header is
/// not followed by a separator, fails quoting the line, and its first row is not skipped as a header.
#[test]
fn td_sec_41_a_bad_header_or_separator_fails_with_the_line_quoted() {
    let tasks = HashSet::new();
    let row = "| GET | /f/{token} | portals | yes |";
    let cases = [
        (
            "| Method | Path | Wired | Module |\n|---|---|---|---|\n",
            "| Method | Path | Wired | Module |",
        ),
        (
            "| Method | Path | Module |\n|---|---|---|\n",
            "| Method | Path | Module |",
        ),
        ("", row),
        ("| Method | Path | Module | Wired |\n", row),
    ];
    for (head, quoted) in cases {
        let problems = parse_table(&format!("Text.\n\n{head}{row}\n"), &tasks).expect_err(head);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains(&format!("{quoted:?}")), "{problems:?}");
    }
    let rows = parse_table(&format!("Text.\n\n{HEAD}{row}\n"), &tasks).expect("a good table");
    assert_eq!(rows.len(), 1);
}

/// TD-SEC-41: a malformed row (an empty-cell row, a second header, a line not opened by `|`, a
/// blank line inside the table), a duplicate row and an unknown task ID fail with the row quoted.
#[test]
fn td_sec_41_a_bad_row_fails_with_the_row_quoted() {
    let tasks: HashSet<String> = ["T261".to_owned()].into_iter().collect();
    let cases = [
        "| GET | /q/{token} | portals |",
        "| FETCH | /q/{token} | portals | yes |",
        "| GET | q/{token} | portals | yes |",
        "| GET | /q/{token} | Portals | yes |",
        "| GET | /q/{token} | portals | maybe |",
        "| GET | /b/picks/{pick}/confirm | portals | T9999 |",
        "| GET | /q/{token} | portals | yes",
        "| | | | |",
        "| Method | Path | Module | Wired |",
        "GET /q/{token} portals yes",
        "",
    ];
    for case in cases {
        let row = "| GET | /f/{token} | portals | yes |";
        let table = format!("Text.\n\n{HEAD}{case}\n{row}\n");
        let problems = parse_table(&table, &tasks).expect_err(case);
        assert!(
            problems.iter().any(|p| p.contains(&format!("{case:?}"))),
            "{problems:?}"
        );
    }
    let unknown = format!("{HEAD}| GET | /b | portals | T9999 |");
    let unknown = parse_table(&unknown, &tasks).expect_err("unknown");
    assert!(unknown[0].contains("unknown task id T9999"), "{unknown:?}");
    let row = "| GET | /b/picks/{pick}/confirm | portals | T261 |";
    let problems = parse_table(&format!("{HEAD}{row}\n{row}\n"), &tasks).expect_err("duplicate");
    assert_eq!(problems, vec![format!("duplicate row: {row:?}")]);
}
