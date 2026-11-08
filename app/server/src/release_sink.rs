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
//! and the masked pool. `email_chosen` is false, a masked call: the PICK-018 choice is not stored
//! yet (D-1264, D-1740). `Already`, `MovedOn` and `WaitingForAddress` are success to the route; a
//! database or vault failure is `ReleaseError::Database`, its 500, so Stripe sends the event again.
//!
//! [`stripe_webhook`] builds the route's `StripeWebhook` from the settings (SEC-036, SEC-039,
//! SEC-049): the Stripe client, the `whsec_…` signing secret, the configured account and mode,
//! the SEC-040 client-address policy, the vault and the masked pool.

use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use askhava_adapters::api::{AdapterMode, MaskedPool, StripePayments, TwilioConfig};
use askhava_core::api::{
    ClientIpPolicy, Clock, Environment, Settings, SystemClock, Vendor, check_base_url,
};
use askhava_intake::api::job_homeowner;
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use askhava_payments::api::{Releasable, ReleaseError, ReleaseSink, StripeAccount, StripeWebhook};
use askhava_picks::api::{Released, Settled, release, settle};
use askhava_privacy::api::Vault;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::release_minter;

/// The Stripe webhook signing secret (`whsec_…`, SEC-039).
pub const WEBHOOK_SECRET: &str = "ASKHAVA_STRIPE_WEBHOOK_SECRET";
/// `stand_in`, `sandbox` or `live` for Stripe; `live` only in live (SEC-002, SEC-049).
pub const STRIPE_MODE: &str = "ASKHAVA_ADAPTER_MODE_STRIPE";
/// The Stripe API base URL; unset, the stand-in's or Stripe's own by mode (SEC-032).
pub const STRIPE_BASE_URL: &str = "ASKHAVA_STRIPE_BASE_URL";
/// The High alert raised when a cleared pick's job has no open request naming its homeowner, so
/// no proof can be minted (PICK-022, D-1739).
pub const NO_CONTACT_ALERT: &str = "picks.release_no_contact";

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
    /// D-1739): `MovedOn` for a pick that left awaiting confirm, else the release's answer.
    ///
    /// # Errors
    /// [`ReleaseError::Database`] when the settle, the contact lookup, the vault read or the
    /// release's writes fail; the release, once claimed, stands.
    pub async fn settle_and_release(&self, cleared: Releasable) -> Result<Released, ReleaseError> {
        let at = self.clock.now();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ReleaseError::Database)?;
        let settled = settle(&mut tx, &cleared)
            .await
            .map_err(|_| ReleaseError::Database)?;
        tx.commit().await.map_err(|_| ReleaseError::Database)?;
        let job = match settled {
            Settled::Left => return Ok(Released::MovedOn),
            Settled::Moved(job) | Settled::Was(job) => job,
        };
        let Some(contact) = self.contact_of(cleared.pick(), job).await? else {
            return Ok(Released::MovedOn);
        };
        let proof = release_minter().mint(cleared, contact);
        release(&self.pool, (&self.vault, &self.masked), (proof, false), at)
            .await
            .map_err(|_| ReleaseError::Database)
    }

    /// The job's homeowner contact through `intake` (D-1602); with none, one High
    /// [`NO_CONTACT_ALERT`] about the pick and `None`.
    async fn contact_of(&self, pick: Uuid, job: Uuid) -> Result<Option<Uuid>, ReleaseError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|_| ReleaseError::Database)?;
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
