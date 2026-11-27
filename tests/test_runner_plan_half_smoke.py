"""D-5682: the pre-session smoke's cap is 2,100 s (the step's 40 minutes), and a `main` commit whose first-parent
changes since a real full green are only plan and docs paths (`st.PLAN_HALF_PATHS`) runs `smoke.sh --plan-half`:
every check but the Rust and TypeScript half, then the two Rust tests that read kit/tasks.json. A plan-half run is
never a base, its reuse ages from its base's real green (D-5793, D-5876) and a result whose base no longer stands
at use is discarded for the full smoke. Fake SHAs, a fake git and runner; no network, cargo or state branch."""

from __future__ import annotations

import json
import re
from datetime import timedelta
from pathlib import Path
from typing import Any

import pytest

from runner import cycle, github_env as ge, state as st
from test_failure_drills import NOW
from test_runner_keeper import _flow
from test_runner_pre_smoke import HEAD, _env, _pre, _result, job  # noqa: F401  # reason: `job` is a fixture used by name

ROOT = Path(__file__).resolve().parent.parent
A, B, M = "a" * 40, "b" * 40, "d" * 40  # the full green's base, a commit between, a DONE task's squash merge
A_AT = st.iso(NOW - timedelta(hours=1))
PLAN = ["kit/tasks.json", "docs/decisions.md"]


@pytest.fixture(autouse=True)
def _no_live_base(monkeypatch: pytest.MonkeyPatch) -> None:
    """The live job's nightly and base must not reach these tests (D-2660)."""
    for name in (ge.PRE_SMOKE_NIGHTLY, ge.PRE_SMOKE_BASE, ge.PRE_SMOKE_STATE):
        monkeypatch.delenv(name, raising=False)


def _run(sha: str, ok: bool = True, at: str = A_AT, **extra: Any) -> dict[str, Any]:
    return {"sha": sha, "ok": ok, "at": at, "why": "start routine", **extra}


def _half(sha: str, base: str = A, ok: bool = True, at: str = st.iso(NOW - timedelta(minutes=10))) -> dict[str, Any]:
    return _run(sha, ok, at, why=f"start routine (pre-session, plan half of {base})")


def _state(*runs: dict[str, Any], merged: str = "") -> dict[str, Any]:
    tasks = [{"id": "T001", "status": st.DONE, "merged_sha": merged}] if merged else []
    return {"tasks": tasks, "smoke_runs": list(runs)}


def _real(tmp_path: Path, state: dict[str, Any], chain: list[str], diffs: dict[str, list[str] | None],
          nightly: dict[str, Any] | None = None) -> Any:
    """A GitHubEnv on HEAD whose git answers `chain` and `diffs` (None: a git error) and whose state branch holds
    `state` and `nightly` (read through `state_head`); smoke_ancestry 3, NOW, no check red, nothing proven."""
    env, _ = _env(tmp_path)
    plain = env._run

    def run(a: list[str], i: str | None, c: Path) -> tuple[int, str, str]:
        if a[:3] == ["git", "rev-list", "--first-parent"]:
            return 0, "\n".join(chain) + "\n", ""
        if a[:2] == ["git", "diff"]:
            got = diffs.get(a[-1])
            return (128, "", "fatal: bad revision") if got is None else (0, "\n".join(got) + "\n", "")
        return plain(a, i, c)

    shown = {ge.STATE_FILE: state, ge.NIGHTLY_FILE: nightly}
    env._run, env.smoke_ancestry, env.clock, env.state_head = run, 3, lambda: NOW, "f" * 40
    env._show = lambda ref, path: json.dumps(shown[path]) if shown.get(path) is not None else None
    env.main_checks_green = lambda sha: None
    env.green_twin_tree = lambda sha: False
    return env


def _step(env: Any, tmp_path: Path, monkeypatch: pytest.MonkeyPatch, state: dict[str, Any],
          nightly: dict[str, Any] | None = {"sha": A, "conclusion": "success"}) -> list[list[str]]:
    """Run the pre-smoke step on HEAD with `state` and `nightly` (None: no file); returns smoke.sh's argument lists."""
    (tmp_path / "state.json").write_text(json.dumps(state))
    monkeypatch.setenv(ge.PRE_SMOKE_STATE, str(tmp_path / "state.json"))
    if nightly is not None:
        (tmp_path / "nightly.json").write_text(json.dumps(nightly))
    monkeypatch.setenv(ge.PRE_SMOKE_NIGHTLY, str(tmp_path / "nightly.json"))
    seen = _pre(env, lines="smoke: green (plan half)\n")
    env.line = env.pre_smoke()
    return [args for args, _, _ in seen]


# ---- the cap


def test_the_pre_smoke_cap_is_35_minutes_and_the_step_never_cuts_it_first() -> None:
    steps = _flow()["jobs"]["backup"]["steps"]
    order = [step.get("name") or step.get("uses") for step in steps]
    pre = steps[order.index("Run the start-routine smoke before the session (D-2028)")]
    read = steps[order.index("Read the recorded smoke runs for the pre-session smoke (D-2560)")]
    assert ge.PRE_SMOKE_TIMEOUT == 2100 and pre["timeout-minutes"] == 40
    assert pre["timeout-minutes"] >= ge.PRE_SMOKE_TIMEOUT / 60 + 5
    assert "kit/nightly.json?ref=claude/kit-state" in read["run"]
    assert 'echo "RUNNER_PRE_SMOKE_NIGHTLY=$RUNNER_TEMP/pre_smoke_nightly.json" >> "$GITHUB_ENV"' in read["run"]


def test_the_smoke_env_never_carries_the_nightly_or_the_base(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(ge.PRE_SMOKE_NIGHTLY, "/tmp/fake-nightly.json")
    monkeypatch.setenv(ge.PRE_SMOKE_BASE, f"{A} {A_AT}")
    env = ge._smoke_env()
    assert ge.PRE_SMOKE_NIGHTLY not in env and ge.PRE_SMOKE_BASE not in env


# ---- the allow-list and the base


@pytest.mark.parametrize("path", ["kit/tasks.json", "kit/plan.lock", "kit/prompts/T843.md", "kit/prompts/TEMPLATE.md",
                                  "docs/decisions.md", "docs/for-casey-review.md", "docs/test-design.md",
                                  "docs/spec/fake-area.md", "docs/handoff/fake.md"])
def test_each_allow_listed_path_is_plan_only(path: str) -> None:
    assert st.plan_only([path])


@pytest.mark.parametrize("paths", [[], None, ["docs/architecture.md"], ["docs/catalog/x.md"], ["docs/legal/x.md"],
                                   ["docs/copy/x.md"], ["docs/design/x.md"], ["kit/layers.json"], ["kit/runner.json"],
                                   ["kit/allowed_crates.txt"], ["scripts/smoke.sh"], ["tests/x.py"], ["runner/x.py"],
                                   [".github/x.yml"], ["app/core/src/lib.rs"], ["migrations/1.sql"], [".sqlx/q.json"],
                                   ["Cargo.lock"], ["README.md"], ["docs/spec/a/b.md"], ["kit/prompts/T1.md.bak"],
                                   ["kit/tasks.json", "app/core/src/lib.rs"]])
def test_anything_else_is_not_plan_only(paths: list[str] | None) -> None:
    assert not st.plan_only(paths)


def _base(state: dict[str, Any], chain: list[str], diffs: dict[str, list[str] | None], now: Any = NOW) -> Any:
    return st.plan_half_base(state, chain, st.done_merges(state), now, 3, None, diffs.get)


def test_a_plan_only_commit_after_a_real_green_has_that_base() -> None:
    found = _base(_state(_run(A)), [HEAD, A], {HEAD: PLAN})
    assert found is not None and found[0] == A and found[1]["at"] == A_AT and found[2] == PLAN


def test_a_done_merge_then_a_plan_only_commit_has_the_base_and_only_merges_have_none() -> None:
    state = _state(_run(A), merged=M)
    assert _base(state, [HEAD, M, A], {HEAD: PLAN})[0] == A
    assert _base(_state(_run(A), merged=HEAD), [HEAD, A], {}) is None, "only DONE merges: the ancestry reuse's"


@pytest.mark.parametrize("diff", [["app/core/src/lib.rs"], ["kit/tasks.json", "app/core/src/lib.rs"],
                                  ["docs/architecture.md"], [], None])
def test_code_mixed_architecture_empty_or_unreadable_commits_have_no_base(diff: list[str] | None) -> None:
    assert _base(_state(_run(A)), [HEAD, A], {HEAD: diff}) is None


def test_no_base_when_too_old_red_main_doubted_or_a_plan_half_run_would_be_it() -> None:
    assert _base(_state(_run(A)), [HEAD, A], {HEAD: PLAN}, NOW + timedelta(hours=5, minutes=1)) is None
    red = dict(_state(_run(A)), main_red={"check": "smoke", "sha": A})
    assert _base(red, [HEAD, A], {HEAD: PLAN}) is None
    assert _base(_state(_run(A), _run(B, ok=False)), [HEAD, B, A], {HEAD: PLAN, B: PLAN}) is None
    only_half = _state(_half(B))
    assert _base(only_half, [HEAD, B, A], {HEAD: PLAN, B: PLAN}) is None, "a plan-half run is never a base"
    assert st.ancestry_smoke(only_half, [HEAD, B], {HEAD}, NOW, 3) is None
    assert _base(_state(_run(A), _half(B)), [HEAD, B, A], {HEAD: PLAN, B: PLAN})[0] == A


# ---- reuse ages from the base's real green


def test_a_plan_half_green_is_reusable_on_its_head_aging_from_its_base() -> None:
    state = _state(_run(A), _half(B))
    kept = st.reusable_smoke(state, B, NOW)
    assert kept is not None and kept["reused"] == A_AT
    assert st.reusable_smoke(state, B, st.parse(A_AT) + timedelta(hours=6, minutes=1)) is None
    assert st.reusable_smoke(_state(_half(B)), B, NOW) is None, "no real green of the base recorded"
    assert st.reusable_smoke(_state(_run(A), _half(B, base="a" * 12)), B, NOW) is None, "a base that is not 40 hex"
    assert st.reusable_smoke(_state(_run(A), _half(B, ok=False)), B, NOW) is None, "a red plan half counts as red"


def test_a_recorded_reuse_of_a_plan_half_green_still_ages_from_the_base_and_needs_it() -> None:
    reuse = _run(B, at=st.iso(NOW), why=f"start routine (reused {A_AT})", reused=A_AT)
    state = _state(_run(A), _half(B), reuse)
    assert st.reusable_smoke(state, B, NOW + timedelta(hours=4)) is not None
    assert st.reusable_smoke(state, B, st.parse(A_AT) + timedelta(hours=6, minutes=1)) is None
    gone = _state(_half(B), reuse, dict(reuse, at=st.iso(NOW + timedelta(minutes=5))))
    assert st.reusable_smoke(gone, B, NOW + timedelta(minutes=10)) is None, "A's run has left smoke_runs"


def test_a_reuse_whose_green_left_smoke_runs_needs_clean_commits_back_to_a_done_merge() -> None:
    reuse = _run(B, at=st.iso(NOW), why=f"start routine (reused {A_AT})", reused=A_AT)
    state = _state(reuse, merged=M)
    assert st.reusable_smoke(state, B, NOW) is None, "no chain to read"
    assert st.reusable_smoke(state, B, NOW, lambda: [B, M, A]) is not None
    assert st.reusable_smoke(state, B, NOW, lambda: [B, A]) is None, "no DONE merge in reach"
    doubted = dict(state, smoke_env_problems=[{"sha": M}])
    assert st.reusable_smoke(doubted, B, NOW, lambda: [B, M, A]) is None

    def broken() -> list[str]:
        raise RuntimeError("fake git failure")

    assert st.reusable_smoke(state, B, NOW, broken) is None


def test_an_ordinary_reuse_of_a_full_green_beside_an_expired_plan_half_run_stays_reusable() -> None:
    full = _run(B, at=st.iso(NOW - timedelta(minutes=30)))
    reuse = _run(B, at=st.iso(NOW), why="start routine (reused x)", reused=full["at"])
    state = _state(_half(B, at=st.iso(NOW - timedelta(hours=7))), full, reuse)
    assert st.reusable_smoke(state, B, NOW) is not None


# ---- the pre-smoke step


def test_a_plan_only_head_runs_the_plan_half_in_the_step_and_logs_the_reuse(job: Path, tmp_path: Path,
                                                                              monkeypatch: pytest.MonkeyPatch) -> None:
    state = _state(_run(A))
    env = _real(tmp_path, state, [HEAD, A], {HEAD: PLAN})
    assert _step(env, tmp_path, monkeypatch, state) == [["bash", "scripts/smoke.sh", "--plan-half"]]
    assert env.line.startswith(f"smoke reused from {A[:12]}: {HEAD[:12]} changes only plan and docs paths since "
                               f"(2 paths: kit/tasks.json, docs/decisions.md); plan half: exit 0; smoke: green (plan half);")
    result = _result(job)
    assert result["plan_half_of"] == A and result["plan_half_at"] == A_AT and result["ok"] is True


@pytest.mark.parametrize("case", ["code", "mixed", "architecture", "git_error", "empty", "old", "red_nightly",
                                  "no_nightly", "off"])
def test_anything_short_of_a_standing_plan_only_chain_runs_the_full_smoke(job: Path, tmp_path: Path,
                                                                          monkeypatch: pytest.MonkeyPatch, case: str) -> None:
    diffs: dict[str, list[str] | None] = {"code": {HEAD: ["app/core/src/lib.rs"]},
                                          "mixed": {HEAD: ["kit/tasks.json", "app/core/src/lib.rs"]},
                                          "architecture": {HEAD: ["docs/architecture.md"]}, "git_error": {HEAD: None},
                                          "empty": {HEAD: []}}.get(case, {HEAD: PLAN, B: PLAN})
    old = case == "old"
    state = _state(_run(A, at=st.iso(NOW - timedelta(hours=6, minutes=1)) if old else A_AT))
    env = _real(tmp_path, state, [HEAD, B, A], diffs)
    env.smoke_ancestry = 0 if case == "off" else 3
    nightly = None if case == "no_nightly" else {"sha": B, "conclusion": "failure" if case == "red_nightly" else "success"}
    assert _step(env, tmp_path, monkeypatch, state, nightly) == [["bash", "scripts/smoke.sh"]]
    assert "plan_half_of" not in _result(job)


# ---- at use


def _pre_result(job: Path, code: int = 0, base: str = A, at: str = A_AT, **extra: Any) -> dict[str, Any]:
    pre = {"sha": HEAD, "ok": code == 0, "timed_out": False, "exit": code, "seconds": 300, "at": st.iso(NOW),
           "job": "77-1", "last": "smoke: green (plan half)", "plan_half_of": base, "plan_half_at": at, **extra}
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_PRE).write_text(json.dumps(pre))
    return pre


def test_a_standing_plan_half_result_is_used_and_recorded_with_its_marker(job: Path, tmp_path: Path) -> None:
    state = _state(_run(A))
    env = _real(tmp_path, state, [HEAD, A], {HEAD: PLAN})
    _pre_result(job)
    out, ok, head = cycle._smoke(state, env, NOW, "start routine")
    assert ok is True and head == HEAD and env.smoke_source == f"pre-session, plan half of {A}"
    assert any(line.startswith(f"smoke reused from {A[:12]}: {HEAD[:12]} changes only plan") for line in env.log)
    run = out["smoke_runs"][-1]
    assert run["why"] == f"start routine (pre-session, plan half of {A})" and not st._real_green(run)
    assert st.reusable_smoke(out, HEAD, NOW)["reused"] == A_AT


def test_a_red_plan_half_is_recorded_red(job: Path, tmp_path: Path) -> None:
    state = _state(_run(A))
    env = _real(tmp_path, state, [HEAD, A], {HEAD: PLAN})
    _pre_result(job, code=1)
    out, ok, _ = cycle._smoke(state, env, NOW, "start routine")
    assert ok is False and out["smoke_runs"][-1]["ok"] is False and "plan half of" in out["smoke_runs"][-1]["why"]
    assert st.reusable_smoke(out, HEAD, NOW) is None


@pytest.mark.parametrize("why", ["old", "red_nightly", "other_at", "no_at"])
def test_a_plan_half_result_whose_base_no_longer_stands_is_discarded_for_the_full_smoke(job: Path, tmp_path: Path,
                                                                                         why: str) -> None:
    state = _state(_run(A))
    nightly = {"sha": HEAD, "conclusion": "failure"} if why == "red_nightly" else None
    env = _real(tmp_path, state, [HEAD, A], {HEAD: PLAN}, nightly)
    if why == "old":
        env.clock = lambda: st.parse(A_AT) + timedelta(hours=6, minutes=1)
    _pre_result(job, at={"other_at": st.iso(NOW - timedelta(hours=2)), "no_at": ""}.get(why, A_AT))
    calls: list[list[str]] = []
    plain = env._run
    env._run = lambda a, i, c: calls.append(list(a)) or plain(a, i, c)
    out, ok, _ = cycle._smoke(state, env, NOW, "start routine")
    assert ["bash", "scripts/smoke.sh"] in calls, "the full smoke ran in its place"
    assert env.smoke_source == "" and "plan half" not in out["smoke_runs"][-1]["why"]
    assert not (job / ge.SMOKE_PRE).exists() and any("is discarded" in line for line in env.log)


def test_a_background_plan_half_discarded_at_use_is_pending_then_restarted_full(job: Path, tmp_path: Path) -> None:
    state = _state(_run(A))
    env = _real(tmp_path, state, [HEAD, A], {HEAD: PLAN}, {"sha": HEAD, "conclusion": "failure"})
    env.smoke_background, env.smoke_wait, env.sleep = True, 0, lambda s: None
    job.mkdir(parents=True, exist_ok=True)
    (job / ge.SMOKE_BG).write_text(json.dumps({"sha": HEAD, "job": "77-1", "pid": 4242}))
    seen: dict[str, Any] = {"stops": 0, "starts": [], "looks": 0}

    def alive(pid: int) -> bool:  # the result lands while the session waits (the red nightly came meanwhile)
        seen["looks"] += 1
        if seen["looks"] == 2:
            _pre_result(job, source=ge.BACKGROUND)
        return True

    env.bg_alive = alive
    env.bg_stop = lambda: seen.update(stops=seen["stops"] + 1) or "stopped"
    env.bg_start = lambda head, root: seen["starts"].append((head, ge.os.environ.get(ge.PRE_SMOKE_BASE))) or 4343
    env.read_state, env.read_nightly = lambda: state, lambda: {"sha": HEAD, "conclusion": "failure"}
    out, ok, _ = cycle._smoke(state, env, NOW, "start routine")
    assert out == state and ok is False and env.smoke_pending is True and seen["stops"] == 1
    assert not (job / ge.SMOKE_PRE).exists() and not (job / ge.SMOKE_BG).exists()
    env.smoke_pending = False
    cycle._smoke(state, env, NOW, "start routine")
    assert seen["starts"] == [(HEAD, None)], "the next start is the full smoke: no base"


def test_a_background_start_passes_the_base_only_around_bg_start(job: Path, tmp_path: Path) -> None:
    state = _state(_run(A))
    env = _real(tmp_path, state, [HEAD, A], {HEAD: PLAN}, {"sha": A, "conclusion": "success"})
    env.read_state, env.read_nightly = lambda: state, lambda: {"sha": A, "conclusion": "success"}
    seen: list[tuple[Any, ...]] = []
    env.bg_start = lambda *args: seen.append((*args, ge.os.environ.get(ge.PRE_SMOKE_BASE))) or 4343
    assert env._start_background(HEAD) == 4343
    assert seen == [(HEAD, env.root, f"{A} {A_AT}")] and ge.PRE_SMOKE_BASE not in ge.os.environ
    env.smoke_ancestry = 0
    env._start_background(HEAD)
    assert seen[-1] == (HEAD, env.root, None) and ge.PRE_SMOKE_BASE not in ge.os.environ


def test_the_background_child_runs_the_plan_half_from_its_base(job: Path, tmp_path: Path,
                                                               monkeypatch: pytest.MonkeyPatch) -> None:
    env = _real(tmp_path, _state(), [HEAD], {})
    monkeypatch.setenv(ge.PRE_SMOKE_HEAD, HEAD)
    monkeypatch.setenv(ge.PRE_SMOKE_BASE, f"{A} {A_AT}")
    seen = _pre(env, lines="smoke: green (plan half)\n")
    env.pre_smoke()
    assert [args for args, _, _ in seen] == [["bash", "scripts/smoke.sh", "--plan-half"]]
    assert _result(job)["plan_half_of"] == A and _result(job)["source"] == ge.BACKGROUND


def test_the_hand_in_smoke_never_takes_a_plan_half_result(job: Path, tmp_path: Path) -> None:
    env = _real(tmp_path, _state(_run(A)), [HEAD, A], {HEAD: PLAN})
    env.bg_alive, env.smoke_wait = lambda pid: False, 0
    _pre_result(job)
    run: dict[str, Any] = {"sha": HEAD, "job": "77-1", "pid": 4242}
    env._await_after_smoke(HEAD, run)
    assert run["exit"] == 124 and "without a result" in run["output"]


def test_an_own_smoke_collected_with_a_stale_base_is_pending_and_never_red(job: Path, tmp_path: Path) -> None:
    env = _real(tmp_path, _state(_run(A)), [HEAD, A], {HEAD: PLAN})
    env.clock = lambda: st.parse(A_AT) + timedelta(hours=6, minutes=1)
    pre = _pre_result(job)
    ok, _ = env.collect_own_smoke({"sha": HEAD, "pid": None, "alive": False, "result": pre})
    assert ok is False and env.smoke_pending is True and not (job / ge.SMOKE_PRE).exists()


# ---- guards


ALLOWED_NAMES = re.compile(r"tasks\.json|plan\.lock|T[0-9]+\.md|TEMPLATE\.md|decisions\.md|for-casey-review\.md|"
                           r"test-design\.md|docs/spec|docs/handoff|kit/prompts")


def test_no_rust_or_typescript_reads_an_allow_listed_path_but_the_two_plan_half_tests() -> None:
    """Only kit/tasks.json, read at run time by the two tests the plan half runs, may tie the Rust or TypeScript
    half's verdict to a plan-only path; anything else must leave PLAN_HALF_PATHS (D-5682)."""
    literal = re.compile(r'"([^"\n]*)"')
    rust = [p for d in ("app", "tests") for p in (ROOT / d).rglob("*") if p.is_file()
            and (p.suffix == ".rs" or p.name == "Cargo.toml") and "target" not in p.parts]
    hits = {(str(p.relative_to(ROOT)), m) for p in rust for m in literal.findall(p.read_text(encoding="utf-8"))
            if ALLOWED_NAMES.search(m)}
    assert {f for f, m in hits if "tasks.json" in m} == {"app/server/tests/route_wiring.rs",
                                                          "tests/integration/tests/architecture_migrations.rs"}
    assert all(not ALLOWED_NAMES.search(m.replace("kit/tasks.json", "")) for _, m in hits), sorted(hits)
    includes = json.loads((ROOT / "tsconfig.json").read_text(encoding="utf-8"))["include"]
    ts = {p for pattern in includes for p in ROOT.glob(pattern) if p.is_file()}
    reads = re.compile(r"""(?:import|from|require|readFile\w*|fetch)\s*\(?\s*['"`][^'"`]*(?:""" + ALLOWED_NAMES.pattern + ")")
    assert not [p for p in ts if reads.search(p.read_text(encoding="utf-8"))]


def test_the_plan_half_branch_runs_exactly_the_two_test_targets() -> None:
    text = (ROOT / "scripts" / "smoke.sh").read_text(encoding="utf-8")
    branch = text.split('if [ "${FULL}" = "--plan-half" ]; then', 1)[1].split("\nfi\n", 1)[0]
    helper = next(line for line in text.splitlines() if line.startswith("plan_tests() {"))
    assert re.findall(r"--test (\S+?);?(?=\s|$)", helper) == ["route_wiring", "architecture_migrations"]
    assert "cargo test --workspace --locked --test" in helper and "plan_tests" in branch
    assert "smoke: green (plan half)" in branch and "exit 0" in branch
    for absent in ("cargo fmt", "cargo clippy", "cargo build", "sqlx prepare", "cargo audit", "npx", "npm"):
        assert absent not in branch, absent
    assert [line for line in text.splitlines() if line.strip().startswith("cargo test")] == ["  cargo test --workspace --locked"]
    assert "[--full | --plan-half]" in text
