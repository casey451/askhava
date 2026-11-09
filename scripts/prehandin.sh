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
#     rules: eslint and `npm audit --audit-level=low` for the npm tree; cargo-audit and cargo-deny for
#     Rust when both are on the path (`Ci` installs them; elsewhere it says it skipped); and, when the diff
#     touches a crate with a floor in tests/coverage_floor.json, `Ci`'s branch coverage of that crate and
#     every workspace crate that depends on it (their tests carry its coverage: askhava-payments alone
#     measured 61%, with askhava-integration 98.8%), judged by scripts/coverage_floor.py, `Ci`'s own
#     comparison, on those floors only (D-1440). It needs cargo-llvm-cov, the pinned nightly and
#     DATABASE_URL, else it says it skipped; past PREHANDIN_COVERAGE_TIMEOUT (360 s, so the whole gate
#     stays inside a 600 s Bash call) it says so and lets `Ci` judge. Measured 271 s warm, 324 s cold for payments, so it is not in --quick (the runner's 240 s).
# Not here: pytest, cargo and tsc (scripts/smoke.sh runs them; the worker runs both scripts), and `Ci`'s
# workspace-total coverage (4.5 to 6 minutes in a session, D-1300).
#
# The worker runs it after `bash scripts/smoke.sh` as the last step before hand-in (kit/prompts/TEMPLATE.md).
# The runner runs main's copy with --quick on the handed-in head at `python3 -m runner --after` and does not
# hand in a red head (runner/github_env.py `prehandin`, runner/__main__.py `after`, D-1300).
#
# Usage: bash scripts/prehandin.sh [--quick] [--base REF] [--branch NAME]
#   --base    the main commit to compare with (default origin/main, fetched first when the fetch works)
#   --branch  the branch name check_pr_change.py checks paths for (default the current branch)
# PREHANDIN_ROOT names the checkout to check when this file is run from elsewhere (the runner runs
# main's copy against the handed-in tree). Exit 0 means green; on red the failing check's last lines are
# printed and the exit status is 1; a git problem (no such base, no merge base) exits 3, never 1.
set -euo pipefail
cd "${PREHANDIN_ROOT:-$(dirname "$0")/..}"

QUICK="" BASE="" BRANCH=""
while [ $# -gt 0 ]; do
  case "$1" in
    --quick) QUICK=1 ;;
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
RUST_PATHS='^(app/|tests/(integration|security|recovery|fixtures|mutation)/|scripts/run_named_mutants\.py|migrations/|docs/legal/|docs/catalog/|docs/architecture\.md|docs/copy/mail-app-steps\.md|schemas/|scripts/pii_scan\.py|stand_ins/|kit/layers\.json|kit/allowed_crates\.txt|Cargo\.(toml|lock)|rust-toolchain\.toml|clippy\.toml|deny\.toml|\.sqlx/|\.github/workflows/ci\.yml)'
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
  for check in pii_scan trace_spec validate_tasks check_catalog check_legal_docs check_ai_smell check_copy check_code_docs check_rust_rules check_crates check_include_paths check_plan_lock; do
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
    run_check "npm audit --audit-level=low" npm audit --audit-level=low
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
  echo "not run here: pytest, cargo and tsc (scripts/smoke.sh), Ci's workspace total coverage, Twin, Ally"
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

quick_part
if [ -z "${QUICK}" ]; then
  extras
fi
printf '\nprehandin: green%s\n' "${QUICK:+ (quick)}"
