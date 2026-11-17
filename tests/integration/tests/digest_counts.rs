//! TD-OPS-09's digest readers on a fresh database (T605; OPS-030, OPS-041, BIZ-054; D-4202):
//! the BIZ-054 answers, the `license` pauses, the not-covered requests by trade and the no-shows
//! in a week, each planted just inside and just outside both window edges. Fake IDs, codes and
//! words only; no contact detail is planted, and none can come back.

use askhava_businesses::api::{
    AskedBy, LeaveReason, LeavingAnswer, ReasonAnswer, create, leaving_answers, license_paused,
    record_reason,
};
use askhava_intake::api::{NotCoveredCount, not_covered_by_trade};
use askhava_jobs::api::job_number;
use askhava_ops::api::Actor;
use askhava_picks::api::{NoShow, no_shows};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const FROM: &str = "2026-11-09T07:00:00Z";
const BOX_WORDS: &str = "Fake box words for the digest test";

fn from() -> Timestamp {
    FROM.parse().expect("instant")
}

fn to() -> Timestamp {
    from() + SignedDuration::from_hours(7 * 24)
}

/// The four edge times: at `from` and one second before `to` (inside), one second before `from`
/// and at `to` (outside).
fn edges() -> [Timestamp; 4] {
    let second = SignedDuration::from_secs(1);
    [from(), to() - second, from() - second, to()]
}

async fn business(db: &mut PgConnection) -> Uuid {
    create(db, false, from() - SignedDuration::from_hours(24 * 30))
        .await
        .expect("business")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_the_digest_lists_each_leaving_answer_in_its_week(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let [first, last, before, after] = edges();
    let fee = ReasonAnswer::parse("fee", "");
    let other = ReasonAnswer::parse("other", BOX_WORDS);
    let mut ids = Vec::new();
    let plants = [
        (AskedBy::Left, fee.as_ref(), first),
        (AskedBy::Paused, None, last),
        (
            AskedBy::Left,
            other.as_ref(),
            first + SignedDuration::from_hours(1),
        ),
        (AskedBy::Left, fee.as_ref(), before),
        (AskedBy::Paused, fee.as_ref(), after),
    ];
    for (by, answer, at) in plants {
        let id = business(&mut db).await;
        assert!(
            record_reason(&mut db, id, (by, answer), at)
                .await
                .expect("answer")
        );
        ids.push(id);
    }
    let got = leaving_answers(&mut db, (from(), to()))
        .await
        .expect("read");
    let answer = |business, asked_by, choice| LeavingAnswer {
        business,
        asked_by,
        choice,
    };
    assert_eq!(
        got,
        [
            answer(ids[0], AskedBy::Left, Some(LeaveReason::Fee)),
            answer(ids[2], AskedBy::Left, Some(LeaveReason::Other)),
            answer(ids[1], AskedBy::Paused, None),
        ],
        "inside both edges, the skipped answer as None, oldest first"
    );
    assert!(
        !format!("{got:?}").contains(BOX_WORDS),
        "never the box's words"
    );
}

async fn set_status(db: &mut PgConnection, id: Uuid, status: (&str, Option<&str>), at: Timestamp) {
    sqlx::query(
        "UPDATE businesses.business SET status = $2, pause_reason = $3, status_changed_at = $4 \
         WHERE id = $1",
    )
    .bind(id)
    .bind(status.0)
    .bind(status.1)
    .bind(at.to_sqlx())
    .execute(db)
    .await
    .expect("status");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_license_pauses_count_by_their_pause_time(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let inside = from() + SignedDuration::from_hours(1);
    let mut ids = Vec::new();
    let mut plants: Vec<_> = edges()
        .into_iter()
        .map(|at| (("paused", Some("license")), at))
        .collect();
    plants.push((("paused", Some("owner")), inside));
    plants.push((("active", None), inside));
    for (status, at) in plants {
        let id = business(&mut db).await;
        set_status(&mut db, id, status, at).await;
        ids.push(id);
    }
    let got = license_paused(&mut db, (from(), to())).await.expect("read");
    assert_eq!(got, [ids[0], ids[1]], "only license pauses inside the week");
}

async fn request(db: &mut PgConnection) -> Uuid {
    let home: Uuid = sqlx::query_scalar(
        "INSERT INTO intake.homeowner (contact_id, created_at) \
         VALUES (gen_random_uuid(), $1) RETURNING id",
    )
    .bind(from().to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("homeowner");
    sqlx::query_scalar(
        "INSERT INTO intake.conversation (homeowner_id, language, first_inbox_id, \
         consent_template, consent_sent_at, opened_at, closed_at) \
         VALUES ($1, 'en', gen_random_uuid(), 'consent_first', $2, $2, $2) RETURNING id",
    )
    .bind(home)
    .bind((from() - SignedDuration::from_hours(48)).to_sqlx())
    .fetch_one(db)
    .await
    .expect("request")
}

async fn demand(db: &mut PgConnection, conversation: Uuid, trade: &str, at: Timestamp) {
    sqlx::query(
        "INSERT INTO intake.recruiting_demand (conversation_id, town, trade, job_type, created_at) \
         VALUES ($1, 'lake_havasu_city', $2, NULL, $3)",
    )
    .bind(conversation)
    .bind(trade)
    .bind(at.to_sqlx())
    .execute(db)
    .await
    .expect("demand");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_the_digest_rolls_not_covered_requests_up_by_trade(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let [first, last, before, after] = edges();
    let twice = request(&mut db).await;
    demand(&mut db, twice, "plumbing", first).await;
    demand(
        &mut db,
        twice,
        "plumbing",
        first + SignedDuration::from_mins(1),
    )
    .await;
    let plants = [
        ("plumbing", last),
        ("electrical", first),
        ("plumbing", before),
        ("roofing", after),
    ];
    for (trade, at) in plants {
        let id = request(&mut db).await;
        demand(&mut db, id, trade, at).await;
    }
    let got = not_covered_by_trade(&mut db, (from(), to()))
        .await
        .expect("read");
    let count = |trade: &str, requests| NotCoveredCount {
        trade: trade.to_owned(),
        requests,
    };
    assert_eq!(
        got,
        [count("electrical", 1), count("plumbing", 2)],
        "each request once per trade, inside the week only"
    );
}

async fn ended_pick(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    status: &str,
    at: Timestamp,
) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at, \
         ended_at) VALUES ($1, $2, 1, 'page', $3, $4, $5) RETURNING id",
    )
    .bind(job)
    .bind(business)
    .bind(status)
    .bind((from() - SignedDuration::from_hours(24)).to_sqlx())
    .bind(at.to_sqlx())
    .fetch_one(db)
    .await
    .expect("pick")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_no_shows_count_by_their_end_time_with_job_numbers(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let who = business(&mut db).await;
    let mut plants: Vec<_> = edges().into_iter().map(|at| ("no_show", at)).collect();
    plants.push(("expired", from() + SignedDuration::from_hours(1)));
    let mut picks = Vec::new();
    for (status, at) in plants {
        let job = askhava_jobs::api::create(
            &mut db,
            from() - SignedDuration::from_hours(48),
            false,
            Actor::System,
        )
        .await
        .expect("job");
        let pick = ended_pick(&mut db, (job, who), status, at).await;
        picks.push((pick, job_number(&mut db, job).await.expect("number"), at));
    }
    let got = no_shows(&mut db, (from(), to())).await.expect("read");
    let no_show = |(pick, job_number, ended_at): (Uuid, i64, Timestamp)| NoShow {
        pick,
        job_number,
        business: who,
        ended_at,
    };
    assert_eq!(
        got,
        [no_show(picks[0]), no_show(picks[1])],
        "only no-shows inside the week, with their job numbers"
    );
}
