"""Failure drills (Kit Plan K8): each one must end safely, with nothing marked DONE by mistake.

The runner's outside world is replaced by FakeEnv: a git-like state store whose push is
rejected when someone else pushed first, a scripted worker and a scripted PR.
"""

from __future__ import annotations

import copy
from collections.abc import Callable
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

import pytest

from runner import cycle, state as st

NOW = datetime(2026, 11, 1, 12, 0, tzinfo=timezone.utc)
MODELS = {"haiku": "claude-haiku-5-5", "sonnet": "claude-sonnet-5-5", "opus": "claude-opus-5-5"}


class Crash(Exception):
    """The runner process dies (power cut, killed session)."""


def make_task(tid: str, deps: list[str] | None = None, **extra: Any) -> dict[str, Any]:
    return {"id": tid, "deps": deps or [], "type": "build", "owns": [f"app/{tid.lower()}/"], "model": MODELS["sonnet"], **extra}


def good(task: dict[str, Any], model: str) -> cycle.WorkerResult:
    return cycle.WorkerResult("submitted", [f"app/{task['id'].lower()}/x.py"], [model, model])


class FakeEnv:
    def __init__(self, worker: Callable[[dict[str, Any], str, str], cycle.WorkerResult] | None = None) -> None:
        self.store = st.new_state()
        self.worker = worker or (lambda t, m, log: good(t, m))
        self.prs: dict[str, dict[str, Any]] = {}
        self.inbox: list[tuple[str, str]] = []
        self.runs: list[tuple[str, str, str]] = []
        self.pushes = 0
        self.branches: dict[str, list[str]] = {}

    def read_state(self) -> st.State:
        return copy.deepcopy(self.store)

    def push_state(self, new: st.State, base_version: int) -> bool:
        if base_version != self.store["version"]:
            return False
        self.store = copy.deepcopy(new)
        self.pushes += 1
        return True

    def read_inbox(self) -> list[tuple[str, str]]:
        items, self.inbox = self.inbox, []
        return items

    def pr_for(self, task_id: str) -> dict[str, Any] | None:
        return self.prs.get(task_id)

    def gate_body(self, gate: dict[str, Any]) -> str:
        return "checklist"

    def run_worker(self, task: dict[str, Any], model: str, log: str, heartbeat: Callable[[], None]) -> cycle.WorkerResult:
        self.runs.append((task["id"], model, log))
        return self.worker(task, model, log)


def config(graph: list[dict[str, Any]], tmp_path: Path, session: str = "s1") -> cycle.Config:
    protected = [".github/", "runner/", "stand_ins/", "sources/"]
    return cycle.Config(graph, MODELS, protected, ["docs/decisions.md"], tmp_path, session)


def green_pr(sha: str = "abc1234", tree: str = "t1", merged: bool = True) -> dict[str, Any]:
    checks = [{"name": n, "sha": sha, "conclusion": "success"} for n in ("Ci", "Twin", "Ally")]
    return {"merged": merged, "head_sha": sha, "head_tree": tree, "merge_tree": tree, "checks": checks}


def test_happy_path_ends_done_only_from_green_checks_and_equal_trees(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=5)) == "merge_pending"
    assert st.status(env.store, "T001") == st.MERGE_PENDING
    env.prs["T001"] = green_pr()
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == "done"
    assert st.status(env.store, "T001") == st.DONE


def test_tree_mismatch_is_never_done_and_turns_main_red(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    cycle.run_cycle(cfg, env, NOW)
    env.prs["T001"] = dict(green_pr(), merge_tree="other")
    assert cycle.run_cycle(cfg, env, NOW) == "not_done"
    assert st.status(env.store, "T001") == st.BLOCKED and env.store["main_red"] is True
    assert cycle.run_cycle(cfg, env, NOW) == "fix_main"


def test_green_checks_on_a_different_sha_do_not_count(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    cycle.run_cycle(cfg, env, NOW)
    env.prs["T001"] = dict(green_pr(), head_sha="newer00")
    assert cycle.run_cycle(cfg, env, NOW) == "merge_pending"
    assert st.status(env.store, "T001") == st.MERGE_PENDING


def test_nothing_new_is_claimed_while_merge_pending(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001"), make_task("T002")], tmp_path)
    cycle.run_cycle(cfg, env, NOW)
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=1))
    assert [r[0] for r in env.runs] == ["T001"]


# ---- drill 1: crash after a commit

def test_drill_crash_after_a_commit(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)

    def crashing(task: dict[str, Any], model: str, log: str) -> cycle.WorkerResult:
        env.branches[task["id"]] = ["commit-1"]
        raise Crash

    env.worker = crashing
    with pytest.raises(Crash):
        cycle.run_cycle(cfg, env, NOW)
    assert st.status(env.store, "T001") == st.IN_PROGRESS
    assert cycle.run_cycle(config([make_task("T001")], tmp_path, "s2"), env, NOW + timedelta(hours=1)) == "lease_live"
    env.worker = lambda t, m, log: good(t, m)
    later = NOW + timedelta(hours=3, minutes=1)
    assert cycle.run_cycle(config([make_task("T001")], tmp_path, "s2"), env, later) == st.MERGE_PENDING
    assert st.entry(env.store, "T001")["stale_leases"][0]["session"] == "s1"
    assert env.branches["T001"] == ["commit-1"]
    assert st.status(env.store, "T001") != st.DONE


# ---- drill 2: duplicate trigger

def test_drill_duplicate_trigger_only_one_runner_works(tmp_path: Path) -> None:
    env = FakeEnv()
    graph = [make_task("T001")]
    base = env.read_state()
    claimed = st.claim(base, "T001", "first", NOW)
    assert env.push_state(claimed, base["version"]) is True
    assert env.push_state(st.claim(base, "T001", "second", NOW), base["version"]) is False
    assert st.entry(env.store, "T001")["lease"]["session"] == "first"
    assert cycle.run_cycle(config(graph, tmp_path, "second"), env, NOW + timedelta(minutes=1)) == "lease_live"
    assert env.runs == []


def test_duplicate_trigger_losing_the_push_race_exits(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path, "second")
    real_push = env.push_state

    def other_runner_wins(new: st.State, base_version: int) -> bool:
        env.push_state = real_push  # type: ignore[method-assign]  # reason: restore after simulating the race
        rival = st.claim(env.store, "T001", "first", NOW)
        real_push(rival, env.store["version"])
        return real_push(new, base_version)

    env.push_state = other_runner_wins  # type: ignore[method-assign]  # reason: simulate a concurrent push
    assert cycle.run_cycle(cfg, env, NOW) == "lost_race"
    assert env.runs == []
    assert st.entry(env.store, "T001")["lease"]["session"] == "first"


# ---- drill 3: failed test

def test_drill_failed_test_retries_then_opus_then_blocks(tmp_path: Path) -> None:
    graph = [make_task("T001"), make_task("T002", ["T001"]), make_task("T003")]
    env, cfg = FakeEnv(lambda t, m, log: cycle.WorkerResult("tests_failed", log="2 failed")), config(graph, tmp_path)
    for minute in range(3):
        assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=minute)) in (st.PENDING, st.BLOCKED)
    assert [r[1] for r in env.runs] == [MODELS["sonnet"], MODELS["sonnet"], MODELS["opus"]]
    assert env.runs[1][2] == "2 failed"
    assert st.status(env.store, "T001") == st.BLOCKED
    assert st.entry(env.store, "T001")["attempts"] == 3
    assert st.eligible(graph, env.store, NOW) == ["T003"]
    assert not any(s == st.DONE for s in map(lambda t: st.status(env.store, t["id"]), graph))


def test_failed_required_check_on_open_pr_goes_back_for_a_retry(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    cycle.run_cycle(cfg, env, NOW)
    red = green_pr(merged=False)
    red["checks"][0]["conclusion"] = "failure"
    env.prs["T001"] = red
    assert cycle.run_cycle(cfg, env, NOW) == "check_failed"
    assert st.status(env.store, "T001") == st.PENDING and st.entry(env.store, "T001")["attempts"] == 1


# ---- drill 4: usage limit

def test_drill_usage_limit_waits_for_reset_without_counting_a_failure(tmp_path: Path) -> None:
    reset = NOW + timedelta(hours=5)
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("usage_limit", reset_at=reset))
    cfg = config([make_task("T001")], tmp_path)
    assert cycle.run_cycle(cfg, env, NOW) == st.RETRY_AFTER_RESET
    assert st.entry(env.store, "T001")["attempts"] == 0
    assert cycle.run_cycle(cfg, env, NOW + timedelta(hours=1)) == "idle"
    assert len(env.runs) == 1
    env.worker = lambda t, m, log: good(t, m)
    assert cycle.run_cycle(cfg, env, reset) == st.MERGE_PENDING


def test_usage_limit_without_a_reset_time_retries_hourly(tmp_path: Path) -> None:
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("usage_limit"))
    cycle.run_cycle(config([make_task("T001")], tmp_path), env, NOW)
    assert st.entry(env.store, "T001")["retry_after"] == (NOW + timedelta(hours=1)).isoformat()


def test_vendor_outage_backs_off_without_counting_a_failure(tmp_path: Path) -> None:
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("vendor_outage"))
    cfg = config([make_task("T001")], tmp_path)
    assert cycle.run_cycle(cfg, env, NOW) == st.RETRY_LATER
    assert st.entry(env.store, "T001")["retry_after"] == (NOW + timedelta(minutes=15)).isoformat()
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=15)) == st.RETRY_LATER
    assert st.entry(env.store, "T001")["retry_after"] == (NOW + timedelta(minutes=45)).isoformat()
    assert st.entry(env.store, "T001")["attempts"] == 0


# ---- drill 5: blocked permission

def test_drill_blocked_permission_blocks_only_dependents(tmp_path: Path) -> None:
    graph = [make_task("T001"), make_task("T002", ["T001"]), make_task("T003")]

    def worker(task: dict[str, Any], model: str, log: str) -> cycle.WorkerResult:
        if task["id"] == "T001":
            return cycle.WorkerResult("permission_blocked", log="push to main denied")
        return good(task, model)

    env, cfg = FakeEnv(worker), config(graph, tmp_path)
    assert cycle.run_cycle(cfg, env, NOW) == st.BLOCKED
    assert "push to main denied" in st.entry(env.store, "T001")["reason"]
    assert st.entry(env.store, "T001")["attempts"] == 0
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=1)) == st.MERGE_PENDING
    assert [r[0] for r in env.runs] == ["T001", "T003"]
    assert list(tmp_path.iterdir()) == []


# ---- worker handing in the wrong thing

def test_diff_outside_owned_paths_is_rejected(tmp_path: Path) -> None:
    graph = [make_task("T001")]
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("submitted", ["app/t001/x.py", ".github/workflows/ci.yml"], [m]))
    cycle.run_cycle(config(graph, tmp_path), env, NOW)
    assert st.status(env.store, "T001") == st.PENDING
    assert ".github/workflows/ci.yml" in st.entry(env.store, "T001")["last_failure"]


def test_substituted_model_fails_the_attempt(tmp_path: Path) -> None:
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("submitted", ["app/t001/x.py"], [MODELS["haiku"]]))
    cycle.run_cycle(config([make_task("T001")], tmp_path), env, NOW)
    assert st.status(env.store, "T001") == st.PENDING
    assert "claude-haiku-5-5" in st.entry(env.store, "T001")["last_failure"]


def test_heartbeat_renews_the_lease(tmp_path: Path) -> None:
    env = FakeEnv()
    beats: list[str] = []

    def worker_with_heartbeat(task: dict[str, Any], model: str, log: str) -> cycle.WorkerResult:
        beats.append(env.store["tasks"]["T001"]["lease"]["renewed"])
        return good(task, model)

    env.worker = worker_with_heartbeat
    real = env.run_worker

    def run(task: dict[str, Any], model: str, log: str, heartbeat: Callable[[], None]) -> cycle.WorkerResult:
        heartbeat()
        return real(task, model, log, heartbeat)

    env.run_worker = run  # type: ignore[method-assign]  # reason: call the heartbeat the way a worker loop does
    cycle.run_cycle(config([make_task("T001")], tmp_path), env, datetime.now(timezone.utc))
    assert st.status(env.store, "T001") == st.MERGE_PENDING
    assert env.pushes >= 3


# ---- account checkpoint and Gate P

def gate_task() -> dict[str, Any]:
    return {"id": "T127", "deps": ["T001"], "type": "gate", "owns": [],
            "gate": {"kind": "account-checkpoint", "waits_on_owner": True, "outbox": "Your turn: set up accounts",
                     "resume": "Casey comments 'accounts done' in the issue"}}


def test_account_checkpoint_writes_your_turn_and_stops(tmp_path: Path) -> None:
    graph = [make_task("T001"), gate_task(), make_task("T130", ["T127"])]
    env, cfg = FakeEnv(), config(graph, tmp_path)
    env.store = st.with_task(env.store, "T001", status=st.DONE)
    assert cycle.run_cycle(cfg, env, NOW) == "gate_open"
    assert "Your turn: set up accounts" in (tmp_path / "T127.md").read_text(encoding="utf-8")
    assert st.status(env.store, "T127") == st.WAITING_FOR_OWNER
    assert cycle.run_cycle(cfg, env, NOW + timedelta(hours=1)) == "idle"
    assert env.runs == []
    env.inbox = [("T127", "not yet")]
    assert cycle.run_cycle(cfg, env, NOW + timedelta(hours=2)) == "idle"
    env.inbox = [("T127", "accounts done")]
    assert cycle.run_cycle(cfg, env, NOW + timedelta(hours=3)) == st.MERGE_PENDING
    assert [r[0] for r in env.runs] == ["T130"]


def test_gate_p_holds_the_build(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    env.store["build_hold"] = True
    assert cycle.run_cycle(cfg, env, NOW) == "idle" and env.runs == []
    env.inbox = [("GATE-P", "approved to build")]
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING


def test_attempt_model_policy() -> None:
    task = make_task("T001")
    assert [cycle.attempt_model(task, n, MODELS) for n in range(4)] == [MODELS["sonnet"]] * 2 + [MODELS["opus"]] * 2
