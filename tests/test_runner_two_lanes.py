"""Two lanes, stages C and D (D-620): the update merge that keeps a task's pull request current, claiming
on two lanes (non-overlapping owned paths, same stage, dependencies DONE, cleanup alone), red main, the
fall back to one lane, pacing over both lanes, and the backup-runner workflow's lane plumbing.

Drills run over test_failure_drills.FakeEnv; the update merge itself runs over real local git
repositories in tests/test_runner_update_merge.py. Fake data only.
"""

from __future__ import annotations

import dataclasses
import json
import subprocess
import sys
from datetime import timedelta
from pathlib import Path
from typing import Any

import pytest
import yaml

import check_state as cs
import pacing
from runner import __main__ as entry, cycle, lanes, state as st, verify
from test_failure_drills import MODELS, NOW, FakeEnv, config, green_pr, make_task, reviewed

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github" / "workflows" / "backup-runner.yml"


def two_lane(graph: list[dict[str, Any]], tmp_path: Path, session: str, hint: str | None, **kw: Any) -> cycle.Config:
    return dataclasses.replace(config(graph, tmp_path, session, **kw), lanes=2, lane_hint=hint)


def staged(tid: str, stage: str = "2", **extra: Any) -> dict[str, Any]:
    return make_task(tid, stage=stage, **extra)


def handed_off(task: dict[str, Any], model: str, log: str) -> cycle.WorkerResult:
    return cycle.WorkerResult("handed_off")


def head_of(tid: str) -> str:
    return f"h-{tid.lower()}"


def by_head(task: dict[str, Any], model: str, log: str) -> cycle.WorkerResult:
    """A reviewed hand-in of one file inside the task's first owned folder, head h-<id>."""
    return reviewed(task, model, paths=[f"{task['owns'][0]}x.py"], head=head_of(task["id"]))


# ---- occupancy and the lane a run serves (runner/state.py)


def test_a_merge_pending_task_keeps_its_own_lane_busy_and_only_that_lane() -> None:
    s = st.with_task(st.with_task(st.new_state(), "T001"), "T002")
    s = st.claim(s, "T002", "b", NOW, lane="2")
    assert st.entry(s, "T002")["lane"] == "2" and "lane" not in st.entry(st.claim(s, "T001", "a", NOW), "T001")
    s = st.with_lease(st.with_task(s, "T002", status=st.MERGE_PENDING), "2", None)
    assert st.in_flight(s, NOW) == {"T002": "2"}
    with pytest.raises(st.LeaseError, match="MERGE_PENDING on lane 2"):
        st.claim(s, "T001", "a", NOW, lane="2")
    assert st.lane_of(st.claim(s, "T001", "a", NOW), "T001") == "1"
    one = st.with_task(st.with_task(st.new_state(), "T001", status=st.MERGE_PENDING), "T002")
    with pytest.raises(st.LeaseError, match="nothing new is claimed"):
        st.claim(one, "T002", "a", NOW)  # one lane: the old rule, nothing new while any task is MERGE_PENDING


def test_run_lane_picks_the_events_task_lane_then_a_lane_without_a_live_lease() -> None:
    s = st.with_task(st.with_task(st.new_state(), "T001"), "T002")
    assert st.run_lane(s, NOW, 1) == "1" and st.run_lane(s, NOW, 2) == "1"
    s = st.claim(s, "T001", "a", NOW)
    assert st.run_lane(s, NOW, 1) == "1"  # one lane: always lane 1, as before
    assert st.run_lane(s, NOW, 2) == "2" and st.run_lane(s, NOW, 2, "T001") == "2"
    s = st.with_lease(st.with_task(st.claim(s, "T002", "b", NOW, lane="2"), "T002", status=st.MERGE_PENDING), "2", None)
    assert st.run_lane(s, NOW, 2, "T002") == "2"
    s = st.claim(st.with_task(s, "T002", status=st.QUEUED), "T002", "c", NOW, lane="2")
    assert st.run_lane(s, NOW, 2, "T002") == "1"  # every lane holds a live lease: queue behind lane 1


# ---- the lane policy (runner/lanes.py)


def test_claim_lane_counts_tasks_in_flight_and_honours_the_runs_lane() -> None:
    s = st.with_task(st.with_task(st.new_state(), "T001"), "T002")
    assert lanes.claim_lane(1, s, NOW, None) == "1" and lanes.claim_lane(1, s, NOW, "1") == "1"
    s = st.claim(s, "T001", "a", NOW)
    assert lanes.claim_lane(1, s, NOW, None) is None and lanes.claim_lane(2, s, NOW, None) == "2"
    assert lanes.claim_lane(2, s, NOW, "1") is None and lanes.claim_lane(2, s, NOW, "2") == "2"
    assert lanes.claim_lane(2, dict(s, lanes_fallback={"at": "x"}), NOW, "2") is None
    assert lanes.claim_lane(2, s, NOW, "3") is None


def test_refusal_names_overlap_stage_cleanup_and_dependencies() -> None:
    a = staged("T001", owns=["app/a/"])
    done = {"T000"}
    assert lanes.refusal(staged("T002", owns=["app/a/x.rs"]), [a], done, None) == "owns what T001 owns (app/a/x.rs ~ app/a/)"
    assert lanes.refusal(staged("T003", "3", owns=["app/c/"]), [a], done, None) == "T001 is in stage 2, not stage 3"
    assert lanes.refusal(staged("T004", type="cleanup", owns=["app/d/"]), [a], done, None) == "a stage's cleanup task runs alone"
    cleanup = staged("T005", type="cleanup", owns=["scripts/x.py"])
    assert lanes.refusal(staged("T006", owns=["app/f/"]), [cleanup], done, None) == "T005 is a cleanup task, which runs alone"
    assert lanes.refusal(staged("T007", deps=["T000", "T009"], owns=["app/g/"]), [a], done, None) == "dependencies not DONE: T009"
    assert lanes.refusal(staged("T008", owns=["app/h/"]), [a], done, None) == ""
    assert lanes.refusal(staged("T008", type="cleanup"), [], done, None) == ""  # alone it may run
    assert lanes.refusal({"id": st.FIX_MAIN, "owns": ["app/a/"]}, [a], done, None) == ""


def test_two_tasks_owning_locked_files_both_own_the_plan_lock() -> None:
    first, second = staged("T001", owns=["scripts/one.py"]), staged("T002", owns=["docs/governance.md"])
    assert lanes.refusal(second, [first], set(), None) == ""
    assert lanes.refusal(second, [first], set(), entry.locked_path) == "owns what T001 owns (kit/plan.lock ~ kit/plan.lock)"
    assert entry.locked_path("scripts/") and entry.locked_path("docs/spec/texting.md") and entry.locked_path("BUILD_BRIEF.md")
    assert not entry.locked_path("app/") and not entry.locked_path("docs/spec/x/y.md") and not entry.locked_path("runner/cycle.py")


def test_lane_hint_reads_only_a_lane_number() -> None:
    assert entry.lane_hint("2") == "2" and entry.lane_hint(" 1 ") == "1"
    assert entry.lane_hint("") is None and entry.lane_hint("0") is None and entry.lane_hint("fix") is None


# ---- stage D drills: two lanes claiming


def test_drill_two_lanes_claim_non_overlapping_tasks_and_refuse_overlap_other_stages_and_cleanup(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/a/sub/"]), staged("T004", type="cleanup", owns=["app/d/"]),
             staged("T005", "3", owns=["app/e/"]), staged("T003", owns=["app/b/"])]
    env = FakeEnv(handed_off)
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s1", "1"), env, NOW) == "handed_off"
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s2", "2"), env, NOW + timedelta(minutes=1)) == "handed_off"
    held = {lane: lease["task"] for lane, lease in st.live_leases(env.store, NOW + timedelta(minutes=2)).items()}
    assert held == {"1": "T001", "2": "T003"} and st.entry(env.store, "T003")["lane"] == "2"
    assert [r[0] for r in env.runs] == ["T001", "T003"]
    # Both lanes are busy now: a third run serves nothing and claims nothing.
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", None), env, NOW + timedelta(minutes=2)) == "lease_live"


def test_drill_one_lane_with_a_lane_hint_behaves_as_before(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"])]
    env = FakeEnv(by_head)
    cfg = dataclasses.replace(config(graph, tmp_path), lane_hint="1")
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=1)) == "merge_pending"
    assert [r[0] for r in env.runs] == ["T001"] and "lane" not in st.entry(env.store, "T001")


def test_drill_a_run_for_a_busy_lane_claims_nothing_even_with_another_lane_free(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"])]
    env = FakeEnv(handed_off)
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s1", "2"), env, NOW) == "handed_off"
    assert st.lane_of(env.store, "T001") == "2"
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s2", "2"), env, NOW + timedelta(minutes=1)) == "lease_live"
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", "1"), env, NOW + timedelta(minutes=1)) == "handed_off"


def test_drill_the_lane_two_heartbeat_renews_lane_two_and_after_finishes_it(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"])]
    clock = {"now": NOW}
    env = FakeEnv()
    env.run_worker = lambda task, model, log, beat: (clock.update(now=NOW + timedelta(minutes=31)), beat(),  # type: ignore[method-assign]  # reason: a worker that beats once
                                                     cycle.WorkerResult("handed_off"))[-1]
    cfg = two_lane(graph, tmp_path, "s1", "2", clock=lambda: clock["now"])
    assert cycle.run_cycle(cfg, env, NOW) == "handed_off"
    assert env.store["leases"]["2"]["expires"] == st.iso(NOW + timedelta(minutes=31, hours=3))
    done = cycle.after_handoff(cfg, env, lambda task: by_head(task, MODELS["sonnet"], ""))
    assert done == st.MERGE_PENDING and env.store["leases"]["2"] is None
    task = st.entry(env.store, "T001")
    assert task["lane"] == "2" and task["review_lease"]["lane"] == "2" and st.in_flight(env.store, NOW) == {"T001": "2"}
    assert cycle.after_handoff(dataclasses.replace(cfg, session="other"), env, lambda t: by_head(t, MODELS["sonnet"], "")) == "not_lease_holder"


# ---- stage C drills: the update merge


def _two_merge_pending(tmp_path: Path) -> tuple[FakeEnv, list[dict[str, Any]]]:
    """T001 handed in on lane 1, then T002 on lane 2 beside it; both MERGE_PENDING."""
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"])]
    env = FakeEnv(by_head)
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s1", "1"), env, NOW) == st.MERGE_PENDING
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s2", "2"), env, NOW + timedelta(minutes=5)) == st.MERGE_PENDING
    assert st.in_flight(env.store, NOW + timedelta(minutes=6)) == {"T001": "1", "T002": "2"}
    return env, graph


def test_drill_lane_one_merges_lane_two_is_updated_by_the_runner_then_merges(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    env.prs["T001"] = dict(green_pr(head_of("T001")), number=1)
    env.prs["T002"] = dict(green_pr(head_of("T002"), merged=False), number=2, state="open", auto_merge=False)
    env.update = {"status": "pushed", "sha": "m-t002", "main": "main0002"}
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", "1"), env, NOW + timedelta(minutes=10)) == "done"
    t2 = st.entry(env.store, "T002")
    assert st.status(env.store, "T001") == st.DONE and t2["status"] == st.MERGE_PENDING
    assert env.updates == [("claude/task/T002-x", head_of("T002"))]
    assert t2["reviewed_sha"] == "m-t002" and t2["update_merges"][-1]["from"] == head_of("T002")
    assert t2["update_merges"][-1]["main"] == "main0002" and t2["update_merges"][-1]["how"] == "runner"
    assert ("claude/task/T002-x", "m-t002") in env.auto_merges and t2["auto_merge"]["enabled"] is True
    assert verify.auto_merge_reason(env.store, "claude/task/T002-x", "m-t002", st.leases_of(env.store)) == ""
    assert cs.check(env.store, lanes=2, now=NOW) == []
    env.prs["T002"] = dict(green_pr("m-t002"), number=2, merged_sha="sq0002")
    env.update = {"status": "error", "why": "never asked: a merged pull request is not updated"}
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s4", "2"), env, NOW + timedelta(minutes=30)) == "done"
    assert st.status(env.store, "T002") == st.DONE and st.entry(env.store, "T002")["merged_sha"]
    assert not env.store.get("main_red") and len(env.updates) == 1


def test_drill_an_update_on_lane_two_does_not_stop_a_lane_one_claim_but_ends_its_own_lanes_cycle(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    graph.append(staged("T003", owns=["app/c/"]))
    env.prs["T001"] = dict(green_pr(head_of("T001")), number=1)
    env.prs["T002"] = dict(green_pr(head_of("T002"), merged=False), number=2, state="open", auto_merge=True)
    env.update = {"status": "pushed", "sha": "m-t002", "main": "main0002"}
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", "2"), env, NOW + timedelta(minutes=10)) == "branch_updated"
    assert st.status(env.store, "T003") == st.QUEUED and env.auto_merges[-1][1] != "m-t002"  # auto-merge was on already
    env.prs["T002"] = dict(green_pr("m-t002", merged=False), number=2, state="open", auto_merge=True)
    env.update = {"status": "current"}
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s4", "1"), env, NOW + timedelta(minutes=11)) == st.MERGE_PENDING
    assert st.lane_of(env.store, "T003") is None and st.entry(env.store, "T003").get("lane") is None
    assert st.status(env.store, "T003") == st.MERGE_PENDING and st.status(env.store, "T002") == st.MERGE_PENDING


def test_drill_a_pull_request_still_showing_the_head_before_the_update_waits(tmp_path: Path) -> None:
    """GitHub may still list the old head right after the update merge was pushed: that is reviewed work
    on an older main, never "commits pushed after the review", so the task waits (at most UPDATE_WAIT)."""
    env, graph = _two_merge_pending(tmp_path)
    env.prs["T002"] = dict(green_pr(head_of("T002"), merged=False), number=2, state="open", auto_merge=False)
    env.update = {"status": "pushed", "sha": "m-t002", "main": "main0002"}
    cfg = two_lane(graph, tmp_path, "s3", "2")
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == "branch_updated"
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=11)) == "merge_pending"
    t2 = st.entry(env.store, "T002")
    assert t2["attempts"] == 0 and t2["update_waiting_since"] and env.head_checks == [] and not env.closed_prs
    env.prs["T002"] = dict(green_pr(head_of("T002")), number=2)  # and had it merged, it is reviewed work too
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=12)) == "done" and st.status(env.store, "T002") == st.DONE


def test_drill_a_real_conflict_fails_the_attempt_and_the_retry_continues_the_branch(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    env.prs["T002"] = dict(green_pr(head_of("T002"), merged=False), number=7, state="open", auto_merge=True)
    env.update = {"status": "conflict", "paths": ["Cargo.lock", "docs/architecture.md"], "main": "main0002"}
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", "2"), env, NOW + timedelta(minutes=10)) == "update_conflict"
    t2 = st.entry(env.store, "T002")
    assert t2["status"] == st.QUEUED and t2["attempts"] == 1 and "conflicts in Cargo.lock, docs/architecture.md" in t2["last_failure"]
    assert t2["runner_closed_prs"] == [7] and env.closed_prs[0][0] == 7 and cycle.DROP_MARKER in env.closed_prs[0][1]
    assert t2["history"][-1]["result"] == "update_conflict" and env.store["lane_strikes"]["2"]["count"] == 1
    env.prep = {"branch": "claude/task/T002-x", "branch_ready": True, "base": "origin/claude/task/T002-x"}
    env.worker = handed_off
    del env.prs["T002"]
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s4", "2"), env, NOW + timedelta(minutes=11)) == "handed_off"
    assert env.prepared_for[-1] == ("T002", 2) and env.runs[-1][2].startswith("merging main into the task branch conflicts")
    assert st.lane_of(env.store, "T002") == "2"


def test_drill_a_conflict_whose_close_fails_waits_so_any_merge_is_judged(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    env.prs["T002"] = dict(green_pr(head_of("T002"), merged=False), number=7, state="open", auto_merge=True)
    env.update, env.close_ok = {"status": "conflict", "paths": ["README.md"]}, False
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", "1"), env, NOW + timedelta(minutes=10)) == "merge_pending"
    t2 = st.entry(env.store, "T002")
    assert t2["status"] == st.MERGE_PENDING and t2["attempts"] == 0 and "could not be turned off" in t2["auto_merge"]["why"]


def test_drill_update_push_failures_wait_then_fail_and_a_race_is_only_a_wait(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    env.prs["T002"] = dict(green_pr(head_of("T002"), merged=False), number=2, state="open", auto_merge=True)
    env.update = {"status": "push_failed", "why": "! [rejected] (fetch first)"}
    cfg = two_lane(graph, tmp_path, "s3", "2")
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == "merge_pending"
    assert not st.entry(env.store, "T002").get("update_push_failures")
    env.update = {"status": "push_failed", "why": "remote: Permission to fake-owner/fake-repo.git denied"}
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=11)) == "update_push_failed"
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=12)) == "update_push_failed"
    assert env.store.get("lanes_fallback")  # two harness-type strikes in a row on lane 2
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=13)) == "update_push_failed"
    t2 = st.entry(env.store, "T002")
    assert t2["status"] == st.QUEUED and "could not be pushed 3 times" in t2["last_failure"] and t2["runner_closed_prs"] == [2]


def test_drill_an_update_git_cannot_make_waits_at_most_two_hours(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    env.prs["T002"] = dict(green_pr(head_of("T002"), merged=False), number=2, state="open", auto_merge=False)
    env.update = {"status": "moved", "why": "origin/claude/task/T002-x is None"}
    cfg = two_lane(graph, tmp_path, "s3", "2")
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == "merge_pending"
    assert st.entry(env.store, "T002")["update_waiting_since"] == st.iso(NOW + timedelta(minutes=10))
    env.update = {"status": "current"}
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=20))
    assert st.entry(env.store, "T002")["update_waiting_since"] is None  # the wait ended
    env.update = {"status": "error", "why": "git merge-tree failed"}
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=30))
    assert cycle.run_cycle(cfg, env, NOW + timedelta(hours=2, minutes=31)) == "update_push_failed"
    assert "could not be brought up to date with main for 2:00:00" in st.entry(env.store, "T002")["last_failure"]


def test_drill_an_update_merge_whose_record_was_lost_is_adopted_as_the_reviewed_head(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    env.prs["T002"] = dict(green_pr("m-t002", merged=False), number=2, state="open", auto_merge=False)
    env.update_merges, env.update_merge = True, True  # tree-trusted (D-584) and an update merge of the reviewed head
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", "2"), env, NOW + timedelta(minutes=10)) == "merge_pending"
    t2 = st.entry(env.store, "T002")
    assert t2["reviewed_sha"] == "m-t002" and t2["update_merges"][-1]["how"] == "adopted" and t2["attempts"] == 0
    assert env.auto_merges[-1] == ("claude/task/T002-x", "m-t002")
    env.update_merge = False  # a trusted head that is not a merge of the reviewed head: D-584 as before
    env.prs["T002"] = dict(green_pr("r-t002", merged=False), number=2, state="open", auto_merge=False)
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s4", "2"), env, NOW + timedelta(minutes=11)) == "unreviewed_head"


def test_drill_a_lane_one_update_merge_happens_with_one_lane_too(tmp_path: Path) -> None:
    """Stage C does not wait for two lanes: a person's merge into main leaves the one task behind."""
    env, cfg = FakeEnv(by_head), config([staged("T001", owns=["app/a/"])], tmp_path)
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING
    env.prs["T001"] = dict(green_pr(head_of("T001"), merged=False), number=1, state="open", auto_merge=True)
    env.update = {"status": "pushed", "sha": "m-t001", "main": "main0002"}
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=5)) == "branch_updated"
    assert st.entry(env.store, "T001")["reviewed_sha"] == "m-t001" and "lane_strikes" not in env.store


# ---- red main stops both lanes


def test_drill_red_main_stops_both_lanes_and_only_fix_main_is_claimed(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"]), staged("T003", owns=["app/c/"])]
    env = FakeEnv(handed_off)
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s1", "1"), env, NOW) == "handed_off"
    cfg = two_lane(graph, tmp_path, "s2", "2")
    env.store = cycle.set_main_red(env.store, cfg, "nightly", "bad0001", ("T000", "m0", ["app/z.rs"]), NOW)
    assert st.eligible(env.store, NOW) == [st.FIX_MAIN]
    smoked = len(env.smokes)
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=1)) == "handed_off"
    assert st.lane_of(env.store, st.FIX_MAIN) == "2" and len(env.smokes) == smoked  # FIX-MAIN is claimed without a smoke
    assert [r[0] for r in env.runs] == ["T001", st.FIX_MAIN]


def test_drill_red_main_with_fix_main_running_leaves_a_free_lane_idle(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"])]
    env = FakeEnv(handed_off)
    cfg = two_lane(graph, tmp_path, "s1", "1")
    env.store = cycle.set_main_red(st.sync_graph(env.store, graph), cfg, "nightly", "bad0001", ("T000", "m0", []), NOW)
    assert cycle.run_cycle(cfg, env, NOW) == "handed_off"
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s2", "2"), env, NOW + timedelta(minutes=1)) == "fix_main"
    assert [r[0] for r in env.runs] == [st.FIX_MAIN] and st.status(env.store, "T001") == st.QUEUED


# ---- the fall back to one lane


def _rejected(env: FakeEnv, why: str) -> None:
    env.rejection = why
    env.push_branch = lambda branch, head: False  # type: ignore[method-assign]  # reason: every branch push is rejected


def test_drill_two_harness_failures_on_lane_two_fall_back_to_one_lane_and_say_why(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"])]
    env = FakeEnv(by_head)
    _rejected(env, "! [rejected] head0001 -> claude/task/T001-x (non-fast-forward)")
    cfg = two_lane(graph, tmp_path, "s1", "2")
    assert cycle.run_cycle(cfg, env, NOW) == "push_failed"
    assert env.store["lane_strikes"]["2"]["count"] == 1 and not env.store.get("lanes_fallback")
    assert cycle.run_cycle(dataclasses.replace(cfg, session="s2"), env, NOW + timedelta(minutes=5)) == "push_failed"
    fell = env.store["lanes_fallback"]
    assert fell["lane"] == "2" and len(fell["why"]) == 2 and "branch push rejected" in fell["why"][0]
    note = (tmp_path / "lanes-fallback.md").read_text(encoding="utf-8")
    assert "kind: info" in note and "--restore-lanes" in note
    assert cs.check(env.store, lanes=2, now=NOW) == []
    assert any("state.lanes_fallback" in p for p in cs.check(dict(env.store, lanes_fallback={"at": "x"}), lanes=2, now=NOW))
    assert cycle.run_cycle(dataclasses.replace(cfg, session="s3"), env, NOW + timedelta(minutes=6)) == cycle.NO_FREE_LANE
    assert lanes.free_lanes(2, env.store, NOW) == ["1"] and lanes.effective_lanes(2, env.store) == 1
    restored = lanes.restore(env.store)
    assert restored is not None and "lanes_fallback" not in restored and "lane_strikes" not in restored
    assert restored["lanes_fallback_history"][-1]["lane"] == "2" and lanes.restore(restored) is None
    assert lanes.free_lanes(2, restored, NOW) == ["1", "2"]


def test_drill_lane_one_never_strikes_and_an_own_failure_breaks_a_run_of_strikes(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"])]
    env = FakeEnv(by_head)
    _rejected(env, "! [rejected] (non-fast-forward)")
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s1", "1"), env, NOW) == "push_failed"
    assert "lane_strikes" not in env.store
    s = lanes.strike(env.store, "2", "one", NOW)
    assert lanes.strike(s, "1", "x", NOW) is s and lanes.strike(s, "fix", "x", NOW) is s
    env2 = FakeEnv(lambda t, m, log: cycle.WorkerResult("tests_failed", log="a test failed"))
    env2.store = s
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s2", "2"), env2, NOW) == st.QUEUED
    assert env2.store["lane_strikes"]["2"]["count"] == 0 and lanes.clear_strikes(env2.store, "2") is env2.store


def test_drill_an_attempt_that_loses_its_writes_on_lane_two_is_a_strike(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"])]
    env = FakeEnv()

    def stolen(task: dict[str, Any], model: str, log: str) -> cycle.WorkerResult:
        rival = {"task": "T001", "session": "rival", "expires": st.iso(NOW + timedelta(hours=3))}
        env.store = st.with_lease(env.store, "2", rival)
        return by_head(task, model, log)

    env.worker = stolen
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s1", "2"), env, NOW) == "lost_race"
    assert env.store["lane_strikes"]["2"]["count"] == 1 and "could not write its review record" in env.store["lane_strikes"]["2"]["last"][0]["why"]
    assert not env.branch_pushes


def test_a_lost_claim_race_is_retried_and_a_storm_is_a_strike(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("RUNNER_WORKER_CMD", raising=False)
    env = FakeEnv()
    env.sleep = lambda seconds: None  # type: ignore[attr-defined]  # reason: no real wait between tries
    results: list[str] = []

    def scripted(answers: list[str]) -> None:
        queue = list(answers)
        monkeypatch.setattr(cycle, "run_cycle", lambda cfg, e, now: results.append(queue[0]) or queue.pop(0))

    graph = [staged("T001")]
    scripted(["lost_race", "handed_off"])
    assert entry.cycle_with_retries(two_lane(graph, tmp_path, "s1", "2"), env) == "handed_off" and len(results) == 2  # type: ignore[arg-type]  # reason: FakeEnv stands in for GitHubEnv
    results.clear()
    scripted(["lost_race"] * 3)
    assert entry.cycle_with_retries(two_lane(graph, tmp_path, "s1", "2"), env) == "lost_race" and len(results) == 3  # type: ignore[arg-type]  # reason: as above
    assert env.store["lane_strikes"]["2"]["count"] == 1 and "3 lost races in a row" in env.store["lane_strikes"]["2"]["last"][0]["why"]
    results.clear()
    scripted(["lost_race"])
    assert entry.cycle_with_retries(config(graph, tmp_path), env) == "lost_race" and len(results) == 1  # type: ignore[arg-type]  # reason: one lane never retries
    results.clear()
    monkeypatch.setenv("RUNNER_WORKER_CMD", "true")
    scripted(["lost_race"])
    assert entry.cycle_with_retries(two_lane(graph, tmp_path, "s1", "2"), env) == "lost_race" and len(results) == 1  # type: ignore[arg-type]  # reason: a worker command means no retry


def test_lane_outcome_strikes_a_check_failure_only_after_another_lanes_merge() -> None:
    s = st.with_task(st.with_task(st.new_state(), "T002", lane="2", attempt_started_at="2026-11-01T12:00:00Z", last_failure="Ci: failure"),
                     "T001", status=st.DONE, merged_at="2026-11-01T12:30:00Z", merged_sha="m1")
    hit = cycle._lane_outcome(s, "T002", "2", "check_failed", NOW)
    assert hit["lane_strikes"]["2"]["count"] == 1 and "T002 check_failed: Ci: failure" in hit["lane_strikes"]["2"]["last"][0]["why"]
    before = st.with_task(s, "T001", merged_at="2026-11-01T11:00:00Z")
    assert cycle._lane_outcome(lanes.strike(before, "2", "x", NOW), "T002", "2", "check_failed", NOW)["lane_strikes"]["2"]["count"] == 0
    same_lane = st.with_task(s, "T001", lane="2")
    assert "lane_strikes" not in cycle._lane_outcome(same_lane, "T002", "2", "check_failed", NOW)
    assert cycle._lane_outcome(s, "T002", "1", "check_failed", NOW) is s
    assert cycle._lane_outcome(lanes.strike(s, "2", "x", NOW), "T002", "2", "done", NOW)["lane_strikes"]["2"]["count"] == 0


def test_status_names_every_lanes_lease_and_a_fall_back() -> None:
    from runner.github_env import render_status

    s = st.claim(st.claim(st.with_task(st.with_task(st.new_state(), "T001"), "T002"), "T001", "a", NOW), "T002", "b", NOW, lane="2")
    text = render_status(dict(s, lanes_fallback={"at": "2026-11-01T12:00:00Z"}), "", NOW)
    assert "- T001 (lease until 2026-11-01T15:00:00Z)" in text and "- lane 2: T002 (lease until 2026-11-01T15:00:00Z)" in text
    assert "one lane only since 2026-11-01T12:00:00Z" in text


# ---- pacing counts both lanes


def _pacing_config() -> dict[str, Any]:
    cfg = pacing.load_config(ROOT / "kit" / "pacing.json")
    cfg.pop("quota_cmd_default", None)
    return dict(cfg, budget_tokens_five_hour=10000, budget_tokens_week=1000000, headroom=0.0)


def test_pacing_counts_the_estimate_of_an_attempt_running_in_another_lane() -> None:
    s = st.with_task(st.with_task(st.new_state(), "T001", status=st.IN_PROGRESS), "T002")
    s["usage"] = {"by_task": {"T001": {"estimated_tokens": 6000}}}
    task = {"id": "T002", "owns": [], "model": MODELS["sonnet"]}
    assert pacing.in_flight_tokens(s, "T002") == 6000 and pacing.in_flight_tokens(s, "T001") == 0
    found = pacing.assess(s, task, config=_pacing_config(), now=NOW)
    assert found["in_flight"] == 6000 and found["standing"]["five_hour"]["used"] == 6000
    assert "after 6k still running in another lane" in pacing.line("T002", found)
    alone = pacing.assess(st.with_task(s, "T001", status=st.MERGE_PENDING), task, config=_pacing_config(), now=NOW)
    assert "in_flight" not in alone and "still running" not in pacing.line("T002", alone)


def test_drill_pacing_holds_lane_two_while_lane_one_spends_the_window(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("PACING_QUOTA_CMD", raising=False)
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"])]
    for tid in ("a", "b"):
        (tmp_path / "app" / tid).mkdir(parents=True)
        (tmp_path / "app" / tid / "x.py").write_text("x" * 8000)  # about 7.8k estimated tokens each
    pacer = pacing.make_pacer(graph, tmp_path, _pacing_config())
    env = FakeEnv(handed_off)
    first = dataclasses.replace(two_lane(graph, tmp_path, "s1", "1"), pacer=pacer)
    assert cycle.run_cycle(first, env, NOW) == "handed_off"
    assert env.store["usage"]["by_task"]["T001"]["estimated_tokens"] > 5000
    second = dataclasses.replace(two_lane(graph, tmp_path, "s2", "2"), pacer=pacer)
    assert cycle.run_cycle(second, env, NOW + timedelta(minutes=1)) == "paced"
    assert "still running in another lane" in env.store["pacing"]["last"] and env.store["pacing"]["decision"] == "wait"
    bigger = dataclasses.replace(second, pacer=pacing.make_pacer(graph, tmp_path, dict(_pacing_config(), budget_tokens_five_hour=30000)))
    assert cycle.run_cycle(bigger, env, NOW + timedelta(minutes=2)) == "handed_off"


# ---- refill: which lanes the chain step starts a run for


def test_refill_lanes_names_free_lanes_with_a_task_that_may_run_beside_the_others(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/a/x.rs"]), staged("T003", owns=["app/c/"])]
    cfg = two_lane(graph, tmp_path, "s1", None)
    s = st.sync_graph(st.new_state(), graph)
    assert cycle.refill_lanes(cfg, s, NOW) == ["1", "2"]
    s = st.claim(s, "T001", "a", NOW)
    assert cycle.refill_lanes(cfg, s, NOW) == ["2"]
    assert cycle.refill_lanes(cfg, st.with_task(s, "T003", status=st.DONE), NOW) == []  # only T002 left, and it overlaps
    assert cycle.refill_lanes(dataclasses.replace(cfg, lanes=1), s, NOW) == []


# ---- backup-runner.yml: the lane a run serves, its concurrency group, RUNNER_LANE, the tick and the chain


def _workflow() -> dict[str, Any]:
    return yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))


def _step(job: str, name_part: str) -> dict[str, Any]:
    return next(s for s in _workflow()["jobs"][job]["steps"] if name_part in str(s.get("name", "")))


def _pick(tmp_path: Path, state: st.State, lanes_setting: object, wanted: str = "", branch: str = "") -> list[str]:
    """Run the gate's lane-pick snippet over runner/state.py, as the workflow does."""
    script = _step("gate", "Pick the lane")["run"]
    code = script.split("python3 -I -c '", 1)[1].split("\n' ", 1)[0]
    (tmp_path / "state.json").write_text(json.dumps({k: v for k, v in state.items() if k not in ("lease", "leases")}))
    (tmp_path / "lease.json").write_text(json.dumps({"lease": state.get("lease"), "leases": st.leases_of(state)}))
    (tmp_path / "runner.json").write_text(json.dumps({"lanes": lanes_setting}))
    args = [sys.executable, "-I", "-c", code, str(ROOT / "runner" / "state.py"), str(tmp_path / "state.json"),
            str(tmp_path / "lease.json"), str(tmp_path / "runner.json"), wanted, branch]
    return subprocess.run(args, capture_output=True, text=True, check=True).stdout.split()


def test_the_gate_picks_lane_one_with_one_lane_and_a_free_lane_with_two(tmp_path: Path) -> None:
    later = NOW.replace(year=2030)  # the snippet uses the real clock: leases far in the future are live
    s = st.with_task(st.with_task(st.new_state(), "T001"), "T002")
    assert _pick(tmp_path, s, 1) == ["1", "1", "1"]
    held = st.claim(s, "T001", "a", later)
    assert _pick(tmp_path, held, 1, "2", "claude/task/T002-x") == ["1", "1", "-"]
    assert _pick(tmp_path, held, 2) == ["2", "2", "2"]
    assert _pick(tmp_path, held, 2, "1") == ["1", "2", "2"]  # a dispatched lane is served as asked
    pending = st.with_lease(st.with_task(st.claim(held, "T002", "b", later, lane="2"), "T002", status=st.MERGE_PENDING), "2", None)
    assert _pick(tmp_path, pending, 2, "", "claude/task/T002-x") == ["2", "2", "2"]
    assert _pick(tmp_path, s, "2") == ["1", "1", "1"] and _pick(tmp_path, s, True) == ["1", "1", "1"]


def test_the_workflow_runs_one_backup_job_per_lane_and_passes_the_lane() -> None:
    flow = _workflow()
    on = flow.get("on", flow.get(True))
    assert on["workflow_dispatch"]["inputs"]["lane"]["required"] is False
    gate, backup = flow["jobs"]["gate"], flow["jobs"]["backup"]
    assert gate["outputs"]["lane"] == "${{ steps.lane.outputs.lane || '1' }}"
    group = backup["concurrency"]["group"]
    assert "needs.gate.outputs.lane == '1' && 'backup-runner'" in group and "backup-runner-lane-{0}" in group
    assert backup["concurrency"]["cancel-in-progress"] is False
    action = next(s for s in backup["steps"] if str(s.get("uses", "")).startswith("anthropics/claude-code-action"))
    assert action["env"]["RUNNER_LANE"] == "${{ needs.gate.outputs.lane }}"
    assert _step("backup", "Hand in for a session")["env"]["RUNNER_LANE"] == "${{ needs.gate.outputs.lane }}"
    tick = _step("gate", "Hourly tick")["run"]
    assert 'if [ "$LANES" = "1" ]; then\n  gh workflow run backup-runner.yml --ref main --repo "$GITHUB_REPOSITORY"\nelse' in tick
    assert '-f lane="$n"' in tick


def _refill(rows: list[dict[str, Any]], run_id: str = "77") -> str:
    script = _step("backup", "Start the next cycle")["run"]
    code = script.split("refill=\"$(printf '%s' \"$log\" | SESSION=\"actions-${GITHUB_RUN_ID}\" python3 -c '", 1)[1].split("\n' ", 1)[0]
    env = {"JOB_START": "2026-11-01T12:00:00", "SESSION": f"actions-{run_id}", "PATH": "/usr/bin:/bin"}
    stdin = "".join(json.dumps(r) + "\n" for r in rows)
    return subprocess.run([sys.executable, "-c", code], input=stdin, capture_output=True, text=True, env=env, check=True).stdout.strip()


def test_the_chain_refills_free_lanes_only_after_a_result_that_moved_a_task() -> None:
    mine = {"at": "2026-11-01T12:05:00", "session": "actions-77"}
    assert _refill([dict(mine, result="handed_off", refill=[]), dict(mine, result="MERGE_PENDING", refill=["1"])]) == "1"
    assert _refill([dict(mine, result="done", refill=["1", "2"])]) == "1 2"
    for quiet in ("idle", "paced", "lease_live", "no_free_lane", "merge_pending", "lost_race", "branch_updated"):
        assert _refill([dict(mine, result=quiet, refill=["2"])]) == ""
    other = {"at": "2026-11-01T12:06:00", "session": "actions-88", "result": "done", "refill": ["2"]}
    assert _refill([dict(mine, result="idle", refill=["2"]), other]) == ""  # another lane's job is not this one's
    assert _refill([{"at": "2026-11-01T11:00:00", "session": "actions-77", "result": "done", "refill": ["2"]}]) == ""
    assert _refill([dict(mine, result="QUEUED")]) == ""  # one lane: the report carries no refill
