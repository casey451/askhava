//! INT-043's one intake order for the four text paths, and the steps of it that write the card's
//! facts (T162; INT-043, TXT-041, INT-041, INT-029, PRIV-008, CARD-007; D-446, D-460, D-2330,
//! D-3189).
//!
//! [`order`] is the "Intake order" table of `docs/spec/intake.md` as data: regular, urgent, 911
//! list and night (web chat is T103's, D-491). `conversation::after_questions` reads its steps 9
//! to 11 from here. Step 4 is T251's `zip_step` (INT-049, D-1521).
//!
//! The database steps: [`keep_words`] stores each describing text's words through
//! `quotes::api::keep_description` after scrubbing the already-scrubbed words again (it can only
//! remove more, D-3118); [`answer_narrowing`] stores a trade answer and, in its transaction, the
//! narrowing facts row (a refused row is logged with IDs only and changes nothing, D-2330);
//! [`show_built_card`] builds the step-12 card from her stored words, stores it with `keep_card`
//! and shows it in one transaction; [`choose_with_card`] stores the emergency path's card before
//! `answer_choice` can release the job (INT-029); [`spanish_line`] sends TXT-041's line once;
//! [`ask_last_question`] asks step 11, the name and address, once per request (INT-027, D-3702).
//! No raw words are stored and none go into a log line.

use askhava_jobs::api::{FactsError, Recorded as Facts, record_job_facts};
use askhava_messaging::api::Language;
use askhava_privacy::api::{CommonWords, Known, NameMemory, scrub};
use askhava_quotes::api::{
    CardFacts, CardParts, DEFAULT_MIN_WORDS, JobCard, build_card, description, keep_card,
    keep_description,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::conversation::{
    ADDRESS_CONTACT, Answer, NAME_ADDRESS_ASKED, Recorded, ask_name_address, queue_text,
    record_answer,
};
use crate::emergency::{Choice, choice};
use crate::models::{self, IntakeError};
use crate::rules::{Settings, wanted_language};
use crate::service::{Answered, Queued, TextIn, answer_choice};
use crate::summary_step::{Preview, Shown, card_digest, show_card};

/// TXT-041's fixed Spanish line, "Por ahora seguimos en inglés" (its Spanish stays on, D-492).
pub const SPANISH_NOT_YET_TEMPLATE: &str = "spanish_not_yet";

/// A text path of INT-043's "Intake order" table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    /// No 911-list match, "How soon?" not Today.
    Regular,
    /// "Today, it's urgent" (INT-030 answer 1).
    Urgent,
    /// A 911-list match (INT-002).
    List911,
    /// 8 p.m. to 8 a.m. Arizona, with the night count N of INT-033.
    Night(u32),
}

/// One step of INT-043's order; [`Step::cites`] names the row holding its exact text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// 1: the first text in.
    FirstText,
    /// 2: the 911 check runs first, no match.
    SafetyCheck,
    /// 2: a match, the INT-004 text.
    SafetyText,
    /// 3: the first reply.
    FirstReply,
    /// 4 on the 911 path: SAFE first.
    Safe,
    /// 4: the ZIP and area check.
    Zip,
    /// 5: the trade check and coverage line.
    TradeCheck,
    /// 6: "How soon?".
    HowSoon,
    /// 7 at night: the night count first.
    NightCount,
    /// 7: emergency or regular.
    Choice,
    /// 8: the trade questions.
    TradeQuestions,
    /// 9: photos.
    Photos,
    /// 10: in design categories, look pictures and the concept image.
    Design,
    /// 11: first name and address.
    NameAddress,
    /// 12: the card shown, EDIT or SEND, the auto-send.
    CardShown,
    /// 13: cards out.
    CardsOut,
}

impl Step {
    /// The requirement the table cites for this step's text (INT-043).
    #[must_use]
    pub const fn cites(self) -> &'static str {
        match self {
            Self::FirstText => "CON-005",
            Self::SafetyCheck => "INT-002",
            Self::SafetyText => "INT-004",
            Self::FirstReply => "CON-031",
            Self::Safe => "INT-025",
            Self::Zip => "INT-022",
            Self::TradeCheck => "INT-016",
            Self::HowSoon => "INT-030",
            Self::NightCount => "INT-033",
            Self::Choice => "INT-006",
            Self::TradeQuestions => "INT-039",
            Self::Photos => "INT-044",
            Self::Design => "INT-045",
            Self::NameAddress => "INT-027",
            Self::CardShown => "INT-041",
            Self::CardsOut => "TXT-032",
        }
    }
}

/// INT-043, D-446, D-460: the steps of `path` in the table's order. `emergency` is her Emergency
/// choice; it counts only where the choice is offered (urgent, 911 list, night with N above 0),
/// and then the cards go at once and steps 8 to 11 follow them (INT-029). Step 10 applies only in
/// a design category (`conversation::after_questions`).
#[must_use]
pub fn order(path: Path, emergency: bool) -> Vec<Step> {
    let list = path == Path::List911;
    let mut steps = vec![Step::FirstText];
    steps.push(if list {
        Step::SafetyText
    } else {
        Step::SafetyCheck
    });
    steps.push(Step::FirstReply);
    if list {
        steps.push(Step::Safe);
    }
    steps.extend([Step::Zip, Step::TradeCheck, Step::HowSoon]);
    if let Path::Night(count) = path {
        steps.push(Step::NightCount);
        if count > 0 {
            steps.push(Step::Choice);
        }
    } else if path != Path::Regular {
        steps.push(Step::Choice);
    }
    let after = [
        Step::TradeQuestions,
        Step::Photos,
        Step::Design,
        Step::NameAddress,
    ];
    if emergency && steps.contains(&Step::Choice) {
        steps.push(Step::CardsOut);
        steps.extend(after);
    } else {
        steps.extend(after);
        steps.extend([Step::CardShown, Step::CardsOut]);
    }
    steps
}

/// PRIV-008, INT-041, D-3189, D-3118: stores the words of inbound `message` (a text describing
/// `job` of `request`: the opening text or an EDIT's reply) through `keep_description`, scrubbed
/// once more. `true` when stored, `false` for a repeat.
///
/// # Errors
///
/// [`IntakeError::Quotes`].
pub async fn keep_words(
    db: &mut PgConnection,
    (request, job): (Uuid, Uuid),
    message: Uuid,
    (words, at): (&str, Timestamp),
) -> Result<bool, IntakeError> {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let scrubbed = scrub(words, &known, &mut NameMemory::default());
    keep_description(db, (request, job), message, &scrubbed, at)
        .await
        .map_err(|_| IntakeError::Quotes)
}

/// INT-029, INT-039, D-2330: stores a trade-question answer and, when it is stored, `job`'s
/// narrowing facts row (`category` and the remaining `candidates`) in the same transaction. A
/// row that does not narrow is refused by `jobs`, logged with the job ID only, and changes
/// nothing; the answer stays.
///
/// # Errors
///
/// [`IntakeError::Job`] for an unknown job or [`IntakeError::Database`]; nothing is kept.
pub async fn answer_narrowing(
    db: &mut PgConnection,
    answer: &Answer<'_>,
    (job, category, candidates): (Uuid, &str, &[String]),
) -> Result<Recorded, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let recorded = record_answer(&mut tx, answer).await?;
    if matches!(recorded, Recorded::Stored(_)) {
        narrow(&mut tx, job, (category, candidates)).await?;
    }
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(recorded)
}

/// D-2330: one narrowing row through `record_job_facts`; `true` when written.
async fn narrow(
    tx: &mut PgConnection,
    job: Uuid,
    (category, candidates): (&str, &[String]),
) -> Result<bool, IntakeError> {
    match record_job_facts(tx, job, category, candidates).await {
        Ok(done) => Ok(done == Facts::Written),
        Err(FactsError::NotFound) => Err(IntakeError::Job),
        Err(FactsError::Database) => Err(IntakeError::Database),
        Err(FactsError::Empty | FactsError::OtherCategory | FactsError::NotASubset) => {
            tracing::warn!(%job, "intake: a job facts row that does not narrow was refused");
            Ok(false)
        }
    }
}

/// The card of one job (INT-041, INT-029).
#[derive(Debug, Clone)]
pub struct CardIn {
    /// The request.
    pub conversation: Uuid,
    /// The job.
    pub job: Uuid,
    /// The card's parts from the stored answers; `description` is replaced by her stored words.
    pub parts: CardParts,
    /// The catalog category code.
    pub category: String,
    /// The remaining candidate job type codes.
    pub candidates: Vec<String>,
}

/// CARD-007, D-3189: builds the card from `quotes::api::description` and stores it with its digest
/// and facts in the caller's transaction.
async fn build_and_keep(
    tx: &mut PgConnection,
    card: &CardIn,
    emergency: bool,
    at: Timestamp,
) -> Result<JobCard, IntakeError> {
    let quotes = |_| IntakeError::Quotes;
    let words = description(tx, card.job).await.map_err(quotes)?;
    let mut parts = card.parts.clone();
    parts.description = words.map(|w| w.text()).unwrap_or_default();
    let built = build_card(&parts, DEFAULT_MIN_WORDS);
    let facts = CardFacts {
        category: card.category.clone(),
        candidates: card.candidates.clone(),
        emergency,
    };
    let ids = (card.conversation, card.job);
    let digest = card_digest(built.text());
    keep_card(tx, ids, &digest, &built, &facts, at)
        .await
        .map_err(quotes)?;
    Ok(built)
}

/// INT-027, INT-029, INT-043, D-238, D-437, D-3702: step 11, just before step 12's
/// [`show_built_card`]. Asks `ask_name_address` once per request, under the homeowner lock: only
/// in an open request with its consent record, no 911 text waiting for SAFE, this request's area
/// check done (a property stored since it opened), and no earlier ask or answer. `None` when
/// nothing is asked.
///
/// # Errors
///
/// `ask_name_address`'s errors or [`IntakeError::Database`]; nothing is kept on an error.
pub async fn ask_last_question(
    db: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    spanish_on: bool,
) -> Result<Option<Queued>, IntakeError> {
    let Some((homeowner, true)) = models::conversation_state(db, conversation).await? else {
        return Ok(None);
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    models::lock_homeowner(&mut tx, homeowner).await?;
    let state = models::release_state(&mut tx, conversation).await?;
    if !state.consented || state.waiting_safe || !due_to_ask(&mut tx, conversation).await? {
        return Ok(None);
    }
    let asked = ask_name_address(&mut tx, secret, msg, conversation, spanish_on).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(asked)
}

/// Whether request `conversation` reached step 11 and was never asked it: a property of its
/// homeowner stored since the request opened (the area check, INT-049), and no
/// `name_address_asked` or `address_contact` row (INT-027, D-3702).
async fn due_to_ask(db: &mut PgConnection, conversation: Uuid) -> Result<bool, IntakeError> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM intake.property p WHERE p.homeowner_id = c.homeowner_id
               AND p.created_at >= c.opened_at)
             AND NOT EXISTS (SELECT 1 FROM intake.intake_answer a WHERE a.conversation_id = c.id
               AND a.question_id IN ($2, $3)) AS "due!"
           FROM intake.conversation c WHERE c.id = $1"#,
        conversation,
        NAME_ADDRESS_ASKED,
        ADDRESS_CONTACT
    )
    .fetch_optional(db)
    .await
    .map(|due| due.unwrap_or(false))
    .map_err(|_| IntakeError::Database)
}

/// INT-041, CARD-007, D-3189: step 12. Builds and stores the card, then `show_card` shows it, in
/// one transaction, so every shown digest has a stored card. When nothing is texted (an emergency
/// job, no consent record, a card already approved) nothing is stored.
///
/// # Errors
///
/// [`IntakeError::Quotes`] or `show_card`'s errors; nothing is kept on an error.
pub async fn show_built_card(
    db: &mut PgConnection,
    secret: &SecretString,
    card: &CardIn,
    (link, wait, at): (&str, SignedDuration, Timestamp),
) -> Result<Shown, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let built = build_and_keep(&mut tx, card, false, at).await?;
    let preview = Preview {
        conversation: card.conversation,
        job: card.job,
        card: &built,
        link,
        wait,
        at,
    };
    let shown = show_card(&mut tx, secret, &preview).await?;
    if matches!(shown, Shown::Texted(_)) {
        tx.commit().await.map_err(|_| IntakeError::Database)?;
    }
    Ok(shown)
}

/// INT-029, INT-006, D-3189: the reply to the emergency-or-regular question. When it is a choice,
/// the card is built and stored first (the emergency mark only on the Emergency choice), then
/// `answer_choice` stores it and may release the job, all in one transaction.
///
/// # Errors
///
/// [`IntakeError::Quotes`] or `answer_choice`'s errors; nothing is kept on an error.
pub async fn choose_with_card(
    db: &mut PgConnection,
    msg: &TextIn<'_>,
    card: &CardIn,
) -> Result<Answered, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    if let Some(chosen) = choice(msg.inbound.words) {
        let emergency = chosen == Choice::Emergency;
        build_and_keep(&mut tx, card, emergency, msg.received_at).await?;
    }
    let answered = answer_choice(&mut tx, msg, card.conversation).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(answered)
}

/// TXT-041, TXT-039, D-492: while Spanish conversations are off, the request's first message
/// read as Spanish gets the one `spanish_not_yet` text after the first reply (whose sending opened
/// the request); once per request (the outbox key, TXT-011). `None` for any other message.
///
/// # Errors
///
/// [`IntakeError::Template`], [`IntakeError::Outbox`] or [`IntakeError::Database`].
pub async fn spanish_line(
    db: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    settings: Settings,
) -> Result<Option<Queued>, IntakeError> {
    let spanish = wanted_language(msg.inbound.words, msg.inbound.ai_spanish) == Language::Es;
    if settings.spanish_conversations_on || !spanish {
        return Ok(None);
    }
    let first = models::conversation(db, conversation).await?;
    if first.is_none_or(|c| c.first_inbox_id != msg.inbox_id) {
        return Ok(None);
    }
    let (contact, _) = models::recipient(db, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    let template = (SPANISH_NOT_YET_TEMPLATE, Language::Es);
    let queued = queue_text(db, secret, contact, template, &[conversation], false).await?;
    Ok(Some(queued))
}
