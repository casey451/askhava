"""The review proof `--after` asks of a hand-in before it is pushed (D-3728; D-3475, D-1328, D-3444).

Over the worker's head H, from the subagent transcripts (`find_transcripts`) and git on the checkout:
(a) when the task has a reviewer, a `--reviewer-agent` transcript approved exactly H after its own
`git rev-parse HEAD`; (b) a `--worker-agent` transcript shows Git's `[<branch> <abbrev>]` line for each
commit since main and the hand-off's `branch_start` (clean main merges exempt; skipped with a log line
when there is no `branch_start`). `check` answers '' or the refusal log, which starts with REVIEW_MISSING
and holds one `(a)` or `(b)` line per failing commit; `rules_in` reads the rules back for `--after`'s
hold (runner/__main__.py). The full rule is D-3728 in docs/decisions.md.
"""

from __future__ import annotations

import json
import re
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from runner import cycle, verify

REVIEW_MISSING = "review_missing"
VERDICT = re.compile(r"^REVIEW-VERDICT: (?:APPROVED ([0-9a-f]{40})|FINDINGS)[ \t]*$", re.M)
SUBAGENT_FILE = re.compile(r"agent-[0-9a-z]+\.jsonl")
RULE_LINE = re.compile(r"^\((a|b)\) ", re.M)
MIN_ABBREV = 7

# Runs `git <args>` on the runner's checkout: its stripped stdout, or None when git exits non-zero.
Git = Callable[..., "str | None"]


@dataclass(frozen=True)
class Proof:
    """What `--after` hands `collect_result` for the check: the `--worker-agent` and `--reviewer-agent`
    transcript paths, the hand-off's `branch_start` and kit/tasks.json `models` (for `known_models`)."""

    workers: list[str] = field(default_factory=list)
    reviewers: list[str] = field(default_factory=list)
    branch_start: str | None = None
    models: dict[str, str] = field(default_factory=dict)


@dataclass(frozen=True)
class Transcript:
    """One transcript as the rules read it: its models, assistant text blocks and tool-result texts."""

    path: str
    models: list[str]
    texts: list[str]
    results: list[str]


def check(task: dict[str, Any], branch: str, head: str, main: str, proof: Proof, git: Git, log: list[str]) -> str:
    """'' when the head passes rules (a) and (b), else the refusal log (D-3728). `log` gets (b)'s skip line."""
    workers = [read(p) for p in proof.workers if is_subagent(p)]
    worker_paths = {Path(p).resolve() for p in proof.workers}
    reviewers = [read(p) for p in proof.reviewers if is_subagent(p) and Path(p).resolve() not in worker_paths]
    problems: list[tuple[str, str, str]] = []
    if cycle.reviewer_model(task):
        problems += approval_problems(task, branch, head, proof, reviewers, git)
    if proof.branch_start:
        problems += authorship_problems(branch, head, [main, proof.branch_start], workers, git)
    else:
        line = "review proof (b) skipped: the hand-off has no branch_start (prepare_branch returned {}: a dry run or a git failure)"
        print(f"runner: {line}")
        log.append(line)
    if not problems:
        return ""
    rows = [f"({rule}) {_named(sha, git)}: {why}" for sha, rule, why in problems]
    return f"{REVIEW_MISSING} on {head[:12]} (D-3728): not proven reviewed or worker-made\n" + "\n".join(rows)


def rules_in(log: str) -> set[str]:
    """The rules ('a', 'b') a refusal log from `check` names."""
    return set(RULE_LINE.findall(log)) if log.startswith(REVIEW_MISSING) else set()


def is_subagent(path: str) -> bool:
    """Whether the path is a subagent transcript, `.../subagents/agent-<id>.jsonl` (D-170 layout)."""
    found = Path(path)
    return found.parent.name == "subagents" and bool(SUBAGENT_FILE.fullmatch(found.name))


def read(path: str) -> Transcript:
    """The transcript's models (verify.models_in_transcript), its assistant text blocks in order, and the
    text of each tool result; a line that is not a JSON object is skipped."""
    lines = Path(path).read_text(encoding="utf-8").splitlines()
    texts: list[str] = []
    results: list[str] = []
    for line in lines:
        try:
            record = json.loads(line)
        except ValueError:
            continue
        message = record.get("message") if isinstance(record, dict) else None
        content = message.get("content") if isinstance(message, dict) else None
        for block in content if isinstance(content, list) else []:
            if not isinstance(block, dict):
                continue
            if block.get("type") == "text" and message.get("role") == "assistant":
                texts.append(str(block.get("text", "")))
            elif block.get("type") == "tool_result":
                results.append(_result_text(block.get("content")))
    return Transcript(path, verify.models_in_transcript(lines), texts, results)


def _result_text(content: Any) -> str:
    if isinstance(content, list):
        return "\n".join(str(part.get("text", "")) for part in content if isinstance(part, dict))
    return str(content or "")


def last_verdict(transcript: Transcript) -> str | None:
    """The last verdict line of the assistant's own text: the approved SHA, 'FINDINGS', or None for none."""
    found = [m.group(1) or "FINDINGS" for text in transcript.texts for m in VERDICT.finditer(text)]
    return found[-1] if found else None


def commit_abbrevs(branch: str, transcript: Transcript) -> list[str]:
    """The abbreviated SHAs of Git's commit summary lines `[<branch> <abbrev>]` in the tool results."""
    line = re.compile(r"\[" + re.escape(branch) + r"(?: \(root-commit\))? ([0-9a-f]{%d,40})\]" % MIN_ABBREV)
    return [m.group(1) for text in transcript.results for m in line.finditer(text)]


def approval_problems(task: dict[str, Any], branch: str, head: str, proof: Proof, reviewers: list[Transcript],
                      git: Git) -> list[tuple[str, str, str]]:
    """Rule (a): [] when an eligible reviewer approved exactly `head`, else one problem per commit since
    the last approval an eligible reviewer gave (the head alone when there is none)."""
    want, known = str(cycle.reviewer_model(task)), cycle.known_models(task, proof.models)
    eligible = [t for t in reviewers if not commit_abbrevs(branch, t) and not verify.check_model(t.models, want, known)]
    if any(last_verdict(t) == head and any(head in r for r in t.results) for t in eligible):
        return []
    approved = [v for v in (last_verdict(t) for t in eligible) if v and v != "FINDINGS"]
    since = git("rev-list", head, *[f"^{sha}" for sha in approved[-1:]]) if approved else None
    commits = since.split() if since else [head]
    why = (f"no --reviewer-agent transcript on {want} ends in `REVIEW-VERDICT: APPROVED {head}` "
           "with its own `git rev-parse HEAD` result")
    return [(sha, "a", why) for sha in commits]


def authorship_problems(branch: str, head: str, excluded: list[str], workers: list[Transcript],
                        git: Git) -> list[tuple[str, str, str]]:
    """Rule (b): one problem per commit since the branch start and main that no worker transcript shows
    being made; a clean merge of main passes without one."""
    listed = git("rev-list", "--parents", head, *[f"^{sha}" for sha in excluded if sha])
    if listed is None:
        return [(head, "b", "git could not list the commits since the branch start")]
    abbrevs = [a for t in workers for a in commit_abbrevs(branch, t)]
    problems: list[tuple[str, str, str]] = []
    for row in listed.splitlines():
        sha, *parents = row.split()
        if len(parents) >= 2 and clean_merge(sha, parents, excluded[0], git):
            continue
        if not any(sha.startswith(a) and unique(a, git) for a in abbrevs):
            problems.append((sha, "b", f"no --worker-agent transcript holds Git's `[{branch} <abbrev>]` line for it"))
    return problems


def clean_merge(sha: str, parents: list[str], main: str, git: Git) -> bool:
    """Whether the commit merges a commit on main cleanly and unedited: two parents, the second an
    ancestor of main, and its tree that of `git merge-tree --write-tree` of the parents."""
    if len(parents) != 2 or not main or git("merge-base", "--is-ancestor", parents[1], main) is None:
        return False
    merged = git("merge-tree", "--write-tree", parents[0], parents[1])
    return bool(merged) and merged.splitlines()[0] == git("rev-parse", f"{sha}^{{tree}}")


def unique(abbrev: str, git: Git) -> bool:
    """Whether the abbreviation names exactly one object (`git rev-parse --disambiguate`)."""
    found = git("rev-parse", f"--disambiguate={abbrev}")
    return found is not None and len(found.split()) == 1


def _named(sha: str, git: Git) -> str:
    return f"{sha[:12]} {git('log', '-1', '--format=%s', sha) or ''}".rstrip()
