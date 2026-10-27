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


def test_claim_sets_a_three_hour_lease() -> None:
    s = st.claim(st.new_state(), "T001", "session-a", NOW)
    lease = st.entry(s, "T001")["lease"]
    assert st.status(s, "T001") == st.IN_PROGRESS
    assert datetime.fromisoformat(lease["expires"]) == NOW + timedelta(hours=3)


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
    assert not st.renewal_due(s, "T001", NOW + timedelta(minutes=29))
    assert st.renewal_due(s, "T001", NOW + timedelta(minutes=30))
    later = NOW + timedelta(minutes=30)
    s = st.renew(s, "T001", "a", later)
    assert datetime.fromisoformat(st.entry(s, "T001")["lease"]["expires"]) == later + timedelta(hours=3)


def test_renewal_refused_for_other_session_or_after_expiry() -> None:
    s = st.claim(st.new_state(), "T001", "a", NOW)
    with pytest.raises(st.LeaseError):
        st.renew(s, "T001", "other", NOW)
    with pytest.raises(st.LeaseError):
        st.renew(s, "T001", "a", NOW + timedelta(hours=3))
    with pytest.raises(st.LeaseError):
        st.renew(st.new_state(), "T001", "a", NOW)
    assert not st.renewal_due(st.new_state(), "T001", NOW)


def test_expired_lease_returns_task_and_keeps_evidence() -> None:
    s = st.claim(st.new_state(), "T001", "a", NOW)
    assert st.release_expired(s, NOW + timedelta(hours=2)) == s
    freed = st.release_expired(s, NOW + timedelta(hours=3, minutes=1))
    assert st.status(freed, "T001") == st.PENDING
    assert st.entry(freed, "T001")["stale_leases"][0]["session"] == "a"


def test_state_functions_do_not_mutate_their_input() -> None:
    original = st.new_state()
    st.claim(original, "T001", "a", NOW)
    assert original == st.new_state()


def test_backoff_doubles_to_a_cap() -> None:
    assert [st.backoff_minutes(n) for n in range(0, 7)] == [15, 30, 60, 120, 240, 360, 360]


def test_blocked_holds_only_its_dependents() -> None:
    graph = [task("T001"), task("T002", ["T001"]), task("T003"), task("T004", ["T003"])]
    s = st.with_task(st.new_state(), "T001", status=st.BLOCKED)
    assert st.eligible(graph, s, NOW) == ["T003"]
    s = st.with_task(s, "T003", status=st.DONE)
    assert st.eligible(graph, s, NOW) == ["T004"]


def test_retry_waits_for_its_time() -> None:
    graph = [task("T001")]
    after = (NOW + timedelta(hours=1)).isoformat()
    s = st.with_task(st.new_state(), "T001", status=st.RETRY_AFTER_RESET, retry_after=after)
    assert st.eligible(graph, s, NOW) == []
    assert st.eligible(graph, s, NOW + timedelta(hours=1)) == ["T001"]


def test_hold_and_red_main() -> None:
    graph = [task("T001")]
    held = st.new_state()
    held["build_hold"] = True
    assert st.eligible(graph, held, NOW) == []
    red = st.new_state()
    red["main_red"] = True
    assert st.eligible(graph, red, NOW) == [st.FIX_MAIN]


def test_gates_are_never_eligible_and_skipped_satisfies_deps() -> None:
    gate = task("T127", type="gate")
    graph = [gate, task("T130", ["T127"])]
    assert st.eligible(graph, st.new_state(), NOW) == []
    assert st.next_gate(graph, st.new_state()) == gate
    s = st.with_task(st.new_state(), "T127", status=st.SKIPPED)
    assert st.eligible(graph, s, NOW) == ["T130"]
    assert st.next_gate(graph, s) is None


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
    assert "claude-haiku-5-5" in verify.check_model(["claude-haiku-5-5", "claude-sonnet-5-5"], MODELS["sonnet"], known)[0]
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
    return {"id": gid, "type": "gate", "gate": {"outbox": "Your turn: set up accounts", "resume": "Casey comments 'accounts done' in the issue"}}


def test_outbox_file_format(tmp_path: Path) -> None:
    path = outbox.write_outbox(tmp_path, "K3-q1", "Question: x", "question", "body text")
    assert path.read_text(encoding="utf-8") == "---\nid: K3-q1\ntitle: Question: x\nkind: question\n---\nbody text\n"


@pytest.mark.parametrize("args", [("bad id", "t", "gate"), ("ok", "t", "nope"), ("ok", "two\nlines", "gate"), ("ok", " ", "gate")])
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
    assert st.status(s, "T127") == st.PENDING
    s = outbox.apply_owner_answer(s, gate(), "Accounts DONE, thanks", NOW)
    assert st.status(s, "T127") == st.DONE
    assert st.entry(s, "T127")["answers"] == ["working on it", "Accounts DONE, thanks"]


def test_gate_without_a_quoted_phrase_needs_an_affirmative_word() -> None:
    odd = gate()
    odd["gate"]["resume"] = "Casey reports the results"
    assert outbox.resume_phrase(odd) == ""
    assert st.status(outbox.apply_owner_answer(st.new_state(), odd, "wait, not yet", NOW), "T127") == st.PENDING
    assert st.status(outbox.apply_owner_answer(st.new_state(), odd, "Yes, all good", NOW), "T127") == st.DONE
    assert st.status(outbox.apply_owner_answer(st.new_state(), odd, "ago", NOW), "T127") == st.PENDING


def test_apostrophe_in_resume_text_is_not_a_quote() -> None:
    g = gate()
    g["gate"]["resume"] = "Casey's 'go' in the owner issue"
    assert outbox.resume_phrase(g) == "go"


def test_finished_gate_sends_and_does_not_wait(tmp_path: Path) -> None:
    g = {"id": "T143", "type": "gate", "gate": {"kind": "finished", "waits_on_owner": False, "outbox": "Finished: ready for your review"}}
    s = outbox.open_gate(st.new_state(), g, "links", tmp_path)
    assert st.status(s, "T143") == st.DONE and (tmp_path / "T143.md").exists()


def test_gate_without_title_gets_one(tmp_path: Path) -> None:
    g = {"id": "T144", "type": "gate", "gate": {"kind": "gate-b", "waits_on_owner": True, "resume": "Casey reports"}}
    outbox.open_gate(st.new_state(), g, "b", tmp_path)
    assert "title: Your turn: gate-b" in (tmp_path / "T144.md").read_text(encoding="utf-8")


def test_gate_p_holds_until_approved_to_build(tmp_path: Path) -> None:
    s = outbox.hold_for_gate_p(st.new_state(), tmp_path)
    assert s["build_hold"] is True and st.eligible([task("T001")], s, NOW) == []
    s = outbox.apply_gate_p_answer(s, "looks good")
    assert s["build_hold"] is True
    s = outbox.apply_gate_p_answer(s, "Approved to build")
    assert s["build_hold"] is False and st.status(s, "GATE-P") == st.DONE


def test_real_task_file_gates_clear_on_their_own_phrase() -> None:
    graph = json.loads((REPO / "kit" / "tasks.json").read_text(encoding="utf-8"))["tasks"]
    gates = [t for t in graph if t["type"] == "gate" and t["gate"]["waits_on_owner"]]
    assert gates
    for g in gates:
        phrase = outbox.resume_phrase(g)
        assert outbox.clears(g, phrase or "go")
        assert not outbox.clears(g, "not yet")
