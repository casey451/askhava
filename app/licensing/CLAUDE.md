# askhava-licensing: who may receive a card on its licenses

## Purpose

The layer-3 crate that decides, from a business's licenses alone, whether it may receive a job's card (`docs/architecture.md` section 3). T034 built the agency-licensed categories (pest by attestation, alarm, inspection, septic), the city-list match for no-license categories and the boundary job types (dryer vents, holiday lights, appliances, pools). T035 adds the handyman limits, T036 the daily list imports, T153 the license-map class coverage, T180 the live lookup, T037 the card lines.

## What it owns

- `src/api.rs`: the only public surface: `eligibility`, `city_list_standing`, `inspection_fee` and the types below.
- `src/models.rs`: `LicenseKind` (the catalog's `license.kind`), `Agency` (code, license-map name, its one category), `PestRecordCategory`, `PestAttestation` (checked constructor), `CityListStanding`, `CityRecord` (no `Debug`), `Holder`, `Job`, `Eligibility`, `NoCardReason`, `SaleAnswer`, `InspectionFee`, `LicensingError`.
- `src/rules.rs`: the pure rules; no database.
- Migration `0048_licensing_agency_rules.sql`: the `licensing` schema and `licensing.business_license` (one row per other-agency license; a pest row is `self_attested` with its record categories and `paused_at`; no `DELETE` for `askhava_app`).

## Rules it enforces

- LIC-011: `rules::city_list_standing` (business, trade or owner name, case and punctuation ignored; a city license number given on both sides must agree; otherwise `Pending`, D-177, D-856) and `rules::no_license` (a pending business gets no card).
- LIC-012, LIC-047: `rules::pest` (attestation only, termites need wood-destroying, pre-emergent needs weed control, a reported license pauses); `models::PestAttestation::new` (ticked box and a number).
- LIC-013: `rules::roc_and_agency` (Board alarm certificate and ROC cover).
- LIC-014, LIC-030, LIC-031: `rules::handyman` (listed job type of a handyman-allowed category, handyman on the city list, `HandymanCard`).
- LIC-015: `rules::agency_only` (certified inspectors) and `rules::inspection_fee` (for a `home_inspection` job, a yes or a fee-free job type is `FeeFree`; any other category is `Normal`).
- LIC-016: `rules::agency_only` (ADEQ septage hauler).
- LIC-028, LIC-029, LIC-031: `rules::no_license` (listed no-license job types only; refused ones such as a new outlet or vent reroute are `RefusedJobType`; the handyman category gives `HandymanCard` to a business signed up as a handyman, else `NotHandyman`).

## Never break

- Pest work opens only through `Holder::pest`, never through `agencies`, an ROC class or the city list (D-417): `src/tests/agency_rules.rs::pest_jobs_go_only_to_attested_pest_licensees`.
- A no-license business still pending on the city list gets no card: `src/tests/agency_rules.rs::pending_until_a_later_list_shows_the_name`.
- A refused job type never reaches a handyman or no-license business: `src/tests/agency_rules.rs::appliances_and_pools_split_between_licensed_handyman_and_no_license`.
- `Agency` stays equal to the license map's `agencies`, the catalog's agency kinds and the migration's codes: `src/tests/scope_examples.rs`.
- Matching examples M09 to M12 hold: `src/tests/scope_examples.rs::matching_examples_for_agency_rules_hold`.
- `Holder::roc_covers` means the license map covers this job type (T153); this crate never reads a class name to decide ROC cover.

## How to change

- Add an agency-licensed category: an `Agency` variant (code, map name, category), the `agencies` row in `docs/catalog/license_map.yaml`, the migration check list in a new migration; run `cargo test -p askhava-licensing`.
- Add a pest record category: `PestRecordCategory`, `needed_for`, a `licensing.business_license` column and the map's `record_category_needed`.
- Change the city-list match: `rules::city_list_standing`, `compared`; update LIC-011 and D-856.

## Tests that guard it

- `src/tests/agency_rules.rs`: TD-LIC-06, TD-LIC-12, TD-LIC-17.
- `src/tests/scope_examples.rs`: TD-LIC-06, TD-LIC-17 against `docs/catalog/matching_examples.yaml`, `license_map.yaml`, `categories.yaml` and the migration.
- `src/tests/prop_rules.rs`: property tests (R17, D-860) over random businesses and jobs: handyman cards, city-list need, agency licensees exactly, the unlicensed business, the city-list name match and the inspection fee.
