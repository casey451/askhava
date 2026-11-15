"""Runner stage robustness (T445; D-3563 (e), D-3619, D-3795 to D-3797). Fake data only.

(1) The claim gate judges `stage_exit.gate_nightly`'s record: the first failed nightly since the last green
run, on a commit older than main's head, stands in for that green run; any other failure closes the
stages as before. The nightly `record` step writes the `last_green` and `failures_since_green` it needs.
(2) `runner/state.py` `sync_graph` moves a re-staged queued task. (3) `runner/__main__.py` `build` claims
from main's kit/tasks.json, not an older checkout's, and claims nothing when main's file is unreadable.
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import os
import re
import subprocess
import sys
import textwrap
from pathlib import Path
from typing import Any

import stage_exit as se
from runner import __main__ as entry, cycle, state as st
from test_failure_drills import MODELS, NOW, FakeEnv, make_task

ROOT = Path(__file__).resolve().parent.parent
HEAD, OLD, GREEN = "head0001", "old00001", "green001"
GRAPH = [
    {"id": "T010", "stage": "2", "type": "build"},
    {"id": "T057", "stage": "2", "type": "cleanup"},
    {"id": "T058", "stage": "3", "type": "build"},
    {"id": "T059", "stage": "3", "type": "cleanup"},
]
PROOFS = {"2": {"evidence": "fake proof run 1"}, "3": {"evidence": "fake proof run 2"}}


def failed(count: Any = 1, sha: str = OLD, last_green: Any = None) -> dict[str, Any]:
    record = {"schema": 1, "conclusion": "failure", "sha": sha, "results": {"named_mutants": "failure"}}
    record["last_green"] = {"sha": GREEN, "ran_at": "2026-11-12T08:00:00Z"} if last_green is None else last_green
    if count is not None:
        record["failures_since_green"] = count
    return record


def ancestry(pairs: set[tuple[str, str]]) -> se.Ancestor:
    return lambda older, newer: (older, newer) in pairs


OLD_UNDER_HEAD = ancestry({(OLD, HEAD)})


# ---- (1) gate_nightly


def test_a_first_failure_on_an_older_head_stands_in_for_the_last_green_run() -> None:
    assert se.gate_nightly(failed(), HEAD, OLD_UNDER_HEAD) == {"conclusion": "success", "sha": GREEN}


def test_a_failure_on_mains_head_is_returned_raw() -> None:
    raw = failed(sha=HEAD)
    assert se.gate_nightly(raw, HEAD, lambda a, b: True) is raw


def test_a_failure_git_cannot_place_under_mains_head_is_returned_raw() -> None:
    raw = failed()
    assert se.gate_nightly(raw, HEAD, ancestry(set())) is raw  # not an ancestor
    assert se.gate_nightly(raw, HEAD, lambda a, b: None) is raw  # git cannot answer
    assert se.gate_nightly(raw, "", OLD_UNDER_HEAD) is raw  # no fetched head


def test_a_failure_without_a_last_green_run_is_returned_raw() -> None:
    for green in ({}, {"ran_at": "2026-11-12T08:00:00Z"}, "green001"):
        raw = failed(last_green=green)
        assert se.gate_nightly(raw, HEAD, OLD_UNDER_HEAD) is raw, green
    raw = failed()
    del raw["last_green"]
    assert se.gate_nightly(raw, HEAD, OLD_UNDER_HEAD) is raw


def test_a_missing_counter_or_a_second_failure_in_a_row_is_returned_raw() -> None:
    for count in (None, 0, 2, 3, "1"):
        raw = failed(count=count)
        assert se.gate_nightly(raw, HEAD, OLD_UNDER_HEAD) is raw, count


def test_a_success_and_no_record_pass_unchanged() -> None:
    green = {"conclusion": "success", "sha": OLD, "last_green": {"sha": OLD}, "failures_since_green": 0}
    assert se.gate_nightly(green, HEAD, OLD_UNDER_HEAD) is green
    assert se.gate_nightly(None, HEAD, OLD_UNDER_HEAD) is None


def merged_state() -> dict[str, Any]:
    tasks = [{"id": t, "status": "DONE", "merged_sha": f"m-{t}"} for t in ("T010", "T057", "T058", "T059")]
    return {"tasks": tasks, "stage_proofs": PROOFS}


def stages_after(nightly: dict[str, Any]) -> set[str]:
    """passed_stages over gate_nightly's answer: the green run held stage 2's merges, not T059's (stage 3)."""
    pairs = {(OLD, HEAD), ("m-T010", GREEN), ("m-T057", GREEN), ("m-T058", GREEN)}
    pairs |= {(f"m-{t}", OLD) for t in ("T010", "T057", "T058", "T059")}
    is_ancestor = ancestry(pairs)
    return se.passed_stages(GRAPH, merged_state(), se.gate_nightly(nightly, HEAD, is_ancestor), {}, is_ancestor)


def test_the_substituted_record_keeps_a_stage_the_green_run_held_and_closes_one_it_did_not() -> None:
    assert stages_after(failed()) == {"2"}


def test_a_second_failure_in_a_row_or_one_on_mains_head_closes_every_stage() -> None:
    assert stages_after(failed(count=2)) == set()
    assert stages_after(failed(sha=HEAD)) == set()


# ---- (1) the nightly `record` step


def record_script() -> str:
    text = (ROOT / ".github" / "workflows" / "nightly-main.yml").read_text(encoding="utf-8")
    record = text[text.index("\n  record:\n"):]
    found = re.search(r"python3 - <<'PY'\n(.*?)\n\s*PY\n", record, re.S)
    assert found, "the record step's Python was not found"
    return textwrap.dedent(found.group(1))


def run_record(tmp_path: Path, sha: str, green: bool) -> dict[str, Any]:
    result = "success" if green else "failure"
    env = {**os.environ, "CI_RESULT": "success", "TWIN_RESULT": "success", "A11Y_RESULT": "success",
           "MUTANTS_RESULT": result, "STRIPE_IPS_RESULT": "success", "TESTED_SHA": sha,
           "STARTED_AT": "2026-11-13T07:00:00Z"}
    done = subprocess.run([sys.executable, "-I", "-c", record_script()], cwd=tmp_path, env=env,
                          capture_output=True, text=True, check=False)
    assert done.returncode == 0, done.stderr
    return json.loads((tmp_path / "kit" / "nightly.json").read_text(encoding="utf-8"))


def test_the_record_step_keeps_last_green_across_failures_and_moves_it_on_a_success(tmp_path: Path) -> None:
    first = run_record(tmp_path, "fail0001", green=False)
    assert "last_green" not in first and first["failures_since_green"] == 1  # no file before
    green = run_record(tmp_path, "pass0001", green=True)
    assert green["last_green"] == {"sha": "pass0001", "ran_at": green["ran_at"]}
    assert green["failures_since_green"] == 0
    one = run_record(tmp_path, "fail0002", green=False)
    two = run_record(tmp_path, "fail0003", green=False)
    assert one["last_green"] == two["last_green"] == green["last_green"]
    assert (one["failures_since_green"], two["failures_since_green"]) == (1, 2)
    again = run_record(tmp_path, "pass0002", green=True)
    assert again["last_green"]["sha"] == "pass0002" and again["failures_since_green"] == 0
    assert set(again) == {"schema", "conclusion", "results", "started_at", "ran_at", "sha", "last_green",
                          "failures_since_green"}


def test_an_old_record_without_the_new_keys_counts_its_failure_as_the_first(tmp_path: Path) -> None:
    (tmp_path / "kit").mkdir()
    (tmp_path / "kit" / "nightly.json").write_text('{"schema": 1, "conclusion": "success", "sha": "x"}', encoding="utf-8")
    record = run_record(tmp_path, "fail0001", green=False)
    assert "last_green" not in record and record["failures_since_green"] == 1


# ---- (2) sync_graph re-stages


def test_sync_graph_moves_a_queued_entrys_stage_and_leaves_a_done_entrys() -> None:
    before = [{"id": "T111", "stage": 2, "deps": []}, {"id": "T112", "stage": 2, "deps": []}]
    state = st.with_task(st.sync_graph(st.new_state(), before), "T112", status=st.DONE)
    after = st.sync_graph(state, [{"id": "T111", "stage": 4, "deps": []}, {"id": "T112", "stage": 4, "deps": []}])
    assert st.entry(after, "T111")["stage"] == 4
    assert st.entry(after, "T112")["stage"] == 2


# ---- (3) the claim reads main's kit/tasks.json


def kit(tasks: list[dict[str, Any]], **extra: Any) -> dict[str, Any]:
    return {"tasks": tasks, "models": dict(MODELS), "protected_paths": [".github/", "runner/"],
            "shared_paths": ["docs/decisions.md"], **extra}


class FakeGitHub:
    """The parts of runner.github_env.GitHubEnv that `build` itself calls."""

    nightly: dict[str, Any] | None = None

    def __init__(self, root: Path, graph: list[dict[str, Any]], outbox_dir: Path, **_: Any) -> None:
        self.graph = graph

    def main_head(self) -> str:
        return HEAD

    def read_nightly(self) -> dict[str, Any] | None:
        return self.nightly

    def decision_numbers(self) -> set[int] | None:
        return None


def built(tmp_path: Path, monkeypatch: Any, local: Any, main: Any) -> tuple[cycle.Config, Any]:
    """`build` over a checkout whose kit/tasks.json is `local` and a main whose file is `main` (text, a
    kit, or None for unreadable), with GitHub faked; git is asked only for main's file."""
    checkout = tmp_path / "tasks.json"
    checkout.write_text(local if isinstance(local, str) else json.dumps(local), encoding="utf-8")
    shown = main if main is None or isinstance(main, str) else json.dumps(main)
    # A backup-runner job sets RUNNER_LANE for its own lane; `build` reads it into `lane_hint`, so on a
    # lane other than lane 1 these claims would see no free lane (D-3811). The test's lane is its own.
    monkeypatch.delenv("RUNNER_LANE", raising=False)
    monkeypatch.setattr(entry, "KIT_FILE", checkout)
    monkeypatch.setattr(entry, "GitHubEnv", FakeGitHub)
    monkeypatch.setattr(entry, "git_file", lambda sha, path: shown if (sha, path) == (HEAD, "kit/tasks.json") else None)
    args = argparse.Namespace(session="s-test", after=False, renew=False, state_ref="s", lease_ref="l", dry_run=True)
    return entry.build(args, tmp_path)


def claim_config(cfg: cycle.Config, refused: bool = False) -> cycle.Config:
    """The built Config with the hooks that read the live repository switched off; the plan-derived fields
    (graph, models, protected and shared paths, and a refusal's plan-health check) stay as `build` set them."""
    off = {"stage_gate": None, "pacer": None, "on_usage": None, "decisions": None, "stage_proof": None,
           "model_window": None, "crate_full": None, "locked": None, "owner_paths": (), "lanes": 1,
           "claim_first": (), "smoke_ancestry": 0, "clock": lambda: NOW}
    return dataclasses.replace(cfg, **off, **({} if refused else {"plan_health": None}))


def test_builds_gate_judges_gate_nightlys_record(tmp_path: Path, monkeypatch: Any) -> None:
    cfg, env = built(tmp_path, monkeypatch, kit(GRAPH), kit(GRAPH))
    pairs = {(OLD, HEAD), ("m-T010", GREEN), ("m-T057", GREEN), ("m-T058", GREEN)}
    monkeypatch.setattr(se, "git_is_ancestor", lambda root: ancestry(pairs))
    assert cfg.stage_gate is not None
    env.nightly = failed()
    assert cfg.stage_gate(merged_state()) == {"2"}
    env.nightly = failed(count=2)
    assert cfg.stage_gate(merged_state()) == set()


def claims(tmp_path: Path, cfg: cycle.Config, blocked: tuple[str, ...] = ()) -> list[tuple[str, str]]:
    env = FakeEnv()
    env.store = st.sync_graph(env.store, cfg.graph)
    for tid in blocked:
        env.store = st.with_task(env.store, tid, status=st.BLOCKED)
    cycle.run_cycle(cfg, env, NOW)
    return [(tid, model) for tid, model, _ in env.runs]


def test_a_checkout_graph_missing_mains_dependency_never_claims_the_waiting_task(tmp_path: Path, monkeypatch: Any) -> None:
    local = kit([make_task("T001"), make_task("T002")])
    main = kit([make_task("T001"), make_task("T002", deps=["T001"])])
    cfg, env = built(tmp_path, monkeypatch, local, main)
    assert cfg.graph == env.graph == main["tasks"]
    assert claims(tmp_path, claim_config(cfg), blocked=("T001",)) == []
    stale, _ = built(tmp_path, monkeypatch, local, local)  # the old checkout's graph would have claimed it
    assert [c[0] for c in claims(tmp_path, claim_config(stale), blocked=("T001",))] == ["T002"]


def test_mains_paths_and_model_are_the_ones_the_claim_uses(tmp_path: Path, monkeypatch: Any) -> None:
    local = kit([make_task("T001")])
    main = kit([make_task("T001", model=MODELS["opus"])], protected_paths=[".github/", "runner/", "twin/"],
               shared_paths=["docs/decisions.md", "docs/architecture.md"])
    main["models"]["spare"] = "claude-spare-test"
    cfg, _ = built(tmp_path, monkeypatch, local, main)
    assert (cfg.protected, cfg.shared, cfg.models) == (main["protected_paths"], main["shared_paths"], main["models"])
    assert claims(tmp_path, claim_config(cfg)) == [("T001", MODELS["opus"])]


def test_the_same_file_keeps_the_checkouts_kit() -> None:
    local = json.dumps(kit([make_task("T001")]))
    asked: list[tuple[str, str]] = []
    chosen, why = entry.main_kit(local, HEAD, lambda sha, path: asked.append((sha, path)) or local)
    assert (chosen, why, asked) == (json.loads(local), None, [(HEAD, "kit/tasks.json")])


def test_an_unreadable_main_file_claims_nothing_and_says_why(tmp_path: Path, monkeypatch: Any, capsys: Any) -> None:
    local = kit([make_task("T001")])
    assert entry.main_kit(json.dumps(local), "", lambda sha, path: "{}")[1]  # no fetched head
    for text in (None, "{not json", '{"tasks": []}', "[]"):
        cfg, _ = built(tmp_path, monkeypatch, local, text)
        assert cfg.graph == local["tasks"], text
        assert "no claim this cycle" in capsys.readouterr().out, text
        assert claims(tmp_path, claim_config(cfg, refused=True)) == [], text
        assert claims(tmp_path, claim_config(cfg)) == [("T001", MODELS["sonnet"])], text  # only the refusal held it
