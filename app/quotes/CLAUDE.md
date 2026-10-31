# askhava-quotes: who receives each job card, the cards and the quotes

## Purpose

The layer-5 crate that decides who receives each job card, and later holds the cards and the quotes (`docs/architecture.md` section 3). T044 built the matching: a card goes to every active, verified business serving the job's Arizona offer towns whose licenses cover every candidate job type and which ticked one, with no cap (D-1013 to D-1015). T184 added the owner pauses, "jobs nobody else covers", late cards and the coverage count intake tells the homeowner (D-1018 to D-1022). T045 added the anonymous job card and the final plain-code gate every card passes last (D-1024 to D-1026).

## What it owns

- `src/api.rs`: the only public surface: `send_cards`, `send_job_cards`, `send_late_cards`, `coverage`, `card_holders`, the types below and `api::rules` (`card_for`, `card_under`, `match_job`, `coverage`, `recipients`).
- `src/models.rs`: `CardJob`, `OfferTown`, `CardBusiness`, `CardDecision`, `Skip`, `Pooled`, `OwnerPauses`, `Pool`, `Coverage`, `Recipient`, `QuotesError` and the `quotes.job_card` queries.
- `src/rules.rs` (pure): `card_for`, `card_under`, `match_job`, `coverage`, `recipients`, `owner_paused`, `gate`, `serves`, `ticked`, `covered`.
- `src/cards.rs` (pure): `CardParts`, `JobCard` (private fields: town, place, photo IDs, text), `Place`, `Timing`, `Danger`, `DEFAULT_MIN_WORDS`, `build_card`.
- `src/gate.rs` (pure): `final_gate` (CARD-008) and `word_count` (CARD-009).
- `src/service.rs`: `send_cards` (choose and record), `send_job_cards` (full match, new holders only), `send_late_cards` (CARD-018), `coverage` (shipped map), `card_holders`.
- Migration `0067_quotes_matching.sql`: the `quotes` schema and `quotes.job_card` (job and business IDs, handyman flag, `sent_at`; unique per job and business; no `UPDATE`, `DELETE` or `TRUNCATE` for `askhava_app`). No homeowner detail.

## Rules it enforces

- CARD-001: `rules::card_for` (active through `AccountStatus::receives_cards`, verified, serves a town, ticked one candidate, every candidate covered), in the order of `Skip`.
- CARD-002: `rules::serves` (any offered town whose state is `AZ`, the same rules for neighbouring towns; a non-Arizona town never matches, INT-022, D-1014).
- CARD-003: `rules::recipients` keeps every qualifying business; no cap anywhere.
- LIC-035, LIC-043: `rules::covered` fills `Holder::roc_covers` per candidate from `licensing::api::ClassMap::covers` and asks `licensing::api::eligibility_for_request` (never `eligibility`, so the AI-012, LIC-021, LIC-022 handyman block holds); one handyman candidate makes a handyman card (LIC-014, LIC-019).
- JOB-006: `models::insert_cards` uses `ON CONFLICT DO NOTHING`, so `send_cards` is safe to run twice.
- CARD-004, D-1018: `rules::match_job` sends to the `overflow_on` businesses passing every check but the ticks only when the checklist match reaches nobody; `rules::match_job_held` (used by `send_job_cards` and `send_late_cards` with the job's card holders) also keeps overflow off when a holder's checklist covers the job, whatever its status now (TD-LIC-03).
- CARD-016, OPS-003: `AccountStatus::receives_cards` for a paused business; `OwnerPauses` (all, trade codes, town codes, filled by the caller from `ops::api::is_paused`) gives `Skip::OwnerPaused` in `card_under` under "all", a candidate's trade, the job's own town (the first offer town) or every offer town the business serves (D-1018).
- CARD-018, D-042: `send_late_cards` records only new holders, sent now, and only while `JobStatus::takes_late_cards` (Open, No quotes yet). The handler calling it when a business becomes eligible is not built yet (D-1021).
- INT-017, INT-032, D-1019: `coverage` is `Send(n)` from `match_job`; else `NotCovered` when an active, verified business serving an offer town is licensed for every candidate (ticks, overflow and pauses aside); else `Split` when one trade's part alone reaches a business; else `NotCovered`; intake turns it into the text.
- CARD-005, CARD-006, PRIV-006: `cards::JobCard` has no slot for a name, phone, email, street address, on-site contact or original photo; only `build_card` makes one (D-1024).
- CARD-007: `cards::build_card` lines: approved description, card-field answers, "How soon: <answer>" ("Needed by <day>"), "911 text sent: <Danger>", concept-image label; each through the gate, then the whole text again.
- CARD-008: `gate::final_gate` cuts each sentence at its first contact-shaped word (D-1025 lists the shapes); its output is a fixed point.
- CARD-009: `build_card` puts "<Category> job (details on the job page)" when the gated description has fewer than `min_words` words (the caller's setting, default 5).
- Not here yet: CARD-010's restricted-word removal; the card notice (CARD-019 to CARD-021); the BIZ-049 largest-job and recurring-work skips (T155, D-492); CARD-015's daily check feeds `verified`.

## Never break

- A class name is read only by the license map: ROC cover comes from `ClassMap::covers`, never from code: `src/tests/matching_examples.rs::td_lic_02_matching_examples_m01_to_m16`.
- Every candidate must be covered, not just one: `src/tests/matching_examples.rs::td_lic_02_every_candidate_must_be_covered_and_the_town_served`.
- A card goes exactly when qualified, neighbouring towns by the same rule, no cap: `src/tests/prop_matching_props.rs`.
- One card row per job and business: `src/tests/matching_examples.rs::td_lic_02_fifty_qualified_businesses_get_fifty_cards`.
- No status is compared outside `businesses`' `status.rs` or `jobs`' `status.rs`; ask `receives_cards` and `takes_late_cards` (SEC-031).
- No contact survives a card, the CARD-009 line replaces fewer than 5 words, and the gate runs last: `src/tests/prop_card_no_contact.rs`.
- Overflow only when no checklist business gets the card; pauses send nothing: `src/tests/overflow.rs`.
- A late card goes only in Open or No quotes yet, with its own send time: `tests/integration/tests/card_late_eligible.rs`.

## How to change

- Gate miss found: add the case to `src/tests/prop_card_no_contact.rs` first, then a word to the matching list in `src/gate.rs` (one `const` per shape); never drop a test case; log the change in `docs/decisions.md` like D-1025.
- Add a card line or a kind of place: a field on `CardParts` or a `Place` variant, its line in `build_card`, a case in `prop_card_no_contact.rs`; never add a free-text field that bypasses the gate.
- Add a matching check (such as BIZ-049's skips): a `Skip` variant, a step in `rules::card_for`, a field on `CardJob` or `CardBusiness`, cases in both test files; update CARD-001's line here.
- Change which classes get which job types: `docs/catalog/license_map.yaml` only (`app/licensing/CLAUDE.md`), then `cargo test -p askhava-quotes`.
- Change the card table: a new migration, then `sqlx migrate run` and `cargo sqlx prepare --workspace -- --all-targets --all-features`.

## Tests that guard it

- `src/tests/matching_examples.rs`: TD-LIC-02 (M01 to M16 from `docs/catalog/matching_examples.yaml`; 50 qualified businesses get 50 recorded cards, on a fresh database with `DATABASE_URL` set).
- `src/tests/prop_matching_props.rs`: TD-LIC-02 property tests (card exactly when qualified, neighbouring towns, no cap).
- `src/tests/overflow.rs`: TD-LIC-03, and the pure cases of TD-CARD-05, TD-INT-29, TD-INT-25 (`cargo test -p askhava-quotes tests::overflow`).
- `tests/integration/tests/card_late_eligible.rs`: TD-CARD-05, TD-INT-29, TD-INT-25 in the database (`cargo test -p askhava-integration --test card_late_eligible`).
- `src/tests/prop_card_no_contact.rs`: TD-CARD-01 (property and twin adversarial texts), TD-CARD-08, the card cases of TD-CARD-11, TD-INT-13 and TD-INT-33 (`cargo test -p askhava-quotes tests::prop_card_no_contact`).
- Run `cargo test -p askhava-quotes tests::matching_examples` and `cargo test -p askhava-quotes tests::prop_matching_props`.
- TD-LIC-26 (LIC-036, a handyman's text quote gets the portal link) is listed for T044 but covers none of its requirements and needs quote intake by text, which does not exist yet; no case is written here (D-1016).
