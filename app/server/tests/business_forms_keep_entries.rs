//! T588 (A11Y-012, CON-019, BIZ-022, D-4052): a refused business sign-up comes back with every
//! field she entered. The assembled router with the sign-up part and business texting switched
//! on serves `GET /b/signup`; a `POST /b/signup/start` refused for a too-long city license number
//! shows the email, the text phone, the ticked categories, the pest attestation and the E-SIGN box
//! as posted; a refusal for the unticked E-SIGN box keeps the rest (English and Spanish); the
//! `CONSENT_AGAIN` refusal keeps the typed values but ticks neither box; a fresh form binds no
//! value and ticks nothing. Every answer is `no-store` and mints no mailbox. All data is fake; no
//! setting is a real key.

use askhava_catalog::api::{load, parse};
use askhava_core::api::{MIGRATOR, Settings, TestControl};
use askhava_messaging::api::{
    Audience, BusinessEligibility, SendingNumber, file_business_eligibility, switch_on,
};
use askhava_portals::api::{CONSENT_AGAIN, SIGNUP_FORM_ROUTE, SIGNUP_ROUTE};
use askhava_privacy::api::{Vault, mailboxes_of};
use askhava_server::business_signup::ServedSignUp;
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const SECRET: &str = "test-only-id-secret-not-a-real-key-0588";
/// Two distinct fake 32-byte vault keys, version 1.
const SEAL: &str = "1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
const INDEX: &str = "1:BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ=";
/// The fake business sending number.
const NUMBER: &str = "+19285550126";
const EMAIL: &str = "keep-owner@example.test";
const PHONE: &str = "928-555-0177";
const NOON: &str = "2026-11-10T19:00:00Z";

fn pairs() -> Vec<(String, String)> {
    [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_ID_SECRET", SECRET),
        ("VAULT_KEY", SEAL),
        ("VAULT_INDEX_KEY", INDEX),
        ("ASKHAVA_BUSINESS_NUMBER", NUMBER),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect()
}

/// The router as `main` assembles it with the sign-up part, the catalog stored and business
/// texting switched on (TXT-035), so the text phone field is shown.
async fn app(pool: &PgPool) -> Router {
    let catalog = parse(include_str!("../../../docs/catalog/categories.yaml")).expect("catalog");
    let mut db = pool.acquire().await.expect("connection");
    load(&mut db, &catalog).await.expect("catalog stored");
    let (owner, now) = (Uuid::from_u128(0x588), NOON.parse().expect("time"));
    let allowed = BusinessEligibility {
        answered_on: "2026-11-04".parse().expect("date"),
        reference: "FAKE-T588-CASE".to_owned(),
        answer: "FAKE-T588 answer text as received.".to_owned(),
        allowed: true,
        campaign_reference: "FAKE-T588-CAMPAIGN".to_owned(),
    };
    let filed = file_business_eligibility(&mut db, owner, &allowed, now).await;
    filed.expect("filed");
    let on = switch_on(&mut db, Audience::Business, (owner, now)).await;
    assert!(on.expect("switched"), "business texting on");
    let settings = Settings::from_pairs(pairs()).expect("fake settings");
    let control = TestControl::from_settings(&settings, pairs()).expect("test control");
    let signup = ServedSignUp::from_settings(pool.clone(), pairs()).expect("sign-up part");
    let number = SendingNumber::parse(NUMBER).expect("number");
    let parts = RouterParts::from((None, None, None, None))
        .with_sign_up(signup)
        .with_business_number(Some(number));
    build_router(pool.clone(), control.router(), parts)
}

/// The status, `Cache-Control` and page of `request`.
async fn answer(app: &Router, request: Request<Body>) -> (StatusCode, String, String) {
    let response = app.clone().oneshot(request).await.expect("answer");
    let status = response.status();
    let cache = response.headers().get(header::CACHE_CONTROL);
    let cache = cache.and_then(|v| v.to_str().ok()).unwrap_or_default();
    let cache = cache.to_owned();
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (status, cache, String::from_utf8_lossy(&body).into_owned())
}

/// What the browser holds after `GET path`: the pre-session cookie and the form's page.
async fn open(app: &Router, path: &str) -> (String, String) {
    let request = Request::get(path)
        .header(header::HOST, "askhava.test")
        .body(Body::empty())
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("answer");
    let cookies = response.headers().get_all(header::SET_COOKIE).iter();
    let pre = cookies
        .filter_map(|v| v.to_str().ok())
        .find(|c| c.starts_with("__Host-askhava-pre="))
        .and_then(|c| c.split(';').next())
        .expect("the pre-session cookie")
        .to_owned();
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (pre, String::from_utf8_lossy(&body).into_owned())
}

/// The value of the first hidden field `name` on `html`.
fn hidden(html: &str, name: &str) -> String {
    let at = format!("name=\"{name}\" value=\"");
    let rest = &html[html.find(&at).expect("field") + at.len()..];
    rest[..rest.find('"').expect("end")].to_owned()
}

/// `POST /b/signup/start` from the same site with `page`'s tokens and `typed`.
async fn post(
    app: &Router,
    (pre, page): (&str, &str),
    typed: &[(&str, &str)],
) -> (StatusCode, String, String) {
    let tokens = ["csrf", "consent_channels", "consent_seal"].map(|n| (n, hidden(page, n)));
    let enc = |s: &str| {
        let keep = |c: u8| c.is_ascii_alphanumeric() || b"-._".contains(&c);
        let each = s.bytes().map(|c| {
            if keep(c) {
                (c as char).to_string()
            } else {
                format!("%{c:02X}")
            }
        });
        each.collect::<String>()
    };
    let tokens = tokens.iter().map(|(k, v)| (*k, v.as_str()));
    let body: Vec<String> = tokens
        .chain(typed.iter().copied())
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect();
    let request = Request::post(SIGNUP_FORM_ROUTE)
        .header(header::HOST, "askhava.test")
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, pre)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    answer(app, request).await
}

/// The opening tag on `html` that starts with `start`.
fn tag<'a>(html: &'a str, start: &str) -> &'a str {
    let at = html.find(start).unwrap_or_else(|| panic!("no {start}"));
    let end = at + html[at..].find('>').expect("tag end");
    &html[at..=end]
}

/// Whether the checkbox or field with `id` is rendered `checked`.
fn ticked(html: &str, id: &str) -> bool {
    tag(html, &format!("id=\"{id}\"")).contains(" checked")
}

/// Every field a sign-up types: two categories, both boxes ticked, the text choice and phone.
fn typed(city: &str) -> Vec<(&str, &str)> {
    vec![
        ("business_name", "Example Pest and Pipes"),
        ("email", EMAIL),
        ("notice_channel", "both"),
        ("text_phone", PHONE),
        ("category", "plumbing_repair"),
        ("category", "pest_control"),
        ("pest_license", "yes"),
        ("pest_number", "FAKE-PEST-0588"),
        ("city_license_number", city),
        ("consent", "yes"),
    ]
}

/// No mailbox was minted for the typed address (nothing stored on a refusal).
async fn no_mailbox(pool: &PgPool) {
    let key = |k: &str| SecretString::from(k.to_owned());
    let db_url = key("postgres://fake@127.0.0.1:1/none");
    let vault = Vault::new(&key(SEAL), &key(INDEX), &db_url).expect("vault");
    let mut db = pool.acquire().await.expect("connection");
    let found = mailboxes_of(&mut db, &vault, EMAIL).await.expect("read");
    assert!(found.is_empty(), "no mailbox for a refused sign-up");
}

/// A11Y-012, CON-019, D-4052: a sign-up refused for a bad license number comes back with the
/// email, phone, categories and both boxes as posted; the `CONSENT_AGAIN` refusal before it keeps
/// the typed values but ticks neither box, since the shown wording changed.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_bad_license_number_keeps_every_entry_and_both_ticks(pool: PgPool) {
    let app = app(&pool).await;
    let (pre, form) = open(&app, SIGNUP_ROUTE).await;
    assert_eq!(
        hidden(&form, "consent_channels"),
        "email",
        "email preselected"
    );
    let too_long = "C".repeat(65);
    let (status, cache, again) = post(&app, (&pre, &form), &typed(&too_long)).await;
    assert_eq!((status, cache.as_str()), (StatusCode::OK, "no-store"));
    assert!(again.contains(CONSENT_AGAIN.split(',').next().expect("text")));
    assert!(tag(&again, "<input type=\"email\"").contains(&format!("value=\"{EMAIL}\"")));
    assert!(tag(&again, "<input type=\"tel\"").contains(&format!("value=\"{PHONE}\"")));
    assert!(ticked(&again, "cat-plumbing_repair") && ticked(&again, "cat-pest_control"));
    assert!(
        !ticked(&again, "consent"),
        "CON-019: the wording changed, so never ticked"
    );
    assert!(
        !ticked(&again, "pest-license"),
        "never ticked under CONSENT_AGAIN"
    );
    let (status, cache, html) = post(&app, (&pre, &again), &typed(&too_long)).await;
    assert_eq!((status, cache.as_str()), (StatusCode::OK, "no-store"));
    assert!(
        html.contains("id=\"city-license-number-error\""),
        "the license number refused"
    );
    assert!(tag(&html, "<input type=\"email\"").contains(&format!("value=\"{EMAIL}\"")));
    assert!(tag(&html, "<input type=\"tel\"").contains(&format!("value=\"{PHONE}\"")));
    assert!(ticked(&html, "notice-both"), "the choice kept");
    assert!(ticked(&html, "cat-plumbing_repair") && ticked(&html, "cat-pest_control"));
    assert!(
        !ticked(&html, "cat-hvac_repair"),
        "only the ticked categories"
    );
    assert!(ticked(&html, "pest-license"), "ticked in this same post");
    assert!(ticked(&html, "consent"), "ticked in this same post");
    no_mailbox(&pool).await;
}

/// A11Y-012, CON-019, D-4052: a refusal for the unticked E-SIGN box keeps the rest, in English and
/// Spanish; the box stays unticked and the pest attestation stays as posted.
#[sqlx::test(migrator = "MIGRATOR")]
async fn an_unticked_box_keeps_the_rest(pool: PgPool) {
    let app = app(&pool).await;
    for (path, lang) in [
        (SIGNUP_ROUTE.to_owned(), "en"),
        (format!("{SIGNUP_ROUTE}?lang=es"), "es"),
    ] {
        let (pre, form) = open(&app, &path).await;
        let mut fields = typed("");
        fields.retain(|(k, _)| !["consent", "notice_channel"].contains(k));
        fields.push(("notice_channel", "email"));
        fields.push(("lang", lang));
        let (status, cache, html) = post(&app, (&pre, &form), &fields).await;
        assert_eq!(
            (status, cache.as_str()),
            (StatusCode::OK, "no-store"),
            "{lang}"
        );
        assert!(
            html.contains("id=\"consent-error\""),
            "{lang}: the box refused"
        );
        assert!(tag(&html, "<input type=\"email\"").contains(&format!("value=\"{EMAIL}\"")));
        assert!(tag(&html, "<input type=\"tel\"").contains(&format!("value=\"{PHONE}\"")));
        assert!(ticked(&html, "cat-plumbing_repair") && ticked(&html, "cat-pest_control"));
        assert!(
            ticked(&html, "pest-license"),
            "{lang}: the attestation as posted"
        );
        assert!(!ticked(&html, "consent"), "{lang}: not ticked in this post");
    }
    no_mailbox(&pool).await;
}

/// D-4052, TD-CON-29: a fresh form binds no value and ticks nothing, in both languages.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_fresh_form_binds_nothing(pool: PgPool) {
    let app = app(&pool).await;
    for path in [SIGNUP_ROUTE.to_owned(), format!("{SIGNUP_ROUTE}?lang=es")] {
        let (_, form) = open(&app, &path).await;
        assert_eq!(
            tag(&form, "<input type=\"email\""),
            "<input type=\"email\" id=\"email\" name=\"email\" autocomplete=\"email\">"
        );
        assert!(!tag(&form, "<input type=\"tel\"").contains("value="));
        for id in [
            "cat-plumbing_repair",
            "cat-pest_control",
            "pest-license",
            "consent",
        ] {
            assert!(!ticked(&form, id), "{path}: {id} unticked");
        }
    }
}
