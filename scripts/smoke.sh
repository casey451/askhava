#!/usr/bin/env bash
# Smoke test (D-476): what every build session runs on main before it starts a task, and on its
# branch before hand-in (kit/prompts/TEMPLATE.md, BUILD_BRIEF.md:240). It runs the same checks as
# the Ci job, minus the slow dependency audits (add --full for cargo-audit and cargo-deny), against
# a local PostgreSQL 16 it starts if needed. Exit status 0 means green; anything else means red,
# and if this is main, the only task is "fix main".
#
# Usage: bash scripts/smoke.sh [--full]
set -euo pipefail
cd "$(dirname "$0")/.."

FULL="${1:-}"
step() { printf '\n== %s\n' "$1"; }

local_database() {
  # Ci brings its own Postgres service and sets DATABASE_URL; a session starts its own.
  if [ -n "${CI:-}" ] && [ -n "${DATABASE_URL:-}" ]; then
    return
  fi
  if ! command -v pg_lsclusters >/dev/null 2>&1; then
    echo "PostgreSQL 16 is not installed in this session; integration tests run in Ci only." >&2
    return
  fi
  if ! pg_lsclusters | awk '$1 == "16" && $2 == "main" {print $4}' | grep -q online; then
    sudo -n pg_ctlcluster 16 main start
  fi
  # A fresh local-only password each run: never written to a file, printed or committed.
  local pass
  pass="$(head -c 18 /dev/urandom | base64 | tr -dc 'A-Za-z0-9')"
  sudo -n -u postgres psql -qtc "SELECT 1 FROM pg_roles WHERE rolname = 'askhava_ci'" | grep -q 1 \
    || sudo -n -u postgres psql -qc "CREATE ROLE askhava_ci LOGIN CREATEDB"
  sudo -n -u postgres psql -qc "ALTER ROLE askhava_ci PASSWORD '${pass}'" >/dev/null
  sudo -n -u postgres psql -qtc "SELECT 1 FROM pg_database WHERE datname = 'askhava_ci'" | grep -q 1 \
    || sudo -n -u postgres createdb -O askhava_ci askhava_ci
  export DATABASE_URL="postgres://askhava_ci:${pass}@localhost:5432/askhava_ci"
}

step "Sources unchanged"
sha256sum -c --quiet kit/sources.sha256

step "Plan and wording checks"
for check in trace_spec validate_tasks check_catalog check_legal_docs check_ai_smell check_copy check_code_docs pii_scan check_plan_lock check_rust_rules check_crates; do
  python3 "scripts/${check}.py" >/dev/null || { echo "FAILED: scripts/${check}.py"; python3 "scripts/${check}.py" | tail -20; exit 1; }
  echo "ok  ${check}"
done
python3 scripts/gen_prompts.py --check >/dev/null && echo "ok  gen_prompts --check"

step "Python tests"
python3 -m pytest tests -q -p no:cacheprovider

if [ -f Cargo.toml ]; then
  local_database
  step "Rust format, lint and tests"
  toolchain="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)"
  rustup toolchain install "${toolchain}" --profile minimal -c clippy -c rustfmt >/dev/null 2>&1
  export SQLX_OFFLINE=true  # compile against the committed .sqlx/ data, as Ci does (D-481)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --locked -- -D warnings -F clippy::allow_attributes_without_reason
  # Built alone, only each crate's api is public: the one build that enforces the layer boundary (D-481).
  cargo build -p server --locked
  if command -v sqlx >/dev/null 2>&1 && [ -n "${DATABASE_URL:-}" ]; then
    sqlx migrate run
    cargo sqlx prepare --workspace -- --all-targets --all-features --check
  fi
  cargo test --workspace --locked
  if [ "${FULL}" = "--full" ]; then
    step "Rust dependency audit"
    cargo audit
    cargo deny check licenses bans sources advisories
  fi
fi

if [ -f package-lock.json ]; then
  step "TypeScript"
  npm ci --ignore-scripts >/dev/null
  npx tsc --noEmit
fi

printf '\nsmoke: green\n'
