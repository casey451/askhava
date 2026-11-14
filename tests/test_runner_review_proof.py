"""The review proof `--after` asks of a hand-in (runner/review_proof.py, D-3728): rule (a), a reviewer
subagent on the reviewer model approved the exact head after its own `git rev-parse`; rule (b), a worker
subagent's transcript shows Git's commit line for each commit since the branch start (clean main merges
exempt); each rule holds the hand-in once, then the attempt fails. Real git in tmp_path, fake data only.
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from typing import Any

import pytest
import yaml

from runner import __main__ as entry, cycle, review_proof as rp, state as st
from test_runner_live import MODELS, NOW, TASK, git, ident, live_config, make_env, repo  # noqa: F401  # reason: `repo` is the shared fixture

ROOT = Path(__file__).resolve().parent.parent
BRANCH = TASK["branch"]


def commit(root: Path, path: str, text: str, subject: str) -> tuple[str, str]:
    """A commit made the way a worker makes it (plain `git commit`): its SHA and Git's printed output."""
    (root / path).parent.mkdir(parents=True, exist_ok=True)
    (root / path).write_text(text)
    git(root, "add", "-A")
    out = subprocess.run(["git", "commit", "-m", subject], cwd=root, capture_output=True, text=True, check=True).stdout
    return git(root, "rev-parse", "HEAD"), out


def tx(home: Path, agent: str, model: str, texts: tuple[str, ...] = (), results: tuple[str, ...] = (), prompt: str = "") -> str:
    """A subagent transcript (`.../subagents/agent-<id>.jsonl`): the prompt, tool results, then assistant text."""
    path = home / "proj" / "sess" / "subagents" / f"agent-{agent}.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    rows: list[dict[str, Any]] = [{"type": "user", "message": {"role": "user", "content": prompt or "do the task"}}]
    rows += [{"type": "user", "message": {"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": r}]}]}} for r in results]
    rows += [{"type": "assistant", "message": {"role": "assistant", "model": model, "content": [{"type": "text", "text": t}],
                                                "usage": {"input_tokens": 10, "output_tokens": 5}}} for t in texts]
    with path.open("a", encoding="utf-8") as out:
        out.writelines(json.dumps(r) + "\n" for r in rows)
    return str(path)


def approve(sha: str) -> str:
    return f"No findings.\nREVIEW-VERDICT: APPROVED {sha}"


@pytest.fixture()
def work(tmp_path: Path) -> dict[str, Any]:
    """A checkout with main, the task branch prepared from it (branch start) and a worker's first commit."""
    root = tmp_path / "w"
    git(tmp_path, "init", "-q", "-b", "main", str(root))
    ident(root)
    main, _ = commit(root, "README.md", "fake\n", "main")
    git(root, "checkout", "-q", "-b", BRANCH)
    c1, out1 = commit(root, "docs/k10/a.md", "one\n", "first work")
    return {"root": root, "main": main, "c1": c1, "out1": out1, "home": tmp_path / "home"}


def gitter(root: Path) -> rp.Git:
    def run(*args: str) -> str | None:
        done = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True)
        return done.stdout.strip() if done.returncode == 0 else None
    return run


def check(w: dict[str, Any], head: str, workers: list[str], reviewers: list[str], task: dict[str, Any] = TASK,
          git_fn: rp.Git | None = None, start: str | None = "main", log: list[str] | None = None) -> str:
    proof = rp.Proof(workers, reviewers, w["main"] if start == "main" else start, dict(MODELS))
    return rp.check(task, BRANCH, head, w["main"], proof, git_fn or gitter(w["root"]), [] if log is None else log)


def fixed(w: dict[str, Any]) -> tuple[str, list[str]]:
    """A first worker, a findings review of c1, and a fix commit c2 by a fix-round worker."""
    c2, out2 = commit(w["root"], "docs/k10/a.md", "two\n", "fix the findings")
    workers = [tx(w["home"], "w1", MODELS["sonnet"], ("done",), (w["out1"],)), tx(w["home"], "w2", MODELS["sonnet"], ("fixed",), (out2,))]
    tx(w["home"], "r1", MODELS["opus"], ("REVIEW-VERDICT: FINDINGS",), (w["c1"],))
    return c2, workers


def test_happy_path_worker_fix_and_a_new_reviewer_approving_the_head(work: dict[str, Any]) -> None:
    c2, workers = fixed(work)
    r1 = str(work["home"] / "proj" / "sess" / "subagents" / "agent-r1.jsonl")
    r2 = tx(work["home"], "r2", MODELS["opus"], (approve(c2),), (c2,))
    assert check(work, c2, workers, [r1, r2]) == ""


def test_drill_t249_a_fix_commit_after_the_approval_is_refused(work: dict[str, Any]) -> None:
    c2, workers = fixed(work)
    r1 = tx(work["home"], "r0", MODELS["opus"], (approve(work["c1"]),), (work["c1"],))
    log = check(work, c2, workers, [r1])
    assert log.startswith(rp.REVIEW_MISSING) and rp.rules_in(log) == {"a"}
    assert f"(a) {c2[:12]} fix the findings" in log and work["c1"][:12] not in log.split("\n", 1)[1]


REFUSED = {  # one reviewer transcript each, over head c2 after a fix commit: (assistant texts, tool results, prompt)
    "approval of an earlier SHA, head read": (lambda c1, c2: ((approve(c1),), (c1, c2), "")),
    "FINDINGS after APPROVED": (lambda c1, c2: ((approve(c2), "On second look:\nREVIEW-VERDICT: FINDINGS"), (c2,), "")),
    "verdict only in a tool result or the prompt": (lambda c1, c2: (("looked",), (c2, approve(c2)), f"end with {approve(c2)}")),
    "approval without its rev-parse result": (lambda c1, c2: ((approve(c2),), ("diff read",), "")),
}


@pytest.mark.parametrize("case", sorted(REFUSED))
def test_a_reviewer_transcript_that_does_not_prove_the_head_is_refused(work: dict[str, Any], case: str) -> None:
    c2, workers = fixed(work)
    texts, results, prompt = REFUSED[case](work["c1"], c2)
    workers.append(tx(work["home"], "w3", MODELS["sonnet"], (approve(c2),), ()))  # a verdict in a worker's text never counts
    reviewer = tx(work["home"], "r9", MODELS["opus"], texts, results, prompt=prompt)
    assert rp.rules_in(check(work, c2, workers, [reviewer])) == {"a"}


def test_drill_t423_a_commit_line_only_in_the_main_sessions_transcript_is_refused(work: dict[str, Any], tmp_path: Path) -> None:
    c2, out2 = commit(work["root"], "docs/k10/b.md", "main session\n", "main session commit")
    main_session = tmp_path / "home" / "proj" / "session.jsonl"
    main_session.parent.mkdir(parents=True, exist_ok=True)
    main_session.write_text(json.dumps({"type": "user", "message": {"role": "user", "content": [
        {"type": "tool_result", "content": out2}]}}) + "\n")
    w1 = tx(work["home"], "w1", MODELS["sonnet"], ("done",), (work["out1"],))
    r1 = tx(work["home"], "r1", MODELS["opus"], (approve(c2),), (c2,))
    log = check(work, c2, [w1, str(main_session)], [r1])
    assert rp.rules_in(log) == {"b"} and f"(b) {c2[:12]} main session commit" in log and work["c1"][:12] not in log


def test_the_reviewers_model_rule_is_review_problems(work: dict[str, Any]) -> None:
    c1 = work["c1"]
    w1 = tx(work["home"], "w1", MODELS["sonnet"], ("done",), (work["out1"],))
    sonnet = tx(work["home"], "r1", MODELS["sonnet"], (approve(c1),), (c1,))
    assert rp.rules_in(check(work, c1, [w1], [sonnet])) == {"a"}, "a non-reviewer model does not count"
    window = {**TASK, "window_model": {"model": "claude-fable-5-5", "alias": "fable"}}
    fable = tx(work["home"], "r2", "claude-fable-5-5", (approve(c1),), (c1,))
    assert check(work, c1, [w1], [fable], task=window) == "", "a model-window reviewer passes"
    opus = tx(work["home"], "r3", MODELS["opus"], (approve(c1),), (c1,))
    assert check(work, c1, [w1], [opus], task=TASK) == "", "a fallback (mark removed) reviewer on the task's own model passes"


def test_a_reviewer_path_also_given_as_a_worker_or_holding_a_commit_line_is_ignored(work: dict[str, Any]) -> None:
    c1 = work["c1"]
    both = tx(work["home"], "x1", MODELS["opus"], (approve(c1),), (c1, work["out1"]))
    assert rp.rules_in(check(work, c1, [both], [both])) == {"a"}, "a worker path is never a reviewer"
    w1 = tx(work["home"], "w1", MODELS["sonnet"], ("done",), (work["out1"],))
    assert rp.rules_in(check(work, c1, [w1], [both])) == {"a"}, "a reviewer that committed is no reviewer"


def _merge(work: dict[str, Any], edit: bool) -> str:
    root = work["root"]
    git(root, "checkout", "-q", "main")
    work["main"], _ = commit(root, "docs/other.md", "other\n", "main moves")
    git(root, "checkout", "-q", BRANCH)
    git(root, "merge", "-q", "--no-ff", "--no-commit", "main")
    if edit:
        (root / "docs/k10/a.md").write_text("slipped in\n")
        git(root, "add", "-A")
    git(root, "commit", "-qm", "merge main")
    return git(root, "rev-parse", "HEAD")


@pytest.mark.parametrize("edit", [False, True])
def test_a_clean_main_merge_passes_and_an_edited_one_needs_its_line(work: dict[str, Any], edit: bool) -> None:
    start = work["main"]
    head = _merge(work, edit)
    w1 = tx(work["home"], "w1", MODELS["sonnet"], ("done",), (work["out1"],))
    r1 = tx(work["home"], "r1", MODELS["opus"], (approve(head),), (head,))
    log = check(work, head, [w1], [r1], start=start)
    assert (log == "") is (not edit)
    assert not edit or (rp.rules_in(log) == {"b"} and f"(b) {head[:12]} merge main" in log)


def test_a_six_hex_or_ambiguous_abbrev_is_refused(work: dict[str, Any]) -> None:
    c1 = work["c1"]
    r1 = tx(work["home"], "r1", MODELS["opus"], (approve(c1),), (c1,))
    six = tx(work["home"], "w1", MODELS["sonnet"], ("done",), (f"[{BRANCH} {c1[:6]}] first work",))
    assert rp.rules_in(check(work, c1, [six], [r1])) == {"b"}
    seven = tx(work["home"], "w2", MODELS["sonnet"], ("done",), (f"[{BRANCH} {c1[:7]}] first work",))
    real = gitter(work["root"])

    def ambiguous(*args: str) -> str | None:
        return f"{c1}\n{c1[:7]}{'0' * 33}" if args[0] == "rev-parse" and args[1].startswith("--disambiguate=") else real(*args)
    assert check(work, c1, [seven], [r1]) == ""
    assert rp.rules_in(check(work, c1, [seven], [r1], git_fn=ambiguous)) == {"b"}


def test_the_runners_prepared_merge_and_earlier_attempts_are_not_judged(work: dict[str, Any]) -> None:
    """Commits up to the branch start (`base_sha`: an earlier attempt, the runner's prepared merge) are not
    judged; only the worker's commit after it needs its line."""
    head = _merge(work, edit=True)  # the runner's prepared merge, unproven, before the start
    c3, out3 = commit(work["root"], "docs/k10/c.md", "three\n", "this attempt")
    w1 = tx(work["home"], "w1", MODELS["sonnet"], ("done",), (out3,))
    r1 = tx(work["home"], "r1", MODELS["opus"], (approve(c3),), (c3,))
    assert check(work, c3, [w1], [r1], start=head) == ""


def test_no_branch_start_skips_rule_b_with_a_log_line(work: dict[str, Any], capsys: pytest.CaptureFixture[str]) -> None:
    c1, log = work["c1"], []
    r1 = tx(work["home"], "r1", MODELS["opus"], (approve(c1),), (c1,))
    assert check(work, c1, [], [r1], start=None, log=log) == ""
    assert log == [line for line in log if "review proof (b) skipped: the hand-off has no branch_start" in line] and log
    assert "runner: review proof (b) skipped" in capsys.readouterr().out
    plain = {k: v for k, v in TASK.items() if k != "reviewer_model"}
    assert rp.rules_in(check(work, work["c1"], [], [], task=plain)) == {"b"}, "a task without a reviewer skips (a) only"


# ---- --after: collect_result, the holds, the hand-off


def _claimed(repo: dict[str, Path], tmp_path: Path, monkeypatch: pytest.MonkeyPatch, start: str) -> Any:
    monkeypatch.delenv("RUNNER_WORKER_CMD", raising=False)
    monkeypatch.setattr(entry, "ROOT", tmp_path)
    (tmp_path / ".git").mkdir(exist_ok=True)
    (tmp_path / entry.HANDOFF).write_text(json.dumps({"session": "session-a", "task": "K10", "branch_start": start}))
    env = make_env(repo["a"], tmp_path)
    env.smoke_main = lambda: (True, env.main_head())
    env.enable_auto_merge = lambda b, h: (True, "fake")
    env.update_state(lambda s: st.claim(st.with_task(s, "K10"), "K10", "session-a", NOW))
    return env


def _collect(env: Any, workers: list[str], reviewers: list[str], start: str) -> Any:
    proof = rp.Proof(workers, reviewers, start, dict(MODELS))
    return lambda task: env.collect_result(task, "submitted", workers, reviewers, proof=proof)


def test_after_accepts_the_happy_path_and_records_reviewed_sha(repo: dict[str, Path], tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    root, home = repo["a"], tmp_path / "home"
    main = git(root, "rev-parse", "origin/main")
    env = _claimed(repo, tmp_path, monkeypatch, main)
    git(root, "checkout", "-q", "-b", BRANCH, "origin/main")
    _, out1 = commit(root, "docs/k10/a.md", "one\n", "work")
    head, out2 = commit(root, "docs/k10/a.md", "two\n", "fix")
    workers = [tx(home, "w1", MODELS["sonnet"], ("done",), (out1,)), tx(home, "w2", MODELS["sonnet"], ("fixed",), (out2,))]
    reviewers = [tx(home, "r1", MODELS["opus"], ("REVIEW-VERDICT: FINDINGS",), ("x",)), tx(home, "r2", MODELS["opus"], (approve(head),), (head,))]
    cfg = live_config("session-a", tmp_path, env)
    assert cycle.after_handoff(cfg, env, _collect(env, workers, reviewers, main), entry.hold_hand_in) == st.MERGE_PENDING
    assert st.entry(env.read_state(), "K10")["reviewed_sha"] == head


@pytest.mark.parametrize("rule", ["a", "b"])
def test_each_rule_holds_once_on_its_own_counter_then_fails(repo: dict[str, Path], tmp_path: Path, monkeypatch: pytest.MonkeyPatch, rule: str) -> None:
    root, home = repo["a"], tmp_path / "home"
    main = git(root, "rev-parse", "origin/main")
    env = _claimed(repo, tmp_path, monkeypatch, main)
    git(root, "checkout", "-q", "-b", BRANCH, "origin/main")
    head, out1 = commit(root, "docs/k10/a.md", "one\n", "unproven work")
    workers = [tx(home, "w1", MODELS["sonnet"], ("done",), (out1 if rule == "a" else "no line",))]
    reviewers = [tx(home, "r1", MODELS["opus"], (approve(head) if rule == "b" else "REVIEW-VERDICT: FINDINGS",), (head,))]
    cfg, before = live_config("session-a", tmp_path, env), git(repo["origin"], "rev-parse", "claude/kit-state")
    assert cycle.after_handoff(cfg, env, _collect(env, workers, reviewers, main), entry.hold_hand_in) == entry.REVIEW_HELD
    kept = json.loads((tmp_path / entry.HANDOFF).read_text())
    assert kept[f"review_holds_{rule}"] == 1 and f"review_holds_{'b' if rule == 'a' else 'a'}" not in kept
    assert git(repo["origin"], "rev-parse", "claude/kit-state") == before, "a held hand-in writes no state"
    assert cycle.after_handoff(cfg, env, _collect(env, workers, reviewers, main), entry.hold_hand_in) == st.QUEUED
    failure = st.entry(env.read_state(), "K10")["last_failure"]
    assert rp.REVIEW_MISSING in failure and f"({rule}) {head[:12]} unproven work" in failure
    assert git(repo["origin"], "ls-remote", "--heads", ".", BRANCH) == "", "nothing pushed"


def test_the_decisions_reorder_commit_comes_after_the_check(repo: dict[str, Path], tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    root, home = repo["a"], tmp_path / "home"
    main = git(root, "rev-parse", "origin/main")
    env = _claimed(repo, tmp_path, monkeypatch, main)
    git(root, "checkout", "-q", "-b", BRANCH, "origin/main")
    head, out1 = commit(root, "docs/k10/a.md", "one\n", "work")
    runner_commit = git(root, "commit-tree", f"{head}^{{tree}}", "-p", head, "-m", "runner: decisions in order")
    monkeypatch.setattr(env.updates, "in_order", lambda h, m, b: runner_commit)
    w1, r1 = tx(home, "w1", MODELS["sonnet"], ("done",), (out1,)), tx(home, "r1", MODELS["opus"], (approve(head),), (head,))
    res = _collect(env, [w1], [r1], main)(TASK)
    assert res.outcome == "submitted" and res.head_sha == runner_commit


def test_after_passes_the_proof_with_the_hand_offs_branch_start(repo: dict[str, Path], tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    env = make_env(repo["a"], tmp_path)
    seen: dict[str, Any] = {}
    env.collect_result = lambda task, *rest, **kw: seen.update(kw) or cycle.WorkerResult("tests_failed")
    monkeypatch.setattr(entry, "read_handoff", lambda: {"branch_start": "abc1234"})
    monkeypatch.setattr(cycle, "after_handoff", lambda cfg, env, res_for, hold, own_models: (res_for(TASK), hold)[1].__name__)
    args = entry.parse_args(["--after"])
    assert entry.after(args, live_config("s", tmp_path, env), env) == "hold_hand_in"
    assert seen["proof"].branch_start == "abc1234" and seen["proof"].models == MODELS
    assert '"branch_start": env.prepared.get("base_sha")' in (ROOT / "runner" / "__main__.py").read_text(encoding="utf-8")
    assert entry.REVIEW_HELD in entry.AFTER_KEEPS_HANDOFF


def test_the_workflow_states_the_verdict_line_plain_commits_and_a_new_reviewer() -> None:
    steps = yaml.safe_load((ROOT / ".github/workflows/backup-runner.yml").read_text(encoding="utf-8"))
    text = json.dumps(steps)
    for words in ("runs `git rev-parse HEAD` itself on the task branch before reading the diff",
                  "`REVIEW-VERDICT: APPROVED <the 40-hex head it read>` or `REVIEW-VERDICT: FINDINGS`",
                  "commits with plain `git commit` (no `-q`, output not filtered or redirected)",
                  "run a NEW reviewer subagent over the new head", "one `--reviewer-agent` per reviewer agent",
                  "If `--after` prints `runner: review_missing`", "ends with step 4's verdict line (D-891, D-3728)"):
        assert json.dumps(words)[1:-1] in text, words
