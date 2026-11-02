"""The keeper (D-1451): runner/keeper.py decides how long the next keeper sleeps and what it does when it
wakes; backup-runner.yml calls it. Also the smoke run's `cargo clean --workspace` (the T057 stale build)
and kit/pacing.json's public-repository `actions_minutes`. Fake data only."""

from __future__ import annotations

import json
import subprocess
import sys
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


# ---- the smoke's shared target (T057) and the Actions minutes note


def test_the_smoke_run_cleans_the_workspace_members_from_the_shared_target(tmp_path: Path) -> None:
    from runner import github_env as ge

    calls: list[list[str]] = []
    env = ge.GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x",
                       run=lambda a, i, c: (calls.append(list(a)), (0, "", ""))[1])
    env._forget_smoke_build()
    assert calls == []  # no Cargo.toml: nothing to clean
    (tmp_path / "Cargo.toml").write_text("[workspace]\n")
    env._forget_smoke_build()
    assert calls == [["cargo", "clean", "--workspace"]]
    assert "--workspace" in ge.CARGO_CLEAN_MEMBERS and "-p" not in ge.CARGO_CLEAN_MEMBERS


def test_the_clean_runs_with_the_smokes_target_dir(monkeypatch: Any) -> None:
    from runner import github_env as ge

    seen: list[dict[str, str] | None] = []
    monkeypatch.setattr(ge, "_run", lambda a, i, c, e, timeout=600: (seen.append(e), subprocess.CompletedProcess(a, 0, "", ""))[1])
    monkeypatch.setenv("GITHUB_WORKSPACE", "/work/fake")
    monkeypatch.delenv("CARGO_TARGET_DIR", raising=False)
    ge.run_command(list(ge.CARGO_CLEAN_MEMBERS), None, Path("."))
    assert seen[0] is not None and seen[0]["CARGO_TARGET_DIR"] == "/work/fake/target"


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
