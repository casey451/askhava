//! What happens right after the ZIP and area check: a town outside the service area closes the
//! request early, a trade that is not open, a move that starts or ends outside Arizona and a home
//! watch request for rent collection or leasing all move it to Not covered (INT-016, INT-017,
//! INT-018, INT-022, LIC-017, LIC-018; rust brain R1, R5, R11).
//!
//! [`route`] is the pure decision; [`apply`] carries it out in one transaction: the request closes,
//! its jobs leave Intake (`ClosedEarly` or `NotCovered`), the one reply is queued, a request nobody
//! covers yet is logged as recruiting demand (OPS-041 reads it) and an audit row is written.
//!
//! T184 (D-1020): once the trade check passes, [`tell_coverage`] takes the `quotes::api::coverage`
//! count (as CARD-001 and CARD-004 would send the card then) and queues the INT-017 count line, the
//! INT-032 SPLIT offer, or moves the request to Not covered when no active business in the town
//! covers it ([`Why::NobodyCovers`], logged as recruiting demand). [`split_request`] opens the
//! second trade's job on a SPLIT or DIVIDIR reply, so each trade goes out as its own request.

use askhava_catalog::api::is_enabled;
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_messaging::api::{Language, NewText, Relation, enqueue, render};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use askhava_quotes::api::Coverage;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::conversation::{JobStart, Opened, begin_job};
use crate::models::{self, IntakeError, RecruitingDemandRow};
use crate::rules::fold;
use crate::service::{Queued, TextIn};

/// The INT-017 not-covered reply (`{trade}`).
pub const NOT_COVERED_TEMPLATE: &str = "not_covered";
/// The INT-017 count line for 2 or more businesses (`{count}`, `{trade}`).
pub const COUNT_MANY_TEMPLATE: &str = "coverage_count_many";
/// The INT-017 count line for 1 business (`{trade}`).
pub const COUNT_ONE_TEMPLATE: &str = "coverage_count_one";
/// The INT-032 SPLIT offer (DIVIDIR in Spanish).
pub const SPLIT_TEMPLATE: &str = "split_offer";
/// The reply to a request from a town outside the towns list (INT-022).
pub const OUT_OF_AREA_TEMPLATE: &str = "out_of_area";
/// The moving category a LIC-017 state check applies to.
const MOVING: &str = "local_moving";
/// The catalog's refused moving job type (`docs/catalog/categories.yaml`, LIC-017).
const MOVE_LEAVING_ARIZONA: &str = "move leaving Arizona";
/// The categories whose requests may not ask for rent collection or leasing (LIC-018).
const HOME_WATCH: [&str; 2] = ["home_watch", "turnover_cleaning"];

/// The catalog `property management` phrases, English and Spanish, folded (LIC-018, D-485): rent
/// collection, finding or screening tenants, showing the home to renters and leasing. Bare nouns
/// such as "renters" are not here: "clean after the renters check out" is turnover cleaning.
const PROPERTY_MANAGEMENT: [&str; 41] = [
    "collect rent",
    "collecting rent",
    "collect the rent",
    "rent collection",
    "find tenants",
    "find a tenant",
    "find renters",
    "find a renter",
    "screen tenants",
    "screening tenants",
    "tenant screening",
    "screen renters",
    "show the home to renters",
    "show the house to renters",
    "show it to renters",
    "showing the home to renters",
    "showing the house to renters",
    "show the home to tenants",
    "show the house to tenants",
    "lease it out",
    "lease the home",
    "lease the house",
    "leasing",
    "leased out",
    "rent it out",
    "renting out",
    "cobrar la renta",
    "cobrar renta",
    "cobro de renta",
    "cobrar el alquiler",
    "buscar inquilinos",
    "buscar un inquilino",
    "encontrar inquilinos",
    "encontrar un inquilino",
    "filtrar inquilinos",
    "mostrar la casa a inquilinos",
    "mostrar la casa a los inquilinos",
    "arrendar",
    "rentar la casa",
    "rentar mi casa",
    "alquilar la casa",
];

/// A town in the service-area list with its state (INT-022, D-382).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Town<'a> {
    /// The town's name.
    pub name: &'a str,
    /// Its two-letter state.
    pub state: &'a str,
}

/// A towns-list entry the settings check refuses.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TownRefused {
    /// The town is outside Arizona.
    #[error("{name}, {state} needs a new legal review (D-382)")]
    NotArizona {
        /// The town's name.
        name: String,
        /// Its state.
        state: String,
    },
}

/// The settings check for the towns list: Arizona towns only (INT-022, D-382).
///
/// # Errors
///
/// [`TownRefused::NotArizona`] for the first town whose state is not AZ.
pub fn check_towns(towns: &[Town<'_>]) -> Result<(), TownRefused> {
    match towns.iter().find(|t| !is_arizona(t.state)) {
        Some(t) => Err(TownRefused::NotArizona {
            name: t.name.to_owned(),
            state: t.state.to_owned(),
        }),
        None => Ok(()),
    }
}

fn is_arizona(state: &str) -> bool {
    state.trim().eq_ignore_ascii_case("AZ")
}

/// A move that starts or ends outside Arizona is refused (LIC-017, D-082).
#[must_use]
pub fn move_refused(from_state: &str, to_state: &str) -> bool {
    !(is_arizona(from_state) && is_arizona(to_state))
}

/// True when a home watch or turnover cleaning request asks for rent collection, finding or
/// screening tenants, showing the home to renters or leasing, in English or Spanish (LIC-018).
#[must_use]
pub fn asks_property_management(words: &str) -> bool {
    let folded = format!(" {} ", fold(words));
    PROPERTY_MANAGEMENT
        .iter()
        .any(|p| folded.contains(&format!(" {p} ")))
}

/// What the routing step needs to know about a request.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// The town code from the ZIP and area check.
    pub town: &'a str,
    /// The towns list: the town codes the service covers (INT-022).
    pub area: &'a [&'a str],
    /// The catalog category code of the request, such as `local_moving`.
    pub trade: &'a str,
    /// The category is on and open (LIC-002, LIC-003).
    pub trade_open: bool,
    /// The scrubbed words of the description and answers.
    pub words: &'a str,
    /// The states a move starts and ends in, for a moving request (LIC-017).
    pub move_states: Option<(&'a str, &'a str)>,
    /// The candidate job types found so far, such as `move leaving Arizona` (LIC-017).
    pub job_types: &'a [&'a str],
}

/// Why a request is not covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// No category is open for it (INT-017).
    TradeNotOpen,
    /// A move starts or ends outside Arizona (LIC-017).
    MoveOutsideArizona,
    /// A home watch request asks for rent collection or leasing (LIC-018).
    PropertyManagement,
    /// No active business in the town covers the job (INT-017, from `quotes::api::coverage`).
    NobodyCovers,
}

/// The routing decision (INT-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing here stops the request; the next question follows.
    Continue,
    /// The town is outside the towns list: Intake to Closed early (INT-022).
    OutOfArea,
    /// Intake to Not covered with the INT-017 reply.
    NotCovered(Why),
}

/// Decides the first outcome that applies, area check first (INT-016, INT-022, LIC-017,
/// LIC-018).
#[must_use]
pub fn route(req: &Request<'_>) -> Outcome {
    if !req.area.contains(&req.town) {
        return Outcome::OutOfArea;
    }
    if req.trade == MOVING && leaves_arizona(req) {
        return Outcome::NotCovered(Why::MoveOutsideArizona);
    }
    if HOME_WATCH.contains(&req.trade) && asks_property_management(req.words) {
        return Outcome::NotCovered(Why::PropertyManagement);
    }
    if !req.trade_open {
        return Outcome::NotCovered(Why::TradeNotOpen);
    }
    Outcome::Continue
}

/// Checks the codes `apply` writes: the town always, the trade only for Not covered (an
/// out-of-area request may have no trade yet).
fn check_request(req: &Request<'_>, outcome: Outcome) -> Result<(), IntakeError> {
    if matches!(outcome, Outcome::NotCovered(_)) {
        check_code(req.trade, IntakeError::BadTrade)?;
    }
    check_code(req.town, IntakeError::BadTown)
}

fn leaves_arizona(req: &Request<'_>) -> bool {
    req.job_types.contains(&MOVE_LEAVING_ARIZONA)
        || req.move_states.is_some_and(|(a, b)| move_refused(a, b))
}

/// The request, its homeowner message and the language to answer in.
#[derive(Debug, Clone, Copy)]
pub struct Routing<'a> {
    /// The request.
    pub conversation: Uuid,
    /// The message that triggered the check.
    pub msg: &'a TextIn<'a>,
    /// The language of the reply.
    pub language: Language,
    /// Spanish conversations are on (TXT-039).
    pub spanish_on: bool,
    /// The trade as the reply names it, such as `boat lift`.
    pub label: &'a str,
    /// The job type code, if known, for the recruiting-demand row.
    pub job_type: Option<&'a str>,
}

/// Whether category `trade` is on (INT-017, LIC-003).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn trade_is_open(db: &mut PgConnection, trade: &str) -> Result<bool, IntakeError> {
    is_enabled(db, trade)
        .await
        .map_err(|_| IntakeError::Database)
}

/// Carries out a non-[`Outcome::Continue`] outcome for `req` in one transaction, once: the request
/// closes, its jobs move to Closed early or Not covered, the reply is queued, a trade nobody covers
/// yet is logged as recruiting demand (a refused move or home watch request is not: recruiting
/// cannot fill it, D-1008) and an audit row is written (OPS-041). Returns the queued reply, or
/// `None` for [`Outcome::Continue`] and for a request that was already closed.
///
/// # Errors
///
/// [`IntakeError::BadTrade`], [`IntakeError::BadTown`], [`IntakeError::UnknownRequest`],
/// [`IntakeError::Job`], [`IntakeError::Template`], [`IntakeError::Outbox`],
/// [`IntakeError::Audit`] or [`IntakeError::Database`]; nothing is kept on an error.
pub async fn apply(
    db: &mut PgConnection,
    secret: &SecretString,
    (req, outcome): (&Request<'_>, Outcome),
    routing: &Routing<'_>,
) -> Result<Option<Queued>, IntakeError> {
    let (status, template, action) = match outcome {
        Outcome::Continue => return Ok(None),
        Outcome::OutOfArea => (
            JobStatus::ClosedEarly,
            OUT_OF_AREA_TEMPLATE,
            "intake.out_of_area",
        ),
        Outcome::NotCovered(_) => (
            JobStatus::NotCovered,
            NOT_COVERED_TEMPLATE,
            "intake.not_covered",
        ),
    };
    check_request(req, outcome)?;
    let at = routing.msg.received_at;
    if models::conversation_state(db, routing.conversation)
        .await?
        .is_none()
    {
        return Err(IntakeError::UnknownRequest);
    }
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let jobs = models::request_jobs(&mut tx, routing.conversation).await?;
    if !models::close_request(&mut tx, routing.conversation, at).await? {
        return Ok(None);
    }
    for job in jobs {
        leave_intake(&mut tx, job, status, at).await?;
    }
    if logs_demand(outcome) {
        let found = (req.town, req.trade, routing.job_type);
        models::insert_demand(&mut tx, routing.conversation, found, at).await?;
    }
    let queued = queue(&mut tx, secret, template, routing, None).await?;
    let entry = AuditEntry {
        actor: Actor::System,
        action,
        subject_kind: "intake.conversation",
        subject_id: routing.conversation,
        change: Change::Action,
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| IntakeError::Audit)?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(Some(queued))
}

/// INT-017, INT-032, CARD-001: tells the homeowner the coverage `quotes::api::coverage` counted
/// for the request's job: "We'll send this to the <N> <trade> businesses" (the singular for 1),
/// the SPLIT offer, or, when nobody covers it, [`apply`] with [`Why::NobodyCovers`]. A request
/// that was already closed gets nothing. Returns the queued reply.
///
/// # Errors
///
/// As [`apply`]; [`IntakeError::Template`] or [`IntakeError::Outbox`] for the count line.
pub async fn tell_coverage(
    db: &mut PgConnection,
    secret: &SecretString,
    (req, coverage): (&Request<'_>, Coverage),
    routing: &Routing<'_>,
) -> Result<Option<Queued>, IntakeError> {
    let (template, count) = match coverage {
        Coverage::NotCovered => {
            let outcome = Outcome::NotCovered(Why::NobodyCovers);
            return apply(db, secret, (req, outcome), routing).await;
        }
        Coverage::Split => (SPLIT_TEMPLATE, None),
        Coverage::Send(1) => (COUNT_ONE_TEMPLATE, None),
        Coverage::Send(n) => (COUNT_MANY_TEMPLATE, Some(n.to_string())),
    };
    if !open_request(db, routing.conversation).await? {
        return Ok(None);
    }
    queue(db, secret, template, routing, count.as_deref())
        .await
        .map(Some)
}

/// Whether request `conversation` is still open.
async fn open_request(db: &mut PgConnection, conversation: Uuid) -> Result<bool, IntakeError> {
    Ok(models::conversation_state(db, conversation)
        .await?
        .is_some_and(|(_, open)| open))
}

/// True when a reply is SPLIT or DIVIDIR alone (INT-032).
#[must_use]
pub fn is_split_reply(words: &str) -> bool {
    matches!(fold(words).as_str(), "split" | "dividir")
}

/// INT-032, D-1020: on a SPLIT or DIVIDIR reply, opens the job of the request's second trade
/// (`second.trade`, with the request's own description in `second.words`) through the one
/// job-opening path, so each trade is matched and sent as its own request; any other reply opens
/// nothing ([`Opened::Refused`]).
///
/// # Errors
///
/// As `conversation::begin_job`.
pub async fn split_request(
    db: &mut PgConnection,
    reply: &str,
    second: &JobStart<'_>,
) -> Result<Opened, IntakeError> {
    if !is_split_reply(reply) {
        return Ok(Opened::Refused);
    }
    begin_job(db, second).await
}

/// A request recruiting can fill is logged as demand: a trade not open, or nobody in the town
/// covering it (INT-017, OPS-041, D-1008).
fn logs_demand(outcome: Outcome) -> bool {
    matches!(
        outcome,
        Outcome::NotCovered(Why::TradeNotOpen | Why::NobodyCovers)
    )
}

/// The recruiting-demand rows of request `conversation` (INT-017, OPS-041).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn recruiting_demand(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Vec<RecruitingDemandRow>, IntakeError> {
    models::demand_rows(db, conversation).await
}

/// A code stored in `intake.recruiting_demand`: lower case letters, digits and `_`.
fn check_code(code: &str, error: IntakeError) -> Result<(), IntakeError> {
    let valid = !code.is_empty()
        && code.len() <= 64
        && code.starts_with(|c: char| c.is_ascii_lowercase())
        && code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if valid { Ok(()) } else { Err(error) }
}

/// Moves a job still in Intake (or Open) to `status`; a final or moved-on job is left alone.
async fn leave_intake(
    db: &mut PgConnection,
    job: Uuid,
    status: JobStatus,
    at: Timestamp,
) -> Result<(), IntakeError> {
    let from = current(db, job).await.map_err(|_| IntakeError::Job)?.status;
    if from.is_final() {
        return Ok(());
    }
    move_to(db, job, (from, status), at, Actor::System)
        .await
        .map(|_moved_or_moved_on| ())
        .map_err(|_| IntakeError::Job)
}

/// Renders and queues the reply of `template`, with `{count}` when given (TXT-011).
async fn queue(
    db: &mut PgConnection,
    secret: &SecretString,
    template: &'static str,
    routing: &Routing<'_>,
    count: Option<&str>,
) -> Result<Queued, IntakeError> {
    let mut values = vec![("trade", routing.label)];
    values.extend(count.map(|n| ("count", n)));
    let body = render(template, routing.language, routing.spanish_on, &values)
        .map_err(|_| IntakeError::Template)?
        .body;
    let subject = [routing.conversation, routing.msg.inbox_id];
    let text = NewText {
        template_id: template,
        subject: &subject,
        recipient: routing.msg.contact,
        relation: Relation::OpenRequest,
        body: &body,
    };
    let (outbox_id, _) = enqueue(db, secret, &text)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    Ok(Queued {
        outbox_id,
        template,
    })
}
