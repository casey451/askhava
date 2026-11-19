//! TD-WORD-02's Spanish sign-up cases (WEB-019, WORD-013, CON-019, A11Y-007; T488, D-180, D-3144,
//! D-3805, D-3881): the assembled router serves `GET /b/signup?lang=es` and the form `POST` whose
//! hidden `lang` field is `es` as `es_business_signup.html` (Spanish, the draft mark, the
//! `forms.css` link first in `main`, `lang=es` posted again by every form, the consent words exactly
//! `businesses::api::consent_box`'s), and without `lang=es` as `business_signup.html`, unchanged.
//! Every other `Screen` step goes through the same `signup_page` with the language the route read,
//! checked here in both languages. No setting is a real key; all data is fake.

use askhava_businesses::api::{ConsentChannel, MaskedContact, consent_box};
use askhava_catalog::api::{load, parse};
use askhava_core::api::{MIGRATOR, Settings, TestControl};
use askhava_picks::api::{ConfirmSettings, WindowRules};
use askhava_portals::api::{
    AreaEntered, CodeProblem, DoneFacts, Entered, PageLanguage, Parts, SIGNUP_FORM_ROUTE,
    SIGNUP_ROUTE, Screen, fee_numbers, signup_page,
};
use askhava_server::business_signup::ServedSignUp;
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const SECRET: &str = "test-only-id-secret-not-a-real-key-0002";
const FORMS_CSS: &str = "<link rel=\"stylesheet\" href=\"/static/portals/forms.css\">";
const HIDDEN_LANG: &str = "<input type=\"hidden\" name=\"lang\" value=\"es\">";

/// The router as `main` assembles it, with only the sign-up part added (fake settings).
fn app(pool: &PgPool) -> Router {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_ID_SECRET", SECRET),
        (
            "VAULT_KEY",
            "1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        ),
        (
            "VAULT_INDEX_KEY",
            "1:BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ=",
        ),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let control = TestControl::from_settings(&settings, pairs.clone()).expect("test control");
    let signup = ServedSignUp::from_settings(pool.clone(), pairs).expect("sign-up part");
    let parts = RouterParts::from((None, None, None, None)).with_sign_up(signup);
    build_router(pool.clone(), control.router(), parts)
}

/// The answer to `request`: its `Set-Cookie` pair, if any, and its page.
async fn answer(app: &Router, request: Request<Body>) -> (Option<String>, String) {
    let response = app.clone().oneshot(request).await.expect("answer");
    let cookie = response.headers().get(header::SET_COOKIE);
    let cookie = cookie.and_then(|v| v.to_str().ok()?.split(';').next().map(str::to_owned));
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (cookie, String::from_utf8(body.to_vec()).expect("utf-8"))
}

/// `GET uri` with no cookie: the pre-session cookie pair and the page.
async fn open(app: &Router, uri: &str) -> (String, String) {
    let request = Request::get(uri).header(header::HOST, "askhava.test");
    let (cookie, html) = answer(app, request.body(Body::empty()).expect("request")).await;
    (cookie.expect("pre-session cookie"), html)
}

/// The form's fields as a browser posts them, with no consent tick, plus `extra`.
fn unticked(html: &str, extra: &[(&str, &str)]) -> String {
    let hidden = |name: &str| {
        let at = format!("name=\"{name}\" value=\"");
        let rest = &html[html.find(&at).expect("field") + at.len()..];
        rest[..rest.find('"').expect("end")].to_owned()
    };
    let enc = |s: &str| s.bytes().map(|c| format!("%{c:02X}")).collect::<String>();
    let names = ["csrf", "consent_channels", "consent_seal"];
    let fields = names.iter().map(|n| (*n, hidden(n)));
    let fields = fields.chain([("business_name", "Example Plumbing Co".to_owned())]);
    let fields = fields.chain(extra.iter().map(|(n, v)| (*n, (*v).to_owned())));
    let body: Vec<String> = fields.map(|(n, v)| format!("{n}={}", enc(&v))).collect();
    body.join("&")
}

/// `POST` the form from `html` with the pre-session `cookie`, without the consent tick.
async fn post(app: &Router, (cookie, html): &(String, String), extra: &[(&str, &str)]) -> String {
    let request = Request::post(SIGNUP_FORM_ROUTE)
        .header(header::HOST, "askhava.test")
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    let body = Body::from(unticked(html, extra));
    answer(app, request.body(body).expect("request")).await.1
}

/// WEB-019, D-180, A11Y-007: the Spanish page, its draft mark, `forms.css` first in `main`, and
/// `lang=es` posted again by every form on it.
fn assert_spanish(html: &str, title: &str) {
    assert!(html.contains("<html lang=\"es\">"), "{title}");
    assert!(html.contains(&format!("<h1>{title}</h1>")), "{title}");
    assert!(html.contains("<p id=\"draft-mark\">Este texto en español es un borrador."));
    let main = html.split("<main id=\"main\" tabindex=\"-1\">").nth(1);
    assert!(
        main.expect("main").trim_start().starts_with(FORMS_CSS),
        "{title}"
    );
    let forms = html.matches("<form ").count();
    assert_eq!(html.matches(HIDDEN_LANG).count(), forms, "{title}");
}

/// Without `lang=es`: `business_signup.html` as before, no draft mark and no `lang` field.
fn assert_english(html: &str, title: &str) {
    assert!(html.contains("<html lang=\"en\">"), "{title}");
    assert!(html.contains(&format!("<h1>{title}</h1>")), "{title}");
    assert!(
        !html.contains("draft-mark") && !html.contains("name=\"lang\""),
        "{title}"
    );
}

/// CON-019: the words beside the box, unescaped.
fn consent_words(html: &str) -> String {
    let at = html.find("<label for=\"consent\"").expect("the box");
    let rest = &html[at + html[at..].find('>').expect("tag end") + 1..];
    let words = &rest[..rest.find("</label>").expect("label end")];
    words
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// TD-WORD-02 (WEB-019, CON-019, D-3144): `GET /b/signup?lang=es` and its form `POST` carrying
/// `lang=es` are Spanish; the consent words are `consent_box`'s; without `lang=es` both are English.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_word_02_the_served_sign_up_follows_lang_es(pool: PgPool) {
    let catalog = parse(include_str!("../../../docs/catalog/categories.yaml")).expect("catalog");
    let mut conn = pool.acquire().await.expect("connection");
    load(&mut conn, &catalog).await.expect("catalog stored");
    let app = app(&pool);
    let es = open(&app, &format!("{SIGNUP_ROUTE}?lang=es")).await;
    assert_spanish(&es.1, "Registre su negocio");
    let wording = consent_box(&[ConsentChannel::Email]).wording;
    assert_eq!(
        consent_words(&es.1),
        wording,
        "CON-019: the stored words, unchanged"
    );
    assert!(es.1.contains("<label for=\"consent\" lang=\"en\">"));
    assert_note(&es.1, false);
    let again = post(&app, &es, &[("lang", "es")]).await;
    assert_spanish(&again, "Error: Registre su negocio");
    assert!(again.contains("Marque la casilla para aceptar los avisos."));
    assert_eq!(consent_words(&again), wording);
    let en = open(&app, SIGNUP_ROUTE).await;
    assert_english(&en.1, "Sign up your business");
    assert_eq!(consent_words(&en.1), wording);
    let again = post(&app, &en, &[]).await;
    assert_english(&again, "Error: Sign up your business");
    let other = post(&app, &en, &[("lang", "fr")]).await;
    assert_english(&other, "Error: Sign up your business");
}

/// Each step's English title (as escaped) and Spanish title, in the order of the test's screens.
const TITLES: [[&str; 2]; 10] = [
    ["Sign up your business", "Registre su negocio"],
    ["Error: Sign up your business", "Error: Registre su negocio"],
    ["Where your code goes", "A dónde enviamos su código"],
    ["Your sign-up is saved", "Su registro está guardado"],
    ["Your code comes by letter", "Su código llega por carta"],
    ["You&#39;re signed up", "Ya está registrado"],
    [
        "Error: How far do you travel for jobs?",
        "Error: ¿Qué tan lejos viaja para hacer trabajos?",
    ],
    [
        "Error: Show this business is yours",
        "Error: Demuestre que este negocio es suyo",
    ],
    [
        "Check your email for a code",
        "Busque el código en su correo electrónico",
    ],
    [
        "We&#39;re checking your sign-up by hand",
        "Estamos revisando su registro a mano",
    ],
];

/// TD-WORD-02 (WEB-019, D-3144, D-3830): every `Screen` step in Spanish with `PageLanguage::Es`,
/// and with `PageLanguage::En` each keeps its English title.
#[test]
fn td_word_02_every_screen_step_in_spanish() {
    let phone = MaskedContact::phone(&SecretString::from("928-555-0100".to_owned()), false);
    let land = MaskedContact::phone(&SecretString::from("928-555-0101".to_owned()), true);
    let cats = [("plumbing_repair".to_owned(), "Open now")];
    let done = DoneFacts {
        trade: "Plumbing".to_owned(),
        count: 3,
        waitlist: false,
        open_jobs: vec![Uuid::from_u128(0xab)],
    };
    let entered = Entered {
        problem: Some(("business_name", "Enter your business name.")),
        ..Entered::default()
    };
    let problems = [askhava_businesses::api::ServiceAreaProblem::Radius];
    let area = AreaEntered {
        problems: &problems,
        ..AreaEntered::default()
    };
    let screens = [
        Screen::Form(&cats),
        Screen::FormAgain(&cats, &entered),
        Screen::Contact(&phone, Some(&land)),
        Screen::NumberOld(3),
        Screen::Letter(&phone, 3),
        Screen::Done(&done),
        Screen::ServiceArea(&area),
        Screen::IdentityCode(&phone, Some(CodeProblem::Refused)),
        Screen::EmailCode(None),
        Screen::Review,
    ];
    for (screen, [en, es]) in screens.into_iter().zip(TITLES) {
        both(screen, en, es);
    }
}

/// `screen` in Spanish with the title `es` and no `en`, and in English with the title `en`.
fn both(screen: Screen<'_>, en: &str, es: &str) {
    let fees = fee_numbers(ConfirmSettings::default(), WindowRules::default());
    let parts = |language| Parts {
        texting_on: true,
        language,
        csrf: "fake-csrf",
        fees,
    };
    let spanish = signup_page(screen, &parts(PageLanguage::Es)).expect("Spanish page");
    assert_spanish(&spanish, es);
    assert!(!spanish.contains(en), "{es}: no English title");
    let english = signup_page(screen, &parts(PageLanguage::En)).expect("English page");
    assert_english(&english.replace("&#x27;", "&#39;"), en);
}

/// The Spanish note tied to the consent box by `aria-describedby`.
fn consent_note(html: &str) -> &str {
    let at = html.find("<p id=\"consent-note\">").expect("the note");
    &html[at..at + html[at..].find("</p>").expect("note end")]
}

/// CON-019, D-4562: the note says in Spanish every part of `consent_wording` for the channels
/// shown: notices by email not on paper, no recorded or automated calls, changeable in the
/// portal, and only with texts the frequency, the message and data rates and PARAR/AYUDA.
fn assert_note(html: &str, texts: bool) {
    let note = consent_note(html);
    let always = [
        "por correo electrónico, en la dirección de arriba, en lugar de en papel.",
        "nunca le hace llamadas grabadas ni automáticas.",
        "Puede cambiar esto cuando quiera en su portal.",
    ];
    for line in always {
        assert!(note.contains(line), "{line}");
    }
    let texting = [
        "textos con solicitudes de trabajo en el número de teléfono de arriba.",
        "La frecuencia de los mensajes varía.",
        "Pueden aplicarse tarifas de mensajes y datos.",
        "responda PARAR; para pedir ayuda, responda AYUDA.",
    ];
    for line in texting {
        assert_eq!(note.contains(line), texts, "{line}");
    }
    assert!(!note.contains("tres cosas"));
}

/// TD-WORD-02 (CON-019, BIZ-036, D-4562): with texts chosen the box names texts and the Spanish
/// note says the texting lines too; with email alone it says the email lines only.
#[test]
fn td_word_02_the_spanish_note_matches_the_consent_words() {
    let fees = fee_numbers(ConfirmSettings::default(), WindowRules::default());
    let cats = [("plumbing_repair".to_owned(), "Open now")];
    for texts in [true, false] {
        let entered = Entered {
            problem: Some(("consent", "Marque la casilla para aceptar los avisos.")),
            text_chosen: texts,
            ..Entered::default()
        };
        let parts = Parts {
            texting_on: true,
            language: PageLanguage::Es,
            csrf: "fake-csrf",
            fees,
        };
        let html = signup_page(Screen::FormAgain(&cats, &entered), &parts).expect("page");
        let channels = if texts {
            vec![ConsentChannel::Email, ConsentChannel::Text]
        } else {
            vec![ConsentChannel::Email]
        };
        assert_eq!(consent_words(&html), consent_box(&channels).wording);
        assert_note(&html, texts);
    }
}
