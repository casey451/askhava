//! Settings refuse to start on bad configuration (SEC-001, SEC-002, SEC-003, SEC-036; TD-SEC-15, TD-SEC-13).
//!
//! Every case passes pairs, never `std::env::set_var` (R16). Fake keys are
//! built at run time so no key-shaped literal sits in the source (SEC-001).

use secrecy::ExposeSecret;

use crate::settings::{Environment, Settings, SettingsError, Vendor, check_base_url};

const TEST_LIST: &str =
    "twilio:ACtestfake01,stripe:acct_testfake,resend:test.example.com,bucket:askhava-test";
const LIVE_LIST: &str =
    "twilio:AClivefake01,stripe:acct_livefake,resend:mail.example.com,bucket:askhava-live";

fn pairs(extra: &[(&str, &str)], env: &str) -> Vec<(String, String)> {
    let mut all = vec![
        ("ASKHAVA_APP_ENV".to_owned(), env.to_owned()),
        (
            "ASKHAVA_DATABASE_URL".to_owned(),
            "postgres://fake@localhost/fake".to_owned(),
        ),
        ("PATH".to_owned(), "/usr/bin".to_owned()),
    ];
    all.extend(
        extra
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
    );
    all
}

fn live_pairs(overrides: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut extra = vec![
        ("ASKHAVA_ALLOW_LIST_TEST", TEST_LIST),
        ("ASKHAVA_ALLOW_LIST_LIVE", LIVE_LIST),
        ("ASKHAVA_TWILIO_ACCOUNT_SID", "AClivefake01"),
        ("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_livefake"),
        ("ASKHAVA_RESEND_DOMAIN", "mail.example.com"),
        ("ASKHAVA_STORAGE_BUCKET", "askhava-live"),
    ];
    for (key, value) in overrides {
        extra.retain(|(k, _)| k != key);
        extra.push((key, value));
    }
    pairs(&extra, "live")
}

fn stripe_key(mode: &str) -> String {
    format!("sk_{mode}_FAKE0")
}

/// SEC-002: a minimal dev configuration parses, with defaults and other variables ignored.
#[test]
fn minimal_dev_settings_parse_with_defaults() {
    let settings = Settings::from_pairs(pairs(&[], "dev")).unwrap();
    assert_eq!(settings.environment(), Environment::Dev);
    assert_eq!(settings.db_pool_max(), 10);
    assert_eq!(settings.log_level(), tracing::Level::INFO);
    assert!(settings.sentry_dsn().is_none());
    assert_eq!(
        settings.database_url().expose_secret(),
        "postgres://fake@localhost/fake"
    );
}

/// SEC-002: a missing required value stops startup and names the key.
#[test]
fn missing_database_url_stops_startup() {
    let only_env = vec![("ASKHAVA_APP_ENV".to_owned(), "ci".to_owned())];
    let err = Settings::from_pairs(only_env).unwrap_err();
    assert_eq!(err, SettingsError::Missing("ASKHAVA_DATABASE_URL"));
    assert_eq!(
        Settings::from_pairs(Vec::new()).unwrap_err(),
        SettingsError::Missing("ASKHAVA_APP_ENV")
    );
}

/// D-493: an unknown `ASKHAVA_*` key stops startup.
#[test]
fn unknown_askhava_key_stops_startup() {
    let err = Settings::from_pairs(pairs(&[("ASKHAVA_DB_POOL_MAXX", "5")], "dev")).unwrap_err();
    assert_eq!(
        err,
        SettingsError::Unknown("ASKHAVA_DB_POOL_MAXX".to_owned())
    );
}

/// SEC-001, SEC-002: invalid values are refused and no error message echoes the value.
#[test]
fn invalid_values_are_refused_without_echoing_them() {
    let cases = [
        ("ASKHAVA_DB_POOL_MAX", "FAKE-not-a-number"),
        ("ASKHAVA_DB_POOL_MAX", "0"),
        ("ASKHAVA_LOG_LEVEL", "FAKE-loud"),
        ("ASKHAVA_ALLOW_LIST_TEST", "FAKE-no-colon"),
    ];
    for (key, value) in cases {
        let err = Settings::from_pairs(pairs(&[(key, value)], "dev")).unwrap_err();
        assert!(!err.to_string().contains(value), "{key} echoed its value");
    }
    let err = Settings::from_pairs(pairs(&[], "FAKE-staging")).unwrap_err();
    assert_eq!(err, SettingsError::Invalid("ASKHAVA_APP_ENV"));
}

/// SEC-001: `Debug` of the settings shows no secret.
#[test]
fn debug_shows_no_secret() {
    let key = stripe_key("test");
    let settings =
        Settings::from_pairs(pairs(&[("ASKHAVA_STRIPE_SECRET_KEY", &key)], "dev")).unwrap();
    let shown = format!("{settings:?}");
    assert!(!shown.contains(&key));
    assert!(!shown.contains("postgres://fake"));
}

/// SEC-002: a live key in a non-live environment stops startup, in any key.
#[test]
fn live_key_outside_live_stops_startup() {
    let key = stripe_key("live");
    for env in ["dev", "ci", "test"] {
        let extra = [
            ("ASKHAVA_ALLOW_LIST_TEST", TEST_LIST),
            ("ASKHAVA_STRIPE_SECRET_KEY", key.as_str()),
        ];
        let err = Settings::from_pairs(pairs(&extra, env)).unwrap_err();
        assert_eq!(
            err,
            SettingsError::LiveKeyOutsideLive("ASKHAVA_STRIPE_SECRET_KEY")
        );
        assert!(!err.to_string().contains(&key));
    }
    let in_dsn = format!("https://{key}@example.com/1");
    let err = Settings::from_pairs(pairs(&[("ASKHAVA_SENTRY_DSN", &in_dsn)], "dev")).unwrap_err();
    assert_eq!(err, SettingsError::LiveKeyOutsideLive("ASKHAVA_SENTRY_DSN"));
}

/// SEC-002: a test key, or any non-live Stripe key, in live stops startup.
#[test]
fn test_key_in_live_stops_startup() {
    let key = stripe_key("test");
    let err = Settings::from_pairs(live_pairs(&[("ASKHAVA_STRIPE_SECRET_KEY", &key)])).unwrap_err();
    assert_eq!(
        err,
        SettingsError::TestKeyInLive("ASKHAVA_STRIPE_SECRET_KEY")
    );
    let err =
        Settings::from_pairs(live_pairs(&[("ASKHAVA_STRIPE_SECRET_KEY", "FAKE0")])).unwrap_err();
    assert_eq!(
        err,
        SettingsError::TestKeyInLive("ASKHAVA_STRIPE_SECRET_KEY")
    );
    let live_key = stripe_key("live");
    let ok = Settings::from_pairs(live_pairs(&[("ASKHAVA_STRIPE_SECRET_KEY", &live_key)])).unwrap();
    assert_eq!(ok.account(Vendor::Twilio), Some("AClivefake01"));
}

/// TD-SEC-15; SEC-036, SEC-003: a Twilio SID from the test allow-list with `APP_ENV=live` stops startup.
#[test]
fn test_twilio_sid_in_live_stops_startup() {
    let err = Settings::from_pairs(live_pairs(&[(
        "ASKHAVA_TWILIO_ACCOUNT_SID",
        "ACtestfake01",
    )]))
    .unwrap_err();
    assert_eq!(
        err,
        SettingsError::ForeignAccount("ASKHAVA_TWILIO_ACCOUNT_SID")
    );
}

/// TD-SEC-15; SEC-036, SEC-003: a bucket name from the other environment stops startup, both ways.
#[test]
fn bucket_from_other_environment_stops_startup() {
    let err = Settings::from_pairs(live_pairs(&[("ASKHAVA_STORAGE_BUCKET", "askhava-test")]))
        .unwrap_err();
    assert_eq!(err, SettingsError::ForeignAccount("ASKHAVA_STORAGE_BUCKET"));
    let extra = [
        ("ASKHAVA_ALLOW_LIST_TEST", TEST_LIST),
        ("ASKHAVA_ALLOW_LIST_LIVE", LIVE_LIST),
        ("ASKHAVA_STORAGE_BUCKET", "askhava-live"),
    ];
    let err = Settings::from_pairs(pairs(&extra, "test")).unwrap_err();
    assert_eq!(err, SettingsError::ForeignAccount("ASKHAVA_STORAGE_BUCKET"));
    let dev_extra = [
        ("ASKHAVA_ALLOW_LIST_LIVE", LIVE_LIST),
        ("ASKHAVA_STORAGE_BUCKET", "askhava-live"),
    ];
    let err = Settings::from_pairs(pairs(&dev_extra, "ci")).unwrap_err();
    assert_eq!(err, SettingsError::ForeignAccount("ASKHAVA_STORAGE_BUCKET"));
}

/// SEC-036: an identifier on neither list, a missing identifier in live and a missing own list stop startup.
#[test]
fn identifier_off_the_allow_list_stops_startup() {
    let err = Settings::from_pairs(live_pairs(&[(
        "ASKHAVA_RESEND_DOMAIN",
        "other.example.com",
    )]))
    .unwrap_err();
    assert_eq!(err, SettingsError::NotAllowed("ASKHAVA_RESEND_DOMAIN"));
    let mut missing = live_pairs(&[]);
    missing.retain(|(k, _)| k != "ASKHAVA_STRIPE_ACCOUNT_ID");
    assert_eq!(
        Settings::from_pairs(missing).unwrap_err(),
        SettingsError::Missing("ASKHAVA_STRIPE_ACCOUNT_ID")
    );
    let mut no_list = live_pairs(&[]);
    no_list.retain(|(k, _)| k != "ASKHAVA_ALLOW_LIST_LIVE");
    assert_eq!(
        Settings::from_pairs(no_list).unwrap_err(),
        SettingsError::Missing("ASKHAVA_ALLOW_LIST_LIVE")
    );
    let err = Settings::from_pairs(pairs(&[], "test")).unwrap_err();
    assert_eq!(err, SettingsError::Missing("ASKHAVA_ALLOW_LIST_TEST"));
}

/// SEC-036, D-688: live without the test list stops startup, so the foreign-account check always runs.
#[test]
fn live_without_test_list_stops_startup() {
    let mut no_test_list = live_pairs(&[("ASKHAVA_STORAGE_BUCKET", "askhava-test")]);
    no_test_list.retain(|(k, _)| k != "ASKHAVA_ALLOW_LIST_TEST");
    assert_eq!(
        Settings::from_pairs(no_test_list).unwrap_err(),
        SettingsError::Missing("ASKHAVA_ALLOW_LIST_TEST")
    );
}

/// SEC-036, D-690: spacing or case cannot carry a live identifier past the live list in dev or CI.
#[test]
fn identifier_compare_ignores_spacing_and_case() {
    for id in [" askhava-live ", "ASKHAVA-LIVE", "\taskhava-Live"] {
        let extra = [
            ("ASKHAVA_ALLOW_LIST_LIVE", LIVE_LIST),
            ("ASKHAVA_STORAGE_BUCKET", id),
        ];
        for env in ["dev", "ci"] {
            let err = Settings::from_pairs(pairs(&extra, env)).unwrap_err();
            assert_eq!(err, SettingsError::ForeignAccount("ASKHAVA_STORAGE_BUCKET"));
        }
    }
    let ok = Settings::from_pairs(live_pairs(&[(
        "ASKHAVA_RESEND_DOMAIN",
        " Mail.Example.com ",
    )]))
    .unwrap();
    assert_eq!(ok.account(Vendor::Resend), Some("Mail.Example.com"));
}

/// TD-SEC-13; SEC-032, SEC-002: a stand-in URL with `APP_ENV=live` stops startup.
#[test]
fn stand_in_url_in_live_stops_startup() {
    let key = "ASKHAVA_FAKE_BASE_URL";
    for url in [
        "http://127.0.0.1:4010",
        "https://localhost:4010/v1",
        "file:///tmp/stand_in",
        "http://api.example.com",
    ] {
        assert_eq!(
            check_base_url(Environment::Live, key, url),
            Err(SettingsError::StandInInLive(key))
        );
    }
    assert_eq!(
        check_base_url(Environment::Live, key, "https://api.example.com"),
        Ok(())
    );
    assert_eq!(
        check_base_url(Environment::Ci, key, "http://127.0.0.1:4010"),
        Ok(())
    );
    assert_eq!(
        check_base_url(Environment::Dev, key, "file:///tmp/stand_in"),
        Ok(())
    );
    assert_eq!(
        check_base_url(Environment::Ci, key, "ftp://example.com"),
        Err(SettingsError::Invalid(key))
    );
}

/// TD-SEC-13; SEC-032, D-690: case, userinfo and other loopback spellings are still refused in live.
#[test]
fn disguised_local_url_in_live_stops_startup() {
    let key = "ASKHAVA_FAKE_BASE_URL";
    for url in [
        "https://LOCALHOST:4010",
        "https://fake:fake@localhost/v1",
        "https://user@127.0.0.1",
        "https://[::1]:4010",
        "https://[0:0:0:0:0:0:0:1]/v1",
        "https://[::ffff:127.0.0.1]",
        "https://[::]",
        "https://0.0.0.0",
        "https://stand-in.localhost",
        "https://localhost.",
    ] {
        assert_eq!(
            check_base_url(Environment::Live, key, url),
            Err(SettingsError::StandInInLive(key)),
            "{url}"
        );
    }
    for url in [
        "https://localhost.example.com",
        "https://fake@api.example.com/v1",
        "https://[2001:db8::1]:443",
    ] {
        assert_eq!(check_base_url(Environment::Live, key, url), Ok(()), "{url}");
    }
}
