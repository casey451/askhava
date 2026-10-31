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
from types import SimpleNamespace
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


RESYNC_PREP = {"branch": "claude/task/T002-x", "branch_ready": True, "base": "origin/claude/task/T002-x", "resync": True}


def _conflict(env: FakeEnv, number: int = 7) -> None:
    """T002's open pull request (auto-merge on) and a real conflict merging main into its reviewed head."""
    env.prs["T002"] = dict(green_pr(str(st.entry(env.store, "T002")["reviewed_sha"]), merged=False), number=number,
                           state="open", auto_merge=True)
    env.update = {"status": "conflict", "paths": ["Cargo.lock", "docs/architecture.md"], "main": "main0002"}


def test_drill_a_real_conflict_is_resynced_on_the_same_attempt_and_reviewed_again(tmp_path: Path) -> None:
    """The conflict no longer fails the attempt (D-891): the task goes back on its attempt, the next claim
    takes it first, and its worker hands in the merge, which is reviewed and pushed like any hand-in."""
    env, graph = _two_merge_pending(tmp_path)
    _conflict(env)
    env.worker, env.prep = handed_off, RESYNC_PREP
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", "2"), env, NOW + timedelta(minutes=10)) == "handed_off"
    t2 = st.entry(env.store, "T002")
    assert t2["status"] == st.IN_PROGRESS and t2["attempts"] == 0 and st.lane_of(env.store, "T002") == "2"
    assert t2["resync"]["paths"] == ["Cargo.lock", "docs/architecture.md"] and t2["resync"]["from"] == head_of("T002")
    assert t2["resync"]["attempt"] == 1 and len(t2["resyncs"]) == 1 and "lane_strikes" not in env.store
    assert t2["runner_closed_prs"] == [7] and cycle.DROP_MARKER in env.closed_prs[0][1] and "same attempt" in env.closed_prs[0][1]
    assert t2["history"][-1]["result"] == cycle.RESYNC and env.prepared_for[-1] == ("T002", 1)
    assert env.runs[-1][0] == "T002" and env.runs[-1][2].startswith("resync, not a new attempt (D-891)")
    assert env.runs[-1][1] == MODELS["sonnet"]  # the attempt's own model: attempts did not move
    env.worker = lambda task, model, log: reviewed(task, model, paths=["app/b/x.py"], head="m-t002")
    del env.prs["T002"]
    res = env.worker(_task_of(graph, "T002"), MODELS["sonnet"], "")
    assert cycle.finish(two_lane(graph, tmp_path, "s3", "2"), env, env.read_state(), _task_of(graph, "T002"), res,
                        MODELS["sonnet"], NOW + timedelta(minutes=20)) == st.MERGE_PENDING
    t2 = st.entry(env.store, "T002")
    assert t2["reviewed_sha"] == "m-t002" and t2["attempts"] == 0 and "resync" not in t2 and len(t2["resyncs"]) == 1
    assert t2["history"][-1]["resync"] is True and t2["history"][-1]["attempt"] == 1
    assert ("claude/task/T002-x", "m-t002") in env.branch_pushes and ("claude/task/T002-x", "m-t002") in env.auto_merges
    assert cycle.plan_since(t2) == t2["history"][-2]["started_at"]  # a resync resumes its attempt (D-677 measures from it)


def _task_of(graph: list[dict[str, Any]], tid: str) -> dict[str, Any]:
    return next(t for t in graph if t["id"] == tid)


def test_drill_a_conflict_after_the_resync_limit_fails_the_attempt_and_strikes(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    earlier = [{"attempt": 1, "from": "x", "paths": ["Cargo.toml"], "at": "t"}] * cycle.RESYNC_LIMIT
    env.store = st.with_task(env.store, "T002", resyncs=[{"attempt": 0, "from": "y", "paths": [], "at": "t"}, *earlier])
    _conflict(env)
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", "2"), env, NOW + timedelta(minutes=10)) == "update_conflict"
    t2 = st.entry(env.store, "T002")
    assert t2["status"] == st.QUEUED and t2["attempts"] == 1 and "again after 2 resyncs" in t2["last_failure"]
    assert "resync" not in t2 and t2["runner_closed_prs"] == [7] and env.store["lane_strikes"]["2"]["count"] == 1
    assert t2["history"][-1]["result"] == "update_conflict"
    env.store = st.with_task(env.store, "T002", resyncs=earlier[:1])  # an earlier attempt's resyncs never count
    assert len([r for r in st.entry(env.store, "T002")["resyncs"] if r["attempt"] == 2]) == 0


def test_drill_a_resync_that_cannot_start_on_the_reviewed_head_fails_the_attempt(tmp_path: Path) -> None:
    """origin moved, or its pull request was closed by someone: prepare_branch makes no resync branch, so
    the resync cannot run, and the attempt fails as the conflict did before D-891; the retry follows D-583."""
    env, graph = _two_merge_pending(tmp_path)
    _conflict(env)
    env.prep = {"branch": "claude/task/T002-x-a2", "branch_ready": True, "base": "main", "renamed_from": "claude/task/T002-x"}
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", "2"), env, NOW + timedelta(minutes=10)) == st.QUEUED
    t2 = st.entry(env.store, "T002")
    assert t2["attempts"] == 1 and "resync" not in t2 and "the resync could not start" in t2["last_failure"]
    assert env.runs == [("T001", MODELS["sonnet"], ""), ("T002", MODELS["sonnet"], "")]  # no resync worker ran


def test_drill_a_resync_that_crashed_after_its_reviewed_push_re_runs_as_a_plain_attempt(tmp_path: Path) -> None:
    """The resync's merge was reviewed and pushed, then the session died before the release: the lease
    expires with `resync` still set (release_expired), and origin's tip already is `reviewed_sha`. The
    re-run clears `resync` and goes on like any crashed attempt, never failing it (D-891)."""
    env, graph = _two_merge_pending(tmp_path)
    _conflict(env)
    env.worker, env.prep = handed_off, RESYNC_PREP
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", "2"), env, NOW + timedelta(minutes=10)) == "handed_off"
    env.store = st.with_task(env.store, "T002", reviewed_sha="m-t002")  # the review record, then the crash
    del env.prs["T002"]
    env.prep = {"branch": "claude/task/T002-x", "branch_ready": True, "base": "origin/claude/task/T002-x", "remote_sha": "m-t002"}
    later = NOW + timedelta(hours=st.LEASE_HOURS, minutes=11)
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s4", "2"), env, later) == "handed_off"
    t2 = st.entry(env.store, "T002")
    assert t2["status"] == st.IN_PROGRESS and t2["attempts"] == 0 and "resync" not in t2 and len(t2["resyncs"]) == 1
    assert env.runs[-1][0] == "T002" and not env.runs[-1][2].startswith("resync")
    env.store = st.with_task(env.store, "T002", resync=t2["resyncs"][0], reviewed_sha="other")  # tip is not the review
    assert cycle._resync_gate(two_lane(graph, tmp_path, "s4", "2"), env, {"state": env.store},
                              {"id": "T002", "branch_prep": env.prep}, "2") == "fail"


def test_drill_a_resync_whose_close_fails_waits_so_any_merge_is_judged(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    _conflict(env)
    env.close_ok = False
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", "1"), env, NOW + timedelta(minutes=10)) == "merge_pending"
    t2 = st.entry(env.store, "T002")
    assert t2["status"] == st.MERGE_PENDING and "resync" not in t2 and "could not be turned off" in t2["auto_merge"]["why"]


def test_the_handoff_carries_the_resync_text_only_for_a_resync_branch() -> None:
    note = {"attempt": 1, "from": "h", "main": "main0002abcdef0", "paths": ["Cargo.toml"], "at": "t"}
    state = st.claim(st.with_task(st.new_state(), "T002", resync=note), "T002", "a", NOW)
    cfg = dataclasses.replace(config([staged("T002")], Path("."), "a"), lanes=1)
    prep = dict(RESYNC_PREP, main="main0002abcdef0")
    env = SimpleNamespace(last_state=state, dry_run=True, branch_for=lambda tid: "claude/task/T002-x", prepared=prep)
    info = entry.write_handoff(cfg, env)  # type: ignore[arg-type]  # reason: a stand-in for GitHubEnv
    assert info["resync"].startswith("resync, not a new attempt") and "conflicts in Cargo.toml" in info["resync"]
    assert "WITHOUT main merged in" in info["branch_note"] and "git merge main0002abcd" in info["branch_note"]
    plain = SimpleNamespace(**{**vars(env), "prepared": {"branch": "claude/task/T002-x", "branch_ready": True, "base": "main"}})
    assert entry.write_handoff(cfg, plain)["resync"] == ""  # type: ignore[arg-type]  # reason: as above


def test_drill_a_failed_resync_is_a_failed_attempt(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    _conflict(env)
    env.prep = RESYNC_PREP
    env.worker = lambda task, model, log: cycle.WorkerResult("tests_failed", log="smoke red after the merge")
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", "2"), env, NOW + timedelta(minutes=10)) == st.QUEUED
    t2 = st.entry(env.store, "T002")
    assert t2["attempts"] == 1 and "resync" not in t2 and t2["last_failure"] == "smoke red after the merge"
    assert t2["history"][-1]["resync"] is True and t2["history"][-1]["result"] == st.QUEUED


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
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=11)) == "merge_pending"
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=12)) == "merge_pending"
    assert st.entry(env.store, "T002")["update_push_failures"] == 2 and "lane_strikes" not in env.store  # no strike while it waits
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=13)) == "update_push_failed"
    t2 = st.entry(env.store, "T002")
    assert t2["status"] == st.QUEUED and "could not be pushed 3 times" in t2["last_failure"] and t2["runner_closed_prs"] == [2]
    assert env.store["lane_strikes"]["2"]["count"] == 1 and not env.store.get("lanes_fallback")  # one strike: the attempt failed once


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


def test_lane_outcome_strikes_a_check_failure_only_when_the_same_check_passed_before_the_update() -> None:
    failed = [{"attempt": 1, "checks": [{"name": "Ci", "conclusion": "failure"}, {"name": "Twin", "conclusion": "success"}]}]
    update = {"from": "h", "to": "m", "how": "runner", "at": "x", "checks": {"Ci": "success", "Twin": "success", "Ally": "success"}}
    s = st.with_task(st.new_state(), "T002", lane="2", last_failure="Ci: failure", history=failed, update_merges=[update])
    hit = cycle._lane_outcome(s, "T002", "2", "check_failed", NOW)
    assert hit["lane_strikes"]["2"]["count"] == 1 and "T002 check_failed: Ci: failure" in hit["lane_strikes"]["2"]["last"][0]["why"]
    red_before = st.with_task(s, "T002", update_merges=[dict(update, checks={"Ci": "failure"})])
    pending_before = st.with_task(s, "T002", update_merges=[dict(update, checks={"Ci": "None"})])
    no_update = st.with_task(st.new_state(), "T002", lane="2", history=failed)
    for own in (red_before, pending_before, no_update):  # the task's own failure: the run of strikes is broken
        assert cycle._lane_outcome(lanes.strike(own, "2", "x", NOW), "T002", "2", "check_failed", NOW)["lane_strikes"]["2"]["count"] == 0
    assert cycle._lane_outcome(s, "T002", "1", "check_failed", NOW) is s
    assert cycle._lane_outcome(lanes.strike(s, "2", "x", NOW), "T002", "2", "done", NOW)["lane_strikes"]["2"]["count"] == 0
    assert cycle._lane_outcome(s, "T002", "2", "merge_pending", NOW) is s  # a merge still waiting never strikes


def test_drill_a_check_broken_by_mains_change_after_an_update_is_a_strike(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    env.prs["T002"] = dict(green_pr(head_of("T002"), merged=False), number=2, state="open", auto_merge=True)
    env.update = {"status": "pushed", "sha": "m-t002", "main": "main0002"}
    cfg = two_lane(graph, tmp_path, "s3", "2")
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == "branch_updated"
    assert st.entry(env.store, "T002")["update_merges"][-1]["checks"] == {"Ci": "success", "Twin": "success", "Ally": "success"}
    red = dict(green_pr("m-t002", merged=False), number=2, state="open", auto_merge=True)
    red["checks"] = [dict(c, conclusion="failure") if c["name"] == "Ci" else c for c in red["checks"]]
    env.prs["T002"], env.update = red, {"status": "current"}
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=20)) == "check_failed"
    assert env.store["lane_strikes"]["2"]["count"] == 1


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


def _pick(tmp_path: Path, state: st.State, lanes_setting: object, wanted: str = "", branch: str = "", *more: str,
          module: Path = ROOT / "runner" / "state.py") -> list[str]:
    """Run the gate's lane-pick snippet over runner/state.py (or `module`), as the workflow does (`more`: MERGED, D-723)."""
    script = _step("gate", "Pick the lane")["run"]
    code = script.split("python3 -I -c '", 1)[1].split("\n' ", 1)[0]
    (tmp_path / "state.json").write_text(json.dumps({k: v for k, v in state.items() if k not in ("lease", "leases")}))
    (tmp_path / "lease.json").write_text(json.dumps({"lease": state.get("lease"), "leases": st.leases_of(state)}))
    (tmp_path / "runner.json").write_text(json.dumps({"lanes": lanes_setting}))
    args = [sys.executable, "-I", "-c", code, str(module), str(tmp_path / "state.json"),
            str(tmp_path / "lease.json"), str(tmp_path / "runner.json"), wanted, branch, *more]
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
    # Lane 1's task is MERGE_PENDING (no lease) and lane 2 is free: an event for lane 1's task serves
    # lane 2, which can claim, not the busy lane 1 (any run resolves lane 1's merge).
    waiting = st.with_lease(st.with_task(st.claim(s, "T001", "a", later), "T001", status=st.MERGE_PENDING), "1", None)
    assert _pick(tmp_path, waiting, 2, "", "claude/task/T001-x") == ["2", "2", "1", "2"]  # free lanes "1 2" (no live lease)
    assert _pick(tmp_path, dict(held, lanes_fallback={"at": "x", "lane": "2", "why": []}), 2, "2") == ["1", "1", "-"]  # no lane 2 while fallen back


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
    # What a real job writes: the cycle, renewals, the hand-in, and (on an older runner) the safety step's no-op.
    real = [dict(mine, result="handed_off", refill=[]), dict(mine, result="renewed", refill=[]),
            dict(mine, result="MERGE_PENDING", refill=["1"]), dict(mine, result="not_lease_holder", refill=[]),
            dict(mine, result="T001 is MERGE_PENDING", refill=[]), dict(mine, result="renew_refused: x", refill=[])]
    assert _refill(real) == "1"
    assert _refill([dict(mine, result="done", refill=["1", "2"])]) == "1 2"
    for quiet in ("idle", "paced", "lease_live", "no_free_lane", "merge_pending", "lost_race", "branch_updated"):
        assert _refill([dict(mine, result=quiet, refill=["2"])]) == ""
    other = {"at": "2026-11-01T12:06:00", "session": "actions-88", "result": "done", "refill": ["2"]}
    assert _refill([dict(mine, result="idle", refill=["2"]), other]) == ""  # another lane's job is not this one's
    assert _refill([{"at": "2026-11-01T11:00:00", "session": "actions-77", "result": "done", "refill": ["2"]}]) == ""
    assert _refill([dict(mine, result="QUEUED")]) == ""  # one lane: the report carries no refill


def test_the_chain_reads_only_this_jobs_rows_for_a_cleared_red() -> None:
    script = _step("backup", "Start the next cycle")["run"]
    code = script.split("again=\"$(printf '%s' \"$log\" | SESSION=\"actions-${GITHUB_RUN_ID}\" python3 -c '", 1)[1].split("\n' ", 1)[0]
    env = {"JOB_START": "2026-11-01T12:00:00", "SESSION": "actions-77", "PATH": "/usr/bin:/bin"}

    def again(rows: list[dict[str, Any]]) -> str:
        stdin = "".join(json.dumps(r) + "\n" for r in rows)
        return subprocess.run([sys.executable, "-c", code], input=stdin, capture_output=True, text=True, env=env, check=True).stdout.strip()

    assert again([{"at": "2026-11-01T12:05:00", "session": "actions-77", "result": "main_cleared"}]) == "yes"
    assert again([{"at": "2026-11-01T12:05:00", "session": "actions-88", "result": "main_cleared"}]) == "no"


def test_after_removes_the_hand_off_only_when_it_finished_the_task(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(entry, "ROOT", tmp_path)
    (tmp_path / ".git").mkdir()
    handoff = tmp_path / entry.HANDOFF
    for kept in ("not_lease_holder", "lost_race", "state_invalid"):
        handoff.write_text("{}")
        entry.close_handoff(kept)
        assert handoff.exists()  # the safety step must still free the lease
    for finished in (st.MERGE_PENDING, st.QUEUED, "push_failed", "T001 is MERGE_PENDING"):
        handoff.write_text("{}")
        entry.close_handoff(finished)
        assert not handoff.exists()  # so the safety step writes no second, empty --after row
    entry.close_handoff(st.MERGE_PENDING)  # absent is fine


def test_run_lane_prefers_a_lane_with_nothing_in_flight_and_honours_the_fall_back() -> None:
    s = st.with_task(st.with_task(st.new_state(), "T001"), "T002")
    waiting = st.with_lease(st.with_task(st.claim(s, "T001", "a", NOW), "T001", status=st.MERGE_PENDING), "1", None)
    assert st.run_lane(waiting, NOW, 2, "T001") == "2" and st.run_lane(waiting, NOW, 2) == "2"
    assert st.run_lane(waiting, NOW, 1, "T001") == "1"  # one lane: lane 1, as before
    assert st.run_lane(dict(waiting, lanes_fallback={"at": "x"}), NOW, 2, "T001") == "1"


def test_drill_red_main_holds_the_other_lanes_update_merge_and_auto_merge(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    cfg = two_lane(graph, tmp_path, "s3", "2")
    env.store = cycle.set_main_red(env.store, cfg, "nightly", "bad0001", ("T000", "m0", []), NOW)
    env.prs["T002"] = dict(green_pr(head_of("T002"), merged=False), number=2, state="open", auto_merge=False)
    env.update = {"status": "pushed", "sha": "m-t002", "main": "main0002"}
    merges = list(env.auto_merges)
    env.worker = handed_off
    # Both lanes are full, so FIX-MAIN takes the reserved fix lane (D-630); the held lanes move nothing.
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == "handed_off"
    assert st.lane_of(env.store, st.FIX_MAIN) == "fix"
    assert env.updates == [] and env.auto_merges == merges and st.entry(env.store, "T002")["reviewed_sha"] == head_of("T002")
    env.store = dict(env.store, main_red=None)
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=11)) == "branch_updated"  # once main is green it moves again


def test_drill_a_new_review_drops_an_earlier_attempts_update_merges(tmp_path: Path) -> None:
    env = FakeEnv(by_head)
    old = [{"from": "h-old", "to": "m-old", "how": "runner", "at": "x"}]
    env.store = st.with_task(st.sync_graph(env.store, [staged("T001", owns=["app/a/"])]), "T001", update_merges=old,
                             update_waiting_since="2026-11-01T10:00:00Z", update_push_failures=2, attempts=1)
    assert cycle.run_cycle(config([staged("T001", owns=["app/a/"])], tmp_path), env, NOW) == st.MERGE_PENDING
    task = st.entry(env.store, "T001")
    assert task["reviewed_sha"] == head_of("T001") and not {"update_merges", "update_waiting_since", "update_push_failures"} & set(task)
    assert "h-old" not in cycle._earlier_reviewed(task)


def test_finish_lays_only_its_own_usage_onto_a_state_another_lane_moved() -> None:
    base = {"tasks": [], "usage": {"by_task": {"T000": {"output_tokens": 1}}, "windows": {"five_hour": {"start": "s", "tokens": 100}}}}
    ours = {"tasks": [], "usage": {"by_task": {"T000": {"output_tokens": 1}, "T001": {"output_tokens": 50}},
                                   "windows": {"five_hour": {"start": "s", "tokens": 150}}, "last_reading": {"task": "T001"}}}
    fresh = {"tasks": [], "usage": {"by_task": {"T000": {"output_tokens": 1}, "T002": {"output_tokens": 30}},
                                    "windows": {"five_hour": {"start": "s", "tokens": 130}}, "last_reading": {"task": "T002"}}}
    out = cycle._ours_onto(base, ours, fresh)["usage"]
    assert out["by_task"] == {"T000": {"output_tokens": 1}, "T001": {"output_tokens": 50}, "T002": {"output_tokens": 30}}
    assert out["windows"]["five_hour"] == {"start": "s", "tokens": 180} and out["last_reading"] == {"task": "T001"}
    rolled = dict(ours, usage=dict(ours["usage"], windows={"five_hour": {"start": "t", "tokens": 40}}))
    # Ours rolled over to a newer window: that window holds only our tokens; the old one no longer counts (D-630).
    assert cycle._ours_onto(base, rolled, fresh)["usage"]["windows"]["five_hour"] == {"start": "t", "tokens": 40}
    alone = cycle._ours_onto(base, ours, base)["usage"]
    assert alone["windows"]["five_hour"] == {"start": "s", "tokens": 150}  # nobody else moved it: ours as it is
    strikes = cycle._ours_onto({"tasks": [], "lane_strikes": {"2": {"count": 1}}}, {"tasks": [], "lane_strikes": {"2": {"count": 0}}},
                               {"tasks": [], "lane_strikes": {"2": {"count": 1}, "3": {"count": 1}}})
    assert strikes["lane_strikes"] == {"2": {"count": 0}, "3": {"count": 1}}


def test_a_fall_back_note_whose_state_write_was_lost_is_removed(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    env.store = lanes.strike(env.store, "2", "an earlier strike", NOW)
    env.prs["T002"] = dict(green_pr(head_of("T002"), merged=False), number=2, state="open", auto_merge=False)
    env.update = {"status": "conflict", "paths": ["README.md"]}
    env.push_state = lambda new, base: False  # type: ignore[method-assign]  # reason: every state push loses the race
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s3", "2"), env, NOW + timedelta(minutes=10)) == "lost_race"
    assert not (tmp_path / "lanes-fallback.md").exists() and not env.store.get("lanes_fallback")


def test_requeue_with_two_lanes_waits_only_for_a_lease_that_could_meet_the_task(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"]), staged("T003", owns=["app/a/x.rs"]),
             staged("T004", "3", owns=["app/d/"])]
    blocked = st.with_task(st.sync_graph(st.new_state(), graph), "T001", status=st.BLOCKED, attempts=1,
                           reason="branch push rejected (denied); fix the push credential")
    cfg = two_lane(graph, tmp_path, "s1", None)
    side = st.claim(blocked, "T002", "b", NOW, lane="2")
    assert cycle.requeue_refusal(side, "T001", NOW, cfg) == ""
    assert cycle.requeue(side, "T001", "credential fixed", NOW, "me", cfg) is not None
    assert "a live lease is held (T002)" in cycle.requeue_refusal(side, "T001", NOW)  # one lane (no cfg): any lease, as before
    assert "a live lease is held (T002)" in cycle.requeue_refusal(side, "T001", NOW, dataclasses.replace(cfg, lanes=1))
    for other in ("T003", "T004"):  # overlapping owned paths, another stage
        held = st.claim(blocked, other, "c", NOW, lane="2")
        assert f"a live lease is held ({other})" in cycle.requeue_refusal(held, "T001", NOW, cfg)


def test_plan_fix_requeue_with_two_lanes_waits_only_for_a_lease_that_could_meet_the_task(tmp_path: Path) -> None:
    """D-677 with lanes 2 (kit/runner.json on main): the same lease rule as --requeue (D-620)."""
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"]), staged("T003", owns=["app/a/x.rs"]),
             staged("T004", "3", owns=["app/d/"])]
    ended = st.iso(NOW - timedelta(hours=1))
    capped = st.with_task(st.sync_graph(st.new_state(), graph), "T001", status=st.BLOCKED, attempts=3, reason="smoke red",
                          history=[{"attempt": 3, "ended_at": ended, "result": "BLOCKED"}])
    evidence = {"since": ended, "base": "b" * 40, "head": "h" * 40, "plan_then": "1" * 64, "plan_now": "2" * 64,
                "decisions_then": [646], "decisions_now": [646, 677], "task_decisions_then": [], "task_decisions_now": ["D-677"]}
    cfg = two_lane(graph, tmp_path, "s1", None)
    side = st.claim(capped, "T002", "b", NOW, lane="2")
    assert cycle.plan_fix_refusal(side, "T001", "D-677", evidence, NOW, cfg) == ""
    moved = cycle.requeue_plan_fix(side, "T001", "plan fixed", "D-677", evidence, NOW, "me", cfg)
    assert moved is not None and st.status(moved, "T001") == st.QUEUED and st.entry(moved, "T001")["attempts"] == 3
    assert "a live lease is held (T002)" in cycle.plan_fix_refusal(side, "T001", "D-677", evidence, NOW, dataclasses.replace(cfg, lanes=1))
    for other in ("T003", "T004"):  # overlapping owned paths, another stage
        held = st.claim(capped, other, "c", NOW, lane="2")
        assert f"a live lease is held ({other})" in cycle.plan_fix_refusal(held, "T001", "D-677", evidence, NOW, cfg)
        assert cycle.requeue_plan_fix(held, "T001", "x", "D-677", evidence, NOW, "me", cfg) is None


def test_the_lane_count_is_in_the_locked_blueprint() -> None:
    import check_plan_lock

    assert "kit/runner.json" in check_plan_lock.locked_paths(ROOT) and "kit/runner.json" in check_plan_lock.read_lock(ROOT)


# ---- D-630: a red main is always fixable, whatever the lane capacity; pacing windows; strike evidence


def _fix_main_hand_in(task: dict[str, Any], model: str, log: str) -> cycle.WorkerResult:
    """FIX-MAIN's revert of T001's files, reviewed on Opus; other tasks hand in as by_head."""
    if task["id"] == st.FIX_MAIN:
        return reviewed(task, model, paths=["app/a/x.py"], head="fix0001")
    return by_head(task, model, log)


def _held_behind_red_main(tmp_path: Path, fallen_back: bool) -> tuple[FakeEnv, list[dict[str, Any]]]:
    """T001 MERGE_PENDING on lane 1, T002 working on lane 2; T001's merge turns main red (T001 BLOCKED);
    then T002 hands in (MERGE_PENDING, its PR open, held by the red-main guard). With `fallen_back`,
    `lanes_fallback` is set first (capacity 1, both lanes still in flight)."""
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"])]
    env = FakeEnv(by_head)
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s1", "1"), env, NOW) == st.MERGE_PENDING
    env.worker = handed_off
    cfg = two_lane(graph, tmp_path, "s2", "2")
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=1)) == "handed_off"
    if fallen_back:
        env.store = dict(env.store, lanes_fallback={"at": st.iso(NOW), "lane": "2", "why": ["x", "y"]})
    env.store = st.with_task(cycle.set_main_red(env.store, cfg, "tree", "sq0001", ("T001", "sq0001", ["app/a/x.py"]), NOW),
                             "T001", status=st.BLOCKED, reason="squash commit tree differs from the tested head tree")
    assert cycle.after_handoff(cfg, env, lambda t: by_head(t, MODELS["sonnet"], "")) == st.MERGE_PENDING
    env.prs["T002"] = dict(green_pr(head_of("T002"), merged=False), number=2, state="open", auto_merge=False)
    env.worker = _fix_main_hand_in
    return env, graph


@pytest.mark.parametrize("fallen_back", [True, False])
def test_drill_red_main_with_a_held_task_in_a_full_lane_still_gets_fixed(tmp_path: Path, fallen_back: bool) -> None:
    """N1: a fall back (capacity 1) with T002 held MERGE_PENDING by the red-main guard used to leave FIX-MAIN
    unclaimable and main red for good. Without the fall back lane 1 is free and FIX-MAIN takes it, as before."""
    env, graph = _held_behind_red_main(tmp_path, fallen_back)
    cfg = two_lane(graph, tmp_path, "s3", "1")
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=5)) == "fix_main"
    fix = st.entry(env.store, st.FIX_MAIN)
    assert fix["status"] == st.MERGE_PENDING and st.task_lane(fix) == ("fix" if fallen_back else "1")
    assert st.status(env.store, "T002") == st.MERGE_PENDING and env.auto_merges[-1][1] != head_of("T002")  # still held
    if not fallen_back:
        return
    assert st.in_flight(env.store, NOW) == {"T002": "2"}  # the fix lane takes no capacity
    assert cs.check(env.store, lanes=2, now=NOW + timedelta(minutes=5)) == []
    env.prs[st.FIX_MAIN] = dict(green_pr("fix0001"), number=9, merged_sha="sq0009", merged_at="2026-11-01T12:20:00Z")
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=20)) == "main_cleared"
    assert not env.store["main_red"] and st.status(env.store, st.FIX_MAIN) == st.DONE
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=21)) == "merge_pending"
    assert env.auto_merges[-1] == ("claude/task/T002-x", head_of("T002"))  # the guard lifted with main green


def test_drill_lanes_lowered_from_two_to_one_mid_flight_still_fix_a_red_main(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"])]
    env = FakeEnv(by_head)
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s1", "1"), env, NOW) == st.MERGE_PENDING
    assert cycle.run_cycle(two_lane(graph, tmp_path, "s2", "2"), env, NOW + timedelta(minutes=1)) == st.MERGE_PENDING
    for tid, number in (("T001", 1), ("T002", 2)):
        env.prs[tid] = dict(green_pr(head_of(tid), merged=False), number=number, state="open", auto_merge=False)
    one = dataclasses.replace(config(graph, tmp_path, "s3"), lane_hint="1")  # kit/runner.json back to 1
    env.store = cycle.set_main_red(env.store, one, "nightly", "bad0001", ("T000", "m0", ["app/z.rs"]), NOW)
    env.worker = lambda t, m, log: reviewed(t, m, paths=["app/z.rs"], head="fix0001")
    assert cycle.run_cycle(one, env, NOW + timedelta(minutes=5)) == "fix_main"
    assert st.task_lane(st.entry(env.store, st.FIX_MAIN)) == "fix" and st.status(env.store, st.FIX_MAIN) == st.MERGE_PENDING
    assert lanes.fix_lane(env.store, NOW) is None  # FIX-MAIN is in flight: nothing more to claim there


def test_windows_onto_keeps_only_the_newest_window() -> None:
    base = {"five_hour": {"start": "s", "tokens": 100}}
    assert cycle._windows_onto(base, {"five_hour": {"start": "t", "tokens": 40}}, {"five_hour": {"start": "t", "tokens": 30}}) == \
        {"five_hour": {"start": "t", "tokens": 70}}  # both rolled to the same window: both attempts count
    assert cycle._windows_onto(base, {"five_hour": {"start": "s", "tokens": 150}}, {"five_hour": {"start": "u", "tokens": 30}}) == \
        {"five_hour": {"start": "u", "tokens": 30}}  # only the fresh one rolled: ours were spent in the old window
    assert cycle._windows_onto({}, {"five_hour": {"start": "t", "tokens": 40}}, {"five_hour": {"start": None, "tokens": 0}}) == \
        {"five_hour": {"start": "t", "tokens": 40}}


def test_head_checks_records_only_finished_checks() -> None:
    pr = {"head_sha": "h", "checks": [{"name": "Ci", "sha": "h", "conclusion": None}, {"name": "Twin", "sha": "h", "conclusion": "success"},
                                      {"name": "Ally", "sha": "old", "conclusion": "success"}]}
    assert cycle._head_checks(pr) == {"Twin": "success"}


def test_a_check_still_running_at_the_update_is_read_again_when_judging_a_strike() -> None:
    failed = [{"attempt": 1, "checks": [{"name": "Ci", "conclusion": "failure"}]}]
    update = {"from": "h-t002", "to": "m", "how": "runner", "at": "x", "checks": {"Twin": "success"}}
    s = st.with_task(st.new_state(), "T002", lane="2", last_failure="Ci: failure", history=failed, update_merges=[update])
    passed = cycle._lane_outcome(s, "T002", "2", "check_failed", NOW, lambda sha: {"Ci": "success"} if sha == "h-t002" else None)
    assert passed["lane_strikes"]["2"]["count"] == 1
    for later in ({"Ci": "running"}, {"Ci": "failure"}, None):
        assert "lane_strikes" not in cycle._lane_outcome(s, "T002", "2", "check_failed", NOW, lambda sha, v=later: v)
    legacy = st.with_task(s, "T002", update_merges=[dict(update, checks={"Ci": "None"})])  # recorded by D-620's first version
    assert cycle._lane_outcome(legacy, "T002", "2", "check_failed", NOW, lambda sha: {"Ci": "success"})["lane_strikes"]["2"]["count"] == 1


def test_drill_a_strike_judged_on_the_pre_update_heads_final_checks(tmp_path: Path) -> None:
    env, graph = _two_merge_pending(tmp_path)
    pending = dict(green_pr(head_of("T002"), merged=False), number=2, state="open", auto_merge=True)
    pending["checks"] = [dict(c, conclusion=None) if c["name"] == "Ci" else c for c in pending["checks"]]
    env.prs["T002"], env.update = pending, {"status": "pushed", "sha": "m-t002", "main": "main0002"}
    cfg = two_lane(graph, tmp_path, "s3", "2")
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == "branch_updated"
    assert "Ci" not in st.entry(env.store, "T002")["update_merges"][-1]["checks"]
    red = dict(green_pr("m-t002", merged=False), number=2, state="open", auto_merge=True)
    red["checks"] = [dict(c, conclusion="failure") if c["name"] == "Ci" else c for c in red["checks"]]
    env.prs["T002"], env.update = red, {"status": "current"}
    env.checks_of[head_of("T002")] = {"Ci": "success", "Twin": "success", "Ally": "success"}  # Ci passed there in the end
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=20)) == "check_failed"
    assert env.store["lane_strikes"]["2"]["count"] == 1


def test_checks_on_reads_the_latest_run_of_each_required_check(tmp_path: Path) -> None:
    from runner.github_env import GitHubEnv

    runs = {"check_runs": [{"name": "Ci", "status": "completed", "conclusion": "failure", "started_at": "1"},
                           {"name": "Ci", "status": "completed", "conclusion": "success", "started_at": "2"},
                           {"name": "Twin", "status": "in_progress", "conclusion": None, "started_at": "1"},
                           {"name": "Other", "status": "completed", "conclusion": "failure", "started_at": "1"}]}
    answers = {"ok": (0, json.dumps(runs), ""), "fail": (1, "", "HTTP 500")}
    mode = {"now": "ok"}
    env = GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=lambda a, s, c: answers[mode["now"]])
    assert env.checks_on("h") == {"Ci": "success", "Twin": "running"}
    mode["now"] = "fail"
    assert env.checks_on("h") is None and env.main_checks_green("h") is None


# ---- refill after another lane's merge (D-723): the October 30, 2026 stall, where T169's merge on lane 2
# was routed to a lane-1 group already busy with a claim, and the run that finally resolved it reported
# `merge_pending` for its own lane, so the chain step dropped its refill of lane 2.


def _incident() -> st.State:
    """Lane 2's task MERGE_PENDING with no lease (its pull request just merged), lane 1 empty."""
    s = st.with_task(st.with_task(st.with_task(st.new_state(), "T001"), "T002"), "T003")
    return st.with_lease(st.with_task(st.claim(s, "T002", "b", NOW, lane="2"), "T002", status=st.MERGE_PENDING), "2", None)


def test_run_lane_serves_a_merged_pull_requests_own_lane() -> None:
    s = _incident()
    assert st.run_lane(s, NOW, 2, "T002") == "1"  # a ci event (not yet merged) still goes to the free lane (D-630)
    assert st.run_lane(s, NOW, 2, "T002", merged=True) == "2"  # the merge frees lane 2: serve it
    assert st.run_lane(s, NOW, 1, "T002", merged=True) == "1"  # one lane: lane 1, as before
    assert st.run_lane(dict(s, lanes_fallback={"at": "x"}), NOW, 2, "T002", merged=True) == "1"
    assert st.run_lane(s, NOW, 2, "", merged=True) == "1" and st.run_lane(s, NOW, 2, "T999", merged=True) == "1"
    again = st.claim(st.with_task(s, "T002", status=st.QUEUED), "T002", "c", NOW, lane="2")  # lane 2 holds a live lease
    assert st.run_lane(again, NOW, 2, "T002", merged=True) == "1"


def test_the_gate_sends_a_merged_pull_request_to_its_own_lane(tmp_path: Path) -> None:
    later = NOW.replace(year=2030)  # the snippet uses the real clock: leases far in the future are live
    s = st.with_task(st.with_task(st.new_state(), "T001"), "T002")
    pending = st.with_lease(st.with_task(st.claim(s, "T002", "b", later, lane="2"), "T002", status=st.MERGE_PENDING), "2", None)
    assert _pick(tmp_path, pending, 2, "", "claude/task/T002-x", "false") == ["1", "2", "1", "2"]
    assert _pick(tmp_path, pending, 2, "", "claude/task/T002-x", "true") == ["2", "2", "1", "2"]
    assert _pick(tmp_path, pending, 2, "", "claude/task/T002-x") == ["1", "2", "1", "2"]  # no MERGED argument: as before
    assert _pick(tmp_path, pending, 1, "", "claude/task/T002-x", "true") == ["1", "1", "1"]
    assert _pick(tmp_path, pending, 2, "1", "claude/task/T002-x", "true") == ["1", "2", "1", "2"]  # a dispatched lane wins
    step = _step("gate", "Pick the lane")
    assert step["env"]["MERGED"] == "${{ github.event.pull_request.merged == true }}"  # not every closed pull request
    assert '"$WANTED" "$BRANCH" "$MERGED"' in step["run"]


def test_the_chain_refills_after_a_row_that_moved_another_lanes_task() -> None:
    mine = {"at": "2026-11-01T12:05:00", "session": "actions-77"}
    resolved = dict(mine, result="merge_pending", refill=["2"], changed={"keys": {}, "tasks": {"T002": "MERGE_PENDING -> DONE"}})
    assert _refill([resolved]) == "2"  # the row the 14:28 run wrote: its own lane waits, lane 2 was freed
    assert _refill([dict(resolved, changed={"keys": {}, "tasks": {}})]) == ""
    assert _refill([dict(resolved, changed=None)]) == ""
    assert _refill([dict(resolved, result="lease_live")]) == "2"
    assert _refill([dict(resolved, session="actions-88")]) == ""  # another job's row is never this one's


class _Dispatcher:
    """A stand-in for GitHubEnv.dispatch_lane: records each lane, fails or raises on the named ones."""

    def __init__(self, fail: tuple[str, ...] = (), boom: tuple[str, ...] = ()) -> None:
        self.fail, self.boom, self.calls = fail, boom, []

    def dispatch_lane(self, lane: str) -> bool:
        self.calls.append(lane)
        if lane in self.boom:
            raise RuntimeError("gh exploded")
        return lane not in self.fail


def test_start_refill_starts_one_run_per_lane_and_never_raises() -> None:
    env = _Dispatcher(fail=("3",), boom=("4",))
    assert entry.start_refill(["2", "3", "4", "5"], env) == ["2", "5"]  # type: ignore[arg-type]
    assert env.calls == ["2", "3", "4", "5"]
    assert entry.start_refill([], env) == []  # type: ignore[arg-type]


def _main_with(monkeypatch: pytest.MonkeyPatch, tmp_path: Path, result: str, lanes_setting: int,
               argv: list[str]) -> tuple[_Dispatcher, list[dict[str, Any]]]:
    """Run entry.main with the cycle stubbed to `result` and a refill of lane 2; the rows it logs."""
    env, rows = _Dispatcher(), []
    fake = dict(starter="actions", runner_sha="x", log=[], first_state={}, last_state={},
                write_cycle_log=staticmethod(rows.append), dispatch_lane=staticmethod(env.dispatch_lane))
    cfg = dataclasses.replace(config([], tmp_path, "s1"), lanes=lanes_setting, lane_hint="1")
    monkeypatch.setattr(entry, "build", lambda args, outbox: (cfg, type("E", (), fake)()))
    monkeypatch.setattr(entry, "run", lambda args, c, e: result)
    monkeypatch.setattr(entry, "refill", lambda c, e: ["2"])
    monkeypatch.setattr(entry, "write_handoff", lambda c, e: {})
    monkeypatch.setattr(entry, "close_handoff", lambda r: None)
    assert entry.main(argv) == 0
    return env, rows


def test_a_cycle_that_hands_off_starts_its_refill_runs_at_once(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    env, rows = _main_with(monkeypatch, tmp_path, "handed_off", 2, [])
    assert env.calls == ["2"] and rows[-1]["dispatched"] == ["2"] and rows[-1]["refill"] == ["2"]
    for result in ("merge_pending", "idle", st.MERGE_PENDING):  # these end the job within minutes: the chain step refills
        env, rows = _main_with(monkeypatch, tmp_path, result, 2, [])
        assert env.calls == [] and "dispatched" not in rows[-1]
    env, rows = _main_with(monkeypatch, tmp_path, "handed_off", 1, [])
    assert env.calls == [] and "refill" not in rows[-1]  # one lane: nothing new
    env, rows = _main_with(monkeypatch, tmp_path, "handed_off", 2, ["--dry-run"])
    assert env.calls == []


def test_dispatch_lane_uses_the_job_token_and_reports_what_happened(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    from runner import github_env as ge
    from runner.github_env import GitHubEnv

    calls: list[list[str]] = []
    answer = {"now": (0, "", "")}

    def fake(args: list[str], stdin: str | None, cwd: Path) -> tuple[int, str, str]:
        calls.append(args)
        return answer["now"]

    env = GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=fake)
    assert env.dispatch_lane("2") is False and calls == []  # no job token (a routine or local run): nothing started
    monkeypatch.setenv("RUNNER_CHECKS_TOKEN", "fake-job-token")
    assert env.dispatch_lane("2") is True
    assert calls[-1] == ["gh", "workflow", "run", "backup-runner.yml", "--ref", "main", "--repo", "o/r", "-f", "lane=2"]
    assert env.log[-1] == "refill: run started for lane 2"
    answer["now"] = (1, "", "HTTP 403 for ghp_" + "a" * 30)
    assert env.dispatch_lane("1") is False and "lane 1 not started" in env.log[-1] and "[redacted]" in env.log[-1]
    assert env.dispatch_lane("fix") is False and len(calls) == 2
    dry = GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=fake, dry_run=True)
    assert dry.dispatch_lane("2") is False and len(calls) == 2
    monkeypatch.setenv("RUNNER_PUSH_TOKEN", "fake-push-token")
    monkeypatch.setenv("GITHUB_REPOSITORY", "o/r")
    assert ge._long_token_args(calls[0]) is None  # never with the long-lived token
    assert ge._env_for(calls[0])["GH_TOKEN"] == "fake-job-token"
    assert ge._env_for(["gh", "workflow", "run", "other.yml"]) is None  # only the runner's own workflow


def test_the_gate_still_picks_a_lane_with_a_state_module_that_predates_merged(tmp_path: Path) -> None:
    older = tmp_path / "old_state.py"  # a pull request checkout carrying runner/state.py from before D-723
    older.write_text("def live_leases(state, now):\n    return {}\n\n\n"
                     "def run_lane(state, now, lanes, task_id=''):\n    return '2' if task_id == 'T002' else '1'\n")
    s = st.with_task(st.with_task(st.new_state(), "T001"), "T002")
    assert _pick(tmp_path, s, 2, "", "claude/task/T002-x", "true", module=older) == ["2", "2", "1", "2"]
    assert _pick(tmp_path, s, 2, "", "claude/task/T001-x", "false", module=older) == ["1", "2", "1", "2"]


def test_refill_lanes_names_no_lane_while_the_pacer_waits(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T003", owns=["app/c/"])]
    cfg = two_lane(graph, tmp_path, "s1", None)
    s = st.claim(st.sync_graph(st.new_state(), graph), "T001", "a", NOW)
    ahead, behind = st.iso(NOW + timedelta(hours=1)), st.iso(NOW - timedelta(minutes=1))
    assert cycle.refill_lanes(cfg, dict(s, pacing={"decision": "wait", "waiting_until": ahead}), NOW) == []
    assert cycle.refill_lanes(cfg, dict(s, pacing={"decision": "wait", "waiting_until": behind}), NOW) == ["2"]  # the wait is over
    assert cycle.refill_lanes(cfg, dict(s, pacing={"decision": "claim", "waiting_until": None}), NOW) == ["2"]
    assert cycle.refill_lanes(cfg, dict(s, pacing={"decision": "wait", "waiting_until": "not a time"}), NOW) == ["2"]
    assert cycle.refill_lanes(cfg, dict(s, pacing={"decision": "wait", "waiting_until": None}), NOW) == ["2"]
