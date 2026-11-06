//! The `rules_bridge` process: the app's pure rules for the digital twin, JSON lines in and out
//! (D-453, D-481, D-491, T074).
//!
//! One long-lived process. Each stdin line is one request `{"id": n, "call": name, "args": {...}}`;
//! each answer is one stdout line, `{"ok": {"id": n, "result": {...}}}` or
//! `{"error": {"id": n, "code": code}}`. The shapes are `app/server/rules_bridge.schema.json`
//! (hand-written), checked on both sides by `app/server/tests/rules_bridge_contract.rs` and
//! `tests/contract/test_rules_bridge.py`. The calls are the twin's view of the app's rules:
//!
//! - `fee`: `payments::api::period_fee_for`, the introduction fee by job size (FEE-001, FEE-003,
//!   FEE-004, REC-002, the 4-month rule);
//! - `quiet_hours`: `messaging::api::decide` for a non-urgent update text (QH-001 to QH-003, QH-006);
//! - `transition`: `jobs::api::rules::check_move`, the status arrows (JOB-002);
//! - `card` and `job_types`: `quotes::api::rules::card_for` over the shipped license map and catalog
//!   (CARD-001, LIC-043).
//!
//! It reads nothing but stdin and the files compiled in, writes nothing but stdout, holds no
//! database, key or personal data, and exits 0 when stdin closes.

use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};

use askhava_businesses::api::AccountStatus;
use askhava_catalog::api::{Catalog, parse};
use askhava_jobs::api::{JobStatus, rules::check_move};
use askhava_licensing::api::{CityListStanding, ClassMap, Holder, Job, LicenseKind};
use askhava_messaging::api::{AreaCodes, Channel, Decision, Kind, Outgoing, decide};
use askhava_payments::api::{
    FeeJob, FeeQuote, Period, PeriodJob, RecurringCap, Service, VisitTier, period_fee_for,
};
use askhava_quotes::api::{CardBusiness, CardDecision, CardJob, OfferTown, rules::card_for};
use jiff::Timestamp;
use serde_json::{Map, Value, json};
use sqlx::types::Uuid;

/// The catalog compiled into the build, the one `catalog` and `licensing` read (LIC-001).
const CATEGORIES: &str = include_str!("../../../../docs/catalog/categories.yaml");
/// The one offer town of a `card` call: the twin has already checked the business serves the job's
/// town, so the bridge judges licenses and ticks only.
const TOWN: &str = "lake_havasu_city";

/// Why a request got no result; the code is the only thing an error line carries.
#[derive(Debug, Clone, Copy)]
enum Code {
    /// The line is not JSON.
    BadJson,
    /// Not `{id, call, args}` with an unsigned `id` and an object `args`.
    BadRequest,
    /// `call` names no call.
    UnknownCall,
    /// The arguments are missing, extra or of the wrong kind.
    BadArgs,
    /// The app's rule refused the input (a fee error, a number never texted).
    Refused,
}

impl Code {
    /// The code as the schema lists it.
    const fn as_str(self) -> &'static str {
        match self {
            Self::BadJson => "bad_json",
            Self::BadRequest => "bad_request",
            Self::UnknownCall => "unknown_call",
            Self::BadArgs => "bad_args",
            Self::Refused => "refused",
        }
    }
}

/// One call's arguments.
type Args = Map<String, Value>;

/// The parsed tables every call shares, loaded once at start.
struct Rules {
    /// QH-003: the area-code table.
    area_codes: AreaCodes,
    /// LIC-043: the shipped license map.
    map: &'static ClassMap,
    /// LIC-001: the catalog.
    catalog: Catalog,
}

fn main() -> io::Result<()> {
    let rules = Rules {
        area_codes: AreaCodes::embedded().map_err(io::Error::other)?,
        map: ClassMap::shipped().map_err(|_| io::Error::other("license map"))?,
        catalog: parse(CATEGORIES).map_err(|_| io::Error::other("catalog"))?,
    };
    let mut out = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        writeln!(out, "{}", rules.answer(&line))?;
        out.flush()?;
    }
    Ok(())
}

impl Rules {
    /// One answer line for one request line.
    fn answer(&self, line: &str) -> Value {
        let Ok(request) = serde_json::from_str::<Value>(line) else {
            return json!({"error": {"id": null, "code": Code::BadJson.as_str()}});
        };
        let id = request
            .get("id")
            .filter(|id| id.is_u64())
            .cloned()
            .unwrap_or(Value::Null);
        match self.call(&request) {
            Ok(result) => json!({"ok": {"id": id, "result": result}}),
            Err(code) => json!({"error": {"id": id, "code": code.as_str()}}),
        }
    }

    /// Checks the envelope and runs the named call.
    fn call(&self, request: &Value) -> Result<Value, Code> {
        let fields = request.as_object().ok_or(Code::BadRequest)?;
        let shaped = fields.len() == 3 && fields.get("id").is_some_and(Value::is_u64);
        let args = fields.get("args").and_then(Value::as_object);
        let (true, Some(args)) = (shaped, args) else {
            return Err(Code::BadRequest);
        };
        match fields.get("call").and_then(Value::as_str) {
            Some("fee") => fee(args),
            Some("quiet_hours") => self.quiet_hours(args),
            Some("transition") => transition(args),
            Some("card") => self.card(args),
            Some("job_types") => self.job_types(args),
            _ => Err(Code::UnknownCall),
        }
    }

    /// QH-001 to QH-003, QH-006: whether a non-urgent update text to `phone` may go at `at`, and
    /// if not, the start of the next window in Arizona and the area code's zones.
    fn quiet_hours(&self, args: &Args) -> Result<Value, Code> {
        exact(args, &["phone", "at"])?;
        let now: Timestamp = text(args, "at")?.parse().map_err(|_| Code::BadArgs)?;
        let msg = Outgoing {
            phone: text(args, "phone")?,
            kind: Kind::Update,
            channel: Channel::Text,
            emergency_job: false,
            after_hours_business: false,
            emergency_emails: false,
        };
        Ok(
            match decide(&self.area_codes, &msg, now).map_err(|_| Code::Refused)? {
                Decision::Send => json!({"send": true, "until": null}),
                Decision::Hold { until } => json!({"send": false, "until": until.to_string()}),
            },
        )
    }

    /// CARD-001, LIC-043: whether an active, verified business in the job's town with these
    /// license classes (or a handyman on the city list) and these ticks receives the card.
    fn card(&self, args: &Args) -> Result<Value, Code> {
        exact(
            args,
            &["classes", "handyman", "ticked", "candidates", "text"],
        )?;
        let handyman = args
            .get("handyman")
            .and_then(Value::as_bool)
            .ok_or(Code::BadArgs)?;
        let candidates = pairs(args, "candidates")?
            .iter()
            .map(|(category, job_type)| self.job(category, job_type))
            .collect::<Result<Vec<_>, _>>()?;
        let business = CardBusiness {
            id: Uuid::nil(),
            status: AccountStatus::Active,
            verified: true,
            towns: BTreeSet::from([TOWN.to_owned()]),
            ticked: pairs(args, "ticked")?.into_iter().collect(),
            classes: strings(args, "classes")?,
            licenses: Holder {
                handyman,
                city_list: if handyman {
                    CityListStanding::Matched
                } else {
                    CityListStanding::Pending
                },
                ..Holder::default()
            },
        };
        let job = CardJob {
            towns: vec![OfferTown {
                code: TOWN.to_owned(),
                state: "AZ".to_owned(),
            }],
            candidates,
            text: text(args, "text")?.to_owned(),
        };
        let decision = card_for(self.map, &job, &business);
        Ok(json!({
            "card": !matches!(decision, CardDecision::Skip(_)),
            "handyman": matches!(decision, CardDecision::HandymanCard),
        }))
    }

    /// LIC-001: the listed job types of a catalog category, in catalog order.
    fn job_types(&self, args: &Args) -> Result<Value, Code> {
        exact(args, &["category"])?;
        let category = text(args, "category")?;
        let def = self
            .catalog
            .categories
            .iter()
            .find(|c| c.id == category)
            .ok_or(Code::BadArgs)?;
        Ok(json!({"job_types": def.job_types}))
    }

    /// One candidate job type as the license rules need it, from the catalog (LIC-001).
    fn job(&self, category: &str, job_type: &str) -> Result<Job, Code> {
        let def = self
            .catalog
            .categories
            .iter()
            .find(|c| c.id == category)
            .ok_or(Code::BadArgs)?;
        Ok(Job {
            category: category.to_owned(),
            job_type: job_type.to_owned(),
            license: LicenseKind::parse(&def.license.kind).map_err(|_| Code::BadArgs)?,
            handyman_allowed: def.handyman_allowed,
            listed: def.job_types.iter().any(|j| j == job_type),
        })
    }
}

/// FEE-001, FEE-003, FEE-004, REC-002, REC-004: the introduction fee by job size of one quote,
/// through `period_fee_for` with the default visit tier and recurring cap.
fn fee(args: &Args) -> Result<Value, Code> {
    exact(args, &["format", "amount", "high", "period", "service"])?;
    let amount = cents(args, "amount")?;
    let high = cents(args, "high")?;
    let format = text(args, "format")?;
    let quote = match format {
        "fixed" => FeeQuote::Fixed(amount),
        "range" => FeeQuote::Range { low: amount, high },
        "visit" => FeeQuote::DiagnosticVisit { visit_fee: amount },
        _ => return Err(Code::BadArgs),
    };
    let job = PeriodJob {
        job: FeeJob {
            diagnostic_visit: format == "visit",
            visit_tier: VisitTier::DEFAULT,
            fee_free: false,
        },
        service: service(text(args, "service")?)?,
        cap: RecurringCap::DEFAULT,
    };
    let fee =
        period_fee_for(quote, period(text(args, "period")?)?, job).map_err(|_| Code::Refused)?;
    Ok(json!({"cents": fee.cents(), "basis": fee.basis()}))
}

/// REC-004: a period name, or none.
fn period(name: &str) -> Result<Option<Period>, Code> {
    match name {
        "none" => Ok(None),
        "weekly" => Ok(Some(Period::Weekly)),
        "every_two_weeks" => Ok(Some(Period::EveryTwoWeeks)),
        "monthly" => Ok(Some(Period::Monthly)),
        "quarterly" => Ok(Some(Period::Quarterly)),
        "yearly" => Ok(Some(Period::Yearly)),
        _ => Err(Code::BadArgs),
    }
}

/// REC-001, REC-010: what the job's category and job type make of a period.
fn service(name: &str) -> Result<Service, Code> {
    match name {
        "not_recurring" => Ok(Service::NotRecurring),
        "one_time" => Ok(Service::OneTimeJobType),
        "recurring" => Ok(Service::RecurringJobType),
        _ => Err(Code::BadArgs),
    }
}

/// JOB-002: whether the arrows allow the move between two stored status codes.
fn transition(args: &Args) -> Result<Value, Code> {
    exact(args, &["from", "to"])?;
    let status = |key| JobStatus::from_code(text(args, key)?).ok_or(Code::BadArgs);
    Ok(json!({"allowed": check_move(status("from")?, status("to")?).is_ok()}))
}

/// The arguments hold exactly these keys.
fn exact(args: &Args, keys: &[&str]) -> Result<(), Code> {
    let same = args.len() == keys.len() && keys.iter().all(|k| args.contains_key(*k));
    if same { Ok(()) } else { Err(Code::BadArgs) }
}

/// A string argument.
fn text<'a>(args: &'a Args, key: &str) -> Result<&'a str, Code> {
    args.get(key).and_then(Value::as_str).ok_or(Code::BadArgs)
}

/// An amount in whole cents.
fn cents(args: &Args, key: &str) -> Result<u64, Code> {
    args.get(key).and_then(Value::as_u64).ok_or(Code::BadArgs)
}

/// A list of strings.
fn strings(args: &Args, key: &str) -> Result<Vec<String>, Code> {
    let list = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or(Code::BadArgs)?;
    list.iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or(Code::BadArgs))
        .collect()
}

/// A list of `{category, job_type}` objects.
fn pairs(args: &Args, key: &str) -> Result<Vec<(String, String)>, Code> {
    let list = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or(Code::BadArgs)?;
    list.iter()
        .map(|v| {
            let pair = v.as_object().ok_or(Code::BadArgs)?;
            exact(pair, &["category", "job_type"])?;
            Ok((
                text(pair, "category")?.to_owned(),
                text(pair, "job_type")?.to_owned(),
            ))
        })
        .collect()
}
