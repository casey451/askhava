//! The owner digest page (OPS-030, OPS-033, PRIV-006, SEC-005, D-5318, D-5319, T815). `GET`
//! [`OWNER_DIGEST_ROUTE`] shows T524's [`owner_digest_view`]: the last week's digest as counts and
//! job or business numbers, the monthly-review flags, the help requests due in the next 4 days
//! and the week's PROBLEM reply words, read under the owner session's proof only. The owner's
//! view marks the newest digest row opened (`ops::api::digest_opened`), so OPS-033's reminder
//! stops; a refused request marks nothing. Signed out, or any other realm, gets the owner door's
//! 401 with the `/owner/sign-in` link and nothing else. Every answer is `no-store`. No line holds
//! a homeowner contact detail or vault ID (PRIV-028).

use std::sync::Arc;

use askama::Template;
use askhava_core::api::{Clock, arizona_time};
use askhava_ops::api::{digest_opened, latest_digest};
use askhava_privacy::api::{OwnerSessionProof, SessionMint};
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware::from_fn;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum_extra::extract::CookieJar;
use jiff::Timestamp;
use sqlx::{PgConnection, PgPool};

use crate::digest_flags::{
    DigestInputs, DigestLine, OWNER_DIGEST_ROUTE, OwnerDigestView, OwnerFlags, owner_digest_view,
};
use crate::owner_auth::owner_principal;
use crate::owner_step_up_page::owner_signed_out;
use crate::security::no_store;
use crate::session::current_session;

/// The page's state: the pool, the clock, the session mint and the digest's settings.
#[derive(Clone)]
struct DigestRoute {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    mint: Option<&'static SessionMint>,
    inputs: DigestInputs<'static>,
}

/// One digest item as shown: its label, its count (or "not tracked yet") and its references.
struct ShownLine {
    item: &'static str,
    count: String,
    refs: String,
}

#[derive(Template)]
#[template(path = "owner_digest.html")]
struct DigestPage {
    week: String,
    lines: Vec<ShownLine>,
    flags: Vec<String>,
    due_soon: Vec<String>,
    problems: Vec<(i64, String)>,
}

/// OPS-030, OPS-033, SEC-005: the digest page's route behind the owner session and `no-store`;
/// `mint` is the process's session mint, without which no PROBLEM words are read or shown;
/// `inputs` are the flag settings and the test restore's audit actions (D-4932).
pub fn owner_digest_routes(
    pool: PgPool,
    clock: Arc<dyn Clock>,
    mint: Option<&'static SessionMint>,
    inputs: DigestInputs<'static>,
) -> Router {
    let state = DigestRoute {
        pool,
        clock,
        mint,
        inputs,
    };
    Router::new()
        .route(OWNER_DIGEST_ROUTE, get(show))
        .with_state(state)
        .layer(from_fn(no_store))
}

fn failed() -> Response {
    tracing::warn!("owner digest failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// SEC-005, SEC-006, PRIV-006: `Ok(Some(proof))` for the live owner (`Ok(None)` without a mint),
/// `Err` with the owner door's 401 for anyone else.
async fn owner_proof(
    route: &DigestRoute,
    db: &mut PgConnection,
    jar: &CookieJar,
    now: Timestamp,
) -> Result<Option<OwnerSessionProof>, Box<Response>> {
    match owner_principal(db, jar, now).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(Box::new(owner_signed_out())),
        Err(_) => return Err(Box::new(failed())),
    }
    let Some(mint) = route.mint else {
        return Ok(None);
    };
    let session = current_session(db, jar, now)
        .await
        .map_err(|_| Box::new(failed()))?;
    match session.and_then(|s| s.owner_proof(mint)) {
        Some(proof) => Ok(Some(proof)),
        None => Err(Box::new(owner_signed_out())),
    }
}

/// `GET` [`OWNER_DIGEST_ROUTE`] (OPS-030, OPS-033, D-5319): the owner's view, after which the
/// newest digest row is marked opened.
async fn show(State(route): State<DigestRoute>, jar: CookieJar) -> Response {
    let now = route.clock.now();
    let Ok(mut db) = route.pool.acquire().await else {
        return failed();
    };
    let proof = match owner_proof(&route, &mut db, &jar, now).await {
        Ok(proof) => proof,
        Err(refused) => return *refused,
    };
    let view = owner_digest_view(&mut db, proof.as_ref(), now, &route.inputs).await;
    let Ok(view) = view else {
        return failed();
    };
    let Ok(latest) = latest_digest(&mut db).await else {
        return failed();
    };
    if let Some(row) = latest
        && digest_opened(&mut db, row.id, now).await.is_err()
    {
        return failed();
    }
    page(view)
        .render()
        .map_or_else(|_| failed(), |html| Html(html).into_response())
}

/// OPS-030: the view as the page's lines; counts, job numbers and code names only.
fn page(view: OwnerDigestView) -> DigestPage {
    let day = |t: Timestamp| arizona_time(t).date().to_string();
    DigestPage {
        week: format!("{} to {}", day(view.digest.from), day(view.digest.to)),
        lines: view.digest.lines.into_iter().map(shown).collect(),
        flags: flag_lines(view.flags),
        due_soon: view.due_soon,
        problems: view
            .problem_words
            .into_iter()
            .map(|p| (p.number, p.words))
            .collect(),
    }
}

fn shown(line: DigestLine) -> ShownLine {
    ShownLine {
        item: line.item,
        count: line
            .count
            .map_or_else(|| "not tracked yet".to_owned(), |n| n.to_string()),
        refs: line.refs.join(", "),
    }
}

/// A count in tenths, as `4.5`.
fn tenths(n: i64) -> String {
    format!("{}.{}", n / 10, n % 10)
}

/// OPS-027, OPS-031, OPS-034: one plain line per flag; none acts on anything.
fn flag_lines(flags: OwnerFlags) -> Vec<String> {
    let mut out = Vec::new();
    for c in flags.crowded {
        let per_win = c.quotes_per_win_tenths.map_or_else(
            || "no jobs won yet".to_owned(),
            |q| format!("{} quotes for each job won", tenths(q)),
        );
        let median = tenths(i64::from(c.median_tenths));
        out.push(format!(
            "{} looks crowded: {} active businesses, {median} quotes per job and {per_win}",
            c.trade, c.active
        ));
    }
    for trade in flags.no_or_other {
        out.push(format!("Many {trade} jobs end with NO or OTHER."));
    }
    for (business, picks) in flags.lapsed {
        out.push(format!("Business {business} let {picks} picks lapse."));
    }
    for n in flags.repeat_requests {
        out.push(format!("A homeowner made {n} requests with no pick."));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{flag_lines, tenths};
    use crate::digest_flags::{CrowdedTrade, OwnerFlags};

    #[test]
    fn flags_read_as_plain_lines() {
        let flags = OwnerFlags {
            crowded: vec![CrowdedTrade {
                trade: "sample-trade".to_owned(),
                active: 6,
                median_tenths: 45,
                quotes_per_win_tenths: None,
            }],
            no_or_other: vec!["other-trade".to_owned()],
            lapsed: Vec::new(),
            repeat_requests: vec![3],
        };
        let lines = flag_lines(flags);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("4.5 quotes per job and no jobs won yet"));
        assert_eq!(lines[1], "Many other-trade jobs end with NO or OTHER.");
        assert_eq!(tenths(12), "1.2");
    }
}
