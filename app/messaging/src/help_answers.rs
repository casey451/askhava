//! The plain-word auto-answers to a homeowner's next message after HELP (CON-034, CON-018, T482,
//! D-3753, D-3844, D-3882, D-4344).
//!
//! [`help_question`] reads a message by plain word match, never by AI, against the fixed list of
//! common questions: where the quotes are, how to close a request, whether it costs anything and
//! whether this is a person. Only a short message ([`MAX_WORDS`]) that names exactly one question
//! matches, so "my quote page says expired but the plumber said he quoted" is a help request, not
//! the quotes answer. The close and cost answers are the registry's `close_howto` and
//! `cost_answer`; the quotes and person answers are built here ([`quotes_answer`],
//! [`person_answer`]), English and Spanish, each opening `<short name>:` and ending with the
//! "reply with what's wrong" line, and are queued as [`QUOTES_ANSWER`] and [`PERSON_ANSWER`]
//! (IDs outside the registry, as `pick_lapsed` is). [`answered_within`] reads `messaging.outbox`
//! for any of the four answers sent to her in the last 24 hours, so her next message opens a
//! help request instead (`intake`'s `reply_in` calls all of these). [`AUTO_ANSWERED_ACTION`] is
//! the audit action of the digest line each answer writes.

use askhava_core::api::brand;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx as _;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::OutboxError;
use crate::templates::Language;

/// The code-built "where are my quotes" answer's `template_id` (CON-034, D-3882).
pub const QUOTES_ANSWER: &str = "quotes_answer";
/// The code-built WORD-009 "is this a person" answer's `template_id` (CON-034, D-3882).
pub const PERSON_ANSWER: &str = "person_answer";
/// The registry's "how do I close" answer (CON-034).
pub const CLOSE_ANSWER: &str = "close_howto";
/// The registry's "does this cost anything" answer (CON-034, D-3844).
pub const COST_ANSWER: &str = "cost_answer";
/// Every auto-answer's `template_id`, read by [`answered_within`] (D-3882 (3)).
pub const ANSWER_TEMPLATES: [&str; 4] = [QUOTES_ANSWER, CLOSE_ANSWER, COST_ANSWER, PERSON_ANSWER];
/// The audit action of one auto-answer, the weekly digest's line (CON-034, D-3882 (2)).
pub const AUTO_ANSWERED_ACTION: &str = "help.auto_answered";
/// A message after an auto-answer within this long opens a help request (CON-034, OPS-036).
pub const ANSWER_WINDOW: SignedDuration = SignedDuration::from_hours(24);
/// A message longer than this many words is never a plain-word question (D-4344).
pub const MAX_WORDS: usize = 7;

/// One of CON-034's common questions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpQuestion {
    /// Where the quotes are (quotes, quote, cotizaciones).
    Quotes,
    /// How to close a request (close, cerrar, "don't need it", "ya no").
    Close,
    /// Whether it costs anything (cost, fee, pay, charge, costo, pagar, cobran).
    Cost,
    /// Whether this is a person (WORD-009).
    Person,
}

impl HelpQuestion {
    /// The `template_id` its answer is queued under (D-3882).
    #[must_use]
    pub const fn template_id(self) -> &'static str {
        match self {
            Self::Quotes => QUOTES_ANSWER,
            Self::Close => CLOSE_ANSWER,
            Self::Cost => COST_ANSWER,
            Self::Person => PERSON_ANSWER,
        }
    }
}

/// A matched question and whether the word that matched is Spanish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpAsked {
    /// The question.
    pub question: HelpQuestion,
    /// True when a Spanish word or phrase matched, so the answer goes in Spanish when it may.
    pub spanish: bool,
}

/// (question, folded word or phrase, Spanish) for every word CON-034 and WORD-009 list.
const WORDS: &[(HelpQuestion, &str, bool)] = &[
    (HelpQuestion::Quotes, "quote", false),
    (HelpQuestion::Quotes, "quotes", false),
    (HelpQuestion::Quotes, "cotizacion", true),
    (HelpQuestion::Quotes, "cotizaciones", true),
    (HelpQuestion::Close, "close", false),
    (HelpQuestion::Close, "dont need it", false),
    (HelpQuestion::Close, "cerrar", true),
    (HelpQuestion::Close, "ya no", true),
    (HelpQuestion::Cost, "cost", false),
    (HelpQuestion::Cost, "costs", false),
    (HelpQuestion::Cost, "fee", false),
    (HelpQuestion::Cost, "fees", false),
    (HelpQuestion::Cost, "pay", false),
    (HelpQuestion::Cost, "charge", false),
    (HelpQuestion::Cost, "charges", false),
    (HelpQuestion::Cost, "costo", true),
    (HelpQuestion::Cost, "pagar", true),
    (HelpQuestion::Cost, "cobran", true),
    (HelpQuestion::Person, "person", false),
    (HelpQuestion::Person, "human", false),
    (HelpQuestion::Person, "bot", false),
    (HelpQuestion::Person, "robot", false),
    (HelpQuestion::Person, "persona", true),
    (HelpQuestion::Person, "humano", true),
];

/// `words` lower-cased with Spanish accents dropped, apostrophes removed and every other mark a
/// space, padded with one space each side so a phrase matches only whole words.
fn fold(words: &str) -> String {
    let mut out = String::from(" ");
    for c in words.to_lowercase().chars() {
        match c {
            'á' => out.push('a'),
            'é' => out.push('e'),
            'í' => out.push('i'),
            'ó' => out.push('o'),
            'ú' | 'ü' => out.push('u'),
            'ñ' => out.push('n'),
            '\'' | '\u{2019}' => {}
            c if c.is_alphanumeric() => out.push(c),
            _ => out.push(' '),
        }
    }
    let joined = out.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(" {joined} ")
}

/// CON-034, WORD-009, D-4344: the one common question a message of at most [`MAX_WORDS`] words
/// asks, by plain word match (never AI); `None` for a longer message, one naming no question or
/// one naming two (each goes to a help request).
#[must_use]
pub fn help_question(words: &str) -> Option<HelpAsked> {
    let folded = fold(words);
    if folded.split_whitespace().count() > MAX_WORDS {
        return None;
    }
    let mut found: Option<HelpAsked> = None;
    for &(question, word, spanish) in WORDS {
        if !folded.contains(&format!(" {word} ")) {
            continue;
        }
        match found {
            Some(seen) if seen.question != question => return None,
            Some(seen) => {
                found = Some(HelpAsked {
                    spanish: seen.spanish || spanish,
                    ..seen
                })
            }
            None => found = Some(HelpAsked { question, spanish }),
        }
    }
    found
}

/// The short name every text opens with, and the display name, in `language` (WORD-008).
fn names(language: Language) -> (&'static str, &'static str) {
    match language {
        Language::En => (brand().short_name(), brand().display_name()),
        Language::Es => (brand().es().short_name(), brand().es().display_name()),
    }
}

/// CON-034's last line: "If that doesn't fix it, reply with what's wrong ..." in `language`.
fn fix_line(language: Language) -> String {
    let (_, name) = names(language);
    match language {
        Language::En => format!(
            "If that doesn't fix it, reply with what's wrong and the {name} owner gets it right away."
        ),
        Language::Es => format!(
            "Si eso no lo resuelve, responda con lo que pasa y el dueño de {name} lo atiende de inmediato."
        ),
    }
}

/// (status codes, English words, Spanish words) for each status a homeowner's job can show;
/// any other code reads as the open job's words, [`OPEN_WORDS`].
const STATUS_WORDS: &[(&[&str], &str, &str)] = &[
    (
        &["intake"],
        "we're still getting your request ready",
        "todavía estamos preparando su solicitud",
    ),
    (
        &["quotes_ready"],
        "your quotes are ready",
        "sus cotizaciones están listas",
    ),
    (
        &["awaiting_confirm", "payment_pending"],
        "waiting for the business you picked to confirm",
        "esperando que el negocio que eligió confirme",
    ),
    (
        &["connected", "done"],
        "you're connected with the business you picked",
        "ya está en contacto con el negocio que eligió",
    ),
    (
        &["closed", "closed_early", "cancelled"],
        "this request is closed",
        "esta solicitud está cerrada",
    ),
    (
        &["not_covered"],
        "no local business covers this job yet",
        "ningún negocio local cubre este trabajo todavía",
    ),
];
/// The words of an open job with no quotes yet (`open`, `no_quotes_yet`).
const OPEN_WORDS: (&str, &str) = (
    "local businesses have it, no quotes yet",
    "los negocios locales la tienen, todavía sin cotizaciones",
);

/// A job status code (`jobs::api::JobStatus::code`) in plain words in `language`.
fn status_words(status: &str, language: Language) -> &'static str {
    let (en, es) = STATUS_WORDS
        .iter()
        .find(|(codes, _, _)| codes.contains(&status))
        .map_or(OPEN_WORDS, |&(_, en, es)| (en, es));
    match language {
        Language::En => en,
        Language::Es => es,
    }
}

/// CON-034, D-3882: the "where are my quotes" answer for job `number` with status code `status`:
/// the status, the LIST line and her quote-page `link`, ending with the fix line.
#[must_use]
pub fn quotes_answer(number: i64, status: &str, link: &str, language: Language) -> String {
    let (b, said, fix) = (
        names(language).0,
        status_words(status, language),
        fix_line(language),
    );
    match language {
        Language::En => format!(
            "{b}: Request {number}: {said}. Reply LIST to get your quotes by text, or see them at: {link}\n{fix}"
        ),
        Language::Es => format!(
            "{b}: Solicitud {number}: {said}. Responda LISTA para recibir sus cotizaciones por texto, o véalas en: {link}\n{fix}"
        ),
    }
}

/// CON-034, WORD-009, D-3882: the "is this a person" answer, never a claim to be a person.
#[must_use]
pub fn person_answer(language: Language) -> String {
    let ((b, name), fix) = (names(language), fix_line(language));
    match language {
        Language::En => format!("{b}: {name} is a local service. Our texts are automated. {fix}"),
        Language::Es => {
            format!("{b}: {name} es un servicio local. Nuestros textos son automatizados. {fix}")
        }
    }
}

/// CON-034, D-3882 (3): whether an auto-answer was queued to `contact` within [`ANSWER_WINDOW`]
/// before `at`, read from `messaging.outbox` by [`ANSWER_TEMPLATES`].
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn answered_within(
    db: &mut PgConnection,
    contact: Uuid,
    at: Timestamp,
) -> Result<bool, OutboxError> {
    let since = at - ANSWER_WINDOW;
    let ids: Vec<String> = ANSWER_TEMPLATES.iter().map(|t| (*t).to_owned()).collect();
    sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM messaging.outbox WHERE recipient_id = $1 \
         AND template_id = ANY($2) AND created_at >= $3 AND created_at <= $4) AS \"answered!\"",
        contact,
        &ids,
        since.to_sqlx() as _,
        at.to_sqlx() as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| OutboxError::Database)
}
