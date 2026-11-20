"""The WIP policy (T727, D-4720): when a failed attempt's head is kept on a `claude/wip/<id>-a<n>` ref, when a
claim continues it, and the sweep that deletes every ref no task records. The mechanics (scans, pushes, the
listing) are T726's and replaced here by WipEnv. Fake data only."""

from __future__ import annotations

import copy
from datetime import timedelta
from pathlib import Path
from typing import Any

import check_state as cs
import pytest

from runner import cycle, state as st
from tests.test_failure_drills import NOW, FakeEnv, config, green_pr, make_task

HEAD = "a" * 40
OTHER = "b" * 40
REF = "claude/wip/T001-a1"
REVIEWER = "claude-opus-5-5"


def result(outcome: str = "review_failed", log: str = "REVIEW-VERDICT: FINDINGS app/t001/x.py:3 off by one", **extra: Any) -> cycle.WorkerResult:
    fields: dict[str, Any] = {"head_sha": HEAD, "commits_ahead": 2, "branch": "claude/task/T001-x", "reviewer_evidence": [REVIEWER],
                              "changed_paths": ["app/t001/x.py"], "model_evidence": ["claude-sonnet-5-5"] * 2}
    return cycle.WorkerResult(outcome, log=log, **{**fields, **extra})


class WipEnv(FakeEnv):
    """FakeEnv with T726's WIP methods: a scripted check, push, listing and delete."""

    def __init__(self, res: cycle.WorkerResult | None = None) -> None:
        super().__init__(lambda t, m, log: copy.deepcopy(res or result()))
        self.prep = {"branch": "claude/task/T001-x", "branch_ready": True, "base": "main", "base_sha": "main0001", "remote_sha": ""}
        self.flag, self.refusal, self.push_ok, self.delete_ok = True, "", True, True
        self.wip_pushes: list[tuple[str, str, Any]] = []
        self.refs: dict[str, str] = {}
        self.deleted: list[str] = []
        self.offered: list[Any] = []
        self.continue_ok = True
        self.after_listing: Any = None

    def wip_continue(self) -> bool:
        return self.flag

    def wip_check(self, head: str) -> str:
        if isinstance(self.refusal, Exception):
            raise self.refusal
        return self.refusal

    def push_wip(self, ref: str, sha: str) -> bool:
        self.wip_pushes.append((ref, sha, copy.deepcopy(st.entry(self.store, "T001").get("wip"))))
        if self.push_ok:
            self.refs[ref] = sha
        return self.push_ok

    def prepare_branch(self, task: dict[str, Any], attempt: int, session: str) -> dict[str, Any]:
        self.offered.append(task.get("wip_continue"))
        prep = super().prepare_branch(task, attempt, session)
        if task.get("wip_continue"):
            done = {"base": "wip", "continued_from": task["wip_continue"]["ref"]} if self.continue_ok else {"wip_dropped": "the WIP ref is missing"}
            prep.update(done)
        return prep

    def wip_refs(self) -> dict[str, str] | None:
        listed = dict(self.refs)
        if self.after_listing:
            self.after_listing(self)
        return listed

    def delete_wip_ref(self, name: str) -> bool:
        self.deleted.append(name)
        if self.delete_ok:
            self.refs.pop(name, None)
        return self.delete_ok


def run(env: FakeEnv, tmp_path: Path, minutes: int = 0) -> dict[str, Any]:
    cycle.run_cycle(config([make_task("T001")], tmp_path), env, NOW + timedelta(minutes=minutes))
    return st.entry(env.store, "T001")


def test_a_review_failure_records_the_wip_before_pushing_it(tmp_path: Path) -> None:
    env = WipEnv()
    entry = run(env, tmp_path)
    rec = {"ref": REF, "sha": HEAD, "attempt": 1, "kind": "review", "at": st.iso(NOW)}
    assert env.wip_pushes == [(REF, HEAD, rec)]  # the store already held the record when push_wip ran
    assert entry["wip"] == rec and "wip_claim" not in entry and "wip_skipped" not in entry["history"][-1]
    assert env.branch_pushes == [] and env.auto_merges == []  # never a task branch, a pull request or auto-merge


@pytest.mark.parametrize("outcome", ["review_failed", "tests_failed"])
def test_review_failed_counts_exactly_as_tests_failed(tmp_path: Path, outcome: str) -> None:
    env = FakeEnv(lambda t, m, log: result(outcome))  # an Env without the WIP methods: nothing new
    entry = run(env, tmp_path)
    assert (entry["status"], entry["attempts"], entry["last_failure"]) == (st.QUEUED, 1, result().log)
    assert not {"wip", "wip_claim", "wip_used"} & set(entry)
    env.store = st.with_task(env.store, "T001", attempts=cycle.MAX_ATTEMPTS - 1)
    assert run(env, tmp_path, 10)["status"] == st.BLOCKED


@pytest.mark.parametrize("refusal", ["blocked path: runner/cycle.py", "gitleaks: 1 finding in app/t001/x.py",
                                     "scanner error: gitleaks exited 2", RuntimeError("scanner crashed")])
def test_a_refusal_or_a_scanner_error_pushes_nothing_and_says_why(tmp_path: Path, refusal: Any) -> None:
    env = WipEnv()
    env.refusal = refusal
    entry = run(env, tmp_path)
    assert env.wip_pushes == [] and "wip" not in entry and entry["status"] == st.QUEUED
    assert str(refusal) in entry["history"][-1]["wip_skipped"]


def test_a_failed_push_drops_the_record_and_says_so(tmp_path: Path) -> None:
    env = WipEnv()
    env.push_ok = False
    entry = run(env, tmp_path)
    assert len(env.wip_pushes) == 1 and "wip" not in entry and entry["history"][-1]["wip_skipped"] == "the WIP push failed"


@pytest.mark.parametrize("log", ["the worker weakened test_x", "an assertion removed in tests/a.py", "Test removed: test_b",
                                 "edits a PROTECTED PATH", "tampered with runner/"])
def test_a_log_naming_test_weakening_or_tampering_pushes_nothing(tmp_path: Path, log: str) -> None:
    env = WipEnv(result(log=log))
    entry = run(env, tmp_path)
    assert env.wip_pushes == [] and "wip" not in entry and entry["history"][-1]["wip_skipped"]


@pytest.mark.parametrize("res", [
    result("tests_failed", "session ended without handing in"),  # a timeout
    result("tests_failed", "prehandin red: smoke failed"),  # a smoke-red or prehandin-red gate
    result("submitted", changed_paths=["runner/cycle.py"]),  # a path-check failure
    result("usage_limit"), result("api_error", "Overloaded"),
    result(reviewer_evidence=[]),  # review_failed without a reviewer's evidence
    result(commits_ahead=0),
])
def test_every_other_ending_pushes_nothing(tmp_path: Path, res: cycle.WorkerResult) -> None:
    env = WipEnv(res)
    entry = run(env, tmp_path)
    assert env.wip_pushes == [] and "wip" not in entry and "wip_claim" not in entry
    assert "wip_skipped" not in (entry.get("history") or [{}])[-1]


def test_a_task_branch_on_origin_or_the_flag_off_pushes_nothing(tmp_path: Path) -> None:
    env = WipEnv()
    env.prep["remote_sha"] = OTHER
    assert "wip" not in run(env, tmp_path) and env.wip_pushes == []
    off = WipEnv()
    off.flag = False
    assert "wip" not in run(off, tmp_path) and off.wip_pushes == []


def test_the_flag_turned_off_never_continues_a_kept_wip(tmp_path: Path) -> None:
    env = WipEnv()
    assert run(env, tmp_path)["wip"]["ref"] == REF  # kept while the flag was on
    env.flag = False  # the overseer's kill switch in kit/runner.json (D-4720)
    entry = run(env, tmp_path, 10)
    assert env.offered == [None, None] and len(env.wip_pushes) == 1
    assert "continued_from" not in entry["history"][-1] and "wip" not in entry


def test_a_continued_attempt_once_only_then_fresh(tmp_path: Path) -> None:
    env = WipEnv()
    run(env, tmp_path)
    entry = run(env, tmp_path, 10)  # the retry continues the kept head and fails again
    assert env.offered == [None, {"ref": REF, "sha": HEAD}]
    assert entry["wip_used"] is True and "wip" not in entry and entry["history"][-1]["continued_from"] == REF
    assert len(env.wip_pushes) == 1  # a continued attempt that fails again pushes nothing
    run(env, tmp_path, 20)
    assert env.offered[-1] is None and len(env.wip_pushes) == 1


def test_a_missing_ref_or_sha_mismatch_starts_fresh_and_drops_the_wip(tmp_path: Path) -> None:
    env = WipEnv()
    run(env, tmp_path)
    env.continue_ok = False
    entry = run(env, tmp_path, 10)
    assert entry["history"][-1]["wip_dropped"] == "the WIP ref is missing" and "wip" not in entry and entry["wip_used"] is True


def plan_gap_kept(tmp_path: Path) -> WipEnv:
    env = WipEnv(result("plan_gap", "PLAN GAP: outside ownership: app/other/x.rs, `docs/y.md`", reviewer_evidence=[]))
    entry = run(env, tmp_path)
    assert entry["status"] == st.BLOCKED and entry["plan_gap"]["paths"] == ["app/other/x.rs", "docs/y.md"]
    assert entry["wip"]["kind"] == "plan_gap" and env.wip_pushes
    return env


def test_a_plan_gap_wip_continues_only_after_a_plan_fix_requeue(tmp_path: Path) -> None:
    env = plan_gap_kept(tmp_path)
    later = st.iso(NOW + timedelta(minutes=5))
    env.store = st.with_task(env.store, "T001", status=st.QUEUED, requeues=[{"at": later, "plan_fix": "D-123"}])
    run(env, tmp_path, 10)
    assert env.offered[-1] == {"ref": "claude/wip/T001-a1", "sha": HEAD}


def test_a_plan_gap_wip_is_not_continued_after_a_plain_requeue(tmp_path: Path) -> None:
    env = plan_gap_kept(tmp_path)
    requeued = cycle.requeue(env.store, "T001", "mistaken", NOW + timedelta(minutes=5), "test")
    assert requeued is not None and "plan_gap" not in st.entry(requeued, "T001")  # the reported record goes
    env.store, env.worker = requeued, lambda t, m, log: result("tests_failed", "fails again")
    entry = run(env, tmp_path, 10)
    assert env.offered[-1] is None and "wip" not in entry  # a fresh start drops the record
    early = {"wip": {"ref": REF, "sha": HEAD, "kind": "plan_gap", "at": st.iso(NOW)}, "requeues": [{"at": st.iso(NOW), "plan_fix": "D-1"}]}
    assert cycle._wip_offer(env, early) is None  # a plan fix not later than the record


def test_no_pull_request_auto_merge_or_adoption_from_a_wip_ref(tmp_path: Path) -> None:
    env = WipEnv()
    run(env, tmp_path)
    env.prs["T001"] = green_pr(HEAD, merged=False)  # even a pull request on the WIP head
    dead = {"task": "T001", "session": "dead", "expires": st.iso(NOW)}
    env.store = st.with_lease(st.with_task(env.store, "T001", status=st.IN_PROGRESS), st.LANE_ONE, dead)
    env.worker = lambda t, m, log: result("tests_failed", "fails again")
    entry = run(env, tmp_path, 30)
    assert entry["status"] != st.MERGE_PENDING and env.auto_merges == [] and env.branch_pushes == []


def recorded(tid: str, sha: str, status: str = st.QUEUED) -> dict[str, Any]:
    return {"id": tid, "deps": [], "status": status, "attempts": 1, "wip": {"ref": f"claude/wip/{tid}-a1", "sha": sha}}


@pytest.mark.parametrize("flag", [True, False])
def test_the_sweep_deletes_every_unrecorded_ref_and_keeps_a_recorded_one(flag: bool) -> None:
    env = WipEnv()
    env.flag = flag
    env.store = cycle.drop_closed_wips(dict(st.new_state(), tasks=[
        recorded("T001", HEAD, st.DONE), recorded("T002", HEAD, st.SKIPPED), recorded("T003", HEAD), recorded("T004", OTHER)]))
    env.refs = {f"claude/wip/T00{n}-a1": HEAD for n in range(1, 5)} | {"claude/wip/T009-a3": HEAD}

    def lane_records(e: WipEnv) -> None:  # another lane records then pushes between the listing and the read
        e.store = dict(e.store, tasks=[*e.store["tasks"], recorded("T005", OTHER)])
        e.refs["claude/wip/T005-a1"] = OTHER

    env.after_listing = lane_records
    assert cycle.wip_sweep(env) == ["claude/wip/T001-a1", "claude/wip/T002-a1", "claude/wip/T004-a1", "claude/wip/T009-a3"]
    assert set(env.refs) == {"claude/wip/T003-a1", "claude/wip/T005-a1"}


def test_the_sweep_keeps_a_ref_recorded_between_the_listing_and_the_read() -> None:
    env = WipEnv()
    env.store = dict(st.new_state(), tasks=[{"id": "T006", "deps": [], "status": st.IN_PROGRESS}])
    env.refs = {"claude/wip/T006-a1": HEAD}  # pushed before the listing, so recorded before it too (D-515's order)
    env.after_listing = lambda e: setattr(e, "store", dict(e.store, tasks=[recorded("T006", HEAD, st.IN_PROGRESS)]))
    assert cycle.wip_sweep(env) == [] and env.refs == {"claude/wip/T006-a1": HEAD}


def test_a_failed_delete_is_retried_and_a_plain_env_sweeps_nothing() -> None:
    env = WipEnv()
    env.refs, env.delete_ok = {"claude/wip/T009-a1": HEAD}, False
    assert cycle.wip_sweep(env) == [] and cycle.wip_sweep(env) == [] and env.deleted == ["claude/wip/T009-a1"] * 2
    assert cycle.wip_sweep(FakeEnv()) == []  # type: ignore[arg-type]


def test_the_cycle_sweeps_and_drops_a_done_tasks_wip(tmp_path: Path) -> None:
    env = WipEnv()
    run(env, tmp_path)
    env.store = st.with_task(env.store, "T001", status=st.DONE)
    run(env, tmp_path, 10)
    assert "wip" not in st.entry(env.store, "T001") and env.deleted == [REF] and env.refs == {}


def test_check_state_reads_states_with_and_without_the_wip_keys(tmp_path: Path) -> None:
    env = WipEnv()
    plain = copy.deepcopy(env.store)
    run(env, tmp_path)
    kept = env.store
    env.store = st.with_task(env.store, "T001", plan_gap={"at": st.iso(NOW), "problems": ["x"], "paths": ["a/b.rs"]})
    run(env, tmp_path, 10)
    for state in (plain, kept, env.store):
        assert cs.check(state, lanes=1, now=NOW) == []
    assert "wip" in st.entry(kept, "T001") and "continued_from" in st.entry(env.store, "T001")["history"][-1]
