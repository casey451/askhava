"""Source rules clippy cannot enforce on its own (BUILD_BRIEF.md section 12, SEC-031, D-479).

Fails on, anywhere under app/ and tests/ (Rust files):
- `#[allow(...)]` or `#![allow(...)]` without a `reason = "..."` (clippy is also run with
  `-F clippy::allow_attributes_without_reason`; this scan catches it in a session too);
- `todo!`, `unimplemented!`, `dbg!`, and `TODO`, `FIXME`, `XXX` comments;
- `unsafe` blocks or functions (every crate also sets `unsafe_code = "forbid"`);
- in a status module (a file under `app/*/src/` whose name is `status.rs` or that defines a
  `Status` enum), a wildcard match arm `_ =>` or a `matches!(` on a status enum, as SEC-031
  requires, outside the enum's own `impl` block;
- outside a status module, a glob import of a status enum (`use ...Status::*`) or an `==` /
  `!=` comparison against a status variant, which would hide a new status from the compiler;
- in any Askama template under `app/*/templates/`, `|safe` or `escape = "none"` (D-481).

Nothing to scan (no app/ yet) exits 0. Usage: python3 scripts/check_rust_rules.py [--root .]
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ALLOW = re.compile(r"#!?\[allow\(")
ALLOW_WITH_REASON = re.compile(r"#!?\[allow\([^\]]*reason\s*=")
BANNED_MACROS = re.compile(r"\b(todo|unimplemented|dbg)!\s*\(")
BANNED_WORDS = re.compile(r"\b(TODO|FIXME|XXX)\b")
UNSAFE = re.compile(r"\bunsafe\s*(\{|fn\b|impl\b)")
STATUS_ENUM = re.compile(r"^\s*pub(?:\(crate\))?\s+enum\s+\w*Status\b", re.M)
WILDCARD_ARM = re.compile(r"^\s*_\s*=>")
MATCHES_MACRO = re.compile(r"\bmatches!\s*\(")
IMPL_START = re.compile(r"^\s*impl\b.*\b(\w*Status)\b")
STATUS_GLOB = re.compile(r"\buse\b[^;]*\w*Status::\*")
STATUS_EQ = re.compile(r"[=!]=\s*(?:\w+::)*\w*Status::\w+")
TEMPLATE_UNSAFE = re.compile(r"\|\s*safe\b|escape\s*=\s*\"none\"")


def scan_file(path: Path, rel: str) -> list[str]:
    text = path.read_text(encoding="utf-8", errors="replace")
    problems: list[str] = []
    is_status_module = path.name == "status.rs" or bool(STATUS_ENUM.search(text))
    in_status_impl = False
    depth = 0
    for lineno, line in enumerate(text.splitlines(), start=1):
        where = f"{rel}:{lineno}"
        if ALLOW.search(line) and not ALLOW_WITH_REASON.search(line):
            problems.append(f"{where}: #[allow] without a reason")
        if BANNED_MACROS.search(line):
            problems.append(f"{where}: todo!/unimplemented!/dbg! is never merged")
        if BANNED_WORDS.search(line) and "check_rust_rules" not in line:
            problems.append(f"{where}: TODO/FIXME/XXX comment on main")
        if UNSAFE.search(line):
            problems.append(f"{where}: unsafe code is forbidden")
        if is_status_module:
            if not in_status_impl and IMPL_START.match(line):
                in_status_impl, depth = True, 0
            if in_status_impl:
                depth += line.count("{") - line.count("}")
                if depth <= 0 and "}" in line:
                    in_status_impl = False
                continue
            if WILDCARD_ARM.match(line):
                problems.append(f"{where}: wildcard arm in a status module (SEC-031)")
            if MATCHES_MACRO.search(line):
                problems.append(f"{where}: matches! on a status enum outside its impl (SEC-031)")
        else:
            if STATUS_GLOB.search(line):
                problems.append(f"{where}: glob import of a status enum (SEC-031)")
            if STATUS_EQ.search(line):
                problems.append(f"{where}: == or != on a status variant outside its status module (SEC-031)")
    return problems


def scan_template(path: Path, rel: str) -> list[str]:
    problems = []
    for lineno, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), start=1):
        if TEMPLATE_UNSAFE.search(line):
            problems.append(f"{rel}:{lineno}: template disables escaping (SEC-011)")
    return problems


def run(root: Path) -> list[str]:
    problems: list[str] = []
    for folder in ("app", "tests"):
        base = root / folder
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            if "/target/" in f"/{rel}" or "/ui/" in f"/{rel}":  # trybuild fixtures may break rules on purpose
                continue
            problems += scan_file(path, rel)
    if (root / "app").is_dir():
        for path in sorted((root / "app").glob("*/templates/**/*.html")):
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
