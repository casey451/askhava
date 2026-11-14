"""D-3269 (b): the update merge resolves a crate `app/<crate>/CLAUDE.md` or `app/<crate>/src/api.rs` that
both sides only added lines to, and checks the result again; any other shape stays a real conflict and
goes to a resync (D-891). Over real local git repositories (the `repo` fixture of tests/test_runner_live.py)
and, for the rustfmt step, over a fake command runner. Fake data only.
"""

from __future__ import annotations

import os
import shutil
from pathlib import Path

import pytest

import check_code_docs
from runner import github_env as ge
from runner.update_merge import UpdateMerge, exported, insert_only, reexports_only
from test_runner_live import git, repo  # noqa: F401  # reason: `repo` is the shared fixture
from test_runner_update_merge import BRANCH, env_on, parents, push_branch, push_main

ROOT = Path(__file__).resolve().parent.parent
DOC_PATH, API_PATH = "app/pay/CLAUDE.md", "app/pay/src/api.rs"
HEADS = tuple(h.removeprefix("## ") for h in check_code_docs.HEADINGS)  # the check's own headings, so they cannot drift (T437)
OWNS = "- what it owns\n"  # a second Never-break bullet the tests below add lines after
DOC = "# app/pay\n\n" + "".join(f"## {h}\n\n- {h.lower()}\n" + (OWNS if h == "Never break" else "") + "\n" for h in HEADS)
API = "//! The public face of a fake crate.\n\npub use crate::a::A;\npub use crate::c::C;\n\n/// Does nothing.\npub fn f() {}\n"
RUSTFMT = shutil.which("rustfmt") is not None


def test_ci_has_rustfmt_for_the_api_tests() -> None:
    """ci.yml's Python job sets REQUIRE_RUSTFMT, so the two real-git api.rs tests below can never skip there."""
    assert RUSTFMT or not os.environ.get("REQUIRE_RUSTFMT"), "REQUIRE_RUSTFMT is set but rustfmt is not installed"


def padded(lines: int) -> str:
    """DOC with filler bullets under Purpose until it is `lines` lines long."""
    extra = lines - len(DOC.splitlines())
    return DOC.replace("- purpose\n", "- purpose\n" + "".join(f"- filler {n}\n" for n in range(extra)))


def with_tools(env: ge.GitHubEnv, root: Path) -> ge.GitHubEnv:
    """The runner's checkout with the real scripts/check_code_docs.py, which the resolution runs again."""
    (root / "scripts").mkdir(exist_ok=True)
    shutil.copy(ROOT / "scripts" / "check_code_docs.py", root / "scripts" / "check_code_docs.py")
    return env


def update(repo: dict[str, Path], tmp_path: Path, reviewed: str) -> dict[str, object]:
    return with_tools(env_on(repo, tmp_path), repo["b"]).update_branch(BRANCH, reviewed)


def two_sides(repo: dict[str, Path], path: str, base: str, ours: str, theirs: str) -> tuple[str, str]:
    """`base` on main, `ours` on the task branch, then `theirs` on main: (reviewed head, main's head)."""
    push_main(repo, path, base)
    reviewed = push_branch(repo, path, ours)
    return reviewed, push_main(repo, path, theirs)


# ---- the crate CLAUDE.md


def test_a_crate_doc_both_sides_added_to_is_resolved_main_first(repo: dict[str, Path], tmp_path: Path) -> None:
    assert insert_only(DOC_PATH) and insert_only(API_PATH) and not insert_only("app/pay/src/lib.rs")
    ours, theirs = DOC.replace("- what it owns\n", "- what it owns\n- branch\n"), DOC.replace("- what it owns\n", "- what it owns\n- main\n")
    reviewed, main2 = two_sides(repo, DOC_PATH, DOC, ours, theirs)
    done = update(repo, tmp_path, reviewed)
    assert done["status"] == "pushed" and parents(repo["b"], str(done["sha"])) == [reviewed, main2]
    merged = git(repo["b"], "show", f"{done['sha']}:{DOC_PATH}")
    assert "- what it owns\n- main\n- branch\n" in merged


def test_a_crate_doc_one_side_edited_stays_a_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    ours = DOC.replace("- what it owns\n", "- what it owns, edited\n- branch\n")
    reviewed, _ = two_sides(repo, DOC_PATH, DOC, ours, DOC.replace("- what it owns\n", "- what it owns\n- main\n"))
    done = update(repo, tmp_path, reviewed)
    assert done["status"] == "conflict" and done["paths"] == [DOC_PATH]
    assert git(repo["origin"], "rev-parse", BRANCH) == reviewed


def test_a_crate_doc_over_its_limit_once_merged_stays_a_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    base = padded(119)
    ours, theirs = base.replace("- what it owns\n", "- what it owns\n- branch\n"), base.replace("- what it owns\n", "- what it owns\n- main\n")
    assert len(ours.splitlines()) == len(theirs.splitlines()) == 120  # each side alone is within the limit
    reviewed, _ = two_sides(repo, DOC_PATH, base, ours, theirs)
    assert update(repo, tmp_path, reviewed)["paths"] == [DOC_PATH]
    assert git(repo["origin"], "rev-parse", BRANCH) == reviewed


def test_a_crate_doc_git_merged_cleanly_over_its_limit_stays_a_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    base = padded(119)
    ours, theirs = base.replace("- purpose\n", "- purpose\n- branch\n"), base.replace("- never break\n", "- never break\n- main\n")
    reviewed, _ = two_sides(repo, DOC_PATH, base, ours, theirs)
    assert update(repo, tmp_path, reviewed)["paths"] == [DOC_PATH]  # no git conflict, but 121 lines would fail Ci


def test_a_crate_doc_that_loses_a_heading_order_stays_a_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    ours = DOC.replace("- what it owns\n", "- what it owns\n\n## Purpose\n")  # a second Purpose heading breaks the order
    reviewed, _ = two_sides(repo, DOC_PATH, DOC, ours, DOC.replace("- what it owns\n", "- what it owns\n\n## Notes\n"))
    assert update(repo, tmp_path, reviewed)["paths"] == [DOC_PATH]


def test_without_the_code_docs_check_a_crate_doc_conflict_stays_a_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    ours, theirs = DOC.replace("- what it owns\n", "- what it owns\n- branch\n"), DOC.replace("- what it owns\n", "- what it owns\n- main\n")
    reviewed, _ = two_sides(repo, DOC_PATH, DOC, ours, theirs)
    assert env_on(repo, tmp_path).update_branch(BRANCH, reviewed)["paths"] == [DOC_PATH]


# ---- the crate api.rs


@pytest.mark.skipif(not RUSTFMT and not os.environ.get("REQUIRE_RUSTFMT"), reason="rustfmt is not installed; the fake-runner tests below cover the rule")
def test_api_reexports_both_sides_added_in_rustfmt_order_are_resolved(repo: dict[str, Path], tmp_path: Path) -> None:
    ours = API.replace("pub use crate::a::A;\n", "pub use crate::a::A;\npub use crate::bb::Bb;\n")
    theirs = API.replace("pub use crate::a::A;\n", "pub use crate::a::A;\npub use crate::b::{B, B1 as B2};\n")
    reviewed, main2 = two_sides(repo, API_PATH, API, ours, theirs)
    done = update(repo, tmp_path, reviewed)
    assert done["status"] == "pushed" and parents(repo["b"], str(done["sha"])) == [reviewed, main2]
    merged = git(repo["b"], "show", f"{done['sha']}:{API_PATH}")
    assert "pub use crate::a::A;\npub use crate::b::{B, B1 as B2};\npub use crate::bb::Bb;\npub use crate::c::C;" in merged


@pytest.mark.skipif(not RUSTFMT and not os.environ.get("REQUIRE_RUSTFMT"), reason="rustfmt is not installed; the fake-runner tests below cover the rule")
def test_api_reexports_out_of_rustfmt_order_stay_a_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    ours = API.replace("pub use crate::a::A;\n", "pub use crate::a::A;\npub use crate::b::B;\n")
    theirs = API.replace("pub use crate::a::A;\n", "pub use crate::a::A;\npub use crate::bz::Bz;\n")
    reviewed, _ = two_sides(repo, API_PATH, API, ours, theirs)  # main's `bz` first, then the branch's `b`: cargo fmt would fail
    assert update(repo, tmp_path, reviewed)["paths"] == [API_PATH]


def test_api_additions_that_are_not_reexports_stay_a_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    ours = API.replace("/// Does nothing.\n", "/// Branch.\npub fn g() {}\n\n/// Does nothing.\n")
    theirs = API.replace("/// Does nothing.\n", "/// Main.\npub fn h() {}\n\n/// Does nothing.\n")
    reviewed, _ = two_sides(repo, API_PATH, API, ours, theirs)
    assert update(repo, tmp_path, reviewed)["paths"] == [API_PATH]
    assert git(repo["origin"], "rev-parse", BRANCH) == reviewed


def test_api_one_side_edited_a_line_stays_a_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    ours = API.replace("pub use crate::a::A;\n", "pub use crate::a::{A, A2};\npub use crate::b::B;\n")
    theirs = API.replace("pub use crate::a::A;\n", "pub use crate::a::A;\npub use crate::bb::Bb;\n")
    reviewed, _ = two_sides(repo, API_PATH, API, ours, theirs)
    assert update(repo, tmp_path, reviewed)["paths"] == [API_PATH]


def test_reexport_lines_and_their_names() -> None:
    assert exported("pub use crate::a::B;") == ["B"] and exported("pub mod x;") == ["x"]
    assert exported("pub use crate::a::{B, c::D as E};") == ["B", "E"] and exported("pub use crate::a::*;") == []
    assert exported("pub use crate::a::B as C;") == ["C"] and exported("pub use crate::m::{self, B};") == ["m", "B"]
    assert not reexports_only(["pub mod m;\n", "pub use crate::m::{self, X};\n"])  # `m` twice
    assert reexports_only(["pub use crate::a::B;\n", "pub mod x;\n", "pub use crate::c::{D, E};\n"])
    assert not reexports_only([])  # nothing inserted is not a re-export conflict
    assert not reexports_only(["pub use crate::a::B;\n", "\n"])  # a blank line would start a new rustfmt group
    assert not reexports_only(["pub use crate::a::{\n"])  # a multi-line use: only one-line re-exports
    for line in ("use crate::a::B;\n", "pub(crate) use crate::a::B;\n", "pub fn f() {}\n", "#[cfg(test)]\n", "// pub use x::Y;\n"):
        assert not reexports_only([line]), line
    assert not reexports_only(["pub use crate::a::X;\n", "pub use crate::b::X;\n"])  # X twice would not compile
    assert not reexports_only(["pub use crate::a::X;\n", "pub use crate::b::{Y, Z as X};\n"])


class FakeRun:
    """A command runner for UpdateMerge: rustfmt answers as told; nothing else is called in these tests."""

    def __init__(self, answer: tuple[int, str | None] | None) -> None:
        self.answer: tuple[int, str | None] | None = answer
        self.calls: list[list[str]] = []

    def __call__(self, args: list[str], stdin: str | None, cwd: Path) -> tuple[int, str, str]:
        self.calls.append(args)
        if self.answer is None:
            raise FileNotFoundError("rustfmt")
        code, out = self.answer
        return code, stdin if out is None else out, ""


def test_the_api_check_needs_rustfmt_to_leave_the_file_unchanged(tmp_path: Path) -> None:
    (tmp_path / "Cargo.toml").write_text('[workspace.package]\nedition = "2021"\n')
    lines = ["pub use crate::b::B;\n"]
    same = UpdateMerge(FakeRun((0, None)), tmp_path, dict)
    assert same._crate_ok(API_PATH, API, lines) is True
    assert same._run.calls == [["rustfmt", "--edition", "2021", "--emit", "stdout"]]  # type: ignore[attr-defined]  # reason: the fake records calls
    assert UpdateMerge(FakeRun((0, API + "\n")), tmp_path, dict)._crate_ok(API_PATH, API, lines) is False  # rustfmt would change it
    assert UpdateMerge(FakeRun((1, None)), tmp_path, dict)._crate_ok(API_PATH, API, lines) is False  # rustfmt refused it
    assert UpdateMerge(FakeRun(None), tmp_path, dict)._crate_ok(API_PATH, API, lines) is False  # no rustfmt at all
    never = UpdateMerge(FakeRun((0, None)), tmp_path, dict)
    assert never._crate_ok(API_PATH, API, ["pub fn g() {}\n"]) is False and never._run.calls == []  # type: ignore[attr-defined]  # reason: as above
    assert UpdateMerge(FakeRun((0, None)), tmp_path / "none", dict)._edition() == "2024"  # no Cargo.toml: the workspace's edition


# ---- review round 1 (D-3269): a table row across the join, and a line both sides added


def test_a_table_row_never_meets_a_non_row_across_the_join(tmp_path: Path) -> None:
    from runner.update_merge import insertions_after_main  # noqa: PLC0415  # reason: beside its sibling helpers
    from test_runner_update_merge import _ours_base_main  # noqa: PLC0415  # reason: the diff3 helper of the D-891 tests
    base = "# Notes\n\nText.\n"
    table = base + "\n| a | b |\n|---|---|\n| 1 | 2 |\n"
    assert insertions_after_main(_ours_base_main(tmp_path, base, base + "\nMore text.\n", table)) is None  # main ends in a row
    assert insertions_after_main(_ours_base_main(tmp_path, base, base + "| 3 | 4 |\n", base + "\nMore text.\n")) is None
    rows = insertions_after_main(_ours_base_main(tmp_path, table, table + "| 5 | 6 |\n", table + "| 3 | 4 |\n"))
    assert rows == table + "| 3 | 4 |\n| 5 | 6 |\n"  # rows after rows stay one table


def test_a_line_both_sides_added_to_a_crate_doc_stays_a_conflict(repo: dict[str, Path], tmp_path: Path) -> None:
    ours = DOC.replace("- what it owns\n", "- what it owns\n- shared\n- branch\n")
    theirs = DOC.replace("- what it owns\n", "- what it owns\n- shared\n- main\n")
    reviewed, _ = two_sides(repo, DOC_PATH, DOC, ours, theirs)
    assert update(repo, tmp_path, reviewed)["paths"] == [DOC_PATH]
    from runner.update_merge import insertions_after_main  # noqa: PLC0415  # reason: the rule for other files is unchanged
    from test_runner_update_merge import _ours_base_main  # noqa: PLC0415  # reason: as above
    review = "# Review\n\n## One\n\n- Nothing needs you now.\n"
    merged = insertions_after_main(_ours_base_main(tmp_path, review, review + "\n## Two\n\n- Nothing needs you now.\n",
                                                   review + "\n## Three\n\n- Nothing needs you now.\n"))
    assert merged is not None and merged.count("- Nothing needs you now.") == 3  # for-casey-review items repeat a line by design
