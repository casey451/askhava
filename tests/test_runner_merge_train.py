"""The merge train, depth 2 (D-4281): while the turn holder A's head A' runs its checks, the runner merges the next
"line" task B's reviewed head with A' and pushes it as a new throwaway ref `claude/train/<B>-<A' first 12 hex>`;
stale train refs are deleted on every merge-line pass. The train reads and writes no state. Over the merge-line
fakes (test_runner_merge_line.LineEnv) and a GitHubEnv with a planted command runner. Fake data only.
"""

from __future__ import annotations

import copy
import inspect
import json
from datetime import timedelta
from pathlib import Path
from typing import Any

import pytest

import test_runner_merge_early as early
import test_runner_merge_line as line
from runner import cycle, state as st
from runner.github_env import GitHubEnv
from test_failure_drills import NOW, config, green_pr, make_task
from test_runner_merge_line import LineEnv, branch, head, open_pr, pending

A1 = "a1" * 20  # T001's update merge (A')
A0 = "a0" * 20  # an older head of T001
TRAIN = f"claude/train/T002-{A1[:12]}"


class TrainEnv(LineEnv):
    """LineEnv with the train's Env calls; `train_depth` is set per test (absent: an Env without the reader)."""

    def __init__(self) -> None:
        super().__init__()
        self.refs: list[dict[str, Any]] = []
        self.merges: list[tuple[str, tuple[str, str], tuple[str, str]]] = []
        self.merge_answer: dict[str, Any] = {"status": "pushed", "sha": "x" * 40}
        self.deleted: list[str] = []
        self.delete_ok = True

    def train_refs(self) -> list[dict[str, Any]]:
        return copy.deepcopy(self.refs)

    def train_merge(self, name: str, task: tuple[str, str], base: tuple[str, str]) -> dict[str, Any]:
        self.merges.append((name, task, base))
        return copy.deepcopy(self.merge_answer)

    def delete_train_ref(self, name: str) -> bool:
        self.deleted.append(name)
        return self.delete_ok


def ref(task: str, base: str, minutes_old: int = 10) -> dict[str, Any]:
    return {"name": f"claude/train/{task}-{base[:12]}", "task": task, "base": base[:12], "sha": "y" * 40,
            "at": st.iso(NOW + timedelta(minutes=30 - minutes_old))}


def two_open(tmp_path: Path, depth: Any = 2) -> tuple[TrainEnv, cycle.Config, st.State]:
    """T001 (handed in first) and T002 open, checks running, behind main; T001's update merge is A1."""
    env, cfg = TrainEnv(), config([make_task(t) for t in ("T001", "T002", "T003")], tmp_path)
    env.train_depth = lambda: depth  # type: ignore[method-assign]
    state = st.new_state()
    for tid, minutes, number in (("T001", 0, 1), ("T002", 10, 2)):
        state = pending(state, tid, minutes, str(number))
        env.prs[tid] = open_pr(tid, number)
    env.by_branch[branch("T001")] = {"status": "pushed", "sha": A1, "main": "main0002"}
    return env, cfg, state


def run(env: LineEnv, cfg: cycle.Config, state: st.State, minutes: int = 30) -> tuple[st.State, list[tuple[str, str, str]]]:
    return cycle._resolve_merges(state, env, cfg, NOW + timedelta(minutes=minutes))


def without_train(env: TrainEnv, cfg: cycle.Config, state: st.State) -> tuple[str, list[tuple[str, str, str]]]:
    """The pass as on main: a plain LineEnv with the same answers."""
    plain = LineEnv()
    plain.prs, plain.by_branch = copy.deepcopy(env.prs), copy.deepcopy(env.by_branch)
    out, outcomes = run(plain, cfg, copy.deepcopy(state))
    return json.dumps(out, sort_keys=True), outcomes


@pytest.mark.parametrize("depth", [1, None, "2", 2.5, True, False, {"depth": 2}, -3, "absent"])
def test_off_gives_no_train_and_the_same_state_while_stale_refs_are_still_deleted(tmp_path: Path, depth: Any) -> None:
    env, cfg, state = two_open(tmp_path, depth)
    if depth == "absent":
        del env.train_depth  # an Env without the reader
    env.refs = [ref("T009", A1)]
    out, outcomes = run(env, cfg, copy.deepcopy(state))
    assert env.merges == [] and cycle.train_depth(env) == 1
    assert (json.dumps(out, sort_keys=True), outcomes) == without_train(env, cfg, state)
    assert env.deleted == ["claude/train/T009-" + A1[:12]]


def test_depth_two_merges_bs_reviewed_head_with_a_prime_onto_the_train_ref_only(tmp_path: Path, capsys: Any) -> None:
    env, cfg, state = two_open(tmp_path)
    out, outcomes = run(env, cfg, copy.deepcopy(state))
    assert env.merges == [(TRAIN, (branch("T002"), head("T002")), (branch("T001"), A1))]
    assert (json.dumps(out, sort_keys=True), outcomes) == without_train(env, cfg, state)
    assert env.updates == [(branch("T001"), head("T001"))] and env.deleted == []
    assert f"pushed to {TRAIN}" in capsys.readouterr().out
    env.train_depth = lambda: 7  # type: ignore[method-assign]
    assert cycle.train_depth(env) == 2


def test_a_head_already_current_with_pending_checks_is_a_train_base(tmp_path: Path) -> None:
    env, cfg, state = two_open(tmp_path)
    state = st.with_task(state, "T001", reviewed_sha=A1)
    env.prs["T001"] = dict(open_pr("T001", 1), head_sha=A1)
    env.by_branch[branch("T001")] = {"status": "current"}
    run(env, cfg, state)
    assert [m[0] for m in env.merges] == [TRAIN]


def no_train_cases() -> list[Any]:
    def red(env: TrainEnv, s: st.State) -> st.State:
        return dict(s, main_red={"task": "T009", "head": "main0002"})

    def fix_main(env: TrainEnv, s: st.State) -> st.State:
        env.prs[st.FIX_MAIN] = open_pr(st.FIX_MAIN, 9)
        env.by_branch[branch(st.FIX_MAIN)] = {"status": "pushed", "sha": A1, "main": "main0002"}
        return dict(pending(s, st.FIX_MAIN, 30, "fix"), main_red={"task": "T009", "head": "main0002"})

    def b_late(env: TrainEnv, s: st.State) -> st.State:
        env.prs["T002"]["checks"] = [{"name": "Governance", "sha": head("T002"), "conclusion": "failure"}]
        return s

    def b_stuck(env: TrainEnv, s: st.State) -> st.State:
        return st.with_task(s, "T002", update_waiting_since=st.iso(NOW - timedelta(hours=1)))

    def b_out(env: TrainEnv, s: st.State) -> st.State:
        env.prs["T002"] = open_pr("T002", 2, checks="failed")
        return s

    def a_finished(env: TrainEnv, s: st.State) -> st.State:
        env.prs["T001"] = open_pr("T001", 1, checks="green")
        env.by_branch[branch("T001")] = {"status": "current"}
        return s

    def a_unreviewed(env: TrainEnv, s: st.State) -> st.State:
        env.prs["T001"] = dict(open_pr("T001", 1), head_sha=A0)  # not the reviewed head, not an update merge of it
        return s

    return [red, fix_main, b_late, b_stuck, b_out, a_finished, a_unreviewed]


@pytest.mark.parametrize("plant", no_train_cases(), ids=lambda f: f.__name__)
def test_no_train(tmp_path: Path, plant: Any) -> None:
    env, cfg, state = two_open(tmp_path)
    state = plant(env, state)
    out, outcomes = run(env, cfg, copy.deepcopy(state))
    assert env.merges == []
    assert (json.dumps(out, sort_keys=True), outcomes) == without_train(env, cfg, state)


@pytest.mark.parametrize("answer", [{"status": "conflict", "paths": ["app/t002/x.rs"]},
                                    {"status": "error", "why": "the train merge failed its own tree check"},
                                    {"status": "stale", "why": "main is not in it"}])
def test_a_failed_build_makes_no_ref_is_logged_and_leaves_b_as_it_was(tmp_path: Path, capsys: Any, answer: dict[str, Any]) -> None:
    env, cfg, state = two_open(tmp_path)
    env.merge_answer = answer
    env.refs = [ref("T002", "0" * 40)]  # an older train of T002 stays unless something replaced it
    env.store = copy.deepcopy(state)
    out, outcomes = run(env, cfg, copy.deepcopy(state))
    assert (json.dumps(out, sort_keys=True), outcomes) == without_train(env, cfg, state)
    assert f"no ref for T002 on T001's head {A1[:12]} ({answer['status']}" in capsys.readouterr().out
    assert env.deleted == ["claude/train/T002-000000000000"]  # its base is no MERGE_PENDING head


def test_a_listed_or_racing_duplicate_is_left_as_it_is(tmp_path: Path, capsys: Any) -> None:
    env, cfg, state = two_open(tmp_path)
    env.refs = [{**ref("T002", A1), "name": TRAIN}]
    run(env, cfg, copy.deepcopy(state))
    assert env.merges == [] and env.deleted == []
    env.refs, env.merge_answer = [], {"status": "exists", "why": "! [rejected] (non-fast-forward)"}
    run(env, cfg, copy.deepcopy(state))
    assert [m[0] for m in env.merges] == [TRAIN] and "already exists, left as it is" in capsys.readouterr().out


def stale_cases() -> list[Any]:
    old_base = A0  # a head T001 no longer has

    def b_done(s: st.State) -> tuple[st.State, dict[str, Any]]:
        return st.with_task(s, "T002", status=st.DONE), ref("T002", A1)

    def b_failed(s: st.State) -> tuple[st.State, dict[str, Any]]:
        return st.with_task(s, "T002", status=st.QUEUED), ref("T002", A1)

    def a_failed(s: st.State) -> tuple[st.State, dict[str, Any]]:
        return st.with_task(s, "T001", status=st.QUEUED), ref("T002", A1)

    def a_newer_head(s: st.State) -> tuple[st.State, dict[str, Any]]:
        return s, ref("T002", old_base)

    def past_six_hours(s: st.State) -> tuple[st.State, dict[str, Any]]:
        return s, ref("T002", A1, minutes_old=6 * 60 + 1)

    return [b_done, b_failed, a_failed, a_newer_head, past_six_hours]


@pytest.mark.parametrize("plant", stale_cases(), ids=lambda f: f.__name__)
def test_a_stale_ref_is_deleted_and_a_failed_delete_is_retried(tmp_path: Path, plant: Any) -> None:
    env, cfg, state = two_open(tmp_path, depth=1)
    state = st.with_task(state, "T001", reviewed_sha=A1)
    env.prs["T001"] = dict(open_pr("T001", 1), head_sha=A1)
    state, stale = plant(state)
    env.refs, env.delete_ok = [stale], False
    run(env, cfg, copy.deepcopy(state))
    run(env, cfg, copy.deepcopy(state))
    assert env.deleted == [stale["name"], stale["name"]]


def test_a_fresh_ref_of_a_pending_task_on_a_live_head_stays_and_a_new_one_replaces_the_older(tmp_path: Path) -> None:
    env, cfg, state = two_open(tmp_path)
    state = st.with_task(state, "T003", status=st.DONE, reviewed_sha=A0)
    env.refs = [ref("T002", A0)]  # T002 on a DONE task's head, 10 minutes old
    run(env, cfg, copy.deepcopy(state))
    assert env.deleted == [ref("T002", A0)["name"]] and [m[0] for m in env.merges] == [TRAIN]
    env.merges, env.deleted, env.merge_answer = [], [], {"status": "conflict", "paths": ["x"]}
    run(env, cfg, copy.deepcopy(state))
    assert env.deleted == []


def all_fixtures() -> list[Any]:
    cases = []
    for module in (line, early):
        for name, fn in inspect.getmembers(module, inspect.isfunction):
            if not name.startswith("test_") or fn.__module__ != module.__name__:
                continue
            marks = [m for m in getattr(fn, "pytestmark", []) if m.name == "parametrize"]
            values = [getattr(v, "values", (v,))[0] for v in marks[0].args[1]] if marks else [None]
            cases += [pytest.param(fn, marks[0].args[0] if marks else "", v, id=f"{module.__name__}.{name}-{i}") for i, v in enumerate(values)]
    return cases


@pytest.mark.parametrize(("fn", "arg", "value"), all_fixtures())
def test_every_merge_line_fixture_gives_the_same_state_and_outcomes_at_depth_1_and_2(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch, fn: Any, arg: str, value: Any) -> None:
    real = cycle._resolve_merges

    def twin(state: st.State, env: Any, cfg: cycle.Config, now: Any, open_only: bool = False) -> Any:
        expected = real(copy.deepcopy(state), _copy(env, None), cfg, now, open_only)
        for depth in (1, 2):
            got = real(copy.deepcopy(state), _copy(env, depth), cfg, now, open_only)
            assert (json.dumps(got[0], sort_keys=True), got[1]) == (json.dumps(expected[0], sort_keys=True), expected[1])
        return real(state, env, cfg, now, open_only)

    monkeypatch.setattr(cycle, "_resolve_merges", twin)
    params = inspect.signature(fn).parameters
    kwargs = {"tmp_path": tmp_path, "monkeypatch": monkeypatch, arg: value}
    fn(**{k: v for k, v in kwargs.items() if k in params})


def _copy(env: Any, depth: int | None) -> Any:
    """A copy of a fake Env; with `depth`, carrying the train's Env calls (one stale ref listed)."""
    if isinstance(env, cycle._EarlyEnv):
        out: Any = cycle._EarlyEnv(copy.deepcopy(env._env))
    else:
        out = copy.deepcopy(env)
    if depth is not None:
        out.train_depth = lambda: depth
        out.train_refs = lambda: [ref("T999", "f" * 40)]
        out.train_merge = lambda name, task, base: {"status": "pushed", "sha": "x" * 40}
        out.delete_train_ref = lambda name: True
    return out


def test_a_state_from_a_pass_with_a_train_passes_check_state(tmp_path: Path) -> None:
    import check_state  # scripts/, on the path through tests/conftest.py

    env, cfg, state = two_open(tmp_path)
    out, _ = run(env, cfg, state)
    assert env.merges and check_state.check(out) == check_state.check(without_state(env, cfg, state))


def without_state(env: TrainEnv, cfg: cycle.Config, state: st.State) -> st.State:
    return json.loads(without_train(env, cfg, state)[0])


# ---- GitHubEnv (planted command runner, no real git)


def github_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> tuple[GitHubEnv, list[list[str]]]:
    monkeypatch.delenv("RUNNER_PUSH_TOKEN", raising=False)
    calls: list[list[str]] = []

    def fake(args: list[str], stdin: str | None, cwd: Path, env: dict[str, str] | None = None, timeout: int = 600) -> tuple[int, str, str]:
        calls.append(args)
        return 0, "", ""

    return GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=fake), calls


class FakeMerge:
    def __init__(self, sha: str | None, check: tuple[bool, str]) -> None:
        self.sha, self.check, self.made = sha, check, []

    def commit(self, reviewed: str, main: str, branch: str) -> tuple[str | None, list[str]]:
        self.made.append((reviewed, main, branch))
        return self.sha, ([] if self.sha else ["app/t002/x.rs"])

    def is_update_merge(self, reviewed: str, head: str) -> tuple[bool, str]:
        return self.check


def planted(env: GitHubEnv, merge: FakeMerge) -> None:
    tips = {branch("T002"): "b" * 40, branch("T001"): A1}
    env.main_head = lambda: "c" * 40  # type: ignore[method-assign]
    env._remote_tip = lambda name: tips.get(name)  # type: ignore[method-assign]
    env._is_ancestor = lambda older, newer: older == "c" * 40  # type: ignore[method-assign]
    env.updates = merge  # type: ignore[assignment]


def test_train_merge_pushes_exactly_a_create_of_the_train_ref(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    env, calls = github_env(tmp_path, monkeypatch)
    merge = FakeMerge("d" * 40, (True, A1))
    planted(env, merge)
    assert env.train_merge(TRAIN, (branch("T002"), "b" * 40), (branch("T001"), A1)) == {"status": "pushed", "sha": "d" * 40}
    assert merge.made == [("b" * 40, A1, branch("T002"))]
    assert calls == [["git", "push", "--porcelain", "origin", f"{'d' * 40}:refs/heads/{TRAIN}"]]


@pytest.mark.parametrize(("merge", "status"), [(FakeMerge(None, (False, "")), "conflict"), (FakeMerge("d" * 40, (True, "e" * 40)), "error")])
def test_train_merge_pushes_nothing_on_a_conflict_or_a_failed_tree_check(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch, merge: FakeMerge, status: str) -> None:
    env, calls = github_env(tmp_path, monkeypatch)
    planted(env, merge)
    assert env.train_merge(TRAIN, (branch("T002"), "b" * 40), (branch("T001"), A1))["status"] == status and calls == []
    assert env.train_merge("claude/task/T002-x", (branch("T002"), "b" * 40), (branch("T001"), A1))["status"] == "error"


@pytest.mark.parametrize("name", ["claude/task/T1-x", "main", "claude/kit-state", "", "claude/train/../task/T1-x",
                                  f"claude/train/T1-{'a' * 12}\n", f"+claude/train/T1-{'a' * 12}"])
def test_delete_train_ref_refuses_any_other_name_without_a_git_call(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, name: str) -> None:
    env, calls = github_env(tmp_path, monkeypatch)
    assert env.delete_train_ref(name) is False and calls == []


def test_delete_train_ref_pushes_exactly_the_delete_of_that_ref(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    env, calls = github_env(tmp_path, monkeypatch)
    assert env.delete_train_ref(TRAIN) is True
    assert calls == [["git", "push", "--porcelain", "origin", f":refs/heads/{TRAIN}"]]


@pytest.mark.parametrize(("text", "want"), [('{"train_depth": 2}', 2), ("{}", None), ("{not json", None), ("[2]", None), (None, None)])
def test_train_depth_is_read_from_kit_runner_json_under_the_root(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, text: str | None, want: Any) -> None:
    env, _ = github_env(tmp_path, monkeypatch)
    if text is not None:
        (tmp_path / "kit").mkdir()
        (tmp_path / "kit" / "runner.json").write_text(text, encoding="utf-8")
    assert env.train_depth() == want
    assert json.loads(Path("kit/runner.json").read_text(encoding="utf-8"))["train_depth"] == 2


def test_train_refs_lists_only_train_shaped_names_with_their_committer_time(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    calls: list[list[str]] = []
    listing = f"{'d' * 40}\trefs/heads/{TRAIN}\n{'e' * 40}\trefs/heads/claude/train/odd-name\n"

    def fake(args: list[str], stdin: str | None, cwd: Path) -> tuple[int, str, str]:
        calls.append(args)
        return 0, (listing if "ls-remote" in args else "1793534400\n" if "show" in args else ""), ""

    env = GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=fake)
    assert env.train_refs() == [{"name": TRAIN, "task": "T002", "base": A1[:12], "sha": "d" * 40, "at": "2026-11-01T12:00:00Z"}]
    assert not any("push" in c for c in calls)
