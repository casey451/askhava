"""Read the session's own plan usage for the runner's pacing (PACING_QUOTA_CMD candidate, D-486).

Claude Code shows plan usage with its `/usage` command; that figure comes from the account's
OAuth usage endpoint. This probe asks the same endpoint with the session's own OAuth token (read
from Claude Code's credentials file, never printed) and prints the shape scripts/pacing.py expects:

    {"five_hour": {"utilization": 0.42, "resets_at": "..."}, "week": {"utilization": 0.18, "resets_at": "..."}}

Utilisation is printed as 0..1 from the endpoint's percent. On any failure it prints one `quota_probe: <why>` line on stderr (never the token) and exits 1,
so the runner falls back to its own ledger (state.usage.source = "ledger") and the pacing row says why (D-1128). **Unverified until the D-486 rehearsal:** the endpoint,
header and field names below are the ones Claude Code and the public usage monitors use as of
2026; if the rehearsal shows a different shape, fix this file, not pacing.py. It never touches a
vendor API or a product key, and the token leaves this process only in the request header.

Usage: python3 scripts/quota_probe.py   (set PACING_QUOTA_CMD="python3 scripts/quota_probe.py")
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
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
        # The endpoint reports percent (0 to 100), as Claude Code's /usage shows it; divide always, so a
        # reading of 1 means 1%, never a full window. The rehearsal confirms the unit (D-486).
        utilisation = float(window["utilization"]) / 100.0
        out[ours] = {"utilization": utilisation, "resets_at": window.get("resets_at")}
    return out


def _note(text: str) -> int:
    """Why the probe gave no reading, on stderr for the pacing row (D-1128); never the token or the body."""
    print(f"quota_probe: {text}", file=sys.stderr)
    return 1


def main() -> int:
    token = _token()
    if not token:
        return _note("no token: CLAUDE_CODE_OAUTH_TOKEN unset and no credentials file")
    try:
        raw = _fetch(token)
    except urllib.error.HTTPError as error:
        return _note(f"HTTP {error.code} from the usage endpoint")
    except Exception as error:  # noqa: BLE001 (any failure means: use the ledger)
        return _note(f"{type(error).__name__} calling the usage endpoint")
    shaped = to_pacing_shape(raw)
    if shaped is None:
        keys = ", ".join(sorted(k for k in raw)) if isinstance(raw, dict) else type(raw).__name__
        return _note(f"reply has no five_hour and seven_day windows with utilization (top-level keys: {keys})")
    print(json.dumps(shaped))
    return 0


if __name__ == "__main__":
    sys.exit(main())
