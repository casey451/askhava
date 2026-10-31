# askhava-quotes: who receives each job card, the cards and the quotes

## Purpose

The layer-5 crate that decides who receives each job card, and later holds the cards and the quotes (`docs/architecture.md` section 3). T044 built the matching: a card goes to every active, verified business serving the job's Arizona offer towns whose licenses cover every candidate job type and which ticked one, with no cap (D-1013 to D-1015). T184 added the owner pauses, "jobs nobody else covers", late cards and the coverage count intake tells the homeowner (D-1018 to D-1022). T045 added the anonymous job card and the final plain-code gate every card passes last (D-1024 to D-1026). T185 added the carrier-restricted removal to that gate and the per-business Spanish, fee-free and "Sent to <N> businesses." lines (D-1031 to D-1033). T046 added quote intake (the 24-hour window, the checked quote, its bounces and the stored record) and the every-window-ended check that hands the counts to `jobs` (D-1048 to D-1052). T186 added the quote card: the same lines for the homeowner and the business's preview, its own words always, an AI summary only when allowed and claim-free (D-1111 to D-1113).

## What it owns

- `src/api.rs`: the only public surface: `send_cards`, `send_job_cards`, `send_late_cards`, `coverage`, `card_holders`, `parse_quote`, `submit_quote`, `confirm_quote`, `mark_unavailable`, `shown_quotes`, `window_open`, `QUOTE_WINDOW`, `windows_ended`, `run_windows_end`, `WINDOWS_KIND`, `quote_card`, `check_summary`, `switch_on`, `POLICY_SUMMARY_SENTENCE`, the types below and `api::rules` (`card_for`, `card_under`, `match_job`, `coverage`, `recipients`).
- `src/models.rs`: `CardJob`, `CardLines`, `OfferTown`, `CardBusiness`, `CardDecision`, `Skip`, `Pooled`, `OwnerPauses`, `Pool`, `Coverage`, `Recipient`, `QuotesError`, the `quotes.job_card` queries and the `quotes.quote` queries (`card_sent_at`, `upsert_quote`, `edit_facts`, `mark_unavailable`, `confirm_quote`, `shown_quotes`, `window_state`, `stored_card`).
- `src/intake.rs`: `Format`, `Period`, `DayPart`, `Price`, `QuoteForm`, `QuoteRules`, `Bounce` (with the business's line, `message`), `Quote` (only `parse_quote` makes one), `Submitted`, `window_open`, `parse_quote` (pure), `submit_quote`, `confirm_quote`, `mark_unavailable`, `shown_quotes`. `Price` and `QuoteForm` have hand-written `Debug` that leaves out the notes, original wording and tier names (R6).
- `src/quote_card.rs`: `QuoteCard` (`lines(language)`, no viewer argument), `QuoteFacts`, `TripFee`, `Parts`, `Permit`, `Spanish`, `CardSource`, `quote_card` (reads `models::stored_card` and the business's after-hours tick), `SummarySetting` (default `Off`), `switch_on`, `POLICY_SUMMARY_SENTENCE`, `check_summary`, `Summary` (only `check_summary` makes one); the English and Spanish card words (`EN`, `ES`).
- `src/handlers.rs`: the `quotes.windows_end` task: `schedule`, `windows_ended`, `run_windows_end`, `WindowsCheck`.
- `src/rules.rs` (pure): `card_for`, `card_under`, `match_job`, `coverage`, `recipients`, `owner_paused`, `gate`, `serves`, `ticked`, `covered`.
- `src/cards.rs` (pure): `CardParts`, `JobCard` (private fields: town, place, photo IDs, text), `Place`, `Timing`, `Danger`, `DEFAULT_MIN_WORDS`, `build_card`, `JobCard::text_for`.
- `src/gate.rs` (pure): `final_gate` (CARD-008, CARD-010) and `word_count` (CARD-009).
- `src/service.rs`: `send_cards` (choose and record), `send_job_cards` (full match, new holders only), `send_late_cards` (CARD-018), `coverage` (shipped map), `card_holders`.
- Migration `0067_quotes_matching.sql`: the `quotes` schema and `quotes.job_card` (job and business IDs, handyman flag, `sent_at`; unique per job and business; no `UPDATE`, `DELETE` or `TRUNCATE` for `askhava_app`). No homeowner detail.
- Migration `0070_quotes_quote.sql`: `quotes.quote` (one per business per job; format, amounts, tiers, period, setup, hourly rate and hours, on-site tick, notes, original wording, `ai_read`, `confirmed`, earliest visit, `received_at`, `edited_at`, `withdrawn_at`, `unavailable_at`; no expiry column; no `DELETE` or `TRUNCATE` for `askhava_app`).

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
- CARD-010, D-485, D-492: each `gate::gate_once` pass runs `strip_restricted` after the contact cut (a domain holding a listed word is cut whole, D-1031): every `catalog::api::RESTRICTED_WORDS` phrase goes, whole words, any case, longest first; each removal logs only `entry=<list entry>`; a line with none is unchanged.
- CARD-012: `JobCard::text_for` adds "Homeowner prefers Spanish" only when `CardLines::prefers_spanish` and the business speaks Spanish; no matching input holds a language.
- CARD-019, FEE-045: `text_for` adds "No introduction fee for this job." when `CardLines::fee_free` and ends with "Sent to <N> businesses." ("Sent to 1 business.", D-1032); N is the caller's count, never capped.
- PICK-001, D-042, D-043: `intake::window_open` (24 hours from the business's own `job_card.sent_at`, shut at exactly 24 hours); `submit_quote` refuses no card, a closed window, or a job failing `JobStatus::takes_quotes` (D-1048).
- PICK-002, PICK-006, PICK-051: `submit_quote` stores the checked quote, a resubmission an edit (clearing any withdrawal); an edit is refused (`Submitted::EditClosed`) unless `JobStatus::takes_quote_edits` and no quote on the job is unavailable, while another business's first quote is still taken (D-043); confirmed unless `ai_read`; `confirm_quote` confirms only while `JobStatus::takes_quotes`; `shown_quotes` lists only confirmed, unwithdrawn quotes (D-1049).
- PICK-053, FEE-043, FEE-044, LIC-019: `parse_quote` requires the job's format; hourly with hours is a range (in `range` or `diagnostic_visit` jobs), without hours a site visit in `diagnostic_visit` only (`NeedsHours` elsewhere); the handyman limit is `licensing::api::check_quote` on the top, never the setup charge; no fee code here (D-1050, D-491).
- PICK-007, CARD-010: `intake::text_bounce` bounces notes, wording or a tier name when `final_gate` would cut a word, or on a restricted phrase; `Bounce` carries no text (D-1051).
- TXT-042, JOB-025: each accepted quote schedules `quotes.windows_end` for the last window end; `windows_ended` re-schedules for a later card, else passes `Windows { quoted, sent, choosable }` to `jobs::api::all_windows_ended`; `choosable` leaves out withdrawn quotes and those `picks` marked with `mark_unavailable` after a failed pick (D-1051).
- PICK-005, PICK-040, PICK-041, PICK-044: `QuoteCard::lines`: name, license lines, the "check it yourself" line, WORD-014's "Independent business, not affiliated with Ask Hava.", "Gets emergency requests at night" only for an after-hours business (BIZ-007), the six facts in fixed order, the format's definition plus "Free estimate" for no trip fee (never on a visit-fee quote), then the business's own words; English or Spanish (D-1111, D-1113).
- PICK-003, PICK-004, D-373: `check_summary` returns none while `SummarySetting::Off` and none when any summary word or number is outside the original wording and `LINKING_WORDS`; `switch_on` needs a `D-<n>` decision and a published policy holding `POLICY_SUMMARY_SENTENCE`; `quote_card` never fills a summary, as none is stored (D-1112).
- Not here yet: the first-quote move to Quotes ready (JOB-009); the PICK-040 fact columns (T148, so `CardSource::facts` comes from the caller); the Google and Yelp links (T165) and the price line (T164) on the card; the "no longer available" line (D-1051); the stored owner-portal summary setting; the withdraw action (PICK-051); calling `mark_unavailable` on a failed pick is `picks`' (T063).
- Not here yet: the card notice and the provider-accepted count (CARD-019 to CARD-021, T147); the BIZ-049 largest-job and recurring-work skips (T155, D-492); CARD-015's daily check feeds `verified`.

## Never break

- A class name is read only by the license map: ROC cover comes from `ClassMap::covers`, never from code: `src/tests/matching_examples.rs::td_lic_02_matching_examples_m01_to_m16`.
- Every candidate must be covered, not just one: `src/tests/matching_examples.rs::td_lic_02_every_candidate_must_be_covered_and_the_town_served`.
- A card goes exactly when qualified, neighbouring towns by the same rule, no cap: `src/tests/prop_matching_props.rs`.
- One card row per job and business: `src/tests/matching_examples.rs::td_lic_02_fifty_qualified_businesses_get_fifty_cards`.
- No status is compared outside `businesses`' `status.rs` or `jobs`' `status.rs`; ask `receives_cards` and `takes_late_cards` (SEC-031).
- No contact survives a card, the CARD-009 line replaces fewer than 5 words, and the gate runs last: `src/tests/prop_card_no_contact.rs`.
- No restricted phrase survives the gate and the log holds only the entry; the Spanish line only for Spanish speakers; the sent and fee-free lines: `src/tests/card_lines.rs`.
- Overflow only when no checklist business gets the card; pauses send nothing: `src/tests/overflow.rs`.
- A late card goes only in Open or No quotes yet, with its own send time: `tests/integration/tests/card_late_eligible.rs`.
- A quote is taken only inside its card's 24 hours and before Confirm; an unconfirmed quote is never shown; the all-quotes-in text is queued once, after the last window: `tests/integration/tests/quote_window.rs`.
- Contact in a quote bounces, and a bounce line repeats nothing the business wrote: `src/tests/quote_bounce.rs`.
- The business's preview is the homeowner's card, character for character, and the night line moves nothing: `tests/integration/tests/quote_card.rs`, `src/tests/quote_card.rs::td_pick_44_after_hours_line_only_when_ticked`.
- No summary while the setting is off, none that adds a word: `src/tests/quote_card.rs::td_pick_29_summary_off_by_default_and_never_adds_a_claim`.
- A setup charge never changes whether a quote passes: `src/tests/quote_parse.rs::td_fee_27_setup_never_counts`.

## How to change

- Change what a quote stores: a new migration on `quotes.quote`, the `Columns` spread in `models.rs`, `cargo sqlx prepare --workspace -- --all-targets --all-features`; never add an expiry (PICK-051).
- Change a card line or its Spanish: the `EN` and `ES` words in `src/quote_card.rs`, the expected lines in `src/tests/quote_card.rs`; the fixed spec lines (PICK-003, PICK-040, PICK-041, PICK-044) change only with the spec.
- A quote bounce miss: fix it in the card gate (`src/gate.rs`, recipe below), so cards and quotes stay on one rule; add the case to `src/tests/quote_bounce.rs`.

- Restricted list change: `app/catalog/src/restricted_words.rs` only (`catalog` owns it); `tests::card_lines` checks every entry.
- Gate miss found: add the case to `src/tests/prop_card_no_contact.rs` first, then a word to the matching list in `src/gate.rs` (one `const` per shape); never drop a test case; log the change in `docs/decisions.md` like D-1025.
- Add a card line or a kind of place: a field on `CardParts` or a `Place` variant, its line in `build_card`, a case in `prop_card_no_contact.rs`; never add a free-text field that bypasses the gate.
- Add a matching check (such as BIZ-049's skips): a `Skip` variant, a step in `rules::card_for`, a field on `CardJob` or `CardBusiness`, cases in both test files; update CARD-001's line here.
- Change which classes get which job types: `docs/catalog/license_map.yaml` only (`app/licensing/CLAUDE.md`), then `cargo test -p askhava-quotes`.
- Change the card table: a new migration, then `sqlx migrate run` and `cargo sqlx prepare --workspace -- --all-targets --all-features`.

## Tests that guard it

- `src/tests/quote_parse.rs`: TD-PICK-01 (pure window), TD-PICK-02 (fields, refusals), TD-FEE-27's quote-form cases and property (`cargo test -p askhava-quotes tests::quote_parse`).
- `src/tests/quote_bounce.rs`: TD-PICK-03 (`cargo test -p askhava-quotes tests::quote_bounce`).
- `src/tests/quote_card.rs` and `tests/integration/tests/quote_card.rs`: TD-PICK-18, TD-PICK-29, TD-PICK-44 (`cargo test -p askhava-quotes tests::quote_card`, `cargo test -p askhava-integration --test quote_card`).
- `tests/integration/tests/quote_window.rs`: TD-PICK-01, TD-PICK-02 and TD-JOB-03's every-window-ended cases in the database (`cargo test -p askhava-integration --test quote_window`).

- `src/tests/matching_examples.rs`: TD-LIC-02 (M01 to M16 from `docs/catalog/matching_examples.yaml`; 50 qualified businesses get 50 recorded cards, on a fresh database with `DATABASE_URL` set).
- `src/tests/prop_matching_props.rs`: TD-LIC-02 property tests (card exactly when qualified, neighbouring towns, no cap).
- `src/tests/overflow.rs`: TD-LIC-03, and the pure cases of TD-CARD-05, TD-INT-29, TD-INT-25 (`cargo test -p askhava-quotes tests::overflow`).
- `tests/integration/tests/card_late_eligible.rs`: TD-CARD-05, TD-INT-29, TD-INT-25 in the database (`cargo test -p askhava-integration --test card_late_eligible`).
- `src/tests/prop_card_no_contact.rs`: TD-CARD-01 (property and twin adversarial texts), TD-CARD-08, the card cases of TD-CARD-11, TD-INT-13 and TD-INT-33 (`cargo test -p askhava-quotes tests::prop_card_no_contact`).
- `src/tests/card_lines.rs`: TD-CARD-02, the card cases of TD-CARD-03 and TD-CARD-07 (`cargo test -p askhava-quotes tests::card_lines`).
- Run `cargo test -p askhava-quotes tests::matching_examples` and `cargo test -p askhava-quotes tests::prop_matching_props`.
- TD-LIC-26 (LIC-036, a handyman's text quote gets the portal link) is listed for T044 but covers none of its requirements and needs quote intake by text, which does not exist yet; no case is written here (D-1016).
