# askhava-quotes: who receives each job card, the cards and the quotes

## Purpose

The layer-5 crate that decides who receives each job card, and later holds the cards and the quotes (`docs/architecture.md` section 3). T044 built the matching: a card goes to every active, verified business serving the job's Arizona offer towns whose licenses cover every candidate job type and which ticked one, with no cap (D-1013 to D-1015).

## What it owns

- `src/api.rs`: the only public surface: `send_cards`, `card_holders`, the types below and `api::rules` (`card_for`, `recipients`).
- `src/models.rs`: `CardJob`, `OfferTown`, `CardBusiness`, `CardDecision`, `Skip`, `Recipient`, `QuotesError` and the `quotes.job_card` queries.
- `src/rules.rs` (pure): `card_for`, `recipients`, `serves`, `ticked`, `covered`.
- `src/service.rs`: `send_cards` (choose and record), `card_holders`.
- Migration `0067_quotes_matching.sql`: the `quotes` schema and `quotes.job_card` (job and business IDs, handyman flag, `sent_at`; unique per job and business; no `DELETE` for `askhava_app`). No homeowner detail.

## Rules it enforces

- CARD-001: `rules::card_for` (active through `AccountStatus::receives_cards`, verified, serves a town, ticked one candidate, every candidate covered), in the order of `Skip`.
- CARD-002: `rules::serves` (any offered town whose state is `AZ`, the same rules for neighbouring towns; a non-Arizona town never matches, INT-022, D-1014).
- CARD-003: `rules::recipients` keeps every qualifying business; no cap anywhere.
- LIC-035, LIC-043: `rules::covered` fills `Holder::roc_covers` per candidate from `licensing::api::ClassMap::covers` and asks `licensing::api::eligibility_for_request` (never `eligibility`, so the AI-012, LIC-021, LIC-022 handyman block holds); one handyman candidate makes a handyman card (LIC-014, LIC-019).
- JOB-006: `models::insert_cards` uses `ON CONFLICT DO NOTHING`, so `send_cards` is safe to run twice.
- Not here yet: the BIZ-049 largest-job and recurring-work skips (T155, D-492); CARD-004 overflow, CARD-015 daily check and CARD-016 owner pause switches feed `verified` or later checks.

## Never break

- A class name is read only by the license map: ROC cover comes from `ClassMap::covers`, never from code: `src/tests/matching_examples.rs::td_lic_02_matching_examples_m01_to_m16`.
- Every candidate must be covered, not just one: `src/tests/matching_examples.rs::td_lic_02_every_candidate_must_be_covered_and_the_town_served`.
- A card goes exactly when qualified, neighbouring towns by the same rule, no cap: `src/tests/prop_matching_props.rs`.
- One card row per job and business: `src/tests/matching_examples.rs::td_lic_02_fifty_qualified_businesses_get_fifty_cards`.
- No status is compared outside `businesses`' `status.rs`; ask `receives_cards` (SEC-031).

## How to change

- Add a matching check (such as BIZ-049's skips): a `Skip` variant, a step in `rules::card_for`, a field on `CardJob` or `CardBusiness`, cases in both test files; update CARD-001's line here.
- Change which classes get which job types: `docs/catalog/license_map.yaml` only (`app/licensing/CLAUDE.md`), then `cargo test -p askhava-quotes`.
- Change the card table: a new migration, then `sqlx migrate run` and `cargo sqlx prepare --workspace -- --all-targets --all-features`.

## Tests that guard it

- `src/tests/matching_examples.rs`: TD-LIC-02 (M01 to M16 from `docs/catalog/matching_examples.yaml`; 50 qualified businesses get 50 recorded cards, on a fresh database with `DATABASE_URL` set).
- `src/tests/prop_matching_props.rs`: TD-LIC-02 property tests (card exactly when qualified, neighbouring towns, no cap).
- Run `cargo test -p askhava-quotes tests::matching_examples` and `cargo test -p askhava-quotes tests::prop_matching_props`.
- TD-LIC-26 (LIC-036, a handyman's text quote gets the portal link) is listed for T044 but covers none of its requirements and needs quote intake by text, which does not exist yet; no case is written here (D-1016).
