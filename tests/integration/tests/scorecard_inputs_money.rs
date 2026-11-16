//! The pilot scorecard's money, pick and quote inputs (OPS-039, OPS-047, PICK-040, PICK-052,
//! FEE-046; D-458, D-468, D-3753, D-3844, D-3877, D-3920, D-3921), each read from planted fake
//! ledger, pick and quote rows: rows outside the window are left out, the free share leaves out a
//! business's first win, request-level inputs count only requests first carded in the window and
//! quotes received before its end, and an empty window gives `None`. A returned note writes one
//! audit row with no note text and is counted once. All IDs, amounts and notes are fake.

use askhava_core::api::{Settings, SettingsError};
use askhava_licensing::api::LicenseKind;
use askhava_ops::api::export_audit;
use askhava_payments::api::{
    LedgerTotals, Share, free_share, ledger_totals, paying_again, refund_reasons,
};
use askhava_picks::api::{confirmed_share, diagnostic_share, picked_requests};
use askhava_quotes::api::{
    Format, NoteProblem, Price, QuoteForm, QuoteRules, RequestQuotes, Saved, TradeJobs,
    active_businesses, check_note, diagnostic_quotes, jobs_by_trade, median_quote_minutes,
    notes_returned, parse_quote, quotes_per_business, record_note_returned, request_quotes,
    requests_with_quotes, save_quote,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const HOUR: i64 = 3_600;
const DAY: i64 = 24 * HOUR;

/// Fake day `d`, hour `h` (seconds after an arbitrary fake epoch).
fn at(d: i64, h: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + d * DAY + h * HOUR).expect("time")
}

/// The window: day 10 up to day 17.
const WINDOW: (i64, i64) = (10, 17);

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

/// One ledger entry of `kind` for `business` on `job`, `cents`, on day `d`.
async fn entry(
    db: &mut PgConnection,
    kind: &str,
    (business, job): (u128, u128),
    d: i64,
    cents: i64,
) {
    let key = format!("t477.{kind}.{business}.{job}.{d}");
    sqlx::query(
        "INSERT INTO payments.ledger_entry (kind, business_id, job_id, amount_cents, \
         idempotency_key, recorded_at) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(kind)
    .bind(id(business))
    .bind(id(job))
    .bind(cents)
    .bind(key)
    .bind(at(d, 9).to_sqlx())
    .execute(db)
    .await
    .expect("entry");
}

/// A charged $75 pick on `job` with its evidence, refunded for `trigger` on day `d`.
async fn refund(db: &mut PgConnection, pick: u128, job: u128, trigger: &str, d: i64) {
    let (pick, job, intent) = (id(pick), id(job), format!("pi_FakeT477{}", pick % 1_000));
    let charged = at(d - 1, 9).to_sqlx();
    sqlx::query(
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, \
         stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, $5)",
    )
    .bind(pick)
    .bind(id(1))
    .bind(job)
    .bind(&intent)
    .bind(charged)
    .execute(&mut *db)
    .await
    .expect("intent");
    sqlx::query(
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at, \
         decided_at) VALUES ($1, 1, $2, 'succeeded', $3, $3)",
    )
    .bind(pick)
    .bind(id(1))
    .bind(charged)
    .execute(&mut *db)
    .await
    .expect("attempt");
    sqlx::query(
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no, \
         idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version, \
         amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1, \
         'pick-' || $1::text || '-try1', 'cus_FakeT477', $4, 1, 7500, $5, $5)",
    )
    .bind(pick)
    .bind(id(1))
    .bind(job)
    .bind(&intent)
    .bind(charged)
    .execute(&mut *db)
    .await
    .expect("evidence");
    sqlx::query(
        "INSERT INTO payments.refund (pick_id, trigger, status, tries, requested_at) \
         VALUES ($1, $2, 'succeeded', 1, $3)",
    )
    .bind(pick)
    .bind(trigger)
    .bind(at(d, 9).to_sqlx())
    .execute(db)
    .await
    .expect("refund");
}

/// Ledger rows: business 1's first win is a charge on day 2; in the window it pays again on day
/// 11, gets a repeat-customer $0 on day 12, a Stripe fee, a refund and a dispute; business 2's
/// first win is free on day 11 and it pays $199 on day 12; business 3 pays after the window and
/// business 4 on another trade's job (51).
async fn plant_ledger(db: &mut PgConnection) {
    entry(db, "charge", (1, 10), 2, 7_500).await;
    entry(db, "charge", (1, 11), 11, 7_500).await;
    entry(db, "processing-fee", (1, 11), 11, 250).await;
    entry(db, "repeat-customer", (1, 12), 12, 0).await;
    entry(db, "refund", (1, 11), 13, 7_500).await;
    entry(db, "dispute", (1, 11), 14, 7_500).await;
    entry(db, "free-first-win", (2, 13), 11, 0).await;
    entry(db, "charge", (2, 14), 12, 19_900).await;
    entry(db, "charge", (3, 15), 20, 2_500).await;
    entry(db, "charge", (4, 51), 11, 7_500).await;
    refund(db, 90, 11, "unreachable", 13).await;
    refund(db, 91, 13, "cancelled-before-talk", 5).await;
    refund(db, 92, 51, "unreachable", 13).await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn money_inputs_from_planted_ledger_rows(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    plant_ledger(&mut db).await;
    let jobs: Vec<Uuid> = (10..=15).map(id).collect();
    let (from, to) = (at(WINDOW.0, 0), at(WINDOW.1, 0));
    let totals = ledger_totals(&mut db, &jobs, from, to)
        .await
        .expect("totals");
    let want = LedgerTotals {
        paid_introductions: 2,
        fees_cents: 27_400,
        stripe_fees_cents: 250,
        refunds: 1,
        refunded_cents: 7_500,
        disputes: 1,
    };
    assert_eq!(totals, Some(want));
    assert_eq!(want.average_fee_cents(), Some(13_700));
    // 27,400 - 2,357 (8.6%, rounded up) - 250 - 7,500 = 17,293 over 2.
    assert_eq!(want.net_per_introduction_cents(860), Some(8_646));
    assert_eq!(want.net_per_introduction_cents(0), Some(9_825));
    // Business 2's free first win is its first introduction, so it is left out (D-458).
    let free = free_share(&mut db, &jobs, from, to).await.expect("free");
    assert_eq!(free, Some(Share { part: 1, whole: 3 }));
    let again = paying_again(&mut db, &jobs, from, to).await.expect("again");
    assert_eq!(again, Some(Share { part: 1, whole: 2 }));
    let reasons = refund_reasons(&mut db, &jobs, from, to)
        .await
        .expect("reasons");
    assert_eq!(reasons, Some(vec![("unreachable".to_owned(), 1)]));
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_empty_money_window_gives_none(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    plant_ledger(&mut db).await;
    let jobs: Vec<Uuid> = (10..=15).map(id).collect();
    let (from, to) = (at(30, 0), at(37, 0));
    assert_eq!(ledger_totals(&mut db, &jobs, from, to).await, Ok(None));
    assert_eq!(free_share(&mut db, &jobs, from, to).await, Ok(None));
    assert_eq!(paying_again(&mut db, &jobs, from, to).await, Ok(None));
    assert_eq!(refund_reasons(&mut db, &jobs, from, to).await, Ok(None));
    let none = LedgerTotals {
        paid_introductions: 0,
        fees_cents: 0,
        stripe_fees_cents: 250,
        refunds: 0,
        refunded_cents: 0,
        disputes: 0,
    };
    assert_eq!(none.average_fee_cents(), None);
    assert_eq!(none.net_per_introduction_cents(860), None);
}

/// A job's sent card in `trade` on day `d` at 08:00, with a photo when `photo`.
async fn card(db: &mut PgConnection, job: u128, trade: &str, d: i64, photo: bool) {
    let photos: Vec<Uuid> = if photo { vec![id(700)] } else { vec![] };
    sqlx::query(
        "INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place, \
         photo_ids, category, candidates, emergency, recorded_at, sent_at) VALUES ($1, $1, \
         repeat('a', 32), 'Fake card.', 'Lake Havasu City', 'house', $2, $3, '{}', false, $4, $4)",
    )
    .bind(id(job))
    .bind(photos)
    .bind(trade)
    .bind(at(d, 8).to_sqlx())
    .execute(db)
    .await
    .expect("card");
}

/// A quote by `business` on `job` at `when`, a diagnostic visit when `minutes` is 30.
async fn quote(
    db: &mut PgConnection,
    (job, business): (u128, u128),
    when: Timestamp,
    minutes: Option<i32>,
) {
    let format = if minutes == Some(30) {
        "diagnostic_visit"
    } else {
        "firm"
    };
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at, quote_minutes) \
         VALUES ($1, $2, $3, 20000, false, '', '', false, true, $4, $5)",
    )
    .bind(id(job))
    .bind(id(business))
    .bind(format)
    .bind(when.to_sqlx())
    .bind(minutes)
    .execute(db)
    .await
    .expect("quote");
}

/// A pick of `business`'s quote on `job` on day `d`, in `status`.
async fn pick(db: &mut PgConnection, (job, business): (u128, u128), d: i64, status: &str) {
    sqlx::query(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', $3, $4)",
    )
    .bind(id(job))
    .bind(id(business))
    .bind(status)
    .bind(at(d, 12).to_sqlx())
    .execute(db)
    .await
    .expect("pick");
}

/// Plumbing jobs 21 (day 11, photo, quotes at 1, 5 and 30 hours), 22 (day 12, one at 3 hours),
/// 23 (carded on day 5, before the window, 2 quotes), 24 (carded on day 18, after it) and 26
/// (day 16, its one quote after the window ends); electrical job 25 (day 11, no quote).
async fn plant_quotes(db: &mut PgConnection) {
    for (job, trade, d, photo) in [
        (21, "plumbing", 11, true),
        (22, "plumbing", 12, false),
        (23, "plumbing", 5, false),
        (24, "plumbing", 18, false),
        (25, "electrical", 11, false),
        (26, "plumbing", 16, false),
    ] {
        card(db, job, trade, d, photo).await;
    }
    quote(db, (21, 1), at(11, 9), Some(30)).await;
    quote(db, (21, 2), at(11, 13), Some(50)).await;
    quote(db, (21, 3), at(12, 14), None).await;
    quote(db, (22, 1), at(12, 11), Some(20)).await;
    quote(db, (23, 1), at(5, 9), Some(40)).await;
    quote(db, (23, 4), at(5, 10), Some(40)).await;
    quote(db, (24, 5), at(18, 9), Some(10)).await;
    quote(db, (26, 5), at(17, 1), Some(10)).await;
    pick(db, (21, 1), 12, "paid").await;
    pick(db, (22, 1), 18, "expired").await;
    pick(db, (23, 1), 10, "expired").await;
    pick(db, (23, 4), 11, "free").await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn quote_and_pick_inputs_from_planted_rows(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    plant_quotes(&mut db).await;
    let (from, to) = (at(WINDOW.0, 0), at(WINDOW.1, 0));
    let trades = jobs_by_trade(&mut db, to).await.expect("trades");
    let plumbing = vec![id(21), id(22), id(23), id(26)];
    let want = vec![
        TradeJobs {
            trade: "electrical".to_owned(),
            jobs: vec![id(25)],
        },
        TradeJobs {
            trade: "plumbing".to_owned(),
            jobs: plumbing.clone(),
        },
    ];
    assert_eq!(trades, want, "job 24, carded after the window, is left out");
    let got = request_quotes(&mut db, &plumbing, from, to)
        .await
        .expect("requests");
    let want = RequestQuotes {
        requests: 3,
        within_24_hours: [2, 1, 0],
        median_first_quote: Some(SignedDuration::from_hours(2)),
        p75_first_quote: Some(SignedDuration::from_mins(150)),
        with_photo: 1,
    };
    assert_eq!(got, Some(want));
    let one = requests_with_quotes(&mut db, &plumbing, (from, to), 1)
        .await
        .expect("one");
    assert_eq!(
        one,
        vec![id(21), id(22)],
        "job 23 was carded before the window"
    );
    let two = requests_with_quotes(&mut db, &plumbing, (from, to), 2)
        .await
        .expect("two");
    assert_eq!(two, vec![id(21)]);
    let per = quotes_per_business(&mut db, &plumbing, from, to)
        .await
        .expect("per");
    assert_eq!(per, Some(vec![(id(1), 2), (id(2), 1), (id(3), 1)]));
    let active = active_businesses(&mut db, &plumbing, to)
        .await
        .expect("active");
    assert_eq!(active, Some(4), "business 5 quoted only after the window");
    let minutes = median_quote_minutes(&mut db, &plumbing, from, to)
        .await
        .expect("minutes");
    assert_eq!(minutes, Some(SignedDuration::from_mins(30)));
    let diagnostic = diagnostic_quotes(&mut db, &plumbing)
        .await
        .expect("diagnostic");
    assert_eq!(diagnostic, vec![(id(21), id(1))]);
    let picked = picked_requests(&mut db, &one, to).await.expect("picked");
    assert_eq!(
        picked,
        Some(Share { part: 1, whole: 2 }),
        "job 22's pick came after the window"
    );
    let picked = picked_requests(&mut db, &two, to).await.expect("picked");
    assert_eq!(picked, Some(Share { part: 1, whole: 1 }));
    let confirmed = confirmed_share(&mut db, &plumbing, from, to)
        .await
        .expect("confirmed");
    assert_eq!(confirmed, Some(Share { part: 2, whole: 3 }));
    let share = diagnostic_share(&mut db, &plumbing, (from, to), &diagnostic).await;
    assert_eq!(share.expect("share"), Some(Share { part: 1, whole: 3 }));
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_empty_quote_window_gives_none(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    plant_quotes(&mut db).await;
    let plumbing = vec![id(21), id(22), id(23), id(26)];
    let (from, to) = (at(60, 0), at(67, 0));
    assert_eq!(request_quotes(&mut db, &plumbing, from, to).await, Ok(None));
    assert_eq!(
        requests_with_quotes(&mut db, &plumbing, (from, to), 1).await,
        Ok(vec![])
    );
    assert_eq!(
        quotes_per_business(&mut db, &plumbing, from, to).await,
        Ok(None)
    );
    assert_eq!(active_businesses(&mut db, &plumbing, to).await, Ok(None));
    assert_eq!(
        median_quote_minutes(&mut db, &plumbing, from, to).await,
        Ok(None)
    );
    assert_eq!(picked_requests(&mut db, &[], to).await, Ok(None));
    assert_eq!(
        confirmed_share(&mut db, &plumbing, from, to).await,
        Ok(None)
    );
    assert_eq!(
        diagnostic_share(&mut db, &plumbing, (from, to), &[]).await,
        Ok(None)
    );
    assert_eq!(notes_returned(&mut db, &plumbing, from, to).await, Ok(None));
}

/// The returned notes' audit rows, as (actor ID, subject ID, field).
async fn returned(db: &mut PgConnection) -> Vec<(Option<Uuid>, Uuid, Option<String>)> {
    let rows = export_audit(db, 0, 100).await.expect("audit");
    rows.into_iter()
        .map(|r| {
            assert_eq!(r.action, "quotes.note_returned");
            assert_eq!(
                (r.actor_kind.as_str(), r.subject_kind.as_str()),
                ("business", "jobs.job")
            );
            assert_eq!((r.from_status, r.to_status), (None, None));
            (r.actor_id, r.subject_id, r.field)
        })
        .collect()
}

/// PICK-052, PRIV-028, D-3844, D-3921: a note sent back through `save_quote` writes one
/// `quotes.note_returned` row by the business on the job, holding only the reason's code name,
/// and is counted once in its trade and window. A note with a fake phone number never passes
/// `parse_quote` (its contact bounce), so the served form returns it and records it through
/// `record_note_returned`, which writes the `contact` code name only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_returned_note_is_audited_without_its_text_and_counted_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let phone_note = format!("Call me at {}-{}-{} to book.", 928, 555, "0100");
    assert_eq!(check_note(&phone_note), Err(NoteProblem::Contact));
    let form = QuoteForm {
        price: Price::Firm(30_000),
        notes: "Pay cash and save 10%.".to_owned(),
        original: String::new(),
        ai_read: false,
        earliest_visit: None,
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    };
    let bad = parse_quote(form, &rules).expect("quote");
    let (job, business) = (id(41), id(42));
    let got = save_quote(&mut db, (job, business), &bad, None, Timestamp::now()).await;
    assert_eq!(got, Ok(Saved::NoteBack(NoteProblem::OffPlatform)));
    let field = Some("off_platform".to_owned());
    assert_eq!(
        returned(&mut db).await,
        vec![(Some(business), job, field.clone())]
    );
    let hour = SignedDuration::from_hours(1);
    let (from, to) = (Timestamp::now() - hour, Timestamp::now() + hour);
    assert_eq!(notes_returned(&mut db, &[job], from, to).await, Ok(Some(1)));
    assert_eq!(notes_returned(&mut db, &[id(43)], from, to).await, Ok(None));
    assert_eq!(
        notes_returned(&mut db, &[job], to, to + hour).await,
        Ok(None)
    );
    let other = (id(43), id(44));
    let recorded = record_note_returned(&mut db, other, NoteProblem::Contact).await;
    assert_eq!(recorded, Ok(()));
    let contact = (Some(other.1), other.0, Some("contact".to_owned()));
    assert_eq!(
        returned(&mut db).await,
        vec![(Some(business), job, field), contact]
    );
    assert_eq!(
        notes_returned(&mut db, &[job, other.0], from, to).await,
        Ok(Some(2))
    );
}

fn settings(extra: Option<&str>) -> Result<Settings, SettingsError> {
    let mut pairs = vec![
        ("ASKHAVA_APP_ENV".to_owned(), "dev".to_owned()),
        (
            "ASKHAVA_DATABASE_URL".to_owned(),
            "postgres://fake@localhost/fake".to_owned(),
        ),
    ];
    if let Some(value) = extra {
        pairs.push((
            "ASKHAVA_SALES_TAX_SET_ASIDE_BASIS_POINTS".to_owned(),
            value.to_owned(),
        ));
    }
    Settings::from_pairs(pairs)
}

/// FEE-046, D-3844: the set-aside defaults to 860 basis points, takes 0 to 10,000 and refuses
/// anything else by key.
#[test]
fn the_set_aside_setting_parses_with_its_default_and_refuses_out_of_range() {
    let rate = |extra| settings(extra).map(|s| s.sales_tax_set_aside_basis_points());
    assert_eq!(rate(None), Ok(860));
    assert_eq!(rate(Some("0")), Ok(0));
    assert_eq!(rate(Some("10000")), Ok(10_000));
    let refused = Err(SettingsError::Invalid(
        "ASKHAVA_SALES_TAX_SET_ASIDE_BASIS_POINTS",
    ));
    for bad in ["10001", "-1", "8.6", "lots"] {
        assert_eq!(rate(Some(bad)), refused, "{bad}");
    }
}
