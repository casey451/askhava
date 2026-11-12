"""The backup job's build caches stay small and single, and only `main` saves shared caches (D-3143).

GitHub keeps 10 GB of Actions cache per repository. Each merge used to add about 7.3 GB of backup-job entries
(the worker's whole `target/debug` per commit, the smoke's `target/smoke` per compiling job), which pushed `Ci`'s
`main`-only entries (D-2782) out within hours. These tests pin the fix: `runner/cache.py` cuts the worker build to
its dependency artifacts and deletes a family's superseded entries just before a newer save; the workflows save
only from `main`'s ref; and what the restores read, and so what is tested, is unchanged.
"""

from __future__ import annotations

import json
import re
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import pytest
import yaml

from runner import cache

ROOT = Path(__file__).resolve().parent.parent
FLOWS = ROOT / ".github" / "workflows"
MAIN = "github.ref == 'refs/heads/main'"


def _flow(name: str) -> dict[str, Any]:
    return yaml.safe_load((FLOWS / name).read_text(encoding="utf-8"))


def _steps(name: str, job: str) -> list[dict[str, Any]]:
    return list(_flow(name)["jobs"][job]["steps"])


def _named(steps: list[dict[str, Any]], name: str) -> dict[str, Any]:
    return next(s for s in steps if s.get("name") == name)


# ---- seed and clear: the worker build takes its dependencies from the smoke build; its entry holds the registry


def _target(debug: Path) -> Path:
    for rel in ("deps/libserde-0123456789abcdef.rlib", "deps/libaskhava_core-0123456789abcdef.rlib",
                "deps/payments-0123456789abcdef", ".fingerprint/serde-0123456789abcdef/lib",
                ".fingerprint/askhava-core-0123456789abcdef/lib", "build/ring-0123456789abcdef/out/x.o",
                "build/askhava-server-0123456789abcdef/out/y", "incremental/askhava_core-1abc/s-x/q.bin",
                "askhava-server", "examples/demo-0123456789abcdef", ".cargo-lock", "deps/README"):
        (debug / rel).parent.mkdir(parents=True, exist_ok=True)
        (debug / rel).write_text("x")
    return debug


MEMBERS = {"askhava-core", "askhava-server", "payments"}


def _metadata(names: set[str]) -> str:
    return json.dumps({"packages": [{"name": n, "targets": [{"name": n.replace("-", "_")}]} for n in sorted(names)]})


def test_seed_copies_only_the_smoke_builds_dependency_artifacts(tmp_path: Path) -> None:
    smoke, debug = _target(tmp_path / "target" / "smoke" / "debug"), tmp_path / "target" / "debug"
    line = cache.seed(tmp_path, smoke, debug, lambda args: (0, _metadata(MEMBERS)))
    left = sorted(str(p.relative_to(debug)) for p in debug.rglob("*") if p.is_file())
    assert left == [".fingerprint/serde-0123456789abcdef/lib", "build/ring-0123456789abcdef/out/x.o",
                    "deps/libserde-0123456789abcdef.rlib"]
    assert line.startswith("worker build seeded from the smoke build: 3 entries")
    assert (smoke / "deps" / "payments-0123456789abcdef").exists()  # a copy: the smoke build is untouched
    (smoke / "deps" / "libtokio-0123456789abcdef.rlib").write_text("x")
    assert "already holds a build" in cache.seed(tmp_path, smoke, debug, lambda args: (0, _metadata(MEMBERS)))
    assert not (debug / "deps" / "libtokio-0123456789abcdef.rlib").exists()


def test_seed_copies_nothing_without_members_a_smoke_build_or_room(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    smoke, debug = _target(tmp_path / "smoke"), tmp_path / "debug"
    assert "members are unknown" in cache.seed(tmp_path, smoke, debug, lambda args: (1, ""))
    assert "members are unknown" in cache.seed(tmp_path, smoke, debug, lambda args: (0, "not json"))
    assert "no deps folder" in cache.seed(tmp_path, tmp_path / "none", debug, lambda args: (0, _metadata(MEMBERS)))
    monkeypatch.setattr(cache.shutil, "disk_usage", lambda path: SimpleNamespace(free=1))
    assert "MB free" in cache.seed(tmp_path, smoke, debug, lambda args: (0, _metadata(MEMBERS)))
    assert not debug.exists()


def test_clear_empties_the_worker_build_and_says_so_on_one_output_line(tmp_path: Path) -> None:
    debug = _target(tmp_path / "debug")
    assert cache.clear(debug) == "pruned=yes" and not debug.exists()
    assert cache.clear(debug) == "pruned=yes"  # nothing to empty
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "keep.txt").write_text("x")
    (tmp_path / "link").symlink_to(outside, target_is_directory=True)
    assert cache.clear(tmp_path / "link") == "pruned=yes" and (outside / "keep.txt").exists()


def test_clear_says_no_when_it_cannot_empty(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    debug = _target(tmp_path / "debug")

    def refuse(path: Path) -> None:
        raise OSError("busy")

    monkeypatch.setattr(cache.shutil, "rmtree", refuse)
    assert cache.clear(debug) == "pruned=no"


KEY_VALUE = re.compile(r"^\w+=")


@pytest.mark.parametrize("argv", [["clear", "--target", "{tmp}/debug"], ["drop", "--prefix", "rust-smoke-Linux-", "--before", ""],
                                  ["seed", "--root", "{tmp}", "--smoke", "{tmp}/none", "--target", "{tmp}/debug"]])
def test_standard_output_holds_only_key_value_lines(tmp_path: Path, capsys: pytest.CaptureFixture[str], argv: list[str]) -> None:
    """The workflow appends standard output to $GITHUB_OUTPUT, which refuses a line without `=`: every message
    goes to standard error."""
    _target(tmp_path / "debug")
    assert cache.main([a.format(tmp=tmp_path) for a in argv]) == 0
    out = capsys.readouterr()
    assert all(KEY_VALUE.match(line) for line in out.out.splitlines()) and out.err.strip()


# ---- drop: a family's superseded entries


ROWS = [
    {"id": 1, "key": "rust-smoke-Linux-h-aaa-1-1", "created_at": "2026-11-12T04:00:00.000Z"},
    {"id": 2, "key": "rust-smoke-Linux-h-bbb-2-1", "created_at": "2026-11-12T04:59:59Z"},
    {"id": 3, "key": "rust-smoke-Linux-h-ccc-3-1", "created_at": "2026-11-12T05:00:00Z"},  # saved since the job began
    {"id": 4, "key": "rust-Linux-h-aaa", "created_at": "2026-11-12T03:00:00Z"},
    {"id": 5, "key": "rust-smoke-Linux-h-ddd-4-1", "created_at": ""},
    {"key": "rust-smoke-Linux-h-eee-5-1", "created_at": "2026-11-12T01:00:00Z"},
    {"id": 7, "key": "v0-rust-checks-Linux-x64-1-2", "created_at": "2026-11-11T17:25:00Z"},
]


def test_superseded_names_only_older_entries_of_the_family() -> None:
    assert cache.superseded(ROWS, "rust-smoke-Linux-", "2026-11-12T05:00:00") == [1, 2]
    assert cache.superseded(ROWS, "rust-Linux-", "2026-11-12T05:00:00") == [4]  # never a smoke entry
    assert cache.superseded(ROWS, "rust-smoke-Linux-", "2026-11-12T04:00:00") == []
    assert cache.superseded(ROWS, "v0-", "2026-11-12T05:00:00") == [7]


class Gh:
    """`gh api` over a list of cache rows: list pages and deletes, with failures on request."""

    def __init__(self, rows: list[dict[str, Any]], fail_list: bool = False, fail_delete: set[int] | None = None) -> None:
        self.rows, self.fail_list, self.fail_delete = rows, fail_list, fail_delete or set()
        self.calls: list[list[str]] = []

    def __call__(self, args: list[str]) -> tuple[int, str]:
        self.calls.append(args)
        if args[:4] == ["gh", "api", "-X", "DELETE"]:
            found = int(args[4].rsplit("/", 1)[1])
            return (1, "") if found in self.fail_delete else (0, "")
        if self.fail_list:
            return 1, ""
        query = dict(part.split("=", 1) for part in args[2].split("?", 1)[1].split("&"))
        page, size = int(query["page"]), int(query["per_page"])
        rows = [r for r in self.rows if str(r.get("key", "")).startswith(query["key"])]
        return 0, json.dumps({"total_count": len(rows), "actions_caches": rows[(page - 1) * size: page * size]})


def test_drop_deletes_the_superseded_entries_and_counts_failures(capsys: pytest.CaptureFixture[str]) -> None:
    rows = [r for r in ROWS if r.get("id") != 3]  # no newer entry: this job saves
    gh = Gh(rows, fail_delete={2})
    assert cache.drop("o/r", "rust-smoke-Linux-", "2026-11-12T05:00:00", gh) == "save=yes"
    assert "dropped 1 of 2 superseded entries under rust-smoke-Linux- (4 listed)" in capsys.readouterr().err
    assert [c[4] for c in gh.calls if c[:4] == ["gh", "api", "-X", "DELETE"]] == ["repos/o/r/actions/caches/1",
                                                                                 "repos/o/r/actions/caches/2"]


def test_a_newer_entry_from_another_job_means_nothing_dropped_and_nothing_saved(capsys: pytest.CaptureFixture[str]) -> None:
    gh = Gh(ROWS)
    assert cache.drop("o/r", "rust-smoke-Linux-", "2026-11-12T05:00:00", gh) == "save=no"
    assert not [c for c in gh.calls if "DELETE" in c] and "nothing saved" in capsys.readouterr().err
    assert cache.newer(ROWS, "rust-Linux-", "2026-11-12T05:00:00") is False  # the smoke entry is another family


def test_drop_lists_every_page_and_deletes_nothing_when_a_page_is_unreadable(capsys: pytest.CaptureFixture[str]) -> None:
    many = [{"id": i, "key": f"rust-smoke-Linux-{i}", "created_at": "2026-11-12T00:00:00Z"} for i in range(1, 251)]
    gh = Gh(many)
    assert cache.drop("o/r", "rust-smoke-Linux-", "2026-11-12T05:00:00", gh) == "save=yes"
    assert "dropped 250 of 250" in capsys.readouterr().err
    assert sum(1 for c in gh.calls if c[:3] == ["gh", "api", "repos/o/r/actions/caches?key=rust-smoke-Linux-&per_page=100&page=3"]) == 1
    broken = Gh(ROWS, fail_list=True)
    assert cache.drop("o/r", "rust-smoke-Linux-", "2026-11-12T05:00:00", broken) == "save=yes"  # as before D-3143
    assert "not listed; none dropped" in capsys.readouterr().err
    assert not [c for c in broken.calls if "DELETE" in c]
    assert cache.list_caches("o/r", "x", lambda args: (0, "[]")) is None  # a page that is not the expected object


def test_main_never_fails_the_step(capsys: pytest.CaptureFixture[str]) -> None:
    assert cache.main(["drop", "--prefix", "rust-smoke-Linux-", "--before", ""]) == 0
    out = capsys.readouterr()
    assert out.out == "save=yes\n" and "not dropped" in out.err
    assert cache.run_command(["/nonexistent/program"])[0] != 0


# ---- the workflows


def test_the_worker_cache_is_restored_seeded_from_the_smoke_and_saved_empty_from_main_only() -> None:
    steps = _steps("backup-runner.yml", "backup")
    order = [s.get("name") or s.get("uses") for s in steps]
    restore = _named(steps, "Rust build cache (D-514)")
    seed = _named(steps, "Give the worker the smoke build's dependencies (D-3143)")
    clear = _named(steps, "Empty the worker build before the save (D-3143)")
    drop = _named(steps, "Drop the superseded worker build entries (D-3143)")
    save = _named(steps, "Rust build cache, save (D-514, D-3143)")
    assert restore["uses"] == "actions/cache/restore@v4" and save["uses"] == "actions/cache/save@v4"
    assert restore["with"]["path"] == save["with"]["path"] and save["with"]["key"] == "${{ steps.rust-cache.outputs.cache-primary-key }}"
    assert restore["id"] == "rust-cache" and clear["id"] == "rust-prune" and drop["id"] == "rust-drop"
    assert MAIN in clear["if"] and "steps.rust-cache.outputs.cache-hit != 'true'" in clear["if"] and clear["if"].startswith("always()")
    assert drop["if"] == "always() && steps.rust-prune.outputs.pruned == 'yes'"
    assert save["if"] == drop["if"] + " && steps.rust-drop.outputs.save != 'no'"
    assert order.index("Run the start-routine smoke before the session (D-2028)") < order.index(seed["name"])
    assert order.index(seed["name"]) < order.index("anthropics/claude-code-action@v1") < order.index(clear["name"])
    assert order.index(clear["name"]) < order.index(drop["name"]) < order.index(save["name"])
    assert order.index("Smoke build cache, save (D-1911)") < order.index(clear["name"])
    assert "python3 -m runner.cache seed" in seed["run"] and seed["continue-on-error"] is True
    for step in (clear, drop):
        assert 'cd "$RUNNER_TEMP/keeper" && python3 -m runner.cache' in step["run"] and step["run"].rstrip().endswith('| tee -a "$GITHUB_OUTPUT"')
    assert drop["env"]["PREFIX"] == "rust-${{ runner.os }}-" and restore["with"]["key"].startswith(drop["env"]["PREFIX"])
    assert not any(str(s.get("uses", "")).startswith("actions/cache@") for s in steps)  # no save left to a post step


def test_the_smoke_entry_is_saved_only_from_main_alone_after_its_older_entries_are_dropped() -> None:
    steps = _steps("backup-runner.yml", "backup")
    order = [s.get("name") or s.get("uses") for s in steps]
    drop = _named(steps, "Drop the superseded smoke build entries (D-3143)")
    save = _named(steps, "Smoke build cache, save (D-1911)")
    restore = _named(steps, "Smoke build cache, restore (D-1890, D-1911)")
    assert drop["id"] == "smoke-drop" and MAIN in drop["if"] and "hashFiles('target/smoke-ran')" in drop["if"]
    assert save["if"] == drop["if"].replace(" && steps.bg-smoke", " && steps.smoke-drop.outputs.save != 'no' && steps.bg-smoke")
    assert order.index(drop["name"]) + 1 == order.index(save["name"]) and drop["continue-on-error"] is True
    assert drop["env"]["PREFIX"] == "rust-smoke-${{ runner.os }}-" and restore["with"]["key"].startswith(drop["env"]["PREFIX"])
    assert '--before "${JOB_START:-}"' in drop["run"] and 'cd "$RUNNER_TEMP/keeper"' in drop["run"]
    assert drop["run"].rstrip().endswith('| tee -a "$GITHUB_OUTPUT"')
    start = _named(steps, "Note when this job started (for the chaining step)")["run"]
    assert "JOB_START=$(date -u +%Y-%m-%dT%H:%M:%S)" in start  # the format `superseded` compares to the second


@pytest.mark.parametrize(("flow", "job"), [("ci.yml", "e2e"), ("a11y.yml", "a11y")])
def test_the_playwright_browsers_are_saved_from_main_only(flow: str, job: str) -> None:
    steps = _steps(flow, job)
    restore = next(s for s in steps if s.get("id") == "browsers")
    save = _named(steps, "Save the Playwright browsers to the cache (main only, D-3143)")
    assert restore["uses"].startswith("actions/cache/restore@") and save["uses"].startswith("actions/cache/save@")
    assert restore["uses"].split("@")[1] == save["uses"].split("@")[1]
    assert save["if"].startswith(MAIN + " && steps.browsers.outputs.cache-hit != 'true'")
    assert save["with"] == {"path": restore["with"]["path"], "key": "${{ steps.browsers.outputs.cache-primary-key }}"}
    order = [s.get("name") for s in steps]
    assert order.index("Chromium and its system libraries") < order.index(save["name"])


def test_no_branch_run_saves_a_shared_cache() -> None:
    """Every save in the workflows a task branch runs (ci, twin, a11y) is guarded by main's ref: a rust-cache
    `save-if`, or an `if` on an actions/cache/save step; no combined actions/cache step saves at a job's end."""
    for flow in ("ci.yml", "twin.yml", "a11y.yml"):
        for job in _flow(flow)["jobs"].values():
            for step in job.get("steps") or []:
                uses = str(step.get("uses", ""))
                assert not uses.startswith("actions/cache@"), f"{flow}: {step.get('name')}"
                if uses.startswith("Swatinem/rust-cache@"):
                    assert step["with"]["save-if"] == "${{ " + MAIN + " }}", f"{flow}: {step.get('name')}"
                if uses.startswith("actions/cache/save@"):
                    assert str(step.get("if", "")).startswith(MAIN), f"{flow}: {step.get('name')}"
