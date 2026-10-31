"""Traceability check for docs/spec/*.md.

Checks that requirement IDs are unique and use their file's prefix, that every
requirement cites at least one `path:LINE` whose file exists in the repository
with a non-blank line at that number, that every named decision (D-###) exists
in docs/decisions.md, that no requirement contains a superseded value, and that
no requirement's text implies Ask Hava is a contractor (WORD-022) or uses a
banned description such as "connect you with" (WORD-023).
Prints counts; exits 1 on any error.

Usage: python3 scripts/trace_spec.py [--root REPO_ROOT]
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

REQ_START = "- **"
REQ_LINE = re.compile(r"^- \*\*(?P<id>[A-Z0-9]+-\d{3})\*\* (?P<body>.+)$")
PREFIX_LINE = re.compile(r"^Prefix: `(?P<prefix>[A-Z0-9]+)`")
CITATION = re.compile(r"`(?P<path>[^`\s]+):(?P<line>\d+)`")
DECISION_REF = re.compile(r"\bD-\d{3,}\b")
DECISION_DEF = re.compile(r"^- \*\*(?P<id>D-\d{3,})\*\*")
SOURCE_MARK = " Source: "
SPEC_DIR = Path("docs/spec")
DECISIONS_FILE = Path("docs/decisions.md")
SKIP_FILES = frozenset({"index.md"})
SUPERSEDED: tuple[tuple[re.Pattern[str], str], ...] = (
    (re.compile(r"havasu home quotes", re.IGNORECASE), "old brand"),
    (re.compile(r"\$(?:45|59|149)\b"), "old fee"),
    (re.compile(r"\b(?:2|two) months\b", re.IGNORECASE), "old recurring rule"),
)
# WORD-022 (D-360): wording that says or implies Ask Hava hires, dispatches, assigns, sends a
# worker, schedules, books, subcontracts, manages, warrants, guarantees, prices, bids on or
# recommends work, or that a business works for or through Ask Hava. One list for the spec
# check below, the catalog check (scripts/check_catalog.py) and the app's wording scan (T104).
_NOT_A_CONTRACTOR_PATTERNS: tuple[str, ...] = (
    (
        r"\bour (?:pros?|professionals|network|contractors?|plumbers?|electricians?|technicians?"
        r"|techs?|crews?|businesses|workers|team of)\b"
    ),
    r"\byour (?:contractor|pro|plumber|electrician|technician|handyman)\b",
    (
        r"\b(?:we|Ask Hava)(?:'ll| will)? (?:dispatch|assign|schedule|book)s? (?:you )?"
        r"(?:a|an|the|your|someone)\b"
    ),
    (
        r"\bsend(?:s|ing)? (?:you )?(?:a|an) (?:plumber|pro|electrician|technician|tech|handyman"
        r"|contractor|crew|worker)\b"
    ),
    r"\bbook(?:s|ed|ing|ings)?\b",
    r"\bhir(?:e|es|ed|ing) (?:through|via|with) (?:us|Ask Hava)\b",
    r"\bthrough (?:Ask Hava|us)\b",
    (
        r"\bdispatch(?:es|ed|ing)? (?:a|an|the|your) (?:plumber|pro|business|contractor|technician"
        r"|crew|worker)\b"
    ),
    r"\bassign(?:s|ed|ing|ment|ments)?\b",
    r"\b(?:we|Ask Hava) (?:found|matched) you\b",
    r"\bmatch(?:es|ed)? you (?:with|to)\b",
    r"\bservice[- ]providers?\b",
    r"\bprovider partners?\b",
    r"\b(?:our|Ask Hava's|Ask Hava|referral) network\b",
    r"\bsubcontract\w*",
    r"\b(?:we|Ask Hava) (?:guarantee|warrant|recommend|manage|negotiate|supervise|inspect)s?\b",
    r"\bAsk Hava(?:'s)? (?:guarantee|warranty|pros?|plumbers?|contractors?|technicians?)\b",
    r"\bguaranteed\b",
    r"\b(?:work|works|working) for (?:us|Ask Hava)\b",
    r"\bour (?:prices?|rates?|quotes?|estimates?)\b",
    r"\bhire (?:us|Ask Hava)\b",
    r"\bnuestr[oa]s? (?:profesionales|red|contratistas|técnicos|plomeros|electricistas)\b",
    r"\ble enviaremos (?:un|una) (?:plomero|técnico|profesional|contratista|electricista)\b",
    r"\bsubcontrat\w*",
    # WORD-023 (D-377): one honest description; never "connect you with" or a referral network.
    r"\bconnect(?:s|ing)? (?:you|homeowners|customers|people) (?:with|to)\b",
    r"\breferral (?:network|service)s?\b",
)
NOT_A_CONTRACTOR: tuple[tuple[re.Pattern[str], str], ...] = tuple(
    (re.compile(p, re.IGNORECASE), "implies Ask Hava is a contractor (WORD-022)")
    for p in _NOT_A_CONTRACTOR_PATTERNS
)
# Rules that quote the banned phrases in order to ban them (D-360).
WORDING_RULE_IDS = frozenset(
    {"WORD-011", "WORD-020", "WORD-022", "WORD-023", "TERMS-012", "TERMS-026", "TERMS-027"}
)


@dataclass(frozen=True)
class Citation:
    path: str
    line: int


@dataclass(frozen=True)
class Requirement:
    req_id: str
    spec_file: str
    lineno: int
    text: str
    citations: tuple[Citation, ...]
    decisions: tuple[str, ...]

    @property
    def where(self) -> str:
        return f"{self.spec_file}:{self.lineno}"


@dataclass
class Report:
    requirements: list[Requirement]
    errors: list[str]


def parse_requirement(spec_file: str, lineno: int, line: str) -> Requirement | str:
    """Parse one requirement line, or return an error message."""
    match = REQ_LINE.match(line)
    if match is None:
        return f"{spec_file}:{lineno}: malformed requirement line"
    body = match.group("body")
    if SOURCE_MARK not in body:
        return f"{spec_file}:{lineno}: {match.group('id')} has no 'Source:' part"
    text, source = body.rsplit(SOURCE_MARK, 1)
    citations = tuple(
        Citation(m.group("path"), int(m.group("line"))) for m in CITATION.finditer(source)
    )
    if not citations:
        return f"{spec_file}:{lineno}: {match.group('id')} has no citation"
    if not text.strip():
        return f"{spec_file}:{lineno}: {match.group('id')} has no requirement text"
    return Requirement(
        req_id=match.group("id"),
        spec_file=spec_file,
        lineno=lineno,
        text=text.strip(),
        citations=citations,
        decisions=tuple(DECISION_REF.findall(text)),
    )


def parse_spec_file(path: Path, root: Path) -> tuple[str | None, list[Requirement], list[str]]:
    """Return the file's declared prefix, its requirements and any parse errors."""
    spec_file = path.relative_to(root).as_posix()
    prefix: str | None = None
    requirements: list[Requirement] = []
    errors: list[str] = []
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        prefix_match = PREFIX_LINE.match(line)
        if prefix_match and prefix is None:
            prefix = prefix_match.group("prefix")
        if not line.startswith(REQ_START):
            continue
        parsed = parse_requirement(spec_file, lineno, line)
        if isinstance(parsed, str):
            errors.append(parsed)
        else:
            requirements.append(parsed)
    if prefix is None:
        errors.append(f"{spec_file}: no 'Prefix:' line")
    return prefix, requirements, errors


def check_prefixes(prefixes: dict[str, str | None], reqs: list[Requirement]) -> list[str]:
    """Each file has its own prefix, and each ID uses its file's prefix."""
    errors: list[str] = []
    seen: dict[str, str] = {}
    for spec_file, prefix in prefixes.items():
        if prefix is None:
            continue
        if prefix in seen:
            errors.append(f"{spec_file}: prefix {prefix} already used by {seen[prefix]}")
        seen[prefix] = spec_file
    for req in reqs:
        expected = prefixes.get(req.spec_file)
        if expected is not None and not req.req_id.startswith(f"{expected}-"):
            errors.append(f"{req.where}: {req.req_id} does not use prefix {expected}")
    return errors


def check_unique_ids(reqs: list[Requirement]) -> list[str]:
    first: dict[str, str] = {}
    errors: list[str] = []
    for req in reqs:
        if req.req_id in first:
            errors.append(f"{req.where}: duplicate ID {req.req_id} (first at {first[req.req_id]})")
        else:
            first[req.req_id] = req.where
    return errors


def load_lines(root: Path, rel: str, cache: dict[str, list[str] | None]) -> list[str] | None:
    """Lines of a repository file, or None if it is missing or outside the repository."""
    if rel not in cache:
        candidate = (root / rel).resolve()
        inside = candidate.is_relative_to(root.resolve())
        if Path(rel).is_absolute() or not inside or not candidate.is_file():
            cache[rel] = None
        else:
            cache[rel] = candidate.read_text(encoding="utf-8").splitlines()
    return cache[rel]


def check_citation(root: Path, cite: Citation, cache: dict[str, list[str] | None]) -> str | None:
    if cite.path.startswith(SPEC_DIR.as_posix() + "/"):
        return f"cites the spec itself ({cite.path})"
    lines = load_lines(root, cite.path, cache)
    if lines is None:
        return f"cited file not found in repository: {cite.path}"
    if not 1 <= cite.line <= len(lines):
        return f"{cite.path}:{cite.line} is past the end ({len(lines)} lines)"
    if not lines[cite.line - 1].strip():
        return f"{cite.path}:{cite.line} is a blank line"
    return None


def check_citations(root: Path, reqs: list[Requirement]) -> list[str]:
    cache: dict[str, list[str] | None] = {}
    errors: list[str] = []
    for req in reqs:
        for cite in req.citations:
            problem = check_citation(root, cite, cache)
            if problem:
                errors.append(f"{req.where}: {req.req_id} {problem}")
    return errors


def known_decisions(root: Path) -> set[str]:
    path = root / DECISIONS_FILE
    if not path.is_file():
        return set()
    lines = path.read_text(encoding="utf-8").splitlines()
    return {m.group("id") for line in lines if (m := DECISION_DEF.match(line))}


def check_decisions(root: Path, reqs: list[Requirement]) -> list[str]:
    known = known_decisions(root)
    return [
        f"{req.where}: {req.req_id} names {ref}, which is not in {DECISIONS_FILE.as_posix()}"
        for req in reqs
        for ref in req.decisions
        if ref not in known
    ]


def check_superseded(reqs: list[Requirement]) -> list[str]:
    return [
        f"{req.where}: {req.req_id} contains a superseded value ({label}: {pat.pattern})"
        for req in reqs
        for pat, label in SUPERSEDED
        if pat.search(req.text)
    ]


def check_not_a_contractor(reqs: list[Requirement]) -> list[str]:
    """WORD-022: no requirement text (before "Source:") implies Ask Hava is a contractor."""
    return [
        f"{req.where}: {req.req_id} {label}: {match.group(0)!r}"
        for req in reqs
        if req.req_id not in WORDING_RULE_IDS
        for pat, label in NOT_A_CONTRACTOR
        for match in [pat.search(req.text)]
        if match
    ]


def run(root: Path) -> Report:
    """Parse every spec file under root and run all checks."""
    spec_dir = root / SPEC_DIR
    files = sorted(p for p in spec_dir.glob("*.md") if p.name not in SKIP_FILES)
    if not files:
        return Report([], [f"no spec files in {SPEC_DIR.as_posix()}"])
    prefixes: dict[str, str | None] = {}
    reqs: list[Requirement] = []
    errors: list[str] = []
    for path in files:
        prefix, file_reqs, file_errors = parse_spec_file(path, root)
        prefixes[path.relative_to(root).as_posix()] = prefix
        reqs.extend(file_reqs)
        errors.extend(file_errors)
    errors += check_prefixes(prefixes, reqs)
    errors += check_unique_ids(reqs)
    errors += check_citations(root, reqs)
    errors += check_decisions(root, reqs)
    errors += check_superseded(reqs)
    errors += check_not_a_contractor(reqs)
    return Report(reqs, errors)


def format_counts(report: Report) -> list[str]:
    """Human-readable counts per spec file, per cited source and in total."""
    reqs = report.requirements
    per_file = Counter(r.spec_file for r in reqs)
    per_source = Counter(c.path for r in reqs for c in r.citations)
    distinct = {(c.path, c.line) for r in reqs for c in r.citations}
    lines = ["Requirements per spec file:"]
    lines += [f"  {name}: {count}" for name, count in sorted(per_file.items())]
    lines.append("Citations per source file:")
    lines += [f"  {name}: {count}" for name, count in per_source.most_common()]
    lines.append(f"Spec files: {len(per_file)}")
    lines.append(f"Requirements: {len(reqs)}")
    lines.append(f"Citations: {sum(per_source.values())} ({len(distinct)} distinct lines)")
    with_decision = sum(1 for r in reqs if r.decisions)
    lines.append(f"Requirements resting on a logged decision: {with_decision}")
    lines.append(f"Errors: {len(report.errors)}")
    return lines


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="repository root (default: the parent of scripts/)",
    )
    args = parser.parse_args(argv)
    report = run(args.root)
    for line in format_counts(report):
        print(line)
    for error in report.errors:
        print(f"ERROR {error}", file=sys.stderr)
    return 1 if report.errors else 0


if __name__ == "__main__":
    sys.exit(main())
