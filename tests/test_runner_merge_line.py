"""The merge line (D-3418): when `main` moves, the runner brings only one MERGE_PENDING branch up to date.

Before D-3418 `_resolve_merges` merged `main` into every behind branch each cycle, restarting Ci, Twin and Ally
on all of them while only one could merge next (about 1.9 wasted runs per merge). Now the turn goes to the oldest
hand-in (FIX-MAIN first) whose pull request is open, not red and not stuck; the others skip the update merge but
still get auto-merge. Over test_failure_drills.FakeEnv with a per-branch `update_branch` answer. Fake data only.
"""

from __future__ import annotations

import copy
from datetime import timedelta
from pathlib import Path
from typing import Any

from runner import cycle, state as st
from test_failure_drills import NOW, FakeEnv, config, green_pr, make_task


class LineEnv(FakeEnv):
    """FakeEnv whose `update_branch` answers per branch (default: already current)."""

    def __init__(self) -> None:
        super().__init__()
        self.by_branch: dict[str, dict[str, Any]] = {}

    def update_branch(self, branch: str, reviewed: str) -> dict[str, Any]:
        self.updates.append((branch, reviewed))
        return copy.deepcopy(self.by_branch.get(branch, {"status": "current"}))


def branch(tid: str) -> str:
    return f"claude/task/{tid}-x"


def head(tid: str) -> str:
    return f"head-{tid.lower()}"


def pending(state: st.State, tid: str, minutes: int, lane: str) -> st.State:
    """`tid` MERGE_PENDING on `lane`, handed in `minutes` after NOW, its reviewed head pushed."""
    ended = st.iso(NOW + timedelta(minutes=minutes))
    return st.with_task(state, tid, status=st.MERGE_PENDING, branch=branch(tid), reviewed_sha=head(tid), lane=lane,
                        review_lease={"task": tid, "lane": lane}, history=[{"attempt": 1, "ended_at": ended}])


def open_pr(tid: str, number: int, checks: str = "pending", auto_merge: bool = False) -> dict[str, Any]:
    """An open pull request of the task's reviewed head: checks running, green or failed."""
    pr = dict(green_pr(head(tid), merged=False), number=number, state="open", auto_merge=auto_merge)
    if checks == "pending":
        pr["checks"] = []
    elif checks == "failed":
        pr["checks"] = [{"name": n, "sha": head(tid), "conclusion": "failure" if n == "Ci" else "success"} for n in ("Ci", "Twin", "Ally")]
        pr["failure_log"] = "Ci: failure"
    return pr


def three_behind(tmp_path: Path) -> tuple[LineEnv, cycle.Config, st.State]:
    """T003 handed in first, then T001, then T002; all three open and behind main."""
    env, cfg = LineEnv(), config([make_task(t) for t in ("T001", "T002", "T003")], tmp_path)
    state = st.new_state()
    for tid, minutes, lane, number in (("T003", 0, "3", 3), ("T001", 10, "1", 1), ("T002", 20, "2", 2)):
        state = pending(state, tid, minutes, lane)
        env.prs[tid] = open_pr(tid, number)
        env.by_branch[branch(tid)] = {"status": "pushed", "sha": f"m-{tid.lower()}", "main": "main0002"}
    return env, cfg, state


def resolve(env: LineEnv, cfg: cycle.Config, state: st.State, minutes: int = 30) -> tuple[st.State, dict[str, str]]:
    out, outcomes = cycle._resolve_merges(state, env, cfg, NOW + timedelta(minutes=minutes))
    return out, {tid: result for tid, _, result in outcomes}


def test_only_the_oldest_hand_in_is_updated_and_the_others_wait_untouched(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    out, results = resolve(env, cfg, state)
    assert env.updates == [(branch("T003"), head("T003"))]
    assert results == {"T001": "merge_pending", "T002": "merge_pending", "T003": "branch_updated"}
    assert st.entry(out, "T003")["reviewed_sha"] == "m-t003"
    for tid in ("T001", "T002"):  # never tried, so no update wait starts on them
        assert st.entry(out, tid)["reviewed_sha"] == head(tid) and not st.entry(out, tid).get("update_waiting_since")
    assert [t for t, _, _ in cycle._resolve_merges(state, copy.deepcopy(env), cfg, NOW)[1]] == ["T003", "T001", "T002"]


def test_a_red_oldest_leaves_the_line_and_the_next_is_updated_in_the_same_cycle(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    env.prs["T003"] = open_pr("T003", 3, checks="failed")
    out, results = resolve(env, cfg, state)
    assert results["T003"] == "check_failed" and st.status(out, "T003") == st.QUEUED
    assert env.updates == [(branch("T001"), head("T001"))] and results["T001"] == "branch_updated"


def test_a_conflicting_oldest_goes_to_resync_and_the_next_is_updated(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    env.by_branch[branch("T003")] = {"status": "conflict", "paths": ["app/t003/x.py"], "main": "main0002"}
    out, results = resolve(env, cfg, state)
    assert results["T003"] == cycle.RESYNC and st.status(out, "T003") == st.QUEUED and st.entry(out, "T003")["resync"]
    assert env.updates == [(branch("T003"), head("T003")), (branch("T001"), head("T001"))]
    assert results["T001"] == "branch_updated" and results["T002"] == "merge_pending"


def test_a_task_off_its_turn_already_current_still_gets_auto_merge(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    env.prs["T002"] = open_pr("T002", 2, checks="green")
    out, _ = resolve(env, cfg, state)
    assert (branch("T002"), head("T002")) in env.auto_merges and st.entry(out, "T002")["auto_merge"]["enabled"] is True
    assert all(b != branch("T002") for b, _ in env.updates)


def test_once_the_turn_task_merges_the_next_oldest_is_updated(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    state, _ = resolve(env, cfg, state)
    env.prs["T003"] = dict(green_pr("m-t003"), number=3, merged_sha="sq0003")
    env.updates.clear()
    out, results = resolve(env, cfg, state, minutes=50)
    assert results["T003"] == "done" and st.status(out, "T003") == st.DONE
    assert env.updates == [(branch("T001"), head("T001"))] and results["T001"] == "branch_updated"
    assert results["T002"] == "merge_pending"


def test_while_main_is_red_nothing_but_fix_main_moves(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    state = dict(state, main_red={"check": "Ci", "since": st.iso(NOW)})
    out, results = resolve(env, cfg, state)
    assert env.updates == [] and env.auto_merges == []
    assert set(results.values()) == {"merge_pending"} and out == state


def test_fix_main_takes_the_turn_before_any_older_hand_in(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    state = pending(state, st.FIX_MAIN, 25, "fix")
    env.prs[st.FIX_MAIN] = open_pr(st.FIX_MAIN, 9)
    env.by_branch[branch(st.FIX_MAIN)] = {"status": "pushed", "sha": "m-fix", "main": "main0002"}
    _, results = resolve(env, cfg, state)
    assert env.updates == [(branch(st.FIX_MAIN), head(st.FIX_MAIN))] and results[st.FIX_MAIN] == "branch_updated"


def test_a_stuck_turn_branch_keeps_trying_but_no_longer_holds_the_line(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    env.by_branch[branch("T003")] = {"status": "error", "why": "fake git error"}
    state, results = resolve(env, cfg, state)
    assert results["T003"] == "merge_pending" and env.updates == [(branch("T003"), head("T003"))]
    env.updates.clear()
    state, _ = resolve(env, cfg, state, minutes=30 + 29)  # under MERGE_LINE_STUCK: still its turn
    assert env.updates == [(branch("T003"), head("T003"))]
    env.updates.clear()
    stuck_at = 30 + int(cycle.MERGE_LINE_STUCK.total_seconds() // 60)
    out, results = resolve(env, cfg, state, minutes=stuck_at)
    assert env.updates == [(branch("T003"), head("T003")), (branch("T001"), head("T001"))]
    assert results["T001"] == "branch_updated" and st.status(out, "T003") == st.MERGE_PENDING


def test_a_task_without_a_pull_request_never_holds_the_line(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    del env.prs["T003"]
    _, results = resolve(env, cfg, state)
    assert results["T003"] == "merge_pending" and env.updates == [(branch("T001"), head("T001"))]


def test_the_line_is_ordered_by_a_hand_in_update_merge_then_by_id(tmp_path: Path) -> None:
    state = pending(pending(st.new_state(), "T002", 5, "2"), "T001", 5, "1")
    state = st.with_task(state, "T009", status=st.MERGE_PENDING)  # no hand-in time recorded: last
    early = {"from": "x", "to": "y", "how": "hand-in", "at": st.iso(NOW + timedelta(minutes=1))}
    state = pending(state, "T005", 30, "5")
    state = st.with_task(state, "T005", update_merges=[early])
    assert cycle._merge_line(state) == ["T005", "T001", "T002", "T009"]


def current_and_green(env: LineEnv, tid: str, sha: str, auto_merge: bool = True) -> None:
    """The task's branch is current with main and its pull request shows `sha` with Ci, Twin and Ally green."""
    env.prs[tid] = dict(green_pr(sha, merged=False), number=3, state="open", auto_merge=auto_merge)
    env.by_branch[branch(tid)] = {"status": "current"}


def test_a_green_current_turn_holder_that_never_merges_gives_up_the_line_at_the_limit(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    current_and_green(env, "T003", head("T003"))
    state, _ = resolve(env, cfg, state)  # T003 takes the turn at minute 30: nothing to push
    assert st.entry(state, "T003")["merge_turn_since"] == st.iso(NOW + timedelta(minutes=30))
    assert env.updates == [(branch("T003"), head("T003"))]
    limit = int(cycle.MERGE_LINE_TURN_LIMIT.total_seconds() // 60)
    env.updates.clear()
    state, _ = resolve(env, cfg, state, minutes=30 + limit - 1)  # just under the limit: it still holds the turn
    assert env.updates == [(branch("T003"), head("T003"))]
    env.updates.clear()
    out, results = resolve(env, cfg, state, minutes=30 + limit)
    assert env.updates == [(branch("T001"), head("T001"))] and results["T001"] == "branch_updated"
    assert st.status(out, "T003") == st.MERGE_PENDING and results["T002"] == "merge_pending"


def test_a_turn_holder_whose_checks_never_run_gives_up_the_line_after_its_update(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    state, _ = resolve(env, cfg, state)  # T003 updated to m-t003 at minute 30
    env.prs["T003"] = dict(open_pr("T003", 3), head_sha="m-t003")  # its checks never start
    env.by_branch[branch("T003")] = {"status": "current"}
    limit = int(cycle.MERGE_LINE_TURN_LIMIT.total_seconds() // 60)
    env.updates.clear()
    state, _ = resolve(env, cfg, state, minutes=30 + limit - 1)
    assert env.updates == [(branch("T003"), "m-t003")]
    env.updates.clear()
    _, results = resolve(env, cfg, state, minutes=30 + limit)
    assert env.updates == [(branch("T001"), head("T001"))] and results["T001"] == "branch_updated"


def test_a_red_governance_check_never_holds_the_line_but_updates_when_no_one_else_needs_the_turn(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    current_and_green(env, "T003", head("T003"))
    env.prs["T003"]["checks"].append({"name": "Governance", "sha": head("T003"), "conclusion": "failure"})
    out, results = resolve(env, cfg, state)
    assert env.updates == [(branch("T001"), head("T001"))] and st.status(out, "T003") == st.MERGE_PENDING
    alone = st.new_state()
    alone["tasks"] = [st.entry(state, "T003")]
    env.updates.clear()
    env.by_branch[branch("T003")] = {"status": "pushed", "sha": "m-t003", "main": "main0002"}
    _, results = resolve(env, cfg, alone)
    assert env.updates == [(branch("T003"), head("T003"))] and results["T003"] == "branch_updated"


def test_a_turn_holder_preempted_by_fix_main_loses_its_turn_time(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    state, _ = resolve(env, cfg, state)
    assert st.entry(state, "T003")["merge_turn_since"]
    state = pending(state, st.FIX_MAIN, 40, "fix")
    env.prs[st.FIX_MAIN] = open_pr(st.FIX_MAIN, 9)
    out, _ = resolve(env, cfg, state, minutes=41)
    assert st.entry(out, st.FIX_MAIN)["merge_turn_since"] and not st.entry(out, "T003").get("merge_turn_since")
