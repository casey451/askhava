//! The Rust side of the `rules_bridge` contract (D-481, D-491, T074): every request this test
//! sends and every answer the binary gives must fit `app/server/rules_bridge.schema.json`, judged by
//! a small validator for the schema's subset (no JSON-schema crate is allowed); the answers are
//! the app's rules (FEE-001, REC-002, QH-001, JOB-002, CARD-001); and the bridge stays inside its
//! budget of 1 ms a call. The Python side is `tests/contract/test_rules_bridge.py`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Instant;

use serde_json::{Value, json};

const SCHEMA: &str = include_str!("../rules_bridge.schema.json");

/// The bridge process, one line per call.
struct Bridge {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Bridge {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rules_bridge"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the bridge starts");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn line(&mut self, line: &str) -> Value {
        writeln!(self.stdin, "{line}").expect("write");
        let mut answer = String::new();
        self.stdout.read_line(&mut answer).expect("read");
        serde_json::from_str(&answer).expect("the answer is JSON")
    }

    fn finish(self) {
        let Self {
            mut child, stdin, ..
        } = self;
        drop(stdin);
        assert!(child.wait().expect("exit").success(), "exit code 0");
    }
}

/// Follows a `#/$defs/...` pointer.
fn lookup<'a>(root: &'a Value, pointer: &str) -> &'a Value {
    let path = pointer.strip_prefix('#').expect("local ref");
    root.pointer(path).expect("ref resolves")
}

/// True when `value` has the JSON-schema type `name`.
fn has_type(value: &Value, name: &str) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    }
}

/// The schema-subset problems of `value`, each with its JSON path.
fn problems(value: &Value, schema: &Value, root: &Value, at: &str) -> Vec<String> {
    if let Some(Value::String(pointer)) = schema.get("$ref") {
        return problems(value, lookup(root, pointer), root, at);
    }
    let mut out = Vec::new();
    let types: Vec<&str> = match schema.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if !types.is_empty() && !types.iter().any(|t| has_type(value, t)) {
        out.push(format!("{at}: not {types:?}"));
    }
    if let Some(Value::Array(allowed)) = schema.get("enum")
        && !allowed.contains(value)
    {
        out.push(format!("{at}: not one of {allowed:?}"));
    }
    let low = schema.get("minimum").and_then(Value::as_i64);
    if low.is_some_and(|low| value.as_i64().is_some_and(|v| v < low)) {
        out.push(format!("{at}: below the minimum"));
    }
    out.extend(object_problems(value, schema, root, at));
    out.extend(array_problems(value, schema, root, at));
    out
}

/// `required`, `properties` and `additionalProperties: false`.
fn object_problems(value: &Value, schema: &Value, root: &Value, at: &str) -> Vec<String> {
    let Some(fields) = value.as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let properties = schema.get("properties").and_then(Value::as_object);
    for key in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !key.as_str().is_some_and(|k| fields.contains_key(k)) {
            out.push(format!("{at}: missing {key}"));
        }
    }
    let closed = schema.get("additionalProperties") == Some(&Value::Bool(false));
    for (key, field) in fields {
        match properties.and_then(|p| p.get(key)) {
            Some(sub) => out.extend(problems(field, sub, root, &format!("{at}.{key}"))),
            None if closed => out.push(format!("{at}: unexpected {key}")),
            None => {}
        }
    }
    out
}

/// `items`, `minItems` and `maxItems`.
fn array_problems(value: &Value, schema: &Value, root: &Value, at: &str) -> Vec<String> {
    let Some(list) = value.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let len = u64::try_from(list.len()).expect("length");
    if schema
        .get("minItems")
        .and_then(Value::as_u64)
        .is_some_and(|n| len < n)
    {
        out.push(format!("{at}: too few items"));
    }
    if schema
        .get("maxItems")
        .and_then(Value::as_u64)
        .is_some_and(|n| len > n)
    {
        out.push(format!("{at}: too many items"));
    }
    if let Some(items) = schema.get("items") {
        for (i, item) in list.iter().enumerate() {
            out.extend(problems(item, items, root, &format!("{at}[{i}]")));
        }
    }
    out
}

/// Sends one schema-checked request and returns its schema-checked result, or the error code.
fn call(bridge: &mut Bridge, schema: &Value, request: &Value) -> Result<Value, String> {
    let name = request["call"].as_str().expect("call");
    let calls = &schema["$defs"]["calls"][name];
    let mut found = problems(request, &schema["$defs"]["request"], schema, "request");
    found.extend(problems(&request["args"], &calls["args"], schema, "args"));
    assert!(found.is_empty(), "request off the schema: {found:?}");
    let answer = bridge.line(&request.to_string());
    if let Some(ok) = answer.get("ok") {
        let mut found = problems(&answer, &schema["$defs"]["ok"], schema, "answer");
        found.extend(problems(&ok["result"], &calls["result"], schema, "result"));
        assert!(found.is_empty(), "answer off the schema: {found:?}");
        assert_eq!(ok["id"], request["id"], "the answer names its request");
        return Ok(ok["result"].clone());
    }
    let found = problems(&answer, &schema["$defs"]["error"], schema, "answer");
    assert!(found.is_empty(), "error off the schema: {found:?}");
    Err(answer["error"]["code"].as_str().expect("code").to_owned())
}

fn schema() -> Value {
    serde_json::from_str(SCHEMA).expect("the schema is JSON")
}

fn fee(amount: u64, high: u64, format: &str, period: &str) -> Value {
    json!({"id": 1, "call": "fee", "args": {"format": format, "amount": amount, "high": high,
        "period": period, "service": if period == "none" { "not_recurring" } else { "one_time" }}})
}

/// FEE-001, FEE-003, FEE-004, REC-002: the tier edges, a range midpoint, a visit quote and the
/// 4-month rule ($150 a month is a $600 basis, $75; $124.99 a month is $25).
#[test]
fn fee_answers_are_the_tiers_and_the_four_month_rule() {
    let (schema, mut bridge) = (schema(), Bridge::start());
    let cases = [
        (fee(49_999, 49_999, "fixed", "none"), 2_500),
        (fee(50_000, 50_000, "fixed", "none"), 7_500),
        (fee(150_000, 150_000, "fixed", "none"), 7_500),
        (fee(150_001, 150_001, "fixed", "none"), 19_900),
        (fee(40_000, 70_000, "range", "none"), 7_500),
        (fee(9_500, 9_500, "visit", "none"), 2_500),
        (fee(15_000, 15_000, "fixed", "monthly"), 7_500),
        (fee(12_499, 12_499, "fixed", "monthly"), 2_500),
        (fee(37_501, 37_501, "fixed", "monthly"), 19_900),
    ];
    for (request, cents) in cases {
        let result = call(&mut bridge, &schema, &request).expect("a fee");
        assert_eq!(result["cents"], cents, "{request}");
    }
    let refused = call(&mut bridge, &schema, &fee(70_000, 40_000, "range", "none"));
    assert_eq!(refused, Err("refused".to_owned()), "a range high below low");
    bridge.finish();
}

/// QH-001, QH-003, QH-006: 7:59 p.m. Arizona goes, 8:00 p.m. waits for 8:00 a.m., an unknown
/// area code is refused.
#[test]
fn quiet_hours_answers_follow_the_window() {
    let (schema, mut bridge) = (schema(), Bridge::start());
    let at = |phone: &str, at: &str| json!({"id": 2, "call": "quiet_hours", "args": {"phone": phone, "at": at}});
    let open = call(
        &mut bridge,
        &schema,
        &at("+19285550100", "2026-01-06T02:59:59Z"),
    );
    assert_eq!(open, Ok(json!({"send": true, "until": null})));
    let held = call(
        &mut bridge,
        &schema,
        &at("+19285550100", "2026-01-06T03:00:00Z"),
    );
    assert_eq!(
        held,
        Ok(json!({"send": false, "until": "2026-01-06T15:00:00Z"}))
    );
    let unknown = call(
        &mut bridge,
        &schema,
        &at("+19995550100", "2026-01-06T18:00:00Z"),
    );
    assert_eq!(unknown, Err("refused".to_owned()));
    bridge.finish();
}

/// JOB-002 and CARD-001: an arrow and a missing arrow; an AC-only license gets an AC card but not
/// a furnace card (LIC-035), and nothing ticked is no card.
#[test]
fn transition_and_card_answers_are_the_apps() {
    let (schema, mut bridge) = (schema(), Bridge::start());
    let mv = |from: &str, to: &str| json!({"id": 3, "call": "transition", "args": {"from": from, "to": to}});
    assert_eq!(
        call(&mut bridge, &schema, &mv("open", "quotes_ready")),
        Ok(json!({"allowed": true}))
    );
    assert_eq!(
        call(&mut bridge, &schema, &mv("open", "done")),
        Ok(json!({"allowed": false}))
    );
    let ac = json!({"category": "hvac_repair", "job_type": "AC not cooling"});
    let furnace = json!({"category": "hvac_repair", "job_type": "furnace repair"});
    let card = |ticked: Value, candidate: &Value| {
        json!({"id": 4, "call": "card", "args": {
        "classes": ["R-39R Air Conditioning and Refrigeration"], "handyman": false,
        "ticked": ticked, "candidates": [candidate], "text": ""}})
    };
    let both = json!([ac.clone(), furnace.clone()]);
    let got = |r: Result<Value, String>| r.expect("a card answer")["card"].clone();
    assert_eq!(
        got(call(&mut bridge, &schema, &card(both.clone(), &ac))),
        true
    );
    assert_eq!(
        got(call(&mut bridge, &schema, &card(both, &furnace))),
        false
    );
    assert_eq!(
        got(call(&mut bridge, &schema, &card(json!([]), &ac))),
        false
    );
    let kinds = json!({"id": 5, "call": "job_types", "args": {"category": "hvac_repair"}});
    let listed = call(&mut bridge, &schema, &kinds).expect("job types");
    assert!(
        listed["job_types"]
            .as_array()
            .is_some_and(|l| l.contains(&json!("AC not cooling")))
    );
    bridge.finish();
}

/// The envelope errors, each still on the schema, and the validator itself refuses an extra field.
#[test]
fn bad_lines_get_schema_errors() {
    let (schema, mut bridge) = (schema(), Bridge::start());
    let lines = [
        ("not json", "bad_json"),
        (r#"{"id": 6, "call": "fee"}"#, "bad_request"),
        (
            r#"{"id": 7, "call": "nothing", "args": {}}"#,
            "unknown_call",
        ),
        (
            r#"{"id": 8, "call": "fee", "args": {"format": "fixed"}}"#,
            "bad_args",
        ),
        (
            r#"{"id": 9, "call": "transition", "args": {"from": "open", "to": "nowhere"}}"#,
            "bad_args",
        ),
    ];
    for (line, code) in lines {
        let answer = bridge.line(line);
        assert!(problems(&answer, &schema["$defs"]["error"], &schema, "answer").is_empty());
        assert_eq!(answer["error"]["code"], code, "{line}");
    }
    let extra = json!({"ok": {"id": 1, "result": {}, "more": 1}});
    assert!(!problems(&extra, &schema["$defs"]["ok"], &schema, "answer").is_empty());
    bridge.finish();
}

/// D-491: the budget is 1 ms a call (measured about 0.05 ms); 2,000 fee calls take under 2 s.
#[test]
fn two_thousand_calls_fit_the_budget() {
    let mut bridge = Bridge::start();
    let line = fee(15_000, 15_000, "fixed", "monthly").to_string();
    let started = Instant::now();
    for _ in 0..2_000 {
        assert!(bridge.line(&line).get("ok").is_some());
    }
    let spent = started.elapsed();
    assert!(spent.as_millis() < 2_000, "2,000 calls took {spent:?}");
    bridge.finish();
}
