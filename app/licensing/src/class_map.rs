//! The license class map: which ROC classes and subclassifications receive which job types, read
//! from `docs/catalog/license_map.yaml` under one set of inheritance rules, the commercial-only stop
//! at sign-up, the R-61 and R-62 project limits and each category's `launch_open` flag (LIC-035,
//! LIC-043 to LIC-046, D-392).
//!
//! Pure, no database. Both catalog files are compiled in; [`ClassMap::shipped`] reads them once and
//! a map that does not load covers nothing (fail closed). `quotes` fills `Holder::roc_covers` from
//! [`ClassMap::covers`]; `businesses` calls [`ClassMap::sign_up`] with the classes the state record
//! shows; the quote form calls [`ClassMap::check_project_limit`]. The words a business reads on a
//! stop or a refusal are the portal's. Class names are catalog data, never a person's detail.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use serde::Deserialize;
use thiserror::Error;

use crate::handyman::QuotePrice;

/// The compiled-in license map (LIC-043).
const LICENSE_MAP: &str = include_str!("../../../docs/catalog/license_map.yaml");

/// The compiled-in category catalog: job types, refused job types and flags (LIC-001, LIC-046).
const CATEGORIES: &str = include_str!("../../../docs/catalog/categories.yaml");

/// Structural job types a `no_structural` class (R-62) never receives, whatever the map grants
/// (LIC-045, D-983): the handyman category's refused permit-type row and the structural work the
/// handyman trade notes name (`docs/catalog/trade_kb/trades/handyman.yaml`).
pub const STRUCTURAL_JOB_TYPES: [&str; 5] = [
    "wall removal",
    "wall removal or any permit-type job",
    "new walls",
    "structural framing",
    "patio covers",
];

/// One job type of one category: `(category id, job type)`.
pub type JobTypeKey = (String, String);

/// Why the license map or catalog could not be loaded. Names catalog entries only.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ClassMapError {
    /// A file is not the expected YAML shape.
    #[error("a catalog file does not parse")]
    Malformed,
    /// A class is listed twice (LIC-043).
    #[error("class {0} is listed twice")]
    DuplicateClass(String),
    /// A commercial-only class maps a job type or includes a class (LIC-044).
    #[error("commercial-only class {0} maps job types")]
    CommercialOnlyMapped(String),
    /// A class names a category the catalog lacks.
    #[error("class {0} names an unknown category")]
    UnknownCategory(String),
    /// A class names a job type its category does not list or refuse (LIC-043).
    #[error("class {0} names a job type outside its category")]
    UnknownJobType(String),
    /// A class includes a class the map lacks.
    #[error("class {0} includes an unknown class")]
    UnknownInclude(String),
    /// A chain of `includes` returns to a class already on it.
    #[error("class {0} includes itself")]
    IncludeCycle(String),
    /// A `project_limit` is not a dollar amount with cents.
    #[error("class {0} has a malformed project limit")]
    BadLimit(String),
    /// A launch-open category is not enabled (LIC-046).
    #[error("category {0} is launch-open but not enabled")]
    LaunchOpenNotEnabled(String),
}

/// What sign-up does with the ROC classes on a state record (LIC-044).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignUpOutcome {
    /// Activate; the job types these classes receive, for the checklist.
    Activate(BTreeSet<JobTypeKey>),
    /// Every class is commercial-only: the LIC-044 message, not activated.
    CommercialOnly,
    /// No class is mapped to any home job type (an unmapped or unreadable subclassification,
    /// LIC-035): not activated until the map lists it (D-984).
    NothingMapped,
}

/// Why a quote is refused under a class project limit (LIC-045).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectLimitRefusal {
    /// No class of the business covers the job type.
    NotCovered,
    /// Every qualifying class has a limit and the top of the price is above the highest of them;
    /// the portal explains the limit, in cents.
    OverProjectLimit {
        /// The limit applied, in cents.
        limit_cents: u64,
    },
}

/// A category's state at sign-up and for homeowner requests (LIC-046, BIZ-021).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opening {
    /// Enabled and launch-open: "Open now".
    OpenNow,
    /// Enabled, `launch_open: false`: "Coming later", waitlist sign-ups only.
    ComingLater,
    /// Not enabled or not in the catalog: never routed (LIC-003).
    Off,
}

/// What happens to a homeowner request in a category (LIC-046, OPS-006, INT-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestIntake {
    /// Launch-open with enough active businesses: the request is taken.
    Open,
    /// Launch-open but under the active-business setting: handled as INT-017 says.
    TooFewBusinesses,
    /// "Coming later": no request; business sign-ups go to the waitlist.
    WaitlistOnly,
    /// The category is off.
    NotRouted,
}

/// LIC-046, OPS-006: a launch-open category takes requests only with at least `min_active` active
/// businesses (the setting, default 2); fewer is INT-017's case.
#[must_use]
pub const fn request_intake(opening: Opening, active: u32, min_active: u32) -> RequestIntake {
    match opening {
        Opening::OpenNow if active >= min_active => RequestIntake::Open,
        Opening::OpenNow => RequestIntake::TooFewBusinesses,
        Opening::ComingLater => RequestIntake::WaitlistOnly,
        Opening::Off => RequestIntake::NotRouted,
    }
}

#[derive(Deserialize)]
struct MapFile {
    commercial_only: Vec<String>,
    classes: Vec<ClassRow>,
    #[serde(default)]
    scope_tags: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    #[serde(default)]
    unclear: Vec<UnclearRow>,
}

#[derive(Deserialize)]
struct ClassRow {
    class: String,
    #[serde(default)]
    scope: Option<Vec<String>>,
    #[serde(default)]
    job_types: BTreeMap<String, Grant>,
    #[serde(default)]
    includes: Vec<String>,
    #[serde(default)]
    project_limit: Option<String>,
    #[serde(default)]
    no_structural: bool,
}

#[derive(Deserialize)]
struct Grant {
    #[serde(default)]
    all: bool,
    #[serde(default)]
    also: Vec<String>,
    #[serde(default)]
    only: Vec<String>,
}

#[derive(Deserialize)]
struct UnclearRow {
    class: String,
    category: String,
    job_type: String,
}

#[derive(Deserialize)]
struct CategoryFile {
    categories: Vec<CategoryRow>,
}

#[derive(Deserialize)]
struct CategoryRow {
    id: String,
    enabled: bool,
    #[serde(default)]
    launch_open: bool,
    #[serde(default)]
    job_types: Vec<String>,
    #[serde(default)]
    refused: Vec<Refused>,
}

#[derive(Deserialize)]
struct Refused {
    job: String,
}

/// A category's listed and refused job types and its opening.
struct Category {
    listed: Vec<String>,
    refused: BTreeSet<String>,
    opening: Opening,
}

/// One class row after its own grants are read, before `includes` are followed.
struct Direct {
    grants: BTreeSet<JobTypeKey>,
    includes: Vec<String>,
    no_structural: bool,
}

/// The loaded map: each class's resolved job types and limit, the commercial-only list and each
/// category's opening. Class names are compared upper case with single spaces.
#[derive(Debug)]
pub struct ClassMap {
    covers: BTreeMap<String, BTreeSet<JobTypeKey>>,
    limits: BTreeMap<String, u64>,
    commercial_only: Vec<String>,
    openings: BTreeMap<String, Opening>,
}

static SHIPPED: LazyLock<Result<ClassMap, ClassMapError>> =
    LazyLock::new(|| ClassMap::load(LICENSE_MAP, CATEGORIES));

impl ClassMap {
    /// The map built from the compiled-in catalog files.
    ///
    /// # Errors
    ///
    /// The [`ClassMapError`] the shipped files fail with; callers then cover nothing.
    pub fn shipped() -> Result<&'static Self, ClassMapError> {
        SHIPPED.as_ref().map_err(Clone::clone)
    }

    /// LIC-043, LIC-035, LIC-045, LIC-046, D-392: reads a license map and a category catalog and
    /// resolves every class: its own grants (`all` the listed job types, `also` refused ones,
    /// `only` the named ones), kept to its `scope` tags for a subclassification, then the classes
    /// it includes; a row marked unclear is removed from the class and so never inherited; a
    /// `no_structural` class loses [`STRUCTURAL_JOB_TYPES`].
    ///
    /// # Errors
    ///
    /// A [`ClassMapError`] for a malformed file, a repeated class, a mapped commercial-only class,
    /// an unknown category, job type or include, an include cycle, a bad limit or a launch-open
    /// category that is not enabled.
    pub fn load(map: &str, categories: &str) -> Result<Self, ClassMapError> {
        let cats = read_categories(categories)?;
        let file: MapFile = serde_saphyr::from_str(map).map_err(|_| ClassMapError::Malformed)?;
        let commercial_only: Vec<String> = file.commercial_only.iter().map(|c| norm(c)).collect();
        let mut direct = BTreeMap::new();
        let mut limits = BTreeMap::new();
        for row in &file.classes {
            let name = norm(&row.class);
            if is_commercial(&commercial_only, &name)
                && (!row.job_types.is_empty() || !row.includes.is_empty())
            {
                return Err(ClassMapError::CommercialOnlyMapped(row.class.clone()));
            }
            if let Some(limit) = &row.project_limit {
                let cents =
                    cents(limit).ok_or_else(|| ClassMapError::BadLimit(row.class.clone()))?;
                limits.insert(name.clone(), cents);
            }
            let read = read_direct(row, &cats, &file.scope_tags)?;
            if direct.insert(name, read).is_some() {
                return Err(ClassMapError::DuplicateClass(row.class.clone()));
            }
        }
        let mut unclear: BTreeMap<String, BTreeSet<JobTypeKey>> = BTreeMap::new();
        for row in &file.unclear {
            let key = (row.category.clone(), row.job_type.clone());
            unclear.entry(norm(&row.class)).or_default().insert(key);
        }
        let mut covers = BTreeMap::new();
        for name in direct.keys() {
            resolve(name, &direct, &unclear, &mut covers, &mut Vec::new())?;
        }
        let openings = cats.into_iter().map(|(id, c)| (id, c.opening)).collect();
        Ok(Self {
            covers,
            limits,
            commercial_only,
            openings,
        })
    }

    /// LIC-043, LIC-035, CARD-001: whether this one class (or subclassification, as named on the
    /// state record) receives the job type. An unmapped class receives nothing.
    #[must_use]
    pub fn class_covers(&self, class: &str, category: &str, job_type: &str) -> bool {
        self.covers
            .get(&norm(class))
            .is_some_and(|set| set.contains(&(category.to_owned(), job_type.to_owned())))
    }

    /// LIC-043, CARD-001: whether any of a business's classes receives the job type; this is
    /// `Holder::roc_covers`.
    #[must_use]
    pub fn covers(&self, classes: &[&str], category: &str, job_type: &str) -> bool {
        classes
            .iter()
            .any(|class| self.class_covers(class, category, job_type))
    }

    /// LIC-043: every job type a business's classes receive.
    #[must_use]
    pub fn job_types(&self, classes: &[&str]) -> BTreeSet<JobTypeKey> {
        classes
            .iter()
            .filter_map(|class| self.covers.get(&norm(class)))
            .flatten()
            .cloned()
            .collect()
    }

    /// LIC-044, LIC-035, D-392, D-984: a record whose classes are all commercial-only (A, the A-
    /// classes, B-1, B-2, every C- class) is stopped; one whose classes reach no home job type is
    /// not activated; any other activates with the job types its classes receive.
    #[must_use]
    pub fn sign_up(&self, classes: &[&str]) -> SignUpOutcome {
        let all_commercial = !classes.is_empty()
            && classes
                .iter()
                .all(|class| is_commercial(&self.commercial_only, &norm(class)));
        if all_commercial {
            return SignUpOutcome::CommercialOnly;
        }
        let job_types = self.job_types(classes);
        if job_types.is_empty() {
            SignUpOutcome::NothingMapped
        } else {
            SignUpOutcome::Activate(job_types)
        }
    }

    /// LIC-045, D-392: a quote is refused when every class of the business that covers the job
    /// type has an R4-9-103 project limit (R-61 $50,000, R-62 $5,000) and the top of the price is
    /// above the highest of them; at the limit it is accepted. A trip-fee quote has no top.
    ///
    /// # Errors
    ///
    /// [`ProjectLimitRefusal::NotCovered`] when no class covers the job type, or
    /// [`ProjectLimitRefusal::OverProjectLimit`].
    pub fn check_project_limit(
        &self,
        classes: &[&str],
        category: &str,
        job_type: &str,
        price: QuotePrice,
    ) -> Result<(), ProjectLimitRefusal> {
        let qualifying: Vec<String> = classes
            .iter()
            .filter(|class| self.class_covers(class, category, job_type))
            .map(|class| norm(class))
            .collect();
        if qualifying.is_empty() {
            return Err(ProjectLimitRefusal::NotCovered);
        }
        let limits: Option<Vec<u64>> = qualifying
            .iter()
            .map(|class| self.limits.get(class).copied())
            .collect();
        let limit = limits.and_then(|all| all.into_iter().max());
        match (limit, price.top()) {
            (Some(limit_cents), Some(top)) if top > limit_cents => {
                Err(ProjectLimitRefusal::OverProjectLimit { limit_cents })
            }
            _ => Ok(()),
        }
    }

    /// LIC-046, BIZ-021: the category's opening from its `enabled` and `launch_open` flags.
    #[must_use]
    pub fn opening(&self, category: &str) -> Opening {
        self.openings.get(category).copied().unwrap_or(Opening::Off)
    }
}

/// Reads the category catalog; a launch-open category must be enabled (LIC-046).
fn read_categories(text: &str) -> Result<BTreeMap<String, Category>, ClassMapError> {
    let file: CategoryFile = serde_saphyr::from_str(text).map_err(|_| ClassMapError::Malformed)?;
    let mut cats = BTreeMap::new();
    for row in file.categories {
        let opening = match (row.enabled, row.launch_open) {
            (true, true) => Opening::OpenNow,
            (true, false) => Opening::ComingLater,
            (false, false) => Opening::Off,
            (false, true) => return Err(ClassMapError::LaunchOpenNotEnabled(row.id)),
        };
        let category = Category {
            listed: row.job_types,
            refused: row.refused.into_iter().map(|r| r.job).collect(),
            opening,
        };
        cats.insert(row.id, category);
    }
    Ok(cats)
}

/// LIC-043, LIC-035, D-504: a class row's own grants, each job type checked against its category
/// and, for a subclassification, kept only when one of its scope tags is in the row's scope.
fn read_direct(
    row: &ClassRow,
    cats: &BTreeMap<String, Category>,
    tags: &BTreeMap<String, BTreeMap<String, Vec<String>>>,
) -> Result<Direct, ClassMapError> {
    let mut grants = BTreeSet::new();
    for (cat_id, grant) in &row.job_types {
        let cat = cats
            .get(cat_id)
            .ok_or_else(|| ClassMapError::UnknownCategory(row.class.clone()))?;
        let named = grant.also.iter().chain(&grant.only);
        if named
            .clone()
            .any(|jt| !cat.listed.contains(jt) && !cat.refused.contains(jt))
        {
            return Err(ClassMapError::UnknownJobType(row.class.clone()));
        }
        let all = cat.listed.iter().filter(|_| grant.all);
        for job_type in all.chain(named) {
            let in_scope = row.scope.as_ref().is_none_or(|scope| {
                tags.get(cat_id)
                    .and_then(|by_job| by_job.get(job_type))
                    .is_some_and(|t| t.iter().any(|tag| scope.contains(tag)))
            });
            if in_scope {
                grants.insert((cat_id.clone(), job_type.clone()));
            }
        }
    }
    Ok(Direct {
        grants,
        includes: row.includes.iter().map(|c| norm(c)).collect(),
        no_structural: row.no_structural,
    })
}

/// LIC-043, LIC-045: a class's job types: its own grants and those of every class it includes,
/// less its unclear rows (so they are never inherited) and, for `no_structural`, structural work.
fn resolve(
    name: &str,
    direct: &BTreeMap<String, Direct>,
    unclear: &BTreeMap<String, BTreeSet<JobTypeKey>>,
    done: &mut BTreeMap<String, BTreeSet<JobTypeKey>>,
    path: &mut Vec<String>,
) -> Result<BTreeSet<JobTypeKey>, ClassMapError> {
    if let Some(set) = done.get(name) {
        return Ok(set.clone());
    }
    if path.iter().any(|on_path| on_path == name) {
        return Err(ClassMapError::IncludeCycle(name.to_owned()));
    }
    let row = direct
        .get(name)
        .ok_or_else(|| ClassMapError::UnknownInclude(path.last().cloned().unwrap_or_default()))?;
    path.push(name.to_owned());
    let mut set = row.grants.clone();
    for included in &row.includes {
        set.extend(resolve(included, direct, unclear, done, path)?);
    }
    path.pop();
    if let Some(rows) = unclear.get(name) {
        set.retain(|key| !rows.contains(key));
    }
    if row.no_structural {
        set.retain(|(_, job_type)| !STRUCTURAL_JOB_TYPES.contains(&job_type.as_str()));
    }
    done.insert(name.to_owned(), set.clone());
    Ok(set)
}

/// LIC-044: whether a class is commercial-only; a list entry ending in `-` (`C-`) is a prefix.
fn is_commercial(list: &[String], class: &str) -> bool {
    list.iter().any(|entry| {
        if entry.ends_with('-') {
            class.starts_with(entry.as_str())
        } else {
            entry == class
        }
    })
}

/// A class name upper case with single spaces, as the ROC list writes classes (LIC-006).
fn norm(class: &str) -> String {
    class
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
}

/// A `"50000.00"` dollar amount in cents; `None` unless it is digits, a point and two digits.
fn cents(amount: &str) -> Option<u64> {
    let (dollars, cents) = amount.split_once('.')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(dollars) || cents.len() != 2 || !digits(cents) {
        return None;
    }
    dollars
        .parse::<u64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(cents.parse().ok()?)
}
