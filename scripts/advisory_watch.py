"""Advisory watch: tell Casey about a new RustSec or npm advisory before it fails hand-ins (D-4269, SEC-021).

`.github/workflows/advisory-watch.yml` runs this every 4 hours on `main`. It only reports: a found
advisory exits 0. It writes nothing but `outbox/*.md` in a `claude/kit-state` checkout (new
`outbox/advisory-<ID>.md` files and the `dispatched:` front-matter line), never `kit/nightly.json` or
`kit/state.json`. The outbox id is the dedupe record; no other state file exists.

Usage:
  python3 scripts/advisory_watch.py scan --cargo-audit audit.json --cargo-audit-exit N
      --cargo-deny deny.json --cargo-deny-exit N --npm npm.json --npm-exit N --kit-state DIR --out found.json
  python3 scripts/advisory_watch.py report --found found.json --kit-state DIR
Both print `new=<n>` and `pending=<n>` (and append them to `$GITHUB_OUTPUT` when it is set).
Exit 0 on success (advisories found or not), 2 when a report cannot be judged (a `::error::` line names
the tool and nothing is written).

What is read (shapes of the pinned tool versions, checked against real runs):
- cargo-audit 0.22.2 `cargo audit --json` (stdout): one object `{database, lockfile, settings,
  vulnerabilities: {found, count, list: [{advisory: {id, package, title, ...}, versions: {patched: [..],
  unaffected: [..]}, affected, package: {name, version, source, ...}}]}, warnings: {<kind>: [..]}}`.
  Only `vulnerabilities.list` fails `cargo audit`; the informational `warnings` are skipped.
- cargo-deny 0.20.2 `cargo deny --format json check advisories` (stderr): JSON Lines, each
  `{type, fields}`. `type` `log` and `summary` are skipped; `type` `diagnostic` has `fields: {severity,
  code, message, advisory: {id, title, ...} | absent, graphs: [{Krate: {name, version}, parents}],
  labels, notes: ["ID: ..", .., "Solution: Upgrade to <range> (try ..)" | "Solution: No safe upgrade
  is available!"]}`. Only `severity` `error` counts; `code` `yanked` has no advisory and becomes
  `yanked-<crate>-<version>`. Any other `type`, or a line that is not JSON, exits 2.
- npm `npm audit --json` (auditReportVersion 2): judged by `scripts/npm_audit.py` (`parse_report`,
  then `judge` with its `EXCEPTIONS`), so `Ci` and the watch agree; each problem it names becomes one
  entry (its GHSA id, else `npm-<package>`), its `version` the package's affected `range` and its
  `patched` from `fixAvailable` (`{name, version}`) or the advisory's `<X` range.
Both cargo tools apply `.cargo/audit.toml` and `deny.toml` ignores themselves.

`pending` counts each `outbox/*.md` (any kind, not `outbox/sent/`) whose last commit on the
kit-state branch is over 30 minutes old and whose `dispatched:` is not today (UTC); a file whose front
matter does not parse the way `notify.yml`'s `parse()` reads it is left out and never stamped.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import npm_audit

ID = re.compile(r"[A-Za-z0-9._-]+")
KINDS = {"question", "approval", "gate", "blocked", "info"}
DENY_CODES = {"vulnerability", "unmaintained", "unsound"}
SETTLE_SECONDS = 30 * 60
NPM_PROBLEM = re.compile(r"(?P<root>[^:]+): (?P<ident>\S+) on (?P<pkg>\S+) \([^)]*\): (?P<title>.*)", re.DOTALL)
NPM_ORPHAN = re.compile(r"(?P<root>[^:]+): reported vulnerable but traces to no advisory")
NOTHING = "Nothing needs you; the next plan change updates the lock (D-4141, D-4223)."


class WatchError(Exception):
    """A report cannot be judged; the message names the tool."""


def entry(ident: str, ecosystem: str, package: str, version: str, title: str, patched: str) -> dict[str, str]:
    """One found advisory, in the shape `found.json` holds."""
    return {"id": ident, "ecosystem": ecosystem, "package": package, "version": version, "title": title,
            "patched": patched}


def read_text(path: Path, tool: str) -> str:
    """The report's text, or WatchError when it is missing or empty."""
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise WatchError(f"{tool}: cannot read {path}: {exc}") from exc
    if not text.strip():
        raise WatchError(f"{tool}: report {path} is empty")
    return text


def cargo_audit(text: str, exit_code: int) -> list[dict[str, str]]:
    """Entries from a cargo-audit 0.22.2 JSON report (vulnerabilities only; warnings never fail it)."""
    try:
        listed = json.loads(text)["vulnerabilities"]["list"]
        found = [entry(v["advisory"]["id"], "crates", v["package"]["name"], v["package"]["version"],
                       v["advisory"]["title"], ", ".join(v["versions"]["patched"])) for v in listed]
    except (ValueError, KeyError, TypeError) as exc:
        raise WatchError(f"cargo-audit: report is not JSON of the known shape: {exc!r}") from exc
    if exit_code and not found:
        raise WatchError(f"cargo-audit: exited {exit_code} with no vulnerability listed")
    return found


def deny_patched(notes: list[Any]) -> str:
    """The upgrade range from a cargo-deny `Solution: Upgrade to ...` note, or ''."""
    for note in notes:
        text = str(note)
        if text.startswith("Solution: Upgrade to "):
            return text.removeprefix("Solution: Upgrade to ").split(" (try", 1)[0].strip()
    return ""


def deny_entry(fields: dict[str, Any]) -> dict[str, str]:
    """One entry from an error-severity cargo-deny diagnostic, or WatchError for an unknown one."""
    krate = fields["graphs"][0]["Krate"]
    name, version = krate["name"], krate["version"]
    code = fields.get("code")
    if code == "yanked":
        return entry(f"yanked-{name}-{version}", "crates", name, version, f"{name} {version} is yanked", "")
    advisory = fields.get("advisory")
    if code not in DENY_CODES or not isinstance(advisory, dict):
        raise WatchError(f"cargo-deny: unknown error diagnostic {code!r}: {fields.get('message')!r}")
    return entry(advisory["id"], "crates", name, version, advisory["title"], deny_patched(fields.get("notes", [])))


def cargo_deny(text: str, exit_code: int) -> list[dict[str, str]]:
    """Entries from cargo-deny 0.20.2 JSON Lines (error-severity diagnostics only)."""
    found: list[dict[str, str]] = []
    for number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        try:
            item = json.loads(line)
            kind = item["type"]
            if kind in ("log", "summary"):
                continue
            if kind != "diagnostic":
                raise WatchError(f"cargo-deny: line {number} has unknown type {kind!r}")
            if item["fields"]["severity"] == "error":
                found.append(deny_entry(item["fields"]))
        except (ValueError, KeyError, TypeError, IndexError) as exc:
            raise WatchError(f"cargo-deny: line {number} is not JSON of the known shape: {exc!r}") from exc
    if exit_code and not found:
        raise WatchError(f"cargo-deny: exited {exit_code} with no error diagnostic")
    return found


def npm_patched(vuln: dict[str, Any], raw: str) -> str:
    """The fixed version npm names: `fixAvailable`'s {name, version}, else the `<X` range of the advisory
    `judge` named `raw` (its GHSA id, else its url or source)."""
    fix = vuln.get("fixAvailable")
    if isinstance(fix, dict) and fix.get("name") and fix.get("version"):
        return f"{fix['name']} {fix['version']}"
    for item in vuln.get("via") or []:
        if isinstance(item, dict) and (npm_audit.advisory_id(item)
                                       or str(item.get("url") or item.get("source"))) == raw:
            found = re.fullmatch(r"<\s*([0-9][^\s|]*)", str(item.get("range", "")))
            if found:
                return f">={found.group(1)}"
    return ""


def npm_entry(problem: str, vulns: dict[str, Any]) -> dict[str, str]:
    """One entry from one problem line `npm_audit.judge` printed."""
    orphan = NPM_ORPHAN.fullmatch(problem)
    if orphan:
        pkg, ident, title = orphan["root"], "", "reported vulnerable but traces to no advisory"
    else:
        found = NPM_PROBLEM.fullmatch(problem)
        if not found:
            raise WatchError(f"npm: problem of an unknown shape: {problem!r}")
        pkg, ident, title = found["pkg"], found["ident"], found["title"]
    vuln = vulns.get(pkg) or {}
    patched = npm_patched(vuln, ident)
    if not npm_audit.GHSA.fullmatch(ident):
        ident = "npm-" + re.sub(r"[^A-Za-z0-9._-]+", "-", pkg).strip("-")
    return entry(ident, "npm", pkg, str(vuln.get("range", "")), title, patched)


def npm(text: str, exit_code: int) -> list[dict[str, str]]:
    """Entries for every advisory `scripts/npm_audit.py` would fail `Ci` on (its exceptions apply)."""
    try:
        vulns = npm_audit.parse_report(text)
        problems = npm_audit.judge(vulns, npm_audit.EXCEPTIONS)[0]
    except npm_audit.AuditError as exc:
        raise WatchError(f"npm: {exc}") from exc
    if exit_code and not vulns:
        raise WatchError(f"npm: exited {exit_code} with no vulnerable package listed")
    return [npm_entry(problem, vulns) for problem in problems]


def merge(found: list[dict[str, str]]) -> list[dict[str, str]]:
    """Sorted and deduplicated by (id, package, version), keeping a known patched version."""
    kept: dict[tuple[str, str, str], dict[str, str]] = {}
    for item in found:
        if not ID.fullmatch(item["id"]):
            raise WatchError(f"{item['ecosystem']}: advisory id {item['id']!r} has characters outside A-Za-z0-9._-")
        key = (item["id"], item["package"], item["version"])
        if key not in kept or (not kept[key]["patched"] and item["patched"]):
            kept[key] = item
    return [kept[key] for key in sorted(kept)]


def parse_front(text: str) -> dict[str, str]:
    """Front matter read exactly as `parse()` in `.github/workflows/notify.yml` reads it, or ValueError."""
    text = text.replace("\r\n", "\n")
    if not text.startswith("---\n"):
        raise ValueError("missing front matter")
    head, sep, _ = text[4:].partition("\n---\n")
    if not sep:
        if not text[4:].endswith("\n---"):
            raise ValueError("unterminated front matter")
        head = text[4:-4]
    meta: dict[str, str] = {}
    for line in head.split("\n"):
        if line.strip():
            key, colon, val = line.partition(":")
            if not colon:
                raise ValueError(f"bad front matter line: {line!r}")
            meta[key.strip()] = val.strip()
    if not ID.fullmatch(meta.get("id", "")) or not meta.get("title") or meta.get("kind") not in KINDS:
        raise ValueError("id, title or kind invalid")
    return meta


def today() -> str:
    """Today's date in UTC, YYYY-MM-DD."""
    return datetime.now(UTC).date().isoformat()


def committed_at(kit_state: Path, path: Path) -> int | None:
    """The Unix time of the last commit that touched `path` in the kit-state checkout, or None."""
    done = subprocess.run(["git", "log", "-1", "--format=%ct", "--", str(path.relative_to(kit_state))],
                          cwd=kit_state, capture_output=True, text=True, check=False)
    stamp = done.stdout.strip()
    return int(stamp) if done.returncode == 0 and stamp.isdigit() else None


def pending(kit_state: Path) -> list[Path]:
    """Unsent outbox files left over 30 minutes, not dispatched today and parsing as notify reads them."""
    found: list[Path] = []
    now, day = time.time(), today()
    for path in sorted((kit_state / "outbox").glob("*.md")):
        try:
            meta = parse_front(path.read_text(encoding="utf-8"))
        except (OSError, ValueError, UnicodeDecodeError):
            continue
        stamp = committed_at(kit_state, path)
        if stamp is not None and now - stamp > SETTLE_SECONDS and meta.get("dispatched") != day:
            found.append(path)
    return found


def new_ids(found: list[dict[str, str]], kit_state: Path) -> list[str]:
    """Advisory ids with no `outbox/advisory-<ID>.md` in outbox/ or outbox/sent/."""
    outbox = kit_state / "outbox"
    ids = sorted({item["id"] for item in found})
    return [i for i in ids if not (outbox / f"advisory-{i}.md").exists()
            and not (outbox / "sent" / f"advisory-{i}.md").exists()]


def stamp(path: Path, day: str) -> None:
    """Set the front-matter line `dispatched: <day>` (replaced or added); nothing else changes."""
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
    end = next(i for i in range(1, len(lines)) if lines[i].rstrip("\r\n") == "---")
    newline = lines[0][3:] or "\n"
    for i in range(1, end):
        if lines[i].partition(":")[0].strip() == "dispatched":
            ending = lines[i][len(lines[i].rstrip("\r\n")):]
            lines[i] = f"dispatched: {day}{ending}"
            break
    else:
        lines.insert(end, f"dispatched: {day}{newline}")
    path.write_text("".join(lines), encoding="utf-8")


def outbox_text(ident: str, items: list[dict[str, str]]) -> str:
    """The outbox file for one advisory id (`kit/outbox_format.md`)."""
    first = items[0]
    lines = [f"- {i['package']} {i['version']} ({i['ecosystem']}): {i['title']}; fixed in "
             f"{i['patched'] or 'no patched release yet'}." for i in items]
    return (f"---\nid: advisory-{ident}\ntitle: Advisory {ident} on {first['package']} {first['version']}\n"
            f"kind: info\n---\nThe advisory watch found advisory {ident} against a locked dependency; it will "
            f"fail every hand-in that touches Rust or npm inputs.\n\n" + "\n".join(lines) + f"\n\n{NOTHING}\n")


def emit(new: int, waiting: int) -> None:
    """Print the counts and append them to `$GITHUB_OUTPUT` when it is set."""
    text = f"new={new}\npending={waiting}\n"
    print(text, end="")
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as out:
            out.write(text)


def scan(args: argparse.Namespace) -> int:
    """Judge the three reports, write found.json and print the counts."""
    found = merge([*cargo_audit(read_text(args.cargo_audit, "cargo-audit"), args.cargo_audit_exit),
                   *cargo_deny(read_text(args.cargo_deny, "cargo-deny"), args.cargo_deny_exit),
                   *npm(read_text(args.npm, "npm"), args.npm_exit)])
    args.out.write_text(json.dumps(found, indent=1) + "\n", encoding="utf-8")
    emit(len(new_ids(found, args.kit_state)), len(pending(args.kit_state)))
    return 0


def load_found(path: Path) -> list[dict[str, str]]:
    """found.json as `scan` wrote it, or WatchError."""
    try:
        found = json.loads(read_text(path, "found.json"))
        return merge([entry(*(str(item[k]) for k in ("id", "ecosystem", "package", "version", "title",
                                                     "patched"))) for item in found])
    except (ValueError, KeyError, TypeError) as exc:
        raise WatchError(f"found.json: not the shape scan writes: {exc!r}") from exc


def report(args: argparse.Namespace) -> int:
    """Recompute new and pending from this checkout, write new files and stamp everything dispatched."""
    found = load_found(args.found)
    fresh = new_ids(found, args.kit_state)
    waiting = pending(args.kit_state)
    day = today()
    for ident in fresh:
        path = args.kit_state / "outbox" / f"advisory-{ident}.md"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(outbox_text(ident, [i for i in found if i["id"] == ident]), encoding="utf-8")
        waiting.append(path)
    for path in waiting:
        stamp(path, day)
    emit(len(fresh), len(waiting) - len(fresh))
    return 0


def parser() -> argparse.ArgumentParser:
    """The two subcommands and their arguments."""
    top = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = top.add_subparsers(dest="command", required=True)
    one = sub.add_parser("scan")
    for name in ("cargo-audit", "cargo-deny", "npm"):
        one.add_argument(f"--{name}", type=Path, required=True)
        one.add_argument(f"--{name}-exit", type=int, required=True)
    one.add_argument("--out", type=Path, required=True)
    two = sub.add_parser("report")
    two.add_argument("--found", type=Path, required=True)
    for each in (one, two):
        each.add_argument("--kit-state", type=Path, required=True)
    return top


def main(argv: list[str] | None = None) -> int:
    """Run one subcommand; 2 with a `::error::` line when a report cannot be judged."""
    args = parser().parse_args(argv)
    try:
        return scan(args) if args.command == "scan" else report(args)
    except WatchError as exc:
        print(f"::error::advisory watch: {exc}")
        return 2


if __name__ == "__main__":
    sys.exit(main())
