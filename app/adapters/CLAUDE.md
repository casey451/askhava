# askhava-adapters: one trait per vendor

## Purpose

The layer-1 crate that talks to outside vendors. Each vendor has one trait and one HTTP client whose base URL comes from settings, so the stand-in server (`stand_ins/server.py`), the vendor's sandbox and the live API differ only by configuration (`docs/architecture.md` section 7, D-453, D-703). T016 built Twilio texting, voice and Verify; T176 built payments, AI (stand-in only), email, letters, license sources, storage and the guarded fetch of untrusted URLs; T018 adds the webhook signature checks. The settings keys of T176's vendors are added by the tasks that wire each client (D-725).

## What it owns

- `src/lib.rs`: the crate root; one `mod` line per file.
- `src/api.rs`: the only public surface.
- `src/config.rs`: `TwilioConfig::from_settings`, `AdapterMode`, `MaskedPool`, `PoolNumber`, `DEFAULT_POOL_SIZE`.
- `src/base.rs`: `IdempotencyKey::derive`, `AdapterError`, `twilio_clients`, the shared Twilio HTTP client, `VendorHttp` (the other vendors' client), `is_safe_name`, and the SEC-041 guard: `fetch_untrusted`, `FetchPolicy`, `FetchError`, `is_public_address`, `TWILIO_MEDIA_HOSTS`, `MAX_REDIRECTS`.
- `src/payments.rs`: `PaymentsAdapter`, `StripePayments`, `Charge`, `PaymentIntent`, `IntentMetadata`.
- `src/ai.rs`: `AiAdapter`, `StandInAi`.
- `src/email.rs`: `EmailAdapter`, `ResendEmail`, `Email` (with `remove_suppression`, BIZ-056).
- `src/letters.rs`: `LettersAdapter`, `LobLetters`, `SentLetter`.
- `src/license_sources.rs`: `LicenseSource`, `LicenseLists`.
- `src/storage.rs`: `StorageAdapter`, `LocalStorage`.
- `src/sms.rs`: `SmsAdapter`, `TwilioSms`, `SentText`.
- `src/voice.rs`: `VoiceAdapter`, `TwilioVoice`, `CallState`.
- `src/verify.rs`: `VerifyAdapter`, `TwilioVerify`, `CodeChannel`.
- No tables.

## Rules it enforces

- SEC-018: `base::IdempotencyKey::derive`; every trait method that acts outside takes `idempotency: &IdempotencyKey` (read-only prefixes `fetch_`, `get_`, `list_`, `read_`, `check_`, `lookup_` excepted, D-596).
- SEC-002, SEC-032: `config::TwilioConfig::from_settings` (`live` mode only in live, `stand_in` and `sandbox` never there; base URLs through `askhava_core::api::check_base_url`).
- SEC-041, D-724: `base::fetch_untrusted` is the only way to fetch a URL a person or vendor supplied: listed https hosts on port 443 only, no userinfo, no environment proxy, lookup under the timeout, every resolved address public and pinned (IPv6 only inside 2000::/3), at most 3 re-checked redirects, size cap, one timeout over the whole fetch; an allowed-host entry that is empty, dot-led, dot-ended or dotless is dropped so it fails closed (D-727).
- AI-009: `ai::StandInAi::new` accepts only an `http` base URL on a loopback IP literal (not `localhost`) and holds no key.
- PICK-022, SEC-030: `payments::PaymentIntent` carries currency, customer and `metadata.pick_id`; `livemode` is `Option<bool>` and `None` is never test mode (D-728). Vendor IDs pass `base::is_vendor_id` before a path or form.
- BIZ-056: `email::EmailAdapter::remove_suppression`; the address must be `local@domain` with both parts non-empty or it is `InvalidId` before any request; the caller writes the audit row.
- CALL-017, D-160: `config::MaskedPool` is exactly `ASKHAVA_MASKED_POOL_SIZE` (default 10) distinct numbers from `ASKHAVA_MASKED_NUMBERS`; `voice::VoiceAdapter::open_call` takes only a `PoolNumber`.

## Never break

- No adapter has a separate fake code path; the stand-in is reached over HTTP like the vendor (D-453): `tests/adapter_config.rs::texting_client_talks_to_the_stand_in_over_http`.
- The voice adapter never buys, provisions or releases a number: `tests/adapter_config.rs::voice_adapter_has_no_buy_number_method`.
- Every outside action takes a derived key with no personal data: `every_outside_action_takes_an_idempotency_key`, `idempotency_key_is_derived_from_what_it_identifies`.
- Twilio ignores the `Idempotency-Key` header: for Twilio the derived key is only a sent-log marker (`X-Askhava-Message-Key`, OPS-017). Duplicate protection comes from the caller's outbox checking the sent log for the key before any resend, never from Twilio (SEC-018, D-703).
- `AdapterError::NotSent` (client build or connect failure) is the only error after which a resend cannot duplicate; after `OutcomeUnknown` the caller checks the sent log first. The HTTP client follows no redirects (`redirect::Policy::none()`), so credentials never go to another host.
- `AdapterError` carries codes only, never a URL, number or vendor message (PRIV-016).
- A vendor ID from a caller is checked before it goes into a URL path (`base::is_sid`), or pushed as one percent-encoded segment (`VendorHttp::request`).
- `FetchPolicy::with_loopback_for_tests` stays `#[cfg(test)]`; private, link-local and metadata addresses are refused even under it: `tests/ssrf_guard.rs`.
- Stored objects are write-once (hard link, never rename; a same-bytes retry is `Ok`, other bytes `ObjectExists`) and names cannot climb out of the directory (`storage::LocalStorage`, D-726, D-728).
- Every HTTP client is built with `no_proxy()`: `tests/ssrf_guard.rs::fetch_and_vendor_clients_ignore_proxy_settings` (D-727).
- `adapters` depends only on `core` and the HTTP and crypto crates (`kit/layers.json`). Because it may not depend on any domain crate, its traits take and return `serde_json::Value` (or plain IDs and amounts) where a domain type would fit; the calling crate owns the typed shape.
- Only the `ai` crate names `AiAdapter` or `StandInAi`; every other crate goes through `ai`, which scrubs input and validates output (AI-009). The layer rules cannot express this yet, so it is a review rule here.

## How to change

- Add a vendor: a new `src/<vendor>.rs` with a trait whose outside-acting methods take `idempotency: &IdempotencyKey`, an HTTP client, its keys added to `KEYS` in `app/core/src/settings.rs` and parsed in a config here, a `mod` line in `src/lib.rs`, re-exports in `src/api.rs`, a row in `docs/architecture.md` section 7 and a decision entry.
- Change the pool size default: CALL-017 and D-160 first, then `DEFAULT_POOL_SIZE` in `src/config.rs` and `masked_pool_is_a_fixed_list_sized_by_settings`.
- Add a Twilio setting: the name in core's `KEYS`, the constant and parse in `src/config.rs`, a case in `src/tests/adapter_config.rs`.
- Run `cargo test -p askhava-adapters`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` and `bash scripts/smoke.sh`.

## Tests that guard it

- `src/tests/adapter_config.rs` (TD-SEC-03, TD-CALL-01, TD-SEC-13): settings, mode and pool cases, and the texting, voice and Verify clients over HTTP against `python3 -m stand_ins.server` (needs `python3` on the path). `src/tests/ssrf_guard.rs` (TD-SEC-20, TD-AI-01, D-724 to D-728): the guarded fetch against a local fake host, the AI stand-in-only rule, storage, license lists, malformed IDs, suppression removal against a recording server, and the payments, letters, email and AI clients over HTTP against the stand-in. T018 adds `tests/security/tests/idempotency_keys.rs`; T175's architecture test checks the trait rule across crates.
