//! TD-WORD-38 (WORD-001, WORD-008, WORD-021, FEE-049, TERMS-016; T295, D-1932, D-2843): a brand
//! switch changes every channel. This test binary installs a fake brand (Testbrand, `tb.example`,
//! prefix TESTBRND) before anything renders, short enough that every registry text keeps its
//! recorded segments (TXT-044), then renders through the crates' public functions: a homeowner and
//! a business text in English and Spanish, a homeowner and a business email, the receipt email, the
//! quote page and the site layout, the voice greeting, the intent description and statement
//! descriptor, a question-bank text and a quote-card label. Each holds the fake name or domain and
//! none holds "Ask Hava", `askhava.com` or the whole word ASKHAVA.
//!
//! The legal records keep their words until they are re-issued (TERMS-016): the compiled-in
//! homeowner Terms, `AUTHORIZATION_TEXT` and the WORD-022 list each name the embedded brand's legal
//! or display name, and the same check reports all three under the fake brand, so a switch fails
//! `Ci` until they are re-issued; the SHA-256 of `AUTHORIZATION_TEXT` is pinned per
//! `AUTHORIZATION_VERSION`, so rewording it without a new version fails. All data is fake.

use std::sync::{Arc, Once};

use askhava_ai::api::{INTAKE_READING, WriteSources};
use askhava_calls::api::{greeting_en, greeting_es};
use askhava_catalog::api::question_bank;
use askhava_core::api::{Brand, IdSecret, LinkToken, ManualClock, brand, install_brand};
use askhava_intake::api::{AiTurn, BankRun, BankSettings, Next};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, create, move_to};
use askhava_licensing::api::{CardLanguage, CardLine};
use askhava_messaging::api::{
    Audience, FROM, Language, MailboxKind, Notice, PostalAddress, ReceiptFacts, ReceiptSender,
    Recipient, render, render_email, render_receipt,
};
use askhava_ops::api::Actor;
use askhava_payments::api::{AUTHORIZATION_TEXT, AUTHORIZATION_VERSION, IntentText};
use askhava_picks::api::terms_line;
use askhava_portals::api::quote_page_routes;
use askhava_privacy::api::{CommonWords, Known, NameMemory, scrub};
use askhava_quotes::api::{
    DayPart, Format, Parts, Permit, QuoteCard, QuoteFacts, Spanish, TripFee,
};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use jiff::Timestamp;
use jiff::civil::date;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

/// The fake brand (D-2843): every name and the domain differ from the embedded brand's.
const FAKE: &str = r#"{
  "display_name": "Testbrand",
  "business_name": "Testbrand for Business",
  "short_name": "Testbrand",
  "legal_name": "Testbrand LLC",
  "spoken_legal_name": "Testbrand L L C",
  "domain": "tb.example",
  "email_from_name": "Testbrand",
  "email_from_address": "jobs@notify.tb.example",
  "statement_prefix": "TESTBRND",
  "forbidden_alone": ["Testbrand"],
  "es": {
    "display_name": "Testbrand",
    "business_name": "Testbrand for Business",
    "short_name": "Testbrand",
    "legal_name": "Testbrand LLC"
  }
}"#;

/// The compiled-in homeowner Terms (`askhava_server`'s `HOMEOWNER_TERMS`, the same file).
const HOMEOWNER_TERMS: &str = include_str!("../../../docs/legal/homeowner-terms.md");
/// The WORD-022 list `ai` compiles in (`output_checks.rs`'s `WORD_022_FILE`, the same file).
const WORD_022_LIST: &str = include_str!("../../../app/ai/data/word_022_phrases.json");

/// The SHA-256 of `AUTHORIZATION_TEXT` for each `AUTHORIZATION_VERSION` (FEE-015, TERMS-016): a
/// reworded authorization needs a new version and a new row here.
const AUTHORIZATION_SHA256: [(i32, &str); 1] = [(
    1,
    "4cc85df1e5f51f40c7940427aa61904f35d94c423a886f19f76bc326d84274cd",
)];

const POSTAL: &str = "PO Box 0000, Lake Havasu City, AZ 86403";

/// Installs the fake brand once for this binary, before anything renders (D-1932).
fn fake_brand() -> &'static Brand {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let fake = Brand::parse(FAKE).expect("the fake brand passes the brand checks");
        install_brand(fake).expect("installed before anything read the brand");
    });
    let b = brand();
    assert_eq!(b.display_name(), "Testbrand", "the fake brand is installed");
    b
}

/// TD-WORD-38: `text` holds the fake name or domain and no old brand word.
fn rebranded(what: &str, text: &str) {
    assert!(
        ["Testbrand", "tb.example", "TESTBRND"]
            .iter()
            .any(|fake| text.contains(fake)),
        "{what} lacks the fake brand: {text}"
    );
    let lower = text.to_lowercase();
    assert!(!lower.contains("ask hava"), "{what} names Ask Hava: {text}");
    assert!(
        !lower.contains("askhava.com"),
        "{what} names askhava.com: {text}"
    );
    let whole = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word == "ASKHAVA");
    assert!(!whole, "{what} holds ASKHAVA: {text}");
}

/// The legal records that do not name `b`'s legal or display name (TERMS-016).
fn records_not_naming(b: &Brand) -> Vec<&'static str> {
    let names = |text: &str| text.contains(b.legal_name()) || text.contains(b.display_name());
    [
        ("homeowner terms", HOMEOWNER_TERMS),
        ("AUTHORIZATION_TEXT", AUTHORIZATION_TEXT),
        ("WORD-022 list", WORD_022_LIST),
    ]
    .into_iter()
    .filter(|(_, text)| !names(text))
    .map(|(record, _)| record)
    .collect()
}

/// True when `text` is the authorization pinned for `version` (FEE-015, TERMS-016).
fn authorization_pinned(version: i32, text: &str) -> bool {
    let digest: String = Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    AUTHORIZATION_SHA256
        .iter()
        .any(|(v, hash)| *v == version && *hash == digest)
}

/// TD-WORD-38 (WORD-001, WORD-008, TXT-044): a homeowner and a business text in English and
/// Spanish from the registry, filled with the fake brand and within their recorded segments; the
/// text pick's Terms line and the AI instruction take it too.
#[test]
fn td_word_38_texts_take_the_fake_brand() {
    let b = fake_brand();
    for language in [Language::En, Language::Es] {
        let consent = render("consent", language, true, &[]).expect("consent");
        assert_eq!(consent.language, language);
        assert!(consent.body.starts_with("Testbrand:"), "{}", consent.body);
        rebranded("homeowner text", &consent.body);
        assert!(consent.body.contains("tb.example/terms"));
        let values = [("job", "1042"), ("time", "3:00 p.m.")];
        let notice = render("pick_notice", language, true, &values).expect("pick notice");
        assert_eq!(notice.language, language);
        assert!(notice.body.starts_with(&format!("{}:", b.short_name())));
        rebranded("business text", &notice.body);
        rebranded("terms line", terms_line(language));
    }
    let prompt = INTAKE_READING.text.filled();
    assert!(prompt.starts_with("You sort one home-service request for Testbrand."));
    rebranded("AI instruction", &prompt);
}

/// TD-WORD-38 (WORD-001, SEC-025): the homeowner and business emails (sender, subject, body,
/// footer) and the receipt email take the fake brand.
#[test]
fn td_word_38_emails_take_the_fake_brand() {
    fake_brand();
    rebranded("sender", &FROM.to_string());
    assert_eq!(FROM.to_string(), "\"Testbrand\" <jobs@notify.tb.example>");
    let postal = PostalAddress::new(POSTAL, MailboxKind::PoBox).expect("postal");
    for (audience, notice) in [
        (Audience::Homeowner, Notice::OptedOut),
        (Audience::Business, Notice::StillOpen(1042)),
    ] {
        for language in [Language::En, Language::Es] {
            let to = Recipient {
                id: Uuid::from_u128(0x7295_0001),
                audience,
                language,
                manage_url: "https://tb.example/b",
            };
            let email = render_email(&postal, &to, notice).expect("email");
            rebranded("email subject", &email.subject);
            rebranded("email body", &email.plain);
            rebranded("email html", &email.html);
            assert!(email.plain.contains("Testbrand LLC, PO Box 0000"), "footer");
        }
    }
    let secret = SecretString::from("fake-id-secret-for-tests-only-t295".to_owned());
    let sender = ReceiptSender::new(secret, POSTAL, MailboxKind::PoBox).expect("sender");
    let text = IntentText::for_job(1042).expect("job number");
    let facts = ReceiptFacts {
        pick: Uuid::from_u128(0x7295_0002),
        job_number: 1042,
        fee_cents: 2500,
        line: text.description(),
    };
    let receipt = render_receipt(&sender, Language::En, &facts).expect("receipt");
    rebranded("receipt subject", &receipt.subject);
    rebranded("receipt", &receipt.plain);
    rebranded("receipt html", &receipt.html);
    assert!(
        receipt
            .plain
            .contains("Testbrand introduction service, job 1042")
    );
}

/// TD-WORD-38 (FEE-049, WORD-001): the intent description and the statement descriptor take the
/// fake brand's name and prefix, within `DESCRIPTOR_MAX`.
#[test]
fn td_word_38_intent_text_takes_the_fake_brand() {
    fake_brand();
    let text = IntentText::for_job(1042).expect("job number");
    assert_eq!(
        text.description(),
        "Testbrand introduction service, job 1042"
    );
    assert_eq!(text.statement_descriptor(), "TESTBRND* JOB 1042");
    rebranded("intent description", &text.description());
    rebranded("statement descriptor", &text.statement_descriptor());
    assert!(IntentText::for_job(9_999_999).is_ok());
}

/// TD-WORD-38 (WORD-001, CALL-019): the voice greeting names the fake legal name.
#[test]
fn td_word_38_voice_greeting_takes_the_fake_brand() {
    fake_brand();
    assert!(greeting_en().starts_with("This is Testbrand LLC."));
    assert!(greeting_es().starts_with("Habla Testbrand LLC."));
    rebranded("greeting", &greeting_en());
    rebranded("saludo", &greeting_es());
}

/// TD-WORD-38 (WORD-008, INT-039): a question-bank text opens with the fake short name, in English
/// and Spanish.
#[test]
fn td_word_38_question_bank_text_takes_the_fake_brand() {
    fake_brand();
    let bank = question_bank().expect("bank");
    let words = "The house is warm and the system makes a noise";
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let scrubbed = scrub(words, &known, &mut NameMemory::default());
    let sources = WriteSources {
        words: &scrubbed,
        answers: &[],
        photo_notes: &[],
        business_names: &[],
    };
    for spanish in [false, true] {
        let set = &bank.sets["hvac_repair"];
        let mut run = BankRun::new(set, spanish, false, BankSettings::default());
        let Next::Ask(prompt) = run.next(words, &AiTurn::default(), &sources) else {
            panic!("a question");
        };
        let text = prompt.render();
        assert!(text.starts_with("Testbrand: "), "{text}");
        rebranded("question", &text);
    }
}

/// TD-WORD-38 (WORD-001, PICK-041): the quote card's check-it-yourself and independence labels
/// name the fake brand, in English and Spanish.
#[test]
fn td_word_38_quote_card_label_takes_the_fake_brand() {
    fake_brand();
    let card = QuoteCard {
        business_name: "Desert Sample Plumbing".to_owned(),
        license_lines: vec![CardLine {
            text: "No license needed for this job.".to_owned(),
            link: None,
        }],
        after_hours: false,
        format: Format::Firm,
        facts: QuoteFacts {
            trip_fee: TripFee::Counts,
            parts: Parts::Included,
            permit: Permit::NotNeeded,
            may_change: true,
            earliest_visit: (date(2026, 11, 3), DayPart::Morning),
            spanish: Spanish::Both,
        },
        original: "Replace the kitchen faucet".to_owned(),
        summary: None,
    };
    for language in [CardLanguage::English, CardLanguage::Spanish] {
        let lines = card.lines(language);
        rebranded("check-it-yourself label", &lines[2].text);
        rebranded("independence label", &lines[3].text);
    }
}

/// TD-WORD-38 (WORD-001, TERMS-016): on the embedded brand each legal record names its legal or
/// display name; under the fake brand the same check reports all three, so a switch fails until
/// they are re-issued under a new version.
#[test]
fn td_word_38_legal_records_name_the_embedded_brand() {
    let embedded = Brand::embedded().expect("embedded brand");
    assert_eq!(records_not_naming(&embedded), Vec::<&str>::new());
    assert_eq!(
        records_not_naming(fake_brand()),
        ["homeowner terms", "AUTHORIZATION_TEXT", "WORD-022 list"]
    );
}

/// TD-WORD-38 (FEE-015, TERMS-016): the authorization's SHA-256 matches the hash pinned for the
/// current version, and a changed text under the same version does not.
#[test]
fn td_word_38_authorization_text_is_pinned_per_version() {
    assert!(authorization_pinned(
        AUTHORIZATION_VERSION,
        AUTHORIZATION_TEXT
    ));
    let changed = AUTHORIZATION_TEXT.replace("any time", "anytime");
    assert_ne!(changed, AUTHORIZATION_TEXT);
    assert!(!authorization_pinned(AUTHORIZATION_VERSION, &changed));
    assert!(!authorization_pinned(
        AUTHORIZATION_VERSION + 1,
        AUTHORIZATION_TEXT
    ));
}

/// An open job and its quote-page token (fake secret and times).
async fn open_job(pool: &PgPool, now: Timestamp) -> LinkToken {
    let mut db = pool.acquire().await.expect("db");
    let job = create(&mut db, now, false, Actor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let _ = move_to(&mut db, job, open, now, Actor::System)
        .await
        .expect("open");
    let key = SecretString::from("fake-id-secret-for-tests-t295-0001".to_owned());
    let secret = IdSecret::new(1, key).expect("secret");
    issue(&mut db, &secret, job, now).await.expect("token")
}

/// TD-WORD-38 (WORD-001, WEB-001): the quote page and the site layout around it take the fake brand.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_word_38_quote_page_and_layout_take_the_fake_brand(pool: PgPool) {
    fake_brand();
    let now: Timestamp = "2026-11-02T21:00:00-07:00".parse().expect("time");
    let token = open_job(&pool, now).await;
    let request = Request::get(format!("/q/{}", token.as_str()))
        .header(header::HOST, "tb.example")
        .body(Body::empty())
        .expect("request");
    let app = quote_page_routes(pool.clone(), Arc::new(ManualClock::new(now)));
    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    let body = String::from_utf8(bytes.to_vec()).expect("utf8");
    assert!(body.contains("| Testbrand</title>"), "layout title");
    assert!(
        body.contains("<a href=\"/\">Testbrand</a>"),
        "layout home link"
    );
    assert!(body.contains("Testbrand doesn't rank businesses or show ratings."));
    rebranded("quote page", &body);
}
