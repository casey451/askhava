"""Read the session's own plan usage for the runner's pacing (PACING_QUOTA_CMD candidate, D-486).

Claude Code shows plan usage with its `/usage` command; that figure comes from the account's
OAuth usage endpoint. This probe asks the same endpoint with the session's own OAuth token (read
from Claude Code's credentials file, never printed) and prints the shape scripts/pacing.py expects:

    {"five_hour": {"utilization": 0.42, "resets_at": "..."}, "week": {"utilization": 0.18, "resets_at": "..."}}

Utilisation is 0..1. On any failure it prints nothing and exits 1, so the runner falls back to its
own ledger (state.usage.source = "ledger"). **Unverified until the D-486 rehearsal:** the endpoint,
header and field names below are the ones Claude Code and the public usage monitors use as of
2026; if the rehearsal shows a different shape, fix this file, not pacing.py. It never touches a
vendor API or a product key, and the token leaves this process only in the request header.

Usage: python3 scripts/quota_probe.py   (set PACING_QUOTA_CMD="python3 scripts/quota_probe.py")
"""

from __future__ import annotations

import json
import os
import sys
import urllib.request
from pathlib import Path

ENDPOINT = "https://api.anthropic.com/api/oauth/usage"
CREDENTIALS = Path.home() / ".claude" / ".credentials.json"
FIELDS = {"five_hour": "five_hour", "week": "seven_day"}


def _token() -> str | None:
    """The OAuth access token from Claude Code's credentials file, or the CLAUDE_CODE_OAUTH_TOKEN variable."""
    env = os.environ.get("CLAUDE_CODE_OAUTH_TOKEN")
    if env:
        return env
    try:
        data = json.loads(CREDENTIALS.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    oauth = data.get("claudeAiOauth") or {}
    token = oauth.get("accessToken")
    return token if isinstance(token, str) and token else None


def _fetch(token: str) -> dict:
    request = urllib.request.Request(
        ENDPOINT,
        headers={
            "Authorization": f"Bearer {token}",
            "anthropic-beta": "oauth-2025-04-20",
            "Accept": "application/json",
            "User-Agent": "askhava-runner-pacing/1",
        },
    )
    with urllib.request.urlopen(request, timeout=20) as response:  # noqa: S310 (fixed https URL)
        return json.loads(response.read().decode("utf-8"))


def to_pacing_shape(raw: dict) -> dict | None:
    """Map the endpoint's windows onto pacing.py's two windows; None when a window is missing."""
    out = {}
    for ours, theirs in FIELDS.items():
        window = raw.get(theirs)
        if not isinstance(window, dict) or "utilization" not in window:
            return None
        utilisation = float(window["utilization"])
        if utilisation > 1.0:  # the endpoint reports percent
            utilisation /= 100.0
        out[ours] = {"utilization": utilisation, "resets_at": window.get("resets_at")}
    return out


def main() -> int:
    token = _token()
    if not token:
        return 1
    try:
        shaped = to_pacing_shape(_fetch(token))
    except Exception:  # noqa: BLE001 (any failure means: use the ledger)
        return 1
    if shaped is None:
        return 1
    print(json.dumps(shaped))
    return 0


if __name__ == "__main__":
    sys.exit(main())
