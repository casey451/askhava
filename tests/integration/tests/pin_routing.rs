//! PIN routing, the paid-business read-back and the who's-calling text on a fresh database
//! (TD-CALL-02: CALL-003, CALL-004, CALL-005; TD-CALL-11: CALL-025; TD-TXT-29: TXT-027; TD-QH-04:
//! QH-004, TXT-027; TD-CALL-12: INT-029, CALL-001; D-026, D-480, D-491, D-1684 to D-1688).
//! Fake numbers (555-01xx and the 500-555 test range), fake names and IDs only.

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_calls::api::{
    Answer, Attestation, CallTurn, Connected, IncomingCall, JobPin, Line, PIN_ALERT, Party,
    RoutedLine, Step, WHOS_CALLING_KIND, WhosCalling, WhosCallingSent, answer_call, assign_number,
    job_pin, route, send_whos_calling,
};
use askhava_core::api::Settings;
use askhava_messaging::api::{Language, sender_add};
use askhava_picks::api::{ReleaseFacts, ReleaseStep, plan_release};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const HER_PHONE: &str = "+19285550161";
const BUSINESS_PHONE: &str = "+19285550162";
const STRANGER_PHONE: &str = "+19285550163";
/// 10:00 Arizona.
const DAY: &str = "2026-11-02T17:00:00Z";
/// 23:10 Arizona, and 08:00 Arizona the next morning.
const NIGHT: &str = "2026-11-03T06:10:00Z";
const MORNING: &str = "2026-11-03T15:00:00Z";

fn ts(at: &str) -> Timestamp {
    at.parse().expect("instant")
}

/// A pool of one fake number, so every line shares it.
fn one_number_pool() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
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
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    TwilioConfig::from_settings(&settings, pairs)
        .expect("pool")
        .pool()
        .clone()
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x57), &key(0x67), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("fake-outbox-secret-0123456789abcdef".to_owned())
}

/// Her fake contact: name, phone and address.
async fn her(db: &mut PgConnection) -> Uuid {
    let s = |v: &str| Some(SecretString::from(v.to_owned()));
    let new = NewContact {
        name: s("Testa Example"),
        phone: s(HER_PHONE),
        email: None,
        address: s("123 Fake St, Lake Havasu City, AZ 86403"),
        onsite_contact: None,
    };
    store_contact(db, &vault(), &new).await.expect("contact")
}

async fn line(db: &mut PgConnection, pool: &MaskedPool, (h, b): (Uuid, Uuid)) -> Line {
    let job = Connected {
        job: Uuid::new_v4(),
        homeowner: h,
        business: b,
    };
    assign_number(db, pool, job, ts(DAY)).await.expect("assign")
}

fn turn(attestation: Attestation, tries: (u8, u8), entered: Option<JobPin>) -> CallTurn {
    CallTurn {
        attestation,
        tries_call: tries.0,
        tries_today: tries.1,
        entered,
    }
}

/// A PIN that is not `line`'s.
fn wrong(line: Uuid) -> Option<JobPin> {
    let right = job_pin(line).digits();
    let other = if right == "0000" { "0001" } else { "0000" };
    JobPin::parse(other)
}

fn routed(line: u128, homeowner: Uuid, business: Uuid) -> RoutedLine {
    RoutedLine {
        line: Uuid::from_u128(line),
        homeowner,
        business,
    }
}

async fn count(db: &mut PgConnection, sql: &'static str, bind: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(bind)
        .fetch_one(db)
        .await
        .expect("count")
}

/// TD-CALL-02 (CALL-003): an unlinked phone must enter the PIN; a wrong PIN is refused and never
/// connects, the right one connects; a malformed entry is no PIN.
#[test]
fn td_call_02_an_unlinked_phone_needs_the_pin_and_a_wrong_one_is_refused() {
    let l = routed(1_234, Uuid::from_u128(1), Uuid::from_u128(2));
    let a = Attestation::A;
    assert_eq!(
        route(Party::Unlinked, &[l], turn(a, (0, 0), None)),
        Step::AskPin
    );
    assert_eq!(
        route(Party::Unlinked, &[l], turn(a, (0, 0), wrong(l.line))),
        Step::WrongPin
    );
    let right = Some(job_pin(l.line));
    assert_eq!(
        route(Party::Unlinked, &[l], turn(a, (0, 0), right)),
        Step::Connect(l.line)
    );
    assert_eq!(job_pin(l.line).digits(), "1234");
    for bad in ["123", "12345", "12a4", "１２３４"] {
        assert_eq!(JobPin::parse(bad), None);
    }
}

/// TD-CALL-02 (CALL-004): a homeowner or a business with two jobs on one number is routed only by
/// PIN, to the job the PIN names; with one job it connects straight away.
#[test]
fn td_call_02_two_jobs_on_one_number_are_routed_only_by_pin() {
    let (h, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
    let lines = [routed(1_111, h, b), routed(2_222, h, b)];
    let a = Attestation::A;
    for party in [Party::Homeowner(h), Party::Business(b)] {
        assert_eq!(route(party, &lines, turn(a, (0, 0), None)), Step::AskPin);
        let pin = Some(job_pin(lines[1].line));
        assert_eq!(
            route(party, &lines, turn(a, (0, 0), pin)),
            Step::Connect(lines[1].line)
        );
        assert_eq!(
            route(party, &lines[..1], turn(a, (0, 0), None)),
            Step::Connect(lines[0].line)
        );
    }
}

/// TD-CALL-11 (CALL-025): 10 wrong PINs from one unlinked number never connect; each call is
/// dropped after its third wrong try (no 4th attempt), and once 6 were used that day the next
/// attempt is refused before the prompt.
#[test]
fn td_call_11_ten_wrong_pins_never_connect_and_the_7th_of_the_day_is_refused() {
    let l = routed(5_555, Uuid::from_u128(1), Uuid::from_u128(2));
    let (mut today, mut made, mut hang_ups) = (0_u8, 0, 0);
    'calls: while made < 10 {
        for call_try in 0..3_u8 {
            match route(
                Party::Unlinked,
                &[l],
                turn(Attestation::A, (call_try, today), wrong(l.line)),
            ) {
                Step::Refuse => break 'calls,
                Step::Connect(_) | Step::ReadBack(_) => panic!("a wrong PIN went through"),
                Step::HangUp => {
                    (today, made, hang_ups) = (today + 1, made + 1, hang_ups + 1);
                    continue 'calls;
                }
                Step::WrongPin | Step::AskPin => (today, made) = (today + 1, made + 1),
            }
        }
        panic!("a call took a 4th try");
    }
    assert_eq!((today, hang_ups), (6, 2));
    assert_eq!(
        route(Party::Unlinked, &[l], turn(Attestation::A, (0, 6), None)),
        Step::Refuse
    );
}

/// TD-CALL-11, TD-CALL-02 (CALL-005, CALL-025, D-026): after 2 bad PINs a phone linked to the
/// business holding the lines hears the read-back only on an A-attested call; B or C, and any
/// unlinked phone, get PIN routing only.
#[test]
fn td_call_11_only_attestation_a_gets_the_read_back() {
    let (h, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
    let lines = [routed(1_111, h, b), routed(2_222, h, b)];
    let bad = JobPin::parse("9999");
    let linked = Party::Business(b);
    let a = route(
        linked,
        &lines,
        turn(Attestation::parse(Some("A")), (1, 1), bad),
    );
    assert_eq!(a, Step::ReadBack(lines[1].line));
    for other in [Some("B"), Some("C"), None] {
        let step = route(linked, &lines, turn(Attestation::parse(other), (1, 1), bad));
        assert_eq!(step, Step::WrongPin);
    }
    let stranger = route(Party::Unlinked, &lines, turn(Attestation::A, (1, 1), bad));
    assert_eq!(stranger, Step::WrongPin);
    let first = route(linked, &lines, turn(Attestation::A, (0, 0), bad));
    assert_eq!(first, Step::WrongPin);
}

/// TD-CALL-02 (CALL-005, CALL-002, D-026): on the database, the paid business's linked phone hears
/// her first name and street only (never the town or surname), audited; an unlinked phone hears
/// nothing; the line's number is never her phone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_02_the_read_back_goes_only_to_the_paid_business(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let (pool, vault) = (one_number_pool(), vault());
    let (contact, business) = (her(&mut db).await, Uuid::new_v4());
    let first = line(&mut db, &pool, (contact, business)).await;
    let second = line(&mut db, &pool, (contact, business)).await;
    assert_ne!(second.number().as_str(), HER_PHONE);
    sender_add(&mut db, &vault, (business, Uuid::new_v4()), BUSINESS_PHONE)
        .await
        .expect("link");
    let bad = wrong(first.id()).filter(|p| *p != job_pin(second.id()));
    let ask = |phone: &'static str| (SecretString::from(phone.to_owned()), bad);
    let (phone, pin) = ask(BUSINESS_PHONE);
    let call = IncomingCall {
        pool_number: first.number().as_str(),
        caller: &phone,
    };
    let heard = answer_call(&mut db, &vault, &call, turn(Attestation::A, (1, 1), pin)).await;
    let Ok(Answer::ReadBack(id, words)) = heard else {
        panic!("no read-back")
    };
    assert_eq!(id, second.id());
    assert_eq!(
        words
            .first_name()
            .map(|w| w.expose_secret().to_owned())
            .as_deref(),
        Some("Testa")
    );
    assert_eq!(
        words
            .street()
            .map(|w| w.expose_secret().to_owned())
            .as_deref(),
        Some("123 Fake St")
    );
    let audited = "SELECT count(*) FROM ops.audit_log WHERE action = $1";
    assert_eq!(
        count(&mut db, audited, "vault_read.calls.call_readback").await,
        2
    );
    let (phone, pin) = ask(STRANGER_PHONE);
    let call = IncomingCall {
        pool_number: first.number().as_str(),
        caller: &phone,
    };
    let heard = answer_call(&mut db, &vault, &call, turn(Attestation::A, (1, 1), pin)).await;
    assert!(matches!(heard, Ok(Answer::Step(Step::WrongPin))));
}

/// TD-CALL-11 (CALL-025): the third wrong try of a call hangs up and raises one alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_11_a_hang_up_raises_one_alert(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let (pool, vault) = (one_number_pool(), vault());
    let contact = her(&mut db).await;
    let l = line(&mut db, &pool, (contact, Uuid::new_v4())).await;
    let phone = SecretString::from(STRANGER_PHONE.to_owned());
    let call = IncomingCall {
        pool_number: l.number().as_str(),
        caller: &phone,
    };
    let heard = answer_call(
        &mut db,
        &vault,
        &call,
        turn(Attestation::A, (2, 2), wrong(l.id())),
    )
    .await;
    assert!(matches!(heard, Ok(Answer::Step(Step::HangUp))));
    let alerts = "SELECT count(*) FROM ops.alert WHERE kind = $1";
    assert_eq!(count(&mut db, alerts, PIN_ALERT).await, 1);
}

fn text(contact: Uuid, emergency: bool, email_chosen: bool) -> WhosCalling {
    WhosCalling {
        number: "+15005550100".to_owned(),
        job: Uuid::new_v4(),
        job_number: 1042,
        contact,
        business: "Test Plumbing".to_owned(),
        emergency,
        email_chosen,
        language: (Language::En, false),
    }
}

async fn body(db: &mut PgConnection, template: &str) -> String {
    sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE template_id = $1")
        .bind(template)
        .fetch_one(db)
        .await
        .expect("outbox row")
}

/// TD-QH-04, TD-TXT-29 (QH-004, TXT-027): at 23:10 on an emergency job the who's-calling text goes
/// at once, naming the business and the masked number with the call-back and "in writing" lines
/// and no call window; the same text on a regular job is held to 08:00:00.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_qh_04_the_whos_calling_text_goes_at_night_only_on_an_emergency(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let (vault, secret) = (vault(), secret());
    let contact = her(&mut db).await;
    let urgent = text(contact, true, false);
    let sent = send_whos_calling(&mut db, (&vault, &secret), &urgent, ts(NIGHT)).await;
    assert_eq!(sent, Ok(WhosCallingSent::Queued));
    let words = body(&mut db, "whos_calling").await;
    for part in [
        "Test Plumbing",
        "+15005550100",
        "request 1042",
        "If you miss it, call them back on this number. It works for 30 days.",
        "Ask for the price and any changes in writing before work starts. Don't pay in full up front.",
    ] {
        assert!(words.contains(part), "missing {part}");
    }
    assert!(!words.contains(" a.m.") && !words.contains(" p.m."));
    let regular = text(contact, false, false);
    let held = send_whos_calling(&mut db, (&vault, &secret), &regular, ts(NIGHT)).await;
    assert_eq!(held, Ok(WhosCallingSent::Held(ts(MORNING))));
    let due: Timestamp = sqlx::query_scalar("SELECT run_at FROM core.task WHERE kind = $1")
        .bind(WHOS_CALLING_KIND)
        .fetch_one(&mut *db)
        .await
        .map(|t: jiff_sqlx::Timestamp| t.to_jiff())
        .expect("held task");
    assert_eq!(due, ts(MORNING));
}

/// TD-TXT-29 (TXT-027, PICK-018): an email-contact homeowner still gets a text, which says
/// "They'll email you at the address you chose." and never "call".
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_29_an_email_contact_homeowner_gets_the_email_wording(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let (vault, secret) = (vault(), secret());
    let chose_email = text(her(&mut db).await, false, true);
    let sent = send_whos_calling(&mut db, (&vault, &secret), &chose_email, ts(DAY)).await;
    assert_eq!(sent, Ok(WhosCallingSent::Queued));
    let words = body(&mut db, "whos_calling_email").await;
    assert_eq!(
        words,
        "Ask Hava: Test Plumbing accepted request 1042. They'll email you at the address you \
         chose. Ask for the price and any changes in writing before work starts. Don't pay in \
         full up front."
    );
    assert!(!words.to_lowercase().contains("call"));
}

/// TD-CALL-12 (INT-029, CALL-001): an emergency job with no address is not released, so no masked
/// line opens; once the address is in it is released and the line opens.
#[test]
fn td_call_12_no_masked_line_before_an_emergency_address() {
    let facts = |address_in| ReleaseFacts {
        released: false,
        ended: false,
        confirmed: true,
        emergency: true,
        address_in,
    };
    assert_eq!(
        plan_release(facts(Some(false))),
        ReleaseStep::WaitForAddress
    );
    assert_eq!(plan_release(facts(Some(true))), ReleaseStep::Release);
}
