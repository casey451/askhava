//! T776 (D-5081, finding F20): the pure reading of a homeowner's kept words that feeds
//! `routing::route`'s refusals. LIC-017: whether a moving request names a move that does not stay
//! in Arizona, from folded phrase lists (as `routing`'s property-management list is, D-1011),
//! English and Spanish; a match is handed to `route` as `move_states` with one side not Arizona,
//! so `routing::move_refused` decides. LIC-018's property-management reading stays in `routing`
//! (`asks_property_management`); `order_dispatch` passes both inputs at the trade step (INT-016,
//! INT-017) and again before a card is shown or sent.

use crate::rules::fold;

/// Arizona, as `routing::move_refused` reads a state.
const ARIZONA: &str = "AZ";

/// The other side of a move the words place outside Arizona; never a state code.
const ELSEWHERE: &str = "outside Arizona";

/// Phrases that say the move leaves the state, folded (LIC-017).
const LEAVING: [&str; 11] = [
    "out of state",
    "another state",
    "different state",
    "out of arizona",
    "leaving arizona",
    "cross country",
    "across the country",
    "interstate",
    "fuera del estado",
    "otro estado",
    "fuera de arizona",
];

/// The full name of every other US state, and Mexico (which "New Mexico" also contains, so both
/// match), folded. A name directly followed by "king" is a bed size, not a place (LIC-017).
const STATES: [&str; 50] = [
    "alabama",
    "alaska",
    "arkansas",
    "california",
    "colorado",
    "connecticut",
    "delaware",
    "florida",
    "georgia",
    "hawaii",
    "idaho",
    "illinois",
    "indiana",
    "iowa",
    "kansas",
    "kentucky",
    "louisiana",
    "maine",
    "maryland",
    "massachusetts",
    "michigan",
    "minnesota",
    "mississippi",
    "missouri",
    "montana",
    "nebraska",
    "nevada",
    "new hampshire",
    "new jersey",
    "new mexico",
    "new york",
    "north carolina",
    "north dakota",
    "ohio",
    "oklahoma",
    "oregon",
    "pennsylvania",
    "rhode island",
    "south carolina",
    "south dakota",
    "tennessee",
    "texas",
    "utah",
    "vermont",
    "virginia",
    "washington",
    "west virginia",
    "wisconsin",
    "wyoming",
    "mexico",
];

/// The out-of-state places people near Lake Havasu name, folded (LIC-017). No Arizona town is
/// here, so Phoenix, Bullhead City, Kingman, Parker, Yuma, Tucson and Flagstaff never match.
const PLACES: [&str; 12] = [
    "needles",
    "laughlin",
    "las vegas",
    "henderson",
    "blythe",
    "palm springs",
    "los angeles",
    "san diego",
    "reno",
    "salt lake city",
    "albuquerque",
    "denver",
];

/// LIC-017: true when `words` name a move that does not stay in Arizona: a [`LEAVING`] phrase, a
/// [`PLACES`] town or a [`STATES`] name that is not a bed size ("California king"), whole words
/// only.
pub(crate) fn names_move_out(words: &str) -> bool {
    let folded = format!(" {} ", fold(words));
    let has = |p: &&str| folded.contains(&format!(" {p} "));
    LEAVING.iter().chain(&PLACES).any(has) || STATES.iter().any(|s| names_state(&folded, s))
}

/// True when `state` stands in `folded` (space-padded) as a whole name not followed by "king".
fn names_state(folded: &str, state: &str) -> bool {
    let name = format!(" {state} ");
    folded
        .match_indices(&name)
        .any(|(at, _)| !folded[at + name.len()..].starts_with("king "))
}

/// LIC-017: the `move_states` input of `routing::Request` read from `words`: Arizona to somewhere
/// else when [`names_move_out`] matches, so `routing::move_refused` refuses it; `None` otherwise.
pub(crate) fn move_states(words: &str) -> Option<(&'static str, &'static str)> {
    names_move_out(words).then_some((ARIZONA, ELSEWHERE))
}

#[cfg(test)]
mod tests {
    use super::{LEAVING, PLACES, STATES, move_states, names_move_out};
    use crate::routing::move_refused;

    #[test]
    fn lic_017_every_leaving_phrase_place_and_state_matches() {
        for phrase in LEAVING.iter().chain(&PLACES).chain(&STATES) {
            let words = format!("Need movers, {phrase} next week");
            assert!(names_move_out(&words), "{words}");
            assert!(names_move_out(&words.to_uppercase()), "{words} in capitals");
        }
        assert!(names_move_out("Mudanza a otro estado"));
        assert!(names_move_out("Moving to Las Vegas."));
    }

    #[test]
    fn lic_017_a_match_is_a_move_routing_refuses() {
        let (from, to) = move_states("movers to Needles").expect("a move out");
        assert!(move_refused(from, to));
        assert_eq!(move_states("movers across town"), None);
    }

    #[test]
    fn lic_017_an_arizona_town_never_matches() {
        let towns = [
            "Phoenix",
            "Bullhead City",
            "Kingman",
            "Parker",
            "Yuma",
            "Tucson",
            "Flagstaff",
            "Lake Havasu City",
        ];
        for town in towns {
            assert!(!names_move_out(&format!("Need movers to {town}")), "{town}");
        }
        assert!(!names_move_out("moving within Arizona"));
    }

    #[test]
    fn lic_017_mexico_and_new_mexico_both_match() {
        assert!(names_move_out("moving to New Mexico"));
        assert!(names_move_out("mudanza a México"));
        assert!(!names_move_out("a new bed frame"));
    }

    #[test]
    fn lic_017_a_bed_size_is_not_a_place() {
        assert!(!names_move_out("movers for a California king bed"));
        assert!(!names_move_out("a california king"));
        assert!(names_move_out("a California king bed to California"));
    }

    #[test]
    fn lic_017_whole_words_only() {
        assert!(!names_move_out("the renovation is done"));
        assert!(!names_move_out("a state of the art fridge"));
        assert!(!names_move_out("Arkansasville movers"));
    }
}
