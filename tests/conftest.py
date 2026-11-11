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


@pytest.fixture(autouse=True)
def _no_live_smoke_job(monkeypatch: pytest.MonkeyPatch) -> None:
    """The session step sets RUNNER_SMOKE_BACKGROUND, the background smoke's child RUNNER_PRE_SMOKE_HEAD, the
    pre-smoke step RUNNER_PRE_SMOKE_STATE, and Actions GITHUB_RUN_ID and GITHUB_RUN_ATTEMPT; the smoke runs this suite with them, so the smoke tests would judge the live job's own smoke files
    instead of their fixtures (33 red on 2b62f938, D-2660). Tests start without them and set them where they test
    them, as for the tokens above (D-510)."""
    for name in ("RUNNER_SMOKE_BACKGROUND", "RUNNER_PRE_SMOKE_HEAD", "RUNNER_PRE_SMOKE_STATE",
                 "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"):
        monkeypatch.delenv(name, raising=False)
