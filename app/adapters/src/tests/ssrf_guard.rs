//! The guarded fetch of untrusted URLs, the AI stand-in-only client, and the
//! other vendor clients against `stand_ins/server.py` (SEC-041, AI-009,
//! SEC-018; TD-SEC-20, TD-AI-01).
//!
//! A local server on 127.0.0.1 plays the remote host under the test-only
//! loopback allowance; private, link-local and metadata addresses stay refused
//! under it. All data is fake.

use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use secrecy::SecretString;

use crate::api::{
    AdapterError, AiAdapter, Charge, Email, EmailAdapter, FetchError, FetchPolicy, IdempotencyKey,
    LocalStorage, PaymentsAdapter, ResendEmail, StandInAi, StorageAdapter, StripePayments,
    fetch_untrusted, is_public_address,
};

const MB: usize = 1024 * 1024;

fn answer(path: &str) -> String {
    let redirect =
        |to: &str| format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\n\r\n");
    match path {
        "/to-metadata" => redirect("http://169.254.169.254/latest/meta-data/"),
        "/to-private" => redirect("http://10.0.0.1/"),
        "/off-host" => redirect("http://media.example.com/photo.jpg"),
        "/hop" => redirect("/ok"),
        "/loop" => redirect("/loop"),
        _ => "HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nfake media".to_owned(),
    }
}

fn reply(stream: TcpStream) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let _ = reader.read_line(&mut line);
    let path = line.split(' ').nth(1).unwrap_or("/").to_owned();
    while reader.read_line(&mut line).is_ok_and(|n| n > 2) {}
    let mut stream = reader.into_inner();
    if path == "/hang" {
        std::thread::sleep(Duration::from_secs(5));
    } else if path == "/big" {
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
        let chunk = vec![b'x'; 64 * 1024];
        for _ in 0..(30 * MB / chunk.len()) {
            if stream.write_all(&chunk).is_err() {
                return;
            }
        }
    } else {
        let _ = stream.write_all(answer(&path).as_bytes());
    }
}

fn remote_host() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || reply(stream));
        }
    });
    port
}

fn test_policy(timeout_ms: u64) -> FetchPolicy {
    let hosts = ["127.0.0.1", "10.0.0.1", "169.254.169.254"];
    FetchPolicy::new(&hosts, 10 * MB, Duration::from_millis(timeout_ms)).with_loopback_for_tests()
}

/// TD-SEC-20; SEC-041: private, loopback, link-local, metadata and shared ranges are refused, also inside IPv6.
#[test]
fn only_public_addresses_pass() {
    let refused = [
        "169.254.169.254",
        "10.0.0.1",
        "127.0.0.1",
        "192.168.1.1",
        "172.16.0.1",
        "100.100.100.200",
        "0.0.0.0",
        "::1",
        "::ffff:10.0.0.1",
        "fe80::1",
        "fd00:ec2::254",
        "64:ff9b::a9fe:a9fe",
    ];
    for ip in refused {
        assert!(!is_public_address(ip.parse::<IpAddr>().unwrap()), "{ip}");
    }
    for ip in ["8.8.8.8", "2606:4700:4700::1111"] {
        assert!(is_public_address(ip.parse::<IpAddr>().unwrap()), "{ip}");
    }
}

/// TD-SEC-20; SEC-041: a redirect to the metadata or a private address is refused after resolution.
#[tokio::test]
async fn redirect_to_metadata_or_private_address_is_refused() {
    let port = remote_host();
    for path in ["to-metadata", "to-private"] {
        let url = format!("http://127.0.0.1:{port}/{path}");
        let got = fetch_untrusted(&test_policy(2000), &url).await;
        assert_eq!(got, Err(FetchError::AddressRefused), "{path}");
    }
}

/// TD-SEC-20; SEC-041: a redirect off the allowed hosts is not followed; one on them is, and loops stop.
#[tokio::test]
async fn redirect_off_the_allowed_hosts_is_not_followed() {
    let port = remote_host();
    let fetch = |path: &str| {
        let url = format!("http://127.0.0.1:{port}/{path}");
        async move { fetch_untrusted(&test_policy(2000), &url).await }
    };
    assert_eq!(fetch("off-host").await, Err(FetchError::HostNotAllowed));
    assert_eq!(fetch("hop").await, Ok(b"fake media".to_vec()));
    assert_eq!(fetch("loop").await, Err(FetchError::TooManyRedirects));
}

/// TD-SEC-20; SEC-041: media only from Twilio's hosts, over https; a name resolving to loopback is refused.
#[tokio::test]
async fn media_from_a_non_twilio_host_is_refused() {
    let media = FetchPolicy::twilio_media();
    for url in [
        "https://media.example.com/photo.jpg",
        "https://evilapi.twilio.com/photo.jpg",
        "https://api.twilio.com.example.com/photo.jpg",
        "http://api.twilio.com/2010-04-01/Accounts/AC0/Messages/MM0/Media/ME0",
    ] {
        assert_eq!(
            fetch_untrusted(&media, url).await,
            Err(FetchError::HostNotAllowed),
            "{url}"
        );
    }
    let local = FetchPolicy::new(&["localhost"], MB, Duration::from_secs(2));
    let got = fetch_untrusted(&local, "https://localhost:9/photo.jpg").await;
    assert_eq!(got, Err(FetchError::AddressRefused));
}

/// TD-SEC-20; SEC-041: a 30 MB body is cut at the 10 MB cap; a hanging host times out.
#[tokio::test]
async fn big_bodies_are_cut_and_hanging_hosts_time_out() {
    let port = remote_host();
    let big = format!("http://127.0.0.1:{port}/big");
    assert_eq!(
        fetch_untrusted(&test_policy(10_000), &big).await,
        Err(FetchError::TooLarge)
    );
    let hang = format!("http://127.0.0.1:{port}/hang");
    assert_eq!(
        fetch_untrusted(&test_policy(300), &hang).await,
        Err(FetchError::TimedOut)
    );
}

/// TD-AI-01; AI-009: the only AI client accepts only a loopback stand-in URL and holds no key.
#[test]
fn ai_client_reaches_only_the_stand_in() {
    for url in [
        "https://api.anthropic.com",
        "http://10.0.0.1:4010/anthropic",
        "file:///tmp",
    ] {
        assert_eq!(
            StandInAi::new(url).err(),
            Some(AdapterError::BadConfig),
            "{url}"
        );
    }
    assert!(StandInAi::new("http://127.0.0.1:4010/anthropic").is_ok());
    assert!(StandInAi::new("http://[::1]:4010/anthropic").is_ok());
}

/// SEC-018: storage is write-once by name and refuses names that climb out of its directory.
#[tokio::test]
async fn storage_writes_once_and_refuses_unsafe_names() {
    let dir = std::env::temp_dir().join(format!("askhava-storage-{}", std::process::id()));
    let store = LocalStorage::new(&format!("file://{}", dir.display())).unwrap();
    let k = key("storage.put", "obj-1");
    store
        .put_object(&k, "stream/2026-10-30/a.bin", b"first".to_vec())
        .await
        .unwrap();
    store
        .put_object(&k, "stream/2026-10-30/a.bin", b"second".to_vec())
        .await
        .unwrap();
    assert_eq!(
        store.read_object("stream/2026-10-30/a.bin").await.unwrap(),
        b"first"
    );
    let bad = store.put_object(&k, "../escape.bin", Vec::new()).await;
    assert_eq!(bad, Err(AdapterError::InvalidId));
    let _ = std::fs::remove_dir_all(dir);
}

fn key(action: &str, id: &str) -> IdempotencyKey {
    IdempotencyKey::derive(&SecretString::from("fake-id-secret"), action, &[id])
}

struct StandIn(Child, u16);

impl Drop for StandIn {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn stand_in() -> StandIn {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let child = Command::new("python3")
        .args(["-m", "stand_ins.server", "--port", &port.to_string()])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .stdout(Stdio::null())
        .spawn();
    let server = StandIn(child.unwrap(), port);
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return server;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("stand-in server did not start");
}

/// SEC-018, SEC-032: payments talk to the stand-in over HTTP; a repeated key returns the same charge.
#[tokio::test]
async fn payments_client_talks_to_the_stand_in() {
    let server = stand_in();
    let base = |vendor: &str| format!("http://127.0.0.1:{}/{vendor}", server.1);
    let stripe = StripePayments::new(&base("stripe"), SecretString::from("sk_test_fake")).unwrap();
    let customer = stripe
        .create_customer(&key("customer", "biz-1"), "biz-1")
        .await
        .unwrap();
    let charge = Charge {
        customer: &customer,
        payment_method: "pm_card_visa",
        amount_cents: 2500,
        pick_id: "pick-1",
    };
    let paid = stripe
        .create_payment_intent(&key("charge", "pick-1"), charge)
        .await
        .unwrap();
    assert_eq!((paid.status.as_str(), paid.amount), ("succeeded", 2500));
    let again = stripe
        .create_payment_intent(&key("charge", "pick-1"), charge)
        .await
        .unwrap();
    assert_eq!(again.id, paid.id);
    assert_eq!(stripe.fetch_payment_intent(&paid.id).await.unwrap(), paid);
    assert!(
        stripe
            .create_refund(&key("refund", "pick-1"), &paid.id)
            .await
            .unwrap()
            .starts_with("re_")
    );
}

/// SEC-018, SEC-032, AI-009: email and the stand-in AI talk to the stand-in over HTTP with derived keys.
#[tokio::test]
async fn email_and_ai_clients_talk_to_the_stand_in() {
    let server = stand_in();
    let base = |vendor: &str| format!("http://127.0.0.1:{}/{vendor}", server.1);
    let resend = ResendEmail::new(&base("resend"), SecretString::from("re_stand_in_fake")).unwrap();
    let email = Email {
        from: "Ask Hava <jobs@notify.askhava.com>",
        to: "owner@example.com",
        reply_to: "help@askhava.com",
        subject: "Your Ask Hava account",
        html: "<p>Fake</p>",
        text: "Fake",
    };
    assert!(
        !resend
            .send_email(&key("email", "e-1"), &email)
            .await
            .unwrap()
            .is_empty()
    );
    let ai = StandInAi::new(&base("anthropic")).unwrap();
    let request = serde_json::json!({"model": "stand-in", "max_tokens": 64, "messages": [{"role": "user", "content": "leak under sink"}]});
    let reply = ai
        .create_message(&key("ai", "job-1"), &request)
        .await
        .unwrap();
    assert_eq!(reply["stop_reason"], "end_turn");
}
