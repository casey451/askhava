"""TD-OPS-33 (T285, D-1869): a task closes DONE with an `owned_gap` mark when a file it owns, or a route it
was to wire, is missing on main at the merge; the mark clears by itself and STATUS.md lists it.

The drills' FakeEnv stands in for GitHub, with a fake commit store for `text_at` and `files_at`. Fake
paths and commits only.
"""

from __future__ import annotations

import json
import subprocess
from datetime import timedelta
from pathlib import Path
from typing import Any

import pytest

from runner import cycle, state as st
from runner.github_env import Board, GitHubEnv, render_status
from tests.test_failure_drills import NOW, FakeEnv, config, green_pr, make_task

ARCH = """# Architecture

## 13. Routes

| Method | Path | Module | Wired |
|---|---|---|---|
| GET | /q/{token} | portals | yes |
| POST | /b/sign-in | portals | {wired} |

## 14. Next
"""


class GitEnv(FakeEnv):
    """FakeEnv with commits: {sha: {path: text}}; a sha not in `commits` is one git cannot read."""

    def __init__(self) -> None:
        super().__init__()
        self.commits: dict[str, dict[str, str]] = {}

    def _tree(self, sha: str) -> dict[str, str]:
        if sha not in self.commits:
            raise RuntimeError(f"git cat-file -e {sha} failed")
        return self.commits[sha]

    def text_at(self, sha: str, path: str) -> str | None:
        return self._tree(sha).get(path)

    def files_at(self, sha: str, path: str) -> list[str]:
        return [p for p in self._tree(sha) if p == path or (path.endswith("/") and p.startswith(path))]


def commit(owns: dict[str, list[str]], files: list[str], wired: str = "yes") -> dict[str, str]:
    plan = {"tasks": [{"id": tid, "owns": paths} for tid, paths in owns.items()]}
    return {"kit/tasks.json": json.dumps(plan), "docs/architecture.md": ARCH.replace("{wired}", wired),
            **{f: "x" for f in files}}


T1_OWNS = ["app/t001/src/purge.rs", "app/t001/templates/"]


def close_t001(tmp_path: Path, tree: dict[str, str]) -> tuple[GitEnv, cycle.Config]:
    """Claim T001 (T002 waits for the lane) and merge its pull request at commit m1 with `tree`."""
    env, cfg = GitEnv(), config([make_task("T001"), make_task("T002")], tmp_path)
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING
    env.commits["m1"] = tree
    env.prs["T001"] = {**green_pr(), "merged_sha": "m1", "files": ["app/t001/src/purge.rs"]}
    return env, cfg


def outbox_items(tmp_path: Path) -> list[Path]:
    return sorted(tmp_path.glob("*-owned-gap.md"))


def test_all_present_is_done_with_no_mark_and_no_outbox(tmp_path: Path) -> None:
    env, cfg = close_t001(tmp_path, commit({"T001": T1_OWNS}, ["app/t001/src/purge.rs", "app/t001/templates/a.html"]))
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == st.MERGE_PENDING  # T001 closed, T002 claimed
    entry = st.entry(env.store, "T001")
    assert entry["status"] == st.DONE and "owned_gap" not in entry
    assert outbox_items(tmp_path) == []


def test_missing_file_is_done_with_a_mark_one_outbox_item_and_the_lane_free(tmp_path: Path) -> None:
    env, cfg = close_t001(tmp_path, commit({"T001": T1_OWNS}, ["app/t001/templates/a.html"]))
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == st.MERGE_PENDING  # T001 closed, T002 claimed
    entry = st.entry(env.store, "T001")
    assert entry["status"] == st.DONE and entry["merged_sha"] == "m1"
    assert entry["owned_gap"] == {"at": st.iso(NOW + timedelta(minutes=10)),
                                  "problems": ["T001: app/t001/src/purge.rs missing on main"]}
    [item] = outbox_items(tmp_path)
    text = item.read_text(encoding="utf-8")
    assert "title: T001: owned file missing at close" in text and "kind: blocked" in text
    assert "app/t001/src/purge.rs" in text
    assert st.status(env.store, "T002") == st.MERGE_PENDING, "the lane is free for the next claim"
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=20))
    assert len(outbox_items(tmp_path)) == 1, "one outbox item per close"


def test_an_empty_owned_folder_is_a_miss(tmp_path: Path) -> None:
    env, cfg = close_t001(tmp_path, commit({"T001": T1_OWNS}, ["app/t001/src/purge.rs", "app/t001/templatesX"]))
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10))
    assert st.entry(env.store, "T001")["owned_gap"]["problems"] == ["T001: app/t001/templates/ missing on main"]


def test_a_route_row_still_naming_the_task_is_a_miss(tmp_path: Path) -> None:
    env, cfg = close_t001(tmp_path, commit({"T001": ["app/t001/src/purge.rs"]}, ["app/t001/src/purge.rs"], wired="T001"))
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == st.MERGE_PENDING  # T001 closed, T002 claimed
    assert st.entry(env.store, "T001")["owned_gap"]["problems"] == [
        "T001: route POST /b/sign-in still reads Wired T001 in docs/architecture.md section 13"]


def test_problems_kept_are_capped(tmp_path: Path) -> None:
    owns = [f"app/t001/f{i}.rs" for i in range(cycle.PLAN_GAP_PROBLEMS_KEPT + 3)]
    env, cfg = close_t001(tmp_path, commit({"T001": owns}, []))
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10))
    problems = st.entry(env.store, "T001")["owned_gap"]["problems"]
    assert len(problems) == cycle.PLAN_GAP_PROBLEMS_KEPT + 1 and problems[-1] == "and 3 more"


def test_a_git_error_at_close_leaves_done_without_a_mark(tmp_path: Path) -> None:
    env, cfg = close_t001(tmp_path, commit({"T001": T1_OWNS}, []))
    env.commits.pop("m1")
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == st.MERGE_PENDING  # T001 closed, T002 claimed
    assert st.status(env.store, "T001") == st.DONE
    assert "owned_gap" not in st.entry(env.store, "T001") and outbox_items(tmp_path) == []


def test_empty_handin_and_fix_main_are_not_checked(tmp_path: Path) -> None:
    env = GitEnv()
    state = st.with_task(st.new_state(), "T009", status=st.DONE, merged_sha="m1", empty_handin=True)
    state = st.with_task(state, st.FIX_MAIN, status=st.DONE, merged_sha="m1")
    env.commits["m1"] = commit({"T009": ["app/t009/x.rs"], st.FIX_MAIN: ["app/fix/x.rs"]}, [])
    cfg = config([], tmp_path)
    for tid in ("T009", st.FIX_MAIN):
        assert cycle.mark_owned_gap(state, env, cfg, tid, NOW) == state
    assert outbox_items(tmp_path) == []


def test_an_env_without_commit_reads_skips_the_check(tmp_path: Path) -> None:
    state = st.with_task(st.new_state(), "T001", status=st.DONE, merged_sha="m1")
    assert cycle.mark_owned_gap(state, FakeEnv(), config([], tmp_path), "T001", NOW) == state
    assert cycle.owned_problems(GitEnv(), "T001", "", "") is None


def marked(tmp_path: Path) -> GitEnv:
    """T001 DONE with a mark for a missing file, T002 now running; main's head is `env.head`."""
    env, cfg = close_t001(tmp_path, commit({"T001": T1_OWNS}, ["app/t001/templates/a.html"]))
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10))
    assert "owned_gap" in st.entry(env.store, "T001")
    return env


def test_the_mark_clears_when_the_file_reaches_main(tmp_path: Path) -> None:
    env = marked(tmp_path)
    env.commits[env.head] = commit({"T001": T1_OWNS}, ["app/t001/templates/a.html"])
    assert cycle.recheck_owned_gaps(env.store, env) == env.store, "still missing: unchanged, no churn"
    env.commits[env.head] = commit({"T001": T1_OWNS}, ["app/t001/templates/a.html", "app/t001/src/purge.rs"])
    assert "owned_gap" not in st.entry(cycle.recheck_owned_gaps(env.store, env), "T001")


def test_the_mark_clears_when_a_plan_change_moves_the_path(tmp_path: Path) -> None:
    env = marked(tmp_path)
    moved = {"T001": ["app/t001/templates/"], "T050": ["app/t001/src/purge.rs"]}
    env.commits[env.head] = commit(moved, ["app/t001/templates/a.html"])
    assert "owned_gap" not in st.entry(cycle.recheck_owned_gaps(env.store, env), "T001")


def test_a_file_the_task_deletes_is_no_miss_and_a_plan_change_naming_it_clears_the_mark(tmp_path: Path) -> None:
    """D-2922: T295 deleted tests/brand_literal_baseline.json as its goal said; once kit/tasks.json lists the
    file under that task's `deletes`, its absence is no gap at a close and a standing mark clears."""
    env = marked(tmp_path)
    plan = {"tasks": [{"id": "T001", "owns": T1_OWNS, "deletes": ["app/t001/src/purge.rs"]}]}
    env.commits[env.head] = {**commit({"T001": T1_OWNS}, ["app/t001/templates/a.html"]),
                             "kit/tasks.json": json.dumps(plan)}
    assert cycle.owned_problems(env, "T001", env.head, env.head) == []
    assert "owned_gap" not in st.entry(cycle.recheck_owned_gaps(env.store, env), "T001")
    plan["tasks"][0]["deletes"] = []
    env.commits[env.head]["kit/tasks.json"] = json.dumps(plan)
    assert cycle.owned_problems(env, "T001", env.head, env.head) == ["T001: app/t001/src/purge.rs missing on main"]


def test_the_mark_narrows_and_clears_by_route_rows(tmp_path: Path) -> None:
    env = GitEnv()
    gap = {"at": "2026-11-01T12:00:00Z", "problems": ["T001: a", "T001: b"]}
    state = st.with_task(st.new_state(), "T001", status=st.DONE, merged_sha="m1", owned_gap=gap)
    env.commits[env.head] = commit({"T001": []}, [], wired="T001")
    narrowed = cycle.recheck_owned_gaps(state, env)
    assert st.entry(narrowed, "T001")["owned_gap"]["problems"] == [
        "T001: route POST /b/sign-in still reads Wired T001 in docs/architecture.md section 13"]
    assert st.entry(narrowed, "T001")["owned_gap"]["at"] == gap["at"]
    for wired in ("yes", "T300"):
        env.commits[env.head] = commit({"T001": []}, [], wired=wired)
        assert "owned_gap" not in st.entry(cycle.recheck_owned_gaps(state, env), "T001")


def test_the_mark_stays_on_a_git_error(tmp_path: Path) -> None:
    env = marked(tmp_path)
    assert env.head not in env.commits
    assert cycle.recheck_owned_gaps(env.store, env) == env.store
    env.commits[env.head] = {"docs/architecture.md": ARCH}  # no kit/tasks.json: no evidence either
    assert cycle.recheck_owned_gaps(env.store, env) == env.store


def test_a_run_cycle_clears_the_mark(tmp_path: Path) -> None:
    env, cfg = close_t001(tmp_path, commit({"T001": T1_OWNS}, ["app/t001/templates/a.html"]))
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10))
    env.prs.pop("T001")
    env.commits[env.head] = commit({"T001": T1_OWNS}, ["app/t001/templates/a.html", "app/t001/src/purge.rs"])
    cycle.run_cycle(cfg, env, NOW + timedelta(minutes=20))
    assert "owned_gap" not in st.entry(env.store, "T001")


def test_status_needs_attention_lists_the_mark_on_a_done_task() -> None:
    gap = {"at": "2026-11-01T12:00:00Z", "problems": ["T001: app/t001/src/purge.rs missing on main"]}
    state = st.with_task(st.new_state(), "T001", status=st.DONE, owned_gap=gap)
    text = render_status(state, "**Requirements verified:** 1 of 9", NOW, "fake pacing", Board((), None, (), ()))
    attention = text.split("## Needs attention", 1)[1].split("\n## ", 1)[0]
    assert "- OWNED GAP T001 (DONE, since 2026-11-01T12:00:00Z" in attention
    assert "app/t001/src/purge.rs missing on main" in attention


def git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=repo, capture_output=True, text=True, check=True).stdout.strip()


def test_github_env_reads_files_and_text_at_a_commit(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    (repo / "app" / "x").mkdir(parents=True)
    (repo / "app" / "x" / "a.rs").write_text("fake\n", encoding="utf-8")
    git(repo, "init", "-q")
    git(repo, "add", ".")
    git(repo, "-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "-m", "fake")
    sha = git(repo, "rev-parse", "HEAD")
    env: Any = GitHubEnv(repo, [], tmp_path, repo="o/r", starter="manual", runner_sha="x")
    assert env.files_at(sha, "app/x/") == ["app/x/a.rs"] and env.files_at(sha, "app/y/") == []
    assert env.text_at(sha, "app/x/a.rs") == "fake\n" and env.text_at(sha, "app/x/b.rs") is None
    with pytest.raises(RuntimeError):
        env.files_at("0" * 40, "app/x/")
