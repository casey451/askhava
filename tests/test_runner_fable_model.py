"""Fable as a model the plan can name (T753, D-4761, D-4766 to D-4768).

A task's first attempt runs on its own model; a task that failed once runs on Fable when the `models` map
names Fable (Opus without one, D-2474); a Fable refusal or usage limit moves the attempt to Opus without
counting an attempt, and later attempts stay on Opus; a model window mark still wins (D-2225); and
scripts/validate_tasks.py accepts every value of the `models` map, Fable counting as Opus.
"""

from __future__ import annotations

import copy
import dataclasses
from datetime import timedelta
from pathlib import Path
from typing import Any

import check_state as cs
from runner import cycle, model_window as mw, state as st
from test_failure_drills import MODELS, NOW, FakeEnv, config, good, make_task
from test_validate_tasks import OPUS, assert_error, errors_for, task, valid_tasks, vt

# A fixture ID: the tests use their own models map and never depend on the plan's Fable entry. It is the
# ID D-4766 confirmed from the repository (docs/handoff/2026-11-10-early.md:71 and the commit trailers).
FABLE = "claude-fable-5-2"
WITH_FABLE = {**MODELS, "fable": FABLE}
REFUSAL = f"There's an issue with the selected model ({FABLE}). It may not exist or you may not have access to it."


def fable_config(graph: list[dict[str, Any]], tmp_path: Path) -> cycle.Config:
    return dataclasses.replace(config(graph, tmp_path), models=WITH_FABLE)


# ---- attempt_model and worker_model


def test_a_first_attempt_keeps_the_task_model() -> None:
    for model in MODELS.values():
        assert cycle.attempt_model(make_task("T001", model=model), 0, WITH_FABLE) == model
    assert cycle.attempt_model(make_task("T001", model=FABLE), 0, WITH_FABLE) == FABLE


def test_a_failed_once_task_goes_to_fable_and_to_opus_without_a_fable_entry() -> None:
    for model in [*MODELS.values(), FABLE]:
        tsk = make_task("T001", model=model)
        assert [cycle.attempt_model(tsk, n, WITH_FABLE) for n in (1, 2)] == [FABLE, FABLE]
    sonnet = make_task("T001")
    assert [cycle.attempt_model(sonnet, n, MODELS) for n in range(3)] == [MODELS["sonnet"], MODELS["opus"], MODELS["opus"]]


def test_a_fable_fallback_keeps_every_later_attempt_on_opus() -> None:
    off = make_task("T001", model=FABLE, **{cycle.FABLE_OFF: {"at": st.iso(NOW), "why": "usage limit"}})
    assert [cycle.attempt_model(off, n, WITH_FABLE) for n in range(3)] == [MODELS["opus"]] * 3
    assert cycle.attempt_model(dict(off, model=MODELS["sonnet"]), 0, WITH_FABLE) == MODELS["sonnet"]


def test_a_window_mark_still_wins() -> None:
    mark = {"model": "claude-opus-5-5", "alias": "opus"}
    tsk = make_task("T001", **{mw.MARK: mark})
    assert cycle.worker_model(tsk, 1, WITH_FABLE) == mark["model"]
    windowed_fable = make_task("T001", **{mw.MARK: {"model": FABLE, "alias": "fable"}, cycle.FABLE_OFF: {"at": "x"}})
    assert cycle.worker_model(windowed_fable, 1, WITH_FABLE) == FABLE


def test_the_cycle_retries_a_failed_task_on_fable(tmp_path: Path) -> None:
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("tests_failed", log="1 failed"))
    cfg = fable_config([make_task("T001")], tmp_path)
    for minute in range(3):
        cycle.run_cycle(cfg, env, NOW + timedelta(minutes=minute))
    assert [r[1] for r in env.runs] == [MODELS["sonnet"], FABLE, FABLE]
    assert st.status(env.store, "T001") == st.BLOCKED


# ---- the fallback to Opus: a usage limit


def _failed_once(tmp_path: Path) -> tuple[FakeEnv, cycle.Config]:
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("tests_failed", log="1 failed"))
    cfg = fable_config([make_task("T001")], tmp_path)
    cycle.run_cycle(cfg, env, NOW)
    assert st.entry(env.store, "T001")["attempts"] == 1
    return env, cfg


def _limit_moves_to_opus(tmp_path: Path, res: cycle.WorkerResult) -> None:
    env, cfg = _failed_once(tmp_path)
    env.worker = lambda t, m, log: res
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=1)) == st.QUEUED
    entry = st.entry(env.store, "T001")
    assert env.runs[-1][1] == FABLE and entry["attempts"] == 1 and "limit_hit" not in (env.store.get("usage") or {})
    assert entry[cycle.FABLE_OFF]["at"] == st.iso(NOW + timedelta(minutes=1))
    assert cs.check(env.store) == []
    env.worker = lambda t, m, log: cycle.WorkerResult("tests_failed", log="1 failed")
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=2))
    assert env.runs[-1][1] == MODELS["opus"] and st.entry(env.store, "T001")["attempts"] == 2
    env.worker = lambda t, m, log: good(t, m)
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=3)) == st.MERGE_PENDING
    assert env.runs[-1][1] == MODELS["opus"]


def test_a_fable_usage_limit_reruns_on_opus_without_counting_an_attempt(tmp_path: Path) -> None:
    _limit_moves_to_opus(tmp_path, cycle.WorkerResult("usage_limit", reset_at=NOW + timedelta(hours=5)))


def test_a_fable_rate_limit_death_reruns_on_opus_too(tmp_path: Path) -> None:
    _limit_moves_to_opus(tmp_path, cycle.WorkerResult("api_error", log="API Error: 429 rate_limit_error"))


def test_an_opus_usage_limit_still_waits_for_the_reset(tmp_path: Path) -> None:
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("usage_limit", reset_at=NOW + timedelta(hours=5)))
    cfg = fable_config([make_task("T001", model=MODELS["opus"])], tmp_path)
    assert cycle.run_cycle(cfg, env, NOW) == st.RETRY_AFTER_RESET
    assert cycle.FABLE_OFF not in st.entry(env.store, "T001")


def test_another_fable_failure_counts_an_attempt(tmp_path: Path) -> None:
    env, cfg = _failed_once(tmp_path)
    env.worker = lambda t, m, log: cycle.WorkerResult("api_error", log="API Error: 500 api_error")
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=1))
    assert cycle.FABLE_OFF not in st.entry(env.store, "T001")


# ---- the fallback to Opus: a refusal handed back with --model-fallback


def _claimed_on_fable(tmp_path: Path) -> tuple[FakeEnv, cycle.Config]:
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("handed_off"))
    cfg = fable_config([make_task("T001", model=FABLE)], tmp_path)
    assert cycle.run_cycle(cfg, env, NOW) == "handed_off"
    assert env.runs[-1][1] == FABLE
    return env, cfg


def test_a_fable_refusal_moves_the_attempt_to_opus_with_the_count_unchanged(tmp_path: Path) -> None:
    env, cfg = _claimed_on_fable(tmp_path)
    line = cycle.model_fallback(cfg, env, REFUSAL)
    assert line.startswith(f"fable: {FABLE} refused for T001")
    first = copy.deepcopy(st.entry(env.store, "T001")[cycle.FABLE_OFF])
    assert first["why"].startswith("refused: ")
    cycle.model_fallback(cfg, env, REFUSAL)
    assert st.entry(env.store, "T001")[cycle.FABLE_OFF] == first  # recorded once
    res = cycle.WorkerResult("submitted", ["app/t001/x.py"], [MODELS["opus"]])
    assert cycle.after_handoff(cfg, env, lambda t: res, own_models=True) == st.MERGE_PENDING
    entry = st.entry(env.store, "T001")
    assert entry["attempts"] == 0 and entry["history"][-1]["model"] == MODELS["opus"]
    assert cs.check(env.store) == []


def test_a_refusal_for_an_attempt_not_on_fable_changes_nothing(tmp_path: Path) -> None:
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("handed_off"))
    cfg = fable_config([make_task("T001")], tmp_path)
    cycle.run_cycle(cfg, env, NOW)
    before = copy.deepcopy(env.store)
    assert "nothing to fall back from" in cycle.model_fallback(cfg, env, REFUSAL)
    assert "nothing to fall back from" in cycle.model_fallback(dataclasses.replace(cfg, models=MODELS), env, REFUSAL)
    assert env.store == before


# ---- scripts/validate_tasks.py


def test_validate_tasks_accepts_the_fable_id_from_the_models_map(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001").update(model=FABLE, reviewer_model=FABLE, effort="high", review_areas=["money"])
    assert errors_for(tmp_path, tasks, models=WITH_FABLE) == []
    assert_error(errors_for(tmp_path, tasks, models=MODELS), "is not a full model ID")


def test_validate_tasks_refuses_an_unknown_model(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001").update(model="claude-unknown-5-5")
    assert_error(errors_for(tmp_path, tasks, models=WITH_FABLE), "is not a full model ID")
    assert_error(errors_for(tmp_path, valid_tasks(), models={**MODELS, "fable": "fable"}), "full model ID")


def test_plan_models_without_a_map_is_the_fixed_three() -> None:
    assert vt.plan_models({}) == vt.Models()
    found = vt.plan_models({"models": WITH_FABLE})
    assert found.ids == frozenset(WITH_FABLE.values()) and found.opus == frozenset({OPUS, FABLE})
