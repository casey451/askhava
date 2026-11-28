# Architecture section 11

Moved here verbatim from `docs/architecture.md` (D-6022), whose index lists every part. Until T885 makes `docs/architecture/` a shared path, a task adds new notes to the index, at the end of the section this part came from, and the next plan change moves them here.

## 11. Pinned stack versions

**Policy (D-096, D-453):** the newest stable release published on or before October 14, 2026 (14 days before this plan change), unless cargo-audit or npm audit reports a known vulnerability fixed only in a later release, in which case the fixed release. Versions, dates and licenses were read from the crates.io and npm registry APIs and the Rust release channel in this session on October 28, 2026. T009 writes them into the workspace `Cargo.toml` as exact (`=`) versions and commits `Cargo.lock`; T020 writes `package.json` and commits `package-lock.json` (`BUILD_BRIEF.md:114`). The Python versions this section listed before D-453 now apply only to the tooling, stand-ins and twin (pytest 9.1.1 and PyYAML 6.0.1 in `Ci`).

**Toolchain and platform**

| Component | Version | How verified | Note |
|---|---|---|---|
| Rust | 1.99.0 (2026-10-01), `rust-toolchain.toml` with clippy and rustfmt | Rust release channel; installed and used in this session | 1.99.1 (2026-10-15) is past the cutoff; T009 reads its notes and takes it only for a security fix |
| Edition | 2024 | the scratch workspace built with it | |
| PostgreSQL | 16 (16.15 in the build VM) | `psql --version` | Railway's minor version and the CI service image digest: **verify at T009** |
| Node | 22 (22.23.4 in the build VM) | `node --version` | Only for `tsc`, eslint and Playwright; never in the production image |

**Runtime crates** (each MIT, Apache-2.0, BSD-3-Clause, Unlicense or MPL-2.0; `deny.toml` allows those plus the permissive licenses their dependencies use: MIT-0, ISC, Zlib, Unicode-3.0 and CDLA-Permissive-2.0, and skips the unpublished first-party crates)

| Crate | Version (date) | License | Use |
|---|---|---|---|
| axum (features `multipart`) | 0.8.9 (2026-04-14) | MIT | HTTP server and routing |
| axum-extra (`cookie`) | 0.12.6 (2026-04-14) | MIT | Signed `__Host-` cookies |
| tokio (`macros`, `rt-multi-thread`, `signal`, `net`, `time`, `sync`) | 1.53.3 (2026-10-13) | MIT | Async runtime; `sync` is named because the worker's semaphore and watch need it and sqlx only happens to enable it (D-491); 1.54.1 is past the cutoff |
| tower / tower-http (`set-header`, `limit`, `fs`, `trace`, `request-id`, `timeout`, `catch-panic`, `sensitive-headers`, `compression-gzip`, `compression-br`) | 0.5.3 / 0.7.1 | MIT | Middleware: headers, body limits, static files, tracing, request IDs, timeouts, panic capture (D-481), gzip and Brotli for the site stylesheet and mark only (D-5357) |
| askama | 0.16.1 (2026-09-04) | MIT OR Apache-2.0 | Compile-time HTML templates, autoescaped |
| pulldown-cmark | 0.13.6 (2026-10-30) | MIT | Renders the legal documents once at start-up from `include_str!` of each file (`app/site/src/legal_pages.rs`, T221, D-491, D-3878, D-4070); 0.13.6 for RUSTSEC-2026-0414 and RUSTSEC-2026-0433 (D-4031) |
| serde / serde_json | 1.0.229 / 1.0.154 | MIT OR Apache-2.0 | Parsing with `deny_unknown_fields` |
| serde-saphyr | 2.0.0 (2026-10-13) | MIT OR Apache-2.0 | Reading the catalog YAML (`serde_yaml` is unmaintained) |
| sqlx (`default-features = false`; `postgres`, `runtime-tokio`, `tls-rustls-aws-lc-rs`, `macros`, `migrate`, `uuid`, `json`, `sqlx-toml`) | 0.9.0 (2026-05-21) | MIT OR Apache-2.0 | Postgres pool, compile-checked queries, migrations; the aws-lc-rs provider so sqlx and reqwest share one rustls provider, installed as the process default first thing in both binaries (D-481) |
| jiff-sqlx | 0.2.0 (2026-05-26) | Unlicense OR MIT | `timestamptz` columns as jiff types in `query!` through each crate's `sqlx.toml` `[macros.type-overrides]` `'TIMESTAMPTZ' = "jiff_sqlx::Timestamp"` (uppercase key; sqlx has no jiff feature, D-481, D-488) |
| unicode-normalization | 0.1.24 | MIT OR Apache-2.0 | The scrubber's NFKC fold before the plain-code patterns run (AI-020, T023; D-491); `ai`'s same fold before the AI-021 screen (T024, D-814) |
| rustls (`default-features = false`; `aws_lc_rs`, `std`) | 0.23.46 | Apache-2.0 OR ISC OR MIT | `CryptoProvider::install_default()` at the top of both binaries; the one provider sqlx and reqwest share (D-488) |
| askama_web (`axum-0.8`) | 0.16.0 | MIT OR Apache-2.0 | Askama templates as axum responses (`askama_axum` is discontinued, D-481) |
| reqwest (`default-features = false`; `rustls`, `json`, `form`) | 0.13.5 (2026-09-08) | MIT OR Apache-2.0 | Every vendor API client |
| hmac / sha1 / sha2 | 0.13.0 / 0.11.0 / 0.11.1 | MIT OR Apache-2.0 | Webhook signatures (Twilio HMAC-SHA1, Stripe and Svix HMAC-SHA256), derived IDs, blind index |
| aes-gcm | 0.11.1 (2026-08-21) | Apache-2.0 OR MIT | Vault field encryption (PRIV-002) |
| argon2 | 0.6.0 (2026-08-27) | MIT OR Apache-2.0 | Owner password hashing |
| webauthn-rs | 0.5.5 (2026-04-30) | MPL-2.0 | Owner passkeys (D-098); brings in OpenSSL, so the image carries `libssl3` |
| totp-rs | 6.0.0 (2026-08-06) | MIT | Owner TOTP second factor (D-098) |
| rand | 0.10.3 (2026-09-20) | MIT OR Apache-2.0 | Codes and tokens from the OS random source |
| subtle | 2.6.1 (2024-06-24) | BSD-3-Clause | Constant-time comparison of codes and tokens |
| secrecy / zeroize | 0.10.3 / 1.9.1 | Apache-2.0 OR MIT | Secrets kept out of `Debug` and zeroed on drop |
| uuid | 1.28.0 (2026-10-10) | Apache-2.0 OR MIT | Row IDs; 1.29.0 is past the cutoff |
| jiff (`tzdb-bundle-always`) | 0.2.38 (2026-10-06) | Unlicense OR MIT | Arizona time and area-code time zones for quiet hours, with the zone data built in |
| image (`jpeg`, `png`, `webp`, no default features, D-1790) | 0.25.10 (2026-03-10) | MIT OR Apache-2.0 | Photo re-encoding with metadata removed (PRIV-014), under explicit decode limits (PRIV-025). HEIC is not decoded at launch: the upload form accepts `image/jpeg,image/png,image/webp` so phones convert, and a HEIC upload gets the PRIV-025 reply (D-480, D-481) |
| tracing / tracing-subscriber (`json`, `env-filter`) | 0.1.44 / 0.3.23 | MIT | Structured logs with redaction |
| sentry (no default features, D-4287: `backtrace`, `contexts`, `debug-images`, `logs`, `metrics`, `panic`, `release-health`, `rustls`, `reqwest`, `tracing`; no `transport`, so no `native-tls`) | 0.49.3 (2026-09-21) | MIT | Error reports with PII off |
| thiserror / base64 / age | 2.0.21 / 0.23.1 / =0.11.5 (`default-features = false`; never `ssh` or `plugin`, D-4707) | MIT OR Apache-2.0 | Error types; encoding (automod dropped, D-481: `lib.rs` is hand-written and the runner is serial); `age` encrypts the nightly export to an offline X25519 recipient's public key (PRIV-024, T109, D-3883, D-4707; cargo-audit and cargo-deny results in D-4697) |

**Development and checks**

| Component | Version (date) | License | Note |
|---|---|---|---|
| proptest | 1.12.0 (2026-10-10) | MIT OR Apache-2.0 | Property tests |
| trybuild | 1.0.122 (2026-10-07) | MIT OR Apache-2.0 | Compile-fail proofs (SEC-030, SEC-031); one `TestCases` per test binary, its `.stderr` files tied to the exact rustc version (D-488) |
| toml | 1.1.8 | MIT OR Apache-2.0 | The architecture test reads every `Cargo.toml` and `kit/layers.json` (D-481, D-488) |
| cargo-mutants | 27.1.0 (2026-06-02) | MIT | Nightly mutation tests (nightly-main `cargo-mutants` job, report only until `tests/mutation/equivalent.toml` exists, D-1286) |
| cargo-audit | 0.22.2 (2026-06-05) | Apache-2.0 OR MIT | RustSec advisories on every PR |
| cargo-deny | 0.20.2 (2026-07-09) | MIT OR Apache-2.0 | Licenses, bans, sources |
| cargo-llvm-cov | 0.9.1 (2026-09-06) | Apache-2.0 OR MIT | Coverage; branch mode on `nightly-2026-10-15` (chosen at T169, D-694) used only to measure, over `rules.rs` and `service.rs` files, in its own CI job |
| sqlx-cli | 0.9.0 (2026-05-21) | MIT OR Apache-2.0 | `cargo sqlx prepare` for the committed offline query data |
| cargo-chef | 0.1.78 (2026-08-12) | Apache-2.0 OR MIT | The Dockerfile's dependency-caching stage (T019; the trial of D-491 cooked 233 dependency units in 249 s and the 19 crates in 36 s) |
| typescript | 6.0.3 (2026-04-16) | Apache-2.0 | `tsc --strict` for the browser extras (7.0.2 is the native compiler and typescript-eslint 8.71.2 requires `>=4.8.4 <6.1.0`; checked on npm, D-481) |
| @playwright/test | 1.64.1 (2026-10-14) | Apache-2.0 | e2e and accessibility journeys |
| @axe-core/playwright | 4.13.0 (2026-08-11) | MPL-2.0 | axe in Playwright; 4.14.0 is past the cutoff |
| eslint / typescript-eslint | 10.12.0 / 8.71.2 | MIT | TypeScript lint |
| gitleaks | 8.28.0 (pinned with its SHA-256 in `ci.yml` and `backup-runner.yml`, run with `--redact --verbose`, D-582) | MIT | Secret scan |
| stripe/stripe-mock image, OWASP ZAP image, Docker base image | `rust:1.99.0-slim-trixie`, `debian:trixie-slim` and `node:22.23.4-trixie-slim` (the browser-script stage; digests in `Dockerfile`, D-652, D-1281); stripe-mock `v0.206.0` (digest in `ci.yml`'s `E2e` job, D-1282); the ZAP image **verify at T009** | — | Pinned by digest (SEC-021); the runtime is a Debian slim image with `libssl3t64`, CA certificates and PostgreSQL 16's client tools (`postgresql-client-16` from the PostgreSQL apt repository, its signing key pinned by fingerprint in the `pgdg` stage, `/usr/lib/postgresql/16/bin` first on `PATH`; T109, D-4708) |

The architecture test's own helpers (a TOML reader, `regex`, `walkdir` or `cargo_metadata`) are pinned by T175 under the new-library rule and added to `kit/allowed_crates.txt` in the same decision (D-481). `Ci` caches the Cargo registry and `target/` with `Swatinem/rust-cache` pinned by commit SHA, keyed on `Cargo.lock` and the toolchain, with `CARGO_INCREMENTAL=0` and `[profile.dev] debug = "line-tables-only"`; the per-crate clippy runs are dropped in favour of the single `cargo build -p askhava-server` boundary build; the audit tools install prebuilt and pinned; coverage runs in its own parallel job (`Coverage`; `Python` runs the pytest suite, `Checks` the rest, `E2e` the Playwright journeys, and `Ci` is the gate over all of them, D-694, D-1282, D-2782); `Lint and slow tests` runs fmt, clippy, the offline query check and the Rust tests named by the workflow's `SPLIT_TESTS` filters (the trybuild compile-fail drivers and the TD-AI-03 property tests) beside `Checks`, which runs the same `cargo test --workspace --locked` with `--skip` for each filter, so every test runs in exactly one job and `Ci` needs all five (D-3142, `tests/test_ci_workflow.py`); every Rust build cache in `ci.yml`, `twin.yml` and `a11y.yml` and the sqlx-cli cache are restored on every run and saved only on `main`, where nightly-main calls the three workflows, so branch runs no longer evict `main`'s entries from the repository's 10 GB cache, and the plan scripts read YAML with libyaml's `CSafeLoader` when PyYAML has it (D-2782); a warm run must finish in 15 minutes or less, and the Actions-minutes budget is confirmed with Casey before T001 (D-481).

**Resolution and audit run in this session:** a scratch workspace with every crate above at its exact version, on Rust 1.99.0, resolved 401 crates and built (the full 19-crate skeleton trial of D-488 resolved 421 external packages with the same pins, `docs/audit/rust-skeleton-trial-2026-10-29.md`); `cargo clippy --workspace --all-targets -- -D warnings` passed. `cargo audit` (0.22.2, 1,381 advisories) found no vulnerabilities and one unmaintained warning: RUSTSEC-2026-0374, `basic-toml`, used only at compile time by `askama_derive` to read Askama's config. `cargo deny check licenses bans sources advisories` (0.20.2) passed with the license list above, crates.io as the only source, wildcard versions denied except workspace paths, and that one advisory ignored with its reason (T109 adds a second, RUSTSEC-2026-0173, `proc-macro-error2`, unmaintained and reached only through `age` -> `i18n-embed-fl`, a compile-time proc-macro, review by 2027-01-31; `tests/test_age_lock_guard.py` fails if any other crate reaches it, if `crypto-bigint` below 0.7.6 (RUSTSEC-2026-0387, never ignored) or any `rsa` enters the lock, D-4707); duplicate versions are warnings only. T009 writes this `deny.toml` and re-runs both (cargo-audit reads the whole `Cargo.lock`, optional crates included, so `.cargo/audit.toml` ignores RUSTSEC-2026-0459 for `libsqlite3-sys`, which no build includes, D-2144); `Ci` runs `cargo audit` (fails on any vulnerability) and `cargo deny` on every PR. The scratch workspace is not committed. npm packages are audited when T020 writes `package-lock.json`; `Ci` and the pre-hand-in gate run `python3 scripts/npm_audit.py`, which runs `npm audit --json` and fails on any advisory of any severity unless its `EXCEPTIONS` constant excuses that advisory on that package with a decision (one entry: GHSA-fc5q-39xc-vjpf on `picomatch`, which has no patched release and is reached only through the dev-only typescript-eslint chain, limited to the nine packages that reach it today and printing a warning, never a failure, after its review date, 2026-12-17, D-4223), printing each excused advisory every run and a note when one is no longer reported.

No Web Push crate is pinned: the installable business app with push is a later, optional phase (BIZ-041, D-307), and the spec change that adds it picks the crate under the policy above and logs its purpose, license and audit result.

