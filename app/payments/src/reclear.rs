//! The re-clear of a pick whose release waited for an emergency address (INT-029, PICK-022,
//! SEC-030; T255, D-1563, D-1739, D-3358, D-3384).
//!
//! On an emergency job a release that comes before the address stores a `picks.address_wait` row
//! and releases nothing; the proof it was given is consumed. When the address arrives, `server`'s
//! `intake.address_in` handler first confirms that wait row (`picks::api::awaiting_address`), so
//! the pick already had a proof `paid_release` or `no_fee_release` verified, and then calls
//! [`reclear_release`] with the pick ID only. A pick with a `payments.pick_intent` row is cleared
//! only from its own stored success (`Releasable::from_stored_success`); a pick with none only
//! through `no_fee_release` with the pair of its own `payments.pick_fee` row (D-2302). No redirect,
//! homeowner message, new event or caller value but the pick can clear a pick. `payments` cannot
//! read `picks` tables, so the wait-row precondition is the caller's; a source scan below keeps
//! the callers to `server`'s sink and its test.

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::pick_fee::pick_fee;
use crate::release_proof::{Releasable, ReleaseError, no_fee_release};

/// Re-clears `pick` for the release that waited for its address (INT-029, PICK-022, SEC-030,
/// D-3358): its stored charge success when it has a PaymentIntent, else its recorded $0 decision
/// for its own `payments.pick_fee` business and job. Call it only after
/// `picks::api::awaiting_address` found the pick's wait row, in the transaction that then settles.
///
/// # Errors
/// [`ReleaseError::PickEnded`] or [`ReleaseError::NotPaid`] for a paid pick (see
/// `Releasable::from_stored_success`); [`ReleaseError::NoFeeDecision`] for a pick with no intent
/// and no stored fee or $0 entry; [`ReleaseError::Database`].
pub async fn reclear_release(
    conn: &mut PgConnection,
    pick: Uuid,
) -> Result<Releasable, ReleaseError> {
    match Releasable::from_stored_success(conn, pick).await {
        Err(ReleaseError::WrongIntent) => {}
        other => return other,
    }
    let stored = pick_fee(conn, pick)
        .await
        .map_err(|_| ReleaseError::Database)?
        .ok_or(ReleaseError::NoFeeDecision)?;
    no_fee_release(conn, pick, (stored.business.0, stored.job)).await
}

/// TD-INT-39's `payments` cases (T255, D-3358, D-3431): rows inserted directly, fake IDs only.
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use askhava_privacy::api::ReleaseBasis;
    use sqlx::types::Uuid;
    use sqlx::{PgConnection, PgPool};

    use super::reclear_release;
    use crate::release_proof::{Releasable, ReleaseError};

    const BUSINESS: Uuid = Uuid::from_u128(0xB0);
    const JOB: Uuid = Uuid::from_u128(0xC0);

    async fn run(db: &mut PgConnection, sql: &'static str, pick: Uuid) {
        sqlx::query(sql)
            .bind(pick)
            .bind(BUSINESS)
            .bind(JOB)
            .execute(&mut *db)
            .await
            .expect(sql);
    }

    fn pi(pick: Uuid) -> String {
        format!("pi_{}", pick.simple())
    }

    async fn intent(db: &mut PgConnection, pick: Uuid) {
        let sql = "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, \
                   stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, \
                   'pi_' || replace($1::text, '-', ''), true, now())";
        run(db, sql, pick).await;
    }

    async fn attempt(db: &mut PgConnection, pick: Uuid, try_no: i32, status: &str) {
        sqlx::query(
            "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at) \
             VALUES ($1, $2, gen_random_uuid(), $3, now())",
        )
        .bind(pick)
        .bind(try_no)
        .bind(status)
        .execute(&mut *db)
        .await
        .expect("attempt");
    }

    async fn evidence(db: &mut PgConnection, pick: Uuid, (stripe, cents): (&str, i64)) {
        sqlx::query(
            "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, \
             try_no, idempotency_label, stripe_customer_id, stripe_intent_id, amount_cents, \
             pressed_at, charged_at) SELECT $1, $2, $3, press_id, try_no, \
             'pick-' || $1::text || '-try' || try_no, 'cus_fake1', $4, $5, now(), now() \
             FROM payments.charge_attempt WHERE pick_id = $1 ORDER BY try_no LIMIT 1",
        )
        .bind(pick)
        .bind(BUSINESS)
        .bind(JOB)
        .bind(stripe)
        .bind(cents)
        .execute(&mut *db)
        .await
        .expect("evidence");
    }

    /// A pick with a stored success: intent, one succeeded attempt and its evidence.
    async fn paid(db: &mut PgConnection, pick: Uuid) {
        intent(db, pick).await;
        attempt(db, pick, 1, "succeeded").await;
        evidence(db, pick, (&pi(pick), 7500)).await;
    }

    async fn cleared(db: &mut PgConnection, pick: Uuid) -> Result<Releasable, ReleaseError> {
        reclear_release(db, pick).await
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_int_39_a_stored_success_clears_exactly_that_pick(pool: PgPool) {
        let mut db = pool.acquire().await.expect("connection");
        let (pick, other) = (Uuid::from_u128(1), Uuid::from_u128(2));
        paid(&mut db, pick).await;
        intent(&mut db, other).await;
        let done = cleared(&mut db, pick).await.expect("cleared");
        assert_eq!(
            (done.pick(), done.basis()),
            (pick, ReleaseBasis::PaidCharge)
        );
        assert_eq!(cleared(&mut db, other).await, Err(ReleaseError::NotPaid));
        let unknown = Uuid::from_u128(3);
        assert_eq!(
            Releasable::from_stored_success(&mut db, unknown).await,
            Err(ReleaseError::WrongIntent)
        );
        assert_eq!(
            cleared(&mut db, unknown).await,
            Err(ReleaseError::NoFeeDecision)
        );
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_int_39_an_ended_refunded_or_disputed_pick_is_not_cleared(pool: PgPool) {
        let mut db = pool.acquire().await.expect("connection");
        let ended = Uuid::from_u128(10);
        paid(&mut db, ended).await;
        let end = "UPDATE payments.pick_intent SET ended_at = now() WHERE pick_id = $1 \
                   AND business_id = $2 AND job_id = $3";
        run(&mut db, end, ended).await;
        assert_eq!(cleared(&mut db, ended).await, Err(ReleaseError::PickEnded));
        let statuses = [
            "sending",
            "failed",
            "succeeded",
            "blocked",
            "pending",
            "canceled",
        ];
        for (n, status) in (11_u128..).zip(statuses) {
            let pick = Uuid::from_u128(n);
            paid(&mut db, pick).await;
            sqlx::query(
                "INSERT INTO payments.refund (pick_id, trigger, status, tries, requested_at) \
                 VALUES ($1, 'unreachable', $2, 1, now())",
            )
            .bind(pick)
            .bind(status)
            .execute(&mut *db)
            .await
            .expect("refund");
            assert_eq!(
                cleared(&mut db, pick).await,
                Err(ReleaseError::PickEnded),
                "{status}"
            );
        }
        let disputed = Uuid::from_u128(20);
        paid(&mut db, disputed).await;
        let dispute = "INSERT INTO payments.dispute (stripe_dispute_id, pick_id, amount_cents, \
                       due_at, opened_at) SELECT 'dp_fake20', $1, 7500, now(), now() \
                       WHERE $2::uuid IS NOT NULL AND $3::uuid IS NOT NULL";
        run(&mut db, dispute, disputed).await;
        assert_eq!(
            cleared(&mut db, disputed).await,
            Err(ReleaseError::PickEnded)
        );
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_int_39_no_succeeded_latest_attempt_or_evidence_is_not_paid(pool: PgPool) {
        let mut db = pool.acquire().await.expect("connection");
        let none = Uuid::from_u128(30);
        intent(&mut db, none).await;
        assert_eq!(cleared(&mut db, none).await, Err(ReleaseError::NotPaid));
        let later = Uuid::from_u128(31);
        paid(&mut db, later).await;
        attempt(&mut db, later, 2, "declined").await;
        assert_eq!(cleared(&mut db, later).await, Err(ReleaseError::NotPaid));
        let bare = Uuid::from_u128(32);
        intent(&mut db, bare).await;
        attempt(&mut db, bare, 1, "succeeded").await;
        assert_eq!(cleared(&mut db, bare).await, Err(ReleaseError::NotPaid));
        let other_intent = Uuid::from_u128(33);
        intent(&mut db, other_intent).await;
        attempt(&mut db, other_intent, 1, "succeeded").await;
        evidence(&mut db, other_intent, ("pi_another", 7500)).await;
        assert_eq!(
            cleared(&mut db, other_intent).await,
            Err(ReleaseError::NotPaid)
        );
        let other_amount = Uuid::from_u128(34);
        intent(&mut db, other_amount).await;
        attempt(&mut db, other_amount, 1, "succeeded").await;
        evidence(&mut db, other_amount, (&pi(other_amount), 2500)).await;
        assert_eq!(
            cleared(&mut db, other_amount).await,
            Err(ReleaseError::NotPaid)
        );
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_int_39_the_first_failing_check_wins(pool: PgPool) {
        let mut db = pool.acquire().await.expect("connection");
        let pick = Uuid::from_u128(40);
        intent(&mut db, pick).await;
        attempt(&mut db, pick, 1, "declined").await;
        let end = "UPDATE payments.pick_intent SET ended_at = now() WHERE pick_id = $1 \
                   AND business_id = $2 AND job_id = $3";
        run(&mut db, end, pick).await;
        assert_eq!(cleared(&mut db, pick).await, Err(ReleaseError::PickEnded));
    }

    async fn pick_fee(db: &mut PgConnection, pick: Uuid, job: Uuid) {
        sqlx::query(
            "INSERT INTO payments.pick_fee (pick_id, business_id, job_id, mode, kind, \
             amount_cents, terms_version, fixed_at) VALUES ($1, $2, $3, 'tiers', 'fee_free', 0, \
             1, now())",
        )
        .bind(pick)
        .bind(BUSINESS)
        .bind(job)
        .execute(&mut *db)
        .await
        .expect("pick fee");
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_int_39_the_zero_fee_path_takes_only_the_picks_own_pair(pool: PgPool) {
        let mut db = pool.acquire().await.expect("connection");
        let (free, unrecorded) = (Uuid::from_u128(50), Uuid::from_u128(51));
        let other_job = Uuid::from_u128(0xC1);
        pick_fee(&mut db, free, JOB).await;
        pick_fee(&mut db, unrecorded, other_job).await;
        let entry = "INSERT INTO payments.ledger_entry (kind, business_id, job_id, amount_cents, \
                     idempotency_key) SELECT 'free-first-win', $2, $3, 0, 'fake.' || $1::text";
        run(&mut db, entry, free).await;
        let done = cleared(&mut db, free).await.expect("free");
        assert_eq!((done.pick(), done.basis()), (free, ReleaseBasis::NoFeeDue));
        assert_eq!(
            cleared(&mut db, unrecorded).await,
            Err(ReleaseError::NoFeeDecision)
        );
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_int_39_a_broken_connection_is_a_database_error(pool: PgPool) {
        let mut db = pool.acquire().await.expect("connection");
        let pick = Uuid::from_u128(60);
        paid(&mut db, pick).await;
        sqlx::query("SET default_transaction_read_only = on")
            .execute(&mut *db)
            .await
            .expect("read only");
        assert_eq!(cleared(&mut db, pick).await, Err(ReleaseError::Database));
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_int_39_a_failed_fee_read_is_a_database_error(pool: PgPool) {
        let mut db = pool.acquire().await.expect("connection");
        sqlx::query("REVOKE SELECT ON payments.pick_fee FROM askhava_app")
            .execute(&mut *db)
            .await
            .expect("revoke");
        sqlx::query("SET ROLE askhava_app")
            .execute(&mut *db)
            .await
            .expect("role");
        let pick = Uuid::from_u128(61);
        assert_eq!(cleared(&mut db, pick).await, Err(ReleaseError::Database));
    }

    fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && !path.ends_with("target") {
                sources(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
    }

    /// The source without its `//` comments: docs may name the re-clear, code may not (D-3349).
    fn code_of(text: &str) -> String {
        let code = text
            .lines()
            .map(|l| l.split("//").next().unwrap_or_default());
        code.collect::<Vec<_>>().join("\n")
    }

    /// D-3358: only the re-clear, the `api` export, `server`'s sink and its test name
    /// `reclear_release`, and only `payments`' own sources name `from_stored_success`.
    #[test]
    fn td_int_39_only_the_named_files_call_the_re_clear() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files = Vec::new();
        for top in ["app", "tests"] {
            sources(&root.join(top), &mut files);
        }
        assert!(files.len() > 100, "the scan found the workspace");
        let allowed = [
            "app/payments/src/reclear.rs",
            "app/payments/src/api.rs",
            "app/server/src/release_sink.rs",
            "app/server/tests/address_rerelease.rs",
        ];
        let mut wrong = Vec::new();
        for file in files {
            let text = code_of(&std::fs::read_to_string(&file).unwrap_or_default());
            let name = file.strip_prefix(&root).unwrap_or(&file).to_string_lossy();
            let name = name.replace('\\', "/");
            if text.contains("reclear_release") && !allowed.contains(&name.as_str()) {
                wrong.push(name.clone());
            }
            if text.contains("from_stored_success") && !name.starts_with("app/payments/src/") {
                wrong.push(name);
            }
        }
        assert!(wrong.is_empty(), "named outside their files: {wrong:?}");
    }
}
