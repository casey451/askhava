//! The production release: `payments`' cleared pick becomes a paid or free pick, a proof and
//! one release (PICK-022, PICK-023, PRIV-005, BIZ-014, CALL-001; T254, D-1563, D-1564, D-1739,
//! D-1740).
//!
//! [`PickReleaser`] is the `payments` `ReleaseSink` the Stripe webhook route hands each
//! [`Releasable`] (`paid_release`), and the same sink takes `no_fee_release`'s for a $0 decision.
//! For each it settles the pick (`picks::api::settle`: awaiting confirm to `paid` or `free`, one
//! commit) and releases whenever the pick is now `paid` or `free` for the clearing's basis,
//! whether this call moved it or an earlier one did, so a restart between the settle's commit
//! and the release's claim releases on the retry, exactly once (the release row is the once). A
//! pick that left awaiting confirm (cancelled, expired) is not moved and nothing is released. The
//! homeowner's contact is the one `intake::api::job_homeowner` names (T246's page pick reads the
//! same); the proof is minted with [`release_minter`] and `picks::api::release` gets the vault
//! and the masked pool. `email_chosen` is the homeowner's PICK-018 choice stored with her page
//! pick, `picks::api::contact_choice` (T266, D-1761): email opens the address she shared, a call
//! or a text pick (no stored choice) is a masked call. `Already`, `MovedOn` and
//! `WaitingForAddress` are success to the route; a database or vault failure is
//! `ReleaseError::Database`, its 500, so Stripe sends the event again.
//!
//! T433 (FEE-021, D-1547): a paid clearing whose pick `picks` left (cancelled, expired) is a late
//! success: in the settle's transaction `payments::api::late_success` queues its refund and Casey's
//! alert, once per pick, and nothing is released.
//!
//! [`stripe_webhook`] builds the route's `StripeWebhook` from the settings (SEC-036, SEC-039,
//! SEC-049): the Stripe client, the `whsec_…` signing secret, the configured account and mode,
//! the SEC-040 client-address policy, the vault and the masked pool.
//!
//! T255 (INT-029, D-1563, D-1739, D-3358, D-3431): [`address_in`], the `intake.address_in`
//! handler ([`register_address_in`]), re-releases an emergency pick whose release waited for the
//! address: `picks::api::awaiting_address`, `payments::api::reclear_release` (its only caller)
//! and `picks::api::settle` in one transaction, then the same mint and release, so the masked
//! line opens then.
//!
//! T408 (OPS-001, OPS-024, D-3144): in safe mode `settle_and_release` holds the pick instead.
//! T707 (D-4419): in safe mode an `intake.address_in` run releases nothing and queues the next
//! run 15 minutes later (`address_in_due`); an unreadable safe mode fails closed. Every settle,
//! held or not, leaves a pick whose job is already final (FEE-017, FEE-021, D-4791).

use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use askhava_adapters::api::{AdapterMode, MaskedPool, StripePayments, TwilioConfig};
use askhava_core::api::{
    ClientIpPolicy, Clock, Environment, HandlerError, NewTask, Registry, Settings, SystemClock,
    Vendor, check_base_url, enqueue,
};
use askhava_intake::api::{ADDRESS_IN_KIND, job_homeowner};
use askhava_jobs::api::current;
use askhava_ops::api::{NewAlert, Severity, raise_alert, safe_mode_on};
use askhava_payments::api::{
    MoneyEventError, PickNow, Releasable, ReleaseError, ReleaseSink, StripeAccount, StripeWebhook,
    late_success, reclear_release,
};
use askhava_picks::api::{
    Released, Settled, awaiting_address, contact_choice, is_released, release, settle,
};
use askhava_privacy::api::{ReleaseBasis, Vault};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool, Postgres, Transaction};

use crate::release_hold::HOLD_EVERY;
use crate::{release_hold, release_minter};

/// The Stripe webhook signing secret (`whsec_…`, SEC-039).
pub const WEBHOOK_SECRET: &str = "ASKHAVA_STRIPE_WEBHOOK_SECRET";
/// `stand_in`, `sandbox` or `live` for Stripe; `live` only in live (SEC-002, SEC-049).
pub const STRIPE_MODE: &str = "ASKHAVA_ADAPTER_MODE_STRIPE";
/// The Stripe API base URL; unset, the stand-in's or Stripe's own by mode (SEC-032).
pub const STRIPE_BASE_URL: &str = "ASKHAVA_STRIPE_BASE_URL";
/// The High alert raised when a cleared pick's job has no open request naming its homeowner, so
/// no proof can be minted (PICK-022, D-1739).
pub const NO_CONTACT_ALERT: &str = "picks.release_no_contact";

const BAD_ADDRESS_IN: &str = "bad intake.address_in payload";
const ADDRESS_IN_FAILED: &str = "the re-release after the address failed";

/// The stand-in's host, the only source a stand-in webhook may come from (SEC-049).
const STAND_IN_HOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const STAND_IN_STRIPE: &str = "http://127.0.0.1:4010/stripe";
const STRIPE_API: &str = "https://api.stripe.com";

/// The production Stripe webhook route's state: the real Stripe client and this sink (D-1480).
pub type StripeHooks = Arc<StripeWebhook<StripePayments, PickReleaser>>;

/// The release sink: the pool, the vault the release reads, the masked pool its line comes from
/// and the clock (PICK-022, CALL-001).
pub struct PickReleaser {
    pool: PgPool,
    vault: Vault,
    masked: MaskedPool,
    clock: Arc<dyn Clock>,
}

impl PickReleaser {
    /// A sink over `pool`, `vault` and `masked` on `clock`.
    #[must_use]
    pub fn new(pool: PgPool, vault: Vault, masked: MaskedPool, clock: Arc<dyn Clock>) -> Self {
        Self {
            pool,
            vault,
            masked,
            clock,
        }
    }

    /// Settles, mints and releases `cleared` (see the module docs; PICK-022, PRIV-005, BIZ-014,
    /// D-1739): `MovedOn` for a pick that left awaiting confirm (a paid one's late-success refund
    /// queued, FEE-021), else the release's answer. While safe mode is on (OPS-001, OPS-024,
    /// D-3144) nothing is settled or released: a pick already released answers `Already`, any
    /// other queues its one held-release task (`release_hold`) and answers `MovedOn`.
    ///
    /// # Errors
    /// [`ReleaseError::Database`] when the safe-mode read (fails closed: nothing settled, released
    /// or queued), the hold, the settle, the contact lookup, the vault read or the release's
    /// writes fail; the release, once claimed, stands.
    pub async fn settle_and_release(&self, cleared: Releasable) -> Result<Released, ReleaseError> {
        let at = self.clock.now();
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|_| ReleaseError::Database)?;
        if safe_mode_on(&mut conn)
            .await
            .map_err(|_| ReleaseError::Database)?
        {
            return release_hold::hold(&mut conn, cleared.pick(), at).await;
        }
        drop(conn);
        self.settle_release(cleared, at).await
    }

    /// The releaser's clock: the instant a held release is cleared and released at (OPS-024).
    pub(crate) fn now(&self) -> Timestamp {
        self.clock.now()
    }

    /// The held-release task's settle and release, after its own safe-mode read (OPS-024,
    /// FEE-017, FEE-021, PICK-022, D-4419): as [`Self::settle_and_release`] once safe mode is
    /// off: a pick whose job went final (cancelled) while it was held is not moved, a paid one
    /// gets its one late-success refund and the answer is `MovedOn`.
    pub(crate) async fn release_now(
        &self,
        cleared: Releasable,
        at: Timestamp,
    ) -> Result<Released, ReleaseError> {
        self.settle_release(cleared, at).await
    }

    /// Settles and releases `cleared` at `at`; on every path (held or not, D-4791) a pick moved
    /// now for a job already final is rolled back and left (FEE-017, PICK-022, T707), so a later
    /// verified event for that pick releases nothing either.
    async fn settle_release(
        &self,
        cleared: Releasable,
        at: Timestamp,
    ) -> Result<Released, ReleaseError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ReleaseError::Database)?;
        let settled = settle(&mut tx, &cleared)
            .await
            .map_err(|_| ReleaseError::Database)?;
        if job_went_final(&mut tx, settled, cleared.pick()).await? {
            return leave_final(&self.pool, tx, &cleared, at).await;
        }
        if settled == Settled::Left && matches!(cleared.basis(), ReleaseBasis::PaidCharge) {
            moved_on_success(&mut tx, cleared.pick(), at).await?;
        }
        tx.commit().await.map_err(|_| ReleaseError::Database)?;
        let keys = (&self.vault, &self.masked);
        release_settled(&self.pool, keys, (cleared, settled), at).await
    }
}

/// FEE-017, OPS-024, PICK-022 (T707, D-4792): whether the settled pick's job is already in a
/// final status (cancelled, such as while its release was held or its address deferred) with
/// the pick not yet released: a pick the settle just moved (`Settled::Moved`), or one already
/// paid or free (`Settled::Was`) with no `picks.release` row; a `Was` pick already released is
/// left to [`release_settled`], which answers `Already`, so nothing is refunded.
async fn job_went_final(
    conn: &mut PgConnection,
    settled: Settled,
    pick: Uuid,
) -> Result<bool, ReleaseError> {
    let job = match settled {
        Settled::Left => return Ok(false),
        Settled::Moved(job) => job,
        Settled::Was(job) => {
            let claimed = is_released(&mut *conn, pick).await;
            if claimed.map_err(|_| ReleaseError::Database)? {
                return Ok(false);
            }
            job
        }
    };
    let found = current(conn, job)
        .await
        .map_err(|_| ReleaseError::Database)?;
    Ok(found.status.is_final())
}

/// FEE-017, FEE-021, PICK-022 (T707, D-4791, D-4792): leaves a pick whose job went final: the
/// settle's transaction `tx` is rolled back (no move, no re-clear), a paid pick gets its one
/// late-success refund ([`moved_on_success`]) in a new one, and nothing is released (`MovedOn`).
async fn leave_final(
    pool: &PgPool,
    tx: Transaction<'_, Postgres>,
    cleared: &Releasable,
    at: Timestamp,
) -> Result<Released, ReleaseError> {
    tx.rollback().await.map_err(|_| ReleaseError::Database)?;
    if matches!(cleared.basis(), ReleaseBasis::PaidCharge) {
        let mut tx = pool.begin().await.map_err(|_| ReleaseError::Database)?;
        moved_on_success(&mut tx, cleared.pick(), at).await?;
        tx.commit().await.map_err(|_| ReleaseError::Database)?;
    }
    Ok(Released::MovedOn)
}

/// FEE-021, D-1547 (T433): a verified paid success for a pick `picks` left (`Settled::Left`) is a
/// late success: `payments::api::late_success` with `PickNow::MovedOn`, in the settle's
/// transaction, queues its refund and raises Casey's alert once per pick; nothing is released. A
/// pick with no stored charge has nothing to refund.
async fn moved_on_success(
    conn: &mut PgConnection,
    pick: Uuid,
    at: Timestamp,
) -> Result<(), ReleaseError> {
    match late_success(conn, pick, PickNow::MovedOn, at).await {
        Ok(_) | Err(MoneyEventError::NothingCharged) => Ok(()),
        Err(_) => Err(ReleaseError::Database),
    }
}

/// Mints and releases a settled clearing: `MovedOn` for a pick the settle left, else the
/// release for the job's homeowner contact with her stored contact choice as `email_chosen`
/// (PICK-018, PICK-022, PICK-042, PRIV-005, D-1739, D-1761); the first release and T255's
/// re-release both come here.
async fn release_settled(
    pool: &PgPool,
    (vault, masked): (&Vault, &MaskedPool),
    (cleared, settled): (Releasable, Settled),
    at: Timestamp,
) -> Result<Released, ReleaseError> {
    let job = match settled {
        Settled::Left => return Ok(Released::MovedOn),
        Settled::Moved(job) | Settled::Was(job) => job,
    };
    let Some(contact) = contact_of(pool, cleared.pick(), job).await? else {
        return Ok(Released::MovedOn);
    };
    let email_chosen = email_chosen(pool, cleared.pick()).await?;
    let proof = release_minter().mint(cleared, contact);
    release(pool, (vault, masked), (proof, email_chosen), at)
        .await
        .map_err(|_| ReleaseError::Database)
}

/// Whether the homeowner chose email at the page pick (`picks::api::contact_choice`; PICK-018,
/// D-1761); a text pick is a masked call.
async fn email_chosen(pool: &PgPool, pick: Uuid) -> Result<bool, ReleaseError> {
    let mut conn = pool.acquire().await.map_err(|_| ReleaseError::Database)?;
    let choice = contact_choice(&mut conn, pick)
        .await
        .map_err(|_| ReleaseError::Database)?;
    Ok(choice.email_chosen())
}

/// The job's homeowner contact through `intake` (D-1602); with none, one High
/// [`NO_CONTACT_ALERT`] about the pick and `None` (the release is `MovedOn` and nothing
/// retries it; a person acts on the alert, D-2017).
async fn contact_of(pool: &PgPool, pick: Uuid, job: Uuid) -> Result<Option<Uuid>, ReleaseError> {
    let mut conn = pool.acquire().await.map_err(|_| ReleaseError::Database)?;
    let found = job_homeowner(&mut conn, job)
        .await
        .map_err(|_| ReleaseError::Database)?;
    if found.is_none() {
        let alert = NewAlert {
            kind: NO_CONTACT_ALERT,
            severity: Severity::High,
            subject: Some(("pick", pick)),
        };
        raise_alert(&mut conn, &alert)
            .await
            .map_err(|_| ReleaseError::Database)?;
    }
    Ok(found.map(|h| h.contact))
}

/// The re-release once an emergency address arrived (INT-029, PICK-022, SEC-030, OPS-001,
/// OPS-024, D-1563, D-1739, D-3358, D-3384, D-4419): [`address_in_due`] with no due time, so
/// safe mode is read first and, while it is on, nothing is released and the next run is queued.
/// Then, in one transaction, `picks::api::awaiting_address` must find the pick's
/// `picks.address_wait` row (with none, nothing is re-cleared or released),
/// `payments::api::reclear_release` re-clears it from its own stored success or recorded $0
/// decision and `picks::api::settle` checks it is still `paid` or `free`; the commit comes after
/// the settle, so no refund starts between the check and the settle. Then the same mint and
/// release as a first clearing: the address goes at most once (the release row), only through
/// that proof, and the masked line opens. A pick that left paid or free, ended, was refunded or
/// disputed is `MovedOn`; so is one whose job went final while it waited (FEE-017, FEE-021,
/// D-4792: nothing re-cleared, a paid pick's one late-success refund); a second address finds
/// the release row, `Already`.
///
/// # Errors
/// [`ReleaseError::Database`] when the safe-mode read (fails closed: nothing released or
/// queued), the deferral, a read, the settle, the vault read or the release's writes fail; the
/// task is tried again.
pub async fn address_in(
    pool: &PgPool,
    keys: (&Vault, &MaskedPool),
    pick: Uuid,
    at: Timestamp,
) -> Result<Released, ReleaseError> {
    address_in_due(pool, keys, pick, (at, None)).await
}

/// The safe-mode gate of the re-release (OPS-001, OPS-024, D-3263, D-3879, D-4419): `run_at` is
/// the run's payload due time (`None` for `intake`'s first run). An unreadable `ops.safe_mode`
/// is `ReleaseError::Database` (fails closed); while it is on, nothing is released, the next
/// run is queued ([`defer_address_in`]) and the answer is `MovedOn`; else [`rerelease`].
async fn address_in_due(
    pool: &PgPool,
    keys: (&Vault, &MaskedPool),
    pick: Uuid,
    (at, run_at): (Timestamp, Option<Timestamp>),
) -> Result<Released, ReleaseError> {
    let mut conn = pool.acquire().await.map_err(|_| ReleaseError::Database)?;
    if safe_mode_on(&mut conn)
        .await
        .map_err(|_| ReleaseError::Database)?
    {
        defer_address_in(&mut conn, pick, at, run_at).await?;
        return Ok(Released::MovedOn);
    }
    drop(conn);
    rerelease(pool, keys, pick, at).await
}

/// Queues the next [`ADDRESS_IN_KIND`] run of `pick` [`HOLD_EVERY`] after the run's due time
/// (`at` when it has none), payload `{"pick", "run_at"}` (OPS-024, D-3263, D-4419). The first
/// deferral's key is `intake.address_in.<pick>.held` (`intake`'s own `intake.address_in.<pick>`
/// is used for all time), every later one `intake.address_in.<pick>.<run_at microseconds>` of
/// the run it follows, so a rerun of a run queues nothing.
async fn defer_address_in(
    conn: &mut PgConnection,
    pick: Uuid,
    at: Timestamp,
    run_at: Option<Timestamp>,
) -> Result<(), ReleaseError> {
    let (due, key) = match run_at {
        None => (at, format!("{ADDRESS_IN_KIND}.{pick}.held")),
        Some(due) => (
            due,
            format!("{ADDRESS_IN_KIND}.{pick}.{}", due.as_microsecond()),
        ),
    };
    let next = due.checked_add(HOLD_EVERY).unwrap_or(due);
    let payload = json!({"pick": pick, "run_at": next.to_string()});
    let mut task = NewTask::new(ADDRESS_IN_KIND, payload, next);
    task.dedupe_key = Some(key);
    enqueue(conn, &task)
        .await
        .map(|_| ())
        .map_err(|_| ReleaseError::Database)
}

/// The re-release itself, once safe mode is off (see [`address_in`]; INT-029, PICK-022,
/// SEC-030, D-3384).
async fn rerelease(
    pool: &PgPool,
    keys: (&Vault, &MaskedPool),
    pick: Uuid,
    at: Timestamp,
) -> Result<Released, ReleaseError> {
    let mut tx = pool.begin().await.map_err(|_| ReleaseError::Database)?;
    let waiting = awaiting_address(&mut tx, pick)
        .await
        .map_err(|_| ReleaseError::Database)?;
    if !waiting {
        return Ok(Released::MovedOn);
    }
    let cleared = match reclear_release(&mut tx, pick).await {
        Ok(cleared) => cleared,
        Err(ReleaseError::Database) => return Err(ReleaseError::Database),
        Err(_) => return Ok(Released::MovedOn),
    };
    let settled = settle(&mut tx, &cleared)
        .await
        .map_err(|_| ReleaseError::Database)?;
    if job_went_final(&mut tx, settled, pick).await? {
        return leave_final(pool, tx, &cleared, at).await;
    }
    tx.commit().await.map_err(|_| ReleaseError::Database)?;
    release_settled(pool, keys, (cleared, settled), at).await
}

/// The pick and the optional due time of an `intake.address_in` payload (`{"pick"}` from
/// `intake`, `{"pick", "run_at"}` from a deferral; D-4419); a bad `run_at` is a bad payload.
fn address_payload(payload: &Value) -> Option<(Uuid, Option<Timestamp>)> {
    let pick = Uuid::parse_str(payload["pick"].as_str()?).ok()?;
    let run_at = match &payload["run_at"] {
        Value::Null => None,
        due => Some(due.as_str()?.parse().ok()?),
    };
    Some((pick, run_at))
}

/// Registers the [`ADDRESS_IN_KIND`] handler over the vault from `pairs` and `masked` on
/// `clock` (INT-029, OPS-001, D-1739, D-4419): it reads the payload's pick and due time and runs
/// the safe-mode gate and re-release. Called by `worker_registry` with the Twilio settings;
/// without both vault keys nothing is registered (dev and CI), as `vault_from` allows.
///
/// # Errors
/// A malformed vault key, named, never its value.
pub fn register_address_in<I>(
    registry: &mut Registry,
    settings: &Settings,
    (masked, clock): (MaskedPool, Arc<dyn Clock>),
    pairs: I,
) -> io::Result<()>
where
    I: IntoIterator<Item = (String, String)>,
{
    let Some(vault) = crate::vault_from(settings, pairs)? else {
        return Ok(());
    };
    let vault = Arc::new(vault);
    registry.register(ADDRESS_IN_KIND, move |pool, task| {
        let (vault, masked, at) = (Arc::clone(&vault), masked.clone(), clock.now());
        async move {
            let parsed = address_payload(&task.payload);
            let (pick, run_at) = parsed.ok_or(HandlerError(BAD_ADDRESS_IN))?;
            address_in_due(&pool, (&vault, &masked), pick, (at, run_at))
                .await
                .map(|_| ())
                .map_err(|_| HandlerError(ADDRESS_IN_FAILED))
        }
    });
    Ok(())
}

impl ReleaseSink for PickReleaser {
    async fn release(&self, cleared: Releasable) -> Result<(), ReleaseError> {
        self.settle_and_release(cleared).await.map(|_| ())
    }
}

/// The Stripe webhook route's state from `settings` and the same `pairs` (SEC-036, SEC-039,
/// SEC-040, SEC-049, D-1480): with [`WEBHOOK_SECRET`] set, the Stripe secret key, the Stripe
/// account, both vault keys and the Twilio settings (the masked pool) are required; unset, `None`
/// in dev and CI (no route) and an error in the test and live projects, which Stripe must reach.
///
/// # Errors
/// A missing or bad key, named, never its value.
pub fn stripe_webhook<I>(
    settings: &Settings,
    pool: PgPool,
    pairs: I,
) -> io::Result<Option<StripeHooks>>
where
    I: IntoIterator<Item = (String, String)>,
{
    let pairs: Vec<(String, String)> = pairs.into_iter().collect();
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(k, v)| k == key && !v.trim().is_empty())
            .map(|(_, v)| v.trim().to_owned())
    };
    let Some(secret) = get(WEBHOOK_SECRET) else {
        return match settings.environment() {
            Environment::Dev | Environment::Ci => Ok(None),
            Environment::Test | Environment::Live => Err(missing(WEBHOOK_SECRET)),
        };
    };
    let mode = stripe_mode(settings.environment(), get(STRIPE_MODE).as_deref())?;
    let stripe = stripe_client(settings, mode, get(STRIPE_BASE_URL))?;
    let account = settings
        .account(Vendor::Stripe)
        .ok_or_else(|| missing("ASKHAVA_STRIPE_ACCOUNT_ID"))?;
    let vault = crate::vault_from(settings, pairs.clone())?
        .ok_or_else(|| missing("VAULT_KEY and VAULT_INDEX_KEY"))?;
    let twilio = TwilioConfig::from_settings(settings, pairs.clone()).map_err(io::Error::other)?;
    let client_ip = ClientIpPolicy::from_pairs(pairs).map_err(io::Error::other)?;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let sink = PickReleaser::new(
        pool.clone(),
        vault,
        twilio.pool().clone(),
        Arc::clone(&clock),
    );
    Ok(Some(Arc::new(StripeWebhook {
        pool,
        stripe,
        secret: SecretString::from(secret),
        account: StripeAccount {
            live: settings.environment() == Environment::Live,
            account: account.to_owned(),
        },
        mode,
        stand_in_host: STAND_IN_HOST,
        client_ip,
        clock,
        sink,
    })))
}

fn missing(key: &str) -> io::Error {
    io::Error::other(format!("missing {key}"))
}

/// The Stripe adapter mode: unset is `live` in live and `stand_in` elsewhere; `live` only in
/// live, and never `stand_in` or `sandbox` in live (SEC-002).
fn stripe_mode(environment: Environment, raw: Option<&str>) -> io::Result<AdapterMode> {
    let live = environment == Environment::Live;
    let mode = match raw {
        None if live => AdapterMode::Live,
        None | Some("stand_in") => AdapterMode::StandIn,
        Some("sandbox") => AdapterMode::Sandbox,
        Some("live") => AdapterMode::Live,
        Some(_) => return Err(io::Error::other(format!("{STRIPE_MODE} is not a mode"))),
    };
    if live != (mode == AdapterMode::Live) {
        return Err(io::Error::other(format!(
            "{STRIPE_MODE} does not fit the environment"
        )));
    }
    Ok(mode)
}

/// The Stripe client: the secret key from the settings and the base URL, checked (SEC-032).
fn stripe_client(
    settings: &Settings,
    mode: AdapterMode,
    base: Option<String>,
) -> io::Result<StripePayments> {
    let key = settings
        .stripe_secret_key()
        .ok_or_else(|| missing("ASKHAVA_STRIPE_SECRET_KEY"))?;
    let base = base.unwrap_or_else(|| match mode {
        AdapterMode::StandIn => STAND_IN_STRIPE.to_owned(),
        AdapterMode::Sandbox | AdapterMode::Live => STRIPE_API.to_owned(),
    });
    check_base_url(settings.environment(), STRIPE_BASE_URL, &base).map_err(io::Error::other)?;
    StripePayments::new(&base, key.clone()).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    //! The route's settings (SEC-002, SEC-036, SEC-049, D-1480): when the Stripe webhook is built,
    //! and the mode rule. The release itself is `tests/release_recovery.rs`. Fake values only.

    use askhava_adapters::api::AdapterMode;
    use askhava_core::api::{Environment, Settings};
    use sqlx::postgres::PgPoolOptions;

    use super::{STRIPE_MODE, WEBHOOK_SECRET, stripe_mode, stripe_webhook};

    const TEST_LIST: &str =
        "twilio:ACtestfake01,stripe:acct_testfake,resend:test.example.com,bucket:askhava-test";

    fn pairs(env: &str, extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let list = [("ASKHAVA_ALLOW_LIST_TEST", TEST_LIST)];
        let base = [
            ("ASKHAVA_APP_ENV", env),
            ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        ];
        let list: &[(&str, &str)] = if env == "test" { &list } else { &[] };
        base.iter()
            .chain(list)
            .chain(extra)
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn built(env: &str, extra: &[(&str, &str)]) -> Result<bool, String> {
        let all = pairs(env, extra);
        let settings = Settings::from_pairs(all.clone()).map_err(|e| e.to_string())?;
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://fake@127.0.0.1:1/none")
            .expect("a lazy pool");
        stripe_webhook(&settings, pool, all)
            .map(|hooks| hooks.is_some())
            .map_err(|e| e.to_string())
    }

    /// No signing secret: no route in dev and CI, an error naming the key in the test project.
    #[tokio::test]
    async fn no_signing_secret_is_no_route_in_dev_and_ci_and_an_error_in_test() {
        assert_eq!(built("dev", &[]), Ok(false));
        assert_eq!(built("ci", &[]), Ok(false));
        let error = built("test", &[]).expect_err("required in test");
        assert!(error.contains(WEBHOOK_SECRET), "{error}");
    }

    /// With the secret, the Stripe key is required and named, never echoed.
    #[tokio::test]
    async fn with_a_secret_the_stripe_key_is_required() {
        let secret = [(WEBHOOK_SECRET, "whsec_fake_value_0001")];
        let error = built("dev", &secret).expect_err("no Stripe key");
        assert!(error.contains("ASKHAVA_STRIPE_SECRET_KEY"), "{error}");
        assert!(!error.contains("whsec_fake_value_0001"), "{error}");
        let bad_mode = [
            (WEBHOOK_SECRET, "whsec_fake_value_0001"),
            (STRIPE_MODE, "live"),
        ];
        let error = built("dev", &bad_mode).expect_err("live outside live");
        assert!(error.contains(STRIPE_MODE), "{error}");
    }

    /// SEC-002: live only in live; stand-in by default elsewhere; an unknown mode is refused.
    #[test]
    fn the_stripe_mode_fits_the_environment() {
        assert_eq!(
            stripe_mode(Environment::Dev, None).ok(),
            Some(AdapterMode::StandIn)
        );
        assert_eq!(
            stripe_mode(Environment::Live, None).ok(),
            Some(AdapterMode::Live)
        );
        assert_eq!(
            stripe_mode(Environment::Test, Some("sandbox")).ok(),
            Some(AdapterMode::Sandbox)
        );
        assert!(stripe_mode(Environment::Live, Some("sandbox")).is_err());
        assert!(stripe_mode(Environment::Live, Some("stand_in")).is_err());
        assert!(stripe_mode(Environment::Dev, Some("live")).is_err());
        assert!(stripe_mode(Environment::Dev, Some("other")).is_err());
    }
}
