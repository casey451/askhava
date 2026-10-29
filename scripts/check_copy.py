"""Measure customer copy against the copy panel's plain-language bar (docs/copy/copy-panel.md, WORD-029, D-474).

What a script can measure; the copy panel reviewer judges the rest. For each block of copy:
- every sentence has 20 words or fewer;
- the reading grade (Flesch-Kincaid) is 8 or lower for homeowners and 9 or lower for businesses,
  for blocks of 20 words or more;
- no superlative or unprovable claim word (best, fastest, cheapest, #1, guaranteed ...).
Blocks whose wording the spec fixes for legal or safety reasons are marked "fixed" and skipped.

It reads:
- docs/copy/site-copy.md: each "## <page> · <block> (homeowner|business[, fixed])" heading,
  followed by its copy on lines starting with "> ";
- once they exist, page templates under app/*/templates/ (visible text only); a path containing
  "business" is business copy.

Usage: python3 scripts/check_copy.py [--root REPO_ROOT]
Exit status 1 lists each problem.
"""

from __future__ import annotations

import argparse
import html
import re
import sys
from dataclasses import dataclass
from pathlib import Path

SITE_COPY = Path("docs/copy/site-copy.md")
MAX_WORDS = 20
MAX_GRADE = {"homeowner": 8.0, "business": 9.0}
MIN_WORDS_FOR_GRADE = 20
SUPERLATIVES = re.compile(
    r"\b(best|fastest|cheapest|easiest|greatest|leading|#1|number one|top-rated|guaranteed?|"
    r"most trusted|lowest prices?|unbeatable|perfect)\b",
    re.IGNORECASE,
)
HEADING = re.compile(r"^## (?P<name>.+?) \((?P<tags>[a-z, ]+)\)\s*$")
SENTENCE_END = re.compile(r"(?<=[.?!])\s+(?=[A-Z0-9\"“(])")
WORD = re.compile(r"[A-Za-z0-9$][A-Za-z0-9$'’,.-]*")


@dataclass
class Block:
    where: str
    audience: str
    fixed: bool
    text: str


def syllables(word: str) -> int:
    """A plain-English syllable estimate (vowel groups, a silent final e dropped)."""
    w = re.sub(r"[^a-z]", "", word.lower())
    if not w:
        return 1
    count = len(re.findall(r"[aeiouy]+", w))
    if w.endswith("e") and not w.endswith(("le", "ee")) and count > 1:
        count -= 1
    return max(count, 1)


def sentences(text: str) -> list[str]:
    return [s.strip() for s in SENTENCE_END.split(text.strip()) if s.strip()]


def words(text: str) -> list[str]:
    return WORD.findall(text)


def grade(text: str) -> float:
    """Flesch-Kincaid grade level."""
    sents = sentences(text) or [text]
    ws = words(text)
    if not ws:
        return 0.0
    syl = sum(syllables(w) for w in ws)
    return 0.39 * len(ws) / len(sents) + 11.8 * syl / len(ws) - 15.59


def check_block(block: Block) -> list[str]:
    if block.fixed:
        return []
    problems: list[str] = []
    for sent in sentences(block.text):
        n = len(words(sent))
        if n > MAX_WORDS:
            problems.append(f"{block.where}: {n}-word sentence (limit {MAX_WORDS}): {sent[:70]}")
    for match in sorted({m.group(0).lower() for m in SUPERLATIVES.finditer(block.text)}):
        problems.append(f"{block.where}: claim word '{match}' (say the fact instead)")
    if len(words(block.text)) >= MIN_WORDS_FOR_GRADE:
        level = grade(block.text)
        limit = MAX_GRADE[block.audience]
        if level > limit:
            problems.append(f"{block.where}: reading grade {level:.1f} (limit {limit:.0f} for {block.audience}s)")
    return problems


def site_copy_blocks(root: Path) -> list[Block]:
    path = root / SITE_COPY
    if not path.is_file():
        return []
    blocks: list[Block] = []
    current: Block | None = None
    for line in path.read_text(encoding="utf-8").splitlines():
        head = HEADING.match(line)
        if head:
            tags = [t.strip() for t in head.group("tags").split(",")]
            audience = "business" if "business" in tags else "homeowner"
            current = Block(f"{SITE_COPY} · {head.group('name')}", audience, "fixed" in tags, "")
            blocks.append(current)
        elif line.startswith("> ") and current is not None:
            current.text = f"{current.text} {line[2:].strip()}".strip()
        elif line.startswith("## "):
            current = None
    return blocks


def visible_text(markup: str) -> str:
    markup = re.sub(r"(?is)<(script|style)\b.*?</\1>", " ", markup)
    markup = re.sub(r"\{[{%].*?[%}]\}", " ", markup)
    return html.unescape(re.sub(r"<[^>]+>", "\n", markup))


def template_blocks(root: Path) -> list[Block]:
    blocks: list[Block] = []
    app = root / "app"
    if not app.is_dir():
        return blocks
    for path in sorted(app.glob("*/templates/**/*.html")):
        rel = path.relative_to(root).as_posix()
        audience = "business" if "business" in rel else "homeowner"
        for i, para in enumerate(p.strip() for p in visible_text(path.read_text(encoding="utf-8")).split("\n")):
            if para:
                blocks.append(Block(f"{rel} · text {i}", audience, False, " ".join(para.split())))
    return blocks


def run(root: Path) -> list[str]:
    problems: list[str] = []
    for block in site_copy_blocks(root) + template_blocks(root):
        problems += check_block(block)
    return problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args(argv)
    problems = run(args.root)
    for problem in problems:
        print(problem)
    print(f"check_copy: {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
