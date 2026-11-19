"""Measure customer copy against the copy panel's plain-language bar (docs/copy/copy-panel.md, WORD-029, D-474).

What a script can measure; the copy panel reviewer judges the rest. For each block of copy:
- every sentence has 20 words or fewer;
- the reading grade (Flesch-Kincaid) is 8 or lower for homeowners and 9 or lower for businesses,
  for blocks of 20 words or more;
- no superlative or unprovable claim word (best, fastest, cheapest, #1, guaranteed ...).
Blocks whose wording a requirement fixes verbatim (legal, safety or any spec-fixed sentence, WORD-029, D-482) are
marked "fixed" and skipped.

Arrows (WEB-029, D-4246): an arrow (the ARROW blocks, or a typed ->, => or <-) is a problem in a site-copy line, in
any page template (es_*.html included) outside the one allowed place (the whole text of a <span aria-hidden="true">
that is a direct child of a <button> whose classes hold "button" and not "button-quiet", last, after the button's
words, one per button), and in a CSS content: value (an arrow or its \\XXXX escape) in app/**/*.css or in a Rust
"const ..._CSS: &str" under app/.

It reads:
- docs/copy/site-copy.md: each "## <page> · <block> (homeowner|business[, fixed])" heading,
  followed by its copy on lines starting with "> ";
- once they exist, page templates under app/*/templates/ (visible text only, parsed with html.parser); a
  path containing "business" is business copy. Each block keeps its "<path> · text <n>" name (D-3881).

Template exemptions (T519, D-3878, D-3905) hold only in a "clean" element: only inline tags, no {% %} (even in
an HTML comment), |safe or start tag of its own name inside, closed by its own end tag while innermost; {# #}
is removed. es_*.html files are skipped. In lang="es", text with a SPANISH and no ENGLISH word is skipped; 5 or
more words with no SPANISH word are a problem. data-fixed="<ID>" (WORD-029, D-482) is skipped when the docs/spec/
"- **ID**" line holds its text word for word ({{ x }} = any one word), else a problem.

Legal drafts (WORD-031, D-492, D-3889): each section (split at Markdown headings) of the six public drafts in
LEGAL_DRAFTS is graded on its own limit, LEGAL_MAX_GRADE (homeowner 9, business 10; MAX_GRADE is not used). Each
paragraph sentence, list item and table row counts as a sentence; a sentence a docs/spec/ requirement holds word
for word, or Spanish text, is not measured; a section with fewer than 20 measured words is not graded. The
README, the security program and the incident plan are internal and not measured. --legal-grades prints each grade.

Usage: python3 scripts/check_copy.py [--root REPO_ROOT] [--legal-grades]
Exit status 1 lists each problem.
"""

from __future__ import annotations

import argparse
import html
import re
import sys
from dataclasses import dataclass, field
from html.parser import HTMLParser
from pathlib import Path

SITE_COPY = Path("docs/copy/site-copy.md")
MAX_WORDS = 20
MAX_GRADE = {"homeowner": 8.0, "business": 9.0}
MIN_WORDS_FOR_GRADE = 20
LEGAL_DRAFTS = {  # WORD-031, D-492: the six public drafts and their readers; internal documents are not measured
    "docs/legal/homeowner-terms.md": "homeowner",
    "docs/legal/business-terms.md": "business",
    "docs/legal/privacy-policy.md": "homeowner",
    "docs/legal/sms-terms.md": "homeowner",
    "docs/legal/business-email-notices.md": "business",
    "docs/legal/accessibility-statement.md": "homeowner",
}
LEGAL_MAX_GRADE = {"homeowner": 9.0, "business": 10.0}
SUPERLATIVES = re.compile(
    r"\b(best|fastest|cheapest|easiest|greatest|leading|#1|number one|top-rated|guaranteed?|"
    r"most trusted|lowest prices?|unbeatable|perfect)\b",
    re.IGNORECASE,
)
HEADING = re.compile(r"^## (?P<name>.+?) \((?P<tags>[a-z, ]+)\)\s*$")
SENTENCE_END = re.compile(r"(?<=[.?!])\s+(?=[A-Z0-9$\"“(])")
WORD = re.compile(r"[A-Za-z0-9$][A-Za-z0-9$'’,.-]*")
SPANISH = frozenset("de el la los que su para por usted es y en".split())
ENGLISH = frozenset("the and you your we to with our of for".split())
INLINE = frozenset("a abbr b bdi bdo br cite code data dfn em i kbd mark q s small span strong sub sup time u wbr".split())
VOID = frozenset("area base br col embed hr img input link meta source track wbr".split())
EXPR, STMT = "\ue000", "\ue001"  # a {{ expression }}; a {% statement %} or |safe expression
ARROW = re.compile("[\u2190-\u21ff\u2794-\u27bf\u27f0-\u27ff\u2900-\u297f\u2b00-\u2bff]|->|=>|<-")
CSS_CONTENT = re.compile(r"""(?<![\w-])content\s*:((?:"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|[^;}"'])*)""")
CSS_ESCAPE = re.compile(r"\\([0-9a-fA-F]{1,6})\s?")
RUST_CSS = re.compile(r'(?s)\bconst\s+\w*_CSS\s*:\s*&(?:\'static\s+)?str\s*=\s*(?:r(#*)"(.*?)"\1|"((?:[^"\\]|\\.)*)")')
REQUIREMENT = re.compile(r"^- \*\*(?P<id>[A-Z]+-\d+)\*\* (?P<text>.+)$", re.MULTILINE)


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
            readers = "businesses" if block.audience == "business" else "homeowners"
            problems.append(f"{block.where}: reading grade {level:.1f} (limit {limit:.0f} for {readers})")
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


@dataclass(eq=False)
class Element:
    """An element that may exempt its text (lang, data-fixed); see the module docstring for "clean"."""
    name: str
    lang: str | None
    fixed: str | None
    clean: bool
    closed: bool = False
    attrs: dict[str, str | None] = field(default_factory=dict)
    text: list[str] = field(default_factory=list)

    def plain(self) -> str:
        return " ".join("".join(self.text).split())

    def spanish(self) -> bool:
        return (self.lang or "").split("-")[0] == "es"


class TemplateText(HTMLParser):
    """Visible text pieces with the elements open around each, numbered as tags and line breaks split them."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.stack: list[Element] = []
        self.pieces: list[tuple[int, str, tuple[Element, ...]]] = []
        self.index, self.buf = 0, ""

    def flush(self) -> None:
        self.pieces.append((self.index, self.buf, tuple(self.stack)))
        self.index, self.buf = self.index + 1, ""

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        self.flush()
        stmt = STMT in (self.get_starttag_text() or "")
        for el in self.stack:
            el.clean = el.clean and not stmt and tag in INLINE and el.name != tag
        if tag in VOID:
            return
        found = dict(reversed(attrs))
        lang = (found.get("lang") or "").strip().lower() if "lang" in found else None
        fixed = (found.get("data-fixed") or "").strip() if "data-fixed" in found else None
        self.stack.append(Element(tag, lang, fixed, not stmt, attrs=found))

    handle_startendtag = handle_starttag

    def handle_endtag(self, tag: str) -> None:
        self.flush()
        i = next((i for i in range(len(self.stack) - 1, -1, -1) if self.stack[i].name == tag), None)
        if i is not None:
            self.stack[i].closed = i == len(self.stack) - 1
            del self.stack[i:]

    def handle_data(self, data: str) -> None:
        for el in self.stack:
            el.clean = el.clean and STMT not in data
            el.text.append(data)
        first, *rest = data.split("\n")
        self.buf += first
        for line in rest:
            self.flush()
            self.buf = line

    def handle_comment(self, data: str) -> None:  # counts as a tag, as before T519; Askama runs a {% %} in it
        self.flush()
        for el in self.stack:
            el.clean = el.clean and STMT not in data

    handle_decl = handle_pi = unknown_decl = handle_comment


def parse_template(markup: str) -> TemplateText:
    markup = re.sub(r"(?s)\{#.*?#\}", lambda m: re.sub(  # {# #} never renders; its tags keep block numbers (D-3881)
        r"</?[A-Za-z][^>]*>|([^<]+|<)", lambda t: "\n" * t[0].count("\n") if t[1] else "<!---->", m[0]), markup)
    markup = re.sub(r"(?is)<(script|style)\b.*?</\1>", " ", markup)
    markup = re.sub(r"(?s)\{%.*?%\}|\{\{[^}]*\|\s*safe\b.*?\}\}", STMT, markup)
    parser = TemplateText()
    parser.feed(re.sub(r"(?s)\{\{.*?\}\}", EXPR, markup))
    parser.close()
    parser.flush()
    return parser


def lowered_words(text: str) -> list[str]:
    return re.findall(r"\w+", text.replace(EXPR, " ").lower())


def innermost_lang(stack: tuple[Element, ...]) -> Element | None:
    return next((el for el in reversed(stack) if el.lang is not None), None)


def fixed_problem(el: Element, requirements: dict[str, str]) -> str | None:
    """Why a data-fixed element exempts nothing, or None when its requirement holds its text (WORD-029, D-482)."""
    if not (el.clean and el.closed):
        return "must hold only inline tags, no {% %}, and close with its own end tag"
    if el.fixed not in requirements:
        return "names no requirement in docs/spec/"
    pattern = r"(?<!\w)" + re.escape(el.plain()).replace(EXPR, r"\S+") + r"(?!\w)"
    if not re.search(pattern, requirements[el.fixed]):
        return f"text is not word for word in its requirement: {el.plain()[:70]}"
    return None


def load_requirements(root: Path) -> dict[str, str]:
    texts = [p.read_text(encoding="utf-8") for p in sorted((root / "docs/spec").glob("*.md"))]
    return {m["id"]: " ".join(m["text"].split()) for t in texts for m in REQUIREMENT.finditer(t)}


def template_paths(root: Path) -> list[Path]:
    app = root / "app"
    return sorted(app.glob("*/templates/**/*.html")) if app.is_dir() else []


def parsed_templates(root: Path) -> list[tuple[str, TemplateText]]:
    """Each page template but es_*.html (Spanish, D-3878), parsed."""
    return [(p.relative_to(root).as_posix(), parse_template(p.read_text(encoding="utf-8")))
            for p in template_paths(root) if not p.name.startswith("es_")]


def template_problems(root: Path) -> list[str]:
    """data-fixed markers no requirement holds, and lang="es" text with no Spanish word (WORD-029, D-3878)."""
    problems: list[str] = []
    requirements = load_requirements(root)
    for rel, parser in parsed_templates(root):
        own: dict[Element | None, list[str]] = {}
        for _, raw, stack in parser.pieces:
            own.setdefault(innermost_lang(stack), []).extend(lowered_words(raw))
        for el in dict.fromkeys(el for _, _, stack in parser.pieces for el in stack):
            if el.fixed is not None and (why := fixed_problem(el, requirements)):
                problems.append(f'{rel}: data-fixed="{el.fixed}" {why}')
            mine = own.get(el, [])
            if el.spanish() and len(mine) >= 5 and not SPANISH.intersection(mine):
                problems.append(f'{rel}: lang="es" text holds no common Spanish word: {el.plain()[:70]}')
    return problems


def exempt(raw: str, stack: tuple[Element, ...], requirements: dict[str, str]) -> bool:
    """True for text a clean data-fixed element pins, or Spanish text in a clean lang="es" element (D-3878)."""
    if any(el.fixed is not None and fixed_problem(el, requirements) is None for el in stack):
        return True
    lang, found = innermost_lang(stack) or Element("", None, None, False), set(lowered_words(raw))
    return lang.spanish() and lang.clean and lang.closed and bool(found & SPANISH) and not found & ENGLISH


def template_blocks(root: Path) -> list[Block]:
    blocks: list[Block] = []
    requirements = load_requirements(root)
    for rel, parser in parsed_templates(root):
        audience = "business" if "business" in rel else "homeowner"
        for i, raw, stack in parser.pieces:
            text = " ".join(raw.replace(EXPR, " ").replace(STMT, " ").split())
            if text and not exempt(raw, stack, requirements):
                blocks.append(Block(f"{rel} · text {i}", audience, False, text))
    return blocks


def allowed_arrow(stack: tuple[Element, ...]) -> bool:
    """WEB-029, D-4246: text that is the one arrow a primary button may end with, in its own aria-hidden span."""
    if len(stack) < 2 or stack[-1].name != "span" or stack[-2].name != "button":
        return False
    span, button = stack[-1], stack[-2]
    classes = (button.attrs.get("class") or "").split()
    words, _, after = button.plain().rpartition(span.plain())
    return (span.attrs.get("aria-hidden") == "true" and span.closed and "button" in classes
            and "button-quiet" not in classes and len(span.plain()) == 1 and bool(ARROW.fullmatch(span.plain()))
            and after == "" and len(ARROW.findall(button.plain())) == 1 and bool(re.search(r"[\w\ue000]", words)))


def template_arrow_problems(root: Path) -> list[str]:
    """WEB-029, D-4246: arrows in template text, tags or <style> content: values (es_*.html included) outside
    the one allowed place; <script> is skipped, where JavaScript's => is code, not copy."""
    problems: list[str] = []
    for path in template_paths(root):
        rel, markup = path.relative_to(root).as_posix(), path.read_text(encoding="utf-8")
        parser = parse_template(markup)
        for _, raw, stack in parser.pieces:
            text = " ".join(raw.replace(EXPR, " ").replace(STMT, " ").split())
            if ARROW.search(text) and not allowed_arrow(stack):
                problems.append(f"{rel}: arrow outside a primary button's aria-hidden span: {text[:70]}")
        bare = re.sub(r"(?s)\{#.*?#\}|\{%.*?%\}|\{\{.*?\}\}|<!--.*?-->|<(script|style)\b.*?</\1>", " ", markup)
        for tag in re.findall(r"<[A-Za-z][^>]*>", bare):
            if ARROW.search(html.unescape(tag)):
                problems.append(f"{rel}: arrow in a tag: {tag[:70]}")
        for css in re.findall(r"(?is)<style\b[^>]*>(.*?)</style>", markup):
            problems += css_arrow_problems(rel, css)
    return problems


def css_arrow_problems(where: str, css: str) -> list[str]:
    """WEB-029, D-4246: an arrow, typed or as a CSS escape, in a content: value."""
    problems: list[str] = []
    for match in CSS_CONTENT.finditer(css):
        value = CSS_ESCAPE.sub(lambda e: chr(min(int(e[1], 16), 0x10FFFF)), match[1])
        if ARROW.search(value):
            problems.append(f"{where}: arrow in CSS content: {match[0].strip()[:70]}")
    return problems


def rust_string(literal: str) -> str:
    """A Rust string literal's text: line continuations, \\u{...}, quote and backslash escapes resolved."""
    text = re.sub(r"\\\n\s*", "", literal)
    return re.sub(r"\\u\{([0-9a-fA-F]+)\}|\\(.)", lambda e: chr(int(e[1], 16)) if e[1] else
                  {"n": "\n", "t": "\t", "r": "\r"}.get(e[2], e[2]), text)


def style_arrow_problems(root: Path) -> list[str]:
    """WEB-029, D-4246: CSS content arrows in app/**/*.css and in every Rust const ..._CSS: &str under app/."""
    app = root / "app"
    if not app.is_dir():
        return []
    problems: list[str] = []
    for path in sorted(app.rglob("*.css")):
        problems += css_arrow_problems(path.relative_to(root).as_posix(), path.read_text(encoding="utf-8"))
    for path in sorted(app.rglob("*.rs")):
        for match in RUST_CSS.finditer(path.read_text(encoding="utf-8")):
            css = match[2] if match[2] is not None else rust_string(match[3])
            problems += css_arrow_problems(path.relative_to(root).as_posix(), css)
    return problems


def arrow_problems(root: Path) -> list[str]:
    """WEB-029, D-4246: every arrow in copy, templates and CSS content but the one primary-button arrow."""
    problems = [f"{block.where}: arrow in copy: {block.text[:70]}"
                for block in site_copy_blocks(root) if ARROW.search(html.unescape(block.text))]
    return problems + template_arrow_problems(root) + style_arrow_problems(root)


def markdown_plain(line: str) -> str:
    """A Markdown unit's reading text: list marker, links, code ticks and emphasis removed."""
    line = re.sub(r"^\s*(?:(?:[-*+]|\d+\.)\s+)?", "", line)
    line = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", line)
    return " ".join(re.sub(r"`|\*\*|__|(?<!\w)[*_]|[*_](?!\w)", "", line).split())


def legal_units(lines: list[str]) -> list[str]:
    """WORD-031 (D-3889): a section's paragraphs, each list item and each table row as its own unit."""
    units: list[str] = []
    current: list[str] = []
    for raw in lines + [""]:
        line = re.sub(r"^\s*>\s?", "", raw)
        item = re.match(r"^\s*(?:[-*+]|\d+\.)\s+", line)
        row = line.lstrip().startswith("|")
        if not line.strip() or item or row:
            units.append(" ".join(current))
            current = []
        if row and not re.fullmatch(r"\s*\|[\s:|-]*", line):
            units.append(" ".join(cell.strip() for cell in line.strip().strip("|").split("|")))
        elif line.strip() and not row:
            current.append(line)
    return [plain for unit in units if (plain := markdown_plain(unit))]


def legal_sections(text: str) -> list[tuple[str, list[str]]]:
    """WORD-031: the draft split at each Markdown heading (heading lines are labels, not measured)."""
    sections: list[tuple[str, list[str]]] = [("opening", [])]
    for line in text.splitlines():
        head = re.match(r"^#{1,6}\s+(.+?)\s*$", line)
        if head:
            sections.append((head.group(1), []))
        else:
            sections[-1][1].append(line)
    return [(name, legal_units(lines)) for name, lines in sections]


def padded(text: str) -> str:
    """Text as a space-padded string of lowered words, for word-for-word matching."""
    return f" {' '.join(lowered_words(text))} "


@dataclass
class FixedText:
    """WORD-031: what docs/spec/ fixes word for word: each requirement, and each quoted span of 5 or more words."""
    requirements: list[str]
    phrases: list[str]


def fixed_text(root: Path) -> FixedText:
    texts = list(load_requirements(root).values())
    quoted = [q for t in texts for q in re.findall(r"[\"“]([^\"”]+)[\"”]", t) if len(lowered_words(q)) >= 5]
    return FixedText([padded(t) for t in texts], [padded(q) for q in quoted])


def measured_text(sentence: str, fixed: FixedText) -> str:
    """WORD-031: the part of a sentence the legal measure grades: "" for a sentence a requirement holds word for
    word or Spanish text (D-3878); otherwise the sentence without any requirement-quoted span it contains."""
    found = lowered_words(sentence)
    whole = padded(sentence)
    if not found or any(whole in req for req in fixed.requirements):
        return ""
    if set(found) & SPANISH and not set(found) & ENGLISH:
        return ""
    rest = whole
    for phrase in fixed.phrases:
        rest = rest.replace(phrase, " ")
    return sentence if rest == whole else " ".join(rest.split())


def sentences_grade(sents: list[str]) -> float:
    """Flesch-Kincaid grade of sentences already split (a list item or table row is one, D-3889)."""
    ws = [w for s in sents for w in words(s)]
    if not sents or not ws:
        return 0.0
    return 0.39 * len(ws) / len(sents) + 11.8 * sum(syllables(w) for w in ws) / len(ws) - 15.59


def legal_grades(root: Path) -> list[tuple[str, str, float]]:
    """WORD-031: (where, audience, grade) for each section of 20 or more measured words of the six drafts."""
    fixed = fixed_text(root)
    found: list[tuple[str, str, float]] = []
    for rel, audience in LEGAL_DRAFTS.items():
        path = root / rel
        if not path.is_file():
            continue
        for name, units in legal_sections(path.read_text(encoding="utf-8")):
            sents = [m for unit in units for s in sentences(unit) if (m := measured_text(s, fixed))]
            if sum(len(words(s)) for s in sents) >= MIN_WORDS_FOR_GRADE:
                found.append((f"{rel} · {name}", audience, sentences_grade(sents)))
    return found


def legal_grade_problem(where: str, audience: str, level: float) -> str | None:
    """WORD-031: the problem for a legal section above its audience's limit (homeowner 9, business 10)."""
    limit = LEGAL_MAX_GRADE[audience]
    if level <= limit:
        return None
    readers = "businesses" if audience == "business" else "homeowners"
    return f"{where}: reading grade {level:.1f} (legal limit {limit:.0f} for {readers})"


def legal_problems(root: Path) -> list[str]:
    """WORD-031: each over-grade section of the six public legal drafts."""
    return [p for args in legal_grades(root) if (p := legal_grade_problem(*args))]


def run(root: Path) -> list[str]:
    problems: list[str] = []
    for block in site_copy_blocks(root) + template_blocks(root):
        problems += check_block(block)
    return problems + template_problems(root) + arrow_problems(root) + legal_problems(root)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--legal-grades", action="store_true", help="print each legal section's grade (WORD-031)")
    args = parser.parse_args(argv)
    if args.legal_grades:
        for where, audience, level in legal_grades(args.root):
            print(f"{level:5.1f} {audience:9} {where}")
    problems = run(args.root)
    for problem in problems:
        print(problem)
    print(f"check_copy: {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
