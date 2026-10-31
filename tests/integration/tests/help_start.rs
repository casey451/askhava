//! The HELP, INFO and AYUDA reply and START (TD-CON-07, TD-CON-15, TD-CON-19; CON-018, CON-013,
//! CON-015, D-066, D-131, D-289, D-290, D-349, D-371).
//!
//! Proves through `messaging::api` alone that a text that is only HELP, INFO or AYUDA gets the
//! CON-018 reply at 11 p.m. (Spanish for AYUDA) and from an opted-out number; that "I need help
//! with my sink" is not a keyword; that the reply holds the help email, askhava.com/help, the
//! reply times from the settings, the LIST line, the roaming line for a Canadian number only and
//! the STOP instruction, no personal name, no voice number and no "replies within" without
//! "usually"; that the help phone can never be the texting number; and that START lifts the
//! listing. The carrier is a fake. All numbers and data are fake.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, IdempotencyKey, SentText, SmsAdapter};
use askhava_messaging::api::{
    HelpSettings, HelpSettingsError, InboundText, Keyword, Language, NewText, OutboxError, Outcome,
    ProviderKeyword, Registry, Relation, To, enqueue, help_text, keyword, on_inbound_text,
    reply_to_help, send_checked,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const FROM: &str = "+19285550100";
const ARIZONA: &str = "+19285550142";
const CANADA: &str = "+14165550142";
/// 11 p.m. in Arizona on a fake date (06:00 UTC the next day).
const NIGHT: &str = "2026-11-12T06:00:00Z";

/// A fake carrier that keeps every text it is given.
#[derive(Clone, Default)]
struct FakeCarrier {
    sent: Arc<Mutex<Vec<(String, String)>>>,
}

impl SmsAdapter for FakeCarrier {
    async fn send_text(
        &self,
        _key: &IdempotencyKey,
        to: &str,
        _from: &str,
        body: &str,
    ) -> Result<SentText, AdapterError> {
        let mut sent = self.sent.lock().expect("lock");
        sent.push((to.to_owned(), body.to_owned()));
        Ok(SentText {
            sid: format!("SM{:032}", sent.len()),
            status: "queued".to_owned(),
        })
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x52), &key(0x62), &url).expect("vault")
}

fn night() -> Timestamp {
    NIGHT.parse().expect("time")
}

fn settings(days: u32, away: Option<&str>) -> HelpSettings {
    HelpSettings::new((days, 24), away, (None, FROM)).expect("settings")
}

async fn contact(db: &mut PgConnection, vault: &Vault, phone: &str) -> Uuid {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    store_contact(db, vault, &new).await.expect("contact")
}

fn inbound<'a>(
    her: Uuid,
    phone: &'a str,
    text: &'a str,
    provider: ProviderKeyword,
) -> InboundText<'a> {
    InboundText {
        inbox_id: Uuid::from_u128(text.bytes().fold(0x1b0, |n, b| {
            n.wrapping_mul(257).wrapping_add(u128::from(b))
        })),
        contact: her,
        relation: Relation::OpenRequest,
        phone,
        text,
        provider,
        emergency: false,
    }
}

/// Answers `text` from `phone` at 11 p.m. as the inbox handler would: the opt-out check, then the
/// HELP reply, sent through the checked send. Returns the body the carrier got, if any.
async fn answer(
    (db, vault): (&mut PgConnection, &Vault),
    carrier: &FakeCarrier,
    (her, phone): (Uuid, &str),
    text: &str,
) -> Option<String> {
    let msg = inbound(her, phone, text, ProviderKeyword::None);
    on_inbound_text(db, (&secret(), vault), &msg, night())
        .await
        .expect("inbound");
    let row = reply_to_help(db, &secret(), &msg, &settings(2, None))
        .await
        .expect("reply")?;
    let to = To::Homeowner {
        vault,
        relation: Relation::OpenRequest,
    };
    send_checked(db, (&secret(), carrier, vault), row, (to, FROM), night())
        .await
        .expect("sent")?;
    carrier
        .sent
        .lock()
        .expect("lock")
        .last()
        .map(|s| s.1.clone())
}

/// TD-CON-07, CON-018, D-131, D-066: HELP, INFO and AYUDA at 11 p.m. get the reply naming Ask Hava,
/// the help contacts and STOP at once; AYUDA gets Spanish; "help" with other words is no keyword.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_07_help_info_ayuda_at_night(pool: PgPool) {
    let (vault, carrier) = (vault(), FakeCarrier::default());
    let mut db = pool.acquire().await.expect("connection");
    let her = contact(&mut db, &vault, ARIZONA).await;
    for word in ["HELP", "info", "Help!"] {
        let body = answer((&mut db, &vault), &carrier, (her, ARIZONA), word).await;
        let body = body.expect("a reply");
        for part in [
            "Ask Hava:",
            "help@askhava.com",
            "askhava.com/help",
            "Reply STOP",
        ] {
            assert!(body.contains(part), "{word}: {part}");
        }
        assert!(body.contains("usually replies"));
    }
    let body = answer((&mut db, &vault), &carrier, (her, ARIZONA), "AYUDA").await;
    let body = body.expect("a reply");
    assert!(body.contains("Díganos qué pasa") && body.contains("Responda PARAR"));
    let sink = "I need help with my sink";
    assert_eq!(keyword(sink), None);
    assert_eq!(
        answer((&mut db, &vault), &carrier, (her, ARIZONA), sink).await,
        None
    );
}

/// TD-CON-07, CON-013, D-131, CON-015: the same reply from an opted-out number; its other texts
/// are still refused; START lifts the listing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_07_help_from_an_opted_out_number_and_start(pool: PgPool) {
    let (vault, carrier) = (vault(), FakeCarrier::default());
    let mut db = pool.acquire().await.expect("connection");
    let her = contact(&mut db, &vault, ARIZONA).await;
    let stop = inbound(her, ARIZONA, "STOP", ProviderKeyword::OptOut);
    on_inbound_text(&mut db, (&secret(), &vault), &stop, night())
        .await
        .expect("stop");
    let body = answer((&mut db, &vault), &carrier, (her, ARIZONA), "AYUDA").await;
    assert!(body.expect("a reply").contains("askhava.com/help"));
    let other = NewText {
        template_id: "first_quote",
        subject: &[Uuid::from_u128(7)],
        recipient: her,
        relation: Relation::OpenRequest,
        body: "Ask Hava: fake body",
    };
    let (row, _) = enqueue(&mut db, &secret(), &other).await.expect("queued");
    let to = To::Homeowner {
        vault: &vault,
        relation: Relation::OpenRequest,
    };
    let refused = send_checked(
        &mut db,
        (&secret(), &carrier, &vault),
        row,
        (to, FROM),
        night(),
    )
    .await;
    assert_eq!(refused.err(), Some(OutboxError::Suppressed));
    let start = inbound(her, ARIZONA, "START", ProviderKeyword::Start);
    let outcome = on_inbound_text(&mut db, (&secret(), &vault), &start, night()).await;
    assert_eq!(outcome, Ok(Outcome::Resubscribed));
    let to = To::Homeowner {
        vault: &vault,
        relation: Relation::OpenRequest,
    };
    let sent = send_checked(
        &mut db,
        (&secret(), &carrier, &vault),
        row,
        (to, FROM),
        night(),
    )
    .await;
    assert!(sent.expect("sent").is_some());
}

/// TD-CON-19, CON-018, D-371: the D-371 text, plus the roaming line for a Canadian number only;
/// no reply anywhere says "replies within" without "usually".
#[test]
fn td_con_19_the_reply_is_the_d_371_text_with_roaming_for_canada() {
    let set = settings(2, None);
    let us = help_text(&set, Language::En, ARIZONA).expect("text");
    let ca = help_text(&set, Language::En, CANADA).expect("text");
    for body in [&us, &ca] {
        assert!(body.contains("usually replies within 2 days"));
        assert!(body.contains("If anyone is in danger, call 911."));
    }
    assert!(!us.contains("roaming") && ca.contains("Canadian numbers may pay roaming charges."));
    let registry = Registry::embedded().expect("registry");
    for id in registry.ids() {
        let text = registry.template(&id, Language::En).map(|t| t.0.to_owned());
        let text = text.expect("english");
        assert!(!text.contains("replies within") || text.contains("usually replies within"));
    }
}

/// TD-CON-15, CON-018, D-289, D-290, D-349: the fixed sentences in both languages, the LIST line,
/// no personal name or old promise, no voice number; the reply time follows the setting; the help
/// phone can never be the texting number.
#[test]
fn td_con_15_the_reply_holds_the_fixed_sentences_and_follows_the_settings() {
    let en = help_text(&settings(2, None), Language::En, ARIZONA).expect("text");
    let wording = "Tell us what's wrong in one text. The Ask Hava owner gets HELP messages right \
        away and usually replies within 2 days, or within 24 hours if your job is urgent. This \
        number is text-only. If anyone is in danger, call 911.";
    let list = "Can't open links? Reply LIST to get your quotes by text.";
    for part in [wording, list, "help@askhava.com", "askhava.com/help"] {
        assert!(en.contains(part), "{part}");
    }
    let es = help_text(&settings(2, None), Language::Es, ARIZONA).expect("text");
    let palabras = "Díganos qué pasa en un solo texto. El dueño de Ask Hava recibe los mensajes de \
        AYUDA de inmediato y normalmente responde en 2 días, o en 24 horas si su trabajo es \
        urgente. Este número es solo para textos. Si alguien está en peligro, llame al 911.";
    let lista = "¿No puede abrir enlaces? Responda LISTA para recibir sus cotizaciones por texto.";
    for part in [palabras, lista, "help@askhava.com", "askhava.com/help"] {
        assert!(es.contains(part), "{part}");
    }
    for body in [&en, &es] {
        for banned in ["Casey", "twice a week", "within 4 days", "dos veces"] {
            assert!(!body.contains(banned), "{banned}");
        }
        let digits: String = body.chars().filter(char::is_ascii_digit).collect();
        assert_eq!(
            digits, "224911",
            "only the times and 911, so no voice number"
        );
    }
    let three = help_text(&settings(3, None), Language::En, ARIZONA).expect("text");
    assert!(three.contains("usually replies within 3 days") && three != en);
    let away = help_text(&settings(2, Some("Nov 20")), Language::En, CANADA).expect("text");
    assert!(away.contains("is away until Nov 20 and will reply by then."));
    assert!(!away.contains("text-only") && away.contains("roaming"));
    let same = HelpSettings::new((2, 24), None, (Some("(928) 555-0100"), FROM));
    assert_eq!(same, Err(HelpSettingsError::HelpPhoneIsTextingNumber));
    let apart = HelpSettings::new((2, 24), None, (Some("+19285550111"), FROM));
    assert!(apart.is_ok());
    assert_eq!(keyword("ayuda"), Some(Keyword::Help(Language::Es)));
}
