"""Offline checks of the route benchmark (T417, TD-OPS-42, D-3292): planted timings, snapshots and
records; no app, no database. Non-fake values are assembled at run time, never stored."""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from perf import bench

BOUNDS = [100, 250, 500, 1_000, 2_500, 5_000, 10_000, 25_000, 50_000, 100_000, 250_000, 500_000,
          1_000_000, 2_500_000, 5_000_000, None]


def snapshot(counts: dict[int | None, int]) -> dict:
    return {"statements": 0, "buckets": [{"le_us": b, "count": counts.get(b, 0)} for b in BOUNDS]}


def test_td_ops_42_nearest_rank_of_1_to_100_ms() -> None:
    timings = [float(ms) for ms in range(100, 0, -1)]
    assert [bench.nearest_rank(timings, p) for p in (50, 95, 99)] == [50.0, 95.0, 99.0]


def test_td_ops_42_acquire_p95_from_bucket_differences() -> None:
    before = snapshot({100: 500, 1_000: 7})
    after = snapshot({100: 590, 1_000: 12, 25_000: 5})
    # 90 + 5 + 5 = 100 new acquires; the 95th falls in the 1 ms bucket, the slowest at 25 ms.
    assert bench.bucket_p95(before, after) == 1_000
    assert bench.bucket_max(before, after) == 25_000
    assert bench.bucket_p95(after, snapshot({100: 590, 1_000: 12, 25_000: 5, None: 6})) == ">5000000"
    assert bench.bucket_p95(after, after).startswith("not exposed")


def test_td_ops_42_a_404_is_not_an_error_a_500_and_a_timeout_are() -> None:
    assert not bench.is_error(404)
    assert bench.is_error(500)
    assert bench.is_error(None)
    assert bench.error_rate([200, 404, 500, None]) == 0.5


@pytest.mark.parametrize(("route", "goal"), [("zip_lookup", 100.0), ("matching", 250.0)])
def test_td_ops_42_latency_goals_met_at_the_edge_missed_just_over(route: str, goal: float) -> None:
    at_edge = bench.summarize(route, [(goal, 200, 10)] * 20, {})
    over = bench.summarize(route, [(goal + 0.01, 200, 10)] * 20, {})
    name = f"p95_ms_goal_{goal:g}"
    assert at_edge["verdicts"][name] == "met"
    assert over["verdicts"][name] == "missed"


def test_td_ops_42_error_goal_met_at_the_edge_missed_just_over_and_a_miss_never_fails() -> None:
    edge = [(1.0, 200, 10)] * 999 + [(1.0, 500, 10)]
    over = [(1.0, 200, 10)] * 998 + [(1.0, 500, 10), (1.0, None, 0)]
    assert bench.summarize("home", edge, {})["verdicts"]["error_rate_under_0.1%"] == "met"
    assert bench.summarize("home", over, {})["verdicts"]["error_rate_under_0.1%"] == "missed"
    assert bench.verdict("not exposed: no route", 100.0) == "not measurable"


def _street() -> str:
    return "12 " + "Ma" + "ple Ave"


@pytest.mark.parametrize(
    "planted",
    [lambda: "+1602" + "555" + "1234", lambda: "pat" + "@" + "gmail.com", _street],
    ids=["phone", "email", "street"],
)
def test_td_ops_42_a_non_fake_record_is_refused_before_it_is_written(tmp_path: Path, planted) -> None:
    out = tmp_path / "run.json"
    with pytest.raises(ValueError, match="not fake data"):
        bench.write_record(out, {"routes": {"intake_ack": {"note": planted()}}})
    assert not out.exists()


def test_td_ops_42_a_fake_record_is_written(tmp_path: Path) -> None:
    out = tmp_path / "run.json"
    bench.write_record(out, {"from": "+16025550142", "mail": "owner@example.com", "at": "1 Test Way"})
    assert out.exists()
