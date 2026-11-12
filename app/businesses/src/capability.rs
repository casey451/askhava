//! Sign-up capability setup (BIZ-049, BIZ-004, BIZ-007, BIZ-010, CARD-001, LIC-043; D-393, D-492).
//!
//! [`checklist_offer`] builds the job-type checklist from the license map: every job type the
//! business's ROC classes receive (pre-ticked, LIC-043), then every no-license job type of an
//! enabled category as an unticked extra, grouped by trade (catalog category). Nothing else can be
//! ticked: [`save_capability`] stores the ticks through `service::set_job_types` with only the
//! offer allowed, so a licensed job type outside the license is refused (`NotCovered`). A
//! recurring-work "no" drops the recurring job types (a recurring category's job types that are
//! not one-time) before storing. The towns, the after-hours tick and the two answers matching
//! reads, the largest job ([`LargestJob`]) and recurring work, are stored in the same transaction
//! (`businesses.capability`, migration 0150); `quotes::capability_match` skips a card by them.
//!
//! T368 (D-2701, D-3101): [`holder_offer`] and [`offer_for`] build the checklist from everything
//! the business holds (ROC classes, agency licenses, pest attestation, handyman and city-list
//! standing) through `licensing::api::eligibility_for_request`, over the stored category switch
//! ([`stored_enabled`]), so the checklist and matching cannot disagree; [`reapply_license`] offers
//! only stored-on categories. [`checklist_offer`] (classes only, the file's flag) is kept for the
//! ROC-only callers and tests.

use std::collections::{BTreeMap, BTreeSet};

use askhava_catalog::api::{Catalog, enabled_categories};
use askhava_core::api::{NewTask, brand, enqueue};
use askhava_licensing::api::{
    CityListStanding, ClassMap, Eligibility, HolderParts, Job, JobRequest, LicenseKind,
    eligibility_for_request,
};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::holding::{Known, holding};
use crate::models::{self, BusinessesError, db};
use crate::rules::JobTypeChoice;
use crate::service::{Ticks, pause_for_rule, set_job_types, set_ticks, set_towns};
use crate::status::PauseReason;

/// The largest typed amount accepted, in cents ($10,000,000); a larger one does not parse
/// ([`LargestJob::parse`] gives `None`, the form's error), so "no limit" must be chosen.
pub const LARGEST_JOB_MAX_CENTS: u64 = 1_000_000_000;

/// BIZ-049: the largest job a business will quote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LargestJob {
    /// No limit.
    NoLimit,
    /// At most this many cents.
    Cents(u64),
}

impl LargestJob {
    /// BIZ-049: reads the form's choice (`none`, `1500`, `5000` or `typed`) and, for `typed`, a
    /// dollar amount such as "2,500" or "$2,500.00". `None` for a missing choice or a typed amount
    /// that is empty, zero, not money or over [`LARGEST_JOB_MAX_CENTS`].
    #[must_use]
    pub fn parse(choice: &str, typed: &str) -> Option<Self> {
        match choice {
            "none" => Some(Self::NoLimit),
            "1500" => Some(Self::Cents(150_000)),
            "5000" => Some(Self::Cents(500_000)),
            "typed" => dollars(typed)
                .filter(|c| (1..=LARGEST_JOB_MAX_CENTS).contains(c))
                .map(Self::Cents),
            _ => None,
        }
    }

    /// The limit in cents; `None` is no limit.
    #[must_use]
    pub const fn cents(self) -> Option<u64> {
        match self {
            Self::NoLimit => None,
            Self::Cents(c) => Some(c),
        }
    }

    /// CARD-001, BIZ-049: true when an amount of `cents` is above the limit, so a size or budget
    /// band whose floor is `cents` lies entirely above it (a floor equal to the limit is not).
    #[must_use]
    pub const fn below(self, cents: u64) -> bool {
        match self {
            Self::NoLimit => false,
            Self::Cents(limit) => cents > limit,
        }
    }

    /// How the answer reads back: "No limit" or "$1,500".
    #[must_use]
    pub fn line(self) -> String {
        self.cents()
            .map_or_else(|| "No limit".to_owned(), whole_dollars)
    }
}

/// "$1,500" for 150000 cents (cents dropped only when zero).
#[must_use]
pub fn whole_dollars(cents: u64) -> String {
    let digits = (cents / 100).to_string();
    let mut out = String::new();
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(d);
    }
    match cents % 100 {
        0 => format!("${out}"),
        c => format!("${out}.{c:02}"),
    }
}

/// A typed dollar amount in cents: digits with optional `$`, commas and two decimals.
fn dollars(text: &str) -> Option<u64> {
    let t: String = text.trim().trim_start_matches('$').replace(',', "");
    let (whole, part) = t.split_once('.').unwrap_or((&t, "00"));
    let ok = |s: &str, max: usize| {
        !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_digit())
    };
    if !ok(whole, 8) || !ok(part, 2) || part.len() != 2 {
        return None;
    }
    Some(whole.parse::<u64>().ok()? * 100 + part.parse::<u64>().ok()?)
}

/// The two capability answers matching reads (BIZ-049, CARD-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capability {
    /// The largest job the business will quote.
    pub largest: LargestJob,
    /// Whether it takes recurring work.
    pub recurring: bool,
}

/// One checklist row (BIZ-049).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferRow {
    /// The category and job type.
    pub choice: JobTypeChoice,
    /// The license covers it, so it starts ticked; false is a no-license extra, unticked.
    pub licensed: bool,
    /// A recurring job type, dropped by a recurring-work "no".
    pub recurring: bool,
}

/// One trade's rows, in catalog order (BIZ-049).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeOffer {
    /// The category code.
    pub category: String,
    /// The category's display name.
    pub name: String,
    /// Its rows.
    pub rows: Vec<OfferRow>,
}

/// REC-002, BIZ-049: a recurring category's job type that is not one of its one-time jobs.
fn recurring_in((recurring, one_time): (bool, &[String]), job_type: &str) -> bool {
    recurring && !one_time.iter().any(|t| t == job_type)
}

/// BIZ-049, CARD-001: whether `job_type` in `category` is recurring work.
#[must_use]
pub fn is_recurring(catalog: &Catalog, category: &str, job_type: &str) -> bool {
    catalog
        .categories
        .iter()
        .any(|c| c.id == category && recurring_in((c.recurring, &c.one_time_job_types), job_type))
}

/// BIZ-049, BIZ-004, LIC-043: the checklist for a business holding `classes` (empty after a
/// city-list match): the enabled categories' job types the license map gives the classes, then
/// the enabled no-license categories' job types as extras; a trade with neither is left out.
#[must_use]
pub fn checklist_offer(map: &ClassMap, catalog: &Catalog, classes: &[&str]) -> Vec<TradeOffer> {
    let covered = map.job_types(classes);
    let on: BTreeSet<&str> = catalog
        .categories
        .iter()
        .filter(|c| c.enabled)
        .map(|c| c.id.as_str())
        .collect();
    build_offer(
        catalog,
        |id| on.contains(id),
        |def, t| {
            let licensed = covered.contains(&(def.id.to_owned(), t.to_owned()));
            (licensed || def.kind == "none").then_some(licensed)
        },
    )
}

/// BIZ-049, BIZ-004, BIZ-021, LIC-002, LIC-003, LIC-012, LIC-014, LIC-015, LIC-016, LIC-019,
/// LIC-021, LIC-022, LIC-027, LIC-047 (D-2701, D-3101): the checklist for everything `parts`
/// holds (ROC classes, agency licenses, pest attestation, handyman and city-list standing), over
/// the categories in the stored switch's `enabled` set. A job type of a licensed category (kind
/// not `none`) is pre-ticked exactly when `licensing::api::eligibility` gives a `Card`; any job
/// type is an unticked extra when it gives a `HandymanCard` and the job type is not gas,
/// smoke-alarm, sprinkler or permit work; a no-license job type is an unticked extra unless it is
/// refused or needs a handyman the business is not (`NotHandyman`), read as if the city list had
/// matched, so a pending match neither hides nor adds a trade. Licensed trades come first.
#[must_use]
pub fn holder_offer(
    map: &ClassMap,
    catalog: &Catalog,
    parts: &HolderParts,
    enabled: &BTreeSet<String>,
) -> Vec<TradeOffer> {
    let on = |id: &str| enabled.contains(id);
    build_offer(catalog, on, |def, t| holder_row(map, parts, def, t))
}

/// One row of [`holder_offer`]: `Some(true)` pre-ticked, `Some(false)` an unticked extra, `None`
/// left out (an unknown license kind is left out too).
fn holder_row(map: &ClassMap, parts: &HolderParts, def: &Trade<'_>, t: &str) -> Option<bool> {
    let license = LicenseKind::parse(def.kind).ok()?;
    let job = Job {
        category: def.id.to_owned(),
        job_type: t.to_owned(),
        license,
        handyman_allowed: def.handyman_allowed,
        listed: true,
    };
    let mut holder = parts.for_job(map, def.id, t);
    let no_license = license == LicenseKind::None;
    if no_license {
        holder.city_list = CityListStanding::Matched;
    }
    let own: &[String] = &[];
    let request = JobRequest {
        text: "",
        candidates: own,
    };
    match eligibility_for_request(&job, &holder, &request) {
        Eligibility::Card => Some(!no_license),
        Eligibility::HandymanCard => Some(false),
        Eligibility::NoCard(_) => None,
    }
}

/// LIC-002, LIC-003, D-3101: the codes of the categories switched on in the stored catalog; a
/// category with no stored row is off (the rule intake's `catalog::api::is_enabled` follows).
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn stored_enabled(conn: &mut PgConnection) -> Result<BTreeSet<String>, BusinessesError> {
    let codes = enabled_categories(conn).await;
    Ok(codes
        .map_err(|_| BusinessesError::Database)?
        .into_iter()
        .collect())
}

/// BIZ-049, BIZ-004, LIC-027, D-3101: [`holder_offer`] for business `id`, its holder read once
/// through [`holding`] (with what the caller `known`s) and the categories through
/// [`stored_enabled`]; what the served sign-up and the portal show and pass to [`save_capability`].
///
/// # Errors
///
/// [`BusinessesError::UnknownBusiness`] or [`BusinessesError::Database`].
pub async fn offer_for(
    conn: &mut PgConnection,
    id: Uuid,
    known: Known<'_>,
    (map, catalog): (&ClassMap, &Catalog),
) -> Result<Vec<TradeOffer>, BusinessesError> {
    let parts = holding(conn, id, known).await?;
    let enabled = stored_enabled(conn).await?;
    Ok(holder_offer(map, catalog, &parts, &enabled))
}

/// What a checklist row needs of its category: the code, the license kind and the handyman flag.
struct Trade<'a> {
    id: &'a str,
    kind: &'a str,
    handyman_allowed: bool,
}

/// The trades of a checklist: per category `on` says is switched on, the job types `row` offers
/// (`Some(licensed)`), a trade with none left out, licensed trades first (BIZ-049).
fn build_offer(
    catalog: &Catalog,
    on: impl Fn(&str) -> bool,
    row: impl Fn(&Trade<'_>, &str) -> Option<bool>,
) -> Vec<TradeOffer> {
    let mut offer = Vec::new();
    for def in catalog.categories.iter().filter(|c| on(&c.id)) {
        let rule = (def.recurring, def.one_time_job_types.as_slice());
        let trade = Trade {
            id: &def.id,
            kind: &def.license.kind,
            handyman_allowed: def.handyman_allowed,
        };
        let rows: Vec<OfferRow> = def
            .job_types
            .iter()
            .filter_map(|t| {
                row(&trade, t).map(|licensed| OfferRow {
                    choice: JobTypeChoice::new(&def.id, t),
                    licensed,
                    recurring: recurring_in(rule, t),
                })
            })
            .collect();
        if !rows.is_empty() {
            offer.push(TradeOffer {
                category: def.id.clone(),
                name: def.name.clone(),
                rows,
            });
        }
    }
    offer.sort_by_key(|t| !t.rows.iter().any(|r| r.licensed));
    offer
}

/// BIZ-049: the rows that start ticked, every job type the license covers.
#[must_use]
pub fn pre_ticked(offer: &[TradeOffer]) -> Vec<JobTypeChoice> {
    let rows = offer.iter().flat_map(|t| &t.rows);
    rows.filter(|r| r.licensed)
        .map(|r| r.choice.clone())
        .collect()
}

/// Every answer of the capability step (BIZ-049).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityAnswers {
    /// The ticked job types.
    pub ticked: Vec<JobTypeChoice>,
    /// The town codes served (BIZ-010).
    pub towns: Vec<String>,
    /// The largest job and recurring work.
    pub capability: Capability,
    /// Takes after-hours emergencies (BIZ-007).
    pub after_hours: bool,
}

/// BIZ-049: the ticks to store: a recurring-work "no" drops the offer's recurring rows.
#[must_use]
pub fn kept_ticks(offer: &[TradeOffer], answers: &CapabilityAnswers) -> Vec<JobTypeChoice> {
    let recurring: BTreeSet<&JobTypeChoice> = offer
        .iter()
        .flat_map(|t| &t.rows)
        .filter(|r| r.recurring)
        .map(|r| &r.choice)
        .collect();
    let keep = |c: &&JobTypeChoice| answers.capability.recurring || !recurring.contains(c);
    answers.ticked.iter().filter(keep).cloned().collect()
}

/// BIZ-049, BIZ-004, BIZ-007, BIZ-010, LIC-043: stores the step's answers for business `id` in
/// one transaction with their audit rows: the ticks ([`kept_ticks`]) allowed only from `offer`,
/// the towns, the after-hours tick, then the largest job and recurring answer. Returns the stored
/// ticks.
///
/// # Errors
///
/// [`BusinessesError::NotCovered`] for a tick outside `offer` (nothing is stored), `BadCode`,
/// `UnknownBusiness`, `Database` or `Audit`.
pub async fn save_capability(
    conn: &mut PgConnection,
    id: Uuid,
    offer: &[TradeOffer],
    answers: &CapabilityAnswers,
    (actor, now): (Actor, Timestamp),
) -> Result<Vec<JobTypeChoice>, BusinessesError> {
    let allowed: BTreeSet<JobTypeChoice> = offer
        .iter()
        .flat_map(|t| t.rows.iter().map(|r| r.choice.clone()))
        .collect();
    let kept = kept_ticks(offer, answers);
    let towns: Vec<&str> = answers.towns.iter().map(String::as_str).collect();
    let mut tx = db(conn.begin().await)?;
    set_job_types(&mut tx, id, &kept, &allowed, actor).await?;
    set_towns(&mut tx, id, &towns, actor).await?;
    let b = models::read(&mut tx, id).await?;
    let ticks = Ticks {
        overflow_on: b.overflow_on,
        speaks_spanish: b.speaks_spanish,
        insurance: b.insurance,
        after_hours: answers.after_hours,
    };
    set_ticks(&mut tx, id, ticks, actor).await?;
    store(&mut tx, id, answers.capability, now).await?;
    let entry = AuditEntry {
        actor,
        action: "businesses.field_changed",
        subject_kind: "businesses.business",
        subject_id: id,
        change: Change::Field("capability"),
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| BusinessesError::Audit)?;
    db(tx.commit().await)?;
    Ok(kept)
}

/// Writes or replaces the business's capability row.
async fn store(
    tx: &mut PgConnection,
    id: Uuid,
    cap: Capability,
    now: Timestamp,
) -> Result<(), BusinessesError> {
    let cents = cap.largest.cents().and_then(|c| i64::try_from(c).ok());
    let q = sqlx::query!(
        "INSERT INTO businesses.capability (business_id, largest_job_cents, recurring, changed_at) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (business_id) DO UPDATE SET \
         largest_job_cents = EXCLUDED.largest_job_cents, recurring = EXCLUDED.recurring, \
         changed_at = EXCLUDED.changed_at",
        id,
        cents,
        cap.recurring,
        now.to_sqlx() as _
    );
    db(q.execute(tx).await).map(|_| ())
}

/// BIZ-049, CARD-001: the stored answers of each of `ids` that answered; a business with no row
/// is left out (matching then skips it for neither answer).
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn capabilities(
    conn: &mut PgConnection,
    ids: &[Uuid],
) -> Result<BTreeMap<Uuid, Capability>, BusinessesError> {
    let q = sqlx::query!(
        "SELECT business_id, largest_job_cents, recurring FROM businesses.capability \
         WHERE business_id = ANY($1)",
        ids
    );
    let rows = db(q.fetch_all(conn).await)?;
    let read = |c: Option<i64>| match c.map(u64::try_from) {
        None => Some(LargestJob::NoLimit),
        Some(Ok(c)) => Some(LargestJob::Cents(c)),
        Some(Err(_)) => None,
    };
    rows.into_iter()
        .map(|r| {
            let largest = read(r.largest_job_cents).ok_or(BusinessesError::Database)?;
            let cap = Capability {
                largest,
                recurring: r.recurring,
            };
            Ok((r.business_id, cap))
        })
        .collect()
}

/// BIZ-050: the email task queued when a daily license list changes a business's job types; its
/// payload holds `business`, `removed` and `offered` (each `[category, job type]` pairs),
/// `dropped_classes` (the reason) and `paused`. The sender renders [`license_change_words`].
pub const LICENSE_CHANGE_KIND: &str = "businesses.license_change";

/// BIZ-050: what re-applying the license map changed for one business.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reapplied {
    /// Ticked job types the old classes covered and the new classes no longer cover, removed.
    pub removed: Vec<JobTypeChoice>,
    /// Job types of a switched-on category only the new classes cover, offered, never ticked.
    pub offered: Vec<JobTypeChoice>,
    /// The removal left no job type ticked, so the business was paused (D-393).
    pub paused: bool,
}

/// BIZ-050, LIC-043, D-2595: the pure part of [`reapply_license`]: of `ticks`, the job types the
/// `old` classes covered that the `new` classes no longer cover (removed; a no-license or
/// agency-licensed job type, which no class covers, is never removed here), and the job types of
/// a switched-on category the `new` classes cover that the `old` did not and that are not ticked
/// (offered).
#[must_use]
pub fn license_change(
    map: &ClassMap,
    catalog: &Catalog,
    ticks: &[JobTypeChoice],
    classes: (&[&str], &[&str]),
) -> Reapplied {
    let on = |c: &str| catalog.categories.iter().any(|d| d.id == c && d.enabled);
    change_in(map, on, ticks, classes)
}

/// [`license_change`] with the switched-on test `on` (the file's flag, or the stored switch in
/// [`reapply_license`], LIC-002, D-3101).
fn change_in(
    map: &ClassMap,
    on: impl Fn(&str) -> bool,
    ticks: &[JobTypeChoice],
    (old, new): (&[&str], &[&str]),
) -> Reapplied {
    let key = |c: &JobTypeChoice| (c.category.clone(), c.job_type.clone());
    let (was, now) = (map.job_types(old), map.job_types(new));
    let lost: BTreeSet<_> = was.difference(&now).collect();
    let removed = ticks.iter().filter(|c| lost.contains(&key(c)));
    let ticked: BTreeSet<_> = ticks.iter().map(key).collect();
    let offered = now
        .difference(&was)
        .filter(|k| !ticked.contains(*k) && on(&k.0));
    Reapplied {
        removed: removed.cloned().collect(),
        offered: offered.map(|(c, t)| JobTypeChoice::new(c, t)).collect(),
        paused: false,
    }
}

/// BIZ-050, LIC-043, D-393: re-applies the license map to business `id` when the daily list of
/// `list` (its date) moves its classes from `old` to `new`, in one transaction: locks the business
/// row, removes the [`license_change`] job types with an audit row, pauses the business through
/// `pause_for_rule` (`NoCoveredJobType`) when a removal leaves nothing ticked and it was taking
/// cards, and queues one [`LICENSE_CHANGE_KIND`] email when anything was removed or offered (one
/// per business, class change and list date, D-2596). Nothing is ever ticked here. Only categories
/// of `catalog` switched on in the stored catalog are offered (LIC-002, D-3101).
///
/// # Errors
///
/// `UnknownBusiness`, `MovedOn`, `Database` or `Audit`.
pub async fn reapply_license(
    conn: &mut PgConnection,
    id: Uuid,
    (map, catalog): (&ClassMap, &Catalog),
    classes: (&[&str], &[&str]),
    (list, now): (Date, Timestamp),
) -> Result<Reapplied, BusinessesError> {
    let mut tx = db(conn.begin().await)?;
    let lock = sqlx::query!(
        "SELECT id FROM businesses.business WHERE id = $1 FOR UPDATE",
        id
    );
    db(lock.fetch_optional(&mut *tx).await)?.ok_or(BusinessesError::UnknownBusiness)?;
    let ticks = models::job_types(&mut tx, id).await?;
    let stored = stored_enabled(&mut tx).await?;
    let on = |c: &str| stored.contains(c) && catalog.categories.iter().any(|d| d.id == c);
    let mut change = change_in(map, on, &ticks, classes);
    if !change.removed.is_empty() {
        let kept: Vec<JobTypeChoice> = ticks
            .into_iter()
            .filter(|c| !change.removed.contains(c))
            .collect();
        models::replace_job_types(&mut tx, id, &kept).await?;
        let entry = AuditEntry {
            actor: Actor::System,
            action: "businesses.field_changed",
            subject_kind: "businesses.business",
            subject_id: id,
            change: Change::Field("job_types"),
        };
        let logged = audit(&mut tx, &entry).await;
        logged.map_err(|_| BusinessesError::Audit)?;
        change.paused = kept.is_empty() && pause_bare(&mut tx, id, now).await?;
    }
    if !change.removed.is_empty() || !change.offered.is_empty() {
        queue_notice(&mut tx, (id, list), &change, classes, now).await?;
    }
    db(tx.commit().await)?;
    Ok(change)
}

/// D-393, BIZ-050: pauses an active (or self-paused) business for `NoCoveredJobType`; one in any
/// other status (pending, already paused by a rule) is left as it is. True when it was paused.
async fn pause_bare(
    tx: &mut PgConnection,
    id: Uuid,
    now: Timestamp,
) -> Result<bool, BusinessesError> {
    let reason = PauseReason::NoCoveredJobType;
    match pause_for_rule(tx, id, reason, Actor::System, now).await {
        Ok(_) => Ok(true),
        Err(BusinessesError::Refused) => Ok(false),
        Err(e) => Err(e),
    }
}

/// LIC-006: a class as the license map reads it (single spaces, upper case), so `r-11 ` and
/// `R-11` are the same class.
fn class_norm(class: &str) -> String {
    let words: Vec<&str> = class.split_whitespace().collect();
    words.join(" ").to_ascii_uppercase()
}

/// BIZ-050, R13, D-2596: the email's dedupe key: the business, the list date and a SHA256
/// digest of the normalised, sorted old and new classes, so a retry at any later time queues
/// nothing new.
fn notice_key(id: Uuid, list: Date, (old, new): (&[&str], &[&str])) -> String {
    let set = |v: &[&str]| -> BTreeSet<String> { v.iter().map(|c| class_norm(c)).collect() };
    let mut hash = Sha256::new();
    for side in [set(old), set(new)] {
        hash.update((side.len() as u64).to_be_bytes());
        for class in side {
            hash.update((class.len() as u64).to_be_bytes());
            hash.update(class.as_bytes());
        }
    }
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    format!("{LICENSE_CHANGE_KIND}.{id}.{list}.{digest}")
}

/// BIZ-050: queues the owner's [`LICENSE_CHANGE_KIND`] email (no contact value in the payload).
async fn queue_notice(
    tx: &mut PgConnection,
    (id, list): (Uuid, Date),
    change: &Reapplied,
    (old, new): (&[&str], &[&str]),
    now: Timestamp,
) -> Result<(), BusinessesError> {
    let pairs = |v: &[JobTypeChoice]| -> Vec<[String; 2]> {
        v.iter()
            .map(|c| [c.category.clone(), c.job_type.clone()])
            .collect()
    };
    let kept: BTreeSet<String> = new.iter().map(|c| class_norm(c)).collect();
    let dropped: Vec<&str> = old
        .iter()
        .filter(|c| !kept.contains(&class_norm(c)))
        .copied()
        .collect();
    let payload = json!({
        "business": id,
        "removed": pairs(&change.removed),
        "offered": pairs(&change.offered),
        "dropped_classes": dropped,
        "paused": change.paused,
    });
    let mut task = NewTask::new(LICENSE_CHANGE_KIND, payload, now);
    task.dedupe_key = Some(notice_key(id, list, (old, new)));
    enqueue(tx, &task)
        .await
        .map(|_| ())
        .map_err(|_| BusinessesError::Database)
}

/// The lines of the [`LICENSE_CHANGE_KIND`] email in one language; `{c}` is the classes,
/// `{j}` the job types.
struct Lines {
    subject: &'static str,
    changed: &'static str,
    dropped: &'static str,
    removed: &'static str,
    paused: &'static str,
    offered: &'static str,
}

const EN: Lines = Lines {
    subject: "We changed your job types after a license update",
    changed: "Arizona's license list now shows a different class on your license",
    dropped: "Arizona's license list no longer shows {c} on your license",
    removed: ". So you won't get cards for these job types anymore: {j}. If the state's list is wrong, reply to this email.",
    paused: "That leaves no job types on your list, so we've paused your account. You won't get job cards while it's paused.",
    offered: "Your license now also covers: {j}. We didn't add these for you. To get those jobs, check them on the Job types page in your account.",
};

const ES: Lines = Lines {
    subject: "Cambiamos sus tipos de trabajo por un cambio en su licencia",
    changed: "La lista de licencias de Arizona ahora muestra otra clase en su licencia",
    dropped: "La lista de licencias de Arizona ya no muestra {c} en su licencia",
    removed: ". Por eso ya no recibirá tarjetas para estos tipos de trabajo: {j}. Si la lista del estado está mal, responda a este correo.",
    paused: "No le queda ningún tipo de trabajo, así que pausamos su cuenta. No recibirá tarjetas mientras esté pausada.",
    offered: "Su licencia ahora también cubre: {j}. No los agregamos por usted. Para recibir esos trabajos, márquelos en la página Tipos de trabajo de su cuenta.",
};

/// BIZ-050, BIZ-018: the owner's email for [`LICENSE_CHANGE_KIND`] as `[subject, body]`, in
/// Spanish when `spanish`. The body names the `removed` job types and why (`dropped`, the classes
/// the list no longer shows; empty when a class changed instead), says when the account was
/// paused, and names the `offered` job types without ticking them.
#[must_use]
pub fn license_change_words(spanish: bool, change: &Reapplied, dropped: &[&str]) -> [String; 2] {
    let (l, b) = if spanish {
        (&ES, brand().es().short_name().to_owned())
    } else {
        (&EN, brand().short_name().to_owned())
    };
    let names = |v: &[JobTypeChoice]| {
        let v: Vec<&str> = v.iter().map(|c| c.job_type.as_str()).collect();
        v.join(", ")
    };
    let mut body = Vec::new();
    if !change.removed.is_empty() {
        let why = match dropped.join(", ") {
            c if c.is_empty() => l.changed.to_owned(),
            c => l.dropped.replace("{c}", &c),
        };
        body.push(why + &l.removed.replace("{j}", &names(&change.removed)));
    }
    if change.paused {
        body.push(l.paused.to_owned());
    }
    if !change.offered.is_empty() {
        body.push(l.offered.replace("{j}", &names(&change.offered)));
    }
    [format!("{b}: {}", l.subject), body.join("\n\n")]
}
