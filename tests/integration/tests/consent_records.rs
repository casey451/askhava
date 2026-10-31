//! Consent records, the gates on cards and texts, and the homeowner terms acceptance on a fresh
//! database (TD-CON-02, TD-CON-03, TD-CON-14, TD-CON-24, TD-TERMS-07; CON-005, CON-007, CON-021,
//! TERMS-001, TERMS-016, TERMS-033).
//!
//! Proves through `intake::api` alone that a new number's first text stores its consent record at
//! once (time, inbound message ID, channel, consent-text version) with no reply asked for; that
//! agreement words after the consent text record nothing; that no card may go and no one may be
//! texted before consent; and that only a pick stores a terms acceptance, with every TERMS-033
//! field. All IDs, numbers and texts are fake.

use askhava_intake::api::rules::{Inbound, Settings, Then};
use askhava_intake::api::{
    IntakeError, TermsAcceptance, TermsChannel, TextIn, card_allowed, consent_records, may_text,
    receive_text, record_terms_acceptance, terms_acceptances,
};
use askhava_messaging::api::Language;
use jiff::Timestamp;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use sqlx::pool::PoolConnection;
use sqlx::types::Uuid;

type Db = PoolConnection<sqlx::Postgres>;

const TERMS: &str = "Fake homeowner terms, version 1, for tests only.";
const ASSENT: &str = "By picking, you agree to the Terms";

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn at(minute: i64) -> Timestamp {
    let start: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
    start + jiff::SignedDuration::from_mins(minute)
}

fn words(words: &str) -> Inbound<'_> {
    Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    }
}

fn text(contact: Uuid, minute: i64, inbound: Inbound<'_>) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128((contact.as_u128() << 16) + minute as u128),
        contact,
        inbound,
        received_at: at(minute),
    }
}

/// Sends `said` in order after a first text and returns the request.
async fn converse(db: &mut PgConnection, contact: Uuid, said: &[&str], settings: Settings) -> Uuid {
    let first = receive_text(
        db,
        &secret(),
        &text(contact, 0, words("my sink leaks")),
        settings,
    )
    .await
    .expect("first");
    for (i, reply) in said.iter().enumerate() {
        let msg = text(contact, i as i64 + 1, words(reply));
        let handled = receive_text(db, &secret(), &msg, settings)
            .await
            .expect("reply");
        assert!(handled.texts.is_empty(), "{reply} queued a text");
        assert_eq!(handled.then, Then::IntakeReply, "{reply}");
    }
    first.conversation.expect("request")
}

/// TD-CON-02 (CON-005, D-419): the first text records consent at once, with its time, inbound
/// message ID, channel and consent-text version, and the first question follows with no wait.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_02_first_text_records_consent_at_once(mut db: Db) {
    let contact = Uuid::from_u128(0xA1);
    let first = text(contact, 0, words("my sink leaks"));
    let handled = receive_text(&mut db, &secret(), &first, Settings::default())
        .await
        .expect("first");
    assert_eq!(handled.then, Then::FirstQuestion);
    let id = handled.conversation.expect("request");
    let records = consent_records(&mut db, id).await.expect("records");
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(
        (r.channel.as_str(), r.inbox_id),
        ("sms", Some(first.inbox_id))
    );
    assert_eq!(
        (r.web_chat_session, r.consented_at),
        (None, first.received_at)
    );
    assert!(r.consent_version.starts_with("consent:en:"));
    assert_eq!(r.consent_version.len(), "consent:en:".len() + 12);
}

/// TD-CON-02 (CON-005): a different consent wording is a different version: the Spanish text and
/// the safety-call text each get their own.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_02_each_consent_wording_has_its_own_version(mut db: Db) {
    let spanish = Settings {
        spanish_conversations_on: true,
        safety_call_on: false,
    };
    let call = Settings {
        spanish_conversations_on: false,
        safety_call_on: true,
    };
    let cases = [
        (0xB1, "my sink leaks", Settings::default()),
        (0xB2, "Hola, mi lavabo gotea", spanish),
        (0xB3, "my sink leaks", call),
    ];
    let mut versions = Vec::new();
    for (n, said, settings) in cases {
        let msg = text(Uuid::from_u128(n), 0, words(said));
        let handled = receive_text(&mut db, &secret(), &msg, settings)
            .await
            .expect("first");
        let id = handled.conversation.expect("request");
        versions.push(
            consent_records(&mut db, id).await.expect("read")[0]
                .consent_version
                .clone(),
        );
    }
    assert!(versions[1].starts_with("consent:es:"), "{}", versions[1]);
    assert!(
        versions[2].starts_with("consent_safety_call:en:"),
        "{}",
        versions[2]
    );
    assert!(versions[0] != versions[1] && versions[0] != versions[2]);
}

/// TD-CON-02 (CON-005, TERMS-001, TERMS-016, D-420): "ok", "yes please", "Y" and "Sí" after the
/// consent text are intake answers: no second consent record, no terms acceptance.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_02_agreement_replies_are_no_consent_or_terms_event(mut db: Db) {
    let said = ["ok", "yes please", "Y", "Sí"];
    let id = converse(&mut db, Uuid::from_u128(0xA2), &said, Settings::default()).await;
    assert_eq!(consent_records(&mut db, id).await.expect("read").len(), 1);
    assert!(
        terms_acceptances(&mut db, id)
            .await
            .expect("read")
            .is_empty()
    );
}

/// TD-CON-14 (CON-027, D-419): "fine", "go ahead" and "de acuerdo" after the consent text are
/// intake replies and record nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_14_fine_and_go_ahead_record_nothing(mut db: Db) {
    let said = ["fine", "go ahead", "de acuerdo"];
    let id = converse(&mut db, Uuid::from_u128(0xA3), &said, Settings::default()).await;
    assert_eq!(consent_records(&mut db, id).await.expect("read").len(), 1);
    assert!(
        terms_acceptances(&mut db, id)
            .await
            .expect("read")
            .is_empty()
    );
}

/// TD-CON-03 (CON-007, CON-021): no card before consent; a HELP-only number and a tenant or
/// on-site contact who never wrote may not be texted; every queued text's recipient has consent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_03_no_card_or_text_before_consent(mut db: Db) {
    assert!(
        !card_allowed(&mut db, Uuid::from_u128(0xDEAD))
            .await
            .expect("card")
    );
    let help_only = Uuid::from_u128(0xA4);
    let help = Inbound {
        help: true,
        ..words("HELP")
    };
    receive_text(
        &mut db,
        &secret(),
        &text(help_only, 0, help),
        Settings::default(),
    )
    .await
    .expect("help");
    let tenant = Uuid::from_u128(0xA5);
    let writer = Uuid::from_u128(0xA6);
    let gas = Inbound {
        emergency: true,
        ..words("I smell gas")
    };
    let handled = receive_text(
        &mut db,
        &secret(),
        &text(writer, 0, gas),
        Settings::default(),
    )
    .await
    .expect("gas");
    let id = handled.conversation.expect("request");
    assert!(card_allowed(&mut db, id).await.expect("card"));
    assert!(!may_text(&mut db, help_only).await.expect("help"));
    assert!(!may_text(&mut db, tenant).await.expect("tenant"));
    let recipients: Vec<Uuid> = sqlx::query_scalar("SELECT recipient_id FROM messaging.outbox")
        .fetch_all(&mut *db)
        .await
        .expect("outbox");
    assert_eq!(recipients.len(), 2);
    for recipient in recipients {
        assert!(may_text(&mut db, recipient).await.expect("recipient"));
    }
}

/// TD-CON-24 (CON-004, CON-005, CON-027, D-419): over generated conversations no queued text asks
/// for "Reply OK", "Reply YES" or "Responda OK", no step waits on a reply, and each request has
/// exactly one consent record.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_24_generated_conversations_never_wait_for_consent(mut db: Db) {
    let pool = [
        "ok",
        "is this a real person?",
        "Hola, mi lavabo gotea",
        "no",
        "Sí",
        "3 pm",
    ];
    for n in 0..12_u128 {
        let settings = Settings {
            spanish_conversations_on: n % 2 == 0,
            safety_call_on: n % 3 == 0,
        };
        let start = pool[(n as usize) % pool.len()];
        let contact = Uuid::from_u128(0x1000 + n);
        let first = receive_text(
            &mut db,
            &secret(),
            &text(contact, 0, words(start)),
            settings,
        )
        .await
        .expect("first");
        assert_eq!(first.then, Then::FirstQuestion);
        for k in 1..=4_i64 {
            let said = pool[(n as usize * 7 + k as usize) % pool.len()];
            let next = receive_text(&mut db, &secret(), &text(contact, k, words(said)), settings)
                .await
                .expect("reply");
            assert!(matches!(
                next.then,
                Then::IntakeReply | Then::DiscloseThenAsk
            ));
        }
        let id = first.conversation.expect("request");
        assert_eq!(consent_records(&mut db, id).await.expect("read").len(), 1);
    }
    let bodies: Vec<String> = sqlx::query_scalar("SELECT body FROM messaging.outbox")
        .fetch_all(&mut *db)
        .await
        .expect("outbox");
    assert_eq!(bodies.len(), 12);
    for body in bodies {
        for ask in ["Reply OK", "Reply YES", "Responda OK"] {
            assert!(!body.contains(ask), "asks {ask}");
        }
    }
}

fn acceptance(conversation: Uuid, pick: Uuid, channel: TermsChannel) -> TermsAcceptance<'static> {
    TermsAcceptance {
        conversation,
        pick,
        document: "homeowner_terms",
        version: "2026-11-01",
        published_text: TERMS,
        language: Language::En,
        channel,
        assent_line: ASSENT,
        accepted_at: at(30),
    }
}

/// TD-TERMS-07 (TERMS-016, TERMS-033, D-492): a quote-page pick and a text pick each store one row
/// with every field; the same pick again stores nothing more; an unknown request is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_terms_07_a_pick_stores_one_full_acceptance_row(mut db: Db) {
    let id = converse(&mut db, Uuid::from_u128(0xA7), &["ok"], Settings::default()).await;
    let page = acceptance(id, Uuid::from_u128(0xF1), TermsChannel::QuotePage);
    assert!(record_terms_acceptance(&mut db, &page).await.expect("page"));
    assert!(
        !record_terms_acceptance(&mut db, &page)
            .await
            .expect("again")
    );
    let inbox_id = Uuid::from_u128(0xF9);
    let by_text = acceptance(
        id,
        Uuid::from_u128(0xF2),
        TermsChannel::TextPick { inbox_id },
    );
    assert!(
        record_terms_acceptance(&mut db, &by_text)
            .await
            .expect("text")
    );
    let rows = terms_acceptances(&mut db, id).await.expect("read");
    assert_eq!(rows.len(), 2);
    let hash: String = Sha256::digest(TERMS.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    for row in &rows {
        assert_eq!(
            (row.document.as_str(), row.version.as_str()),
            ("homeowner_terms", "2026-11-01")
        );
        assert_eq!(
            (row.text_sha256.as_str(), row.language.as_str()),
            (hash.as_str(), "en")
        );
        assert_eq!(
            (row.assent_line.as_str(), row.accepted_at),
            (ASSENT, at(30))
        );
        assert_eq!(row.conversation_id, id);
    }
    let channels: Vec<_> = rows
        .iter()
        .map(|r| (r.channel.as_str(), r.pick_id, r.inbox_id))
        .collect();
    assert!(channels.contains(&("quote_page", Uuid::from_u128(0xF1), None)));
    assert!(channels.contains(&("text_pick", Uuid::from_u128(0xF2), Some(inbox_id))));
    let unknown = acceptance(
        Uuid::from_u128(0xBAD),
        Uuid::from_u128(0xF3),
        TermsChannel::QuotePage,
    );
    let refused = record_terms_acceptance(&mut db, &unknown).await;
    assert_eq!(refused, Err(IntakeError::UnknownRequest));
}

/// TERMS-016 (postgres brain P5): the app role may add a terms acceptance but never change one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn terms_acceptances_are_append_only_for_the_app_role(mut db: Db) {
    let id = converse(&mut db, Uuid::from_u128(0xA8), &[], Settings::default()).await;
    let page = acceptance(id, Uuid::from_u128(0xF4), TermsChannel::QuotePage);
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    assert!(
        record_terms_acceptance(&mut db, &page)
            .await
            .expect("insert")
    );
    for sql in [
        "UPDATE intake.terms_acceptance SET version = '2026-12-01'",
        "DELETE FROM intake.terms_acceptance",
        "TRUNCATE intake.terms_acceptance",
    ] {
        assert!(sqlx::query(sql).execute(&mut *db).await.is_err(), "{sql}");
    }
}
