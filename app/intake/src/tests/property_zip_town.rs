//! The job's ZIP is read only from a property in the job's own town (CARD-024, INT-049, D-1521;
//! T444, D-3563 (b); related: TD-INT-38, the ZIP step's entry).
//!
//! `cards_out::property_zip` is keyed by the town `models::property_town` read, so a property
//! stored between the two reads in another town is never paired with the older town. That race
//! cannot be made on demand through the public API, so these cases call `property_zip` directly
//! with a homeowner who has an older property in one town and a newer one in another. Fake IDs
//! only; the ZIPs are public area codes.

use std::sync::atomic::{AtomicU64, Ordering};

use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;

use crate::api::{TextIn, conversation, receive_text};
use crate::cards_out::property_zip;
use crate::models;
use crate::rules::{Inbound, Settings};

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const OLDER: (&str, &str) = ("lake_havasu_city", "86403");
const NEWER: (&str, &str) = ("kingman", "86401");

/// A new fake ID for each call.
fn fresh() -> Uuid {
    static NEXT: AtomicU64 = AtomicU64::new(0x4440_0000);
    Uuid::from_u128(u128::from(NEXT.fetch_add(1, Ordering::Relaxed)))
}

/// A fake request whose homeowner has an older property in `OLDER`'s town and a newer one in
/// `NEWER`'s town; returns the request ID.
async fn two_towns(db: &mut Db) -> Uuid {
    let contact = fresh();
    let at: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
    let first = TextIn {
        inbox_id: fresh(),
        contact,
        inbound: Inbound {
            words: "my sink is leaking",
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at,
    };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let handled = receive_text(db, &secret, &first, Settings::default()).await;
    let id = handled.expect("open").conversation.expect("request");
    let row = conversation(db, id).await.expect("read").expect("row");
    for (minute, place) in [(1, OLDER), (2, NEWER)] {
        let stored = at + SignedDuration::from_mins(minute);
        models::insert_property(db, (row.homeowner_id, contact), place, stored)
            .await
            .expect("property");
    }
    id
}

/// CARD-024, D-1521 (INT-049): keyed by the older property's town, the older property's ZIP is
/// read, never the newer property's ZIP from another town.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn card_024_the_older_town_reads_its_own_zip(mut db: Db) {
    let id = two_towns(&mut db).await;
    let zip = property_zip(&mut db, id, OLDER.0).await.expect("zip");
    assert_eq!(zip.as_deref(), Some(OLDER.1));
}

/// CARD-024, D-1521 (INT-049): keyed by the newer property's town, the newer property's ZIP is
/// read.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn card_024_the_newer_town_reads_its_own_zip(mut db: Db) {
    let id = two_towns(&mut db).await;
    let zip = property_zip(&mut db, id, NEWER.0).await.expect("zip");
    assert_eq!(zip.as_deref(), Some(NEWER.1));
}

/// CARD-024, D-1521 (INT-049): a town with no property of the homeowner reads no ZIP.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn card_024_a_town_with_no_property_reads_no_zip(mut db: Db) {
    let id = two_towns(&mut db).await;
    let zip = property_zip(&mut db, id, "bullhead_city")
        .await
        .expect("zip");
    assert_eq!(zip, None);
}
