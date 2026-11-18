//! The owner portal's tools (T108; TD-SEC-22, TD-FEE-06, TD-OPS-01, TD-BIZ-07; FEE-007, LIC-002,
//! OPS-003, OPS-035, SEC-043, CON-024, BIZ-027; D-3882, D-3890): each pause switch changes
//! `ops::api::is_paused` and writes one audit row; the fee mode and a category switch need a fresh
//! factor, are logged, and the fee terms land in `payments.fee_terms`; the home page renders the
//! pushed alerts, the weekly opt-out rates and each needs-review sign-up's 4-day target line from
//! planted rows, and no page holds a contact detail planted in the vault; each homeowner-line and
//! banner change needs a fresh factor and is logged; a form posted to another route is refused.
//! Every password, code, contact and ID is fake.

use askhava_businesses::api::{create, review_target_line};
use askhava_catalog::api::{category_list, load_embedded};
use askhava_messaging::api::{Audience, LaunchChannel, NumberState, launch_channel, load_lines};
use askhava_ops::api::{BannerBy, Switch, acknowledge_alert, banner_shown, is_paused, show_banner};
use askhava_payments::api::{FeeMode, fee_terms_in_force};
use askhava_portals::api::{
    ChangeForm, CookieJar, OWNER_ALERT_ACK_ROUTE, OWNER_BANNER_ROUTE, OWNER_CATEGORY_ROUTE,
    OWNER_FEE_MODE_ROUTE, OWNER_PAUSE_ROUTE, OWNER_TEXTING_ROUTE, OwnerAuth, OwnerChange,
    OwnerPrincipal, OwnerToolError, PauseSwitch, SecondFactor, apply_owner_change, bootstrap_owner,
    owner_home_page, owner_principal, owner_settings_page, owner_sign_in, parse_change,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const PASSWORD: &str = "fake owner pass phrase 2026";
const TOKEN: &str = "fake-bootstrap-token-0123456789abcdef";
/// A fake homeowner's name, phone, email and address, stored in the vault before a page renders.
const CONTACT: [&str; 4] = [
    "Dana Testperson",
    "+19285550188",
    "dana@example.com",
    "123 Fake Test Way",
];

/// Stores [`CONTACT`] in the vault (fake keys); its contact ID.
async fn plant_contact(db: &mut PgConnection) -> Uuid {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let vault = Vault::new(&key(0x41), &key(0x42), &url).expect("vault");
    let field = |i: usize| Some(SecretString::from(CONTACT[i]));
    let new = NewContact {
        name: field(0),
        phone: field(1),
        email: field(2),
        address: field(3),
        onsite_contact: None,
    };
    store_contact(db, &vault, &new).await.expect("contact")
}

/// No planted contact value, nor the phone's last digits, is on `page`.
fn assert_no_contact(page: &str) {
    for value in CONTACT.into_iter().chain(["5550188", "Dana"]) {
        assert!(!page.contains(value), "the page holds {value}");
    }
}

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_010).expect("time") + SignedDuration::from_secs(secs)
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `now`.
fn totp_code(secret: &[u8], now: Timestamp) -> String {
    let step = u64::try_from(now.as_second() / 30).expect("step");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[offset..offset + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

/// The bootstrapped owner signed in at 0 with a TOTP code; the jar of that session.
async fn signed_in(db: &mut PgConnection) -> CookieJar {
    bootstrap_owner(db, Some(TOKEN), PASSWORD, at(0))
        .await
        .expect("bootstrap");
    let secret: Vec<u8> = sqlx::query_scalar("SELECT totp_secret FROM portals.owner_factor")
        .fetch_one(&mut *db)
        .await
        .expect("secret");
    let code = totp_code(&secret, at(0));
    let auth = OwnerAuth::new("askhava.test", "https://askhava.test").expect("relying party");
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    owner_sign_in(db, CookieJar::new(), factor, &auth, at(0))
        .await
        .expect("signed in")
}

async fn principal(db: &mut PgConnection, jar: &CookieJar, t: i64) -> OwnerPrincipal {
    owner_principal(db, jar, at(t))
        .await
        .expect("read")
        .expect("live")
}

async fn audit_rows(db: &mut PgConnection, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.audit_log WHERE action = $1")
        .bind(action)
        .fetch_one(db)
        .await
        .expect("audit")
}

fn form(change: &str, target: &str, to: &str) -> ChangeForm {
    let (change, target, to) = (change.into(), target.into(), to.into());
    ChangeForm { change, target, to }
}

/// The first switched-on category's code.
async fn a_category(db: &mut PgConnection) -> String {
    load_embedded(db).await.expect("catalog");
    let list = category_list(db).await.expect("list");
    list.categories
        .into_iter()
        .find(|c| c.enabled)
        .expect("one on")
        .code
}

/// TD-OPS-01 (OPS-003): each pause switch, a trade's included, turns `is_paused` on and off with
/// one audit row per change and none for a repeat; no fresh factor is needed to pause.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_01_each_pause_switch_changes_behavior_and_is_logged(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let trade = a_category(&mut db).await;
    let jar = signed_in(&mut db).await;
    let owner = principal(&mut db, &jar, 600).await;
    let stored = askhava_catalog::api::category(&mut db, &trade)
        .await
        .expect("read");
    let pauses = [
        ("pause_all", "", Switch::All),
        ("pause_charging", "", Switch::Charging),
        ("pause_texting", "", Switch::Texting),
        (
            "pause_trade",
            trade.as_str(),
            Switch::Trade(stored.expect("stored").id),
        ),
    ];
    let mut logged = 0;
    for (change, target, switch) in pauses {
        for (to, paused) in [("on", true), ("on", true), ("off", false)] {
            let parsed = parse_change(OWNER_PAUSE_ROUTE, &form(change, target, to)).expect("form");
            let changed = apply_owner_change(&mut db, &owner, parsed, at(600)).await;
            assert_eq!(
                is_paused(&mut db, switch).await.expect("read"),
                paused,
                "{change}"
            );
            logged += i64::from(changed.expect("changed"));
            let rows =
                audit_rows(&mut db, "switch_on").await + audit_rows(&mut db, "switch_off").await;
            assert_eq!(rows, logged, "{change} {to}: one row per change");
        }
    }
    assert_eq!(logged, 8);
}

/// TD-FEE-06, TD-SEC-22 (FEE-007, SEC-043): with no factor in the last 5 minutes the fee-mode
/// switch is refused and nothing changes; with one it is logged and stored in `payments.fee_terms`
/// with a new authorization version; a repeat writes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_06_the_fee_mode_switch_needs_a_fresh_factor_and_is_stored(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let jar = signed_in(&mut db).await;
    let before = fee_terms_in_force(&mut db).await.expect("terms");
    let flat = parse_change(OWNER_FEE_MODE_ROUTE, &form("fee_mode", "flat", "")).expect("form");
    let stale = principal(&mut db, &jar, 301).await;
    let refused = apply_owner_change(&mut db, &stale, flat.clone(), at(301)).await;
    assert_eq!(
        refused,
        Err(OwnerToolError::NeedsFreshFactor),
        "5:01 is stale"
    );
    assert_eq!(fee_terms_in_force(&mut db).await.expect("terms"), before);
    let action = askhava_payments::api::FEE_MODE_ACTION;
    assert_eq!(audit_rows(&mut db, action).await, 0, "nothing logged");
    sqlx::query("UPDATE portals.owner_session SET factor_at = $1::timestamptz")
        .bind(at(400).to_string())
        .execute(&mut *db)
        .await
        .expect("fresh factor");
    let fresh = principal(&mut db, &jar, 420).await;
    let switched = apply_owner_change(&mut db, &fresh, flat.clone(), at(420)).await;
    assert_eq!(switched, Ok(true));
    let after = fee_terms_in_force(&mut db).await.expect("terms");
    assert_eq!(after.mode(), FeeMode::Flat);
    assert!(after.authorization_version() > before.authorization_version());
    assert_eq!(audit_rows(&mut db, action).await, 1, "logged once");
    let again = apply_owner_change(&mut db, &fresh, flat, at(430)).await;
    assert_eq!((again, audit_rows(&mut db, action).await), (Ok(false), 1));
}

/// LIC-002, SEC-043: a category switch needs a fresh factor, then is logged and shows on the
/// settings page.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_category_switch_needs_a_fresh_factor_and_is_logged(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let code = a_category(&mut db).await;
    plant_contact(&mut db).await;
    let jar = signed_in(&mut db).await;
    let off = form("category", &code, "off");
    let change = parse_change(OWNER_CATEGORY_ROUTE, &off).expect("form");
    assert_eq!(change, OwnerChange::Category(code.clone(), false));
    let stale = principal(&mut db, &jar, 900).await;
    let refused = apply_owner_change(&mut db, &stale, change.clone(), at(900)).await;
    assert_eq!(refused, Err(OwnerToolError::NeedsFreshFactor));
    let fresh = principal(&mut db, &jar, 60).await;
    assert_eq!(
        apply_owner_change(&mut db, &fresh, change, at(60)).await,
        Ok(true)
    );
    assert_eq!(audit_rows(&mut db, "catalog.category_switched").await, 1);
    let page = owner_settings_page(&mut db, "fake-csrf")
        .await
        .expect("page");
    let turn_on =
        format!("name=\"target\" value=\"{code}\"><input type=\"hidden\" name=\"to\" value=\"on\"");
    assert!(
        page.contains(&turn_on),
        "the switched-off category offers on"
    );
    assert_no_contact(&page);
}

/// Plants one needs-review sign-up waiting since `since`.
async fn needs_review(db: &mut PgConnection, since: Timestamp) -> Uuid {
    let business = create(db, false, since).await.expect("business");
    sqlx::query("UPDATE businesses.business SET status = 'needs_review' WHERE id = $1")
        .bind(business)
        .execute(&mut *db)
        .await
        .expect("status");
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, review_since, submitted_at) \
         VALUES ($1, gen_random_uuid(), $2::timestamptz, $2::timestamptz)",
    )
    .bind(business)
    .bind(since.to_string())
    .execute(&mut *db)
    .await
    .expect("signup");
    business
}

/// Plants last week's homeowner texts and opt-outs and one pushed High alert.
async fn plant_week_and_alert(db: &mut PgConnection, now: Timestamp) -> Uuid {
    let last_week = now - SignedDuration::from_hours(24 * 7);
    sqlx::query(
        "INSERT INTO messaging.outbox (idempotency_key, template_id, recipient_id, \
         recipient_kind, subject_ids, body, status, provider_sid, sent_at) \
         SELECT encode(sha256(gen_random_uuid()::text::bytea), 'hex'), 'consent', gen_random_uuid(), 'homeowner', \
           ARRAY[gen_random_uuid()], NULL, 'sent', 'SM' || md5(gen_random_uuid()::text), \
           $1::timestamptz FROM generate_series(1, 100)",
    )
    .bind(last_week.to_string())
    .execute(&mut *db)
    .await
    .expect("texts");
    sqlx::query(
        "INSERT INTO messaging.optout_event (action, channel, detected, index_version, digest, \
         effective_at) SELECT 'opt_out', 'text', 'word:stop', 1, \
         sha256(gen_random_uuid()::text::bytea), $1::timestamptz FROM generate_series(1, 3)",
    )
    .bind(last_week.to_string())
    .execute(&mut *db)
    .await
    .expect("opt-outs");
    pushed_alert(db, ("ops.health_check_failed", "high"), Uuid::new_v4()).await
}

/// Plants one pushed `(kind, severity)` alert about the record `about`.
async fn pushed_alert(db: &mut PgConnection, (kind, severity): (&str, &str), about: Uuid) -> Uuid {
    let alert: Uuid = sqlx::query_scalar(
        "INSERT INTO ops.alert (kind, severity, subject_kind, subject_id) \
         VALUES ($1, $2, 'privacy.contact', $3) RETURNING id",
    )
    .bind(kind)
    .bind(severity)
    .bind(about)
    .fetch_one(&mut *db)
    .await
    .expect("alert");
    sqlx::query("INSERT INTO ops.alert_push (alert_id, pushed_at) VALUES ($1, now())")
        .bind(alert)
        .execute(&mut *db)
        .await
        .expect("push");
    alert
}

/// OPS-028, OPS-025: plants a pushed critical alert about a vault contact that the safety call
/// acknowledged (no ack row) and a pushed text-delivery alert about her.
async fn plant_answered_and_delivery(db: &mut PgConnection) {
    let her = plant_contact(db).await;
    let called = pushed_alert(db, ("payments.ledger_mismatch", "critical"), her).await;
    let answered = acknowledge_alert(db, called, at(-5)).await;
    assert!(answered.expect("call ack"));
    pushed_alert(db, (askhava_ops::api::DELIVERY_ALERT, "high"), her).await;
}

/// How many "I've seen this alert" buttons and "Seen" lines `page` holds.
fn alert_marks(page: &str) -> (usize, usize) {
    let buttons = page.matches("I've seen this alert").count();
    (buttons, page.matches("Seen ").count())
}

/// TD-BIZ-07 (BIZ-027), CON-024, OPS-035, OPS-001, OPS-025: the home page shows each needs-review
/// sign-up's 4-day target line, last week's opt-out rate over the line, the pushed alert with its
/// acknowledgement (which then resolves it, logged), a critical alert answered on the safety call
/// as seen with no button, a text-delivery alert with no button, and no planted contact detail.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_07_the_home_page_renders_reviews_rates_and_alerts(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    let since = at(-86_400);
    let business = needs_review(&mut db, since).await;
    let alert = plant_week_and_alert(&mut db, at(0)).await;
    plant_answered_and_delivery(&mut db).await;
    let page = owner_home_page(&mut db, "fake-csrf", 4, at(0))
        .await
        .expect("page");
    assert_eq!(
        alert_marks(&page),
        (1, 1),
        "one button; the answered call is seen"
    );
    assert!(page.contains("This clears by itself once texts are getting through again."));
    assert!(
        page.contains(&business.to_string()),
        "the sign-up is listed"
    );
    let target = review_target_line(since, 4);
    assert!(
        target.contains("4 calendar days") && page.contains(&target),
        "{target}"
    );
    assert!(page.contains("We texted 100; 3 opted out (3.0%). <strong>At or over the 3% alert"));
    assert!(page.contains("ops.health_check_failed (high)") && page.contains(&alert.to_string()));
    assert!(page.contains("/static/portals/forms.css"));
    assert_no_contact(&page);
    let ack = form("acknowledge", &alert.to_string(), "");
    let change = parse_change(OWNER_ALERT_ACK_ROUTE, &ack).expect("form");
    let jar = signed_in(&mut db).await;
    let owner = principal(&mut db, &jar, 10).await;
    let done = apply_owner_change(&mut db, &owner, change.clone(), at(10)).await;
    let again = apply_owner_change(&mut db, &owner, change, at(11)).await;
    assert_eq!((done, again), (Ok(true), Ok(false)));
    assert_eq!(
        audit_rows(&mut db, askhava_ops::api::OWNER_ACK_ACTION).await,
        1
    );
    let page = owner_home_page(&mut db, "fake-csrf", 4, at(20))
        .await
        .expect("page");
    assert_eq!(alert_marks(&page), (0, 2));
    let trade = PauseSwitch::Trade("no_such_trade".into());
    let unknown = apply_owner_change(&mut db, &owner, OwnerChange::Pause(trade, true), at(12));
    assert_eq!(unknown.await, Err(OwnerToolError::Refused));
}

/// The homeowner line as the page reads it: texting on, launch channel, number state.
async fn line(db: &mut PgConnection) -> (bool, LaunchChannel, &'static str) {
    let lines = load_lines(db, (None, None)).await.expect("lines");
    let launch = launch_channel(db, Audience::Homeowner)
        .await
        .expect("launch");
    let state = match lines.homeowner.state {
        NumberState::Working => "working",
        NumberState::Suspended { .. } => "suspended",
        NumberState::Restored { .. } => "restored",
    };
    (lines.homeowner.texting_on, launch, state)
}

/// TD-SEC-22 (SEC-043, TXT-021, TXT-022, TXT-024, D-1843): homeowner texting off and on, the launch
/// channel, the suspension and the restore each are refused at 5:01 with nothing changed, then
/// change the line with one `messaging.mode_changed` row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn each_homeowner_line_change_needs_a_fresh_factor_and_is_logged(pool: PgPool) {
    use LaunchChannel::{Sms, WebAndEmail};
    let mut db = pool.acquire().await.expect("db");
    let jar = signed_in(&mut db).await;
    let (stale, fresh) = (
        principal(&mut db, &jar, 900).await,
        principal(&mut db, &jar, 60).await,
    );
    let steps = [
        (
            form("homeowner_texting", "", "off"),
            (false, Sms, "working"),
        ),
        (form("homeowner_texting", "", "on"), (true, Sms, "working")),
        (
            form("launch_channel", "web_and_email", ""),
            (false, WebAndEmail, "working"),
        ),
        (
            form("number_suspended", "", ""),
            (false, WebAndEmail, "suspended"),
        ),
        (
            form("number_restored", "", ""),
            (false, WebAndEmail, "restored"),
        ),
    ];
    for (logged, (body, after)) in (1..).zip(steps) {
        let change = parse_change(OWNER_TEXTING_ROUTE, &body).expect("form");
        let before = line(&mut db).await;
        let refused = apply_owner_change(&mut db, &stale, change.clone(), at(900)).await;
        let unchanged = (line(&mut db).await, mode_rows(&mut db).await);
        let stale_case = (refused, unchanged);
        let want = (Err(OwnerToolError::NeedsFreshFactor), (before, logged - 1));
        assert_eq!(stale_case, want, "{}: refused changes nothing", body.change);
        let done = apply_owner_change(&mut db, &fresh, change, at(60)).await;
        let now = (done, line(&mut db).await, mode_rows(&mut db).await);
        assert_eq!(now, (Ok(true), after, logged), "{}", body.change);
    }
}

/// The count of homeowner-line change audit rows.
async fn mode_rows(db: &mut PgConnection) -> i64 {
    audit_rows(db, "messaging.mode_changed").await
}

/// TD-SEC-22 (SEC-043, TXT-024, D-1843): hiding the status banner is refused at 5:01 with the
/// banner still shown, then hides it with one owner row in its history.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_banner_off_needs_a_fresh_factor_and_is_logged(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let jar = signed_in(&mut db).await;
    let (stale, fresh) = (
        principal(&mut db, &jar, 900).await,
        principal(&mut db, &jar, 60).await,
    );
    let owner = fresh.owner();
    let shown = show_banner(&mut db, BannerBy::Owner(owner), at(30)).await;
    assert!(shown.expect("banner on"));
    let off = parse_change(OWNER_BANNER_ROUTE, &form("banner_off", "", "")).expect("form");
    let refused = apply_owner_change(&mut db, &stale, off.clone(), at(900)).await;
    assert_eq!(refused, Err(OwnerToolError::NeedsFreshFactor));
    let still = banner_shown(&mut db).await.expect("banner");
    assert!(still, "refused changes nothing");
    assert_eq!(
        apply_owner_change(&mut db, &fresh, off, at(60)).await,
        Ok(true)
    );
    let hidden: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.status_banner WHERE NOT shown AND actor_kind = 'owner'",
    )
    .fetch_one(&mut *db)
    .await
    .expect("rows");
    let shown = banner_shown(&mut db).await.expect("banner");
    assert_eq!((shown, hidden), (false, 1));
}

/// SEC-043, D-1843: a valid form of each family posted to any other owner route is refused, so a
/// guarded change cannot ride an unguarded route.
#[test]
fn a_valid_form_posted_to_another_route_is_refused() {
    let alert = Uuid::new_v4().to_string();
    let forms = [
        (OWNER_PAUSE_ROUTE, form("pause_all", "", "on")),
        (OWNER_FEE_MODE_ROUTE, form("fee_mode", "flat", "")),
        (OWNER_CATEGORY_ROUTE, form("category", "plumbing", "off")),
        (OWNER_TEXTING_ROUTE, form("homeowner_texting", "", "off")),
        (OWNER_BANNER_ROUTE, form("banner_off", "", "")),
        (OWNER_ALERT_ACK_ROUTE, form("acknowledge", &alert, "")),
    ];
    for (own, body) in &forms {
        for (route, _) in &forms {
            let parsed = parse_change(route, body);
            assert_eq!(parsed.is_some(), route == own, "{} to {route}", body.change);
        }
    }
}
