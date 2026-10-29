"""Puts the repository root and scripts/ on sys.path for the K8 harness tests."""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
for entry in (ROOT, ROOT / "scripts"):
    if str(entry) not in sys.path:
        sys.path.insert(0, str(entry))

import pytest


@pytest.fixture(autouse=True)
def _no_live_runner_tokens(monkeypatch: pytest.MonkeyPatch) -> None:
    """The backup job sets RUNNER_PUSH_TOKEN and RUNNER_CHECKS_TOKEN for the live runner; the smoke
    runs this suite inside that job, so tests start without them and set them only where they test
    them (D-510)."""
    monkeypatch.delenv("RUNNER_PUSH_TOKEN", raising=False)
    monkeypatch.delenv("RUNNER_CHECKS_TOKEN", raising=False)
