"""Every email is real text with a plain-text part and meaningful link text (T115; A11Y-019, D-317).

D-4240's closed lists, one definition for worker and reviewer.

Scope: every `.rs` file under `app/*/src/` outside a `tests` folder, each read up to its first
`#[cfg(test)]` line. Plain-text part (rule 7): each `Email` literal sets `text` and each `Rendered`
literal `plain`, with no `..` field, and the value (and, for a bare name, the right side of the
nearest `let` above that binds it) is no empty form. The provider's `Email<'a>` keeps both parts
and never skips one. Email HTML (rule 8): `app/messaging/templates/email_layout.html`, every
`app/*/templates/*.html` that names it, and every `source = r#"…"#` in scope that names it carries
no image form, has text, and every link's text says where it goes. Fake data only.

Run: python3 -m pytest tests/docs/test_email_accessibility.py -q -p no:cacheprovider
"""

from __future__ import annotations

import html
import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
LAYOUT = ROOT / "app" / "messaging" / "templates" / "email_layout.html"
PROVIDER = ROOT / "app" / "adapters" / "src" / "email.rs"

SEGMENT = r"(?:crate|super|self|email|api|adapters|askhava_adapters|messaging|askhava_messaging)::"
LITERAL = re.compile(rf"(?<![\w:])(?:{SEGMENT})*(Email|Rendered|Self)\s*\{{")
NOT_AFTER = re.compile(r"(?:\b(?:struct|enum|impl|for|let)|->)\s*$")
IMPL = re.compile(
    r"impl(?:<[^>]*>)?\s+(?:[\w:]+(?:<[^>]*>)?\s+for\s+)?(Email|Rendered)(?:<[^>]*>)?\s*\{\s*$"
)
FIELD = {"Email": "text", "Rendered": "plain"}
METHODS = r"(?:\.(?:as_str|clone|to_string|to_owned|into|as_ref|trim|borrow)\(\)|\[\.\.\])"
FORM = (
    r'(?:""|r#*""#*|(?:std::string::)?String::new\(\)|(?:Default|String|str|<[^>]+>)::default\(\)'
    r'|String::from\(""\)|String::with_capacity\([^)]*\)|format!\(""\)|concat!\(\))'
)
EMPTY = re.compile(rf"(?:&(?:mut)?|\*)*{FORM}{METHODS}*")
EMPTY_UNWRAP = re.compile(
    rf"\.unwrap_or_default\(\)|\.unwrap_or\({EMPTY.pattern}\)"
    rf"|\.unwrap_or_else\((?:String::new|String::default|Default::default|\|\|{EMPTY.pattern})\)"
)
BARE = re.compile(
    r"(?:&\s*(?:mut\s+)?|\*\s*)*([A-Za-z_]\w*)\s*"
    r"(?:\.\s*(?:as_str|clone|to_string|to_owned|into|as_ref|trim|borrow)\s*\(\s*\)|\[\s*\.\.\s*\])*"
)
LET = re.compile(r"let\s+(\((?:[^()]*)\)|(?:mut\s+)?\w+)\s*(?::[^=]*)?=(.*)", re.DOTALL)
CLOSE = {"(": ")", "[": "]", "{": "}"}

IMAGE = re.compile(
    r"<\s*(?:img|picture|svg|canvas|object|embed|video|audio|iframe|map|area)\b"
    r"|\btype\s*=\s*[\"']?image\b|\burl\s*\(|\bbackground\s*=|\bbackground-image\b|\bdata:image\b",
    re.IGNORECASE,
)
VAGUE = {
    "click here", "here", "click", "tap here", "tap", "press here", "this link", "link",
    "this page", "read more", "learn more", "more", "more info", "more information", "details",
    "see more", "view", "open", "go", "continue", "haga clic aquí", "haga clic aqui", "clic aquí",
    "clic aqui", "aquí", "aqui", "pulse aquí", "toque aquí", "este enlace", "enlace", "más", "mas",
    "leer más", "ver más", "más información", "ver", "abrir", "continuar",
}  # fmt: skip
VAGUE_RE = re.compile(r"\b(?:click|tap|press) here\b|\b(?:clic|pulse|toque) aqu[ií]\b")


def scope(text: str) -> str:
    """A Rust file up to its first `#[cfg(test)]` line."""
    lines = text.split("\n")
    end = next((i for i, line in enumerate(lines) if line.strip() == "#[cfg(test)]"), len(lines))
    return "\n".join(lines[:end])


def rust_files() -> list[Path]:
    files = (ROOT / "app").glob("*/src/**/*.rs")
    return sorted(p for p in files if "tests" not in p.relative_to(ROOT / "app").parts[2:])


def skip_string(text: str, i: int) -> int:
    """The index after the string or raw string starting at `i`, else `i`."""
    raw = re.match(r'r(#*)"', text[i:])
    if raw and (i == 0 or not re.match(r"\w", text[i - 1])):
        end = text.find('"' + raw.group(1), i + len(raw.group(0)))
        return len(text) if end < 0 else end + 1 + len(raw.group(1))
    char = re.match(r"'(?:\\.|[^\\'])'", text[i:])
    if char:
        return i + len(char.group(0))
    if text[i] != '"':
        return i
    j = i + 1
    while j < len(text) and text[j] != '"':
        j += 2 if text[j] == "\\" else 1
    return j + 1


def split_top(text: str, stop: str = "") -> tuple[list[str], int]:
    """`text` split at commas outside brackets and strings, ending at an unmatched close or `stop`."""
    parts, depth, start, i = [], 0, 0, 0
    while i < len(text):
        after = skip_string(text, i)
        if after != i:
            i = after
            continue
        ch = text[i]
        if ch in CLOSE:
            depth += 1
        elif ch in ")]}" or (ch == stop and depth == 0):
            if depth == 0:
                break
            depth -= 1
        elif ch == "," and depth == 0:
            parts.append(text[start:i])
            start = i + 1
        i += 1
    parts.append(text[start:i])
    return [p.strip() for p in parts if p.strip()], i


def literals(code: str) -> list[tuple[str, list[str], int]]:
    """A11Y-019, D-4240 rule 6: each `Email`/`Rendered` literal: its type, fields and line."""
    found = []
    for m in LITERAL.finditer(code):
        if NOT_AFTER.search(code[max(0, m.start() - 40) : m.start()]):
            continue
        kind = m.group(1)
        if kind == "Self":
            above = [ln.strip() for ln in code[: m.start()].split("\n")]
            impl = next((ln for ln in reversed(above) if ln.startswith("impl")), "")
            owner = IMPL.fullmatch(impl)
            if owner is None:
                continue
            kind = owner.group(1)
        fields, _ = split_top(code[m.end() :])
        found.append((kind, fields, code.count("\n", 0, m.start())))
    return found


def let_value(code: str, line: int, name: str) -> str | None:
    """The right side of the nearest `let` above `line` binding `name`; None for `let mut` or none."""
    lines = code.split("\n")
    for i in range(line - 1, -1, -1):
        stmt = lines[i].strip()
        if not stmt.startswith("let "):
            continue
        rest = "\n".join(lines[i:])
        _, end = split_top(rest, ";")
        m = LET.match(rest[:end].strip())
        if m is None:
            binds = re.match(rf"let\s+[^=]*\b{re.escape(name)}\b", stmt)
            if binds:
                return None
            continue
        names = [n.strip() for n in m.group(1).strip("()").split(",")]
        bare = [re.sub(r"^mut\s+", "", n) for n in names]
        if name not in bare:
            continue
        k = bare.index(name)
        if names[k].startswith("mut "):
            return None
        rhs = m.group(2).strip()
        if len(names) == 1 and not m.group(1).startswith("("):
            return rhs
        if rhs.startswith("("):
            elements, _ = split_top(rhs[1:])
            return elements[k] if k < len(elements) else None
        return None
    return None


def is_empty(value: str) -> bool:
    """D-4240 rule 7: the value, whitespace removed, is an empty form."""
    flat = re.sub(r"\s+", "", value)
    return EMPTY.fullmatch(flat) is not None or EMPTY_UNWRAP.search(flat) is not None


def plain_text_faults(code: str) -> list[str]:
    """A11Y-019, D-4240 rule 7: each literal with no plain-text part, a `..` field or an empty one."""
    faults = []
    for kind, fields, line in literals(code):
        want = FIELD[kind]
        if any(f.startswith("..") for f in fields):
            faults.append(f"{kind} at line {line + 1}: `..` field")
        named = {re.match(r"\w*", f).group(0): f for f in fields}
        if want not in named:
            faults.append(f"{kind} at line {line + 1}: no `{want}`")
            continue
        field = named[want]
        value = field.split(":", 1)[1].strip() if ":" in field else field
        values = [value]
        bare = BARE.fullmatch(value)
        if bare:
            traced = let_value(code, line, bare.group(1))
            values += [] if traced is None else [traced]
        if any(is_empty(v) for v in values):
            faults.append(f"{kind} at line {line + 1}: empty `{want}`")
    return faults


def rust_scopes() -> list[tuple[str, str]]:
    return [(str(p.relative_to(ROOT)), scope(p.read_text(encoding="utf-8"))) for p in rust_files()]


def test_a11y_019_every_email_literal_has_a_plain_text_part() -> None:
    """A11Y-019, D-317: every `Email` sets `text` and every `Rendered` `plain`, never empty."""
    counts = {"Email": 0, "Rendered": 0}
    for name, code in rust_scopes():
        for kind, _, _ in literals(code):
            counts[kind] += 1
        assert plain_text_faults(code) == [], name
    assert counts["Email"] > 0 and counts["Rendered"] > 0, counts


def test_a11y_019_provider_email_keeps_both_parts() -> None:
    """A11Y-019: the provider's `Email<'a>` holds `html` and `text` and never skips one."""
    code = PROVIDER.read_text(encoding="utf-8")
    start = code.index("pub struct Email<'a> {")
    body = code[start : code.index("\n}", start)]
    assert "pub html: &'a str," in body and "pub text: &'a str," in body
    assert "skip_serializing" not in body


def email_sources() -> list[tuple[str, str]]:
    """The layout, every template that names it and every inline `source` in scope that names it."""
    found = [(str(LAYOUT.relative_to(ROOT)), LAYOUT.read_text(encoding="utf-8"))]
    for path in sorted((ROOT / "app").glob("*/templates/*.html")):
        text = path.read_text(encoding="utf-8")
        if path != LAYOUT and "email_layout" in text:
            found.append((str(path.relative_to(ROOT)), text))
    for name, code in rust_scopes():
        for m in re.finditer(r'source\s*=\s*r#"(.*?)"#', code, re.DOTALL):
            if "email_layout" in m.group(1):
                found.append((name, m.group(1)))
    return found


def link_label(inner: str) -> str:
    """A link's text: tags removed, entities unescaped, spaces squashed, end marks dropped."""
    text = html.unescape(re.sub(r"<[^>]*>", "", inner))
    return re.sub(r"\s+", " ", text).strip().rstrip(".!:;,>»").strip().lower()


def html_faults(source: str) -> list[str]:
    """A11Y-019, D-4240 rule 8: image forms, a blank body and vague or empty link text."""
    faults = [f"image: {m.group(0)}" for m in IMAGE.finditer(source)]
    body = source
    if "{% extends" in source:
        m = re.search(r"\{%-?\s*block content\s*-?%\}(.*?)\{%-?\s*endblock", source, re.DOTALL)
        body = m.group(1) if m else ""
    for pattern in (r"\{#.*?#\}", r"\{%.*?%\}", r"<(title|style|script)\b.*?</\1\s*>", r"<[^>]*>"):
        body = re.sub(pattern, " ", body, flags=re.DOTALL | re.IGNORECASE)
    if body.strip() == "":
        faults.append("no text")
    for m in re.finditer(r"<a\b[^>]*>(.*?)</a\s*>", source, re.DOTALL | re.IGNORECASE):
        label = link_label(m.group(1))
        if not re.search(r"[^\W_]", label) or label in VAGUE or VAGUE_RE.search(label):
            faults.append(f"link text: {m.group(1)!r}")
    return faults


def test_a11y_019_every_email_source_is_text_with_clear_links() -> None:
    """A11Y-019, D-317: the email sources as they stand pass, and there are six or more."""
    sources = email_sources()
    assert len(sources) >= 6, [n for n, _ in sources]
    for name, source in sources:
        assert html_faults(source) == [], name


def in_layout(body: str) -> str:
    return '{% extends "email_layout.html" %}{% block content %}\n' + body + "\n{% endblock %}"


def link(text: str) -> str:
    return in_layout(f'<p><a href="https://example.test/j/1042">{text}</a></p>')


HTML_FAIL = [
    *(in_layout(body) for body in (
        '<p>Hi</p><img src="/static/receipt.png" alt="Your receipt">',
        "<p>Hi</p><svg><title>Your receipt</title></svg>",
        '<table><tr><td background="/static/receipt.png">Hi</td></tr></table>',
        '<p style="background:url(/static/receipt.png)">Hi</p>', '<p style="background-image:none">Hi</p>',
        '<form><input type="image" alt="Confirm job 1042"></form><p>Hi</p>',
        '<p>Hi</p><img src="data:image/png;base64,AAAA" alt="Confirm job 1042">', "<p></p>",
    )),
    *(link(text) for text in (
        "Click here", "here", "Click here to confirm job 1042", "Read more.", "<strong>More</strong>", "",
        "&nbsp;", "→", "Haga clic aquí", "Aquí", "Ver más",
    )),
]  # fmt: skip

HTML_PASS = [
    *(link(t) for t in ("Confirm job 1042", "{{ label }}", "Manage email notices", "Vea su página de cotizaciones")),
    in_layout('<p style="background:#F3ECE0;">Hi</p>'),
]


@pytest.mark.parametrize("source", HTML_FAIL)
def test_a11y_019_planted_image_blank_or_vague_link_is_caught(source: str) -> None:
    """A11Y-019: each planted image form, blank body or vague link text is found."""
    assert html_faults(source) != [], source


@pytest.mark.parametrize("source", HTML_PASS)
def test_a11y_019_planted_text_and_clear_links_pass(source: str) -> None:
    """A11Y-019, D-317: link text that names the action and job, and a colour background, pass."""
    assert html_faults(source) == [], source


def email(text_field: str, lets: str = "") -> str:
    return (
        f"fn f() {{\n{lets}\n    let full = Email {{\n        from: email.from,\n"
        f"        html: &html,\n        {text_field}\n    }};\n}}\n"
    )


def rendered(plain_field: str, lets: str = "", head: str = "Rendered") -> str:
    return f"fn f() {{\n{lets}\n    Ok({head} {{\n        subject,\n        html,\n        {plain_field}\n    }})\n}}\n"


TUPLE_EMPTY = "let (html, text) = (email.html.replace(MARK, link), String::new());"
EMAIL_FAIL = [
    *(
        email(f"text: {v},")
        for v in (
            '""', "&String::new()", "&String::default()", '&"".to_string()', '&"".to_owned()',
            '&String::from("")', '&format!("")', "Default::default()", "<&str>::default()",
            'r""', "String::new().as_str()", 'body.unwrap_or("")', '&String::new()[..]', '&""[..]', "&concat!()",
            "&std::string::String::new()",
        )
    ),
    email("text: &text,", "    let text = String::new();"),
    email("text,", '    let text = "";'),
    email("text: &text,", "    " + TUPLE_EMPTY),
    email("text: text.as_str(),", "    let text = String::new();"),
    'fn f() {\n    let e = crate::api::Email { from, html: &h, text: "" };\n}\n',
    "impl<'a> Email<'a> {\n    fn blank() -> Self {\n        Self { from, html, text: \"\" }\n    }\n}\n",
    email("..other"),
    email("subject: s,"),
]  # fmt: skip

RENDERED_FAIL = [
    *(
        rendered(f"plain: {v},")
        for v in (
            "String::new()", "String::default()", "Default::default()", '"".to_string()',
            '"".to_owned()', '"".into()', 'String::from("")', 'format!("")',
            "String::with_capacity(64)", "r.plain.unwrap_or_default()",
            "r.plain.unwrap_or_else(String::new)", "r.plain.unwrap_or(String::new())",
        )
    ),
    rendered("plain,", "    let plain = String::new();"),
    "fn f() {\n    let plain = String::default();\n    Ok(Rendered {\n        subject, html,\n        plain,\n    })\n}\n",
    rendered("plain,", "    let plain: String = Default::default();"),
    rendered("plain: plain.clone(),", "    let plain = String::new();"),
    rendered("plain,", "    let (html, plain) = (view.render()?, String::new());", "email::Rendered"),
    rendered("plain: String::new(),", "", "super::email::Rendered"),
    "impl Rendered {\n    fn blank() -> Self {\n        Self { subject, html, plain: String::new() }\n    }\n}\n",
    "impl Default for Rendered {\n    fn default() -> Self {\n        Self { subject, html, plain: String::new() }\n    }\n}\n",
    rendered("..Rendered::default()"),
    rendered("lang,"),
]  # fmt: skip


@pytest.mark.parametrize("code", EMAIL_FAIL + RENDERED_FAIL)
def test_a11y_019_planted_missing_or_empty_plain_text_is_caught(code: str) -> None:
    """A11Y-019: each planted literal with no plain-text part, or an empty one, is found."""
    assert len(literals(code)) == 1, code
    assert plain_text_faults(code) != [], code


PLAIN_PASS = [
    email("text: &claim.rendered.plain,"),
    email("text: &text,", "let (html, text) = (email.html.replace(LINK_MARK, link), email.text.replace(LINK_MARK, link));"),
    rendered("plain,", '    let plain = format!("{message}\\n\\n--\\n{why}\\n");'),
    rendered("plain,", 'let page = page.map(|u| format!("{u}\\n")).unwrap_or_default();\nlet plain = format!("{page}\\n");'),
    rendered("plain: r.plain.ok_or(EmailError::Database)?,"),
    rendered('plain: lines.join("\\n"),'),
    rendered('plain: body.replace(MARK, ""),'),
    rendered("plain,", '    let mut plain = String::new();\n    writeln!(plain, "{line}")?;'),
    rendered("plain,", '    let (plain, html) = (format!("{message}\\n"), String::new());'),
    "impl Rendered {\n    fn new() -> Self {\n        Self { subject, html, plain }\n    }\n}\n",
]

NOT_LITERALS = [
    'impl Layout {\n    fn new() -> Self {\n        Self { lang: "en" }\n    }\n}\n',
    "impl ResendEmail {\n    fn new() -> Self {\n        Ok(Self { http })\n    }\n}\n",
    "impl EmailAdapter for ResendEmail {\n    fn new() -> Self {\n        Self { http }\n    }\n}\n",
    "pub struct Rendered {\n    pub plain: String,\n}\n",
    "fn render(n: Notice<'_>) -> Rendered {\n    todo()\n}\n",
    "impl fmt::Debug for Rendered {\n}\n",
    "if facts.contact != Contact::Email {\n    skip();\n}\n",
    "let Rendered { subject, html, plain } = rendered;\n",
]


@pytest.mark.parametrize("code", PLAIN_PASS)
def test_a11y_019_planted_real_plain_text_passes(code: str) -> None:
    """A11Y-019: a literal whose plain-text part is real text passes."""
    assert len(literals(code)) == 1, code
    assert plain_text_faults(code) == [], code


@pytest.mark.parametrize("code", NOT_LITERALS)
def test_a11y_019_planted_non_literals_are_not_read(code: str) -> None:
    """A11Y-019, D-4240 rule 6: a struct, signature, impl head, comparison or pattern is no literal."""
    assert literals(code) == [], code
