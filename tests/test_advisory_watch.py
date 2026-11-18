"""scripts/advisory_watch.py and .github/workflows/advisory-watch.yml (D-4269, SEC-021).

Every report is planted by the test into `tmp_path` in the shape the pinned tools print (cargo-audit
0.22.2, cargo-deny 0.20.2 JSON Lines, npm auditReportVersion 2); no test runs a tool or reaches the
network. Each kit-state checkout is a scratch git repository with planted commit dates.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import time
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

import advisory_watch as watch
import pytest
import yaml

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github" / "workflows" / "advisory-watch.yml"
RUSTSEC = "RUSTSEC-2099-0001"
PICOMATCH = "GHSA-fc5q-39xc-vjpf"
OTHER = "GHSA-2222-3333-4444"
TODAY = datetime.now(UTC).date()


def audit(*ids: str) -> str:
    """A cargo-audit report listing one vulnerability per id."""
    listed = [{"advisory": {"id": i, "package": "fakecrate", "title": f"planted flaw {i}"},
               "versions": {"patched": [">=1.2.4"], "unaffected": []}, "affected": None,
               "package": {"name": "fakecrate", "version": "1.2.3", "source": "registry"}} for i in ids]
    return json.dumps({"database": {}, "lockfile": {}, "settings": {},
                       "vulnerabilities": {"found": bool(ids), "count": len(ids), "list": listed},
                       "warnings": {"unmaintained": [{"kind": "unmaintained", "advisory": {"id": "RUSTSEC-2099-0009"}}]}})


def diagnostic(severity: str, ident: str, code: str = "vulnerability") -> dict[str, Any]:
    """One cargo-deny diagnostic line."""
    return {"type": "diagnostic", "fields": {
        "severity": severity, "code": code, "message": f"planted {ident}",
        "advisory": {"id": ident, "title": f"planted flaw {ident}"},
        "graphs": [{"Krate": {"name": "fakecrate", "version": "1.2.3"}, "parents": []}],
        "labels": [], "notes": [f"ID: {ident}", "Solution: Upgrade to >=1.2.4 (try `cargo update -p fakecrate`)"]}}


def deny(*items: dict[str, Any]) -> str:
    """cargo-deny JSON Lines: a log line, the items, a summary line."""
    lines = [{"type": "log", "fields": {"level": "INFO", "message": "fetched"}}, *items,
             {"type": "summary", "fields": {"advisories": {"errors": 0}}}]
    return "".join(json.dumps(line) + "\n" for line in lines)


def npm_vuln(name: str, via: list[Any]) -> dict[str, Any]:
    """One npm vulnerable-package entry."""
    return {"name": name, "severity": "high", "isDirect": False, "via": via, "effects": [], "range": "<=4.0.7",
            "nodes": [f"node_modules/{name}"], "fixAvailable": False}


def npm_advisory(name: str, url: str) -> dict[str, Any]:
    """One npm advisory inside a `via` list."""
    return {"source": 1, "name": name, "dependency": name, "title": f"planted advisory on {name}", "url": url,
            "severity": "high", "range": "<4.0.8"}


def npm(vulns: dict[str, Any]) -> str:
    """An npm audit report around a vulnerabilities map."""
    return json.dumps({"auditReportVersion": 2, "vulnerabilities": vulns,
                       "metadata": {"vulnerabilities": {"total": len(vulns)}}})


def picomatch_only() -> dict[str, Any]:
    """The D-4223-excused picomatch advisory and nothing else."""
    return {"picomatch": npm_vuln("picomatch", [npm_advisory("picomatch", f"https://github.com/advisories/{PICOMATCH}")])}


def git(repo: Path, *args: str, when: float | None = None) -> str:
    """Run git in the scratch repository, committing at `when` (Unix time) if given."""
    env = {**os.environ, "GIT_AUTHOR_NAME": "Test", "GIT_AUTHOR_EMAIL": "test@example.test",
           "GIT_COMMITTER_NAME": "Test", "GIT_COMMITTER_EMAIL": "test@example.test"}
    if when is not None:
        env["GIT_AUTHOR_DATE"] = env["GIT_COMMITTER_DATE"] = f"@{int(when)} +0000"
    return subprocess.run(["git", *args], cwd=repo, env=env, check=True, capture_output=True, text=True).stdout


def plant(repo: Path, name: str, text: str, age: float) -> Path:
    """Commit `text` as `name` in the repository, `age` seconds ago."""
    path = repo / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    git(repo, "add", name)
    git(repo, "commit", "-q", "-m", name, when=time.time() - age)
    return path


@pytest.fixture
def kit(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """A kit-state checkout with kit/nightly.json, kit/state.json and an empty outbox."""
    monkeypatch.delenv("GITHUB_OUTPUT", raising=False)
    repo = tmp_path / "kit-state"
    repo.mkdir()
    git(repo, "init", "-q")
    plant(repo, "kit/nightly.json", '{"conclusion": "success"}\n', 7200)
    plant(repo, "kit/state.json", '{"tasks": {}}\n', 7200)
    (repo / "outbox" / "sent").mkdir(parents=True)
    return repo


def scan(tmp: Path, kit: Path, cargo: str = audit(), deny_text: str = deny(), npm_text: str = npm({}),
         exits: tuple[int, int, int] = (0, 0, 0)) -> int:
    """Write the three reports and run `scan`."""
    for name, text in (("audit.json", cargo), ("deny.json", deny_text), ("npm.json", npm_text)):
        (tmp / name).write_text(text, encoding="utf-8")
    return watch.main(["scan", "--cargo-audit", str(tmp / "audit.json"), "--cargo-audit-exit", str(exits[0]),
                       "--cargo-deny", str(tmp / "deny.json"), "--cargo-deny-exit", str(exits[1]),
                       "--npm", str(tmp / "npm.json"), "--npm-exit", str(exits[2]),
                       "--kit-state", str(kit), "--out", str(tmp / "found.json")])


def report(tmp: Path, kit: Path) -> int:
    """Run `report` on the scan's found.json."""
    return watch.main(["report", "--found", str(tmp / "found.json"), "--kit-state", str(kit)])


def counts(text: str) -> tuple[int, int]:
    """The last new= and pending= values printed."""
    return (int(re.findall(r"^new=(\d+)$", text, re.MULTILINE)[-1]), int(re.findall(r"^pending=(\d+)$", text, re.MULTILINE)[-1]))


def found(tmp: Path) -> list[dict[str, str]]:
    """found.json as written."""
    return json.loads((tmp / "found.json").read_text(encoding="utf-8"))


def test_a_new_rustsec_id_gives_new_1_and_one_outbox_file(tmp_path: Path, kit: Path, capsys) -> None:
    assert scan(tmp_path, kit, cargo=audit(RUSTSEC), exits=(1, 0, 0)) == 0
    assert counts(capsys.readouterr().out) == (1, 0)
    assert report(tmp_path, kit) == 0
    assert counts(capsys.readouterr().out) == (1, 0)
    files = sorted((kit / "outbox").glob("*.md"))
    assert [f.name for f in files] == [f"advisory-{RUSTSEC}.md"]
    text = files[0].read_text(encoding="utf-8")
    assert text.startswith(f"---\nid: advisory-{RUSTSEC}\ntitle: Advisory {RUSTSEC} on fakecrate 1.2.3\n"
                           f"kind: info\ndispatched: {TODAY.isoformat()}\n---\n")
    for part in ("fakecrate", "1.2.3", RUSTSEC, f"planted flaw {RUSTSEC}", ">=1.2.4", watch.NOTHING):
        assert part in text
    assert watch.parse_front(text)["kind"] == "info"


@pytest.mark.parametrize("where", ["outbox", "outbox/sent"])
def test_a_known_id_gives_new_0_and_writes_nothing(tmp_path: Path, kit: Path, capsys, where: str) -> None:
    known = plant(kit, f"{where}/advisory-{RUSTSEC}.md", f"---\nid: advisory-{RUSTSEC}\ntitle: t\nkind: info\n"
                  f"dispatched: {TODAY.isoformat()}\n---\nbody\n", 7200)
    before = known.read_bytes()
    assert scan(tmp_path, kit, cargo=audit(RUSTSEC), exits=(1, 0, 0)) == 0
    assert counts(capsys.readouterr().out) == (0, 0)
    assert report(tmp_path, kit) == 0
    assert counts(capsys.readouterr().out) == (0, 0)
    assert known.read_bytes() == before
    assert not git(kit, "status", "--porcelain")


def test_a_clean_report_writes_nothing(tmp_path: Path, kit: Path, capsys) -> None:
    assert scan(tmp_path, kit) == 0
    assert found(tmp_path) == []
    assert report(tmp_path, kit) == 0
    assert counts(capsys.readouterr().out) == (0, 0)
    assert not git(kit, "status", "--porcelain")


def test_npm_exit_1_on_only_the_excused_picomatch_advisory_is_clean(tmp_path: Path, kit: Path) -> None:
    assert scan(tmp_path, kit, npm_text=npm(picomatch_only()), exits=(0, 0, 1)) == 0
    assert found(tmp_path) == []
    assert report(tmp_path, kit) == 0
    assert not list((kit / "outbox").glob("*.md"))


def test_an_unexcused_npm_advisory_writes_one_file(tmp_path: Path, kit: Path) -> None:
    vulns = picomatch_only()
    vulns["lodash"] = npm_vuln("lodash", [npm_advisory("lodash", f"https://github.com/advisories/{OTHER}")])
    vulns["lodash"]["fixAvailable"] = {"name": "lodash", "version": "4.17.99", "isSemVerMajor": False}
    assert scan(tmp_path, kit, npm_text=npm(vulns), exits=(0, 0, 1)) == 0
    assert found(tmp_path) == [watch.entry(OTHER, "npm", "lodash", "<=4.0.7", "planted advisory on lodash",
                                           "lodash 4.17.99")]
    assert report(tmp_path, kit) == 0
    assert [f.name for f in (kit / "outbox").glob("*.md")] == [f"advisory-{OTHER}.md"]


def test_an_npm_problem_without_a_ghsa_id_becomes_npm_package(tmp_path: Path, kit: Path) -> None:
    vulns = {"@fake/pkg": npm_vuln("@fake/pkg", [npm_advisory("@fake/pkg", "https://example.test/advisory/7")])}
    assert scan(tmp_path, kit, npm_text=npm(vulns), exits=(0, 0, 1)) == 0
    assert [(e["id"], e["patched"]) for e in found(tmp_path)] == [("npm-fake-pkg", ">=4.0.8")]


BROKEN = ['{"vulnerabilities": {"list": [', "", "not json at all", "{}"]


@pytest.mark.parametrize("tool", ["cargo", "deny_text", "npm_text"])
@pytest.mark.parametrize("text", BROKEN, ids=["truncated", "empty", "not-json", "unknown-shape"])
def test_a_broken_report_exits_2_and_writes_nothing(tmp_path: Path, kit: Path, capsys, tool: str,
                                                    text: str) -> None:
    if tool == "deny_text" and text == "{}":
        text = '{"fields": {}}\n'
    assert scan(tmp_path, kit, **{tool: text}) == 2
    assert "::error::" in capsys.readouterr().out
    assert not (tmp_path / "found.json").exists()


@pytest.mark.parametrize("exits", [(1, 0, 0), (0, 101, 0), (0, 0, 1)], ids=["cargo-audit", "cargo-deny", "npm"])
def test_a_non_zero_exit_with_zero_raw_entries_exits_2(tmp_path: Path, kit: Path, capsys, exits) -> None:
    assert scan(tmp_path, kit, exits=exits) == 2
    assert "::error::" in capsys.readouterr().out
    assert not (tmp_path / "found.json").exists()


@pytest.mark.parametrize("bad", ["RUSTSEC/2099-0001", "RUSTSEC+2099"])
def test_an_id_outside_the_outbox_set_exits_2(tmp_path: Path, kit: Path, bad: str) -> None:
    assert scan(tmp_path, kit, cargo=audit(bad), exits=(1, 0, 0)) == 2
    assert not (tmp_path / "found.json").exists()


def test_cargo_deny_counts_only_error_diagnostics(tmp_path: Path, kit: Path) -> None:
    lines = deny(diagnostic("error", RUSTSEC), diagnostic("warning", "RUSTSEC-2099-0002"))
    assert scan(tmp_path, kit, deny_text=lines, exits=(0, 1, 0)) == 0
    assert found(tmp_path) == [watch.entry(RUSTSEC, "crates", "fakecrate", "1.2.3", f"planted flaw {RUSTSEC}",
                                           ">=1.2.4")]


def test_cargo_deny_names_a_yanked_crate_by_crate_and_version(tmp_path: Path, kit: Path) -> None:
    yanked = diagnostic("error", "x", code="yanked")
    del yanked["fields"]["advisory"]
    assert scan(tmp_path, kit, deny_text=deny(yanked), exits=(0, 1, 0)) == 0
    assert [e["id"] for e in found(tmp_path)] == ["yanked-fakecrate-1.2.3"]


@pytest.mark.parametrize("line", ["Error: unexpected argument", '{"type": "progress", "fields": {}}'])
def test_cargo_deny_fails_on_a_line_not_json_or_of_unknown_type(tmp_path: Path, kit: Path, line: str) -> None:
    assert scan(tmp_path, kit, deny_text=deny() + line + "\n") == 2
    assert not (tmp_path / "found.json").exists()


def message(ident: str, extra: str = "") -> str:
    """An outbox file of kind question."""
    return f"---\nid: {ident}\ntitle: Question: planted\nkind: question\n{extra}---\nA planted question.\n"


def test_pending_counts_only_settled_unsent_undispatched_parsable_files(tmp_path: Path, kit: Path, capsys) -> None:
    yesterday = (TODAY - timedelta(days=1)).isoformat()
    old = plant(kit, "outbox/K1-q1.md", message("K1-q1"), 7200)
    plant(kit, "outbox/K1-q2.md", message("K1-q2"), 300)
    plant(kit, "outbox/sent/K1-q3.md", message("K1-q3"), 7200)
    plant(kit, "outbox/K1-q4.md", message("K1-q4", f"dispatched: {TODAY.isoformat()}\n"), 7200)
    again = plant(kit, "outbox/K1-q5.md", message("K1-q5", f"dispatched: {yesterday}\n"), 7200)
    broken = plant(kit, "outbox/K1-q6.md", "no front matter here\n", 7200)
    before = {p: p.read_text(encoding="utf-8") for p in (old, again, broken)}
    assert scan(tmp_path, kit) == 0
    assert counts(capsys.readouterr().out) == (0, 2)
    assert report(tmp_path, kit) == 0
    assert counts(capsys.readouterr().out) == (0, 2)
    assert old.read_text(encoding="utf-8") == before[old].replace(
        "kind: question\n", f"kind: question\ndispatched: {TODAY.isoformat()}\n")
    assert again.read_text(encoding="utf-8") == before[again].replace(yesterday, TODAY.isoformat())
    assert broken.read_text(encoding="utf-8") == before[broken]
    changed = git(kit, "status", "--porcelain").split()
    assert sorted(changed) == ["M", "M", "outbox/K1-q1.md", "outbox/K1-q5.md"]


def test_a_run_leaves_the_kit_state_files_byte_identical(tmp_path: Path, kit: Path) -> None:
    before = {name: (kit / name).read_bytes() for name in ("kit/nightly.json", "kit/state.json")}
    assert scan(tmp_path, kit, cargo=audit(RUSTSEC), exits=(1, 0, 0)) == 0
    assert report(tmp_path, kit) == 0
    assert {name: (kit / name).read_bytes() for name in before} == before


def workflow(path: Path = WORKFLOW) -> dict[Any, Any]:
    """A workflow file as YAML (`on` reads as True under YAML 1.1)."""
    return yaml.safe_load(path.read_text(encoding="utf-8"))


def ci_step(name: str) -> dict[str, Any]:
    """The first ci.yml step whose name or `uses` starts with `name`."""
    for job in workflow(ROOT / ".github" / "workflows" / "ci.yml")["jobs"].values():
        for step in job.get("steps") or []:
            if str(step.get("name", "")).startswith(name) or str(step.get("uses", "")).startswith(name):
                return step
    raise AssertionError(name)


def test_the_workflow_triggers_and_job_permissions() -> None:
    doc = workflow()
    assert set(doc[True]) == {"schedule", "workflow_dispatch"}
    assert doc[True]["schedule"] == [{"cron": "23 */4 * * *"}]
    assert doc["permissions"] == {"contents": "read"}
    assert doc["concurrency"] == {"group": "advisory-watch", "cancel-in-progress": False}
    scan_job, report_job = doc["jobs"]["scan"], doc["jobs"]["report"]
    assert set(doc["jobs"]) == {"scan", "report"}
    assert "write" not in json.dumps(scan_job.get("permissions", {})) and "concurrency" not in scan_job
    assert scan_job["timeout-minutes"] == 20
    checkouts = [s for s in scan_job["steps"] if str(s.get("uses", "")).startswith("actions/checkout@")]
    assert len(checkouts) == 2 and all(s["with"]["persist-credentials"] is False for s in checkouts)
    assert checkouts[1]["with"]["ref"] == "claude/kit-state" and checkouts[1]["with"]["fetch-depth"] == 0
    assert report_job["concurrency"] == {"group": "kit-state-writer", "cancel-in-progress": False}
    assert report_job["permissions"] == {"contents": "write", "actions": "write"}
    assert report_job["needs"] == "scan"
    assert report_job["if"] == ("github.ref == 'refs/heads/main' && (needs.scan.outputs.new != '0' "
                                "|| needs.scan.outputs.pending != '0')")


def test_the_workflow_pins_every_action_never_forces_and_never_names_the_nightly_record() -> None:
    text = WORKFLOW.read_text(encoding="utf-8")
    uses = re.findall(r"uses:\s*(\S+)", text)
    assert uses and all(re.fullmatch(r"[\w.-]+/[\w.-]+@[0-9a-f]{40}", u) for u in uses)
    assert not re.search(r"git push[^\n]*(--force|-f\b)", text)
    assert "kit/nightly.json" not in text
    names = {job.get("name", key) for key, job in workflow()["jobs"].items()}
    assert not names & {"Ci", "Twin", "Ally", "Checks"}


def test_the_tools_and_setup_node_match_ci() -> None:
    steps = workflow()["jobs"]["scan"]["steps"]
    name = "Prebuilt tools (cargo-audit, cargo-deny) from pinned musl release tarballs (D-693)"
    mine = next(s for s in steps if s.get("name") == name)
    assert mine["env"] == ci_step(name)["env"] and mine["run"] == ci_step(name)["run"]
    node = next(s for s in steps if str(s.get("uses", "")).startswith("actions/setup-node@"))
    assert node["uses"] == ci_step("actions/setup-node@")["uses"] and node["with"]["node-version"] == "22"


def test_notify_gains_workflow_dispatch_beside_its_push_trigger() -> None:
    on = workflow(ROOT / ".github" / "workflows" / "notify.yml")[True]
    assert set(on) == {"push", "workflow_dispatch"}
    assert on["push"] == {"branches": ["claude/kit-state"], "paths": ["outbox/**"]}
