# askhava-licensing: who may receive a card on its licenses

## Purpose
The layer-3 crate that decides, from a business's licenses alone, whether it may receive a job's card (`docs/architecture.md` section 3). T034 built the agency-licensed categories (pest by attestation, alarm, inspection, septic), the city-list match for no-license categories and the boundary job types (dryer vents, holiday lights, appliances, pools).

## Never break
- Pest work opens only through `Holder::pest`, never through `agencies`, an ROC class or the city list (D-417): `src/tests/agency_rules.rs::pest_jobs_go_only_to_attested_pest_licensees`.
- A no-license business still pending on the city list gets no card: `src/tests/agency_rules.rs::pending_until_a_later_list_shows_the_name`.
- A refused job type never reaches a handyman or no-license business: `src/tests/agency_rules.rs::appliances_and_pools_split_between_licensed_handyman_and_no_license`.
- `Agency` stays equal to the license map's `agencies`, the catalog's agency kinds and the migration's codes: `src/tests/scope_examples.rs`.
- Matching examples M09 to M12 hold: `src/tests/scope_examples.rs::matching_examples_for_agency_rules_hold`.
- A gas, smoke-alarm, sprinkler or permit-type request never gets a `HandymanCard`, and an unreadable gas list blocks every one: `src/tests/prop_handyman_never_gas.rs`, `src/tests/handyman.rs::malformed_gas_file_is_refused`.
- The older `eligibility` (in `src/api.rs`, from `rules`) does not apply the AI-012, LIC-021, LIC-022 handyman block; code that chooses recipients must call `eligibility_for_request`.
- The gas list keeps every AI-012 word: `src/tests/handyman.rs::gas_list_holds_ai_012_words_and_catches_d_157_cases`.
- `Holder::roc_covers` means the license map covers this job type (T153); this crate never reads a class name to decide ROC cover outside `class_map`, and `class_map` reads only the map: `src/tests/class_map.rs::td_lic_19_golden_rows`, `td_lic_19_matching_examples_agree`.
- An unclear row is never inherited and R-62 never gets structural work: `src/tests/class_map.rs::td_lic_19_unclear_row_is_never_inherited`, `src/tests/class_limits.rs::r_62_never_receives_structural_work`.
- A changed list header records nothing, and a malformed row never stops an import: `tests/integration/tests/license_imports.rs::daily_roc_import_records_rows_and_counts_malformed_ones`.
- A cleanly cut list under half the previous one never becomes the latest: `tests/integration/tests/license_imports.rs::a_list_under_half_the_previous_one_is_refused`.
- A card needs the latest daily check `active`: `tests/integration/tests/license_imports.rs::card_needs_latest_daily_check_active`.
- An empty or truncated list never becomes the latest: `tests/integration/tests/license_imports.rs::truncated_lists_are_refused_and_conflicts_keep_the_stricter_status`.
- A truncated or lapsed list entry never strips a business's job types: `class_changes` skips a number missing from the newer list or with no Active class (`tests/integration/tests/license_reapply_daily.rs::td_biz_48_a_removed_class_unticks_pauses_and_emails_once`).
- A failed or misread live lookup records nothing and never changes a verified status: `tests/integration/tests/license_lookup_failures.rs::a_failed_lookup_is_unknown_and_keeps_the_verified_status`.
- A second license for one business and agency is refused and replaces nothing; the audit row holds the agency code, never the number: `tests/integration/tests/holder_read.rs::td_lic_29_a_second_license_for_one_agency_is_refused`.
- A holder never replaces `roc_check_allows_card`: its classes say what the license map covers, and an ROC card still needs the latest daily check `active` and fresh (CARD-015): `tests/integration/tests/holder_read.rs::an_inactive_roc_class_is_never_held`.
- A reported pest license gives no pest card, general or termite, until a found number replaces it; a report alerts once and audits no number: `tests/integration/tests/pest_license_report.rs::td_lic_22_a_wrong_license_report_pauses_pest_jobs`, `td_lic_22_a_found_number_clears_the_pause`.
- A record read stores nothing and logs no value: `tests/integration/tests/signup_own_record.rs::td_lic_30_reads_write_nothing_and_log_no_value`; a misread is never `Found`: `td_lic_30_unreadable_reads_raise_one_alert_a_day`.
- One chain per list: the next run's dedupe key is the list and the slot instant, and the alerts commit with it; never queue a second next run or raise the alerts outside that transaction: `tests/integration/tests/daily_license_lists.rs::td_lic_28_a_refused_list_fails_and_a_retry_raises_nothing_twice`. More: `(never 21)` in NOTES.md.
- Licensing never calls `businesses`: the handyman standing, ROC number and names come in `HolderInputs` (`kit/layers.json`).

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/licensing/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/licensing/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
