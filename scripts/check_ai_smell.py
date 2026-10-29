"""Fail on AI smell in customer-facing words and pages (WORD-027, D-452).

Reads the word, pattern and design lists from docs/ai_smell.yaml and scans:
- the quoted customer copy in docs/spec/*.md (text before " Source:", skipping exempt requirements)
- the quoted copy in docs/voice.md and every string in docs/catalog/question_bank.yaml
- the visible text and markup of docs/design/reference/*.html
- once the app exists: app/**/templates/**, app/**/*.txt, app/**/messages/*.yaml and app/**/*.css

Each hit prints as path:line: rule: "snippet" with the fix. Exit 1 if there is any hit.

Usage: python3 scripts/check_ai_smell.py [--root REPO_ROOT] [PATH ...]
"""

from __future__ import annotations

import argparse
import html
import re
import sys
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import yaml

LIST_FILE = Path("docs/ai_smell.yaml")
ROW = re.compile(r"^- \*\*([A-Z0-9]+-\d+)\*\* (.*)$")
QUOTED = re.compile(r"\"([^\"\n]{3,})\"|“([^”\n]{3,})”")
TAG = re.compile(r"<[^>]+>")
BLOCK = re.compile(r"<(style|script)\b.*?</\1>", re.S | re.I)
TEMPLATE_TAG = re.compile(r"\{%.*?%\}|\{\{.*?\}\}|\{#.*?#\}", re.S)
TS_STRING = re.compile(r"\"((?:[^\"\\\n]|\\.)*)\"|'((?:[^'\\\n]|\\.)*)'|`([^`]*)`")
APP_GLOBS = ("app/**/templates/**/*", "app/**/*.txt", "app/**/messages/*.yaml", "app/**/*.css", "app/**/*.ts")
MARKUP = (".html", ".css", ".j2", ".jinja")


@dataclass(frozen=True)
class Rule:
    name: str
    regex: re.Pattern[str]
    fix: str
    allow_in: tuple[str, ...] = ()


@dataclass(frozen=True)
class Hit:
    path: str
    line: int
    rule: str
    snippet: str
    fix: str

    def show(self) -> str:
        return f'{self.path}:{self.line}: {self.rule}: "{self.snippet}" ({self.fix})'


@dataclass(frozen=True)
class Lists:
    text_rules: tuple[Rule, ...]
    design_rules: tuple[Rule, ...]
    exempt: frozenset[str]
    not_copy: frozenset[str]
    allowed: tuple[str, ...]


def word_rule(word: str) -> Rule:
    """A listed word or phrase, matched at word boundaries with common endings."""
    body = re.escape(word).replace(r"\ ", r"\s+")
    regex = re.compile(rf"(?<![\w-]){body}(s|d|ed|ing|ly|es)?(?![\w-])", re.I)
    return Rule(f"word '{word}'", regex, "Say it plainly (docs/voice.md).")


def pattern_rule(item: dict[str, Any]) -> Rule:
    allow = tuple(item.get("allow_in", ()))
    return Rule(item["name"], re.compile(item["regex"], re.I | re.M), item["fix"], allow)


def load_lists(root: Path) -> Lists:
    data = yaml.safe_load((root / LIST_FILE).read_text(encoding="utf-8"))
    text = [word_rule(w) for w in data["words"]] + [pattern_rule(p) for p in data["patterns"]]
    design = [pattern_rule(p) for p in data["design_patterns"]]
    allowed = tuple(a["phrase"] for a in data.get("allowed_phrases", []))
    exempt = frozenset(data["exempt_requirements"])
    not_copy = frozenset(data["not_copy_requirements"])
    return Lists(tuple(text), tuple(design), exempt, not_copy, allowed)


def scan_text(lists: Lists, path: str, line: int, text: str) -> Iterator[Hit]:
    """Every text rule that matches one piece of customer copy."""
    for phrase in lists.allowed:
        text = text.replace(phrase, " ")
    for rule in lists.text_rules:
        match = rule.regex.search(text)
        if match:
            yield Hit(path, line, rule.name, match.group(0).strip()[:60], rule.fix)


def quoted(text: str) -> Iterator[str]:
    for match in QUOTED.finditer(text):
        yield match.group(1) or match.group(2)


def spec_copy(lists: Lists, path: Path, rel: str) -> Iterator[Hit]:
    """Quoted copy in spec rows, before the Source field; exempt rows skipped."""
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        row = ROW.match(raw)
        if not row or row.group(1) in lists.exempt | lists.not_copy:
            continue
        body = row.group(2).split(" Source:", 1)[0]
        for piece in quoted(body):
            yield from scan_text(lists, rel, number, piece)


def voice_copy(lists: Lists, path: Path, rel: str) -> Iterator[Hit]:
    """Quoted copy in the voice guide's table rows (the examples customers would see)."""
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if raw.startswith("|"):
            for piece in quoted(raw.rsplit("|", 2)[-2]):
                yield from scan_text(lists, rel, number, piece)


def strings_in(node: Any) -> Iterator[str]:
    if isinstance(node, str):
        yield node
    elif isinstance(node, dict):
        for value in node.values():
            yield from strings_in(value)
    elif isinstance(node, list):
        for value in node:
            yield from strings_in(value)


def yaml_copy(lists: Lists, path: Path, rel: str) -> Iterator[Hit]:
    """Every string value in a YAML file of customer copy (line numbers by first appearance)."""
    source = path.read_text(encoding="utf-8")
    for value in strings_in(yaml.safe_load(source)):
        if " " in value:
            line = next((i for i, raw in enumerate(source.splitlines(), 1) if value[:30] in raw), 0)
            yield from scan_text(lists, rel, line, value)


def visible_lines(source: str) -> Iterator[tuple[int, str]]:
    """The text a reader sees in a page, by source line."""
    keep_lines = lambda m: " " + "\n" * m.group(0).count("\n")  # noqa: E731
    blanked = TAG.sub(keep_lines, BLOCK.sub(keep_lines, TEMPLATE_TAG.sub(keep_lines, source)))
    for number, raw in enumerate(blanked.splitlines(), 1):
        text = re.sub(r"\s+", " ", html.unescape(raw)).strip()
        if text:
            yield number, text


def markup_file(lists: Lists, path: Path, rel: str) -> Iterator[Hit]:
    """Visible text against the text rules; the whole file against the design rules."""
    source = path.read_text(encoding="utf-8")
    if path.suffix != ".css":
        for number, text in visible_lines(source):
            yield from scan_text(lists, rel, number, text)
    for rule in lists.design_rules:
        if any(rel.startswith(prefix) for prefix in rule.allow_in):
            continue
        for number, raw in enumerate(source.splitlines(), 1):
            match = rule.regex.search(raw)
            if match:
                yield Hit(rel, number, rule.name, match.group(0)[:60], rule.fix)


def script_file(lists: Lists, path: Path, rel: str) -> Iterator[Hit]:
    """Only the string literals in a TypeScript file are words a person might read."""
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        for match in TS_STRING.finditer(raw):
            text = next(g for g in match.groups() if g is not None)
            if " " in text:
                yield from scan_text(lists, rel, number, text)


def plain_file(lists: Lists, path: Path, rel: str) -> Iterator[Hit]:
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        yield from scan_text(lists, rel, number, raw)


def scan_file(lists: Lists, root: Path, path: Path) -> Iterator[Hit]:
    rel = path.relative_to(root).as_posix() if path.is_relative_to(root) else path.as_posix()
    if rel.startswith("docs/spec/") and path.suffix == ".md":
        yield from spec_copy(lists, path, rel)
    elif rel == "docs/voice.md":
        yield from voice_copy(lists, path, rel)
    elif path.suffix == ".yaml":
        yield from yaml_copy(lists, path, rel)
    elif path.suffix == ".ts":
        yield from script_file(lists, path, rel)
    elif path.suffix in MARKUP:
        yield from markup_file(lists, path, rel)
    else:
        yield from plain_file(lists, path, rel)


def default_paths(root: Path) -> list[Path]:
    paths = sorted((root / "docs/spec").glob("*.md"))
    paths += [root / "docs/voice.md", root / "docs/catalog/question_bank.yaml"]
    paths += sorted((root / "docs/design/reference").glob("*.html"))
    for pattern in APP_GLOBS:
        paths += sorted(p for p in root.glob(pattern) if p.is_file())
    return [p for p in paths if p.is_file()]


def run(root: Path, paths: list[Path]) -> list[Hit]:
    lists = load_lists(root)
    hits: list[Hit] = []
    for path in paths or default_paths(root):
        hits.extend(scan_file(lists, root, path.resolve()))
    return hits


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("paths", nargs="*", type=Path)
    args = parser.parse_args(argv)
    root = args.root.resolve()
    hits = run(root, [root / p if not p.is_absolute() else p for p in args.paths])
    for hit in hits:
        print(hit.show())
    print(f"AI smell: {len(hits)} hit(s)")
    return 1 if hits else 0


if __name__ == "__main__":
    sys.exit(main())
