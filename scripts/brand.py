"""The brand as data for the build's own checks (D-1932; WORD-001, WORD-008, WORD-021, TXT-006, FEE-049).

Reads app/core/data/brand.json, the one source the app compiles in (app/core/src/brand.rs), with the
same fields, checks and tokens: load() parses and checks the file, naming a bad field;
fill(text, spanish=False) replaces {brand}, {brand_business}, {brand_short}, {brand_legal} and
{domain}, leaves every other {name} placeholder alone and refuses an unknown {brand...} token.

Usage: python3 scripts/brand.py   (prints the checked brand's display name)
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

BRAND_FILE = Path(__file__).resolve().parent.parent / "app" / "core" / "data" / "brand.json"
FIELDS = (
    "display_name",
    "business_name",
    "short_name",
    "legal_name",
    "spoken_legal_name",
    "domain",
    "email_from_name",
    "email_from_address",
    "statement_prefix",
    "forbidden_alone",
    "es",
)
ES_FIELDS = ("display_name", "business_name", "short_name", "legal_name")
SHORT_NAME_MAX = 20  # WORD-008: every text opens "<short_name>:"
STATEMENT_MAX = 22  # FEE-049: the card statement's descriptor limit
STATEMENT_JOINER = "* JOB "
JOB_NUMBER_DIGITS = 7
LABEL = re.compile(r"^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$")
PREFIX = re.compile(r"^(?=.*[A-Z])[A-Z0-9]{5,10}$")
TOKEN = re.compile(r"\{([^{}]*)\}")


class BrandError(ValueError):
    """A brand file that does not parse or a field that breaks its rule (the field is named)."""


def is_host_name(host: str) -> bool:
    """TXT-006: a lower-case host name of two or more labels."""
    labels = host.split(".")
    return len(host) <= 253 and len(labels) >= 2 and all(LABEL.match(label) for label in labels)


def sender_on_domain(address: str, domain: str) -> bool:
    """SEC-025: the sender is local@domain or local@<sub>.domain."""
    local, at, host = address.partition("@")
    if not at or not local or re.search(r"[\s@]", local) or not is_host_name(host):
        return False
    return host == domain or host.endswith(f".{domain}")


def statement_prefix_fits(prefix: str) -> bool:
    """FEE-049: 5 to 10 upper-case letters or digits, one letter, and room for '* JOB 1234567'."""
    return bool(PREFIX.match(prefix)) and len(prefix) + len(STATEMENT_JOINER) + JOB_NUMBER_DIGITS <= STATEMENT_MAX


def _shape(data: Any) -> None:
    if not isinstance(data, dict) or set(data) != set(FIELDS):
        raise BrandError("the brand file does not parse: fields must be exactly " + ", ".join(FIELDS))
    if not isinstance(data["es"], dict) or set(data["es"]) != set(ES_FIELDS):
        raise BrandError("the brand file does not parse: es must hold " + ", ".join(ES_FIELDS))


def _filled(data: dict[str, Any]) -> None:
    named = [(f, data[f]) for f in FIELDS if f not in ("es", "forbidden_alone")]
    named += [(f"es.{f}", data["es"][f]) for f in ES_FIELDS]
    for name, value in named:
        if not isinstance(value, str) or not value.strip():
            raise BrandError(f"the brand field {name} is not valid")
    words = data["forbidden_alone"]
    if not isinstance(words, list) or not words or any(not isinstance(w, str) or not w.strip() for w in words):
        raise BrandError("the brand field forbidden_alone is not valid")


def check(data: Any) -> dict[str, Any]:
    """The same checks as Brand::parse in app/core/src/brand.rs; returns the data or raises BrandError."""
    _shape(data)
    _filled(data)
    es = data["es"]
    rules = (
        ("short_name", ":" not in data["short_name"] and len(data["short_name"]) <= SHORT_NAME_MAX),
        ("es.short_name", ":" not in es["short_name"] and len(es["short_name"]) <= SHORT_NAME_MAX),
        ("legal_name", data["display_name"] in data["legal_name"]),
        ("es.legal_name", es["display_name"] in es["legal_name"]),
        ("domain", is_host_name(data["domain"])),
        ("email_from_address", sender_on_domain(data["email_from_address"], data["domain"])),
        ("statement_prefix", statement_prefix_fits(data["statement_prefix"])),
    )
    for name, ok in rules:
        if not ok:
            raise BrandError(f"the brand field {name} is not valid")
    return data


def load(path: Path = BRAND_FILE) -> dict[str, Any]:
    """Parses and checks the brand file (default: app/core/data/brand.json)."""
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as err:
        raise BrandError(f"the brand file does not parse: {err}") from err
    return check(data)


def tokens(data: dict[str, Any], spanish: bool) -> dict[str, str]:
    """The five brand tokens and their values, with the Spanish names when spanish."""
    names = data["es"] if spanish else data
    return {
        "brand": names["display_name"],
        "brand_business": names["business_name"],
        "brand_short": names["short_name"],
        "brand_legal": names["legal_name"],
        "domain": data["domain"],
    }


def fill(text: str, spanish: bool = False, data: dict[str, Any] | None = None) -> str:
    """Replaces the brand tokens in text (WORD-008, TXT-006); other {name} placeholders stay.

    data is a checked brand (load()); the default is app/core/data/brand.json.
    """
    return fill_tokens(text, tokens(data if data is not None else load(), spanish))


def fill_tokens(text: str, values: dict[str, str]) -> str:
    """Replaces each {token} named in values; an unknown {brand...} token raises BrandError."""

    def one(match: re.Match[str]) -> str:
        name = match.group(1)
        if name in values:
            return values[name]
        if name.startswith("brand"):
            raise BrandError(f"unknown brand token {{{name}}}")
        return match.group(0)

    return TOKEN.sub(one, text)


if __name__ == "__main__":
    print(load()["display_name"])
    sys.exit(0)
