//! Relay-friendly PIN prompts, the homeowner's code call, the safety voice call behind its switch
//! and Casey's bridge (TD-CALL-06: CALL-013 to CALL-016; TD-CALL-09: CALL-014, CALL-019,
//! CALL-022, CALL-023; TD-TXT-09's T080 cases: the call placed from intake's event; OPS-028;
//! D-366, D-384, D-385, D-485, D-491, D-1704 to D-1707). The suspension cases of TD-TXT-09 are
//! T096's. Fake numbers (555-01xx and the 500-555 test range) and IDs only.

use std::sync::Mutex;

use askhava_adapters::api::{
    AdapterError, CallState, IdempotencyKey, MaskedPool, PoolNumber, TwilioConfig, VoiceAdapter,
};
use askhava_calls::api::{
    AnsweredBy, Approval, CallGate, Conditions, PIN_PROMPT, Party, PinHeard, PinWait,
    SAFETY_CALL_ACTION, SAFETY_CALL_KIND, SAFETY_CLAIM_ACTION, SAFETY_FAILED_ACTION,
    SAFETY_KEY_ACTION, SAFETY_RESULT_ACTION, SAFETY_STOP_ACTION, SafetyCall, SafetyDialer,
    SafetyOutcome, SafetySwitch, VoiceError, code_call_twiml, dial_twiml, may_code_call,
    owner_bridge_twiml, pin_heard, pin_prompt_twiml, place_safety_call, ring_owner,
    safety_call_answered, safety_call_key, safety_script, safety_twiml,
};
use askhava_core::api::Settings;
use askhava_intake::api::rules::safety_call_trigger;
use askhava_intake::api::{SafetyCallAsk, enqueue_safety_call};
use askhava_ops::api::{ALERT_ACKNOWLEDGED_ACTION, NewAlert, Severity, raise_alert};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

const HER_PHONE: &str = "+19285550171";
const MAIN: &str = "+19285550100";
const CASEY: &str = "+19285550199";
/// 10:00 Arizona.
const DAY: &str = "2026-11-02T17:00:00Z";

fn ts() -> Timestamp {
    DAY.parse().expect("instant")
}

/// The one fake pool number.
fn pool_number() -> PoolNumber {
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
    let pool: MaskedPool = TwilioConfig::from_settings(&settings, pairs)
        .expect("pool")
        .pool()
        .clone();
    pool.numbers()[0].clone()
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x5a), &key(0x6a), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests-only-000000".to_owned())
}

/// A voice adapter that records each call (to, from) and places nothing.
#[derive(Default)]
struct FakeVoice(Mutex<Vec<(String, String)>>);

impl FakeVoice {
    fn calls(&self) -> Vec<(String, String)> {
        self.0.lock().expect("calls").clone()
    }
}

impl VoiceAdapter for FakeVoice {
    async fn open_call(
        &self,
        _key: &IdempotencyKey,
        to: &str,
        caller_id: &PoolNumber,
    ) -> Result<CallState, AdapterError> {
        let mut calls = self.0.lock().expect("calls");
        calls.push((to.to_owned(), caller_id.as_str().to_owned()));
        Ok(CallState {
            sid: format!("CA{:032}", calls.len()),
            status: "queued".to_owned(),
        })
    }

    async fn end_call(
        &self,
        _key: &IdempotencyKey,
        _call_sid: &str,
    ) -> Result<CallState, AdapterError> {
        Err(AdapterError::InvalidId)
    }
}

/// A voice adapter that counts each attempt and refuses it, as a provider outage would.
#[derive(Default)]
struct RefusingVoice(Mutex<usize>);

impl VoiceAdapter for RefusingVoice {
    async fn open_call(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        _caller_id: &PoolNumber,
    ) -> Result<CallState, AdapterError> {
        *self.0.lock().expect("tries") += 1;
        Err(AdapterError::InvalidId)
    }

    async fn end_call(
        &self,
        _key: &IdempotencyKey,
        _call_sid: &str,
    ) -> Result<CallState, AdapterError> {
        Err(AdapterError::InvalidId)
    }
}

/// Every CALL-022 item met.
fn all_met() -> Conditions {
    Conditions {
        narrowed_trigger: true,
        recent_only: true,
        script: true,
        other_number: true,
        list_checked: true,
        consent_text: true,
        logged: true,
    }
}

fn approval() -> Approval {
    Approval {
        date: "2026-10-30".parse().expect("date"),
        reference: "FAKE-REF-1".to_owned(),
    }
}

fn switched_on() -> SafetySwitch {
    SafetySwitch::turn_on(Some(&approval()), &all_met()).expect("on")
}

/// TD-CALL-06 (CALL-013, CALL-023, D-485): each PIN prompt is spoken words, waits the 10-second
/// setting, takes digits entered at 9 seconds, repeats once and then ends the call.
#[test]
fn td_call_06_each_pin_prompt_is_spoken_waits_10_seconds_and_repeats_once() {
    let wait = PinWait::default();
    assert_eq!(wait.seconds(), 10);
    let set = |v: &str| PinWait::from_pairs([("ASKHAVA_PIN_WAIT_SECONDS".into(), v.into())]);
    assert_eq!(set("20").expect("20").seconds(), 20);
    assert!(set("9").is_err() && set("ten").is_err() && set("61").is_err());
    assert_eq!(set("").expect("empty"), wait);
    assert_eq!(pin_heard(wait, Some(9), None), PinHeard::Digits);
    assert_eq!(pin_heard(wait, None, Some(9)), PinHeard::Digits);
    assert_eq!(pin_heard(wait, Some(10), None), PinHeard::Ended);
    assert_eq!(pin_heard(wait, None, None), PinHeard::Ended);
    let twiml = pin_prompt_twiml(wait, "/webhooks/twilio/voice/pin");
    assert_eq!(twiml.matches("timeout=\"10\"").count(), 2);
    assert_eq!(twiml.matches(PIN_PROMPT).count(), 2);
    assert!(twiml.ends_with("<Hangup/></Response>"));
    assert!(!twiml.contains("<Play") && !twiml.contains("<Dial"));
}

/// TD-CALL-06, TD-CALL-09 (CALL-014, CALL-015, D-366): the code call opens "This is Ask Hava LLC",
/// speaks slowly, says the code twice, reads the main number (not the calling one) and offers
/// "press 1 to hear it again".
#[test]
fn td_call_06_the_code_call_opens_with_ask_hava_llc_and_says_the_code_twice() {
    let from = pool_number();
    let code = SecretString::from("123456".to_owned());
    let twiml = code_call_twiml(&from, MAIN, &code, "/again").expect("twiml");
    let twiml = twiml.expose_secret();
    assert!(twiml.contains("<prosody rate=\"slow\">This is Ask Hava LLC."));
    assert_eq!(twiml.matches("1, 2, 3, 4, 5, 6").count(), 2);
    assert!(twiml.contains("dial 9 2 8, 5 5 5, 0 1 0 0."));
    assert!(
        !twiml.contains("5, 0, 0, 5, 5, 5"),
        "never the calling number"
    );
    assert!(twiml.contains("Press 1 to hear it again.") && twiml.contains("action=\"/again\""));
    assert!(code_call_twiml(&from, from.as_str(), &code, "/again").is_none());
    for bad in ["12", "+1234", "+123456", "12 34", "12-34"] {
        let code = SecretString::from(bad.to_owned());
        assert!(
            code_call_twiml(&from, MAIN, &code, "/again").is_none(),
            "{bad}"
        );
    }
}

/// TD-CALL-09 (CALL-014, BIZ-046): the code call only on her tap, never to a business.
#[test]
fn td_call_09_the_code_call_goes_only_on_her_tap_and_never_to_a_business() {
    let her = Party::Homeowner(Uuid::from_u128(1));
    assert!(may_code_call(true, her));
    assert!(!may_code_call(false, her));
    assert!(!may_code_call(true, Party::Business(Uuid::from_u128(2))));
    assert!(!may_code_call(true, Party::Unlinked));
}

/// TD-CALL-06 (CALL-016, D-359): no text slot carries a business's own words, the inbound route
/// marks a linked business phone a business, and `messaging` never sends from a pool number.
#[test]
fn td_call_06_no_route_lets_a_business_text_a_homeowner() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let read = |p: &str| std::fs::read_to_string(format!("{root}/{p}")).expect("file");
    let slots = [
        "{business}",
        "{count}",
        "{job}",
        "{link}",
        "{number}",
        "{quoted}",
        "{reply_time}",
        "{time}",
        "{trade}",
    ];
    for lang in ["en", "es"] {
        let text = read(&format!("app/messaging/templates/{lang}.json"));
        for slot in text.split('{').skip(1).filter_map(|s| s.split_once('}')) {
            let name = format!("{{{}}}", slot.0);
            let fixed = !slot.0.chars().all(|c| c.is_ascii_lowercase() || c == '_');
            assert!(fixed || slots.contains(&name.as_str()), "{name} in {lang}");
        }
    }
    let route = read("app/server/src/inbound_text.rs");
    assert!(route.contains("Some(id) => Ok((id, Relation::Business))"));
    for file in ["service.rs", "optout.rs", "api.rs"] {
        assert!(!read(&format!("app/messaging/src/{file}")).contains("PoolNumber"));
    }
}

/// TD-CALL-09 (CALL-019, CALL-022, D-275, D-384): the switch is off at launch and can't be turned
/// on while any condition is unmet, even with an approval recorded, nor without an approval.
#[test]
fn td_call_09_the_safety_switch_stays_off_while_any_condition_is_unmet() {
    assert!(!SafetySwitch::off().is_on() && !SafetySwitch::default().is_on());
    for unmet in 0..7 {
        let mut c = all_met();
        let item = [
            &mut c.narrowed_trigger,
            &mut c.recent_only,
            &mut c.script,
            &mut c.other_number,
            &mut c.list_checked,
            &mut c.consent_text,
            &mut c.logged,
        ];
        *item.into_iter().nth(unmet).expect("item") = false;
        assert!(SafetySwitch::turn_on(Some(&approval()), &c).is_none());
    }
    assert!(SafetySwitch::turn_on(None, &all_met()).is_none());
    let blank = Approval {
        reference: " ".to_owned(),
        ..approval()
    };
    assert!(SafetySwitch::turn_on(Some(&blank), &all_met()).is_none());
    assert!(switched_on().is_on());
}

/// TD-CALL-09 (CALL-022, D-384, D-1705): the voice trigger: gas, CO, smoke, fire, burning, spark
/// and shock words in English and Spanish, only on a 911-list match; never a drain smell or a bare
/// smell, odor, buzz, hot, alarm, flood, dizzy or heat; never a word that only starts like one
/// ("sparkling", "quemador") or a shock or "descarga" with no electric word; and never a shock
/// with no 911-list match, since then no INT-004 text and no critical alert exist (D-1705).
#[test]
fn td_call_09_only_danger_words_trigger_the_safety_call() {
    for yes in [
        "I smell gas",
        "huele a gas",
        "the CO detector is going off",
        "smoke in the kitchen",
        "there is a fire",
        "the outlet is burning",
        "sparks from the plug",
        "the smoke alarm is going off",
        "the power line is down and I got a shock from the wire",
        "power line sparking by the fence",
        "the breaker is sparking",
        "el enchufe se quema",
    ] {
        assert!(safety_call_trigger(yes), "{yes}");
    }
    for no in [
        "the drain smells",
        "weird odor in the garage",
        "the fan makes a buzz",
        "the room is hot",
        "the alarm",
        "the car alarm went off",
        "basement flood",
        "I feel dizzy",
        "no heat at all",
        "replace the spark plug",
        "100 Smoketree Test Ave",
        "sparkling clean windows please",
        "I was shocked by the price of the last quote",
        "el quemador de la estufa no prende",
        "me hice una quemadura con la plancha",
        "la descarga del inodoro no funciona",
        "new shocks for the trailer",
        "the power line is down",
        "I got a shock from the switch",
        "me dio una descarga el enchufe",
    ] {
        assert!(!safety_call_trigger(no), "{no}");
    }
}

/// TD-CALL-09 (CALL-019, CALL-022): the script starts "This is Ask Hava L L C", says Ask Hava is
/// not an emergency service, tells her to leave and call 911, reads the main number, offers
/// "press 9", holds no job detail and takes under 30 seconds at 2.5 words a second.
#[test]
fn td_call_09_the_safety_script_is_short_and_generic() {
    let from = pool_number();
    let words = safety_script(MAIN, &from).expect("script");
    assert!(words.starts_with("This is Ask Hava L L C."));
    for part in [
        "Ask Hava is not an emergency service.",
        "call 9 1 1",
        "dial 9 2 8, 5 5 5, 0 1 0 0",
        "Press 9 to stop all calls",
    ] {
        assert!(words.contains(part), "{part}");
    }
    for never in ["job", "quote", "request"] {
        assert!(!words.to_lowercase().contains(never), "{never}");
    }
    assert!(words.split_whitespace().count() < 75, "under 30 seconds");
    assert!(
        safety_script(from.as_str(), &from).is_none(),
        "never the calling number"
    );
    let twiml = safety_twiml(MAIN, &from, "/stop").expect("twiml");
    assert!(twiml.contains("numDigits=\"1\"") && twiml.contains("action=\"/stop\""));
}

/// TD-CALL-09 (CALL-023, OPS-028, D-385): a bridged masked call plays nothing to the homeowner (no
/// whisper `url` on the dialed number, no `Say` or `Play`), and Casey's bridge shows the Ask Hava
/// number, reads the safety words to him only, and is never recorded.
#[test]
fn td_call_09_a_bridged_call_plays_nothing_to_the_homeowner() {
    let from = pool_number();
    let her = SecretString::from(HER_PHONE.to_owned());
    let bridged = dial_twiml(&from, &her).expect("dial");
    let bridged = bridged.expose_secret();
    for verb in ["url=", "<Say", "<Play", "<Gather"] {
        assert!(!bridged.contains(verb), "{verb}");
    }
    let words = "Ask Hava: If you are in danger, leave now & call 911.";
    let owner = owner_bridge_twiml(&from, &her, words).expect("bridge");
    let owner = owner.expose_secret();
    let (said, dialed) = owner.split_once("<Dial").expect("dial");
    assert!(said.contains("leave now &amp; call 911."));
    assert!(dialed.contains("callerId=\"+15005550100\"") && dialed.contains("do-not-record"));
    assert!(!owner.contains(CASEY) && !dialed.contains("url="));
}

/// OPS-028 (D-1707): "Call from the Ask Hava number" rings Casey's phone from the pool number.
#[tokio::test]
async fn ops_028_the_owner_bridge_rings_caseys_phone_from_the_ask_hava_number() {
    let (voice, from) = (FakeVoice::default(), pool_number());
    let casey = SecretString::from(CASEY.to_owned());
    let ids = (Uuid::from_u128(7), Uuid::from_u128(8));
    let sid = ring_owner(&voice, &secret(), (&casey, &from), ids)
        .await
        .expect("rung");
    assert!(sid.starts_with("CA"));
    assert_eq!(
        voice.calls(),
        vec![(CASEY.to_owned(), from.as_str().to_owned())]
    );
}

/// D-491: intake's event kind is the one `calls` handles.
#[test]
fn td_txt_09_intakes_event_is_the_calls_handler_kind() {
    assert_eq!(askhava_intake::api::SAFETY_CALL_KIND, SAFETY_CALL_KIND);
}

async fn her(db: &mut PgConnection) -> Uuid {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(HER_PHONE.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    store_contact(db, &vault(), &new).await.expect("contact")
}

async fn critical(db: &mut PgConnection, contact: Uuid) -> Uuid {
    let alert = NewAlert {
        kind: "messaging.opted_out_emergency",
        severity: Severity::Critical,
        subject: Some(("privacy.contact", contact)),
    };
    raise_alert(db, &alert).await.expect("alert")
}

async fn count(db: &mut PgConnection, sql: &'static str, value: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(value)
        .fetch_one(db)
        .await
        .expect("count")
}

/// Queues intake's event for `words` with the switch `on`; whether a task was queued.
async fn ask(db: &mut PgConnection, words: &str, ask: SafetyCallAsk, on: bool) -> bool {
    let mut tx = db.begin().await.expect("tx");
    let queued = enqueue_safety_call(&mut tx, words, ask, on)
        .await
        .expect("enqueue");
    tx.commit().await.expect("commit");
    queued
}

/// TD-CALL-09, TD-TXT-09 (CALL-019, CALL-022, CON-035, D-491): with the switch on, "drain smells"
/// queues nothing, a second message about the same emergency queues no second call, a 31-minute-old
/// "I smell gas" places no call, a fresh one places exactly one call from a non-main number, logged;
/// "press 9" lists her number and no later call goes; with the switch off nothing is queued or placed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_09_one_safety_call_from_a_non_main_number_and_press_9_stops_calls(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let contact = her(&mut db).await;
    let alert = critical(&mut db, contact).await;
    let message = Uuid::new_v4();
    let call_ask = SafetyCallAsk {
        contact,
        message,
        arrived_at: ts(),
        alert,
    };
    assert!(
        !ask(&mut db, "I smell gas", call_ask, false).await,
        "switch off"
    );
    assert!(!ask(&mut db, "the drain smells", call_ask, true).await);
    assert!(ask(&mut db, "I smell gas", call_ask, true).await);
    let again = SafetyCallAsk {
        message: Uuid::new_v4(),
        ..call_ask
    };
    assert!(
        !ask(&mut db, "still smell gas", again, true).await,
        "same emergency"
    );
    let kind_sql = "SELECT count(*) FROM core.task WHERE kind = $1";
    assert_eq!(count(&mut db, kind_sql, SAFETY_CALL_KIND).await, 1);
    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1")
            .bind(SAFETY_CALL_KIND)
            .fetch_one(&mut *db)
            .await
            .expect("payload");
    let queued = SafetyCall::from_payload(&payload).expect("payload parses");

    let (voice, from, key) = (FakeVoice::default(), pool_number(), secret());
    let call = SafetyCall {
        contact,
        message,
        arrived_at: ts(),
        alert,
    };
    assert_eq!(queued, call);
    let mut dialer = SafetyDialer {
        voice: &voice,
        from: &from,
        main: MAIN,
        switch: SafetySwitch::off(),
        secret: &key,
    };
    let now = ts() + SignedDuration::from_mins(5);
    let off = place_safety_call(&mut db, &vault(), &dialer, &call, now).await;
    assert_eq!(off, Ok(SafetyOutcome::SwitchOff));
    dialer.switch = switched_on();
    let late = ts() + SignedDuration::from_mins(31);
    let old = place_safety_call(&mut db, &vault(), &dialer, &call, late).await;
    assert_eq!(old, Ok(SafetyOutcome::TooLate));
    let main_from = SafetyDialer {
        main: from.as_str(),
        ..dialer
    };
    let same = place_safety_call(&mut db, &vault(), &main_from, &call, now).await;
    assert_eq!(same, Ok(SafetyOutcome::FromMain));
    assert!(voice.calls().is_empty());
    let placed = place_safety_call(&mut db, &vault(), &dialer, &call, now)
        .await
        .expect("placed");
    assert!(matches!(placed, SafetyOutcome::Placed(_)));
    assert_eq!(
        voice.calls(),
        vec![(HER_PHONE.to_owned(), from.as_str().to_owned())]
    );
    let audit_sql = "SELECT count(*) FROM ops.audit_log WHERE action = $1";
    assert_eq!(count(&mut db, audit_sql, SAFETY_CALL_ACTION).await, 1);
    let retried = place_safety_call(&mut db, &vault(), &dialer, &call, now).await;
    assert_eq!(retried, Ok(SafetyOutcome::AlreadyPlaced), "a retried task");
    assert_eq!(voice.calls().len(), 1);
    assert_eq!(count(&mut db, audit_sql, SAFETY_CLAIM_ACTION).await, 1);

    assert_eq!(
        safety_call_key(&mut db, &vault(), &call, "1", now).await,
        Ok(false)
    );
    assert_eq!(
        safety_call_key(&mut db, &vault(), &call, "9", now).await,
        Ok(true)
    );
    let after = place_safety_call(&mut db, &vault(), &dialer, &call, now).await;
    assert_eq!(after, Ok(SafetyOutcome::Refused(CallGate::OptedOut)));
    assert_eq!(voice.calls().len(), 1);
}

/// OPS-028 (D-1706): a safety call answered by a person acknowledges Casey's critical alert once,
/// with one audit row for the digest; a machine or nobody leaves it open.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ops_028_a_person_answering_acknowledges_the_critical_alert(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let contact = her(&mut db).await;
    let alert = critical(&mut db, contact).await;
    let call = SafetyCall {
        contact,
        message: Uuid::new_v4(),
        arrived_at: ts(),
        alert,
    };
    for nobody in [AnsweredBy::Machine, AnsweredBy::Nobody, AnsweredBy::Unknown] {
        assert_eq!(
            safety_call_answered(&mut db, &call, nobody, ts()).await,
            Ok(false)
        );
    }
    assert_eq!(
        safety_call_answered(&mut db, &call, AnsweredBy::Person, ts()).await,
        Ok(true)
    );
    assert_eq!(
        safety_call_answered(&mut db, &call, AnsweredBy::Person, ts()).await,
        Ok(false)
    );
    let audit_sql = "SELECT count(*) FROM ops.audit_log WHERE action = $1";
    assert_eq!(
        count(&mut db, audit_sql, ALERT_ACKNOWLEDGED_ACTION).await,
        1
    );
    let open = "SELECT count(*) FROM ops.alert WHERE resolved_at IS NULL AND kind = $1";
    assert_eq!(
        count(&mut db, open, "messaging.opted_out_emergency").await,
        0
    );
}

/// TD-CALL-09 (CALL-019, CALL-022, JOB-006, D-1705): a shock with no 911-list match queues no
/// call; a provider refusal is logged against the message and the retried task dials nothing
/// more (one call per emergency); every key and every result is logged, never only "9" or a
/// person.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_09_each_safety_call_is_claimed_once_and_every_key_and_result_is_logged(
    db: PgPool,
) {
    let mut db = db.acquire().await.expect("connection");
    let contact = her(&mut db).await;
    let alert = critical(&mut db, contact).await;
    let message = Uuid::new_v4();
    let call_ask = SafetyCallAsk {
        contact,
        message,
        arrived_at: ts(),
        alert,
    };
    assert!(!ask(&mut db, "I got a shock from the switch", call_ask, true).await);
    assert!(!ask(&mut db, "me dio una descarga", call_ask, true).await);

    let (voice, from, key) = (RefusingVoice::default(), pool_number(), secret());
    let dialer = SafetyDialer {
        voice: &voice,
        from: &from,
        main: MAIN,
        switch: switched_on(),
        secret: &key,
    };
    let call = SafetyCall {
        contact,
        message,
        arrived_at: ts(),
        alert,
    };
    let now = ts() + SignedDuration::from_mins(2);
    let refused = place_safety_call(&mut db, &vault(), &dialer, &call, now).await;
    assert_eq!(refused, Err(VoiceError::Provider));
    let retried = place_safety_call(&mut db, &vault(), &dialer, &call, now).await;
    assert_eq!(retried, Ok(SafetyOutcome::AlreadyPlaced));
    assert_eq!(*voice.0.lock().expect("tries"), 1, "dialled once");
    let audit_sql = "SELECT count(*) FROM ops.audit_log WHERE action = $1";
    assert_eq!(count(&mut db, audit_sql, SAFETY_FAILED_ACTION).await, 1);
    assert_eq!(count(&mut db, audit_sql, SAFETY_CALL_ACTION).await, 0);

    for (digits, field) in [
        ("1", "key_1"),
        ("#", "key_hash"),
        ("12", "key_other"),
        ("9", "key_9"),
    ] {
        safety_call_key(&mut db, &vault(), &call, digits, now)
            .await
            .expect("key");
        let row: (String, Option<String>) = sqlx::query_as(
            "SELECT action, field FROM ops.audit_log WHERE subject_id = $1 ORDER BY id DESC LIMIT 1",
        )
        .bind(message)
        .fetch_one(&mut *db)
        .await
        .expect("key row");
        let action = if digits == "9" {
            SAFETY_STOP_ACTION
        } else {
            SAFETY_KEY_ACTION
        };
        assert_eq!(row, (action.to_owned(), Some(field.to_owned())), "{digits}");
    }

    for answered in [
        AnsweredBy::Machine,
        AnsweredBy::Nobody,
        AnsweredBy::Unknown,
        AnsweredBy::Person,
    ] {
        safety_call_answered(&mut db, &call, answered, now)
            .await
            .expect("answered");
    }
    let results: Vec<String> = sqlx::query_scalar(
        "SELECT to_status FROM ops.audit_log WHERE action = $1 AND subject_id = $2 ORDER BY id",
    )
    .bind(SAFETY_RESULT_ACTION)
    .bind(message)
    .fetch_all(&mut *db)
    .await
    .expect("results");
    assert_eq!(results, ["machine", "nobody", "unknown", "person"]);
}
