"""A worker session killed by an Anthropic API error uses no attempt (D-3443, BUILD_BRIEF.md:271).

November 13, 2026: T255, T428 and T312 each lost an attempt to "API Error: 403 ... authentication_failed" with
no turn of work done. Drills: such a death with nothing handed in or pushed goes back to QUEUED with its
attempts unchanged, at most twice in a row; a pushed branch, a held hand-in, a non-API error or a third death
in a row counts as today; three deaths within an hour write the outbox item `api-deaths`; the execution file
is read only for a safe label, so nothing from the error text reaches the state, the log or the outbox.
Fake tasks only; the runner's outside world is test_failure_drills.FakeEnv.
"""

from __future__ import annotations

import io
import json
from contextlib import redirect_stdout
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

import yaml

import check_state as cs
from runner import __main__ as entry, api_death as ad, cycle, state as st
from test_failure_drills import NOW, FakeEnv, config, make_task

ROOT = Path(__file__).resolve().parent.parent
FAKE_SECRET = "FAKE-OAUTH-TOKEN-0000-not-real"  # a stand-in a key scanner does not match
CF_403 = ("Failed to authenticate. API Error: 403 <html><title>Attention Required! | Cloudflare</title> cf-ray 8f00fake "
          f"Authorization: Bearer {FAKE_SECRET} (error type authentication_failed, HTTP 403)")


def _exec_file(result_text: str, is_error: bool = True, tool_text: str = "") -> str:
    msgs: list[dict[str, Any]] = [{"type": "system", "subtype": "init"}]
    if tool_text:
        msgs.append({"type": "user", "message": {"content": [{"type": "tool_result", "content": tool_text}]}})
    msgs.append({"type": "result", "subtype": "success", "is_error": is_error, "num_turns": 0, "result": result_text})
    return json.dumps(msgs)


def _log(label: str = "API Error 403 authentication_failed") -> str:
    return ad.LOG + label


def _claimed(tmp_path: Path, tids: tuple[str, ...] = ("T001",), clock: list[datetime] | None = None) -> tuple[FakeEnv, cycle.Config]:
    times = clock if clock is not None else [NOW]
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("handed_off"))
    cfg = config([make_task(t) for t in tids], tmp_path, clock=lambda: times[0])
    assert cycle.run_cycle(cfg, env, times[0]) == "handed_off"
    return env, cfg


def _die(cfg: cycle.Config, env: FakeEnv, outcome: str = ad.API_ERROR, log: str | None = None) -> str:
    return cycle.after_handoff(cfg, env, lambda task: cycle.WorkerResult(outcome, log=_log() if log is None else log))


def _reclaim(cfg: cycle.Config, env: FakeEnv, now: datetime) -> None:
    assert cycle.run_cycle(cfg, env, now) == "handed_off"


# ---- the signature


def test_signature_reads_the_cloudflare_403_and_the_overloaded_and_5xx_deaths() -> None:
    assert ad.signature(_exec_file(CF_403)) == "API Error 403 authentication_failed"
    overloaded = 'API Error: 529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}'
    assert ad.signature(_exec_file(overloaded)) == "API Error 529 overloaded_error"
    assert ad.signature(_exec_file('API Error: 500 {"type":"error","error":{"type":"api_error"}}')) == "API Error 500 api_error"
    assert ad.signature(_exec_file("API Error: 401 invalid x-api-key")) == "API Error 401"
    assert ad.signature(_exec_file("API Error: 429 rate_limit_error")) == "API Error 429 rate_limit_error"
    assert ad.signature(_exec_file("Agent terminated: authentication_failed")) == "API Error authentication_failed"


def test_signature_refuses_everything_else() -> None:
    assert ad.signature(_exec_file(CF_403, is_error=False)) == ""  # a result that is no error
    assert ad.signature(_exec_file("API Error: 400 invalid_request_error")) == ""  # a 4xx that is our request's fault
    assert ad.signature(_exec_file("API Error: 404 not_found_error")) == ""
    assert ad.signature(_exec_file("error_max_turns")) == ""  # an ordinary failed session
    assert ad.signature(_exec_file("Claude execution failed")) == ""
    assert ad.signature(_exec_file("api_error happened")) == ""  # a bare api_error type names no status: refused
    # Only the last result's own text: a tool output quoting the error never qualifies the session.
    assert ad.signature(_exec_file("error_max_turns", tool_text=CF_403)) == ""
    later = json.loads(_exec_file(CF_403)) + [{"type": "result", "is_error": True, "result": "error_during_execution"}]
    assert ad.signature(json.dumps(later)) == ""
    assert ad.signature("") == "" and ad.signature("not json at all") == ""


def test_the_command_prints_only_the_label_and_answers_by_exit_status(tmp_path: Path) -> None:
    f = tmp_path / "exec.json"
    f.write_text(_exec_file(CF_403))
    out = io.StringIO()
    with redirect_stdout(out):
        assert ad.main([str(f)]) == 0
    assert out.getvalue() == "API Error 403 authentication_failed\n" and FAKE_SECRET not in out.getvalue()
    f.write_text(_exec_file("error_max_turns"))
    assert ad.main([str(f)]) == 1 and ad.main([str(tmp_path / "missing")]) == 1 and ad.main([]) == 2


# ---- the hand-in


def test_an_api_death_with_nothing_pushed_goes_back_to_the_queue_attempts_unchanged(tmp_path: Path) -> None:
    env, cfg = _claimed(tmp_path)
    assert _die(cfg, env) == st.QUEUED
    entry_ = st.entry(env.store, "T001")
    assert entry_["attempts"] == 0 and entry_[ad.RUN] == 1 and "D-3443" in entry_["last_failure"]
    row = entry_["history"][-1]
    assert row["api_error"] == "API Error 403 authentication_failed" and row["api_error_free"] is True and row["attempt"] == 1
    assert list(env.store[ad.DEATHS].values()) == [{"task": "T001", "at": st.iso(NOW), "signature": "API Error 403 authentication_failed"}]
    assert not cs.check(env.store, lanes=1)  # the state the runner writes validates (scripts/check_state.py)
    assert not (tmp_path / f"{ad.OUTBOX_ID}.md").exists()


def test_the_third_death_in_a_row_counts_and_any_other_end_resets_the_run(tmp_path: Path) -> None:
    clock = [NOW]
    env, cfg = _claimed(tmp_path, clock=clock)
    assert _die(cfg, env) == st.QUEUED
    clock[0] = NOW + timedelta(hours=2)  # outside the hour, so no alarm muddles the count
    _reclaim(cfg, env, clock[0])
    assert _die(cfg, env) == st.QUEUED and st.entry(env.store, "T001")["attempts"] == 0
    clock[0] = NOW + timedelta(hours=4)
    _reclaim(cfg, env, clock[0])
    assert _die(cfg, env) == st.QUEUED
    third = st.entry(env.store, "T001")
    assert third["attempts"] == 1 and "api_error_free" not in third["history"][-1] and third["history"][-1]["api_error"]
    clock[0] = NOW + timedelta(hours=6)
    _reclaim(cfg, env, clock[0])
    assert _die(cfg, env, "tests_failed", "a real failure") == st.QUEUED
    assert st.entry(env.store, "T001")["attempts"] == 2 and ad.RUN not in st.entry(env.store, "T001")
    assert not cs.check(env.store, lanes=1)


def test_a_non_api_error_and_an_unlabelled_api_error_count(tmp_path: Path) -> None:
    env, cfg = _claimed(tmp_path)
    assert _die(cfg, env, "tests_failed", "session ended without handing in") == st.QUEUED
    assert st.entry(env.store, "T001")["attempts"] == 1
    _reclaim(cfg, env, NOW)
    assert _die(cfg, env, ad.API_ERROR, ad.LOG + "something else") == st.QUEUED  # no API label in the log
    assert st.entry(env.store, "T001")["attempts"] == 2 and ad.RUN not in st.entry(env.store, "T001")


def test_a_free_requeue_on_the_last_attempt_keeps_the_task_out_of_blocked(tmp_path: Path) -> None:
    env, cfg = _claimed(tmp_path)
    env.store = st.with_task(env.store, "T001", attempts=cycle.MAX_ATTEMPTS - 1)
    assert _die(cfg, env) == st.QUEUED and st.entry(env.store, "T001")["attempts"] == cycle.MAX_ATTEMPTS - 1


def _park_the_dead(env: FakeEnv) -> None:
    """Set each requeued task aside, so the next claim takes another task, as three lanes would."""
    for task in env.store["tasks"]:
        if task.get(ad.RUN):
            env.store = st.with_task(env.store, task["id"], status=st.WAITING_FOR_OWNER)


def test_three_deaths_within_an_hour_write_the_outbox_item(tmp_path: Path) -> None:
    clock = [NOW]
    env, cfg = _claimed(tmp_path, ("T001", "T002", "T003"), clock)
    for minutes in (0, 20, 40):
        clock[0] = NOW + timedelta(minutes=minutes)
        if minutes:
            _reclaim(cfg, env, clock[0])
        out = io.StringIO()
        with redirect_stdout(out):
            _die(cfg, env)
        assert ("api-deaths" in out.getvalue()) == (minutes == 40)
        _park_the_dead(env)
    note = (tmp_path / f"{ad.OUTBOX_ID}.md").read_text(encoding="utf-8")
    assert "kind: info" in note and "3 worker sessions died" in note
    assert all(st.entry(env.store, t).get("attempts", 0) == 0 for t in ("T001", "T002", "T003"))
    assert not cs.check(env.store, lanes=1)


def test_deaths_older_than_the_hour_are_dropped_and_never_alarm(tmp_path: Path) -> None:
    state = st.new_state()
    for minutes in (0, 50, 120):
        state = ad.note(state, f"T00{minutes % 7}", "API Error 403", NOW + timedelta(minutes=minutes))
    assert [d["at"] for d in ad.recent(state, NOW + timedelta(minutes=120))] == [st.iso(NOW + timedelta(minutes=120))]
    assert len(state[ad.DEATHS]) == 1  # both older deaths were dropped at the third note
    bad = dict(state, **{ad.DEATHS: {"x": {"task": "T1", "at": "soon", "signature": "s"}, "y": "junk"}})
    assert ad.recent(bad, NOW) == []


def test_the_deaths_of_two_lanes_are_both_kept_on_a_race() -> None:
    base = st.new_state()
    ours = ad.note(base, "T001", "API Error 403", NOW)
    fresh = ad.note(base, "T002", "API Error 403", NOW)
    merged = cycle._ours_onto(base, ours, fresh)
    assert {d["task"] for d in merged[ad.DEATHS].values()} == {"T001", "T002"}


def test_secrets_in_the_error_text_never_reach_the_state_log_or_outbox(tmp_path: Path) -> None:
    clock = [NOW]
    env, cfg = _claimed(tmp_path, ("T001", "T002", "T003"), clock)
    for minutes in (0, 10, 20):
        clock[0] = NOW + timedelta(minutes=minutes)
        if minutes:
            _reclaim(cfg, env, clock[0])
        _die(cfg, env, ad.API_ERROR, ad.LOG + CF_403)  # a careless caller's raw text is relabelled
        _park_the_dead(env)
    assert st.entry(env.store, "T003")["attempts"] == 0 and len(env.store[ad.DEATHS]) == 3
    assert FAKE_SECRET not in json.dumps(env.store) and "cf-ray" not in json.dumps(env.store)
    note = (tmp_path / f"{ad.OUTBOX_ID}.md").read_text(encoding="utf-8")
    assert FAKE_SECRET not in note and "cf-ray" not in note


def test_a_counted_death_keeps_only_the_label_too(tmp_path: Path) -> None:
    env, cfg = _claimed(tmp_path)
    env.store = st.with_task(env.store, "T001", **{ad.RUN: ad.FREE_REQUEUES})
    assert _die(cfg, env, ad.API_ERROR, ad.LOG + CF_403) == st.QUEUED
    assert st.entry(env.store, "T001")["attempts"] == 1 and FAKE_SECRET not in json.dumps(env.store)


# ---- `--after`'s push check


class _Remote:
    def __init__(self, tip: str | None) -> None:
        self.tip, self.asked = tip, []  # type: list[str]

    def remote_tip_now(self, branch: str) -> str | None:
        self.asked.append(branch)
        return self.tip


def test_api_outcome_keeps_the_free_hand_in_only_with_the_branch_unmoved() -> None:
    hand = {"branch_prepared": "claude/task/T001-x", "remote_at_start": "abc123"}
    remote = _Remote("abc123")
    assert entry.api_outcome(ad.API_ERROR, _log(), hand, remote) == (ad.API_ERROR, _log())
    assert remote.asked == ["claude/task/T001-x"]
    assert entry.api_outcome(ad.API_ERROR, _log(), {**hand, "remote_at_start": ""}, _Remote("")) == (ad.API_ERROR, _log())
    for why, hand_, rem in (("pushed", hand, _Remote("def456")), ("unknown tip", hand, _Remote(None)),
                            ("pushed new", {**hand, "remote_at_start": ""}, _Remote("def456")),
                            ("not prepared", {**hand, "remote_at_start": None}, _Remote("abc123")),
                            ("no branch", {"remote_at_start": "abc123"}, _Remote("abc123")),
                            ("held hand-in", {**hand, "prehandin_holds": 1}, _Remote("abc123"))):
        with redirect_stdout(io.StringIO()):
            outcome, log = entry.api_outcome(ad.API_ERROR, _log(), hand_, rem)
        assert outcome == "tests_failed" and "the attempt counts" in log, why
    assert entry.api_outcome(ad.API_ERROR, _log(), hand, object())[0] == "tests_failed"  # an env without the reader
    assert entry.api_outcome("tests_failed", "x", hand, remote) == ("tests_failed", "x")


def test_an_api_death_with_the_branch_pushed_counts_as_today(tmp_path: Path) -> None:
    env, cfg = _claimed(tmp_path)
    with redirect_stdout(io.StringIO()):
        outcome, log = entry.api_outcome(ad.API_ERROR, _log(), {"branch_prepared": "b", "remote_at_start": ""}, _Remote("f00d"))
    assert _die(cfg, env, outcome, log) == st.QUEUED
    entry_ = st.entry(env.store, "T001")
    assert entry_["attempts"] == 1 and ad.RUN not in entry_ and "api_error" not in entry_["history"][-1]


def test_the_handoff_records_origins_branch_at_the_claim() -> None:
    class _Env:
        last_state = st.with_task(st.new_state(), "T001", status=st.IN_PROGRESS)
        dry_run = True
        prepared = {"branch": "claude/task/T001-x", "branch_ready": True, "base": "origin/claude/task/T001-x", "remote_sha": "abc"}

        def branch_for(self, tid: str) -> str:
            return "claude/task/T001-x"

    cfg = config([make_task("T001")], Path("."))
    info = entry.write_handoff(cfg, _Env())  # type: ignore[arg-type]  # reason: a stand-in env with only what the hand-off reads
    assert info["branch_prepared"] == "claude/task/T001-x" and info["remote_at_start"] == "abc"
    _Env.prepared = {"branch": "claude/task/T001-x", "branch_ready": True, "base": "main"}
    assert entry.write_handoff(cfg, _Env())["remote_at_start"] == ""  # type: ignore[arg-type]  # reason: as above
    _Env.prepared = {}
    assert entry.write_handoff(cfg, _Env())["remote_at_start"] is None  # type: ignore[arg-type]  # reason: as above


# ---- the workflow


def test_the_safety_step_hands_in_api_error_after_the_usage_limit_check() -> None:
    doc = yaml.safe_load((ROOT / ".github" / "workflows" / "backup-runner.yml").read_text(encoding="utf-8"))
    step = next(s for s in doc["jobs"]["backup"]["steps"] if s.get("id") == "safety")
    run = step["run"]
    assert 'sig="$(python3 -m runner.api_death "$EXECUTION_FILE")"' in run and "outcome=api_error" in run
    assert 'log="session died of an Anthropic API error: ${sig}"' in run and run.index("--at-limit") < run.index("runner.api_death")
    assert ad.LOG == "session died of an Anthropic API error: "
    case = next(line for line in run.splitlines() if line.strip().startswith('case "$outcome:$out"'))
    assert "api_error" not in case  # its lease is released, so the chain step starts the retry at once
