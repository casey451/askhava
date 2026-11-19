//! The owner's pilot scorecard page (TD-OPS-15's page half, TD-CALL-10's scorecard half and
//! TD-OPS-21's scorecard half; OPS-039, CALL-024, OPS-046, OPS-047; D-396, D-458, D-4280, T158).
//! Every row is planted and fake: numbered job, business and pick IDs, fake pool numbers, no name
//! or address. The page shows each trade and the total for the week and the pilot to date, colors
//! a figure at its go line green, at its stop line amber and past it red, shows the masked calls a
//! person answered, feeds the weekly minutes form into minutes per paid introduction, shows T622's
//! panels and the ladder, and writes nothing (D-396).

use askhava_core::api::Settings;
use askhava_ops::api::{NotAFit, NotAFitReason, record_not_a_fit};
use askhava_portals::api::{
    CookieJar, MinutesForm, MinutesNotice, OwnerAuth, OwnerToolError, ScorecardInputs,
    ScorecardLines, SecondFactor, bootstrap_owner, owner_principal, owner_record_minutes,
    owner_scorecard_page, owner_sign_in,
};
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const HOUR: SignedDuration = SignedDuration::from_hours(1);
const PASSWORD: &str = "fake owner pass phrase 2026";

/// Fake Tuesday 08:00 Arizona time, plus `hours`; the page is read on Thursday at noon.
fn at(hours: i64) -> Timestamp {
    let base: Timestamp = "2026-10-13T15:00:00Z".parse().expect("time");
    base + HOUR * i32::try_from(hours).expect("hours")
}

fn now() -> Timestamp {
    at(52)
}

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

async fn run(
    db: &mut PgConnection,
    sql: &'static str,
    args: (Uuid, Uuid, &str, Timestamp, Option<i32>),
) {
    sqlx::query(sql)
        .bind(args.0)
        .bind(args.1)
        .bind(args.2)
        .bind(args.3.to_sqlx())
        .bind(args.4)
        .execute(db)
        .await
        .expect(sql);
}

/// Jobs 1 and 2 plumbing, 3 electrical, 4 pool, all carded at 0; first quotes at 12 hours (job 1,
/// none on 2), 4 and 5 hours (job 3) and 20 hours (job 4); job 3 picked and both 3 and 4 charged
/// $75.
async fn plant(db: &mut PgConnection) {
    let card = "INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place, \
        photo_ids, category, candidates, emergency, recorded_at, sent_at) VALUES ($1, $1, \
        repeat('a', 32), 'Fake card.', 'Lake Havasu City', 'house', '{}', $3, '{}', false, $4, $4)";
    for (job, trade) in [
        (1, "plumbing"),
        (2, "plumbing"),
        (3, "electrical"),
        (4, "pool"),
    ] {
        run(db, card, (id(job), id(0), trade, at(0), None)).await;
    }
    let quote = "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
        confirm_on_site, notes, original, ai_read, confirmed, received_at, quote_minutes) \
        VALUES ($1, $2, $3, 20000, false, '', '', false, true, $4, $5)";
    for (job, business, h, minutes) in [
        (1, 11, 12, None),
        (3, 12, 4, Some(30)),
        (3, 13, 5, None),
        (4, 14, 20, None),
    ] {
        run(db, quote, (id(job), id(business), "firm", at(h), minutes)).await;
    }
    let pick = "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, \
        picked_at) VALUES ($1, $2, 1, 'page', $3, $4)";
    run(db, pick, (id(3), id(12), "paid", at(6), None)).await;
    let charge = "INSERT INTO payments.ledger_entry (kind, business_id, job_id, amount_cents, \
        idempotency_key, recorded_at) VALUES ('charge', $2, $1, 7500, $3, $4)";
    run(db, charge, (id(3), id(12), "t158.charge.3", at(7), None)).await;
    run(db, charge, (id(4), id(14), "t158.charge.4", at(21), None)).await;
    plant_calls_and_panels(db).await;
}

/// Two masked calls on job 3, one answered by a person; a not-a-fit answer on job 3; one email
/// sent the day before the page is read.
async fn plant_calls_and_panels(db: &mut PgConnection) {
    let line: Uuid = sqlx::query_scalar(
        "INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, pool_number, shared, \
         assigned_at) VALUES ($1, $2, $3, '+19285550111', false, $4) RETURNING id",
    )
    .bind(id(3))
    .bind(id(900))
    .bind(id(12))
    .bind(at(6).to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("line");
    let call = "INSERT INTO calls.call_log (line_id, call_sid, caller, answered_by, \
        length_seconds, reached_45s, ended_at) VALUES ($1, 'CA' || md5($3), 'business', $3, 60, \
        true, $4)";
    run(db, call, (line, id(0), "person", at(8), None)).await;
    run(db, call, (line, id(0), "nobody", at(9), None)).await;
    let answer = NotAFit {
        job: id(3),
        business: id(13),
        trade: "electrical",
        reason: NotAFitReason::NotMyTown,
    };
    assert!(
        record_not_a_fit(db, &answer, at(5))
            .await
            .expect("not a fit")
    );
    sqlx::query(
        "INSERT INTO messaging.email_outbox (idempotency_key, template_id, recipient_id, \
         recipient_kind, subject_ids, class, status, not_before, provider_id, queued_at, sent_at) \
         VALUES (md5('t158') || md5('t158b'), 'fake_template', gen_random_uuid(), 'business', \
         '{}', 'notice', 'sent', $1, 'fake-t158-1', $1, $1)",
    )
    .bind(at(24).to_sqlx())
    .execute(db)
    .await
    .expect("email");
}

fn inputs() -> ScorecardInputs {
    let pairs = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
    ];
    let settings =
        Settings::from_pairs(pairs.map(|(k, v)| (k.to_owned(), v.to_owned()))).expect("settings");
    ScorecardInputs {
        lines: ScorecardLines::from_pairs(settings.scorecard_lines()).expect("lines"),
        set_aside_basis_points: settings.sales_tax_set_aside_basis_points(),
        pilot_start: now() - HOUR * (24 * 7 * 10),
        pushed: vec!["electrical".to_owned()],
    }
}

/// The table row of `label` under heading `scope`.
fn row<'a>(html: &'a str, scope: &str, label: &str) -> &'a str {
    let start = html.find(&format!("<h2>{scope}</h2>")).expect(scope);
    let table = &html[start..start + html[start..].find("</table>").expect("table")];
    let head = format!("<tr><th scope=\"row\">{label}</th>");
    table.lines().find(|l| l.starts_with(&head)).expect(label)
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `t`.
fn totp(secret: &[u8], t: Timestamp) -> String {
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(
        &u64::try_from(t.as_second() / 30)
            .expect("step")
            .to_be_bytes(),
    );
    let hash = mac.finalize().into_bytes();
    let o = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[o..o + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

/// The weekly minutes form, sent by a signed-in owner: a bad entry is refused, a good one saved.
async fn enter_minutes(db: &mut PgConnection) {
    bootstrap_owner(
        db,
        Some("fake-bootstrap-token-0123456789abcdef"),
        PASSWORD,
        now(),
    )
    .await
    .expect("bootstrap");
    let secret: Vec<u8> = sqlx::query_scalar("SELECT totp_secret FROM portals.owner_factor")
        .fetch_one(&mut *db)
        .await
        .expect("secret");
    let auth = OwnerAuth::new("laftly.test", "https://laftly.test").expect("relying party");
    let code = totp(&secret, now());
    let jar = owner_sign_in(
        db,
        CookieJar::new(),
        (PASSWORD, SecondFactor::Totp(&code)),
        &auth,
        now(),
    )
    .await
    .expect("in");
    let owner = owner_principal(db, &jar, now())
        .await
        .expect("read")
        .expect("live");
    let form = |r: &str, o: &str| MinutesForm {
        recruiting: r.to_owned(),
        other: o.to_owned(),
    };
    for bad in [form("ten", "5"), form("5", "-1"), form("10081", "0")] {
        assert_eq!(
            owner_record_minutes(db, &owner, &bad, now()).await,
            Err(OwnerToolError::Refused)
        );
    }
    owner_record_minutes(db, &owner, &form(" 30 ", "50"), now())
        .await
        .expect("saved");
}

async fn writes(db: &mut PgConnection) -> i64 {
    sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM ops.alert) + (SELECT count(*) FROM core.task) \
         + (SELECT count(*) FROM messaging.email_outbox) + (SELECT count(*) FROM messaging.outbox)",
    )
    .fetch_one(db)
    .await
    .expect("writes")
}

/// TD-OPS-15, TD-CALL-10, TD-OPS-21: every trade and the total, colors at the lines, the masked
/// calls answered, minutes per paid introduction from the form, the ladder and T622's panels.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_15_the_scorecard_shows_each_trade_colored_against_its_lines(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    plant(&mut db).await;
    enter_minutes(&mut db).await;
    let before = writes(&mut db).await;
    let html = owner_scorecard_page(&mut db, &inputs(), ("fake-csrf", None), now())
        .await
        .expect("page");
    assert_eq!(
        writes(&mut db).await,
        before,
        "the page decides nothing (D-396)"
    );
    for scope in [
        "All trades",
        "Trade: electrical",
        "Trade: plumbing",
        "Trade: pool",
    ] {
        assert!(html.contains(&format!("<h2>{scope}</h2>")), "{scope}");
    }
    let median = "Median time to the first quote";
    let cell = |text: &str, c: &str| format!("<td>{text} <span class=\"sc-{c}\">({c})</span></td>");
    let electrical = row(&html, "Trade: electrical", median);
    assert!(
        electrical.contains(&cell("4.0 hours", "green")),
        "at the go line: {electrical}"
    );
    assert!(
        row(&html, "Trade: plumbing", median).contains(&cell("12.0 hours", "amber")),
        "at the stop line"
    );
    assert!(
        row(&html, "Trade: pool", median).contains(&cell("20.0 hours", "red")),
        "past the stop line"
    );
    let quoted = "Requests with 1 or more quotes within 24 hours";
    assert!(row(&html, "Trade: plumbing", quoted).contains(&cell("50.0% (1 of 2)", "amber")));
    assert!(row(&html, "All trades", quoted).contains(&cell("75.0% (3 of 4)", "green")));
    let calls = row(&html, "Trade: electrical", "Masked calls a person answered");
    assert!(
        calls.contains("<td>50.0% (1 of 2)</td>"),
        "CALL-024, uncolored: {calls}"
    );
    let fee = row(&html, "All trades", "Average introduction fee by job size");
    assert!(fee.contains(&cell("$75.00", "green")), "{fee}");
    let minutes = row(&html, "All trades", "Your minutes per paid introduction");
    let entry = "40.0 minutes (30 recruiting, 50 everything else)";
    assert!(minutes.contains(&cell(entry, "amber")), "{minutes}");
    assert!(html.contains(
        "2 paid introductions (red), below the pilot target. Monthly targets: 15 for the pilot, 50 for $20,000 a year and 115 for $50,000 a year."
    ));
    assert!(
        html.contains("Email in the first six weeks") && html.contains("<td>1</td>"),
        "OPS-046"
    );
    assert!(
        html.contains("Median minutes a business spent on a quote: 30.0 minutes"),
        "OPS-047"
    );
    assert!(html.contains("Business 00000000-0000-0000-0000-00000000000c: 1.0 quotes per pick"));
    assert!(html.contains("\"Not a fit\" cards, not_my_town: 1"));
}

/// OPS-039: before any data each figure reads "No data yet" uncolored, and a refused minutes form
/// comes back with an error summary and the values as typed; a saved one says so.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_empty_scorecard_and_the_minutes_notices(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let typed = MinutesForm {
        recruiting: "ten".to_owned(),
        other: "5".to_owned(),
    };
    let refused = MinutesNotice::Refused(typed);
    let html = owner_scorecard_page(&mut db, &inputs(), ("fake-csrf", Some(&refused)), now())
        .await
        .expect("page");
    let median = row(&html, "All trades", "Median time to the first quote");
    assert!(
        median.contains("<td>No data yet</td><td>No data yet</td>"),
        "{median}"
    );
    assert!(
        html.contains("<title>Error: Pilot scorecard") && html.contains("id=\"error-summary\"")
    );
    assert!(html.contains("value=\"ten\"") && html.contains("value=\"fake-csrf\""));
    assert!(
        !html.contains("Email in the first six weeks"),
        "no panel before the first send"
    );
    let saved = owner_scorecard_page(
        &mut db,
        &inputs(),
        ("fake-csrf", Some(&MinutesNotice::Saved)),
        now(),
    )
    .await
    .expect("page");
    assert!(saved.contains("Your minutes for this week are saved."));
}
