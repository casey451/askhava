//! The card pool: everything matching needs to send one job's cards, read at send time (T251;
//! CARD-001, CARD-002, CARD-004, CARD-016, BIZ-049, CON-020, LIC-043; D-1013, D-1014, D-1715,
//! D-1760, D-2708, D-2712, D-2943).
//!
//! [`card_pool`] builds the [`CardJob`] (the job's own town only, D-1014; the stored candidates,
//! each with its category's license rule from the stored catalog; the scrubbed description) and
//! the business pool for it: the businesses serving the town (`businesses::api::businesses_in_towns`)
//! whose status `receives_cards` and for which `may_get_cards` holds now, each with its towns,
//! ticked job types, overflow switch and licensing holder (`businesses::api::holding`, with a
//! no-license business's stored names as `Known.city_names`, so a city-list match can stand;
//! never an owner name), filtered through [`capable_pool`] by the job's band floor and recurring
//! flag; and the owner pauses (`ops::api::is_paused`: the "all" switch and the category's trade
//! switch). It never names another module's table.
//!
//! T249 (CARD-024, D-1519, D-1521): with the job's ZIP, [`card_pool`] also fills the [`Reach`]:
//! the job's centroid (`catalog::api::zip_location`) and, for every business serving some other
//! service-area town, its base ZIP centroid and radius (`businesses::api::service_area`); a
//! business whose radius reaches the job joins the pool by the same reads and skips. A job with no
//! ZIP or no centroid, or a business with no service area or a base with no centroid, is towns only.

use std::collections::BTreeSet;

use askhava_businesses::api::{
    BusinessesError, Known, business, business_names, businesses_in_towns, capabilities, holding,
    job_types, may_get_cards, service_area, towns,
};
use askhava_catalog::api::{Category, Centroid, category, service_area_towns, zip_location};
use askhava_licensing::api::{CityRecord, ClassMap, Job, LicenseKind};
use askhava_ops::api::{Switch, is_paused};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::capability_match::{JobFit, capable_pool};
use crate::job_text::CardFacts;
use crate::models::{
    BusinessReach, CardBusiness, CardJob, OfferTown, OwnerPauses, Pooled, QuotesError, Reach,
};

/// What one job's card send needs (CARD-001, CARD-016, CARD-020).
#[derive(Debug, Clone)]
pub struct CardPool {
    /// The shipped license map matching reads (LIC-043).
    pub map: &'static ClassMap,
    /// The job as matching reads it.
    pub job: CardJob,
    /// The businesses that may get the card, after the capability skips.
    pub businesses: Vec<Pooled>,
    /// The owner pauses in force.
    pub pauses: OwnerPauses,
    /// The job's location and the reach of each business in another town (CARD-024).
    pub reach: Reach,
    /// The category's display name, for the card notice's subject.
    pub category_name: String,
    /// The town's display name, for the card notice's subject.
    pub town_name: String,
    /// Every candidate job type is fee-free (FEE-045), for the card's fee line.
    pub fee_free: bool,
}

fn biz<T>(result: Result<T, BusinessesError>) -> Result<T, QuotesError> {
    result.map_err(|_| QuotesError::Business)
}

/// CARD-001, CARD-002, CARD-016, CARD-024, BIZ-049, D-2708: the pool for a job in `town` at `zip`
/// (none: towns only) with the stored card's `facts`, scrubbed `text` and the homeowner's band
/// floor (`None` when unanswered).
///
/// # Errors
/// [`QuotesError::Database`] when the catalog, the town or a pause cannot be read (an unknown
/// category or town is one too), [`QuotesError::LicenseMap`] when the shipped map does not load,
/// [`QuotesError::Business`] when a business cannot be read.
pub async fn card_pool(
    conn: &mut PgConnection,
    ((town, zip), facts): ((&str, Option<&str>), &CardFacts),
    text: String,
    band_floor_cents: Option<u64>,
) -> Result<CardPool, QuotesError> {
    let stored = category(conn, &facts.category)
        .await
        .map_err(|_| QuotesError::Database)?
        .ok_or(QuotesError::Database)?;
    let place = service_area_towns()
        .map_err(|_| QuotesError::Database)?
        .into_iter()
        .find(|t| t.code == town)
        .ok_or(QuotesError::Database)?;
    let fit = JobFit {
        band_floor_cents,
        recurring: facts.candidates.iter().any(|t| recurring(&stored, t)),
    };
    let candidates = candidate_jobs(&stored, &facts.candidates)?;
    let towns = [OfferTown {
        code: place.code.clone(),
        state: place.state.clone(),
    }];
    let job_at = match zip {
        Some(z) => zip_location(z).map_err(|_| QuotesError::Database)?,
        None => None,
    };
    let (ids, reach) = reached(conn, &place.code, job_at).await?;
    let businesses = pooled(conn, &ids, fit).await?;
    let pauses = owner_pauses(conn, &stored).await?;
    let fee_free = facts.candidates.iter().all(|t| fee_free(&stored, t));
    Ok(CardPool {
        map: ClassMap::shipped().map_err(|_| QuotesError::LicenseMap)?,
        job: CardJob {
            towns: towns.to_vec(),
            candidates,
            text,
        },
        businesses,
        pauses,
        reach,
        category_name: stored.name,
        town_name: place.name,
        fee_free,
    })
}

/// REC-002, BIZ-049: recurring work, as `businesses::api::is_recurring` reads the catalog file,
/// read here from the stored category: a recurring category's job type that is not one-time.
fn recurring(stored: &Category, job_type: &str) -> bool {
    stored.recurring
        && !stored
            .job_types
            .iter()
            .any(|t| t.name == job_type && t.one_time)
}

/// FEE-045: the stored job type is fee-free.
fn fee_free(stored: &Category, job_type: &str) -> bool {
    stored
        .job_types
        .iter()
        .any(|t| t.name == job_type && t.fee_free)
}

/// LIC-043: each candidate job type with its category's license rule.
fn candidate_jobs(stored: &Category, job_types: &[String]) -> Result<Vec<Job>, QuotesError> {
    let license = LicenseKind::parse(&stored.license_kind).map_err(|_| QuotesError::Database)?;
    Ok(job_types
        .iter()
        .map(|t| Job {
            category: stored.code.clone(),
            job_type: t.clone(),
            license,
            handyman_allowed: stored.handyman_allowed,
            listed: stored.job_types.iter().any(|s| &s.name == t),
        })
        .collect())
}

/// CARD-016, OPS-003: the "all" switch and the category's trade switch (scoped by the stored
/// category's record ID). No town switch is read: no town has a record ID to scope one (D-3181).
async fn owner_pauses(
    conn: &mut PgConnection,
    stored: &Category,
) -> Result<OwnerPauses, QuotesError> {
    let on = |r: Result<bool, _>| r.map_err(|_| QuotesError::Database);
    let all = on(is_paused(conn, Switch::All).await)?;
    let trade = on(is_paused(conn, Switch::Trade(stored.id)).await)?;
    Ok(OwnerPauses {
        all,
        towns: BTreeSet::new(),
        trades: trade.then(|| stored.code.clone()).into_iter().collect(),
    })
}

/// CARD-001, CARD-024, D-1519: the businesses serving `town`, then, for a job at `job_at`, every
/// business serving another service-area town whose radius reaches it, with its reach.
async fn reached(
    conn: &mut PgConnection,
    town: &str,
    job_at: Option<Centroid>,
) -> Result<(Vec<Uuid>, Reach), QuotesError> {
    let mut ids = biz(businesses_in_towns(conn, &[town.to_owned()]).await)?;
    let in_town: BTreeSet<Uuid> = ids.iter().copied().collect();
    let mut reach = Reach {
        job: job_at,
        ..Reach::default()
    };
    if job_at.is_none() {
        return Ok((ids, reach));
    }
    let others: Vec<String> = service_area_towns()
        .map_err(|_| QuotesError::Database)?
        .into_iter()
        .map(|t| t.code)
        .filter(|c| c != town)
        .collect();
    for id in biz(businesses_in_towns(conn, &others).await)? {
        if in_town.contains(&id) {
            continue;
        }
        let Some(base) = biz(service_area(conn, id).await)?.and_then(|a| {
            let radius_miles = a.area.radius_miles();
            a.location.map(|base| BusinessReach { base, radius_miles })
        }) else {
            continue;
        };
        reach.businesses.insert(id, base);
        if reach.reaches(id) {
            ids.push(id);
        }
    }
    Ok((ids, reach))
}

/// CARD-001, CON-020, BIZ-049: the businesses `ids` that may get cards now, less the capability
/// skips.
async fn pooled(
    conn: &mut PgConnection,
    ids: &[Uuid],
    fit: JobFit,
) -> Result<Vec<Pooled>, QuotesError> {
    let mut pool = Vec::new();
    for &id in ids {
        if let Some(one) = one_business(conn, id).await? {
            pool.push(one);
        }
    }
    let ids: Vec<Uuid> = pool.iter().map(|p| p.card.id).collect();
    let answers = biz(capabilities(conn, &ids).await)?;
    Ok(capable_pool(&pool, fit, &answers))
}

/// One business as matching reads it, or `None` when it may not get cards now (CARD-016,
/// CON-020, D-1715). An active business has passed its identity check; its license standing is
/// the holder's (D-3181).
async fn one_business(conn: &mut PgConnection, id: Uuid) -> Result<Option<Pooled>, QuotesError> {
    let record = biz(business(conn, id).await)?;
    let consented = may_get_cards(conn, id)
        .await
        .map_err(|_| QuotesError::Business)?;
    if !record.status.receives_cards() || !consented {
        return Ok(None);
    }
    let names = biz(business_names(conn, id).await)?.map(|n| CityRecord {
        business_name: n.business_name,
        trade_name: n.trade_name,
        owner_name: None,
        license_number: n.city_license_number,
    });
    let known = Known {
        roc_license: None,
        city_names: names.as_ref(),
    };
    let parts = biz(holding(conn, id, known).await)?;
    let ticked = biz(job_types(conn, id).await)?
        .into_iter()
        .map(|c| (c.category, c.job_type))
        .collect();
    let card = CardBusiness {
        id,
        status: record.status,
        verified: true,
        towns: biz(towns(conn, id).await)?.into_iter().collect(),
        ticked,
        classes: parts.classes,
        licenses: parts.holder,
    };
    Ok(Some(Pooled {
        card,
        overflow_on: record.overflow_on,
    }))
}
