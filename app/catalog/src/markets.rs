//! Markets as reviewed data and job locations by ZIP centroid (INT-048, INT-022, LIC-011; D-1515,
//! D-1518, D-1521, D-3113 to D-3115).
//!
//! `docs/catalog/markets.yaml` and `data/zcta_centroids_az.csv` (the Census Gazetteer's Arizona
//! ZCTA internal points) are compiled in, so nothing reads a folder at run time. [`parse_markets`]
//! checks the markets file as `loader::parse` checks the categories: it refuses a duplicate market
//! ID, a town in two markets, a ZIP under two towns, an unknown IANA time zone, a market with no
//! towns and a state other than AZ without a decision ID (D-382). No code here names a market:
//! every market value comes from the file. [`store`] writes `catalog.market`, `market_town` and
//! `zcta_centroid`; it never sets `launched_at`, a launch being a later owner action.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use serde::Deserialize;
use sqlx::{Connection, PgConnection};

use crate::models::CatalogError;

/// The reviewed markets file, compiled in.
const EMBEDDED: &str = include_str!("../../../docs/catalog/markets.yaml");
/// The Census ZCTA centroid rows, compiled in.
const CENTROIDS: &str = include_str!("../data/zcta_centroids_az.csv");
/// The state whose towns need no further legal review (INT-022, D-382).
const REVIEWED_STATE: &str = "AZ";
/// The mean Earth radius in miles, for [`miles_between`].
const EARTH_MILES: f64 = 3958.8;

/// Why a markets file or centroid table was refused (INT-048). No variant carries a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MarketRefused {
    /// The text does not parse, or a code, state or ZIP is malformed.
    #[error("markets file malformed")]
    Malformed,
    /// Two markets share an ID.
    #[error("duplicate market ID")]
    DuplicateId,
    /// One town code is listed twice, in one market or two.
    #[error("town in two markets")]
    TownInTwoMarkets,
    /// One ZIP is listed under two towns; the reviewer assigns it to one.
    #[error("ZIP under two towns")]
    ZipInTwoTowns,
    /// The time zone is not an IANA name.
    #[error("unknown time zone")]
    UnknownTimeZone,
    /// A market lists no towns.
    #[error("market with no towns")]
    NoTowns,
    /// A state other than AZ without the decision ID of its legal review (INT-022, D-382).
    #[error("state needs a decision ID")]
    StateNeedsDecision,
}

/// A town as the file gives it (INT-048).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TownDef {
    /// The service-area town code (INT-022), such as the one businesses tick.
    pub code: String,
    /// The town's name.
    pub name: String,
    /// Its ZCTAs from the Census ZCTA-to-place relationship file.
    pub zips: Vec<String>,
    /// Reviewed ZIPs with no centroid, which give the town but no location.
    #[serde(default)]
    pub extra_zips: Vec<String>,
}

/// A market as the file gives it (INT-048).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDef {
    /// The market's ID.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its two-letter state.
    pub state: String,
    /// Its IANA time zone.
    pub time_zone: String,
    /// The identity sources for its no-license categories (LIC-011).
    pub identity_sources: Vec<String>,
    /// The decision ID of the legal review a state other than AZ needs (D-382).
    #[serde(default)]
    pub decision: Option<String>,
    /// Its towns.
    pub towns: Vec<TownDef>,
}

/// A checked markets file (INT-048): build it with [`parse_markets`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Markets {
    markets: Vec<MarketDef>,
}

/// A service-area town with its market and state (INT-022, INT-048).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketTown {
    /// The town code.
    pub code: String,
    /// The town's name.
    pub name: String,
    /// Its market's ID.
    pub market: String,
    /// Its market's state.
    pub state: String,
}

/// A ZIP's location: the Census ZCTA internal point, in degrees (INT-048).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Centroid {
    /// Latitude.
    pub lat: f64,
    /// Longitude.
    pub lon: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    markets: Vec<MarketDef>,
}

impl Markets {
    /// The markets, in file order.
    #[must_use]
    pub fn markets(&self) -> &[MarketDef] {
        &self.markets
    }

    /// Every town of every market with its market and state: the service-area towns INT-022
    /// checks (INT-048).
    #[must_use]
    pub fn towns(&self) -> Vec<MarketTown> {
        self.pairs().map(|(m, t)| MarketTown::of(m, t)).collect()
    }

    /// The town a ZIP or reviewed extra ZIP belongs to, or none (INT-048, D-1521).
    #[must_use]
    pub fn town_for_zip(&self, zip: &str) -> Option<MarketTown> {
        let zip = zip.trim();
        self.pairs()
            .find(|(_, t)| t.zips.iter().chain(&t.extra_zips).any(|z| z == zip))
            .map(|(m, t)| MarketTown::of(m, t))
    }

    /// Every (market, town) in file order.
    fn pairs(&self) -> impl Iterator<Item = (&MarketDef, &TownDef)> {
        self.markets
            .iter()
            .flat_map(|m| m.towns.iter().map(move |t| (m, t)))
    }
}

impl MarketTown {
    fn of(m: &MarketDef, t: &TownDef) -> Self {
        Self {
            code: t.code.clone(),
            name: t.name.clone(),
            market: m.id.clone(),
            state: m.state.clone(),
        }
    }
}

/// True for exactly five ASCII digits.
#[must_use]
pub fn is_zip(zip: &str) -> bool {
    zip.len() == 5 && zip.bytes().all(|b| b.is_ascii_digit())
}

/// True for a lower-case code of letters, digits, `_` and `-`.
fn is_code(code: &str) -> bool {
    !code.is_empty()
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// Parses and checks a markets file (INT-048, INT-022, D-382).
///
/// # Errors
///
/// The first [`MarketRefused`] found.
pub fn parse_markets(text: &str) -> Result<Markets, MarketRefused> {
    let file: File = serde_saphyr::from_str(text).map_err(|_| MarketRefused::Malformed)?;
    let (mut ids, mut towns, mut zips) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for m in &file.markets {
        check_market(m)?;
        if !ids.insert(m.id.as_str()) {
            return Err(MarketRefused::DuplicateId);
        }
        for t in &m.towns {
            check_town(t)?;
            if !towns.insert(t.code.as_str()) {
                return Err(MarketRefused::TownInTwoMarkets);
            }
            if !t
                .zips
                .iter()
                .chain(&t.extra_zips)
                .all(|z| zips.insert(z.as_str()))
            {
                return Err(MarketRefused::ZipInTwoTowns);
            }
        }
    }
    Ok(Markets {
        markets: file.markets,
    })
}

/// One market's own rules.
fn check_market(m: &MarketDef) -> Result<(), MarketRefused> {
    let state_ok = m.state.len() == 2 && m.state.bytes().all(|b| b.is_ascii_uppercase());
    if !is_code(&m.id) || !state_ok || m.name.trim().is_empty() || m.identity_sources.is_empty() {
        return Err(MarketRefused::Malformed);
    }
    if jiff::tz::TimeZone::get(&m.time_zone).is_err() {
        return Err(MarketRefused::UnknownTimeZone);
    }
    if m.towns.is_empty() {
        return Err(MarketRefused::NoTowns);
    }
    let reviewed = m.decision.as_deref().is_some_and(|d| d.starts_with("D-"));
    if m.state != REVIEWED_STATE && !reviewed {
        return Err(MarketRefused::StateNeedsDecision);
    }
    Ok(())
}

/// One town's own rules.
fn check_town(t: &TownDef) -> Result<(), MarketRefused> {
    let zips_ok = t.zips.iter().chain(&t.extra_zips).all(|z| is_zip(z));
    if is_code(&t.code) && !t.name.trim().is_empty() && zips_ok {
        Ok(())
    } else {
        Err(MarketRefused::Malformed)
    }
}

/// The markets file compiled into the build, checked (INT-048).
///
/// # Errors
///
/// [`CatalogError::Invalid`] when the file breaks a rule of [`parse_markets`].
pub fn embedded() -> Result<&'static Markets, CatalogError> {
    static PARSED: LazyLock<Result<Markets, MarketRefused>> =
        LazyLock::new(|| parse_markets(EMBEDDED));
    PARSED.as_ref().map_err(|_| CatalogError::Invalid)
}

/// Parses centroid rows `zcta,lat,lon`, skipping `#` lines and the header (INT-048).
///
/// # Errors
///
/// [`MarketRefused::Malformed`] for a row without a 5-digit code and two numbers, or a repeat.
pub fn parse_centroids(text: &str) -> Result<BTreeMap<String, Centroid>, MarketRefused> {
    let mut out = BTreeMap::new();
    let rows = text
        .lines()
        .filter(|l| !l.starts_with('#') && *l != "zcta,lat,lon");
    for row in rows {
        let mut parts = row.split(',');
        let (zip, lat, lon) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(z), Some(a), Some(o), None) => (z, a.parse(), o.parse()),
            _ => return Err(MarketRefused::Malformed),
        };
        let (Ok(lat), Ok(lon), true) = (lat, lon, is_zip(zip)) else {
            return Err(MarketRefused::Malformed);
        };
        if out.insert(zip.to_owned(), Centroid { lat, lon }).is_some() {
            return Err(MarketRefused::Malformed);
        }
    }
    Ok(out)
}

/// The compiled-in centroid table (INT-048).
///
/// # Errors
///
/// [`CatalogError::Invalid`] when a row is malformed.
pub fn centroids() -> Result<&'static BTreeMap<String, Centroid>, CatalogError> {
    static PARSED: LazyLock<Result<BTreeMap<String, Centroid>, MarketRefused>> =
        LazyLock::new(|| parse_centroids(CENTROIDS));
    PARSED.as_ref().map_err(|_| CatalogError::Invalid)
}

/// The great-circle distance in miles between two points (haversine; INT-048, BIZ-058).
#[must_use]
pub fn miles_between(a: Centroid, b: Centroid) -> f64 {
    let (la, lb) = (a.lat.to_radians(), b.lat.to_radians());
    let dlat = lb - la;
    let dlon = (b.lon - a.lon).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la.cos() * lb.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_MILES * h.sqrt().min(1.0).asin()
}

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, CatalogError> {
    r.map_err(|_| CatalogError::Database)
}

/// Writes the markets, their towns and the centroid table in one transaction; safe to run again,
/// and `launched_at` is never written (INT-048).
///
/// # Errors
///
/// [`CatalogError::Database`].
pub async fn store(
    conn: &mut PgConnection,
    markets: &Markets,
    centroids: &BTreeMap<String, Centroid>,
) -> Result<(), CatalogError> {
    let mut tx = db(conn.begin().await)?;
    for m in &markets.markets {
        db(sqlx::query!(
            "INSERT INTO catalog.market (id, name, state, time_zone, identity_sources)
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name,
             state = EXCLUDED.state, time_zone = EXCLUDED.time_zone,
             identity_sources = EXCLUDED.identity_sources",
            m.id,
            m.name,
            m.state,
            m.time_zone,
            &m.identity_sources
        )
        .execute(&mut *tx)
        .await)?;
        for t in &m.towns {
            store_town(&mut tx, &m.id, t).await?;
        }
    }
    store_centroids(&mut tx, centroids).await?;
    db(tx.commit().await)
}

/// One town row, its market and ZIPs replaced by the file's.
async fn store_town(tx: &mut PgConnection, market: &str, t: &TownDef) -> Result<(), CatalogError> {
    db(sqlx::query!(
        "INSERT INTO catalog.market_town (code, market_id, name, zips, extra_zips)
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (code) DO UPDATE SET
         market_id = EXCLUDED.market_id, name = EXCLUDED.name, zips = EXCLUDED.zips,
         extra_zips = EXCLUDED.extra_zips",
        t.code,
        market,
        t.name,
        &t.zips,
        &t.extra_zips
    )
    .execute(&mut *tx)
    .await)
    .map(|_| ())
}

/// The centroid rows in one statement.
async fn store_centroids(
    tx: &mut PgConnection,
    centroids: &BTreeMap<String, Centroid>,
) -> Result<(), CatalogError> {
    let zips: Vec<String> = centroids.keys().cloned().collect();
    let lats: Vec<f64> = centroids.values().map(|c| c.lat).collect();
    let lons: Vec<f64> = centroids.values().map(|c| c.lon).collect();
    db(sqlx::query!(
        "INSERT INTO catalog.zcta_centroid (zcta, lat, lon)
         SELECT * FROM UNNEST($1::text[], $2::float8[], $3::float8[])
         ON CONFLICT (zcta) DO UPDATE SET lat = EXCLUDED.lat, lon = EXCLUDED.lon",
        &zips,
        &lats,
        &lons
    )
    .execute(&mut *tx)
    .await)
    .map(|_| ())
}
