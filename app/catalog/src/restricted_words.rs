//! The carrier-restricted content list (CARD-010, AI-021; D-485, D-492, D-813).
//!
//! The CTIA SHAFT categories (sex, hate, alcohol, firearms, tobacco) plus cannabis and other
//! drugs, in English and Spanish. `quotes` (T185) removes every phrase on it from a description
//! before it goes on a job card (CARD-010), and `ai` refuses any AI-written text holding one
//! (AI-021). Matching is on whole words, case-insensitive, so "Essex" or "gunk" never match, and
//! a hit names the list entry, never the text around it (D-485). Words that are also everyday
//! trade words ("weed" in a yard, "joint" in a pipe, "nail gun", "pistola de calor", a "wine"
//! fridge, "crack" in a wall) are left off on purpose (D-813).

/// Every phrase on the list, lower-case, one or more whole words (CARD-010).
pub const RESTRICTED_WORDS: &[&str] = &[
    // Sex.
    "sex",
    "sexual",
    "sexy",
    "porn",
    "porno",
    "pornography",
    "nude",
    "nudes",
    "escort",
    "escorts",
    "sexo",
    "desnudo",
    "desnuda",
    // Hate.
    "nazi",
    "nazis",
    "neo nazi",
    "white power",
    "kkk",
    // Alcohol.
    "alcohol",
    "beer",
    "beers",
    "liquor",
    "vodka",
    "whiskey",
    "tequila",
    "cerveza",
    "licor",
    // Firearms.
    "firearm",
    "firearms",
    "handgun",
    "handguns",
    "rifle",
    "rifles",
    "shotgun",
    "ammo",
    "ammunition",
    "arma de fuego",
    "armas de fuego",
    "escopeta",
    "municiones",
    // Tobacco.
    "tobacco",
    "cigarette",
    "cigarettes",
    "cigar",
    "cigars",
    "vape",
    "vapes",
    "vaping",
    "nicotine",
    "tabaco",
    "cigarro",
    "cigarrillo",
    "cigarrillos",
    // Cannabis and other drugs.
    "cannabis",
    "marijuana",
    "marihuana",
    "mota",
    "cbd",
    "thc",
    "kratom",
    "cocaine",
    "cocaína",
    "heroin",
    "heroína",
    "meth",
    "methamphetamine",
    "fentanyl",
    "fentanilo",
    "opioid",
    "opioids",
];

/// The first list entry `text` holds as whole words, or none (CARD-010, AI-021).
#[must_use]
pub fn restricted_match(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    RESTRICTED_WORDS.iter().copied().find(|entry| {
        let phrase: Vec<&str> = entry.split(' ').collect();
        words
            .windows(phrase.len())
            .any(|window| window == phrase.as_slice())
    })
}
