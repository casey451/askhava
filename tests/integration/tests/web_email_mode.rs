//! Web-and-email mode, a suspended number and the one configured number (TD-TXT-08, TD-TXT-09,
//! TD-TXT-12, TD-TXT-16, TD-TXT-17, TD-CON-16; TXT-021 to TXT-024, TXT-034, CON-033, D-1804 to
//! D-1808).
//!
//! `messaging::api`'s mode rules decide the routes and `send_on_line` sends through a recording
//! fake carrier; on a suspended number the safety voice call is intake's `calls.safety_call`
//! event, queued with `raise_emergency`'s `voice_call` as its switch, which T080's handler places
//! from a pool number other than the main number (`tests/integration/tests/voice_calls.rs`).
//! Fake numbers only.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, IdempotencyKey, SentText, SmsAdapter};
use askhava_calls::api::{Approval, Conditions, SAFETY_CALL_KIND, SafetySwitch};
use askhava_intake::api::rules::{Check, check, safety_call_trigger};
use askhava_intake::api::{SafetyCallAsk, enqueue_safety_call};
use askhava_messaging::api::{
    Audience, EmergencyFacts, LaunchChannel, Line, LineSendError, Lines, ModeError, NewText,
    NumberState, PAUSED_EMERGENCY_ALERT, Raised, Refusal, Relation, SendingNumber, To, TollFree,
    UpdateRoute, emergency_plan, enqueue, raise_emergency, send_on_line, update_route,
};
use askhava_ops::api::{Actor, Switch, set_switch};
use askhava_picks::api::{CodeRoute, EmailFrom, Texting, code_route};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

const HOMEOWNER_NUMBER: &str = "+19285550100";
const BUSINESS_NUMBER: &str = "+19285550110";
const HER: Uuid = Uuid::from_u128(7);

fn at(mins: i64) -> Timestamp {
    "2026-11-02T17:00:00Z"
        .parse::<Timestamp>()
        .expect("instant")
        + SignedDuration::from_mins(mins)
}

fn number(raw: &str) -> SendingNumber {
    SendingNumber::parse(raw).expect("number")
}

fn lines() -> Lines {
    Lines {
        homeowner: Line::at_launch(LaunchChannel::Sms, Some(number(HOMEOWNER_NUMBER))),
        business: Line::at_launch(LaunchChannel::Sms, Some(number(BUSINESS_NUMBER))),
    }
}

/// The homeowner line suspended at minute 0.
fn suspended() -> Line {
    let mut line = lines().homeowner;
    line.state = line.state.suspend(at(0));
    line
}

/// One message: its words and the facts intake and the caller read from it.
fn case(words: &str, mins: i64, has_email: bool, switch_on: bool) -> (&str, EmergencyFacts) {
    let facts = EmergencyFacts {
        matched: matches!(check(words, false), Check::Match(_)),
        call_trigger: safety_call_trigger(words),
        arrived_at: at(mins),
        has_email,
        opted_out: false,
        safety_switch_on: switch_on,
    };
    (words, facts)
}

/// Runs message `message` from `HER` as the paused path does, in one transaction: the plan,
/// `raise_emergency`, and intake's event with the raised alert and `voice_call`.
async fn handle(
    db: &mut PgConnection,
    line: &Line,
    (words, facts): (&str, EmergencyFacts),
    message: Uuid,
) -> Option<Raised> {
    let plan = emergency_plan(line, &facts)?;
    let mut tx = db.begin().await.expect("tx");
    let raised = raise_emergency(&mut tx, (&plan, line), (HER, message))
        .await
        .expect("raise");
    if let Some(Raised {
        alert: Some(alert),
        voice_call,
    }) = raised
    {
        let ask = SafetyCallAsk {
            contact: HER,
            message,
            arrived_at: facts.arrived_at,
            alert,
        };
        enqueue_safety_call(&mut tx, words, ask, voice_call)
            .await
            .expect("enqueue");
    }
    tx.commit().await.expect("commit");
    raised
}

async fn count(db: &mut PgConnection, sql: &'static str, value: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(value)
        .fetch_one(db)
        .await
        .expect("count")
}

/// Queued safety calls and raised paused-emergency alerts.
async fn tally(db: &mut PgConnection) -> (i64, i64) {
    let calls = "SELECT count(*) FROM core.task WHERE kind = $1";
    let alerts = "SELECT count(*) FROM ops.alert WHERE kind = $1 AND severity = 'critical'";
    (
        count(db, calls, SAFETY_CALL_KIND).await,
        count(db, alerts, PAUSED_EMERGENCY_ALERT).await,
    )
}

/// TD-TXT-08 (TXT-021, TXT-023, TXT-034, D-297, D-1804): started in web-and-email mode, or
/// switched off, no homeowner text goes; updates go by email, or onto the call list without one,
/// and the pick code goes by email to her conversation address.
#[test]
fn td_txt_08_web_and_email_mode_stops_every_homeowner_text() {
    let launched = Line::at_launch(LaunchChannel::WebAndEmail, Some(number(HOMEOWNER_NUMBER)));
    let unnumbered = Line::at_launch(LaunchChannel::Sms, None);
    assert!(!unnumbered.texting_on);
    let mut switched = lines();
    switched.homeowner.texting_on = false;
    for homeowner in [launched, unnumbered, switched.homeowner.clone()] {
        assert_eq!(homeowner.sender(), None);
        let all = Lines {
            homeowner,
            business: lines().business,
        };
        assert_eq!(
            update_route(&all, Audience::Homeowner, true),
            UpdateRoute::Email
        );
        assert_eq!(
            update_route(&all, Audience::Homeowner, false),
            UpdateRoute::CallList
        );
        let texting = if all.homeowner.sender().is_some() {
            Texting::Open
        } else {
            Texting::Off
        };
        assert_eq!(
            code_route(texting, Some(EmailFrom::Conversation)),
            CodeRoute::Email
        );
    }
    assert_eq!(
        SendingNumber::parse("9285550100"),
        Err(ModeError::BadNumber)
    );
}

/// TD-TXT-08 (TXT-022, TXT-023): a suspension pauses texting on the number, updates go by email
/// and a homeowner with no email goes on Casey's call list; a business never does (BIZ-046).
#[test]
fn td_txt_08_a_suspension_uses_email_and_fills_the_call_list() {
    let mut all = lines();
    all.homeowner = suspended();
    assert_eq!(all.homeowner.sender(), None);
    assert_eq!(
        update_route(&all, Audience::Homeowner, true),
        UpdateRoute::Email
    );
    assert_eq!(
        update_route(&all, Audience::Homeowner, false),
        UpdateRoute::CallList
    );
    all.business.state = all.business.state.suspend(at(0));
    assert_eq!(
        update_route(&all, Audience::Business, false),
        UpdateRoute::PortalOnly
    );
}

/// A fake carrier recording the `From` of every text it accepts.
#[derive(Clone, Default)]
struct FakeCarrier(Arc<Mutex<Vec<String>>>);

impl SmsAdapter for FakeCarrier {
    async fn send_text(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        from: &str,
        _body: &str,
    ) -> Result<SentText, AdapterError> {
        let mut sent = self.0.lock().expect("lock");
        sent.push(from.to_owned());
        Ok(SentText {
            sid: format!("SM{:032}", sent.len()),
            status: "queued".to_owned(),
        })
    }
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x52), &key(0x62), &url).expect("vault")
}

/// Queues text `job` for `recipient` and sends it on `all`'s line for `to`; the `From` the
/// carrier saw, or the refusal.
async fn send(
    db: &mut PgConnection,
    (carrier, vault): (&FakeCarrier, &Vault),
    all: &Lines,
    (recipient, to): (Uuid, To<'_>),
    job: u128,
) -> Result<String, LineSendError> {
    let relation = match to {
        To::Homeowner { relation, .. } => relation,
        To::Business(_) => Relation::Business,
    };
    let secret = SecretString::from("fake-id-secret-for-tests".to_owned());
    let new = NewText {
        template_id: "fake_update",
        subject: &[Uuid::from_u128(job)],
        recipient,
        relation,
        body: "Ask Hava: fake body",
    };
    let id = enqueue(db, &secret, &new).await.expect("queued").0;
    send_on_line(db, (&secret, carrier, vault), all, (id, to), at(0)).await?;
    Ok(carrier
        .0
        .lock()
        .expect("lock")
        .last()
        .expect("sent")
        .clone())
}

/// TD-TXT-08 property (TXT-021, TXT-022, TXT-035, OPS-003): over every state of both lines and
/// the homeowner switch, each text the carrier receives comes from its audience's configured
/// number and a closed line sends nothing; the owner's texting pause stops both lines.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_08_every_outbound_text_uses_the_configured_number(pool: PgPool) {
    let (carrier, vault) = (FakeCarrier::default(), vault());
    let mut db = pool.acquire().await.expect("connection");
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from("+19285550142".to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(&mut db, &vault, &new).await.expect("contact");
    let home = To::Homeowner {
        vault: &vault,
        relation: Relation::OpenRequest,
    };
    let biz = (Uuid::from_u128(9), To::Business("+19285550199"));
    let states = [
        NumberState::Working,
        NumberState::Suspended { since: at(0) },
        NumberState::Restored { since: at(0) },
    ];
    let mut job = 0;
    for (on, h, b) in [true, false].into_iter().flat_map(|on| {
        states
            .into_iter()
            .flat_map(move |h| states.map(|b| (on, h, b)))
    }) {
        let mut all = lines();
        (
            all.homeowner.texting_on,
            all.homeowner.state,
            all.business.state,
        ) = (on, h, b);
        for ((recipient, to), own) in [((her, home), HOMEOWNER_NUMBER), (biz, BUSINESS_NUMBER)] {
            job += 1;
            let before = carrier.0.lock().expect("lock").len();
            match send(&mut db, (&carrier, &vault), &all, (recipient, to), job).await {
                Ok(from) => assert_eq!(from, own),
                Err(e) => assert_eq!(
                    (e, carrier.0.lock().expect("lock").len()),
                    (LineSendError::Paused, before)
                ),
            }
        }
    }
    set_switch(&mut db, Switch::Texting, true, Actor::System)
        .await
        .expect("pause");
    let paused = send(&mut db, (&carrier, &vault), &lines(), biz, job + 1).await;
    assert_eq!(paused, Err(LineSendError::Paused));
    assert_eq!(carrier.0.lock().expect("lock").len(), 9);
}

/// TD-TXT-09 (TXT-024, D-144): the provider's restore keeps texting off until Casey switches it
/// on, and he cannot switch it on before the restore.
#[test]
fn td_txt_09_restored_by_twilio_stays_off_until_casey_switches_it_on() {
    let mut line = suspended();
    assert_eq!(line.state.switch_on(), Err(ModeError::NotRestored));
    line.state = line.state.provider_restored();
    assert_eq!(line.sender(), None);
    line.state = line.state.switch_on().expect("restored");
    assert_eq!(line.sender(), Some(&number(HOMEOWNER_NUMBER)));
}

/// TD-TXT-09 (TXT-024, CALL-019, D-206, D-491, D-1808): "smell gas" on a suspended number: a
/// critical alert, the safety text by email and in the web chat, the banner on, and with the switch
/// on one call; a retry of the same message raises nothing more; a second message about the same
/// emergency gets its alert and no call; a message before the suspension, a non-emergency and an
/// open line get no call.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_09_suspended_smell_gas_alerts_and_calls_once(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let line = suspended();
    let gas = case("I smell gas", 1, true, true);
    let plan = emergency_plan(&line, &gas.1).expect("plan");
    assert!(plan.critical_alert && plan.safety_by_email && plan.safety_in_web_chat);
    assert!(plan.banner_on && plan.voice_call);
    let first = Uuid::from_u128(0x1b1);
    let raised = handle(&mut db, &line, gas, first).await.expect("raised");
    assert!(raised.alert.is_some() && raised.voice_call);
    assert_eq!(handle(&mut db, &line, gas, first).await, None);
    assert_eq!(tally(&mut db).await, (1, 1));
    let again = case("the gas smell is worse", 3, true, true);
    let second = handle(&mut db, &line, again, Uuid::from_u128(0x1b2)).await;
    assert!(second.is_some_and(|r| r.alert.is_some() && !r.voice_call));
    assert_eq!(tally(&mut db).await, (1, 2));
    let before = emergency_plan(&line, &case("I smell gas", -1, true, true).1).expect("plan");
    assert!(before.critical_alert && !before.voice_call);
    assert_eq!(
        emergency_plan(&line, &case("the sink drips", 1, true, true).1),
        None
    );
    assert_eq!(emergency_plan(&lines().homeowner, &gas.1), None);
}

/// TD-TXT-09 (TXT-024, D-144, D-1808): texting switched off on a working number: the alert, the
/// web chat, the banner, no call; restored by Twilio and waiting for Casey is still the
/// suspension: a message sent then still gets the one call.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_09_switched_off_and_restored_waiting_for_casey(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let mut off = lines().homeowner;
    off.texting_on = false;
    let plan = emergency_plan(&off, &case("I smell gas", 1, false, true).1).expect("plan");
    assert!(plan.critical_alert && plan.safety_in_web_chat && plan.banner_on);
    assert!(!plan.safety_by_email && !plan.voice_call);
    let mut restored = suspended();
    restored.state = restored.state.provider_restored();
    let gas = case("I smell gas", 30, false, true);
    let raised = handle(&mut db, &restored, gas, Uuid::from_u128(0x1b3)).await;
    assert!(raised.is_some_and(|r| r.voice_call));
    assert_eq!(tally(&mut db).await, (1, 1));
}

/// TD-TXT-12 (TXT-024, CON-017, D-210, D-1808): with an email on file the email and exactly one
/// call; from an opted-out number no call and no second alert: its critical alert is optout's
/// `messaging.opted_out_emergency` on the same inbox row (`optout_flow.rs`, CON-017).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_12_email_and_one_call_but_none_to_an_opted_out_number(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let (words, facts) = case("there is smoke", 2, true, true);
    let opted = EmergencyFacts {
        opted_out: true,
        ..facts
    };
    let plan = emergency_plan(&suspended(), &opted).expect("plan");
    assert!(plan.safety_by_email && plan.banner_on && !plan.critical_alert && !plan.voice_call);
    handle(
        &mut db,
        &suspended(),
        (words, opted),
        Uuid::from_u128(0x1b4),
    )
    .await;
    assert_eq!(tally(&mut db).await, (0, 0));
    let raised = handle(
        &mut db,
        &suspended(),
        (words, facts),
        Uuid::from_u128(0x1b5),
    )
    .await;
    assert!(raised.is_some_and(|r| r.voice_call));
    assert_eq!(tally(&mut db).await, (1, 1));
}

/// TD-TXT-16 (TXT-024, CALL-019, CALL-022, D-275): the setting off (the default): a 911-list
/// message gets the email and no call; "AC died" no alert or safety text; the switch turns on
/// only with an approval and every condition, and then the call goes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_16_no_call_while_the_setting_is_off(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let off = case("I smell gas", 1, true, SafetySwitch::off().is_on());
    let plan = emergency_plan(&suspended(), &off.1).expect("plan");
    assert!(plan.safety_by_email && !plan.voice_call);
    handle(&mut db, &suspended(), off, Uuid::from_u128(0x1b6)).await;
    assert_eq!(tally(&mut db).await, (0, 1));
    assert_eq!(
        emergency_plan(&suspended(), &case("AC died", 1, true, true).1),
        None
    );
    let met = Conditions {
        narrowed_trigger: true,
        recent_only: true,
        script: true,
        other_number: true,
        list_checked: true,
        consent_text: true,
        logged: true,
    };
    let approval = Approval {
        date: "2026-10-30".parse().expect("date"),
        reference: "FAKE-REF-2".to_owned(),
    };
    assert_eq!(SafetySwitch::turn_on(None, &met), None);
    let unmet = Conditions {
        logged: false,
        ..met
    };
    assert_eq!(SafetySwitch::turn_on(Some(&approval), &unmet), None);
    let on = SafetySwitch::turn_on(Some(&approval), &met)
        .expect("on")
        .is_on();
    let gas = case("I smell gas", 2, false, on);
    let raised = handle(&mut db, &suspended(), gas, Uuid::from_u128(0x1b7)).await;
    assert!(raised.is_some_and(|r| r.voice_call));
    assert_eq!(tally(&mut db).await, (1, 2));
}

/// TD-CON-16 (CON-033, TXT-022, D-315, D-1806): the toll-free fallback needs a recorded refusal
/// and Casey's go, is refused for a suspended number and for business templates, and keeps the
/// launch channel web-and-email until verified.
#[test]
fn td_con_16_the_toll_free_fallback_rule() {
    let refusal = Refusal {
        date: "2026-10-01".parse().expect("date"),
        reference: "30951".to_owned(),
    };
    let blank = Refusal {
        reference: " ".to_owned(),
        ..refusal.clone()
    };
    let working = NumberState::Working;
    assert_eq!(
        TollFree::configure(None, true, working),
        Err(ModeError::NoRefusal)
    );
    assert_eq!(
        TollFree::configure(Some(&blank), true, working),
        Err(ModeError::NoRefusal)
    );
    assert_eq!(
        TollFree::configure(Some(&refusal), false, working),
        Err(ModeError::NoGo)
    );
    for paused in [suspended().state, suspended().state.provider_restored()] {
        assert_eq!(
            TollFree::configure(Some(&refusal), true, paused),
            Err(ModeError::Suspended)
        );
    }
    let toll = TollFree::configure(Some(&refusal), true, working).expect("configured");
    assert_eq!(toll.launch_channel(), LaunchChannel::WebAndEmail);
    assert_eq!(
        toll.may_carry(Audience::Business),
        Err(ModeError::BusinessTemplate)
    );
    assert_eq!(toll.may_carry(Audience::Homeowner), Ok(()));
    assert_eq!(toll.verified().launch_channel(), LaunchChannel::Sms);
}

/// TD-TXT-17 (TXT-021, TXT-022, TXT-035, D-303): the homeowner switch off or a homeowner-number
/// suspension leaves business notices unchanged, and the reverse.
#[test]
fn td_txt_17_one_audience_never_changes_the_other() {
    let mut all = lines();
    all.homeowner.texting_on = false;
    all.homeowner.state = all.homeowner.state.suspend(at(0));
    assert_eq!(all.business, lines().business);
    assert_eq!(all.business.sender(), Some(&number(BUSINESS_NUMBER)));
    let mut all = lines();
    all.business.texting_on = false;
    all.business.state = all.business.state.suspend(at(0));
    assert_eq!(all.homeowner.sender(), Some(&number(HOMEOWNER_NUMBER)));
    assert_eq!(
        update_route(&all, Audience::Business, true),
        UpdateRoute::Email
    );
}
