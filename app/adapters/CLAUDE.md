# askhava-adapters: one trait per vendor

## Purpose
The layer-1 crate that talks to outside vendors. Each vendor has one trait and one HTTP client whose base URL comes from settings, so the stand-in server (`stand_ins/server.py`), the vendor's sandbox and the live API differ only by configuration (`docs/architecture.md` section 7, D-453, D-703). T016 built Twilio texting, voice and Verify;

## Never break
- No adapter has a separate fake code path; the stand-in is reached over HTTP like the vendor (D-453): `tests/adapter_config.rs::texting_client_talks_to_the_stand_in_over_http`.
- The voice adapter never buys, provisions or releases a number: `tests/adapter_config.rs::voice_adapter_has_no_buy_number_method`.
- Every outside action takes a derived key with no personal data: `every_outside_action_takes_an_idempotency_key`, `idempotency_key_is_derived_from_what_it_identifies`.
- Twilio ignores the `Idempotency-Key` header: for Twilio the derived key is only a sent-log marker (`X-Askhava-Message-Key`, OPS-017). Twilio stores no such key: the stand-in's `MessageKey` list filter is test-only (`core::test_control`); recovery matches by Twilio's `To`, `From` and `DateSent` filters (D-2764). Duplicate protection comes from the caller's outbox checking the sent log for the key before any resend, never from Twilio (SEC-018, D-703).
- `AdapterError::NotSent` (client build or connect failure) is the only error after which a resend cannot duplicate; after `OutcomeUnknown` the caller checks the sent log first. The HTTP client follows no redirects (`redirect::Policy::none()`), so credentials never go to another host.
- TXT-013, SEC-016 (D-2761): with a status URL set, every text create carries `StatusCallback`, or Twilio never posts its status; the URL must equal `ASKHAVA_PUBLIC_BASE_URL` plus the status route's path, which the route rebuilds to check the signature: `tests/integration/tests/sms_status_callback.rs` (TD-TXT-36).
- `AdapterError` carries codes only, never a URL, number or vendor message (PRIV-016).
- A vendor ID from a caller is checked before it goes into a URL path (`base::is_sid`), or pushed as one percent-encoded segment (`VendorHttp::request`).
- `FetchPolicy::with_loopback_for_tests` stays `#[cfg(test)]`; private, link-local and metadata addresses are refused even under it: `tests/ssrf_guard.rs`.
- Stored objects are write-once (hard link, never rename; a same-bytes retry is `Ok`, other bytes `ObjectExists`) and names cannot climb out of the directory, each part a safe name of at most `storage::MAX_PART` (200) bytes so its temporary file name fits 255 bytes (`storage::LocalStorage`, D-726, D-728, D-2158). Each attempt writes only its own `create_new` temporary file `.<name>.<random>.part` (128 bits from `SysRng`), syncs and closes it before the link, compares bytes only with a published object, removes only its own file, and syncs the parent directory after a new link; never go back to a shared, deterministic temporary name or `std::fs::write` on one (B05, D-1841, D-2155): `tests/storage.rs`.
- No module sees a webhook body before `verify_webhook` passed; `VerifiedWebhook` has private fields and a `Debug` without the body: `tests/security/tests/webhook_forgery.rs`, `replay.rs` (D-820).
- Every HTTP client is built with `no_proxy()`: `tests/ssrf_guard.rs::fetch_and_vendor_clients_ignore_proxy_settings` (D-727).
- `adapters` depends only on `core` and the HTTP and crypto crates (`kit/layers.json`). Because it may not depend on any domain crate, its traits take and return `serde_json::Value` (or plain IDs and amounts) where a domain type would fit; the calling crate owns the typed shape.
- Only the `ai` crate names `AiAdapter` or `StandInAi`; every other crate goes through `ai`, which scrubs input and validates output (AI-009). The layer rules cannot express this yet, so it is a review rule here.

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/adapters/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/adapters/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
