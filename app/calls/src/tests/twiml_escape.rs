//! D-4052 B19 (CALL-005, CALL-013, CALL-014, CALL-020, CALL-022, OPS-028): every value put into
//! TwiML goes through the one `xml_escape` helper, so a `&`, `<` or quote in a word, line or URL is
//! escaped, and the TwiML for today's brand (whose words need no escaping) is byte for byte what it
//! was. Fake numbers (555-01xx and the 500-555 test range) and fake paths only.

use askhava_adapters::api::{MaskedPool, PoolNumber, TwilioConfig};
use askhava_core::api::{Settings, brand};
use secrecy::{ExposeSecret, SecretString};

use crate::api::{
    PinWait, code_call_twiml, greeting_en, greeting_es, greeting_twiml, owner_bridge_twiml,
    pin_prompt_twiml, safety_script, safety_twiml,
};
use crate::routing::xml_escape;

const NUMBER: &str = "+15005550100";
const MAIN: &str = "+19285550100";
const HER: &str = "+19285550171";
const AWKWARD: &str = "/v?a=1&b=<2>\"'";
const ESCAPED: &str = "/v?a=1&amp;b=&lt;2&gt;&quot;&apos;";

/// A pool of one fake number.
fn pool() -> MaskedPool {
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
        ("ASKHAVA_MASKED_NUMBERS", NUMBER),
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

fn from(pool: &MaskedPool) -> PoolNumber {
    pool.numbers().first().expect("a number").clone()
}

fn secret(value: &str) -> SecretString {
    SecretString::from(value.to_owned())
}

#[test]
fn the_helper_escapes_the_five_characters_and_nothing_else() {
    assert_eq!(
        xml_escape("A & B <c> \"d\" 'e'"),
        "A &amp; B &lt;c&gt; &quot;d&quot; &apos;e&apos;"
    );
    assert_eq!(xml_escape("Plain words, 9 1 1."), "Plain words, 9 1 1.");
}

#[test]
fn a_url_with_an_ampersand_less_than_and_quote_is_escaped_in_every_action() {
    let pool = pool();
    let wait = PinWait::default();
    let needle = format!("action=\"{ESCAPED}\"");
    assert!(pin_prompt_twiml(wait, AWKWARD).contains(&needle));
    assert!(
        safety_twiml(MAIN, &from(&pool), AWKWARD)
            .expect("safety")
            .contains(&needle)
    );
    let code = code_call_twiml(&from(&pool), MAIN, &secret("123456"), AWKWARD).expect("code");
    assert!(code.expose_secret().contains(&needle));
}

#[test]
fn spoken_words_with_an_ampersand_less_than_and_quote_are_escaped() {
    let pool = pool();
    let words = "Leave now & call 911 <now> \"please\"";
    let bridge = owner_bridge_twiml(&from(&pool), &secret(HER), words).expect("bridge");
    assert!(
        bridge
            .expose_secret()
            .contains("<Say>Leave now &amp; call 911 &lt;now&gt; &quot;please&quot;</Say>")
    );
}

#[test]
fn todays_brand_needs_no_escaping() {
    let names = [
        brand().legal_name(),
        brand().spoken_legal_name(),
        brand().display_name(),
        brand().es().legal_name(),
    ];
    for name in names {
        assert_eq!(xml_escape(name), name);
    }
}

#[test]
fn the_greeting_twiml_is_byte_for_byte_unchanged() {
    let expected = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response>\
         <Say language=\"en-US\">{}</Say><Say language=\"es-MX\">{}</Say>\
         <Hangup/></Response>",
        greeting_en(),
        greeting_es()
    );
    assert_eq!(greeting_twiml(), expected);
}

#[test]
fn the_safety_and_code_call_twiml_are_byte_for_byte_unchanged() {
    let pool = pool();
    let words = safety_script(MAIN, &from(&pool)).expect("script");
    let safety = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response><Gather input=\"dtmf\" \
         numDigits=\"1\" timeout=\"5\" action=\"/stop\"><Say>{words}</Say></Gather><Hangup/>\
         </Response>"
    );
    assert_eq!(safety_twiml(MAIN, &from(&pool), "/stop"), Some(safety));
    let said = "1, 2, 3, 4";
    let spoken = format!(
        "This is {}. Your code is {said}. Again, your code is {said}. \
         To call us back, dial 9 2 8, 5 5 5, 0 1 0 0. Press 1 to hear it again.",
        brand().legal_name()
    );
    let code = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response><Gather input=\"dtmf\" \
         numDigits=\"1\" timeout=\"10\" action=\"/again\"><Say><prosody rate=\"slow\">{spoken}\
         </prosody></Say></Gather><Hangup/></Response>"
    );
    let twiml = code_call_twiml(&from(&pool), MAIN, &secret("1234"), "/again").expect("code");
    assert_eq!(twiml.expose_secret(), code);
}
