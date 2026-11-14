"""Tests for D-3620: the build notes live in the crates' NOTES.md, never in docs/architecture.md.

Covers scripts/split_arch_notes.py: the check that reports a "What T### built" paragraph in
docs/architecture.md (run here on the real file in its report-only mode, the D-3620 transition; `--strict`
fails), the routing of a
note to its crate with cross-references, the hold-back to docs/build-notes.md, and the mechanical move with
its `--verify` proof. Fake data only.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import split_arch_notes as san  # noqa: E402

LONG = " ".join(f"Fact number {i} about fees (FEE-001)." for i in range(30))
ARCH = (
    "# Architecture\n\n## 2. Layout\nlayout\n\n## 7. Adapters and stand-ins\n\nAdapter table.\n\n"
    "**What T016 built (D-1).** `app/adapters`: the Twilio config; `payments::api` is called.\n\n"
    "**The runner's logic** stays.\n\n## 8. Vault\n\n**As built (T022, D-2):** stays in section 8.\n\n"
    "## 10. Off-database records and recovery\n\nWritten as they happen.\n\n"
    f"**What T250 built (D-3).** `app/payments/src/fees.rs` and `payments::api::fee` (FEE-001). {LONG}\n\n"
    "**What K8 built (D-4).** `stand_ins/` holds the fakes.\n\n"
    "**T063's review fixes (D-5).** `app/payments` names job SHA-256 (a word, not an ID).\n\n"
    "## 11. Pinned stack versions\n\nversions\n"
)


def fake_repo(tmp_path: Path) -> Path:
    (tmp_path / "docs/spec").mkdir(parents=True)
    (tmp_path / "docs/spec/fees.md").write_text("- **FEE-001** Tiers.\n", encoding="utf-8")
    (tmp_path / "docs/architecture.md").write_text(ARCH, encoding="utf-8")
    for crate in ("adapters", "payments"):
        (tmp_path / f"app/{crate}").mkdir(parents=True)
        (tmp_path / f"app/{crate}/Cargo.toml").write_text("[package]\n", encoding="utf-8")
        (tmp_path / f"app/{crate}/NOTES.md").write_text(f"# {crate} notes\n- [-] (owns) Old entry.\n", encoding="utf-8")
    git = ["git", "-C", str(tmp_path)]
    subprocess.run(git + ["init", "-q"], check=True)
    subprocess.run(git + ["add", "-A"], check=True)
    subprocess.run(git + ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-qm", "base"], check=True)
    return tmp_path


def test_real_architecture_build_notes_are_reported_not_failed(capsys) -> None:  # type: ignore[no-untyped-def]
    """D-3620's transition: a build note a task claimed before D-3620 brings into docs/architecture.md on its update
    merge is reported (and swept by the operator's plan pull request), never a failure, until a later plan change
    makes this test run `--check --strict` once every task whose attempt started before D-3620 merged is DONE or SKIPPED."""
    assert san.main(["--root", str(REPO_ROOT), "--check"]) == 0
    assert f"split_arch_notes: {len(san.check(REPO_ROOT))} problem(s)" in capsys.readouterr().out


def test_check_names_a_build_note_line(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    problems = san.check(root)
    assert len(problems) == 4 and problems[0].startswith("docs/architecture.md:10: a build note")


def test_check_passes_design_paragraphs_and_the_stub(tmp_path: Path) -> None:
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs/architecture.md").write_text(
        f"## 7. A\n\n{san.STUB}\n\n**As built (T022, D-2):** a design fact.\n\n**Built by T253 (D-9):** x.\n",
        encoding="utf-8")
    assert san.check(tmp_path) == []


def test_route_picks_the_most_named_crate_and_cross_references_the_rest() -> None:
    crates = ["adapters", "payments", "picks"]
    text = "`app/payments/src/a.rs`, `payments::api::x`, `app/adapters` and the `picks` row."
    assert san.route(text, crates) == ("payments", ["adapters"])
    assert san.route("Only `stand_ins/` and `tests/x.rs`.", crates) == (None, [])
    assert san.route("`app/picks/x` then `app/adapters/y`", crates)[0] == "picks"


def test_tag_numbers_tasks_and_kit_steps() -> None:
    assert san.tag_of(san.NOTE.match("**What T016 built (D-1).**")) == "task 016"
    assert san.tag_of(san.NOTE.match("**What K8 built (D-4).**")) == "kit 8"
    assert san.tag_of(san.NOTE.match("**T063's review fixes (D-5).**")) == "task 063"
    assert san.tag_of(san.NOTE.match("**Stage-3 cleanup (T075, D-1607).**")) == "task 075"
    assert san.NOTE.match("**As built (T022, D-2):**") is None


def test_split_moves_every_note_and_verify_proves_it(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    assert san.split(root) == 0
    arch = (root / "docs/architecture.md").read_text(encoding="utf-8")
    assert san.check(root) == [] and arch.count(san.STUB) == 2
    assert "**The runner's logic** stays." in arch and "**As built (T022, D-2):**" in arch
    payments = (root / "app/payments/NOTES.md").read_text(encoding="utf-8")
    assert payments.startswith("# payments notes\n- [-] (owns) Old entry.\n")
    assert payments.count("(task 250) ") > 2 and all(len(ln) <= 600 for ln in payments.splitlines())
    assert "[FEE-001] (task 250) **What T250 built (D-3).**" in payments
    adapters = (root / "app/adapters/NOTES.md").read_text(encoding="utf-8")
    assert "(task 016) **What T016 built" in adapters
    assert "(task 016) cross-reference (D-3620): the build note of T016 is in `app/adapters/NOTES.md`" in payments
    held = (root / "docs/build-notes.md").read_text(encoding="utf-8")
    assert "(kit 8) **What K8 built" in held and "(task 063) **T063's review fixes" in held
    assert "(task 063) cross-reference (D-3620): the build note of T063 is in `docs/build-notes.md`" in payments
    assert san.verify(root, "HEAD") == []
    assert san.split(root) == 1  # nothing left to move


def test_verify_finds_a_lost_or_doubled_entry(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    san.split(root)
    path = root / "app/payments/NOTES.md"
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
    moved = [i for i, ln in enumerate(lines) if "(task 250) " in ln]
    path.write_text("".join(lines[: moved[1]] + lines[moved[1] + 1 :]), encoding="utf-8")
    assert any("T250: its note appears 0 times" in p for p in san.verify(root, "HEAD"))
    path.write_text("".join(lines) + "".join(lines[moved[0] : moved[-1] + 1]), encoding="utf-8")
    problems = san.verify(root, "HEAD")
    assert any("T250: its note appears 2 times" in p for p in problems)


def test_verify_finds_a_changed_architecture(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    san.split(root)
    arch = root / "docs/architecture.md"
    arch.write_text(arch.read_text(encoding="utf-8").replace("versions", "other"), encoding="utf-8")
    assert any("is lost or changed" in p for p in san.verify(root, "HEAD"))
    arch.write_text(arch.read_text(encoding="utf-8").replace("other", "versions") + "- an added design bullet\n", encoding="utf-8")
    assert san.verify(root, "HEAD") == []


def test_main_check_exit_status(tmp_path: Path, capsys) -> None:  # type: ignore[no-untyped-def]
    root = fake_repo(tmp_path)
    assert san.main(["--root", str(root), "--check"]) == 0  # report-only: the D-3620 transition
    assert "4 problem(s)" in capsys.readouterr().out
    assert san.main(["--root", str(root), "--check", "--strict"]) == 1
    assert "4 problem(s)" in capsys.readouterr().out
    san.main(["--root", str(root)])
    assert san.main(["--root", str(root), "--check"]) == 0
    assert san.main(["--root", str(root), "--check", "--strict"]) == 0
    assert san.main(["--root", str(root), "--verify", "HEAD"]) == 0


def test_a_note_merged_in_after_the_move_moves_too(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    san.split(root)
    arch = root / "docs/architecture.md"
    late = "**What T449 built (D-6).** `app/payments/src/late.rs` counts (FEE-001).\n\n"
    arch.write_text(arch.read_text(encoding="utf-8").replace("## 11.", late + "## 11."), encoding="utf-8")
    assert len(san.check(root)) == 1
    assert san.split(root) == 0
    text = arch.read_text(encoding="utf-8")
    assert san.check(root) == [] and text.count(san.STUB) == 2 and "T449" not in text
    assert "[FEE-001] (task 449) **What T449 built (D-6).**" in (root / "app/payments/NOTES.md").read_text(encoding="utf-8")


def test_a_second_copy_of_a_moved_note_is_refused_and_fails_the_check(tmp_path: Path, capsys) -> None:  # type: ignore[no-untyped-def]
    """A resync that kept an old section 10 brings back notes already moved; sweeping would double them."""
    root = fake_repo(tmp_path)
    san.split(root)
    arch = root / "docs/architecture.md"
    copy = next(ln for ln in ARCH.splitlines() if ln.startswith("**What T250 built"))
    arch.write_text(arch.read_text(encoding="utf-8").replace("## 11.", copy + "\n\n## 11."), encoding="utf-8")
    before = {p: p.read_text(encoding="utf-8") for p in [arch, *sorted((root / "app").glob("*/NOTES.md"))]}
    assert san.split(root) == 1
    assert "T250's build note is already in the NOTES.md files" in capsys.readouterr().out
    assert all(p.read_text(encoding="utf-8") == text for p, text in before.items())
    assert san.main(["--root", str(root), "--check"]) == 1  # even without --strict
    assert "1 second copies" in capsys.readouterr().out


def test_a_late_note_is_not_a_second_copy(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    san.split(root)
    arch = root / "docs/architecture.md"
    late = "**What T449 built (D-6).** `app/payments/src/late.rs` counts (FEE-001).\n\n"
    arch.write_text(arch.read_text(encoding="utf-8").replace("## 11.", late + "## 11."), encoding="utf-8")
    assert san.second_copies(root, arch.read_text(encoding="utf-8")) == []
    assert san.main(["--root", str(root), "--check"]) == 0


def test_every_section_7_note_is_cross_referenced_from_adapters(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    arch = root / "docs/architecture.md"
    late = "**What T377 changed (D-7).** `app/payments/src/sms.rs` and `payments::api::send`.\n\n"
    arch.write_text(arch.read_text(encoding="utf-8").replace("**The runner's logic**", late + "**The runner's logic**"),
                    encoding="utf-8")
    san.split(root)
    adapters = (root / "app/adapters/NOTES.md").read_text(encoding="utf-8")
    assert "(task 377) cross-reference (D-3620): the build note of T377 is in `app/payments/NOTES.md`" in adapters
    assert "(kit 8) cross-reference" not in adapters  # K8 sits in section 10 here
    assert "(task 016) cross-reference" not in adapters  # T016's note is itself in adapters
