# askhava-adapters: one trait per vendor

## Purpose

The layer-1 crate that talks to outside vendors. Each vendor has one trait and one HTTP client whose base URL comes from settings, so the stand-in server (`stand_ins/server.py`), the vendor's sandbox and the live API differ only by configuration (`docs/architecture.md` section 7, D-453, D-703). T016 built Twilio texting, voice and Verify; later tasks add payments, email, letters, AI, storage, license sources and the webhook signature checks (T018).

## What it owns

- `src/lib.rs`: the crate root; one `mod` line per file.
- `src/api.rs`: the only public surface.
- `src/config.rs`: `TwilioConfig::from_settings`, `AdapterMode`, `MaskedPool`, `PoolNumber`, `DEFAULT_POOL_SIZE`.
- `src/base.rs`: `IdempotencyKey::derive`, `AdapterError`, `twilio_clients`, the shared Twilio HTTP client.
- `src/sms.rs`: `SmsAdapter`, `TwilioSms`, `SentText`.
- `src/voice.rs`: `VoiceAdapter`, `TwilioVoice`, `CallState`.
- `src/verify.rs`: `VerifyAdapter`, `TwilioVerify`, `CodeChannel`.
- No tables.

## Rules it enforces

- SEC-018: `base::IdempotencyKey::derive`; every trait method that acts outside takes `idempotency: &IdempotencyKey` (read-only prefixes `fetch_`, `get_`, `list_`, `read_`, `check_`, `lookup_` excepted, D-596).
- SEC-002, SEC-032: `config::TwilioConfig::from_settings` (`live` mode only in live, `stand_in` never there; base URLs through `askhava_core::api::check_base_url`).
- CALL-017, D-160: `config::MaskedPool` is exactly `ASKHAVA_MASKED_POOL_SIZE` (default 10) distinct numbers from `ASKHAVA_MASKED_NUMBERS`; `voice::VoiceAdapter::open_call` takes only a `PoolNumber`.

## Never break

- No adapter has a separate fake code path; the stand-in is reached over HTTP like the vendor (D-453): `tests/adapter_config.rs::texting_client_talks_to_the_stand_in_over_http`.
- The voice adapter never buys, provisions or releases a number: `tests/adapter_config.rs::voice_adapter_has_no_buy_number_method`.
- Every outside action takes a derived key with no personal data: `every_outside_action_takes_an_idempotency_key`, `idempotency_key_is_derived_from_what_it_identifies`.
- `AdapterError` carries codes only, never a URL, number or vendor message (PRIV-016).
- A vendor ID from a caller is checked before it goes into a URL path (`base::is_sid`).
- `adapters` depends only on `core` and the HTTP and crypto crates (`kit/layers.json`).

## How to change

- Add a vendor: a new `src/<vendor>.rs` with a trait whose outside-acting methods take `idempotency: &IdempotencyKey`, an HTTP client, its keys added to `KEYS` in `app/core/src/settings.rs` and parsed in a config here, a `mod` line in `src/lib.rs`, re-exports in `src/api.rs`, a row in `docs/architecture.md` section 7 and a decision entry.
- Change the pool size default: CALL-017 and D-160 first, then `DEFAULT_POOL_SIZE` in `src/config.rs` and `masked_pool_is_a_fixed_list_sized_by_settings`.
- Add a Twilio setting: the name in core's `KEYS`, the constant and parse in `src/config.rs`, a case in `src/tests/adapter_config.rs`.
- Run `cargo test -p askhava-adapters`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` and `bash scripts/smoke.sh`.

## Tests that guard it

- `src/tests/adapter_config.rs` (TD-SEC-03, TD-CALL-01, TD-SEC-13): settings, mode and pool cases, and the texting, voice and Verify clients over HTTP against `python3 -m stand_ins.server` (needs `python3` on the path). T018 adds `tests/security/tests/idempotency_keys.rs`; T175's architecture test checks the trait rule across crates.
