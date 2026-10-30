//! Module-boundary architecture test (SEC-029, SEC-010, FEE-032, SEC-018; TD-SEC-10, TD-FEE-16,
//! TD-SEC-03's architecture case).
//!
//! Reads every workspace member's `Cargo.toml` and Rust source and fails on what the compiler alone
//! does not stop: a first-party dependency the layer table does not allow (followed through
//! `package =` renames and direct paths), a dependency without `workspace = true` or missing from
//! `[workspace.dependencies]`, a member that does not inherit `unsafe_code = "forbid"`, a feature
//! or `optional = true` dependency (only `test-control` on `core` is allowed), a package not named `askhava-<module>` (test-layer
//! crates have fixed names; `tests/recovery` is `askhava-recovery-tests`), an `unsafe` keyword, the
//! release and session mints named outside their crates, the payments adapter named outside
//! `payments`, a route input that is not a `deny_unknown_fields` struct, and an adapter trait method
//! that acts outside without an idempotency key (D-453, D-479, D-488, D-491).
//!
//! The layer table is read from `kit/layers.json`, never from `docs/architecture.md`, so the oracle
//! cannot move with the prose (D-479). `server` (the composition root) may depend on every module and
//! `core` on none. The Rust source is scanned as tokens with comments and string bodies removed, so
//! a word in a comment or a string never counts. Each rule is proved by a planted violation in a
//! scratch tree under `CARGO_TARGET_TMPDIR` that must give exactly its own findings. The migration
//! checks and the SQL-form scan are `architecture_migrations.rs` (T226).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

/// The mint names and the crates that may write them; nothing else can mint a second time (SEC-030, D-488).
const MINTS: [(&str, &[&str]); 4] = [
    ("ReleaseMint", &["privacy", "payments", "server"]),
    ("release_mint", &["privacy", "payments", "server"]),
    ("SessionMint", &["privacy", "payments", "server", "portals"]),
    (
        "session_mint",
        &["privacy", "payments", "server", "portals"],
    ),
];

/// Test-layer crates: folder under `tests/` and fixed package name (D-488).
const TEST_CRATES: [(&str, &str); 4] = [
    ("integration", "askhava-integration"),
    ("security", "askhava-security"),
    ("recovery", "askhava-recovery-tests"),
    ("fixtures", "askhava-fixtures"),
];

/// Crates that may name the payments adapter: its home, its one user and the composition root that
/// wires it (FEE-032, D-595).
const PAYMENTS_ADAPTER_USERS: [&str; 3] = ["adapters", "payments", "server"];

/// Adapter method name prefixes that only read and so take no idempotency key; not `verify_`, as
/// the verify adapter sends codes (SEC-018, D-596).
const READ_ONLY_PREFIXES: [&str; 6] = ["fetch_", "get_", "list_", "read_", "check_", "lookup_"];

/// Axum extractors whose type parameter is parsed from the request (SEC-010).
const INPUT_EXTRACTORS: [&str; 3] = ["Form", "Json", "Query"];

/// Dependency sections of a manifest (also read under every `[target.*]`).
const DEP_SECTIONS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// The layer table of `kit/layers.json` (D-479).
struct Layers {
    modules: BTreeSet<String>,
    calls: BTreeMap<String, BTreeSet<String>>,
    always: BTreeSet<String>,
}

/// One workspace member: its folder, module (or test folder) and manifest.
struct Member {
    rel: String,
    dir: PathBuf,
    module: String,
    test_layer: bool,
    manifest: Table,
}

/// The workspace as the checks see it.
struct Workspace {
    root: PathBuf,
    deps: Table,
    listed: BTreeSet<String>,
    lints_forbid: bool,
    layers: Layers,
    members: Vec<Member>,
}

/// One Rust file as tokens, comments and string bodies removed.
struct Source {
    rel: String,
    tokens: Vec<String>,
}

fn read_toml(path: &Path) -> Table {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.parse()
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn table(t: &Table, key: &str) -> Table {
    t.get(key)
        .and_then(Value::as_table)
        .cloned()
        .unwrap_or_default()
}

fn names(v: &serde_json::Value) -> BTreeSet<String> {
    let list = v.as_array().map(Vec::as_slice).unwrap_or_default();
    list.iter()
        .filter_map(|s| s.as_str().map(String::from))
        .collect()
}

fn load_layers(root: &Path) -> Layers {
    let path = root.join("kit/layers.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&text).expect("kit/layers.json parses");
    let layers = json["layers"].as_array().expect("layers is a list");
    let calls = json["calls"].as_object().expect("calls is an object");
    Layers {
        modules: layers.iter().flat_map(names).collect(),
        calls: calls.iter().map(|(k, v)| (k.clone(), names(v))).collect(),
        always: names(&json["always_allowed"]),
    }
}

fn package_name(dir: &Path) -> Option<String> {
    let manifest = read_toml(&dir.join("Cargo.toml"));
    let name = table(&manifest, "package")
        .get("name")?
        .as_str()?
        .to_string();
    Some(name)
}

fn expand(root: &Path, pattern: &str) -> Vec<String> {
    let Some(parent) = pattern.strip_suffix("/*") else {
        return vec![pattern.to_string()];
    };
    let entries = fs::read_dir(root.join(parent)).expect("member folder exists");
    let mut dirs: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().join("Cargo.toml").is_file())
        .map(|e| format!("{parent}/{}", e.file_name().to_string_lossy()))
        .collect();
    dirs.sort();
    dirs
}

fn member(root: &Path, rel: String) -> Member {
    let dir = root.join(&rel);
    let manifest = read_toml(&dir.join("Cargo.toml"));
    let (folder, test_layer) = match rel.strip_prefix("tests/") {
        Some(folder) => (folder, true),
        None => (rel.rsplit('/').next().unwrap_or_default(), false),
    };
    let module = folder.to_string();
    Member {
        rel,
        dir,
        module,
        test_layer,
        manifest,
    }
}

fn listed_names(root: &Path, deps: &Table) -> BTreeSet<String> {
    deps.iter()
        .map(|(key, spec)| resolve_spec(root, key, spec))
        .collect()
}

fn load_workspace(root: &Path) -> Workspace {
    let ws = table(&read_toml(&root.join("Cargo.toml")), "workspace");
    let deps = table(&ws, "dependencies");
    let rust_lints = table(&table(&ws, "lints"), "rust");
    let lints_forbid = rust_lints.get("unsafe_code").and_then(Value::as_str) == Some("forbid");
    let patterns = ws
        .get("members")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let members = patterns
        .iter()
        .filter_map(Value::as_str)
        .flat_map(|p| expand(root, p))
        .map(|rel| member(root, rel))
        .collect();
    let listed = listed_names(root, &deps);
    let layers = load_layers(root);
    Workspace {
        root: root.to_path_buf(),
        deps,
        listed,
        lints_forbid,
        layers,
        members,
    }
}

/// The real package behind a dependency entry: a `path` is read, then `package =`, then the key.
fn resolve_spec(base: &Path, key: &str, spec: &Value) -> String {
    if let Some(path) = spec.get("path").and_then(Value::as_str) {
        return package_name(&base.join(path)).unwrap_or_else(|| key.to_string());
    }
    spec.get("package")
        .and_then(Value::as_str)
        .unwrap_or(key)
        .to_string()
}

/// Follows a member's entry through the workspace table, so a rename at either level is seen (D-488).
fn resolve(ws: &Workspace, m: &Member, key: &str, spec: &Value) -> String {
    let is_workspace = spec.get("workspace").and_then(Value::as_bool) == Some(true);
    if spec.get("path").is_some() || spec.get("package").is_some() || !is_workspace {
        return resolve_spec(&m.dir, key, spec);
    }
    match ws.deps.get(key) {
        Some(entry) => resolve_spec(&ws.root, key, entry),
        None => key.to_string(),
    }
}

/// What a member may depend on: the layer table's `calls` plus `always_allowed`; `server` every
/// module, `core` none, test-layer crates every module, `askhava-fixtures` as a dev-dependency
/// (SEC-029, D-479, D-488).
fn allowed(ws: &Workspace, m: &Member, section: &str, name: &str) -> bool {
    let target = name.trim_start_matches("askhava-");
    if name == "askhava-fixtures" {
        return m.test_layer || section == "dev-dependencies";
    }
    let is_module = ws.layers.modules.contains(target);
    if m.test_layer || m.module == "server" {
        return is_module;
    }
    let calls = ws.layers.calls.get(&m.module);
    (m.module != "core" && ws.layers.always.contains(target))
        || calls.is_some_and(|c| c.contains(target))
}

fn dep_tables(manifest: &Table) -> Vec<(String, Table)> {
    let mut scopes = vec![manifest.clone()];
    scopes.extend(
        table(manifest, "target")
            .values()
            .filter_map(Value::as_table)
            .cloned(),
    );
    let mut found = Vec::new();
    for scope in &scopes {
        for section in DEP_SECTIONS {
            found.push((section.to_string(), table(scope, section)));
        }
    }
    found
}

/// One dependency entry: `workspace = true`, listed, not renamed, allowed by the layer table (SEC-029).
fn check_dep(ws: &Workspace, m: &Member, section: &str, key: &str, spec: &Value) -> Vec<String> {
    let at = &m.rel;
    let mut out = Vec::new();
    if spec.get("workspace").and_then(Value::as_bool) != Some(true) {
        out.push(format!("{at}: dependency `{key}` lacks `workspace = true`"));
    }
    let name = resolve(ws, m, key, spec);
    if !ws.listed.contains(&name) {
        out.push(format!(
            "{at}: dependency `{name}` is not in [workspace.dependencies]"
        ));
    }
    if name != key {
        out.push(format!(
            "{at}: dependency `{key}` is a rename of package `{name}`"
        ));
    }
    if name.starts_with("askhava-") && !allowed(ws, m, section, &name) {
        let module = &m.module;
        out.push(format!(
            "{at}: `{name}` is not a dependency the layer table allows for `{module}`"
        ));
    }
    out
}

fn check_deps(ws: &Workspace, m: &Member, out: &mut Vec<String>) {
    for (section, deps) in dep_tables(&m.manifest) {
        for (key, spec) in &deps {
            out.extend(check_dep(ws, m, &section, key, spec));
        }
    }
}

/// Package names: `askhava-<module>` for a module in the layer table or `server`; fixed names for the
/// test-layer crates (D-484, D-488).
fn check_name(ws: &Workspace, m: &Member, out: &mut Vec<String>) {
    let expected = if m.test_layer {
        TEST_CRATES
            .iter()
            .find(|(dir, _)| *dir == m.module)
            .map(|(_, name)| (*name).to_string())
    } else if m.module == "server" || ws.layers.modules.contains(&m.module) {
        Some(format!("askhava-{}", m.module))
    } else {
        None
    };
    let actual = table(&m.manifest, "package")
        .get("name")
        .and_then(Value::as_str)
        .map(String::from);
    match expected {
        Some(name) if actual.as_ref() == Some(&name) => {}
        Some(name) => out.push(format!("{}: package must be named `{name}`", m.rel)),
        None => out.push(format!(
            "{}: not a module in kit/layers.json or a test-layer crate",
            m.rel
        )),
    }
}

/// Every member inherits the workspace lints, whose `unsafe_code` is `forbid` (SEC-029, D-481).
fn check_lints(ws: &Workspace, m: &Member, out: &mut Vec<String>) {
    let inherits = table(&m.manifest, "lints")
        .get("workspace")
        .and_then(Value::as_bool);
    if !(ws.lints_forbid && inherits == Some(true)) {
        out.push(format!(
            "{}: unsafe_code = \"forbid\" not inherited ([lints] workspace = true)",
            m.rel
        ));
    }
}

/// No feature may widen visibility, nor an `optional` dependency (an implicit feature); `test-control`
/// on `core` is the one exception (SEC-029, D-481).
fn check_features(m: &Member, out: &mut Vec<String>) {
    let core = !m.test_layer && m.module == "core";
    for name in table(&m.manifest, "features").keys() {
        if !core || name != "test-control" {
            out.push(format!("{}: feature `{name}` is not allowed", m.rel));
        }
    }
    for (_, deps) in dep_tables(&m.manifest) {
        for (key, spec) in &deps {
            if !core && spec.get("optional").and_then(Value::as_bool) == Some(true) {
                out.push(format!(
                    "{}: optional dependency `{key}` is a feature",
                    m.rel
                ));
            }
        }
    }
}

fn skip_line(c: &[char], i: usize) -> usize {
    c[i..]
        .iter()
        .position(|&ch| ch == '\n')
        .map_or(c.len(), |p| i + p)
}

fn skip_block(c: &[char], mut i: usize) -> usize {
    let mut depth = 0;
    while i < c.len() {
        let pair = (c[i], c.get(i + 1).copied().unwrap_or(' '));
        match pair {
            ('/', '*') => depth += 1,
            ('*', '/') => depth -= 1,
            _ => {}
        }
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

/// Skips a string body starting after its opening quote; returns the index after the close.
fn skip_string(c: &[char], mut i: usize) -> usize {
    while i < c.len() && c[i] != '"' {
        i += if c[i] == '\\' { 2 } else { 1 };
    }
    i + 1
}

fn skip_raw(c: &[char], mut i: usize, hashes: usize) -> usize {
    while i < c.len() {
        if c[i] == '"' && (1..=hashes).all(|k| c.get(i + k) == Some(&'#')) {
            return i + 1 + hashes;
        }
        i += 1;
    }
    i
}

/// A char literal is skipped; a lifetime keeps its name as a token.
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

fn word(c: &[char], i: usize, out: &mut Vec<String>) -> usize {
    let end = c[i..]
        .iter()
        .position(|ch| !(ch.is_alphanumeric() || *ch == '_'))
        .map_or(c.len(), |p| i + p);
    let w: String = c[i..end].iter().collect();
    let next = c.get(end).copied().unwrap_or(' ');
    let hashes = c[end..].iter().take_while(|&&ch| ch == '#').count();
    match (w.as_str(), next) {
        ("b", '"') => return skip_string(c, end + 1),
        ("b", '\'') => return skip_quote(c, end),
        ("r" | "br", '"' | '#') if c.get(end + hashes) == Some(&'"') => {
            return skip_raw(c, end + hashes + 1, hashes);
        }
        ("r", '#') => return word(c, end + 1, out),
        _ => out.push(w),
    }
    end
}

fn step(c: &[char], i: usize, out: &mut Vec<String>) -> usize {
    let ch = c[i];
    match (ch, c.get(i + 1).copied().unwrap_or(' ')) {
        ('/', '/') => skip_line(c, i),
        ('/', '*') => skip_block(c, i),
        ('"', _) => skip_string(c, i + 1),
        ('\'', _) => skip_quote(c, i),
        _ if ch.is_alphanumeric() || ch == '_' => word(c, i, out),
        _ => {
            if !ch.is_whitespace() {
                out.push(ch.to_string());
            }
            i + 1
        }
    }
}

/// Rust source as identifier and punctuation tokens, with comments and string bodies removed.
fn tokens(src: &str) -> Vec<String> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        i = step(&c, i, &mut out);
    }
    out
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

fn sources(ws: &Workspace, m: &Member) -> Vec<Source> {
    let mut paths = Vec::new();
    rs_files(&m.dir, &mut paths);
    paths.sort();
    let read =
        |p: &PathBuf| fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let rel = |p: &PathBuf| {
        p.strip_prefix(&ws.root)
            .unwrap_or(p)
            .to_string_lossy()
            .into_owned()
    };
    paths
        .iter()
        .map(|p| Source {
            rel: rel(p),
            tokens: tokens(&read(p)),
        })
        .collect()
}

/// Index of the token that closes the bracket at `open`, or the end.
fn matching(t: &[String], open: usize) -> usize {
    let (o, c) = if t[open] == "(" {
        ("(", ")")
    } else {
        ("{", "}")
    };
    let mut depth = 0;
    for (k, s) in t.iter().enumerate().skip(open) {
        depth += i32::from(s == o) - i32::from(s == c);
        if depth == 0 {
            return k;
        }
    }
    t.len()
}

/// The parameter tokens of the `fn` at `at`.
fn params(t: &[String], at: usize) -> &[String] {
    match t[at..].iter().position(|s| s == "(") {
        Some(p) => &t[at + p..matching(t, at + p)],
        None => &[],
    }
}

/// Identifiers and globs of the path after `askhava_adapters` (a `use` tree or a qualified name).
fn path_idents(t: &[String]) -> Vec<&str> {
    let mut depth = 0;
    let mut out = Vec::new();
    for s in t {
        match s.as_str() {
            "{" => depth += 1,
            "}" if depth == 0 => break,
            "}" => depth -= 1,
            "," if depth == 0 => break,
            ":" | "," => {}
            w if w == "*" || w.starts_with(|ch: char| ch.is_alphabetic() || ch == '_') => {
                out.push(w)
            }
            _ => break,
        }
    }
    out
}

/// Structs whose attributes carry `deny_unknown_fields` (SEC-010).
fn strict_structs(t: &[String]) -> Vec<String> {
    let attrs = |i: usize| {
        t[..i]
            .iter()
            .rev()
            .take_while(|s| !matches!(s.as_str(), ";" | "{" | "}"))
            .any(|s| s == "deny_unknown_fields")
    };
    (0..t.len().saturating_sub(1))
        .filter(|&i| t[i] == "struct" && attrs(i))
        .map(|i| t[i + 1].clone())
        .collect()
}

/// Methods of every `trait` in the file, as indices of their `fn` tokens.
fn trait_methods(t: &[String]) -> Vec<usize> {
    let mut found = Vec::new();
    for start in (0..t.len()).filter(|&i| t[i] == "trait") {
        let Some(open) = t[start..].iter().position(|s| s == "{").map(|p| start + p) else {
            continue;
        };
        found.extend((open..matching(t, open)).filter(|&k| t[k] == "fn"));
    }
    found
}

/// `unsafe` is forbidden in every crate; the lint stops it at build time, this names the file (SEC-029).
fn check_unsafe(f: &Source, out: &mut Vec<String>) {
    if f.tokens.iter().any(|s| s == "unsafe") {
        out.push(format!("{}: `unsafe` is forbidden", f.rel));
    }
}

/// A mint named outside its crates could mint a second time (SEC-030, D-488). The compile-fail cases
/// in `tests/security/ui/` must name them, so that folder is the one exception.
fn check_mints(m: &Member, f: &Source, out: &mut Vec<String>) {
    if m.test_layer && m.module == "security" && f.rel.contains("/ui/") {
        return;
    }
    for (mint, homes) in MINTS {
        let home = !m.test_layer && homes.contains(&m.module.as_str());
        if !home && f.tokens.iter().any(|s| s == mint) {
            out.push(format!("{}: `{mint}` named outside {homes:?}", f.rel));
        }
    }
}

/// Only `payments` imports the payments adapter; money moves only through `payments` (FEE-032).
/// Elsewhere an alias (`as`) or a glob (`*`) of an adapters path could hide it, so both fail (D-595).
fn check_payments_adapter(m: &Member, f: &Source, out: &mut Vec<String>) {
    if !m.test_layer && PAYMENTS_ADAPTER_USERS.contains(&m.module.as_str()) {
        return;
    }
    let t = &f.tokens;
    let names_adapter = |w: &&str| {
        let w = w.to_lowercase();
        w == "payments" || w.contains("stripe") || w == "as" || w == "*"
    };
    let hit = (0..t.len())
        .filter(|&i| t[i] == "askhava_adapters")
        .any(|i| path_idents(&t[i + 1..]).iter().any(names_adapter));
    if hit {
        out.push(format!(
            "{}: imports the payments adapter outside `payments`",
            f.rel
        ));
    }
}

/// Every `Form`, `Json` or `Query` input is a struct of this crate with `deny_unknown_fields` (SEC-010).
fn check_route_inputs(f: &Source, strict: &BTreeSet<String>, out: &mut Vec<String>) {
    let t = &f.tokens;
    for at in (0..t.len()).filter(|&i| t[i] == "fn") {
        for w in params(t, at).windows(3) {
            let input = INPUT_EXTRACTORS.contains(&w[0].as_str()) && w[1] == "<";
            if input && !strict.contains(&w[2]) {
                out.push(format!(
                    "{}: route input `{}` is not a deny_unknown_fields struct",
                    f.rel, w[2]
                ));
            }
        }
    }
}

/// Every adapter trait method that acts outside takes an idempotency key (SEC-018, TD-SEC-03, D-491).
fn check_idempotency(f: &Source, out: &mut Vec<String>) {
    let t = &f.tokens;
    for at in trait_methods(t) {
        let name = t.get(at + 1).map_or("", String::as_str);
        let reads = READ_ONLY_PREFIXES.iter().any(|p| name.starts_with(p));
        if !reads
            && !params(t, at)
                .iter()
                .any(|s| s.to_lowercase().contains("idempotency"))
        {
            out.push(format!(
                "{}: adapter method `{name}` takes no idempotency key",
                f.rel
            ));
        }
    }
}

fn check_sources(ws: &Workspace, m: &Member, out: &mut Vec<String>) {
    let files = sources(ws, m);
    let strict: BTreeSet<String> = files
        .iter()
        .flat_map(|f| strict_structs(&f.tokens))
        .collect();
    for f in &files {
        check_unsafe(f, out);
        check_mints(m, f, out);
        check_payments_adapter(m, f, out);
        if !m.test_layer {
            check_route_inputs(f, &strict, out);
        }
        if !m.test_layer && m.module == "adapters" {
            check_idempotency(f, out);
        }
    }
}

/// Every finding for the workspace at `root` (SEC-029, SEC-010, FEE-032, SEC-018).
fn check_workspace(root: &Path) -> Vec<String> {
    let ws = load_workspace(root);
    let mut out = Vec::new();
    for m in &ws.members {
        check_name(&ws, m, &mut out);
        check_lints(&ws, m, &mut out);
        check_features(m, &mut out);
        check_deps(&ws, m, &mut out);
        check_sources(&ws, m, &mut out);
    }
    out
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

/// TD-SEC-10, TD-FEE-16, TD-SEC-03: the real workspace has no finding.
#[test]
fn td_sec_10_workspace_obeys_the_layer_table() {
    let root = repo_root();
    let ws = load_workspace(&root);
    let rels: Vec<&str> = ws.members.iter().map(|m| m.rel.as_str()).collect();
    for expected in ["app/core", "app/server", "tests/integration"] {
        assert!(
            rels.contains(&expected),
            "{expected} missing from members {rels:?}"
        );
    }
    assert_eq!(check_workspace(&root), Vec::<String>::new());
}

const ROOT_MANIFEST: &str = "[workspace]\nmembers = [\"app/*\", \"tests/recovery\"]\n\n\
[workspace.dependencies]\naskhava-core = { path = \"app/core\" }\n\
askhava-adapters = { path = \"app/adapters\" }\naskhava-payments = { path = \"app/payments\" }\n\
askhava-jobs = { path = \"app/jobs\" }\naskhava-picks = { path = \"app/picks\" }\nserde = \"1\"\n\n\
[workspace.lints.rust]\nunsafe_code = \"forbid\"\n";

/// A planted member manifest: package name, `[dependencies]` lines, then anything after.
macro_rules! pkg {
    ($name:literal, $deps:literal) => {
        pkg!($name, $deps, "")
    };
    ($name:literal, $deps:literal, $tail:literal) => {
        concat!(
            "[package]\nname = \"",
            $name,
            "\"\n\n[dependencies]\n",
            $deps,
            "\n\n[lints]\nworkspace = true\n",
            $tail
        )
    };
}

/// The clean scratch tree: every file and its text. The adapter is idempotent, `payments` wires its
/// adapter and mint, the route input is strict, `core` has `test-control`, and the words in the
/// comment and strings of `routes.rs` must not count (SEC-010, SEC-018, FEE-032).
#[rustfmt::skip]
const BASE: [(&str, &str); 10] = [
    ("Cargo.toml", ROOT_MANIFEST),
    ("app/core/Cargo.toml", pkg!("askhava-core", "", "\n[features]\ntest-control = []\n")),
    ("app/adapters/Cargo.toml", pkg!("askhava-adapters", "askhava-core = { workspace = true }")),
    ("app/payments/Cargo.toml", pkg!("askhava-payments", "askhava-adapters = { workspace = true }")),
    ("app/jobs/Cargo.toml", pkg!("askhava-jobs", "serde = { workspace = true }")),
    ("app/picks/Cargo.toml", pkg!("askhava-picks", "askhava-jobs = { workspace = true }\naskhava-payments = { workspace = true }")),
    ("tests/recovery/Cargo.toml", pkg!("askhava-recovery-tests", "")),
    ("app/adapters/src/lib.rs", "pub trait Payments {\n    fn create_charge(&self, idempotency_key: &str);\n    fn fetch_charge(&self, id: &str);\n}\n"),
    ("app/payments/src/lib.rs", "use askhava_adapters::api::payments::Stripe;\npub fn wire() { let _m = release_mint(); }\n"),
    ("app/jobs/src/routes.rs", "// release_mint, unsafe\n#[derive(Deserialize)]\n#[serde(deny_unknown_fields)]\npub struct NewJob { title: String }\nasync fn create(Form(f): Form<NewJob>) -> Json<Out> { let s = \"unsafe { ReleaseMint }\"; let r = r#\"SessionMint\"#; let c = '\\''; }\n"),
];

const JOBS: &str = "app/jobs/Cargo.toml";
const JOBS_SRC: &str = "app/jobs/src/lib.rs";
const FORBID_OFF: &str =
    "[workspace]\nmembers = [\"app/core\"]\n[workspace.lints.rust]\nunsafe_code = \"allow\"\n";

/// Planted violations: case, the file written over the clean tree, its text, and the findings it
/// must give, each a substring of one finding and nothing more (TD-SEC-10, TD-FEE-16, TD-SEC-03).
#[rustfmt::skip]
const CASES: [(&str, &str, &str, &[&str]); 24] = [
    ("upward", JOBS, pkg!("askhava-jobs", "askhava-picks = { workspace = true }"), &["app/jobs: `askhava-picks` is not a dependency the layer table allows for `jobs`"]),
    ("rename", JOBS, pkg!("askhava-jobs", "pk = { workspace = true, package = \"askhava-picks\" }"), &["`pk` is a rename of package `askhava-picks`", "allows for `jobs`"]),
    ("path", JOBS, pkg!("askhava-jobs", "pk = { path = \"../picks\" }"), &["`pk` lacks `workspace = true`", "`pk` is a rename", "allows for `jobs`"]),
    ("not_workspace", JOBS, pkg!("askhava-jobs", "serde = \"1\""), &["app/jobs: dependency `serde` lacks `workspace = true`"]),
    ("unlisted", JOBS, pkg!("askhava-jobs", "regex = { workspace = true }"), &["app/jobs: dependency `regex` is not in [workspace.dependencies]"]),
    ("core_upward", "app/core/Cargo.toml", pkg!("askhava-core", "askhava-adapters = { workspace = true }"), &["app/core: `askhava-adapters` is not a dependency the layer table allows for `core`"]),
    ("no_lints", JOBS, "[package]\nname = \"askhava-jobs\"\n", &["app/jobs: unsafe_code = \"forbid\" not inherited"]),
    ("forbid_off", "Cargo.toml", FORBID_OFF, &["app/core: unsafe_code = \"forbid\" not inherited"]),
    ("feature", JOBS, pkg!("askhava-jobs", "", "\n[features]\ntesting = []\n"), &["app/jobs: feature `testing` is not allowed"]),
    ("optional_dep", JOBS, pkg!("askhava-jobs", "serde = { workspace = true, optional = true }"), &["app/jobs: optional dependency `serde` is a feature"]),
    ("bad_name", "app/picks/Cargo.toml", pkg!("picks", ""), &["app/picks: package must be named `askhava-picks`"]),
    ("recovery_name", "tests/recovery/Cargo.toml", pkg!("askhava-recovery", ""), &["tests/recovery: package must be named `askhava-recovery-tests`"]),
    ("unknown_module", "app/billing/Cargo.toml", pkg!("askhava-billing", ""), &["app/billing: not a module in kit/layers.json"]),
    ("unsafe", JOBS_SRC, "fn f() { unsafe { } }\n", &["app/jobs/src/lib.rs: `unsafe` is forbidden"]),
    ("release_mint", JOBS_SRC, "fn f() { let _m = askhava_privacy::api::release_mint(); }\n", &["app/jobs/src/lib.rs: `release_mint` named outside"]),
    ("session_mint", "tests/recovery/tests/drill.rs", "fn f(m: SessionMint) {}\n", &["tests/recovery/tests/drill.rs: `SessionMint` named outside"]),
    ("payments_adapter", JOBS_SRC, "use askhava_adapters::api::{sms, payments::Charge};\n", &["app/jobs/src/lib.rs: imports the payments adapter"]),
    ("stripe_adapter", "app/picks/src/lib.rs", "fn f(c: askhava_adapters::api::StripeClient) {}\n", &["app/picks/src/lib.rs: imports the payments adapter"]),
    ("alias_adapter", JOBS_SRC, "use askhava_adapters as ad;\n", &["app/jobs/src/lib.rs: imports the payments adapter"]),
    ("glob_adapter", JOBS_SRC, "use askhava_adapters::api::*;\n", &["app/jobs/src/lib.rs: imports the payments adapter"]),
    ("loose_input", JOBS_SRC, "#[derive(Deserialize)]\npub struct Loose { a: u8 }\nasync fn h(Form(f): Form<Loose>) {}\n", &["app/jobs/src/lib.rs: route input `Loose` is not a deny_unknown_fields struct"]),
    ("map_input", JOBS_SRC, "async fn h(Query(q): Query<HashMap<String, String>>) {}\n", &["app/jobs/src/lib.rs: route input `HashMap`"]),
    ("no_idempotency", "app/adapters/src/sms.rs", "pub trait Sms {\n    async fn send_text(&self, to: &str);\n}\n", &["app/adapters/src/sms.rs: adapter method `send_text` takes no idempotency key"]),
    ("verify_send_code", "app/adapters/src/verify.rs", "pub trait Verify {\n    fn verify_send_code(&self, to: &str);\n}\n", &["app/adapters/src/verify.rs: adapter method `verify_send_code` takes no idempotency key"]),
];

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("a parent folder")).expect("create folder");
    fs::write(&path, content).expect("write planted file");
}

/// A clean scratch workspace under `CARGO_TARGET_TMPDIR` with the real `kit/layers.json` (D-488).
fn plant(case: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("architecture")
        .join(case);
    if root.exists() {
        fs::remove_dir_all(&root).expect("clear the old plant");
    }
    let layers = fs::read_to_string(repo_root().join("kit/layers.json")).expect("kit/layers.json");
    write(&root, "kit/layers.json", &layers);
    for (rel, content) in BASE {
        write(&root, rel, content);
    }
    root
}

/// TD-SEC-10: the clean planted tree has no finding, so each case shows only its own.
#[test]
fn td_sec_10_clean_plant_passes() {
    assert_eq!(check_workspace(&plant("clean")), Vec::<String>::new());
}

/// TD-SEC-10 (SEC-029, SEC-010), TD-FEE-16 (FEE-032), TD-SEC-03 (SEC-018): each planted violation
/// gives exactly its own findings.
#[test]
fn td_sec_10_planted_violations_fail() {
    for (case, rel, content, expected) in CASES {
        let root = plant(case);
        write(&root, rel, content);
        let findings = check_workspace(&root);
        for want in expected {
            assert!(
                findings.iter().any(|f| f.contains(want)),
                "{case}: no {want:?} in {findings:?}"
            );
        }
        assert_eq!(
            findings.len(),
            expected.len(),
            "{case}: exactly its own, got {findings:?}"
        );
    }
}
