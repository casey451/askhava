"""Duplicate-function scan the stage cleanups run (T100, T126, T142; D-1362, D-1607, D-3870).

Finds non-test Rust functions under app/ whose bodies are identical after comments are dropped and
whitespace is normalized, with 4 or more non-comment lines, in two or more places. Test code is
skipped: files under a `tests/` directory (`src/tests/` included), `target/`, files with an inner
`#![cfg(test)]`, and every item behind `#[cfg(test)]`, `#[test]` or `#[tokio::test]`.

Prints one line per group (`duplicate:` or `kept (<reason>):`, then each place as
`path:line fn_name`) and a count. A group in KEEP (below) is printed as kept and is never a finding.

Exit 0 by default (report-only); `--strict` exits 1 while any group is not kept, which is what a
cleanup's "zero findings" means. Usage: python3 scripts/check_duplicates.py [--root .] [--strict]
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

from check_rust_rules import split_views

MIN_LINES = 4

# Groups that stay, each with its reason: a group whose set of `path fn_name` places equals an
# entry's set is printed as kept. Only groups that are not real duplicates belong here (D-3870).
KEEP: tuple[tuple[frozenset[str], str], ...] = (
    (
        frozenset({"app/businesses/src/channel_preference.rs as_str", "app/portals/src/business_signup.rs value"}),
        "two enums' strings that only look alike: the stored NoticeChannel and the posted form's NoticeChoice",
    ),
    (
        frozenset({"app/licensing/src/daily_lists.rs name", "app/licensing/src/own_record.rs code"}),
        "two enums' strings that only look alike: DailyList's settings name and RecordList's adapter path",
    ),
    (
        frozenset({"app/businesses/src/backup_address.rs code_of", "app/businesses/src/email_verification.rs code_of"}),
        "each keys its code with its own CODE_ACTION (backup_code, mailbox_code), so the codes never cross",
    ),
    (
        frozenset({"app/messaging/src/mailbox_code_mail.rs new", "app/messaging/src/signin_mail.rs new"}),
        "constructors of two different mailers (Self is MailboxCodeSender, SignInLinkSender)",
    ),
)

FN_NAME = re.compile(r"\bfn\s+(\w+)")
TEST_ATTR = re.compile(r"#\[\s*(?:cfg\s*\(\s*test\s*\)|test|tokio\s*::\s*test\b[^\]]*)\s*\]")
INNER_TEST_CFG = re.compile(r"#!\[\s*cfg\s*\(\s*test\s*\)\s*\]")
SPACES = re.compile(r"\s+")
PUNCT_SPACE = re.compile(r" ?([^\w ]) ?")


def item_stop(bare: str, start: int) -> int:
    """Index of the first `{` or `;` at paren/bracket depth 0 from `start`, or -1.

    Angle brackets are never counted: `<` is also a comparison or a shift (`1 << 10`), and `->`
    holds a `>`; a generic's bounds never contain `{` or `;`, so they need no depth.
    """
    depth = 0
    for i in range(start, len(bare)):
        c = bare[i]
        if c in "([":
            depth += 1
        elif c in ")]":
            depth = max(depth - 1, 0)
        elif c in "{;" and depth == 0:
            return i
    return -1


def brace_end(bare: str, open_at: int) -> int:
    """Index just past the `}` that closes the `{` at `open_at` (end of text when unbalanced)."""
    depth = 0
    for i in range(open_at, len(bare)):
        if bare[i] == "{":
            depth += 1
        elif bare[i] == "}":
            depth -= 1
            if depth == 0:
                return i + 1
    return len(bare)


def item_end(bare: str, start: int) -> int:
    """Index just past the item that starts at `start`: its `;`, or its `{...}` block."""
    stop = item_stop(bare, start)
    if stop < 0:
        return len(bare)
    return stop + 1 if bare[stop] == ";" else brace_end(bare, stop)


def blank(text: str, start: int, end: int) -> str:
    """`text` with `[start, end)` replaced by spaces, newlines kept so line numbers stay."""
    return text[:start] + "".join(c if c == "\n" else " " for c in text[start:end]) + text[end:]


def strip_tests(code: str, bare: str) -> tuple[str, str]:
    """Blank every test-only item in both views (same shape, so offsets agree)."""
    if INNER_TEST_CFG.search(bare):
        return blank(code, 0, len(code)), blank(bare, 0, len(bare))
    pos = 0
    while match := TEST_ATTR.search(bare, pos):
        end = item_end(bare, match.end())
        code, bare = blank(code, match.start(), end), blank(bare, match.start(), end)
        pos = end
    return code, bare


def normalize(body: str) -> tuple[str, int]:
    """Whitespace-normalized body and its count of non-blank (non-comment) lines."""
    lines = [SPACES.sub(" ", line).strip() for line in body.splitlines()]
    lines = [line for line in lines if line]
    return PUNCT_SPACE.sub(r"\1", " ".join(lines)), len(lines)


def functions(text: str) -> list[tuple[str, int, str]]:
    """(fn name, line, normalized body) of each non-test function with at least MIN_LINES lines."""
    code, bare, _ = split_views(text)
    code, bare = strip_tests(code, bare)
    found = []
    for match in FN_NAME.finditer(bare):
        stop = item_stop(bare, match.end())
        if stop < 0 or bare[stop] != "{":
            continue
        key, count = normalize(code[stop + 1 : brace_end(bare, stop) - 1])
        if count >= MIN_LINES:
            found.append((match.group(1), bare.count("\n", 0, match.start()) + 1, key))
    return found


def rust_files(root: Path) -> list[tuple[Path, str]]:
    """Every non-test `.rs` file under app/, with its repository-relative path."""
    base = root / "app"
    if not base.is_dir():
        return []
    files = [(p, p.relative_to(root).as_posix()) for p in sorted(base.rglob("*.rs"))]
    return [(p, rel) for p, rel in files if not {"tests", "target"} & set(rel.split("/")[:-1])]


def groups(root: Path) -> list[list[tuple[str, int, str]]]:
    """Each group of two or more places (path, line, fn name) that share one body, in path order."""
    by_body: dict[str, list[tuple[str, int, str]]] = {}
    for path, rel in rust_files(root):
        for name, line, key in functions(path.read_text(encoding="utf-8", errors="replace")):
            by_body.setdefault(key, []).append((rel, line, name))
    return [places for places in by_body.values() if len(places) > 1]


def kept_reason(places: list[tuple[str, int, str]]) -> str | None:
    """The KEEP reason for this group, or None when it is a finding."""
    names = frozenset(f"{rel} {name}" for rel, _, name in places)
    return next((reason for keep, reason in KEEP if keep == names), None)


def report(root: Path) -> int:
    """Print every group and the count; return the number of findings (groups not kept)."""
    findings = kept = 0
    for places in groups(root):
        where = ", ".join(f"{rel}:{line} {name}" for rel, line, name in places)
        reason = kept_reason(places)
        if reason is None:
            findings += 1
            print(f"duplicate: {where}")
        else:
            kept += 1
            print(f"kept ({reason}): {where}")
    print(f"duplicates: {findings + kept} group(s), {kept} kept, {findings} finding(s)")
    return findings


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", default=".")
    parser.add_argument("--strict", action="store_true", help="exit 1 while any group is not kept")
    args = parser.parse_args(argv)
    findings = report(Path(args.root).resolve())
    return 1 if args.strict and findings else 0


if __name__ == "__main__":
    raise SystemExit(main())
