"""The brand-literal guard and the Python side of the brand file (TD-WORD-34; WORD-001, WORD-008,
WORD-021, TXT-006, FEE-049; D-1932, D-1933).

The brand is data (app/core/data/brand.json): customer-visible code under app/ takes it from
core::api::brand() or a template token, never a literal. This test fails, naming file and line, on
any line of app/** a customer can see that holds, in any letter case, the brand's display name, short
name, legal name, spoken legal name, domain, a forbidden_alone word or the statement prefix as a
whole word.

Scanned: every file under app/; a file that is not UTF-8 text fails unless its extension is in
BINARY_EXTENSIONS. Skipped: Rust under src/tests/ or a tests/ folder and #[cfg(test)] items; comment
lines (//, {# #}, <!-- -->); CLAUDE.md, Cargo.toml and the brand file itself; internal identifiers
(askhava_ and askhava- crate, role and schema names, ASKHAVA_* keys, a dotted setting or cookie
name, a JSON schema's $id). Exempt by name, each with its reason, in EXEMPT_FILES and EXEMPT_CONSTS.

Until T292 to T295 finish, tests/brand_literal_baseline.json (file to count of lines) lets a listed
file keep at most its count; every other file must have none. The baseline only shrinks: an entry
above the same entry in main's baseline fails (read with git show; skipped when main has none).
The planted cases build a temporary tree. Fake data only ("Fakeco" is invented).

Run: python3 -m pytest tests/docs/test_brand_literals.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
import re
import subprocess
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import brand
import check_catalog
import check_legal_docs
import pytest

ROOT = Path(__file__).resolve().parents[2]
BASELINE = Path("tests") / "brand_literal_baseline.json"
BRAND_FILE = "app/core/data/brand.json"
# Files that cannot hold customer text a literal check could read; anything else must be UTF-8.
BINARY_EXTENSIONS = frozenset({".png", ".jpg", ".jpeg", ".webp", ".gif", ".ico", ".woff", ".woff2"})
SKIP_NAMES = frozenset({"CLAUDE.md", "Cargo.toml"})
EXEMPT_FILES = {
    "app/ai/data/word_022_phrases.json": "mirrors scripts/trace_spec.py's spec patterns (D-493)",
}
EXEMPT_CONSTS = {
    ("app/payments/", "AUTHORIZATION_TEXT"): "a legal record stored under AUTHORIZATION_VERSION (FEE-015)",
}
COMMENT_BLOCKS = (("{#", "#}"), ("<!--", "-->"))


def brand_pattern(data: dict[str, Any]) -> re.Pattern[str]:
    """Every brand word as a whole word in any letter case, the longest first."""
    words = {
        data["display_name"],
        data["short_name"],
        data["legal_name"],
        data["spoken_legal_name"],
        data["domain"],
        data["statement_prefix"],
        *data["forbidden_alone"],
    }
    alternatives = "|".join(re.escape(w) for w in sorted(words, key=len, reverse=True))
    return re.compile(rf"(?<!\w)(?:{alternatives})(?!\w)", re.IGNORECASE)


def is_identifier(line: str, match: re.Match[str]) -> bool:
    """askhava-core, askhava.vault_probe and the like: one bare token joined by '-' or '.x'."""
    if re.search(r"[\s.]", match.group(0)):
        return False
    before = line[match.start() - 1 : match.start()]
    after = line[match.end() : match.end() + 2]
    return before == "-" or after[:1] == "-" or (after[:1] == "." and after[1:2].islower())


def is_rust_test_file(rel: str) -> bool:
    return rel.endswith(".rs") and "tests" in Path(rel).parts[:-1]


def skip_item(lines: list[str], start: int) -> int:
    """The index after the item starting at lines[start]: a braced block or a line ending ';'."""
    depth, seen_brace = 0, False
    for index in range(start, len(lines)):
        depth += lines[index].count("{") - lines[index].count("}")
        seen_brace = seen_brace or "{" in lines[index]
        if (seen_brace and depth <= 0) or (not seen_brace and lines[index].rstrip().endswith(";")):
            return index + 1
    return len(lines)


def exempt_const(rel: str, stripped: str) -> bool:
    return any(rel.startswith(prefix) and f"const {name}" in stripped for prefix, name in EXEMPT_CONSTS)


def visible_lines(rel: str, text: str) -> Iterator[tuple[int, str]]:
    """The lines a customer could see: no comment line, test item or exempt constant."""
    lines = text.splitlines()
    index, close = 0, ""
    while index < len(lines):
        stripped = lines[index].strip()
        index += 1
        if close:
            close = "" if close in stripped else close
        elif stripped.startswith("//") or stripped.startswith('"$id"'):
            continue
        elif ender := next((c for o, c in COMMENT_BLOCKS if stripped.startswith(o)), ""):
            close = "" if ender in stripped[2:] else ender
        elif rel.endswith(".rs") and stripped.startswith("#[cfg(test)]"):
            index = skip_item(lines, index)
        elif exempt_const(rel, stripped):
            index = skip_item(lines, index - 1)
        else:
            yield index, lines[index - 1]


def file_hits(rel: str, text: str, pattern: re.Pattern[str]) -> list[str]:
    """'file:line: text' for each visible line holding a brand word that is not an identifier."""
    return [
        f"{rel}:{n}: {line.strip()[:120]}"
        for n, line in visible_lines(rel, text)
        if any(not is_identifier(line, m) for m in pattern.finditer(line))
    ]


def scan(root: Path, data: dict[str, Any]) -> tuple[dict[str, list[str]], list[str]]:
    """Every file under root/app with hits, and the files that are neither UTF-8 nor listed binary."""
    pattern = brand_pattern(data)
    found: dict[str, list[str]] = {}
    unreadable: list[str] = []
    for path in sorted(p for p in (root / "app").rglob("*") if p.is_file()):
        rel = path.relative_to(root).as_posix()
        if path.name in SKIP_NAMES or rel == BRAND_FILE or rel in EXEMPT_FILES or is_rust_test_file(rel):
            continue
        try:
            text = path.read_bytes().decode("utf-8")
        except UnicodeDecodeError:
            if path.suffix.lower() not in BINARY_EXTENSIONS:
                unreadable.append(f"{rel}: not UTF-8 text and not a listed binary type")
            continue
        if hits := file_hits(rel, text, pattern):
            found[rel] = hits
    return found, unreadable


def over_baseline(found: dict[str, list[str]], baseline: dict[str, int]) -> list[str]:
    """Each hit of a file over its baseline count (none allowed for an unlisted file)."""
    return [
        f"{hit} (brand literal; {len(hits)} lines, baseline {baseline.get(rel, 0)})"
        for rel, hits in sorted(found.items())
        if len(hits) > baseline.get(rel, 0)
        for hit in hits
    ]


def grown(baseline: dict[str, int], main: dict[str, int] | None) -> list[str]:
    """Entries above main's baseline: the baseline only shrinks (D-1933)."""
    if main is None:
        return []
    return [
        f"{BASELINE.as_posix()}: {rel} is {count}, above main's {main.get(rel, 0)}"
        for rel, count in sorted(baseline.items())
        if count > main.get(rel, 0)
    ]


def main_baseline(root: Path) -> dict[str, int] | None:
    """The baseline at main (or origin/main), or None when neither has it."""
    for ref in ("main", "origin/main"):
        shown = subprocess.run(
            ["git", "-C", str(root), "show", f"{ref}:{BASELINE.as_posix()}"],
            capture_output=True,
            text=True,
            check=False,
        )
        if shown.returncode == 0:
            return json.loads(shown.stdout)
    return None


def guard(root: Path, data: dict[str, Any], main: dict[str, int] | None) -> list[str]:
    """Every problem the guard reports for the tree at root."""
    path = root / BASELINE
    baseline = json.loads(path.read_text(encoding="utf-8")) if path.is_file() else {}
    found, unreadable = scan(root, data)
    return unreadable + over_baseline(found, baseline) + grown(baseline, main)


def test_td_word_34_no_new_brand_literal_under_app() -> None:
    """TD-WORD-34: the tree passes the guard against its baseline and main's."""
    problems = guard(ROOT, brand.load(), main_baseline(ROOT))
    assert problems == [], "\n".join(problems)


# The planted cases: a temporary tree with its own app/ and baseline.


def plant(root: Path, files: dict[str, str | bytes], baseline: dict[str, int] | None = None) -> Path:
    for rel, body in files.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(body, bytes):
            path.write_bytes(body)
        else:
            path.write_text(body, encoding="utf-8")
    if baseline is not None:
        (root / BASELINE).parent.mkdir(parents=True, exist_ok=True)
        (root / BASELINE).write_text(json.dumps(baseline), encoding="utf-8")
    return root


@pytest.mark.parametrize(
    "line",
    [
        'const NAME: &str = "Ask Hava";',
        'let s = "ASK HAVA: hi";',
        "<p>ask hava helps</p>",
        '"text": "See askhava.com/help"',
        "<p>Hava says hi</p>",
        'let p = "ASKHAVA* JOB 1";',
        'let l = "Ask Hava LLC";',
        "say Ask Hava L L C",
        'let d = "Ask Hava-branded";',
    ],
)
def test_td_word_34_a_new_file_with_a_brand_word_fails(tmp_path: Path, line: str) -> None:
    root = plant(tmp_path, {"app/fake/src/lib.rs": f"//! Fake.\n{line}\n"})
    problems = guard(root, brand.load(), None)
    assert problems and problems[0].startswith("app/fake/src/lib.rs:2:"), problems


RS = "app/fake/src/lib.rs"
HTML = "app/fake/templates/page.html"


@pytest.mark.parametrize(
    ("rel", "body"),
    [
        (RS, "// Ask Hava in a comment\n/// Ask Hava doc\n//! Ask Hava module doc\n"),
        (HTML, "{# Ask Hava #}\n{#\nAsk Hava\n#}\n<!-- Ask Hava -->\n<!--\nAsk Hava\n-->\n"),
        (RS, '#[cfg(test)]\nmod tests {\n    fn f() {\n        let s = "Ask Hava";\n    }\n}\n'),
        (RS, '#[cfg(test)]\nconst T: &str =\n    "Ask Hava";\n'),
        (RS, "use askhava_core::api::brand;\nlet k = \"ASKHAVA_ID_SECRET\";\nlet r = \"askhava_app\";\n"),
        (RS, 'let c = "askhava-core";\nlet p = "askhava.vault_probe";\nlet c = "askhava.csrf.session";\n'),
        ("app/fake/x.schema.json", '{\n  "$id": "https://askhava.com/schemas/x.json",\n}\n'),
        (HTML, "Lake Havasu City and Havasupai\n"),
    ],
)
def test_td_word_34_comments_test_modules_and_identifiers_pass(tmp_path: Path, rel: str, body: str) -> None:
    root = plant(tmp_path, {rel: body})
    assert guard(root, brand.load(), None) == []


def test_td_word_34_test_folders_skipped_files_and_exemptions_pass(tmp_path: Path) -> None:
    hit = 'let s = "Ask Hava";\n'
    root = plant(
        tmp_path,
        {
            "app/fake/src/tests/a.rs": hit,
            "app/fake/tests/b.rs": hit,
            "app/fake/CLAUDE.md": hit,
            "app/fake/Cargo.toml": hit,
            BRAND_FILE: hit,
            "app/ai/data/word_022_phrases.json": hit,
            "app/payments/src/models.rs": 'pub const AUTHORIZATION_TEXT: &str = "I authorize Ask Hava \\\nLLC";\n',
            "app/fake/static/logo.png": b"\x89PNG\r\n\x1a\n\xff\xfe",
        },
    )
    assert guard(root, brand.load(), None) == []


def test_td_word_34_a_non_text_file_of_an_unlisted_type_fails(tmp_path: Path) -> None:
    root = plant(tmp_path, {"app/fake/data/blob.dat": b"\xff\xfe\x00Ask"})
    assert guard(root, brand.load(), None) == ["app/fake/data/blob.dat: not UTF-8 text and not a listed binary type"]


def test_td_word_34_the_baseline_allows_its_count_and_no_more(tmp_path: Path) -> None:
    two = 'let a = "Ask Hava";\nlet b = "Ask Hava";\n'
    root = plant(tmp_path, {"app/fake/src/lib.rs": two}, {"app/fake/src/lib.rs": 2})
    assert guard(root, brand.load(), None) == []
    plant(root, {"app/fake/src/lib.rs": two + 'let c = "askhava.com";\n'})
    problems = guard(root, brand.load(), None)
    assert len(problems) == 3 and all("baseline 2" in p for p in problems), problems


def test_td_word_34_a_baseline_entry_above_mains_fails(tmp_path: Path) -> None:
    root = plant(tmp_path, {}, {"app/fake/src/lib.rs": 3, "app/fake/src/other.rs": 1})
    (root / "app").mkdir()
    problems = guard(root, brand.load(), {"app/fake/src/lib.rs": 2, "app/fake/src/other.rs": 1})
    assert problems == [f"{BASELINE.as_posix()}: app/fake/src/lib.rs is 3, above main's 2"]
    assert grown({"app/new.rs": 1}, {}) == [f"{BASELINE.as_posix()}: app/new.rs is 1, above main's 0"]


def test_td_word_34_main_baseline_reads_git_or_gives_none(tmp_path: Path) -> None:
    assert main_baseline(tmp_path) is None
    on_main = main_baseline(ROOT)
    assert on_main is None or isinstance(on_main, dict)


# scripts/brand.py fills and checks the same way as app/core/src/brand.rs.


def fake_brand() -> dict[str, Any]:
    return {
        "display_name": "Fakeco",
        "business_name": "Fakeco for Business",
        "short_name": "Fakeco",
        "legal_name": "Fakeco LLC",
        "spoken_legal_name": "Fakeco L L C",
        "domain": "fakeco.test",
        "email_from_name": "Fakeco",
        "email_from_address": "jobs@notify.fakeco.test",
        "statement_prefix": "FAKECO",
        "forbidden_alone": ["Fake"],
        "es": {
            "display_name": "Fakeco ES",
            "business_name": "Fakeco para Negocios",
            "short_name": "FakecoES",
            "legal_name": "Fakeco ES LLC",
        },
    }


def test_td_word_34_brand_py_fills_like_the_app() -> None:
    data = brand.check(fake_brand())
    text = "{brand_short}: {brand}, {brand_business}, {brand_legal}, {domain}/help, job {job}"
    assert brand.fill(text, False, data) == "Fakeco: Fakeco, Fakeco for Business, Fakeco LLC, fakeco.test/help, job {job}"
    assert (
        brand.fill(text, True, data)
        == "FakecoES: Fakeco ES, Fakeco para Negocios, Fakeco ES LLC, fakeco.test/help, job {job}"
    )
    with pytest.raises(brand.BrandError, match="brand_x"):
        brand.fill("{brand_x}", data=data)
    assert brand.fill("{brand}") == brand.load()["display_name"]


@pytest.mark.parametrize(
    ("path", "value", "field"),
    [
        (("display_name",), "", "display_name"),
        (("es", "short_name"), " ", "es.short_name"),
        (("forbidden_alone",), [], "forbidden_alone"),
        (("short_name",), "Fake:co", "short_name"),
        (("short_name",), "Fakeco Fakeco Fakeco1", "short_name"),
        (("legal_name",), "Other LLC", "legal_name"),
        (("es", "legal_name"), "Other LLC", "es.legal_name"),
        (("domain",), "Fakeco.test", "domain"),
        (("email_from_address",), "jobs@other.test", "email_from_address"),
        (("statement_prefix",), "FAKE", "statement_prefix"),
        (("statement_prefix",), "FAKECOFAKEC", "statement_prefix"),
        (("statement_prefix",), "1234567", "statement_prefix"),
        (("statement_prefix",), "FAKECOFAKE", "statement_prefix"),
    ],
)
def test_td_word_34_brand_py_refuses_each_bad_field(path: tuple[str, ...], value: Any, field: str) -> None:
    data = fake_brand()
    slot = data
    for key in path[:-1]:
        slot = slot[key]
    slot[path[-1]] = value
    with pytest.raises(brand.BrandError, match=re.escape(f"field {field} ")):
        brand.check(data)


def test_td_word_34_brand_py_refuses_an_unknown_field_or_a_bad_file(tmp_path: Path) -> None:
    with pytest.raises(brand.BrandError, match="does not parse"):
        brand.check({**fake_brand(), "nickname": "Fakey"})
    with pytest.raises(brand.BrandError, match="does not parse"):
        brand.check({**fake_brand(), "es": {"display_name": "Fakeco"}})
    bad = tmp_path / "brand.json"
    bad.write_text("{", encoding="utf-8")
    with pytest.raises(brand.BrandError, match="does not parse"):
        brand.load(bad)


# The build's checks read the brand file.


def test_td_word_34_the_legal_check_fails_on_the_drafts_under_a_fake_brand(tmp_path: Path) -> None:
    fake = tmp_path / "brand.json"
    fake.write_text(json.dumps(fake_brand()), encoding="utf-8")
    assert check_legal_docs.run(ROOT).errors == []
    errors = check_legal_docs.run(ROOT, fake).errors
    assert any("missing WORD-002 wording: 'Fakeco is not a contractor" in e for e in errors), errors
    assert not any("spec text" in e for e in errors), errors


def test_td_word_34_the_legal_check_builds_word_021_from_the_brand() -> None:
    (pattern, label), = check_legal_docs.alone_patterns(brand.load())
    assert label == "'Hava' alone (WORD-021)"
    assert pattern.search("Hava said") and not pattern.search("Ask Hava said")
    (other, _), = check_legal_docs.alone_patterns(fake_brand() | {"forbidden_alone": ["Zed"]})
    assert other.search("Zed") and not other.search("Zedd")


def test_td_word_34_a_safety_prompt_must_open_with_the_short_name() -> None:
    short = brand.load()["short_name"]
    good = {"id": "p1", "en": "{brand_short}: Stay safe.", "es": f"{short}: Cuídese."}
    assert check_catalog.check_safety_prompt_brand(good) == []
    for bad in ({**good, "en": "Stay safe."}, {**good, "es": "{brand_x}: hola"}):
        assert check_catalog.check_safety_prompt_brand(bad) == [
            f"question bank: safety prompt p1 must start '{short}:'"
        ]
