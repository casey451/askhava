//! TD-SEC-75 (SEC-043, SEC-020, SEC-014, SEC-040; T651, D-4294): the owner's try limits through
//! the assembled router against PostgreSQL. Step-up tries are limited per session, and the try
//! past the limit checks no code and ends the session; junk passwords that fill `OWNER_TRIES_ALL`
//! never lock the passkey path; a passkey answer that fails runs no password check; passkey
//! challenges are limited per address (an IPv6 /64), not by one table-wide cap.
//! Fake data only: the pass phrase, token and authenticator secret are made for this file, and
//! the passkey is a software one whose private scalar is 1 (its public key is P-256's generator).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_core::api::{MIGRATOR, brand};
use askhava_portals::api::{
    CookieJar, OwnerAuth, SecondFactor, bootstrap_owner, csrf_token, finish_passkey_registration,
    owner_principal, owner_sign_in, passkey_registration_options, passkey_sign_in_options,
};
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderValue, Request, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use rustls::SignatureScheme;
use rustls::crypto::aws_lc_rs::sign::any_ecdsa_type;
use rustls::pki_types::{PrivateKeyDer, PrivateSec1KeyDer};
use secrecy::ExposeSecret;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use sqlx::types::Uuid;
use totp_rs::Secret;
use tower::ServiceExt;

const TOKEN: &str = "fake-t651-bootstrap-token-0123456789";
const PASSWORD: &str = "fake t651 owner pass phrase";
const SIGN_IN: &str = "/owner/sign-in";
const STEP_UP: &str = "/owner/step-up";
const PASSKEY: &str = "/owner/sign-in/passkey";
const CRED_ID: [u8; 16] = [0x51; 16];
/// P-256's generator, the public key of the private scalar 1.
const GX: &str = "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296";
const GY: &str = "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5";

type Jar = std::collections::BTreeMap<String, String>;

struct World {
    app: Router,
    pool: PgPool,
    secret: Vec<u8>,
    logs: Logs,
    _guard: tracing::subscriber::DefaultGuard,
}

struct Reply {
    status: u16,
    text: String,
    set: Jar,
}

/// The messages and `kind` fields of the log events this test saw.
#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<(String, String)>>>);

#[derive(Default)]
struct Fields(Vec<(String, String)>);

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }
}

impl tracing::Subscriber for Logs {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let get = |name: &str| {
            let found = fields.0.iter().find(|(n, _)| n == name);
            found.map(|(_, v)| v.clone()).unwrap_or_default()
        };
        let mut all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        all.push((get("message"), get("kind")));
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// How many argon2 password checks ran so far.
fn password_checks(w: &World) -> usize {
    let all = w.logs.0.lock().unwrap_or_else(PoisonError::into_inner);
    all.iter()
        .filter(|(m, _)| m == "owner password checked")
        .count()
}

/// How many `auth_failed` security events were logged so far.
fn auth_failures(w: &World) -> usize {
    let all = w.logs.0.lock().unwrap_or_else(PoisonError::into_inner);
    let failed = |(m, k): &&(String, String)| m == "security event" && k == "auth_failed";
    all.iter().filter(failed).count()
}

async fn world(pool: &PgPool) -> World {
    let mut db = pool.acquire().await.expect("db");
    let enrolment = bootstrap_owner(&mut db, Some(TOKEN), PASSWORD, Timestamp::now()).await;
    let base32 = enrolment.expect("bootstrap").totp_secret;
    let secret = Secret::try_from_base32(base32.expose_secret()).expect("base32");
    let parts = RouterParts::from((None, None, None, None));
    let logs = Logs::default();
    World {
        app: build_router(pool.clone(), Router::new(), parts),
        pool: pool.clone(),
        secret: secret.as_bytes().to_vec(),
        _guard: tracing::subscriber::set_default(logs.clone()),
        logs,
    }
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `at`.
fn totp_code(secret: &[u8], at: Timestamp) -> String {
    let step = u64::try_from(at.as_second() / 30).expect("step");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[offset..offset + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

/// A 6-digit code no step from a minute ago to a minute ahead uses.
fn wrong_code(secret: &[u8]) -> String {
    let now = Timestamp::now();
    let near: Vec<String> = (-2..=2)
        .map(|k| totp_code(secret, now + SignedDuration::from_secs(30 * k)))
        .collect();
    (0..100)
        .map(|n| format!("{:06}", 100_000 + n))
        .find(|code| !near.contains(code))
        .expect("a wrong code")
}

fn portals_jar(jar: &Jar) -> CookieJar {
    let cookies: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut headers = HeaderMap::new();
    let value = HeaderValue::from_str(&cookies.join("; ")).expect("cookie");
    headers.insert(header::COOKIE, value);
    CookieJar::from_headers(&headers)
}

fn v4(n: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(203, 0, 113, n))
}

/// A same-origin request from `ip` with `jar`'s cookies and their CSRF token before `fields`.
async fn send(
    w: &World,
    (method, path): (&str, &str),
    jar: &Jar,
    fields: &str,
    ip: IpAddr,
) -> Reply {
    let cookies: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let csrf = csrf_token(&portals_jar(jar)).unwrap_or_default();
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "askhava.test")
        .header(header::ORIGIN, "https://askhava.test")
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}{fields}")))
        .expect("request");
    let peer = SocketAddr::new(ip, 40_000);
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = w.app.clone().oneshot(request).await.expect("response");
    let (parts, body) = response.into_parts();
    let set = parts.headers.get_all(header::SET_COOKIE).iter();
    let set = set.filter_map(|v| v.to_str().ok()?.split(';').next()?.split_once('='));
    let bytes = to_bytes(body, 1 << 20).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        text: String::from_utf8_lossy(&bytes).into_owned(),
        set: set.map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
    }
}

/// The anonymous browser: the sign-in page's pre-session cookie.
async fn visitor(w: &World) -> Jar {
    send(w, ("GET", SIGN_IN), &Jar::new(), "", v4(1)).await.set
}

/// A sign-in `POST` of `password` and `code`.
async fn sign_in(w: &World, jar: &Jar, (password, code): (&str, &str), ip: IpAddr) -> Reply {
    let fields = format!("&password={}&code={code}", password.replace(' ', "+"));
    send(w, ("POST", SIGN_IN), jar, &fields, ip).await
}

fn relying_party() -> OwnerAuth {
    let domain = brand().domain();
    OwnerAuth::new(domain, &format!("https://{domain}")).expect("relying party")
}

/// An owner session started through `portals` at `at` (a TOTP step before any the routes use).
async fn owner_at(w: &World, at: Timestamp) -> Jar {
    let mut db = w.pool.acquire().await.expect("db");
    let code = totp_code(&w.secret, at);
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    let jar = owner_sign_in(&mut db, CookieJar::new(), factor, &relying_party(), at).await;
    let jar = jar.expect("owner signed in");
    jar.iter()
        .map(|c| (c.name().to_owned(), c.value().to_owned()))
        .collect()
}

fn unhex(text: &str) -> Vec<u8> {
    let byte = |i: usize| u8::from_str_radix(&text[i..i + 2], 16).expect("hex");
    (0..text.len()).step_by(2).map(byte).collect()
}

fn cbor_head(out: &mut Vec<u8>, major: u8, len: usize) {
    match u8::try_from(len) {
        Ok(n) if n < 24 => out.push(major | n),
        Ok(n) => out.extend([major | 24, n]),
        Err(_) => panic!("short items only"),
    }
}

fn cbor_text(out: &mut Vec<u8>, text: &str) {
    cbor_head(out, 0x60, text.len());
    out.extend(text.as_bytes());
}

fn client_data(kind: &str, options: &str) -> Vec<u8> {
    let options: serde_json::Value = serde_json::from_str(options).expect("options");
    let challenge = options["publicKey"]["challenge"]
        .as_str()
        .expect("challenge");
    let origin = format!("https://{}", brand().domain());
    let data = serde_json::json!({"type": kind, "challenge": challenge, "origin": origin});
    data.to_string().into_bytes()
}

fn rp_hash() -> Vec<u8> {
    Sha256::digest(brand().domain().as_bytes()).to_vec()
}

/// The software authenticator's "none" attestation answer to registration `options`.
fn attestation(options: &str) -> String {
    let mut data = rp_hash();
    data.extend([0x45, 0, 0, 0, 0]);
    data.extend([0; 16]);
    data.extend([0, 16]);
    data.extend(CRED_ID);
    data.extend([0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20]);
    data.extend(unhex(GX));
    data.extend([0x22, 0x58, 0x20]);
    data.extend(unhex(GY));
    let mut object = vec![0xa3];
    cbor_text(&mut object, "fmt");
    cbor_text(&mut object, "none");
    cbor_text(&mut object, "attStmt");
    object.push(0xa0);
    cbor_text(&mut object, "authData");
    cbor_head(&mut object, 0x40, data.len());
    object.extend(data);
    let response = serde_json::json!({
        "attestationObject": B64.encode(object),
        "clientDataJSON": B64.encode(client_data("webauthn.create", options)),
    });
    let id = B64.encode(CRED_ID);
    let answer =
        serde_json::json!({"id": id, "rawId": id, "response": response, "type": "public-key"});
    answer.to_string()
}

/// The software authenticator's assertion for sign-in `options`, its signature spoilt when `bad`.
fn assertion(options: &str, bad: bool) -> String {
    let mut data = rp_hash();
    data.extend([0x05, 0, 0, 0, 1]);
    let client = client_data("webauthn.get", options);
    let mut signed = data.clone();
    signed.extend(Sha256::digest(&client));
    let mut sec1 = vec![0x30, 0x31, 0x02, 0x01, 0x01, 0x04, 0x20];
    sec1.extend([0; 31]);
    sec1.push(1);
    sec1.extend([
        0xa0, 0x0a, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07,
    ]);
    let key = any_ecdsa_type(&PrivateKeyDer::Sec1(PrivateSec1KeyDer::from(sec1))).expect("key");
    if bad {
        signed.push(0);
    }
    let signer = key.choose_scheme(&[SignatureScheme::ECDSA_NISTP256_SHA256]);
    let signature = signer.expect("P-256").sign(&signed).expect("signed");
    let response = serde_json::json!({
        "authenticatorData": B64.encode(data),
        "clientDataJSON": B64.encode(client),
        "signature": B64.encode(signature),
    });
    let id = B64.encode(CRED_ID);
    let answer =
        serde_json::json!({"id": id, "rawId": id, "response": response, "type": "public-key"});
    answer.to_string()
}

/// Registers the software passkey through `portals` on a session started two minutes ago.
async fn register_passkey(w: &World) {
    let at = Timestamp::now() - SignedDuration::from_mins(2);
    let jar = portals_jar(&owner_at(w, at).await);
    let mut db = w.pool.acquire().await.expect("db");
    let owner = owner_principal(&mut db, &jar, at)
        .await
        .expect("read")
        .expect("owner");
    let auth = relying_party();
    let started = passkey_registration_options(&mut db, &owner, (&auth, STEP_UP), at);
    let (challenge, options) = started.await.expect("registration options");
    let answer = attestation(&options);
    let finish = (&auth, challenge, answer.as_str(), STEP_UP);
    let done = finish_passkey_registration(&mut db, &owner, finish, at).await;
    assert_eq!(done, Ok(()), "the software passkey is registered");
}

fn form_escape(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => char::from(b).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A passkey sign-in from `ip`: a challenge from the route, then the right password and the
/// software passkey's answer (spoilt when `bad`).
async fn passkey_sign_in(w: &World, jar: &Jar, ip: IpAddr, bad: bool) -> Reply {
    let issued = send(w, ("POST", PASSKEY), jar, "", ip).await;
    assert_eq!(issued.status, 200, "a challenge: {}", issued.text);
    let issued: serde_json::Value = serde_json::from_str(&issued.text).expect("json");
    let challenge = issued["challenge"].as_str().expect("id");
    let answer = assertion(issued["options"].as_str().expect("options"), bad);
    let fields = format!(
        "&password={}&challenge={challenge}&passkey={}",
        form_escape(PASSWORD),
        form_escape(&answer)
    );
    send(w, ("POST", SIGN_IN), jar, &fields, ip).await
}

/// TD-SEC-75 (SEC-043, SEC-020): five wrong step-up codes on one session are each refused as
/// today; the sixth try, even with the right code, checks no code and ends the session.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_75_step_up_tries_past_the_limit_end_the_session(pool: PgPool) {
    let w = world(&pool).await;
    let jar = owner_at(&w, Timestamp::now() - SignedDuration::from_mins(2)).await;
    let wrong = wrong_code(&w.secret);
    for _ in 0..5 {
        let reply = send(
            &w,
            ("POST", STEP_UP),
            &jar,
            &format!("&code={wrong}"),
            v4(20),
        )
        .await;
        assert_eq!(reply.status, 400, "a wrong code: {}", reply.text);
        assert!(!reply.text.contains("href=\"/owner/sign-in\""));
    }
    assert_eq!(auth_failures(&w), 5);
    let right = format!("&code={}", totp_code(&w.secret, Timestamp::now()));
    let sixth = send(&w, ("POST", STEP_UP), &jar, &right, v4(21)).await;
    assert_eq!(sixth.status, 401, "past the limit: {}", sixth.text);
    assert!(sixth.text.contains("href=\"/owner/sign-in\""));
    assert_eq!(
        sixth.set.get("__Host-askhava-session").map(String::as_str),
        Some("")
    );
    assert_eq!(auth_failures(&w), 5, "the code was not checked");
    let after = send(&w, ("GET", STEP_UP), &jar, "", v4(21)).await;
    assert_eq!(after.status, 401, "the session was ended on the server");
}

/// TD-SEC-75 (SEC-040, D-492): with `OWNER_TRIES_ALL` filled by junk passwords from four
/// addresses, a password-and-TOTP sign-in from a fifth is refused before its argon2, while a
/// verified passkey answer from it signs in.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_75_junk_passwords_never_lock_the_passkey_path(pool: PgPool) {
    let w = world(&pool).await;
    register_passkey(&w).await;
    let jar = visitor(&w).await;
    let wrong = wrong_code(&w.secret);
    for n in 0..20u8 {
        let junk = sign_in(
            &w,
            &jar,
            ("fake junk pass phrase t651", &wrong),
            v4(30 + n / 5),
        )
        .await;
        assert_eq!(junk.status, 401);
    }
    let checks = password_checks(&w);
    let code = totp_code(&w.secret, Timestamp::now());
    let password = sign_in(&w, &jar, (PASSWORD, &code), v4(40)).await;
    assert_eq!(password.status, 401, "the account limit refuses passwords");
    assert_eq!(password_checks(&w), checks, "refused before any argon2");
    let passkey = passkey_sign_in(&w, &jar, v4(40), false).await;
    assert_eq!(
        passkey.status, 303,
        "the passkey path signs in: {}",
        passkey.text
    );
    assert!(
        passkey
            .set
            .get("__Host-askhava-session")
            .is_some_and(|v| !v.is_empty())
    );
}

/// TD-SEC-75 (SEC-040, D-4294): a passkey-shaped post whose assertion fails runs no password
/// check, even with the right password, and is counted in `OWNER_TRIES_ALL`: twenty of them
/// from four addresses refuse a later password sign-in from a fifth.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_75_a_failed_assertion_checks_no_password(pool: PgPool) {
    let w = world(&pool).await;
    register_passkey(&w).await;
    let jar = visitor(&w).await;
    let checks = password_checks(&w);
    let spoilt = passkey_sign_in(&w, &jar, v4(50), true).await;
    assert_eq!(spoilt.status, 401, "a spoilt assertion signs no one in");
    assert!(!spoilt.set.contains_key("__Host-askhava-session"));
    for n in 1..20u8 {
        let fields = format!(
            "&password={}&challenge={}&passkey=x",
            form_escape(PASSWORD),
            Uuid::new_v4()
        );
        let fake = send(&w, ("POST", SIGN_IN), &jar, &fields, v4(50 + n / 5)).await;
        assert_eq!(fake.status, 401);
    }
    assert_eq!(
        password_checks(&w),
        checks,
        "no failed assertion ran argon2"
    );
    let code = totp_code(&w.secret, Timestamp::now());
    let password = sign_in(&w, &jar, (PASSWORD, &code), v4(60)).await;
    assert_eq!(
        password.status, 401,
        "failed assertions count for the account"
    );
}

/// TD-SEC-75 (SEC-014, SEC-040, D-4294): twenty live challenges, ten of them issued to one
/// address, do not stop a challenge for another; that address's eleventh in the hour is a 429;
/// an IPv6 client counts by its /64.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_75_passkey_challenges_are_limited_per_address(pool: PgPool) {
    let w = world(&pool).await;
    register_passkey(&w).await;
    let jar = visitor(&w).await;
    let mut db = w.pool.acquire().await.expect("db");
    for _ in 0..10 {
        let live = passkey_sign_in_options(&mut db, &relying_party(), Timestamp::now()).await;
        assert!(matches!(live, Ok(Some(_))), "a live challenge");
    }
    for _ in 0..10 {
        assert_eq!(
            send(&w, ("POST", PASSKEY), &jar, "", v4(70)).await.status,
            200
        );
    }
    assert_eq!(
        send(&w, ("POST", PASSKEY), &jar, "", v4(70)).await.status,
        429
    );
    assert_eq!(
        send(&w, ("POST", PASSKEY), &jar, "", v4(71)).await.status,
        200
    );
    let v6 = |prefix: u16, host: u16| IpAddr::from([0x2001, 0xdb8, 0, prefix, 0, 0, 0, host]);
    for host in 1..=10 {
        assert_eq!(
            send(&w, ("POST", PASSKEY), &jar, "", v6(5, host))
                .await
                .status,
            200
        );
    }
    let same_64 = send(&w, ("POST", PASSKEY), &jar, "", v6(5, 0xff)).await;
    assert_eq!(same_64.status, 429, "one /64 is one address");
    assert_eq!(
        send(&w, ("POST", PASSKEY), &jar, "", v6(6, 1)).await.status,
        200
    );
}
