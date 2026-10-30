"""Reserved decision numbers (D-530): each claim gives the task a block of D-numbers in `decision_ids`
and advances `next_decision` in the same state write, so parallel work never takes one number twice.
Covers runner/state.py's reservation, the hand-off field and scripts/check_state.py's cross-check.
Fake data only.
"""

from __future__ import annotations

from datetime import datetime, timedelta, timezone
from types import SimpleNamespace

import check_state as cs
from runner import __main__ as entry, state as st

NOW = datetime(2026, 11, 1, 12, 0, tzinfo=timezone.utc)
MAIN = {1, 2, 3, 517, 521}  # the D-numbers on main's docs/decisions.md in these tests


def two_tasks() -> st.State:
    return st.with_task(st.with_task(st.new_state(), "T001"), "T002")


def test_decision_numbers_reads_entry_lines_only_and_keeps_repeats() -> None:
    text = "# Decisions\n\nSee D-900 in prose.\n- **D-001** a.\n- **D-1000** b.\n  - **D-777** indented.\n- **D-001** again.\n"
    assert st.decision_numbers(text) == [1, 1000, 1]


def test_claim_reserves_a_block_above_mains_highest_number_and_advances_the_counter() -> None:
    s = st.claim(two_tasks(), "T001", "a", NOW, taken=MAIN)
    assert st.entry(s, "T001")["decision_ids"] == [522, 523, 524, 525, 526]
    assert s["next_decision"] == 527 and s["lease"]["task"] == "T001"
    assert cs.check(s, now=NOW) == []


def test_a_claim_without_mains_numbers_reserves_nothing() -> None:
    s = st.claim(two_tasks(), "T001", "a", NOW)
    assert "decision_ids" not in st.entry(s, "T001") and "next_decision" not in s


def test_a_reclaim_reuses_the_tasks_block() -> None:
    s = st.claim(two_tasks(), "T001", "a", NOW, taken=MAIN)
    s = st.release_expired(s, NOW + timedelta(hours=3, minutes=1))
    assert st.status(s, "T001") == st.QUEUED
    again = st.claim(s, "T001", "b", NOW + timedelta(hours=4), taken=MAIN)
    assert st.entry(again, "T001")["decision_ids"] == [522, 523, 524, 525, 526] and again["next_decision"] == 527


def test_a_block_whose_number_reached_main_is_replaced() -> None:
    """A FIX-MAIN or a re-queued task whose number landed on main meanwhile (a human used it) gets a new block."""
    s = st.claim(two_tasks(), "T001", "a", NOW, taken=MAIN)
    s = st.release_expired(s, NOW + timedelta(hours=3, minutes=1))
    again = st.claim(s, "T001", "b", NOW + timedelta(hours=4), taken=MAIN | {524})
    assert st.entry(again, "T001")["decision_ids"] == [527, 528, 529, 530, 531] and again["next_decision"] == 532


def test_two_claims_get_disjoint_blocks() -> None:
    s = st.claim(st.with_leases_map(two_tasks()), "T001", "a", NOW, taken=MAIN)
    s = st.claim(s, "T002", "b", NOW, lane="2", taken=MAIN)
    first, second = st.entry(s, "T001")["decision_ids"], st.entry(s, "T002")["decision_ids"]
    assert not set(first) & set(second) and second == [527, 528, 529, 530, 531] and s["next_decision"] == 532
    assert cs.check(s, lanes=2, now=NOW) == []


def test_the_counter_never_hands_out_a_number_main_already_uses() -> None:
    """A human fix above the counter (governance section 3.6) lands on main: the next block starts above it."""
    s = dict(two_tasks(), next_decision=530)
    s = st.claim(s, "T001", "a", NOW, taken=MAIN | {533})
    assert st.entry(s, "T001")["decision_ids"] == [534, 535, 536, 537, 538]
    s = st.claim(dict(two_tasks(), next_decision=600), "T002", "a", NOW, taken=MAIN)
    assert st.entry(s, "T002")["decision_ids"][0] == 600


def test_a_lost_counter_still_starts_above_every_reserved_block() -> None:
    s = st.with_task(two_tasks(), "T002", decision_ids=[540, 541, 542, 543, 544])
    s = st.claim(s, "T001", "a", NOW, taken=MAIN)
    assert st.entry(s, "T001")["decision_ids"][0] == 545


def test_fix_main_gets_a_block_too() -> None:
    s = st.with_task(dict(two_tasks(), main_red={"check": "smoke", "sha": "abc"}), st.FIX_MAIN, type="harness")
    s = st.claim(s, st.FIX_MAIN, "a", NOW, taken=MAIN)
    assert st.entry(s, st.FIX_MAIN)["decision_ids"] == [522, 523, 524, 525, 526]


def test_the_handoff_carries_the_tasks_decision_ids() -> None:
    s = st.claim(two_tasks(), "T001", "a", NOW, taken=MAIN)
    env = SimpleNamespace(last_state=s, dry_run=True, branch_for=lambda tid: f"claude/task/{tid}-x", prepared={})
    cfg = SimpleNamespace(session="a", graph=[{"id": "T001", "deps": []}], models={})
    info = entry.write_handoff(cfg, env)  # type: ignore[arg-type]  # reason: a stand-in for Config and GitHubEnv
    assert info["task"] == "T001" and info["decision_ids"] == [522, 523, 524, 525, 526]
    old = st.claim(two_tasks(), "T001", "a", NOW)
    assert entry.write_handoff(cfg, SimpleNamespace(last_state=old, dry_run=True, branch_for=env.branch_for, prepared={}))["decision_ids"] == []  # type: ignore[arg-type]  # reason: as above


def test_check_state_validates_the_counter_and_the_blocks() -> None:
    base = {"schema": 1, "lease": None, "tasks": [{"id": "T001"}, {"id": "T002"}]}
    assert cs.check(base) == []  # a state written before D-530 stays valid
    good = {"schema": 1, "next_decision": 532, "tasks": [{"id": "T001", "decision_ids": [522, 523]}, {"id": "T002", "decision_ids": [527]}]}
    assert cs.check(good) == []
    assert cs.check(dict(good, next_decision="532")) == ["state.next_decision: expected integer, got str"]
    bad_ids = {"schema": 1, "tasks": [{"id": "T001", "decision_ids": ["D-523"]}]}
    assert cs.check(bad_ids) == ["state.tasks[0].decision_ids[0]: expected integer, got str"]
    shared = {"schema": 1, "tasks": [{"id": "T001", "decision_ids": [522, 523]}, {"id": "T002", "decision_ids": [523]}]}
    assert cs.check(shared) == ["state.tasks: D-523 is reserved by T001, T002"]
    behind = dict(good, next_decision=527)
    assert cs.check(behind) == ["state.next_decision: 527 is not above the highest reserved number D-527"]
