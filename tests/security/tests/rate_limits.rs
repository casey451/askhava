//! Rate limits and brute-force stops (T089; TD-SEC-02, TD-SEC-19, TD-PICK-36 portal cases;
//! SEC-014, SEC-015, SEC-020, SEC-040, PICK-055): 5 new requests per phone per hour, a burst of
//! 100 fake requests blocked and alerted, code, link and owner-password brute force stopped,
//! exact counts across two pools, the client IP only from the edge header the setting names, and
//! the portal Verify caps. Every IP, ID and number is fake (documentation ranges).

use std::net::IpAddr;

use askhava_core::api::ClientIpPolicy;
use askhava_intake::api::{
    ALERT_TRIES, Admission, Key, Limit, LimitError, REQUESTS_ALL, REQUESTS_PER_IP,
    REQUESTS_PER_PHONE, admit, admit_new_request,
};
use askhava_portals::api::{
    CODE_SENDS_PER_IP, CODE_SENDS_PER_NUMBER, CODE_SENDS_UNKNOWN, CODE_TRIES_PER_BUSINESS,
    CODE_TRIES_PER_IP, LINKS_PER_BUSINESS, OWNER_TRIES_ALL, OWNER_TRIES_PER_IP, OwnerAuth,
    QUOTES_PER_BUSINESS, SIGNUPS_ALL, admit_code_send, admit_code_try, admit_owner_password,
    admit_quote, admit_sign_in_link, admit_sign_up, passkey_sign_in_options, verify_all,
};
use axum::http::{HeaderMap, HeaderValue};
use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;

/// 2026-09-22 12:00 UTC (5 a.m. in Arizona), the start of an hour.
fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_078_400).expect("time") + SignedDuration::from_secs(secs)
}

fn ip(n: u32) -> IpAddr {
    IpAddr::from([198, 51, (n / 250) as u8, (n % 250) as u8 + 1])
}

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x7e57_0000_0000_0000_0000_0000_0000_0000 + n)
}

/// How many of `outcomes` were admitted.
fn admitted(outcomes: &[Admission]) -> usize {
    outcomes
        .iter()
        .filter(|a| **a == Admission::Admitted)
        .count()
}

/// Every counter hit stored so far, all buckets together.
async fn counter_hits(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(hits), 0)::bigint FROM core.rate_limit")
        .fetch_one(pool)
        .await
        .expect("hits")
}

async fn alerts(pool: &PgPool, kind: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT severity FROM ops.alert WHERE kind = $1")
        .bind(kind)
        .fetch_all(pool)
        .await
        .expect("alerts")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_02_five_new_requests_per_phone_per_hour_and_the_sixth_is_refused(pool: PgPool) {
    let mut outcomes = Vec::new();
    for n in 0..6 {
        let ask = admit_new_request(&pool, (id(1), Some(ip(n))), at(i64::from(n))).await;
        outcomes.push(ask.expect("counted"));
    }
    assert_eq!(admitted(&outcomes), 5);
    assert_eq!(outcomes[5], Admission::Refused(REQUESTS_PER_PHONE));
    assert_eq!(
        alerts(&pool, REQUESTS_PER_PHONE.scope).await,
        ["normal"],
        "one homeowner's limit is a digest line, not a High alert (D-1953)"
    );
    let next_hour = admit_new_request(&pool, (id(1), None), at(3_600)).await;
    assert_eq!(next_hour.expect("counted"), Admission::Admitted);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_02_a_burst_of_100_fake_requests_in_an_hour_is_blocked_and_alerted(pool: PgPool) {
    let mut spread = Vec::new();
    for n in 0..100 {
        let who = (id(u128::from(n)), Some(ip(n)));
        spread.push(
            admit_new_request(&pool, who, at(i64::from(n) * 30))
                .await
                .expect("counted"),
        );
    }
    assert_eq!(admitted(&spread), REQUESTS_ALL.max as usize);
    assert!(
        spread[40..]
            .iter()
            .all(|a| *a == Admission::Refused(REQUESTS_ALL))
    );
    assert_eq!(
        alerts(&pool, REQUESTS_ALL.scope).await,
        ["high"],
        "one alert, not 60"
    );
    let mut one_ip = Vec::new();
    for n in 0..100_u32 {
        let who = (id(1_000 + u128::from(n)), Some(ip(900)));
        one_ip.push(
            admit_new_request(&pool, who, at(7_200))
                .await
                .expect("counted"),
        );
    }
    assert_eq!(admitted(&one_ip), REQUESTS_PER_IP.max as usize);
    assert_eq!(alerts(&pool, REQUESTS_PER_IP.scope).await.len(), 1);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_02_brute_force_on_codes_links_and_owner_sign_in_stops_at_the_limits(pool: PgPool) {
    let mut tries = Vec::new();
    for _ in 0..25 {
        tries.push(
            admit_code_try(&pool, (ip(1), None), at(0))
                .await
                .expect("counted"),
        );
    }
    assert_eq!(admitted(&tries), CODE_TRIES_PER_IP.max as usize);
    let mut spread = Vec::new();
    for n in 0..25 {
        let ask = admit_code_try(&pool, (ip(10 + n), Some(id(7))), at(0)).await;
        spread.push(ask.expect("counted"));
    }
    assert_eq!(
        admitted(&spread),
        CODE_TRIES_PER_BUSINESS.max as usize,
        "IP hopping"
    );
    let mut links = Vec::new();
    for n in 0..15 {
        let ask = admit_sign_in_link(&pool, (ip(100 + n), Some(id(8))), at(0)).await;
        links.push(ask.expect("counted"));
    }
    assert_eq!(admitted(&links), LINKS_PER_BUSINESS.max as usize);
    let mut owner = Vec::new();
    for _ in 0..8 {
        owner.push(
            admit_owner_password(&pool, ip(200), at(0))
                .await
                .expect("counted"),
        );
    }
    assert_eq!(admitted(&owner), OWNER_TRIES_PER_IP.max as usize);
    let mut hopping = Vec::new();
    for n in 0..30 {
        hopping.push(
            admit_owner_password(&pool, ip(300 + n), at(0))
                .await
                .expect("counted"),
        );
    }
    let left = (OWNER_TRIES_ALL.max - OWNER_TRIES_PER_IP.max) as usize;
    assert_eq!(
        admitted(&hopping),
        left,
        "the account limit holds across IPs"
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_19_twenty_concurrent_requests_on_two_processes_admit_exactly_five(pool: PgPool) {
    let other = PgPoolOptions::new()
        .connect_with(pool.connect_options().as_ref().clone())
        .await
        .expect("second process pool");
    let mut running = Vec::new();
    for n in 0..20 {
        let pool = if n % 2 == 0 {
            pool.clone()
        } else {
            other.clone()
        };
        running.push(tokio::spawn(async move {
            admit_new_request(&pool, (id(42), None), at(0))
                .await
                .expect("counted")
        }));
    }
    let mut outcomes = Vec::new();
    for task in running {
        outcomes.push(task.await.expect("joined"));
    }
    assert_eq!(admitted(&outcomes), 5);
}

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.insert(*name, HeaderValue::from_str(value).expect("header"));
    }
    map
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_19_the_client_ip_comes_only_from_the_header_the_setting_names(pool: PgPool) {
    let settings = [
        ("ASKHAVA_CLIENT_IP_HEADER", "x-edge-client"),
        ("ASKHAVA_PROXIED_HOSTS", "proxied.askhava.test"),
    ];
    let policy = ClientIpPolicy::from_pairs(settings.map(|(k, v)| (k.to_owned(), v.to_owned())))
        .expect("policy");
    let peer: IpAddr = [10, 0, 0, 1].into();
    let mut outcomes = Vec::new();
    for n in 0..12 {
        let spoof = format!("203.0.113.{n}");
        let sent = headers(&[
            ("x-edge-client", "198.51.100.9"),
            ("x-forwarded-for", &spoof),
        ]);
        let client = policy.client_ip(&sent, peer);
        assert_eq!(client, IpAddr::from([198, 51, 100, 9]));
        outcomes.push(
            admit_code_send(&pool, (client, None), (at(0), 500))
                .await
                .expect("ok"),
        );
    }
    assert_eq!(
        admitted(&outcomes),
        CODE_SENDS_PER_IP.max as usize,
        "spoofed XFF"
    );
    let cf = ("cf-connecting-ip", "192.0.2.77");
    let dns_only = headers(&[
        ("host", "askhava.test"),
        ("x-edge-client", "173.245.48.5"),
        cf,
    ]);
    assert_eq!(
        policy.client_ip(&dns_only, peer),
        IpAddr::from([173, 245, 48, 5])
    );
    let from_cf = headers(&[
        ("host", "proxied.askhava.test"),
        ("x-edge-client", "173.245.48.5"),
        cf,
    ]);
    assert_eq!(
        policy.client_ip(&from_cf, peer),
        IpAddr::from([192, 0, 2, 77])
    );
    let not_cf = headers(&[
        ("host", "proxied.askhava.test"),
        ("x-edge-client", "198.51.100.9"),
        cf,
    ]);
    assert_eq!(
        policy.client_ip(&not_cf, peer),
        IpAddr::from([198, 51, 100, 9])
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_19_the_passkey_path_is_never_hard_locked_by_password_throttling(pool: PgPool) {
    for n in 0..25 {
        admit_owner_password(&pool, ip(n), at(0))
            .await
            .expect("counted");
    }
    let refused = admit_owner_password(&pool, ip(30), at(0))
        .await
        .expect("counted");
    assert_eq!(refused, Admission::Refused(OWNER_TRIES_ALL));
    let auth = OwnerAuth::new("askhava.test", "https://askhava.test").expect("relying party");
    let mut db = pool.acquire().await.expect("connection");
    let options = passkey_sign_in_options(&mut db, &auth, at(1)).await;
    assert!(
        options.is_ok(),
        "the passkey path answers while passwords are throttled"
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_36_portal_code_sends_are_capped_per_number_and_globally(pool: PgPool) {
    let mut sends = Vec::new();
    for n in 0..6 {
        let ask = admit_code_send(&pool, (ip(n), Some(id(5))), (at(i64::from(n)), 500)).await;
        sends.push(ask.expect("counted"));
    }
    assert_eq!(admitted(&sends), 5);
    assert_eq!(sends[5], Admission::Refused(CODE_SENDS_PER_NUMBER));
    let next_arizona_day = at(19 * 3_600);
    let next_day = admit_code_send(&pool, (ip(9), Some(id(5))), (next_arizona_day, 500)).await;
    assert_eq!(
        next_day.expect("counted"),
        Admission::Admitted,
        "a new Arizona day"
    );
    let mut global = Vec::new();
    for n in 0..5 {
        let who = (ip(50 + n), Some(id(100 + u128::from(n))));
        global.push(
            admit_code_send(&pool, who, (at(2 * 86_400), 3))
                .await
                .expect("counted"),
        );
    }
    assert_eq!(admitted(&global), 3);
    assert_eq!(global[3], Admission::Refused(verify_all(3)));
    assert_eq!(alerts(&pool, verify_all(3).scope).await, ["high"]);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_02_fifty_sign_ups_a_day_and_quotes_per_business(pool: PgPool) {
    let mut signups = Vec::new();
    for n in 0..55 {
        signups.push(
            admit_sign_up(&pool, ip(n), at(i64::from(n)))
                .await
                .expect("counted"),
        );
    }
    assert_eq!(admitted(&signups), SIGNUPS_ALL.max as usize);
    let mut quotes = Vec::new();
    for n in 0..35 {
        quotes.push(
            admit_quote(&pool, (id(9), Some(ip(n))), at(0))
                .await
                .expect("counted"),
        );
    }
    assert_eq!(admitted(&quotes), QUOTES_PER_BUSINESS.max as usize);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_36_unknown_and_known_numbers_spend_the_same_counter_writes(pool: PgPool) {
    let known = admit_code_send(&pool, (ip(1), Some(id(7))), (at(0), 500)).await;
    assert_eq!(known.expect("counted"), Admission::Admitted);
    let after_known = counter_hits(&pool).await;
    let unknown = admit_code_send(&pool, (ip(2), None), (at(0), 500)).await;
    assert_eq!(unknown.expect("counted"), Admission::Admitted);
    let after_unknown = counter_hits(&pool).await;
    assert_eq!(after_known, 3, "a known number: IP, number and global");
    assert_eq!(
        after_unknown - after_known,
        after_known,
        "an unknown number does the same number of upserts (SEC-008, D-1954)"
    );
    assert!(alerts(&pool, CODE_SENDS_UNKNOWN.scope).await.is_empty());
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_02_an_alert_that_cannot_be_written_is_an_error_not_a_silent_refusal(pool: PgPool) {
    let broken = Limit::hourly("intake.Not-A-Code-Name", 1);
    let checks = [(broken, Key::All)];
    let first = admit(&pool, &checks, at(0)).await;
    assert_eq!(first.expect("counted"), Admission::Admitted);
    assert_eq!(
        admit(&pool, &checks, at(1)).await,
        Err(LimitError::Alert),
        "every one of the tries failed, so the caller hears of it"
    );
    assert_eq!(ALERT_TRIES, 3, "a passing database fault gets more tries");
}
