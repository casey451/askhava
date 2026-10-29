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

from runner import cycle, state as st, verify

NOW = datetime(2026, 11, 1, 12, 0, tzinfo=timezone.utc)
MODELS = {"haiku": "claude-haiku-5-5", "sonnet": "claude-sonnet-5-5", "opus": "claude-opus-5-5"}


class Crash(Exception):
    """The runner process dies (power cut, killed session)."""


def make_task(tid: str, deps: list[str] | None = None, **extra: Any) -> dict[str, Any]:
    return {
        "id": tid,
        "deps": deps or [],
        "type": "build",
        "owns": [f"app/{tid.lower()}/"],
        "model": MODELS["sonnet"],
        **extra,
    }


def good(task: dict[str, Any], model: str) -> cycle.WorkerResult:
    return cycle.WorkerResult("submitted", [f"app/{task['id'].lower()}/x.py"], [model, model])


class FakeEnv:
    def __init__(self, worker: Callable[[dict[str, Any], str, str], cycle.WorkerResult] | None = None) -> None:
        self.store = st.new_state()
        self.worker = worker or (lambda t, m, log: good(t, m))
        self.prs: dict[str, dict[str, Any]] = {}
        self.inbox: list[tuple[str, str, str]] = []
        self.acked: list[str] = []
        self.runs: list[tuple[str, str, str]] = []
        self.pushes = 0
        self.nightly: dict[str, Any] | None = None
        self.head = "main0001"
        self.smoke_green = True
        self.checks_green: bool | None = None
        self.smokes: list[str] = []
        self.branch_pushes: list[tuple[str, str]] = []
        self.auto_merges: list[tuple[str, str]] = []
        self.main_merges = 0

    def read_state(self) -> st.State:
        return copy.deepcopy(self.store)

    def push_state(self, new: st.State, base: st.State) -> bool:
        if base != self.store:
            return False
        self.store = copy.deepcopy(new)
        self.pushes += 1
        return True

    def read_inbox(self) -> list[tuple[str, str, str]]:
        return list(self.inbox)

    def ack_inbox(self, file_ids: list[str]) -> None:
        self.acked += file_ids
        self.inbox = [i for i in self.inbox if i[0] not in file_ids]

    def pr_for(self, task_id: str) -> dict[str, Any] | None:
        return self.prs.get(task_id)

    def gate_body(self, gate: dict[str, Any]) -> str:
        return "checklist"

    def run_worker(
        self, task: dict[str, Any], model: str, log: str, heartbeat: Callable[[], None]
    ) -> cycle.WorkerResult:
        self.runs.append((task["id"], model, log))
        return self.worker(task, model, log)

    def read_nightly(self) -> dict[str, Any] | None:
        return copy.deepcopy(self.nightly)

    def main_head(self) -> str:
        return self.head

    def smoke_main(self) -> tuple[bool, str]:
        self.smokes.append(self.head)
        return self.smoke_green, self.head

    def main_checks_green(self, head: str) -> bool | None:
        return self.checks_green

    def push_branch(self, branch: str, head_sha: str) -> bool:
        self.branch_pushes.append((branch, head_sha))
        return True

    def enable_auto_merge(self, branch: str, head_sha: str) -> tuple[bool, str]:
        self.auto_merges.append((branch, head_sha))
        return True, "fake"

    def merge_main(self) -> str:
        self.main_merges += 1
        return "merged"


def config(
    graph: list[dict[str, Any]], tmp_path: Path, session: str = "s1", clock: Callable[[], datetime] | None = None
) -> cycle.Config:
    protected = [".github/", "runner/", "stand_ins/", "sources/"]
    return cycle.Config(graph, MODELS, protected, ["docs/decisions.md"], tmp_path, session, clock or (lambda: NOW))


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
    assert st.status(env.store, "T001") == st.BLOCKED
    assert env.store["main_red"]["check"] == "tree" and env.store["main_red"]["task"] == "T001"
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
    assert st.status(env.store, "T001") != st.DONE


# ---- drill 2: duplicate trigger


def test_drill_duplicate_trigger_only_one_runner_works(tmp_path: Path) -> None:
    env = FakeEnv()
    graph = [make_task("T001")]
    base = env.read_state()
    claimed = st.claim(base, "T001", "first", NOW)
    assert env.push_state(claimed, base) is True
    assert env.push_state(st.claim(base, "T001", "second", NOW), base) is False
    assert env.store["lease"]["session"] == "first"
    assert cycle.run_cycle(config(graph, tmp_path, "second"), env, NOW + timedelta(minutes=1)) == "lease_live"
    assert env.runs == []


def test_duplicate_trigger_losing_the_push_race_exits(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path, "second")
    real_push = env.push_state

    def other_runner_wins(new: st.State, base: st.State) -> bool:
        env.push_state = real_push  # type: ignore[method-assign]  # reason: restore after simulating the race
        rival = st.claim(env.store, "T001", "first", NOW)
        real_push(rival, env.store)
        return real_push(new, base)

    env.push_state = other_runner_wins  # type: ignore[method-assign]  # reason: simulate a concurrent push
    assert cycle.run_cycle(cfg, env, NOW) == "lost_race"
    assert env.runs == []
    assert env.store["lease"]["session"] == "first"


# ---- drill 3: failed test


def test_drill_failed_test_retries_then_opus_then_blocks(tmp_path: Path) -> None:
    graph = [make_task("T001"), make_task("T002", ["T001"]), make_task("T003")]
    env, cfg = FakeEnv(lambda t, m, log: cycle.WorkerResult("tests_failed", log="2 failed")), config(graph, tmp_path)
    for minute in range(3):
        assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=minute)) in (st.QUEUED, st.BLOCKED)
    assert [r[1] for r in env.runs] == [MODELS["sonnet"], MODELS["sonnet"], MODELS["opus"]]
    assert env.runs[1][2] == "2 failed"
    assert st.status(env.store, "T001") == st.BLOCKED
    assert st.entry(env.store, "T001")["attempts"] == 3
    assert st.eligible(env.store, NOW) == ["T003"]
    assert all(st.status(env.store, t["id"]) != st.DONE for t in graph)


def test_failed_required_check_on_open_pr_goes_back_for_a_retry(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    cycle.run_cycle(cfg, env, NOW)
    red = green_pr(merged=False)
    red["checks"][0]["conclusion"] = "failure"
    env.prs["T001"] = red
    assert cycle.run_cycle(cfg, env, NOW) == "check_failed"
    assert st.status(env.store, "T001") == st.QUEUED and st.entry(env.store, "T001")["attempts"] == 1


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
    assert st.entry(env.store, "T001")["retry_after"] == st.iso(NOW + timedelta(hours=1))


def test_vendor_outage_backs_off_without_counting_a_failure(tmp_path: Path) -> None:
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("vendor_outage"))
    cfg = config([make_task("T001")], tmp_path)
    assert cycle.run_cycle(cfg, env, NOW) == st.RETRY_LATER
    assert st.entry(env.store, "T001")["retry_after"] == st.iso(NOW + timedelta(minutes=15))
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=15)) == st.RETRY_LATER
    assert st.entry(env.store, "T001")["retry_after"] == st.iso(NOW + timedelta(minutes=45))
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
    assert st.status(env.store, "T001") == st.QUEUED
    assert ".github/workflows/ci.yml" in st.entry(env.store, "T001")["last_failure"]


def test_substituted_model_fails_the_attempt(tmp_path: Path) -> None:
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("submitted", ["app/t001/x.py"], [MODELS["haiku"]]))
    cycle.run_cycle(config([make_task("T001")], tmp_path), env, NOW)
    assert st.status(env.store, "T001") == st.QUEUED
    assert "claude-haiku-5-5" in st.entry(env.store, "T001")["last_failure"]


def test_heartbeat_renews_only_when_due_using_the_injected_clock(tmp_path: Path) -> None:
    env = FakeEnv()
    clock = {"now": NOW}
    seen: list[str] = []

    def run(task: dict[str, Any], model: str, log: str, heartbeat: Callable[[], None]) -> cycle.WorkerResult:
        original = env.store["lease"]["expires"]
        clock["now"] = NOW + timedelta(minutes=29)
        pushes = env.pushes
        heartbeat()
        assert env.pushes == pushes and env.store["lease"]["expires"] == original
        clock["now"] = NOW + timedelta(minutes=31)
        heartbeat()
        seen.append(env.store["lease"]["expires"])
        assert st.parse(seen[0]) == NOW + timedelta(minutes=31, hours=3) > st.parse(original)
        return good(task, model)

    env.run_worker = run  # type: ignore[method-assign]  # reason: call the heartbeat the way a worker loop does
    cfg = config([make_task("T001")], tmp_path, clock=lambda: clock["now"])
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING and seen


def test_lost_final_push_is_reported(tmp_path: Path) -> None:
    env = FakeEnv()
    real = env.push_state

    def rival_moves_state(new: st.State, base: st.State) -> bool:
        if new["tasks"][0]["status"] == st.MERGE_PENDING:
            return False
        return real(new, base)

    env.push_state = rival_moves_state  # type: ignore[method-assign]  # reason: simulate a rejected last push
    assert cycle.run_cycle(config([make_task("T001")], tmp_path), env, NOW) == "lost_race"


def test_green_but_unmerged_stays_merge_pending_and_blocks_new_claims(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001"), make_task("T002")], tmp_path)
    cycle.run_cycle(cfg, env, NOW)
    env.prs["T001"] = green_pr(merged=False)
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=5)) == "merge_pending"
    assert st.status(env.store, "T001") == st.MERGE_PENDING and st.status(env.store, "T002") == st.QUEUED
    assert [r[0] for r in env.runs] == ["T001"]
    env.prs["T001"]["merged"] = True
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == st.MERGE_PENDING  # T001 done, T002 claimed
    assert st.status(env.store, "T001") == st.DONE and [r[0] for r in env.runs] == ["T001", "T002"]


def test_crash_after_merge_before_merge_pending_is_adopted_not_redone(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)

    def crashing(task: dict[str, Any], model: str, log: str) -> cycle.WorkerResult:
        env.prs["T001"] = green_pr()
        raise Crash

    env.worker = crashing
    with pytest.raises(Crash):
        cycle.run_cycle(cfg, env, NOW)
    later = NOW + timedelta(hours=3, minutes=1)
    assert cycle.run_cycle(config([make_task("T001")], tmp_path, "s2"), env, later) == "done"
    assert st.status(env.store, "T001") == st.DONE and len(env.runs) == 1


# ---- inbox answers survive an early exit


def test_inbox_is_recorded_and_acked_even_when_the_lease_is_live(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path, "s2")
    env.store = st.claim(env.store, "T001", "s1", NOW)
    env.inbox = [("6-1", "T127", "hello")]
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=5)) == "lease_live"
    assert env.store["inbox_processed"] == ["6-1"] and env.store["owner_answers"][0]["note"] == "hello"
    assert env.acked == ["6-1"] and env.inbox == []


def test_inbox_is_not_acked_when_the_push_is_rejected(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    env.inbox = [("6-1", "T127", "hello")]
    env.push_state = lambda new, base: False  # type: ignore[method-assign]  # reason: simulate a lost race
    assert cycle.run_cycle(cfg, env, NOW) == "lost_race"
    assert env.acked == [] and env.inbox == [("6-1", "T127", "hello")]


def test_gate_answer_is_not_lost_when_a_worker_is_running(tmp_path: Path) -> None:
    graph = [make_task("T001"), gate_task()]
    env, cfg = FakeEnv(), config(graph, tmp_path, "s2")
    env.store = st.with_task(env.store, "T127", status=st.WAITING_FOR_OWNER, type="gate", deps=["T001"])
    env.store = st.claim(env.store, "T001", "s1", NOW)
    env.inbox = [("7-1", "T127", "accounts done")]
    assert cycle.run_cycle(cfg, env, NOW) == "lease_live"
    assert st.status(env.store, "T127") == st.DONE and env.acked == ["7-1"]


# ---- account checkpoint and Gate P


def gate_task() -> dict[str, Any]:
    return {
        "id": "T127",
        "deps": ["T001"],
        "type": "gate",
        "owns": [],
        "gate": {
            "kind": "account-checkpoint",
            "waits_on_owner": True,
            "outbox": "Your turn: set up accounts",
            "resume": "Casey comments 'accounts done' in the issue",
        },
    }


def test_account_checkpoint_writes_your_turn_and_stops(tmp_path: Path) -> None:
    graph = [make_task("T001"), gate_task(), make_task("T130", ["T127"])]
    env, cfg = FakeEnv(), config(graph, tmp_path)
    env.store = st.with_task(env.store, "T001", status=st.DONE)
    assert cycle.run_cycle(cfg, env, NOW) == "gate_open"
    assert "Your turn: set up accounts" in (tmp_path / "T127.md").read_text(encoding="utf-8")
    assert st.status(env.store, "T127") == st.WAITING_FOR_OWNER
    assert cycle.run_cycle(cfg, env, NOW + timedelta(hours=1)) == "idle"
    assert env.runs == []
    env.inbox = [("f1", "T127", "not yet")]
    assert cycle.run_cycle(cfg, env, NOW + timedelta(hours=2)) == "idle"
    env.inbox = [("f2", "T127", "accounts done")]
    assert cycle.run_cycle(cfg, env, NOW + timedelta(hours=3)) == st.MERGE_PENDING
    assert [r[0] for r in env.runs] == ["T130"]


def test_gate_p_holds_the_build(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    env.store["build_hold"] = "Gate P"
    assert cycle.run_cycle(cfg, env, NOW) == "idle" and env.runs == []
    env.inbox = [("f3", "GATE-P", "approved to build")]
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING


def test_attempt_model_policy() -> None:
    task = make_task("T001")
    assert [cycle.attempt_model(task, n, MODELS) for n in range(4)] == [MODELS["sonnet"]] * 2 + [MODELS["opus"]] * 2


# ---- Gate P (BUILD_BRIEF.md section 16) on the real state shape


def real_state_after_k8() -> st.State:
    import json

    path = Path(__file__).resolve().parent / "fixtures" / "kit_state_real_shape.json"
    state: st.State = json.loads(path.read_text(encoding="utf-8"))
    state = st.release_expired(state, datetime(2026, 10, 27, 21, 0, tzinfo=timezone.utc))
    return st.with_task(state, "K8", status=st.DONE)


def test_gate_p_opens_when_k9_is_done_and_stops_the_build(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    env.store = st.with_task(real_state_after_k8(), "K9", status=st.DONE)
    assert cycle.run_cycle(cfg, env, NOW) == "gate_open"
    assert "title: Gate P: your presentation is ready" in (tmp_path / "GATE-P.md").read_text(encoding="utf-8")
    assert st.status(env.store, "GATE-P") == st.WAITING_FOR_OWNER and env.store["build_hold"]
    assert cycle.run_cycle(cfg, env, NOW + timedelta(hours=1)) == "idle" and env.runs == []
    env.inbox = [("p1", "GATE-P", "looks nice")]
    assert cycle.run_cycle(cfg, env, NOW + timedelta(hours=2)) == "idle" and env.runs == []
    env.inbox = [("p2", "GATE-P", "approved to build")]
    assert cycle.run_cycle(cfg, env, NOW + timedelta(hours=3)) == st.MERGE_PENDING
    assert [r[0] for r in env.runs] == ["T001"] and env.store["build_hold"] is None


def test_gate_p_waits_for_k9_and_kit_work_continues_under_the_hold(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    env.store = real_state_after_k8()
    env.worker = lambda t, m, log: cycle.WorkerResult("submitted", [], [m], reviewer_evidence=[MODELS["opus"]])
    assert env.store["build_hold"]
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING
    assert [r[0] for r in env.runs] == ["K9"]
    assert st.status(env.store, "GATE-P") == st.QUEUED and not (tmp_path / "GATE-P.md").exists()


def test_a_set_build_hold_is_the_gate_even_without_gate_p_opening(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    env.store["build_hold"] = "Gate P"
    assert cycle.run_cycle(cfg, env, NOW) == "idle" and env.runs == []


# ---- D-486 drills: lease_live from code, red main, the review gate, stage gating


def reviewed(task: dict[str, Any], model: str, paths: list[str] | None = None, head: str = "head0001") -> cycle.WorkerResult:
    files = paths if paths is not None else [f"app/{task['id'].lower()}/x.py"]
    return cycle.WorkerResult("submitted", files, [model], branch=f"claude/task/{task['id']}-x", head_sha=head,
                              reviewer_evidence=[MODELS["opus"]])


def test_drill_session_holding_a_live_lease_makes_the_next_exit_lease_live(tmp_path: Path) -> None:
    """Acceptance (D-486): the first session is handed the task and holds the lease; a second exits from code."""
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("handed_off"))
    graph = [make_task("T001"), make_task("T002")]
    assert cycle.run_cycle(config(graph, tmp_path, "rehearsal-1"), env, NOW) == "handed_off"
    assert env.store["lease"]["session"] == "rehearsal-1" and st.status(env.store, "T001") == st.IN_PROGRESS
    assert cycle.run_cycle(config(graph, tmp_path, "rehearsal-2"), env, NOW + timedelta(minutes=10)) == "lease_live"
    assert [r[0] for r in env.runs] == ["T001"]


def fix_main_worker(task: dict[str, Any], model: str, log: str) -> cycle.WorkerResult:
    if task["id"] == st.FIX_MAIN:
        return reviewed(task, model, ["app/t001/x.py"], head="revert01")
    return reviewed(task, model)


def go_red(tmp_path: Path) -> tuple[FakeEnv, cycle.Config]:
    graph = [make_task("T001"), make_task("T002"), make_task("T003", ["T001"])]
    env, cfg = FakeEnv(fix_main_worker), config(graph, tmp_path)
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING
    env.prs["T001"] = dict(green_pr(), merge_tree="other", merged_sha="bad0001", files=["app/t001/x.py"])
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=5)) == "not_done"
    return env, cfg


def test_drill_red_main_reverts_clears_and_continues(tmp_path: Path) -> None:
    """Governance section 5: red -> only FIX-MAIN -> revert PR -> verified green -> cleared -> back in queue."""
    env, cfg = go_red(tmp_path)
    red = env.store["main_red"]
    assert red["check"] == "tree" and red["sha"] == "bad0001" and red["task"] == "T001"
    fix = st.entry(env.store, st.FIX_MAIN)
    assert fix["type"] == "harness" and fix["model"] == MODELS["sonnet"] and fix["reviewer_model"] == MODELS["opus"]
    assert fix["owns"] == ["app/t001/x.py"] and "git revert --no-edit bad0001" in fix["goal"]
    assert st.eligible(env.store, NOW) == [st.FIX_MAIN]
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == "fix_main"
    assert env.runs[-1][:2] == (st.FIX_MAIN, MODELS["sonnet"]) and st.status(env.store, st.FIX_MAIN) == st.MERGE_PENDING
    assert st.status(env.store, "T002") == st.QUEUED  # nothing else ran while main was red
    env.prs[st.FIX_MAIN] = green_pr(sha="revert01", tree="t-revert")
    env.head = "main0002"
    env.nightly = {"conclusion": "failure", "sha": "main0001"}  # an old red nightly does not hold the new head
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=20)) == "main_cleared"
    assert not env.store["main_red"] and env.store["main_red_history"][-1]["cleared_on"] == "main0002"
    assert env.store["smoke_runs"][-1] == {"sha": "main0002", "ok": True, "at": st.iso(NOW + timedelta(minutes=20)), "why": "verify FIX-MAIN"}
    assert st.status(env.store, "T001") == st.QUEUED and st.entry(env.store, "T001")["attempts"] == 1
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=30)) == st.MERGE_PENDING
    assert env.runs[-1][0] == "T001"


def test_drill_red_main_stays_red_on_a_red_smoke_run_or_a_red_nightly_on_head(tmp_path: Path) -> None:
    env, cfg = go_red(tmp_path)
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10))
    env.prs[st.FIX_MAIN] = green_pr(sha="revert01", tree="t-revert")
    env.smoke_green = False
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=20)) == "main_red"
    assert env.store["main_red"] and st.status(env.store, st.FIX_MAIN) == st.QUEUED
    assert st.entry(env.store, st.FIX_MAIN)["attempts"] == 1 and env.store["smoke_runs"][-1]["ok"] is False
    assert st.status(env.store, "T001") == st.BLOCKED  # not back in the queue until main is green
    env.store = st.with_task(env.store, st.FIX_MAIN, status=st.DONE)
    env.smoke_green, env.nightly = True, {"conclusion": "failure", "sha": env.head}
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=30)) == "main_red" and env.store["main_red"]


def test_drill_a_red_nightly_on_head_turns_main_red_and_blames_the_last_merge(tmp_path: Path) -> None:
    graph = [make_task("T001"), make_task("T002")]
    env, cfg = FakeEnv(fix_main_worker), config(graph, tmp_path)
    env.store = st.with_task(env.store, "T001", status=st.DONE, merged_sha="m1", merged_at="2026-11-01T10:00:00Z", files=["app/t001/x.py"])
    env.nightly = {"conclusion": "failure", "sha": env.head}
    assert cycle.run_cycle(cfg, env, NOW) == "main_red"
    assert env.store["main_red"]["check"] == "nightly" and env.store["main_red"]["revert_sha"] == "m1"
    assert st.eligible(env.store, NOW) == [st.FIX_MAIN] and env.runs == []


def test_drill_a_red_smoke_run_before_a_claim_turns_main_red(tmp_path: Path) -> None:
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    env.smoke_green = False
    assert cycle.run_cycle(cfg, env, NOW) == "main_red"
    assert env.store["main_red"]["check"] == "smoke" and env.runs == []


def test_drill_push_without_a_review_record_gets_no_auto_merge(tmp_path: Path) -> None:
    """Acceptance (D-486): auto-pr's decision (runner/verify.py) refuses any head the runner did not review."""
    env, cfg = FakeEnv(lambda t, m, log: reviewed(t, m, head="good0001")), config([make_task("T001")], tmp_path)
    seen: dict[str, Any] = {}

    def push_branch(branch: str, head: str) -> bool:
        seen["at_push"] = copy.deepcopy(env.store)  # what auto-pr would read when the push lands
        return True

    env.push_branch = push_branch  # type: ignore[method-assign]  # reason: observe the order of the two pushes
    claimed = st.claim(st.sync_graph(env.store, cfg.graph), "T001", "s1", NOW)
    assert not verify.auto_merge_allowed(claimed, "claude/task/T001-x", "good0001")  # pushed by hand: no record
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING
    at_push = seen["at_push"]
    assert verify.auto_merge_allowed(at_push, "claude/task/T001-x", "good0001", at_push["lease"])
    assert not verify.auto_merge_allowed(at_push, "claude/task/T001-x", "other0002", at_push["lease"])
    assert not verify.auto_merge_allowed(at_push, "claude/task/T001-x", "good0001", {"task": "T002"})
    assert not verify.auto_merge_allowed(at_push, "claude/task/T002-x", "good0001")
    assert not verify.auto_merge_allowed(at_push, "feature/T001", "good0001")
    assert verify.auto_merge_allowed(env.store, "claude/task/T001-x", "good0001")  # MERGE_PENDING, lease released
    done = st.with_task(env.store, "T001", status=st.DONE)
    assert "not in flight" in verify.auto_merge_reason(done, "claude/task/T001-x", "good0001")
    forged = st.with_task(env.store, "T001", review_lease=None)
    assert "not written under its lease" in verify.auto_merge_reason(forged, "claude/task/T001-x", "good0001")


def test_drill_missing_reviewer_evidence_fails_the_attempt(tmp_path: Path) -> None:
    task = make_task("T001", reviewer_model=MODELS["opus"])
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("submitted", ["app/t001/x.py"], [m], head_sha="h"))
    assert cycle.run_cycle(config([task], tmp_path), env, NOW) == st.QUEUED
    assert "review: no model evidence" in st.entry(env.store, "T001")["last_failure"]
    assert env.branch_pushes == [] and "reviewed_sha" not in st.entry(env.store, "T001")


def test_per_attempt_history_and_usage_are_recorded(tmp_path: Path) -> None:
    def worker(task: dict[str, Any], model: str, log: str) -> cycle.WorkerResult:
        res = reviewed(task, model)
        res.usage = {"input_tokens": 7, "output_tokens": 3}
        return res

    env = FakeEnv(worker)
    assert cycle.run_cycle(config([make_task("T001")], tmp_path), env, NOW) == st.MERGE_PENDING
    entry = st.entry(env.store, "T001")
    hist = entry["history"][-1]
    assert hist["attempt"] == 1 and hist["model"] == MODELS["sonnet"] and hist["path_check_ok"] is True
    assert hist["started_at"] == st.iso(NOW) and hist["result"] == st.MERGE_PENDING and hist["head_sha"] == "head0001"
    assert env.store["usage"]["by_task"]["T001"]["output_tokens"] == 3
    env.prs["T001"] = green_pr(sha="head0001")
    env.prs["T001"]["number"] = 12
    cycle.run_cycle(config([make_task("T001")], tmp_path), env, NOW + timedelta(minutes=5))
    closed = st.entry(env.store, "T001")["history"][-1]
    assert closed["pr"] == 12 and closed["result"] == "done" and len(closed["checks"]) == 3


def stage_graph() -> list[dict[str, Any]]:
    return [
        make_task("T010", stage="2"), make_task("T011", stage="2"), make_task("T012", ["T010"], stage="2"),
        make_task("T057", ["T010", "T011", "T012"], stage="2", type="cleanup"),
        make_task("T058", ["T057"], stage="3"), make_task("T059", stage="3"),
    ]


def stage_gate(env: FakeEnv, graph: list[dict[str, Any]]) -> Callable[[st.State], set[str]]:
    import stage_exit

    return lambda state: stage_exit.passed_stages(graph, state, env.nightly, {}, lambda older, newer: True)


def test_drill_stage_gating_by_the_previous_stage_exit_test(tmp_path: Path) -> None:
    """A BLOCKED mid-stage task holds only its dependents inside the stage; later-stage tasks wait for the
    stage exit test, not for a dependency chain, and become eligible once it passes (D-486)."""
    graph = stage_graph()
    env = FakeEnv()
    cfg = config(graph, tmp_path)
    cfg.stage_gate = stage_gate(env, graph)
    env.store = st.with_task(st.sync_graph(env.store, graph), "T010", status=st.BLOCKED, reason="3 attempts")
    passed = cfg.stage_gate(env.store)
    assert st.eligible(env.store, NOW, passed) == ["T011"]  # T012 waits on T010; T059 waits on stage 2's exit
    for tid in ("T010", "T011", "T012", "T057"):
        env.store = st.with_task(env.store, tid, status=st.DONE, merged_sha=f"m-{tid}")
    assert st.eligible(env.store, NOW, cfg.stage_gate(env.store)) == []  # no nightly, no proof: still closed
    env.nightly = {"conclusion": "success", "sha": "n1"}
    assert st.eligible(env.store, NOW, cfg.stage_gate(env.store)) == []  # proof missing
    env.store["stage_proofs"] = {"2": {"evidence": "reduced J1 green in Ci run 77"}}
    assert st.eligible(env.store, NOW, cfg.stage_gate(env.store)) == ["T058", "T059"]
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING and env.runs[-1][0] == "T058"


def test_previous_stage_skips_gate_only_stages() -> None:
    state = st.sync_graph(st.new_state(), [make_task("A", stage="5"), make_task("B", stage="0.5"), make_task("C", stage="6"),
                                           make_task("D", stage="checkpoint"), make_task("E", stage="1")])
    assert st.previous_stage(state, "6") == "5" and st.previous_stage(state, "1") is None
    assert st.previous_stage(state, "checkpoint") is None


# ---- D-486 additions: the runner turns on auto-merge, cleanup proofs, pacing


def test_drill_runner_enables_auto_merge_only_for_the_reviewed_head(tmp_path: Path) -> None:
    """Auto-merge comes from the runner after its review (its own identity, so pull_request.closed fires)."""
    env = FakeEnv(lambda t, m, log: reviewed(t, m, head="good0001"))
    cfg = config([make_task("T001"), make_task("T002")], tmp_path)
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING
    assert env.auto_merges == [("claude/task/T001-x", "good0001")]
    assert st.entry(env.store, "T001")["auto_merge"]["enabled"] is True
    env.prs["T001"] = dict(green_pr(sha="hand0002", merged=False), auto_merge=False)  # someone pushed by hand
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=5)) == "merge_pending"
    assert env.auto_merges == [("claude/task/T001-x", "good0001")]  # not re-enabled for the unreviewed head
    assert "no runner review record for hand0002" in st.entry(env.store, "T001")["auto_merge"]["why"]
    env.prs["T001"] = dict(green_pr(sha="good0001", merged=False), auto_merge=False)  # an earlier enable failed
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10))
    assert env.auto_merges[-1] == ("claude/task/T001-x", "good0001")


def test_drill_cleanup_needs_its_stage_proof_to_be_done(tmp_path: Path) -> None:
    graph = [make_task("T057", stage="2", type="cleanup")]
    env, cfg = FakeEnv(lambda t, m, log: reviewed(t, m, head="c1")), config(graph, tmp_path)
    cycle.run_cycle(cfg, env, NOW)
    env.prs["T057"] = dict(green_pr(sha="c1"), merged_sha="m57")
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=5))
    entry = st.entry(env.store, "T057")
    assert entry["status"] == st.BLOCKED and entry["reason"] == "stage proof missing" and entry["merged_sha"] == "m57"
    env.store["stage_proofs"] = {"2": {"evidence": "J1 steps 1-6 green in Ci run 9"}}
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10))
    assert st.status(env.store, "T057") == st.DONE
    assert env.main_merges == 3  # main is merged into claude/kit-state every cycle


def test_drill_pacing_claims_the_next_task_that_fits_and_waits_only_when_none_does(tmp_path: Path) -> None:
    sizes = {"T001": "wait", "T002": "claim"}

    def pacer(state: st.State, task: dict[str, Any], now: datetime) -> tuple[str, str | None, st.State]:
        verdict = sizes.get(task["id"], "claim")
        until = st.iso(now + timedelta(hours=2)) if verdict == "wait" else None
        return verdict, until, dict(state, pacing={"last": f"{task['id']} {verdict}", "waiting_until": until})

    graph = [make_task("T001"), make_task("T002")]
    env, cfg = FakeEnv(lambda t, m, log: reviewed(t, m)), config(graph, tmp_path)
    cfg.pacer = pacer
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING and env.runs[-1][0] == "T002"
    assert env.store["pacing"]["last"] == "T002 claim"
    env2 = FakeEnv(lambda t, m, log: reviewed(t, m))
    sizes["T002"] = "wait"
    cfg2 = config(graph, tmp_path)
    cfg2.pacer = pacer
    assert cycle.run_cycle(cfg2, env2, NOW) == "paced" and env2.runs == []
    assert env2.store["pacing"]["waiting_until"] == st.iso(NOW + timedelta(hours=2))
    sizes.update(T001="defer", T002="wait")
    assert cycle.run_cycle(cfg2, env2, NOW + timedelta(minutes=1)) == st.MERGE_PENDING and env2.runs[-1][0] == "T001"


def test_drill_usage_limit_records_the_reset_it_reports(tmp_path: Path) -> None:
    reset = NOW + timedelta(hours=4)
    env = FakeEnv(lambda t, m, log: cycle.WorkerResult("usage_limit", reset_at=reset))
    assert cycle.run_cycle(config([make_task("T001")], tmp_path), env, NOW) == st.RETRY_AFTER_RESET
    assert env.store["usage"]["limit_hit"] == {"task": "T001", "at": st.iso(NOW), "reset_at": st.iso(reset)}


def test_usage_feeds_the_pacing_windows(tmp_path: Path) -> None:
    import pacing

    def worker(task: dict[str, Any], model: str, log: str) -> cycle.WorkerResult:
        res = reviewed(task, model)
        res.usage = {"output_tokens": 1000, "cache_read_input_tokens": 10_000}
        return res

    env, cfg = FakeEnv(worker), config([make_task("T001")], tmp_path)
    cfg.on_usage = pacing.usage_recorder()
    cycle.run_cycle(cfg, env, NOW)
    assert env.store["usage"]["windows"]["five_hour"] == {"start": st.iso(NOW), "tokens": 2000}


def test_drill_a_red_smoke_run_with_green_checks_on_the_head_blames_nobody(tmp_path: Path) -> None:
    """D-498: Ci, Twin and Ally green on the commit the smoke failed on means the runner's own
    environment is at fault; nothing is claimed and main is not marked red."""
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    env.smoke_green, env.checks_green = False, True
    assert cycle.run_cycle(cfg, env, NOW) == "smoke_env"
    assert env.store.get("main_red") is None and env.runs == []
    assert env.store["smoke_env_problems"][-1]["sha"] == env.head
    assert not st.entry(env.store, st.FIX_MAIN)


def test_drill_a_false_smoke_red_is_withdrawn_before_fix_main_runs(tmp_path: Path) -> None:
    """D-498: a main_red that a smoke run set is withdrawn, and FIX-MAIN skipped, once the head's checks
    read green; the blamed merge is not reverted and keeps its DONE record."""
    env, cfg = FakeEnv(), config([make_task("T001")], tmp_path)
    env.smoke_green = False
    assert cycle.run_cycle(cfg, env, NOW) == "main_red"
    assert st.status(env.store, st.FIX_MAIN) == st.QUEUED
    env.smoke_green, env.checks_green = True, True
    assert cycle.run_cycle(cfg, env, NOW) == "false_red_cleared"
    assert env.store.get("main_red") is None and env.store["main_red_history"][-1]["false_red"] is True
    assert st.status(env.store, st.FIX_MAIN) == st.SKIPPED and env.runs == []
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING and env.runs[0][0] == "T001"
