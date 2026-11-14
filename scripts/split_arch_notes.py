"""Move the build notes out of docs/architecture.md into the crates' NOTES.md files (D-3620).

docs/architecture.md describes the design; for a long time every task also appended a "What T### built"
paragraph to section 7 or 10, until those paragraphs were most of the file (about 290 KB of 487 KB). A worker
whose prompt cites one of those sections carries them on every turn (scripts/gen_prompts.py counts a cited
section whole, D-1350), and every task appending to one shared section made merge conflicts. This tool moves
them, mechanically (no rewording), the way scripts/split_crate_doc.py moved crate detail (D-3474):

- each build-note paragraph of sections 7 and 10 (one starting `**What T### built`, `**What T### changed`,
  `**What K# built`, `**T###'s review fixes` or `**Stage-N cleanup (T###`) goes to the end of the NOTES.md of
  the crate it is mostly about (route), cut into entries of at most CHUNK characters at sentence ends and
  "; " (then ", ", then spaces), each `- [REQ-IDS] (task N) text`, so `grep -n '(task 250)'` prints every
  line of T250's note in order; K8's tag is `(kit 8)`;
- a paragraph naming no crate, or one an app/ file may not hold (held_back: a brand literal, or a word shaped
  like a requirement ID that docs/spec/ does not define), goes to docs/build-notes.md in the same form, with a
  cross-reference entry in the crate it is about;
- every other crate the paragraph names strongly (an `app/<crate>/` path, `<crate>::` or an `askhava_<crate>`
  or `NNNN_<crate>_` name) gets one cross-reference entry (XREF) pointing at where the note went;
- each section keeps a short stub (STUB) in place of its notes, saying where they are and how to grep them.

Usage:
  python3 scripts/split_arch_notes.py               # move the notes (refuses when none are left); run it
      again after a merge brings a task's late "What T### built" paragraph back, and it moves that one too
  python3 scripts/split_arch_notes.py --verify REF  # prove the move from docs/architecture.md at REF: the
      file holds, in order, every line of REF's with each section's notes replaced by its stub (lines may be added), every crate NOTES.md starts with REF's,
      and every note's entries appear, whole and in order, exactly once across the destinations
  python3 scripts/split_arch_notes.py --check       # report each build note left in docs/architecture.md; exit 0
      (the transition: tasks claimed before D-3620 may still bring one in on their update merge, and the
      operator sweeps them with a plan pull request that runs this tool)
  python3 scripts/split_arch_notes.py --check --strict   # the same, exit 1 on any (a later plan change turns
      this on once every task whose attempt started before D-3620 merged is DONE or SKIPPED)
Exit status 1 names each problem.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path
from typing import Callable

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_code_docs as ccd  # noqa: E402
import split_crate_doc as scd  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
ARCHITECTURE = "docs/architecture.md"
BUILD_NOTES = "docs/build-notes.md"
SECTIONS = (7, 10)
# A build-note paragraph: what one task built or changed, its review fixes or a stage cleanup (D-3620).
NOTE = re.compile(r"^\*\*(?:What (?P<kind>[TK])(?P<num>\d+) (?:built|changed)\b|T(?P<rev>\d+)'s review fixes\b"
                  r"|Stage-\d+ cleanup \(T(?P<stage>\d+)\b)")
ENTRY = re.compile(r"^- \[[^\]]*\] \((?P<tag>(?:task|kit) \d+)\) (?P<text>.*)$")
XREF = " cross-reference (D-3620): the build note of "
BUILD_NOTES_TITLE = ("# Build notes that name no single crate (D-3620): one entry per line, `- [REQUIREMENT-IDS] "
                     "(task N) text`; grep by task or ID (`grep -n '(task 250)' docs/build-notes.md`), never read "
                     "whole. Moved here from docs/architecture.md sections 7 and 10; new build notes go to the "
                     "crate's NOTES.md (kit/prompts/TEMPLATE.md).")
STUB = ("**Build notes** (D-3620). What each task built here, once a \"What T### built\" paragraph in this "
        "section, is one entry per line in the `NOTES.md` of the crate it is about (`docs/build-notes.md` when it "
        "names no crate), each line tagged with its task: `grep -n '(task 250)' app/*/NOTES.md docs/build-notes.md` "
        "prints every line of T250's note in order, and `grep -rn 'T250' app/*/NOTES.md docs/build-notes.md` every "
        "mention. A new build note goes at the end of its crate's `NOTES.md`, never here (section 12; "
        "`python3 scripts/split_arch_notes.py --check`).")


def tag_of(head: re.Match[str]) -> str:
    """`task 250` for T250's note, `kit 8` for K8's."""
    if head.group("kind") == "K":
        return f"kit {head.group('num')}"
    return f"task {head.group('num') or head.group('rev') or head.group('stage')}"


def task_name(tag: str) -> str:
    kind, num = tag.split()
    return f"{'K' if kind == 'kit' else 'T'}{num}"


def crate_names(root: Path) -> list[str]:
    return sorted(p.parent.name for p in (root / "app").glob("*/Cargo.toml"))


def strong_marks(crate: str) -> re.Pattern[str]:
    """Where a paragraph names a crate for sure: its folder, a path into it, its Rust or migration name."""
    return re.compile(rf"app/{crate}\b|(?<![\w:]){crate}::|askhava_{crate}\b|\b\d{{4}}_{crate}_")


def weak_marks(crate: str) -> re.Pattern[str]:
    """A crate named in backticks, or one of its schema's tables (`payments.charge`)."""
    return re.compile(rf"`{crate}`|`{crate}\.[a-z_]+")


def scores(text: str, crates: list[str]) -> dict[str, tuple[int, int, int]]:
    """Per named crate: (strong mentions, weak mentions, minus the first strong mention's offset)."""
    out: dict[str, tuple[int, int, int]] = {}
    for crate in crates:
        strong = [m.start() for m in strong_marks(crate).finditer(text)]
        weak = len(weak_marks(crate).findall(text))
        if strong or weak:
            out[crate] = (len(strong), weak, -(strong[0] if strong else len(text)))
    return out


def route(text: str, crates: list[str]) -> tuple[str | None, list[str]]:
    """(the crate a note goes to, or None for docs/build-notes.md; the other crates it names strongly).

    The crate with the most strong mentions wins, then the most weak ones, then the one named first; a note
    with no strong mention of any crate names no crate."""
    found = scores(text, crates)
    strong = [c for c, s in found.items() if s[0]]
    if not strong:
        return None, []
    best = max(strong, key=lambda c: found[c])
    return best, [c for c in sorted(strong) if c != best]


def note_entries(tag: str, text: str, known: set[str]) -> list[str]:
    """NOTES.md lines for one note: each chunk with the requirement IDs it names, else the note's first four."""
    lead = scd.ids_in(text, known)[:4]
    return [f"- [{' '.join(scd.ids_in(c, known) or lead) or '-'}] ({tag}) {c}" for c in scd.chunks(text)]


def xref_entry(tag: str, text: str, dest: str, known: set[str]) -> str:
    ids = " ".join(scd.ids_in(text, known)[:4]) or "-"
    return f"- [{ids}] ({tag}){XREF}{task_name(tag)} is in `{dest}`: `grep -n '({tag})' {dest}`."


def section_numbers(lines: list[str]) -> list[int]:
    """The `## N.` section each line is in (0 before the first)."""
    out, section = [], 0
    for line in lines:
        heading = re.match(r"^## (\d+)\. ", line)
        section = int(heading.group(1)) if heading else section
        out.append(section)
    return out


def notes_of(text: str) -> list[tuple[int, int, str, str]]:
    """Each build note in SECTIONS: (section, line index, tag, paragraph), in file order."""
    lines = text.split("\n")
    return [(sec, i, tag_of(head), line) for i, (sec, line) in enumerate(zip(section_numbers(lines), lines))
            if sec in SECTIONS and (head := NOTE.match(line))]


def strip_notes(text: str) -> str:
    """`text` with each section's notes (and the blank line after each) replaced by one STUB where the first was;
    a section that already has the STUB (a note merged in after the move) only loses its notes."""
    lines, notes = text.split("\n"), notes_of(text)
    drop = {i for _, i, _, _ in notes} | {i + 1 for _, i, _, _ in notes if i + 1 < len(lines) and not lines[i + 1]}
    stubbed = {sec for sec, line in zip(section_numbers(lines), lines) if line == STUB}
    first = {min(i for s, i, _, _ in notes if s == sec) for sec in {s for s, _, _, _ in notes} - stubbed}
    out: list[str] = []
    for index, line in enumerate(lines):
        if index in first:
            out += [STUB, ""]
        if index not in drop:
            out.append(line)
    return "\n".join(out)


def held_back(known: set[str], brand: Callable[[str], bool]) -> Callable[[str], bool]:
    """Whether an entry may not live under app/: it holds a brand literal (the brand guard scans every app/ file
    but a CLAUDE.md, until T437) or a word shaped like a requirement ID that docs/spec/ does not define
    (`SHA-256`, a job number), which scripts/check_code_docs.py refuses in a NOTES.md."""
    return lambda line: brand(line) or any(i not in known for i in ccd.REQ_REF.findall(line))


def plan(text: str, crates: list[str], known: set[str], hold: Callable[[str], bool]) -> dict[str, list[str]]:
    """{destination path: entry lines to append} for every note of `text`, in file order."""
    out: dict[str, list[str]] = {}
    for _, _, tag, para in notes_of(text):
        crate, others = route(para, crates)
        lines = note_entries(tag, para, known)
        held = any(hold(line) for line in lines)
        dest = BUILD_NOTES if crate is None or held else f"app/{crate}/NOTES.md"
        out.setdefault(dest, []).extend(lines)
        refs = ([crate] if held and crate else []) + others
        for other in refs:
            out.setdefault(f"app/{other}/NOTES.md", []).append(xref_entry(tag, para, dest, known))
    return out


def split(root: Path) -> int:
    arch = root / ARCHITECTURE
    text = arch.read_text(encoding="utf-8")
    if not notes_of(text):
        print("split_arch_notes: no build notes left in docs/architecture.md; nothing to move")
        return 1
    known = ccd.spec_ids(root)
    moves = plan(text, crate_names(root), known, held_back(known, scd.brand_guard()))
    for dest, lines in sorted(moves.items()):
        path = root / dest
        old = path.read_text(encoding="utf-8") if path.is_file() else BUILD_NOTES_TITLE + "\n"
        path.write_text(old + "".join(f"{ln}\n" for ln in lines), encoding="utf-8")
        print(f"{dest}: +{len(lines)} entries")
    arch.write_text(strip_notes(text), encoding="utf-8")
    return 0


def git_show(root: Path, ref: str, path: str) -> str | None:
    shown = subprocess.run(["git", "-C", str(root), "show", f"{ref}:{path}"], capture_output=True, text=True)
    return shown.stdout if shown.returncode == 0 else None


def runs(entries: list[tuple[str, str]], tag: str, want: list[str]) -> int:
    """How many times `want` appears as consecutive entries tagged `tag` (cross-references left out)."""
    size, run = len(want), [(tag, w) for w in want]
    return sum(1 for i in range(len(entries) - size + 1) if entries[i : i + size] == run)


def tagged(text: str) -> list[tuple[str, str]]:
    """(tag, text) of each `(task N)` or `(kit N)` entry that is not a cross-reference."""
    matches = (ENTRY.match(line) for line in text.splitlines())
    return [(m.group("tag"), m.group("text")) for m in matches if m and not m.group("text").startswith(XREF[1:])]


def first_missing(want: list[str], have: list[str]) -> int | None:
    """The index of the first line of `want` that `have` does not hold in the same order, or None."""
    rest = iter(have)
    for index, line in enumerate(want):
        if not any(line == other for other in rest):
            return index
    return None


def verify(root: Path, ref: str) -> list[str]:
    """Problems with the move from docs/architecture.md at `ref` (empty when it is lossless)."""
    old = git_show(root, ref, ARCHITECTURE)
    if old is None:
        return [f"{ARCHITECTURE} not found at {ref}"]
    problems = []
    missing = first_missing(strip_notes(old).split("\n"), (root / ARCHITECTURE).read_text(encoding="utf-8").split("\n"))
    if missing is not None:
        problems.append(f"{ARCHITECTURE}: line {missing + 1} of {ref}'s file with the build notes replaced by the stub "
                        "is lost or changed (only added lines are allowed)")
    dests = [f"app/{c}/NOTES.md" for c in crate_names(root)] + [BUILD_NOTES]
    have = {d: (root / d).read_text(encoding="utf-8") if (root / d).is_file() else "" for d in dests}
    for dest in dests:
        before = git_show(root, ref, dest)
        if before is not None and not have[dest].startswith(before):
            problems.append(f"{dest}: does not start with {ref}'s copy")
    entries = {d: tagged(t) for d, t in have.items()}
    notes = notes_of(old)
    for _, _, tag, para in notes:
        want = scd.chunks(para)
        found = sum(runs(e, tag, want) for e in entries.values())
        if found != 1:
            problems.append(f"{task_name(tag)}: its note appears {found} times across the destinations, not once")
    moved = sum(len(scd.chunks(p)) for _, _, _, p in notes)
    held = sum(len(e) for e in entries.values())
    if held != moved:
        problems.append(f"destinations hold {held} task-tagged entries, the moved notes make {moved}")
    return problems


def check(root: Path) -> list[str]:
    """A build-note paragraph anywhere in docs/architecture.md (D-3620): it belongs in its crate's NOTES.md."""
    path = root / ARCHITECTURE
    if not path.is_file():
        return []
    return [f"{ARCHITECTURE}:{n}: a build note ({line[:40]}...) belongs at the end of its crate's NOTES.md, one "
            f"entry per line (kit/prompts/TEMPLATE.md, D-3620)"
            for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1) if NOTE.match(line)]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=ROOT)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--verify", metavar="REF", help="prove the move from docs/architecture.md at REF; write nothing")
    mode.add_argument("--check", action="store_true", help="report each build note left in docs/architecture.md")
    parser.add_argument("--strict", action="store_true", help="with --check: exit 1 on any build note")
    args = parser.parse_args(argv)
    if not (args.verify or args.check):
        return split(args.root)
    problems = verify(args.root, args.verify) if args.verify else check(args.root)
    for problem in problems:
        print(problem)
    print(f"split_arch_notes: {len(problems)} problem(s)")
    return 1 if problems and (args.verify or args.strict) else 0


if __name__ == "__main__":
    sys.exit(main())
