"""`--after`'s hold for a missing review proof (runner/__main__.py `hold_review`, D-3728, D-4796): a rule (b)
commit at the branch head holds the hand-in once, one below the head fails fast with a line naming it; and the
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


def test_a_commit_below_the_head_fails_fast(handoff: Path, capsys: pytest.CaptureFixture[str]) -> None:
    res = cycle.WorkerResult(outcome="submitted", log=refusal(BELOW, HEAD))
    assert entry.buried_commits(res.log) == [BELOW[:12]]
    assert entry.hold_review(res) == ""
    out = capsys.readouterr().out
    assert BELOW[:12] in out and "a NEW worker must redo" in out and HEAD[:12] not in out.splitlines()[0]
    assert "review_holds_b" not in json.loads(handoff.read_text(encoding="utf-8")), "no hold was spent"


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
