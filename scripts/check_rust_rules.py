"""Source rules clippy cannot enforce on its own (BUILD_BRIEF.md section 12, SEC-031, D-479, D-590).

Fails on, anywhere under app/ and tests/ (Rust files, every path but `target/`, trybuild `ui/`
fixtures included):
- `#[allow(...)]` or `#![allow(...)]` without a `reason = "..."`;
- `todo!`, `unimplemented!`, `dbg!`, and `TODO`, `FIXME`, `XXX` in any `//` or `/* */` comment;
- `unsafe` blocks, functions or impls (code only: a comment or string that says `unsafe {` is fine);
- in a status module (a file named `status.rs`), a `_ =>` or binding catch-all arm (`other =>`)
  or a `matches!` outside the status enums' own `impl` blocks. Status enums are every enum a
  `status.rs` defines (whatever its name) and every enum named `*Status` defined elsewhere
  outside `app/adapters/` (a vendor's own `*Status` enum is not ours);
- outside a status module, a glob, alias (`Status as S`) or variant import of a status enum, an
  `==` / `!=` against a variant on either side, `matches!`, `assert_eq!`/`assert_ne!` naming a
  variant, `if let` / `while let` / `let ... else` on one, a match arm on one and a catch-all
  binding arm in a match that also has such an arm;
- in a Rust file, `#[template(... escape = "none" ...)]` or `ext = "txt"`; in any file under
  `app/*/templates/`, `|safe`, `{% filter safe %}` or `escape = "none"` (D-481, D-590).

Nothing to scan (no app/ yet) exits 0. Usage: python3 scripts/check_rust_rules.py [--root .]
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ALLOW = re.compile(r"#!?\[allow\(")
ALLOW_WITH_REASON = re.compile(r"#!?\[allow\([^\]]*reason\s*=", re.S)
BANNED_MACROS = re.compile(r"\b(todo|unimplemented|dbg)!\s*\(")
BANNED_WORDS = re.compile(r"\b(TODO|FIXME)\b|\bXXX\b(?!-)")
UNSAFE = re.compile(r"\bunsafe\s*(\{|fn\b|impl\b)")
ENUM_DEF = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+(\w+)", re.M)
WILDCARD_ARM = re.compile(r"(?:^|[,{])\s*_\s*(?:if\b[^=]*)?=>")
BINDING_ARM = re.compile(r"^\s*(?:ref\s+|mut\s+)*[a-z]\w*\s*(?:if\b.*)?=>")
CHECKED_MACRO = re.compile(r"\b(matches|assert_eq|assert_ne|debug_assert_eq|debug_assert_ne)!\s*\(")
USE_STMT = re.compile(r"\buse\b[^;]*;")
TEMPLATE_ATTR = re.compile(r"#\[template\b.*?\)\s*\]", re.S)
TEMPLATE_ATTR_UNSAFE = re.compile(r"escape\s*=\s*\"none\"|\bext\s*=\s*\"txt\"")
TEMPLATE_UNSAFE = re.compile(r"\|\s*safe\b|escape\s*=\s*\"none\"|\{%-?\s*filter\s+safe\b")
RAW_STRING = re.compile(r"b?r(#*)\"")
CHAR_LITERAL = re.compile(r"'(?:\\[^']+|[^\\'])'")
VENDOR_DIR = "app/adapters/"


def split_views(text: str) -> tuple[str, str, str]:
    """Return three same-shape copies of `text` (newlines kept, so line numbers match).

    `code` blanks comments, `bare` blanks comments and string contents, `note` keeps only comments.
    """
    code: list[str] = []
    bare: list[str] = []
    note: list[str] = []

    def add(seg: str, kind: str) -> None:
        blank = "".join(c if c == "\n" else " " for c in seg)
        code.append(seg if kind != "comment" else blank)
        note.append(seg if kind == "comment" else blank)
        if kind == "string":
            bare.append(seg[0] + blank[1:-1] + seg[-1] if len(seg) > 1 else blank)
        else:
            bare.append(seg if kind == "code" else blank)

    i, n = 0, len(text)
    while i < n:
        end, kind = i + 1, "code"
        raw = RAW_STRING.match(text, i) if text[i] in "rb" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")) else None
        if text.startswith("//", i):
            end = text.find("\n", i)
            end, kind = (n if end < 0 else end), "comment"
        elif text.startswith("/*", i):
            end, depth = i + 2, 1
            while end < n and depth:
                depth += (text.startswith("/*", end)) - (text.startswith("*/", end))
                end += 2 if text.startswith(("/*", "*/"), end) else 1
            kind = "comment"
        elif raw:
            close = text.find('"' + raw.group(1), raw.end())
            end, kind = (n if close < 0 else close + 1 + len(raw.group(1))), "string"
        elif text[i] == '"':
            end = i + 1
            while end < n and text[end] != '"':
                end += 2 if text[end] == "\\" else 1
            end, kind = min(end + 1, n), "string"
        elif text[i] == "'" and CHAR_LITERAL.match(text, i):
            end, kind = CHAR_LITERAL.match(text, i).end(), "string"
        add(text[i:end], kind)
        i = end
    return "".join(code), "".join(bare), "".join(note)


def enum_names(text: str) -> list[str]:
    return ENUM_DEF.findall(split_views(text)[1])


def collect_status_names(files: list[tuple[Path, str]]) -> set[str]:
    """Status enums: every enum a `status.rs` defines, plus `*Status` enums elsewhere (not vendors')."""
    names: set[str] = set()
    for path, rel in files:
        defined = enum_names(path.read_text(encoding="utf-8", errors="replace"))
        if path.name == "status.rs":
            names.update(defined)
        elif not rel.startswith(VENDOR_DIR):
            names.update(n for n in defined if n.endswith("Status"))
    return names


def status_pattern(names: set[str], rel: str) -> str:
    """Regex alternation naming the status enums this file must not act on directly."""
    alternatives = sorted(re.escape(n) for n in names)
    if not rel.startswith(VENDOR_DIR):
        alternatives.append(r"\w*Status")
    return "(?:" + "|".join(alternatives) + ")" if alternatives else r"(?!)"


def line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def macro_calls(bare: str) -> list[tuple[str, int, str]]:
    """(macro name, line, argument text) for every matches!/assert_eq!-style call."""
    calls = []
    for m in CHECKED_MACRO.finditer(bare):
        depth, k = 1, m.end()
        while k < len(bare) and depth:
            depth += (bare[k] == "(") - (bare[k] == ")")
            k += 1
        calls.append((m.group(1), line_of(bare, m.start()), bare[m.end() : k]))
    return calls


def impl_lines(lines: list[str], names: set[str]) -> set[int]:
    """Line numbers inside an `impl` block of a status enum (its own helpers may match freely)."""
    start = re.compile(r"^\s*impl\b[^{]*\b(?:" + "|".join(sorted(re.escape(n) for n in names) or ["(?!)"]) + r")\b")
    inside: set[int] = set()
    open_impl, depth = False, 0
    for lineno, line in enumerate(lines, start=1):
        if not open_impl and start.match(line):
            open_impl, depth = True, 0
        if open_impl:
            inside.add(lineno)
            depth += line.count("{") - line.count("}")
            open_impl = not (depth <= 0 and "}" in line)
    return inside


def check_basics(code: str, bare: str, note: str, rel: str) -> list[str]:
    """allow, banned macros, banned comment words, unsafe and template-attribute escapes."""
    problems = []
    for m in ALLOW.finditer(bare):
        close = bare.find(")]", m.start())
        attribute = bare[m.start() : close if close >= 0 else len(bare)]
        if not ALLOW_WITH_REASON.search(attribute + ")]"):
            problems.append(f"{rel}:{line_of(bare, m.start())}: #[allow] without a reason")
    for m in BANNED_MACROS.finditer(bare):
        problems.append(f"{rel}:{line_of(bare, m.start())}: todo!/unimplemented!/dbg! is never merged")
    for lineno, line in enumerate(note.splitlines(), start=1):
        if BANNED_WORDS.search(line) and "check_rust_rules" not in line:
            problems.append(f"{rel}:{lineno}: TODO/FIXME/XXX comment on main")
    for m in UNSAFE.finditer(bare):
        problems.append(f"{rel}:{line_of(bare, m.start())}: unsafe code is forbidden")
    for attr in TEMPLATE_ATTR.finditer(code):
        if TEMPLATE_ATTR_UNSAFE.search(attr.group(0)):
            problems.append(f"{rel}:{line_of(code, attr.start())}: template attribute disables escaping (SEC-011)")
    return problems


def check_status_module(bare: str, names: set[str], rel: str) -> list[str]:
    """Inside status.rs, outside the enums' impl blocks: no catch-all arm, no matches!."""
    lines = bare.splitlines()
    inside = impl_lines(lines, names)
    problems = []
    for lineno, line in enumerate(lines, start=1):
        if lineno not in inside and (WILDCARD_ARM.search(line) or BINDING_ARM.match(line)):
            problems.append(f"{rel}:{lineno}: wildcard arm in a status module (SEC-031)")
    for macro, lineno, _args in macro_calls(bare):
        if macro == "matches" and lineno not in inside:
            problems.append(f"{rel}:{lineno}: matches! on a status enum outside its impl (SEC-031)")
    return problems


def check_outside_status_module(bare: str, pattern: str, rel: str) -> list[str]:
    """Outside status.rs: any way of acting on a status variant hides a new status from the compiler."""
    found: dict[int, str] = {}
    acting = "match, matches!, assert_eq!, if let or let-else on a status enum outside its status module (SEC-031)"
    for use in USE_STMT.finditer(bare):
        body = use.group(0)
        if re.search(pattern + r"::\*", body):
            found[line_of(bare, use.start())] = "glob import of a status enum (SEC-031)"
        elif re.search(pattern + r"::(?:\{|\w)", body):
            found[line_of(bare, use.start())] = "variant import of a status enum (SEC-031)"
        elif re.search(r"\b" + pattern + r"\s+as\b", body):
            found[line_of(bare, use.start())] = "alias import of a status enum (SEC-031)"
    for m in re.finditer(r"[=!]=(?!>)\s*(?:\w+::)*" + pattern + r"::\w+|\b" + pattern + r"::\w+\s*[=!]=(?!>)", bare):
        found[line_of(bare, m.start())] = "== or != on a status variant outside its status module (SEC-031)"
    for m in re.finditer(r"\blet\b[^=;]*\b" + pattern + r"::", bare):
        found[line_of(bare, m.start())] = acting
    for _macro, lineno, args in macro_calls(bare):
        if re.search(r"\b" + pattern + r"::", args):
            found[lineno] = acting
    arm = re.compile(r"^\s*\|?\s*(?:\w+::)*" + pattern + r"::\w+.*=>")
    arm_depths: set[int] = set()
    depth = 0
    for lineno, line in enumerate(bare.splitlines(), start=1):
        arm_depths = {d for d in arm_depths if d <= depth}
        if arm.match(line):
            found[lineno] = acting
            arm_depths.add(depth)
        elif depth in arm_depths and (BINDING_ARM.match(line) or WILDCARD_ARM.search(line)):
            found[lineno] = "catch-all arm in a match on a status enum (SEC-031)"
        depth += line.count("{") - line.count("}")
    return [f"{rel}:{lineno}: {message}" for lineno, message in sorted(found.items())]


def scan_file(path: Path, rel: str, names: set[str]) -> list[str]:
    code, bare, note = split_views(path.read_text(encoding="utf-8", errors="replace"))
    problems = check_basics(code, bare, note, rel)
    if path.name == "status.rs":
        problems += check_status_module(bare, names, rel)
    else:
        problems += check_outside_status_module(bare, status_pattern(names, rel), rel)
    return problems


def scan_template(path: Path, rel: str) -> list[str]:
    problems = []
    for lineno, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), start=1):
        if TEMPLATE_UNSAFE.search(line):
            problems.append(f"{rel}:{lineno}: template disables escaping (SEC-011)")
    return problems


def run(root: Path) -> list[str]:
    files: list[tuple[Path, str]] = []
    for folder in ("app", "tests"):
        base = root / folder
        if base.is_dir():
            files += [(p, p.relative_to(root).as_posix()) for p in sorted(base.rglob("*.rs"))]
    files = [(p, rel) for p, rel in files if "/target/" not in f"/{rel}"]
    names = collect_status_names(files)
    problems: list[str] = []
    for path, rel in files:
        problems += scan_file(path, rel, names)
    if (root / "app").is_dir():
        for path in sorted((root / "app").glob("*/templates/**/*")):
            if path.is_file():
                problems += scan_template(path, path.relative_to(root).as_posix())
    return problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", default=".")
    args = parser.parse_args(argv)
    problems = run(Path(args.root).resolve())
    for p in problems:
        print(f"ERROR {p}", file=sys.stderr)
    print(f"rust rules: {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
