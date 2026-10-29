"""Shared helpers: deterministic IDs and the idempotency-key store."""

from __future__ import annotations

import copy
import hashlib
import json
from typing import Any

Reply = tuple[int, dict[str, Any]]


def fake_id(prefix: str, n: int, length: int = 24, alphabet: str = "hex") -> str:
    digest = hashlib.sha256(f"{prefix}:{n}".encode()).hexdigest()
    body = digest[:length]
    return prefix + (body if alphabet == "hex" else body.upper())


class IdempotencyStore:
    """Replays a stored reply for a repeated key; flags the same key with different input."""

    def __init__(self) -> None:
        self._seen: dict[str, tuple[str, Reply]] = {}

    @staticmethod
    def _fingerprint(params: Any) -> str:
        return json.dumps(params, sort_keys=True, default=str)

    def lookup(self, key: str | None, params: Any) -> tuple[str, Reply | None]:
        """Returns ('new', None), ('replay', reply) or ('conflict', None)."""
        if key is None or key not in self._seen:
            return "new", None
        fingerprint, reply = self._seen[key]
        if fingerprint != self._fingerprint(params):
            return "conflict", None
        return "replay", copy.deepcopy(reply)

    def store(self, key: str | None, params: Any, reply: Reply) -> Reply:
        if key is not None:
            self._seen[key] = (self._fingerprint(params), copy.deepcopy(reply))
        return reply
