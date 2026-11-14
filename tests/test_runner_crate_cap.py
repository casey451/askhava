"""kit/runner.json "crate_cap" (D-3303): with "exclusive_crates", an `app/` crate may hold up to `crate_cap` tasks in
flight (IN_PROGRESS or MERGE_PENDING, any lane) instead of D-3269's one, except a crate whose `CLAUDE.md` is at its
line limit, which still holds one. D-3269's reservation stays: a blocked top candidate keeps the last free slot in
each of its crates, so narrower tasks cannot overtake it. Refill counts exactly the claims runs would make.

On November 12, 2026 at 19:31Z only 4 of 7 lanes were busy because every claimable task shared a crate with work in
flight. Drills run over test_failure_drills.FakeEnv. Fake data only.
"""

from __future__ import annotations

import dataclasses
import inspect
import json
import subprocess
from datetime import timedelta
from pathlib import Path
from typing import Any

from test_failure_drills import NOW, FakeEnv, config, make_task
from test_runner_two_lanes import handed_off

import check_code_docs
from runner import __main__ as entry
from runner import cycle, lanes
from runner import state as st

ROOT = Path(__file__).resolve().parent.parent
NEVER_FULL = frozenset[str]()


def graph() -> list[dict[str, Any]]:
    """T001 holds `pay` in the tests; T002 and T003 (by module) also write it; T004 `site`; T005 no crate;
    T020 holds `web`; T010, wide, writes `pay` and `web`."""
    return [make_task("T001", owns=["app/pay/src/a.rs"]), make_task("T002", owns=["app/pay/src/b.rs"]),
            make_task("T003", owns=["tests/integration/tests/c.rs"], module="pay"),
            make_task("T004", owns=["app/site/src/d.rs"]), make_task("T005", owns=["docs/notes.md"], module=None),
            make_task("T020", owns=["app/web/src/e.rs"]), make_task("T010", owns=["app/pay/src/w.rs", "app/web/src/w.rs"])]


def by_id(tasks: list[dict[str, Any]]) -> Any:
    table = {t["id"]: t for t in tasks}
    return lambda tid: table[tid]


def holding(tasks: list[dict[str, Any]], *ids: str) -> st.State:
    """`ids` in flight on lanes 1, 2, ... (a live lease each)."""
    s = st.sync_graph(st.new_state(), tasks)
    for n, tid in enumerate(ids, start=1):
        s = st.claim(s, tid, f"s{tid}", NOW, lane=str(n))
    return s


def capped(tasks: list[dict[str, Any]], tmp_path: Path, lane: str, full: frozenset[str] = NEVER_FULL, **kw: Any) -> cycle.Config:
    """A Config with exclusive crates, a cap of 2 and `full` crates at their CLAUDE.md line limit."""
    cfg = dataclasses.replace(config(tasks, tmp_path, f"s{lane}"), lanes=7, lane_hint=lane, exclusive_crates=True,
                              crate_cap=2, crate_full=lambda crate, held: crate in full)
    return dataclasses.replace(cfg, **kw)


def limit(full: frozenset[str] = NEVER_FULL) -> lanes.Limit:
    return lanes.crate_limit(2, lambda crate, held: crate in full)


def test_cap_two_allows_a_second_task_in_a_crate_under_its_line_limit_and_refuses_a_third(tmp_path: Path) -> None:
    tasks = graph()[:5]
    one = holding(tasks, "T001")
    assert lanes.exclusive_ids(["T002", "T003", "T004"], one, NOW, by_id(tasks), limit()) == ["T002", "T003", "T004"]
    assert lanes.exclusive_ids(["T002", "T004"], one, NOW, by_id(tasks)) == ["T004"]  # D-3269's default limit: one
    two = holding(tasks, "T001", "T002")
    assert lanes.exclusive_ids(["T003", "T004"], two, NOW, by_id(tasks), limit()) == ["T004"]
    why = lanes.crate_clash(tasks[2], [tasks[0], tasks[1]], limit())
    assert why == "writes app/pay/, which T001, T002 in flight also write (at most 2 there; D-3269, D-3303)"
    env = FakeEnv(handed_off)
    env.store = holding(tasks[:3], "T001")
    assert cycle.run_cycle(capped(tasks[:3], tmp_path, "2"), env, NOW) == "handed_off"
    assert env.runs[0][0] == "T002" and st.lane_of(env.store, "T002") == "2"
    assert cycle.run_cycle(capped(tasks[:3], tmp_path, "3"), env, NOW + timedelta(minutes=1)) == "idle"
    assert len(env.runs) == 1 and not env.store.get("lane_strikes")  # the third waits: no strike, the lane stays free


def test_a_crate_at_its_line_limit_refuses_a_second_task(tmp_path: Path) -> None:
    tasks = graph()[:5]
    one = holding(tasks, "T001")
    pay_full = frozenset({"pay"})
    assert lanes.exclusive_ids(["T002", "T003", "T004"], one, NOW, by_id(tasks), limit(pay_full)) == ["T004"]
    assert lanes.crate_clash(tasks[1], [tasks[0]], limit(pay_full)).endswith("(at most 1 there; D-3269, D-3303)")
    assert (limit(pay_full)("pay"), limit(pay_full)("site"), lanes.crate_limit(1, None)("site"), lanes.crate_limit(3, None)("x")) == (1, 2, 1, 3)
    env = FakeEnv(handed_off)
    env.store = holding(tasks[:3], "T001")
    assert cycle.run_cycle(capped(tasks[:3], tmp_path, "2", pay_full), env, NOW) == "idle" and env.runs == []


def test_the_reservation_still_keeps_the_last_slot_for_the_blocked_top_candidate(tmp_path: Path) -> None:
    """T010 (first in line) writes `pay` and `web`; `web` is at its line limit and T020 holds it, so T010 waits.
    T002 would take `pay`'s last slot (T001 holds the other) and is refused; T004 in `site` still claims."""
    tasks = graph()
    web_full = frozenset({"web"})
    s = holding(tasks, "T001", "T020")
    order = ["T010", "T002", "T004", "T005"]
    assert lanes.exclusive_ids(order, s, NOW, by_id(tasks), limit(web_full)) == ["T004", "T005"]
    assert lanes.exclusive_ids(["T002", "T010", "T004"], s, NOW, by_id(tasks), limit(web_full)) == ["T002", "T004"]  # only the first reserves
    free_pay = holding(tasks, "T020")  # `pay` empty: T002 leaves one slot for T010, so it may claim
    assert lanes.exclusive_ids(order, free_pay, NOW, by_id(tasks), limit(web_full)) == ["T002", "T004", "T005"]
    env = FakeEnv(handed_off)
    env.store = s
    cfg = capped(tasks, tmp_path, "3", web_full, claim_first=("T010", "T002"))
    assert cycle.run_cycle(cfg, env, NOW) == "handed_off" and env.runs[0][0] == "T004"
    env.store = st.with_lease(st.with_task(env.store, "T020", status=st.DONE), "2", None)  # T020 merged, lane 2 free
    assert cycle.run_cycle(dataclasses.replace(cfg, lane_hint="4"), env, NOW + timedelta(minutes=1)) == "handed_off"
    assert env.runs[1][0] == "T010"  # the holder finished: the wide task takes `pay`'s second slot at once


def claims_on_refill(tasks: list[dict[str, Any]], tmp_path: Path, full: frozenset[str]) -> tuple[list[str], list[str]]:
    """The lanes refill names beside T001, then the tasks one run per named lane actually claims."""
    env = FakeEnv(handed_off)
    env.store = holding(tasks, "T001")
    named = cycle.refill_lanes(capped(tasks, tmp_path, "1", full), env.store, NOW)
    for n, lane in enumerate(named, start=1):
        assert cycle.run_cycle(capped(tasks, tmp_path, lane, full), env, NOW + timedelta(minutes=n)) == "handed_off"
    after = NOW + timedelta(minutes=len(named) + 1)
    assert cycle.refill_lanes(capped(tasks, tmp_path, "1", full), env.store, after) == []
    assert cycle.run_cycle(capped(tasks, tmp_path, "7", full), env, after) == "idle"  # nothing left to claim
    return named, [run[0] for run in env.runs]


def test_refill_and_claim_agree(tmp_path: Path) -> None:
    tasks = graph()[:5]
    named, claimed = claims_on_refill(tasks, tmp_path, NEVER_FULL)
    assert named == ["2", "3", "4"] and claimed == ["T002", "T004", "T005"]  # T003 would be `pay`'s third
    named, claimed = claims_on_refill(tasks, tmp_path, frozenset({"pay"}))
    assert named == ["2", "3"] and claimed == ["T004", "T005"]
    s = holding(tasks, "T001")  # each alone fits, but not all together: refill must count claim after claim
    assert lanes.exclusive_ids(["T002", "T003", "T004"], s, NOW, by_id(tasks), limit()) == ["T002", "T003", "T004"]
    assert lanes.claimable_together(["T002", "T003", "T004"], s, NOW, by_id(tasks), limit()) == ["T002", "T004"]


def test_the_cap_is_read_from_kit_runner_json_and_fails_safe_to_one(tmp_path: Path) -> None:
    f = tmp_path / "runner.json"
    assert entry.crate_cap(f) == 1  # no file
    for raw, want in (("{not json", 1), ("[]", 1), ('{"lanes": 7}', 1), ('{"crate_cap": "2"}', 1), ('{"crate_cap": true}', 1),
                      ('{"crate_cap": 0}', 1), ('{"crate_cap": -2}', 1), ('{"crate_cap": 2.0}', 1), ('{"crate_cap": 2}', 2),
                      ('{"crate_cap": 3}', 3)):
        f.write_text(raw, encoding="utf-8")
        assert entry.crate_cap(f) == want, raw


def test_a_crate_is_full_at_the_code_docs_line_limit_read_from_the_checkout(tmp_path: Path) -> None:
    most = check_code_docs.MAX_CRATE_DOC_LINES
    assert most == 120
    full = entry.crate_doc_full(tmp_path)
    for name, count, want in (("under", most - 1, False), ("at", most, True), ("over", most + 1, True)):
        (tmp_path / "app" / name).mkdir(parents=True)
        (tmp_path / "app" / name / "CLAUDE.md").write_text("line\n" * count, encoding="utf-8")
        assert full(name, 0) is want, name
    assert full("missing", 0) is True  # unreadable counts as full, the safer reading
    (tmp_path / "app" / "under" / "CLAUDE.md").write_text("line\n" * most, encoding="utf-8")
    assert full("under", 0) is True  # read at each claim, never cached


def test_a_crate_is_full_when_its_lines_and_its_tasks_in_flight_reach_the_limit(tmp_path: Path) -> None:
    """Review round 1: each task in flight adds at least a line, so 119 lines and one task in flight leave no room."""
    most = check_code_docs.MAX_CRATE_DOC_LINES
    for name, count in (("pay", most - 1), ("site", most - 2)):
        (tmp_path / "app" / name).mkdir(parents=True)
        (tmp_path / "app" / name / "CLAUDE.md").write_text("line\n" * count, encoding="utf-8")
    full = entry.crate_doc_full(tmp_path)
    assert (full("pay", 0), full("pay", 1), full("site", 1), full("site", 2)) == (False, True, False, True)
    tasks = graph()[:5]
    capped_limit = lanes.crate_limit(2, full)
    assert lanes.exclusive_ids(["T002", "T004"], st.sync_graph(st.new_state(), tasks), NOW, by_id(tasks), capped_limit) == ["T002", "T004"]
    assert lanes.exclusive_ids(["T002", "T004"], holding(tasks, "T001"), NOW, by_id(tasks), capped_limit) == ["T004"]


def git(root: Path, *args: str) -> None:
    subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True)


def test_the_line_count_is_read_from_origin_main_not_the_checked_out_branch(tmp_path: Path) -> None:
    """Review round 1: the end-of-run refill may run with a task branch checked out; claim and refill must agree."""
    most = check_code_docs.MAX_CRATE_DOC_LINES
    doc = tmp_path / "app" / "pay" / "CLAUDE.md"
    doc.parent.mkdir(parents=True)
    doc.write_text("line\n" * most, encoding="utf-8")
    full = entry.crate_doc_full(tmp_path)
    assert full("pay", 0) is True  # not a git checkout: the file in the checkout stands in
    git(tmp_path, "init", "-q")
    git(tmp_path, "-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "--allow-empty", "-m", "base")
    doc.write_text("line\n" * 10, encoding="utf-8")
    git(tmp_path, "add", "-A")
    git(tmp_path, "-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "-m", "main")
    git(tmp_path, "update-ref", "refs/remotes/origin/main", "HEAD")
    doc.write_text("line\n" * most, encoding="utf-8")  # a task branch's copy at the limit
    assert full("pay", 0) is False and entry.crate_doc_text(tmp_path, "pay") == "line\n" * 10


def test_the_repository_sets_a_cap_of_two_with_the_guardrail_and_the_entry_point_passes_it() -> None:
    raw = json.loads((ROOT / "kit" / "runner.json").read_text(encoding="utf-8"))
    assert raw.get("crate_cap") == 2 and raw.get("exclusive_crates") is True and raw.get("prefer_disjoint_crates") is True
    assert "D-3303" in raw["crate_cap_note"] and "1 merge in 5" in raw["crate_cap_note"]
    source = inspect.getsource(entry.build)
    assert 'crate_cap=crate_cap(ROOT / "kit" / "runner.json")' in source and "crate_full=crate_doc_full(ROOT)" in source


def test_the_reservation_reads_the_limit_with_the_later_task_counted(tmp_path: Path) -> None:
    """Review round 2: `pay` is at 119 lines with nothing in flight. T002 taking it would make it full (119 + 1),
    leaving no slot for the blocked T010 (its `web` is full and held by T020), so T002 is refused; `site` still claims."""
    most = check_code_docs.MAX_CRATE_DOC_LINES
    for name, count in (("pay", most - 1), ("web", most), ("site", 10)):
        (tmp_path / "app" / name).mkdir(parents=True)
        (tmp_path / "app" / name / "CLAUDE.md").write_text("line\n" * count, encoding="utf-8")
    tasks = graph()
    capped_limit = lanes.crate_limit(2, entry.crate_doc_full(tmp_path))
    s = holding(tasks, "T020")
    assert lanes.exclusive_ids(["T010", "T002", "T004"], s, NOW, by_id(tasks), capped_limit) == ["T004"]
    assert lanes.exclusive_ids(["T002", "T004"], s, NOW, by_id(tasks), capped_limit) == ["T002", "T004"]  # no reservation: it may


def write_doc(root: Path, crate: str, size: int) -> None:
    """A crate guide of `size` UTF-8 bytes in a few lines, far under the line limit."""
    doc = root / "app" / crate / "CLAUDE.md"
    doc.parent.mkdir(parents=True, exist_ok=True)
    doc.write_text("x" * (size - 1) + "\n", encoding="utf-8")


def test_a_crate_is_full_when_its_bytes_and_a_task_bullet_pass_the_byte_budget(tmp_path: Path) -> None:
    """T437 (D-3474): a guide's bytes plus 400 for the next task and 400 for each one in flight over 8,192 is full."""
    budget, room = check_code_docs.MAX_CRATE_DOC_BYTES, check_code_docs.CRATE_DOC_TASK_BYTES
    assert (budget, room) == (8_192, 400)
    write_doc(tmp_path, "small", 2_000)
    write_doc(tmp_path, "edge", budget - room)
    write_doc(tmp_path, "near", budget - room + 1)
    full = entry.crate_doc_full(tmp_path)
    assert (full("small", 0), full("edge", 0), full("near", 0)) == (False, False, True)
    assert (full("edge", 1), full("small", 14), full("small", 15)) == (True, False, True)  # 400 bytes per task held


def test_a_check_without_the_byte_constants_falls_back_to_the_line_rule(tmp_path: Path, monkeypatch: Any) -> None:
    """An older runner copy of scripts/check_code_docs.py without the byte constants leaves the line rule alone."""
    write_doc(tmp_path, "big", 20_000)
    (tmp_path / "app" / "long").mkdir(parents=True)
    (tmp_path / "app" / "long" / "CLAUDE.md").write_text("line\n" * check_code_docs.MAX_CRATE_DOC_LINES, encoding="utf-8")
    full = entry.crate_doc_full(tmp_path)
    assert full("big", 0) is True
    monkeypatch.delattr(check_code_docs, "MAX_CRATE_DOC_BYTES")
    monkeypatch.delattr(check_code_docs, "CRATE_DOC_TASK_BYTES")
    assert (full("big", 0), full("long", 0), full("missing", 0)) == (False, True, True)
