//! TD-CON-32 (T350, D-2621; CON-010, CON-013, CON-035, AI-020): an opt-out phrase that follows a
//! stated name is still heard after the scrubber, and an opt-out word is never remembered or
//! removed as a name, so a later opt-out from the same number is heard too.
//!
//! The inbound route detects a phrase opt-out on the scrubbed text (`messaging::api::detect_opt_out`
//! after `privacy::api::read_inbound_for`), so a scrubber that takes "stop" as the second word of a
//! name loses the opt-out. `privacy` keeps its own never-a-name list (`NEVER_A_NAME`, it may not
//! depend on `messaging`); the coverage case here fails when messaging's lists gain a word it lacks.
//! Every name and phone is fictional.

use askhava_integration::set_app_role;
use askhava_messaging::api::{
    APP_OPT_OUT_WORDS, OPT_OUT_PHRASES, PROVIDER_OPT_OUT_WORDS, RESUBSCRIBE_WORDS,
    SPANISH_OPT_OUT_WORDS, detect_opt_out,
};
use askhava_privacy::api::{
    CommonWords, InboundWords, Known, NEVER_A_NAME, NameMemory, NewContact, Vault,
    read_inbound_for, scrub, store_contact,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use secrecy::SecretString;
use sqlx::PgPool;

const PREFIXES: [&str; 5] = [
    "This is Bob ",
    "I'm Bob ",
    "I am Bob ",
    "It's Maria ",
    "Soy Ana ",
];
/// The sole HELP words the route passes in (`app/server/src/inbound_text.rs`, CON-018).
const HELP_WORDS: [&str; 3] = ["help", "info", "ayuda"];
const WORDS: InboundWords<'static> = InboundWords {
    emergency: &[],
    opt_out: &PROVIDER_OPT_OUT_WORDS,
    help: &HELP_WORDS,
    start: &RESUBSCRIBE_WORDS,
};

fn scrubbed(raw: &str) -> String {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    scrub(raw, &known, &mut NameMemory::default())
        .as_str()
        .to_owned()
}

fn vault() -> Vault {
    let key = |byte: u8| format!("1:{}", STANDARD.encode([byte; 32]));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x3a).into(), &key(0x4b).into(), &url).expect("vault")
}

/// Folds like messaging's detector: lower case, accents off, apostrophes dropped, other marks
/// splitting words.
fn folded_words(text: &str) -> Vec<String> {
    let plain: String = text
        .to_lowercase()
        .chars()
        .filter(|&c| c != '\'')
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' | 'ü' => 'u',
            'ñ' => 'n',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect();
    plain.split_whitespace().map(str::to_owned).collect()
}

/// TD-CON-32: every opt-out phrase after a stated name survives the scrubber and is detected.
#[test]
fn td_con_32_every_phrase_after_a_name_is_still_an_opt_out() {
    for prefix in PREFIXES {
        for (id, phrase) in OPT_OUT_PHRASES {
            let raw = format!("{prefix}{phrase}");
            let out = scrubbed(&raw);
            assert!(
                detect_opt_out(&out).is_some(),
                "{id} lost after {prefix:?}: {out:?}"
            );
            assert!(!out.contains("Bob") && !out.contains("Maria") && !out.contains("Ana "));
        }
    }
}

/// TD-CON-32: privacy's never-a-name list holds every word of messaging's opt-out phrases and its
/// sole opt-out, HELP and START words.
#[test]
fn td_con_32_the_never_a_name_list_covers_messagings_lists() {
    let phrases = OPT_OUT_PHRASES.iter().map(|(_, p)| *p);
    let sole = PROVIDER_OPT_OUT_WORDS
        .iter()
        .chain(&SPANISH_OPT_OUT_WORDS)
        .chain(&APP_OPT_OUT_WORDS)
        .chain(&RESUBSCRIBE_WORDS)
        .chain(&HELP_WORDS)
        .copied();
    for text in phrases.chain(sole) {
        for word in folded_words(text) {
            assert!(
                NEVER_A_NAME.contains(&word.as_str()),
                "{word:?} (from {text:?}) missing from privacy's NEVER_A_NAME"
            );
        }
    }
}

/// TD-CON-32 (AI-020 unchanged): a stated name is still cut.
#[test]
fn td_con_32_a_stated_name_is_still_removed() {
    assert_eq!(
        scrubbed("This is Bob, sink leaking"),
        "This is [removed] sink leaking"
    );
    assert_eq!(
        scrubbed("I'm Bob stop texting me"),
        "I'm [removed] stop texting me"
    );
}

/// TD-CON-32: "This is Bob stop texting me" then "Stop texting me please" from one number: "stop"
/// is never remembered as her name, so the second text is an opt-out too.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_32_a_later_opt_out_from_the_same_number_is_heard(pool: PgPool) {
    let vault = vault();
    let mut db = pool.acquire().await.expect("connection");
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    let new = NewContact {
        phone: Some(SecretString::from("+19285550191".to_owned())),
        ..NewContact::default()
    };
    let her = store_contact(&mut db, &vault, &new).await.expect("contact");
    for raw in ["This is Bob stop texting me", "Stop texting me please"] {
        let read = read_inbound_for(&mut db, &vault, her, raw, &WORDS)
            .await
            .expect("read");
        let out = read.text.as_str();
        assert!(detect_opt_out(out).is_some(), "{raw:?} read as {out:?}");
    }
    let later = read_inbound_for(&mut db, &vault, her, "Bob again", &WORDS)
        .await
        .expect("read");
    assert_eq!(later.text.as_str(), "[removed] again");
}

/// TD-CON-32: a known name word that is an opt-out word (as a name stored before D-2621 reaches the
/// scrubber, with the vault name) is ignored; the other words still go.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_32_a_known_name_that_is_an_opt_out_word_is_ignored(pool: PgPool) {
    let vault = vault();
    let mut db = pool.acquire().await.expect("connection");
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    let new = NewContact {
        name: Some(SecretString::from("Zorba Stop".to_owned())),
        phone: Some(SecretString::from("+19285550192".to_owned())),
        ..NewContact::default()
    };
    let her = store_contact(&mut db, &vault, &new).await.expect("contact");
    let read = read_inbound_for(&mut db, &vault, her, "Zorba says stop texting me", &WORDS)
        .await
        .expect("read");
    assert_eq!(read.text.as_str(), "[removed] says stop texting me");
    assert!(detect_opt_out(read.text.as_str()).is_some());
}
