//! TD-PICK-10 and TD-INT-34 pure cases (PICK-022, PICK-023, PICK-024, PRIV-005, PRIV-020,
//! BIZ-014, INT-029): a stateful property over random event orders; the view and the notice never
//! carry the phone or any homeowner detail. Fake data only.

use askhava_privacy::api::{ContactDetails, Field};
use proptest::prelude::*;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;

use crate::api::{
    EMAIL_CHOSEN_FIELDS, RELEASE_FIELDS, ReleaseFacts, ReleaseStep, ReleaseView, plan_release,
};
use crate::api::{RELEASE_TEMPLATE, RELEASE_TEXT_TEMPLATE, release_notice};

const NAME: &str = "Testy McFake";
const PHONE: &str = "+19285550142";
const STREET: &str = "1 Test Way";
const EMAIL: &str = "person1@example.com";

#[derive(Debug, Clone, Copy)]
enum Event {
    VerifiedSuccess,
    Redirect,
    NoFeeDecision,
    ProofArrives,
    AddressArrives,
    Ends,
}

/// The model: what is true of the pick, and what the business was shown.
#[derive(Debug, Default)]
struct World {
    paid: bool,
    free: bool,
    ended: bool,
    emergency: bool,
    address_in: bool,
    released: bool,
    shown: u32,
    asked_again: u32,
}

impl World {
    /// A proof exists only for a verified success or a recorded $0 decision (SEC-030); with one,
    /// the release runs [`plan_release`] as `release` does.
    fn attempt(&mut self) {
        if !(self.paid || self.free) {
            return;
        }
        let mut facts = ReleaseFacts {
            released: self.released,
            ended: self.ended,
            emergency: self.emergency,
            address_in: None,
        };
        if plan_release(facts) != ReleaseStep::Read {
            return;
        }
        facts.address_in = Some(self.address_in);
        match plan_release(facts) {
            ReleaseStep::Release => {
                assert!(!self.emergency || self.address_in, "TD-INT-34");
                self.released = true;
                self.shown += 1;
            }
            ReleaseStep::WaitForAddress => self.asked_again += 1,
            ReleaseStep::Already | ReleaseStep::MovedOn | ReleaseStep::Read => {
                panic!("a read pick plans only a release or a wait")
            }
        }
    }

    fn apply(&mut self, event: Event) {
        match event {
            Event::VerifiedSuccess => self.paid = true,
            Event::NoFeeDecision => self.free = true,
            Event::AddressArrives => self.address_in = true,
            Event::Ends => self.ended = true,
            Event::Redirect | Event::ProofArrives => {}
        }
        if !matches!(event, Event::Redirect | Event::Ends | Event::AddressArrives) {
            self.attempt();
        }
    }
}

fn event() -> impl Strategy<Value = Event> {
    prop_oneof![
        Just(Event::VerifiedSuccess),
        Just(Event::Redirect),
        Just(Event::NoFeeDecision),
        Just(Event::ProofArrives),
        Just(Event::AddressArrives),
        Just(Event::Ends),
    ]
}

proptest! {
    /// TD-PICK-10: in every reachable state the business has seen the details at most once, and
    /// only on a paid pick or a recorded $0 decision, never after the pick ended; a redirect alone
    /// shows nothing (PICK-022, PRIV-005, BIZ-014). TD-INT-34: an emergency job shows nothing
    /// before its address is in, and each such acceptance asks again (INT-029).
    #[test]
    fn td_pick_10_details_only_once_and_only_when_paid_or_free(
        emergency in any::<bool>(),
        address in any::<bool>(),
        events in prop::collection::vec(event(), 0..24),
    ) {
        let mut world = World { emergency, address_in: address, ..World::default() };
        for e in events {
            let was_ended = world.ended;
            let before = world.shown;
            world.apply(e);
            prop_assert!(world.shown <= 1);
            prop_assert!(world.shown == 0 || world.paid || world.free);
            prop_assert!(!was_ended || world.shown == before);
            if matches!(e, Event::Redirect) {
                prop_assert_eq!(world.shown, before);
            }
        }
        prop_assert!(world.asked_again == 0 || world.emergency);
    }
}

/// TD-INT-34: an emergency pick with no address waits; with it, it releases; a regular job
/// releases without one (INT-029).
#[test]
fn td_int_34_an_emergency_acceptance_before_the_address_waits() {
    let facts = |emergency, address_in| ReleaseFacts {
        released: false,
        ended: false,
        emergency,
        address_in,
    };
    assert_eq!(plan_release(facts(true, None)), ReleaseStep::Read);
    assert_eq!(
        plan_release(facts(true, Some(false))),
        ReleaseStep::WaitForAddress
    );
    assert_eq!(plan_release(facts(true, Some(true))), ReleaseStep::Release);
    assert_eq!(
        plan_release(facts(false, Some(false))),
        ReleaseStep::Release
    );
}

fn secret(s: &str) -> Option<SecretString> {
    Some(SecretString::from(s.to_owned()))
}

/// TD-PICK-10 (PICK-023, PRIV-020): the release opens no phone, and a view made from details that
/// hold one shows the first name, address and email only.
#[test]
fn td_pick_10_the_view_never_holds_the_phone() {
    assert!(!RELEASE_FIELDS.contains(&Field::Phone) && !RELEASE_FIELDS.contains(&Field::Email));
    assert!(!EMAIL_CHOSEN_FIELDS.contains(&Field::Phone));
    let details = ContactDetails {
        name: secret(NAME),
        phone: secret(PHONE),
        address: secret(STREET),
        email: secret(EMAIL),
        ..ContactDetails::default()
    };
    let view = ReleaseView::from_details(details);
    let shown = |v: Option<&SecretString>| v.map(|s| s.expose_secret().to_owned());
    assert_eq!(shown(view.first_name()).as_deref(), Some("Testy"));
    assert_eq!(shown(view.address()).as_deref(), Some(STREET));
    assert_eq!(shown(view.email()).as_deref(), Some(EMAIL));
    assert!(!format!("{view:?}").contains(PHONE));
    let blank = ContactDetails {
        name: secret("  "),
        ..ContactDetails::default()
    };
    assert!(ReleaseView::from_details(blank).first_name().is_none());
}

/// TD-PICK-10 (PICK-024, D-454): the notice holds templates, IDs and the job number only; no
/// homeowner detail and no call window.
#[test]
fn td_pick_10_the_release_notice_carries_no_homeowner_detail() {
    let (business, pick, to) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
    let notice = release_notice(business, pick, &[to], 1042);
    let mut keys: Vec<&str> = notice
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();
    keys.sort_unstable();
    let want = [
        "business",
        "pick",
        "template",
        "text_template",
        "to",
        "values",
    ];
    assert_eq!(keys, want);
    assert_eq!(notice["values"], serde_json::json!({"job": 1042}));
    assert_eq!(notice["template"], RELEASE_TEMPLATE);
    assert_eq!(notice["text_template"], RELEASE_TEXT_TEMPLATE);
    let text = notice.to_string();
    for detail in [NAME, "Testy", PHONE, STREET, EMAIL] {
        assert!(!text.contains(detail), "{detail}");
    }
}
