"""PII and key scanner: fails on real-looking personal data or key-shaped strings.

Fake data only (BUILD_BRIEF.md:66; D-109). Flags, outside sources/:
  phone    a US number that is not in the fictional 555-0100 to 555-0199 block
           (or Twilio's 500-555-00xx magic numbers)
  address  a street address whose street name is not a fake one (Test, Example, ...)
  name     a labelled name field (first_name, homeowner_name, ...) holding a full
           name that is not on the fake-name list
  key      a string shaped like a vendor key, token or private key
Findings print as path:line:rule with the match redacted. Exit 1 on any finding.

Usage: python3 scripts/pii_scan.py [--root REPO_ROOT]
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from collections.abc import Iterable, Iterator
from pathlib import Path

SKIP_PREFIXES = ("sources/", ".git/")
SKIP_FILES = ("kit/sources.sha256", "uv.lock")
MAX_BYTES = 1_000_000

FAKE_STREETS = {"Test", "Example", "Sample", "Fake", "Canary", "Sentinel", "Placeholder"}
FAKE_NAMES = {
    "Zelda Canary",
    "Ada Example",
    "Sam Sample",
    "Pat Placeholder",
    "Alex Fakeperson",
    "Test Homeowner",
    "Test Business",
    "Test Owner",
    "Test User",
}
FAKE_NAME_WORDS = {"Test", "Example", "Sample", "Fake", "Canary", "Sentinel", "Placeholder"}
ALLOWED_PHONE = re.compile(r"^(?:555-?01\d\d|500-?555-?00\d\d)$")
ZERO_SID = re.compile(r"^(?:AC|SK)(0{32})$")

PHONE = re.compile(r"(?<![\w.])(?:\+?1[\s.-]?)?\(?([2-9]\d{2})\)?[\s.-]?(\d{3})[\s.-]?(\d{4})(?![\w])")
PHONE_PLUS = re.compile(r"\+1\d{10}(?!\d)")
STREET_TYPES = "Street|St|Avenue|Ave|Road|Rd|Drive|Dr|Lane|Ln|Boulevard|Blvd|Way|Court|Ct|Circle|Cir|Place|Pl"
STREET_WORD = r"[A-Z][a-z]+(?:[A-Z][a-z]+)?"  # also Mc/Mac names such as McDowell
ADDRESS = re.compile(rf"\b\d{{1,5}} (?:(?:[NSEW]|[NS][EW])\.? )?((?:{STREET_WORD} ){{1,3}})(?:{STREET_TYPES})\b\.?")
NAME_FIELD = re.compile(
    r"(?i:(?:full_?name|first_?name|last_?name|homeowner_?name|customer_?name|owner_?name|contact_?name))"
    r"[\"']?\s*[:=]\s*[\"']([A-Z][a-z]+(?: [A-Z][a-z]+)+)[\"']"
)
KEY_PATTERNS = {
    "stripe key": re.compile(r"\b[sr]k_(?:live|test)_[A-Za-z0-9]{16,}"),
    "stripe key (publishable)": re.compile(r"\bpk_(?:live|test)_[A-Za-z0-9]{16,}"),
    "webhook secret": re.compile(r"\bwhsec_[A-Za-z0-9+/=]{20,}"),
    "aws key": re.compile(r"\bAKIA[0-9A-Z]{16}\b"),
    "github token": re.compile(r"\b(?:ghp|gho|ghs|ghu)_[A-Za-z0-9]{30,}|\bgithub_pat_[A-Za-z0-9_]{30,}"),
    "slack token": re.compile(r"\bxox[abprs]-[A-Za-z0-9-]{10,}"),
    "anthropic key": re.compile(r"\bsk-ant-[A-Za-z0-9_-]{20,}"),
    "sendgrid or resend key": re.compile(r"\b(?:SG\.[A-Za-z0-9_-]{16,}|re_[A-Za-z0-9]{24,})"),
    "twilio sid": re.compile(r"\b(?:AC|SK)[0-9a-f]{32}\b"),
    "private key": re.compile(r"-----BEGIN (?:[A-Z]+ )?PRIVATE KEY-----"),
}


Finding = tuple[str, int, str]


def _phone_findings(line: str) -> Iterator[str]:
    for match in PHONE.finditer(line):
        local = f"{match.group(2)}-{match.group(3)}"
        if ALLOWED_PHONE.match(local) or ALLOWED_PHONE.match(f"{match.group(1)}-{match.group(2)}-{match.group(3)}"):
            continue
        if re.fullmatch(r"1\d{9}", match.group(0)):
            continue  # a bare run starting with 1 is a Unix timestamp or ID; area codes never start with 1
        yield "phone"
    for match in PHONE_PLUS.finditer(line):
        digits = match.group(0)[2:]
        if not ALLOWED_PHONE.match(f"{digits[3:6]}-{digits[6:]}") and not ALLOWED_PHONE.match(
            f"{digits[:3]}-{digits[3:6]}-{digits[6:]}"
        ):
            yield "phone"


def _address_findings(line: str) -> Iterator[str]:
    for match in ADDRESS.finditer(line):
        words = set(match.group(1).split())
        if not words & FAKE_STREETS:
            yield "address"


def _name_findings(line: str) -> Iterator[str]:
    for match in NAME_FIELD.finditer(line):
        name = match.group(1)
        if name not in FAKE_NAMES and not set(name.split()) & FAKE_NAME_WORDS:
            yield "name"


def _key_findings(line: str) -> Iterator[str]:
    for label, pattern in KEY_PATTERNS.items():
        for match in pattern.finditer(line):
            if not ZERO_SID.match(match.group(0)):
                yield label


def scan_text(path: str, text: str) -> list[Finding]:
    """Findings for one file's text as (path, line number, rule)."""
    found: list[Finding] = []
    for number, line in enumerate(text.splitlines(), start=1):
        hits = [r for check in (_phone_findings, _address_findings, _name_findings, _key_findings) for r in check(line)]
        found.extend((path, number, rule) for rule in dict.fromkeys(hits))
    return found


def tracked_files(root: Path) -> list[str]:
    try:
        out = subprocess.run(
            ["git", "-C", str(root), "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
            capture_output=True,
            check=True,
        ).stdout.decode()
        return [p for p in out.split("\0") if p]
    except (OSError, subprocess.CalledProcessError):
        return sorted(str(p.relative_to(root)) for p in root.rglob("*") if p.is_file() and ".git" not in p.parts)


def scan_files(root: Path, paths: Iterable[str], skipped: list[str] | None = None) -> list[Finding]:
    """Scan files; paths that could not be scanned (too large) are appended to `skipped`."""
    found: list[Finding] = []
    for rel in paths:
        if rel.startswith(SKIP_PREFIXES) or rel in SKIP_FILES:
            continue
        file = root / rel
        if not file.is_file():
            continue
        if file.stat().st_size > MAX_BYTES:
            if skipped is not None:
                skipped.append(f"{rel} (over {MAX_BYTES} bytes)")
            continue
        found.extend(scan_text(rel, file.read_text(encoding="utf-8", errors="replace")))
    return found


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0] if __doc__ else "")
    parser.add_argument("--root", default=".")
    args = parser.parse_args(argv)
    root = Path(args.root).resolve()
    skipped: list[str] = []
    findings = scan_files(root, tracked_files(root), skipped)
    for path, line, rule in findings:
        print(f"{path}:{line}: {rule} (match redacted)")
    for item in skipped:
        print(f"skipped, not scanned: {item}")
    print(f"pii_scan: {len(findings)} finding(s), {len(skipped)} file(s) skipped")
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
