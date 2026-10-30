//! Agency-licensed categories, the city list and the boundary job types (TD-LIC-06, TD-LIC-12,
//! TD-LIC-17; LIC-011 to LIC-016, LIC-028 to LIC-031, LIC-047). Jobs are built from the real
//! catalog file; every business is fake.

use std::collections::BTreeSet;

use crate::api::{
    Agency, CityListStanding, CityRecord, Eligibility, Holder, InspectionFee, Job, LicenseKind,
    NoCardReason, PestAttestation, PestRecordCategory, SaleAnswer, city_list_standing, eligibility,
    inspection_fee,
};

const CATEGORIES: &str = include_str!("../../../../docs/catalog/categories.yaml");

/// The job `job_type` of catalog category `code`, as `quotes` would build it.
fn job(code: &str, job_type: &str) -> Job {
    let catalog = askhava_catalog::api::parse(CATEGORIES).expect("catalog parses");
    let def = catalog
        .categories
        .iter()
        .find(|c| c.id == code)
        .expect("category");
    Job {
        category: def.id.clone(),
        job_type: job_type.to_owned(),
        license: LicenseKind::parse(&def.license.kind).expect("kind"),
        handyman_allowed: def.handyman_allowed,
        listed: def.job_types.iter().any(|j| j == job_type),
    }
}

fn on_city_list() -> Holder {
    Holder {
        city_list: CityListStanding::Matched,
        ..Holder::default()
    }
}

fn handyman() -> Holder {
    Holder {
        handyman: true,
        ..on_city_list()
    }
}

fn roc() -> Holder {
    Holder {
        roc_covers: true,
        ..Holder::default()
    }
}

fn agency(agency: Agency) -> Holder {
    Holder {
        agencies: BTreeSet::from([agency]),
        ..Holder::default()
    }
}

fn pest(wood: bool, weed: bool) -> Holder {
    let attested = PestAttestation::new(true, "PMB-0000", wood, weed).expect("attestation");
    Holder {
        pest: Some(attested),
        ..Holder::default()
    }
}

fn no_card(reason: NoCardReason) -> Eligibility {
    Eligibility::NoCard(reason)
}

/// TD-LIC-06 (LIC-013, LIC-015, LIC-016): each agency category reaches only its licensees; a
/// business with another agency, an ROC class or a city-list match alone gets nothing.
#[test]
fn agency_categories_reach_only_their_licensees() {
    let cases = [
        ("home_inspection", "buyer inspection", Agency::HomeInspector),
        ("septic_pumping", "tank pumping", Agency::SeptageHauler),
    ];
    for (code, job_type, needed) in cases {
        let job = job(code, job_type);
        assert_eq!(eligibility(&job, &agency(needed)), Eligibility::Card);
        let other = Agency::ALL.into_iter().filter(|a| *a != needed).collect();
        let wrong = Holder {
            agencies: other,
            roc_covers: true,
            ..handyman()
        };
        assert_eq!(
            eligibility(&job, &wrong),
            no_card(NoCardReason::AgencyMissing(needed))
        );
    }
    let alarm = job("alarm_systems", "alarm install");
    let both = Holder {
        roc_covers: true,
        ..agency(Agency::AlarmBusiness)
    };
    assert_eq!(eligibility(&alarm, &both), Eligibility::Card);
    let missing = NoCardReason::AgencyMissing(Agency::AlarmBusiness);
    assert_eq!(eligibility(&alarm, &roc()), no_card(missing));
    let cert_only = agency(Agency::AlarmBusiness);
    assert_eq!(
        eligibility(&alarm, &cert_only),
        no_card(NoCardReason::RocNotCovered)
    );
    assert_eq!(eligibility(&alarm, &handyman()), no_card(missing));
}

/// TD-LIC-06 (LIC-014): cameras go to R-67-covered businesses or to handymen as handyman cards
/// (under $1,000, no permit); a handyman still pending on the city list gets none.
#[test]
fn cameras_go_to_low_voltage_licensees_or_handymen() {
    let camera = job("security_cameras", "camera install");
    assert_eq!(eligibility(&camera, &roc()), Eligibility::Card);
    assert_eq!(eligibility(&camera, &handyman()), Eligibility::HandymanCard);
    let pending = Holder {
        handyman: true,
        ..Holder::default()
    };
    assert_eq!(
        eligibility(&camera, &pending),
        no_card(NoCardReason::CityListPending)
    );
    assert_eq!(
        eligibility(&camera, &on_city_list()),
        no_card(NoCardReason::RocNotCovered)
    );
}

/// TD-LIC-06, TD-LIC-17 (LIC-012, LIC-047): pest jobs go only to attested businesses; termites and
/// pre-emergent weed control also need the attested record category; an ROC class, a city-list
/// match or an `agencies` entry never opens pest work; a homeowner's report pauses it.
#[test]
fn pest_jobs_go_only_to_attested_pest_licensees() {
    let general = job("pest_control", "general pest service");
    let termites = job("pest_control", "termites");
    let weeds = job("pest_control", "pre-emergent weed control");
    assert_eq!(
        eligibility(&general, &pest(false, false)),
        Eligibility::Card
    );
    let wood = NoCardReason::PestCategoryMissing(PestRecordCategory::WoodDestroying);
    let weed = NoCardReason::PestCategoryMissing(PestRecordCategory::WeedControl);
    assert_eq!(eligibility(&termites, &pest(false, true)), no_card(wood));
    assert_eq!(
        eligibility(&termites, &pest(true, false)),
        Eligibility::Card
    );
    assert_eq!(eligibility(&weeds, &pest(true, false)), no_card(weed));
    assert_eq!(eligibility(&weeds, &pest(false, true)), Eligibility::Card);
    let unattested = Holder {
        roc_covers: true,
        ..agency(Agency::PestManagement)
    };
    let missing = no_card(NoCardReason::AgencyMissing(Agency::PestManagement));
    for holder in [unattested, handyman(), on_city_list()] {
        assert_eq!(eligibility(&weeds, &holder), missing);
        assert_eq!(eligibility(&general, &holder), missing);
    }
    let reported = PestAttestation::new(true, "PMB-0000", true, true).expect("attestation");
    let paused = Holder {
        pest: Some(reported.reported_wrong()),
        ..Holder::default()
    };
    assert_eq!(
        eligibility(&general, &paused),
        no_card(NoCardReason::PestPaused)
    );
}

/// TD-LIC-17 (LIC-047): the attestation needs the ticked box and a license number; the number is
/// kept for the card line.
#[test]
fn pest_attestation_needs_the_box_and_a_number() {
    assert!(PestAttestation::new(false, "PMB-0000", false, false).is_err());
    for bad in ["", "   ", "PMB 0000", "PMB#0", "123456789012345678901"] {
        assert!(
            PestAttestation::new(true, bad, false, false).is_err(),
            "{bad:?}"
        );
    }
    let ok = PestAttestation::new(true, " PMB-0000 ", false, true).expect("attestation");
    assert_eq!(ok.number(), "PMB-0000");
    assert!(ok.holds(PestRecordCategory::WeedControl));
    assert!(!ok.holds(PestRecordCategory::WoodDestroying));
    assert!(!ok.is_paused());
}

/// TD-LIC-17 (LIC-015): a yes to the sale question or a named buyer, pre-listing or refinance
/// inspection is fee-free and goes to certified inspectors only; a general condition inspection
/// keeps the normal fee.
#[test]
fn sale_related_inspections_are_fee_free_and_go_to_inspectors() {
    let catalog = askhava_catalog::api::parse(CATEGORIES).expect("catalog parses");
    let def = catalog
        .categories
        .iter()
        .find(|c| c.id == "home_inspection")
        .expect("category");
    let general = "home condition inspection (no sale or refinance)";
    for job_type in &def.job_types {
        let fee_free = def.fee_free_job_types.contains(job_type);
        let expected = if fee_free {
            InspectionFee::FeeFree
        } else {
            InspectionFee::Normal
        };
        assert_eq!(
            inspection_fee(SaleAnswer::No, fee_free),
            expected,
            "{job_type}"
        );
        assert_eq!(inspection_fee(SaleAnswer::NotAnswered, fee_free), expected);
        assert_eq!(
            inspection_fee(SaleAnswer::Yes, fee_free),
            InspectionFee::FeeFree
        );
        assert_eq!(fee_free, job_type != general, "{job_type}");
        let inspection = job("home_inspection", job_type);
        assert_eq!(
            eligibility(&inspection, &agency(Agency::HomeInspector)),
            Eligibility::Card
        );
        let missing = no_card(NoCardReason::AgencyMissing(Agency::HomeInspector));
        assert_eq!(
            eligibility(
                &inspection,
                &Holder {
                    roc_covers: true,
                    ..handyman()
                }
            ),
            missing
        );
    }
}

fn record(
    business: &str,
    trade: Option<&str>,
    owner: Option<&str>,
    number: Option<&str>,
) -> CityRecord {
    CityRecord {
        business_name: business.to_owned(),
        trade_name: trade.map(str::to_owned),
        owner_name: owner.map(str::to_owned),
        license_number: number.map(str::to_owned),
    }
}

/// TD-LIC-06 (LIC-011): a no-license business matches the city list on its business name, trade
/// name or owner name, ignoring case and punctuation; a disagreeing license number blocks the row.
#[test]
fn no_license_business_must_match_the_city_list_by_name() {
    let list = [
        record(
            "Sample Vent Works LLC",
            Some("Fake Lint Pros"),
            Some("Pat Example"),
            Some("B-1000"),
        ),
        record("Test Shade Co", None, None, None),
    ];
    let matched = [
        record("sample vent works, llc", None, None, None),
        record("Other Name", Some("FAKE LINT PROS"), None, None),
        record("Other Name", None, Some("Pat  Example"), Some("b-1000")),
        record("Test Shade Co.", None, None, Some("B-9999")),
    ];
    for business in &matched {
        assert_eq!(
            city_list_standing(business, &list),
            CityListStanding::Matched
        );
    }
    let pending = [
        record("Sample Vent Works LLC", None, None, Some("B-2000")),
        record("Sample Vent", None, None, None),
        record("", Some("   "), Some("..."), None),
    ];
    for business in &pending {
        assert_eq!(
            city_list_standing(business, &list),
            CityListStanding::Pending
        );
    }
}

/// TD-LIC-06 (LIC-011, D-177): a business not yet on the list waits as pending, gets no card, and
/// matches on a later daily list; with no match it stays pending, never active.
#[test]
fn pending_until_a_later_list_shows_the_name() {
    let business = record("Fake Dryer Duct Co", None, None, None);
    let monday = [record("Unrelated Test LLC", None, None, None)];
    let tuesday = [
        monday[0].clone(),
        record("Fake Dryer Duct Co", None, None, None),
    ];
    let vent = job("dryer_vent_cleaning", "dryer vent cleaning");
    let standing = city_list_standing(&business, &monday);
    assert_eq!(standing, CityListStanding::Pending);
    let waiting = Holder {
        city_list: standing,
        roc_covers: true,
        ..Holder::default()
    };
    assert_eq!(
        eligibility(&vent, &waiting),
        no_card(NoCardReason::CityListPending)
    );
    assert_eq!(
        city_list_standing(&business, &monday),
        CityListStanding::Pending
    );
    assert_eq!(
        city_list_standing(&business, &[]),
        CityListStanding::Pending
    );
    let standing = city_list_standing(&business, &tuesday);
    assert_eq!(standing, CityListStanding::Matched);
    let active = Holder {
        city_list: standing,
        ..Holder::default()
    };
    assert_eq!(eligibility(&vent, &active), Eligibility::Card);
}

/// TD-LIC-12 (LIC-028, LIC-029): vent cleaning and plug-in lights need no license; a vent reroute is
/// a handyman card in the handyman category; a new outlet is refused there and goes to R-11 only.
#[test]
fn vent_and_lights_split_between_no_license_handyman_and_electrician() {
    let refused = no_card(NoCardReason::RefusedJobType);
    assert_eq!(
        eligibility(
            &job("dryer_vent_cleaning", "dryer vent cleaning"),
            &on_city_list()
        ),
        Eligibility::Card
    );
    let reroute_here = job("dryer_vent_cleaning", "rerouting or replacing a vent");
    assert_eq!(eligibility(&reroute_here, &handyman()), refused);
    let reroute = job("handyman", "dryer vent reroute");
    assert_eq!(
        eligibility(&reroute, &handyman()),
        Eligibility::HandymanCard
    );
    assert_eq!(
        eligibility(
            &job("holiday_lights", "plug-in light hanging"),
            &on_city_list()
        ),
        Eligibility::Card
    );
    let outlet_here = job("holiday_lights", "new outlet, circuit or hardwiring");
    assert_eq!(
        eligibility(
            &outlet_here,
            &Holder {
                roc_covers: true,
                ..on_city_list()
            }
        ),
        refused
    );
    let outlet = job("electrical", "new circuit");
    assert_eq!(eligibility(&outlet, &roc()), Eligibility::Card);
    assert_eq!(
        eligibility(&outlet, &handyman()),
        no_card(NoCardReason::RocNotCovered)
    );
}

/// TD-LIC-12 (LIC-030, LIC-031): freestanding appliance repair goes to R-63 or a handyman, gas work
/// to licensed businesses only; weekly pool cleaning needs no license, equipment goes to R-6 or a
/// handyman, and a gas heater only to a licensed business.
#[test]
fn appliances_and_pools_split_between_licensed_handyman_and_no_license() {
    let refused = no_card(NoCardReason::RefusedJobType);
    let washer = job("appliance_repair", "washer");
    assert_eq!(eligibility(&washer, &roc()), Eligibility::Card);
    assert_eq!(eligibility(&washer, &handyman()), Eligibility::HandymanCard);
    for gas in [
        "gas appliance repair (gas dryer, range or oven)",
        "gas, plumbing or electrical line work",
    ] {
        let gas = job("appliance_repair", gas);
        assert_eq!(eligibility(&gas, &handyman()), refused);
        assert_eq!(eligibility(&gas, &roc()), Eligibility::Card);
    }
    assert_eq!(
        eligibility(
            &job("pool_cleaning", "weekly pool service"),
            &on_city_list()
        ),
        Eligibility::Card
    );
    assert_eq!(
        eligibility(&job("pool_cleaning", "pool equipment repair"), &handyman()),
        refused
    );
    let equipment = job("pool_repair", "equipment replacement");
    assert_eq!(eligibility(&equipment, &roc()), Eligibility::Card);
    assert_eq!(
        eligibility(&equipment, &handyman()),
        Eligibility::HandymanCard
    );
    let heater = job("pool_repair", "gas pool heater");
    assert_eq!(eligibility(&heater, &handyman()), refused);
    assert_eq!(
        eligibility(
            &heater,
            &Holder {
                roc_covers: true,
                ..handyman()
            }
        ),
        Eligibility::Card
    );
}
