//! OPS-039's go and stop lines and their pure color judge (T621, D-4280, D-4448).
//!
//! The pilot scorecard colors only the 17 measures of `docs/PREBUILD.md` section 7 (refunds and
//! bank disputes are two measures there, each with its own pair) and shows D-458's 15, 50 and
//! 115 paid-introduction ladder; every other OPS-039 measure is uncolored. The lines are the
//! `ASKHAVA_SCORECARD_*` settings: `portals` never reads `Settings`, so the serving route passes
//! `Settings::scorecard_lines()` to [`ScorecardLines::from_pairs`]. Values are in the unit the
//! key names: whole percent for shares, hours, dollars, minutes, counts, and complaints per 20
//! jobs. A go line is met at or past it ("70% or more"); a stop line is crossed only beyond it
//! ("under 50%"), as the pilot plan words them (D-4448). Three lines hold only from a pilot week
//! on ("after 4 weeks": week 5, "by week 8": week 8, "month 3": week 9); before that the
//! measure is uncolored. Nothing here acts on a color: the page decides nothing (D-396, D-446).
//! No I/O, no database.

use std::collections::BTreeMap;

use thiserror::Error;

const PREFIX: &str = "ASKHAVA_SCORECARD_";
const LADDER_KEYS: [&str; 3] = ["LADDER_PILOT", "LADDER_20K", "LADDER_50K"];

/// An OPS-039 scorecard measure with a single value per trade or total (OPS-039, D-458).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measure {
    /// Requests with 1 or more quotes within 24 hours, percent.
    QuotedWithin24h,
    /// Routable requests with 2 or more quotes within 24 hours, percent.
    TwoQuotesWithin24h,
    /// 75th-percentile time to the first quote, hours.
    P75FirstQuoteHours,
    /// Median time to the first quote, hours.
    MedianFirstQuoteHours,
    /// Picks per request with quotes, percent.
    PicksPerQuotedRequest,
    /// Picks confirmed rather than lapsed, percent.
    PicksConfirmed,
    /// Average fee per paid introduction, dollars.
    AverageFee,
    /// Free share of introductions after each business's first win, percent.
    FreeShare,
    /// Requests started per week, count.
    RequestsPerWeek,
    /// Paid introductions this month, count.
    PaidIntroductionsThisMonth,
    /// Net per paid introduction, dollars.
    NetPerIntroduction,
    /// Started intakes that reached the card step, percent.
    IntakeReachedCard,
    /// Business sign-ups active within 15 minutes, percent.
    SignupsActiveIn15Min,
    /// Refunds, percent of paid introductions.
    Refunds,
    /// Bank disputes, percent of paid introductions.
    BankDisputes,
    /// Businesses paying again within 60 days, percent.
    PayingAgainIn60Days,
    /// Casey's minutes per paid introduction, recruiting counted separately.
    MinutesPerIntroduction,
    /// Homeowner complaints per 20 jobs.
    ComplaintsPer20Jobs,
    /// Masked calls answered (CALL-024); no line.
    MaskedCallsAnswered,
    /// Share of picks that were diagnostic-visit quotes; no line.
    DiagnosticVisitPicks,
    /// Share of cards approved without an edit; no line.
    CardsApprovedWithoutEdit,
    /// Share of requests with a photo; no line.
    RequestsWithPhoto,
    /// Notes returned to businesses (PICK-052); no line.
    NotesReturned,
    /// Routable requests with 3 or more quotes within 24 hours; no line.
    ThreeQuotesWithin24h,
    /// Picks among requests with 2 or more quotes; no line.
    PicksAmongTwoQuoteRequests,
    /// Active businesses per trade; no line.
    ActiveBusinesses,
    /// Quotes per win per trade (D-468); no line.
    QuotesPerWin,
    /// Share of business sign-ups sent to a mailed letter or review; no line.
    SignupsToLetterOrReview,
}

/// A measure's color against its lines; `Uncolored` when it has none or they do not yet hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    /// At or past the go line.
    Green,
    /// Between the go and stop lines.
    Amber,
    /// Beyond the stop line.
    Red,
    /// No line applies.
    Uncolored,
}

/// Where this month's paid introductions stand on D-458's ladder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LadderRung {
    /// Under the pilot rung.
    Below,
    /// The pilot rung (15 a month by default).
    Pilot,
    /// The $20,000 a year pace (50 a month by default).
    Pace20k,
    /// The $50,000 a year pace (115 a month by default).
    Pace50k,
}

#[derive(Clone, Copy)]
enum Better {
    More,
    Less,
}

struct LineSpec {
    measure: Measure,
    stem: &'static str,
    better: Better,
    from_week: u32,
}

const fn spec(measure: Measure, stem: &'static str, better: Better, from_week: u32) -> LineSpec {
    LineSpec {
        measure,
        stem,
        better,
        from_week,
    }
}

/// The measures with lines, in `docs/PREBUILD.md` section 7's order (D-4280, D-4448).
const LINES: [LineSpec; 18] = [
    spec(Measure::QuotedWithin24h, "QUOTED_24H", Better::More, 5),
    spec(
        Measure::TwoQuotesWithin24h,
        "TWO_QUOTES_24H",
        Better::More,
        1,
    ),
    spec(
        Measure::P75FirstQuoteHours,
        "P75_FIRST_QUOTE_HOURS",
        Better::Less,
        1,
    ),
    spec(
        Measure::MedianFirstQuoteHours,
        "MEDIAN_FIRST_QUOTE_HOURS",
        Better::Less,
        1,
    ),
    spec(
        Measure::PicksPerQuotedRequest,
        "PICKS_PER_QUOTED",
        Better::More,
        1,
    ),
    spec(Measure::PicksConfirmed, "PICKS_CONFIRMED", Better::More, 1),
    spec(Measure::AverageFee, "AVERAGE_FEE_DOLLARS", Better::More, 1),
    spec(Measure::FreeShare, "FREE_SHARE", Better::Less, 1),
    spec(
        Measure::RequestsPerWeek,
        "REQUESTS_PER_WEEK",
        Better::More,
        8,
    ),
    spec(
        Measure::PaidIntroductionsThisMonth,
        "PAID_INTROS_MONTH",
        Better::More,
        9,
    ),
    spec(
        Measure::NetPerIntroduction,
        "NET_PER_INTRO_DOLLARS",
        Better::More,
        1,
    ),
    spec(
        Measure::IntakeReachedCard,
        "INTAKE_CARD_STEP",
        Better::More,
        1,
    ),
    spec(
        Measure::SignupsActiveIn15Min,
        "SIGNUPS_ACTIVE_15MIN",
        Better::More,
        1,
    ),
    spec(Measure::Refunds, "REFUNDS", Better::Less, 1),
    spec(Measure::BankDisputes, "BANK_DISPUTES", Better::Less, 1),
    spec(
        Measure::PayingAgainIn60Days,
        "PAYING_AGAIN_60D",
        Better::More,
        1,
    ),
    spec(
        Measure::MinutesPerIntroduction,
        "MINUTES_PER_INTRO",
        Better::Less,
        1,
    ),
    spec(
        Measure::ComplaintsPer20Jobs,
        "COMPLAINTS_PER_20",
        Better::Less,
        1,
    ),
];

/// A scorecard key missing from, or unknown to, the pairs given; names only the key.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum LinesError {
    /// A line or ladder key was not given.
    #[error("scorecard setting {0} is missing")]
    Missing(String),
    /// A key no line or rung reads.
    #[error("scorecard setting {0} is unknown")]
    Unknown(String),
}

/// Every go and stop line and ladder rung, filled from the settings (OPS-039, D-458, D-4448).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScorecardLines {
    pairs: [(u32, u32); LINES.len()],
    ladder: [u32; 3],
}

impl ScorecardLines {
    /// Reads every `ASKHAVA_SCORECARD_*` line from `Settings::scorecard_lines()` (OPS-039, D-4448).
    ///
    /// # Errors
    ///
    /// [`LinesError::Missing`] when a line or rung key is absent, [`LinesError::Unknown`] for a
    /// key no line reads, so the two crates' key lists cannot drift apart unnoticed.
    pub fn from_pairs<'a, I>(pairs: I) -> Result<Self, LinesError>
    where
        I: IntoIterator<Item = (&'a str, u32)>,
    {
        let mut given: BTreeMap<&str, u32> = pairs.into_iter().collect();
        let mut take = |stem: &str| {
            let key = format!("{PREFIX}{stem}");
            given.remove(key.as_str()).ok_or(LinesError::Missing(key))
        };
        let mut lines = [(0, 0); LINES.len()];
        for (pair, line) in lines.iter_mut().zip(&LINES) {
            let go = take(&format!("{}_GO", line.stem))?;
            *pair = (go, take(&format!("{}_STOP", line.stem))?);
        }
        let ladder = [
            take(LADDER_KEYS[0])?,
            take(LADDER_KEYS[1])?,
            take(LADDER_KEYS[2])?,
        ];
        match given.into_keys().next() {
            Some(key) => Err(LinesError::Unknown(key.to_owned())),
            None => Ok(Self {
                pairs: lines,
                ladder,
            }),
        }
    }

    /// D-458's ladder rungs: the pilot, $20,000 and $50,000 a year paces, paid introductions a month.
    #[must_use]
    pub fn ladder(&self) -> [u32; 3] {
        self.ladder
    }
}

/// OPS-039, D-4280, D-4448: colors `value` (in the measure's unit) against its lines in pilot
/// week `pilot_week` (1-based). Green at or past the go line, red beyond the stop line, amber
/// between; a "less is better" measure is judged the other way. A measure with no line, a line
/// whose week has not come, or a value that is not a finite number is uncolored.
#[must_use]
pub fn color(measure: Measure, value: f64, pilot_week: u32, lines: &ScorecardLines) -> Color {
    let Some(index) = LINES.iter().position(|line| line.measure == measure) else {
        return Color::Uncolored;
    };
    let line = &LINES[index];
    if pilot_week < line.from_week || !value.is_finite() {
        return Color::Uncolored;
    }
    let (go, stop) = lines.pairs[index];
    let (go, stop) = (f64::from(go), f64::from(stop));
    let (green, red) = match line.better {
        Better::More => (value >= go, value < stop),
        Better::Less => (value <= go, value > stop),
    };
    if green {
        Color::Green
    } else if red {
        Color::Red
    } else {
        Color::Amber
    }
}

/// OPS-039, D-458: the highest ladder rung this month's paid introductions have reached.
#[must_use]
pub fn ladder_rung(paid_this_month: u32, lines: &ScorecardLines) -> LadderRung {
    let [pilot, pace_20k, pace_50k] = lines.ladder;
    if paid_this_month >= pace_50k {
        LadderRung::Pace50k
    } else if paid_this_month >= pace_20k {
        LadderRung::Pace20k
    } else if paid_this_month >= pilot {
        LadderRung::Pilot
    } else {
        LadderRung::Below
    }
}

#[cfg(test)]
mod tests {
    use askhava_core::api::Settings;
    use proptest::prelude::*;

    use super::*;

    const STEP: f64 = 0.01;
    const LATE: u32 = 13;

    /// Each line as `docs/PREBUILD.md` section 7 states it: (measure, go, stop, more is better).
    const PREBUILD: [(Measure, f64, f64, bool); 18] = [
        (Measure::QuotedWithin24h, 70.0, 50.0, true),
        (Measure::TwoQuotesWithin24h, 60.0, 30.0, true),
        (Measure::P75FirstQuoteHours, 12.0, 24.0, false),
        (Measure::MedianFirstQuoteHours, 4.0, 12.0, false),
        (Measure::PicksPerQuotedRequest, 40.0, 25.0, true),
        (Measure::PicksConfirmed, 85.0, 75.0, true),
        (Measure::AverageFee, 50.0, 35.0, true),
        (Measure::FreeShare, 20.0, 40.0, false),
        (Measure::RequestsPerWeek, 15.0, 6.0, true),
        (Measure::PaidIntroductionsThisMonth, 15.0, 5.0, true),
        (Measure::NetPerIntroduction, 40.0, 28.0, true),
        (Measure::IntakeReachedCard, 80.0, 60.0, true),
        (Measure::SignupsActiveIn15Min, 70.0, 40.0, true),
        (Measure::Refunds, 10.0, 20.0, false),
        (Measure::BankDisputes, 1.0, 2.0, false),
        (Measure::PayingAgainIn60Days, 50.0, 30.0, true),
        (Measure::MinutesPerIntroduction, 20.0, 45.0, false),
        (Measure::ComplaintsPer20Jobs, 1.0, 2.0, false),
    ];

    fn settings(extra: &[(&str, &str)]) -> Settings {
        let base = [
            ("ASKHAVA_APP_ENV", "dev"),
            ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        ];
        let pairs = base.iter().chain(extra);
        Settings::from_pairs(pairs.map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))).unwrap()
    }

    fn lines_from(settings: &Settings) -> ScorecardLines {
        ScorecardLines::from_pairs(settings.scorecard_lines()).unwrap()
    }

    fn pilot_plan() -> ScorecardLines {
        lines_from(&settings(&[]))
    }

    #[test]
    fn each_prebuild_line_colors_right_at_just_inside_and_just_outside() {
        let lines = pilot_plan();
        for (measure, go, stop, more) in PREBUILD {
            let toward = if more { STEP } else { -STEP };
            let at = |value| color(measure, value, LATE, &lines);
            assert_eq!(at(go), Color::Green, "{measure:?} at go");
            assert_eq!(at(go + toward), Color::Green, "{measure:?} past go");
            assert_eq!(at(go - toward), Color::Amber, "{measure:?} short of go");
            assert_eq!(at(stop), Color::Amber, "{measure:?} at stop");
            assert_eq!(at(stop + toward), Color::Amber, "{measure:?} inside stop");
            assert_eq!(at(stop - toward), Color::Red, "{measure:?} beyond stop");
        }
    }

    #[test]
    fn ladder_rungs_turn_at_15_50_and_115() {
        let lines = pilot_plan();
        assert_eq!(lines.ladder(), [15, 50, 115]);
        let cases = [
            (14, LadderRung::Below),
            (15, LadderRung::Pilot),
            (16, LadderRung::Pilot),
            (49, LadderRung::Pilot),
            (50, LadderRung::Pace20k),
            (51, LadderRung::Pace20k),
            (114, LadderRung::Pace20k),
            (115, LadderRung::Pace50k),
            (116, LadderRung::Pace50k),
        ];
        for (paid, rung) in cases {
            assert_eq!(ladder_rung(paid, &lines), rung, "{paid} paid");
        }
    }

    #[test]
    fn time_conditioned_lines_stay_uncolored_until_their_week() {
        let lines = pilot_plan();
        let cases = [
            (Measure::QuotedWithin24h, 5),
            (Measure::RequestsPerWeek, 8),
            (Measure::PaidIntroductionsThisMonth, 9),
        ];
        for (measure, week) in cases {
            assert_eq!(color(measure, 0.0, week - 1, &lines), Color::Uncolored);
            assert_eq!(color(measure, 0.0, week, &lines), Color::Red);
        }
        assert_eq!(color(Measure::Refunds, 0.0, 1, &lines), Color::Green);
    }

    #[test]
    fn a_measure_with_no_line_or_no_number_has_no_color() {
        let lines = pilot_plan();
        for measure in [
            Measure::MaskedCallsAnswered,
            Measure::DiagnosticVisitPicks,
            Measure::CardsApprovedWithoutEdit,
            Measure::RequestsWithPhoto,
            Measure::NotesReturned,
            Measure::ThreeQuotesWithin24h,
            Measure::PicksAmongTwoQuoteRequests,
            Measure::ActiveBusinesses,
            Measure::QuotesPerWin,
            Measure::SignupsToLetterOrReview,
        ] {
            assert_eq!(color(measure, 50.0, LATE, &lines), Color::Uncolored);
        }
        assert_eq!(
            color(Measure::AverageFee, f64::NAN, LATE, &lines),
            Color::Uncolored
        );
    }

    #[test]
    fn a_setting_moves_its_line_and_a_bad_or_stray_key_is_refused() {
        let moved = lines_from(&settings(&[
            ("ASKHAVA_SCORECARD_QUOTED_24H_GO", "75"),
            ("ASKHAVA_SCORECARD_LADDER_PILOT", "20"),
        ]));
        assert_eq!(
            color(Measure::QuotedWithin24h, 74.0, LATE, &moved),
            Color::Amber
        );
        assert_eq!(ladder_rung(19, &moved), LadderRung::Below);
        let bad = [
            ("ASKHAVA_APP_ENV".to_owned(), "dev".to_owned()),
            (
                "ASKHAVA_DATABASE_URL".to_owned(),
                "postgres://fake".to_owned(),
            ),
            ("ASKHAVA_SCORECARD_REFUNDS_GO".to_owned(), "ten".to_owned()),
        ];
        let refused = Settings::from_pairs(bad).unwrap_err().to_string();
        assert!(refused.contains("ASKHAVA_SCORECARD_REFUNDS_GO") && !refused.contains("ten"));
        let settings = settings(&[]);
        let short = settings.scorecard_lines().skip(1);
        let missing = ScorecardLines::from_pairs(short).unwrap_err();
        assert_eq!(
            missing,
            LinesError::Missing(format!("{PREFIX}QUOTED_24H_GO"))
        );
        let stray = settings
            .scorecard_lines()
            .chain([("ASKHAVA_SCORECARD_X", 1)]);
        let unknown = ScorecardLines::from_pairs(stray).unwrap_err();
        assert_eq!(
            unknown,
            LinesError::Unknown("ASKHAVA_SCORECARD_X".to_owned())
        );
    }

    fn rank(color: Color) -> u8 {
        match color {
            Color::Red => 0,
            Color::Amber => 1,
            Color::Green => 2,
            Color::Uncolored => 3,
        }
    }

    proptest! {
        /// OPS-039: moving a value toward the go line never worsens its color, whatever the lines.
        #[test]
        fn moving_toward_go_never_worsens(
            index in 0..LINES.len(),
            go in 0..200_u32,
            stop in 0..200_u32,
            a in 0.0..250.0_f64,
            b in 0.0..250.0_f64,
            week in 1..20_u32,
        ) {
            let mut lines = pilot_plan();
            lines.pairs[index] = (go, stop);
            let measure = LINES[index].measure;
            let (low, high) = if a <= b { (a, b) } else { (b, a) };
            let (from, to) = match LINES[index].better {
                Better::More => (low, high),
                Better::Less => (high, low),
            };
            let before = color(measure, from, week, &lines);
            let after = color(measure, to, week, &lines);
            prop_assert_eq!(before == Color::Uncolored, after == Color::Uncolored);
            prop_assert!(rank(after) >= rank(before));
        }
    }
}
