//! The pilot scorecard's request, call and sign-up inputs on a fresh database (T478; OPS-039,
//! INT-047, BIZ-053, CALL-024; D-3753, D-3883): each reader counts its planted rows inside a
//! half-open window and none outside it, a `?g=oo` sign-up and a `?g=cp` web chat count by
//! source, a text request counts as "text", and the readers without a trade count only the job
//! IDs they are passed. Fake IDs, codes and times only; no contact detail is planted.

use askhava_businesses::api::{
    SignupRoutes, SignupSource, complaints_for_jobs, create, signup_routes, signups_by_source,
};
use askhava_calls::api::{CallsAnswered, calls_answered};
use askhava_intake::api::{
    CardApprovals, SourceCount, StopCount, cards_approved, intake_completion, requests_by_source,
};
use askhava_jobs::api::{TradeJobs, WeekRequests, jobs_by_trade, requests_per_week};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// Monday 00:00 Arizona time.
const FROM: &str = "2026-11-09T07:00:00Z";

fn from() -> Timestamp {
    FROM.parse().expect("instant")
}

fn hours(n: i64) -> SignedDuration {
    SignedDuration::from_hours(n)
}

/// Two Arizona weeks from [`FROM`].
fn window() -> (Timestamp, Timestamp) {
    (from(), from() + hours(14 * 24))
}

async fn job(db: &mut PgConnection, at: Timestamp, trade: Option<&str>) -> Uuid {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO jobs.job (status, created_at, status_at, timer_from) \
         VALUES ('open', $1, $1, $1) RETURNING id",
    )
    .bind(at.to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("job");
    if let Some(trade) = trade {
        sqlx::query(
            "INSERT INTO jobs.job_facts (job_id, seq, category, job_types, set_at) \
             VALUES ($1, 1, $2, ARRAY['fake_type'], $3)",
        )
        .bind(id)
        .bind(trade)
        .bind(at.to_sqlx())
        .execute(&mut *db)
        .await
        .expect("facts");
    }
    id
}

async fn request(db: &mut PgConnection, source: &str, at: Timestamp, job: Option<Uuid>) -> Uuid {
    let owner: Uuid = sqlx::query_scalar(
        "INSERT INTO intake.homeowner (contact_id, created_at) VALUES ($1, $2) RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(at.to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("homeowner");
    sqlx::query_scalar(
        "INSERT INTO intake.conversation (homeowner_id, job_id, language, first_inbox_id, \
         consent_template, consent_sent_at, opened_at, request_source) \
         VALUES ($1, $2, 'en', $3, 'consent_first', $4, $4, $5) RETURNING id",
    )
    .bind(owner)
    .bind(job)
    .bind(Uuid::new_v4())
    .bind(at.to_sqlx())
    .bind(source)
    .fetch_one(&mut *db)
    .await
    .expect("conversation")
}

async fn answer(
    db: &mut PgConnection,
    conversation: Uuid,
    (q, choice): (&str, &str),
    at: Timestamp,
) {
    sqlx::query(
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(conversation)
    .bind(q)
    .bind(choice)
    .bind(at.to_sqlx())
    .execute(&mut *db)
    .await
    .expect("answer");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ops_039_requests_per_week_and_jobs_by_trade(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = from();
    let a = job(&mut db, t, Some("plumbing")).await;
    let b = job(&mut db, t + hours(30), Some("plumbing")).await;
    let c = job(&mut db, t + hours(8 * 24), Some("electrical")).await;
    job(&mut db, t + hours(2), None).await;
    job(&mut db, t - SignedDuration::from_secs(1), Some("plumbing")).await;
    job(&mut db, window().1, Some("plumbing")).await;
    let weeks = requests_per_week(&mut db, window()).await.expect("weeks");
    let week = |w: i64, trade: Option<&str>, requests| WeekRequests {
        week: t + hours(w * 7 * 24),
        trade: trade.map(str::to_owned),
        requests,
    };
    assert_eq!(
        weeks,
        vec![
            week(0, Some("plumbing"), 2),
            week(0, None, 1),
            week(1, Some("electrical"), 1),
        ]
    );
    let trades = jobs_by_trade(&mut db, window()).await.expect("trades");
    let trade = |trade: &str, jobs: Vec<Uuid>| TradeJobs {
        trade: trade.to_owned(),
        jobs,
    };
    assert_eq!(
        trades,
        vec![trade("electrical", vec![c]), trade("plumbing", vec![a, b])]
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn int_047_requests_count_by_source_and_completion_by_step(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = from();
    let web_g = request(&mut db, "cp", t, None).await;
    let text = request(&mut db, "text", t + hours(1), None).await;
    request(&mut db, "web", t + hours(2), None).await;
    let card = request(&mut db, "text", t + hours(3), None).await;
    let urgent = request(&mut db, "text", t + hours(4), None).await;
    request(&mut db, "cp", t - hours(1), None).await;
    answer(&mut db, web_g, ("heard", "msg"), t).await;
    answer(&mut db, web_g, ("zip_asks", "one"), t + hours(1)).await;
    answer(&mut db, text, ("heard", "msg"), t + hours(1)).await;
    answer(&mut db, text, ("zip_asks", "one"), t + hours(2)).await;
    answer(&mut db, card, ("heard", "msg"), t + hours(3)).await;
    let shown = format!("card.{}", Uuid::new_v4().simple());
    answer(&mut db, card, (&shown, "shown.ab.1"), t + hours(4)).await;
    answer(
        &mut db,
        urgent,
        ("emergency_choice", "emergency"),
        t + hours(4),
    )
    .await;
    let sources = requests_by_source(&mut db, window())
        .await
        .expect("sources");
    let source = |source: &str, requests| SourceCount {
        source: source.to_owned(),
        requests,
    };
    assert_eq!(
        sources,
        vec![source("cp", 1), source("text", 3), source("web", 1)]
    );
    let done = intake_completion(&mut db, window())
        .await
        .expect("completion");
    assert_eq!((done.started, done.reached_card), (5, 2));
    let stop = |step: &str, requests| StopCount {
        step: step.to_owned(),
        requests,
    };
    assert_eq!(done.stopped_at, vec![stop("zip_asks", 2), stop("none", 1)]);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ops_039_cards_approved_without_an_edit_count_only_the_jobs_passed(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = from();
    let mut jobs = Vec::new();
    for (edit, at) in [(false, t), (true, t + hours(1)), (false, t - hours(1))] {
        let id = job(&mut db, at, Some("plumbing")).await;
        let conv = request(&mut db, "text", at, Some(id)).await;
        let q = format!("card.{}", id.simple());
        if edit {
            answer(&mut db, conv, (&q, "edit"), at).await;
        }
        answer(
            &mut db,
            conv,
            (&q, "sent.ab"),
            at + SignedDuration::from_secs(1),
        )
        .await;
        jobs.push(id);
    }
    let other = job(&mut db, t, Some("electrical")).await;
    let conv = request(&mut db, "text", t, Some(other)).await;
    let q = format!("card.{}", other.simple());
    answer(&mut db, conv, (&q, "sent.cd"), t + hours(1)).await;
    let got = cards_approved(&mut db, &jobs, window())
        .await
        .expect("cards");
    let want = CardApprovals {
        approved: 2,
        without_edit: 1,
    };
    assert_eq!(got, want);
}

async fn line(db: &mut PgConnection, job: Uuid, seq: u32) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, pool_number, shared, \
         assigned_at) VALUES ($1, $2, $3, $4, false, $5) RETURNING id",
    )
    .bind(job)
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(format!("+1928555{:04}", 100 + seq))
    .bind(from().to_sqlx())
    .fetch_one(db)
    .await
    .expect("line")
}

async fn call(db: &mut PgConnection, line: Uuid, answered_by: &str, ended: Timestamp) {
    sqlx::query(
        "INSERT INTO calls.call_log (line_id, call_sid, caller, answered_by, length_seconds, \
         reached_45s, ended_at) VALUES ($1, $2, 'business', $3, 60, true, $4)",
    )
    .bind(line)
    .bind(format!("CA{}", Uuid::new_v4().simple()))
    .bind(answered_by)
    .bind(ended.to_sqlx())
    .execute(db)
    .await
    .expect("call");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn call_024_counts_the_masked_calls_a_person_answered(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = from();
    let mine = job(&mut db, t, Some("plumbing")).await;
    let theirs = job(&mut db, t, Some("electrical")).await;
    let a = line(&mut db, mine, 1).await;
    let b = line(&mut db, theirs, 2).await;
    for (by, at) in [
        ("person", t),
        ("machine", t + hours(1)),
        ("nobody", t + hours(2)),
        ("person", t - hours(1)),
        ("person", window().1),
    ] {
        call(&mut db, a, by, at).await;
    }
    call(&mut db, b, "person", t).await;
    let got = calls_answered(&mut db, &[mine], window())
        .await
        .expect("calls");
    let want = CallsAnswered {
        calls: 3,
        answered: 1,
    };
    assert_eq!(got, want);
    let all = calls_answered(&mut db, &[mine, theirs], window())
        .await
        .expect("all");
    assert_eq!((all.calls, all.answered), (4, 2));
}

/// One sign-up: `(g code, heard from, identity step, under review, minutes to active)`.
type Signup<'a> = (
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    bool,
    Option<i64>,
);

async fn signup(db: &mut PgConnection, at: Timestamp, s: Signup<'_>) -> Uuid {
    let (g_code, heard, step, review, active) = s;
    let id = create(db, false, at).await.expect("business");
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, heard_from, g_code, \
         identity_step, review_since, submitted_at) \
         VALUES ($1, $2, $3, $4, $5, CASE WHEN $6 THEN $7 END, $7)",
    )
    .bind(id)
    .bind(Uuid::new_v4())
    .bind(heard)
    .bind(g_code)
    .bind(step)
    .bind(review)
    .bind(at.to_sqlx())
    .execute(&mut *db)
    .await
    .expect("signup");
    if let Some(minutes) = active {
        sqlx::query(
            "UPDATE businesses.business SET status = 'active', status_changed_at = $2 \
             WHERE id = $1",
        )
        .bind(id)
        .bind((at + SignedDuration::from_mins(minutes)).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("active");
    }
    id
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn biz_053_signups_count_by_source_and_route(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = from();
    signup(
        &mut db,
        t,
        (Some("oo"), None, Some("text"), false, Some(10)),
    )
    .await;
    signup(
        &mut db,
        t + hours(1),
        (Some("oo"), Some("search"), Some("letter"), false, Some(15)),
    )
    .await;
    signup(
        &mut db,
        t + hours(2),
        (None, Some("search"), Some("email"), true, Some(16)),
    )
    .await;
    signup(&mut db, t + hours(3), (None, None, None, false, None)).await;
    signup(
        &mut db,
        t - hours(1),
        (Some("oo"), None, Some("letter"), true, Some(1)),
    )
    .await;
    let sources = signups_by_source(&mut db, window(), None)
        .await
        .expect("sources");
    let source = |g: Option<&str>, heard: Option<&str>, signups| SignupSource {
        g_code: g.map(str::to_owned),
        heard_from: heard.map(str::to_owned),
        signups,
    };
    assert_eq!(
        sources,
        vec![
            source(Some("oo"), Some("search"), 1),
            source(Some("oo"), None, 1),
            source(None, Some("search"), 1),
            source(None, None, 1),
        ]
    );
    let routes = signup_routes(&mut db, window(), None)
        .await
        .expect("routes");
    let want = SignupRoutes {
        signups: 4,
        letter_or_review: 2,
        active_within_15_minutes_approx: 2,
    };
    assert_eq!(routes, want);
}

async fn set_trades(db: &mut PgConnection, business: Uuid, trades: &[&str]) {
    sqlx::query("UPDATE businesses.signup SET trades = $2 WHERE business_id = $1")
        .bind(business)
        .bind(trades)
        .execute(&mut *db)
        .await
        .expect("trades");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn biz_053_signups_split_by_their_stored_trades(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = from();
    let plumber = signup(&mut db, t, (Some("oo"), None, Some("letter"), false, None)).await;
    set_trades(&mut db, plumber, &["plumbing"]).await;
    let sparky = signup(
        &mut db,
        t + hours(1),
        (Some("oo"), None, None, false, Some(5)),
    )
    .await;
    set_trades(&mut db, sparky, &["electrical"]).await;
    let both = signup(&mut db, t + hours(2), (None, None, None, true, None)).await;
    set_trades(&mut db, both, &["plumbing", "electrical"]).await;
    let oo = |signups| SignupSource {
        g_code: Some("oo".to_owned()),
        heard_from: None,
        signups,
    };
    let none = SignupSource {
        g_code: None,
        heard_from: None,
        signups: 1,
    };
    for trade in ["plumbing", "electrical"] {
        let split = signups_by_source(&mut db, window(), Some(trade)).await;
        assert_eq!(split.expect("split"), vec![oo(1), none.clone()], "{trade}");
    }
    let total = signups_by_source(&mut db, window(), None)
        .await
        .expect("total");
    assert_eq!(total, vec![oo(2), none]);
    let roofing = signups_by_source(&mut db, window(), Some("roofing")).await;
    assert_eq!(roofing.expect("roofing"), vec![]);
    let routes = |signups, letter_or_review, active| SignupRoutes {
        signups,
        letter_or_review,
        active_within_15_minutes_approx: active,
    };
    let plumbing = signup_routes(&mut db, window(), Some("plumbing")).await;
    assert_eq!(plumbing.expect("plumbing routes"), routes(2, 2, 0));
    let electrical = signup_routes(&mut db, window(), Some("electrical")).await;
    assert_eq!(electrical.expect("electrical routes"), routes(2, 1, 1));
    let total = signup_routes(&mut db, window(), None).await;
    assert_eq!(total.expect("total routes"), routes(3, 2, 1));
}

async fn complaint(db: &mut PgConnection, business: Uuid, job: Uuid, at: Timestamp) {
    sqlx::query(
        "INSERT INTO businesses.complaint (business_id, job_id, kind, safety, received_at) \
         VALUES ($1, $2, 'no_show', false, $3)",
    )
    .bind(business)
    .bind(job)
    .bind(at.to_sqlx())
    .execute(db)
    .await
    .expect("complaint");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ops_039_complaints_count_only_the_jobs_passed_in_the_window(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = from();
    let business = create(&mut db, false, t).await.expect("business");
    let (mine, theirs) = (Uuid::new_v4(), Uuid::new_v4());
    for at in [t, t + hours(5), t - hours(1), window().1] {
        complaint(&mut db, business, mine, at).await;
    }
    complaint(&mut db, business, theirs, t).await;
    let count = complaints_for_jobs(&mut db, &[mine], window()).await;
    assert_eq!(count.expect("complaints"), 2);
    let none = complaints_for_jobs(&mut db, &[], window()).await;
    assert_eq!(none.expect("none"), 0);
}
