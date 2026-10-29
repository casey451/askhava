# Rust skeleton trial, October 29, 2026 (D-488)

**What ran:** a throwaway build, outside this repository, of the full 19-crate workspace that `docs/architecture.md` section 2 describes: `askhava-<module>` crates with hand-written `lib.rs`, `api.rs` with `pub mod rules`, `status.rs` and `src/tests/mod.rs`; path dependencies exactly as `kit/layers.json` allows, each edge exercised by a probe call; `tests/integration`, `tests/security` (trybuild), `tests/recovery` and `tests/fixtures`; the capability types of section 4 rule 4; one `timestamptz` migration; sqlx offline data; askama_web; both binaries; cargo-audit and cargo-deny with the section 11 policy. Rust 1.99.0, PostgreSQL 16.15, 2 vCPU, 7 GB. Every section 11 pin at its exact version, and the full runtime set too. **Locked** (`kit/plan.lock`); T009, T012, T168, T169, T175, T022, T189, T192 and T219 read it.

**Result:** the plan does not compile as written. With the ten deviations below, applied to the plan by D-488, the whole workspace builds, lints and tests green with zero warnings, and audit and deny are clean.

## 1. What held

- Every pin resolves; the full set resolves 421 external packages (444 lockfile entries), not the 401 D-481 counted.
- The `[workspace.lints]` set works with `clippy.toml` thresholds 50 and 4: a 54-line function, 5-deep nesting and an `unsafe` block each fail with the expected message.
- The layer boundary holds: a call from `picks` into `payments`' private `service` fails `E0603`; a call to `payments::api::rules` compiles; an upward `jobs -> picks` dependency fails as a cyclic package dependency.
- trybuild in `tests/security` with no features: a struct literal on either proof or mint fails `E0451`, `Default` fails `E0599`, `Clone` fails `E0308`.
- The architecture test (`toml` crate) passes on the workspace and catches a planted `site -> payments` dependency, a `jobs -> picks` dependency hidden behind a `package =` rename, and `release_mint` named in `jobs`.
- `sqlx::migrate!("../../migrations")` and `server migrate` work; `#[sqlx::test(migrator = ...)]` compiles offline; `cargo sqlx prepare --workspace -- --all-targets --all-features` writes `.sqlx/` and `--check` catches a changed query; `SQLX_OFFLINE=true cargo build --workspace --all-targets --locked` needs no database.
- One `rustls 0.23.46` with aws-lc-rs is shared by sqlx and reqwest; `ring` is in the lockfile as an optional dependency but never compiled.
- askama_web `WebTemplate` returned from a handler gives `text/html; charset=utf-8` and escapes `<script>`; a per-crate `askama.toml` with `dirs = ["templates", "../site/templates"]` lets `portals` extend `site`'s layout.
- The server answers `/healthz` and exits 0 on SIGINT; the worker runs and exits 0.
- cargo-audit 0.22.2: 0 vulnerabilities, the one allowed warning (RUSTSEC-2026-0374 `basic-toml`, through askama_derive) as section 11 predicted. cargo-deny 0.20.2: advisories, bans, licenses and sources ok. Both install from their GitHub release tarballs in seconds.
- `scripts/check_crates.py` found the two crates the trial added that the allow-list lacked (`rustls`, `toml`): true positives.

## 2. Deviations the plan takes (D-488)

| # | Finding | What the plan now says |
|---|---|---|
| D1 | `tests/recovery` cannot be `askhava-recovery`: cargo refuses two packages with one name | The recovery test crate is `askhava-recovery-tests`; the `askhava-<module>` rule (TD-SEC-10) names that one exception; `kit/allowed_crates.txt` lists it |
| D2 | jiff-sqlx timestamps fail in `query!` ("optional sqlx feature `time` required for type TIMESTAMPTZ") | sqlx gains the `sqlx-toml` feature and each crate with queries carries a `sqlx.toml` with `[macros.type-overrides]` `'TIMESTAMPTZ' = "jiff_sqlx::Timestamp"` (the key is case-sensitive; lowercase does nothing); `sqlx.toml` is a shared path a module's first database task may add |
| D3 | `payments` cannot reach `privacy`: `kit/layers.json` let it call only `adapters`, `catalog` and `ops`, so section 4 rule 4 could not compile | `payments` calls `privacy` (layer 2 to layer 3, downward); `kit/layers.json` and section 3 updated |
| D4 | The capability does not stop other crates from minting: `release_mint().mint(42)` compiles from any crate that depends on `privacy`, never writing the type name. Only a struct literal, `Default` and `Clone` are compile errors | SEC-030 and rule 4 now say what is true: the compile-fail cases prove the proof cannot be forged by construction; the run-time once-guard plus the architecture test's source scan for `ReleaseMint`, `release_mint`, `SessionMint` and `session_mint` outside `privacy`, `payments`, `server` (and `portals` for the session mint) stop a second minter; `register_handlers()` runs once per process, so tests that build the composition share one fixture. The mint wiring now has owners: T022 (the types), T189 (payments' minter and the server hookup) |
| D5 | `wildcard_enum_match_arm` alone misses a `_` that covers one variant, and `matches!` is never caught | Every `status.rs` carries `#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants)]`; `scripts/check_rust_rules.py` gains the missed shapes (T219) |
| D6 | `cargo build -p server` matches no package under D-484 naming, so `smoke.sh` would go red on the first session with a `Cargo.toml` | `-p askhava-server` in `scripts/smoke.sh`, `ci.yml`, TD-SEC-10 and the task goals |
| D7 | `install_default()` needs an explicit `rustls` dependency; the architecture test needs `toml` | `rustls =0.23.46` (`default-features = false`, `aws_lc_rs`, `std`) and `toml =1.1.8` (dev, architecture test) join section 11 and the allow-list |
| D8 | `scripts/check_rust_rules.py` misses an alias import (`Status as S`), a non-glob variant import, `let Status::X = s else`, a rustfmt-split `matches!`, reversed `Status::X == s`, `assert_eq!(s, Status::X)`, an `other =>` catch-all, status enums not named `*Status`, every path containing `/ui/` (an `unsafe` block there goes unreported), `/* TODO */`, `escape = "none"` in a `#[template]` attribute, `ext = "txt"` templates and `{% filter safe %}`; and it wrongly flags a vendor `*Status` enum's file as a status module and `unsafe {` inside a comment | New harness task T219 fixes each shape with a planted-case test, before the first `status.rs` lands |
| D9 | The `sqlx migrate build-script` output fails `missing_docs` and needs `--source ../../migrations` for `app/core` | T012 generates it with `--source` and adds the `//!` line |
| D10 | Clippy 1.99 surprises: `manual_is_multiple_of`; `collapsible_if` and `match_like_matches_macro` fire before the nesting or wildcard lints; a `pub fn` in a private module nothing re-exports is dead code | Recorded for every Rust task through `docs/brains/rust-production.md`'s checklist; no plan change |

Also found: the smoke test's comment that `cargo build -p server` alone "enforces the layer boundary" was stale (with no features, every build enforces the same visibility); `tests/fixtures` shares its directory with the Python fixture `tests/fixtures/kit_state_real_shape.json`.

## 3. Timings (2 vCPU, Rust 1.99.0, `SQLX_OFFLINE=true`, `debug = "line-tables-only"`)

| Step | Wall time | Note |
|---|---|---|
| Cold `cargo build` (19 crates, base set) | 106 s | `target/` 963 MB |
| Cold `cargo build` (full section 11 set, 345 units) | 127 s | |
| Cold clippy, `--workspace --all-targets -D warnings` | 109 s | clippy does not reuse build artifacts: 67 to 203 s on a build-warm target, under 1.1 s warm after a one-line change |
| Cold `cargo test --workspace` | 515 s | trybuild alone 406 s: it rebuilds `privacy`'s dependency tree under `target/tests/trybuild` (1.7 GB) |
| `cargo test` after a build | 420 s | two trybuild `TestCases` in parallel tests built the scratch project twice (291 s): one `TestCases` per test binary |
| One-line change in a layer-1 crate: build, then test | 2.8 s, 8.6 s | |
| Release `askhava-server` | 200 s (base), 301 s (full) | 9.6 MB; debug 43 MB |
| `cargo sqlx prepare` / `--check` | 3.7 s / 2.8 s | |
| `cargo install sqlx-cli 0.9.0` | 2 min 24 s | T169 caches it |

`target/` peaked at 3.6 GB. A cold full `Ci` pass is 12 to 15 minutes on 2 vCPU; the 15-minute ceiling (`BUILD_BRIEF.md` section 9) holds only with a warm cache that includes `target/tests/trybuild`, which T169 must verify rust-cache keeps.

## 4. What each task's prompt now carries

- **T009:** members `["app/*", "tests/integration"]` (no `tests/*` glob: that directory holds Python, `contract/` and `fixtures/`); reqwest `default-features = false`; sqlx `default-features = false` with `postgres`, `runtime-tokio`, `tls-rustls-aws-lc-rs`, `macros`, `migrate`, `uuid`, `json`, `sqlx-toml`; a logged `excessive-nesting-threshold` (unset or 0 turns the lint off); `-p askhava-server`; decision entries for `rustls` and `toml`; both wildcard denies in every `status.rs`; the mint wiring belongs to T022 and T189.
- **T175:** the architecture test follows `package =` renames, requires `workspace = true` on every dependency, scans for the four mint names outside their crates, special-cases `server` and `core` (absent from `layers.json` `calls`), maps `tests/recovery` to `askhava-recovery-tests`, and plants its violations in a temporary tree under `CARGO_TARGET_TMPDIR`; the trial's version was about 240 lines, so the migration-order and cross-schema checks must stay compact.
- **T012:** `MIGRATOR = sqlx::migrate!("../../migrations")`; `build.rs` from `sqlx migrate build-script --source ../../migrations` plus a `//!` line; `app/core/sqlx.toml` with the uppercase `TIMESTAMPTZ` override; the CI role needs `CREATEDB`; the local `DATABASE_URL` names a user (without one: `Peer authentication failed for user "anonymous"`).
- **T168:** `askhava-recovery-tests`; one `TestCases` per test binary; each driver (T023, T192, T179) globs only its own cases (`ui/forge_release_proof_*.rs`) since they share `ui/`; minimal dev-dependencies in the security crate; a trybuild *pass* case documenting D4; `.stderr` files are tied to the exact rustc version (`TRYBUILD=overwrite` after a toolchain bump).
- **T169:** `cargo build -p askhava-server --locked`; `sqlx migrate run`, then `cargo sqlx prepare --workspace --check -- --all-targets --all-features`; cargo-audit and cargo-deny from the musl release tarballs with pinned SHA-256s; clippy first in the same job; about 12 to 15 minutes cold, seconds warm; 3.5 GB of disk for `target/`; confirm the cache keeps `target/tests/trybuild`.
- **T022, T189:** the mint wiring (D4): `privacy::api` defines the types and `release_mint()`; `payments` turns a `VerifiedStripeEvent` (made only by `adapters` after the HMAC check) for `charge.succeeded` into a proof; `server/src/lib.rs` creates the mints once and hands them over; payments' unit tests get a mint no other way.

## 5. Not verified by the trial

Compiled but never exercised: tower-http, axum-extra, serde-saphyr, argon2, webauthn-rs (which pulls in openssl 0.10.81), totp-rs, image, sentry, tracing-subscriber, proptest. Not built: `Settings::from_iter`, the worker claim loop, the database roles and grants, `rules_bridge`, the cross-schema-FK and migration-order checks, the non-literal `sqlx::query` check, the Docker image, timings with `CARGO_INCREMENTAL=0`, whether `install_default()` is needed at all, an actual TLS handshake, and whether rust-cache keeps `target/tests/trybuild`. Each belongs to the task that owns it; none changes the plan.

Environment notes for later sessions: Rust 1.99.0 installs with rustup here; cargo-audit and cargo-deny install from GitHub release tarballs through the proxy while the GitHub API is blocked; the PostgreSQL peer-auth `root` role works for sqlx.
