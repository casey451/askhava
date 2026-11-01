//! The request's question set, one question at a time (INT-039, CARD-007, AI-019; D-394, D-430,
//! D-431, D-436, D-492).
//!
//! [`BankRun`] walks one `docs/catalog/question_bank.yaml` set in its order (step 8 of the INT-043
//! table): it applies each `ask_if` on an earlier answer, never asks more than [`MAX_ASKED`], adds
//! "Not sure" ("No sé") to every numbered list and stores each answer for the card
//! ([`BankRun::card_answers`]). With AI off it asks the fixed template and skips a question only
//! when exactly one option's words (two or more) appear in her request ([`keyword_answer`]). With
//! AI on (AI-008) the reworded text passes `ai::api::accept_question` or the template is asked, the
//! options stay the bank's, one follow-up is allowed only from the set's `kb_trade`, and a
//! question is skipped only when the reading's confidence reaches [`BankSettings::skip_threshold`],
//! which is unset until a recorded real-model run (AI-027) sets it, so nothing is skipped. A
//! numbered answer naming a smell of no stated kind is followed by [`SMELL_EN`]; its answers 1 and 2
//! give [`Heard::Safety911`], the caller's INT-004 text and hold; a smell answer and the smell
//! list are never skipped, so that check always runs. Nothing here sends or stores.

use askhava_ai::api::{WriteSources, accept_question};
use askhava_catalog::api::{Question, QuestionSet};

use crate::rules::fold;

/// The most questions one request is asked, follow-ups included (INT-039).
pub const MAX_ASKED: usize = 8;
/// The answer added to every numbered list, English and Spanish (INT-039).
pub const NOT_SURE: [&str; 2] = ["Not sure", "No sé"];
/// The smell follow-up, word for word (INT-039, D-436), English then Spanish.
pub const SMELL_EN: &str = "What kind of smell?";
/// The Spanish smell follow-up (the bank's `smell_kind` draft).
pub const SMELL_ES: &str = "¿Qué tipo de olor?";
const SMELL_OPTIONS: [[&str; 3]; 2] = [
    ["Gas or rotten egg", "Burning", "Something else"],
    ["Gas o huevo podrido", "Quemado", "Otra cosa"],
];
const SMELL_ID: &str = "smell_kind";
const FOLLOW_UP_ID: &str = "follow_up";

/// The two settings the run reads (INT-039, AI-008).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BankSettings {
    /// Whether "AI on scrubbed text" is on (AI-008).
    pub ai_on: bool,
    /// The INT-039 skip threshold; `None` until a recorded real-model run sets it (AI-027, D-492).
    pub skip_threshold: Option<f64>,
}

/// What the AI offered for the next question while AI is on (AI-019); all empty with AI off.
#[derive(Debug, Clone, Copy, Default)]
pub struct AiTurn<'a> {
    /// Its rewording of the next question's text.
    pub reword: Option<&'a str>,
    /// Its reading of one question's answer: (question ID, answer number, confidence 0 to 1).
    pub reading: Option<(&'a str, u32, f64)>,
    /// One follow-up question and the `kb_trade` brief it was drawn from.
    pub follow_up: Option<(&'a str, &'a str)>,
}

/// A stored answer (INT-012, CARD-007): an option number, "Not sure", or free words stored as
/// the code `text` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Given {
    /// A numbered option, 1-based.
    Option(u32),
    /// "Not sure" or "No sé".
    NotSure,
    /// A free-text answer; the words stay in the inbox, never here.
    Text,
}

/// One question shown to the homeowner (INT-039).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The bank question ID, `smell_kind` or `follow_up`.
    pub id: String,
    /// The question text: the template, or the AI's when it passed AI-019.
    pub text: String,
    /// The numbered answers, always the bank's (or the smell list), "Not sure" last on bank lists.
    pub options: Vec<String>,
    /// True when `text` is the AI's wording.
    pub reworded: bool,
}

impl Prompt {
    /// The text as sent: "Ask Hava: <question> 1) <option> 2) <option>".
    #[must_use]
    pub fn render(&self) -> String {
        let numbered = self.options.iter().enumerate();
        numbered.fold(format!("Ask Hava: {}", self.text), |s, (i, o)| {
            format!("{s} {}) {o}", i + 1)
        })
    }
}

/// The next step of the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Send this question and wait for the reply.
    Ask(Prompt),
    /// The set is finished (or [`MAX_ASKED`] reached); photos come next (INT-044).
    Done,
}

/// What a reply did (INT-039).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heard {
    /// Stored; ask [`BankRun::next`].
    Stored(Given),
    /// Unusable on a numbered question; ask the same prompt again.
    Again,
    /// Smell answer 1 or 2: send the INT-004 text and hold the card until SAFE (INT-005).
    Safety911,
}

/// One request's walk through its set (INT-039).
#[derive(Debug, Clone)]
pub struct BankRun<'a> {
    set: &'a QuestionSet,
    spanish: bool,
    emergency: bool,
    settings: BankSettings,
    next_index: usize,
    asked: usize,
    followed_up: bool,
    smell_due: bool,
    pending: Option<Prompt>,
    answers: Vec<(String, Given)>,
}

/// Whether the questions may start: a job chosen as an emergency is asked only after its cards
/// went out (INT-029, INT-039).
#[must_use]
pub fn starts_now(emergency: bool, cards_out: bool) -> bool {
    !emergency || cards_out
}

/// INT-039: the answer her own words give when exactly one option of two or more words appears
/// in them as whole words; the AI-off skip rule.
#[must_use]
pub fn keyword_answer(options: &[String], words: &str) -> Option<u32> {
    let padded = format!(" {} ", fold(words));
    let hits: Vec<usize> = (0..options.len())
        .filter(|&i| {
            let o = fold(&options[i]);
            o.contains(' ') && padded.contains(&format!(" {o} "))
        })
        .collect();
    match hits.as_slice() {
        [one] => u32::try_from(*one + 1).ok(),
        _ => None,
    }
}

/// INT-039, D-436: a numbered answer that names a smell without naming its kind ("Strange noise
/// or smell"); "Fuel smell" names its kind and gets nothing extra (TD-INT-24).
#[must_use]
pub fn asks_smell_kind(option_en: &str) -> bool {
    let folded = fold(option_en);
    let words: Vec<&str> = folded.split(' ').collect();
    words.iter().position(|w| *w == "smell").is_some_and(|at| {
        at == 0 || matches!(words[at - 1], "or" | "a" | "strange" | "odd" | "weird")
    })
}

fn is_smell(q: &Question) -> bool {
    q.options_en
        .as_ref()
        .is_some_and(|o| o.iter().map(String::as_str).eq(SMELL_OPTIONS[0]))
}

impl<'a> BankRun<'a> {
    /// A run over `set` in the conversation's language; on an emergency job only the questions
    /// marked `emergency` are asked (INT-039).
    #[must_use]
    pub fn new(
        set: &'a QuestionSet,
        spanish: bool,
        emergency: bool,
        settings: BankSettings,
    ) -> Self {
        Self {
            set,
            spanish,
            emergency,
            settings,
            next_index: 0,
            asked: 0,
            followed_up: false,
            smell_due: false,
            pending: None,
            answers: Vec::new(),
        }
    }

    /// The stored answers, by question ID, in the order given or skipped (CARD-007).
    #[must_use]
    pub fn answers(&self) -> &[(String, Given)] {
        &self.answers
    }

    /// The next question for her request `words` (INT-039, AI-019). A question her words answer
    /// is stored and passed over: by [`keyword_answer`] with AI off, by the reading at or above a
    /// set skip threshold with AI on.
    pub fn next(&mut self, words: &str, ai: &AiTurn<'_>, sources: &WriteSources<'_>) -> Next {
        if self.asked >= MAX_ASKED {
            return Next::Done;
        }
        if let Some(p) = self.smell_prompt().or_else(|| self.follow_up(ai, sources)) {
            return self.ask(p);
        }
        while let Some(q) = self.set.questions.get(self.next_index) {
            self.next_index += 1;
            if !self.applies(q) {
                continue;
            }
            if let Some(n) = self.skip_answer(q, words, ai) {
                self.answers.push((q.id.clone(), Given::Option(n)));
                continue;
            }
            let prompt = self.bank_prompt(q, ai, sources);
            return self.ask(prompt);
        }
        Next::Done
    }

    fn ask(&mut self, prompt: Prompt) -> Next {
        self.asked += 1;
        self.pending = Some(prompt.clone());
        Next::Ask(prompt)
    }

    /// `ask_if` holds, the emergency flag allows it, and a smell list already asked is not repeated.
    fn applies(&self, q: &Question) -> bool {
        let shown = q.ask_if.as_ref().is_none_or(|c| {
            self.answers.iter().any(|(id, g)| {
                *id == c.question && matches!(g, Given::Option(n) if c.answers.contains(n))
            })
        });
        let smell_done = is_smell(q) && self.answers.iter().any(|(id, _)| id == SMELL_ID);
        shown && (!self.emergency || q.emergency) && !smell_done
    }

    /// The answer a skip would store, never for the smell list or an answer that asks the smell
    /// question: those are always asked, so `hear` runs the INT-004 check on them (INT-039).
    fn skip_answer(&self, q: &Question, words: &str, ai: &AiTurn<'_>) -> Option<u32> {
        let n = self.skip_reading(q, words, ai)?;
        let english = q.options_en.as_ref();
        let smelly = english
            .and_then(|o| o.get(n as usize - 1))
            .is_some_and(|o| asks_smell_kind(o));
        (!is_smell(q) && !smelly).then_some(n)
    }

    fn skip_reading(&self, q: &Question, words: &str, ai: &AiTurn<'_>) -> Option<u32> {
        let options = self.options_of(q)?;
        if !self.settings.ai_on {
            return keyword_answer(options, words);
        }
        let (_, n, confidence) = ai.reading.filter(|r| r.0 == q.id)?;
        let threshold = self.settings.skip_threshold?;
        let valid = n >= 1 && n as usize <= options.len();
        (valid && confidence >= threshold).then_some(n)
    }

    fn options_of(&self, q: &'a Question) -> Option<&'a Vec<String>> {
        if self.spanish {
            q.options_es.as_ref()
        } else {
            q.options_en.as_ref()
        }
    }

    fn bank_prompt(&self, q: &Question, ai: &AiTurn<'_>, sources: &WriteSources<'_>) -> Prompt {
        let template = if self.spanish { &q.es } else { &q.en };
        let (text, reworded) = match ai.reword.filter(|_| self.settings.ai_on) {
            Some(own) => {
                let written = accept_question(own, template, sources);
                (written.text().to_owned(), written.hit().is_none())
            }
            None => (template.clone(), false),
        };
        let mut options = self.options_of(q).cloned().unwrap_or_default();
        if !options.is_empty() && !is_smell(q) {
            options.push(NOT_SURE[usize::from(self.spanish)].to_owned());
        }
        Prompt {
            id: q.id.clone(),
            text,
            options,
            reworded,
        }
    }

    fn smell_prompt(&mut self) -> Option<Prompt> {
        if !std::mem::take(&mut self.smell_due) {
            return None;
        }
        let lang = usize::from(self.spanish);
        Some(Prompt {
            id: SMELL_ID.to_owned(),
            text: [SMELL_EN, SMELL_ES][lang].to_owned(),
            options: SMELL_OPTIONS[lang]
                .iter()
                .map(|o| (*o).to_owned())
                .collect(),
            reworded: false,
        })
    }

    /// AI-019, D-431: one follow-up, AI on, drawn from this set's `kb_trade`, passing every check.
    fn follow_up(&mut self, ai: &AiTurn<'_>, sources: &WriteSources<'_>) -> Option<Prompt> {
        let (text, brief) = ai.follow_up?;
        let allowed = self.settings.ai_on && !self.followed_up && self.asked > 0;
        if !allowed || self.set.kb_trade.as_deref() != Some(brief) {
            return None;
        }
        let written = accept_question(text, "", sources);
        written.hit().is_none().then(|| {
            self.followed_up = true;
            Prompt {
                id: FOLLOW_UP_ID.to_owned(),
                text: written.text().to_owned(),
                options: Vec::new(),
                reworded: true,
            }
        })
    }

    /// Reads her reply to the pending prompt (INT-011, INT-012, INT-039): a lone number, one
    /// option's words or "Not sure"/"No sé"; any words for a free-text question.
    pub fn hear(&mut self, reply: &str) -> Heard {
        let Some(prompt) = self.pending.clone() else {
            return Heard::Again;
        };
        let given = if prompt.options.is_empty() {
            (!fold(reply).is_empty()).then_some(Given::Text)
        } else {
            read_choice(&prompt.options, reply)
        };
        let Some(given) = given else {
            return Heard::Again;
        };
        self.pending = None;
        self.answers.push((prompt.id.clone(), given));
        self.after(&prompt, given)
    }

    fn after(&mut self, prompt: &Prompt, given: Given) -> Heard {
        let smell_list = prompt.id == SMELL_ID
            || self
                .set
                .questions
                .iter()
                .any(|q| q.id == prompt.id && is_smell(q));
        if smell_list && matches!(given, Given::Option(1 | 2)) {
            return Heard::Safety911;
        }
        let english = self.set.questions.iter().find(|q| q.id == prompt.id);
        let option_en = english
            .and_then(|q| q.options_en.as_ref())
            .zip(match given {
                Given::Option(n) => Some(n as usize),
                _ => None,
            });
        if let Some((options, n)) = option_en {
            let has_list = self.set.questions.iter().any(is_smell);
            let asked = self.answers.iter().any(|(id, _)| id == SMELL_ID);
            self.smell_due =
                !has_list && !asked && options.get(n - 1).is_some_and(|o| asks_smell_kind(o));
        }
        Heard::Stored(given)
    }

    /// CARD-007: the card-field answers as (question template, chosen option) in the
    /// conversation's language, skipped ones included; free text never.
    #[must_use]
    pub fn card_answers(&self) -> Vec<(String, String)> {
        let lang = usize::from(self.spanish);
        self.answers
            .iter()
            .filter_map(|(id, given)| {
                let q = self
                    .set
                    .questions
                    .iter()
                    .find(|q| q.id == *id && q.card_field)?;
                let label = if self.spanish { &q.es } else { &q.en };
                let answer = match given {
                    Given::Option(n) => self.options_of(q)?.get(*n as usize - 1)?.clone(),
                    Given::NotSure => NOT_SURE[lang].to_owned(),
                    Given::Text => return None,
                };
                Some((label.clone(), answer))
            })
            .collect()
    }
}

/// A lone option number (the last one is "Not sure" on a bank list), the whole words of exactly
/// one option, or "Not sure"/"No sé" (INT-011, INT-039).
fn read_choice(options: &[String], reply: &str) -> Option<Given> {
    let folded = fold(reply);
    let count = options.len();
    if let Ok(n) = folded.parse::<usize>() {
        let option = n.checked_sub(1).and_then(|i| options.get(i))?;
        return Some(if is_not_sure(option) {
            Given::NotSure
        } else {
            Given::Option(u32::try_from(n).ok()?)
        });
    }
    if is_not_sure(&folded) || matches!(folded.as_str(), "i dont know" | "dont know") {
        return Some(Given::NotSure);
    }
    let padded = format!(" {folded} ");
    let named: Vec<usize> = (0..count)
        .filter(|&i| {
            !is_not_sure(&options[i]) && padded.contains(&format!(" {} ", fold(&options[i])))
        })
        .collect();
    match named.as_slice() {
        [one] => u32::try_from(*one + 1).ok().map(Given::Option),
        _ => None,
    }
}

fn is_not_sure(text: &str) -> bool {
    NOT_SURE.iter().any(|n| fold(n) == fold(text))
}
