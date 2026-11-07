//! Web-and-email mode, a suspended number and the one configured number (TD-TXT-08, TD-TXT-09,
//! TD-TXT-12, TD-TXT-16, TD-TXT-17, TD-CON-16; TXT-021 to TXT-024, TXT-034, CON-033, D-1804 to
//! D-1806).
//!
//! `messaging::api`'s mode rules decide the routes; on a suspended number the safety voice call is
//! intake's `calls.safety_call` event, queued with the plan's `voice_call` as its switch, which
//! T080's handler places from a pool number other than the main number
//! (`tests/integration/tests/voice_calls.rs`). Fake numbers only.

use askhava_calls::api::{Approval, Conditions, SAFETY_CALL_KIND, SafetySwitch};
use askhava_intake::api::rules::{Check, check, safety_call_trigger};
use askhava_intake::api::{SafetyCallAsk, enqueue_safety_call};
use askhava_messaging::api::{
    Audience, EmergencyFacts, EmergencyPlan, LaunchChannel, Line, Lines, ModeError, NumberState,
    PAUSED_EMERGENCY_ALERT, Refusal, SendingNumber, TollFree, UpdateRoute, emergency_plan,
    raise_emergency_alert, update_route,
};
use askhava_picks::api::{CodeRoute, EmailFrom, Texting, code_route};
use jiff::{SignedDuration, Timestamp};
use proptest::prelude::*;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

const HOMEOWNER_NUMBER: &str = "+19285550100";
const BUSINESS_NUMBER: &str = "+19285550110";

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

fn facts(words: &str, mins: i64, has_email: bool, switch_on: bool) -> EmergencyFacts {
    EmergencyFacts {
        matched: matches!(check(words, false), Check::Match(_)),
        call_trigger: safety_call_trigger(words),
        arrived_at: at(mins),
        has_email,
        opted_out: false,
        safety_switch_on: switch_on,
    }
}

/// Raises the plan's alert and queues intake's event with the plan's `voice_call`, as the
/// suspension path does; the alert ID.
async fn handle(db: &mut PgConnection, plan: &EmergencyPlan, alert: Option<Uuid>) -> Uuid {
    let mut tx = db.begin().await.expect("tx");
    let message = Uuid::new_v4();
    let raised = raise_emergency_alert(&mut tx, plan, message)
        .await
        .expect("alert");
    let ask = SafetyCallAsk {
        contact: Uuid::from_u128(7),
        message,
        arrived_at: at(1),
        alert: alert.or(raised).expect("an alert"),
    };
    enqueue_safety_call(&mut tx, "I smell gas", ask, plan.voice_call)
        .await
        .expect("enqueue");
    tx.commit().await.expect("commit");
    ask.alert
}

async fn count(db: &mut PgConnection, sql: &'static str, value: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(value)
        .fetch_one(db)
        .await
        .expect("count")
}

async fn calls(db: &mut PgConnection) -> i64 {
    count(
        db,
        "SELECT count(*) FROM core.task WHERE kind = $1",
        SAFETY_CALL_KIND,
    )
    .await
}

async fn alerts(db: &mut PgConnection) -> i64 {
    let sql = "SELECT count(*) FROM ops.alert WHERE kind = $1 AND severity = 'critical'";
    count(db, sql, PAUSED_EMERGENCY_ALERT).await
}

/// TD-TXT-08 (TXT-021, TXT-023, TXT-034, D-297, D-1804): started in web-and-email mode, or
/// switched off, no homeowner text goes; updates go by email, or onto the call list without one,
/// and the pick code goes by email to her conversation address.
#[test]
fn td_txt_08_web_and_email_mode_stops_every_homeowner_text() {
    let launched = Line::at_launch(LaunchChannel::WebAndEmail, Some(number(HOMEOWNER_NUMBER)));
    let mut switched = lines();
    switched.homeowner.texting_on = false;
    for homeowner in [launched, switched.homeowner.clone()] {
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

fn any_state() -> impl Strategy<Value = NumberState> {
    prop_oneof![
        Just(NumberState::Working),
        Just(NumberState::Suspended { since: at(0) }),
        Just(NumberState::Restored { since: at(0) }),
    ]
}

proptest! {
    /// TD-TXT-08 property (TXT-022): every outbound text uses the audience's configured number;
    /// no state yields another number.
    #[test]
    fn td_txt_08_every_outbound_text_uses_the_configured_number(
        on in any::<bool>(), home in any_state(), biz in any_state(), email in any::<bool>(),
    ) {
        let mut all = lines();
        (all.homeowner.texting_on, all.homeowner.state, all.business.state) = (on, home, biz);
        for (audience, own) in [(Audience::Homeowner, HOMEOWNER_NUMBER), (Audience::Business, BUSINESS_NUMBER)] {
            if let UpdateRoute::Text(from) = update_route(&all, audience, email) {
                prop_assert_eq!(from.as_str(), own);
            }
        }
    }
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

/// TD-TXT-09 (TXT-024, CALL-019, D-206, D-491): "smell gas" on a suspended number: a critical
/// alert, the safety text by email and in the web chat, the banner on, and with the switch on one
/// call; a message before the suspension, a non-emergency and a second message get no call.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_09_suspended_smell_gas_alerts_and_calls_once(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let line = suspended();
    let plan = emergency_plan(&line, &facts("I smell gas", 1, true, true)).expect("plan");
    assert!(plan.critical_alert && plan.safety_by_email && plan.safety_in_web_chat);
    assert!(plan.banner_on && plan.voice_call);
    let alert = handle(&mut db, &plan, None).await;
    handle(&mut db, &plan, Some(alert)).await;
    assert_eq!((calls(&mut db).await, alerts(&mut db).await), (1, 2));
    let before = emergency_plan(&line, &facts("I smell gas", -1, true, true)).expect("plan");
    assert!(before.critical_alert && !before.voice_call);
    assert_eq!(
        emergency_plan(&line, &facts("the sink drips", 1, true, true)),
        None
    );
    assert_eq!(
        emergency_plan(&lines().homeowner, &facts("I smell gas", 1, true, true)),
        None
    );
}

/// TD-TXT-12 (TXT-024, D-210): with an email on file the email and exactly one call; from an
/// opted-out number no call, the critical alert still fires.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_12_email_and_one_call_but_none_to_an_opted_out_number(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let opted = EmergencyFacts {
        opted_out: true,
        ..facts("there is smoke", 2, true, true)
    };
    let plan = emergency_plan(&suspended(), &opted).expect("plan");
    assert!(plan.safety_by_email && !plan.voice_call);
    handle(&mut db, &plan, None).await;
    assert_eq!((calls(&mut db).await, alerts(&mut db).await), (0, 1));
    let plan = emergency_plan(&suspended(), &facts("there is smoke", 2, true, true)).expect("plan");
    handle(&mut db, &plan, None).await;
    assert_eq!((calls(&mut db).await, alerts(&mut db).await), (1, 2));
}

/// TD-TXT-16 (TXT-024, CALL-019, CALL-022, D-275): the setting off (the default): a 911-list
/// message gets the email and no call; "AC died" no alert or safety text; the switch turns on
/// only with an approval and every condition, and then one call per 911-list message.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_16_no_call_while_the_setting_is_off(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let switch = SafetySwitch::off();
    let plan = emergency_plan(&suspended(), &facts("I smell gas", 1, true, switch.is_on()));
    let plan = plan.expect("plan");
    assert!(plan.safety_by_email && !plan.voice_call);
    handle(&mut db, &plan, None).await;
    assert_eq!(calls(&mut db).await, 0);
    assert_eq!(
        emergency_plan(&suspended(), &facts("AC died", 1, true, true)),
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
    let plan = emergency_plan(&suspended(), &facts("I smell gas", 1, false, on)).expect("plan");
    assert!(!plan.safety_by_email && plan.voice_call);
    handle(&mut db, &plan, None).await;
    assert_eq!(calls(&mut db).await, 1);
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
