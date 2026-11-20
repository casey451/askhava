#!/usr/bin/env bash
# Pre-hand-in gate (D-1300): the `Ci` steps scripts/smoke.sh does not run, run on the task branch itself
# before it is handed in, so a failure is fixed inside the worker's session instead of costing a new
# attempt. Of 85 tasks DONE on November 1, 2026, 18 needed a second attempt, and 10 of those failed a `Ci`
# step the worker never ran on its branch: the change rules against main (T047, T167, T184), the PII scan
# (T176), the npm audit (T227), a Python contract test (T055), the coverage floor (T020, T022, T033, T036).
#
#   quick (always, under 30 seconds, no network): the sources fingerprint and file count, the secret scan
#     (gitleaks, when on the path), every plan, wording and source-rule check, `gen_prompts.py --check`,
#     the skip guard, then `check_plan_lock.py` and `check_pr_change.py --base <main> --branch <branch>`,
#     as the `Ci` "Checks" job runs them;
#   extras (unless --quick): what `Ci` adds when the diff against main touches it, by `Ci`'s own path
#     rules: eslint and `scripts/npm_audit.py` (D-4223) for the npm tree; cargo-audit and cargo-deny for
#     Rust when both are on the path (`Ci` installs them; elsewhere it says it skipped); and, when the diff
#     touches a crate with a floor in tests/coverage_floor.json, `Ci`'s branch coverage of that crate and
#     every workspace crate that depends on it (their tests carry its coverage: askhava-payments alone
#     measured 61%, with askhava-integration 98.8%), judged by scripts/coverage_floor.py, `Ci`'s own
#     comparison, on those floors only (D-1440). It needs cargo-llvm-cov, the pinned nightly and
#     DATABASE_URL, else it says it skipped; past PREHANDIN_COVERAGE_TIMEOUT (360 s, so the whole gate
#     stays inside a 600 s Bash call) it says so and lets `Ci` judge. Measured 271 s warm, 324 s cold for payments, so it is not in --quick (the runner's 240 s).
#   --ally (its own call, D-3837): the required `Ally` check (.github/workflows/a11y.yml) on the branch, when
#     scripts/prehandin_ally.py `select` names a changed path that can change what it renders (page templates
#     and scripts, page-crate and route sources, the router composition, migrations, the docs, data and
#     Cargo files pages compile in, the suite and its server); else it prints "not needed". It needs cargo, node_modules with @playwright/test (npm ci when
#     missing), a Chromium that launches (one `npx playwright install chromium` try), DATABASE_URL or the
#     local PostgreSQL 16 smoke.sh uses (a fresh askhava_ally database each run), and the suite's ports free;
#     a missing one says it skipped, never red, and so does a run in which no test ran (the build, the
#     migrations, the stand-ins or a server did not start: its errors and log are shown, `Ci` judges it); a
#     skip ends `prehandin --ally: skipped (Ci runs Ally)`, never "green". It builds the test-control binaries and runs the suite as
#     a11y.yml does, printing only failed tests and counts; past PREHANDIN_ALLY_TIMEOUT (540 s for the whole
#     call, set-up included, inside one 600 s Bash call) it says so and lets `Ci` judge. Measured in a 2-core session (D-3837): 210 s for the
#     test-control build from cold, 149 s for the suite (334 tests); Ci takes 85 to 115 s and 90 to 120 s.
#     Too long beside coverage in one call, so it is its own call after this one (kit/prompts/TEMPLATE.md),
#     and never in --quick (the runner's 240 s).
# Not here: pytest, cargo and tsc (scripts/smoke.sh runs them; the worker runs both scripts), `Ci`'s
# workspace-total coverage (4.5 to 6 minutes in a session, D-1300), and Twin.
#
# The worker runs it after `bash scripts/smoke.sh` as the last step before hand-in (kit/prompts/TEMPLATE.md).
# The runner runs main's copy with --quick on the handed-in head at `python3 -m runner --after` and does not
# hand in a red head (runner/github_env.py `prehandin`, runner/__main__.py `after`, D-1300).
#
# Usage: bash scripts/prehandin.sh [--quick | --ally] [--base REF] [--branch NAME]
#   --base    the main commit to compare with (default origin/main, fetched first when the fetch works)
#   --branch  the branch name check_pr_change.py checks paths for (default the current branch)
# PREHANDIN_ROOT names the checkout to check when this file is run from elsewhere (the runner runs
# main's copy against the handed-in tree). Exit 0 means green; on red the failing check's last lines are
# printed and the exit status is 1; a git problem (no such base, no merge base) exits 3, never 1.
set -euo pipefail
cd "${PREHANDIN_ROOT:-$(dirname "$0")/..}"

QUICK="" ALLY="" BASE="" BRANCH=""
while [ $# -gt 0 ]; do
  case "$1" in
    --quick) QUICK=1 ;;
    --ally) ALLY=1 ;;
    --base) BASE="$2"; shift ;;
    --branch) BRANCH="$2"; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done
if [ -z "${BASE}" ]; then
  git fetch --quiet --no-tags origin main:refs/remotes/origin/main 2>/dev/null \
    || echo "note: origin/main not fetched; comparing with the local origin/main"
  BASE="origin/main"
fi
BRANCH="${BRANCH:-$(git symbolic-ref --short -q HEAD || true)}"
# A git problem is not a verdict on the work: exit 3, which the runner logs and lets through (D-1300).
git rev-parse --verify --quiet "${BASE}^{commit}" >/dev/null || { echo "CANNOT CHECK: base ${BASE} is not a commit here"; exit 3; }
MERGE_BASE="$(git merge-base "${BASE}" HEAD)" || { echo "CANNOT CHECK: no merge base between ${BASE} and HEAD"; exit 3; }
CHANGED="$(git diff --no-renames --name-only "${MERGE_BASE}" HEAD)"

step() { printf '\n== %s\n' "$1"; }
fail() { echo "FAILED: $1"; exit 1; }

# The same path rules as ci.yml's "What this change touches" step (tests/test_prehandin.py keeps them equal).
RUST_PATHS='^(app/|tests/(integration|security|recovery|fixtures|mutation)/|scripts/run_named_mutants\.py|migrations/|docs/legal/|docs/catalog/|docs/architecture\.md|docs/copy/mail-app-steps\.md|schemas/|scripts/pii_scan\.py|scripts/bootstrap_roles\.sql|stand_ins/|kit/layers\.json|kit/allowed_crates\.txt|Cargo\.(toml|lock)|rust-toolchain\.toml|clippy\.toml|deny\.toml|\.sqlx/|\.github/workflows/ci\.yml)'
NPM_PATHS='^(package(-lock)?\.json|tsconfig|eslint\.config|tests/(e2e|a11y)/|app/[^/]+/static/.*\.ts$)'
touches() { printf '%s\n' "${CHANGED}" | grep -Eq "$1"; }

run_check() {
  # One check: quiet when green, its last 30 lines when red.
  local name="$1"; shift
  local out
  if out="$("$@" 2>&1)"; then
    echo "ok  ${name}"
  else
    printf '%s\n' "${out}" | tail -30
    fail "${name}"
  fi
}

secret_scan() {
  if ! command -v gitleaks >/dev/null 2>&1; then
    echo "skipped: no gitleaks on PATH (Ci runs it)"
    return
  fi
  local scan
  scan="$(mktemp -d)"
  git ls-files -z --cached --others --exclude-standard \
    | tar --null --files-from=- --ignore-failed-read -cf - 2>/dev/null | tar -xf - -C "${scan}"
  (cd "${scan}" && gitleaks dir . --no-banner --redact --verbose --exit-code 1) || { rm -rf "${scan}"; fail "secret scan (gitleaks)"; }
  rm -rf "${scan}"
  echo "ok  gitleaks"
}

skip_guard() {
  # ci.yml: a check that exists on main cannot be skipped by deleting its inputs (D-479).
  local f
  for f in Cargo.toml Cargo.lock package-lock.json rust-toolchain.toml deny.toml tests/coverage_floor.json; do
    if git cat-file -e "${BASE}:${f}" 2>/dev/null && [ ! -f "${f}" ]; then
      fail "${f} exists on main but not here: a required check would be skipped"
    fi
  done
  echo "ok  skip guard"
}

quick_part() {
  step "Sources unchanged"
  sha256sum -c --quiet kit/sources.sha256 || fail "sources/ differs from kit/sources.sha256"
  [ "$(find sources -type f | wc -l)" -eq "$(wc -l < kit/sources.sha256)" ] || fail "sources/ file count differs from kit/sources.sha256"
  echo "ok  sources"
  step "Secret scan (gitleaks)"
  secret_scan
  step "Plan, wording and source rules (as Ci runs them)"
  local check
  for check in pii_scan trace_spec validate_tasks check_catalog check_legal_docs check_ai_smell check_copy check_code_docs check_rust_rules check_crates check_include_paths check_conflict_markers check_plan_lock; do
    if [ ! -f "scripts/${check}.py" ]; then
      # A check main added after this branch was made: Ci on the branch would not run it either. Merging
      # origin/main into the branch (a plain merge, never a reset or rebase) brings it (D-1300).
      echo "skipped: scripts/${check}.py not on this head"
      continue
    fi
    run_check "${check}" python3 "scripts/${check}.py"
  done
  run_check "gen_prompts --check" python3 scripts/gen_prompts.py --check
  skip_guard
  step "Change rules against ${BASE} (check_pr_change.py, branch '${BRANCH:--}')"
  run_check "check_pr_change" python3 scripts/check_pr_change.py --base "${BASE}" --branch "${BRANCH}"
}

extras() {
  if [ -f package-lock.json ] && touches "${NPM_PATHS}"; then
    step "npm tree changed: eslint and npm audit (as Ci)"
    [ -d node_modules ] || run_check "npm ci" npm ci --ignore-scripts
    if [ -f eslint.config.js ]; then run_check "eslint" npx eslint .; fi
    run_check "npm audit (scripts/npm_audit.py)" python3 scripts/npm_audit.py
  fi
  if [ -f Cargo.lock ] && touches "${RUST_PATHS}"; then
    step "Rust changed: dependency audit (as Ci)"
    if command -v cargo-audit >/dev/null 2>&1 && command -v cargo-deny >/dev/null 2>&1; then
      run_check "cargo audit" cargo audit
      run_check "cargo deny" cargo deny check licenses bans sources advisories
    else
      echo "skipped: cargo-audit or cargo-deny not on PATH (Ci runs them)"
    fi
  fi
  coverage
  ally_hint
  echo "not run here: pytest, cargo and tsc (scripts/smoke.sh), Ci's workspace total coverage, Twin"
}

# Ally on the branch (D-3837): a11y.yml's suite, run by `--ally` when the change can affect it.
ALLY_TIMEOUT="${PREHANDIN_ALLY_TIMEOUT:-540}"
ALLY_REPORT="test-results/a11y-report/results.json"
ally_paths() {
  # The changed paths that can change what Ally renders; empty when none can or the selector is missing.
  [ -f scripts/prehandin_ally.py ] && [ -f tests/a11y/playwright.config.ts ] || return 0
  printf '%s\n' "${CHANGED}" | python3 scripts/prehandin_ally.py select --base "${MERGE_BASE}" 2>/dev/null || true
}

ally_hint() {
  local picked
  picked="$(ally_paths)"
  if [ -n "${picked}" ]; then
    echo "Ally: this change touches $(printf '%s\n' "${picked}" | head -1); run bash scripts/prehandin.sh --ally"
  else
    echo "Ally: not needed (no changed path can change what it renders)"
  fi
}

pg16_path() {
  # D-4708: initdb and pg_ctl live only in PostgreSQL 16's bin directory and /usr/bin/pg_dump is Debian's
  # pg_wrapper, so the tests that run them by name need it first on PATH (Ci's preflight step does the same).
  local pg16=/usr/lib/postgresql/16/bin
  if [ -d "${pg16}" ]; then
    export PATH="${pg16}:${PATH}"
  else
    echo "${pg16} is missing: PostgreSQL 16's initdb, pg_ctl and pg_dump are not on the path (D-4708)" >&2
  fi
}

ally_database() {
  # DATABASE_URL as given (Ci), else a fresh askhava_ally database on the local PostgreSQL 16 that
  # scripts/smoke.sh starts, with smoke.sh's roles and a fresh password never printed. Fails, never exits.
  [ -n "${DATABASE_URL:-}" ] && return 0
  command -v pg_lsclusters >/dev/null 2>&1 && sudo -n true 2>/dev/null || return 1
  pg_lsclusters | awk '$1 == "16" && $2 == "main" {print $4}' | grep -q online || sudo -n pg_ctlcluster 16 main start || return 1
  local pass role sql=(sudo -n -u postgres psql -q)
  pass="$(head -c 18 /dev/urandom | base64 | tr -dc 'A-Za-z0-9')"
  "${sql[@]}" -tc "SELECT 1 FROM pg_roles WHERE rolname = 'askhava_ci'" | grep -q 1 \
    || "${sql[@]}" -c "CREATE ROLE askhava_ci LOGIN CREATEDB" >/dev/null || return 1
  "${sql[@]}" -c "ALTER ROLE askhava_ci PASSWORD '${pass}'" >/dev/null || return 1
  for role in askhava_app askhava_migrator; do
    "${sql[@]}" -tc "SELECT 1 FROM pg_roles WHERE rolname = '${role}'" | grep -q 1 \
      || "${sql[@]}" -c "CREATE ROLE ${role} NOLOGIN" >/dev/null || return 1
    "${sql[@]}" -c "GRANT ${role} TO askhava_ci" >/dev/null 2>&1 || return 1
  done
  "${sql[@]}" -c "DROP DATABASE IF EXISTS askhava_ally WITH (FORCE)" >/dev/null 2>&1 || return 1
  sudo -n -u postgres createdb -O askhava_ci askhava_ally || return 1
  export DATABASE_URL="postgres://askhava_ci:${pass}@localhost:5432/askhava_ally"
}

chromium_launches() {
  timeout 60 node -e "
    const { chromium } = require('@playwright/test');
    (async () => { const b = await chromium.launch(); await (await b.newPage()).setContent('<p>x</p>'); await b.close(); })()
      .catch(() => process.exit(1));" >/dev/null 2>&1
}

ally_ready() {
  # Every tool the suite's globalSetup (tests/e2e/webserver.ts) uses; a missing one is a skip, never red.
  local tool port web="${E2E_PORT:-3000}"
  for tool in cargo node npx python3; do
    command -v "${tool}" >/dev/null 2>&1 || { echo "skipped: no ${tool} on PATH (Ci runs Ally)"; return 1; }
  done
  if [ ! -d node_modules/@playwright/test ] && ! npm ci --ignore-scripts >/dev/null 2>&1; then
    echo "skipped: npm ci failed, no @playwright/test (Ci runs Ally)"; return 1
  fi
  if ! chromium_launches; then
    timeout 180 npx playwright install chromium >/dev/null 2>&1 || true
    chromium_launches || { echo "skipped: Chromium is not installed or does not launch (Ci runs Ally)"; return 1; }
  fi
  ally_database || { echo "skipped: no DATABASE_URL and no local PostgreSQL 16 to make one (Ci runs Ally)"; return 1; }
  for port in 4010 "${web}" "$((web + 19))"; do
    if (exec 3<>"/dev/tcp/127.0.0.1/${port}") 2>/dev/null; then
      echo "skipped: port ${port} is in use; the suite needs 4010, ${web} and $((web + 19)) free (Ci runs Ally)"; return 1
    fi
  done
}

ALLY_VERDICT="green"
ally_skip() { echo "skipped: $1 (Ci runs Ally)"; ALLY_VERDICT="skipped (Ci runs Ally)"; }

ally_part() {
  step "Ally: pages changed? (scripts/prehandin_ally.py)"
  local picked log code=0 ts built=() left summary
  picked="$(ally_paths)"
  if [ -z "${picked}" ]; then
    echo "not needed: no changed path can change what Ally renders"
    ALLY_VERDICT="green (not needed)"
    return 0
  fi
  printf '%s\n' "${picked}" | head -3 | sed 's/^/touches /'
  step "Ally (as a11y.yml runs it: test-control build, then the suite)"
  ally_ready || { ALLY_VERDICT="skipped (Ci runs Ally)"; return 0; }
  # The whole call stays inside PREHANDIN_ALLY_TIMEOUT, the kill grace and the clean-up included.
  left=$((ALLY_TIMEOUT - SECONDS - 30))
  if [ "${left}" -lt 120 ]; then ally_skip "${left} s left of ${ALLY_TIMEOUT} s after the set-up"; return 0; fi
  for ts in app/*/static/*.ts; do
    if [ -e "${ts}" ] && [ ! -e "${ts%.ts}.js" ]; then built+=("${ts%.ts}.js"); fi
  done
  log="$(mktemp)"
  rm -f "${ALLY_REPORT}"
  # timeout signals the whole process group, so the servers the suite started stop with it.
  CI=true SQLX_OFFLINE=true timeout --kill-after=20 "${left}" bash -c \
    'npm run build && npx playwright test --config tests/a11y/playwright.config.ts' >"${log}" 2>&1 || code=$?
  [ "${#built[@]}" -eq 0 ] || rm -f "${built[@]}"  # npm run build's .js files are not tracked: leave none behind
  if [ "${code}" -eq 124 ] || [ "${code}" -eq 137 ]; then
    rm -f "${log}"; ally_skip "not finished in ${ALLY_TIMEOUT} s"; return 0
  fi
  summary=0
  if [ -f "${ALLY_REPORT}" ]; then python3 scripts/prehandin_ally.py summary "${ALLY_REPORT}" || summary=$?; else summary=3; fi
  if [ "${code}" -ne 0 ] && [ "${summary}" -ne 0 ]; then
    # No test ran: the build, the migrations, the stand-ins or a server did not start. Not judged here.
    tail -30 "${log}"
    ally_skip "the Ally suite did not start here ($(ally_cause "${log}")); its log is ${log}"
    return 0
  fi
  rm -f "${log}"
  [ "${code}" -eq 0 ] || fail "Ally (fix the failures above; the full report is ${ALLY_REPORT})"
}

ally_cause() {
  # The first error line the report or the log gives, for the skip line.
  local cause=""
  [ -f "${ALLY_REPORT}" ] && cause="$(python3 scripts/prehandin_ally.py summary "${ALLY_REPORT}" 2>/dev/null | sed -n 's/^SETUP ERROR //p' | head -1)"
  [ -n "${cause}" ] || cause="$(grep -m1 -E 'Error|error' "$1" || true)"
  printf '%s' "${cause:-exit without a report}" | cut -c1-160
}

# Ci's Coverage job for the floored crates this change touches (D-1440): the same nightly, flags and
# comparison (scripts/coverage_floor.py; tests/test_prehandin.py keeps NIGHTLY and the flags equal to ci.yml).
NIGHTLY="nightly-2026-10-15"
COVERAGE_TIMEOUT="${PREHANDIN_COVERAGE_TIMEOUT:-360}"
coverage() {
  [ -f tests/coverage_floor.json ] && [ -f scripts/coverage_floor.py ] && [ -f Cargo.lock ] || return 0
  local picked packages=() only=() judged="" kind name report code
  picked="$(printf '%s\n' "${CHANGED}" | python3 scripts/coverage_floor.py select)" \
    || { echo "skipped: scripts/coverage_floor.py select could not read the workspace (Ci measures it)"; return 0; }
  [ -n "${picked}" ] || return 0
  while read -r kind name; do
    if [ "${kind}" = package ]; then packages+=(-p "${name}"); else only+=(--only "${name}"); judged+=" ${name}"; fi
  done <<< "${picked}"
  step "Floored code changed: branch coverage of${judged} (as Ci)"
  if ! command -v cargo-llvm-cov >/dev/null 2>&1 || ! rustup toolchain list 2>/dev/null | grep -q "^${NIGHTLY}" || [ -z "${DATABASE_URL:-}" ]; then
    echo "skipped: needs cargo-llvm-cov, the ${NIGHTLY} toolchain and DATABASE_URL (Ci measures it)"
    return 0
  fi
  df -h "${CARGO_LLVM_COV_TARGET_DIR:-.}" 2>/dev/null | tail -1 || true  # disk room for the instrumented build
  report="$(mktemp)"
  code=0
  timeout "${COVERAGE_TIMEOUT}" cargo "+${NIGHTLY}" llvm-cov "${packages[@]}" --locked --branch --json --summary-only \
    --ignore-filename-regex '(/tests/|/bin/)' --output-path "${report}" >"${report}.log" 2>&1 || code=$?
  if [ "${code}" -eq 124 ]; then
    echo "skipped: not finished in ${COVERAGE_TIMEOUT} s (Ci measures it)"
  elif [ "${code}" -ne 0 ]; then
    tail -30 "${report}.log"; rm -f "${report}" "${report}.log"; fail "cargo llvm-cov (a test failed under coverage)"
  else
    run_check "coverage floor" python3 scripts/coverage_floor.py compare "${report}" "${only[@]}" --base "${BASE}"
  fi
  rm -f "${report}" "${report}.log"
}

pg16_path
if [ -n "${ALLY}" ]; then
  ally_part
  printf '\nprehandin --ally: %s\n' "${ALLY_VERDICT}"
  exit 0
fi
quick_part
if [ -z "${QUICK}" ]; then
  extras
fi
printf '\nprehandin: green%s\n' "${QUICK:+ (quick)}"
