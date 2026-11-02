"""The owner's dashboard at the top of STATUS.md (runner/status.py, runner.github_env.render_status, D-1462).

A fixture state with fake task ids and titles covers an in-flight task, a retry with its failure, a plan
gap, an idle lane, an old last run, the keeper present and absent, and the week reading.
"""

from __future__ import annotations

import json
from datetime import datetime, timedelta, timezone
from pathlib import Path

from runner import state as st, status
from runner.github_env import Board, GitHubEnv, render_status

NOW = datetime(2026, 11, 2, 18, 0, tzinfo=timezone.utc)
GRAPH = [
    {"id": "T001", "title": "fake: first task", "stage": "1", "type": "build"},
    {"id": "T002", "title": "fake: second task", "stage": "2", "type": "build"},
    {"id": "T003", "title": "fake: third task", "stage": "2", "type": "build"},
    {"id": "T004", "title": "fake: held task", "stage": "2", "type": "build"},
    {"id": "T005", "title": "fake: owner gate", "stage": "0.5", "type": "gate"},
]


def _row(attempt: int, result: str, start: str, end: str) -> dict[str, object]:
    return {"attempt": attempt, "result": result, "started_at": start, "ended_at": end}


def fixture(in_flight: bool = True) -> dict[str, object]:
    state = st.new_state()
    state = st.with_task(state, "T001", status=st.DONE, stage="1", history=[_row(1, "done", "2026-11-02T09:00:00Z", "2026-11-02T09:40:00Z")])
    state = st.with_task(state, "T002", status=st.DONE, stage="2", last_failure="Ci: failure; Coverage: failure\nCoverage failed at step: floor | x" + "y" * 200,
                         history=[_row(1, "check_failed", "2026-11-02T10:00:00Z", "2026-11-02T10:30:00Z"),
                                  _row(2, "done", "2026-11-02T10:35:00Z", "2026-11-02T11:20:00Z")])
    state = st.with_task(state, "T003", status=st.IN_PROGRESS if in_flight else st.QUEUED, stage="2", attempt_started_at="2026-11-02T17:30:00Z")
    state = st.with_task(state, "T004", stage="2", plan_gap={"at": "2026-11-02T12:00:00Z", "problems": ["tests/fake_missing.py: no task owns it"]})
    state["stage_proofs"] = {"1": {"evidence": "fake"}}
    state["usage"] = {"session_reading": {"week": {"utilization": 0.93, "resets_at": "2026-11-05T17:00:00Z"}}}
    state["pacing"] = {"standing": {"week": {"used": 186_000_000, "budget": 194_000_000, "left": 8_000_000}}}
    if in_flight:
        state["lease"] = {"task": "T003", "session": "fake", "expires": "2026-11-02T20:30:00Z"}
    return st.with_leases_map(state)


def render(state: dict[str, object], last_run: datetime | None, strict: tuple[str, ...] = ()) -> str:
    return render_status(state, "**Requirements verified:** 1 of 9", NOW, "fake pacing", Board(tuple(GRAPH), last_run, strict, ()))


def test_verdict_and_dashboard_for_an_in_flight_task() -> None:
    text = render(fixture(), NOW - timedelta(minutes=10))
    assert "**Building: T003 in progress since 17:30Z; 2 of 4 done; week 93% used, resets Thu 2026-11-05 17:00Z (10:00 am Phoenix).**" in text
    assert "- **Tasks done:** 2 of 4 build tasks (owner gates not counted)" in text
    assert "- **Stage:** stage 2; stage proofs recorded: 1" in text
    assert "- **Lane:** lane 1: T003 claimed 17:30Z, lease until 20:30Z" in text
    assert "- **Week:** 93% used, 8.0M tokens left; resets Thu 2026-11-05 17:00Z (10:00 am Phoenix)" in text
    assert "- **Last run:** 17:50Z, 10 min before this update" in text and "WARNING" not in text
    assert text.index("**Building") < text.index("## Needs attention") < text.index("## Last 24 hours") < text.index("**Requirements verified")


def test_last_24_hours_shows_the_retry_reason_cut_to_120_characters() -> None:
    text = render(fixture(), NOW)
    row = next(line for line in text.splitlines() if line.startswith("| T002 | fake: second task | done | 2 | 45 min | 11:20Z | "))
    reason = row.removeprefix("| T002 | fake: second task | done | 2 | 45 min | 11:20Z | ").removesuffix(" |")
    assert reason.startswith("Ci: failure; Coverage: failure / Coverage failed at step: floor \\| x")
    assert len(reason.replace("\\|", "|")) == status.REASON_CHARS and reason.endswith("y")
    assert "| T002 | fake: second task | check_failed | 1 | 30 min | 10:30Z |  |" in text
    assert "| T001 | fake: first task | done | 1 | 40 min | 09:40Z |  |" in text


def test_rows_older_than_a_day_are_left_out() -> None:
    assert status.last_day(fixture(), GRAPH, NOW + timedelta(days=2)) == ["- no attempt ended in the last 24 hours"]


def test_plan_gap_strict_red_and_hold_need_attention() -> None:
    state = dict(fixture(), build_hold="Gate P: fake hold")
    text = render(state, NOW, strict=("T001: stored context_pct 1% but it measures 2%; regenerate", "T004: fake drift"))
    assert "- PLAN GAP T004 (since 2026-11-02T12:00:00Z, no attempt used" in text
    assert "- Strict plan check red: 1 problem(s) on tasks still to run (+1 on DONE tasks); first: T004: fake drift" in text
    assert "- BUILD HOLD: Gate P: fake hold" in text and "**On hold: Gate P: fake hold;" in text


def test_idle_lane_old_last_run_and_unknown_keeper() -> None:
    text = render(fixture(in_flight=False), NOW - timedelta(hours=3))
    assert "**Idle since 11:20Z; 2 of 4 done;" in text and "- **Lane:** idle since 11:20Z (no task in progress)" in text
    assert "- **Last run:** 15:00Z, 3 h 0 min before this update" in text
    assert "- **WARNING:** no runner cycle logged for over 90 minutes before this update" in text
    assert "- **Keeper:** unknown" in text
    assert "- **Last run:** unknown, unknown before this update" in render(fixture(), None)


def test_keeper_and_red_main_and_pacing_wait() -> None:
    state = dict(fixture(in_flight=False), keeper={"last_run_at": "2026-11-02T17:40:00Z", "next_wake_at": "2026-11-02T18:00:00Z"})
    state["pacing"] = {"decision": "wait", "waiting_until": "2026-11-02T19:00:00Z"}
    text = render(state, NOW)
    assert "- **Keeper:** last ran 17:40Z, next wake 18:00Z" in text and "**Waiting for tokens until 19:00Z;" in text
    assert "93% used; resets" in text  # no ledger left figure without pacing standing
    assert "**Stopped: main is red" in render(dict(state, main_red={"check": "nightly"}), NOW)


def test_week_falls_back_to_the_ledger_and_to_no_reading() -> None:
    state = {"pacing": {"standing": {"week": {"used": 50, "budget": 100, "left": 50, "resets_at": "2026-11-05T17:00:00Z"}}}}
    assert status.week(state) == ("50% used (ledger), 0.0M tokens left", st.parse("2026-11-05T17:00:00Z"))
    assert status.week({}) == ("no reading yet", None)
    assert status.clock(None) == "unknown"


def test_no_stage_left_and_unknown_finish() -> None:
    state = {"tasks": [{"id": t["id"], "status": st.DONE} for t in GRAPH]}
    assert status.current_stage(state, GRAPH) == "every stage's tasks done; stage proofs recorded: none"
    assert status.lane_line({"tasks": []}) == "idle since unknown (no task in progress)"


def test_status_md_carries_no_failure_token_phone_or_email() -> None:
    """A failure text is redacted before it reaches the table (TOKEN_LIKE and the PII scanner's shapes)."""
    state = fixture()
    state["tasks"][1]["last_failure"] = "ghp_" + "a" * 30 + " 928-555-0100 fake@example.com"
    text = render(state, NOW)
    assert "ghp_" not in text and "555-0100" not in text and "fake@example.com" not in text and "[redacted]" in text


def test_strict_problems_only_on_done_tasks_are_not_red() -> None:
    done_only = ("T001: stored context_pct 1% but it measures 2%; regenerate",)
    assert status.strict_line(fixture(), done_only) == "" and status.strict_line(fixture(), ()) == ""
    assert "Strict plan check" not in render(fixture(), NOW, strict=done_only)


def test_malformed_state_fields_never_stop_the_render() -> None:
    """A bad field becomes one "unavailable" line (D-1462): attempt "x", a None history row, a string lease."""
    state = fixture()
    state["tasks"][1]["history"] = [None, {"attempt": "x", "result": "check_failed", "ended_at": "2026-11-02T11:00:00Z"}]
    text = render(state, NOW)
    assert "| T002 | fake: second task | check_failed | x | ? | 11:00Z | Ci: failure" in text
    state["lease"] = "not a lease"
    state["leases"] = {"1": "not a lease"}
    text = render(state, NOW)
    assert "- dashboard unavailable: AttributeError" in text and "## Progress (requirements verified)" in text


def test_board_readings_from_the_env(tmp_path: Path) -> None:
    """strict_check runs once per process; the last run comes from the last cycle-log line."""
    calls: list[list[str]] = []

    def fake(args: list[str], stdin: str | None, cwd: Path) -> tuple[int, str, str]:
        calls.append(args)
        if args[:2] == ["python3", "scripts/gen_prompts.py"]:
            return 1, "T001: about 1 tokens\nERROR T001: stored context_pct 1% but it measures 2%; regenerate\nchecked 1 prompts, 1 problems\n", ""
        if args[:2] == ["git", "show"]:
            return 0, json.dumps({"at": "2026-11-02T17:00:00+00:00"}) + "\n", ""
        return 0, "12:00Z Harness: fake change (D-9999) (#1)\n12:05Z T001: fake task (#2)\n", ""

    env = GitHubEnv(tmp_path, GRAPH, tmp_path, repo="fake-owner/fake-repo", starter="routine", runner_sha="f" * 40, run=fake)
    env.state_head = "f" * 40
    board = env.board(NOW)
    assert board.strict == ("T001: stored context_pct 1% but it measures 2%; regenerate",)
    assert board.last_run == st.parse("2026-11-02T17:00:00Z") and board.merges == ("12:00Z Harness: fake change (D-9999) (#1)",)
    env.board(NOW)
    assert sum(1 for c in calls if c[:2] == ["python3", "scripts/gen_prompts.py"]) == 1


def test_board_readings_tolerate_failures(tmp_path: Path) -> None:
    def broken(args: list[str], stdin: str | None, cwd: Path) -> tuple[int, str, str]:
        return (2, "", "no such script") if args[0] == "python3" else (0, "not json\n", "")

    env = GitHubEnv(tmp_path, GRAPH, tmp_path, repo="fake-owner/fake-repo", starter="routine", runner_sha="f" * 40, run=broken)
    env.state_head = "f" * 40
    board = env.board(NOW)
    assert board.strict == () and board.last_run is None and board.merges == ()
