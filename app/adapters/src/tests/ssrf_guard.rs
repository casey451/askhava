//! The guarded fetch of untrusted URLs, the AI stand-in-only client, and the
//! other vendor clients against `stand_ins/server.py` (SEC-041, AI-009,
//! SEC-018; TD-SEC-20, TD-AI-01).
//!
//! A local server on 127.0.0.1 plays the remote host under the test-only
//! loopback allowance; private, link-local and metadata addresses stay refused
//! under it. The suppression removal runs against a one-request recording
//! server, because the stand-in has no suppression route until T133 (D-492);
//! the proxy case re-runs this test binary with proxy variables set (no
//! `set_var`, R16). All data is fake.

use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use secrecy::SecretString;

use crate::api::{
    AdapterError, AiAdapter, Charge, Email, EmailAdapter, FetchError, FetchPolicy, IdempotencyKey,
    LettersAdapter, LicenseLists, LicenseSource, LobLetters, LocalStorage, PaymentsAdapter,
    ResendEmail, StandInAi, StorageAdapter, StripePayments, fetch_untrusted, is_public_address,
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
        "/slow" => redirect("/slow"),
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
    } else if path == "/slow" {
        std::thread::sleep(Duration::from_millis(200));
        let _ = stream.write_all(answer(&path).as_bytes());
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
        "192.0.0.170",
        "2002:a9fe:a9fe::1",
        "2001:0:4136:e378:8000:63bf:3fff:fdd2",
        "fec0::1",
        "100::1",
        "64:ff9b:1::a9fe:a9fe",
    ];
    for ip in refused {
        assert!(!is_public_address(ip.parse::<IpAddr>().unwrap()), "{ip}");
    }
    for ip in ["8.8.8.8", "2606:4700:4700::1111"] {
        assert!(is_public_address(ip.parse::<IpAddr>().unwrap()), "{ip}");
    }
}

/// TD-SEC-20; SEC-041: IPv6 passes only inside global unicast 2000::/3, minus the tunnel and documentation blocks in it.
#[test]
fn ipv6_outside_global_unicast_is_refused() {
    let refused = [
        "1fff:ffff::1",
        "4000::1",
        "c000::1",
        "fc00::1",
        "fe00::1",
        "400::1",
        "2001:db8::1",
        "2001:0:1::1",
        "2002::1",
    ];
    for ip in refused {
        assert!(!is_public_address(ip.parse::<IpAddr>().unwrap()), "{ip}");
    }
    for ip in ["2000::1", "2001:4860:4860::8888", "2a00:1450::1", "3ffe::1"] {
        assert!(is_public_address(ip.parse::<IpAddr>().unwrap()), "{ip}");
    }
}

/// TD-SEC-20; SEC-041: an empty, dot-led, dot-ended or dotless allowed-host entry is dropped, so it never matches every host.
#[tokio::test]
async fn bad_allowed_host_entries_fail_closed() {
    let port = remote_host();
    for entry in ["", ".", ".0.0.1", "127.0.0.", "127"] {
        let policy =
            FetchPolicy::new(&[entry], MB, Duration::from_secs(2)).with_loopback_for_tests();
        for host in ["127.0.0.1", "127.0.0.1."] {
            let url = format!("http://{host}:{port}/ok");
            let got = fetch_untrusted(&policy, &url).await;
            assert_eq!(got, Err(FetchError::HostNotAllowed), "{entry:?} {host}");
        }
    }
}

/// TD-SEC-20; SEC-041: the timeout bounds the whole fetch, so a chain of slow redirects, each under it, still times out.
#[tokio::test]
async fn slow_redirect_chains_time_out_overall() {
    let port = remote_host();
    let url = format!("http://127.0.0.1:{port}/slow");
    let started = std::time::Instant::now();
    assert_eq!(
        fetch_untrusted(&test_policy(500), &url).await,
        Err(FetchError::TimedOut)
    );
    assert!(started.elapsed() < Duration::from_millis(700));
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

/// TD-SEC-20; SEC-041: media only from Twilio's hosts, over https; a listed host resolving to loopback is refused
/// (a dotted host, since a dotless entry such as `localhost` is dropped from the policy).
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
    let local = FetchPolicy::new(&["127.0.0.1"], MB, Duration::from_secs(2));
    let got = fetch_untrusted(&local, "https://127.0.0.1/photo.jpg").await;
    assert_eq!(got, Err(FetchError::AddressRefused));
}

/// TD-SEC-20; SEC-041: a URL with a user name or password, or a port other than 443, is refused before any lookup.
#[tokio::test]
async fn userinfo_and_other_ports_are_refused() {
    // The userinfo cases use a reserved example host (PRIV-019) that the policy
    // allows, so only the user name or password can cause the refusal.
    let fake = FetchPolicy::new(&["media.example.com"], MB, Duration::from_secs(2));
    let media = FetchPolicy::twilio_media();
    for (policy, url) in [
        (&fake, "https://user@media.example.com/photo.jpg"),
        (&fake, "https://:secret@media.example.com/photo.jpg"),
        (&media, "https://api.twilio.com:8443/photo.jpg"),
    ] {
        assert_eq!(
            fetch_untrusted(policy, url).await,
            Err(FetchError::HostNotAllowed),
            "{url}"
        );
    }
}

const PROXY_CHILD: &str = "ASKHAVA_TEST_PROXY_CHILD";

/// SEC-041, SEC-032: proxy variables in the environment are ignored, so the pinned addresses are the ones dialled.
#[tokio::test]
async fn fetch_and_vendor_clients_ignore_proxy_settings() {
    if std::env::var_os(PROXY_CHILD).is_some() {
        let port = remote_host();
        let url = format!("http://127.0.0.1:{port}/ok");
        let got = fetch_untrusted(&test_policy(2000), &url).await;
        assert_eq!(got, Ok(b"fake media".to_vec()));
        let lists = LicenseLists::new(&format!("http://127.0.0.1:{port}/lists")).unwrap();
        assert_eq!(
            lists.fetch_daily_list("daily.csv").await.unwrap(),
            b"fake media"
        );
        return;
    }
    let (proxy, seen) = counting_proxy();
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::ssrf_guard::fetch_and_vendor_clients_ignore_proxy_settings",
        ])
        .env(PROXY_CHILD, "1")
        .envs(
            [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
            ]
            .map(|name| (name, proxy.as_str())),
        )
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 0);
}

fn counting_proxy() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = seen.clone();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nvia proxy");
        }
    });
    (url, seen)
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
        "http://localhost:4010/anthropic",
        "http://127.0.0.1.example.com:4010/anthropic",
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
        .put_object(&k, "stream/2026-10-30/a.bin", b"first".to_vec())
        .await
        .unwrap();
    let other = store
        .put_object(&k, "stream/2026-10-30/a.bin", b"second".to_vec())
        .await;
    assert_eq!(other, Err(AdapterError::ObjectExists));
    store
        .put_object(&k, "stream/2026-10-30/a.txt", b"other".to_vec())
        .await
        .unwrap();
    assert_eq!(
        store.read_object("stream/2026-10-30/a.bin").await.unwrap(),
        b"first"
    );
    let bad = store.put_object(&k, "../escape.bin", Vec::new()).await;
    assert_eq!(bad, Err(AdapterError::InvalidId));
    let mut left: Vec<_> = std::fs::read_dir(dir.join("stream/2026-10-30"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    left.sort();
    assert_eq!(left, ["a.bin", "a.txt"]);
    let _ = std::fs::remove_dir_all(dir);
}

/// SEC-032: license lists read fixture files under a `file://` base and the same paths over HTTP; unsafe names never reach either.
#[tokio::test]
async fn license_lists_read_files_and_http() {
    let dir = std::env::temp_dir().join(format!("askhava-lists-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("lookup")).unwrap();
    std::fs::write(dir.join("daily.csv"), "ROC000001,Fake Plumbing LLC").unwrap();
    std::fs::write(dir.join("lookup/ROC000001"), "active").unwrap();
    let files = LicenseLists::new(&format!("file://{}", dir.display())).unwrap();
    assert_eq!(
        files.fetch_daily_list("daily.csv").await.unwrap(),
        b"ROC000001,Fake Plumbing LLC"
    );
    assert_eq!(files.lookup_license("ROC000001").await.unwrap(), b"active");
    let missing = files.fetch_daily_list("none.csv").await;
    assert_eq!(
        missing,
        Err(AdapterError::Vendor {
            status: 404,
            code: None
        })
    );
    for bad in ["../daily.csv", "", ".hidden"] {
        assert_eq!(
            files.lookup_license(bad).await,
            Err(AdapterError::InvalidId),
            "{bad}"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
    let port = remote_host();
    let web = LicenseLists::new(&format!("http://127.0.0.1:{port}/lists")).unwrap();
    assert_eq!(
        web.lookup_license("ROC000001").await.unwrap(),
        b"fake media"
    );
    assert_eq!(
        web.fetch_daily_list("a/b").await,
        Err(AdapterError::InvalidId)
    );
}

/// A fake vendor that answers one request with `{}` and hands back its port
/// and the request line and headers it received.
fn recording_vendor() -> (u16, std::sync::mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (send, heads) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let stream = listener.incoming().flatten().next().unwrap();
        let mut reader = BufReader::new(stream);
        let mut head = String::new();
        while reader.read_line(&mut head).is_ok_and(|n| n > 2) {}
        let _ = reader
            .into_inner()
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
        let _ = send.send(head);
    });
    (port, heads)
}

/// BIZ-056, SEC-018: removing a suppression is one authenticated DELETE of the address as one encoded segment, with the derived key.
#[tokio::test]
async fn remove_suppression_deletes_one_encoded_address() {
    let (port, heads) = recording_vendor();
    let base = format!("http://127.0.0.1:{port}/resend");
    let resend = ResendEmail::new(&base, SecretString::from("re_stand_in_fake")).unwrap();
    let k = key("email.unsuppress", "biz-1");
    resend
        .remove_suppression(&k, "owner/x@example.com")
        .await
        .unwrap();
    let head = heads.recv().unwrap().to_ascii_lowercase();
    assert!(
        head.starts_with("delete /resend/suppressions/owner%2fx@example.com http/1.1"),
        "{head}"
    );
    assert!(head.contains(&format!("idempotency-key: {}", k.as_str())));
    assert!(head.contains("authorization: bearer re_stand_in_fake"));
}

/// BIZ-056: an address that is empty, a dot segment or not `local@domain` is refused before any request, so it never hits the whole collection.
#[tokio::test]
async fn remove_suppression_refuses_non_address_segments() {
    let resend =
        ResendEmail::new("http://127.0.0.1:9/resend", SecretString::from("re_fake")).unwrap();
    let k = key("email.unsuppress", "biz-1");
    for address in ["", ".", "..", "owner", "@example.com", "owner@"] {
        let got = resend.remove_suppression(&k, address).await;
        assert_eq!(got, Err(AdapterError::InvalidId), "{address:?}");
    }
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

/// SEC-018, SEC-032, PICK-022: payments talk to the stand-in over HTTP; a repeated key returns the same charge and refund.
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
    assert_eq!(
        (paid.currency.as_str(), paid.customer.as_str()),
        ("usd", customer.as_str())
    );
    assert_eq!(
        (paid.metadata.pick_id.as_str(), paid.livemode),
        ("pick-1", None)
    );
    assert_eq!(stripe.fetch_payment_intent(&paid.id).await.unwrap(), paid);
    refund_replays_once(&stripe, &paid.id).await;
}

/// FEE-026, SEC-018: a refund repeated under the same key returns the same
/// refund; a second refund under a new key is refused by the vendor.
async fn refund_replays_once(stripe: &StripePayments, payment_intent: &str) {
    let refund = stripe
        .create_refund(&key("refund", "pick-1"), payment_intent)
        .await
        .unwrap();
    assert!(refund.starts_with("re_"));
    let replay = stripe
        .create_refund(&key("refund", "pick-1"), payment_intent)
        .await;
    assert_eq!(replay, Ok(refund));
    let second = stripe
        .create_refund(&key("refund", "pick-1b"), payment_intent)
        .await;
    let refused = AdapterError::Vendor {
        status: 400,
        code: None,
    };
    assert_eq!(second, Err(refused));
}

/// SEC-032: payment and letter IDs not of the vendor's form never reach a path or a form.
#[tokio::test]
async fn malformed_vendor_ids_are_refused() {
    let stripe = StripePayments::new(
        "http://127.0.0.1:9/stripe",
        SecretString::from("sk_test_fake"),
    )
    .unwrap();
    let lob = LobLetters::new("http://127.0.0.1:9/lob", SecretString::from("test_fake")).unwrap();
    for bad in ["", "pi_", "pi_../customers", "pi_a.b", "ch_123", "ltr_x/y"] {
        assert_eq!(
            stripe.fetch_payment_intent(bad).await,
            Err(AdapterError::InvalidId),
            "{bad}"
        );
        let refund = stripe.create_refund(&key("refund", "pick-2"), bad).await;
        assert_eq!(refund, Err(AdapterError::InvalidId), "{bad}");
        assert_eq!(
            lob.fetch_letter(bad).await,
            Err(AdapterError::InvalidId),
            "{bad}"
        );
    }
    let charge = Charge {
        customer: "cus_1",
        payment_method: "pm_a.b",
        amount_cents: 2500,
        pick_id: "pick-2",
    };
    let got = stripe
        .create_payment_intent(&key("charge", "pick-2"), charge)
        .await;
    assert_eq!(got, Err(AdapterError::InvalidId));
}

/// SEC-018, SEC-032: letters go to the stand-in Lob; a repeated key returns the same letter, which reads back.
#[tokio::test]
async fn letters_client_talks_to_the_stand_in() {
    let server = stand_in();
    let base = format!("http://127.0.0.1:{}/lob", server.1);
    let lob = LobLetters::new(&base, SecretString::from("test_fake")).unwrap();
    let address = |name: &str| serde_json::json!({"name": name, "address_line1": "100 Example St", "address_city": "Lake Havasu City", "address_state": "AZ", "address_zip": "86403"});
    let letter = serde_json::json!({"to": address("Fake Plumbing LLC"), "from": address("Ask Hava"), "file": "<p>Fake code 000000</p>", "color": false});
    let sent = lob
        .send_letter(&key("letter", "biz-1"), &letter)
        .await
        .unwrap();
    assert!(sent.id.starts_with("ltr_"));
    let again = lob
        .send_letter(&key("letter", "biz-1"), &letter)
        .await
        .unwrap();
    assert_eq!(again, sent);
    assert_eq!(lob.fetch_letter(&sent.id).await.unwrap(), sent);
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
