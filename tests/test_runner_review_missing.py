"""`--after`'s hold for a missing review proof (runner/__main__.py `hold_review`, D-3728, D-4796): a rule (b)
commit at the branch head holds the hand-in once, and so does one below the head, with a line naming it that says to
re-name every `--worker-agent` id, else a NEW worker must redo it (D-4908, an omitted id, D-1328); and the
backup-runner workflow tells every fix-round worker to commit plainly and names the `fable` alias with its
Opus fallback (D-4767). Fake data only.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest
import yaml

from runner import __main__ as entry, cycle, review_proof as rp

ROOT = Path(__file__).resolve().parent.parent
HEAD = "a1b2c3d4e5f6" + "0" * 28
BELOW = "0f1e2d3c4b5a" + "1" * 28
SENTENCE = ("Commit with plain `git commit` in its own Bash call, never `-q`, redirected or piped, so Git's "
            "`[<branch> <abbrev>]` line is in your transcript (D-3728).")


def refusal(*shas: str) -> str:
    rows = [f"(b) {sha[:12]} Fake commit: no --worker-agent transcript holds Git's line" for sha in shas]
    return f"{rp.REVIEW_MISSING} on {HEAD[:12]} (D-3728): not proven reviewed or worker-made\n" + "\n".join(rows)


@pytest.fixture
def handoff(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    monkeypatch.setattr(entry, "ROOT", tmp_path)
    path = tmp_path / entry.HANDOFF
    path.parent.mkdir(parents=True)
    path.write_text(json.dumps({"task": "K10"}), encoding="utf-8")
    return path


def test_a_head_commit_under_rule_b_holds_once(handoff: Path) -> None:
    res = cycle.WorkerResult(outcome="submitted", log=refusal(HEAD))
    assert entry.buried_commits(res.log) == []
    assert entry.hold_review(res) == entry.REVIEW_HELD
    assert json.loads(handoff.read_text(encoding="utf-8"))["review_holds_b"] == 1
    assert entry.hold_review(res) == "", "the second refusal fails the attempt"


def test_a_commit_below_the_head_holds_once_naming_it(handoff: Path, capsys: pytest.CaptureFixture[str]) -> None:
    res = cycle.WorkerResult(outcome="submitted", log=refusal(BELOW, HEAD))
    assert entry.buried_commits(res.log) == [BELOW[:12]]
    assert entry.hold_review(res) == entry.REVIEW_HELD
    first = capsys.readouterr().out.splitlines()[0]
    assert BELOW[:12] in first and HEAD[:12] not in first
    assert "naming every --worker-agent id" in first and "a NEW worker must redo" in first
    assert json.loads(handoff.read_text(encoding="utf-8"))["review_holds_b"] == 1
    assert entry.hold_review(res) == "", "the second refusal fails the attempt"


def test_an_omitted_worker_id_holds_and_a_rerun_naming_it_passes(handoff: Path) -> None:
    branch = "claude/task/K10-fake"
    shas = {"c1": BELOW, "c2": HEAD}

    def git(*args: str) -> str | None:
        if args[0] == "rev-list":
            return f"{HEAD} {BELOW}\n{BELOW} {'f' * 40}"
        return next(s for s in shas.values() if s.startswith(args[1].split("=", 1)[1]))

    def worker(sha: str) -> rp.Transcript:
        return rp.Transcript(path=f"agent-{sha[:4]}.jsonl", models=[], texts=[],
                             results=[f"[{branch} {sha[:9]}] Fake commit"])

    omitted = rp.authorship_problems(branch, HEAD, ["e" * 40], [worker(HEAD)], git)
    assert [sha for sha, rule, _ in omitted] == [BELOW] and omitted[0][1] == "b"
    res = cycle.WorkerResult(outcome="submitted", log=refusal(BELOW))
    assert entry.hold_review(res) == entry.REVIEW_HELD, "one hold lets the session re-name every id"
    assert rp.authorship_problems(branch, HEAD, ["e" * 40], [worker(BELOW), worker(HEAD)], git) == []


def test_other_logs_name_no_buried_commit() -> None:
    assert entry.buried_commits("prehandin_red\n(b) 0f1e2d3c4b5a x") == []
    assert entry.buried_commits(f"{rp.REVIEW_MISSING} on {HEAD[:12]} (D-3728): x\n(a) {BELOW[:12]} y") == []


def _steps() -> dict[str, str]:
    text = "\n".join(str(v) for v in _strings(yaml.safe_load((ROOT / ".github/workflows/backup-runner.yml").read_text(encoding="utf-8"))))
    found: dict[str, str] = {}
    for line in text.splitlines():
        head = line.strip().split(" ", 1)[0]
        if head in ("3.", "4c.", "5a.", "5b."):
            found[head] = line
    return found


def _strings(node: object) -> list[str]:
    if isinstance(node, str):
        return [node]
    if isinstance(node, dict):
        node = list(node.values())
    return [s for item in node for s in _strings(item)] if isinstance(node, list) else []


def test_the_workflow_fix_rounds_commit_plainly_and_step_3_knows_fable() -> None:
    steps = _steps()
    for name in ("4c.", "5a.", "5b."):
        assert SENTENCE in steps[name], name
    assert "alias haiku, sonnet, opus or fable" in steps["3."]
    assert "refuses Fable" in steps["3."] and "on `opus` and add `--model-fallback" in steps["3."]
