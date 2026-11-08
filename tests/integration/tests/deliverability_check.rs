//! The sign-up inbox check and the sample pick step (TD-BIZ-18, TD-BIZ-28: BIZ-042, BIZ-040,
//! CON-020, SEC-025; D-328, D-352, D-380, D-462). The test emergency email is TD-BIZ-24's (T150).
//! All IDs and addresses are fake.

use askhava_businesses::api::{
    Acting, EmailCheck, InboxAnswer, PICK_ALERT_YES_LIFE, Roles, SAMPLE_CARD_KIND,
    SAMPLE_PICK_KIND, add_member, add_owner, business, create, email_check, offer_sample_pick,
    queue_pick_retests, record_inbox_answer, record_ok_reply, record_pick_alert, sample_card_words,
    sample_pick_words, send_sample_card, send_sample_pick,
};
use askhava_core::api::brand;
use askhava_portals::api::{
    BUSINESS_SESSION_LIFE, BusinessPrincipal, CookieJar, HELP_REQUEST_KIND, LAST_STEP_LINE,
    MailApp, NO_EMAIL_LINE, PICK_DND_LINE, PICK_SKIP_LINE, REPLY_RECEIVED, Realm,
    SAMPLE_PICK_TITLE, StepList, business_principal, email_check_page, sample_pick_on_open,
    sample_pick_page, start_session, steps,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const OWNER_BOX: Uuid = Uuid::from_u128(0xe1);
const BILLING_BOX: Uuid = Uuid::from_u128(0xe2);
const CARDS_BOX: Uuid = Uuid::from_u128(0xe3);

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000).expect("time") + SignedDuration::from_secs(secs)
}

/// The owner, a "Confirm and billing" member and a "Cards and quotes" member.
async fn team(db: &mut PgConnection) -> [Acting; 3] {
    let business = create(db, false, at(0)).await.expect("business");
    let owner = add_owner(db, business, OWNER_BOX, at(0))
        .await
        .expect("owner");
    let owner = Acting {
        business,
        member: owner,
    };
    let mut people = vec![owner];
    for (mailbox, roles) in [(BILLING_BOX, (false, true)), (CARDS_BOX, (true, false))] {
        let roles = Roles::new(roles.0, roles.1).expect("roles");
        let member = add_member(db, owner, mailbox, roles, at(0)).await;
        people.push(Acting {
            business,
            member: member.expect("member"),
        });
    }
    [people[0], people[1], people[2]]
}

async fn notices(db: &mut PgConnection, kind: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1 ORDER BY id")
        .bind(kind)
        .fetch_all(db)
        .await
        .expect("tasks")
}

async fn principal(db: &mut PgConnection, who: Acting) -> BusinessPrincipal {
    let login = (Realm::Business, who.member);
    let jar = start_session(db, CookieJar::new(), login, BUSINESS_SESSION_LIFE, at(0)).await;
    let jar = jar.expect("session");
    sqlx::query(
        "INSERT INTO portals.business_session (session_id, business_id) \
         SELECT id, $2 FROM portals.session WHERE actor_id = $1",
    )
    .bind(who.member)
    .bind(who.business)
    .execute(&mut *db)
    .await
    .expect("business session");
    business_principal(db, &jar, at(1))
        .await
        .expect("read")
        .expect("signed in")
}

/// TD-BIZ-18 (BIZ-042, SEC-025, CON-020): the sample card goes to the person's main address with
/// IDs only, its words are fake and say it is not a real job in both languages; the resend is the
/// same call; activation never waits on it and it is no job card (the sender's FROM is
/// `messaging`'s, D-1987).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_18_the_sample_card_comes_from_the_real_sender(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let [owner, ..] = team(&mut db).await;
    let before = business(&mut db, owner.business)
        .await
        .expect("read")
        .status;
    send_sample_card(&mut db, owner, at(10))
        .await
        .expect("send");
    send_sample_card(&mut db, owner, at(70))
        .await
        .expect("resend");
    let sent = notices(&mut db, SAMPLE_CARD_KIND).await;
    assert_eq!(sent.len(), 2, "send and resend");
    assert_eq!(sent[0]["to"], serde_json::json!([OWNER_BOX.to_string()]));
    assert!(
        sent[0]["card"].is_null() && sent[0]["values"]["job"].is_null(),
        "no job, no card"
    );
    let [subject, body] = sample_card_words(false);
    assert_eq!(
        subject,
        format!("{}: Sample job card (not a real job)", brand().short_name())
    );
    assert!(body.contains("made up") && body.contains("did it arrive in your inbox?"));
    let [es_subject, _] = sample_card_words(true);
    assert!(es_subject.contains("no es un trabajo real"));
    let after = business(&mut db, owner.business)
        .await
        .expect("read")
        .status;
    assert_eq!(
        before.code(),
        after.code(),
        "the check never moves the account"
    );
    assert_eq!(after.code(), "pending");
    let job_cards: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM core.task WHERE payload->>'template' = 'job_card_notice'",
    )
    .fetch_one(&mut *db)
    .await
    .expect("count");
    assert_eq!(job_cards, 0, "the sample is not counted as a card");
}

/// TD-BIZ-18 (BIZ-042): the screen holds the Spam line and a resend; Spam, Promotions or Not yet
/// show that mail app's steps, Inbox shows none; the last step and its per-app guide (Gmail's
/// Primary tab) always show; a team member at first sign-in gets the same screen.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_18_answers_show_the_mail_apps_steps(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let [owner, billing, _] = team(&mut db).await;
    let first = email_check(&mut db, billing).await.expect("read");
    assert_eq!(
        first,
        EmailCheck::default(),
        "a member starts at the first step"
    );
    let page = email_check_page(first, MailApp::Gmail, "csrf").expect("page");
    assert!(page.contains(NO_EMAIL_LINE) && page.contains("Send me a sample job card"));
    send_sample_card(&mut db, owner, at(1)).await.expect("send");
    for answer in InboxAnswer::ALL {
        record_inbox_answer(&mut db, owner, answer, at(2))
            .await
            .expect("answer");
        let check = email_check(&mut db, owner).await.expect("read");
        assert_eq!(check.answer, Some(answer));
        for app in MailApp::ALL {
            let page = email_check_page(check, app, "csrf").expect("page");
            let first_step = &steps(StepList::Inbox, app)[0].text;
            let shown = page.contains(&askama_escape(first_step));
            assert_eq!(shown, answer != InboxAnswer::Inbox, "{answer:?} {app:?}");
            assert!(page.contains("Send the sample again") && page.contains(LAST_STEP_LINE));
        }
    }
    let gmail = steps(StepList::LastStep, MailApp::Gmail);
    assert!(gmail.iter().any(|s| s.text.contains("Primary tab")));
    let spam = steps(StepList::Inbox, MailApp::Gmail);
    assert!(
        spam.iter()
            .any(|s| s.text.contains("Never send it to Spam"))
    );
}

/// TD-BIZ-18 (BIZ-042, BIZ-038, D-352): an OK reply shows "Reply received", runs no command and
/// opens no help request.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_18_an_ok_reply_is_shown_and_never_a_command(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let [owner, ..] = team(&mut db).await;
    let page = email_check_page(EmailCheck::default(), MailApp::Other, "c").expect("page");
    assert!(!page.contains(REPLY_RECEIVED));
    record_ok_reply(&mut db, owner, at(5)).await.expect("ok");
    let check = email_check(&mut db, owner).await.expect("read");
    assert!(check.reply_received);
    let page = email_check_page(check, MailApp::Other, "c").expect("page");
    assert!(page.contains(REPLY_RECEIVED));
    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(HELP_REQUEST_KIND)
        .fetch_one(&mut *db)
        .await
        .expect("count");
    assert_eq!(tasks, 0, "no help request");
}

/// BIZ-042: every step names its maker's help page (but "another mail app") and a check date.
#[test]
fn every_step_has_its_help_page_and_check_date() {
    for list in [StepList::Inbox, StepList::LastStep, StepList::PickAlert] {
        for app in MailApp::ALL {
            let all = steps(list, app);
            assert!(!all.is_empty(), "{list:?} {app:?}");
            for s in all {
                assert_eq!(s.help_page.is_empty(), app == MailApp::Other, "{}", s.text);
                assert!(s.help_page.is_empty() || s.help_page.starts_with("https://"));
                assert_eq!(s.checked.len(), 10, "{}", s.text);
            }
        }
    }
}

/// TD-BIZ-28 (BIZ-042, D-462): the sample pick step shows the first time an owner or "Confirm
/// and billing" person opens a real job card, never again, never to "Cards and quotes" only and
/// not during sign-up; it holds the Do Not Disturb line and the "Skip this" line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_28_the_sample_pick_shows_on_the_first_real_card(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let [owner, billing, cards] = team(&mut db).await;
    let signup = email_check_page(EmailCheck::default(), MailApp::Gmail, "c").expect("page");
    assert!(!signup.contains(SAMPLE_PICK_TITLE), "not during sign-up");
    let me = principal(&mut db, owner).await;
    let step = sample_pick_on_open(&mut db, &me, MailApp::IPhone, ("c", at(5))).await;
    let step = step.expect("open").expect("first real card");
    assert!(step.contains(SAMPLE_PICK_TITLE) && step.contains(PICK_DND_LINE));
    assert!(step.contains(&askama_escape(PICK_SKIP_LINE)));
    let again = sample_pick_on_open(&mut db, &me, MailApp::IPhone, ("c", at(6))).await;
    assert!(again.expect("open").is_none(), "only the first time");
    assert!(
        offer_sample_pick(&mut db, billing, at(7))
            .await
            .expect("billing")
    );
    assert!(
        !offer_sample_pick(&mut db, cards, at(7))
            .await
            .expect("cards")
    );
    let no = sample_pick_page(MailApp::Gmail, true, "c").expect("page");
    let alert = &steps(StepList::PickAlert, MailApp::Gmail)[0].text;
    assert!(
        no.contains(&askama_escape(alert)),
        "a No shows the steps again"
    );
}

/// TD-BIZ-28 (BIZ-042, D-380): the sample pick goes with a Confirm-by time; a Yes lasts 90 days,
/// then one re-test email; a No keeps no Yes; nothing changes the account.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_28_a_yes_lasts_ninety_days_then_a_retest(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let [owner, billing, _] = team(&mut db).await;
    send_sample_pick(&mut db, owner, at(0)).await.expect("send");
    let sent = notices(&mut db, SAMPLE_PICK_KIND).await;
    assert!(sent[0]["confirm_by"].is_i64(), "Confirm by a time");
    let [subject, body] = sample_pick_words("2 p.m. Arizona time", false);
    assert!(subject.ends_with("Sample pick (not a real job). Confirm by 2 p.m. Arizona time."));
    assert!(body.ends_with("did your phone alert you?"));
    assert!(sample_pick_words("2 p.m.", true)[0].contains("Confirme antes de 2 p.m."));
    record_pick_alert(&mut db, owner, true, at(10))
        .await
        .expect("yes");
    record_pick_alert(&mut db, billing, false, at(10))
        .await
        .expect("no");
    let day = SignedDuration::from_hours(24).as_secs();
    let early = queue_pick_retests(&mut db, PICK_ALERT_YES_LIFE, at(10 + 89 * day)).await;
    assert_eq!(early.expect("89 days"), 0);
    let due = queue_pick_retests(&mut db, PICK_ALERT_YES_LIFE, at(10 + 90 * day)).await;
    assert_eq!(due.expect("90 days"), 1, "only the Yes is re-tested");
    let twice = queue_pick_retests(&mut db, PICK_ALERT_YES_LIFE, at(11 + 90 * day)).await;
    assert_eq!(twice.expect("again"), 0);
    assert_eq!(notices(&mut db, SAMPLE_PICK_KIND).await.len(), 2);
    let status = business(&mut db, owner.business)
        .await
        .expect("read")
        .status;
    assert_eq!(status.code(), "pending");
}

/// The page escapes text as askama does, so an apostrophe or quote matches.
fn askama_escape(s: &str) -> String {
    s.replace('&', "&#38;")
        .replace('\'', "&#39;")
        .replace('"', "&#34;")
}
