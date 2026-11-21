//! The English wording of each question set's photo asks, read from the set's `kb_trade` file
//! (INT-044, D-432, D-5251).
//!
//! The question bank lists photo ids per set (`photos`); the English `ask` of each id lives in that
//! trade file's `photos_to_request`. Both files are compiled in and parsed once. Spanish wording is
//! not here (TXT-039).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::question_bank::QuestionBank;

/// The reviewed question bank, compiled in.
const BANK: &str = include_str!("../../../docs/catalog/question_bank.yaml");

/// The part of a trade file this module reads.
#[derive(Deserialize)]
struct TradeFile {
    /// Every photo the trade may ask for.
    #[serde(default)]
    photos_to_request: Vec<PhotoDef>,
}

/// One entry of `photos_to_request`.
#[derive(Deserialize)]
struct PhotoDef {
    /// The id the question bank lists.
    id: String,
    /// The English ask.
    ask: String,
}

/// The trade file compiled in for a `kb_trade` name; `None` for a name with no file.
fn trade_text(kb_trade: &str) -> Option<&'static str> {
    Some(match kb_trade {
        "appliance_repair" => {
            include_str!("../../../docs/catalog/trade_kb/trades/appliance_repair.yaml")
        }
        "boat_dock" => include_str!("../../../docs/catalog/trade_kb/trades/boat_dock.yaml"),
        "electrical" => include_str!("../../../docs/catalog/trade_kb/trades/electrical.yaml"),
        "flooring" => include_str!("../../../docs/catalog/trade_kb/trades/flooring.yaml"),
        "gutter_solar_cleaning" => {
            include_str!("../../../docs/catalog/trade_kb/trades/gutter_solar_cleaning.yaml")
        }
        "handyman" => include_str!("../../../docs/catalog/trade_kb/trades/handyman.yaml"),
        "home_inspection" => {
            include_str!("../../../docs/catalog/trade_kb/trades/home_inspection.yaml")
        }
        "house_cleaning" => {
            include_str!("../../../docs/catalog/trade_kb/trades/house_cleaning.yaml")
        }
        "hvac" => include_str!("../../../docs/catalog/trade_kb/trades/hvac.yaml"),
        "junk_removal" => include_str!("../../../docs/catalog/trade_kb/trades/junk_removal.yaml"),
        "landscaping_irrigation" => {
            include_str!("../../../docs/catalog/trade_kb/trades/landscaping_irrigation.yaml")
        }
        "moving" => include_str!("../../../docs/catalog/trade_kb/trades/moving.yaml"),
        "painting" => include_str!("../../../docs/catalog/trade_kb/trades/painting.yaml"),
        "pavers_concrete" => {
            include_str!("../../../docs/catalog/trade_kb/trades/pavers_concrete.yaml")
        }
        "pest_control" => include_str!("../../../docs/catalog/trade_kb/trades/pest_control.yaml"),
        "plumbing" => include_str!("../../../docs/catalog/trade_kb/trades/plumbing.yaml"),
        "pool_cleaning" => include_str!("../../../docs/catalog/trade_kb/trades/pool_cleaning.yaml"),
        "pool_spa_repair" => {
            include_str!("../../../docs/catalog/trade_kb/trades/pool_spa_repair.yaml")
        }
        "roofing" => include_str!("../../../docs/catalog/trade_kb/trades/roofing.yaml"),
        "security_alarm" => {
            include_str!("../../../docs/catalog/trade_kb/trades/security_alarm.yaml")
        }
        "septic" => include_str!("../../../docs/catalog/trade_kb/trades/septic.yaml"),
        "water_treatment" => {
            include_str!("../../../docs/catalog/trade_kb/trades/water_treatment.yaml")
        }
        "window_cleaning" => {
            include_str!("../../../docs/catalog/trade_kb/trades/window_cleaning.yaml")
        }
        "windows_doors" => include_str!("../../../docs/catalog/trade_kb/trades/windows_doors.yaml"),
        _ => return None,
    })
}

/// Every set's asks, by set id, in the bank's order; a set or id that does not resolve is left out.
fn load() -> BTreeMap<String, Vec<String>> {
    let bank: QuestionBank = serde_saphyr::from_str(BANK).unwrap_or(QuestionBank {
        version: 0,
        sets: BTreeMap::new(),
    });
    bank.sets
        .into_iter()
        .map(|(id, set)| {
            let defs = set
                .kb_trade
                .as_deref()
                .and_then(trade_text)
                .and_then(|text| serde_saphyr::from_str::<TradeFile>(text).ok())
                .map(|file| file.photos_to_request)
                .unwrap_or_default();
            let asks = set
                .photos
                .iter()
                .filter_map(|photo| defs.iter().find(|d| &d.id == photo))
                .map(|d| d.ask.clone())
                .collect();
            (id, asks)
        })
        .collect()
}

/// The English photo asks of a question set, in the bank's order (at most 3); none for a set with no
/// photos or an unknown set (INT-044, D-432).
pub fn photo_asks(set_id: &str) -> Vec<String> {
    static ASKS: OnceLock<BTreeMap<String, Vec<String>>> = OnceLock::new();
    ASKS.get_or_init(load)
        .get(set_id)
        .cloned()
        .unwrap_or_default()
}
