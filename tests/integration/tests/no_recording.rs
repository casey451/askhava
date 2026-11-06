//! Masked calls are never recorded, and each finished call is logged once (TD-CALL-03: CALL-006;
//! TD-CALL-04's log cases: CALL-007, CALL-008; D-141, D-1689 to D-1691). A fake vendor catches the
//! voice adapter's real requests; the log is written and read on a fresh database. Fake numbers
//! (555-01xx and the 500-555 test range), fake SIDs and IDs only.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::Receiver;

use askhava_adapters::api::{IdempotencyKey, TwilioConfig, VoiceAdapter, twilio_clients};
use askhava_calls::api::{
    AnsweredBy, Caller, Connected, FinishedCall, assign_number, call_log, dial_twiml,
    log_finished_call,
};
use askhava_core::api::Settings;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use sqlx::types::Uuid;

const HER_PHONE: &str = "+19285550178";
const SID: &str = "CA00000000000000000000000000000078";

/// A Twilio config in `ci` whose API base is `base`, with one fake pool number.
fn config(base: &str) -> TwilioConfig {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "ci"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_TWILIO_BASE_URL", base),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", base),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    TwilioConfig::from_settings(&settings, pairs).expect("config")
}

/// A fake vendor that answers `count` requests with a call and hands back each whole request.
fn recording_vendor(count: usize) -> (u16, Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("port").port();
    let (send, requests) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten().take(count) {
            let mut reader = BufReader::new(stream);
            let (mut head, mut line) = (String::new(), String::new());
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                head.push_str(&line);
                line.clear();
            }
            let length = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap_or(0))
                })
                .unwrap_or(0);
            let mut body = vec![0; length];
            let _ = reader.read_exact(&mut body);
            let reply = format!("{{\"sid\":\"{SID}\",\"status\":\"queued\"}}");
            let answer = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
            let _ = reader.into_inner().write_all(answer.as_bytes());
            let _ = send.send(head + &String::from_utf8_lossy(&body));
        }
    });
    (port, requests)
}

/// TD-CALL-03 (CALL-006): neither request the voice adapter sends, opening or ending a call, names
/// recording, so the provider's default (never record) holds.
#[tokio::test]
async fn td_call_03_no_call_request_asks_for_recording() {
    let (port, requests) = recording_vendor(2);
    let config = config(&format!("http://127.0.0.1:{port}/twilio"));
    let (_, voice, _) = twilio_clients(&config).expect("clients");
    let key = IdempotencyKey::derive(&SecretString::from("fake-id-secret"), "call.open", &["l"]);
    let from = &config.pool().numbers()[0];
    voice.open_call(&key, HER_PHONE, from).await.expect("open");
    voice.end_call(&key, SID).await.expect("end");
    for _ in 0..2 {
        let request = requests.recv().expect("request").to_ascii_lowercase();
        assert!(request.starts_with("post /twilio/"), "{request}");
        assert!(
            !request.contains("record"),
            "a call request names recording"
        );
        assert!(!request.contains("trim="), "a call request names recording");
    }
}

/// TD-CALL-03 (CALL-006): the masked call's TwiML says `do-not-record`, never `<Record>` or any
/// other record mode, and refuses a number that is not E.164 rather than write it into the XML.
#[test]
fn td_call_03_the_twiml_never_records() {
    let config = config("http://127.0.0.1:9/twilio");
    let from = &config.pool().numbers()[0];
    let twiml = dial_twiml(from, &SecretString::from(HER_PHONE)).expect("twiml");
    let xml = twiml.expose_secret();
    assert!(xml.contains("<Dial callerId=\"+15005550100\" record=\"do-not-record\">"));
    assert!(xml.contains("<Number machineDetection=\"Enable\">+19285550178</Number>"));
    assert_eq!(xml.matches("record=").count(), 1, "{xml}");
    assert!(!xml.contains("<Record") && !xml.contains("Recording"));
    for bad in ["9285550178", "+1928555017<", "+1", "+1234567890123456", ""] {
        assert!(
            dial_twiml(from, &SecretString::from(bad)).is_none(),
            "{bad}"
        );
    }
}

/// TD-CALL-03 (CALL-006): no `calls` or voice source names a recording verb, attribute or callback.
#[test]
fn td_call_03_no_source_asks_for_recording() {
    let sources = [
        include_str!("../../../app/adapters/src/voice.rs"),
        include_str!("../../../app/calls/src/handlers.rs"),
        include_str!("../../../app/calls/src/routing.rs"),
        include_str!("../../../app/calls/src/service.rs"),
    ];
    let banned = [
        "<record",
        "recordingstatuscallback",
        "\"record\"",
        "record-from",
        "recordingchannels",
    ];
    for source in sources.map(str::to_lowercase) {
        for word in banned {
            assert!(
                !source.contains(word),
                "a calls or voice source names {word}"
            );
        }
    }
}

/// A finished call on `line`; calls end in the order of their SID's last digit.
fn call(
    line: Uuid,
    sid: &str,
    (caller, answered): (Caller, AnsweredBy),
    secs: i64,
) -> FinishedCall {
    FinishedCall {
        line,
        call_sid: sid.to_owned(),
        caller,
        answered,
        length: SignedDuration::from_secs(secs),
        ended_at: Timestamp::from_second(1_793_000_000 + i64::from(sid.as_bytes()[33]) * 60)
            .expect("instant"),
    }
}

/// TD-CALL-04 (CALL-007, CALL-008, D-141): every finished call is logged once with who called,
/// who answered, its length and the 45-second mark; a repeated callback logs nothing; the log is
/// append-only for the app role.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_04_each_finished_call_is_logged_once(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let pool = config("http://127.0.0.1:9/twilio").pool().clone();
    let job = Connected {
        job: Uuid::new_v4(),
        homeowner: Uuid::new_v4(),
        business: Uuid::new_v4(),
    };
    let line = assign_number(
        &mut db,
        &pool,
        job,
        Timestamp::from_second(1_793_000_000).expect("t"),
    )
    .await
    .expect("line");
    let calls = [
        call(
            line.id(),
            "CA00000000000000000000000000000001",
            (Caller::Business, AnsweredBy::Person),
            5,
        ),
        call(
            line.id(),
            "CA00000000000000000000000000000002",
            (Caller::Business, AnsweredBy::Machine),
            90,
        ),
        call(
            line.id(),
            "CA00000000000000000000000000000003",
            (Caller::Homeowner, AnsweredBy::Person),
            45,
        ),
        call(
            line.id(),
            "CA00000000000000000000000000000004",
            (Caller::Business, AnsweredBy::Nobody),
            0,
        ),
    ];
    for c in &calls {
        assert!(log_finished_call(&mut db, c).await.expect("log"));
    }
    assert!(!log_finished_call(&mut db, &calls[0]).await.expect("repeat"));
    let log = call_log(&mut db, line.id()).await.expect("read");
    assert_eq!(log.len(), 4);
    for (got, want) in log.iter().zip(&calls) {
        assert_eq!(
            (got.caller, got.answered, got.length),
            (want.caller, want.answered, want.length)
        );
        assert_eq!(
            got.reached_45s,
            want.length >= SignedDuration::from_secs(45)
        );
        assert_eq!(got.ended_at, want.ended_at);
    }
    let stray = call(
        Uuid::new_v4(),
        "CA00000000000000000000000000000005",
        (Caller::Business, AnsweredBy::Person),
        5,
    );
    assert!(log_finished_call(&mut db, &stray).await.is_err());
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    for denied in [
        "UPDATE calls.call_log SET answered_by = 'person'",
        "DELETE FROM calls.call_log",
    ] {
        let error = sqlx::query(denied)
            .execute(&mut *db)
            .await
            .expect_err(denied);
        assert!(error.to_string().contains("permission denied"), "{denied}");
    }
}
