"""Tests for scripts/pacing.py (pause only before the usage limits, D-486). Fake data only."""

from __future__ import annotations

import json
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

import pacing as pc

NOW = datetime(2026, 11, 1, 12, 0, tzinfo=timezone.utc)
CFG: dict[str, Any] = {
    "budget_tokens_five_hour": 1_000_000, "budget_tokens_week": 10_000_000, "headroom": 0.10, "bytes_per_token": 4,
    "turn_factor": 3.0, "model_multiplier": {"haiku": 1.0, "sonnet": 1.3, "opus": 1.6}, "calibrate_after": 3,
    "weights": {"input_tokens": 1.0, "output_tokens": 1.0, "cache_creation_input_tokens": 1.0, "cache_read_input_tokens": 0.1},
    "window_hours": {"five_hour": 5, "week": 168}, "budget_source": "test",
}


def repo(tmp_path: Path, size: int = 40_000) -> Path:
    (tmp_path / "app" / "fees").mkdir(parents=True)
    (tmp_path / "app" / "fees" / "lib.rs").write_text("x" * size)
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "spec.md").write_text("y" * 2_000)
    (tmp_path / "kit" / "prompts").mkdir(parents=True)
    (tmp_path / "kit" / "prompts" / "T058.md").write_text("z" * 2_000)
    return tmp_path


TASK = {"id": "T058", "owns": ["app/fees/", "app/fees/new.rs"], "reads": ["docs/spec.md"], "model": "claude-sonnet-5-5"}


def test_estimate_is_bytes_over_four_times_multiplier_times_turn_factor(tmp_path: Path) -> None:
    root = repo(tmp_path)
    tokens, size, basis = pc.estimate(TASK, {"tasks": []}, CFG, root)
    assert size == 44_000 and basis == "default"
    assert tokens == int(44_000 / 4 * 1.3 * 3.0)
    opus = dict(TASK, model="claude-opus-5-5")
    assert pc.estimate(opus, {"tasks": []}, CFG, root)[0] == int(44_000 / 4 * 1.6 * 3.0)


def test_paths_outside_the_repository_are_not_read(tmp_path: Path) -> None:
    assert pc.task_bytes({"id": "X", "owns": ["/etc/passwd", "../x"], "prompt": "none.md"}, tmp_path) == 0


def calibrated_state(n: int) -> dict[str, Any]:
    tasks = [{"id": f"T{i}", "status": "DONE"} for i in range(n)]
    by_task = {f"T{i}": {"estimated_bytes": 10_000, "output_tokens": 20_000 * (i + 1), "last_model": "claude-sonnet-5-5"} for i in range(n)}
    return {"tasks": tasks, "usage": {"by_task": by_task}}


def test_calibration_replaces_the_multiplier_after_three_done_tasks(tmp_path: Path) -> None:
    root = repo(tmp_path)
    assert pc.calibrated(calibrated_state(2), CFG) == {}
    assert pc.calibrated(calibrated_state(3), CFG) == {"sonnet": 4.0}  # median of 2, 4, 6 tokens per byte
    tokens, size, basis = pc.estimate(TASK, calibrated_state(3), CFG, root)
    assert basis == "calibrated" and tokens == size * 4


def test_weighted_tokens_count_cache_reads_at_a_tenth() -> None:
    usage = {"input_tokens": 10, "output_tokens": 100, "cache_creation_input_tokens": 1000, "cache_read_input_tokens": 10_000}
    assert pc.weighted(usage, CFG) == 10 + 100 + 1000 + 1000


def test_ledger_windows_add_and_roll_over() -> None:
    state = pc.record_usage({"tasks": []}, {"output_tokens": 500}, NOW, CFG)
    assert state["usage"]["windows"]["five_hour"] == {"start": pc.iso(NOW), "tokens": 500}
    state = pc.record_usage(state, {"output_tokens": 300}, NOW + timedelta(hours=1), CFG)
    assert state["usage"]["windows"]["five_hour"]["tokens"] == 800 and state["usage"]["windows"]["week"]["tokens"] == 800
    later = NOW + timedelta(hours=5, minutes=1)
    state = pc.record_usage(state, {"output_tokens": 100}, later, CFG)
    assert state["usage"]["windows"]["five_hour"] == {"start": pc.iso(later), "tokens": 100}
    assert state["usage"]["windows"]["week"]["tokens"] == 900


def spent(tokens: int, at: datetime = NOW) -> dict[str, Any]:
    return {"tasks": [], "usage": {"windows": {w: {"start": pc.iso(at), "tokens": tokens} for w in pc.WINDOWS}}}


def test_claim_when_it_fits_both_windows(tmp_path: Path) -> None:
    root = repo(tmp_path)
    assert pc.decide(spent(0), TASK, root=root, config=CFG, now=NOW) == ("claim", None)
    need = pc.estimate(TASK, {"tasks": []}, CFG, root)[0]
    assert pc.decide(spent(900_000 - need), TASK, root=root, config=CFG, now=NOW) == ("claim", None)


def test_wait_until_the_reset_that_makes_it_fit(tmp_path: Path) -> None:
    root = repo(tmp_path)
    assert pc.decide(spent(899_000), TASK, root=root, config=CFG, now=NOW) == ("wait", pc.iso(NOW + timedelta(hours=5)))
    week_full = {"tasks": [], "usage": {"windows": {"five_hour": {"start": pc.iso(NOW), "tokens": 0},
                                                    "week": {"start": pc.iso(NOW - timedelta(days=2)), "tokens": 8_999_000}}}}
    assert pc.decide(week_full, TASK, root=root, config=CFG, now=NOW) == ("wait", pc.iso(NOW + timedelta(days=5)))


def test_a_task_too_big_for_an_empty_window_is_deferred_not_held_forever(tmp_path: Path) -> None:
    root = repo(tmp_path, size=2_000_000)
    verdict, reason = pc.decide(spent(0), TASK, root=root, config=CFG, now=NOW)
    assert verdict == "defer" and "exceeds an empty five_hour window" in str(reason)


def test_a_reported_usage_limit_reset_wins_over_the_ledger(tmp_path: Path) -> None:
    reset = pc.iso(NOW + timedelta(hours=2))
    state = dict(spent(0), usage={"limit_hit": {"task": "T057", "reset_at": reset}})
    assert pc.decide(state, TASK, root=repo(tmp_path), config=CFG, now=NOW) == ("wait", reset)
    assert pc.decide(state, TASK, root=tmp_path, config=CFG, now=NOW + timedelta(hours=3))[0] == "claim"


def test_quota_command_reading_wins_and_the_source_is_recorded(tmp_path: Path) -> None:
    script = tmp_path / "quota.py"
    reading = {"five_hour": {"utilization": 0.95, "resets_at": "2026-11-01T14:00:00Z"}, "week": {"utilization": 0.1, "resets_at": None}}
    script.write_text(f"print({json.dumps(json.dumps(reading))})")
    quota = pc.read_quota(f"{sys.executable} {script}")
    assert quota == reading
    found = pc.assess(spent(0), TASK, root=repo(tmp_path), config=CFG, now=NOW, quota=quota)
    assert found["source"] == "quota_cmd" and found["decision"] == ("wait", "2026-11-01T14:00:00Z")
    assert pc.read_quota(f"{sys.executable} -c print(1)") is None and pc.read_quota("") is None


def test_pacer_records_line_estimate_source_and_projection(tmp_path: Path) -> None:
    root = repo(tmp_path)
    graph = [TASK, dict(TASK, id="T059"), {"id": "T127", "type": "gate"}]
    verdict, detail, state = pc.make_pacer(graph, root, CFG)({"tasks": []}, TASK, NOW)
    assert (verdict, detail) == ("claim", None)
    assert state["pacing"]["last"].startswith("T058 estimated ") and state["pacing"]["last"].endswith("; claimed")
    assert "five-hour window has 900k left, week 9.0M" in state["pacing"]["last"]
    assert state["usage"]["source"] == "ledger" and state["usage"]["by_task"]["T058"]["estimated_bytes"] == 44_000
    assert state["pacing"]["remaining_estimate"] > 0 and state["pacing"]["projected_finish"] > pc.iso(NOW)
    assert "projected finish" in pc.status_line(state) and pc.status_line({}) == "Pacing: no reading yet."


def test_projection_uses_the_tighter_sustained_rate(tmp_path: Path) -> None:
    root = repo(tmp_path)
    remaining, finish = pc.projected_finish({"tasks": []}, [TASK], CFG, NOW, root)
    rate = min(900_000 / 5, 9_000_000 / 168)
    assert finish == pc.iso(NOW + timedelta(hours=remaining / rate))


def test_real_config_loads_and_cli_runs(capsys: Any) -> None:
    cfg = pc.load_config()
    assert cfg["headroom"] == 0.10 and set(cfg["model_multiplier"]) == {"haiku", "sonnet", "opus"}
    assert pc.main(["--state", "/nonexistent.json", "--task", "T058"]) == 0
    assert "projected finish" in capsys.readouterr().out


def test_spend_line_holds_the_week_to_a_straight_line_to_the_reset(tmp_path: Path) -> None:
    """D-1128: from the anchor reading to the effective week budget a few hours before the reset."""
    root = repo(tmp_path)
    need = pc.estimate(TASK, {"tasks": []}, CFG, root)[0]
    cfg = dict(CFG, spread={"anchor_at": pc.iso(NOW), "anchor_used_week": 1_000_000, "finish_hours_before_reset": 3})
    week_start = NOW - timedelta(days=2)  # resets in 5 days; the line ends 3 hours before that
    state = {"tasks": [], "usage": {"windows": {"five_hour": {"start": pc.iso(NOW), "tokens": 0},
                                                "week": {"start": pc.iso(week_start), "tokens": 1_000_000}}}}
    found = pc.assess(state, TASK, root=root, config=cfg, now=NOW)
    verdict, until = found["decision"]
    assert verdict == "wait" and found["spend_line"] == until and "spend line" in pc.line("T001", found)
    line_len = timedelta(days=5) - timedelta(hours=3)
    expected = NOW + line_len * (need / (9_000_000 - 1_000_000))
    assert abs((pc.parse(until) - expected).total_seconds()) < 2
    assert pc.decide(state, TASK, root=root, config=cfg, now=pc.parse(until) + timedelta(seconds=1)) == ("claim", None)
    far = pc.decide(dict(state, usage={"windows": {"five_hour": {"start": pc.iso(NOW), "tokens": 0},
                                                   "week": {"start": pc.iso(week_start), "tokens": 8_500_000}}}),
                    TASK, root=root, config=cfg, now=NOW + timedelta(days=1))
    assert far[0] == "wait" and NOW + timedelta(days=4) < pc.parse(far[1]) <= week_start + timedelta(days=7) - timedelta(hours=3)
    assert pc.decide(state, TASK, root=root, config=cfg, now=NOW - timedelta(hours=1)) == ("claim", None), "before the anchor"
    assert pc.decide(state, TASK, root=root, config=CFG, now=NOW) == ("claim", None), "no spread configured"
    assert pc.decide(spent(899_000), TASK, root=root, config=cfg, now=NOW) == ("wait", pc.iso(NOW + timedelta(hours=5))), "the five-hour wait still wins"


def test_a_failing_probe_says_why_in_the_source(tmp_path: Path) -> None:
    """D-1128: the pacing row names the probe's last stderr line, so a silent fallback can be diagnosed."""
    probe = tmp_path / "probe.py"
    probe.write_text("import sys; print('quota_probe: HTTP 403 from the usage endpoint', file=sys.stderr); sys.exit(1)\n", encoding="utf-8")
    assert pc.read_quota(f"{sys.executable} {probe}") is None
    stand, source = pc.standing(spent(0), CFG, NOW, None)
    assert source == "ledger; quota_probe: HTTP 403 from the usage endpoint" and stand["week"]["used"] == 0
    probe.write_text("print('{\"five_hour\": {\"utilization\": 0.5}, \"week\": {\"utilization\": 0.25}}')\n", encoding="utf-8")
    reading = pc.read_quota(f"{sys.executable} {probe}")
    assert reading and pc.standing(spent(0), CFG, NOW, reading)[1] == "quota_cmd"
    assert pc.read_quota("") is None and pc.standing(spent(0), CFG, NOW, None)[1] == "ledger"
