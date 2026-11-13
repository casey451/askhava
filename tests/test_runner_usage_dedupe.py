"""Tests for T438 (D-3475, D-3536): transcript usage counts each reply once, new usage.by_task entries are
marked `usage_counting: "message_id"`, and the pacer scales only unmarked, per-line figures by
`pre_dedupe_factor`; the ledger windows stay as they were. Fake data only."""

from __future__ import annotations

import json
from datetime import datetime, timezone
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import check_state as cs
import pacing as pc
import pytest

from runner import cycle, github_env as ge, verify

NOW = datetime(2026, 11, 13, 12, 0, tzinfo=timezone.utc)
CFG: dict[str, Any] = {
    "budget_tokens_five_hour": None, "budget_tokens_week": 10_000_000, "headroom": 0.10, "bytes_per_token": 4,
    "turn_factor": 3.0, "model_multiplier": {"haiku": 1.0, "sonnet": 1.3, "opus": 1.6}, "calibrate_after": 3,
    "weights": {"input_tokens": 1.0, "output_tokens": 1.0, "cache_creation_input_tokens": 1.0, "cache_read_input_tokens": 0.1},
    "window_hours": {"five_hour": 5, "week": 168}, "budget_source": "test",
}
FACTOR_CFG = dict(CFG, pre_dedupe_factor=0.6)
M1 = {"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 20}


def line(usage: dict[str, int] | None, mid: str | None = None) -> str:
    message: dict[str, Any] = {} if usage is None else {"usage": usage}
    if mid is not None:
        message["id"] = mid
    return json.dumps({"type": "assistant", "message": message})


FIXTURE = [
    line(M1, "msg_1"), line(M1, "msg_1"), line(M1, "msg_1"),
    line({"input_tokens": 7, "output_tokens": 1}, "msg_2"), line({"input_tokens": 7, "output_tokens": 9}, "msg_2"),
    line({"input_tokens": 3, "output_tokens": 2}),
    line(None, "msg_3"),
    "{not json",
    "[1, 2]",
]
PER_REPLY = {"input_tokens": 20, "output_tokens": 16, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 20}


def test_each_reply_counts_once_and_the_last_line_of_an_id_wins() -> None:
    assert verify.usage_in_transcript(FIXTURE) == PER_REPLY
    assert verify.usage_in_transcript([]) == dict.fromkeys(verify.USAGE_FIELDS, 0)


def test_an_id_less_line_counts_on_its_own_each_time() -> None:
    anon = line({"output_tokens": 4})
    assert verify.usage_in_transcript([anon, anon])["output_tokens"] == 8


def test_read_transcripts_and_role_reading_record_deduped_figures(tmp_path: Path) -> None:
    paths = []
    for name in ("worker.jsonl", "reviewer.jsonl"):
        (tmp_path / name).write_text("\n".join(FIXTURE) + "\n", encoding="utf-8")
        paths.append(str(tmp_path / name))
    _, totals = ge.read_transcripts(paths)
    assert totals == {k: 2 * v for k, v in PER_REPLY.items()}
    assert ge.role_reading(paths, totals) == {"tokens": 2 * sum(PER_REPLY.values()), "turns": 6}


def result(**usage: int) -> cycle.WorkerResult:
    split = {"worker": {"tokens": 30, "turns": 2}}
    return cycle.WorkerResult("submitted", usage=usage or {"output_tokens": 42}, usage_split=split)


def recorded(by_task: dict[str, Any], on_usage: Any = None) -> dict[str, Any]:
    state = {"tasks": [{"id": "T001"}], "usage": {"by_task": by_task}}
    out = cycle._record_usage(state, "T001", "claude-opus-5-5", result(), NOW, SimpleNamespace(on_usage=on_usage))
    return out["usage"]["by_task"]["T001"]


def test_a_new_entry_is_marked_with_or_without_the_pacer() -> None:
    seeded = {"T001": {"estimated_bytes": 9_000, "estimated_tokens": 30_000}}
    assert recorded(json.loads(json.dumps(seeded)))["usage_counting"] == "message_id"
    assert recorded({})["usage_counting"] == "message_id"
    with_pacer = recorded(json.loads(json.dumps(seeded)), pc.usage_recorder(config=FACTOR_CFG))
    assert with_pacer["usage_counting"] == "message_id" and with_pacer["estimated_tokens"] == 30_000
    assert with_pacer["output_tokens"] == 42


def test_an_entry_with_earlier_tokens_stays_unmarked_and_turns_are_never_scaled() -> None:
    entry = recorded({"T001": {"estimated_bytes": 9_000, "output_tokens": 100, "worker_turns": 3, "worker_tokens": 7}})
    assert "usage_counting" not in entry
    assert (entry["output_tokens"], entry["worker_turns"], entry["worker_tokens"]) == (142, 5, 37)


def calibrated_state(marked: bool) -> dict[str, Any]:
    tasks = [{"id": f"T{i}", "status": "DONE"} for i in range(3)] + [{"id": "T9", "status": "QUEUED"}]
    mark = {"usage_counting": "message_id"} if marked else {}
    by_task = {f"T{i}": {"estimated_bytes": 10_000, "output_tokens": 20_000 * (i + 1), "last_model": "claude-sonnet-5-5", **mark}
               for i in range(3)}
    return {"tasks": tasks, "usage": {"by_task": by_task}}


def test_calibration_scales_only_unmarked_entries() -> None:
    assert pc.calibrated(calibrated_state(False), CFG) == {"sonnet": 4.0}
    assert pc.calibrated(calibrated_state(False), FACTOR_CFG)["sonnet"] == pytest.approx(4.0 * 0.6)
    assert pc.calibrated(calibrated_state(True), FACTOR_CFG) == {"sonnet": 4.0}


def test_projected_finish_falls_by_the_factor_for_a_calibrated_model(tmp_path: Path) -> None:
    (tmp_path / "owned.rs").write_text("x" * 1_000)
    (tmp_path / "prompt.md").write_text("y" * 1_000)
    graph = [{"id": "T9", "model": "claude-sonnet-5-5", "owns": ["owned.rs"], "prompt": "prompt.md"}]
    old, _ = pc.projected_finish(calibrated_state(False), graph, CFG, NOW, tmp_path)
    new, _ = pc.projected_finish(calibrated_state(False), graph, FACTOR_CFG, NOW, tmp_path)
    assert old == 8_000 and new == pytest.approx(old * 0.6, abs=1)
    assert pc.estimate(graph[0], calibrated_state(False), FACTOR_CFG, tmp_path)[2] == "calibrated"


def test_the_ledger_windows_stay_as_today() -> None:
    usage = {"output_tokens": 1000, "cache_read_input_tokens": 10_000}
    first = pc.record_usage({}, usage, NOW, FACTOR_CFG)
    assert first["usage"]["windows"]["five_hour"] == {"start": pc.iso(NOW), "tokens": 2000}
    second = pc.record_usage(first, usage, NOW, FACTOR_CFG)
    assert second["usage"]["windows"]["week"] == {"start": pc.iso(NOW), "tokens": 4000}


def test_check_state_validates_entries_with_and_without_the_marker() -> None:
    base = {"schema": 1, "lease": None, "tasks": [{"id": "T001", "deps": [], "status": "QUEUED", "attempts": 0}]}
    plain = dict(base, usage={"by_task": {"T001": {"output_tokens": 5}}})
    marked = dict(base, usage={"by_task": {"T001": {"output_tokens": 5, "usage_counting": "message_id"}}})
    wrong = dict(base, usage={"by_task": {"T001": {"usage_counting": "per_line"}}})
    assert cs.check(plain) == [] and cs.check(marked) == []
    assert cs.check(wrong) == ["state.usage.by_task.T001.usage_counting: 'per_line' is not one of ['message_id']"]


def test_the_shipped_config_carries_the_factor() -> None:
    cfg = pc.load_config(Path(__file__).resolve().parents[1] / "kit" / "pacing.json")
    assert cfg["pre_dedupe_factor"] == 0.6
