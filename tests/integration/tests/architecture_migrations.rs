//! Module-boundary architecture test, part 2: migrations and SQL (SEC-029; TD-SEC-10, D-491, D-492).
//!
//! Fails on a migration whose name is not `0NNN_<slug>.sql` or whose number is used twice (numbers
//! rise, gaps allowed); whose first line (second after `-- no-transaction`) does not read
//! `-- <module> (T###)`, `<module>` being the schema the file touches (D-492); whose owner is not in
//! `kit/tasks.json`; or whose higher number belongs to an ancestor of a lower-numbered file's task
//! (two tasks that may run in either order never fail, D-491). Fails on a `<module>.`, `SCHEMA x` or
//! `search_path` naming another module's schema, with a separate message for `REFERENCES` (`core`
//! may name `public`), in a migration or in a query macro's SQL under `app/`; and on SQL outside the
//! query macros in `app/` (the function forms, `AssertSqlSafe`, `QueryBuilder`, `Executor`,
//! `query_unchecked!`, a glob or alias of `sqlx`), found by a lexer scan (D-600).
//!
//! SQL is lexed with comments and `'…'` strings removed; unquoted names fold to lower case, quoted
//! names keep their case, and dollar-quoted bodies are scanned as SQL so a function body cannot hide
//! a reference. Each rule is proved by a planted violation under `CARGO_TARGET_TMPDIR` that gives
//! exactly one finding. The manifest checks are `architecture.rs` (T175).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// The `sqlx` function forms that take SQL as a runtime value; as macros (`name!`) they are allowed.
#[rustfmt::skip]
const FN_FORMS: [&str; 6] = ["query", "query_as", "query_scalar", "query_with", "query_as_with", "query_scalar_with"];

/// Names that build or pass SQL outside the literal macros wherever they appear (SEC-029, D-491).
const ALWAYS_FORBIDDEN: [&str; 3] = ["AssertSqlSafe", "QueryBuilder", "raw_sql"];

/// The query macros whose SQL is scanned for other modules' schemas; `query_file*` read the file.
#[rustfmt::skip]
const QUERY_MACROS: [&str; 6] = ["query", "query_as", "query_scalar", "query_file", "query_file_as", "query_file_scalar"];

/// The module schemas (`kit/layers.json`) and the task graph (`kit/tasks.json`).
struct Plan {
    schemas: BTreeSet<String>,
    deps: BTreeMap<String, Vec<String>>,
}

/// One migration file: name, number, header (schema, task) and SQL tokens.
struct Migration {
    name: String,
    number: Option<u32>,
    header: Option<(String, String)>,
    tokens: Vec<String>,
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&read(path)).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn strs(v: &serde_json::Value) -> Vec<String> {
    let list = v.as_array().map(Vec::as_slice).unwrap_or_default();
    list.iter()
        .filter_map(|s| s.as_str().map(String::from))
        .collect()
}

fn load_plan(root: &Path) -> Plan {
    let layers = json(&root.join("kit/layers.json"));
    let tasks = json(&root.join("kit/tasks.json"));
    let layer_list = layers["layers"].as_array().expect("layers is a list");
    let task_list = tasks["tasks"].as_array().expect("tasks is a list");
    Plan {
        schemas: layer_list.iter().flat_map(strs).collect(),
        deps: task_list
            .iter()
            .filter_map(|t| Some((t["id"].as_str()?.to_string(), strs(&t["deps"]))))
            .collect(),
    }
}

/// Every task `id` depends on, directly or through others.
fn ancestors(plan: &Plan, id: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut todo = vec![id.to_string()];
    while let Some(next) = todo.pop() {
        for dep in plan.deps.get(&next).into_iter().flatten() {
            if seen.insert(dep.clone()) {
                todo.push(dep.clone());
            }
        }
    }
    seen
}

fn skip_line(c: &[char], i: usize) -> usize {
    c[i..]
        .iter()
        .position(|&ch| ch == '\n')
        .map_or(c.len(), |p| i + p)
}

/// Skips a nested `/* … */` comment starting at `i` (Rust and PostgreSQL both nest them).
fn skip_block(c: &[char], mut i: usize) -> usize {
    let mut depth = 0;
    while i < c.len() {
        let pair = (c[i], c.get(i + 1).copied().unwrap_or(' '));
        depth += i32::from(pair == ('/', '*')) - i32::from(pair == ('*', '/'));
        i += if matches!(pair, ('/', '*') | ('*', '/')) {
            2
        } else {
            1
        };
        if depth == 0 {
            return i;
        }
    }
    i
}

/// Reads a body after its opening `close` char; `\` escapes when `backslash`, a doubled `close`
/// stands for itself. Returns the body and the index after the closing char.
fn delimited(c: &[char], mut i: usize, close: char, backslash: bool) -> (String, usize) {
    let mut body = String::new();
    while i < c.len() {
        if backslash && c[i] == '\\' {
            body.extend(c.get(i + 1));
            i += 2;
        } else if c[i] == close && c.get(i + 1) == Some(&close) && !backslash {
            body.push(close);
            i += 2;
        } else if c[i] == close {
            return (body, i + 1);
        } else {
            body.push(c[i]);
            i += 1;
        }
    }
    (body, i)
}

fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn word_end(c: &[char], i: usize) -> usize {
    c[i..]
        .iter()
        .position(|&ch| !(is_word(ch) || ch == '$'))
        .map_or(c.len(), |p| i + p)
}

/// A `$tag$` dollar-quote delimiter at `i`: its length, so the body after it is lexed as SQL.
fn dollar_tag(c: &[char], i: usize) -> Option<usize> {
    let rest = &c[i + 1..];
    let end = rest.iter().position(|&ch| !is_word(ch))?;
    let starts_ok = rest.first().is_none_or(|ch| !ch.is_ascii_digit());
    (rest[end] == '$' && starts_ok).then_some(end + 2)
}

fn sql_step(c: &[char], i: usize, out: &mut Vec<String>) -> usize {
    let next = c.get(i + 1).copied().unwrap_or(' ');
    match (c[i], next) {
        ('-', '-') => skip_line(c, i),
        ('/', '*') => skip_block(c, i),
        ('\'', _) => {
            let (body, end) = delimited(c, i + 1, '\'', false);
            out.push(format!("'{body}"));
            end
        }
        ('"', _) => {
            let (body, end) = delimited(c, i + 1, '"', false);
            out.push(body);
            end
        }
        ('$', _) => i + dollar_tag(c, i).unwrap_or(1),
        ('e' | 'E', '\'') => {
            let (body, end) = delimited(c, i + 2, '\'', true);
            out.push(format!("'{body}"));
            end
        }
        (ch, _) if is_word(ch) => {
            let end = word_end(c, i);
            out.push(c[i..end].iter().collect::<String>().to_lowercase());
            end
        }
        (ch, _) => {
            if !ch.is_whitespace() {
                out.push(ch.to_string());
            }
            i + 1
        }
    }
}

/// SQL as tokens: comments gone, unquoted names lower-cased, quoted names as written, string
/// literals kept as one token starting with `'` so no rule reads their words (D-600).
fn sql_tokens(sql: &str) -> Vec<String> {
    let c: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        i = sql_step(&c, i, &mut out);
    }
    out
}

/// A schema `own` may not name: another module's, or `public` unless `own` is `core`.
fn foreign(plan: &Plan, own: &str, name: &str) -> bool {
    let schema = plan.schemas.contains(name) || name == "public";
    schema && name != own && !(own == "core" && name == "public")
}

/// The names listed after `SCHEMA` or `search_path` (a quoted value split on commas).
fn listed_after(t: &[String], mut k: usize) -> Vec<String> {
    let skip = ["if", "not", "exists", "to", "="];
    while t.get(k).is_some_and(|s| skip.contains(&s.as_str())) {
        k += 1;
    }
    let mut out = Vec::new();
    while let Some(name) = t.get(k) {
        let names = name.strip_prefix('\'').unwrap_or(name).split(',');
        out.extend(names.map(|n| n.trim().to_string()));
        if t.get(k + 1).map(String::as_str) != Some(",") {
            break;
        }
        k += 2;
    }
    out
}

/// Every reference in `t` to another module's schema (Arch 4 rule 3, SEC-029, D-491).
fn schema_refs(plan: &Plan, own: &str, at: &str, t: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for (k, s) in t.iter().enumerate() {
        let dotted = t.get(k + 1).is_some_and(|n| n == ".") && foreign(plan, own, s);
        if dotted && k > 0 && t[k - 1] == "references" {
            out.push(format!("{at}: cross-schema foreign key `REFERENCES {s}.`"));
        } else if dotted {
            out.push(format!("{at}: `{s}.` names another module's schema"));
        }
        let listed = if s == "schema" || s == "search_path" {
            listed_after(t, k + 1)
        } else {
            Vec::new()
        };
        for name in listed.iter().filter(|n| foreign(plan, own, n)) {
            out.push(format!("{at}: `{s} {name}` names another module's schema"));
        }
    }
    out
}

/// `0NNN_<slug>.sql`: four digits, then a lower-case slug.
fn number(name: &str) -> Option<u32> {
    let (digits, slug) = name.strip_suffix(".sql")?.split_once('_')?;
    let slug_ok = !slug.is_empty()
        && slug
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_');
    let digits_ok = digits.len() == 4 && digits.chars().all(|ch| ch.is_ascii_digit());
    (slug_ok && digits_ok).then(|| digits.parse().ok())?
}

/// `-- <module> (T###)` on line 1, or line 2 after `-- no-transaction` (D-491, D-492).
fn header(plan: &Plan, text: &str) -> Option<(String, String)> {
    let mut lines = text.lines();
    let mut line = lines.next()?;
    if line.trim_end() == "-- no-transaction" {
        line = lines.next()?;
    }
    let (module, rest) = line.strip_prefix("-- ")?.split_once(" (")?;
    let task = rest.get(..4)?;
    let task_ok = task.starts_with('T') && task[1..].chars().all(|ch| ch.is_ascii_digit());
    let closed = rest[4..].starts_with(')');
    (task_ok && closed && plan.schemas.contains(module))
        .then(|| (module.to_string(), task.to_string()))
}

fn load_migrations(root: &Path, plan: &Plan) -> Vec<Migration> {
    let Ok(entries) = fs::read_dir(root.join("migrations")) else {
        return Vec::new();
    };
    let mut found: Vec<Migration> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let text = read(&e.path());
            Migration {
                number: number(&name),
                header: header(plan, &text),
                tokens: sql_tokens(&text),
                name,
            }
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// Names, numbers, headers and owners of every migration (SEC-029, D-491).
fn check_files(plan: &Plan, files: &[Migration], out: &mut Vec<String>) {
    let mut used: BTreeMap<u32, usize> = BTreeMap::new();
    for m in files {
        let at = format!("migrations/{}", m.name);
        match m.number {
            Some(n) => *used.entry(n).or_default() += 1,
            None => out.push(format!("{at}: not named 0NNN_<slug>.sql")),
        }
        match &m.header {
            None => out.push(format!(
                "{at}: first line (second after `-- no-transaction`) must read `-- <module> (T###)` naming a schema in kit/layers.json"
            )),
            Some((_, task)) if !plan.deps.contains_key(task) => {
                out.push(format!("{at}: owner `{task}` is not in kit/tasks.json"));
            }
            Some(_) => {}
        }
    }
    for (n, count) in used.into_iter().filter(|&(_, count)| count > 1) {
        out.push(format!(
            "migrations: number {n:04} is used by {count} files"
        ));
    }
}

/// A higher number never belongs to an ancestor of a lower-numbered file's task, since a fresh
/// database runs files in number order (D-491); tasks that may run in either order never fail.
fn check_order(plan: &Plan, files: &[Migration], out: &mut Vec<String>) {
    let mut owned: Vec<(u32, &str, &str)> = files
        .iter()
        .filter_map(|m| Some((m.number?, m.header.as_ref()?.1.as_str(), m.name.as_str())))
        .filter(|(_, task, _)| plan.deps.contains_key(*task))
        .collect();
    owned.sort_unstable();
    for (i, (low, low_task, low_name)) in owned.iter().enumerate() {
        let before = ancestors(plan, low_task);
        for (high, high_task, high_name) in &owned[i + 1..] {
            if high > low && before.contains(*high_task) {
                out.push(format!(
                    "migrations/{high_name}: {high_task} is an ancestor of {low_task} but its number is higher than migrations/{low_name}"
                ));
            }
        }
    }
}

fn check_migrations(plan: &Plan, root: &Path, out: &mut Vec<String>) {
    let files = load_migrations(root, plan);
    check_files(plan, &files, out);
    check_order(plan, &files, out);
    for m in &files {
        if let Some((own, _)) = &m.header {
            let at = format!("migrations/{}", m.name);
            out.extend(schema_refs(plan, own, &at, &m.tokens));
        }
    }
}

fn skip_quote(c: &[char], i: usize) -> usize {
    if c.get(i + 1) == Some(&'\\') {
        let rest = c.get(i + 3..).unwrap_or_default();
        return rest
            .iter()
            .position(|&ch| ch == '\'')
            .map_or(c.len(), |p| i + 4 + p);
    }
    if c.get(i + 2) == Some(&'\'') {
        i + 3
    } else {
        i + 1
    }
}

/// A raw string body after `r###"`; returns the body and the index after the close.
fn raw_body(c: &[char], start: usize, hashes: usize) -> (String, usize) {
    let mut i = start;
    while i < c.len() {
        if c[i] == '"' && (1..=hashes).all(|k| c.get(i + k) == Some(&'#')) {
            return (c[start..i].iter().collect(), i + 1 + hashes);
        }
        i += 1;
    }
    (c[start..].iter().collect(), i)
}

fn rust_word(c: &[char], i: usize, out: &mut Vec<String>) -> usize {
    let end = c[i..]
        .iter()
        .position(|&ch| !is_word(ch))
        .map_or(c.len(), |p| i + p);
    let w: String = c[i..end].iter().collect();
    let hashes = c[end..].iter().take_while(|&&ch| ch == '#').count();
    let raw = matches!(w.as_str(), "r" | "br") && c.get(end + hashes) == Some(&'"');
    if raw {
        let (body, next) = raw_body(c, end + hashes + 1, hashes);
        out.push(format!("\"{body}"));
        return next;
    }
    match (w.as_str(), c.get(end).copied().unwrap_or(' ')) {
        ("b", '\'') => skip_quote(c, end),
        ("r", '#') => rust_word(c, end + 1, out),
        _ => {
            out.push(w);
            end
        }
    }
}

fn rust_step(c: &[char], i: usize, out: &mut Vec<String>) -> usize {
    match (c[i], c.get(i + 1).copied().unwrap_or(' ')) {
        ('/', '/') => skip_line(c, i),
        ('/', '*') => skip_block(c, i),
        ('"', _) => {
            let (body, end) = delimited(c, i + 1, '"', true);
            out.push(format!("\"{body}"));
            end
        }
        ('\'', _) => skip_quote(c, i),
        (ch, _) if is_word(ch) => rust_word(c, i, out),
        (ch, _) => {
            if !ch.is_whitespace() {
                out.push(ch.to_string());
            }
            i + 1
        }
    }
}

/// Rust source as tokens: comments gone, each string literal one token starting with `"`.
fn rust_tokens(src: &str) -> Vec<String> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        i = rust_step(&c, i, &mut out);
    }
    out
}

/// Token indices of the names in the path after the `sqlx` at `at`, `use` trees included.
fn sqlx_path(t: &[String], at: usize) -> Vec<usize> {
    let mut depth = 0;
    let mut found = Vec::new();
    for (k, s) in t.iter().enumerate().skip(at + 1) {
        match s.as_str() {
            "{" => depth += 1,
            "}" | "," if depth == 0 => break,
            "}" => depth -= 1,
            ":" | "," => {}
            w if w == "*" || w.starts_with(|ch: char| ch.is_alphabetic() || ch == '_') => {
                found.push(k);
            }
            _ => break,
        }
    }
    found
}

/// Whether the name at `k` reached through `sqlx::` runs SQL outside the macros.
fn forbidden_in_path(t: &[String], k: usize) -> bool {
    let name = t[k].as_str();
    let macro_call = t.get(k + 1).is_some_and(|s| s == "!");
    (FN_FORMS.contains(&name) && !macro_call) || name.contains("Executor") || name == "*"
}

fn forbidden_anywhere(name: &str) -> bool {
    ALWAYS_FORBIDDEN.contains(&name) || (name.starts_with("query") && name.ends_with("_unchecked"))
}

/// No SQL outside the literal query macros: the lexer finds the forms clippy cannot (SEC-029, D-491).
fn check_sql_forms(rel: &str, t: &[String], out: &mut Vec<String>) {
    let mut hits: BTreeSet<usize> = (0..t.len())
        .filter(|&k| forbidden_anywhere(&t[k]))
        .collect();
    for at in (0..t.len()).filter(|&k| t[k] == "sqlx") {
        if t.get(at + 1).is_some_and(|s| s == "as") {
            out.push(format!(
                "{rel}: `sqlx` renamed with `as` hides the SQL forms"
            ));
        }
        hits.extend(
            sqlx_path(t, at)
                .into_iter()
                .filter(|&k| forbidden_in_path(t, k)),
        );
    }
    for k in hits {
        out.push(format!("{rel}: SQL outside the query macros: `{}`", t[k]));
    }
}

/// The SQL of each query macro names only the crate's own schema (Arch 4 rule 3, SEC-029).
fn check_query_schemas(plan: &Plan, dir: &Path, rel: &str, t: &[String], out: &mut Vec<String>) {
    let module = dir.file_name().unwrap_or_default().to_string_lossy();
    for k in 0..t.len().saturating_sub(2) {
        if !(QUERY_MACROS.contains(&t[k].as_str()) && t[k + 1] == "!" && t[k + 2] == "(") {
            continue;
        }
        let Some(body) = t[k + 3..].iter().find_map(|s| s.strip_prefix('"')) else {
            continue;
        };
        let (at, sql) = if t[k].starts_with("query_file") {
            let path = dir.join(body);
            let at = format!("{}/{body}", rel.split("/src/").next().unwrap_or(rel));
            (at, fs::read_to_string(path).unwrap_or_default())
        } else {
            (rel.to_string(), body.to_string())
        };
        out.extend(schema_refs(plan, &module, &at, &sql_tokens(&sql)));
    }
}

fn rs_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for path in entries.filter_map(Result::ok).map(|e| e.path()) {
        if path.is_dir() && path.file_name().is_some_and(|n| n != "target") {
            rs_files(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

fn check_app(plan: &Plan, root: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(root.join("app")) else {
        return;
    };
    let mut crates: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    crates.sort();
    for dir in crates.iter().filter(|d| d.is_dir()) {
        let mut files = Vec::new();
        rs_files(dir, &mut files);
        files.sort();
        for path in files {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
            let t = rust_tokens(&read(&path));
            check_sql_forms(&rel, &t, out);
            check_query_schemas(plan, dir, &rel, &t, out);
        }
    }
}

/// Every migration and SQL finding for the repository at `root` (SEC-029, D-491).
fn check_repo(root: &Path) -> Vec<String> {
    let plan = load_plan(root);
    let mut out = Vec::new();
    check_migrations(&plan, root, &mut out);
    check_app(&plan, root, &mut out);
    out
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

/// TD-SEC-10: the real migrations and `app/` sources have no finding.
#[test]
fn td_sec_10_migrations_and_sql_obey_the_rules() {
    let root = repo_root();
    assert!(load_plan(&root).deps.contains_key("T226"));
    assert_eq!(check_repo(&root), Vec::<String>::new());
}

const TASKS: &str = r#"{"tasks": [{"id": "T001", "deps": []}, {"id": "T002", "deps": ["T001"]}, {"id": "T003", "deps": []}]}"#;
const NEXT: &str = "migrations/0005_jobs_next.sql";
const JOBS_SRC: &str = "app/jobs/src/lib.rs";

/// The clean scratch tree. `core` names `public`; T003 (`picks`) may run in either order with the
/// others; the words in comments, strings and a quoted column must not count; a dollar-quoted body
/// names only its own schema; the macros name only `jobs` (D-491, D-492).
#[rustfmt::skip]
const BASE: [(&str, &str); 6] = [
    ("kit/tasks.json", TASKS),
    ("migrations/0001_core_base.sql", "-- core (T001): lock down public\nREVOKE CREATE ON SCHEMA public FROM PUBLIC;\nCREATE SCHEMA IF NOT EXISTS core;\nCREATE TABLE core.task (id bigint);\n"),
    ("migrations/0002_jobs_job.sql", "-- no-transaction\n-- jobs (T002): jobs\nCREATE SCHEMA jobs;\nGRANT USAGE ON SCHEMA jobs TO askhava_app;\n-- picks.pick in a comment /* public.x */\nCREATE TABLE jobs.job (id bigint, note text DEFAULT 'picks.pick', \"quotes\" text, rate numeric DEFAULT 1.5);\nCREATE FUNCTION jobs.touch() RETURNS trigger AS $fn$ BEGIN RETURN NEW; END $fn$ LANGUAGE plpgsql SET search_path = jobs, pg_temp;\nSELECT E'it\\'s picks.x', $1;\n"),
    ("migrations/0004_picks_pick.sql", "-- picks (T003)\nCREATE TABLE picks.pick (id bigint);\n"),
    (JOBS_SRC, "//! raw_sql and picks.pick in a comment\nfn f(p: &sqlx::PgPool) -> sqlx::Result<()> {\n    let a = sqlx::query!(\"SELECT id FROM jobs.job WHERE note = 'picks.x'\");\n    let b = sqlx::query_as!(Job, r#\"SELECT \"id\" FROM jobs.job -- picks.pick\"#);\n    let c = sqlx::query_file!(\"queries/job.sql\");\n    let s = \"sqlx::query(QueryBuilder)\";\n}\n"),
    ("app/jobs/queries/job.sql", "SELECT id FROM jobs.job\n"),
];

/// Planted violations: case, the file written over the clean tree, its text, and the one finding
/// it must give, a substring of the only finding (TD-SEC-10, SEC-029, D-491).
#[rustfmt::skip]
const CASES: [(&str, &str, &str, &str); 27] = [
    ("bad_name", "migrations/12_jobs.sql", "-- jobs (T002)\n", "migrations/12_jobs.sql: not named 0NNN_<slug>.sql"),
    ("up_down", "migrations/0005_jobs_x.up.sql", "-- jobs (T002)\n", "migrations/0005_jobs_x.up.sql: not named"),
    ("duplicate", "migrations/0002_jobs_more.sql", "-- jobs (T002)\n", "migrations: number 0002 is used by 2 files"),
    ("no_header", NEXT, "CREATE TABLE jobs.next (id bigint);\n", "migrations/0005_jobs_next.sql: first line"),
    ("header_line_three", NEXT, "-- no-transaction\n\n-- jobs (T002)\n", "migrations/0005_jobs_next.sql: first line"),
    ("header_not_schema", NEXT, "-- billing (T002)\n", "migrations/0005_jobs_next.sql: first line"),
    ("unknown_owner", NEXT, "-- jobs (T999)\n", "migrations/0005_jobs_next.sql: owner `T999` is not in kit/tasks.json"),
    ("ancestor_higher", "migrations/0005_core_late.sql", "-- core (T001)\nCREATE TABLE core.late (id bigint);\n", "migrations/0005_core_late.sql: T001 is an ancestor of T002 but its number is higher than migrations/0002_jobs_job.sql"),
    ("dotted", NEXT, "-- jobs (T002)\nSELECT id FROM picks.pick;\n", "migrations/0005_jobs_next.sql: `picks.` names another module's schema"),
    ("upper_case", NEXT, "-- jobs (T002)\nSELECT id FROM PICKS.Pick;\n", "`picks.` names another module's schema"),
    ("quoted_name", NEXT, "-- jobs (T002)\nSELECT id FROM \"picks\".pick;\n", "`picks.` names another module's schema"),
    ("dollar_body", NEXT, "-- jobs (T002)\nCREATE FUNCTION jobs.f() RETURNS bigint AS $$ SELECT count(*) FROM picks.pick $$ LANGUAGE sql;\n", "`picks.` names another module's schema"),
    ("foreign_key", NEXT, "-- jobs (T002)\nALTER TABLE jobs.job ADD COLUMN pick bigint REFERENCES picks.pick (id);\n", "cross-schema foreign key `REFERENCES picks.`"),
    ("schema_word", NEXT, "-- jobs (T002)\nGRANT USAGE ON SCHEMA jobs, picks TO askhava_app;\n", "`schema picks` names another module's schema"),
    ("public_outside_core", NEXT, "-- jobs (T002)\nCREATE TABLE public.next (id bigint);\n", "`public.` names another module's schema"),
    ("search_path", NEXT, "-- jobs (T002)\nSET search_path = 'jobs,picks';\n", "`search_path picks` names another module's schema"),
    ("fn_form", JOBS_SRC, "fn f() { let q = sqlx::query(\"SELECT 1\"); }\n", "app/jobs/src/lib.rs: SQL outside the query macros: `query`"),
    ("use_fn_form", JOBS_SRC, "use sqlx::{query_as, PgPool};\n", "SQL outside the query macros: `query_as`"),
    ("raw_sql", JOBS_SRC, "fn f(s: &str) { let q = raw_sql(s); }\n", "SQL outside the query macros: `raw_sql`"),
    ("assert_sql_safe", JOBS_SRC, "use sqlx::AssertSqlSafe;\n", "SQL outside the query macros: `AssertSqlSafe`"),
    ("query_builder", JOBS_SRC, "fn f() { let b = QueryBuilder::<Postgres>::new(\"SELECT \"); }\n", "SQL outside the query macros: `QueryBuilder`"),
    ("executor", JOBS_SRC, "use sqlx::{Executor as _, PgPool};\n", "SQL outside the query macros: `Executor`"),
    ("unchecked", JOBS_SRC, "fn f() { let r = sqlx::query_unchecked!(\"SELECT 1\"); }\n", "SQL outside the query macros: `query_unchecked`"),
    ("glob", JOBS_SRC, "use sqlx::prelude::*;\n", "SQL outside the query macros: `*`"),
    ("alias", JOBS_SRC, "use sqlx as db;\n", "app/jobs/src/lib.rs: `sqlx` renamed with `as`"),
    ("macro_inline", JOBS_SRC, "fn f() { let r = sqlx::query!(\"SELECT id FROM picks.pick\"); }\n", "app/jobs/src/lib.rs: `picks.` names another module's schema"),
    ("macro_schema", "app/jobs/queries/job.sql", "SELECT id FROM picks.pick\n", "app/jobs/queries/job.sql: `picks.` names another module's schema"),
];

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("a parent folder")).expect("create folder");
    fs::write(&path, content).expect("write planted file");
}

/// A clean scratch tree under `CARGO_TARGET_TMPDIR` with the real `kit/layers.json`.
fn plant(case: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("architecture_migrations")
        .join(case);
    if root.exists() {
        fs::remove_dir_all(&root).expect("clear the old plant");
    }
    write(
        &root,
        "kit/layers.json",
        &read(&repo_root().join("kit/layers.json")),
    );
    for (rel, content) in BASE {
        write(&root, rel, content);
    }
    root
}

/// TD-SEC-10: the clean planted tree has no finding, so each case shows only its own.
#[test]
fn td_sec_10_clean_plant_passes() {
    assert_eq!(check_repo(&plant("clean")), Vec::<String>::new());
}

/// TD-SEC-10 (SEC-029, D-491): each planted violation gives exactly one finding, its own.
#[test]
fn td_sec_10_planted_violations_fail() {
    for (case, rel, content, want) in CASES {
        let root = plant(case);
        write(&root, rel, content);
        let findings = check_repo(&root);
        assert_eq!(findings.len(), 1, "{case}: one finding, got {findings:?}");
        assert!(
            findings[0].contains(want),
            "{case}: no {want:?} in {findings:?}"
        );
    }
}
