"""Run `npm audit` and fail on any advisory except the few this plan names (D-4223, SEC-021).

`Ci` ("TypeScript checks and npm audit") and `scripts/prehandin.sh` call this in place of
`npm audit --audit-level=low`. It runs `npm audit --json` from the repository root and fails on every
reported vulnerable package, whatever its severity (info and low included), unless every advisory that
package traces to is excused by an entry in `EXCEPTIONS` below.

How a package is traced: npm lists, for each vulnerable package, a `via` list. A dict entry is an
advisory (its GHSA id is read from its `url`); a string entry names another vulnerable package, whose
own `via` is followed in turn (each package once, so a cycle ends). An advisory is excused only when
its GHSA id, the package it is reported on and the advisory's own `name` all match one exception. A
package whose trace reaches no advisory at all, an advisory without a GHSA id, or a string naming a
package npm did not report, fails.

It fails closed: npm missing, output that is not JSON, an `error` key, an `auditReportVersion` other
than 2, a vulnerability count that disagrees with the list, or an exception entry missing a field.
Every excused advisory is printed on every run so it stays visible; an exception npm no longer reports
prints a note (not a failure) so a later plan change removes it.

The exception list is a constant here, not a data file, because `scripts/*.py` is fingerprinted in
`kit/plan.lock`: adding or widening an exception is a plan change with a `docs/decisions.md` entry.

Usage: python3 scripts/npm_audit.py [--input REPORT.json]
  --input reads a saved `npm audit --json` report instead of running npm (for tests and checks).
Exit status 0 when nothing unexcused is reported, 1 otherwise.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
GHSA = re.compile(r"GHSA(?:-[23456789cfghjmpqrvwx]{4}){3}")
DECISION = re.compile(r"D-\d+")
REPORT_VERSION = 2
EXCEPTION_FIELDS = ("ghsa", "package", "decision", "reason")

# Each entry excuses one advisory on one package, nothing else. Remove it by a plan change once a
# patched release exists (the script prints a note when npm stops reporting it).
EXCEPTIONS: tuple[dict[str, str], ...] = (
    {
        "ghsa": "GHSA-fc5q-39xc-vjpf",
        "package": "picomatch",
        "decision": "D-4223",
        "reason": "ReDoS in every picomatch release, no patched version; dev-only lint chain "
        "(typescript-eslint), never in the production image, glob patterns are ours",
    },
)


class AuditError(Exception):
    """The report cannot be judged: npm failed, the shape is unknown or an exception is malformed."""


def check_exceptions(exceptions: tuple[dict[str, str], ...]) -> None:
    """Raise AuditError unless every exception has every field, a GHSA id and a decision number."""
    for entry in exceptions:
        missing = [f for f in EXCEPTION_FIELDS if not str(entry.get(f, "")).strip()]
        if missing:
            raise AuditError(f"exception {entry!r} lacks {', '.join(missing)}")
        if not GHSA.fullmatch(entry["ghsa"]) or not DECISION.fullmatch(entry["decision"]):
            raise AuditError(f"exception {entry!r} needs a GHSA id and a D-#### decision")


def parse_report(text: str) -> dict[str, Any]:
    """Return the `vulnerabilities` map of an `npm audit --json` report, or raise AuditError."""
    try:
        report = json.loads(text)
    except ValueError as exc:
        raise AuditError(f"npm audit did not print JSON: {exc}") from exc
    if not isinstance(report, dict):
        raise AuditError("npm audit report is not a JSON object")
    if "error" in report:
        raise AuditError(f"npm audit reported an error: {report['error']!r}")
    if report.get("auditReportVersion") != REPORT_VERSION:
        raise AuditError(f"unknown npm audit report version {report.get('auditReportVersion')!r}")
    vulns = report.get("vulnerabilities")
    metadata = report.get("metadata")
    counts = metadata.get("vulnerabilities") if isinstance(metadata, dict) else None
    total = counts.get("total") if isinstance(counts, dict) else None
    if not isinstance(vulns, dict) or not all(isinstance(v, dict) for v in vulns.values()):
        raise AuditError("npm audit report has no vulnerabilities map")
    if total != len(vulns):
        raise AuditError(f"npm audit counts {total!r} vulnerable packages but lists {len(vulns)}")
    return vulns


def advisory_id(advisory: dict[str, Any]) -> str:
    """The GHSA id in an advisory's url, or '' when it has none."""
    found = GHSA.search(str(advisory.get("url", "")))
    return found.group(0) if found else ""


def trace(vulns: dict[str, Any], start: str) -> list[tuple[str, dict[str, Any]]]:
    """Every (package, advisory) reachable from `start` through `via`, each package visited once."""
    found: list[tuple[str, dict[str, Any]]] = []
    seen: set[str] = set()
    todo = [start]
    while todo:
        name = todo.pop()
        if name in seen:
            continue
        seen.add(name)
        if name not in vulns:
            raise AuditError(f"{start}: via names {name}, which npm did not report")
        via = vulns[name].get("via")
        if not isinstance(via, list):
            raise AuditError(f"{name}: via is not a list")
        for item in via:
            if isinstance(item, dict):
                found.append((name, item))
            elif isinstance(item, str):
                todo.append(item)
            else:
                raise AuditError(f"{name}: via entry {item!r} is neither an advisory nor a package")
    return found


def excused_by(package: str, advisory: dict[str, Any], exceptions: tuple[dict[str, str], ...]) -> str:
    """The decision that excuses this advisory on this package, or '' when none does."""
    ghsa = advisory_id(advisory)
    for entry in exceptions:
        if ghsa and ghsa == entry["ghsa"] and package == entry["package"] == advisory.get("name"):
            return entry["decision"]
    return ""


def judge(vulns: dict[str, Any], exceptions: tuple[dict[str, str], ...]) -> tuple[list[str], set[str]]:
    """Return (problems, excused GHSA ids) for a vulnerabilities map."""
    check_exceptions(exceptions)
    problems: list[str] = []
    excused: set[str] = set()
    for name in sorted(vulns):
        found = trace(vulns, name)
        if not found:
            problems.append(f"{name}: reported vulnerable but traces to no advisory")
        for package, advisory in found:
            if excused_by(package, advisory, exceptions):
                excused.add(advisory_id(advisory))
            else:
                ident = advisory_id(advisory) or str(advisory.get("url") or advisory.get("source"))
                problems.append(f"{name}: {ident} on {package} ({advisory.get('severity')}): "
                                f"{advisory.get('title')}")
    return problems, excused


def run_npm() -> str:
    """Run `npm audit --json` at the repository root and return what it printed."""
    try:
        done = subprocess.run(["npm", "audit", "--json", "--audit-level=low"], cwd=ROOT, check=False,
                              capture_output=True, text=True)
    except OSError as exc:
        raise AuditError(f"cannot run npm: {exc}") from exc
    return done.stdout


def report(text: str, exceptions: tuple[dict[str, str], ...] = EXCEPTIONS) -> int:
    """Judge one report's text, print the outcome and return the exit status."""
    try:
        problems, excused = judge(parse_report(text), exceptions)
    except AuditError as exc:
        print(f"npm audit: FAILED (cannot judge): {exc}")
        return 1
    for entry in exceptions:
        if entry["ghsa"] in excused:
            print(f"npm audit: excused {entry['ghsa']} on {entry['package']} ({entry['decision']}): "
                  f"{entry['reason']}")
        else:
            print(f"npm audit: note: {entry['ghsa']} on {entry['package']} ({entry['decision']}) is no "
                  "longer reported; a plan change should remove its exception")
    for line in sorted(set(problems)):
        print(f"npm audit: {line}")
    print("npm audit: FAILED" if problems else "npm audit: ok")
    return 1 if problems else 0


def main(argv: list[str] | None = None) -> int:
    """Parse the arguments, read or run the audit, and judge it."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--input", type=Path, help="a saved `npm audit --json` report")
    args = parser.parse_args(argv)
    try:
        text = args.input.read_text(encoding="utf-8") if args.input else run_npm()
    except (OSError, AuditError) as exc:
        print(f"npm audit: FAILED (cannot judge): {exc}")
        return 1
    return report(text)


if __name__ == "__main__":
    sys.exit(main())
