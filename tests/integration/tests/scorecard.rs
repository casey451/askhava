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
    plant_routing(db).await;
    plant_before_this_week(db).await;
}

/// The jobs rows of jobs 1 to 4 (this week) and 5 (`at(-408)`, Saturday Sep 26, before this
/// Monday and this month but inside the pilot); an expired pick on job 1 (no win); a $2.50 Stripe
/// fee on job 3; one complaint on job 3 against active business 12.
async fn plant_routing(db: &mut PgConnection) {
    let job = "INSERT INTO jobs.job (id, status, created_at, status_at, timer_from) \
        VALUES ($1, 'open', $4, $4, $4)";
    let facts = "INSERT INTO jobs.job_facts (job_id, seq, category, job_types, set_at) \
        VALUES ($1, 1, $3, '{fake_type}', $4)";
    for (n, trade, h) in [
        (1, "plumbing", 0),
        (2, "plumbing", 0),
        (3, "electrical", 0),
        (4, "pool", 0),
        (5, "plumbing", -408),
    ] {
        run(db, job, (id(n), id(0), trade, at(h), None)).await;
        run(db, facts, (id(n), id(0), trade, at(h), None)).await;
    }
    let pick = "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, \
        picked_at) VALUES ($1, $2, 1, 'page', $3, $4)";
    run(db, pick, (id(1), id(11), "expired", at(13), None)).await;
    let fee = "INSERT INTO payments.ledger_entry (kind, business_id, job_id, amount_cents, \
        idempotency_key, recorded_at) VALUES ('processing-fee', $2, $1, 250, $3, $4)";
    run(db, fee, (id(3), id(12), "t158.fee.3", at(7), None)).await;
    let business = "INSERT INTO businesses.business (id, status, created_at, status_changed_at) \
        VALUES ($2, $3, $4, $4)";
    run(db, business, (id(0), id(12), "active", at(-500), None)).await;
    let complaint = "INSERT INTO businesses.complaint (business_id, job_id, kind, safety, \
        received_at) VALUES ($2, $1, $3, false, $4)";
    run(db, complaint, (id(3), id(12), "late", at(10), None)).await;
}

/// Job 5 (plumbing, before this week): carded, quoted by business 11 eight hours later, picked
/// and charged $25; a web chat through `?g=cp` and a text request this week; a sign-up through
/// `?g=oo` answering "Facebook group" this week.
async fn plant_before_this_week(db: &mut PgConnection) {
    let card = "INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place, \
        photo_ids, category, candidates, emergency, recorded_at, sent_at) VALUES ($1, $1, \
        repeat('a', 32), 'Fake card.', 'Lake Havasu City', 'house', '{}', $3, '{}', false, $4, $4)";
    run(db, card, (id(5), id(0), "plumbing", at(-408), None)).await;
    let quote = "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
        confirm_on_site, notes, original, ai_read, confirmed, received_at, quote_minutes) \
        VALUES ($1, $2, $3, 20000, false, '', '', false, true, $4, $5)";
    run(db, quote, (id(5), id(11), "firm", at(-400), None)).await;
    let pick = "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, \
        picked_at) VALUES ($1, $2, 1, 'page', $3, $4)";
    run(db, pick, (id(5), id(11), "paid", at(-398), None)).await;
    let charge = "INSERT INTO payments.ledger_entry (kind, business_id, job_id, amount_cents, \
        idempotency_key, recorded_at) VALUES ('charge', $2, $1, 2500, $3, $4)";
    run(db, charge, (id(5), id(11), "t158.charge.5", at(-397), None)).await;
    let chat = "WITH h AS (INSERT INTO intake.homeowner (contact_id, created_at) VALUES ($1, $4) \
        RETURNING id) INSERT INTO intake.conversation (homeowner_id, language, first_inbox_id, \
        consent_template, consent_sent_at, opened_at, request_source) SELECT h.id, 'en', $2, \
        'fake_consent', $4, $4, $3 FROM h";
    run(db, chat, (id(901), id(902), "cp", at(1), None)).await;
    run(db, chat, (id(903), id(904), "text", at(2), None)).await;
    let signup = "INSERT INTO businesses.signup (business_id, mailbox_id, g_code, heard_from, \
        submitted_at) VALUES ($1, $2, $3, 'facebook_group', $4)";
    run(db, signup, (id(12), id(905), "oo", at(3), None)).await;
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
        // A Friday noon 76 days back: the pilot starts later in the week than the page is read
        // (Thursday), so a walk in 7-day steps from it would skip this week's minutes entry.
        pilot_start: now() - HOUR * (24 * 76),
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
    let entry = |m: &str| format!("{m} minutes (30 recruiting, 50 everything else)");
    let both = cell(&entry("40.0"), "amber") + &cell(&entry("26.7"), "amber");
    assert!(
        minutes.contains(&both),
        "the pilot reads this week's entry: {minutes}"
    );
    the_derived_figures_split_the_week_from_the_pilot(&html);
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

/// TD-OPS-15, D-468, D-4702: quotes per win (over confirmed picks), complaints per 20 jobs,
/// requests started per week, net per paid introduction and the by-source lists, each with a
/// different week and pilot figure (job 5 falls before this Monday).
fn the_derived_figures_split_the_week_from_the_pilot(html: &str) {
    let red = |t: &str| format!("<td>{t} <span class=\"sc-red\">(red)</span></td>");
    let green = |t: &str| format!("<td>{t} <span class=\"sc-green\">(green)</span></td>");
    let quoted = "Requests with 1 or more quotes within 24 hours";
    let expect = [
        // 4 quotes over 1 confirmed pick (the expired one is no win); pilot 5 over 2.
        (
            "All trades",
            "Quotes per win",
            "<td>4.0</td><td>2.5</td>".to_owned(),
        ),
        // 1 complaint times 20 over 4 jobs, then over 5.
        (
            "All trades",
            "Homeowner complaints per 20 jobs",
            red("5.0") + &red("4.0"),
        ),
        (
            "Trade: electrical",
            "Homeowner complaints per 20 jobs",
            red("20.0") + &red("20.0"),
        ),
        // 4 requests in 1 week; 5 requests over the pilot's 11 weeks.
        (
            "All trades",
            "Requests started per week",
            red("4.0") + &red("0.5"),
        ),
        // $150 less the 8.6% set-aside ($12.90) and $2.50 Stripe fee, over 2; pilot $175 over 3.
        (
            "All trades",
            "Net per paid introduction",
            green("$67.30") + &green("$52.48"),
        ),
        (
            "All trades",
            quoted,
            green("75.0% (3 of 4)") + &green("80.0% (4 of 5)"),
        ),
    ];
    for (scope, label, cells) in expect {
        let r = row(html, scope, label);
        assert!(r.contains(&cells), "{scope}, {label}: {r}");
    }
    for entry in ["cp: 1", "text: 1", "link oo, answer facebook_group: 1"] {
        assert!(
            html.contains(&format!("<li>{entry}</li>")),
            "INT-047, BIZ-053: {entry}"
        );
    }
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
