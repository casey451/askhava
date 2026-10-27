"""Deterministic stand-in AI (AI-005, AI-006, AI-009, AI-010, AI-011).

Takes only ScrubbedText, returns only the fixed intake fields (trade, job type, emergency,
urgent, language, rental, major, missing info), validated against the schema before it
is returned; anything else is discarded. It has no tools: a request that offers any is
refused. It is plain keyword rules, so the same text always gives the same reading.
"""

from __future__ import annotations

import re
from typing import Any

TRADES = ("plumbing", "heating_ac", "electrical", "handyman", "other")
FIELDS: dict[str, type] = {
    "trade": str,
    "job_type": str,
    "emergency": bool,
    "urgent": bool,
    "language": str,
    "rental": bool,
    "major": bool,
    "missing_info": list,
}
KEYWORDS = {
    "plumbing": ("leak", "toilet", "drain", "faucet", "pipe", "sink"),
    "heating_ac": ("ac ", "a/c", "air condition", "furnace", "heat", "thermostat"),
    "electrical": ("outlet", "breaker", "wiring", "light switch", "sparks"),
    "handyman": ("patch", "paint", "hang", "shelf", "door"),
}
EMERGENCY = ("gas", "smoke", "fire", "flooding", "sparks", "burning")
SPANISH = ("hola", "necesito", "tengo", "fuga", "aire", "gracias")


class ScrubbedText(str):
    """Text that has passed the scrubber; the only input the AI adapter accepts (AI-001)."""


class SchemaError(ValueError):
    """The reading does not match the schema; the caller discards it (AI-005)."""


def validate(reading: Any) -> dict[str, Any]:
    if not isinstance(reading, dict) or set(reading) != set(FIELDS):
        raise SchemaError("fields differ from the schema")
    for name, kind in FIELDS.items():
        if not isinstance(reading[name], kind):
            raise SchemaError(f"{name} has the wrong type")
    if reading["trade"] not in TRADES or reading["language"] not in ("en", "es"):
        raise SchemaError("value outside the allowed list")
    return dict(reading)


def read_request(text: ScrubbedText, tools: list[Any] | None = None) -> dict[str, Any]:
    """Read a scrubbed homeowner request. Refuses plain text and any tool offer."""
    if not isinstance(text, ScrubbedText):
        raise TypeError("the AI takes ScrubbedText only")
    if tools:
        raise ValueError("the AI has no tools")
    lower = f" {text.lower()} "
    trade = next((t for t, words in KEYWORDS.items() if any(w in lower for w in words)), "other")
    reading = {
        "trade": trade,
        "job_type": trade if trade != "other" else "unknown",
        "emergency": any(re.search(rf"\b{w}", lower) for w in EMERGENCY),
        "urgent": "today" in lower or "asap" in lower or "urgent" in lower,
        "language": "es" if any(w in lower for w in SPANISH) else "en",
        "rental": "rental" in lower or "tenant" in lower,
        "major": "replace" in lower or "install" in lower or "remodel" in lower,
        "missing_info": [] if trade != "other" else ["trade"],
    }
    return validate(reading)
