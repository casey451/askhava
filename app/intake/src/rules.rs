//! Pure rules for the first reply to a request (CON-001 to CON-004, CON-026, CON-027, CON-031).
//!
//! [`plan`] decides, from whether this request already had its consent text and what the new
//! message is, which texts go now and what the conversation does next. The consent text is the
//! first message of every request (after the 911 text on a 911-list match, INT-004, D-008), it asks
//! for no reply (D-419), carries no fee line (CON-003, the template's words), and it goes at most
//! once per request. A text that is only an opt-out word
//! or HELP, INFO or AYUDA was already answered by `messaging` and never gets the consent text
//! (D-131). Every later reply is a conversation step, never a wait for consent (CON-027).

use askhava_messaging::api::Language;

/// The CON-031 consent text's template ID (`messaging` templates).
pub const CONSENT_TEMPLATE: &str = "consent";
/// The consent text with CALL-022's recorded-call sentence, used only while the safety voice call
/// is on (CALL-019, CALL-022, D-384).
pub const CONSENT_CALL_TEMPLATE: &str = "consent_safety_call";
/// The INT-004 911 text's template ID, sent before the consent text on a 911-list match (D-436).
pub const SAFETY_TEMPLATE: &str = "emergency_911";
/// The CON-027 line for "who is this?" or "is this a real person?", put before the next question.
pub const DISCLOSURE_TEMPLATE: &str = "who_is_this";

/// What `messaging` and the reader found in one inbound text (flags and scrubbed words only).
#[derive(Debug, Clone, Copy)]
pub struct Inbound<'a> {
    /// The scrubbed words (`privacy::api::read_inbound`), never the raw text.
    pub words: &'a str,
    /// The text is an opt-out (CON-008, CON-010), already handled by `messaging`.
    pub opt_out: bool,
    /// The text is only HELP, INFO or AYUDA (CON-018), already answered by `messaging`.
    pub help: bool,
    /// The reader matched the 911 list (INT-002, INT-004).
    pub emergency: bool,
    /// The AI reading's language, where the AI may run (AI-008); `None` when it did not.
    pub ai_spanish: Option<bool>,
}

/// The owner settings the first reply reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct Settings {
    /// "Spanish conversations" (TXT-039), off by default.
    pub spanish_conversations_on: bool,
    /// "Safety voice call" (CALL-019), off by default and only on once CALL-022 is met.
    pub safety_call_on: bool,
}

/// A text to queue now, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Send {
    /// The INT-004 911 text, in the language the message reads as (its Spanish stays on).
    Safety(Language),
    /// The consent text from `template`, in `language` (CON-026, CON-031).
    Consent {
        /// [`CONSENT_TEMPLATE`] or [`CONSENT_CALL_TEMPLATE`].
        template: &'static str,
        /// English unless the message reads as Spanish and Spanish conversations are on.
        language: Language,
    },
}

/// What the conversation does after the texts of a [`Plan`] (CON-027).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Then {
    /// Nothing here: `messaging` already answered (opt-out or HELP).
    Nothing,
    /// A new request: the first intake question follows at once as its own text (CON-031).
    FirstQuestion,
    /// A reply in the conversation: an intake answer, or the question asked again (T040).
    IntakeReply,
    /// The [`DISCLOSURE_TEMPLATE`] line, then the next question, in one text (CON-027).
    DiscloseThenAsk,
}

/// The decision for one inbound text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The texts to queue now, in this order.
    pub texts: Vec<Send>,
    /// What follows.
    pub then: Then,
}

/// The first reply rule (CON-001, CON-004, CON-026, CON-027, D-008, D-131, D-419): `consent_sent`
/// says whether the open request already had its consent text. A new request gets the 911 text
/// first on a 911-list match, then the consent text; nothing ever waits for a reply.
#[must_use]
pub fn plan(consent_sent: bool, msg: &Inbound<'_>, settings: Settings) -> Plan {
    if msg.opt_out || msg.help {
        return Plan {
            texts: Vec::new(),
            then: Then::Nothing,
        };
    }
    if consent_sent {
        let then = if asks_who(msg.words) {
            Then::DiscloseThenAsk
        } else {
            Then::IntakeReply
        };
        return Plan {
            texts: Vec::new(),
            then,
        };
    }
    let wanted = wanted_language(msg.words, msg.ai_spanish);
    let mut texts = Vec::with_capacity(2);
    if msg.emergency {
        texts.push(Send::Safety(wanted));
    }
    texts.push(Send::Consent {
        template: consent_template(settings.safety_call_on),
        language: consent_language(wanted, settings.spanish_conversations_on),
    });
    Plan {
        texts,
        then: Then::FirstQuestion,
    }
}

/// The consent template: CALL-022's sentence only while the safety voice call is on (D-384).
#[must_use]
pub fn consent_template(safety_call_on: bool) -> &'static str {
    if safety_call_on {
        CONSENT_CALL_TEMPLATE
    } else {
        CONSENT_TEMPLATE
    }
}

/// The consent text's language (CON-026, TXT-039, D-065, D-427): Spanish only when the first
/// message reads as Spanish and Spanish conversations are on.
#[must_use]
pub fn consent_language(wanted: Language, spanish_conversations_on: bool) -> Language {
    if spanish_conversations_on {
        wanted
    } else {
        Language::En
    }
}

/// The language a message reads as (D-065): Spanish when the keyword rules find it or the AI
/// reading says so where the AI may run.
#[must_use]
pub fn wanted_language(words: &str, ai_spanish: Option<bool>) -> Language {
    if reads_spanish(words) || ai_spanish == Some(true) {
        Language::Es
    } else {
        Language::En
    }
}

/// Common Spanish words that are not English words, folded (no accents, lower case).
const SPANISH_WORDS: &[&str] = &[
    "hola",
    "buenos",
    "buenas",
    "mis",
    "necesito",
    "tengo",
    "tiene",
    "por",
    "favor",
    "gracias",
    "el",
    "los",
    "las",
    "del",
    "una",
    "que",
    "con",
    "para",
    "esta",
    "gotea",
    "fuga",
    "agua",
    "casa",
    "bano",
    "cocina",
    "funciona",
    "puede",
    "rompio",
    "aire",
    "calentador",
    "lavabo",
    "techo",
    "puerta",
    "luz",
    "estufa",
    "huele",
];

/// The keyword rule of D-065: two different Spanish words, or a letter only Spanish uses here.
#[must_use]
pub fn reads_spanish(words: &str) -> bool {
    if words.chars().any(|c| matches!(c, 'ñ' | 'Ñ' | '¿' | '¡')) {
        return true;
    }
    let folded = fold(words);
    let mut found: Vec<&str> = folded
        .split(' ')
        .filter(|w| SPANISH_WORDS.contains(w))
        .collect();
    found.sort_unstable();
    found.dedup();
    found.len() >= 2
}

/// Questions about who is texting (CON-027), folded, matched as whole words in order.
const WHO_PHRASES: &[&str] = &[
    "who is this",
    "whos this",
    "who are you",
    "is this a person",
    "is this a real person",
    "is this a bot",
    "is this a robot",
    "is this real",
    "is this automated",
    "are you a person",
    "are you real",
    "are you a bot",
    "are you human",
    "is this a human",
    "quien es",
    "quien habla",
    "es una persona",
    "eres una persona",
    "es un robot",
];

/// Whether a reply asks who this is or whether it is a person (CON-027).
#[must_use]
pub fn asks_who(words: &str) -> bool {
    let folded = format!(" {} ", fold(words));
    WHO_PHRASES
        .iter()
        .any(|p| folded.contains(&format!(" {p} ")))
}

/// Lower case, Spanish accents folded, apostrophes dropped, every other non-letter a space, and
/// runs of spaces made one.
fn fold(words: &str) -> String {
    let mapped: String = words
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '\'' | '\u{2019}'))
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
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The stored code of a language (`intake.conversation.language`).
#[must_use]
pub fn language_code(language: Language) -> &'static str {
    match language {
        Language::En => "en",
        Language::Es => "es",
    }
}

/// The language of a stored code; anything but `es` is English.
#[must_use]
pub fn language_from_code(code: &str) -> Language {
    if code == "es" {
        Language::Es
    } else {
        Language::En
    }
}
