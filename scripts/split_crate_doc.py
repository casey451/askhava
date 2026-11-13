"""Split each crate's CLAUDE.md into a small guide and a greppable NOTES.md (D-3474).

Claude Code loads a crate's CLAUDE.md on its own whenever a session reads a file in that crate, so
every byte there is paid by every session in the crate. This tool moves the detail out, mechanically
(no rewording), and checks that nothing was lost:

- app/<crate>/CLAUDE.md keeps the title, the first sentence of `## Purpose`, the `## Never break`
  list (a long item keeps its first sentences and points to the rest) and a `## Detail` pointer, under
  scripts/check_code_docs.py's byte budget (it aims at TARGET_BYTES, leaving room to grow);
- app/<crate>/NOTES.md gets everything else, one entry per line: `- [REQ-IDS] (section) text`, the
  requirement IDs the text names (or the IDs of the item it continues, or `-`), the old heading as a
  short tag and at most CHUNK characters of the original words. Long items are cut at sentence ends
  and "; " (then ", ", then spaces), never inside a word.

Usage:
  python3 scripts/split_crate_doc.py [app/payments ...]   # split those crates (none: all) in place;
      a crate already split (its CLAUDE.md has `## Detail`) is left alone
  python3 scripts/split_crate_doc.py --sweep [app/...]   # move entry lines written under a split
      CLAUDE.md's `## Detail` paragraph (before NOTES.md was a shared path, T437) to the end of NOTES.md
  python3 scripts/split_crate_doc.py --verify origin/main [app/...]   # every word and sentence of each
      crate's CLAUDE.md at that git ref is still in its CLAUDE.md or NOTES.md (no crates: all of them)
Exit status 1 names each crate that fails.
"""

from __future__ import annotations

import argparse
import importlib.util
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path
from typing import Callable

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_code_docs as ccd  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
CHUNK = 400  # characters of original text per NOTES.md entry (D-3474)
TARGET_BYTES = 7_000  # what a split aims at, under ccd.MAX_CRATE_DOC_BYTES, so tasks can still add invariants
TAGS = {"Purpose": "purpose", "What it owns": "owns", "Rules it enforces": "rules", "Never break": "never",
        "How to change": "change", "Tests that guard it": "tests"}
CUTS = (re.compile(r"(?<=[.;]) (?=\S)"), re.compile(r"(?<=,) (?=\S)"), re.compile(r" (?=\S)"))
MORE = re.compile(r" More: `\(never \d+\)` in NOTES\.md\.$")
ENTRY = re.compile(r"^- \[[^\]]*\] \([a-z]+(?: \d+)?\) ")


def pieces(text: str, level: int = 0) -> list[str]:
    """`text` cut at sentence ends and "; ", each piece over CHUNK cut again at ", " then at spaces."""
    out: list[str] = []
    for part in CUTS[level].split(text):
        out += pieces(part, level + 1) if len(part) > CHUNK and level + 1 < len(CUTS) else [part]
    return out


def chunks(text: str) -> list[str]:
    """`text` packed into runs of whole pieces of at most CHUNK characters; " ".join gives `text` back."""
    out: list[str] = []
    for piece in pieces(text):
        if out and len(out[-1]) + 1 + len(piece) <= CHUNK:
            out[-1] += " " + piece
        else:
            out.append(piece)
    assert " ".join(out) == text, "a split changed the text"
    return out


def ids_in(text: str, known: set[str]) -> list[str]:
    """The requirement IDs `text` names that docs/spec/ defines, in order, once each."""
    return list(dict.fromkeys(i for i in ccd.REQ_REF.findall(text) if i in known))


def entries(tag: str, text: str, known: set[str]) -> list[str]:
    """NOTES.md lines for one original item: each chunk with its IDs, else the item's first IDs."""
    lines, lead = [], ids_in(text, known)[:4]
    for chunk in chunks(text):
        ids = ids_in(chunk, known) or lead
        lines.append(f"- [{' '.join(ids) or '-'}] ({tag}) {chunk}")
    return lines


def sections(text: str) -> tuple[str, list[tuple[str, list[str]]]]:
    """(the title line, [(heading, its non-blank lines)]) of an old-format crate CLAUDE.md."""
    title, out = "", []
    for line in text.splitlines():
        if line.startswith("## "):
            out.append((line[3:].strip(), []))
        elif out and line.strip():
            out[-1][1].append(line[2:] if line.startswith("- ") else line.strip())
        elif line.startswith("# ") and not title:
            title = line
    return title, out


def detail(crate: str) -> str:
    return (f"## Detail\nEverything else about this crate (what it owns, the code behind each requirement ID, "
            f"change recipes, the tests that guard it, decisions and history) is in `app/{crate}/NOTES.md`, one "
            f"entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word "
            f"(`grep -n 'FEE-039' app/{crate}/NOTES.md`); never read it whole. Add your own entries at its end, "
            f"one line each (`docs/code-docs.md` section 2).\n")


def brand_guard() -> Callable[[str], bool]:
    """Whether a line holds a brand literal, by the brand guard's own rule (tests/docs/test_brand_literals.py,
    D-1933). That guard scans every app/ file except a CLAUDE.md, so an entry naming the brand stays at the
    end of the guide, after `## Detail`, until T437 exempts NOTES.md; `--sweep` moves it then."""
    path = ROOT / "tests" / "docs" / "test_brand_literals.py"
    spec = importlib.util.spec_from_file_location("brand_literal_guard", path)
    if spec is None or spec.loader is None:
        return lambda line: False
    guard = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(guard)
    pattern = guard.brand_pattern(guard.brand.load())
    return lambda line: any(not guard.is_identifier(line, m) for m in pattern.finditer(line))


def guide(title: str, purpose: str, never: list[str], crate: str) -> str:
    items = "".join(f"- {line}\n" for line in never)
    return f"{title}\n\n## Purpose\n{purpose}\n\n## Never break\n{items}\n{detail(crate)}"


def fit_never(items: list[list[str]], fixed: int) -> list[int]:
    """How many chunks of each Never-break item stay in CLAUDE.md: all, unless the guide would pass
    TARGET_BYTES; then the item with the most kept bytes gives up its last chunk, until it fits."""
    kept = [len(c) for c in items]
    def size() -> int:
        return fixed + sum(len(" ".join(c[:k]).encode()) + 40 for c, k in zip(items, kept))
    while size() > TARGET_BYTES:
        cands = [i for i, k in enumerate(kept) if k > 1]
        if not cands:
            raise SystemExit("Never break does not fit even at one sentence per item")
        worst = max(cands, key=lambda i: len(" ".join(items[i][:kept[i]])))
        kept[worst] -= 1
    return kept


def split(crate: str, text: str, known: set[str], hold: Callable[[str], bool] = lambda line: False) -> tuple[str, str]:
    """(new CLAUDE.md, new NOTES.md) for one crate's old-format CLAUDE.md; entries `hold` flags stay at the
    guide's end, after `## Detail` (see brand_guard)."""
    title, secs = sections(text)
    notes = [f"# {crate} notes (D-3474): one entry per line, `- [REQUIREMENT-IDS] (section) text`; "
             f"grep by ID, never read whole; add yours at the end, one line each."]
    purpose, never_items, never_lines = "", [], []
    for heading, units in secs:
        tag = TAGS[heading]
        for unit in units:
            if tag == "purpose" and not purpose:
                first, *rest = chunks(unit)
                purpose = first
                notes += [e for r in rest for e in entries(tag, r, known)]
            elif tag == "never":
                never_items.append(chunks(unit))
            else:
                notes += entries(tag, unit, known)
    fixed = len(guide(title, purpose, [], crate).encode())
    for n, (cs, k) in enumerate(zip(never_items, fit_never(never_items, fixed)), 1):
        head = " ".join(cs[:k])
        never_lines.append(head if k == len(cs) else f"{head} More: `(never {n})` in NOTES.md.")
        notes += [e for c in cs[k:] for e in entries(f"never {n}", c, known)]
    held = [n for n in notes[1:] if hold(n)]
    notes = [n for n in notes if n not in held]
    return guide(title, purpose, never_lines, crate) + "".join(f"{n}\n" for n in held), "\n".join(notes) + "\n"


def words_of_old(text: str) -> tuple[Counter[str], list[str]]:
    """(word counts, sentence pieces) of an old-format CLAUDE.md, headings and title left out."""
    _, secs = sections(text)
    units = [u for _, us in secs for u in us]
    return Counter(w for u in units for w in u.split()), [p for u in units for p in pieces(u)]


def text_of_new(guide_text: str, notes_text: str) -> str:
    """The original words a split guide and its NOTES.md hold, markup and pointers left out."""
    body, _, tail = guide_text.partition("\n## Detail\n")
    keep = [MORE.sub("", ln[2:] if ln.startswith("- ") else ln) for ln in body.splitlines()[1:] if not ln.startswith("## ")]
    keep += [ENTRY.sub("", ln) for ln in notes_text.splitlines()[1:] + tail.splitlines() if ENTRY.match(ln)]
    return "\n".join(keep)


def lost(old: str, guide_text: str, notes_text: str) -> list[str]:
    """Words or sentences of `old` that the split files no longer hold (empty when nothing was lost)."""
    have = text_of_new(guide_text, notes_text)
    counts, sentences = words_of_old(old)
    held = Counter(have.split())
    missing = [f"word {w!r} x{n - held[w]}" for w, n in counts.items() if held[w] < n]
    flat = " ".join(have.split())
    missing += [f"sentence {s[:60]!r}" for s in sentences if " ".join(s.split()) not in flat]
    return missing


def crates(names: list[str]) -> list[str]:
    return [Path(n).name for n in names] or sorted(p.parent.name for p in (ROOT / "app").glob("*/CLAUDE.md"))


def verify(ref: str, names: list[str]) -> int:
    bad = 0
    for crate in crates(names):
        old = subprocess.run(["git", "-C", str(ROOT), "show", f"{ref}:app/{crate}/CLAUDE.md"],
                             capture_output=True, text=True, check=True).stdout
        base = ROOT / "app" / crate
        gone = lost(old, (base / "CLAUDE.md").read_text(encoding="utf-8"), (base / "NOTES.md").read_text(encoding="utf-8"))
        bad += bool(gone)
        print(f"{crate}: {'LOST ' + '; '.join(gone[:5]) if gone else 'every word and sentence kept'}")
    return 1 if bad else 0


def sweep(names: list[str]) -> int:
    """Move each `- [` entry line after a split CLAUDE.md's `## Detail` paragraph to the end of NOTES.md."""
    for crate in crates(names):
        base = ROOT / "app" / crate
        head, sep, tail = (base / "CLAUDE.md").read_text(encoding="utf-8").partition("\n## Detail\n")
        moved = [ln for ln in tail.splitlines() if ENTRY.match(ln)]
        if not sep or not moved:
            continue
        kept = "\n".join(ln for ln in tail.splitlines() if not ENTRY.match(ln)).rstrip("\n") + "\n"
        notes = (base / "NOTES.md").read_text(encoding="utf-8")
        (base / "NOTES.md").write_text(notes + "".join(f"{ln}\n" for ln in moved), encoding="utf-8")
        (base / "CLAUDE.md").write_text(head + sep + kept, encoding="utf-8")
        print(f"{crate}: moved {len(moved)} entries to NOTES.md")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("crates", nargs="*", help="crate folders (app/<crate>); none with --verify means all")
    parser.add_argument("--verify", metavar="REF", help="check against each CLAUDE.md at this git ref; write nothing")
    parser.add_argument("--sweep", action="store_true", help="move entry lines under `## Detail` to NOTES.md")
    args = parser.parse_args(argv)
    if args.sweep:
        return sweep(args.crates)
    if args.verify:
        return verify(args.verify, args.crates)
    hold = brand_guard()
    known = ccd.spec_ids(ROOT)
    for crate in crates(args.crates):
        base = ROOT / "app" / crate
        old = (base / "CLAUDE.md").read_text(encoding="utf-8")
        if "\n## Detail\n" in old:
            print(f"{crate}: already split, left alone")
            continue
        new_guide, notes = split(crate, old, known, hold)
        if lost(old, new_guide, notes):
            print(f"{crate}: refused, the split would lose words")
            return 1
        (base / "CLAUDE.md").write_text(new_guide, encoding="utf-8")
        (base / "NOTES.md").write_text(notes, encoding="utf-8")
        print(f"{crate}: CLAUDE.md {len(old.encode())} -> {len(new_guide.encode())} bytes, NOTES.md {len(notes.splitlines()) - 1} entries")
    return 0


if __name__ == "__main__":
    sys.exit(main())
