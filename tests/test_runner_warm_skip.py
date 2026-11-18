"""The smoke warm-up builds nothing when the pre-session smoke will be reused (D-4281): the same rule over the same
state file as the pre-session step (`GitHubEnv._reusable_line`, D-2560, D-3058, D-3156); anything that doubts the
record, or no record at all, warms exactly as before (D-2007). Fake data only; no real cargo, git or network."""

from __future__ import annotations

import json
from datetime import timedelta
from pathlib import Path
from typing import Any

import pytest

from runner import __main__ as entry, github_env as ge, state as st
from test_failure_drills import NOW
from test_runner_keeper import _flow, _warm_env
from test_runner_pre_smoke import HEAD
from test_runner_smoke_reuse import CHAIN, OLD, _merged, _run, _state, _state_file, _with_chain

WARM = "Warm the smoke build before the session (D-2007)"
READ = "Read the recorded smoke runs for the pre-session smoke (D-2560)"
PRE = "Run the start-routine smoke before the session (D-2028)"


def _env(tmp_path: Path, monkeypatch: Any, ancestry: int = 0) -> tuple[Any, list[list[str]], list[Any]]:
    """`_warm_env` on HEAD at NOW, every compile worth saving, `smoke_ancestry` set and the fake git's first-parent
    chain HEAD, CHAIN[1] to CHAIN[5]."""
    env, calls, built = _warm_env(tmp_path, monkeypatch)
    monkeypatch.setattr(ge, "WARM_SAVE_SECONDS", 0)
    env.clock, env.smoke_ancestry = (lambda: NOW), ancestry
    _with_chain(env, [HEAD, *CHAIN[1:6]])
    return env, calls, built


def _skipped(tmp_path: Path, calls: list[list[str]], built: list[Any]) -> bool:
    """Nothing was prepared or built: no git worktree, no `warm_run`, no SMOKE_RAN."""
    worktree = [c for c in calls if c[:2] == ["git", "worktree"]]
    return not worktree and built == [] and not (tmp_path / "target" / ge.SMOKE_RAN).exists()


def _warmed(tmp_path: Path, line: str, built: list[Any]) -> bool:
    """The warm-up ran as today: every compile step, the cache marked for saving."""
    return (line.startswith(f"smoke warm-up on {HEAD[:12]}: toolchain exit 0, clippy exit 1, build exit 0, test exit 0; ")
            and line.endswith(", cache marked for saving") and len(built) == 4
            and (tmp_path / "target" / ge.SMOKE_RAN).exists())


def test_a_same_head_reusable_green_skips_the_warm_up(tmp_path: Path, monkeypatch: Any) -> None:
    env, calls, built = _env(tmp_path, monkeypatch)
    since = st.iso(NOW - timedelta(hours=2))
    _state_file(tmp_path, monkeypatch, _state(_run(HEAD, at=since)))
    assert env.warm_smoke() == (f"smoke warm-up skipped on {HEAD[:12]}: a green smoke of this head is recorded "
                                f"({since}); the session reuses it")
    assert _skipped(tmp_path, calls, built)


def _ancestry_file(tmp_path: Path, monkeypatch: Any, tasks: list[dict[str, Any]]) -> str:
    """A real green on CHAIN[2] 30 minutes ago with `tasks` merged since; returns its stamp."""
    since = st.iso(NOW - timedelta(minutes=30))
    _state_file(tmp_path, monkeypatch, dict(_state(_run(CHAIN[2], at=since)), tasks=tasks))
    return since


def test_an_ancestry_reuse_skips_the_warm_up_when_the_setting_allows_it(tmp_path: Path, monkeypatch: Any) -> None:
    env, calls, built = _env(tmp_path, monkeypatch, ancestry=5)
    since = _ancestry_file(tmp_path, monkeypatch, _merged(HEAD, CHAIN[1]))
    assert env.warm_smoke() == (f"smoke warm-up skipped on {HEAD[:12]}: the green smoke of {CHAIN[2][:12]} ({since}), "
                                "2 proven merge(s) back, stands for it (D-3058, D-3156); the session reuses it")
    assert _skipped(tmp_path, calls, built)


def test_the_same_ancestry_state_with_the_setting_off_warms(tmp_path: Path, monkeypatch: Any) -> None:
    env, _, built = _env(tmp_path, monkeypatch, ancestry=0)
    _ancestry_file(tmp_path, monkeypatch, _merged(HEAD, CHAIN[1]))
    assert _warmed(tmp_path, env.warm_smoke(), built)


def test_a_green_twin_tree_that_raises_warms_and_one_that_proves_skips(tmp_path: Path, monkeypatch: Any) -> None:
    env, calls, built = _env(tmp_path, monkeypatch, ancestry=5)
    _ancestry_file(tmp_path, monkeypatch, _merged(HEAD))  # CHAIN[1] is no DONE task's: only a twin tree proves it
    env.green_twin_tree = lambda sha: sha == CHAIN[1]
    assert env.warm_smoke().startswith(f"smoke warm-up skipped on {HEAD[:12]}: the green smoke of {CHAIN[2][:12]}")
    assert _skipped(tmp_path, calls, built)
    env.green_twin_tree = lambda sha: (_ for _ in ()).throw(RuntimeError("api"))
    assert _warmed(tmp_path, env.warm_smoke(), built)


@pytest.mark.parametrize("why", ["no_file", "unreadable", "malformed", "main_red", "later_red", "later_timeout",
                                 "too_old", "other_head"])
def test_anything_that_doubts_the_record_warms_exactly_as_today(tmp_path: Path, monkeypatch: Any, why: str) -> None:
    env, _, built = _env(tmp_path, monkeypatch, ancestry=5)
    red = {"check": "nightly", "sha": HEAD, "task": "", "revert_sha": "", "files": [], "set_at": st.iso(NOW)}
    states = {"main_red": _state(_run(HEAD), main_red=red), "later_red": _state(_run(HEAD), _run(HEAD, ok=False)),
              "later_timeout": _state(_run(HEAD), _run(HEAD, ok=False, timed_out=True)),
              "too_old": _state(_run(HEAD, at=NOW - timedelta(hours=6, seconds=1))), "other_head": _state(_run(OLD))}
    if why in states:
        _state_file(tmp_path, monkeypatch, states[why])
    elif why != "no_file":
        bad = tmp_path / "bad.json"
        bad.write_text("{not json") if why == "malformed" else bad.mkdir()
        monkeypatch.setenv(ge.PRE_SMOKE_STATE, str(bad))
    assert _warmed(tmp_path, env.warm_smoke(), built)


def test_a_dry_run_is_unchanged_even_with_a_reusable_record(tmp_path: Path, monkeypatch: Any) -> None:
    env, calls, built = _env(tmp_path, monkeypatch)
    _state_file(tmp_path, monkeypatch, _state(_run(HEAD)))
    env.dry_run = True
    assert env.warm_smoke() == f"dry-run: smoke warm-up not run on {HEAD[:12]}"
    assert _skipped(tmp_path, calls, built)


def test_the_warm_smoke_cli_reads_smoke_ancestry_reuse(monkeypatch: Any, capsys: Any) -> None:
    seen: list[int] = []
    monkeypatch.setattr(entry, "build", lambda *a: (_ for _ in ()).throw(AssertionError("no cycle")))
    monkeypatch.setattr(ge.GitHubEnv, "warm_smoke", lambda self: seen.append(self.smoke_ancestry) or "done")
    assert entry.main(["--warm-smoke"]) == 0 and capsys.readouterr().out.strip().endswith("runner: done")
    setting = json.loads((entry.ROOT / "kit" / "runner.json").read_text(encoding="utf-8"))["smoke_ancestry_reuse"]
    assert seen == [setting] == [entry.smoke_ancestry(entry.ROOT / "kit" / "runner.json")]


def test_the_workflow_reads_the_state_then_warms_then_smokes_and_gives_the_warm_up_no_secret() -> None:
    steps = _flow()["jobs"]["backup"]["steps"]
    order = [step.get("name") or step.get("uses") for step in steps]
    assert order.index(READ) < order.index(WARM) < order.index(PRE)
    warm, pre = steps[order.index(WARM)], steps[order.index(PRE)]
    assert warm["env"] == {ge.PRE_SMOKE_STATE: "${{ runner.temp }}/pre_smoke_state.json",
                           "RUNNER_CHECKS_TOKEN": "${{ github.token }}"} == pre["env"]
    assert warm["if"] == "hashFiles('Cargo.lock') != ''" and warm["continue-on-error"] is True
    assert warm["timeout-minutes"] == 25 and warm["run"].strip() == "python3 -m runner --warm-smoke"
    assert "secrets." not in json.dumps(warm)
