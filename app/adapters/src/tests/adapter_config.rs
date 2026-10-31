//! Adapter mode, base URLs, the fixed masked pool, derived keys, and the Twilio
//! clients over HTTP against `stand_ins/server.py` (SEC-018, SEC-032, CALL-017;
//! TD-SEC-03, TD-CALL-01, TD-SEC-13).
//!
//! Pairs only, never `std::env::set_var` (R16). Every number is a fake
//! 555-style test number; the stand-in Twilio SID is `AC` and 32 zeros.

use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use askhava_core::api::{Settings, SettingsError};
use secrecy::SecretString;

use crate::api::{
    AdapterError, AdapterMode, CodeChannel, IdempotencyKey, SmsAdapter, TwilioConfig,
    VerifyAdapter, VoiceAdapter, twilio_clients,
};

const SID: &str = "AC00000000000000000000000000000000";
const VA: &str = "VA00000000000000000000000000000001";
const FROM: &str = "+15005550006";
const TO: &str = "+15005550010";

fn pool(n: usize) -> String {
    (0..n)
        .map(|i| format!("+1500555{:04}", 100 + i))
        .collect::<Vec<_>>()
        .join(",")
}

fn pairs(env: &str, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let numbers = pool(10);
    let mut all = vec![
        ("ASKHAVA_APP_ENV", env),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        ("ASKHAVA_TWILIO_ACCOUNT_SID", SID),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        ("ASKHAVA_TWILIO_VERIFY_SERVICE_SID", VA),
        ("ASKHAVA_MASKED_NUMBERS", numbers.as_str()),
    ];
    all.retain(|(k, _)| !extra.iter().any(|(e, _)| e == k));
    all.extend_from_slice(extra);
    all.into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

fn config(env: &str, extra: &[(&str, &str)]) -> Result<TwilioConfig, SettingsError> {
    let pairs = pairs(env, extra);
    TwilioConfig::from_settings(&Settings::from_pairs(pairs.clone()).unwrap(), pairs)
}

fn key(action: &str, id: &str) -> IdempotencyKey {
    IdempotencyKey::derive(&SecretString::from("fake-id-secret"), action, &[id])
}

/// TD-SEC-13; SEC-032, SEC-002: outside live the default is the stand-in server, and the base URL setting overrides it.
#[test]
fn base_urls_come_from_settings() {
    let dev = config("dev", &[]).unwrap();
    assert_eq!(dev.mode(), AdapterMode::StandIn);
    assert_eq!(dev.api_base(), "http://127.0.0.1:4010/twilio");
    let set = config(
        "ci",
        &[("ASKHAVA_TWILIO_BASE_URL", "http://127.0.0.1:9/twilio/")],
    )
    .unwrap();
    assert_eq!(set.api_base(), "http://127.0.0.1:9/twilio");
    let sandbox = config("dev", &[("ASKHAVA_ADAPTER_MODE_TWILIO", "sandbox")]).unwrap();
    assert_eq!(sandbox.mode(), AdapterMode::Sandbox);
    assert_eq!(sandbox.verify_base(), "https://verify.twilio.com");
}

/// TD-SEC-13; SEC-002: `live` mode outside live, and a stand-in or sandbox mode or a stand-in URL in live, stop startup.
#[test]
fn mode_and_stand_in_urls_are_checked_against_the_environment() {
    let live_mode = config("dev", &[("ASKHAVA_ADAPTER_MODE_TWILIO", "live")]);
    assert!(matches!(
        live_mode,
        Err(SettingsError::LiveKeyOutsideLive(_))
    ));
    let bad = config("dev", &[("ASKHAVA_ADAPTER_MODE_TWILIO", "fake")]);
    assert!(matches!(
        bad,
        Err(SettingsError::Invalid("ASKHAVA_ADAPTER_MODE_TWILIO"))
    ));
    let live = live_settings();
    let stand_in = TwilioConfig::from_settings(&live, [mode("stand_in")]);
    assert!(matches!(stand_in, Err(SettingsError::StandInInLive(_))));
    let sandbox = TwilioConfig::from_settings(&live, [mode("sandbox")]);
    assert!(matches!(
        sandbox,
        Err(SettingsError::TestKeyInLive("ASKHAVA_ADAPTER_MODE_TWILIO"))
    ));
    let local = TwilioConfig::from_settings(
        &live,
        [(
            "ASKHAVA_TWILIO_BASE_URL".to_owned(),
            "http://127.0.0.1:4010/twilio".to_owned(),
        )],
    );
    assert!(matches!(
        local,
        Err(SettingsError::StandInInLive("ASKHAVA_TWILIO_BASE_URL"))
    ));
}

fn mode(value: &str) -> (String, String) {
    ("ASKHAVA_ADAPTER_MODE_TWILIO".to_owned(), value.to_owned())
}

/// A valid live `Settings` with fake allow-listed identifiers (SEC-036).
fn live_settings() -> Settings {
    let list = |tag: &str| {
        format!("twilio:AC{tag},stripe:acct_{tag},resend:{tag}.example.com,bucket:{tag}")
    };
    let (test, live) = (list("testfake"), list("livefake"));
    let pairs = [
        ("ASKHAVA_APP_ENV", "live"),
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://fake@db.example.com/fake",
        ),
        ("ASKHAVA_ALLOW_LIST_TEST", test.as_str()),
        ("ASKHAVA_ALLOW_LIST_LIVE", live.as_str()),
        ("ASKHAVA_TWILIO_ACCOUNT_SID", "AClivefake"),
        ("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_livefake"),
        ("ASKHAVA_RESEND_DOMAIN", "livefake.example.com"),
        ("ASKHAVA_STORAGE_BUCKET", "livefake"),
    ];
    Settings::from_pairs(pairs.map(|(k, v)| (k.to_owned(), v.to_owned()))).unwrap()
}

/// TD-CALL-01; CALL-017, D-160: the pool is a fixed list from settings, 10 numbers unless sized otherwise.
#[test]
fn masked_pool_is_a_fixed_list_sized_by_settings() {
    assert_eq!(crate::api::DEFAULT_POOL_SIZE, 10);
    assert_eq!(config("dev", &[]).unwrap().pool().numbers().len(), 10);
    let nine = pool(9);
    let short = config("dev", &[("ASKHAVA_MASKED_NUMBERS", nine.as_str())]);
    assert!(matches!(
        short,
        Err(SettingsError::Invalid("ASKHAVA_MASKED_NUMBERS"))
    ));
    let sized = config(
        "dev",
        &[
            ("ASKHAVA_MASKED_POOL_SIZE", "9"),
            ("ASKHAVA_MASKED_NUMBERS", nine.as_str()),
        ],
    );
    assert_eq!(sized.unwrap().pool().numbers().len(), 9);
    let twice = format!("{},+15005550100", pool(9));
    let dup = config("dev", &[("ASKHAVA_MASKED_NUMBERS", twice.as_str())]);
    assert!(matches!(dup, Err(SettingsError::Invalid(_))));
}

/// TD-CALL-01; CALL-017: the voice adapter has no method that buys, provisions or releases a number.
#[test]
fn voice_adapter_has_no_buy_number_method() {
    let source = include_str!("../voice.rs").to_lowercase();
    for banned in [
        "incomingphonenumbers",
        "availablephonenumbers",
        "fn buy",
        "fn purchase",
        "fn provision",
        "fn release",
    ] {
        assert!(!source.contains(banned), "voice.rs mentions {banned}");
    }
}

/// TD-SEC-03; SEC-018: every trait method that acts outside takes an idempotency key (D-596).
#[test]
fn every_outside_action_takes_an_idempotency_key() {
    let read_only = ["fetch_", "get_", "list_", "read_", "check_", "lookup_"];
    for source in [
        include_str!("../sms.rs"),
        include_str!("../voice.rs"),
        include_str!("../verify.rs"),
    ] {
        let traits = source
            .split("pub trait ")
            .skip(1)
            .map(|t| t.split("\n}\n").next().unwrap_or_default());
        for body in traits {
            for method in body.split("fn ").skip(1) {
                let signature = method.split(')').next().unwrap_or_default();
                let exempt = read_only.iter().any(|p| method.starts_with(p));
                assert!(
                    exempt || signature.contains("idempotency"),
                    "missing key: fn {signature}"
                );
            }
        }
    }
}

/// TD-SEC-03; SEC-018: the key is stable for the same record, differs per record and action, and holds no input text.
#[test]
fn idempotency_key_is_derived_from_what_it_identifies() {
    assert_eq!(key("sms.send", "outbox-1"), key("sms.send", "outbox-1"));
    assert_ne!(key("sms.send", "outbox-1"), key("sms.send", "outbox-2"));
    assert_ne!(key("sms.send", "outbox-1"), key("call.open", "outbox-1"));
    let joined = IdempotencyKey::derive(&SecretString::from("fake-id-secret"), "a", &["bc"]);
    assert_ne!(
        joined,
        IdempotencyKey::derive(&SecretString::from("fake-id-secret"), "ab", &["c"])
    );
    let text = key("sms.send", "outbox-1");
    assert_eq!(text.as_str().len(), 64);
    assert!(!text.as_str().contains("outbox"));
}

/// The stand-in server on a port it bound itself, killed on drop. Shared with
/// `ssrf_guard.rs` so every test in this binary starts it the same way.
pub(super) struct StandIn(Child, pub(super) u16);

impl Drop for StandIn {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// How many free ports `stand_in` tries before giving up.
const STAND_IN_STARTS: usize = 5;

/// How long one start may take to print its "listening" line. The old connect
/// probe allowed 5 seconds; the server normally prints within a fraction of a
/// second, so this only bounds a child that neither prints nor exits.
const STAND_IN_READY: Duration = Duration::from_secs(10);

/// Starts `python3 -m stand_ins.server` and returns once that child, and no
/// other process, is listening on the port. A free port is only free when it
/// is looked up: before the child binds it another test can take it, the
/// child then exits with "Address already in use", and a plain connect probe
/// would reach the other test's socket. So readiness is the child's own
/// "listening" line, printed after its bind, and a child that exits, prints
/// something else or stays silent past `STAND_IN_READY` is killed and started
/// again on a new port; after `STAND_IN_STARTS` failed starts the panic shows
/// what each child said (D-1086).
pub(super) fn stand_in() -> StandIn {
    let mut failures = Vec::new();
    for _ in 0..STAND_IN_STARTS {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut command = Command::new("python3");
        command
            .args(["-m", "stand_ins.server", "--port", &port.to_string()])
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        match start_and_wait(command, port, STAND_IN_READY) {
            Ok(server) => return server,
            Err(failure) => failures.push(failure),
        }
    }
    panic!("stand-in server did not start: {failures:#?}");
}

/// Spawns `command` and waits up to `within` for its first stdout line to be
/// `stand-ins listening on http://127.0.0.1:<port>`. The line is read on a
/// helper thread, which then drains stdout so the child never writes to a
/// closed pipe. On any other outcome the child is killed and reaped and the
/// error says what it printed on stdout and stderr (D-1086).
fn start_and_wait(mut command: Command, port: u16, within: Duration) -> Result<StandIn, String> {
    let spawned = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = spawned.map_err(|error| format!("spawn failed: {error}"))?;
    let stdout = child.stdout.take().unwrap();
    let (sender, first_line) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        let _ = sender.send(line);
        let _ = std::io::copy(&mut reader, &mut std::io::sink());
    });
    let mut server = StandIn(child, port);
    let ready = format!("stand-ins listening on http://127.0.0.1:{port}");
    let said = match first_line.recv_timeout(within) {
        Ok(line) if line.trim_end() == ready => {
            pass_stderr_through(&mut server.0);
            return Ok(server);
        }
        Ok(line) => format!("stdout {line:?}"),
        Err(_) => format!("no line within {within:?}"),
    };
    let _ = server.0.kill();
    let _ = server.0.wait();
    let mut stderr = String::new();
    if let Some(mut pipe) = server.0.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    Err(format!("port {port}: {said}; stderr {stderr:?}"))
}

/// Copies a running server's stderr to the test's own, as an inherited
/// stderr would, so a traceback from a request still shows in the test log.
fn pass_stderr_through(child: &mut Child) {
    if let Some(mut pipe) = child.stderr.take() {
        std::thread::spawn(move || std::io::copy(&mut pipe, &mut std::io::stderr()));
    }
}

/// D-1086: a child that never prints its "listening" line is killed after the
/// wait and reported, so a stuck start cannot hang the test run.
#[test]
fn a_silent_stand_in_start_is_killed_and_reported() {
    let mut command = Command::new("python3");
    command.args(["-c", "import time; time.sleep(60)"]);
    let started = std::time::Instant::now();
    let failure = start_and_wait(command, 1, Duration::from_millis(300)).err();
    assert!(failure.unwrap().contains("no line within"));
    assert!(started.elapsed() < Duration::from_secs(30));
}

/// D-1086: a child that exits before its "listening" line (as on "Address
/// already in use") is a failed start whose stderr is kept for the panic.
#[test]
fn a_stand_in_that_exits_early_is_reported_with_its_stderr() {
    let mut command = Command::new("python3");
    command.args(["-c", "import sys; sys.exit('fake bind failure')"]);
    let failure = start_and_wait(command, 1, STAND_IN_READY).err().unwrap();
    assert!(failure.contains("stdout \"\""));
    assert!(failure.contains("fake bind failure"));
}

fn stand_in_config(port: u16) -> TwilioConfig {
    let base = format!("http://127.0.0.1:{port}/twilio");
    config(
        "ci",
        &[
            ("ASKHAVA_TWILIO_BASE_URL", base.as_str()),
            ("ASKHAVA_TWILIO_VERIFY_BASE_URL", base.as_str()),
        ],
    )
    .unwrap()
}

async fn get_json(url: &str) -> serde_json::Value {
    let request = reqwest::Client::new().get(url);
    let response = request.basic_auth(SID, Some("fake-auth-token")).send();
    response.await.unwrap().json().await.unwrap()
}

/// TD-SEC-13, TD-SEC-03; SEC-032, SEC-018: a text goes over HTTP carrying its derived key, and a vendor refusal keeps only codes.
#[tokio::test]
async fn texting_client_talks_to_the_stand_in_over_http() {
    let server = stand_in();
    let (sms, _, _) = twilio_clients(&stand_in_config(server.1)).unwrap();
    let sent = sms
        .send_text(&key("sms.send", "outbox-1"), TO, FROM, "Ask Hava: test")
        .await
        .unwrap();
    assert!(sent.sid.starts_with("SM"));
    let log = format!(
        "http://127.0.0.1:{}/twilio/2010-04-01/Accounts/{SID}/Messages.json?MessageKey={}",
        server.1,
        key("sms.send", "outbox-1").as_str()
    );
    assert_eq!(
        get_json(&log).await["messages"][0]["sid"],
        sent.sid.as_str()
    );
    let refused = sms
        .send_text(
            &key("sms.send", "outbox-2"),
            "+15005550001",
            FROM,
            "Ask Hava: test",
        )
        .await;
    let expected = AdapterError::Vendor {
        status: 400,
        code: Some(21211),
    };
    assert_eq!(refused, Err(expected));
}

/// TD-CALL-01, TD-SEC-13; CALL-017, SEC-032: a call opens with a pool number as caller ID and ends; a bad SID never reaches a URL.
#[tokio::test]
async fn voice_client_dials_from_the_pool_over_http() {
    let server = stand_in();
    let config = stand_in_config(server.1);
    let (_, voice, _) = twilio_clients(&config).unwrap();
    let number = &config.pool().numbers()[0];
    let call = voice
        .open_call(&key("call.open", "session-1"), TO, number)
        .await
        .unwrap();
    let calls = format!(
        "http://127.0.0.1:{}/twilio/2010-04-01/Accounts/{SID}/Calls.json",
        server.1
    );
    assert_eq!(get_json(&calls).await["calls"][0]["from"], number.as_str());
    let end_key = key("call.end", "session-1");
    let ended = voice.end_call(&end_key, &call.sid).await.unwrap();
    assert_eq!(ended.status, "completed");
    let bad = voice.end_call(&key("call.end", "x"), "../x").await;
    assert_eq!(bad, Err(AdapterError::InvalidId));
}

/// TD-SEC-13; SEC-032, SEC-018: Verify sends a code under its key and checks a wrong and the right code over HTTP.
#[tokio::test]
async fn verify_client_sends_and_checks_over_http() {
    let server = stand_in();
    let (_, _, verify) = twilio_clients(&stand_in_config(server.1)).unwrap();
    let sid = verify
        .send_code(&key("verify.send", "attempt-1"), TO, CodeChannel::Sms)
        .await
        .unwrap();
    assert!(sid.starts_with("VE"));
    assert!(!verify.check_code(TO, "not-it").await.unwrap());
    let code_url = format!(
        "http://127.0.0.1:{}/_stand_in/twilio/code?to=%2B15005550010",
        server.1
    );
    let code = get_json(&code_url).await;
    let right = verify.check_code(TO, code["code"].as_str().unwrap());
    assert!(right.await.unwrap());
}

/// SEC-018: a refused connection is `NotSent`, so the caller knows a retry cannot duplicate the text.
#[tokio::test]
async fn unreachable_vendor_is_not_sent() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (sms, _, _) = twilio_clients(&stand_in_config(port)).unwrap();
    let result = sms
        .send_text(&key("sms.send", "outbox-3"), TO, FROM, "Ask Hava: test")
        .await;
    assert_eq!(result, Err(AdapterError::NotSent));
}
