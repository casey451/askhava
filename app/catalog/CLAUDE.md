# askhava-catalog: categories, job types, license and fee rules, category switches

## Purpose

The layer-2 crate that loads `docs/catalog/categories.yaml` (the K2b list) into the `catalog` schema and says which categories are on (`docs/architecture.md` section 3). T021 built the loader, the category/job-type/license-rule/fee-rule tables, the not-offered list and the owner's logged switch. T177 added the question bank loader and the no-warranty rule. Not here yet: the not-offered reply (T043), numeric settings and feature switches beyond the category switch.

## What it owns

- `src/api.rs`: the only public surface: `load_embedded`, `load`, `category`, `is_enabled`, `enabled_categories`, `is_not_offered`, `set_category_enabled`.
- `src/loader.rs`: `embedded` (the file is `include_str!`ed), `parse` (pure checks), `normalize`.
- `src/question_bank.rs`: `parse_bank`, `embedded_bank` (pure, in memory; `api::question_bank`, `api::parse_question_bank`): sets of 3 to 8 questions, both languages, card fields multiple choice only (LIC-040, D-754).
- `src/service.rs`: the SQL: `load`, reads, `set_enabled`.
- `src/models.rs`: file types (`Catalog`, `CategoryDef`, `LicenseRule`), `Category`, `CatalogError`.
- Migration `0027_catalog_module.sql`: `catalog.category`, `job_type`, `license_rule`, `fee_rule`, `not_offered`.

## Rules it enforces

- LIC-001: each category stores job types (with one-time, fee-free, kind and format), license rule, recurring flag, fee tier, after-hours flag and keywords.
- LIC-002: `set_category_enabled` updates the one column and writes an `ops` audit row (`catalog.category_switched`, subject `catalog.category`) in one transaction; no code change is needed to switch.
- LIC-003, LIC-005: a new category starts as the file says (`enabled: false` for garage doors, junk removal); a category not in the file is never stored and reads as off.
- LIC-004: the eight not-offered kinds are forced off by `parse` and refused by `set_category_enabled` (`NotOffered`); `is_not_offered` serves T043.
- AI-017: no function takes a license class; classes come from the reviewed file only; a ROC category with no classes cannot be switched on (`LicenseUnresolved`); only `Actor::Owner` switches (`OwnerOnly`).

- LIC-042: `loader::covers_warranty` makes `parse` refuse a category, job type or keyword (and `parse_bank` a set code) that covers a warranty, protection plan, service contract or guarantee (D-755).

## Never break

- Loading again never switches a category on; it switches off, with a system audit row, one now not offered, ROC with no classes, or dropped from the file (D-747): `an_unresolved_class_and_a_non_owner_cannot_switch_on`, `a_reload_with_no_classes_switches_a_roc_category_off`, `a_reload_applies_a_changed_not_offered_list`, `a_category_dropped_from_the_file_is_switched_off`.
- A repeat switch writes no audit row: `switching_a_category_is_logged`.
- `CatalogError` carries no value or database message (PRIV-016).
- `catalog` calls `ops` only (`kit/layers.json`).

## How to change

- Add or change a category: edit the file (a `docs/decisions.md` entry and `scripts/check_catalog.py`), then `load_embedded` (the server does not call it yet; D-748 says the first task with a start-up pool calls it); a changed `enabled` for an existing category does not apply, switch it through `set_category_enabled`.
- Add a column: a new migration starting `-- catalog (T###)`, update `service.rs`, regenerate `.sqlx/`.
- Run `cargo test -p askhava-catalog`, `cargo test -p askhava-integration --test catalog_settings`, clippy and `bash scripts/smoke.sh`.

## Tests that guard it

- `src/tests/question_bank_load.rs` (TD-LIC-24, TD-LIC-16; LIC-040 to LIC-042).
- `src/tests/loader.rs` and `tests/integration/tests/catalog_settings.rs` (TD-LIC-01; LIC-001 to LIC-005, AI-017).
