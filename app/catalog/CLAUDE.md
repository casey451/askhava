# askhava-catalog: categories, job types, license and fee rules, category switches

## Purpose
The layer-2 crate that loads `docs/catalog/categories.yaml` (the K2b list) into the `catalog` schema and says which categories are on (`docs/architecture.md` section 3). T021 built the loader, the category/job-type/license-rule/fee-rule tables, the not-offered list and the owner's logged switch. T177 added the question bank loader and the no-warranty rule.

## Never break
- INT-048, D-1515: no code names a market or hard-codes its towns, ZIPs, state or time zone; a new market is a data change to `markets.yaml` with a decision entry (the file is in `kit/plan.lock`, D-3114). The Phoenix metro market lives only in `src/tests/markets.rs`. Coordinates are copied from the Census file by script, never typed; 86515 is left out on purpose (its point is in New Mexico, D-3115). `launched_at` is never written by the sync. The sync only adds and updates rows (the app role has no DELETE on the market tables), so a town, market or centroid removed from the files stays in `catalog.market`, `market_town` and `zcta_centroid`; read markets, towns and locations only through the compiled-in API (`markets`, `service_area_towns`, `town_for_zip`, `zip_location`), never from those tables.
- Loading again never switches a category on; it switches off, with a system audit row, one now not offered, ROC with no classes, or dropped from the file (D-747): `an_unresolved_class_and_a_non_owner_cannot_switch_on`, `a_reload_with_no_classes_switches_a_roc_category_off`, `a_reload_applies_a_changed_not_offered_list`, `a_category_dropped_from_the_file_is_switched_off`.
- A repeat switch writes no audit row: `switching_a_category_is_logged`.
- `CatalogError` carries no value or database message (PRIV-016).
- `catalog` calls `ops` only (`kit/layers.json`).

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/catalog/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/catalog/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
