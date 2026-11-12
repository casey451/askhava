//! The ZIP-to-town lookup measured at national size, measure only (TD-OPS-41 `perf_zip_scale`; INT-048; D-3292, D-3298).
//!
//! Builds a synthetic markets file of 41,000 made-up ZIP codes over 29,000 made-up towns in
//! one market, checks it with `parse_markets` (the same checks the reviewed file meets), then
//! times `Markets::town_for_zip` for the first, middle and last ZIP and an unknown one over a
//! fixed number of calls and prints one JSON line with the p50 and p95 per lookup in
//! microseconds. It fails only when a lookup returns the wrong town, or a town for the unknown
//! ZIP, never on time: the targets are goals (D-3292). It is `#[ignore]` so `Ci` stays fast; the
//! route benchmark (T417) runs it with `cargo test -p askhava-integration --test perf_zip_scale
//! -- --ignored`. Every code, name and ZIP is made up.
//!
//! Why 29,000 towns and not 30,000: `parse_markets` reads YAML with serde-saphyr's default budget
//! of 250,000 nodes, and a town costs seven nodes plus one per ZIP, so 30,000 towns with 41,000
//! ZIPs (about 251,000 nodes) is refused as malformed. That ceiling is a finding for the
//! performance track, not something this measurement changes (D-3299).

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

use askhava_catalog::api::{Markets, parse_markets};
use serde_json::{Value, json};

/// Towns in the synthetic file: just under the YAML node budget (see the module comment).
const TOWNS: usize = 29_000;
/// Towns that get a second ZIP, so the file holds `TOWNS + TWO_ZIP_TOWNS` ZIPs.
const TWO_ZIP_TOWNS: usize = 12_000;
/// The first synthetic ZIP; every ZIP after it is the next number.
const FIRST_ZIP: usize = 10_000;
/// Timed calls per lookup.
const CALLS: usize = 200;
/// A ZIP no synthetic town holds.
const UNKNOWN_ZIP: &str = "99999";

/// The town that holds the `n`th ZIP in file order: the first `TWO_ZIP_TOWNS` towns hold two
/// consecutive ZIPs each, every later town one.
fn town_of(n: usize) -> usize {
    if n < 2 * TWO_ZIP_TOWNS {
        n / 2
    } else {
        n - TWO_ZIP_TOWNS
    }
}

fn zip(n: usize) -> String {
    format!("{:05}", FIRST_ZIP + n)
}

/// The synthetic markets file: one AZ market, made-up town codes, names and ZIPs.
fn synthetic_file() -> String {
    let mut text = String::from(
        "markets:\n  - id: fake-national\n    name: Fake National\n    state: AZ\n    \
         time_zone: America/Phoenix\n    identity_sources: [city]\n    towns:\n",
    );
    let mut next = 0;
    for town in 0..TOWNS {
        let mut zips = format!("\"{}\"", zip(next));
        next += 1;
        if town < TWO_ZIP_TOWNS {
            write!(zips, ", \"{}\"", zip(next)).expect("string write");
            next += 1;
        }
        writeln!(
            text,
            "      - code: fake_town_{town:05}\n        name: Faketown {town:05}\n        zips: [{zips}]"
        )
        .expect("string write");
    }
    text
}

/// Nearest-rank percentile of sorted samples, in microseconds.
fn percentile(sorted: &[f64], pct: usize) -> f64 {
    let rank = (pct * sorted.len()).div_ceil(100).max(1);
    sorted[rank - 1]
}

/// Times `CALLS` lookups of `zip` and returns the town code found and the p50 and p95.
fn time_lookup(markets: &Markets, zip: &str) -> (Option<String>, Value) {
    let mut samples = Vec::with_capacity(CALLS);
    let mut found = None;
    for _ in 0..CALLS {
        let start = Instant::now();
        let town = black_box(markets.town_for_zip(black_box(zip)));
        samples.push(start.elapsed().as_secs_f64() * 1_000_000.0);
        found = town.map(|t| t.code);
    }
    samples.sort_by(f64::total_cmp);
    let timing = json!({
        "p50_us": percentile(&samples, 50),
        "p95_us": percentile(&samples, 95),
    });
    (found, timing)
}

/// TD-OPS-41: at about 41,000 ZIPs the first, middle and last ZIP find their own town and an
/// unknown ZIP finds none; the timings are printed, never judged (D-3292).
#[test]
#[ignore = "national-size measurement; the route benchmark (T417) runs it with --ignored"]
fn td_ops_41_zip_lookup_at_national_size() {
    let zips = TOWNS + TWO_ZIP_TOWNS;
    let markets = parse_markets(&synthetic_file()).expect("the synthetic file passes the checks");
    assert_eq!(markets.towns().len(), TOWNS);
    let mut lookups = serde_json::Map::new();
    for (label, n) in [("first", 0), ("middle", zips / 2), ("last", zips - 1)] {
        let (found, timing) = time_lookup(&markets, &zip(n));
        let expected = format!("fake_town_{:05}", town_of(n));
        assert_eq!(found.as_deref(), Some(expected.as_str()), "{label} ZIP");
        lookups.insert(label.to_owned(), timing);
    }
    let (found, timing) = time_lookup(&markets, UNKNOWN_ZIP);
    assert_eq!(found, None, "the unknown ZIP found a town");
    lookups.insert("unknown".to_owned(), timing);
    let line = json!({
        "measure": "zip_lookup",
        "towns": TOWNS,
        "zips": zips,
        "calls": CALLS,
        "lookups": lookups,
    });
    println!("{line}");
}
