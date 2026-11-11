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
(the lower-case askhava_ and askhava- crate, role and schema names, ASKHAVA_* keys, an askhava.<x>
setting or cookie name, a JSON schema's $id), never a brand word joined by a hyphen ("Hava-approved"). Exempt by name, each with its reason, in EXEMPT_FILES and EXEMPT_CONSTS.

Strict since T295 (D-1933): there is no baseline, so any brand literal in a scanned file fails, and
tests/brand_literal_baseline.json must not come back. The planted cases build a temporary tree.
Fake data only ("Fakeco" is invented).

Run: python3 -m pytest tests/docs/test_brand_literals.py -q -p no:cacheprovider
"""

from __future__ import annotations

import importlib.util
import json
import re
import sys
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import brand
import check_catalog
import check_legal_docs
import pytest

ROOT = Path(__file__).resolve().parents[2]
RETIRED_BASELINE = Path("tests") / "brand_literal_baseline.json"
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
CFG_TEST = "#[cfg(test)]"


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


# The fixed internal name of crates, roles, schemas, settings and cookies; it stays when the brand
# changes, so it is a literal here, not read from the brand file.
INTERNAL = "askhava"
# askhava-core, askhava_app, askhava.vault_probe: the lower-case internal name, then '-', '_' or
# '.', then a lower-case letter. ASKHAVA_ID_SECRET never matches the brand pattern ('_' is a word
# character), so it needs no rule.
INTERNAL_AFTER = re.compile(r"[-_.][a-z]")
# X-Askhava-Message-Key: an internal HTTP header between the app and its stand-ins.
HEADER = re.compile(r"(?<![\w-])X-Askhava-[A-Z]")


def is_identifier(line: str, match: re.Match[str]) -> bool:
    """askhava-core, askhava_app, askhava.vault_probe and an X-Askhava- header: only the internal
    name, exactly in that case, never another brand word."""
    if match.group(0) == INTERNAL:
        return bool(INTERNAL_AFTER.match(line, match.end()))
    return match.group(0) == INTERNAL.capitalize() and bool(HEADER.match(line, max(match.start() - 2, 0)))


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
        elif rel.endswith(".rs") and stripped.startswith(CFG_TEST):
            # The item is on the attribute's line ("#[cfg(test)] mod tests;") or starts below it.
            index = skip_item(lines, index - 1 if stripped[len(CFG_TEST) :].strip() else index)
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


def literals(found: dict[str, list[str]]) -> list[str]:
    """Every hit of every file: no file may keep a brand literal (D-1933)."""
    return [f"{hit} (brand literal)" for _, hits in sorted(found.items()) for hit in hits]


def guard(root: Path, data: dict[str, Any]) -> list[str]:
    """Every problem the guard reports for the tree at root, the retired baseline file included."""
    found, unreadable = scan(root, data)
    retired = [f"{RETIRED_BASELINE.as_posix()}: the baseline is retired (D-1933)"]
    return unreadable + literals(found) + (retired if (root / RETIRED_BASELINE).exists() else [])


def test_td_word_34_no_new_brand_literal_under_app() -> None:
    """TD-WORD-34, TD-WORD-38: the tree passes the strict guard, with no baseline file."""
    problems = guard(ROOT, brand.load())
    assert problems == [], "\n".join(problems)


# The planted cases: a temporary tree with its own app/.


def plant(root: Path, files: dict[str, str | bytes]) -> Path:
    for rel, body in files.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(body, bytes):
            path.write_bytes(body)
        else:
            path.write_text(body, encoding="utf-8")
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
        "<p>Hava-approved pros</p>",
        "<p>pro-Hava</p>",
        "<p>Askhava-branded and askhava.com</p>",
        "<p>ASKHAVA-x</p>",
        "<p>Hava.approved</p>",
        'let h = "Y-Askhava-Key";',
    ],
)
def test_td_word_34_a_new_file_with_a_brand_word_fails(tmp_path: Path, line: str) -> None:
    root = plant(tmp_path, {"app/fake/src/lib.rs": f"//! Fake.\n{line}\n"})
    problems = guard(root, brand.load())
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
        (RS, '.header("X-Askhava-Message-Key", key);\n'),
        (RS, '#[cfg(test)] fn f() {\n    let s = "Ask Hava";\n}\n'),
        ("app/fake/x.schema.json", '{\n  "$id": "https://askhava.com/schemas/x.json",\n}\n'),
        (HTML, "Lake Havasu City and Havasupai\n"),
    ],
)
def test_td_word_34_comments_test_modules_and_identifiers_pass(tmp_path: Path, rel: str, body: str) -> None:
    root = plant(tmp_path, {rel: body})
    assert guard(root, brand.load()) == []


def test_td_word_34_a_one_line_cfg_test_item_does_not_hide_the_next_line(tmp_path: Path) -> None:
    body = '#[cfg(test)] mod tests;\nlet s = "Ask Hava";\n#[cfg(test)]\nmod more;\nlet t = "Hava";\n'
    problems = guard(plant(tmp_path, {RS: body}), brand.load())
    assert [p.split(" ", 1)[0] for p in problems] == [f"{RS}:2:", f"{RS}:5:"], problems


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
    assert guard(root, brand.load()) == []


def test_td_word_34_a_non_text_file_of_an_unlisted_type_fails(tmp_path: Path) -> None:
    root = plant(tmp_path, {"app/fake/data/blob.dat": b"\xff\xfe\x00Ask"})
    assert guard(root, brand.load()) == ["app/fake/data/blob.dat: not UTF-8 text and not a listed binary type"]


def test_td_word_38_one_literal_fails_and_a_baseline_file_is_refused(tmp_path: Path) -> None:
    """TD-WORD-38: with no baseline any brand literal fails, and a baseline file is not read."""
    root = plant(tmp_path, {"app/fake/src/lib.rs": 'let a = "Ask Hava";\n'})
    assert guard(root, brand.load()) == ['app/fake/src/lib.rs:1: let a = "Ask Hava"; (brand literal)']
    plant(root, {"app/fake/src/lib.rs": "//! Fake.\n", RETIRED_BASELINE.as_posix(): '{"app/fake/src/lib.rs": 1}'})
    assert guard(root, brand.load()) == [f"{RETIRED_BASELINE.as_posix()}: the baseline is retired (D-1933)"]


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


RUST_BRAND_TESTS = ROOT / "app" / "core" / "src" / "tests" / "brand.rs"


def fill_cases() -> list[dict[str, Any]]:
    """The JSON table in brand.rs's FILL_CASES raw string, the one table both languages run."""
    text = RUST_BRAND_TESTS.read_text(encoding="utf-8")
    found = re.search(r'const FILL_CASES: &str = r#"(.*?)"#;', text, re.DOTALL)
    assert found, "FILL_CASES not found in app/core/src/tests/brand.rs"
    cases: list[dict[str, Any]] = json.loads(found.group(1))
    return cases


def test_td_word_34_brand_py_reads_nested_and_stray_braces_like_the_app() -> None:
    """The same cases as app/core/src/tests/brand.rs: each fills alike or names the same token."""
    data = brand.check(fake_brand())
    cases = fill_cases()
    assert len(cases) >= 10
    for case in cases:
        if "unknown" in case:
            with pytest.raises(brand.BrandError, match=re.escape(f"{{{case['unknown']}}}")):
                brand.fill(case["text"], case["spanish"], data)
        else:
            assert brand.fill(case["text"], case["spanish"], data) == case["filled"], case


@pytest.mark.parametrize(
    ("path", "value", "field"),
    [
        (("domain",), "fakeco.test\n", "domain"),
        (("statement_prefix",), "FAKECO\n", "statement_prefix"),
        (("email_from_address",), "jobs@fakeco.test\n", "email_from_address"),
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


@pytest.mark.parametrize(
    ("display", "fine", "alone"),
    [
        ("Hava Home", "Hava Home helps", "Hava helps"),
        ("Go Hava Now", "Go Hava Now helps", "Go Hava helps"),
        ("Ask Hava", "Ask Hava helps", "Hava helps"),
    ],
)
def test_td_word_34_word_021_allows_the_word_only_inside_the_display_name(display: str, fine: str, alone: str) -> None:
    (pattern, _), = check_legal_docs.alone_patterns({"display_name": display, "forbidden_alone": ["Hava"]})
    assert not pattern.search(fine) and pattern.search(alone) and pattern.search(f"{fine}. Hava says")
    assert check_legal_docs.alone_patterns({"display_name": "Hava", "forbidden_alone": ["Hava"]}) == ()


def test_td_word_34_a_bad_brand_file_makes_pii_scan_stricter_not_a_crash(monkeypatch: pytest.MonkeyPatch) -> None:
    """The runner loads scripts/pii_scan.py by path and catches only load errors (runner/github_env.py)."""
    scripts = str(ROOT / "scripts")

    def broken(*_: Any) -> dict[str, Any]:
        raise brand.BrandError("the brand field domain is not valid")

    monkeypatch.setattr(brand, "load", broken)
    monkeypatch.setattr(sys, "path", [p for p in sys.path if p != scripts])
    spec = importlib.util.spec_from_file_location("pii_scan_fake_brand", ROOT / "scripts" / "pii_scan.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert module.OWN_DOMAINS == ()
    assert scripts not in sys.path
    monkeypatch.undo()
    assert module.own_domains() == (brand.load()["domain"],)


def test_td_word_34_a_safety_prompt_must_open_with_the_short_name() -> None:
    short = brand.load()["short_name"]
    good = {"id": "p1", "en": "{brand_short}: Stay safe.", "es": f"{short}: Cuídese."}
    assert check_catalog.check_safety_prompt_brand(good, brand.load()) == []
    for bad in ({**good, "en": "Stay safe."}, {**good, "es": "{brand_x}: hola"}):
        assert check_catalog.check_safety_prompt_brand(bad, brand.load()) == [
            f"question bank: safety prompt p1 must start '{short}:'"
        ]
