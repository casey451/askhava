"""The merge line's round robin (D-4392): a task whose turn ran out goes to the back of the line, not out of it.

Under D-3418 alone a spent turn made the task "late" for good: it updated only in a cycle where no "line" task
held the turn, and with several lanes handing in that never came (T623, PR #1007, November 18). Now a spent task
is re-queued at the moment its turn ended (`_merge_line` with `now`), takes a fresh turn when it comes round
(`_mark_turn`), and a turn ends at MERGE_LINE_TURN_CAP however often its branch moved. "Late" means only a
ruleset-red head. Over test_runner_merge_line's fakes. Fake data only.
"""

from __future__ import annotations

from datetime import timedelta
from pathlib import Path
from typing import Any

import pytest

from runner import cycle, state as st
from test_failure_drills import NOW, config, green_pr, make_task
from test_runner_merge_line import LineEnv, branch, current_and_green, head, open_pr, pending, resolve, three_behind

LIMIT = int(cycle.MERGE_LINE_TURN_LIMIT.total_seconds() // 60)
CAP = int(cycle.MERGE_LINE_TURN_CAP.total_seconds() // 60)


def since(state: st.State, tid: str) -> Any:
    return st.entry(state, tid).get("merge_turn_since")


def at(minutes: int) -> str:
    return st.iso(NOW + timedelta(minutes=minutes))


class Sim:
    """A line of tasks over LineEnv: `main` has a version; a branch is behind until its update merge pushed."""

    def __init__(self, tmp_path: Path, ids: list[str]) -> None:
        self.env, self.cfg = LineEnv(), config([make_task(t) for t in ids], tmp_path)
        self.state, self.main, self.version = st.new_state(), 0, {}  # type: ignore[var-annotated]
        self.number = 0

    def hand_in(self, tid: str, minutes: int) -> None:
        self.number += 1
        self.state = pending(self.state, tid, minutes, tid)
        self.env.prs[tid] = open_pr(tid, self.number)
        self.version[tid] = -1

    def merge(self, tid: str) -> None:
        sha = str(st.entry(self.state, tid)["reviewed_sha"])
        self.env.prs[tid] = dict(green_pr(sha), number=99, merged_sha=f"sq-{tid}")
        self.main += 1

    def cycle(self, minutes: int) -> dict[str, str]:
        for tid, version in self.version.items():
            current = version == self.main
            self.env.by_branch[branch(tid)] = {"status": "current"} if current else \
                {"status": "pushed", "sha": f"m-{tid.lower()}-{minutes}", "main": f"main{self.main}"}
        self.env.updates.clear()
        self.state, results = resolve(self.env, self.cfg, self.state, minutes)
        for tid, result in results.items():
            if result == "branch_updated":
                self.version[tid] = self.main
                self.env.prs[tid] = dict(open_pr(tid, self.env.prs[tid]["number"]), head_sha=st.entry(self.state, tid)["reviewed_sha"])
        return results

    def holders(self, minutes: int) -> list[str]:
        """MERGE_PENDING tasks whose turn is not spent: at most one (one turn flag)."""
        now = NOW + timedelta(minutes=minutes)
        return [t["id"] for t in self.state["tasks"] if t.get("status") == st.MERGE_PENDING
                and t.get("merge_turn_since") and not cycle._turn_spent(t, now)]


def t623_run(tmp_path: Path) -> tuple[Sim, int, list[str]]:
    """T623 handed in at minute 0 takes the turn and is updated; no cycle until minute 50 while main moves and four
    tasks hand in; then a cycle every 5 minutes, a hand-in every 10 and each holder merged 20 minutes into its turn.
    Returns the run stopped at T623's fresh turn, that minute, and the tasks that left before it."""
    later = [f"T{800 + n}" for n in range(30)]
    sim = Sim(tmp_path, ["T623", "T610", "T620", "T630", "T640", *later])
    sim.hand_in("T623", 0)
    assert sim.cycle(0)["T623"] == "branch_updated" and since(sim.state, "T623") == at(0)
    sim.main += 1
    for tid, minutes in (("T610", 10), ("T620", 20), ("T630", 30), ("T640", 40)):
        sim.hand_in(tid, minutes)
    left: list[str] = []
    for minutes in range(50, 300, 5):
        if minutes % 10 == 0:
            sim.hand_in(later[(minutes - 50) // 10], minutes)
        for tid in sim.holders(minutes):
            if st.parse(str(since(sim.state, tid))) >= NOW + timedelta(minutes=50) and \
                    NOW + timedelta(minutes=minutes) - st.parse(str(since(sim.state, tid))) >= timedelta(minutes=20):
                sim.merge(tid)
                left.append(tid)
        held = {t: since(sim.state, t) for t in sim.holders(minutes)}
        results = sim.cycle(minutes)
        assert len(sim.holders(minutes)) <= 1
        assert list(results.values()).count("branch_updated") <= 1
        if "T623" not in sim.holders(minutes):
            assert since(sim.state, "T623") == at(0)  # re-queued: keeps its place, never preempts
        for tid in held.keys() & set(sim.holders(minutes)):
            assert since(sim.state, tid) == held[tid]  # the holder's turn time does not change across cycles
        if results.get("T623") == "branch_updated":
            return sim, minutes, left
    raise AssertionError("T623 never got an update merge (D-4392)")


def test_t623_is_requeued_and_updated_after_the_tasks_ahead_have_held(tmp_path: Path) -> None:
    sim, minutes, left = t623_run(tmp_path)
    assert left == ["T610", "T620", "T630", "T640"]
    assert minutes <= 130 + 5 and since(sim.state, "T623") == at(minutes) and sim.holders(minutes) == ["T623"]
    for task in sim.state["tasks"]:
        if task["id"].startswith("T8") and task.get("status") == st.MERGE_PENDING:
            assert not task.get("merge_turn_since") and not task.get("update_merges")


def test_a_holder_updated_every_20_minutes_gives_up_the_turn_at_the_cap(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    state, _ = resolve(env, cfg, state)  # T003 takes the turn at minute 30 and is updated
    for minutes in range(50, 30 + CAP, 20):
        env.prs["T003"] = dict(open_pr("T003", 3), head_sha=st.entry(state, "T003")["reviewed_sha"])
        env.by_branch[branch("T003")] = {"status": "pushed", "sha": f"m-t003-{minutes}", "main": f"main{minutes}"}
        state, results = resolve(env, cfg, state, minutes)
        assert results["T003"] == "branch_updated" and since(state, "T003") == at(30)
    env.prs["T003"] = dict(open_pr("T003", 3), head_sha=st.entry(state, "T003")["reviewed_sha"])
    env.by_branch[branch("T003")] = {"status": "current"}
    env.updates.clear()
    state, results = resolve(env, cfg, state, 30 + CAP - 1)
    assert since(state, "T003") == at(30) and results["T001"] == "merge_pending"
    out, results = resolve(env, cfg, state, 30 + CAP)
    assert results["T001"] == "branch_updated" and since(out, "T001") == at(30 + CAP)
    assert since(out, "T003") == at(30) and st.status(out, "T003") == st.MERGE_PENDING


@pytest.mark.parametrize("moved", [True, False], ids=["new-head", "no-new-head"])
def test_a_holder_whose_checks_never_run_gives_up_the_line_each_round(tmp_path: Path, moved: bool) -> None:
    env, cfg, state = three_behind(tmp_path)
    state, _ = resolve(env, cfg, state)  # T003 updated to m-t003 at minute 30; its checks never start
    env.prs["T003"] = dict(open_pr("T003", 3), head_sha="m-t003")
    env.by_branch[branch("T003")] = {"status": "current"}
    state, results = resolve(env, cfg, state, 30 + LIMIT)
    assert results["T001"] == "branch_updated" and since(state, "T003") == at(30)
    state, results = resolve(env, cfg, state, 80)  # T001 still holds: T003 waits
    assert results["T003"] == "merge_pending" and since(state, "T003") == at(30)
    env.prs["T001"] = dict(green_pr(str(st.entry(state, "T001")["reviewed_sha"])), number=9, merged_sha="sq-T001")
    state, results = resolve(env, cfg, state, 85)
    assert since(state, "T002") == at(85) and since(state, "T003") == at(30)
    state = pending(state, "T004", 100, "4")
    env.prs["T004"] = open_pr("T004", 4)
    env.by_branch[branch("T004")] = {"status": "pushed", "sha": "m-t004", "main": "main9"}
    if moved:
        env.by_branch[branch("T003")] = {"status": "pushed", "sha": "m-t003-2", "main": "main9"}
    env.prs["T002"] = dict(green_pr(str(st.entry(state, "T002")["reviewed_sha"])), number=9, merged_sha="sq-T002")
    state, results = resolve(env, cfg, state, 95)
    assert since(state, "T003") == at(95) and results["T003"] == ("branch_updated" if moved else "merge_pending")
    if moved:
        env.prs["T003"] = dict(open_pr("T003", 3), head_sha="m-t003-2")
        env.by_branch[branch("T003")] = {"status": "current"}
    state, results = resolve(env, cfg, state, 95 + LIMIT - 1)
    assert since(state, "T003") == at(95) and results["T004"] == "merge_pending"
    out, results = resolve(env, cfg, state, 95 + LIMIT)
    assert results["T004"] == "branch_updated" and since(out, "T003") == at(95)


def test_a_spent_task_alone_in_the_line_takes_a_fresh_turn_and_its_update_at_once(tmp_path: Path) -> None:
    env, cfg = LineEnv(), config([make_task("T003")], tmp_path)
    state = st.with_task(pending(st.new_state(), "T003", 0, "3"), "T003", merge_turn_since=at(0))
    env.prs["T003"] = open_pr("T003", 3)
    env.by_branch[branch("T003")] = {"status": "pushed", "sha": "m-t003", "main": "main0002"}
    out, results = resolve(env, cfg, state, 60)
    assert results["T003"] == "branch_updated" and since(out, "T003") == at(60)


def test_a_ruleset_red_task_with_a_spent_turn_stays_late(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    state = st.with_task(state, "T003", merge_turn_since=at(-60))
    current_and_green(env, "T003", head("T003"))
    env.prs["T003"]["checks"].append({"name": "Governance", "sha": head("T003"), "conclusion": "failure"})
    env.by_branch[branch("T003")] = {"status": "pushed", "sha": "m-t003", "main": "main0002"}
    for minutes in (30, 200):  # its line time is long past T001's and T002's, yet it never holds
        env.updates.clear()
        state, results = resolve(env, cfg, state, minutes)
        assert env.updates == [(branch("T001"), head("T001"))] or minutes == 200
        assert results["T003"] == "merge_pending" and all(b != branch("T003") for b, _ in env.updates)
    alone = st.new_state()
    alone["tasks"] = [st.entry(state, "T003")]
    env.updates.clear()
    _, results = resolve(env, cfg, alone, 210)
    assert env.updates == [(branch("T003"), head("T003"))] and results["T003"] == "branch_updated"


def test_fix_main_comes_first_and_a_red_main_moves_nothing_else(tmp_path: Path) -> None:
    env, cfg, state = three_behind(tmp_path)
    state = st.with_task(state, "T003", merge_turn_since=at(-60))  # re-queued long before FIX-MAIN's hand-in
    state = pending(state, st.FIX_MAIN, 25, "fix")
    env.prs[st.FIX_MAIN] = open_pr(st.FIX_MAIN, 9)
    env.by_branch[branch(st.FIX_MAIN)] = {"status": "pushed", "sha": "m-fix", "main": "main0002"}
    assert cycle._merge_line(state, NOW + timedelta(minutes=30))[0] == st.FIX_MAIN
    _, results = resolve(env, cfg, state)
    assert env.updates == [(branch(st.FIX_MAIN), head(st.FIX_MAIN))] and results[st.FIX_MAIN] == "branch_updated"
    del env.prs[st.FIX_MAIN]
    red = dict(st.with_task(state, st.FIX_MAIN, status=st.DONE), main_red={"check": "Ci", "since": at(0)})
    env.updates.clear()
    env.auto_merges.clear()
    out, results = resolve(env, cfg, red)
    assert env.updates == [] and env.auto_merges == [] and set(results.values()) == {"merge_pending"}
    assert out == red


def test_the_line_puts_a_spent_task_at_its_turn_end_compared_as_times(tmp_path: Path) -> None:
    state = pending(pending(st.new_state(), "T001", 10, "1"), "T002", 50, "2")
    state = pending(state, "T003", 0, "3")
    state = st.with_task(state, "T003", merge_turn_since=at(0))  # its turn ended at minute 45
    assert cycle._merge_line(state) == ["T003", "T001", "T002"]
    assert cycle._merge_line(state, NOW + timedelta(minutes=60)) == ["T001", "T003", "T002"]
