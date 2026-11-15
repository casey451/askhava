"""The pre-hand-in gate's Ally step (D-3837): which changed paths can turn `Ally` red, and a short summary of a run.

`Ally` (.github/workflows/a11y.yml, `npx playwright test --config tests/a11y/playwright.config.ts`) renders
every page state the route table and `tests/a11y/page_states.ts` name, on the test-control build that
`tests/e2e/webserver.ts` starts. scripts/prehandin.sh `--ally` runs that suite on the task branch only when
`select` names a path, so a page failure (T261 attempt 4: a route with no `PAGE_STATES` entry, A11Y-002; T462
attempt 1: axe target-size on its own buttons) is fixed inside the session instead of costing an attempt.

`select` reads the changed paths (one per line on stdin) and prints each one that can change what `Ally`
renders or checks, nothing when none can:

- a page template (`app/<crate>/templates/*.html`), a browser script or stylesheet (`app/<crate>/static/`),
  an `askama.toml`;
- the suite and its server: `tests/a11y/`, `tests/e2e/webserver.ts` and `tests/e2e/keyed.ts` (the suite
  imports both), the stand-ins it starts (`stand_ins/`), the npm tree (`package.json`, `package-lock.json`),
  `.github/workflows/a11y.yml`;
- `migrations/` (each page state is seeded by SQL against the migrated tables);
- Rust: any source of a page crate (a crate with an `.html` template and a `.route(` call outside its tests:
  its handlers build the page views); any other source file that holds a `.route(` call on the head or the
  base (`ROUTES` in `tests/a11y/fixtures.ts` is read from every `.route(` in `app/*/src`); and the router
  composition, `app/server/src/lib.rs` and `main.rs` (whether a route is served at all).

Not a trigger: `docs/architecture.md` section 13. The suite never reads it; the route a row flips to `yes`
reaches `Ally` through the `.route(` call that serves it, which is a trigger.

`summary` reads the suite's JSON report (`test-results/a11y-report/results.json`) and prints one line per
failed test, its location and the first line of its error, then the counts, so the worker reads failures only.

Usage:
  git diff --name-only BASE HEAD | python3 scripts/prehandin_ally.py select --base BASE [--root REPO]
  python3 scripts/prehandin_ally.py summary test-results/a11y-report/results.json
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections.abc import Callable, Iterable, Iterator
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
# Non-Rust paths that change what `Ally` renders or how it runs.
ALLY_PATHS = re.compile(
    r"^(app/[^/]+/templates/[^/]+\.html$|app/[^/]+/static/|app/[^/]+/askama\.toml$|tests/a11y/"
    r"|tests/e2e/(webserver|keyed)\.ts$|stand_ins/|migrations/|package(-lock)?\.json$|\.github/workflows/a11y\.yml$)"
)
RUST_SOURCE = re.compile(r"^app/([^/]+)/src/(.+\.rs)$")
COMPOSITION = ("app/server/src/lib.rs", "app/server/src/main.rs")
ROUTE_CALL = ".route("
MAX_FAILURES = 20
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def in_tests(rel: str) -> bool:
    """A source under a `tests` folder: `fixtures.ts` leaves those out of the route table, so do we."""
    return "tests" in Path(rel).parts[:-1] or rel == "tests.rs"


def page_crates(root: Path) -> set[str]:
    """Crates with an `.html` template and a `.route(` call outside their tests (today `portals`)."""
    found: set[str] = set()
    for crate in sorted(p for p in (root / "app").glob("*") if p.is_dir()):
        if not any((crate / "templates").glob("*.html")):
            continue
        sources = (f for f in (crate / "src").rglob("*.rs") if not in_tests(f.relative_to(crate / "src").as_posix()))
        if any(ROUTE_CALL in f.read_text(encoding="utf-8", errors="replace") for f in sources):
            found.add(crate.name)
    return found


def git_text(root: Path, base: str) -> Callable[[str], str]:
    """A reader of a file's text at the base commit; empty when it did not exist there."""

    def read(path: str) -> str:
        done = subprocess.run(["git", "show", f"{base}:{path}"], cwd=root, capture_output=True, text=True, check=False)
        return done.stdout if done.returncode == 0 else ""

    return read


def head_text(root: Path, path: str) -> str:
    """A file's text in the working tree; empty when the change deleted it."""
    try:
        return (root / path).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def triggers(changed: Iterable[str], root: Path, base_text: Callable[[str], str]) -> Iterator[str]:
    """Each changed path that can change what `Ally` renders or checks, in the order given."""
    pages: set[str] | None = None
    for path in (p.strip() for p in changed):
        if not path:
            continue
        if ALLY_PATHS.match(path) or path in COMPOSITION:
            yield path
            continue
        rust = RUST_SOURCE.match(path)
        if rust is None or in_tests(rust.group(2)):
            continue
        pages = page_crates(root) if pages is None else pages
        if rust.group(1) in pages or ROUTE_CALL in head_text(root, path) or ROUTE_CALL in base_text(path):
            yield path


def failures(report: dict[str, Any]) -> Iterator[str]:
    """One line per failed test: `file:line title: first error line`."""

    def specs(suite: dict[str, Any]) -> Iterator[dict[str, Any]]:
        yield from suite.get("specs", [])
        for child in suite.get("suites", []):
            yield from specs(child)

    for suite in report.get("suites", []):
        for spec in specs(suite):
            for test in spec.get("tests", []):
                results = test.get("results", [])
                if not results or results[-1].get("status") in ("passed", "skipped"):
                    continue
                error = (results[-1].get("error") or {}).get("message", results[-1].get("status", ""))
                first = next((ln.strip() for ln in ANSI.sub("", str(error)).splitlines() if ln.strip()), "")
                yield f"{spec.get('file', '')}:{spec.get('line', '')} {spec.get('title', '')}: {first[:200]}"


def summary(path: Path) -> str:
    """The failed tests (at most MAX_FAILURES) and the counts of the run."""
    report = json.loads(path.read_text(encoding="utf-8"))
    failed = list(failures(report))
    stats = report.get("stats", {})
    lines = [f"FAIL {line}" for line in failed[:MAX_FAILURES]]
    if len(failed) > MAX_FAILURES:
        lines.append(f"... and {len(failed) - MAX_FAILURES} more failed tests")
    lines.append(f"Ally: {len(failed)} failed, {stats.get('expected', 0)} passed, {stats.get('skipped', 0)} skipped")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
    sub = parser.add_subparsers(dest="command", required=True)
    pick = sub.add_parser("select", help="print each changed path (stdin) that can change Ally's result")
    pick.add_argument("--base", required=True, help="the merge base the change is compared with")
    pick.add_argument("--root", type=Path, default=ROOT)
    show = sub.add_parser("summary", help="print the failed tests of a Playwright JSON report")
    show.add_argument("report", type=Path)
    args = parser.parse_args(argv)
    if args.command == "select":
        for path in triggers(sys.stdin.read().splitlines(), args.root, git_text(args.root, args.base)):
            print(path)
        return 0
    try:
        print(summary(args.report))
    except (OSError, ValueError) as error:
        print(f"no readable report at {args.report}: {str(error)[:200]}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
