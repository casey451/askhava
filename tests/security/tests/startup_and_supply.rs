//! Startup key checks, the closed route-input scan and supply-chain checks (SEC-001, SEC-002,
//! SEC-003, SEC-010, SEC-021, SEC-035, SEC-036, SEC-045; TD-SEC-05, TD-SEC-15, TD-SEC-24; T121).
//!
//! The other settings cases are in `app/core/src/tests/settings.rs`, the control-router cases in
//! `control_router_absent.rs` and the Dockerfile digests in `image_pinned.rs` (D-5574 G2). The
//! SEC-010 scan is D-4258's closed rule: every `.rs` file under `app/*/src/` (`src/tests/` skipped;
//! comments, string literals and `#[cfg(test)]` items removed) is searched for the input forms F1
//! (`Json`, `Form`, `Query`, `CsrfForm`, `Path` parameters), F2 (their turbofish), F3 (`RawQuery`,
//! `RawForm`, `Multipart`, `Bytes` parameters), F4 (a serde format's `from_*`) and F5 (`to_bytes(`,
//! `.split('&')`). A form type must deny unknown fields, nested types too, or be a
//! `REFUSING_TRY_FROM` struct; every other site must be in `CLASSIFIED` under one class. A tuple
//! struct (a newtype) carries no names and is judged by its fields. Every rule has planted cases.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use askhava_core::api::{Settings, SettingsError};

/// `try_from` structs whose conversion refuses an unknown name (D-5574): `file struct | refusal`.
const REFUSING_TRY_FROM: &str = r#"
try_from server/src/business_portal.rs TimesForm | _ => return Err(format!("unknown field `{name}`")),
try_from server/src/business_portal.rs JobTypesForm | _ => return Err(format!("unknown field `{name}`")),
try_from server/src/business_signup.rs CapabilityForm | Err("an unknown or repeated capability field")
try_from server/src/business_step_up.rs StepUpForm | _ => return Err(format!("unknown field `{name}`")),
try_from server/src/address_changes.rs Posted | return Err(format!("unknown field `{name}`"));
try_from server/src/business_guarded_changes.rs Posted | return Err(format!("unknown field `{name}`"));
"#;

/// Each raw or manual parse site: `class file fn…`, keyed by file and enclosing `fn` (or top-level
/// `static`), one class each (D-4258, D-5410, D-5574).
const CLASSIFIED: &str = "
provider-webhook payments/src/routes.rs receive checked
provider-webhook payments/src/release_proof.rs from_webhook
provider-webhook messaging/src/email.rs parse_event
provider-webhook privacy/src/reader.rs read_inbound_email
provider-webhook adapters/src/signatures.rs json_id form_pairs
provider-webhook server/src/call_status.rs status
provider-webhook server/src/inbound_text.rs inbound status
provider-webhook server/src/inbound_voice.rs voice
provider-webhook server/src/voice_status.rs status key key_query
own-query calls/src/voice.rs from_query
own-query server/src/inbound_voice.rs parse
webauthn portals/src/owner_auth.rs passkey_ok finish_passkey_registration
raw-query portals/src/business_auth.rs signin_page signin_open signin_post from_raw
raw-query portals/src/quote_page.rs show page_query
raw-query server/src/business_signup.rs from_query
raw-query portals/src/update_channel.rs confirm_query
csrf-peek server/src/chat_codes.rs posted_csrf check_token
csrf-peek server/src/business_signin.rs has_code note_client
names-refused server/src/business_signup.rs same_site
names-refused server/src/homeowner_portal.rs upload_fields
outbound-response adapters/src/base.rs post_form json twilio_answer vendor_error
outbound-response adapters/src/sms.rs fetch_message_from error_answer read_answer
stored portals/src/owner_auth.rs owner_passkeys passkey_registration_options
stored server/src/offdb_stream.rs verify_audit_chain
stored messaging/src/help_requests.rs run_help_notice
stored messaging/src/spanish_switch.rs latest
cli server/src/bin/rules_bridge.rs answer
ai-output ai/src/schemas.rs parse_reading
embedded adapters/src/signatures.rs STRIPE_IPS
embedded ai/src/output_checks.rs is_opener word_022
embedded ai/src/schemas.rs intake_schema
embedded catalog/src/loader.rs parse
embedded catalog/src/markets.rs parse_markets
embedded catalog/src/question_bank.rs parse_bank
embedded catalog/src/photo_asks.rs load
embedded core/src/brand.rs parse
embedded intake/src/design.rs is_design_category
embedded intake/src/golden_run.rs golden_rows
embedded licensing/src/class_map.rs load read_categories
embedded licensing/src/handyman.rs parse_gas
embedded licensing/src/lines.rs parse_templates
embedded messaging/src/rules.rs parse
embedded messaging/src/templates.rs parse_with
";

/// `names-refused` bodies: `file fn list | refusal`, the closed list the body names and the
/// refusal it gives for a name outside it or repeated.
const NAMES_REFUSED: &str = "
server/src/business_signup.rs same_site names | BAD_REQUEST
server/src/homeowner_portal.rs upload_fields UPLOAD_PARTS | known = false
";

const EXTRACTORS: [&str; 5] = ["Json", "Form", "Query", "CsrfForm", "Path"];
const PREFIXES: [&str; 5] = [
    "",
    "axum::",
    "axum::extract::",
    "security::",
    "crate::security::",
];
const RAW_PARAMS: [&str; 4] = ["RawQuery", "RawForm", "Multipart", "Bytes"];
const PARSERS: &str =
    "serde_json serde_urlencoded serde_saphyr serde_yaml toml serde_qs serde_html_form";
const OPEN_TYPES: [&str; 4] = ["HashMap", "BTreeMap", "Vec", "Value"];
const SCALARS: &str = "String Uuid bool u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize";

/// One scanned file: its path from the root, its crate and its code.
struct Src {
    rel: String,
    krate: String,
    code: String,
}

/// A `struct` or `enum`: its file, crate, name, attributes, body and shape.
struct Item {
    rel: String,
    krate: String,
    name: String,
    attrs: String,
    body: String,
    tuple: bool,
    is_enum: bool,
}

/// The files and their items, for one scan.
struct World {
    srcs: Vec<Src>,
    items: Vec<Item>,
}

/// The rows of a table: the words before any `|`.
fn rows(table: &str) -> impl Iterator<Item = Vec<&str>> {
    table.lines().map(words).filter(|w| !w.is_empty())
}

/// The whitespace-separated words of `line` before any `|`.
fn words(line: &str) -> Vec<&str> {
    line.split('|')
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect()
}

/// Whether `table` lists `name` for `rel` (`app/` dropped).
fn listed(table: &str, rel: &str, name: &str) -> bool {
    let rel = rel.trim_start_matches("app/");
    rows(table).any(|w| w.get(1) == Some(&rel) && w[2..].contains(&name))
}

fn ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Comments dropped, string literals emptied, character literals kept (a bracket in one becomes
/// `_`, so `.split('&')` is still seen and `'{'` never unbalances a brace).
fn strip(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let (mut out, mut i) = (String::with_capacity(src.len()), 0);
    while i < c.len() {
        let prev_ident = i > 0 && (c[i - 1].is_alphanumeric() || c[i - 1] == '_');
        let end = match (c[i], c.get(i + 1).copied()) {
            ('/', Some('/')) => (i..c.len()).find(|&j| c[j] == '\n').or(Some(c.len())),
            ('/', Some('*')) => Some(block_end(&c, i)),
            ('"', _) => Some(quoted(&c, i + 1, '"')),
            ('r', Some('"' | '#')) if !prev_ident || matches!(c[i - 1], 'b' | 'c') => {
                raw_end(&c, i)
            }
            ('\'', _) => {
                let end = char_end(&c, i).unwrap_or(i + 1);
                let safe = |&ch: &char| if "{}()[]<>;".contains(ch) { '_' } else { ch };
                out.extend(c[i..end].iter().map(safe));
                i = end;
                continue;
            }
            _ => None,
        };
        if let Some(e) = end {
            out.push_str(if c[i] == '/' { " " } else { "\"\"" });
            i = e;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

fn block_end(c: &[char], at: usize) -> usize {
    let (mut depth, mut j) = (0_usize, at);
    while j + 1 < c.len() {
        match (c[j], c[j + 1]) {
            ('/', '*') => (depth, j) = (depth + 1, j + 2),
            ('*', '/') if depth == 1 => return j + 2,
            ('*', '/') => (depth, j) = (depth - 1, j + 2),
            _ => j += 1,
        }
    }
    c.len()
}

fn quoted(c: &[char], mut j: usize, quote: char) -> usize {
    while j < c.len() {
        match c[j] {
            '\\' => j += 2,
            ch if ch == quote => return j + 1,
            _ => j += 1,
        }
    }
    c.len()
}

fn raw_end(c: &[char], at: usize) -> Option<usize> {
    let hashes = c[at + 1..].iter().take_while(|&&ch| ch == '#').count();
    if c.get(at + 1 + hashes) != Some(&'"') {
        return None;
    }
    let close: Vec<char> = std::iter::once('"')
        .chain(std::iter::repeat_n('#', hashes))
        .collect();
    let found = (at + 2 + hashes..c.len()).find(|&j| c[j..].starts_with(&close));
    Some(found.map_or(c.len(), |j| j + close.len()))
}

/// The end of the character literal at `at`, or `None` for a lifetime.
fn char_end(c: &[char], at: usize) -> Option<usize> {
    match (c.get(at + 1), c.get(at + 2)) {
        (Some('\\'), _) => Some(quoted(c, at + 1, '\'')),
        (Some(_), Some('\'')) => Some(at + 3),
        _ => None,
    }
}

/// The index just past the bracket closing the one at `open` (`{`, `(`, `[` or `<`; `->` skipped).
fn close(code: &str, open: usize) -> usize {
    let b = code.as_bytes();
    let pair = ["{}", "()", "[]", "<>"]
        .into_iter()
        .find(|p| p.as_bytes()[0] == b[open]);
    let [o, c] = pair.unwrap_or("<>").as_bytes() else {
        return b.len();
    };
    let mut depth = 0_i32;
    for j in open..b.len() {
        let arrow = *c == b'>' && j > 0 && b[j - 1] == b'-';
        depth += i32::from(b[j] == *o) - i32::from(b[j] == *c && !arrow);
        if depth == 0 {
            return j + 1;
        }
    }
    b.len()
}

/// The first of `stops` at bracket depth 0 from `from` (generics, tuples and arrays skipped).
fn first_at_depth0(code: &str, from: usize, stops: &[u8]) -> Option<usize> {
    let b = code.as_bytes();
    let mut j = from;
    while j < b.len() {
        if stops.contains(&b[j]) {
            return Some(j);
        }
        j = if b"([<".contains(&b[j]) {
            close(code, j)
        } else {
            j + 1
        };
    }
    None
}

/// Every `#[cfg(test)]` item removed to its matching `}` or `;`.
fn drop_test_items(mut code: String) -> String {
    while let Some(at) = code.find("#[cfg(test)]") {
        let end = match first_at_depth0(&code, at + 12, b"{;") {
            Some(j) if code.as_bytes()[j] == b'{' => close(&code, j),
            Some(j) => j + 1,
            None => code.len(),
        };
        code.replace_range(at..end, " ");
    }
    code
}

/// Each whole-word occurrence of `word` in `code`.
fn word_at(code: &str, word: &str) -> Vec<usize> {
    let b = code.as_bytes();
    let whole = |&at: &usize| {
        let after = b.get(at + word.len()).is_none_or(|&c| !ident(c));
        (at == 0 || !ident(b[at - 1])) && after
    };
    code.match_indices(word)
        .map(|(at, _)| at)
        .filter(whole)
        .collect()
}

fn ident_from(code: &str, at: usize) -> &str {
    let rest = code[at..].trim_start();
    &rest[..rest.bytes().take_while(|&c| ident(c)).count()]
}

/// The top-level comma-separated parts of `text`.
fn split_top(text: &str) -> Vec<&str> {
    let (mut parts, mut start, mut j) = (Vec::new(), 0, 0);
    while let Some(at) = first_at_depth0(text, j, b",{") {
        j = close(text, at);
        if text.as_bytes()[at] == b',' {
            parts.push(text[start..at].trim());
            (start, j) = (at + 1, at + 1);
        }
    }
    parts.push(text[start..].trim());
    parts.into_iter().filter(|p| !p.is_empty()).collect()
}

/// Every `struct` and `enum` of `src`; its attributes are the text since the previous item.
fn items_of(src: &Src) -> Vec<Item> {
    let code = &src.code;
    let mut found = Vec::new();
    for (kw, is_enum) in [("struct", false), ("enum", true)] {
        for at in word_at(code, kw) {
            let from = code[..at].rfind([';', '}', '{']).map_or(0, |j| j + 1);
            let Some(open) = first_at_depth0(code, at + kw.len(), b"{(;") else {
                continue;
            };
            let end = if code.as_bytes()[open] == b';' {
                open + 1
            } else {
                close(code, open) - 1
            };
            found.push(Item {
                rel: src.rel.clone(),
                krate: src.krate.clone(),
                name: ident_from(code, at + kw.len()).to_owned(),
                attrs: code[from..at].to_owned(),
                body: code[(open + 1).min(end)..end].to_owned(),
                tuple: code.as_bytes()[open] == b'(',
                is_enum,
            });
        }
    }
    found
}

/// The `use` imports of `code`: (full path, local name), braces expanded, `as` renames kept.
fn uses(code: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for at in word_at(code, "use") {
        let end = code[at..].find(';').map_or(code.len(), |j| at + j);
        expand("", &code[at + 3..end], &mut out);
    }
    out
}

fn expand(prefix: &str, tree: &str, out: &mut Vec<(String, String)>) {
    let tree = tree.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(open) = tree.find('{') {
        let head = format!("{prefix}{}", &tree[..open]);
        for part in split_top(&tree[open + 1..close(&tree, open) - 1]) {
            expand(&head, part, out);
        }
        return;
    }
    let (path, alias) = tree.split_once(" as ").unwrap_or((tree.as_str(), ""));
    let full = format!("{prefix}{}", path.trim());
    let last = full.rsplit("::").next().unwrap_or_default().to_owned();
    out.push((
        full,
        if alias.is_empty() {
            last
        } else {
            alias.trim().to_owned()
        },
    ));
}

/// The target of `type name = …;` in `code`, if any.
fn alias_of(code: &str, name: &str) -> Option<String> {
    word_at(code, "type").into_iter().find_map(|at| {
        let rest = code[at + 4..].trim_start().strip_prefix(name)?;
        let target = rest.trim_start().strip_prefix('=')?;
        Some(target[..target.find(';')?].trim().to_owned())
    })
}

/// `text` with each `type name = …;` alias of `code` replaced by its target, nested aliases too.
fn unalias(code: &str, text: &str) -> String {
    let mut out = text.to_owned();
    for _ in 0..4 {
        for at in word_at(code, "type") {
            let name = ident_from(code, at + 4);
            let Some(target) = alias_of(code, name).filter(|_| !name.is_empty()) else {
                continue;
            };
            for j in word_at(&out, name).into_iter().rev() {
                out.replace_range(j..j + name.len(), &target);
            }
        }
    }
    out
}

/// A `fn`: its name, parameter text and body span.
type FnSig = (String, String, Option<(usize, usize)>);

/// The `fn`s of `code`.
fn fns(code: &str) -> Vec<FnSig> {
    let mut out = Vec::new();
    for at in word_at(code, "fn") {
        let name = ident_from(code, at + 2);
        let Some(open) = code[at..].find('(').map(|j| at + j) else {
            continue;
        };
        let shut = close(code, open);
        let body = first_at_depth0(code, shut, b"{;").filter(|&j| code.as_bytes()[j] == b'{');
        let span = body.map(|j| (j, close(code, j)));
        if !name.is_empty() {
            out.push((name.to_owned(), code[open + 1..shut - 1].to_owned(), span));
        }
    }
    out
}

/// The site key of `pos`: the innermost enclosing `fn`, else the last top-level `static`.
fn site_of(code: &str, pos: usize) -> String {
    let around = |(n, _, b): FnSig| b.filter(|&(s, e)| s <= pos && pos < e).map(|(s, _)| (s, n));
    if let Some((_, name)) = fns(code).into_iter().filter_map(around).max() {
        return name;
    }
    let last = word_at(&code[..pos], "static").last().copied();
    last.map(|at| ident_from(code, at + 6).to_owned())
        .unwrap_or_default()
}

/// A local name standing for an extractor, and whether its bare (unprefixed) form counts.
type ExtName = (String, &'static str, bool);

/// The local names standing for each extractor: the name itself (its bare form not counted when
/// imported from outside `axum`, `CsrfForm` from anywhere; its `axum::` paths always are) and each
/// `use … as` rename.
fn extractor_names(code: &str) -> Vec<ExtName> {
    let imports = uses(code);
    let mut out = Vec::new();
    for ext in EXTRACTORS {
        let foreign = |(p, l): &(String, String)| l == ext && !p.starts_with("axum");
        let bare = ext == "CsrfForm" || !imports.iter().any(foreign);
        out.push((ext.to_owned(), ext, bare));
        for (path, local) in &imports {
            if path.ends_with(&format!("::{ext}")) && local != ext {
                out.push((local.clone(), ext, true));
            }
        }
    }
    out
}

/// Each `name<T>` (`name::<T>` when `turbofish`) in `text` under an allowed path: (at, extractor, T).
fn generic_uses(
    text: &str,
    names: &[ExtName],
    turbofish: bool,
) -> Vec<(usize, &'static str, String)> {
    let open = if turbofish { "::<" } else { "<" };
    let path_char = |c: char| c.is_alphanumeric() || c == '_' || c == ':';
    let mut out = Vec::new();
    for (local, ext, bare) in names {
        for at in word_at(text, local) {
            let from = text[..at].rfind(|c| !path_char(c)).map_or(0, |j| j + 1);
            let lt = at + local.len() + open.len() - 1;
            let prefix = &text[from..at];
            let counted = PREFIXES.contains(&prefix) && (*bare || !prefix.is_empty());
            if counted && text[at + local.len()..].starts_with(open) {
                out.push((
                    at,
                    *ext,
                    text[lt + 1..close(text, lt) - 1].trim().to_owned(),
                ));
            }
        }
    }
    out
}

/// The scan's findings in `world`: failing form types and unclassified sites (SEC-010).
fn findings(world: &World) -> Vec<String> {
    let mut bad = Vec::new();
    for src in &world.srcs {
        let names = extractor_names(&src.code);
        let strict =
            |ext: &str, ty: &str| check_type(world, src, ty, ext == "Path", &mut BTreeSet::new());
        for (func, params, _) in fns(&src.code) {
            let params = unalias(&src.code, &params);
            let forms = generic_uses(&params, &names, false).into_iter();
            let loose =
                forms.filter_map(|(_, ext, ty)| strict(ext, &ty).err().map(|why| (ext, ty, why)));
            bad.extend(
                loose.map(|(ext, ty, why)| format!("{} {func}: F1 {ext}<{ty}>: {why}", src.rel)),
            );
            if RAW_PARAMS
                .iter()
                .any(|raw| !word_at(&params, raw).is_empty())
            {
                bad.extend(unclassified(src, &func, "F3"));
            }
        }
        let turbofish = generic_uses(&src.code, &names, true).into_iter();
        let raw = turbofish
            .filter(|(_, ext, ty)| strict(ext, ty).is_err())
            .map(|(at, _, _)| at);
        for at in raw.chain(raw_parses(&src.code)) {
            bad.extend(unclassified(src, &site_of(&src.code, at), "F2/F4/F5"));
        }
    }
    bad
}

/// The F4 and F5 positions in `code`, outside `use` lines.
fn raw_parses(code: &str) -> Vec<usize> {
    let mut at = Vec::new();
    for p in PARSERS.split(' ') {
        at.extend(
            word_at(code, p)
                .into_iter()
                .filter(|&j| code[j + p.len()..].starts_with("::from_")),
        );
    }
    let forms = word_at(code, "form_urlencoded").into_iter();
    at.extend(forms.filter(|&j| code[j..].starts_with("form_urlencoded::parse")));
    let parser = |path: &str| {
        PARSERS
            .split(' ')
            .any(|p| path.starts_with(&format!("{p}::")))
    };
    for (path, local) in uses(code) {
        if local.starts_with("from_") && parser(&path) {
            let bare =
                |&j: &usize| !code[..j].ends_with("::") && !code[..j].trim_end().ends_with("fn");
            at.extend(word_at(code, &local).into_iter().filter(bare));
        }
    }
    at.extend(
        word_at(code, "to_bytes")
            .into_iter()
            .filter(|&j| code[j + 8..].starts_with('(')),
    );
    at.extend(code.match_indices(".split('&')").map(|(j, _)| j));
    let stmt =
        |j: usize| code[code[..j].rfind([';', '{', '}']).map_or(0, |s| s + 1)..j].trim_start();
    at.into_iter()
        .filter(|&j| !stmt(j).starts_with("use ") && !stmt(j).starts_with("pub use "))
        .collect()
}

/// A finding unless `func` of `src` is in `CLASSIFIED`; a `names-refused` body must also name its
/// closed list and give its refusal.
fn unclassified(src: &Src, func: &str, form: &str) -> Option<String> {
    if !listed(CLASSIFIED, &src.rel, func) {
        return Some(format!("{} {func}: {form} site not in CLASSIFIED", src.rel));
    }
    let rel = src.rel.trim_start_matches("app/");
    let row = NAMES_REFUSED
        .lines()
        .find(|l| l.starts_with(&format!("{rel} {func} ")))?;
    let (head, refusal) = row.split_once(" | ")?;
    let list = head.rsplit(' ').next()?;
    let bodies = fns(&src.code).into_iter().filter(|(n, _, _)| n == func);
    let body = |(s, e): (usize, usize)| &src.code[s..e];
    let ok = bodies
        .filter_map(|(_, _, b)| b.map(body))
        .any(|t| !word_at(t, list).is_empty() && t.contains(refusal));
    (!ok).then(|| {
        format!(
            "{} {func}: names-refused body lacks `{list}` or `{refusal}`",
            src.rel
        )
    })
}

/// SEC-010 rule (3) for a form type `ty` read in `src` (`path` for `Path<T>`).
fn check_type(
    world: &World,
    src: &Src,
    ty: &str,
    path: bool,
    seen: &mut BTreeSet<String>,
) -> Result<(), String> {
    let ty = ty.trim().trim_start_matches('&').trim();
    if let Some(inner) = ty.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        if !path {
            return Err("a tuple names no fields".into());
        }
        return split_top(inner)
            .iter()
            .try_for_each(|p| check_type(world, src, p, path, seen));
    }
    let head = ty
        .split('<')
        .next()
        .unwrap_or_default()
        .rsplit("::")
        .next()
        .unwrap_or_default();
    if OPEN_TYPES.contains(&head) {
        return Err(format!("`{head}` cannot deny unknown fields"));
    }
    if SCALARS.split(' ').any(|s| s == head) {
        return if path {
            Ok(())
        } else {
            Err(format!("`{head}` is not a struct"))
        };
    }
    if let Some(target) = alias_of(&src.code, head) {
        return check_type(world, src, &target, path, seen);
    }
    let renamed = uses(&src.code)
        .into_iter()
        .find(|(p, l)| l == head && !p.ends_with(head));
    let name = renamed.map_or(head.to_owned(), |(p, _)| {
        p.rsplit("::").next().unwrap_or_default().to_owned()
    });
    if src.rel.ends_with("portals/src/security.rs") && name == "T" {
        return Ok(());
    }
    item_passes(world, find_item(world, src, &name)?, seen)
}

/// The definition of `name` seen from `src`: same file, else same crate, else the workspace's one.
fn find_item<'w>(world: &'w World, src: &Src, name: &str) -> Result<&'w Item, String> {
    let all: Vec<&Item> = world.items.iter().filter(|i| i.name == name).collect();
    let scopes: [&dyn Fn(&Item) -> bool; 3] =
        [&|i| i.rel == src.rel, &|i| i.krate == src.krate, &|_| true];
    for scope in scopes {
        match all
            .iter()
            .copied()
            .filter(|i| scope(i))
            .collect::<Vec<_>>()
            .as_slice()
        {
            [one] => return Ok(*one),
            [] => {}
            _ => return Err(format!("`{name}` is defined more than once")),
        }
    }
    Err(format!("no definition of `{name}`"))
}

/// Whether `attrs` hold a plain `#[serde(…deny_unknown_fields…)]`, not one under `cfg_attr`.
fn denies_unknown(attrs: &str) -> bool {
    attrs
        .split("#[")
        .any(|a| a.trim_start().starts_with("serde") && a.contains("deny_unknown_fields"))
}

/// Whether `item` refuses unknown fields, every nested workspace field type doing the same.
fn item_passes(world: &World, item: &Item, seen: &mut BTreeSet<String>) -> Result<(), String> {
    let name = &item.name;
    if !seen.insert(format!("{}::{name}", item.rel)) {
        return Ok(());
    }
    if item.is_enum {
        let payload = |v: &&str| {
            v.rsplit_once(']')
                .map_or(*v, |(_, t)| t)
                .contains(['(', '{'])
        };
        let data = split_top(&item.body).iter().any(payload);
        return if data {
            Err(format!("enum `{name}` carries data"))
        } else {
            Ok(())
        };
    }
    if item.attrs.contains("try_from") {
        let ok = listed(REFUSING_TRY_FROM, &item.rel, name);
        return if ok {
            Ok(())
        } else {
            Err(format!(
                "`{name}` is a try_from struct not in REFUSING_TRY_FROM"
            ))
        };
    }
    if !item.tuple && !denies_unknown(&item.attrs) {
        return Err(format!("`{name}` lacks deny_unknown_fields"));
    }
    let src = world
        .srcs
        .iter()
        .find(|s| s.rel == item.rel)
        .ok_or("no source")?;
    for field in split_top(&item.body) {
        if field.contains("flatten") {
            return Err(format!("`{name}` flattens a field"));
        }
        let ty = field.rsplit_once(']').map_or(field, |(_, t)| t);
        let ty = if item.tuple {
            ty
        } else {
            ty.split_once(':').map_or(ty, |(_, t)| t)
        };
        field_passes(world, src, ty, seen)?;
    }
    Ok(())
}

/// A field type passes when it names no open map and every workspace `Deserialize` type it names passes.
fn field_passes(
    world: &World,
    src: &Src,
    ty: &str,
    seen: &mut BTreeSet<String>,
) -> Result<(), String> {
    for word in ty
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
    {
        if ["HashMap", "BTreeMap", "Value"].contains(&word) {
            return Err(format!("a field is a `{word}`"));
        }
        if world
            .items
            .iter()
            .any(|i| i.name == word && i.attrs.contains("Deserialize"))
        {
            item_passes(world, find_item(world, src, word)?, seen)?;
        }
    }
    Ok(())
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rs_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for path in fs::read_dir(dir)
        .expect("readable directory")
        .flatten()
        .map(|e| e.path())
    {
        if path.is_dir() && !path.ends_with("src/tests") {
            rs_files(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// A world of `(rel, source)` files.
fn world(files: Vec<(String, String)>) -> World {
    let src = |(rel, text): (String, String)| Src {
        krate: rel.split('/').nth(1).unwrap_or_default().to_owned(),
        code: drop_test_items(strip(&text)),
        rel,
    };
    let srcs: Vec<Src> = files.into_iter().map(src).collect();
    let items = srcs.iter().flat_map(items_of).collect();
    World { srcs, items }
}

/// Every `.rs` file under `app/*/src/`, `src/tests/` skipped.
fn tree() -> World {
    let mut files = Vec::new();
    for krate in fs::read_dir(root().join("app")).expect("app/").flatten() {
        if krate.path().join("src").is_dir() {
            rs_files(&krate.path().join("src"), &mut files);
        }
    }
    let rel = |p: &PathBuf| {
        p.strip_prefix(root())
            .expect("under root")
            .to_string_lossy()
            .into_owned()
    };
    world(
        files
            .iter()
            .map(|p| (rel(p), fs::read_to_string(p).expect("readable")))
            .collect(),
    )
}

/// TD-SEC-05; SEC-010: every route input on `main` passes the closed scan, and every listed
/// site still exists.
#[test]
fn td_sec_05_every_route_input_refuses_unknown_fields() {
    let world = tree();
    assert!(world.srcs.len() > 300, "scope: {} files", world.srcs.len());
    assert_eq!(findings(&world), Vec::<String>::new());
    for row in rows(CLASSIFIED).chain(rows(REFUSING_TRY_FROM)) {
        let src = world
            .srcs
            .iter()
            .find(|s| s.rel == format!("app/{}", row[1]))
            .expect(row[1]);
        for name in &row[2..] {
            assert!(
                !word_at(&src.code, name).is_empty(),
                "{} {name}: a listed site is gone",
                row[1]
            );
        }
    }
}

const DEFS: &str = "#[derive(Deserialize)] struct Loose { a: String }
#[derive(Deserialize)] #[serde(deny_unknown_fields)] struct Strict { a: String, kind: Kind }
#[derive(Deserialize)] enum Kind { #[serde(rename = \"one\")] One, Two }\n";

/// Must fail, one finding each, every case planted beside `DEFS` (D-4258 (6)).
const MUST_FAIL: &str = r#"
async fn f(Form(x): Form<Loose>) {}
async fn f(Json(x): Json<HashMap<String, String>>) {}
async fn f(q: Query<BTreeMap<String, String>>) {}
async fn f(Form(x): Form<Vec<(String, String)>>) {}
async fn f(Json(x): Json<serde_json::Value>) {}
async fn f(r: Request) { axum::Form::<Vec<(String, String)>>::from_request(r, &()).await; }
type In = Loose; async fn f(Form(x): Form<In>) {}
use axum::Json as J; async fn f(J(x): J<Loose>) {}
#[derive(Deserialize)] #[serde(deny_unknown_fields)] struct W { a: String, #[serde(flatten)] b: HashMap<String, String> } async fn f(Form(x): Form<W>) {}
#[derive(Deserialize)] #[serde(deny_unknown_fields)] struct O { inner: Loose } async fn f(Form(x): Form<O>) {}
async fn f(Path(x): Path<Loose>) {}
async fn f(m: Multipart) {}
async fn f(RawQuery(q): RawQuery) {}
async fn f(RawForm(b): RawForm) {}
async fn f(body: axum::body::Bytes) {}
fn f(body: &[u8]) { let _: Strict = serde_json::from_slice(&body).unwrap(); }
use serde_json::from_str; fn f(body: &str) { let _: Strict = from_str(body).unwrap(); }
fn f(q: &str) { q.split('&'); }
async fn f(r: Request) { axum::body::to_bytes(r.into_body(), 9).await; }
#[derive(Deserialize)] #[serde(try_from = "Vec<(String, String)>")] struct T2 { a: String } async fn f(Form(x): Form<T2>) {}
use std::path::Path; async fn f(x: axum::extract::Path<Loose>) {}
use sqlx::types::Json; async fn f(x: axum::Json<Loose>) {}
type In = axum::Form<Loose>; async fn f(x: In) {}
#[derive(Deserialize)] enum E { A, B(String) } async fn f(Json(x): Json<E>) {}
#[derive(Deserialize)] struct Tu(HashMap<String, String>); async fn f(Json(x): Json<Tu>) {}
#[derive(Deserialize)] #[cfg_attr(test, serde(deny_unknown_fields))] struct C { a: String } async fn f(Form(x): Form<C>) {}
"#;

/// Must pass, planted beside `DEFS`: strict forms, positional paths and a return type.
const MUST_PASS: &str = "
async fn f(Json(x): Json<Strict>) {}
async fn f(Form(x): axum::Form<Strict>) {}
async fn f(Query(x): axum::extract::Query<Strict>) {}
async fn f(CsrfForm(x): CsrfForm<Strict>) {}
async fn f(Path((a, b)): Path<(String, String)>) {}
async fn f(Path(id): Path<Uuid>) {}
fn f() -> Json<serde_json::Value> { g() }
use sqlx::types::Json; fn f(x: Json<Loose>) {}
#[derive(Deserialize)] struct Tu(Strict); async fn f(Json(x): Json<Tu>) {}
type In = axum::Form<Strict>; async fn f(x: In) {}
";

/// The findings for planted files `(rel under app/, code)`; the first file also holds `DEFS`.
fn planted(files: &[(&str, &str)]) -> Vec<String> {
    let file = |(i, (rel, code)): (usize, &(&str, &str))| {
        (
            format!("app/{rel}"),
            format!("{}{code}", if i == 0 { DEFS } else { "" }),
        )
    };
    findings(&world(files.iter().enumerate().map(file).collect()))
}

/// TD-SEC-05; SEC-010: each planted loose form, raw parse and unlisted site is one finding.
#[test]
fn td_sec_05_planted_loose_inputs_fail_the_scan() {
    for code in MUST_FAIL.lines().filter(|l| !l.is_empty()) {
        assert_eq!(planted(&[("demo/src/lib.rs", code)]).len(), 1, "{code}");
    }
    let strict = "#[derive(Deserialize)] #[serde(deny_unknown_fields)] struct Body { a: String }";
    let loose =
        "#[derive(Deserialize)] struct Body { a: String } async fn f(Form(x): Form<Body>) {}";
    assert_eq!(
        planted(&[("one/src/lib.rs", strict), ("two/src/lib.rs", loose)]).len(),
        1
    );
    let peek = "async fn same_site(r: Request) { axum::Form::<Vec<(String, String)>>::from_request(r, &()) }";
    assert_eq!(
        planted(&[("server/src/business_signup.rs", peek)]).len(),
        1,
        "no closed list"
    );
}

/// TD-SEC-05; SEC-010: strict forms, positional paths, a listed `try_from`, `CsrfForm`'s own `T`
/// and a return type are not findings.
#[test]
fn td_sec_05_planted_strict_inputs_pass_the_scan() {
    let none = Vec::<String>::new();
    for code in MUST_PASS.lines().filter(|l| !l.is_empty()) {
        assert_eq!(planted(&[("demo/src/lib.rs", code)]), none, "{code}");
    }
    let times = "#[derive(Deserialize)] #[serde(try_from = \"Vec<(String, String)>\")] struct TimesForm { a: String }
        async fn f(CsrfForm(x): CsrfForm<TimesForm>) {}";
    assert_eq!(planted(&[("server/src/business_portal.rs", times)]), none);
    let own = "async fn from_request(Form(x): Form<T>) {}";
    assert_eq!(planted(&[("portals/src/security.rs", own)]), none);
}

const TEST_LIST: &str =
    "twilio:ACtestfake01,stripe:acct_testfake,resend:test.example.com,bucket:askhava-test";
const LIVE_LIST: &str =
    "twilio:AClivefake01,stripe:acct_livefake,resend:mail.example.com,bucket:askhava-live";
const LIVE_IDS: &str = "ASKHAVA_TWILIO_ACCOUNT_SID=AClivefake01 ASKHAVA_STRIPE_ACCOUNT_ID=acct_livefake \
    ASKHAVA_RESEND_DOMAIN=mail.example.com ASKHAVA_STORAGE_BUCKET=askhava-live";

/// Startup with fake settings for `env`, `extra` replacing a default (R16: pairs, never `set_var`).
fn start(env: &str, extra: &[(&str, &str)]) -> Result<Settings, SettingsError> {
    let mut pairs = vec![
        ("ASKHAVA_APP_ENV", env),
        ("ASKHAVA_ALLOW_LIST_TEST", TEST_LIST),
    ];
    pairs.push(("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"));
    if env == "live" {
        pairs.push(("ASKHAVA_ALLOW_LIST_LIVE", LIVE_LIST));
        pairs.extend(
            LIVE_IDS
                .split_whitespace()
                .filter_map(|p| p.split_once('=')),
        );
    }
    for &(key, value) in extra {
        pairs.retain(|(k, _)| *k != key);
        pairs.push((key, value));
    }
    Settings::from_pairs(pairs.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())))
}

/// TD-SEC-05, TD-SEC-15; SEC-002, SEC-003, SEC-036: a live key in test, a test key in live and an
/// off-list vendor account each stop startup; the right live settings start.
#[test]
fn td_sec_05_wrong_keys_and_accounts_stop_startup() {
    let (live, test) = (
        format!("sk_{}_FAKE0", "live"),
        format!("sk_{}_FAKE0", "test"),
    );
    let (key, sid) = ("ASKHAVA_STRIPE_SECRET_KEY", "ASKHAVA_TWILIO_ACCOUNT_SID");
    let refused = |env, extra: &[(&str, &str)]| start(env, extra).err();
    assert_eq!(
        refused("test", &[(key, &live)]),
        Some(SettingsError::LiveKeyOutsideLive(key))
    );
    assert_eq!(
        refused("live", &[(key, &test)]),
        Some(SettingsError::TestKeyInLive(key))
    );
    assert_eq!(
        refused("live", &[(sid, "ACtestfake01")]),
        Some(SettingsError::ForeignAccount(sid))
    );
    assert!(refused("live", &[("ASKHAVA_STORAGE_BUCKET", "askhava-elsewhere")]).is_some());
    assert!(refused("live", &[(key, &live)]).is_none());
}

/// Each external `uses:` without a 40-character SHA and each `image:` or shorthand `container:`
/// without a digest in `text`. Each line is cut at `{` and `,` so a flow-style mapping
/// (`steps: [{uses: a/b@v1}]`) is read key by key, and any run of dashes and spaces before a key
/// (`-   uses:`) is dropped.
fn unpinned(text: &str) -> Vec<String> {
    let hex = |s: &str, n: usize| {
        s.len() == n && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
    };
    let digest = |v: &str| v.rsplit_once("@sha256:").is_some_and(|(_, h)| hex(h, 64));
    let sha = |v: &str| v.starts_with("./") || v.rsplit_once('@').is_some_and(|(_, r)| hex(r, 40));
    let mut bad = Vec::new();
    let parts = text.lines().flat_map(|l| l.split(['{', ',']));
    for line in parts.map(|l| l.trim().trim_start_matches(['-', ' ', '[']).trim_start()) {
        let Some((key @ ("uses" | "image" | "container"), value)) = line.split_once(':') else {
            continue;
        };
        let value = value
            .split(" #")
            .next()
            .unwrap_or_default()
            .trim()
            .trim_end_matches(['}', ']', ' '])
            .trim_matches(['"', '\'']);
        if key == "container" && value.is_empty() {
            continue;
        }
        let docker = key != "uses" || value.starts_with("docker://");
        if !(if docker { digest(value) } else { sha(value) }) {
            bad.push(line.to_owned());
        }
    }
    bad
}

fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).expect(rel)
}

/// TD-SEC-24; SEC-045, SEC-021 (PT-01, D-2347): every workflow's external `uses:` is pinned to a
/// commit SHA and every image by digest; a tag, a branch or a bare image fails.
#[test]
fn td_sec_24_every_workflow_action_and_image_is_pinned() {
    let mut seen = 0;
    for path in fs::read_dir(root().join(".github/workflows"))
        .expect("dir")
        .flatten()
        .map(|e| e.path())
    {
        if path.extension().is_some_and(|e| e == "yml" || e == "yaml") {
            seen += 1;
            assert_eq!(
                unpinned(&fs::read_to_string(&path).expect("readable")),
                Vec::<String>::new()
            );
        }
    }
    assert!(seen >= 5, "{seen} workflows");
    for bad in [
        "uses: actions/checkout@v4",
        "- uses: a/b@main",
        "-   uses: a/b@v4",
        "steps: [{uses: a/b@v1}]",
        "steps: [{name: x, uses: a/b@v1}]",
        "- {uses: \"a/b@v1\"}",
        "image: postgres:16",
        "uses: docker://alpine:3",
        "container: postgres:16",
    ] {
        assert_eq!(unpinned(bad).len(), 1, "{bad}");
    }
    let (sha, digest) = ("0123456789abcdef0123456789abcdef01234567", "a".repeat(64));
    let good = format!(
        "- uses: a/b@{sha} # v4\nuses: ./.github/workflows/ci.yml\nimage: p:16@sha256:{digest}\ncontainer:\ncontainer: p@sha256:{digest}\nsteps: [{{uses: a/b@{sha}}}, {{uses: ./x}}]\n-   uses: a/b@{sha}"
    );
    assert_eq!(unpinned(&good), Vec::<String>::new());
}

/// What `ci.yml` lacks: SEC-045's commit-range scan and its planted check (T828), and SEC-021's
/// audits, each run on its own line with no `||` to swallow a hit.
fn ci_gaps(ci: &str) -> Vec<&'static str> {
    let needles = [
        (
            "commit-range gitleaks",
            "/gitleaks\" git --log-opts=\"$range\"",
        ),
        (
            "planted two-commit check",
            "\"$gl\" git \"$plant\" --log-opts=HEAD",
        ),
        ("cargo audit", "\n            cargo audit\n"),
        (
            "cargo deny",
            "\n            cargo deny check licenses bans sources advisories\n",
        ),
        ("npm audit", "\n            python3 scripts/npm_audit.py\n"),
    ];
    needles
        .into_iter()
        .filter(|(_, n)| !ci.contains(n))
        .map(|(gap, _)| gap)
        .collect()
}

/// What `deny.toml` lacks: the three denies, or a `Review by YYYY-MM-DD` reason on an ignore.
fn deny_gaps(text: &str) -> Vec<String> {
    let denies = [
        "yanked = \"deny\"",
        "unknown-registry = \"deny\"",
        "unknown-git = \"deny\"",
    ];
    let missing = denies
        .iter()
        .filter(|d| !text.lines().any(|l| l.trim() == **d));
    let mut gaps: Vec<String> = missing.map(|d| (*d).to_owned()).collect();
    let section = text
        .split("[advisories]")
        .nth(1)
        .unwrap_or_default()
        .split("\n[")
        .next();
    for line in section
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("id = "))
    {
        let reason = line.split_once("reason = \"").map_or("", |(_, r)| r);
        let date = reason
            .split_once("Review by ")
            .map_or("", |(_, d)| d.get(..10).unwrap_or_default());
        let digit = |(i, c): (usize, u8)| {
            if i == 4 || i == 7 {
                c == b'-'
            } else {
                c.is_ascii_digit()
            }
        };
        if date.len() != 10 || !date.bytes().enumerate().all(digit) {
            gaps.push(line.trim().to_owned());
        }
    }
    gaps
}

/// TD-SEC-05, TD-SEC-24; SEC-021, SEC-045: `ci.yml` scans the commit range with its planted check
/// and runs the audits, `deny.toml` holds its denies and dated reasons, Dependabot watches Actions
/// and Docker, and both lock files are committed; a planted text lacking each fails.
#[test]
fn td_sec_24_supply_chain_steps_are_present_and_fail_on_a_gap() {
    let ci = read(".github/workflows/ci.yml");
    assert_eq!(ci_gaps(&ci), Vec::<&str>::new());
    assert_eq!(
        ci_gaps(&ci.replace("git --log-opts=\"$range\"", "dir .")),
        ["commit-range gitleaks"]
    );
    assert_eq!(
        ci_gaps(&ci.replace("--log-opts=HEAD", "")),
        ["planted two-commit check"]
    );
    assert_eq!(
        ci_gaps(&ci.replace("cargo audit\n", "cargo audit || true\n")),
        ["cargo audit"]
    );
    let deny = read("deny.toml");
    assert_eq!(deny_gaps(&deny), Vec::<String>::new());
    assert_eq!(
        deny_gaps(&deny.replace("unknown-git = \"deny\"", "unknown-git = \"warn\"")).len(),
        1
    );
    let undated = "[advisories]\nyanked = \"deny\"\nignore = [\n  { id = \"RUSTSEC-0000-0000\", reason = \"x\" },\n]";
    assert_eq!(
        deny_gaps(undated).len(),
        3,
        "two denies and the undated ignore"
    );
    let watched =
        |t: &str| ["github-actions", "docker"].map(|e| t.contains(&format!("ecosystem: \"{e}\"")));
    assert_eq!(watched(&read(".github/dependabot.yml")), [true, true]);
    assert_eq!(watched("- package-ecosystem: \"docker\""), [false, true]);
    for lock in ["Cargo.lock", "package-lock.json"] {
        let out = Command::new("git")
            .args(["ls-files", "--error-unmatch", lock])
            .current_dir(root())
            .output();
        assert!(out.is_ok_and(|o| o.status.success()), "{lock} is committed");
    }
}
