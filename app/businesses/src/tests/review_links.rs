//! Pure checks of `links.rs` (BIZ-052, SEC-041, BIZ-022; D-3034, D-3037): the Business Profile
//! and Yelp page shapes, the phone and nested-link refusals, the guarded opener's mapping of a
//! fetch result and the city license number screen. Fake links and numbers only; no network.

use askhava_adapters::api::FetchError;

use crate::about_us::AboutUsRefusal;
use crate::links::{NameRefusal, ReviewSite, on_site, opened, screen_names};

/// D-3037: the shapes Google and Yelp give out are accepted.
#[test]
fn td_biz_32_profile_shapes_are_accepted() {
    let google = [
        "https://g.page/example-sample-plumbing",
        "https://maps.app.goo.gl/AbCd1234567Ef",
        "https://share.google/AbCdEf9876543",
        "https://g.co/kgs/AbC1234",
        "https://www.google.com/maps/place/Sample+Plumbing/@34.4838826,-114.3225123,17z/data=!3m1!4b1!4m6!3m5!1s0x80d1a2b3c4d5e6f7:0x1234567890abcdef!8m2!3d34.4838826!4d-114.3225123",
        "https://maps.google.com/?cid=12345678901234567890",
        "https://business.google.com/example",
    ];
    for url in google {
        assert!(on_site(ReviewSite::Google, url), "{url}");
    }
    for url in [
        "https://www.yelp.com/biz/example-sample-plumbing-2",
        "https://yelp.com/biz/example-sample-plumbing",
    ] {
        assert!(on_site(ReviewSite::Yelp, url), "{url}");
    }
}

/// D-3037: any other page on Google's or Yelp's domains is refused.
#[test]
fn td_biz_32_other_pages_on_the_domains_are_refused() {
    let google = [
        "https://www.google.com/search?q=sample+plumbing",
        "https://google.com/maps/place/x",
        "https://sites.google.com/view/example-sample",
        "https://docs.google.com/document/d/example",
        "https://www.google.com/url?q=https://example.com",
        "https://maps.google.com/url?q=https://example.com",
        "https://g.page/",
        "https://g.co/x",
        "https://evil.g.page/x",
    ];
    for url in google {
        assert!(!on_site(ReviewSite::Google, url), "{url}");
    }
    for url in [
        "https://www.yelp.com/search?find_desc=sample",
        "https://www.yelp.com/biz/",
        "https://m.yelp.com/biz/example",
        "https://www.yelp.com/user_details?userid=example",
    ] {
        assert!(!on_site(ReviewSite::Yelp, url), "{url}");
    }
}

/// D-3037: a link holding a phone number or another link is refused, also percent-encoded.
#[test]
fn td_biz_32_a_link_holding_a_phone_or_a_link_is_refused() {
    let google = [
        "https://www.google.com/maps/search/928-555-0100",
        "https://www.google.com/maps/place/Sample+Plumbing+(928)+555+0100",
        "https://www.google.com/maps/search/928%20555%200100",
        "https://maps.google.com/?cid=1&q=9285550100",
        "https://www.google.com/maps/place/x?u=https%3A%2F%2Fexample.com",
    ];
    for url in google {
        assert!(!on_site(ReviewSite::Google, url), "{url}");
    }
    let yelp = "https://www.yelp.com/biz/sample-plumbing-928-555-0100";
    assert!(!on_site(ReviewSite::Yelp, yelp));
}

/// SEC-041, D-3034: a normal answer or a body over the cap opens; every other failure does not.
#[test]
fn td_sec_20_guarded_fetch_result_maps_to_open() {
    assert!(opened(&Ok(Vec::new())));
    assert!(opened(&Err(FetchError::TooLarge)));
    let shut = [
        FetchError::BadUrl,
        FetchError::HostNotAllowed,
        FetchError::AddressRefused,
        FetchError::TooManyRedirects,
        FetchError::TimedOut,
        FetchError::Status(403),
        FetchError::Status(404),
        FetchError::Failed,
    ];
    for e in shut {
        let label = format!("{e:?}");
        assert!(!opened(&Err(e)), "{label}");
    }
}

/// BIZ-022, D-3037: the city license number gets the public-name screen; a plain number passes.
#[test]
fn td_biz_32_city_license_number_is_screened() {
    let ok = screen_names("Sample Plumbing", None, Some(" CL-7001 "));
    assert_eq!(ok, Ok(("Sample Plumbing", None, Some("CL-7001"))));
    let cases = [
        ("928-555-0100", AboutUsRefusal::Contact),
        ("sampleplumbing.com", AboutUsRefusal::Contact),
        ("https://example.com", AboutUsRefusal::Contact),
        ("search us on Google", AboutUsRefusal::Channel),
    ];
    for (city, why) in cases {
        let got = screen_names("Sample Plumbing", None, Some(city));
        assert_eq!(got, Err(NameRefusal::CityLicenseScreened(why)), "{city}");
    }
}
