"""The model window (D-2225): a time-boxed switch of every model the runner starts, read from kit/runner.json.

Casey, November 9, 2026: switch everything to Fable at the weekly reset and run the usage to the limit; then
back to Opus. Fake tasks only; the runner's outside world is test_failure_drills.FakeEnv.
"""

from __future__ import annotations

import copy
import dataclasses
import io
import json
from contextlib import redirect_stdout
from datetime import datetime, timedelta
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import pytest
import yaml

import check_state as cs
import pacing
from runner import __main__ as entry, cycle, keeper as kp, model_window as mw, state as st
from test_failure_drills import MODELS, NOW, FakeEnv, config, make_task

ROOT = Path(__file__).resolve().parent.parent
FABLE = "claude-fable-5-1"
REFUSAL = "There's an issue with the selected model (claude-fable-5-1). It may not exist or you may not have access to it."


def window(start: datetime = NOW - timedelta(hours=1), until: datetime | None = None, run_to_limit: bool = True,
           end_on_limit: bool = False) -> dict[str, Any]:
    raw = {"model": FABLE, "alias": "fable", "from": st.iso(start), "until": st.iso(until) if until else None,
           "run_to_limit": run_to_limit, "end_on_limit": end_on_limit}
    found = mw.parse_window(raw)
    assert found is not None
    return found


def windowed(graph: list[dict[str, Any]], tmp_path: Path, win: dict[str, Any] | None, session: str = "s1") -> cycle.Config:
    return dataclasses.replace(config(graph, tmp_path, session), model_window=win)


# ---- the window itself


def test_the_window_is_half_open_and_until_null_is_open() -> None:
    win = window(NOW, NOW + timedelta(hours=2))
    assert not mw.in_window(win, NOW - timedelta(seconds=1))
    assert mw.in_window(win, NOW)  # `from` is inside
    assert mw.in_window(win, NOW + timedelta(hours=2) - timedelta(seconds=1))
    assert not mw.in_window(win, NOW + timedelta(hours=2))  # `until` is outside
    open_ended = window(NOW, None)
    assert mw.in_window(open_ended, NOW + timedelta(days=365))
    assert not mw.in_window(None, NOW)


def test_a_malformed_window_is_no_window() -> None:
    good = {"model": FABLE, "alias": "fable", "from": "2026-11-12T17:00:00Z", "until": None, "run_to_limit": True}
    assert mw.parse_window(good) is not None
    for bad in ({**good, "model": "fable"}, {**good, "alias": "Fable 5.1"}, {**good, "from": "Thursday"}, {**good, "from": None},
                {**good, "until": "soon"}, {**good, "run_to_limit": "yes"}, "fable", None, []):
        assert mw.parse_window(bad) is None, bad
    assert mw.parse_window({k: v for k, v in good.items() if k != "run_to_limit"})["run_to_limit"] is False  # type: ignore[index]  # reason: parsed above


def test_load_reads_kit_runner_json_and_fails_safe(tmp_path: Path) -> None:
    f = tmp_path / "runner.json"
    assert mw.load(f) is None  # no file
    f.write_text("{not json")
    assert mw.load(f) is None
    f.write_text('{"lanes": 1}')
    assert mw.load(f) is None
    f.write_text(json.dumps({"lanes": 1, "model_window": {"model": FABLE, "alias": "fable", "from": "2026-11-12T17:00:00Z", "until": None}}))
    assert mw.load(f) is not None and entry.lanes(f) == 1


def test_the_repository_window_is_caseys_thursday_switch() -> None:
    """kit/runner.json as Casey asked on November 9: Fable from the weekly reset, open-ended, to the limit."""
    raw = json.loads((ROOT / "kit" / "runner.json").read_text(encoding="utf-8"))
    assert raw["lanes"] == 2 and raw["owner_review"] is False  # the locked switches as set: two lanes since D-2462 (D-620)
    win = mw.load()
    assert win is not None and win["model"] == FABLE and win["alias"] == "fable"
    assert st.iso(win["from"]) == "2026-11-12T17:00:00Z" and win["until"] is None and win["run_to_limit"] is True
    assert win["end_on_limit"] is True
    reset = st.parse("2026-11-12T17:00:00Z")
    assert not mw.in_window(win, reset - timedelta(seconds=1)) and mw.in_window(win, reset)
    assert mw.session_model(win, reset - timedelta(seconds=1), None) == "claude-sonnet-5-5"
    assert mw.session_model(win, reset, None) == FABLE


# ---- workers and reviewers


def test_inside_the_window_the_worker_runs_on_its_model_and_the_plan_is_untouched(tmp_path: Path) -> None:
    graph = [make_task("T001")]
    before = copy.deepcopy(graph)
    env, cfg = FakeEnv(), windowed(graph, tmp_path, window())
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING
    assert env.runs[0][1] == FABLE  # the worker was asked for the window's model, not the task's Sonnet
    assert graph == before and cfg.models == MODELS  # kit/tasks.json's entry and models are not changed
    task = st.entry(env.store, "T001")
    assert task["window_model"] == {"model": FABLE, "alias": "fable"}
    assert task["history"][-1]["model"] == FABLE  # the history keeps the model that really ran
    assert cs.check(env.store) == []


def test_usage_records_keep_the_model_that_ran(tmp_path: Path) -> None:
    graph = [make_task("T001")]
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("submitted", ["app/t001/x.py"], [m], usage={"input_tokens": 10}))
    assert cycle.run_cycle(windowed(graph, tmp_path, window()), env, NOW) == st.MERGE_PENDING
    assert env.store["usage"]["by_task"]["T001"]["last_model"] == FABLE


def test_evidence_of_the_task_model_fails_a_windowed_attempt(tmp_path: Path) -> None:
    """The model check still holds: an attempt claimed on the window's model must show that model ran."""
    graph = [make_task("T001")]
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("submitted", ["app/t001/x.py"], [MODELS["sonnet"]]))
    assert cycle.run_cycle(windowed(graph, tmp_path, window()), env, NOW) == st.QUEUED
    assert f"model {MODELS['sonnet']} ran, task names {FABLE}" in st.entry(env.store, "T001")["last_failure"]


def test_the_reviewer_runs_on_the_window_model_too(tmp_path: Path) -> None:
    graph = [make_task("T001", reviewer_model=MODELS["opus"])]

    def worker(review: str) -> Any:
        return lambda t, m, log: cycle.WorkerResult("submitted", ["app/t001/x.py"], [m], reviewer_evidence=[review])

    env = FakeEnv(worker(FABLE))
    assert cycle.run_cycle(windowed(graph, tmp_path, window()), env, NOW) == st.MERGE_PENDING
    assert st.entry(env.store, "T001")["history"][-1]["reviewer_model"] == FABLE
    env = FakeEnv(worker(MODELS["opus"]))
    assert cycle.run_cycle(windowed(graph, tmp_path, window()), env, NOW) == st.QUEUED
    assert f"review: model {MODELS['opus']} ran, task names {FABLE}" in st.entry(env.store, "T001")["last_failure"]


def test_fix_main_runs_on_the_window_model_inside_it(tmp_path: Path) -> None:
    env = FakeEnv()
    env.store = cycle.set_main_red(st.new_state(), config([], tmp_path), "smoke", "main0001", ("T001", "abc", ["app/t001/x.py"]), NOW)
    cycle.run_cycle(windowed([], tmp_path, window()), env, NOW)
    assert env.runs and env.runs[0][0] == st.FIX_MAIN and env.runs[0][1] == FABLE


def _run_both(tmp_path: Path, win: dict[str, Any] | None, when: datetime) -> tuple[FakeEnv, FakeEnv]:
    graph = [make_task("T001", reviewer_model=MODELS["opus"]), make_task("T002")]

    def worker(t: dict[str, Any], m: str, log: str) -> cycle.WorkerResult:
        return cycle.WorkerResult("submitted", [f"app/{t['id'].lower()}/x.py"], [m], reviewer_evidence=[MODELS["opus"]])

    plain, switched = FakeEnv(worker), FakeEnv(worker)
    clock = (lambda: when)
    cycle.run_cycle(dataclasses.replace(config(graph, tmp_path, clock=clock), model_window=None), plain, when)
    cycle.run_cycle(dataclasses.replace(config(graph, tmp_path, clock=clock), model_window=win), switched, when)
    return plain, switched


def test_outside_the_window_everything_is_as_without_one(tmp_path: Path) -> None:
    """Before `from`, at `until` and after it: the same worker calls and the same state, key for key."""
    for win, when in ((window(NOW + timedelta(minutes=1)), NOW), (window(NOW - timedelta(hours=2), NOW), NOW),
                      (window(NOW - timedelta(hours=2), NOW - timedelta(hours=1)), NOW)):
        plain, switched = _run_both(tmp_path, win, when)
        assert switched.runs == plain.runs and switched.runs[0][1] == MODELS["sonnet"]
        assert json.dumps(switched.store, sort_keys=True) == json.dumps(plain.store, sort_keys=True)


def test_setting_until_in_the_past_restores_todays_behaviour(tmp_path: Path) -> None:
    """The overseer's one-line change: `until` set. A task first claimed in the window and retried after it
    runs on its own model again, and its old mark goes."""
    graph = [make_task("T001")]
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("tests_failed", log="red"))
    cycle.run_cycle(windowed(graph, tmp_path, window()), env, NOW)
    assert st.entry(env.store, "T001")["window_model"]["model"] == FABLE
    ended = window(NOW - timedelta(hours=1), NOW + timedelta(minutes=1))
    later = NOW + timedelta(minutes=5)
    env.worker = lambda t, m, log: cycle.WorkerResult("submitted", ["app/t001/x.py"], [m])
    assert cycle.run_cycle(dataclasses.replace(config(graph, tmp_path, clock=lambda: later), model_window=ended), env, later) == st.MERGE_PENDING
    assert env.runs[-1][1] == MODELS["sonnet"] and "window_model" not in st.entry(env.store, "T001")
    assert mw.session_model(ended, later, env.store) == "claude-sonnet-5-5" and not mw.run_to_limit(ended, later)


def test_an_attempt_is_judged_on_the_model_it_was_claimed_on(tmp_path: Path) -> None:
    """Claimed outside, handed in after `from`: the task's own model is expected, and the reverse."""
    graph = [make_task("T001")]
    win = window(NOW + timedelta(minutes=30))
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("handed_off"))
    cfg = windowed(graph, tmp_path, win)
    assert cycle.run_cycle(cfg, env, NOW) == "handed_off"
    late = dataclasses.replace(cfg, clock=lambda: NOW + timedelta(hours=1))
    res = cycle.WorkerResult("submitted", ["app/t001/x.py"], [MODELS["sonnet"]])
    assert cycle.after_handoff(late, env, lambda task: res) == st.MERGE_PENDING


# ---- the hand-off and the worker command


def test_the_handoff_names_the_alias_and_the_fallback_only_inside_the_window() -> None:
    task = make_task("T001", reviewer_model=MODELS["opus"])
    claimed = st.claim(st.sync_graph(st.new_state(), [task]), "T001", "a", NOW)
    cfg = SimpleNamespace(session="a", graph=[task], models=MODELS)
    env = SimpleNamespace(last_state=claimed, dry_run=True, branch_for=lambda tid: "claude/task/T001-x", prepared={})
    plain = entry.write_handoff(cfg, env)  # type: ignore[arg-type]  # reason: a stand-in for Config and GitHubEnv
    assert "model_window" not in plain and plain["model"] == MODELS["sonnet"] and plain["reviewer_model"] == MODELS["opus"]
    env.last_state = mw.mark_claim(claimed, "T001", window(), NOW)
    info = entry.write_handoff(cfg, env)  # type: ignore[arg-type]  # reason: as above
    assert info["model"] == FABLE and info["reviewer_model"] == FABLE
    assert info["model_window"] == {"model": FABLE, "alias": "fable", "reviewer_alias": "fable",
                                    "fallback_model": MODELS["sonnet"], "fallback_alias": "sonnet",
                                    "fallback_reviewer_model": MODELS["opus"], "fallback_reviewer_alias": "opus"}
    assert {k: v for k, v in info.items() if k not in ("model", "reviewer_model", "model_window")} == \
        {k: v for k, v in plain.items() if k not in ("model", "reviewer_model")}


def test_the_session_prompt_tells_the_worker_call_the_alias_and_the_fallback() -> None:
    text = (ROOT / ".github" / "workflows" / "backup-runner.yml").read_text(encoding="utf-8")
    assert "3d. When the handoff has a `model_window` block" in text
    assert "`fallback_alias`" in text and '--model-fallback "<the error, one line>"' in text


# ---- the orchestrating session's model


def test_the_session_model_follows_the_window() -> None:
    win = window(NOW, NOW + timedelta(hours=1))
    assert mw.session_model(win, NOW - timedelta(seconds=1), None) == "claude-sonnet-5-5"
    assert mw.session_model(win, NOW, None) == FABLE
    assert mw.session_model(win, NOW + timedelta(hours=1), None) == "claude-sonnet-5-5"
    refused = mw.record_refusal(st.new_state(), win, REFUSAL, NOW)
    assert mw.session_model(win, NOW, refused) == "claude-sonnet-5-5"
    other = window(NOW - timedelta(minutes=1), None)  # a new `from` is a new window, tried again
    assert mw.session_model(other, NOW, refused) == FABLE
    assert mw.session_model(None, NOW, None) == "claude-sonnet-5-5"


def _session_model_output(monkeypatch: pytest.MonkeyPatch, when: datetime, state_file: str) -> list[str]:
    monkeypatch.setattr(entry, "now_utc", lambda: when)
    out = io.StringIO()
    with redirect_stdout(out):
        assert entry.main(["--session-model", state_file]) == 0
    return out.getvalue().splitlines()


def test_the_session_model_command_reads_runner_json_and_the_state(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    reset = st.parse("2026-11-12T17:00:00Z")  # the repository's own window
    assert _session_model_output(monkeypatch, reset - timedelta(minutes=1), "") == ["model=claude-sonnet-5-5", "windowed=false"]
    assert _session_model_output(monkeypatch, reset, str(tmp_path / "missing.json")) == [f"model={FABLE}", "windowed=true"]
    win = mw.load()
    assert win is not None
    (tmp_path / "state.json").write_text(json.dumps(mw.record_refusal(st.new_state(), win, REFUSAL, reset)))
    assert _session_model_output(monkeypatch, reset, str(tmp_path / "state.json")) == ["model=claude-sonnet-5-5", "windowed=false"]


def _backup_steps() -> list[dict[str, Any]]:
    doc = yaml.safe_load((ROOT / ".github" / "workflows" / "backup-runner.yml").read_text(encoding="utf-8"))
    return list(doc["jobs"]["backup"]["steps"])


def test_the_workflow_takes_the_session_model_from_the_step_with_todays_fallback() -> None:
    steps = _backup_steps()
    pick = next(i for i, s in enumerate(steps) if s.get("id") == "session-model")
    claude = next(i for i, s in enumerate(steps) if s.get("id") == "claude")
    assert pick < claude and steps[pick]["continue-on-error"] is True
    assert "python3 -m runner --session-model" in steps[pick]["run"] and "GITHUB_OUTPUT" in steps[pick]["run"]
    args = steps[claude]["with"]["claude_args"]
    assert args.startswith(f"--model ${{{{ steps.session-model.outputs.model || '{mw.SESSION_MODEL}' }}}} --allowedTools")
    assert mw.SESSION_MODEL == MODELS["sonnet"]  # the default is today's model (BUILD_BRIEF.md:500)
    refused = next(s for s in steps if "Record a refused window model" in str(s.get("name")))
    assert refused["if"] == "always() && steps.session-model.outputs.windowed == 'true'"
    assert steps.index(refused) > claude and "--model-refused" in refused["run"]


# ---- the fallback when the model is refused


def _claimed_inside(tmp_path: Path, graph: list[dict[str, Any]]) -> tuple[FakeEnv, cycle.Config]:
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("handed_off"))
    cfg = windowed(graph, tmp_path, window())
    assert cycle.run_cycle(cfg, env, NOW) == "handed_off"
    return env, cfg


def test_a_refused_worker_model_falls_back_without_costing_an_attempt(tmp_path: Path) -> None:
    graph = [make_task("T001"), make_task("T002")]
    env, cfg = _claimed_inside(tmp_path, graph)
    line = cycle.model_fallback(cfg, env, REFUSAL)
    assert line.startswith(f"model window: {FABLE} refused for T001")
    assert "window_model" not in st.entry(env.store, "T001")
    assert env.store["model_window_refused"] == {"model": FABLE, "from": st.iso(window()["from"]), "at": st.iso(NOW), "why": REFUSAL}
    note = (tmp_path / f"{mw.OUTBOX_ID}.md").read_text(encoding="utf-8")
    assert "kind: info" in note and "No attempt was counted" in note
    res = cycle.WorkerResult("submitted", ["app/t001/x.py"], [MODELS["sonnet"]])
    assert cycle.after_handoff(cfg, env, lambda task: res, own_models=True) == st.MERGE_PENDING
    assert st.entry(env.store, "T001")["attempts"] == 0 and st.entry(env.store, "T001")["history"][-1]["model"] == MODELS["sonnet"]
    assert cs.check(env.store) == []
    # The rest of that window changes no model, while run_to_limit still holds.
    assert mw.session_model(cfg.model_window, NOW, env.store) == "claude-sonnet-5-5" and mw.run_to_limit(cfg.model_window, NOW)


def test_the_next_claim_after_a_refusal_uses_the_task_model(tmp_path: Path) -> None:
    graph = [make_task("T001")]
    env = FakeEnv()
    env.store = mw.record_refusal(st.new_state(), window(), REFUSAL, NOW)
    assert cycle.run_cycle(windowed(graph, tmp_path, window()), env, NOW) == st.MERGE_PENDING
    assert env.runs[0][1] == MODELS["sonnet"] and "window_model" not in st.entry(env.store, "T001")


def test_own_models_judges_the_task_model_even_when_the_mark_stayed(tmp_path: Path) -> None:
    """If the refusal could not be written, `--after --model-fallback` still judges on the task's own model."""
    env, cfg = _claimed_inside(tmp_path, [make_task("T001")])
    res = cycle.WorkerResult("submitted", ["app/t001/x.py"], [MODELS["sonnet"]])
    assert cycle.after_handoff(cfg, env, lambda task: res, own_models=True) == st.MERGE_PENDING


def test_a_fallback_without_a_mark_or_a_lease_changes_nothing(tmp_path: Path) -> None:
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("handed_off"))
    cfg = windowed([make_task("T001")], tmp_path, None)
    assert cycle.model_fallback(cfg, env, REFUSAL) == ""  # no lease held
    cycle.run_cycle(cfg, env, NOW)
    before = copy.deepcopy(env.store)
    assert "nothing to fall back from" in cycle.model_fallback(cfg, env, REFUSAL)
    assert env.store == before and not (tmp_path / f"{mw.OUTBOX_ID}.md").exists()


def test_a_fallback_after_the_window_changed_only_unmarks(tmp_path: Path) -> None:
    env, cfg = _claimed_inside(tmp_path, [make_task("T001")])
    moved = dataclasses.replace(cfg, model_window=None)
    assert "falls back from" in cycle.model_fallback(moved, env, REFUSAL)
    assert "window_model" not in st.entry(env.store, "T001") and "model_window_refused" not in env.store


def test_the_after_command_passes_the_fallback(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    env, cfg = _claimed_inside(tmp_path, [make_task("T001")])
    env.log = []  # type: ignore[attr-defined]  # reason: GitHubEnv's log list, which `after` appends to
    res = cycle.WorkerResult("submitted", ["app/t001/x.py"], [MODELS["sonnet"]])
    env.collect_result = lambda task, *rest: res  # type: ignore[attr-defined]  # reason: GitHubEnv.collect_result's stand-in
    monkeypatch.setattr(entry, "read_handoff", lambda: {})
    args = entry.parse_args(["--after", "--model-fallback", REFUSAL])
    assert entry.after(args, cfg, env) == st.MERGE_PENDING  # type: ignore[arg-type]  # reason: FakeEnv for GitHubEnv
    assert env.store["model_window_refused"]["model"] == FABLE and env.log[0].startswith("model window:")  # type: ignore[attr-defined]  # reason: as above


def test_a_session_refused_its_model_is_found_only_when_it_used_no_tool() -> None:
    refused = [{"type": "system", "subtype": "init", "model": FABLE},
               {"type": "assistant", "message": {"content": [{"type": "text", "text": f"API Error: {REFUSAL}"}]}},
               {"type": "result", "is_error": True, "result": REFUSAL}]
    assert "issue with the selected model" in mw.session_refusal(refused)
    worked = [*refused[:1], {"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Bash"}]}},
              {"type": "user", "message": {"content": [{"type": "tool_result", "content": REFUSAL}]}}]
    assert mw.session_refusal(worked) == ""  # tool output that quotes the words is never read as a refusal
    assert mw.session_refusal([{"type": "result", "result": "usage limit reached"}]) == ""
    assert mw.is_refusal('{"type":"error","error":{"type":"not_found_error","message":"model: claude-fable-5-1"}}')


def test_the_refused_command_records_the_window_once(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    env, cfg = FakeEnv(), windowed([make_task("T001")], tmp_path, window())
    monkeypatch.setattr(entry, "now_utc", lambda: NOW)
    execution = tmp_path / "execution.json"
    execution.write_text(json.dumps([{"type": "result", "is_error": True, "result": REFUSAL}]))
    assert entry.model_refused(execution, cfg, env) == f"model_window_refused: {FABLE}"  # type: ignore[arg-type]  # reason: FakeEnv
    assert env.store["model_window_refused"]["why"] == REFUSAL and (tmp_path / f"{mw.OUTBOX_ID}.md").exists()
    assert entry.model_refused(execution, cfg, env) == "model_window_inactive"  # type: ignore[arg-type]  # reason: as above
    execution.write_text(json.dumps([{"type": "result", "result": "done"}]))
    assert entry.model_refused(execution, windowed([], tmp_path, window(NOW)), FakeEnv()) == "model_window_ok"  # type: ignore[arg-type]  # reason: as above
    assert entry.model_refused(tmp_path / "none.json", windowed([], tmp_path, window(NOW)), FakeEnv()) == "model_window_unread"  # type: ignore[arg-type]  # reason: as above
    assert set(entry.MODEL_WINDOW_SILENT) == {"model_window_inactive", "model_window_ok", "model_window_unread"}
    assert kp.last_result([{"session": "s", "at": "2026", "result": "idle"}, {"session": "s", "at": "2026", "result": "model_window_refused: x"}],
                          "s", "2026") == "idle"


# ---- the pacer and the keeper


def _over_the_headroom() -> dict[str, Any]:
    """97.5% of the week read 10 minutes ago, the ledger past its placeholder budget: today's pacer waits."""
    state = st.new_state()
    state["usage"] = {"session_reading": {"at": st.iso(NOW - timedelta(minutes=10)),
                                          "week": {"utilization": 0.975, "resets_at": st.iso(NOW + timedelta(days=2))}},
                      "windows": {"five_hour": {"start": st.iso(NOW - timedelta(hours=1)), "tokens": 40_000_000},
                                  "week": {"start": st.iso(NOW - timedelta(days=5)), "tokens": 300_000_000}}}
    return state


def test_run_to_limit_claims_past_the_headroom_and_the_ledger(tmp_path: Path) -> None:
    task = make_task("T001", owns=[])
    state = _over_the_headroom()
    assert pacing.decide(state, task, root=tmp_path, now=NOW)[0] == "wait"
    found = pacing.assess(state, task, root=tmp_path, now=NOW, run_to_limit=True)
    assert found["decision"] == ("claim", None) and "running to the limit (model window, D-2225)" in pacing.line("T001", found)


def test_run_to_limit_waits_for_a_real_limit() -> None:
    task = make_task("T001", owns=[])
    full = _over_the_headroom()
    full["usage"]["session_reading"]["week"]["utilization"] = 1.0
    taken = NOW - timedelta(minutes=10)
    # The reading expires before the week resets: the wait ends then, so a week Casey reset shows in the next reading.
    assert pacing.assess(full, task, now=NOW, run_to_limit=True)["decision"] == ("wait", st.iso(taken + timedelta(hours=6)))
    soon = copy.deepcopy(full)
    soon["usage"]["session_reading"]["week"]["resets_at"] = st.iso(NOW + timedelta(hours=1))
    assert pacing.assess(soon, task, now=NOW, run_to_limit=True)["decision"] == ("wait", st.iso(NOW + timedelta(hours=1)))
    hit = _over_the_headroom()
    hit["usage"]["limit_hit"] = {"task": "T009", "at": st.iso(NOW), "reset_at": st.iso(NOW + timedelta(hours=1))}
    assert pacing.assess(hit, task, now=NOW, run_to_limit=True)["decision"] == ("wait", st.iso(NOW + timedelta(hours=1)))
    quota = {"five_hour": {"utilization": 1.0, "resets_at": st.iso(NOW + timedelta(hours=2))}, "week": {"utilization": 0.5}}
    assert pacing.assess(st.new_state(), task, now=NOW, quota=quota, run_to_limit=True)["decision"] == ("wait", st.iso(NOW + timedelta(hours=2)))


def test_the_pacer_runs_to_the_limit_only_inside_the_window(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("PACING_QUOTA_CMD", raising=False)
    cfg = dict(pacing.load_config(), quota_cmd_default="")
    task = make_task("T001", owns=[])
    win = window(NOW, NOW + timedelta(hours=1))
    plain = pacing.make_pacer([task], tmp_path, cfg)
    switched = pacing.make_pacer([task], tmp_path, cfg, run_to_limit=lambda now, state: mw.run_to_limit(win, now, state))
    state = _over_the_headroom()
    assert switched(state, task, NOW)[0] == "claim" and plain(state, task, NOW)[0] == "wait"
    for when in (NOW - timedelta(seconds=1), NOW + timedelta(hours=1)):  # before `from`, at `until`
        assert switched(state, task, when) == plain(state, task, when)
    off = window(NOW, None, run_to_limit=False)
    assert not mw.run_to_limit(off, NOW) and mw.session_model(off, NOW, None) == FABLE


def test_the_runner_wires_the_pacer_switch_to_kit_runner_json() -> None:
    switch = entry.run_to_limit_now()
    reset = st.parse("2026-11-12T17:00:00Z")
    assert switch(reset, st.new_state()) is True and switch(reset - timedelta(seconds=1), st.new_state()) is False


def test_the_keeper_stops_at_the_real_limit_inside_the_window() -> None:
    def state(used: float, age_hours: float = 0.2) -> dict[str, Any]:
        reading = {"at": st.iso(NOW - timedelta(hours=age_hours)), "week": {"utilization": used, "resets_at": st.iso(NOW + timedelta(days=2))}}
        return {"backup_runner": "on", "tasks": [], "usage": {"session_reading": reading}}

    reset = NOW + timedelta(days=2)
    assert kp.week_stop(state(0.98), NOW) == reset  # today: the 97% stop
    assert kp.week_stop(state(0.98), NOW, to_limit=True) is None  # inside the window: keep going
    assert kp.week_stop(state(1.0), NOW, to_limit=True) == NOW - timedelta(hours=0.2) + timedelta(hours=kp.READING_HOURS)
    assert kp.week_stop(state(1.0, age_hours=7), NOW, to_limit=True) is None  # a stale reading says nothing
    assert kp.week_stop(state(1.0), NOW) == reset
    assert kp.wake_after(state(0.98), {}, NOW, to_limit=False)[1].startswith("the week stopped at its limit")
    assert not kp.wake_after(state(0.98), {}, NOW, to_limit=True)[1].startswith("the week stopped")


# ---- the state schema


def test_check_state_accepts_the_new_values_and_rejects_bad_ones() -> None:
    win = window()
    good = mw.record_refusal(mw.mark_claim(st.claim(st.new_state(), "T001", "a", NOW), "T001", win, NOW), win, REFUSAL, NOW)
    good = mw.mark_claim(good, "T001", window(NOW - timedelta(minutes=5)), NOW)  # a new window: marked again
    assert st.entry(good, "T001")["window_model"] == {"model": FABLE, "alias": "fable"} and cs.check(good) == []
    bad = st.with_task(good, "T001", window_model={"model": "fable"})
    assert any("window_model" in p for p in cs.check(bad))
    assert any("model_window_refused" in p for p in cs.check(dict(good, model_window_refused={"model": FABLE})))


# ---- the window ends at the first limit inside it (end_on_limit)


def _hit(state: st.State, at: datetime) -> st.State:
    out = copy.deepcopy(state)
    out.setdefault("usage", {})["limit_hit"] = {"task": "T009", "at": st.iso(at), "reset_at": st.iso(at + timedelta(hours=1))}
    return out


def _read(state: st.State, at: datetime, week: float, five_hour: float | None = None) -> st.State:
    out = copy.deepcopy(state)
    reading: dict[str, Any] = {"at": st.iso(at), "week": {"utilization": week, "resets_at": st.iso(NOW + timedelta(days=3))}}
    if five_hour is not None:
        reading["five_hour"] = {"utilization": five_hour, "resets_at": st.iso(at + timedelta(hours=2))}
    out.setdefault("usage", {})["session_reading"] = reading
    return out


def test_the_window_closes_on_the_first_limit_and_the_next_claim_uses_the_task_model(tmp_path: Path) -> None:
    win = window(end_on_limit=True)
    graph = [make_task("T001"), make_task("T002")]
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("usage_limit", usage={"input_tokens": 5_000_000}))
    cfg = windowed(graph, tmp_path, win)
    assert cycle.run_cycle(cfg, env, NOW) == st.RETRY_AFTER_RESET  # the limit, hit on the window's model
    assert env.runs[0][1] == FABLE
    assert not mw.closed(win, env.store)  # a limit_hit alone (maybe the five-hour limit) is no proof
    env.store = _read(env.store, NOW + timedelta(minutes=1), 1.0)  # the session's reading: the week is used up
    assert mw.closed(win, env.store) and mw.session_model(win, NOW, env.store) == "claude-sonnet-5-5"
    assert not mw.run_to_limit(win, NOW, env.store)
    env.worker = lambda t, m, log: cycle.WorkerResult("submitted", [f"app/{t['id'].lower()}/x.py"], [m])
    later = NOW + timedelta(hours=2)  # past the hit's reset
    assert cycle.run_cycle(dataclasses.replace(cfg, clock=lambda: later), env, later) == st.MERGE_PENDING
    assert env.runs[-1][1] == MODELS["sonnet"] and "window_model" not in st.entry(env.store, env.runs[-1][0])
    assert env.store["model_window_ended"] == {"model": FABLE, "from": st.iso(win["from"]), "at": st.iso(later), "why": "limit"}
    note = (tmp_path / f"{mw.ENDED_OUTBOX_ID}.md").read_text(encoding="utf-8")
    assert "kind: info" in note and "back on the plan's models" in note
    assert cs.check(env.store) == []
    # It stays closed once recorded, even when a fresh reading after Casey's reset shows the week near zero.
    reset = _read(env.store, later + timedelta(hours=1), 0.02)
    assert mw.closed(win, reset) and mw.session_model(win, later + timedelta(hours=1), reset) == "claude-sonnet-5-5"


def test_only_a_week_reading_at_the_limit_closes_it() -> None:
    win = window(end_on_limit=True)
    assert mw.limit_inside(win, _read(st.new_state(), NOW, 1.0)).startswith("the week read at its limit")
    rejected = _read(st.new_state(), NOW, 0.93)
    rejected["usage"]["session_reading"]["week"]["status"] = "rejected"
    assert mw.limit_inside(win, rejected)  # the API's own refusal counts as the limit
    five = _read(_hit(st.new_state(), NOW), NOW + timedelta(minutes=5), 0.6, five_hour=1.0)
    assert mw.limit_inside(win, five) == "" and not mw.closed(win, five)  # the hit was the five-hour limit
    assert mw.limit_inside(win, _hit(st.new_state(), NOW)) == ""  # a hit with no reading after it: no proof
    assert mw.close_on_limit(st.new_state(), win, NOW) == (st.new_state(), "")
    assert not mw.closed(window(end_on_limit=False), _read(st.new_state(), NOW, 1.0))  # without end_on_limit: runs on


def test_after_a_close_the_keeper_wakes_within_6_hours_and_casey_reset_resumes_on_the_task_model(tmp_path: Path) -> None:
    """B1: limit, close, Casey's reset. The pacer's ledger (piled up while it was not consulted) is dropped at
    the close; the keeper sleeps no longer than the reading's expiry, not to the weekly reset; the cycle after
    the fresh low reading claims on the task's own model at normal pace."""
    win = window(end_on_limit=True)
    graph = [make_task("T001")]
    full = _read(st.sync_graph(st.new_state(), graph), NOW, 1.0)
    full["usage"]["windows"] = {"five_hour": {"start": st.iso(NOW - timedelta(hours=1)), "tokens": 90_000_000},
                                "week": {"start": st.iso(NOW - timedelta(days=1)), "tokens": 400_000_000}}
    ended, why = mw.close_on_limit(full, win, NOW)
    assert why and "windows" not in ended["usage"] and mw.ended(win, ended)
    cfg = dict(pacing.load_config(), quota_cmd_default="")
    pacer = pacing.make_pacer(graph, tmp_path, cfg, run_to_limit=lambda now, state: mw.run_to_limit(win, now, state))
    verdict, until, paced = pacer(ended, graph[0], NOW + timedelta(minutes=5))
    assert verdict == "wait" and st.parse(str(until)) > NOW + timedelta(days=1)  # the normal pacer: until the weekly reset
    awake = {**paced, "backup_runner": "on"}
    minutes, reason = kp.wake_after(awake, {}, NOW + timedelta(minutes=5), to_limit=False, ended=True)
    assert minutes is not None and minutes <= 6 * 60 + kp.RESET_MARGIN, reason
    assert kp.wake_after(awake, {}, NOW + timedelta(minutes=5), to_limit=False, ended=False)[0] == kp.CAP_MINUTES  # before: to the reset
    stale = NOW + timedelta(hours=6, minutes=5)
    assert kp.wake_after(awake, {}, stale, to_limit=False, ended=True) == (kp.IDLE_MINUTES, "idle")
    # Casey's reset: the woken cycle's session reports the week near zero, and the claim goes to Sonnet.
    env = FakeEnv()
    env.store = _read(paced, stale, 0.02)
    run_cfg = dataclasses.replace(windowed(graph, tmp_path, win), pacer=pacer, clock=lambda: stale)
    assert cycle.run_cycle(run_cfg, env, stale) == st.MERGE_PENDING
    assert env.runs[-1][1] == MODELS["sonnet"]


def test_the_keeper_reads_the_ended_record_from_the_repository_window() -> None:
    win = mw.load()
    assert win is not None
    later = st.parse("2026-11-14T12:00:00Z")
    reading = _read(st.new_state(), later, 1.0)
    reading["usage"]["session_reading"]["week"]["resets_at"] = "2026-11-19T17:00:00Z"  # the next weekly reset
    ended, _ = mw.close_on_limit(reading, win, later)
    awake = {**ended, "backup_runner": "on"}
    minutes, reason = kp.wake_after(awake, {}, later + timedelta(minutes=1))
    assert minutes is not None and minutes <= 6 * 60 + kp.RESET_MARGIN and reason.startswith("the week stopped")


def test_a_limit_before_from_does_not_close_a_future_window() -> None:
    win = window(NOW, end_on_limit=True)
    old = _read(_hit(st.new_state(), NOW - timedelta(hours=2)), NOW - timedelta(minutes=1), 1.0)
    assert mw.limit_inside(win, old) == "" and not mw.closed(win, old)
    assert mw.close_on_limit(old, win, NOW)[1] == ""
    assert mw.session_model(win, NOW, old) == FABLE and mw.run_to_limit(win, NOW, old)
    other = window(NOW - timedelta(hours=3), end_on_limit=True)  # a different window: the same limit closes it
    ended, _ = mw.close_on_limit(old, other, NOW)
    assert mw.closed(other, ended) and not mw.closed(win, ended)  # the record names its own `from`


def test_after_the_window_closes_the_pacer_is_back_to_normal(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("PACING_QUOTA_CMD", raising=False)
    cfg = dict(pacing.load_config(), quota_cmd_default="")
    task = make_task("T001", owns=[])
    win = window(end_on_limit=True)
    plain = pacing.make_pacer([task], tmp_path, cfg)
    switched = pacing.make_pacer([task], tmp_path, cfg, run_to_limit=lambda now, state: mw.run_to_limit(win, now, state))
    open_state = _over_the_headroom()  # its 97.5% reading was taken inside the window
    assert switched(open_state, task, NOW)[0] == "claim"
    full = _read(open_state, NOW - timedelta(minutes=10), 1.0)
    ended, _ = mw.close_on_limit(full, win, NOW)
    assert switched(ended, task, NOW) == plain(ended, task, NOW) and plain(ended, task, NOW)[0] == "wait"
    # The stale 100% reading still holds; a fresh one after Casey's reset lets the build go at normal pace.
    fresh = _read(ended, NOW + timedelta(hours=1), 0.01)
    fresh["usage"]["windows"] = {}
    later = NOW + timedelta(hours=1, minutes=5)
    assert switched(fresh, task, later) == plain(fresh, task, later) and plain(fresh, task, later)[0] == "claim"
    assert kp.wake_after({**ended, "backup_runner": "on"}, {}, NOW)[1].startswith("the week stopped")  # the 97% stop is back


def test_the_usage_reading_command_closes_the_window(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    reset = st.parse("2026-11-12T17:00:00Z")  # the repository's own window
    monkeypatch.setattr(entry, "now_utc", lambda: reset + timedelta(hours=30))
    env = FakeEnv()
    env.outbox_dir = tmp_path  # type: ignore[attr-defined]  # reason: GitHubEnv's outbox folder
    execution = tmp_path / "execution.json"
    event = {"type": "rate_limit_event", "rate_limit_info": {"unifiedWindows": {"seven_day": {"utilization": 1.0, "resetsAt": 1795000000}}}}
    execution.write_text(json.dumps([event]))
    assert entry.usage_reading(execution, env).startswith("usage_reading_recorded")  # type: ignore[arg-type]  # reason: FakeEnv
    assert env.store["model_window_ended"]["why"] == "limit" and (tmp_path / f"{mw.ENDED_OUTBOX_ID}.md").exists()


def test_until_still_ends_an_end_on_limit_window() -> None:
    win = window(NOW - timedelta(hours=2), NOW, end_on_limit=True)
    assert mw.session_model(win, NOW, st.new_state()) == "claude-sonnet-5-5" and not mw.run_to_limit(win, NOW, st.new_state())
    assert mw.session_model(win, NOW - timedelta(minutes=1), st.new_state()) == FABLE


# ---- review fixes: the limit stops a session mid-task; the model check; the fallback gate


def _events(*infos: dict[str, Any]) -> str:
    return json.dumps([{"type": "rate_limit_event", "rate_limit_info": info} for info in infos])


def test_at_limit_reads_the_last_reading_of_the_execution_file(tmp_path: Path) -> None:
    from runner import usage_reading as ur

    under = {"unifiedWindows": {"seven_day": {"utilization": 0.8, "resetsAt": 1795000000}}}
    full = {"unifiedWindows": {"seven_day": {"utilization": 1.0, "resetsAt": 1795000000}}}
    five = {"unifiedWindows": {"five_hour": {"utilization": 1.0}, "seven_day": {"utilization": 0.4}}}
    assert not ur.at_limit(_events(under)) and ur.at_limit(_events(under, full)) and ur.at_limit(_events(five))
    assert ur.at_limit(_events({"status": "rejected", "rateLimitType": "seven_day"}))
    assert ur.at_limit(_events(under, {"status": "rejected"}))
    assert not ur.at_limit("") and not ur.at_limit(_events({"status": "allowed", "rateLimitType": "seven_day", "utilization": 0.5}))
    assert ur.latest_reading(_events({"status": "rejected", "rateLimitType": "seven_day"})) == {"week": {"utilization": 1.0, "status": "rejected"}}
    f = tmp_path / "exec.json"
    f.write_text(_events(full))
    assert ur.main(["--at-limit", str(f)]) == 0
    f.write_text(_events(under))
    assert ur.main(["--at-limit", str(f)]) == 1 and ur.main(["--at-limit", str(tmp_path / "none")]) == 1 and ur.main([]) == 2


def test_a_session_stopped_at_the_limit_hands_in_usage_limit_at_no_attempt_cost(tmp_path: Path) -> None:
    step = next(s for s in _backup_steps() if s.get("id") == "safety")
    assert step["env"]["EXECUTION_FILE"] == "${{ steps.claude.outputs.execution_file }}"
    assert 'python3 -m runner.usage_reading --at-limit "$EXECUTION_FILE"' in step["run"] and "outcome=usage_limit" in step["run"]
    assert '--outcome "$outcome"' in step["run"] and "usage_limit:*" in step["run"]  # and no chained run
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("handed_off"))
    cfg = windowed([make_task("T001")], tmp_path, window())
    cycle.run_cycle(cfg, env, NOW)
    assert cycle.after_handoff(cfg, env, lambda task: cycle.WorkerResult("usage_limit", log="stopped")) == st.RETRY_AFTER_RESET
    entry_ = st.entry(env.store, "T001")
    assert entry_["attempts"] == 0 and env.store["usage"]["limit_hit"]["task"] == "T001"


def test_fable_evidence_on_an_unmarked_attempt_is_rejected(tmp_path: Path) -> None:
    unmarked = make_task("T001")
    assert cycle.known_models(unmarked, MODELS) == list(MODELS.values())
    assert cycle.known_models({**unmarked, "window_model": {"model": FABLE, "alias": "fable"}}, MODELS) == [*MODELS.values(), FABLE]
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("submitted", ["app/t001/x.py"], [FABLE]))
    assert cycle.run_cycle(windowed([unmarked], tmp_path, None), env, NOW) == st.QUEUED
    assert f"model {FABLE} ran, task names {MODELS['sonnet']}" in st.entry(env.store, "T001")["last_failure"]


def test_a_fallback_text_that_is_no_refusal_changes_nothing(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    env, cfg = _claimed_inside(tmp_path, [make_task("T001")])
    before = copy.deepcopy(env.store)
    assert cycle.model_fallback(cfg, env, "the tests failed").startswith("model window: not a model refusal")
    assert env.store == before and not (tmp_path / f"{mw.OUTBOX_ID}.md").exists()
    env.log = []  # type: ignore[attr-defined]  # reason: GitHubEnv's log list
    env.collect_result = lambda task, *rest: cycle.WorkerResult("submitted", ["app/t001/x.py"], [MODELS["sonnet"]])  # type: ignore[attr-defined]  # reason: GitHubEnv.collect_result's stand-in
    monkeypatch.setattr(entry, "read_handoff", lambda: {})
    args = entry.parse_args(["--after", "--model-fallback", "the tests failed"])
    assert entry.after(args, cfg, env) == st.QUEUED  # type: ignore[arg-type]  # reason: FakeEnv; judged on the window's model as usual
