//! The Confirm page's two settings (T492, D-3813; SEC-001, SEC-002, D-019): Stripe's publishable
//! key and the Gate C launch date are optional in every environment, a `pk_test_` key is refused
//! in live and a `pk_live_` key outside it, and a bad value names the key, never the value.
//!
//! Every case passes pairs, never `std::env::set_var` (R16). Fake keys are built at run time so
//! no key-shaped literal sits in the source (SEC-001).

use crate::settings::{Settings, SettingsError};

const PUBLISHABLE: &str = "ASKHAVA_STRIPE_PUBLISHABLE_KEY";
const LAUNCH: &str = "ASKHAVA_LAUNCH_DATE";
const ENVIRONMENTS: [&str; 4] = ["dev", "ci", "test", "live"];

/// The pairs `env` needs to start, plus `extra`; test and live get fake allow-lists and accounts.
fn pairs(env: &str, extra: &[(&str, String)]) -> Vec<(String, String)> {
    let test_list =
        "twilio:ACtestfake01,stripe:acct_testfake,resend:test.example.com,bucket:askhava-test";
    let live_list =
        "twilio:AClivefake01,stripe:acct_livefake,resend:mail.example.com,bucket:askhava-live";
    let mut all = vec![
        ("ASKHAVA_APP_ENV", env.to_owned()),
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://fake@localhost/fake".to_owned(),
        ),
    ];
    if env == "test" || env == "live" {
        all.push(("ASKHAVA_ALLOW_LIST_TEST", test_list.to_owned()));
    }
    if env == "live" {
        all.extend([
            ("ASKHAVA_ALLOW_LIST_LIVE", live_list.to_owned()),
            ("ASKHAVA_TWILIO_ACCOUNT_SID", "AClivefake01".to_owned()),
            ("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_livefake".to_owned()),
            ("ASKHAVA_RESEND_DOMAIN", "mail.example.com".to_owned()),
            ("ASKHAVA_STORAGE_BUCKET", "askhava-live".to_owned()),
            (
                "ASKHAVA_PUBLIC_BASE_URL",
                "https://app.example.com".to_owned(),
            ),
        ]);
    }
    all.extend(extra.iter().cloned());
    all.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// A fake publishable key in `mode` (`test` or `live`), built so no key literal sits here.
fn fake_key(mode: &str) -> String {
    format!("pk_{mode}_{}", "Fake0123456789".repeat(2))
}

#[test]
fn both_keys_are_optional_in_every_environment() {
    for env in ENVIRONMENTS {
        let settings = Settings::from_pairs(pairs(env, &[])).expect(env);
        assert_eq!(settings.stripe_publishable_key(), None, "{env}");
        assert_eq!(settings.launch_date(), None, "{env}");
    }
}

#[test]
fn a_test_key_is_accepted_outside_live_and_refused_in_live() {
    for env in ["dev", "ci", "test"] {
        let key = fake_key("test");
        let settings =
            Settings::from_pairs(pairs(env, &[(PUBLISHABLE, format!(" {key} "))])).expect(env);
        assert_eq!(
            settings.stripe_publishable_key(),
            Some(key.as_str()),
            "{env}"
        );
    }
    let refused = Settings::from_pairs(pairs("live", &[(PUBLISHABLE, fake_key("test"))]));
    assert_eq!(
        refused.unwrap_err(),
        SettingsError::TestKeyInLive(PUBLISHABLE)
    );
}

#[test]
fn a_live_key_is_accepted_in_live_and_refused_outside_it() {
    let key = fake_key("live");
    let settings = Settings::from_pairs(pairs("live", &[(PUBLISHABLE, key.clone())])).unwrap();
    assert_eq!(settings.stripe_publishable_key(), Some(key.as_str()));
    for env in ["dev", "ci", "test"] {
        let refused = Settings::from_pairs(pairs(env, &[(PUBLISHABLE, fake_key("live"))]));
        assert_eq!(
            refused.unwrap_err(),
            SettingsError::LiveKeyOutsideLive(PUBLISHABLE),
            "{env}"
        );
    }
}

#[test]
fn a_value_that_is_not_a_publishable_key_names_the_key_only() {
    let secret = format!("sk_{}_{}", "test", "Fake0123456789");
    for bad in [
        "pk_test_",
        "pk_other_Fake0123",
        "Fake0123456789",
        "pk_test_has space",
        secret.as_str(),
    ] {
        let error =
            Settings::from_pairs(pairs("ci", &[(PUBLISHABLE, bad.to_owned())])).unwrap_err();
        assert_eq!(error, SettingsError::Invalid(PUBLISHABLE), "{bad}");
        assert!(
            !error.to_string().contains(bad),
            "the error echoes no value"
        );
    }
}

#[test]
fn a_launch_instant_is_read_in_every_environment() {
    for env in ENVIRONMENTS {
        let raw = "2027-01-15T07:00:00Z".to_owned();
        let settings = Settings::from_pairs(pairs(env, &[(LAUNCH, raw)])).expect(env);
        let expected: jiff::Timestamp = "2027-01-15T00:00:00-07:00".parse().unwrap();
        assert_eq!(settings.launch_date(), Some(expected), "{env}");
    }
}

#[test]
fn a_bad_launch_date_names_the_key_and_never_the_value() {
    for bad in [
        "2027-01-15",
        "next spring",
        "2027-13-01T00:00:00Z",
        "2027-01-15T07:00:00",
    ] {
        for env in ENVIRONMENTS {
            let error = Settings::from_pairs(pairs(env, &[(LAUNCH, bad.to_owned())])).unwrap_err();
            assert_eq!(error, SettingsError::Invalid(LAUNCH), "{env} {bad}");
            assert!(
                !error.to_string().contains(bad),
                "the error echoes no value"
            );
        }
    }
}
