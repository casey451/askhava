# askhava-ai: the AI adapter's caller, strict schemas, no tools

## Purpose

The layer-3 crate every AI call goes through (`docs/architecture.md` section 3). It takes only `ScrubbedText`, builds the Claude Messages request (versioned instruction, the text inside one escaped tag, the committed schema in `output_config`, no tools), sends it through `adapters`' AI client with the AI-023 timeout and retries, keeps only schema-checked fields that match the open catalog, records every call, and screens any AI-written text before it is used. T024 built it; T025 adds the zero-retention switch and keyword-only mode, T163 the AI-019 writer and output checks.

## What it owns

- `src/api.rs`: the only public surface: `read_intake` (the gated one from `switch.rs`), `ai_mode`, `set_ai_switch`, `weekly_counts`, `open_trades`, `screen_text`, `quote_summary`, the types and setting keys.
- `src/models.rs`: `Instruction`, `INTAKE_READING`, `INSTRUCTIONS`, `AiConfig::from_pairs` (`ASKHAVA_AI_MODEL`, `ASKHAVA_AI_TIMEOUT_SECONDS`, `ASKHAVA_AI_DAILY_CAP`), `Endpoint`, `Transport`, `AdapterTransport`, `Fault`, `Reply`, `AiError`, the `ai.call_stat` queries.
- `src/schemas.rs`: the compiled-in `schemas/intake_reading.json`, `RawReading` (`deny_unknown_fields`), `parse_reading`, `OpenTrades`, `IntakeReading`, `wrap` (AI-025 tags), `screen_text` (AI-021).
- `src/service.rs`: the ungated `read_intake`, `send` (timeout and retries), `intake_request`, `reading_from_reply`, `record` (call row plus once-a-day owner alert, plus the reading), `write_reading` (`ai.reading` row and off-database stream event), `code_of`, `interned`.
- `src/switch.rs`: the AI-008 gate (`Readiness`, `Missing`, `AiMode`, `ai_mode`), `set_ai_switch`, the gated `read_intake`, `WeeklyCounts` and `weekly_counts` (AI-014).
- Migration `0040_ai_adapter.sql`: `ai.call_stat` (append-only for `askhava_app`).
- Migration `0041_ai_provider_terms.sql`: `ai.provider_terms` (the account checklist's AI terms and any zero-retention signing date), `ai.privacy_policy_version` (version, effective date, whether it holds the AI-018 sentence; entered by the owner), `ai.switch_history`, `ai.reading` (no text column); all append-only for `askhava_app`.

## Rules it enforces

- AI-001: `models::Endpoint` holds only `/v1/messages` and `/v1/messages/count_tokens`; `service::send` posts to `Messages` only.
- AI-004: every entry point takes `&ScrubbedText`, which only `privacy` can build.
- AI-005, AI-022: `schemas::parse_reading`: exact fields, confidences 0 to 1, enums trimmed and lower-cased, trade open and job type under it, at most `MAX_MISSING_INFO` `missing_info` items, else discarded; each `missing_info` item passes `screen_text` or is dropped (D-814).
- AI-006: `service::intake_request` has no `tools` key; `reading_from_reply` reads only an `end_turn` stop with one text block; `api::quote_summary` is always `None` (PICK-003, D-373).
- AI-008: `switch::ai_mode` (outside live always `Ai`; in live a signed zero-retention date, or the switch on with nothing `Missing`, checked on every call); `switch::set_ai_switch` refuses on while anything is `Missing` (terms, training-free, flagged retention, spend limit at most 3 times expected, today's policy wording); `switch::read_intake` returns `AiOff::SwitchOff` without calling the adapter (D-841).
- AI-013: `service::write_reading`: `ai.reading` without text, trade and job type as code names only in the stream event, same transaction as the call row (D-842).
- AI-014: `switch::weekly_counts`: numbers only.
- AI-015, AI-024: `models::INTAKE_READING` (versioned); the model ID is `AiConfig::model`; each `ai.call_stat` row holds model, instruction version and request ID.
- AI-021: `schemas::screen_text` (on an NFKC, format-character-free copy it returns: CARD-010 list from `catalog::api::restricted_match`, links with any path or query, `TextKind::cap`; D-814); `read_intake` makes no call for unverified web chat or past `ASKHAVA_AI_DAILY_CAP` (alert `ai.daily_cap`).
- AI-023: `service::send` (whole call within the timeout setting, at most 2 retries: connection, 500, 504, 529 with 1 s then 2 s backoff, 429 with `retry-after` waits that long); `service::alert_for` (`ai.key_rejected` for 401 and 403, `ai.spend_limit` for a spend-limit 400 and a 429 without `retry-after`), once a day per kind.
- AI-025: `schemas::wrap`: `&`, `<`, `>` become entities and the tag name, in any case, has its `_` written `&#95;`.

## Never break

- No request ever carries tools: `tests/no_tools.rs::requests_carry_no_tools_and_tool_use_is_discarded`.
- The schema file, `FIELDS` and `output_config` stay equal: `tests/schemas.rs::rust_types_output_config_and_schema_file_are_equal`, `rust_reading_type_takes_exactly_the_schema_fields`.
- `ai` depends on no crate that sends, charges or releases: `tests/no_tools.rs::output_never_sends_charges_or_releases`.
- No transaction is open during a network call; the call row and its alert commit together after it (R11).
- `ai.call_stat` holds no text, only codes and IDs (PRIV-016).
- `ai.reading` has no text column; trade and job type leave only through the off-database stream (`ai_zero_retention.rs::readings_go_off_database_as_codes_and_the_week_is_counts`).
- The gate reads only the newest `ai.provider_terms` row, its zero-retention date included; a newer incomplete row closes it (`ai_zero_retention.rs::a_newer_incomplete_terms_row_closes_the_gate`).
- Other crates reach only the gated `read_intake`; never export `service::read_intake` (AI-008).
- `IntakeReading::missing_info` is for records and question choice only: never send it to a person or put it into a template (D-814).
- `Transport::post` takes the AI job code; the idempotency key is derived from job and subject (SEC-018, D-814).
- The adapter drops error headers and bodies, so through `AdapterTransport` a 429 counts as having no `retry-after` and any 400 as a spend limit; both fail toward the owner alert (D-810).

## How to change

- Change an instruction: edit its text in `src/models.rs` and bump `version` (AI-015); a new model ID needs the AI-024 evaluation and a decision.
- Add an AI job: a new `Instruction` in `INSTRUCTIONS`, its request builder beside `intake_request`, its job code allowed by the `ai.call_stat.job` check.
- Change a length cap: `TextKind::cap` and D-811. Change the restricted list: `app/catalog/src/restricted_words.rs` and D-813.
- Change the schema: `schemas/intake_reading.json` (T167's), `FIELDS`, `RawReading` and `IntakeReading` together; the stand-in reads the same file.
- Add an AI-008 requirement: a `Readiness` field read in `switch::readiness`, a `Missing` variant in order, and a case in `ai_zero_retention.rs`.
- Run `cargo test -p askhava-ai` (needs `python3` and a database), clippy, `cargo fmt --all --check` and `bash scripts/smoke.sh`.

## Tests that guard it

- `src/tests/schemas.rs` (TD-AI-02, TD-AI-13, TD-AI-15): the schema triangle, reading checks, instruction versions, model setting and paths, `AdapterTransport` sending only `Messages` with a job-keyed ID, the `missing_info` cap and screen, the tag-escaping property for both tags.
- `src/tests/no_tools.rs` (TD-AI-02, TD-AI-06, TD-AI-10): no tools, no send or release, no quote summary, the AI-021 screen (links with paths, disguised text) and the CARD-010 list.
- `src/tests/adapter_calls.rs` (TD-AI-10, TD-AI-13, TD-AI-14): fault modes under a paused clock, AI-off endings, call records, daily cap, web-chat rule, and the stand-in over HTTP.
- `tests/integration/tests/ai_zero_retention.rs` (TD-AI-07, TD-AI-04): zero adapter calls with the switch off in live, each missing record refusing the switch, the policy day rule, zero retention (newest row only, not future-dated), the no-text `ai.reading` schema, the stream payload and the weekly counts.
