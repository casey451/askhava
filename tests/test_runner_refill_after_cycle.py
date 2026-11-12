"""Every run that ends refills the free lanes (D-3140), with the guards that keep it safe.

On November 11, 2026, from 22:34 to 22:49 lane 4 sat free with 20 claimable tasks while the rows the
runner wrote (`merge_pending` and `branch_updated` cycles on lanes 3 and 5, usage readings, the keeper's
record) all named it in `refill` and none started a run: only a hand-off started its refill runs itself
(D-723), and the chain step refilled only after a row that moved a task, which a job's usage-reading row
(written after the hand-in) never is. Here: the idle scenario through `entry.main` (it failed before the
fix and passes after it), a run that claims nothing starting nothing (no loop), a bookkeeping row
following its job's own last row, no refill while `main` is red or the pacer waits, never more lanes than
the effective count (`lanes_fallback`), the lease keeping a second run on a lane from claiming twice, the
chain step not starting a lane twice, and the log reader. Fake data only; one fixed clock (D-2886).
"""

from __future__ import annotations

import dataclasses
import json
import subprocess
import sys
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import pytest

from runner import __main__ as entry, cycle, lanes, state as st
from runner.github_env import GitHubEnv
from test_failure_drills import NOW, FakeEnv, config
from test_runner_two_lanes import _step, handed_off, staged


def _graph() -> list[dict[str, Any]]:
    return [staged(f"T00{n}", owns=[f"app/c{n}/"]) for n in range(1, 8)]


def _five(tmp_path: Path, hint: str | None, session: str = "actions-77") -> cycle.Config:
    return dataclasses.replace(config(_graph(), tmp_path, session), lanes=5, lane_hint=hint)


def _pending(state: st.State, tid: str, lane: str) -> st.State:
    """`tid` handed in on `lane`: MERGE_PENDING, its lease released, so it holds the lane by its `lane`."""
    return st.with_lease(st.with_task(st.claim(state, tid, "w", NOW, lane=lane), tid, status=st.MERGE_PENDING), lane, None)


def _november_11() -> st.State:
    """Lanes 1 and 2 hold live leases, lanes 3 and 5 a MERGE_PENDING task; lane 4 is free; T004, T006, T007 wait."""
    s = st.sync_graph(st.new_state(), _graph())
    s = st.claim(st.claim(s, "T001", "a", NOW, lane="1"), "T002", "b", NOW, lane="2")
    return _pending(_pending(s, "T003", "3"), "T005", "5")


class _Env:
    """The GitHubEnv surface `entry.main` uses: the state the run saw last, the log rows, the dispatches."""

    def __init__(self, state: st.State, rows: list[dict[str, Any]] | None = None) -> None:
        self.starter, self.runner_sha, self.log, self.first_state, self.last_state = "actions", "x", [], state, state
        self.rows, self.calls, self.written = list(rows or []), [], []

    def dispatch_lane(self, lane: str) -> bool:
        self.calls.append(lane)
        return True

    def read_cycle_log(self) -> list[dict[str, Any]]:
        return self.rows

    def write_cycle_log(self, report: dict[str, Any]) -> None:
        self.written.append(report)


def _main(monkeypatch: pytest.MonkeyPatch, cfg: cycle.Config, env: _Env, result: str, argv: list[str] | None = None) -> dict[str, Any]:
    """`entry.main` with the run stubbed to `result` and the real `refill` over `env.last_state` at NOW."""
    monkeypatch.setattr(entry, "build", lambda args, outbox: (cfg, env))
    monkeypatch.setattr(entry, "run", lambda args, c, e: result)
    monkeypatch.setattr(entry, "now_utc", lambda: NOW)
    monkeypatch.setattr(entry, "write_handoff", lambda c, e: {})
    monkeypatch.setattr(entry, "close_handoff", lambda r: None)
    assert entry.main(argv or []) == (2 if result == "state_invalid" else 0)
    return env.written[-1]


def test_the_november_11_free_lane_is_refilled_by_a_run_whose_own_lane_waits(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    s = _november_11()
    assert lanes.free_lanes(5, s, NOW) == ["4"] and cycle.refill_lanes(_five(tmp_path, "3"), s, NOW) == ["4"]
    for result in ("merge_pending", "branch_updated", "requeued", "requeued_after_plan_fix", st.BLOCKED, st.MERGE_PENDING):
        env = _Env(s)
        row = _main(monkeypatch, _five(tmp_path, "3"), env, result)
        assert env.calls == ["4"] and row["dispatched"] == ["4"] and row["refill"] == ["4"], result


def test_a_bookkeeping_row_refills_what_its_jobs_last_row_did_not(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    s, mine = _november_11(), {"session": "actions-77"}
    older = dict(mine, result="merge_pending", refill=["4"])  # a row an older runner wrote: it started nothing
    for quiet in ("keeper_armed: 144 minutes", "usage_reading_recorded: week 77%"):
        env = _Env(s, [older, dict(mine, result="renewed"), {"session": "actions-88", "result": "idle"}])
        assert _main(monkeypatch, _five(tmp_path, "3"), env, quiet)["dispatched"] == ["4"] and env.calls == ["4"]
    env = _Env(s, [dict(older, dispatched=["4"])])  # the row started lane 4 itself: not again
    assert "dispatched" not in _main(monkeypatch, _five(tmp_path, "3"), env, "keeper_armed: 144 minutes") and env.calls == []


def test_a_run_that_claimed_nothing_starts_nothing_so_refill_never_loops(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    s, mine = _november_11(), {"session": "actions-77"}
    for result in sorted(entry.REFILL_STOP) + ["requeue_refused: T006 is not BLOCKED"]:
        env = _Env(s)
        assert "dispatched" not in _main(monkeypatch, _five(tmp_path, "4"), env, result) and env.calls == [], result
        env = _Env(s, [dict(mine, result=result, refill=["4"])])  # nor the job's later bookkeeping rows
        assert "dispatched" not in _main(monkeypatch, _five(tmp_path, "4"), env, "keeper_armed: 20 minutes") and env.calls == []
    env = _Env(s, [])  # a bookkeeping row with no row of its own job before it: nothing is known, nothing starts
    assert "dispatched" not in _main(monkeypatch, _five(tmp_path, "4"), env, "keeper_armed: 20 minutes") and env.calls == []
    env = _Env(s)
    _main(monkeypatch, _five(tmp_path, "3"), env, "merge_pending", ["--dry-run"])
    assert env.calls == []


def test_refill_starts_follows_the_rules() -> None:
    def never() -> dict[str, Any]:
        raise AssertionError("a row that tells is decided by itself")

    assert entry.refill_starts("handed_off", ["4"], never) == ["4"]
    assert entry.refill_starts("done", [], never) == []
    assert entry.refill_starts("idle", ["4"], never) is None
    assert entry.refill_starts("T005 is MERGE_PENDING", ["4", "5"], lambda: {"result": "BLOCKED", "dispatched": ["5"]}) == ["4"]
    assert entry.refill_starts("renewed", ["4"], lambda: {"result": "BLOCKED", "dispatched": ["4"]}) is None
    assert entry.refill_starts("keeper_armed: 9 minutes", ["4"], lambda: {"result": "keeper_armed: 9 minutes"}) is None
    assert not entry.tells("") and not entry.tells("model_window_ok") and entry.tells("reserved D-3094")


def test_no_refill_while_main_is_red_or_a_fix_lane_is_needed(tmp_path: Path) -> None:
    cfg, s = _five(tmp_path, None), _november_11()
    red = st.with_task(dict(s, main_red={"at": st.iso(NOW), "source": "smoke"}), st.FIX_MAIN, status=st.QUEUED)
    assert st.eligible(red, NOW) == [st.FIX_MAIN] and lanes.free_lanes(5, red, NOW) == ["4"]
    assert cycle.refill_lanes(cfg, red, NOW) == []
    full = _pending(red, "T006", "4")  # every numbered lane busy: FIX-MAIN needs the "fix" lane
    assert lanes.fix_lane(full, NOW) == st.FIX_LANE and cycle.refill_lanes(cfg, full, NOW) == []
    assert cycle.refill_lanes(cfg, s, NOW) == ["4"]  # the same state with main green: lane 4


def test_no_refill_while_the_pacer_waits_and_never_past_the_effective_lanes(tmp_path: Path) -> None:
    cfg, s = _five(tmp_path, None), _november_11()
    ahead = st.iso(NOW.replace(hour=NOW.hour + 1))
    assert cycle.refill_lanes(cfg, dict(s, pacing={"decision": "wait", "waiting_until": ahead}), NOW) == []
    fallen = dict(s, lanes_fallback={"at": st.iso(NOW), "lane": "4", "why": []})
    assert cycle.refill_lanes(cfg, fallen, NOW) == []  # one lane in use, and lane 1 holds a lease
    quiet = st.sync_graph(st.new_state(), _graph())
    assert cycle.refill_lanes(cfg, dict(quiet, lanes_fallback={"at": st.iso(NOW), "lane": "4", "why": []}), NOW) == ["1"]
    assert cycle.refill_lanes(cfg, _pending(s, "T006", "4"), NOW) == []  # five in flight: no lane to fill


def test_two_runs_started_for_one_lane_claim_once(tmp_path: Path) -> None:
    env = FakeEnv(handed_off)
    env.store = _november_11()
    assert cycle.run_cycle(_five(tmp_path, "4", "s-first"), env, NOW) == "handed_off"
    second = cycle.run_cycle(_five(tmp_path, "4", "s-second"), env, NOW)
    assert second != "handed_off"  # the lease holds lane 4: the second run claims nothing
    assert len(env.runs) == 1 and st.live_leases(env.store, NOW)["4"]["task"] == env.runs[0][0]
    left = cycle.refill_lanes(_five(tmp_path, None), env.store, NOW)
    assert left == [] and entry.refill_starts(second, left, dict) == []  # and it starts no further run


def _chain(rows: list[dict[str, Any]]) -> str:
    script = _step("backup", "Start the next cycle")["run"]
    code = script.split("refill=\"$(printf '%s' \"$log\" | SESSION=\"actions-${GITHUB_RUN_ID}\" python3 -c '", 1)[1].split("\n' ", 1)[0]
    env = {"JOB_START": "2026-11-01T12:00:00", "SESSION": "actions-77", "PATH": "/usr/bin:/bin"}
    stdin = "".join(json.dumps(r) + "\n" for r in rows)
    return subprocess.run([sys.executable, "-c", code], input=stdin, capture_output=True, text=True, env=env, check=True).stdout.strip()


def test_the_chain_step_never_starts_a_lane_its_row_already_started() -> None:
    row = {"at": "2026-11-01T12:05:00", "session": "actions-77", "result": "BLOCKED", "refill": ["1", "4"]}
    assert _chain([row]) == "1 4"  # an older runner's row: the chain step starts both, as before
    assert _chain([dict(row, dispatched=["4"])]) == "1"
    assert _chain([dict(row, dispatched=["1", "4"])]) == ""


def test_the_cycle_log_reader_and_job_row() -> None:
    text = '{"session": "actions-77", "result": "merge_pending"}\nnot json\n[1]\n{"session": "actions-77", "result": "renewed"}\n'
    fake = SimpleNamespace(state_head="h", _show=lambda commit, path: text if path == "kit/runner_log.jsonl" else None)
    rows = GitHubEnv.read_cycle_log(fake)  # type: ignore[arg-type]
    assert [r["result"] for r in rows] == ["merge_pending", "renewed"]
    assert GitHubEnv.read_cycle_log(SimpleNamespace(state_head=None, _show=lambda c, p: None)) == []  # type: ignore[arg-type]
    assert entry.job_row(SimpleNamespace(read_cycle_log=lambda: rows), "actions-77")["result"] == "merge_pending"  # type: ignore[arg-type]
    assert entry.job_row(SimpleNamespace(read_cycle_log=lambda: rows), "actions-88") == {}  # type: ignore[arg-type]

    def broken() -> list[dict[str, Any]]:
        raise RuntimeError("git failed")

    assert entry.job_row(SimpleNamespace(read_cycle_log=broken), "actions-77") == {}  # type: ignore[arg-type]
