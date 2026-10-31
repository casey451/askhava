//! The keyword second opinion, the numbered trade question and its word answers (AI-011, INT-012,
//! INT-013, INT-015, CARD-001, INT-032; D-102 to D-105).
//!
//! Each request is read twice by the AI; [`keyword_reading`] is the independent second opinion on
//! trade and job type (AI-011). [`second_opinion`] settles the trade or builds a [`TradeQuestion`]
//! offering every distinct trade of the two readings and the keywords, up to three (D-104). Its
//! answers match numbers and whole words only ("repair" never matches heating/AC, and generic
//! label words such as "repair" name no option, D-103); a second unusable answer applies the
//! fallback (INT-015). The longest phrase wins, so "water heater" is plumbing (R-37) and "spa
//! heater" pool repair, never the furnace rule's "heater". After a trade is chosen, [`candidates`] keeps
//! that trade's keyword job types (D-102) and every license-boundary keyword job type (D-105), so
//! CARD-001's license check sees the job in the homeowner's own words. [`two_trades`] and
//! [`starts_second`] are INT-032's two-job detection and its ALSO / TAMBIÉN reply.

use crate::rules::fold;

/// One keyword rule: a catalog category ID, the catalog job type it names (`None` for a word that
/// names only the trade), its whole-word phrases (folded), and whether the job type sits behind a
/// license (D-105: such job types stay candidates whatever trade is read or answered).
struct Rule {
    trade: &'static str,
    job_type: Option<&'static str>,
    words: &'static [&'static str],
    boundary: bool,
}

const fn rule(
    trade: &'static str,
    job_type: Option<&'static str>,
    words: &'static [&'static str],
    boundary: bool,
) -> Rule {
    Rule {
        trade,
        job_type,
        words,
        boundary,
    }
}

const PLUMBING: &str = "plumbing_repair";
const HVAC: &str = "hvac_repair";
const ELECTRICAL: &str = "electrical";
const HANDYMAN: &str = "handyman";
const POOL: &str = "pool_repair";

/// The keyword rules; job type names are the catalog's (`docs/catalog/categories.yaml`).
const RULES: &[Rule] = &[
    rule(PLUMBING, None, &["plumber", "plumbing"], false),
    rule(
        PLUMBING,
        Some("leak repair"),
        &["leak", "leaking", "pipe"],
        true,
    ),
    rule(PLUMBING, Some("toilet repair"), &["toilet"], true),
    rule(
        PLUMBING,
        Some("drain clearing"),
        &["drain", "clog", "clogged"],
        true,
    ),
    rule(PLUMBING, Some("faucet replacement"), &["faucet"], true),
    rule(HVAC, None, &["hvac"], false),
    rule(
        HVAC,
        Some("AC not cooling"),
        &["ac", "air conditioner", "warm air", "not cooling"],
        true,
    ),
    rule(HVAC, Some("thermostat"), &["thermostat"], true),
    rule(HVAC, Some("furnace repair"), &["furnace", "heater"], true),
    rule(PLUMBING, None, &["water heater"], false),
    rule(POOL, None, &["pool heater", "spa heater"], false),
    rule(
        HVAC,
        Some("evaporative (swamp) cooler repair"),
        &["swamp cooler", "evaporative cooler"],
        true,
    ),
    rule(ELECTRICAL, None, &["electrician", "electrical"], false),
    rule(ELECTRICAL, Some("outlet repair"), &["outlet"], true),
    rule(ELECTRICAL, Some("breaker trips"), &["breaker"], true),
    rule(ELECTRICAL, Some("light fixture"), &["light fixture"], true),
    rule(ELECTRICAL, Some("panel upgrade"), &["panel"], true),
    rule(
        ELECTRICAL,
        Some("new circuit"),
        &["circuit", "wiring"],
        true,
    ),
    rule(HANDYMAN, None, &["handyman", "small repair"], false),
    rule(HANDYMAN, Some("door repair"), &["door"], false),
    rule(HANDYMAN, Some("drywall patch"), &["drywall"], false),
    rule(HANDYMAN, Some("shelves"), &["shelf", "shelves"], false),
    rule(HANDYMAN, Some("fence repair"), &["fence"], false),
];

/// Label words too generic to name one option ("just a small repair" must not pick appliance
/// repair or roof repair, D-103).
const GENERIC: &[&str] = &[
    "repair",
    "replacement",
    "cleaning",
    "maintenance",
    "pickup",
    "removal",
    "detailing",
    "washing",
    "systems",
    "other",
];

/// The trade as an option label reads (INT-013), whose words, less the [`GENERIC`] ones, also
/// answer the question (D-103).
#[must_use]
pub fn trade_label(trade: &str) -> String {
    match trade {
        PLUMBING => "plumbing".to_owned(),
        HVAC => "heating/AC".to_owned(),
        _ => trade.replace('_', " "),
    }
}

/// The catalog job types the keyword rules know for `trade`, listed by "Which fits best?" (D-102).
#[must_use]
pub fn job_types_of(trade: &str) -> Vec<&'static str> {
    RULES
        .iter()
        .filter(|r| r.trade == trade)
        .filter_map(|r| r.job_type)
        .collect()
}

/// One AI reading of the request (AI-011): a catalog category ID and, where read, a job type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading<'a> {
    /// The catalog category ID, such as `hvac_repair`.
    pub trade: &'a str,
    /// The job type read, such as `AC not cooling`.
    pub job_type: Option<&'a str>,
}

/// What the keyword rules found, in the order the words appear in the message (AI-011).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeywordReading {
    /// The distinct trades matched, first-named first.
    pub trades: Vec<&'static str>,
    /// The (trade, job type) pairs matched, first-named first.
    pub job_types: Vec<(&'static str, &'static str)>,
    /// The job types among `job_types` that sit behind a license (D-105).
    pub boundary: Vec<(&'static str, &'static str)>,
}

/// Where a phrase first appears in `padded` (folded, space-padded) as whole words.
fn first_at(padded: &str, words: &[&str]) -> Option<usize> {
    words
        .iter()
        .filter_map(|w| padded.find(&format!(" {w} ")))
        .min()
}

/// Each rule's first whole-word match, longest phrase first: a matched phrase is blanked out, so
/// a shorter phrase inside it ("heater" in "water heater") never matches there.
fn rule_hits(words: &str) -> Vec<(usize, &'static Rule)> {
    let mut working = format!(" {} ", fold(words));
    let mut phrases: Vec<(&str, &'static Rule)> = RULES
        .iter()
        .flat_map(|r| r.words.iter().map(move |w| (*w, r)))
        .collect();
    phrases.sort_by_key(|(w, _)| std::cmp::Reverse(w.len()));
    let mut hits: Vec<(usize, &'static Rule)> = Vec::new();
    for (w, r) in phrases {
        let needle = format!(" {w} ");
        while let Some(at) = working.find(&needle) {
            working.replace_range(at + 1..at + 1 + w.len(), &" ".repeat(w.len()));
            match hits.iter_mut().find(|(_, h)| std::ptr::eq(*h, r)) {
                Some(hit) => hit.0 = hit.0.min(at),
                None => hits.push((at, r)),
            }
        }
    }
    hits.sort_by_key(|(at, _)| *at);
    hits
}

/// The keyword second opinion on trade and job type (AI-011): whole words only, so "repair"
/// never matches "air" and "leaky" is not "leak"; the longest phrase wins ("water heater").
#[must_use]
pub fn keyword_reading(words: &str) -> KeywordReading {
    let hits = rule_hits(words);
    let mut out = KeywordReading::default();
    for (_, r) in hits {
        if !out.trades.contains(&r.trade) {
            out.trades.push(r.trade);
        }
        if let Some(job) = r.job_type {
            out.job_types.push((r.trade, job));
            if r.boundary {
                out.boundary.push((r.trade, job));
            }
        }
    }
    out
}

/// The numbered trade question (INT-013, D-104) with its INT-015 fallback (D-103).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeQuestion {
    /// The trades offered, option 1 first; a re-ask offers exactly these again (D-103).
    pub options: Vec<String>,
    /// The trade a second unusable answer applies: the keyword trade if exactly one trade matched,
    /// otherwise the first reading (INT-015).
    pub fallback: String,
}

/// The trade decision before any question (AI-011, INT-013).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opinion {
    /// Both readings agree and the keywords name that trade or none: no question.
    Settled(String),
    /// The trade is in doubt: ask this question.
    Ask(TradeQuestion),
}

/// Compares the two readings with the keyword reading (AI-011, INT-013, D-104): the trade is in
/// doubt when the readings differ or the keywords name trades but not the readings' one; the
/// question then offers every distinct trade, readings first, up to three.
#[must_use]
pub fn second_opinion(readings: [Reading<'_>; 2], keywords: &KeywordReading) -> Opinion {
    let first = readings[0].trade;
    let agreed = readings[1].trade == first;
    if agreed && (keywords.trades.is_empty() || keywords.trades.contains(&first)) {
        return Opinion::Settled(first.to_owned());
    }
    let mut options: Vec<String> = Vec::with_capacity(3);
    let all = readings
        .iter()
        .map(|r| r.trade)
        .chain(keywords.trades.iter().copied());
    for trade in all {
        if options.len() < 3 && !options.iter().any(|o| o == trade) {
            options.push(trade.to_owned());
        }
    }
    let fallback = match keywords.trades.as_slice() {
        [only] => (*only).to_owned(),
        _ => first.to_owned(),
    };
    Opinion::Ask(TradeQuestion { options, fallback })
}

/// How one reply to the trade question reads (INT-012, INT-015).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeAnswer {
    /// One option, by its number or its whole words.
    Trade(String),
    /// "Not sure" (or "no sé", "don't know").
    NotSure,
    /// Neither: no option, or more than one.
    Unusable,
}

/// What intake does after a reply (INT-015).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeStep {
    /// The homeowner chose this trade.
    Chosen(String),
    /// Ask once more, with these numbered options: the same ones, in the same order (D-103).
    AskAgain(Vec<String>),
    /// Apply the fallback trade and move on.
    Fallback(String),
}

const NOT_SURE: &[&str] = &["not sure", "dont know", "no idea", "unsure"];

/// "No sé" counts only nearly alone (at most this many words), since "no se" also starts
/// ordinary Spanish sentences ("no se puede cerrar").
const NO_SE_MAX_WORDS: usize = 3;

/// Whether a reply that names no option says "Not sure" (INT-015).
fn not_sure(folded: &str) -> bool {
    let padded = format!(" {folded} ");
    first_at(&padded, NOT_SURE).is_some()
        || (folded.split(' ').count() <= NO_SE_MAX_WORDS && first_at(&padded, &["no se"]).is_some())
}

impl TradeQuestion {
    /// Reads a reply (INT-012, D-103): a lone option number, the whole words of exactly one option
    /// (its trade's keywords or its label's non-generic words), or, naming no option, "Not sure";
    /// anything else is unusable.
    #[must_use]
    pub fn read(&self, words: &str) -> TradeAnswer {
        let folded = fold(words);
        let padded = format!(" {folded} ");
        let mut numbers: Vec<usize> = folded.split(' ').filter_map(|t| t.parse().ok()).collect();
        numbers.dedup();
        if let [n] = numbers.as_slice()
            && let Some(trade) = n.checked_sub(1).and_then(|i| self.options.get(i))
        {
            return TradeAnswer::Trade(trade.clone());
        }
        let hits: Vec<&String> = self
            .options
            .iter()
            .filter(|o| option_named(&padded, o))
            .collect();
        match hits.as_slice() {
            [one] => TradeAnswer::Trade((*one).clone()),
            [] if not_sure(&folded) => TradeAnswer::NotSure,
            _ => TradeAnswer::Unusable,
        }
    }

    /// The step after the `asked`-th asking (1 or 2) got `answer` (INT-015, D-103): "Not sure" or
    /// a second unusable reply applies the fallback, so the question is never asked a third time.
    #[must_use]
    pub fn step(&self, answer: TradeAnswer, asked: u8) -> TradeStep {
        match answer {
            TradeAnswer::Trade(trade) => TradeStep::Chosen(trade),
            TradeAnswer::Unusable if asked < 2 => TradeStep::AskAgain(self.options.clone()),
            TradeAnswer::NotSure | TradeAnswer::Unusable => {
                TradeStep::Fallback(self.fallback.clone())
            }
        }
    }
}

/// Whether the reply holds, as whole words, a keyword of `trade` (longest phrase first) or a
/// non-generic word of its label.
fn option_named(padded: &str, trade: &str) -> bool {
    let label = fold(&trade_label(trade));
    let label_words: Vec<&str> = label.split(' ').filter(|w| !GENERIC.contains(w)).collect();
    first_at(padded, &label_words).is_some()
        || rule_hits(padded).iter().any(|(_, r)| r.trade == trade)
}

/// The candidate job types after the trade is known (D-102, D-105, CARD-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidates {
    /// (trade, job type) pairs; `<trade>_other` only when nothing names a job type of the trade.
    pub job_types: Vec<(String, String)>,
    /// The job types "Which fits best?" lists, when it is asked (INT-014): the trade's job types
    /// when none was named, or the candidates when they span more than one trade.
    pub which_fits: Option<Vec<String>>,
}

/// The candidates for `trade` (D-102, D-105): the readings' and the keywords' job types of that
/// trade, never replaced by `<trade>_other`, plus every license-boundary keyword job type, so a
/// card goes only to a license covering all of them (CARD-001).
#[must_use]
pub fn candidates(trade: &str, readings: [Reading<'_>; 2], kw: &KeywordReading) -> Candidates {
    let mut jobs: Vec<(String, String)> = Vec::new();
    let read = readings.iter().filter_map(|r| Some((r.trade, r.job_type?)));
    for (t, j) in read.chain(kw.job_types.iter().copied()) {
        if t == trade {
            add_once(&mut jobs, t, j);
        }
    }
    let named = !jobs.is_empty();
    if !named {
        add_once(&mut jobs, trade, &format!("{trade}_other"));
    }
    for (t, j) in &kw.boundary {
        add_once(&mut jobs, t, j);
    }
    let spans = jobs.iter().any(|(t, _)| t != trade);
    let which_fits = if !named {
        Some(job_types_of(trade).into_iter().map(str::to_owned).collect())
    } else if spans {
        Some(jobs.iter().map(|(_, j)| j.clone()).collect())
    } else {
        None
    };
    Candidates {
        job_types: jobs,
        which_fits,
    }
}

/// Adds a (trade, job type) pair unless it is already there.
fn add_once(jobs: &mut Vec<(String, String)>, trade: &str, job: &str) {
    let pair = (trade.to_owned(), job.to_owned());
    if !jobs.contains(&pair) {
        jobs.push(pair);
    }
}

/// The first two trades a first message names, first-named first (INT-032): the job starts for
/// the first, and the "That's 2 jobs" reply offers ALSO for the second.
#[must_use]
pub fn two_trades(words: &str) -> Option<(&'static str, &'static str)> {
    match keyword_reading(words).trades.as_slice() {
        [first, second, ..] => Some((first, second)),
        _ => None,
    }
}

/// Whether a reply is ALSO or TAMBIÉN alone, which starts the second job (INT-032, D-493).
#[must_use]
pub fn starts_second(words: &str) -> bool {
    matches!(fold(words).as_str(), "also" | "tambien")
}
