"""Tests for runner.state, runner.verify and runner.outbox (fake tasks only)."""

from __future__ import annotations

import json
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

import pytest

from runner import outbox, state as st, verify

NOW = datetime(2026, 11, 1, 12, 0, tzinfo=timezone.utc)
MODELS = {"haiku": "claude-haiku-5-5", "sonnet": "claude-sonnet-5-5", "opus": "claude-opus-5-5"}
REPO = Path(__file__).resolve().parent.parent


def task(tid: str, deps: list[str] | None = None, **extra: Any) -> dict[str, Any]:
    return {"id": tid, "deps": deps or [], "type": "build", "owns": [f"app/{tid}/"], "model": MODELS["sonnet"], **extra}


def eligible(graph: list[dict[str, Any]], s: st.State, now: datetime) -> list[str]:
    return st.eligible(st.sync_graph(s, graph), now)


def test_claim_sets_a_three_hour_lease() -> None:
    s = st.claim(st.new_state(), "T001", "session-a", NOW)
    assert st.status(s, "T001") == st.IN_PROGRESS
    assert s["lease"] == {"task": "T001", "session": "session-a", "expires": "2026-11-01T15:00:00Z"}


def test_one_task_in_flight() -> None:
    s = st.claim(st.new_state(), "T001", "a", NOW)
    with pytest.raises(st.LeaseError):
        st.claim(s, "T002", "b", NOW + timedelta(minutes=5))


def test_nothing_new_while_merge_pending() -> None:
    s = st.with_task(st.new_state(), "T001", status=st.MERGE_PENDING)
    with pytest.raises(st.LeaseError):
        st.claim(s, "T002", "b", NOW)


def test_cannot_claim_done_or_blocked() -> None:
    s = st.with_task(st.new_state(), "T001", status=st.BLOCKED)
    with pytest.raises(st.LeaseError):
        st.claim(s, "T001", "a", NOW)


def test_renewal_extends_and_is_due_after_thirty_minutes() -> None:
    s = st.claim(st.new_state(), "T001", "a", NOW)
    assert not st.renewal_due(s, NOW + timedelta(minutes=29))
    assert st.renewal_due(s, NOW + timedelta(minutes=30))
    later = NOW + timedelta(minutes=30)
    s = st.renew(s, "T001", "a", later)
    assert st.parse(s["lease"]["expires"]) == later + timedelta(hours=3)
    assert not st.renewal_due(s, later + timedelta(minutes=29))


def test_renewal_refused_for_other_session_or_after_expiry() -> None:
    s = st.claim(st.new_state(), "T001", "a", NOW)
    with pytest.raises(st.LeaseError):
        st.renew(s, "T001", "other", NOW)
    with pytest.raises(st.LeaseError):
        st.renew(s, "T001", "a", NOW + timedelta(hours=3))
    with pytest.raises(st.LeaseError):
        st.renew(st.new_state(), "T001", "a", NOW)
    assert not st.renewal_due(st.new_state(), NOW)


def test_expired_lease_returns_task_and_keeps_evidence() -> None:
    s = st.claim(st.new_state(), "T001", "a", NOW)
    assert st.release_expired(s, NOW + timedelta(hours=2)) == s
    freed = st.release_expired(s, NOW + timedelta(hours=3, minutes=1))
    assert st.status(freed, "T001") == st.QUEUED
    assert st.entry(freed, "T001")["stale_leases"][0]["session"] == "a"
    assert freed["lease"] is None


def test_state_functions_do_not_mutate_their_input() -> None:
    original = st.new_state()
    st.claim(original, "T001", "a", NOW)
    assert original == st.new_state()


def test_backoff_doubles_to_a_cap() -> None:
    assert [st.backoff_minutes(n) for n in range(0, 7)] == [15, 30, 60, 120, 240, 360, 360]


def test_blocked_holds_only_its_dependents() -> None:
    graph = [task("T001"), task("T002", ["T001"]), task("T003"), task("T004", ["T003"])]
    s = st.with_task(st.new_state(), "T001", status=st.BLOCKED)
    assert eligible(graph, s, NOW) == ["T003"]
    s = st.with_task(s, "T003", status=st.DONE)
    assert eligible(graph, s, NOW) == ["T004"]


def test_retry_waits_for_its_time() -> None:
    graph = [task("T001")]
    after = (NOW + timedelta(hours=1)).isoformat()
    s = st.with_task(st.new_state(), "T001", status=st.RETRY_AFTER_RESET, retry_after=after)
    assert eligible(graph, s, NOW) == []
    assert eligible(graph, s, NOW + timedelta(hours=1)) == ["T001"]


def test_hold_and_red_main() -> None:
    graph = [task("T001")]
    held = st.new_state()
    held["build_hold"] = True
    assert eligible(graph, held, NOW) == []
    red = st.new_state()
    red["main_red"] = True
    assert eligible(graph, red, NOW) == [st.FIX_MAIN]


def test_gates_are_never_eligible_and_skipped_satisfies_deps() -> None:
    gate = task("T127", type="gate")
    graph = [gate, task("T130", ["T127"])]
    assert eligible(graph, st.new_state(), NOW) == []
    opened = st.next_gate(st.sync_graph(st.new_state(), graph))
    assert opened is not None and opened["id"] == "T127"
    s = st.with_task(st.new_state(), "T127", status=st.SKIPPED)
    assert eligible(graph, s, NOW) == ["T130"]
    assert st.next_gate(st.sync_graph(s, graph)) is None


# ---- path check


def test_owned_and_shared_paths_pass() -> None:
    t = task("T010")
    assert verify.check_paths(["app/T010/a.py", "docs/decisions.md"], t, [".github/"], ["docs/decisions.md"]) == []


def test_unowned_path_fails() -> None:
    problems = verify.check_paths(["app/other/a.py"], task("T010"), [], [])
    assert problems == ["app/other/a.py: not owned by T010"]


def test_protected_path_needs_harness_even_if_owned() -> None:
    t = task("T010", owns=[".github/workflows/x.yml"])
    assert "protected" in verify.check_paths([".github/workflows/x.yml"], t, [".github/"], [])[0]
    h = dict(t, type="harness")
    assert verify.check_paths([".github/workflows/x.yml"], h, [".github/"], []) == []


def test_harness_still_limited_to_owned_paths() -> None:
    h = task("T010", type="harness", owns=["runner/"])
    assert verify.check_paths(["stand_ins/x.py"], h, ["stand_ins/", "runner/"], [])


def test_state_and_odd_paths_always_fail() -> None:
    t = task("T010", type="harness", owns=["kit/", "outbox/", "/etc/", "app/"])
    bad = ["kit/state.json", "outbox/x.md", "/etc/passwd", "app/../kit/x"]
    assert len(verify.check_paths(bad, t, [], [])) == 4


def test_exact_file_ownership_does_not_cover_siblings() -> None:
    t = task("T010", owns=["pyproject.toml"])
    assert verify.check_paths(["pyproject.toml"], t, [], []) == []
    assert verify.check_paths(["pyproject.toml.bak"], t, [], [])


# ---- model check


def line(model: str) -> str:
    return json.dumps({"type": "assistant", "message": {"model": model, "role": "assistant"}})


def test_models_read_from_transcript_not_from_claims() -> None:
    lines = [line("claude-sonnet-5-5"), "not json", json.dumps([1]), line("<synthetic>"), json.dumps({"message": 3})]
    assert verify.models_in_transcript(lines) == ["claude-sonnet-5-5"]


def test_model_check() -> None:
    known = MODELS.values()
    assert verify.check_model(["claude-sonnet-5-5"] * 3, MODELS["sonnet"], known) == []
    assert (
        "claude-haiku-5-5" in verify.check_model(["claude-haiku-5-5", "claude-sonnet-5-5"], MODELS["sonnet"], known)[0]
    )
    assert "no model evidence" in verify.check_model([], MODELS["sonnet"], known)[0]
    assert "full model ID" in verify.check_model(["sonnet"], "sonnet", known)[0]


# ---- done check


def pr(**over: Any) -> dict[str, Any]:
    checks = [{"name": n, "sha": "abc1234", "conclusion": "success"} for n in ("Ci", "Twin", "Ally")]
    return {"merged": True, "head_sha": "abc1234", "checks": checks, **over}


def test_done_needs_green_checks_on_head_and_equal_trees() -> None:
    assert verify.check_done(pr(), "tree1", "tree1") == []
    assert verify.checks_state(pr()) == "green"


def test_done_refused_when_unmerged_or_tree_differs() -> None:
    assert verify.check_done(pr(merged=False), "t", "t") == ["PR is not merged"]
    assert "tree" in verify.check_done(pr(), "t1", "t2")[0]
    assert "tree" in verify.check_done(pr(), "", "")[0]


def test_green_on_another_sha_does_not_count() -> None:
    stale = pr(checks=[{"name": n, "sha": "old0000", "conclusion": "success"} for n in ("Ci", "Twin", "Ally")])
    problems = verify.check_done(stale, "t", "t")
    assert len(problems) == 3 and verify.checks_state(stale) == "pending"


def test_one_red_check_blocks_done() -> None:
    bad = pr()
    bad["checks"][2]["conclusion"] = "failure"
    assert verify.check_done(bad, "t", "t") == ["Ally: not green on head abc1234"]
    assert verify.checks_state(bad) == "failed"
    bad["checks"][2]["conclusion"] = None
    assert verify.checks_state(bad) == "pending"


# ---- outbox and gates


def gate(gid: str = "T127") -> dict[str, Any]:
    return {
        "id": gid,
        "type": "gate",
        "gate": {"outbox": "Your turn: set up accounts", "resume": "Casey comments 'accounts done' in the issue"},
    }


def test_outbox_file_format(tmp_path: Path) -> None:
    path = outbox.write_outbox(tmp_path, "K3-q1", "Question: x", "question", "body text")
    assert path.read_text(encoding="utf-8") == "---\nid: K3-q1\ntitle: Question: x\nkind: question\n---\nbody text\n"


@pytest.mark.parametrize(
    "args", [("bad id", "t", "gate"), ("ok", "t", "nope"), ("ok", "two\nlines", "gate"), ("ok", " ", "gate")]
)
def test_outbox_rejects_bad_input(tmp_path: Path, args: tuple[str, str, str]) -> None:
    with pytest.raises(ValueError):
        outbox.write_outbox(tmp_path, args[0], args[1], args[2], "b")


def test_account_checkpoint_writes_your_turn_and_waits(tmp_path: Path) -> None:
    s = outbox.open_gate(st.new_state(), gate(), "checklist text", tmp_path)
    assert st.status(s, "T127") == st.WAITING_FOR_OWNER
    text = (tmp_path / "T127.md").read_text(encoding="utf-8")
    assert "title: Your turn: set up accounts" in text and "kind: gate" in text and "checklist text" in text


def test_gateclears_only_on_its_phrase() -> None:
    s = outbox.apply_owner_answer(st.new_state(), gate(), "working on it", NOW)
    assert st.status(s, "T127") == st.QUEUED
    s = outbox.apply_owner_answer(s, gate(), "Accounts DONE, thanks", NOW)
    assert st.status(s, "T127") == st.DONE
    assert st.entry(s, "T127")["answers"] == ["working on it", "Accounts DONE, thanks"]


def test_conditional_gate_clears_on_yes_or_no_and_records_the_answer() -> None:
    g = gate("T130")
    g["gate"].update(conditional=True, resume="Casey answers 'yes' to that exact question")
    for text, want in (("Yes", "yes"), ("no", "no"), ("No, thanks.", "no"), ("No. Do not text me.", "no"), ("No thanks, don't", "no"), ("No, don't", "no")):
        s = outbox.apply_owner_answer(st.new_state(), g, text, NOW)
        assert st.status(s, "T130") == st.DONE and st.entry(s, "T130")["answer"] == want
    for text in ("maybe", "no, wait", "not yet", "yes and no", "yes, don't wait", "yes, never mind", "no, don't wait", "no, not yet", "yes " + "please " * 12):
        s = outbox.apply_owner_answer(st.new_state(), g, text, NOW)
        assert st.status(s, "T130") == st.QUEUED and "answer" not in st.entry(s, "T130")
    assert not outbox.clears(gate("T130"), "no")  # a gate that is not conditional keeps its phrase


def test_gate_without_a_quoted_phrase_needs_an_affirmative_word() -> None:
    odd = gate()
    odd["gate"]["resume"] = "Casey reports the results"
    assert outbox.resume_phrase(odd) == ""
    assert st.status(outbox.apply_owner_answer(st.new_state(), odd, "wait, not yet", NOW), "T127") == st.QUEUED
    assert st.status(outbox.apply_owner_answer(st.new_state(), odd, "Yes, all good", NOW), "T127") == st.DONE
    assert st.status(outbox.apply_owner_answer(st.new_state(), odd, "ago", NOW), "T127") == st.QUEUED


def test_apostrophe_in_resume_text_is_not_a_quote() -> None:
    g = gate()
    g["gate"]["resume"] = "Casey's 'go' in the owner issue"
    assert outbox.resume_phrase(g) == "go"


def test_finished_gate_sends_and_does_not_wait(tmp_path: Path) -> None:
    g = {
        "id": "T143",
        "type": "gate",
        "gate": {"kind": "finished", "waits_on_owner": False, "outbox": "Finished: ready for your review"},
    }
    s = outbox.open_gate(st.new_state(), g, "links", tmp_path)
    assert st.status(s, "T143") == st.DONE and (tmp_path / "T143.md").exists()


def test_gate_without_title_gets_one(tmp_path: Path) -> None:
    g = {"id": "T144", "type": "gate", "gate": {"kind": "gate-b", "waits_on_owner": True, "resume": "Casey reports"}}
    outbox.open_gate(st.new_state(), g, "b", tmp_path)
    assert "title: Your turn: gate-b" in (tmp_path / "T144.md").read_text(encoding="utf-8")


def test_gate_p_holds_until_approved_to_build(tmp_path: Path) -> None:
    s = outbox.hold_for_gate_p(st.new_state(), tmp_path)
    assert s["build_hold"] and eligible([task("T001"), task("K9")], s, NOW) == ["K9"]
    s = outbox.apply_gate_p_answer(s, "looks good")
    assert s["build_hold"]
    s = outbox.apply_gate_p_answer(s, "Approved to build")
    assert s["build_hold"] is None and st.status(s, "GATE-P") == st.DONE


def test_real_task_file_gates_clear_on_their_own_phrase() -> None:
    graph = json.loads((REPO / "kit" / "tasks.json").read_text(encoding="utf-8"))["tasks"]
    gates = [t for t in graph if t["type"] == "gate" and t["gate"]["waits_on_owner"]]
    assert gates
    for g in gates:
        phrase = outbox.resume_phrase(g)
        assert outbox.clears(g, phrase or "go")
        assert not outbox.clears(g, "not yet")


# ---- the real kit/state.json shape (a trimmed copy with fake values)


def real_state() -> st.State:
    state: st.State = json.loads(
        (REPO / "tests" / "fixtures" / "kit_state_real_shape.json").read_text(encoding="utf-8")
    )
    return state


def test_real_shape_is_read_as_is() -> None:
    s = real_state()
    assert s["schema"] == 1 and isinstance(s["tasks"], list)
    assert st.status(s, "K8") == st.IN_PROGRESS and st.status(s, "GATE-P") == st.QUEUED
    assert st.live_lease(s, datetime(2026, 10, 27, 20, 0, tzinfo=timezone.utc)) == s["lease"]
    assert st.live_lease(s, datetime(2026, 10, 27, 21, 0, tzinfo=timezone.utc)) is None


def test_real_shape_round_trips_with_unknown_keys_kept() -> None:
    s = real_state()
    out = st.claim(st.release_expired(s, datetime(2026, 10, 27, 21, 0, tzinfo=timezone.utc)), "K9", "fake", NOW)
    assert out["notes"] == s["notes"] and out["owner_answers"] == s["owner_answers"] and out["phase"] == "kit"
    assert set(out) - set(s) <= {"current"} and out["lease"]["task"] == "K9"
    assert json.loads(json.dumps(out)) == out


def test_real_shape_gate_p_holds_t_tasks_but_not_kit_tasks() -> None:
    s = real_state()
    s = st.with_task(st.release_expired(s, datetime(2026, 10, 27, 21, 0, tzinfo=timezone.utc)), "K8", status=st.DONE)
    s = st.sync_graph(s, [task("T001")])
    assert s["build_hold"] and st.eligible(s, NOW) == ["K9"]
    done = st.with_task(s, "K9", status=st.DONE)
    assert st.eligible(done, NOW) == []


def test_negated_or_long_answers_never_clear_a_gate() -> None:
    """D-479: a go is a short, unnegated reply; a negation or an essay keeps the gate closed."""
    live = {"id": "T145", "type": "gate", "gate": {"resume": "Casey says 'go live'"}}
    for text in ("don't go live yet", "do NOT go live", "wait, go live next week", "no go live"):
        assert not outbox.clears(live, text), text
    assert outbox.clears(live, "Go live!")
    page = {"id": "T128", "type": "gate", "gate": {"resume": "reply go"}}
    assert not outbox.clears(page, "don't go yet")
    assert outbox.clears(page, "go")
    s = st.new_state()
    s = outbox.apply_gate_p_answer(s, "I have NOT approved to build yet")
    assert st.status(s, outbox.GATE_P) != st.DONE
    s = outbox.apply_gate_p_answer(s, "Approved to build.")
    assert st.status(s, outbox.GATE_P) == st.DONE
    long_reply = "approved to build " + "word " * 12
    assert st.status(outbox.apply_gate_p_answer(st.new_state(), long_reply), outbox.GATE_P) != st.DONE


def test_gate_phrase_must_be_a_whole_word() -> None:
    """D-484: 'got it', 'sounds good' and 'yesterday' never clear a 'go' or 'yes' gate."""
    go = {"id": "T128", "type": "gate", "gate": {"resume": "reply 'go'"}}
    for text in ("got it, thanks", "sounds good", "I have a question about the logo", "going to look later"):
        assert not outbox.clears(go, text), text
    assert outbox.clears(go, "Go.")
    yes = {"id": "T130", "type": "gate", "gate": {"resume": "answer 'yes'"}}
    assert not outbox.clears(yes, "yesterday I saw it")
    assert outbox.clears(yes, "yes")


def test_record_inbox_matches_a_lowercase_outbox_id(tmp_path: Path) -> None:
    """The hand-fixed `gate-p` outbox id (October 27) must still clear Gate P (D-497)."""
    from runner import cycle

    class Cfg:
        graph = [task("T001"), task("K9")]

    s = outbox.hold_for_gate_p(st.new_state(), tmp_path)
    out = cycle._record_inbox(s, Cfg(), [("24-1.md", "gate-p", "approved to build")], NOW)
    assert st.status(out, "GATE-P") == st.DONE and out["build_hold"] is None
    assert out["inbox_processed"] == ["24-1.md"] and out["owner_answers"][-1]["outbox_id"] == "gate-p"


def test_owner_review_tasks_wait_for_daytime() -> None:
    """D-511: a task whose PR needs Casey's code-owner review is claimed only 06:00-19:00 Arizona time."""
    from datetime import datetime, timezone

    from runner import cycle as cy

    paths = (".github/", "runner/", "BUILD_BRIEF.md")
    assert cy.needs_owner({"owns": [".github/workflows/twin.yml"]}, paths)
    assert cy.needs_owner({"owns": ["BUILD_BRIEF.md"]}, paths)
    assert not cy.needs_owner({"owns": ["app/catalog/src/lib.rs", "BUILD_BRIEF.md.bak"]}, paths)
    night = datetime(2026, 10, 30, 6, 0, tzinfo=timezone.utc)  # 23:00 in Arizona
    day = datetime(2026, 10, 30, 17, 0, tzinfo=timezone.utc)  # 10:00 in Arizona
    assert not cy.owner_hours(night) and cy.owner_hours(day)
    assert cy.owner_hours(datetime(2026, 10, 30, 13, 0, tzinfo=timezone.utc))  # 06:00
    assert not cy.owner_hours(datetime(2026, 10, 31, 2, 0, tzinfo=timezone.utc))  # 19:00
    graph = [{"id": "T1", "owns": [".github/workflows/x.yml"]}, {"id": "T2", "owns": ["app/x.rs"]}]
    cfg = cy.Config(graph, {}, [], [], Path("."), "s", lambda: day, owner_paths=paths)
    assert cy._by_day(cfg, ["T1", "T2"], night) == ["T2"]
    assert cy._by_day(cfg, ["T1", "T2"], day) == ["T1", "T2"]
    assert cy._by_day(cy.Config(graph, {}, [], [], Path("."), "s", lambda: day), ["T1", "T2"], night) == ["T1", "T2"]


def test_owner_paths_read_from_codeowners(tmp_path: Path) -> None:
    from runner import __main__ as entry

    f = tmp_path / "CODEOWNERS"
    f.write_text("# comment\n/.github/   @someone\n\n/runner/ @someone\n/BUILD_BRIEF.md @someone\n", encoding="utf-8")
    assert entry.owner_paths(f) == (".github/", "runner/", "BUILD_BRIEF.md")
    assert entry.owner_paths(tmp_path / "missing") == ()


def test_owner_review_setting(tmp_path: Path) -> None:
    from runner import __main__ as entry

    f = tmp_path / "runner.json"
    assert entry.owner_review(f) is True
    f.write_text('{"owner_review": false}', encoding="utf-8")
    assert entry.owner_review(f) is False


def test_sync_graph_copies_changed_deps_onto_unfinished_entries() -> None:
    """A dependency a plan change adds later reaches the state entry, so `eligible` and the lane check
    agree (D-1126: T156 gained T187 in kit/tasks.json after it was claimed and looked eligible for ever)."""
    graph = [task("T001"), task("T002"), task("T003", deps=["T001"]), task("T004", deps=["T001"])]
    s = st.sync_graph(st.new_state(), graph)
    s = st.with_task(s, "T001", status=st.DONE)
    s = st.with_task(s, "T004", status=st.DONE)
    moved = [task("T001"), task("T002", deps=["T001", "T003"]), task("T003", deps=["T002"]), task("T004", deps=["T002"])]
    out = st.sync_graph(s, moved)
    assert st.entry(out, "T002")["deps"] == ["T001", "T003"]
    assert st.entry(out, "T003")["deps"] == ["T002"]
    assert st.entry(out, "T004")["deps"] == ["T001"], "a finished entry keeps the deps it ran with"
    assert st.eligible(out, NOW) == []
    assert st.eligible(st.sync_graph(s, graph), NOW) == ["T002", "T003"], "unchanged deps change nothing"
