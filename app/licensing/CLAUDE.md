# askhava-licensing: who may receive a card on its licenses

## Purpose

The layer-3 crate that decides, from a business's licenses alone, whether it may receive a job's card (`docs/architecture.md` section 3). T034 built the agency-licensed categories (pest by attestation, alarm, inspection, septic), the city-list match for no-license categories and the boundary job types (dryer vents, holiday lights, appliances, pools). T035 added the handyman limits (`src/handyman.rs`), T036 the daily list imports and daily check (`src/imports.rs`, `src/service.rs`), T153 the license-map class coverage, T180 the live lookup, T037 the card lines.

## What it owns

- `src/api.rs`: the only public surface: `eligibility`, `eligibility_for_request`, `city_list_standing`, `inspection_fee`, the `handyman` functions, the list imports and checks below and the types below.
- `src/imports.rs` (T036, pure): `parse_roc_list`, `parse_city_list`, `ROC_HEADER`, `CITY_HEADER`, `RocRow`, `RocStatus`, `CityRow` (no `Debug`), `ParsedList`, `ImportError`, `normalise_license_number`.
- `src/service.rs` (T036, database): `import_roc_list`, `import_city_list`, `check_license`, `roc_check_allows_card`, `read_back`, `city_standing`, `ImportSummary`, `DailyCheck`, `ReadBack`, `Freshness`.
- `src/models.rs`: `LicenseKind` (the catalog's `license.kind`), `Agency` (code, license-map name, its one category), `PestRecordCategory`, `PestAttestation` (checked constructor), `CityListStanding`, `CityRecord` (no `Debug`), `Holder`, `Job`, `Eligibility`, `NoCardReason`, `SaleAnswer`, `InspectionFee`, `LicensingError`.
- `src/rules.rs`: the pure rules; no database.
- `src/handyman.rs`: the handyman limits (T035): `never_handyman`, `check_quote`, `split_job`, `QuotePrice`, `QuoteSubmission`, `QuoteRefusal`, `HandymanJob`, `JobRequest`, `NeverHandyman`, `HANDYMAN_CARD_LINE`, `HANDYMAN_LIMIT_CENTS`, `SPLIT_JOB_WINDOW_DAYS`; it compiles in `docs/catalog/gas_keywords.yaml` (D-897).
- Migration `0048_licensing_agency_rules.sql`: the `licensing` schema and `licensing.business_license` (one row per other-agency license; a pest row is `self_attested` with its record categories and `paused_at`; no `DELETE` for `askhava_app`).
- Migration `0050_licensing_imports.sql`: `licensing.list_import`, `licensing.agency_record` (ROC rows), `licensing.city_license_entry`, `licensing.license_check`; append-only for `askhava_app`, no contact column (D-902).

## Rules it enforces

- LIC-011: `rules::city_list_standing` (business, trade or owner name, case and punctuation ignored; a city license number given on both sides must agree; otherwise `Pending`, D-177, D-856) and `rules::no_license` (a pending business gets no card).
- LIC-012, LIC-047: `rules::pest` (attestation only, termites need wood-destroying, pre-emergent needs weed control, a reported license pauses); `models::PestAttestation::new` (ticked box and a number).
- LIC-013: `rules::roc_and_agency` (Board alarm certificate and ROC cover).
- LIC-014, LIC-030, LIC-031: `rules::handyman` (listed job type of a handyman-allowed category, handyman on the city list, `HandymanCard`).
- AI-012, LIC-021, LIC-022: `rules::eligibility_for_request` withholds a `HandymanCard` (`RefusedJobType`) when `handyman::never_handyman` finds a gas word in the request text or a candidate, or a smoke-alarm, fire-sprinkler or permit-type candidate (D-898).
- LIC-019, LIC-020, LIC-025, LIC-036: `handyman::check_quote` (certification box on every handyman quote; $1,000.00 or more, top of any range, refused for construction work by a handyman or in a no-license construction category; no limit outside construction).
- LIC-023: `handyman::split_job` (same business, same address, less than the window apart, a shared job type or tops together reaching $1,000).
- LIC-024: `handyman::HANDYMAN_CARD_LINE`.
- LIC-015: `rules::agency_only` (certified inspectors) and `rules::inspection_fee` (for a `home_inspection` job, a yes or a fee-free job type is `FeeFree`; any other category is `Normal`).
- LIC-016: `rules::agency_only` (ADEQ septage hauler).
- LIC-028, LIC-029, LIC-031: `rules::no_license` (listed no-license job types only; refused ones such as a new outlet or vent reroute are `RefusedJobType`; the handyman category gives `HandymanCard` to a business signed up as a handyman, else `NotHandyman`).
- CARD-015, LIC-007: `service::check_license` (latest ROC list by date; per license number, D-906: `active` when any class on the number is Active, `inactive`, `not_listed`) and `service::roc_check_allows_card` (the latest check of the covering ROC license is `active` and within the caller's `Freshness`; ROC-licensed categories only: pest is attested, LIC-048, and other-agency, no-license and handyman cards never call it).
- LIC-006, LIC-027: `service::read_back` (classes and issue year from the latest ROC list; never typed); `imports::parse_roc_list` (a wrong header rejects the list, a malformed row is skipped and counted, a list with no good row or more malformed than good rows is refused, `TooFewRows`, and so is a list under half the previous list's rows, `imports::shrank_too_far` called from `service::start_import`, D-905; license numbers upper case with no spaces; a repeated number and class keeps the stricter status). LIC-027 is partial here: the read-back is the ROC's only; other agencies' come with T180.
- LIC-010: the lists are read only by license number (`read_back`, `check_license`) or matched against one business's names (`service::city_standing`); no function lists them out and no contact column is stored.

## Never break

- Pest work opens only through `Holder::pest`, never through `agencies`, an ROC class or the city list (D-417): `src/tests/agency_rules.rs::pest_jobs_go_only_to_attested_pest_licensees`.
- A no-license business still pending on the city list gets no card: `src/tests/agency_rules.rs::pending_until_a_later_list_shows_the_name`.
- A refused job type never reaches a handyman or no-license business: `src/tests/agency_rules.rs::appliances_and_pools_split_between_licensed_handyman_and_no_license`.
- `Agency` stays equal to the license map's `agencies`, the catalog's agency kinds and the migration's codes: `src/tests/scope_examples.rs`.
- Matching examples M09 to M12 hold: `src/tests/scope_examples.rs::matching_examples_for_agency_rules_hold`.
- A gas, smoke-alarm, sprinkler or permit-type request never gets a `HandymanCard`, and an unreadable gas list blocks every one: `src/tests/prop_handyman_never_gas.rs`, `src/tests/handyman.rs::malformed_gas_file_is_refused`.
- The older `eligibility` (in `src/api.rs`, from `rules`) does not apply the AI-012, LIC-021, LIC-022 handyman block; code that chooses recipients must call `eligibility_for_request`.
- The gas list keeps every AI-012 word: `src/tests/handyman.rs::gas_list_holds_ai_012_words_and_catches_d_157_cases`.
- `Holder::roc_covers` means the license map covers this job type (T153); this crate never reads a class name to decide ROC cover.
- A changed list header records nothing, and a malformed row never stops an import: `tests/integration/tests/license_imports.rs::daily_roc_import_records_rows_and_counts_malformed_ones`.
- A cleanly cut list under half the previous one never becomes the latest: `tests/integration/tests/license_imports.rs::a_list_under_half_the_previous_one_is_refused`.
- A card needs the latest daily check `active`: `tests/integration/tests/license_imports.rs::card_needs_latest_daily_check_active`.
- An empty or truncated list never becomes the latest: `tests/integration/tests/license_imports.rs::truncated_lists_are_refused_and_conflicts_keep_the_stricter_status`.

## How to change

- Add an agency-licensed category: an `Agency` variant (code, map name, category), the `agencies` row in `docs/catalog/license_map.yaml`, the migration check list in a new migration; run `cargo test -p askhava-licensing`.
- Add a pest record category: `PestRecordCategory`, `needed_for`, a `licensing.business_license` column and the map's `record_category_needed`.
- Add a gas word: `docs/catalog/gas_keywords.yaml`, then `python3 scripts/check_plan_lock.py --write` and a `docs/decisions.md` entry. Add a permit-type or never-at-any-price job type: `PERMIT_TYPE` or `NEVER_AT_ANY_PRICE` in `src/handyman.rs` and a case in `src/tests/handyman.rs`.
- Change the city-list match: `rules::city_list_standing`, `compared`; update LIC-011 and D-856.
- Change a list layout: `ROC_HEADER` or `CITY_HEADER` and its parser in `src/imports.rs`, the fixtures in `tests/integration/tests/license_imports.rs`, and D-902.

## Tests that guard it

- `src/tests/agency_rules.rs`: TD-LIC-06, TD-LIC-12, TD-LIC-17.
- `src/tests/scope_examples.rs`: TD-LIC-06, TD-LIC-17 against `docs/catalog/matching_examples.yaml`, `license_map.yaml`, `categories.yaml` and the migration.
- `src/tests/prop_rules.rs`: property tests (R17, D-860) over random businesses and jobs: handyman cards, city-list need, agency licensees exactly, the unlicensed business, the city-list name match and the inspection fee.
- `src/tests/handyman.rs`: TD-LIC-08 (every worked example through `check_quote`, the certification box, the card line), TD-LIC-09 (the gas list and the blocked job types), TD-LIC-10 (the split-job window and totals).
- `src/tests/prop_handyman_never_gas.rs`: property tests for TD-LIC-09 (gas words and blocked candidates never reach a handyman), TD-LIC-08 (accepted exactly when certified and under the limit) and TD-LIC-10 (near jobs reaching $1,000 are blocked).
- `src/tests/imports.rs` (pure, D-932): the list readers: header check, skipped and counted malformed rows (wrong field count, unclosed quote, over-long field, bad date, unknown status), the `TooFewRows` refusal for both lists, the half-size check (`shrank_too_far`, D-905), quoted fields and number normalising; it keeps `app/licensing/` at or above its 95% branch floor.
- `tests/integration/tests/license_imports.rs` (database): the imports with planted malformed rows, the read-back, the daily check and card gate, the city match and append-only tables.
