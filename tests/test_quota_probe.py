"""Tests for scripts/quota_probe.py (the pacing quota reading, D-486): shape mapping only, no network."""

from __future__ import annotations

import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import quota_probe as qp  # noqa: E402


def test_percent_and_fraction_both_map_to_fractions() -> None:
    raw = {"five_hour": {"utilization": 42, "resets_at": "2026-11-01T00:00:00Z"}, "seven_day": {"utilization": 0.18, "resets_at": "x"}}
    out = qp.to_pacing_shape(raw)
    assert out == {
        "five_hour": {"utilization": 0.42, "resets_at": "2026-11-01T00:00:00Z"},
        "week": {"utilization": 0.18, "resets_at": "x"},
    }


def test_missing_window_means_no_reading() -> None:
    assert qp.to_pacing_shape({"five_hour": {"utilization": 1}}) is None
    assert qp.to_pacing_shape({}) is None


def test_no_token_exits_one(monkeypatch, tmp_path: Path) -> None:
    monkeypatch.delenv("CLAUDE_CODE_OAUTH_TOKEN", raising=False)
    monkeypatch.setattr(qp, "CREDENTIALS", tmp_path / "missing.json")
    assert qp.main() == 1
