"""Deterministic stand-in AI (AI-005, AI-006, AI-009, AI-010, AI-011, AI-022, AI-023).

Takes only ScrubbedText, returns only the AI-010 intake fields with a 0-to-1 confidence for
trade and for job type, validated against the committed schema `schemas/intake_reading.json`
before it is returned; anything else is discarded. It has no tools: a request that offers any
is refused. It is plain keyword rules, so the same text always gives the same reading.

Over HTTP (`stand_ins/server.py`) it answers the Claude Messages API: POST /v1/messages and
/v1/messages/count_tokens. The header `x-stand-in-fault` picks a fault (500, 504, 529, 429 with
or without `retry-after`, 400 spend limit, 401, 403, a `refusal` or `max_tokens` stop) and
`x-stand-in-plant` a planted output (a carrier-restricted word, a URL, 600 characters), so
TD-AI-14 and TD-AI-10 run against it (D-493).
"""

from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path
from typing import Any

SCHEMA_PATH = Path(__file__).resolve().parent.parent / "schemas" / "intake_reading.json"
SCHEMA: dict[str, Any] = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
FIELDS = tuple(SCHEMA["properties"])
TRADES = ("plumbing", "heating_ac", "electrical", "handyman", "other")
KEYWORDS = {
    "plumbing": ("leak", "toilet", "drain", "faucet", "pipe", "sink"),
    "heating_ac": ("ac ", "a/c", "air condition", "furnace", "heat", "thermostat"),
    "electrical": ("outlet", "breaker", "wiring", "light switch", "sparks"),
    "handyman": ("patch", "paint", "hang", "shelf", "door"),
}
EMERGENCY = ("gas", "smoke", "fire", "flooding", "sparks", "burning")
SPANISH = ("hola", "necesito", "tengo", "fuga", "aire", "gracias")
JSON_TYPES: dict[str, Any] = {"string": str, "boolean": bool, "array": list, "number": (int, float)}
FAULTS: dict[str, tuple[int, str, str]] = {
    "500": (500, "api_error", "Internal server error"),
    "504": (504, "timeout_error", "Request timed out"),
    "529": (529, "overloaded_error", "Overloaded"),
    "429": (429, "rate_limit_error", "Number of requests has exceeded your rate limit"),
    "429-retry-after": (429, "rate_limit_error", "Number of requests has exceeded your rate limit"),
    "400-spend-limit": (400, "invalid_request_error", "Your credit balance is too low to access the API."),
    "401": (401, "authentication_error", "invalid x-api-key"),
    "403": (403, "permission_error", "Your API key does not have permission to use the specified resource."),
}
PLANTS = {
    "restricted-word": "Ask Hava: Your plumber also sells cannabis and tobacco.",
    "url": "Ask Hava: See https://example.com/offer for details.",
    "long": "Ask Hava: " + "x" * 590,
}
Answer = tuple[int, dict[str, Any], dict[str, str]]


class ScrubbedText(str):
    """Text that has passed the scrubber; the only input the AI adapter accepts (AI-001)."""


class SchemaError(ValueError):
    """The reading does not match the schema; the caller discards it (AI-005)."""


def _check_value(name: str, rule: dict[str, Any], value: Any) -> None:
    kind = JSON_TYPES[rule["type"]]
    if not isinstance(value, kind) or (rule["type"] != "boolean" and isinstance(value, bool)):
        raise SchemaError(f"{name} has the wrong type")
    if "enum" in rule and value not in rule["enum"]:
        raise SchemaError(f"{name} is outside the allowed list")
    if not rule.get("minimum", value) <= value <= rule.get("maximum", value):
        raise SchemaError(f"{name} is out of range")
    if "items" in rule:
        for item in value:
            _check_value(name, rule["items"], item)


def validate(reading: Any) -> dict[str, Any]:
    """Check a reading against `schemas/intake_reading.json` and the trade list (AI-005, AI-022)."""
    if not isinstance(reading, dict) or set(reading) != set(SCHEMA["required"]):
        raise SchemaError("fields differ from the schema")
    for name, rule in SCHEMA["properties"].items():
        _check_value(name, rule, reading[name])
    if reading["trade"] not in TRADES:
        raise SchemaError("trade outside the allowed list")
    return dict(reading)


def read_request(text: ScrubbedText, tools: list[Any] | None = None) -> dict[str, Any]:
    """Read a scrubbed homeowner request. Refuses plain text and any tool offer (AI-006, AI-010)."""
    if not isinstance(text, ScrubbedText):
        raise TypeError("the AI takes ScrubbedText only")
    if tools:
        raise ValueError("the AI has no tools")
    lower = f" {text.lower()} "
    trade = next((t for t, words in KEYWORDS.items() if any(w in lower for w in words)), "other")
    known = trade != "other"
    reading = {
        "trade": trade,
        "trade_confidence": 0.9 if known else 0.2,
        "job_type": trade if known else "unknown",
        "job_type_confidence": 0.8 if known else 0.1,
        "emergency": any(re.search(rf"\b{w}", lower) for w in EMERGENCY),
        "urgent": "today" in lower or "asap" in lower or "urgent" in lower,
        "language": "es" if any(w in lower for w in SPANISH) else "en",
        "major": "replace" in lower or "install" in lower or "remodel" in lower,
        "missing_info": [] if known else ["trade"],
    }
    return validate(reading)


def api_error(status: int, kind: str, message: str) -> Answer:
    return status, {"type": "error", "error": {"type": kind, "message": message}}, {}


def _user_text(body: dict[str, Any]) -> str:
    content = body["messages"][-1]["content"]
    if isinstance(content, str):
        return content
    return "".join(part.get("text", "") for part in content if part.get("type") == "text")


def _reply(body: dict[str, Any], text: str, stop: str) -> Answer:
    usage = {"input_tokens": len(_user_text(body)) // 4 + 1, "output_tokens": len(text) // 4 + 1}
    message = {
        "id": "msg_" + hashlib.sha256(text.encode()).hexdigest()[:24],
        "type": "message",
        "role": "assistant",
        "model": body.get("model", ""),
        "content": [{"type": "text", "text": text}] if text else [],
        "stop_reason": stop,
        "stop_sequence": None,
        "usage": usage,
    }
    return 200, message, {}


def messages(body: dict[str, Any], fault: str = "", plant: str = "") -> Answer:
    """POST /v1/messages: a structured request (output_config) gets the intake reading as JSON
    text; any other request gets its own text back. Faults and plants come first (AI-023, AI-021)."""
    if fault in FAULTS:
        status, kind, message = FAULTS[fault]
        headers = {"retry-after": "3"} if fault == "429-retry-after" else {}
        return status, api_error(status, kind, message)[1], headers
    if not isinstance(body.get("messages"), list) or not body["messages"] or "max_tokens" not in body:
        return api_error(400, "invalid_request_error", "messages and max_tokens are required")
    if body.get("tools"):
        return api_error(400, "invalid_request_error", "the stand-in AI has no tools")
    text = _user_text(body)
    if body.get("output_config"):
        text = json.dumps(read_request(ScrubbedText(text)), sort_keys=True)
    if fault == "refusal":
        return _reply(body, "", "refusal")
    if fault == "max_tokens":
        return _reply(body, text[: len(text) // 2], "max_tokens")
    return _reply(body, PLANTS.get(plant, text), "end_turn")


def count_tokens(body: dict[str, Any]) -> Answer:
    """POST /v1/messages/count_tokens."""
    if not isinstance(body.get("messages"), list) or not body["messages"]:
        return api_error(400, "invalid_request_error", "messages is required")
    return 200, {"input_tokens": len(_user_text(body)) // 4 + 1}, {}
