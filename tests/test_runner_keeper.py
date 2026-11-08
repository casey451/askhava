"""The keeper (D-1451): runner/keeper.py decides how long the next keeper sleeps and what it does when it
wakes; backup-runner.yml calls it. Also the smoke run's own target dir, fixed worktree and pinned mtimes
(D-1890, which replaced D-1451's `cargo clean --workspace` after the T057 stale build) and kit/pacing.json's public-repository `actions_minutes`. Fake data only."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

import yaml

from runner import keeper as kp, state as st

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github" / "workflows" / "backup-runner.yml"
NOW = datetime(2026, 11, 2, 12, 0, tzinfo=timezone.utc)


def at(minutes: float) -> str:
    return st.iso(NOW + timedelta(minutes=minutes))


def state(**extra: Any) -> dict[str, Any]:
    base: dict[str, Any] = {"schema": 1, "backup_runner": "on", "lease": None, "build_hold": None,
                            "tasks": [{"id": "T900", "status": "QUEUED", "deps": []}]}
    return {**base, **extra}


def lease(*expires: float) -> dict[str, Any]:
    leases = {str(n): {"task": f"T90{n}", "session": "actions-1", "expires": at(m), "lane": str(n)} for n, m in enumerate(expires, 1)}
    return {"lease": leases.get("1"), "leases": leases}


def test_idle_with_work_wakes_in_twenty_minutes() -> None:
    assert kp.wake_after(state(), {}, NOW) == (20, "idle")


def test_a_task_in_flight_wakes_two_minutes_after_the_soonest_lease_ends() -> None:
    minutes, why = kp.wake_after(state(), lease(90, 30), NOW)
    assert minutes == 32 and "lease ends" in why
    assert kp.wake_after(state(), lease(1), NOW)[0] == kp.FLOOR_MINUTES  # never before the arming run ends
    assert kp.wake_after(state(), lease(600), NOW)[0] == kp.CAP_MINUTES  # under GitHub's 6-hour job limit
    assert kp.wake_after(state(), lease(-5), NOW) == (20, "idle")  # an expired lease is not in flight


def test_the_pacer_waiting_wakes_five_minutes_after_its_waiting_until_capped() -> None:
    pacing = {"decision": "wait", "waiting_until": at(60)}
    assert kp.wake_after(state(pacing=pacing), {}, NOW) == (65, f"the pacer waits until {at(60)}")
    far = {"decision": "wait", "waiting_until": at(3000)}
    assert kp.wake_after(state(pacing=far), {}, NOW)[0] == kp.CAP_MINUTES == 340
    past = {"decision": "wait", "waiting_until": at(-1)}
    assert kp.wake_after(state(pacing=past), {}, NOW) == (20, "idle")
    assert kp.wake_after(state(pacing={"decision": "claim", "waiting_until": None}), {}, NOW) == (20, "idle")


def test_the_weekly_stop_chains_sleeps_to_the_reset() -> None:
    reading = {"at": at(-10), "week": {"utilization": 0.97, "resets_at": at(60 * 70)}}
    minutes, why = kp.wake_after(state(usage={"session_reading": reading}), {}, NOW)
    assert minutes == 340 and "resets" in why
    near = {"at": at(-10), "week": {"utilization": 0.98, "resets_at": at(100)}}
    assert kp.wake_after(state(usage={"session_reading": near}), {}, NOW)[0] == 105
    under = {"at": at(-10), "week": {"utilization": 0.84, "resets_at": at(100)}}
    assert kp.wake_after(state(usage={"session_reading": under}), {}, NOW) == (20, "idle")


def test_no_keeper_while_the_flag_is_off_or_a_red_main_waits_for_a_person() -> None:
    assert kp.wake_after(state(backup_runner="off"), {}, NOW)[0] is None
    red = {"check": "smoke", "sha": "abc"}
    escalated = state(main_red=red, tasks=[{"id": st.FIX_MAIN, "status": "BLOCKED", "escalated_at": at(-5)}])
    minutes, why = kp.wake_after(escalated, {}, NOW)
    assert minutes is None and "person" in why
    retrying = state(main_red=red, tasks=[{"id": st.FIX_MAIN, "status": "QUEUED"}])
    assert kp.wake_after(retrying, {}, NOW) == (20, "idle")  # FIX-MAIN can still be claimed: the build fixes main itself


def test_build_hold_and_nothing_to_claim_sleep_long_not_every_twenty_minutes() -> None:
    held = state(build_hold="Gate P", tasks=[{"id": "T900", "status": "QUEUED", "deps": []}])
    minutes, why = kp.wake_after(held, {}, NOW)
    assert minutes == 340 and "build_hold" in why
    assert kp.wake_after(held, {}, NOW, last="")[0] == 340
    kit_task = state(build_hold="Gate P", tasks=[{"id": "K12", "status": "QUEUED", "deps": []}])
    assert kp.wake_after(kit_task, {}, NOW) == (20, "idle")  # kit tasks run under the hold
    retry = state(tasks=[{"id": "T900", "status": "RETRY_LATER", "retry_after": at(45), "deps": []}])
    assert kp.wake_after(retry, {}, NOW)[0] == 50
    merging = state(tasks=[{"id": "T900", "status": "MERGE_PENDING"}])
    assert kp.wake_after(merging, {}, NOW) == (20, "idle")  # a cycle resolves the merge
    assert kp.wake_after(state(), {}, NOW, last="idle")[0] == 340  # the cycle itself found nothing to claim


def test_last_result_reads_only_this_jobs_rows_and_skips_quiet_ones() -> None:
    rows = [{"at": "2026-11-02T11:59:00", "session": "actions-7", "result": "handed_off"},
            {"at": "2026-11-02T12:01:00", "session": "actions-7", "result": "idle"},
            {"at": "2026-11-02T12:02:00", "session": "actions-8", "result": "handed_off"},
            {"at": "2026-11-02T12:03:00", "session": "actions-7", "result": "usage_reading_recorded: week 84%"},
            {"at": "2026-11-02T12:04:00", "session": "actions-7", "result": "T900 is MERGE_PENDING"}]
    assert kp.last_result(rows, "actions-7", "2026-11-02T12:00:00") == "idle"
    assert kp.last_result(rows, "actions-9", "2026-11-02T12:00:00") == ""


def test_on_wake_never_starts_a_cycle_beside_a_live_run_or_lease() -> None:
    runs = [{"id": 5, "status": "completed"}, {"id": 6, "status": "in_progress"}]
    action, minutes, why = kp.on_wake(state(), {}, runs, "9", NOW)
    assert action == "rearm" and minutes == 20 and "run 6 is in_progress" in why
    assert kp.on_wake(state(), {}, [{"id": 9, "status": "in_progress"}], "9", NOW)[0] == "go"  # itself
    assert kp.on_wake(state(), {}, [{"id": 6, "status": "queued"}], "9", NOW)[0] == "rearm"
    assert kp.on_wake(state(), lease(30), [], "9", NOW)[:2] == ("rearm", 32)
    assert kp.on_wake(state(), {}, None, "9", NOW)[0] == "stop"  # an unreadable run list: the hourly tick backstops
    assert kp.on_wake(state(backup_runner="off"), {}, [], "9", NOW)[0] == "stop"
    assert kp.on_wake(state(), {}, [], "9", NOW)[0] == "go"
    assert kp.on_wake(state(build_hold="Gate P"), {}, [], "9", NOW)[0] == "rearm"


def test_clamp_keeps_a_dispatched_sleep_inside_the_job_limit() -> None:
    assert [kp.clamp(v) for v in ("20", "0", "9999", "x", "", " 45 ")] == [20, 1, 340, 20, 20, 45]
    assert kp.CAP_MINUTES * 60 < 350 * 60 <= 6 * 3600 - 600  # the job's timeout-minutes leaves room under 6 h


def test_the_cli_prints_github_output_lines(tmp_path: Path) -> None:
    (tmp_path / "s.json").write_text(json.dumps(state()))
    (tmp_path / "l.json").write_text(json.dumps(lease(30)))
    (tmp_path / "r.json").write_text(json.dumps([]))
    def run(*args: str) -> str:
        done = subprocess.run([sys.executable, "-m", "runner.keeper", *args], cwd=ROOT, capture_output=True, text=True, check=True)
        return done.stdout
    assert run("clamp", "400") == "340\n"
    out = run("arm", "--state", str(tmp_path / "s.json"), "--lease", str(tmp_path / "missing.json"))
    assert out.startswith("wake_after=20\n")
    out = run("wake", "--state", str(tmp_path / "s.json"), "--lease", str(tmp_path / "missing.json"), "--runs", str(tmp_path / "r.json"))
    assert out.splitlines()[0] == "action=go"
    off = tmp_path / "off.json"
    off.write_text(json.dumps(state(backup_runner="off")))
    assert run("arm", "--state", str(off)).startswith("wake_after=\n")
    assert kp.main(["arm", "--state", str(tmp_path / "s.json"), "--lease", str(tmp_path / "l.json")], now=NOW) == 0


# ---- backup-runner.yml


def _flow() -> dict[str, Any]:
    return yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))


def test_the_workflow_arms_one_keeper_and_keeps_every_lock() -> None:
    flow = _flow()
    on = flow.get("on", flow.get(True))
    assert on["workflow_dispatch"]["inputs"]["wake_after"]["required"] is False
    assert on["schedule"][0]["cron"] == "37 * * * *"  # the backstop stays, on an odd minute
    gate, backup, keeper = flow["jobs"]["gate"], flow["jobs"]["backup"], flow["jobs"]["keeper"]
    assert "github.event.inputs.wake_after == ''" in gate["if"]  # a keeper run never runs a cycle itself
    assert "github.event.inputs.wake_after != ''" in keeper["if"]
    assert keeper["concurrency"] == {"group": "backup-runner-keeper", "cancel-in-progress": True}
    assert keeper.get("needs") is None and "postgres" not in str(keeper)
    assert backup["concurrency"]["cancel-in-progress"] is False and "'backup-runner'" in backup["concurrency"]["group"]
    assert keeper["timeout-minutes"] < 360
    arm = backup["steps"][-1]
    assert arm["name"].startswith("Arm the keeper") and arm["if"] == "always()"
    assert "-f wake_after=" in arm["run"] and "runner.keeper arm" in arm["run"]
    wake = keeper["steps"][-1]["run"]
    assert "runner.keeper wake" in wake and "runner.keeper clamp" in wake and "claude/lease" in wake
    assert 'go) gh workflow run backup-runner.yml --ref main --repo "$GITHUB_REPOSITORY" ;;' in wake


# ---- the smoke's own target dir (T057, D-1890) and the Actions minutes note


def _index(tree: Path, files: dict[str, str]) -> str:
    """A `git ls-files -s -z` listing for fake files written under `tree` (the blob is the content's stand-in)."""
    rows = []
    for name, text in files.items():
        (tree / name).parent.mkdir(parents=True, exist_ok=True)
        (tree / name).write_text(text)
        rows.append(f"100644 {text:0<40} 0\t{name}")
    return "\0".join(rows) + "\0"


def test_the_smoke_builds_into_its_own_folder_of_the_cached_target(monkeypatch: Any, tmp_path: Path) -> None:
    from runner import github_env as ge

    monkeypatch.setenv("GITHUB_WORKSPACE", "/work/fake")
    monkeypatch.setenv("RUNNER_TEMP", "/work/_temp")
    monkeypatch.delenv("CARGO_TARGET_DIR", raising=False)
    assert ge.smoke_target_dir() == Path("/work/fake/target/smoke"), "inside `target`, which the Rust cache saves"
    assert ge.smoke_tree_dir() == Path("/work/_temp/askhava-smoke"), "the same path every run"
    seen: list[dict[str, str] | None] = []
    monkeypatch.setattr(ge, "_run", lambda a, i, c, e, timeout=600: (seen.append(e), subprocess.CompletedProcess(a, 0, "", ""))[1])
    ge.run_command(["bash", "scripts/smoke.sh"], None, Path("."))
    ge.run_command(["cargo", "build"], None, Path("."))
    assert seen[0] is not None and seen[0]["CARGO_TARGET_DIR"] == "/work/fake/target/smoke"
    assert seen[0]["CARGO_INCREMENTAL"] == "0", "as in Ci: no incremental data in the cached smoke target"
    assert seen[1] is None or "CARGO_TARGET_DIR" not in seen[1], "the worker's cargo keeps the checkout's own target"
    assert not hasattr(ge, "CARGO_CLEAN_MEMBERS"), "no clean of the worker's target after a smoke (D-1890)"


def test_unchanged_files_keep_their_pinned_mtime_and_changed_ones_get_now(tmp_path: Path) -> None:
    from runner import github_env as ge

    tree, manifest = tmp_path / "tree", tmp_path / "target" / "smoke" / ge.SMOKE_MTIMES
    tree.mkdir()
    index = _index(tree, {"a/src/lib.rs": "a1", "b/src/lib.rs": "b1"})
    assert ge.pin_smoke_mtimes(tree, index, manifest, 1_000) == 7, "no manifest: 2 files and 5 folders are new"
    assert (tree / "a/src/lib.rs").stat().st_mtime_ns == 1_000
    for name in ("a/src/lib.rs", "b/src/lib.rs"):
        (tree / name).unlink()  # a fresh worktree: new files, new mtimes
    index = _index(tree, {"a/src/lib.rs": "a1", "b/src/lib.rs": "b2", "c/src/lib.rs": "c1"})
    assert ge.pin_smoke_mtimes(tree, index, manifest, 2_000) == 5, "b's file, c's file, c's 2 folders and the root"
    assert (tree / "a/src/lib.rs").stat().st_mtime_ns == 1_000, "unchanged content keeps the time before every build of it"
    assert (tree / "b/src/lib.rs").stat().st_mtime_ns == 2_000, "changed content is newer than every earlier build"
    assert (tree / "c/src/lib.rs").stat().st_mtime_ns == 2_000
    assert tree.stat().st_mtime_ns == 2_000, "a folder with a new entry is new (a build script may watch it)"
    assert (tree / "a/src").stat().st_mtime_ns == 1_000, "a folder with the same entries keeps its time"
    index = _index(tree, {"a/src/lib.rs": "a1", "b/src/lib.rs": "b1"})
    ge.pin_smoke_mtimes(tree, index, manifest, 3_000)
    assert (tree / "b/src/lib.rs").stat().st_mtime_ns == 3_000, "content changed back is still newer than the last build"
    assert tree.stat().st_mtime_ns == 3_000, "a folder that lost an entry is new"
    assert set(json.loads(manifest.read_text())) == {"a/src/lib.rs", "b/src/lib.rs", "/", "a/", "a/src/", "b/", "b/src/"}


def test_an_unreadable_manifest_makes_every_file_new(tmp_path: Path) -> None:
    from runner import github_env as ge

    tree, manifest = tmp_path / "tree", tmp_path / "m.json"
    tree.mkdir()
    for junk in ("not json", "[1, 2]", json.dumps({"x.rs": ["blob", "late"]})):
        manifest.write_text(junk)
        assert ge.pin_smoke_mtimes(tree, _index(tree, {"x.rs": "x1"}), manifest, 5_000) == 2, "the file and the root"
    manifest.unlink()
    (tree / "link.rs").symlink_to(tree / "x.rs")
    index = _index(tree, {"x.rs": "x1"}) + "120000 " + "l" * 40 + " 0\tlink.rs\0160000 " + "s" * 40 + " 0\tsub\0"
    assert ge.pin_smoke_mtimes(tree, index, manifest, 6_000) == 2, "links and submodules are skipped"
    assert json.loads(manifest.read_text())["/"][0] == ge.hashlib.sha256(b"x.rs").hexdigest()


def test_smoke_main_uses_the_fixed_tree_and_pins_before_the_smoke(monkeypatch: Any, tmp_path: Path) -> None:
    from runner import github_env as ge

    monkeypatch.setenv("RUNNER_TEMP", str(tmp_path / "temp"))
    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path / "target"))
    tree = ge.smoke_tree_dir()
    calls: list[tuple[list[str], Path]] = []

    def run(a: list[str], i: str | None, c: Path) -> tuple[int, str, str]:
        calls.append((list(a), c))
        if a[:3] == ["git", "worktree", "add"]:
            _index(tree, {"Cargo.toml": "w1"})
        if a[:2] == ["git", "ls-files"]:
            return 0, "100644 " + "w" * 40 + " 0\tCargo.toml\0", ""
        if a[:2] == ["git", "rev-parse"]:
            return 0, "f" * 40 + "\n", ""
        return 0, "ok", ""

    tree.mkdir(parents=True)
    (tree / "left-over").write_text("a crashed run")
    env = ge.GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=run)
    env.main_head = lambda: "f" * 40  # type: ignore[method-assign]  # reason: no remote in this test
    assert env.smoke_main() == (True, "f" * 40)
    cmds = [a for a, _ in calls]
    add = cmds.index(["git", "worktree", "add", "--detach", str(tree), "f" * 40])
    assert cmds.index(["git", "worktree", "remove", "--force", str(tree)]) < add, "a left-over tree goes first"
    assert cmds.index(["git", "ls-files", "-s", "-z"]) < cmds.index(["bash", "scripts/smoke.sh"])
    assert calls[cmds.index(["bash", "scripts/smoke.sh"])][1] == tree
    assert (tmp_path / "target" / "smoke" / ge.SMOKE_MTIMES).exists() and not tree.exists()
    assert env.log[-2] == "smoke mtimes: 2 new or changed paths"
    assert env.log[-3] == f"smoke mtimes: manifest {tmp_path / 'target' / 'smoke' / ge.SMOKE_MTIMES}, 0 entries loaded"
    assert (tmp_path / "target" / ge.SMOKE_RAN).exists(), "the marker the workflow's save step looks for (D-1911)"


def _pin_env(tmp_path: Path, index: list[str], code: list[int]) -> Any:
    """A GitHubEnv whose `git ls-files` answers `index[0]` with exit `code[0]` (both changeable by the test)."""
    from runner import github_env as ge

    def run(a: list[str], i: str | None, c: Path) -> tuple[int, str, str]:
        return (code[0], index[0], "index locked") if a[:2] == ["git", "ls-files"] else (0, "", "")

    return ge.GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=run)


def test_a_failed_pin_then_a_revert_never_reuses_the_old_time(monkeypatch: Any, tmp_path: Path) -> None:
    """Review of D-1890: a pin that fails part-way must not leave the earlier record, or content reverted to
    what it held then would get that old time back and an artifact built from the failed run's content
    would count as fresh."""
    from runner import github_env as ge

    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path / "target"))
    tree, manifest = tmp_path / "tree", tmp_path / "target" / "smoke" / ge.SMOKE_MTIMES
    tree.mkdir()
    index, code = [_index(tree, {"a.rs": "a1", "b.rs": "b1"})], [0]
    env = _pin_env(tmp_path, index, code)
    env._pin_smoke_tree(tree)
    first = (tree / "a.rs").stat().st_mtime_ns
    index[0] = _index(tree, {"a.rs": "a2", "b.rs": "b2"})
    real, calls, armed = os.utime, [0], [True]

    def flaky(path: Any, *args: Any, **kw: Any) -> None:
        calls[0] += 1
        if calls[0] == 2 and armed[0]:
            raise OSError("disk went away")
        real(path, *args, **kw)

    env.log.clear()
    monkeypatch.setattr(ge.os, "utime", flaky)
    env._pin_smoke_tree(tree)
    monkeypatch.setattr(ge.os, "utime", real)
    assert env.log[-1].startswith("smoke mtimes not pinned, all new") and not manifest.exists()
    assert (tree / "b.rs").stat().st_mtime_ns > first, "the whole tree is new after a failed pin"
    index[0] = _index(tree, {"a.rs": "a1", "b.rs": "b1"})  # main reverted to the first content
    env._pin_smoke_tree(tree)
    assert (tree / "a.rs").stat().st_mtime_ns > first, "no record survived, so the reverted file is new"


def test_a_failed_index_read_deletes_the_record_and_stamps_the_tree(monkeypatch: Any, tmp_path: Path) -> None:
    from runner import github_env as ge

    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path / "target"))
    tree, manifest = tmp_path / "tree", tmp_path / "target" / "smoke" / ge.SMOKE_MTIMES
    tree.mkdir()
    index, code = [_index(tree, {"a.rs": "a1"})], [0]
    env = _pin_env(tmp_path, index, code)
    env._pin_smoke_tree(tree)
    os.utime(tree / "a.rs", ns=(5, 5))
    code[0] = 128
    env._pin_smoke_tree(tree)
    assert not manifest.exists() and (tree / "a.rs").stat().st_mtime_ns > 5
    assert env.log[-1] == "smoke mtimes not pinned, all new: index locked"


def test_a_query_data_change_makes_everything_new_and_times_only_move_forward(tmp_path: Path) -> None:
    """D-1890, narrowed by D-1911: only a changed existing `.sqlx/` file makes every path new. Every task that
    added a query made the whole tree new under D-1890's rule, so every smoke rebuilt every member."""
    from runner import github_env as ge

    tree, manifest = tmp_path / "tree", tmp_path / "m.json"
    tree.mkdir()
    ge.pin_smoke_mtimes(tree, _index(tree, {"a.rs": "a1", ".sqlx/q1.json": "q1"}), manifest, 1_000)
    assert ge.pin_smoke_mtimes(tree, _index(tree, {"a.rs": "a1", ".sqlx/q1.json": "q1", ".sqlx/q2.json": "q2"}),
                               manifest, 2_000) == 2, "an added query file: itself and its folder"
    assert (tree / "a.rs").stat().st_mtime_ns == 1_000, "the source naming a new query rebuilds its own crate"
    assert ge.pin_smoke_mtimes(tree, _index(tree, {"a.rs": "a1", ".sqlx/q1.json": "q9", ".sqlx/q2.json": "q2"}),
                               manifest, 3_000) == 5, "a changed query file: everything"
    assert (tree / "a.rs").stat().st_mtime_ns == 3_000, "no dep-info lists `.sqlx/`, so everything rebuilds"
    (tree / ".sqlx/q2.json").unlink()
    assert ge.pin_smoke_mtimes(tree, _index(tree, {"a.rs": "a1", ".sqlx/q1.json": "q9"}), manifest, 4_000) == 1, \
        "a removed query file: only its folder"
    assert (tree / "a.rs").stat().st_mtime_ns == 3_000
    assert ge.pin_smoke_mtimes(tree, _index(tree, {"a.rs": "a2", ".sqlx/q1.json": "q9"}), manifest, 10) == 1
    assert (tree / "a.rs").stat().st_mtime_ns == 4_001, "a clock that stepped back still moves forward"


def _debug(root: Path) -> Path:
    """A fake worker `target/debug`: one dependency and one member in each seeded folder."""
    debug = root / "target" / "debug"
    for name in ("deps/libserde-0123456789abcdef.rlib", "deps/askhava_core-0123456789abcdef.d",
                 "deps/libaskhava_core-0123456789abcdef.rlib", "deps/message_log-0123456789abcdef",
                 "build/serde-0123456789abcdef/output", ".fingerprint/askhava-core-0123456789abcdef/lib",
                 ".fingerprint/serde-fedcba9876543210/lib", "incremental/serde-0123456789abcdef/x", "server"):
        (debug / name).parent.mkdir(parents=True, exist_ok=True)
        (debug / name).write_text(name)
    return debug


def test_the_seed_copies_only_dependency_artifacts_as_real_copies(tmp_path: Path) -> None:
    from runner import github_env as ge

    debug = _debug(tmp_path)
    entries = ge.seed_files(debug, {"askhava-core", "message_log"})
    assert [e.relative_to(debug).as_posix() for e in entries] == [
        "deps/libserde-0123456789abcdef.rlib", "build/serde-0123456789abcdef", ".fingerprint/serde-fedcba9876543210"]
    into = tmp_path / "smoke" / "debug"
    ge.copy_seed(debug, into, entries)
    copied = into / ".fingerprint/serde-fedcba9876543210/lib"
    assert copied.read_text() == ".fingerprint/serde-fedcba9876543210/lib"
    assert copied.stat().st_ino != (debug / ".fingerprint/serde-fedcba9876543210/lib").stat().st_ino, "never a hard link"
    assert ge._tree_size(entries) == sum(len(e) for e in ("deps/libserde-0123456789abcdef.rlib",
                                                          "build/serde-0123456789abcdef/output",
                                                          ".fingerprint/serde-fedcba9876543210/lib"))


def test_the_smoke_target_is_seeded_once_and_only_with_room(monkeypatch: Any, tmp_path: Path) -> None:
    from runner import github_env as ge

    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path / "target"))
    _debug(tmp_path)
    meta = json.dumps({"packages": [{"name": "askhava-core", "targets": [{"name": "askhava_core"}, {"name": "message_log"}]}]})
    env = ge.GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x",
                       run=lambda a, i, c: (0, meta, "") if a[:2] == ["cargo", "metadata"] else (0, "", ""))
    usage = shutil.disk_usage(tmp_path)
    monkeypatch.setattr(ge.shutil, "disk_usage", lambda p: usage._replace(free=ge.SEED_MARGIN))
    env._seed_smoke_target(tmp_path)
    smoke = tmp_path / "target" / "smoke" / "debug"
    assert not smoke.exists() and env.log[-1].startswith("smoke target seed: 3 entries"), "short of disk: logged, not seeded"
    monkeypatch.setattr(ge.shutil, "disk_usage", lambda p: usage._replace(free=ge.SEED_MARGIN * 4))
    (tmp_path / "target" / "smoke").mkdir(parents=True)
    (tmp_path / "target" / "smoke" / ge.SMOKE_MTIMES).write_text("{}")
    env._seed_smoke_target(tmp_path)
    assert sorted(p.name for p in (smoke / "deps").iterdir()) == ["libserde-0123456789abcdef.rlib"]
    assert not (tmp_path / "target" / "smoke" / ge.SMOKE_MTIMES).exists(), "a seeded target starts with every file new"
    (tmp_path / "target" / "debug" / "deps" / "libtokio-0123456789abcdef.rlib").write_text("x")
    env._seed_smoke_target(tmp_path)
    assert not (smoke / "deps" / "libtokio-0123456789abcdef.rlib").exists(), "a target with dependencies is left alone"


def test_the_smoke_build_has_its_own_cache_entry() -> None:
    """D-1890, D-1911: `target/smoke` is its own cache entry; the worker's lists `target/debug` only (an exclusion
    does not reach inside a listed folder); the smoke entry is saved only by a job that ran the smoke, under a
    key with the run id, and restored newest-first from the same commit, then the same lock."""
    steps = {step.get("name"): step for step in _flow()["jobs"]["backup"]["steps"]}
    rust = steps["Rust build cache (D-514)"]["with"]
    restore = steps["Smoke build cache, restore (D-1890, D-1911)"]
    save = steps["Smoke build cache, save (D-1911)"]
    assert "target/debug" in rust["path"].split() and "target" not in rust["path"].split()
    assert restore["uses"] == "actions/cache/restore@v4" and save["uses"] == "actions/cache/save@v4"
    assert restore["with"]["path"] == save["with"]["path"] == "target/smoke"
    assert restore["with"]["key"].endswith("-${{ github.sha }}-${{ github.run_id }}-${{ github.run_attempt }}")
    assert save["with"]["key"] == "${{ steps.%s.outputs.cache-primary-key }}" % restore["id"]
    assert "hashFiles('target/smoke-ran')" in save["if"] and save["if"].startswith("always()")
    keys = restore["with"]["restore-keys"].splitlines()
    assert keys[0].endswith("-${{ github.sha }}-") and all(k.startswith("rust-smoke-") for k in keys)
    assert rust["key"].startswith("rust-${{ runner.os }}-"), "so the worker's restore keys never pick a smoke entry"
    order = [step.get("name") or step.get("uses") for step in _flow()["jobs"]["backup"]["steps"]]
    assert order.index("Smoke build cache, save (D-1911)") > order.index("anthropics/claude-code-action@v1"), "after the session"


def test_actions_minutes_states_the_public_repository() -> None:
    minutes = json.loads((ROOT / "kit" / "pacing.json").read_text(encoding="utf-8"))["actions_minutes"]
    assert minutes["repository"] == "public" and minutes["included_per_month"] is None
    assert "private" not in minutes or minutes["private"]["included_per_month"] == 3000


def test_the_keeper_key_is_recorded_for_status_and_passes_the_schema() -> None:
    from scripts import check_state

    out = kp.record(state(), 400, NOW)
    assert out["keeper"] == {"last_run_at": at(0), "next_wake_at": at(340)}
    assert "keeper" not in state()  # the input is not changed
    assert [p for p in check_state.check(out) if "keeper" in p] == []


def test_keeper_armed_writes_through_update_state(monkeypatch: Any) -> None:
    from runner import __main__ as entry

    seen: list[dict[str, Any]] = []

    class Env:
        def update_state(self, transform: Any) -> dict[str, Any] | None:
            seen.append(transform(state()))
            return seen[-1]

    monkeypatch.setattr(entry, "now_utc", lambda: NOW)
    assert entry.keeper_armed(65, Env()) == "keeper_armed: 65 minutes"  # type: ignore[arg-type]  # reason: a fake env
    assert seen[0]["keeper"]["next_wake_at"] == at(65)
    assert entry.parse_args(["--keeper-armed", "20"]).keeper_armed == 20
    wf = _flow()["jobs"]["backup"]["steps"][-1]["run"]
    assert 'python3 -m runner --keeper-armed "$minutes"' in wf


def test_an_unreadable_state_or_lease_never_looks_free() -> None:
    unreadable = {"unreadable": True}
    assert kp.wake_after(state(), unreadable, NOW)[0] == 20 and "could not be read" in kp.wake_after(state(), unreadable, NOW)[1]
    assert kp.on_wake(state(), unreadable, [], "9", NOW)[:2] == ("rearm", 20)
    assert kp.on_wake(unreadable, {}, [], "9", NOW)[:2] == ("rearm", 20)  # not "the flag is off"
    assert kp.wake_after(unreadable, {}, NOW)[0] == 20
    assert kp.on_wake(state(), {}, [], "9", NOW)[0] == "go"


def test_the_workflow_marks_a_failed_read_unreadable_and_runs_the_keeper_from_mains_worktree() -> None:
    backup = _flow()["jobs"]["backup"]
    note = next(s for s in backup["steps"] if str(s.get("name", "")).startswith("Note when this job started"))["run"]
    assert 'git worktree add -q --detach "$RUNNER_TEMP/keeper" HEAD' in note
    arm = backup["steps"][-1]["run"]
    assert '(cd "$RUNNER_TEMP/keeper" && python3 -m runner --keeper-armed "$minutes"' in arm
    assert 'cd "$RUNNER_TEMP/keeper" && python3 -m runner.keeper arm' in arm
    for script in (arm, _flow()["jobs"]["keeper"]["steps"][-1]["run"]):
        assert 'grep -q "HTTP 404"' in script and '{"unreadable": true}' in script


def test_the_keeper_runs_from_a_worktree_of_the_repository(tmp_path: Path) -> None:
    """The arm step's call works from a detached worktree: `runner` finds its root by its own path."""
    tree = tmp_path / "keeper"
    subprocess.run(["git", "worktree", "add", "-q", "--detach", str(tree), "HEAD"], cwd=ROOT, check=True)
    try:
        done = subprocess.run([sys.executable, "-m", "runner.keeper", "clamp", "30"], cwd=tree, capture_output=True, text=True, check=True)
        assert done.stdout == "30\n"
        helped = subprocess.run([sys.executable, "-m", "runner", "--help"], cwd=tree, capture_output=True, text=True, check=True)
        assert "--keeper-armed" in helped.stdout
    finally:
        subprocess.run(["git", "worktree", "remove", "--force", str(tree)], cwd=ROOT, check=False)


def _warm_env(tmp_path: Path, monkeypatch: Any, fail_at: str = "", red: tuple[str, ...] = ("clippy",)) -> tuple[Any, list[list[str]], list[tuple[list[str], Path, float]]]:
    """A GitHubEnv for `--warm-smoke` (D-2007): a fake git whose worktree add writes a Cargo.toml and a
    rust-toolchain.toml, and a fake `warm_run` that records each cargo call (raising TimeoutExpired at the
    command whose second word is `fail_at`). No real cargo or network."""
    from runner import github_env as ge

    monkeypatch.setenv("RUNNER_TEMP", str(tmp_path / "temp"))
    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path / "target"))
    tree = ge.smoke_tree_dir()
    calls: list[list[str]] = []
    built: list[tuple[list[str], Path, float]] = []

    def run(a: list[str], i: str | None, c: Path) -> tuple[int, str, str]:
        calls.append(list(a))
        if a[:3] == ["git", "worktree", "add"]:
            _index(tree, {"Cargo.toml": "w1"})
            (tree / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.99.0"\n')
        if a[:2] == ["git", "ls-files"]:
            return 0, "100644 " + "w" * 40 + " 0\tCargo.toml\0", ""
        if a[:2] == ["git", "rev-parse"]:
            return 0, "e" * 40 + "\n", ""
        return 0, "", ""

    def warm(cmd: list[str], cwd: Path, left: float) -> int:
        built.append((cmd, cwd, left))
        if cmd[1] == fail_at:
            raise subprocess.TimeoutExpired(cmd, left)
        return 1 if cmd[1] in red else 0

    env = ge.GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=run)
    env.warm_run = warm
    return env, calls, built


def test_the_warm_up_prepares_the_smoke_tree_as_the_smoke_does_and_only_compiles(monkeypatch: Any, tmp_path: Path) -> None:
    """D-2007: `--warm-smoke` uses smoke_main's own tree, seed and pin (so the manifest the session's smoke
    reads is the one it writes), then runs only the compile steps; a red clippy does not stop the build."""
    from runner import github_env as ge

    env, calls, built = _warm_env(tmp_path, monkeypatch)
    tree = ge.smoke_tree_dir()
    assert env.warm_smoke().endswith(", nothing new to cache"), "an instant build (a head already built) is not saved"
    assert not (tmp_path / "target" / ge.SMOKE_RAN).exists(), "no fresh multi-GB entry from an idle job (D-2007 review)"
    calls.clear(), built.clear(), env.log.clear()
    monkeypatch.setattr(ge, "WARM_SAVE_SECONDS", 0)
    line = env.warm_smoke()
    assert line.endswith(", cache marked for saving")
    assert line.startswith(f"smoke warm-up on {'e' * 12}: toolchain exit 0, clippy exit 1, build exit 0, test exit 0; ")
    add = calls.index(["git", "worktree", "add", "--detach", str(tree), "e" * 40])
    assert calls.index(["git", "worktree", "remove", "--force", str(tree)]) < add < calls.index(["git", "ls-files", "-s", "-z"])
    assert [b[0] for b in built] == [["rustup", "toolchain", "install", "1.99.0", "--profile", "minimal", "-c", "clippy", "-c", "rustfmt"],
                                     *(list(c) for c in ge.WARM_COMMANDS)]
    assert all(cwd == tree and 0 < left <= ge.WARM_TIMEOUT for _, cwd, left in built)
    assert ["bash", "scripts/smoke.sh"] not in calls, "the warm-up never runs the smoke itself"
    assert not [c for c in calls if c[:2] in (["git", "push"], ["git", "fetch"]) or c[:1] == ["gh"]], "no state, lease or network"
    assert (tmp_path / "target" / "smoke" / ge.SMOKE_MTIMES).exists() and not tree.exists()
    assert (tmp_path / "target" / ge.SMOKE_RAN).exists(), "the save step keeps a warm build even if the session dies"
    assert "smoke mtimes: 0 new or changed paths" in env.log, "the second run re-pins to the times the first recorded"


def test_a_warm_up_whose_cargo_commands_all_fail_leaves_no_marker(monkeypatch: Any, tmp_path: Path) -> None:
    from runner import github_env as ge

    env, _, built = _warm_env(tmp_path, monkeypatch, red=("clippy", "build", "test"))
    monkeypatch.setattr(ge, "WARM_SAVE_SECONDS", 0)
    assert env.warm_smoke().endswith(", nothing new to cache") and len(built) == 4
    assert not (tmp_path / "target" / ge.SMOKE_RAN).exists()


def test_a_warm_up_that_outruns_its_budget_stops_and_still_cleans_up(monkeypatch: Any, tmp_path: Path) -> None:
    from runner import github_env as ge

    env, _, built = _warm_env(tmp_path, monkeypatch, fail_at="build", red=("clippy", "test"))
    monkeypatch.setattr(ge, "WARM_SAVE_SECONDS", 0)
    line = env.warm_smoke()
    assert f"clippy exit 1, build timed out (budget {ge.WARM_TIMEOUT} s);" in line
    assert [b[0][1] for b in built] == ["toolchain", "clippy", "build"], "no command after the one that timed out"
    assert not ge.smoke_tree_dir().exists() and (tmp_path / "target" / ge.SMOKE_RAN).exists(), "a timeout that compiled is kept"


def test_a_dry_run_warm_up_builds_nothing(monkeypatch: Any, tmp_path: Path) -> None:
    env, calls, built = _warm_env(tmp_path, monkeypatch)
    env.dry_run = True
    assert env.warm_smoke() == f"dry-run: smoke warm-up not run on {'e' * 12}"
    assert built == [] and not [c for c in calls if c[:2] == ["git", "worktree"]]


def test_the_warm_up_compiles_exactly_what_smoke_sh_compiles() -> None:
    """D-2007: the warm-up is wasted unless its flags and environment are the smoke's, so each command is held to
    scripts/smoke.sh's own line (`cargo test` there without `--no-run`), with its SQLX_OFFLINE and toolchain install."""
    from runner import github_env as ge

    smoke = {line.strip() for line in (ROOT / "scripts" / "smoke.sh").read_text(encoding="utf-8").splitlines()}
    warm = {" ".join(c for c in cmd if c != "--no-run") for cmd in ge.WARM_COMMANDS}
    for cmd in warm:
        assert cmd in smoke, cmd
    compiles = {line for line in smoke if line.split()[:2] in (["cargo", "clippy"], ["cargo", "build"], ["cargo", "test"])}
    assert compiles == warm, "every clippy, build and test line of smoke.sh is warmed, and only those"
    assert ge.WARM_COMMANDS[-1][-1] == "--no-run" and sum("--no-run" in c for c in ge.WARM_COMMANDS) == 1
    assert any(line.startswith("export SQLX_OFFLINE=true") for line in smoke)
    assert 'rustup toolchain install "${toolchain}" --profile minimal -c clippy -c rustfmt >/dev/null 2>&1' in smoke
    assert ge.warm_commands(ROOT)[0][:3] == ["rustup", "toolchain", "install"]
    assert ge.WARM_TIMEOUT > ge.SMOKE_TIMEOUT


def test_run_warm_uses_the_smoke_environment_and_kills_the_whole_group(monkeypatch: Any, tmp_path: Path) -> None:
    from runner import github_env as ge

    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path / "target"))
    monkeypatch.delenv("SQLX_OFFLINE", raising=False)
    out = tmp_path / "env.txt"
    script = f'echo "$CARGO_TARGET_DIR $CARGO_INCREMENTAL $SQLX_OFFLINE" > {out}'
    assert ge.run_warm(["sh", "-c", script], tmp_path, 30) == 0
    assert out.read_text().split() == [str(tmp_path / "target" / "smoke"), "0", "true"]
    pid_file = tmp_path / "child.pid"
    try:
        ge.run_warm(["sh", "-c", f"sleep 30 & echo $! > {pid_file}; wait"], tmp_path, 1)
        raise AssertionError("no timeout")
    except subprocess.TimeoutExpired:
        pass
    child = int(pid_file.read_text())
    for _ in range(50):  # SIGKILL is delivered asynchronously: give the kernel up to 5 s to finish the child
        try:
            state = Path(f"/proc/{child}/stat").read_text().split(")")[-1].split()[0]
        except OSError:
            state = "gone"
        if state in ("gone", "Z"):
            break
        time.sleep(0.1)
    assert state in ("gone", "Z"), "a background child (rustc) is killed with the group"


def test_warm_smoke_cli_never_reads_state_and_always_exits_zero(monkeypatch: Any, capsys: Any) -> None:
    from runner import __main__ as entry, github_env as ge

    assert entry.parse_args(["--warm-smoke"]).warm_smoke is True
    monkeypatch.setattr(entry, "build", lambda *a: (_ for _ in ()).throw(AssertionError("no cycle")))
    monkeypatch.setattr(ge.GitHubEnv, "warm_smoke", lambda self: (_ for _ in ()).throw(RuntimeError("git worktree add failed")))
    assert entry.main(["--warm-smoke"]) == 0
    assert capsys.readouterr().out.strip().endswith("runner: smoke warm-up failed: git worktree add failed")
    monkeypatch.undo()
    monkeypatch.setattr(entry, "build", lambda *a: (_ for _ in ()).throw(AssertionError("no cycle")))
    assert entry.main(["--warm-smoke", "--dry-run"]) == 0
    assert "runner: dry-run: smoke warm-up not run on " in capsys.readouterr().out


def test_the_warm_up_step_runs_after_the_cache_restore_and_before_the_session() -> None:
    """D-2007: in the backup job, after the smoke cache is restored and the prerequisites are installed, before the
    session; it can never fail the job, has its own time cap above WARM_TIMEOUT and gets no secret."""
    from runner import github_env as ge

    steps = _flow()["jobs"]["backup"]["steps"]
    order = [step.get("name") or step.get("uses") for step in steps]
    name = "Warm the smoke build before the session (D-2007)"
    warm = steps[order.index(name)]
    assert order.index("Smoke build cache, restore (D-1890, D-1911)") < order.index(name)
    assert order.index("Smoke-test prerequisites (what Ci installs for itself)") < order.index(name)
    assert order.index(name) < order.index("anthropics/claude-code-action@v1") < order.index("Smoke build cache, save (D-1911)")
    assert warm["continue-on-error"] is True and warm["run"].strip() == "python3 -m runner --warm-smoke"
    assert ge.WARM_TIMEOUT / 60 < warm["timeout-minutes"] <= 30
    assert warm["if"] == "hashFiles('Cargo.lock') != ''" and "secrets." not in json.dumps(warm)
