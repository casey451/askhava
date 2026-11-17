"""D-4180: when `main` moves from A to a later head while this job's own smoke of A runs, and A would be the
D-3058/D-3156 ancestry-reuse base for the head once green, the smoke is kept, its verdict is recorded as a real run
on A and the head claims through the unchanged ancestry reuse. When A does not qualify, the smoke is stopped and the
head smoked as before. Fake processes, a fake chain and fake data only; no real cargo, network or state branch."""

from __future__ import annotations

import json
from datetime import timedelta
from pathlib import Path
from typing import Any

import pytest

from runner import cycle, github_env as ge, state as st
from test_failure_drills import NOW, config, make_task
from test_runner_pre_smoke import _env, job  # noqa: F401  # reason: `job` is a fixture used by name
from test_runner_smoke_reuse import CHAIN, AncestryEnv, _merged, _run, _state

AT = st.iso(NOW - timedelta(minutes=2))  # when the kept smoke's result was written
FLAGS = ("smoke_timed_out", "smoke_source", "smoke_seconds", "smoke_pending", "smoke_at")


class KeepEnv(AncestryEnv):
    """AncestryEnv on CHAIN[0] whose smoke and own-smoke readers are the real GitHubEnv's, flags carried over."""

    def __init__(self, real: Any) -> None:
        super().__init__()
        self.real, self.proven = real, set()
        real.main_head = lambda: self.head

    def _carry(self, out: Any) -> Any:
        for name in FLAGS:
            setattr(self, name, getattr(self.real, name))
        return out

    def smoke_main(self) -> tuple[bool, str]:
        self.smokes.append(self.head)
        return self._carry(self.real.smoke_main())

    def own_smoke_for(self, head: str) -> bool:
        return bool(self.real.own_smoke_for(head))

    def settle_background_smoke(self, head: str) -> None:
        self.real.settle_background_smoke(head)

    def own_smoke_state(self, head: str) -> dict[str, Any] | None:
        return self.real.own_smoke_state(head)  # type: ignore[no-any-return]  # reason: the real env's reader

    def collect_own_smoke(self, own: dict[str, Any]) -> tuple[bool, str]:
        return self._carry(self.real.collect_own_smoke(own))  # type: ignore[no-any-return]  # reason: the real env's

    def green_twin_tree(self, sha: str) -> bool:
        return sha in self.proven


def _result(sha: str, code: int = 0, **extra: Any) -> dict[str, Any]:
    return {"sha": sha, "ok": code == 0, "timed_out": False, "exit": code, "seconds": 611, "at": AT, "job": "77-1",
            "last": "smoke: all green" if code == 0 else "smoke: FAILED", "source": ge.BACKGROUND, **extra}


def _keep(tmp_path: Path, job: Path, a: str = CHAIN[1], alive: bool = True) -> tuple[KeepEnv, Any, dict[str, Any]]:
    """This job's background smoke of `a` (pid 4242, running while `alive`); a new smoke gets pid 4343 and runs."""
    real, _ = _env(tmp_path, smoke=(1, "must not run in the call"))
    real.smoke_background, real.smoke_wait, real.sleep = True, 0, lambda s: None
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_BG).write_text(json.dumps({"sha": a, "job": "77-1", "pid": 4242}))
    seen: dict[str, Any] = {"alive": alive, "started": [], "stops": 0}
    real.bg_alive = lambda pid: pid == 4343 or seen["alive"]
    real.bg_start = lambda head, root: seen["started"].append(head) or 4343

    def stop() -> str:
        seen["stops"] += 1
        seen["alive"] = False
        return "stopped"

    real.bg_stop = stop
    env, cfg = KeepEnv(real), config([make_task("T001")], tmp_path)
    cfg.smoke_ancestry = 3
    env.store = dict(_state(), tasks=_merged(CHAIN[0]))
    return env, cfg, seen


def _finish(env: KeepEnv, seen: dict[str, Any], result: dict[str, Any] | None) -> None:
    if result is not None:
        ge.write_pre_smoke(result)
    seen["alive"] = False


def _stopped(env: KeepEnv, a: str) -> bool:
    return f"the background smoke of {a[:12]} stopped: main is now {CHAIN[0][:12]}" in env.real.log


# ---- kept, collected and recorded


def test_a_running_ancestor_smoke_is_kept_then_its_green_is_recorded_and_the_head_claims(job: Path, tmp_path: Path) -> None:
    env, cfg, seen = _keep(tmp_path, job)
    assert cycle.run_cycle(cfg, env, NOW) == cycle.SMOKE_PENDING and env.runs == []
    assert seen["started"] == [] and seen["stops"] == 0 and not _stopped(env, CHAIN[1])
    assert not env.store.get("smoke_runs"), "a running smoke is no verdict"
    _finish(env, seen, _result(CHAIN[1]))
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING and env.runs[0][0] == "T001"
    assert env.store["smoke_runs"][-2] == {"sha": CHAIN[1], "ok": True, "at": AT, "seconds": 611,
                                           "why": f"start routine (background, ancestor of {CHAIN[0][:12]})"}
    assert env.store["smoke_runs"][-1]["base"] == CHAIN[1] and env.store["smoke_runs"][-1]["reused"] == AT
    assert seen["started"] == [] and env.smokes == [], "no second smoke"


def test_an_in_between_commit_proven_only_by_its_twin_tree_keeps_it(job: Path, tmp_path: Path) -> None:
    env, cfg, seen = _keep(tmp_path, job, a=CHAIN[2])
    env.store, env.proven = dict(_state(), tasks=_merged(CHAIN[0])), {CHAIN[1]}
    _finish(env, seen, _result(CHAIN[2]))
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING and env.runs[0][0] == "T001"
    assert env.store["smoke_runs"][-2]["sha"] == CHAIN[2] and env.store["smoke_runs"][-1]["base"] == CHAIN[2]


def test_a_red_on_the_ancestor_is_recorded_claims_nothing_and_smokes_the_head(job: Path, tmp_path: Path) -> None:
    env, cfg, seen = _keep(tmp_path, job)
    _finish(env, seen, _result(CHAIN[1], code=101))
    assert cycle.run_cycle(cfg, env, NOW) == cycle.SMOKE_PENDING and env.runs == []
    assert env.store["smoke_runs"][-1] == {"sha": CHAIN[1], "ok": False, "at": AT, "seconds": 611,
                                           "why": f"start routine (background, ancestor of {CHAIN[0][:12]})"}
    assert not env.store.get("main_red") and not env.store.get("smoke_env_problems")
    assert seen["started"] == [CHAIN[0]] and env.smokes == [CHAIN[0]], "the head is smoked in the same call"


@pytest.mark.parametrize("how", ["lost", "timed_out"])
def test_a_timed_out_ancestor_is_a_timed_out_run_on_it_and_a_smoke_timeout(job: Path, tmp_path: Path, how: str) -> None:
    env, cfg, seen = _keep(tmp_path, job)
    _finish(env, seen, None if how == "lost" else _result(CHAIN[1], code=124, timed_out=True))
    assert cycle.run_cycle(cfg, env, NOW) == "smoke_timeout" and env.runs == []
    run = env.store["smoke_runs"][-1]
    assert run["sha"] == CHAIN[1] and run["ok"] is False and run["timed_out"] is True
    assert run["at"] == (st.iso(NOW) if how == "lost" else AT) and seen["started"] == []
    assert not env.store.get("smoke_env_problems") and not env.store.get("main_red")


def test_a_finished_pre_session_result_for_a_qualifying_ancestor_is_recorded_and_reused(job: Path, tmp_path: Path) -> None:
    env, cfg, seen = _keep(tmp_path, job)
    (job / ge.SMOKE_BG).unlink()
    ge.write_pre_smoke({k: v for k, v in _result(CHAIN[1]).items() if k != "source"})
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING and env.runs[0][0] == "T001"
    assert env.store["smoke_runs"][-2]["why"] == f"start routine (pre-session, ancestor of {CHAIN[0][:12]})"
    assert env.store["smoke_runs"][-1]["base"] == CHAIN[1] and seen["started"] == []


def test_a_result_already_recorded_is_not_recorded_twice_and_keeps_its_own_time() -> None:
    class Done:
        smoke_at, smoke_source, smoke_seconds, smoke_timed_out = AT, ge.BACKGROUND, 611, False

    once = cycle._record_ancestor_run(_state(), Done(), NOW, CHAIN[0], CHAIN[1], True)
    assert once["smoke_runs"][-1]["at"] == AT
    assert cycle._record_ancestor_run(once, Done(), NOW, CHAIN[0], CHAIN[1], True) == once
    assert len(cycle._record_ancestor_run(once, Done(), NOW, CHAIN[0], CHAIN[1], False)["smoke_runs"]) == 2
    Done.smoke_at = "not a time"
    assert cycle._record_ancestor_run(_state(), Done(), NOW, CHAIN[0], CHAIN[1], True)["smoke_runs"][-1]["at"] == st.iso(NOW)


# ---- not qualifying: stopped and the head smoked, as before


@pytest.mark.parametrize("case", ["gap", "past_limit", "off", "unreadable", "main_red", "doubted"])
def test_an_ancestor_that_does_not_qualify_is_stopped_and_the_head_smoked(job: Path, tmp_path: Path, case: str) -> None:
    env, _, seen = _keep(tmp_path, job, a=CHAIN[4] if case == "past_limit" else CHAIN[2])
    state, limit = dict(_state(), tasks=_merged(CHAIN[0], CHAIN[1])), 3
    if case == "gap":
        state["tasks"] = _merged(CHAIN[0])
    elif case == "off":
        limit = 0
    elif case == "unreadable":
        env.chain = []
    elif case == "main_red":
        state["main_red"] = {"check": "nightly", "sha": CHAIN[0], "task": "T900", "revert_sha": "", "files": [], "set_at": st.iso(NOW)}
    elif case == "doubted":
        env.checks_by_sha[CHAIN[2]] = False
    out, ok, head = cycle._start_smoke(state, env, NOW, limit)
    assert seen["stops"] == 1 and _stopped(env, CHAIN[4] if case == "past_limit" else CHAIN[2])
    assert seen["started"] == [CHAIN[0]] and env.smokes == [CHAIN[0]] and not ok and env.smoke_pending
    assert out == state, "nothing recorded on the ancestor"


def test_a_same_head_green_from_another_lane_still_stops_it(job: Path, tmp_path: Path) -> None:
    env, _, seen = _keep(tmp_path, job)
    since = st.iso(NOW - timedelta(minutes=5))
    out, ok, _ = cycle._start_smoke(dict(_state(_run(CHAIN[0], at=since)), tasks=_merged(CHAIN[0])), env, NOW, 3)
    assert ok is True and out["smoke_runs"][-1]["reused"] == since
    assert seen["stops"] == 1 and _stopped(env, CHAIN[1]) and seen["started"] == []


def test_an_env_without_the_readers_changes_nothing() -> None:
    env = AncestryEnv()
    state = dict(_state(), tasks=_merged(CHAIN[0]))
    _, ok, _ = cycle._start_smoke(state, env, NOW, 3)
    assert ok is True and env.smokes == [CHAIN[0]]


# ---- the rule (state.ancestry_pending)


def _pending(state: st.State, sha: str = CHAIN[2], limit: int = 3, proven: Any = None) -> bool:
    return st.ancestry_pending(state, CHAIN[: limit + 1], sha, st.done_merges(state), limit, proven)


def test_ancestry_pending_holds_for_a_clean_ancestor_behind_done_merges() -> None:
    state = dict(_state(), tasks=_merged(CHAIN[0], CHAIN[1]))
    assert _pending(state) and _pending(state, CHAIN[1]) and not _pending(state, CHAIN[3]), "CHAIN[2] is no DONE merge"
    assert _pending(dict(state, tasks=_merged(*CHAIN[:3])), CHAIN[3]), "the limit itself is in reach"
    assert not _pending(state, CHAIN[0]), "the head itself is never its own ancestor"
    assert not _pending(state, CHAIN[4]) and not _pending(state, "f" * 40), "past the limit or off the chain"
    assert not _pending(state, limit=0) and not _pending(state, "")
    red = {"check": "smoke", "sha": CHAIN[0], "task": "", "revert_sha": "", "files": [], "set_at": st.iso(NOW)}
    assert not _pending(dict(state, main_red=red))


@pytest.mark.parametrize("kind", ["red_run", "timed_out_run", "strike", "main_red_history", "real_green"])
@pytest.mark.parametrize("where", [0, 1, 2])
def test_ancestry_pending_refuses_any_doubt_up_to_the_ancestor(kind: str, where: int) -> None:
    state = dict(_state(), tasks=_merged(CHAIN[0], CHAIN[1]))
    sha = CHAIN[where]
    if kind in ("red_run", "timed_out_run"):
        state["smoke_runs"].append(_run(sha, ok=False, timed_out=kind == "timed_out_run"))
    elif kind == "strike":
        state["smoke_env_problems"] = [{"sha": sha, "at": st.iso(NOW), "why": "smoke red while the checks were green"}]
    elif kind == "main_red_history":
        state["main_red_history"] = [{"check": "smoke", "sha": sha, "task": "", "revert_sha": "", "files": [],
                                      "set_at": st.iso(NOW), "cleared_at": st.iso(NOW), "cleared_on": "x"}]
    else:
        state["smoke_runs"].append(_run(sha, at=NOW - timedelta(hours=7)))
    assert _pending(state) is (kind == "real_green" and where == 2), "a real green of its own does not doubt the ancestor"


def test_ancestry_pending_takes_a_proof_only_when_exactly_true() -> None:
    state = dict(_state(), tasks=_merged(CHAIN[0]))
    assert not _pending(state)
    assert _pending(state, proven=lambda sha: sha == CHAIN[1])
    assert not _pending(state, proven=lambda sha: "yes")
    asked: list[str] = []
    _pending(dict(state, tasks=[]), proven=lambda sha: asked.append(sha) or False)
    assert asked == [CHAIN[0]], "asked in order, stopping at the first refusal"
