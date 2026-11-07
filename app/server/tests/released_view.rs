//! TD-PICK-48 (PICK-022, PICK-023, PICK-024, PRIV-003, PRIV-005, PRIV-020, SEC-046; D-1739)
//! against the database: `picks::api::released_view` with a business session proof minted by
//! `server::session_minter()` (`server` is the one test home allowed the session mint,
//! `tests/integration/tests/architecture.rs`). The owner or a "Confirm and billing" member of a
//! released, paid or free pick's business sees the bound contact's first name and address, and
//! the email only on an email-contact release; every refusal is the same not-found answer and reads
//! nothing; viewing again writes no release row, notice, charge, fee or masked line; after the
//! purge or "Delete my data" it says the details were removed and opens nothing. The release row
//! is written here as `picks::api::release` writes it (no test may mint a release proof, D-1563).
//! All names, phones, emails and addresses are fake.

use askhava_core::api::MIGRATOR;
use askhava_ops::api::Actor;
use askhava_picks::api::{ViewAnswer, released_view};
use askhava_privacy::api::{
    BusinessSessionProof, NewContact, Vault, delete_my_data, purge_contact, store_contact,
};
use askhava_server::session_minter;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0x0253_0001);
const OTHER_BUSINESS: Uuid = Uuid::from_u128(0x0253_0002);
const OWNER: Uuid = Uuid::from_u128(0x0253_0011);
const BILLING: Uuid = Uuid::from_u128(0x0253_0012);
const CARDS: Uuid = Uuid::from_u128(0x0253_0013);
const STRANGER: Uuid = Uuid::from_u128(0x0253_0014);

fn vault() -> Vault {
    let key = |k: &str| SecretString::from(format!("1:{k}"));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let (seal, index) = (
        "ERERERERERERERERERERERERERERERERERERERERERE=",
        "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiI=",
    );
    Vault::new(&key(seal), &key(index), &url).expect("vault")
}

fn proof(person: Uuid, business: Uuid) -> BusinessSessionProof {
    session_minter().business(person, business)
}

async fn run(pool: &PgPool, sql: &'static str, ids: &[Uuid]) {
    ids.iter()
        .fold(sqlx::query(sql), |q, id| q.bind(*id))
        .execute(pool)
        .await
        .expect(sql);
}

async fn count(pool: &PgPool, sql: &'static str, ids: &[Uuid]) -> i64 {
    ids.iter()
        .fold(sqlx::query_scalar(sql), |q, id| q.bind(*id))
        .fetch_one(pool)
        .await
        .expect(sql)
}

/// A business with its owner, a "Confirm and billing" member and a "Cards and quotes"-only member,
/// and another business with its own owner.
async fn teams(pool: &PgPool) {
    let business = "INSERT INTO businesses.business (id, status, created_at, status_changed_at) \
                    VALUES ($1, 'active', now(), now())";
    run(pool, business, &[BUSINESS]).await;
    run(pool, business, &[OTHER_BUSINESS]).await;
    let member = "INSERT INTO businesses.team_member (id, business_id, mailbox_id, is_owner, \
                  cards_and_quotes, confirm_and_billing, added_at) \
                  VALUES ($1, $2, gen_random_uuid(), $3, $4, $5, now())";
    for (id, at, owner, cards, billing) in [
        (OWNER, BUSINESS, true, true, true),
        (BILLING, BUSINESS, false, false, true),
        (CARDS, BUSINESS, false, true, false),
        (STRANGER, OTHER_BUSINESS, true, true, true),
    ] {
        sqlx::query(member)
            .bind(id)
            .bind(at)
            .bind(owner)
            .bind(cards)
            .bind(billing)
            .execute(pool)
            .await
            .expect("member");
    }
}

/// A fake homeowner contact with every field; `n` makes her fake phone her own.
async fn contact(pool: &PgPool, (name, n): (&str, u8), email: &str) -> Uuid {
    let s = |v: &str| Some(SecretString::from(v.to_owned()));
    let new = NewContact {
        name: s(name),
        phone: s(&format!("+192855501{n:02}")),
        email: s(email),
        address: s("123 Fake St, Lake Havasu City, AZ 86403"),
        onsite_contact: None,
    };
    let mut conn = pool.acquire().await.expect("conn");
    store_contact(&mut conn, &vault(), &new)
        .await
        .expect("contact")
}

/// A pick of `BUSINESS` at `status`; with `bound`, its release row bound to that contact and email
/// choice, as `picks::api::release` writes it.
async fn pick(pool: &PgPool, status: &str, bound: Option<(Uuid, bool)>) -> Uuid {
    let mut conn = pool.acquire().await.expect("conn");
    let job = askhava_jobs::api::create(&mut conn, Timestamp::now(), false, Actor::System)
        .await
        .expect("job");
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', $3, now()) RETURNING id",
    )
    .bind(job)
    .bind(BUSINESS)
    .bind(status)
    .fetch_one(pool)
    .await
    .expect("pick");
    if let Some((her, email_chosen)) = bound {
        sqlx::query(
            "INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, \
             contact_id, email_chosen) VALUES ($1, $2, $3, 'paid', now(), $4, $5)",
        )
        .bind(id)
        .bind(job)
        .bind(BUSINESS)
        .bind(her)
        .bind(email_chosen)
        .execute(pool)
        .await
        .expect("release");
    }
    id
}

async fn view(pool: &PgPool, who: &BusinessSessionProof, pick: Uuid) -> ViewAnswer {
    released_view(pool, &vault(), who, pick)
        .await
        .expect("view")
}

fn plain(value: Option<&SecretString>) -> Option<String> {
    value.map(|v| v.expose_secret().to_owned())
}

/// The rows a view must never add: release rows, tasks other than the audit stream's (notices),
/// charges, fees, masked lines and alerts.
async fn side_rows(pool: &PgPool) -> i64 {
    count(
        pool,
        "SELECT (SELECT count(*) FROM picks.release) + (SELECT count(*) FROM core.task WHERE kind <> 'ops.offdb_stream') \
         + (SELECT count(*) FROM payments.charge_attempt) \
         + (SELECT count(*) FROM payments.ledger_entry) \
         + (SELECT count(*) FROM calls.number_pool) + (SELECT count(*) FROM ops.alert)",
        &[],
    )
    .await
}

async fn read_rows(pool: &PgPool) -> i64 {
    count(
        pool,
        "SELECT count(*) FROM ops.audit_log WHERE action = 'vault_read.picks.released_view'",
        &[],
    )
    .await
}

/// TD-PICK-48: the owner and a "Confirm and billing" member see the bound contact's first name and
/// address, no email on a call-contact release and never the phone; each view writes its audited
/// read rows (the person as actor, the pick as subject) and nothing else, and repeats the same.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_48_owner_and_billing_member_see_the_bound_contact_again(pool: PgPool) {
    teams(&pool).await;
    contact(&pool, ("Decoy Person", 1), "decoy@example.test").await;
    let her = contact(&pool, ("Testa Q. Example", 2), "testa@example.test").await;
    let id = pick(&pool, "paid", Some((her, false))).await;
    let before = side_rows(&pool).await;
    for person in [OWNER, BILLING, OWNER] {
        let ViewAnswer::Shown(shown) = view(&pool, &proof(person, BUSINESS), id).await else {
            panic!("the view is shown to {person}");
        };
        assert_eq!(plain(shown.first_name()).as_deref(), Some("Testa"));
        let address = plain(shown.address());
        assert_eq!(
            address.as_deref(),
            Some("123 Fake St, Lake Havasu City, AZ 86403")
        );
        assert_eq!(
            plain(shown.email()),
            None,
            "a call-contact release shows no email"
        );
        assert!(!format!("{shown:?}").contains("55501"), "no phone");
    }
    assert_eq!(
        side_rows(&pool).await,
        before,
        "viewing writes no release, notice or charge"
    );
    assert_eq!(
        read_rows(&pool).await,
        9,
        "two fields and the pick row per view"
    );
    let by_person = "SELECT count(*) FROM ops.audit_log \
         WHERE action = 'vault_read.picks.released_view' AND actor_kind = 'business' \
         AND ((subject_kind = 'picks.pick' AND subject_id = $1) \
         OR (subject_kind = 'privacy.contact' AND subject_id = $2 AND field <> 'phone'))";
    assert_eq!(count(&pool, by_person, &[id, her]).await, 9);
    let pick_rows = "SELECT count(*) FROM ops.audit_log \
         WHERE action = 'vault_read.picks.released_view' AND actor_id = $1 \
         AND subject_kind = 'picks.pick'";
    assert_eq!(
        count(&pool, pick_rows, &[BILLING]).await,
        1,
        "the person is the actor"
    );
}

/// TD-PICK-48: an email-contact release on a free pick shows only its bound email.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_48_an_email_release_shows_its_bound_email(pool: PgPool) {
    teams(&pool).await;
    let her = contact(&pool, ("Testa Example", 3), "testa@example.test").await;
    let id = pick(&pool, "free", Some((her, true))).await;
    let ViewAnswer::Shown(shown) = view(&pool, &proof(BILLING, BUSINESS), id).await else {
        panic!("a free pick is shown");
    };
    assert_eq!(plain(shown.email()).as_deref(), Some("testa@example.test"));
    assert_eq!(
        read_rows(&pool).await,
        4,
        "name, address, email and the pick row"
    );
}

/// TD-PICK-48: a "Cards and quotes"-only member, another business's person passing the pick's
/// ID, a pick with no release row and a pick whose status left paid each get the same not-found
/// answer and read nothing from the vault.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_48_every_refusal_is_one_not_found_that_reads_nothing(pool: PgPool) {
    teams(&pool).await;
    let her = contact(&pool, ("Testa Example", 4), "testa@example.test").await;
    let paid = pick(&pool, "paid", Some((her, true))).await;
    let unreleased = pick(&pool, "paid", None).await;
    let no_show = pick(&pool, "no_show", Some((her, true))).await;
    let cases = [
        (proof(CARDS, BUSINESS), paid),
        (proof(STRANGER, OTHER_BUSINESS), paid),
        (proof(STRANGER, BUSINESS), paid),
        (proof(OWNER, BUSINESS), unreleased),
        (proof(OWNER, BUSINESS), no_show),
        (proof(OWNER, BUSINESS), Uuid::from_u128(0x0253_00ff)),
    ];
    for (who, id) in &cases {
        let answer = view(&pool, who, *id).await;
        assert!(matches!(answer, ViewAnswer::NotFound), "{who:?} on {id}");
    }
    assert_eq!(read_rows(&pool).await, 0, "no refusal reads the vault");
}

/// TD-PICK-48: a member removed or demoted after the release is refused at the next view.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_48_a_member_removed_or_demoted_after_the_release_is_refused(pool: PgPool) {
    teams(&pool).await;
    let her = contact(&pool, ("Testa Example", 5), "testa@example.test").await;
    let id = pick(&pool, "paid", Some((her, false))).await;
    let extra = "INSERT INTO businesses.team_member (id, business_id, mailbox_id, is_owner, \
                 cards_and_quotes, confirm_and_billing, added_at) \
                 VALUES ($1, $2, gen_random_uuid(), false, true, true, now())";
    let leaver = Uuid::from_u128(0x0253_0015);
    run(&pool, extra, &[leaver, BUSINESS]).await;
    assert!(matches!(
        view(&pool, &proof(leaver, BUSINESS), id).await,
        ViewAnswer::Shown(_)
    ));
    run(&pool, "UPDATE businesses.team_member SET cards_and_quotes = true, confirm_and_billing = false WHERE id = $1", &[BILLING]).await;
    run(
        &pool,
        "UPDATE businesses.team_member SET removed_at = now() WHERE id = $1",
        &[leaver],
    )
    .await;
    for person in [BILLING, leaver] {
        let answer = view(&pool, &proof(person, BUSINESS), id).await;
        assert!(matches!(answer, ViewAnswer::NotFound), "{person}");
    }
}

/// TD-PICK-48 (PRIV-007, PRIV-011): after the purge or "Delete my data" the view says the details
/// were removed and opens nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_48_after_the_purge_or_delete_my_data_the_details_were_removed(pool: PgPool) {
    teams(&pool).await;
    let purged = contact(&pool, ("Testa Example", 6), "testa@example.test").await;
    let deleted = contact(&pool, ("Tester Example", 7), "tester@example.test").await;
    let first = pick(&pool, "paid", Some((purged, true))).await;
    let second = pick(&pool, "free", Some((deleted, false))).await;
    let mut conn = pool.acquire().await.expect("conn");
    assert!(purge_contact(&mut conn, purged).await.expect("purge"));
    delete_my_data(&mut conn, deleted, &[])
        .await
        .expect("delete");
    for id in [first, second] {
        let answer = view(&pool, &proof(OWNER, BUSINESS), id).await;
        assert!(matches!(answer, ViewAnswer::Removed), "{id}");
    }
    assert_eq!(read_rows(&pool).await, 0, "nothing is opened");
}
