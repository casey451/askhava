"""A dead worker's lease is released early (D-3417).

A backup-runner run whose VM was lost ends without its always() safety hand-in (D-503), so its lease used
to hold the lane for the rest of its 3 hours. The cycle now asks GitHub for the run of every live lease
whose session is `actions-<run id>` (not its own) and frees the lane through `state.release_dead`, the
same `_release_lane` as expiry, once the run's latest attempt has been `completed` for 10 minutes. Every
doubt keeps the lease. The runner's outside world is the FakeEnv of tests/test_failure_drills.py, with a
scripted `run_status`.
"""

from __future__ import annotations

import dataclasses
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

import pytest

from runner import cycle, github_env as ge, state as st
from test_failure_drills import NOW, FakeEnv, config, green_pr, make_task

DEAD = "actions-101"
LATER = NOW + timedelta(hours=1)  # well inside the 3-hour lease
ENDED = st.iso(LATER - timedelta(minutes=30))


class RunsEnv(FakeEnv):
    """FakeEnv with `run_status` answering from `answers` by run id (an Exception is raised)."""

    def __init__(self, answers: dict[int, Any]) -> None:
        super().__init__()
        self.answers = answers
        self.asked: list[int] = []

    def run_status(self, run_id: int) -> tuple[str, str, str] | None:
        self.asked.append(run_id)
        found = self.answers.get(run_id)
        if isinstance(found, Exception):
            raise found
        return found  # type: ignore[no-any-return]  # reason: scripted answers, some deliberately malformed


def held(env: FakeEnv, session: str = DEAD, lane: str = "1", tid: str = "T001", at: datetime = NOW) -> None:
    """`tid` claimed on `lane` by `session`, on its second attempt; Gate P's hold stops any new claim."""
    if not env.store["tasks"]:
        env.store = st.sync_graph(env.store, [make_task("T001"), make_task("T002")])
    env.store = st.with_task(st.claim(env.store, tid, session, at, lane), tid, attempts=1)
    env.store["build_hold"] = "gate-p"


def run(env: FakeEnv, tmp_path: Path, session: str = "actions-999", lanes: int = 1, now: datetime = LATER) -> str:
    cfg = dataclasses.replace(config([make_task("T001"), make_task("T002")], tmp_path, session), lanes=lanes)
    return cycle.run_cycle(cfg, env, now)


def test_a_completed_run_past_the_grace_frees_its_lane_as_expiry_does(tmp_path: Path) -> None:
    env = RunsEnv({101: ("completed", "failure", ENDED)})
    held(env)
    lease = st.lease_in(env.store)
    run(env, tmp_path)
    assert env.asked == [101]
    assert st.lease_in(env.store) is None and st.status(env.store, "T001") == st.QUEUED
    task = st.entry(env.store, "T001")
    assert task["attempts"] == 1 and task["stale_leases"] == [lease]


def test_a_dead_run_with_its_reviewed_pr_is_adopted_merge_pending(tmp_path: Path) -> None:
    env = RunsEnv({101: ("completed", "cancelled", ENDED)})
    held(env)
    env.store = st.with_task(env.store, "T001", reviewed_sha="abc1234", review_lease=st.lease_in(env.store))
    env.prs["T001"] = dict(green_pr(sha="abc1234", merged=False), number=7, state="open")
    run(env, tmp_path)
    assert st.lease_in(env.store) is None and st.status(env.store, "T001") == st.MERGE_PENDING
    assert st.entry(env.store, "T001")["attempts"] == 1


@pytest.mark.parametrize("answer", [
    ("queued", "", ENDED), ("in_progress", "", ENDED), ("waiting", "", ENDED), ("requested", "", ENDED),
    None, RuntimeError("HTTP 502"), ("completed",), ("completed", "failure", "not a time"), "garbage",
])
def test_a_live_rerun_or_any_doubt_keeps_the_lease(tmp_path: Path, answer: Any) -> None:
    env = RunsEnv({101: answer})
    held(env)
    before = st.lease_in(env.store)
    run(env, tmp_path)
    assert env.asked == [101]
    assert st.lease_in(env.store) == before and st.status(env.store, "T001") == st.IN_PROGRESS
    assert "stale_leases" not in st.entry(env.store, "T001")


@pytest.mark.parametrize("session", ["routine-1a2b3c", "manual-101", "s1", "actions-", "actions-12x", "xactions-101"])
def test_a_session_that_is_not_a_backup_run_is_never_asked_about(tmp_path: Path, session: str) -> None:
    env = RunsEnv({101: ("completed", "failure", ENDED)})
    held(env, session)
    run(env, tmp_path)
    assert env.asked == [] and st.lease_in(env.store)["session"] == session


def test_the_cycles_own_session_is_never_released(tmp_path: Path) -> None:
    env = RunsEnv({101: ("completed", "failure", ENDED)})
    held(env)
    run(env, tmp_path, session=DEAD)
    assert env.asked == [] and st.lease_in(env.store)["session"] == DEAD


def test_a_run_completed_within_the_grace_keeps_the_lease_until_it_passes(tmp_path: Path) -> None:
    env = RunsEnv({101: ("completed", "failure", st.iso(LATER - timedelta(minutes=9, seconds=59)))})
    held(env)
    run(env, tmp_path)
    assert st.lease_in(env.store)["session"] == DEAD
    env.answers[101] = ("completed", "failure", st.iso(LATER - cycle.DEAD_RUN_GRACE))
    run(env, tmp_path)
    assert st.lease_in(env.store) is None and st.status(env.store, "T001") == st.QUEUED


def test_an_env_without_run_status_frees_only_on_expiry(tmp_path: Path) -> None:
    env = FakeEnv()
    held(env)
    run(env, tmp_path)
    assert st.lease_in(env.store)["session"] == DEAD
    assert cycle.dead_sessions(env.store, env, "actions-999", LATER) == set()


def test_two_lanes_one_dead_frees_only_that_lane(tmp_path: Path) -> None:
    env = RunsEnv({101: ("completed", "failure", ENDED), 202: ("in_progress", "", ENDED)})
    held(env)
    held(env, "actions-202", lane="2", tid="T002")
    run(env, tmp_path, lanes=2)
    assert sorted(env.asked) == [101, 202]
    assert st.lease_in(env.store) is None and st.status(env.store, "T001") == st.QUEUED
    assert st.lease_in(env.store, "2")["session"] == "actions-202" and st.status(env.store, "T002") == st.IN_PROGRESS
    assert "stale_leases" not in st.entry(env.store, "T002")


def test_an_expired_lease_is_released_by_expiry_and_not_asked_about(tmp_path: Path) -> None:
    env = RunsEnv({101: ("completed", "failure", ENDED)})
    held(env)
    run(env, tmp_path, now=NOW + timedelta(hours=3, minutes=1))
    assert env.asked == [] and st.lease_in(env.store) is None
    assert len(st.entry(env.store, "T001")["stale_leases"]) == 1


def test_a_merge_pending_task_is_released_exactly_as_expiry_releases_it(tmp_path: Path) -> None:
    env = RunsEnv({101: ("completed", "success", ENDED)})
    held(env)
    env.store = st.with_task(env.store, "T001", status=st.MERGE_PENDING)
    expired = st.release_expired(env.store, NOW + timedelta(hours=3, minutes=1))
    assert st.release_dead(env.store, {DEAD}) == expired
    run(env, tmp_path)
    assert st.lease_in(env.store) is None and st.status(env.store, "T001") == st.MERGE_PENDING
    assert st.entry(env.store, "T001")["attempts"] == 1


def test_release_dead_matches_release_expired_and_ignores_other_sessions() -> None:
    state = st.sync_graph(st.new_state(), [make_task("T001"), make_task("T002")])
    state = st.claim(st.claim(state, "T001", DEAD, NOW), "T002", "actions-202", NOW, "2")
    assert st.release_dead(state, set()) == state and st.release_dead(state, {"actions-303"}) == state
    out = st.release_dead(state, {DEAD})
    assert st.lease_in(out) is None and st.status(out, "T001") == st.QUEUED
    assert st.entry(out, "T001")["stale_leases"] == [st.lease_in(state)]
    assert st.lease_in(out, "2") == st.lease_in(state, "2") and st.status(out, "T002") == st.IN_PROGRESS


def test_the_real_env_reads_the_runs_latest_attempt_and_fails_closed(tmp_path: Path) -> None:
    env = ge.GitHubEnv(tmp_path, [], tmp_path / "outbox", repo="fake-owner/fake-repo", starter="routine",
                       runner_sha="feedc0de" * 5, clock=lambda: NOW)
    paths: list[str] = []

    def answer(path: str) -> Any:
        paths.append(path)
        return {"status": "completed", "conclusion": None, "updated_at": ENDED, "run_attempt": 2}

    env.gh_json = answer  # type: ignore[method-assign]  # reason: no network in tests
    assert env.run_status(101) == ("completed", "", ENDED) and paths == ["repos/fake-owner/fake-repo/actions/runs/101?exclude_pull_requests=true"]
    env.gh_json = lambda path: {"status": "completed"}  # type: ignore[method-assign]  # reason: a run with no updated_at
    assert env.run_status(101) is None
    env.gh_json = lambda path: (_ for _ in ()).throw(RuntimeError("HTTP 404"))  # type: ignore[method-assign]  # reason: no such run
    assert env.run_status(101) is None and "run 101 unreadable" in env.log[-1]


def test_the_run_read_uses_the_job_token_and_routes_nothing_else(monkeypatch: pytest.MonkeyPatch) -> None:
    """The run read goes with RUNNER_CHECKS_TOKEN, as the check-run and job reads do (D-502, D-582); no other
    `actions/runs` path, workflow-runs listing or cache call is routed to it, and without the job token
    nothing is."""
    path = "repos/o/r/actions/runs/101?exclude_pull_requests=true"
    call = ["gh", "api", "-H", "Accept: application/vnd.github+json", path]
    monkeypatch.delenv("RUNNER_CHECKS_TOKEN", raising=False)
    assert ge._env_for(call) is None
    monkeypatch.setenv("RUNNER_CHECKS_TOKEN", "fake-job-token")
    monkeypatch.setenv("RUNNER_PUSH_TOKEN", "fake-push-token")
    monkeypatch.setenv("GITHUB_REPOSITORY", "o/r")
    env = ge._env_for(call)
    assert env is not None and env["GH_TOKEN"] == "fake-job-token" and ge._long_token_args(call) is None
    for other in ("repos/o/r/actions/runs/77", "repos/o/r/actions/workflows/backup-runner.yml/runs?status=completed",
                  "repos/o/r/actions/runs?status=in_progress", "repos/o/r/actions/runs/101/cancel",
                  "repos/o/r/actions/runs/101?exclude_pull_requests=true&x=1", "repos/o/r/actions/caches?key=k"):
        assert ge._env_for(["gh", "api", other]) is None, other
    assert ge._env_for(["gh", "api", "-X", "POST", "repos/o/r/actions/runs/101/cancel"]) is None
