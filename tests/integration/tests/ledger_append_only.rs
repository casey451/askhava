//! TD-FEE-16 (append-only case; FEE-028, FEE-029, FEE-030, D-092, D-1495): as `askhava_app`, a
//! `payments.ledger_entry` row and a `payments.reconcile_charge` row can be read and inserted but
//! never updated, deleted or truncated (SQLSTATE 42501), so the money history the reconcile and
//! the statements read cannot be rewritten; a wrong entry is fixed by a new one. Fake IDs only.

use sqlx::PgPool;

/// The rows each table gets as the app role, then the rewrites it must refuse.
const TABLES: [(&str, &str, [&str; 3]); 2] = [
    (
        "INSERT INTO payments.ledger_entry (kind, business_id, job_id, amount_cents, stripe_id,
           idempotency_key) VALUES ('charge', gen_random_uuid(), gen_random_uuid(), 7500,
           'ch_FakeT067', 'fake.t067.append-only')",
        "SELECT count(*) FROM payments.ledger_entry",
        [
            "UPDATE payments.ledger_entry SET amount_cents = 1",
            "DELETE FROM payments.ledger_entry",
            "TRUNCATE payments.ledger_entry",
        ],
    ),
    (
        "INSERT INTO payments.reconcile_charge (stripe_charge_id, stripe_intent_id, pick_id,
           refund_reason, seen_at) VALUES ('ch_FakeT067', 'pi_FakeT067', NULL, 'no-pick', now())",
        "SELECT count(*) FROM payments.reconcile_charge",
        [
            "UPDATE payments.reconcile_charge SET refund_reason = NULL",
            "DELETE FROM payments.reconcile_charge",
            "TRUNCATE payments.reconcile_charge",
        ],
    ),
];

/// TD-FEE-16 (FEE-028, D-092): the ledger and the reconcile record are append-only for the app
/// role, each rewrite refused with 42501 and the row still there.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_16_the_app_role_cannot_rewrite_the_ledger(pool: PgPool) {
    for (insert, read, rewrites) in TABLES {
        let mut conn = pool.acquire().await.expect("conn");
        sqlx::query("SET ROLE askhava_app")
            .execute(&mut *conn)
            .await
            .expect("role");
        sqlx::query(insert)
            .execute(&mut *conn)
            .await
            .expect("insert");
        for rewrite in rewrites {
            let err = sqlx::query(rewrite)
                .execute(&mut *conn)
                .await
                .expect_err(rewrite);
            let code = err.as_database_error().and_then(|e| e.code());
            assert_eq!(code.as_deref(), Some("42501"), "{rewrite}");
        }
        let rows: i64 = sqlx::query_scalar(read)
            .fetch_one(&mut *conn)
            .await
            .expect("read");
        assert_eq!(rows, 1, "{read}");
        sqlx::query("RESET ROLE")
            .execute(&mut *conn)
            .await
            .expect("reset");
    }
}
