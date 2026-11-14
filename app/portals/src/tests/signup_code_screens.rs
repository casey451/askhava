//! T450 (BIZ-002, BIZ-036, BIZ-040, BIZ-042, CON-019, TXT-035, A11Y-030; D-3602): the sign-up code
//! screens, the notice-channel choice with its text phone field, and the sealed consent channels.
//! TD-BIZ-13 and TD-CON-08 cases that fall to this task. Fake data only.

use askhava_businesses::api::{CONSENT_VERSION, ConsentChannel, MaskedContact, consent_wording};
use askhava_core::api::IdSecret;
use askhava_picks::api::{ConfirmSettings, WindowRules};
use secrecy::SecretString;

use crate::business_signup::{
    AreaEntered, CODE_SEND_ROUTE, CONSENT_AGAIN, ChannelPost, CodeProblem, EMAIL_CHECK_ROUTE,
    EMAIL_RESEND_ROUTE, IDENTITY_CHECK_ROUTE, LETTER_ROUTE, NO_EMAIL_HINT, NUMBER_OLD_ROUTE, Parts,
    SERVICE_AREA_ROUTE, SIGNUP_FORM_ROUTE, Screen, TEXT_PHONE_MISSING, fee_numbers, seal_for,
    sealed_signup_page,
};
use crate::pick_flow::PageLanguage;

const CSRF: &str = "fake-csrf-token-1";
const BOX: &str = "<input type=\"checkbox\" id=\"consent\" name=\"consent\" value=\"yes\">";

fn secret() -> IdSecret {
    let key = SecretString::from("fake-id-secret-for-tests-00000001".to_owned());
    IdSecret::new(1, key).expect("secret")
}

fn parts(texting_on: bool) -> Parts<'static> {
    Parts {
        texting_on,
        language: PageLanguage::En,
        csrf: CSRF,
        fees: fee_numbers(ConfirmSettings::default(), WindowRules::default()),
    }
}

fn page(screen: Screen<'_>, texting_on: bool) -> String {
    sealed_signup_page(screen, &parts(texting_on), &secret()).expect("page")
}

fn cats() -> Vec<(String, &'static str)> {
    vec![("plumbing_repair".to_owned(), "Open now")]
}

/// The value of the hidden field `name` on `html`.
fn hidden(html: &str, name: &str) -> String {
    let at = format!("name=\"{name}\" value=\"");
    let rest = &html[html.find(&at).expect("field") + at.len()..];
    rest[..rest.find('"').expect("end")].to_owned()
}

fn post(fields: &[(&str, &str)]) -> Vec<(String, String)> {
    fields
        .iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect()
}

fn parse(fields: &[(&str, &str)]) -> ChannelPost {
    ChannelPost::parse(&post(fields), true, (&secret(), CSRF))
}

/// BIZ-002, A11Y-030: the identity code screen: one labelled one-time-code input, the masked
/// contact, a refused or expired code beside the field, and its route.
#[test]
fn identity_code_screen_renders_its_input_error_and_route() {
    let to = MaskedContact::phone(&SecretString::from("928-555-0142".to_owned()), false);
    let first = page(Screen::IdentityCode(&to, None), false);
    assert!(first.contains(&format!(
        "action=\"{IDENTITY_CHECK_ROUTE}\" id=\"code-form\""
    )));
    assert!(first.contains("<label for=\"code\">6-digit code</label>"));
    assert!(first.contains("inputmode=\"numeric\" autocomplete=\"one-time-code\""));
    assert!(first.contains("mobile phone on your ROC or city license record: (•••) •••-0142"));
    assert!(!first.contains("555-0142") && !first.contains("Error:"));
    assert!(first.contains(&format!("action=\"{CODE_SEND_ROUTE}\"")));
    for problem in [CodeProblem::Refused, CodeProblem::Expired] {
        let again = page(Screen::IdentityCode(&to, Some(problem)), false);
        assert!(again.contains("<title>Error: Show this business is yours"));
        let message = problem.message().replace('\'', "&#39;");
        assert!(again.contains(&format!(
            "id=\"code-error\" class=\"error\">Error: {message}"
        )));
        assert!(again.contains("<a href=\"#code\">"));
        assert!(again.contains("code-hint code-error\" aria-invalid=\"true\""));
    }
}

/// BIZ-040, BIZ-042: the email code screen: its input, the Spam or Junk line, the resend button.
#[test]
fn email_code_screen_renders_its_input_hint_and_resend() {
    let html = page(Screen::EmailCode(None), true);
    assert!(html.contains(&format!("action=\"{EMAIL_CHECK_ROUTE}\" id=\"code-form\"")));
    assert!(html.contains(&format!("<span id=\"code-hint\">{NO_EMAIL_HINT}</span>")));
    assert_eq!(NO_EMAIL_HINT, "No email? Check your Spam or Junk folder.");
    assert!(html.contains("autocomplete=\"one-time-code\""));
    assert!(html.contains(&format!("action=\"{EMAIL_RESEND_ROUTE}\"")));
    assert!(html.contains("id=\"button-resend\">Send the email again</button>"));
    let again = page(Screen::EmailCode(Some(CodeProblem::Expired)), true);
    assert!(again.contains("Error: That code has expired."));
}

/// TD-BIZ-13 (BIZ-036, TXT-035): with texting off only email is offered and no phone field shows;
/// with it on, text and both are offered, never preselected, and the phone field always renders.
#[test]
fn td_biz_13_text_offered_only_while_texting_is_on() {
    let off = page(Screen::Form(&cats()), false);
    assert!(!off.contains("notice_channel") && !off.contains("type=\"tel\""));
    let on = page(Screen::Form(&cats()), true);
    assert!(on.contains("id=\"notice-email\" name=\"notice_channel\" value=\"email\" checked>"));
    assert!(on.contains("id=\"notice-text\" name=\"notice_channel\" value=\"text\">"));
    assert!(on.contains("id=\"notice-both\" name=\"notice_channel\" value=\"both\">"));
    assert!(
        on.contains("<label for=\"text-phone\">Mobile number, used only for job texts</label>")
    );
    assert!(on.contains("type=\"tel\" id=\"text-phone\" name=\"text_phone\" autocomplete=\"tel\""));
    assert!(!on.contains("<script") && !on.contains(" hidden"));
    let choice = on.find("notice-choice").expect("choice");
    assert!(
        choice < on.find(BOX).expect("unticked box"),
        "the box after the choice"
    );
    let ignored = ChannelPost::parse(
        &post(&[("notice_channel", "text")]),
        false,
        (&secret(), CSRF),
    );
    assert!(!ignored.text_chosen() && ignored.text_phone.is_none());
}

/// TD-CON-08 (CON-019, TXT-035, A11Y-030): text with no phone gives `TEXT_PHONE_MISSING`, beside
/// the phone field and linked from the summary.
#[test]
fn td_con_08_text_without_a_phone_asks_for_the_phone() {
    let form = page(Screen::Form(&cats()), true);
    let seal = hidden(&form, "consent_seal");
    let shown = [("consent_channels", "email,text"), ("text_phone", " ")];
    let mut fields = vec![("notice_channel", "both"), ("consent_seal", seal.as_str())];
    fields.extend(shown);
    let p = parse(&fields);
    assert!(
        p.shown.is_none(),
        "a seal made for email only does not cover texts"
    );
    let texts = page(Screen::FormAgain(&cats(), &p.entered()), true);
    let resealed = hidden(&texts, "consent_seal");
    fields[1] = ("consent_seal", resealed.as_str());
    let p = parse(&fields);
    assert_eq!(p.problem(), Some(("consent", TEXT_PHONE_MISSING)));
    let html = page(Screen::FormAgain(&cats(), &p.entered()), true);
    assert!(html.contains(&format!(
        "id=\"text-phone-error\" class=\"error\">Error: {TEXT_PHONE_MISSING}"
    )));
    assert!(html.contains("<a href=\"#text-phone\">") && !html.contains("id=\"consent-error\""));
    assert!(html.contains("id=\"notice-both\" name=\"notice_channel\" value=\"both\" checked>"));
}

/// TD-CON-08 (CON-019, BIZ-036): the wording names texts only when chosen; text ticked against the
/// email-only wording stores nothing and comes back naming texts, unticked; then it stores both.
#[test]
fn td_con_08_text_against_email_only_wording_stores_nothing() {
    let form = page(Screen::Form(&cats()), true);
    let email_only = consent_wording(&[ConsentChannel::Email]);
    assert!(form.contains(&email_only) && form.contains(BOX));
    assert_eq!(hidden(&form, "consent_channels"), "email");
    let seal = hidden(&form, "consent_seal");
    let mut fields = vec![
        ("notice_channel", "text"),
        ("text_phone", "928-555-0199"),
        ("consent", "yes"),
        ("consent_channels", "email"),
        ("consent_seal", seal.as_str()),
    ];
    let p = parse(&fields);
    assert_eq!(
        p.shown,
        Some(vec![ConsentChannel::Email]),
        "the seal verifies"
    );
    assert_eq!(p.consent_channels(), None, "no consent of either channel");
    assert_eq!(p.stored_wording(), None);
    assert_eq!(p.problem(), Some(("consent", CONSENT_AGAIN)));
    assert!(p.entered().text_chosen);
    let again = page(Screen::FormAgain(&cats(), &p.entered()), true);
    let both = [ConsentChannel::Email, ConsentChannel::Text];
    assert!(again.contains(&consent_wording(&both)));
    assert!(
        again.contains(
            "type=\"checkbox\" id=\"consent\" name=\"consent\" value=\"yes\" aria-invalid"
        )
    );
    assert!(!again.contains("value=\"yes\" checked"), "the box unticked");
    let resealed = hidden(&again, "consent_seal");
    fields[3] = ("consent_channels", "email,text");
    fields[4] = ("consent_seal", resealed.as_str());
    let p = parse(&fields);
    assert_eq!(p.consent_channels(), Some(both.to_vec()));
    assert_eq!(p.stored_wording(), Some(consent_wording(&both)));
    assert_eq!(p.problem(), None);
}

/// CON-019, D-3602: a forged field naming texts with a missing, bad or foreign seal (another CSRF
/// token or wording version) stores nothing and gives `FormAgain`.
#[test]
fn forged_shown_channels_store_nothing() {
    let both = [ConsentChannel::Email, ConsentChannel::Text];
    let other_csrf = seal_for(&secret(), &both, CONSENT_VERSION, "fake-csrf-token-2");
    let other_version = seal_for(&secret(), &both, "con019-v0", CSRF);
    let (a, b) = (other_csrf.expect("seal"), other_version.expect("seal"));
    let bad = "0".repeat(64);
    for seal in [None, Some(bad.as_str()), Some(a.as_str()), Some(b.as_str())] {
        let mut fields = vec![
            ("notice_channel", "both"),
            ("text_phone", "928-555-0199"),
            ("consent_channels", "email,text"),
        ];
        fields.extend(seal.map(|s| ("consent_seal", s)));
        let p = parse(&fields);
        assert_eq!(p.shown, None);
        assert_eq!(p.consent_channels(), None);
        assert_eq!(p.stored_wording(), None);
        assert_eq!(p.entered().problem, Some(("consent", CONSENT_AGAIN)));
    }
    let good = seal_for(&secret(), &both, CONSENT_VERSION, CSRF).expect("seal");
    let fields = [
        ("consent_channels", "text,email"),
        ("consent_seal", good.as_str()),
    ];
    assert_eq!(parse(&fields).shown, None, "only the fixed order is read");
}

/// BIZ-036, CON-019: a phone typed with email chosen is dropped; the stored wording comes from the
/// verified shown channels (email and texts), not from the chosen one (email).
#[test]
fn email_chosen_drops_the_phone_and_stores_the_shown_wording() {
    let both = [ConsentChannel::Email, ConsentChannel::Text];
    let seal = seal_for(&secret(), &both, CONSENT_VERSION, CSRF).expect("seal");
    let fields = [
        ("notice_channel", "email"),
        ("text_phone", "928-555-0199"),
        ("consent_channels", "email,text"),
        ("consent_seal", seal.as_str()),
    ];
    let p = parse(&fields);
    assert!(p.text_phone.is_none(), "no phone kept without texts chosen");
    assert_eq!(p.shown, Some(both.to_vec()));
    assert_eq!(p.consent_channels(), Some(vec![ConsentChannel::Email]));
    assert_eq!(p.stored_wording(), Some(consent_wording(&both)));
    assert_eq!(p.problem(), None);
}

/// D-3602: every form on every screen posts to a route constant.
#[test]
fn every_form_posts_to_a_route_constant() {
    let to = MaskedContact::phone(&SecretString::from("928-555-0142".to_owned()), false);
    let routes = [
        SIGNUP_FORM_ROUTE,
        CODE_SEND_ROUTE,
        NUMBER_OLD_ROUTE,
        LETTER_ROUTE,
        IDENTITY_CHECK_ROUTE,
        EMAIL_CHECK_ROUTE,
        EMAIL_RESEND_ROUTE,
        SERVICE_AREA_ROUTE,
    ];
    let (area, cats) = (AreaEntered::default(), cats());
    let screens = [
        Screen::Form(&cats),
        Screen::Contact(&to, None),
        Screen::Letter(&to, 3),
        Screen::IdentityCode(&to, None),
        Screen::EmailCode(None),
        Screen::ServiceArea(&area),
    ];
    for screen in screens {
        let html = page(screen, true);
        let actions: Vec<&str> = html.split("action=\"").skip(1).collect();
        assert!(!actions.is_empty());
        for rest in actions {
            let action = &rest[..rest.find('"').expect("end")];
            assert!(routes.contains(&action), "{action}");
            assert!(action.starts_with("/b/signup/"));
        }
    }
}
