//! Unit tests for the Twilio media delete (INT-044, SEC-018, SEC-032; T794, D-5251): a recording
//! local server stands in for Twilio, so a test sees every request that left, or that none did.
//! Fake SIDs and numbers only.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use askhava_core::api::Settings;
use secrecy::SecretString;

use crate::api::{AdapterError, IdempotencyKey, SmsMediaDelete, TwilioConfig, twilio_clients};

const AC: &str = "AC00000000000000000000000000000000";
const MM: &str = "MM00000000000000000000000000000001";
const ME: &str = "ME00000000000000000000000000000001";

/// Every request line and header block the fake Twilio received.
type Seen = Arc<Mutex<Vec<String>>>;

/// A fake Twilio on a free port that answers every request with `status` and `body`.
fn fake_twilio(status: &'static str, body: &'static str) -> (u16, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Seen::default();
    let record = Arc::clone(&seen);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut head = String::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            while reader.read_line(&mut head).is_ok_and(|n| n > 2) {}
            record.lock().unwrap().push(head);
            let answer = format!(
                "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    (port, seen)
}

fn client(port: u16) -> impl SmsMediaDelete {
    let base = format!("http://127.0.0.1:{port}/twilio");
    let pool: Vec<String> = (100..110).map(|n| format!("+1500555{n:04}")).collect();
    let pool = pool.join(",");
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "ci"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        ("ASKHAVA_TWILIO_ACCOUNT_SID", AC),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_NUMBERS", pool.as_str()),
        ("ASKHAVA_TWILIO_BASE_URL", base.as_str()),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", base.as_str()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).unwrap();
    let config = TwilioConfig::from_settings(&settings, pairs).unwrap();
    twilio_clients(&config).unwrap().0
}

fn key() -> IdempotencyKey {
    IdempotencyKey::derive(&SecretString::from("fake-id-secret"), "media.delete", &[ME])
}

/// INT-044, SEC-018, SEC-032: a delete is one DELETE on the account's Media path with the key and Basic auth; 204 is `Ok`.
#[tokio::test]
async fn a_media_delete_sends_one_delete_and_204_is_ok() {
    let (port, seen) = fake_twilio("204 No Content", "");
    let answer = client(port).delete_message_media(&key(), MM, ME).await;
    assert_eq!(answer, Ok(()));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let path = format!("/twilio/2010-04-01/Accounts/{AC}/Messages/{MM}/Media/{ME}.json");
    assert!(seen[0].starts_with(&format!("DELETE {path} HTTP/1.1\r\n")));
    let head = seen[0].to_ascii_lowercase();
    assert!(head.contains(&format!("idempotency-key: {}", key().as_str())));
    assert!(head.contains("authorization: basic "));
}

/// INT-044, SEC-018: media Twilio no longer has (404) is already deleted, so a retry is `Ok`.
#[tokio::test]
async fn media_already_gone_is_ok() {
    let (port, _) = fake_twilio("404 Not Found", r#"{"code": 20404, "status": 404}"#);
    let answer = client(port).delete_message_media(&key(), MM, ME).await;
    assert_eq!(answer, Ok(()));
}

/// SEC-032, PRIV-016: any other answer is `Vendor` with codes only.
#[tokio::test]
async fn another_answer_is_a_vendor_error() {
    let (port, _) = fake_twilio("401 Unauthorized", r#"{"code": 20003, "status": 401}"#);
    let answer = client(port).delete_message_media(&key(), MM, ME).await;
    let expected = AdapterError::Vendor {
        status: 401,
        code: Some(20003),
    };
    assert_eq!(answer, Err(expected));
}

/// SEC-032: a malformed message or media SID is `InvalidId` and no request leaves.
#[tokio::test]
async fn a_malformed_sid_sends_nothing() {
    let (port, seen) = fake_twilio("204 No Content", "");
    let sms = client(port);
    let bad = [
        ("MM0000000000000000000000000000000", ME),
        ("CA00000000000000000000000000000001", ME),
        ("MM00000000000000000000000000000/..", ME),
        (MM, "MM00000000000000000000000000000001"),
        (MM, "ME0000000000000000000000000000001%"),
        (MM, ""),
    ];
    for (message, media) in bad {
        let answer = sms.delete_message_media(&key(), message, media).await;
        assert_eq!(answer, Err(AdapterError::InvalidId), "{message} {media}");
    }
    assert!(seen.lock().unwrap().is_empty());
}

/// INT-044, SEC-032: a Twilio media URL is deleted at the configured account root, whatever host it names.
#[tokio::test]
async fn a_media_url_is_deleted_at_the_configured_root() {
    let (port, seen) = fake_twilio("204 No Content", "");
    let url = format!("https://api.twilio.com/2010-04-01/Accounts/{AC}/Messages/{MM}/Media/{ME}");
    assert_eq!(client(port).delete_media_at(&key(), &url).await, Ok(()));
    let path = format!("/twilio/2010-04-01/Accounts/{AC}/Messages/{MM}/Media/{ME}.json");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].starts_with(&format!("DELETE {path} HTTP/1.1\r\n")));
}

/// SEC-032: a URL of another shape, another account or with a query is `InvalidId` and no request leaves.
#[tokio::test]
async fn a_url_of_another_shape_sends_nothing() {
    let (port, seen) = fake_twilio("204 No Content", "");
    let sms = client(port);
    let root = "https://api.twilio.com/2010-04-01/Accounts";
    let other = "AC0000000000000000000000000000000Z";
    let bad = [
        format!("{root}/{other}/Messages/{MM}/Media/{ME}"),
        format!("{root}/{AC}/Messages/{MM}/Media/{ME}.json"),
        format!("{root}/{AC}/Messages/{MM}/Media/{ME}?x=1"),
        format!("{root}/{AC}/Messages/{MM}/Media/{ME}#x"),
        format!("{root}/{AC}/Messages/{MM}/Media/{ME}/extra"),
        format!("{root}/{AC}/Messages/{MM}/Media"),
        format!("{root}/{AC}/Calls/{MM}/Media/{ME}"),
        format!("/2010-04-01/Accounts/{AC}/Messages/{MM}/Media/{ME}"),
        format!("file:///2010-04-01/Accounts/{AC}/Messages/{MM}/Media/{ME}"),
        "not a url".to_owned(),
    ];
    for url in bad {
        let answer = sms.delete_media_at(&key(), &url).await;
        assert_eq!(answer, Err(AdapterError::InvalidId), "{url}");
    }
    assert!(seen.lock().unwrap().is_empty());
}

/// INT-044, SEC-032: against the stand-in, a picture text's media URL is deleted and then answers 404; a repeat is `Ok`.
#[tokio::test]
async fn the_stand_in_media_is_gone_after_the_delete() {
    let server = super::adapter_config::stand_in();
    let text = serde_json::json!({"from": "+19285550142", "to": "+15005550006", "body": "", "media": ["image/jpeg"]});
    let inbound = format!("http://127.0.0.1:{}/_stand_in/twilio/inbound", server.1);
    let http = reqwest::Client::new();
    let answer: serde_json::Value = http
        .post(&inbound)
        .json(&text)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let url = answer["params"]["MediaUrl0"].as_str().unwrap().to_owned();
    let sms = client(server.1);
    assert_eq!(sms.delete_media_at(&key(), &url).await, Ok(()));
    let fetched = http
        .get(&url)
        .basic_auth(AC, Some("fake-auth-token"))
        .send()
        .await
        .unwrap();
    assert_eq!(fetched.status(), reqwest::StatusCode::NOT_FOUND);
    assert_eq!(sms.delete_media_at(&key(), &url).await, Ok(()));
}
