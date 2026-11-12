"""Merged tasks are closed before the smoke warm-up (D-3141), by the cycle's own DONE rule.

`python3 -m runner --resolve-merges` (`cycle.resolve_merged`) runs as backup-runner.yml's first runner step, so a
merge frees its lane and its dependents within a minute of the job's start instead of after the warm-up and the
pre-session smoke (T388: merged 22:53Z, DONE 23:19Z on November 11). These tests prove it closes a merged task
without any smoke, never closes one the cycle would not (same state as `_resolve_merges` on the same inputs:
Ci, Twin and Ally green on the merged head, equal trees, a reviewed head; otherwise BLOCKED and a red main),
leaves open pull requests and running checks to the cycle, and that its result reaches the refill and chain logic.
"""

from __future__ import annotations

import copy
import dataclasses
from datetime import timedelta
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import pytest

from runner import __main__ as entry, cycle, state as st
from test_failure_drills import NOW, FakeEnv, config, green_pr as any_green_pr, make_task, reviewed
from test_runner_two_lanes import _refill, _step, _workflow, by_head, head_of, staged, two_lane


HEAD = "head0001"  # the head the fake reviewer approved (test_failure_drills.reviewed)


def green_pr(merged: bool = True) -> dict[str, Any]:
    """A pull request of the reviewed head with Ci, Twin and Ally green on it and equal trees."""
    return dict(any_green_pr(HEAD, merged=merged), merged_sha="sq0001")


def _pending(tmp_path: Path, graph: list[dict[str, Any]] | None = None) -> tuple[FakeEnv, cycle.Config]:
    """T001 handed in and MERGE_PENDING on one lane; T002 depends on it."""
    graph = graph or [make_task("T001"), make_task("T002", deps=["T001"])]
    env, cfg = FakeEnv(lambda t, m, log: reviewed(t, m)), config(graph, tmp_path)
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING
    return env, cfg


def _cycle_would(env: FakeEnv, cfg: cycle.Config, now: Any) -> st.State:
    """What the cycle's own merge step makes of the same state and pull requests."""
    twin = copy.deepcopy(env)
    state, _ = cycle._resolve_merges(twin.read_state(), twin, cfg, now)
    return state


def test_a_merged_green_task_is_done_before_any_smoke_and_its_dependent_is_claimed_next(tmp_path: Path) -> None:
    env, cfg = _pending(tmp_path)
    env.prs["T001"] = green_pr()
    smokes, pushes = list(env.smokes), env.pushes
    assert cycle.resolve_merged(cfg, env, NOW + timedelta(minutes=1)) == cycle.MERGES_RESOLVED
    assert st.status(env.store, "T001") == st.DONE and st.entry(env.store, "T001")["merged_sha"] == "sq0001"
    assert env.smokes == smokes and env.pushes == pushes + 1 and not env.updates  # no smoke, no claim, one write
    assert st.status(env.store, "T002") == st.QUEUED and not st.live_leases(env.store, NOW + timedelta(minutes=1))
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=20)) == st.MERGE_PENDING  # the session's cycle: T002
    assert st.status(env.store, "T002") == st.MERGE_PENDING


@pytest.mark.parametrize("pr", [
    pytest.param(dict(green_pr(), merge_tree="other"), id="squash-tree-differs"),
    pytest.param(dict(green_pr(), checks=[{"name": n, "sha": HEAD, "conclusion": "failure" if n == "Twin" else "success"}
                                          for n in ("Ci", "Twin", "Ally")]), id="a-required-check-failed"),
    pytest.param(dict(green_pr(), head_sha="other00", checks=[{"name": n, "sha": "other00", "conclusion": "success"}
                                                              for n in ("Ci", "Twin", "Ally")]), id="head-nobody-reviewed"),
])
def test_never_done_where_the_cycle_would_not_be_and_main_goes_red_the_same_way(tmp_path: Path, pr: dict[str, Any]) -> None:
    env, cfg = _pending(tmp_path)
    env.prs["T001"] = pr
    env.update_merges = False  # a differing head is not reviewed work plus main (D-584)
    later = NOW + timedelta(minutes=1)
    expected = _cycle_would(env, cfg, later)
    assert cycle.resolve_merged(cfg, env, later) == cycle.MERGES_RESOLVED
    assert env.store == expected  # byte for byte what the cycle would have written
    assert st.status(env.store, "T001") == st.BLOCKED and env.store["main_red"]["task"] == "T001"
    assert st.status(env.store, st.FIX_MAIN) == st.QUEUED


def test_the_green_case_matches_the_cycle_too(tmp_path: Path) -> None:
    env, cfg = _pending(tmp_path)
    env.prs["T001"] = green_pr()
    expected = _cycle_would(env, cfg, NOW)
    assert cycle.resolve_merged(cfg, env, NOW) == cycle.MERGES_RESOLVED and env.store == expected


@pytest.mark.parametrize("pr", [
    pytest.param(None, id="no-pull-request"),
    pytest.param(dict(green_pr(merged=False), state="open", number=7, auto_merge=True), id="open"),
    pytest.param(dict(green_pr(), checks=[]), id="checks-still-running"),
    pytest.param(dict(green_pr(), checks=[{"name": "Ci", "sha": HEAD, "conclusion": None}]), id="a-check-running"),
])
def test_open_absent_or_still_checking_is_left_to_the_cycle_with_nothing_written(tmp_path: Path, pr: dict[str, Any] | None) -> None:
    env, cfg = _pending(tmp_path)
    if pr is not None:
        env.prs["T001"] = pr
    env.update = {"status": "pushed", "sha": "m1", "main": "main0002"}
    before, pushes = copy.deepcopy(env.store), env.pushes
    assert cycle.resolve_merged(cfg, env, NOW + timedelta(minutes=1)) == cycle.NO_MERGED
    assert env.store == before and env.pushes == pushes and not env.updates and not env.closed_prs


def test_a_merged_head_git_cannot_place_starts_no_wait_here(tmp_path: Path) -> None:
    env, cfg = _pending(tmp_path)
    env.prs["T001"] = dict(green_pr(), head_sha="other00",
                           checks=[{"name": n, "sha": "other00", "conclusion": "success"} for n in ("Ci", "Twin", "Ally")])
    env.update_merges = None  # git cannot say (D-584): the cycle starts the bounded wait, not this step
    before = copy.deepcopy(env.store)
    assert cycle.resolve_merged(cfg, env, NOW) == cycle.NO_MERGED and env.store == before


def test_a_lost_race_changes_nothing_and_the_cycle_resolves_it_later(tmp_path: Path) -> None:
    env, cfg = _pending(tmp_path)
    env.prs["T001"] = green_pr()
    real_push = env.push_state
    env.push_state = lambda new, base: False  # type: ignore[method-assign]
    before = copy.deepcopy(env.store)
    assert cycle.resolve_merged(cfg, env, NOW) == "lost_race" and env.store == before
    env.push_state = real_push  # type: ignore[method-assign]
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=5)) in (st.MERGE_PENDING, "done")
    assert st.status(env.store, "T001") == st.DONE


def test_two_lanes_close_every_merged_lane_and_leave_an_open_one(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"]), staged("T003", owns=["app/c/"])]
    env = FakeEnv(by_head)
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s1", "1"), env, NOW) == st.MERGE_PENDING
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s2", "2"), env, NOW + timedelta(minutes=5)) == st.MERGE_PENDING
    env.prs["T001"] = dict(any_green_pr(head_of("T001")), number=1)
    env.prs["T002"] = dict(any_green_pr(head_of("T002"), merged=False), number=2, state="open", auto_merge=True)
    cfg = two_lane(graph, tmp_path, "s3", "1")
    assert cycle.resolve_merged(cfg, env, NOW + timedelta(minutes=10)) == cycle.MERGES_RESOLVED
    assert st.status(env.store, "T001") == st.DONE and st.status(env.store, "T002") == st.MERGE_PENDING
    assert not env.updates  # the open one's update merge stays the cycle's
    assert cycle.refill_lanes(cfg, env.store, NOW + timedelta(minutes=10)) == ["1"]
    assert cycle.refill_lanes(cfg, env.store, NOW + timedelta(minutes=10), own="1") == []  # this job's session takes it


def test_refill_leaves_out_the_own_lane_and_the_task_it_will_take(tmp_path: Path) -> None:
    graph = [staged(t, owns=[f"app/{t.lower()}/"]) for t in ("T001", "T002", "T003")]
    cfg = dataclasses.replace(config(graph, tmp_path), lanes=3, lane_hint="2")
    state = st.sync_graph(st.new_state(), graph)
    assert cycle.refill_lanes(cfg, state, NOW) == ["1", "2", "3"]
    assert cycle.refill_lanes(cfg, state, NOW, own="2") == ["1", "3"]
    two = st.sync_graph(st.new_state(), graph[:2])
    assert cycle.refill_lanes(dataclasses.replace(cfg, graph=graph[:2]), two, NOW, own="2") == ["1"]
    assert cycle.refill_lanes(cfg, state, NOW, own="9") == ["1", "2", "3"]  # a lane not free changes nothing


class _Log:
    """The GitHubEnv surface `lane_report` uses: dispatches and this job's log rows."""

    def __init__(self, rows: list[dict[str, Any]] | None = None, refuse: str = "") -> None:
        self.rows, self.refuse, self.calls = list(rows or []), refuse, []

    def dispatch_lane(self, lane: str) -> bool:
        self.calls.append(lane)
        return lane != self.refuse

    def read_cycle_log(self) -> list[dict[str, Any]]:
        return self.rows


def _report(monkeypatch: pytest.MonkeyPatch, cfg: cycle.Config, env: _Log, result: str, dry: bool = False) -> tuple[dict[str, Any], list[Any]]:
    asked: list[Any] = []

    def refill(c: cycle.Config, e: Any, own: str | None = None) -> list[str]:
        asked.append(own)
        return ["1", "3"]

    monkeypatch.setattr(entry, "refill", refill)
    report: dict[str, Any] = {}
    entry.lane_report(SimpleNamespace(dry_run=dry), cfg, env, result, report)  # type: ignore[arg-type]
    return report, asked


def test_the_early_report_starts_the_other_lanes_and_keeps_only_unstarted_ones_for_the_chain(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    cfg = dataclasses.replace(config([], tmp_path, "actions-77"), lanes=3, lane_hint="2")
    env = _Log(refuse="3")
    report, asked = _report(monkeypatch, cfg, env, cycle.MERGES_RESOLVED)
    assert asked == ["2"] and report == {"lane": "2", "refill": ["3"], "dispatched": ["1"]} and env.calls == ["1", "3"]
    report, asked = _report(monkeypatch, cfg, _Log(), cycle.MERGES_RESOLVED, dry=True)
    assert report == {"lane": "2", "refill": ["1", "3"]}
    report, asked = _report(monkeypatch, cfg, _Log(), "merge_pending")
    assert asked == [None] and report["dispatched"] == ["1", "3"]  # D-3140: every run that ends refills


def test_a_lane_the_early_step_started_is_not_started_again_by_the_sessions_hand_off(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    cfg = dataclasses.replace(config([], tmp_path, "actions-77"), lanes=3, lane_hint="2")
    early = {"at": "2026-11-01T12:01:00", "session": "actions-77", "result": cycle.MERGES_RESOLVED, "dispatched": ["1"]}
    other = {"at": "2026-11-01T12:02:00", "session": "actions-88", "result": "handed_off", "dispatched": ["3"]}
    env = _Log([early, other])
    report, _ = _report(monkeypatch, cfg, env, "handed_off")
    assert env.calls == ["3"] and report["dispatched"] == ["3"]  # lane 1 is on its way; another job's start is not this job's
    env = _Log([early, dict(early, result="usage_reading_recorded: week 1%", dispatched=["3"])])
    _report(monkeypatch, cfg, env, "handed_off")
    assert env.calls == []


def test_nothing_to_close_writes_no_log_row() -> None:
    assert cycle.NO_MERGED in entry.SILENT and cycle.MERGES_RESOLVED not in entry.SILENT
    assert entry.parse_args(["--resolve-merges"]).resolve_merges is True


def test_the_workflow_closes_merges_first_with_the_tokens_the_writes_need() -> None:
    names = [str(s.get("name", s.get("uses", ""))) for s in _workflow()["jobs"]["backup"]["steps"]]
    first = names.index("Close merged tasks before the smoke (D-3141)")
    later = [i for i, n in enumerate(names) if n.startswith(("Rust build cache", "Warm the smoke", "Run the start-routine smoke"))
             or n == "anthropics/claude-code-action@v1"]
    assert len(later) >= 4 and first < min(later) and names[first - 1] == "actions/setup-python@v5"
    step = _step("backup", "Close merged tasks")
    assert "python3 -m runner --resolve-merges" in step["run"] and step["continue-on-error"] is True
    assert {"RUNNER_STARTER", "RUNNER_LANE", "GH_TOKEN", "RUNNER_CHECKS_TOKEN", "RUNNER_PUSH_TOKEN"} <= set(step["env"])


def test_the_chain_refills_after_an_early_close_even_when_the_session_row_moved_nothing() -> None:
    mine = {"at": "2026-11-01T12:05:00", "session": "actions-77"}
    early = dict(mine, result=cycle.MERGES_RESOLVED, changed={"tasks": {"T001": "MERGE_PENDING -> DONE"}}, refill=[])
    session = dict(mine, at="2026-11-01T12:25:00", result="smoke_pending", changed={"tasks": {}}, refill=["2"])
    assert _refill([early, session]) == "2"
    assert _refill([session]) == ""  # without the early close the rule is as before
    assert _refill([early]) == ""  # the early row alone: every lane it found was started already
    assert _refill([dict(early, refill=["3"])]) == "3"  # a lane it could not start is left to the chain
    # The session's row lists lane 3 again, which the early row started: not started twice (D-3141).
    assert _refill([dict(early, dispatched=["3"]), dict(session, refill=["2", "3"])]) == "2"
    moved = dict(session, result="MERGE_PENDING", refill=["2", "3"])
    assert _refill([dict(early, dispatched=["3"], session="actions-88"), moved]) == "2 3"  # another job's start


def test_an_earlier_attempts_merged_pr_beside_an_empty_hand_in_is_left_alone(tmp_path: Path) -> None:
    """The task's reviewed head is already on main (an empty hand-in) and the branch's latest PR carries another
    head: the cycle resolves it without that PR (`_without_pr`, D-584), and so the early step leaves it alone."""
    env, cfg = _pending(tmp_path)
    env.main_commits.add(HEAD)
    env.prs["T001"] = dict(any_green_pr("old0001"), merged_sha="sq0000")
    env.update_merges = False
    before = copy.deepcopy(env.store)
    assert cycle.resolve_merged(cfg, env, NOW) == cycle.NO_MERGED and env.store == before


def test_a_cleanup_task_waiting_for_its_stage_proof_is_left_to_the_cycle(tmp_path: Path) -> None:
    env, cfg = _pending(tmp_path, [make_task("T001", type="cleanup", stage="2")])
    env.prs["T001"] = green_pr()
    before = copy.deepcopy(env.store)
    assert cycle.resolve_merged(cfg, env, NOW) == cycle.NO_MERGED and env.store == before
    env.store["stage_proofs"] = {"2": {"evidence": "nightly run 1 green"}}
    assert cycle.resolve_merged(cfg, env, NOW) == cycle.MERGES_RESOLVED and st.status(env.store, "T001") == st.DONE
