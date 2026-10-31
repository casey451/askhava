"""PII and key scanner: fails on real-looking personal data or key-shaped strings.

Fake data only (BUILD_BRIEF.md:66; D-109). Flags, outside sources/:
  phone    a US number that is not in the fictional 555-0100 to 555-0199 block
           (or Twilio's 500-555-00xx magic numbers)
  address  a street address whose street name is not a fake one (Test, Example, ...)
  name     a labelled name field (first_name, homeowner_name, ...) holding a full
           name that is not on the fake-name list
  key      a string shaped like a vendor key, token or private key
  email    an email address outside example.com, example.org and the reserved
           .test, .example and .invalid domains (PRIV-019), other than Ask Hava's
           own role addresses, vendor test sinks and the listed placeholders
  image    an image under tests/ or docs/design/ whose EXIF carries GPS or camera
           device fields (make, model, serial, lens, owner); read with the
           standard library so Ci needs no imaging library
The twin's own generator literals (a fake over-sharer's address, two fixed record
phones) pass only by exact value in twin/twin.py, never by path (D-109, D-516).
A file that could not be scanned (over the size limit) fails the run unless it
is on SKIP_ALLOWED with a reason (D-485).
Findings print as path:line:rule with the match redacted. Exit 1 on any finding.

Usage: python3 scripts/pii_scan.py [--root REPO_ROOT]
"""

from __future__ import annotations

import argparse
import re
import struct
import subprocess
import sys
from collections.abc import Iterable, Iterator
from pathlib import Path

SKIP_PREFIXES = ("sources/", ".git/")
SKIP_FILES = ("kit/sources.sha256", "uv.lock")
MAX_BYTES = 4_000_000

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


# Email (PRIV-019, D-485): only reserved example domains, Ask Hava's own role addresses,
# vendor test sinks and instruction placeholders. A real person's address never passes.
EMAIL = re.compile(r"(?<![\w.+-])([A-Za-z0-9._%+-]+)@((?:[A-Za-z0-9-]+\.)+[A-Za-z]{2,})(?![\w-])")
EXAMPLE_DOMAINS = ("example.com", "example.org")
RESERVED_TLDS = ("test", "example", "invalid")
OWN_DOMAINS = ("askhava.com",)  # role addresses such as help@ and jobs@notify. (SEC-025)
SERVICE_DOMAINS = ("users.noreply.github.com",)  # GitHub's no-reply commit identities, never a person's inbox
VENDOR_TEST_SINKS = frozenset({"delivered@resend.dev", "bounced@resend.dev", "complained@resend.dev"})
PLACEHOLDER_EMAILS = frozenset({"askhava.quotes@gmail.com"})  # docs/pilot/hand-run-test.md: "for example"
PLACEHOLDER_LOCAL = re.compile(r"^you(?:\+[a-z0-9]+)?$")  # "you+poola@gmail.com" in the dry-run guide
FILE_EXTENSIONS = frozenset({"jpg", "jpeg", "png", "webp", "heic", "gif", "svg", "avif", "woff2", "css", "js", "ts"})

# Image metadata (PRIV-014, PRIV-019, D-485).
IMAGE_EXTENSIONS = (".jpg", ".jpeg", ".png", ".webp", ".heic")
IMAGE_ROOTS = ("tests/", "docs/design/")
GPS_IFD_TAG = 0x8825
EXIF_IFD_TAG = 0x8769
DEVICE_TAGS = frozenset({0x010F, 0x0110, 0xA430, 0xA431, 0xA433, 0xA434, 0xA435})  # make, model, owner, serials, lens
EXIF_MARKERS = (b"Exif\x00\x00", b"eXIf", b"EXIF")
MAX_IMAGE_BYTES = 64_000_000

# Files too large to scan pass only when listed here with a reason (D-485). Empty today.
SKIP_ALLOWED: dict[str, str] = {}

# The twin's generator literals (D-109, D-516): exact values allowed in one file each, never the twin/ path.
# Split so this file does not match itself.
TWIN_GENERATOR_LITERALS: dict[str, tuple[str, ...]] = {
    "twin/twin.py": ("412 Palo Verde " + "Dr", "+1928" + "0000000", "+1928" + "5550000"),
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


def email_allowed(local: str, domain: str) -> bool:
    """True for a reserved example domain, Ask Hava's own domain, a vendor sink or a placeholder."""
    domain = domain.lower()
    address = f"{local.lower()}@{domain}"
    tld = domain.rsplit(".", 1)[-1]
    if tld in FILE_EXTENSIONS or tld in RESERVED_TLDS:
        return True  # "plaster@2x.jpg" is a file name, not an address
    if any(domain == d or domain.endswith(f".{d}") for d in EXAMPLE_DOMAINS + OWN_DOMAINS + SERVICE_DOMAINS):
        return True
    if address in VENDOR_TEST_SINKS or address in PLACEHOLDER_EMAILS:
        return True
    return domain == "gmail.com" and bool(PLACEHOLDER_LOCAL.match(local.lower()))


def _email_findings(line: str) -> Iterator[str]:
    for match in EMAIL.finditer(line):
        if not email_allowed(match.group(1), match.group(2)):
            yield "email"


def scan_text(path: str, text: str) -> list[Finding]:
    """Findings for one file's text as (path, line number, rule)."""
    found: list[Finding] = []
    checks = (_phone_findings, _address_findings, _name_findings, _key_findings, _email_findings)
    allowed = TWIN_GENERATOR_LITERALS.get(path, ())
    for number, line in enumerate(text.splitlines(), start=1):
        for literal in allowed:
            line = line.replace(literal, "")
        hits = [r for check in checks for r in check(line)]
        found.extend((path, number, rule) for rule in dict.fromkeys(hits))
    return found


def _tiff_entries(data: bytes, start: int, offset: int, endian: str) -> list[tuple[int, int]]:
    """(tag, value-offset field) pairs of one IFD, or [] when the IFD does not parse."""
    pos = start + offset
    if offset < 8 or pos + 2 > len(data):
        return []
    (count,) = struct.unpack_from(f"{endian}H", data, pos)
    if count == 0 or count > 512 or pos + 2 + count * 12 > len(data):
        return []
    entries = []
    for i in range(count):
        tag, _kind, _num, value = struct.unpack_from(f"{endian}HHII", data, pos + 2 + i * 12)
        entries.append((tag, value))
    return entries


def tiff_rules(data: bytes, start: int) -> set[str]:
    """Metadata rules broken by the TIFF block at `start` (IFD0 and its Exif sub-IFD)."""
    head = data[start : start + 8]
    if len(head) < 8 or head[:4] not in (b"II*\x00", b"MM\x00*"):
        return set()
    endian = "<" if head[:2] == b"II" else ">"
    (ifd0,) = struct.unpack_from(f"{endian}I", head, 4)
    entries = _tiff_entries(data, start, ifd0, endian)
    sub = next((v for t, v in entries if t == EXIF_IFD_TAG), None)
    if sub is not None:
        entries += _tiff_entries(data, start, sub, endian)
    tags = {t for t, _ in entries}
    found = {"image-gps"} if GPS_IFD_TAG in tags else set()
    return found | ({"image-device"} if tags & DEVICE_TAGS else set())


def image_metadata_rules(data: bytes) -> set[str]:
    """GPS or device metadata in a JPEG APP1, PNG eXIf, WebP EXIF or HEIC Exif block, or XMP GPS."""
    found: set[str] = set()
    for marker in EXIF_MARKERS:
        pos = data.find(marker)
        while pos != -1:
            body = pos + len(marker)
            for start in (body, body + 4, body + 6, body + 10):  # WebP size field and optional Exif header
                found |= tiff_rules(data, start)
            pos = data.find(marker, pos + 1)
    if b"exif:GPSLatitude" in data or b"exif:GPSLongitude" in data:
        found.add("image-gps")
    return found


def is_scanned_image(rel: str) -> bool:
    return rel.lower().endswith(IMAGE_EXTENSIONS) and rel.startswith(IMAGE_ROOTS)


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
        size = file.stat().st_size
        if is_scanned_image(rel):
            data = file.read_bytes()[:MAX_IMAGE_BYTES]
            found.extend((rel, 0, rule) for rule in sorted(image_metadata_rules(data)))
            found.extend(scan_text(rel, data[:MAX_BYTES].decode("utf-8", errors="replace")))
            if size > MAX_IMAGE_BYTES and skipped is not None:
                skipped.append(f"{rel} (over {MAX_IMAGE_BYTES} bytes)")
            continue
        if size > MAX_BYTES:
            if skipped is not None:
                skipped.append(f"{rel} (over {MAX_BYTES} bytes)")
            continue
        found.extend(scan_text(rel, file.read_text(encoding="utf-8", errors="replace")))
    return found


def unallowed(skipped: list[str]) -> list[str]:
    """Skipped files not on SKIP_ALLOWED; each fails the run (D-485)."""
    return [item for item in skipped if item.split(" (", 1)[0] not in SKIP_ALLOWED]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0] if __doc__ else "")
    parser.add_argument("--root", default=".")
    args = parser.parse_args(argv)
    root = Path(args.root).resolve()
    skipped: list[str] = []
    findings = scan_files(root, tracked_files(root), skipped)
    for path, line, rule in findings:
        print(f"{path}:{line}: {rule} (match redacted)")
    failing = unallowed(skipped)
    for item in skipped:
        verdict = "fails, not on SKIP_ALLOWED" if item in failing else "allow-listed"
        print(f"skipped, not scanned: {item} ({verdict})")
    print(f"pii_scan: {len(findings)} finding(s), {len(skipped)} file(s) skipped, {len(failing)} not allow-listed")
    return 1 if findings or failing else 0


if __name__ == "__main__":
    sys.exit(main())
