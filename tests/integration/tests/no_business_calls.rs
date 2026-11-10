//! No automated call to a business, voice codes only on the homeowner's tap, and the "Ask Hava
//! homeowner calls" contact card (TD-BIZ-22, TD-BIZ-25; BIZ-002, BIZ-026, BIZ-039, BIZ-046,
//! SEC-004, SEC-008, PICK-013, TXT-038, CALL-014, CALL-015, CALL-017; D-334, D-344, D-366, D-491,
//! D-492). The J1 and J2 journey sweeps over the fake voice provider are T222's and T099's; here
//! the code paths they rest on: the only code call goes to a homeowner who tapped, no business
//! code goes by voice, no call setting or code path exists, a landline record gets the email step
//! then the letter, and the card holds exactly the current pool. Every number and ID is fake.

use std::path::Path;

use askhava_adapters::api::{CodeChannel, MaskedPool, PoolNumber, TwilioConfig};
use askhava_businesses::api::OpenCardsTimes;
use askhava_businesses::api::{
    CONTACT_CARD_PATH, RecordContacts, Step, next_step, welcome_card_line, welcome_lines,
};
use askhava_calls::api::{Party, code_call_twiml, may_code_call};
use askhava_core::api::Settings;
use askhava_picks::api::{ConfirmSettings, WindowRules};
use askhava_portals::api::{
    CONTACT_CARD_ROUTE, CodeTo, DoneFacts, InboxSwitch, PageLanguage, Parts, Screen, YourNumbers,
    business_home_page, contact_card, fee_numbers, signup_page,
};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;

const MAIN: &str = "+19285550100";

/// A pool of `numbers` (the 500-555 test range), sized to match.
fn pool(numbers: &[&str]) -> MaskedPool {
    let size = numbers.len().to_string();
    let list = numbers.join(",");
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
        ("ASKHAVA_MASKED_POOL_SIZE", size.as_str()),
        ("ASKHAVA_MASKED_NUMBERS", list.as_str()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let config = TwilioConfig::from_settings(&settings, pairs).expect("pool");
    config.pool().clone()
}

fn first(pool: &MaskedPool) -> &PoolNumber {
    &pool.numbers()[0]
}

/// TD-BIZ-25 (CALL-014, CALL-015, SEC-008, PICK-013, BIZ-046, TXT-038): a code call goes only to a
/// homeowner who tapped "Call me with the code instead"; opening the screen alone (no tap) places
/// nothing; no business, tapped or not, ever gets one; the call opens "This is Ask Hava LLC",
/// says the code twice and offers "press 1"; a business's code goes by text or email only.
#[test]
fn td_biz_25_voice_codes_only_on_her_tap_never_to_a_business() {
    let (her, firm) = (Uuid::from_u128(0x71), Uuid::from_u128(0xb1));
    assert!(may_code_call(true, Party::Homeowner(her)));
    assert!(
        !may_code_call(false, Party::Homeowner(her)),
        "screen opened, no tap"
    );
    for tapped in [true, false] {
        assert!(!may_code_call(tapped, Party::Business(firm)));
        assert!(!may_code_call(tapped, Party::Unlinked));
    }
    let pool = pool(&["+15005550101"]);
    let code = SecretString::from("482913".to_owned());
    let twiml = code_call_twiml(first(&pool), MAIN, &code, "/v/again").expect("twiml");
    let words = twiml.expose_secret();
    assert!(words.contains("This is Ask Hava LLC."));
    assert_eq!(
        words.matches("4, 8, 2, 9, 1, 3").count(),
        2,
        "the code twice"
    );
    assert!(words.contains("Press 1 to hear it again."));
    assert!(words.contains("rate=\"slow\""));
    let codes = [
        CodeTo::Phone(SecretString::from("+19285550142".to_owned())),
        CodeTo::Email(SecretString::from("owner@example.com".to_owned())),
    ];
    let channels: Vec<CodeChannel> = codes.iter().map(CodeTo::channel).collect();
    assert_eq!(channels, [CodeChannel::Sms, CodeChannel::Email]);
}

/// TD-BIZ-22, TD-BIZ-25 (BIZ-002, BIZ-026, BIZ-046): a landline record contact (no phone that
/// gets texts) gets the email step, then the letter, then nothing; the chain has no voice step.
#[test]
fn td_biz_25_a_landline_record_gets_email_then_letter_never_a_call() {
    let record = RecordContacts {
        texting_phone: None,
        email: Some(SecretString::from("records@example.com".to_owned())),
        has_address: true,
    };
    assert_eq!(next_step(&record, None), Some(Step::Email));
    assert_eq!(next_step(&record, Some(Step::Email)), Some(Step::Letter));
    assert_eq!(next_step(&record, Some(Step::Letter)), None);
}

/// Every file under `dir` with one of `exts`, recursively.
fn files(dir: &Path, exts: &[&str], out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).expect("dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, exts, out);
        } else if path
            .extension()
            .is_some_and(|e| exts.iter().any(|x| e == *x))
        {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            out.push((path.display().to_string(), text));
        }
    }
}

/// The portal pages a homeowner or the owner sees; every other portal source, template or
/// script is a business screen (sign-up, home, actions, Confirm and pay, release, notices,
/// billing, receipts, mail checks, the emergency switch).
const NOT_BUSINESS: [&str; 9] = [
    "quote_page",
    "pick_flow",
    "pick_code",
    "pick_review",
    "homeowner_",
    "data_download",
    "feedback",
    "owner_",
    "passkey",
];

/// Whether `path` is a business screen's source, template or script in `portals`.
fn business_screen(path: &str) -> bool {
    let Some((_, rest)) = path.split_once("app/portals/") else {
        return false;
    };
    let Some(name) = ["src/", "templates/", "static/"]
        .iter()
        .find_map(|dir| rest.strip_prefix(dir))
    else {
        return false;
    };
    !name.starts_with("tests/") && !NOT_BUSINESS.iter().any(|n| name.starts_with(n))
}

/// TD-BIZ-22, TD-BIZ-25 (BIZ-039, BIZ-046, D-344): no pick-call, emergency-call, test-call or
/// account-call code path or setting exists anywhere in the app or its migrations, and no
/// business screen (every `portals` source, template and script but the homeowner's and the
/// owner's pages, Confirm and pay included) offers "Call me with the code".
#[test]
fn td_biz_22_no_business_call_path_setting_or_control_exists() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut all = Vec::new();
    files(
        &root.join("app"),
        &["rs", "toml", "json", "html", "ts"],
        &mut all,
    );
    files(&root.join("migrations"), &["sql"], &mut all);
    assert!(all.len() > 100, "the scan found the app");
    let screens: Vec<&str> = all
        .iter()
        .map(|(path, _)| path.as_str())
        .filter(|path| business_screen(path))
        .collect();
    for page in [
        "confirm_pay.rs",
        "confirm_pay.html",
        "confirm_pay.ts",
        "business_home.html",
    ] {
        assert!(
            screens.iter().any(|p| p.ends_with(page)),
            "{page} is scanned"
        );
    }
    let banned = ["pick_call", "emergency_call", "test_call", "account_call"];
    for (path, text) in &all {
        let lower = text.to_lowercase();
        for word in banned {
            assert!(!lower.contains(word), "{word} in {path}");
        }
        if business_screen(path) {
            assert!(!lower.contains("call me with the code"), "{path}");
        }
    }
}

fn card_numbers(card: &str) -> Vec<&str> {
    card.split("\r\n")
        .filter_map(|l| l.strip_prefix("TEL;TYPE=VOICE:"))
        .collect()
}

/// TD-BIZ-22 (BIZ-046, CALL-017, CON-020, D-344, D-359, D-491): the card is named "Ask Hava
/// homeowner calls", holds exactly the current pool numbers and the BIZ-046 line; adding a pool
/// number gives an updated card and portal list.
#[test]
fn td_biz_22_the_contact_card_holds_exactly_the_current_pool() {
    let two = pool(&["+15005550101", "+15005550102"]);
    let card = contact_card(two.numbers());
    assert!(card.starts_with("BEGIN:VCARD\r\nVERSION:3.0\r\n"));
    assert!(card.contains("FN:Ask Hava homeowner calls\r\n"));
    assert!(card.contains(
        "NOTE:Calls from these numbers are homeowners who chose your quote\\, calling on an Ask \
         Hava masked number. Ask Hava never calls you.\r\n"
    ));
    assert_eq!(card_numbers(&card), ["+15005550101", "+15005550102"]);
    assert!(!card.contains("+1928"), "no business or homeowner number");
    let three = pool(&["+15005550101", "+15005550102", "+15005550103"]);
    let updated = contact_card(three.numbers());
    assert_eq!(card_numbers(&updated).len(), 3, "the added number is on it");
    let home = business_home_page(
        &YourNumbers::default(),
        (OpenCardsTimes::DEFAULT, false),
        (three.numbers(), &InboxSwitch::default()),
        "fake-csrf",
    )
    .expect("home");
    assert!(
        home.contains("<li>500-555-0103</li>"),
        "the portal lists the new number"
    );
    assert!(home.contains(&format!("href=\"{CONTACT_CARD_ROUTE}\" download")));
}

/// TD-BIZ-22 (BIZ-046, CON-020, D-491): the done screen, and only it, offers the card; the
/// welcome email links to the portal's card download.
#[test]
fn td_biz_22_the_done_screen_and_welcome_email_offer_the_card() {
    let facts = DoneFacts {
        trade: "Plumbing".to_owned(),
        count: 2,
        waitlist: false,
        open_jobs: Vec::new(),
    };
    let parts = Parts {
        texting_on: false,
        language: PageLanguage::En,
        csrf: "fake-csrf",
        fees: fee_numbers(ConfirmSettings::default(), WindowRules::default()),
    };
    let done = signup_page(Screen::Done(&facts), &parts).expect("done");
    assert!(done.contains("(contact card: Ask Hava homeowner calls)"));
    assert!(done.contains(&format!("href=\"{CONTACT_CARD_ROUTE}\" download")));
    assert_eq!(format!("/{CONTACT_CARD_PATH}"), CONTACT_CARD_ROUTE);
    let link = format!("https://askhava.com{CONTACT_CARD_ROUTE}");
    assert!(welcome_card_line().contains(&format!(
        "open {link}. It adds a contact named \"Ask Hava homeowner calls\"."
    )));
    assert!(welcome_lines().contains(&welcome_card_line()));
    let form = signup_page(Screen::Form(&[]), &parts).expect("form");
    assert!(
        !form.contains("contact-card"),
        "only the done screen offers it"
    );
}
