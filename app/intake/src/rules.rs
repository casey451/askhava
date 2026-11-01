//! Pure rules for the first reply to a request (CON-001 to CON-004, CON-026, CON-027, CON-031).
//!
//! [`plan`] decides, from whether this request already had its consent text and what the new
//! message is, which texts go now and what the conversation does next. The consent text is the
//! first message of every request (after the 911 text on a 911-list match, INT-004, D-008), it asks
//! for no reply (D-419), carries no fee line (CON-003, the template's words), and it goes at most
//! once per request. A text that is only an opt-out word
//! or HELP, INFO or AYUDA was already answered by `messaging` and never gets the consent text
//! (D-131). Every later reply is a conversation step, never a wait for consent (CON-027).
//!
//! The conversation rules (T040) are pure too: what opens a job ([`opens_job`], INT-007, INT-008),
//! the INT-010 join window ([`joins_open_job`]) and, after it, the same-trade word-share rule
//! ([`follow_up_joins`], T042), exact answers ([`pick_option`], INT-011, INT-012),
//! the under-18 check ([`age_stop`], INT-038) and the name-and-address split (INT-027).

use askhava_messaging::api::Language;
use jiff::{SignedDuration, Timestamp};

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
pub(crate) fn fold(words: &str) -> String {
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

/// The INT-010 window after the open job's last homeowner message in which a follow-up joins it
/// (a setting, default 2 hours; the caller passes the setting's value).
pub const JOIN_WINDOW: SignedDuration = SignedDuration::from_hours(2);
/// The INT-038 reply's template ID (English, and Spanish that stays on).
pub const UNDER_18_TEMPLATE: &str = "under_18";
/// The INT-027 name-and-address question's template ID.
pub const NAME_ADDRESS_TEMPLATE: &str = "name_address";

/// Words that, as the first word, make a message cancel-like (INT-008), folded.
const CANCEL_WORDS: &[&str] = &[
    "cancel",
    "cancelar",
    "cancela",
    "stop",
    "quit",
    "unsubscribe",
    "nevermind",
    "nvm",
    "parar",
];
/// Phrases that make a message cancel-like anywhere in it, folded.
const CANCEL_PHRASES: &[&str] = &["never mind", "forget it", "olvidalo", "no quiero nada"];
/// The longest message that is cancel-like by holding a cancel word anywhere in it.
const CANCEL_SHORT: usize = 4;

/// Whether a message is cancel-like (INT-008): it starts with a cancel word, holds a cancel
/// phrase, or is a short message holding a cancel word. "The leak will not stop" is a request.
#[must_use]
pub fn cancel_like(words: &str) -> bool {
    let folded = fold(words);
    let tokens: Vec<&str> = folded.split(' ').filter(|t| !t.is_empty()).collect();
    let has_word = tokens.iter().any(|t| CANCEL_WORDS.contains(t));
    let first = tokens.first().is_some_and(|t| CANCEL_WORDS.contains(t));
    let padded = format!(" {folded} ");
    let phrase = CANCEL_PHRASES
        .iter()
        .any(|p| padded.contains(&format!(" {p} ")));
    first || phrase || (has_word && tokens.len() <= CANCEL_SHORT)
}

/// Whether a message opens a new job (INT-007, INT-008, D-138): more than two words, or any length
/// with an emergency keyword, and never a cancel-like message. "ME", "86403" and "ok sure" open
/// nothing.
#[must_use]
pub fn opens_job(words: &str, emergency: bool) -> bool {
    !cancel_like(words) && (emergency || words.split_whitespace().count() > 2)
}

/// Whether a second message at `now` joins the open job whose last homeowner message was at
/// `last`, even if it reads as another trade (INT-010): strictly inside `window`, so 1:59:59 joins
/// and 2:00:00 does not; from then on [`follow_up_joins`] decides.
#[must_use]
pub fn joins_open_job(last: Timestamp, now: Timestamp, window: SignedDuration) -> bool {
    let since = now.duration_since(last);
    !since.is_negative() && since < window
}

/// The INT-010 share of a message's words that must appear in the job's scrubbed description for
/// a same-trade message after the window to join (a setting, default 50%; the caller passes it).
pub const JOIN_SHARE_PERCENT: u8 = 50;
/// Common words left out of the INT-010 word share, folded (English and Spanish).
const COMMON_WORDS: &[&str] = &[
    "the", "a", "an", "my", "is", "it", "its", "and", "or", "of", "to", "in", "on", "at", "for",
    "i", "im", "me", "we", "our", "this", "that", "with", "be", "are", "was", "el", "la", "los",
    "las", "un", "una", "mi", "es", "y", "de", "en", "que",
];

/// Whether at least `share_percent` of the message's distinct words, common words left out, also
/// appear in the job's scrubbed `description` (INT-010, D-485): 5 of 10 joins at 50%, 49 of 100
/// does not; a message of only common words never joins.
#[must_use]
pub fn shares_words(message: &str, description: &str, share_percent: u8) -> bool {
    let folded_description = fold(description);
    let known: Vec<&str> = folded_description.split(' ').collect();
    let folded = fold(message);
    let mut words: Vec<&str> = folded
        .split(' ')
        .filter(|w| !w.is_empty() && !COMMON_WORDS.contains(w))
        .collect();
    words.sort_unstable();
    words.dedup();
    let found = words.iter().filter(|w| known.contains(w)).count();
    !words.is_empty() && found * 100 >= words.len() * usize::from(share_percent)
}

/// Whether a follow-up joins the open job (INT-010, D-485): strictly inside `window` it joins
/// whatever its trade ([`joins_open_job`]); from the window on (2:00:00 included) it joins only
/// when it reads as the job's trade and [`shares_words`] holds; otherwise it opens a new job.
#[must_use]
pub fn follow_up_joins(
    last: Timestamp,
    now: Timestamp,
    window: SignedDuration,
    same_trade: bool,
    message: &str,
    description: &str,
    share_percent: u8,
) -> bool {
    joins_open_job(last, now, window)
        || (!now.duration_since(last).is_negative()
            && same_trade
            && shares_words(message, description, share_percent))
}

/// The first of `options` (code, phrase) whose phrase appears in `words` as whole words, in the
/// order they appear in the message (INT-011, INT-012): "ok" is not in "broke", and "the AC one"
/// picks the option whose phrase is "ac".
#[must_use]
pub fn pick_option<'a>(words: &str, options: &[(&'a str, &str)]) -> Option<&'a str> {
    let padded = format!(" {} ", fold(words));
    options
        .iter()
        .filter_map(|(code, phrase)| {
            let at = padded.find(&format!(" {} ", fold(phrase)))?;
            Some((at, *code))
        })
        .min_by_key(|(at, _)| *at)
        .map(|(_, code)| code)
}

/// Whether a word answer is accepted for a free-answer question (INT-012): "the AC one", "Yes!",
/// a first name or "my email is ..." all count; only an empty message does not.
#[must_use]
pub fn accepts_word_answer(words: &str) -> bool {
    !fold(words).is_empty()
}

/// The age a sender stated (INT-038).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgeStop {
    /// Under 18, or in school: the request closes and no card goes.
    Under18,
    /// Under 13: as [`AgeStop::Under18`], and the request and its data are deleted too.
    Under13,
}

/// Words that may follow "I'm <n>" or "I am <n>" and still mean the sender's own age
/// ("I'm 15 and ..."). Spanish "y" and "en" are not here: "tengo 2 y uno no enfria" is a request.
const AFTER_AGE: &[&str] = &[
    "years", "year", "yrs", "yr", "yo", "and", "but", "so", "my", "our", "the", "a", "we", "can",
    "need", "have",
];
/// The only words that may follow "tengo <n>" for it to be an age (INT-038, D-996).
const AFTER_TENGO: &[&str] = &["anos", "ano"];
/// Words that must come directly before a school phrase for it to be the sender's own school.
const BEFORE_SCHOOL: &[&[&str]] = &[&["im", "in"], &["i", "am", "in"], &["i", "go", "to"]];
/// School phrases a sender uses about themselves (INT-038), folded.
const SCHOOLS: &[&[&str]] = &[
    &["middle", "school"],
    &["junior", "high"],
    &["high", "school"],
];

/// Whether the sender gives their own age as under 18, or says they are in middle school, junior
/// high or high school (INT-038): "I'm 15", "I am 16 years old", "tengo 15 años". Someone else's
/// age ("my 15-year-old broke the door") is a normal request. An emergency keyword is handled
/// first by the caller (INT-002).
#[must_use]
pub fn age_stop(words: &str) -> Option<AgeStop> {
    let folded = fold(words);
    let t: Vec<&str> = folded.split(' ').collect();
    let first_person = |i: usize| match t[i] {
        "im" => true,
        "i" => t.get(i + 1) == Some(&"am"),
        _ => false,
    };
    for i in 0..t.len() {
        let after = if t[i] == "tengo" {
            Some(i + 1)
        } else if first_person(i) {
            Some(i + if t[i] == "i" { 2 } else { 1 })
        } else {
            None
        };
        let age = after.and_then(|j| Some((t.get(j)?.parse::<u8>().ok()?, j)));
        let list = if t[i] == "tengo" {
            AFTER_TENGO
        } else {
            AFTER_AGE
        };
        if let Some((age, j)) = age
            && age < 18
            && t.get(j + 1).is_none_or(|n| list.contains(n))
        {
            return Some(if age < 13 {
                AgeStop::Under13
            } else {
                AgeStop::Under18
            });
        }
    }
    in_school(&t).then_some(AgeStop::Under18)
}

/// Whether the sender says "I'm in", "I am in" or "I go to" directly before a school phrase.
/// "I live near the high school" is a normal request.
fn in_school(t: &[&str]) -> bool {
    SCHOOLS.iter().any(|school| {
        t.windows(school.len()).enumerate().any(|(at, w)| {
            w == *school
                && BEFORE_SCHOOL
                    .iter()
                    .any(|lead| at >= lead.len() && t[at - lead.len()..at] == **lead)
        })
    })
}

/// Splits the raw name-and-address answer (INT-027): the name is the words before the first one
/// that holds a digit, the street address is the rest from that word. `None` when no word holds a
/// digit (no street number), so the question is asked again. The caller keeps both in the vault
/// only (D-146).
#[must_use]
pub fn split_name_address(raw: &str) -> Option<(String, String)> {
    let words: Vec<&str> = raw.split_whitespace().collect();
    let at = words
        .iter()
        .position(|w| w.chars().any(|c| c.is_ascii_digit()))?;
    let name = words[..at].join(" ");
    let address = words[at..].join(" ");
    (!name.is_empty()).then_some((name, address))
}

/// Which jobs a CLOSE text names (JOB-019, JOB-021).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseTarget {
    /// One request number: "CLOSE 1042", or "1042" alone in answer to "Close which?".
    Number(i64),
    /// Every open job: "CLOSE ALL", "CERRAR TODAS", or "all" alone in answer to "Close which?".
    All,
}

/// How a message reads for closing (JOB-019 to JOB-023).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReading {
    /// CLOSE or CERRAR with a number or ALL, or the answer to "Close which?".
    Close(CloseTarget),
    /// A cancel-intent message ("please cancel my request", "never mind", "ya no"), or CLOSE alone.
    Intent,
    /// Mentions cancel without cancel intent ("I had to cancel the plumber", "cancel" alone).
    Mention,
    /// Nothing about closing.
    Other,
}

/// Phrases that show cancel intent anywhere in a message (JOB-019, D-1200), folded.
const INTENT_PHRASES: &[&str] = &[
    "never mind",
    "nevermind",
    "nvm",
    "forget it",
    "no longer need",
    "no longer needed",
    "dont need it",
    "dont need this",
    "do not need it",
    "please cancel",
    "cancel please",
    "cancel it",
    "cancel this",
    "cancel that",
    "cancel my request",
    "cancel the request",
    "cancel my job",
    "cancel the job",
    "close my request",
    "close the request",
    "close this request",
    "ya no lo necesito",
    "ya no necesito",
    "ya no hace falta",
    "ya no quiero",
    "no lo necesito",
    "olvidalo",
    "olvidelo",
    "no quiero nada",
    "por favor cancele",
    "por favor cancela",
    "cancelar por favor",
    "cancelalo",
    "cancelala",
    "cancelelo",
    "cancela mi solicitud",
    "cancele mi solicitud",
    "cancelar mi solicitud",
    "cancelar la solicitud",
    "cerrar mi solicitud",
];
/// Messages that show cancel intent only as the whole message: "ya no" alone, never "el aire ya
/// no funciona" (JOB-019, D-1200).
const INTENT_WHOLE: &[&str] = &["ya no", "ya no gracias", "close", "cerrar"];
/// Words that mention cancelling (JOB-023), folded.
const CANCEL_MENTIONS: &[&str] = &[
    "cancel",
    "canceled",
    "cancelled",
    "canceling",
    "cancelling",
    "cancellation",
    "cancelar",
    "cancela",
    "cancele",
    "cancelo",
    "cancelacion",
];
/// The words for "all" in a CLOSE text or a "Close which?" answer, folded.
const ALL_WORDS: &[&str] = &["all", "todo", "todos", "todas", "both", "ambos", "ambas"];

/// Reads `words` for closing (JOB-019 to JOB-023, D-1200). `asked` is true while the request's
/// "Close which?" waits for an answer, so a lone number or "all" answers it. "cancel" alone, and a
/// message that only mentions cancelling, is [`CloseReading::Mention`] and never closes (JOB-020).
#[must_use]
pub fn close_reading(words: &str, asked: bool) -> CloseReading {
    let folded = fold(words);
    let tokens: Vec<&str> = folded.split(' ').filter(|t| !t.is_empty()).collect();
    let target = |t: &str| match t.parse::<i64>() {
        Ok(n) if t.bytes().all(|b| b.is_ascii_digit()) => Some(CloseTarget::Number(n)),
        _ => ALL_WORDS.contains(&t).then_some(CloseTarget::All),
    };
    match tokens.as_slice() {
        ["close" | "cerrar", t] | [t] if asked || tokens.len() == 2 => {
            if let Some(target) = target(t) {
                return CloseReading::Close(target);
            }
        }
        _ => {}
    }
    let padded = format!(" {folded} ");
    let phrase = INTENT_PHRASES
        .iter()
        .any(|p| padded.contains(&format!(" {p} ")));
    if phrase || INTENT_WHOLE.contains(&folded.as_str()) {
        CloseReading::Intent
    } else if tokens.iter().any(|t| CANCEL_MENTIONS.contains(t)) {
        CloseReading::Mention
    } else {
        CloseReading::Other
    }
}
