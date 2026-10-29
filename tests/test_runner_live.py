"""Tests for the live runner (D-486): runner/github_env.py over real local git repositories (a bare
`origin` in tmp_path, no network), the REST calls through a fake command runner, and runner/__main__.py.
Fake data only.
"""

from __future__ import annotations

import json
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

import pytest

from runner import __main__ as entry, cycle, state as st, verify
from runner.github_env import (
    GitHubEnv,
    alias_for,
    find_transcripts,
    parse_inbox_file,
    read_transcripts,
    render_status,
    run_command,
    state_diff,
)

NOW = datetime(2026, 11, 1, 12, 0, tzinfo=timezone.utc)
MODELS = {"haiku": "claude-haiku-5-5", "sonnet": "claude-sonnet-5-5", "opus": "claude-opus-5-5"}
TASK = {"id": "K10", "deps": [], "type": "kit", "owns": ["docs/k10/"], "model": MODELS["sonnet"],
        "reviewer_model": MODELS["opus"], "branch": "claude/task/K10-practice", "slug": "practice"}


def git(cwd: Path, *args: str) -> str:
    done = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, check=True)
    return done.stdout.strip()


def ident(cwd: Path) -> None:
    git(cwd, "config", "user.name", "Test Runner")
    git(cwd, "config", "user.email", "runner@example.com")


@pytest.fixture()
def repo(tmp_path: Path) -> dict[str, Path]:
    """origin (bare) with main and claude/kit-state, and two clones (two runner sessions)."""
    origin, seed = tmp_path / "origin.git", tmp_path / "seed"
    git(tmp_path, "init", "--bare", "-b", "main", str(origin))
    git(tmp_path, "clone", "-q", str(origin), str(seed))
    ident(seed)
    (seed / "README.md").write_text("fake repo\n")
    git(seed, "add", "-A")
    git(seed, "commit", "-qm", "main")
    git(seed, "push", "-q", "origin", "main")
    git(seed, "checkout", "-q", "--orphan", "claude/kit-state")
    git(seed, "rm", "-rqf", ".")
    (seed / "kit").mkdir()
    state = {"schema": 1, "lease": None, "build_hold": None, "tasks": [], "owner_answers": []}
    (seed / "kit" / "state.json").write_text(json.dumps(state))
    (seed / "PROGRESS.md").write_text("# Progress\n")
    git(seed, "add", "-A")
    git(seed, "commit", "-qm", "state")
    git(seed, "push", "-q", "origin", "claude/kit-state")
    out = {"origin": origin, "seed": seed}
    for name in ("a", "b"):
        git(tmp_path, "clone", "-q", str(origin), str(tmp_path / name))
        ident(tmp_path / name)
        out[name] = tmp_path / name
    return out


def make_env(root: Path, tmp: Path, name: str = "a", **kw: Any) -> GitHubEnv:
    outbox = tmp / f"outbox-{name}"
    outbox.mkdir(exist_ok=True)
    return GitHubEnv(root, [TASK], outbox, repo="fake-owner/fake-repo", starter="routine", runner_sha="feedc0de" * 5,
                     clock=lambda: NOW, **kw)


def origin_file(repo: dict[str, Path], ref: str, path: str) -> str:
    return git(repo["origin"], "show", f"{ref}:{path}")


def push_side_file(repo: dict[str, Path], path: str, text: str) -> None:
    """What notify/inbox/nightly do: a commit to claude/kit-state that leaves kit/state.json alone."""
    seed = repo["seed"]
    git(seed, "pull", "-q", "origin", "claude/kit-state")
    target = seed / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text)
    git(seed, "add", "-A")
    git(seed, "commit", "-qm", "side file")
    git(seed, "push", "-q", "origin", "HEAD:claude/kit-state")


# ---- state and lease on their own refs


def test_push_writes_state_lease_ref_status_progress_and_stamps(repo: dict[str, Path], tmp_path: Path) -> None:
    env = make_env(repo["a"], tmp_path)
    base = env.read_state()
    (env.outbox_dir / "T127.md").write_text("---\nid: T127\ntitle: t\nkind: gate\n---\nbody\n")
    claimed = st.claim(st.with_task(base, "K10"), "K10", "session-a", NOW)
    assert env.push_state(claimed, base) is True
    stored = json.loads(origin_file(repo, "claude/kit-state", "kit/state.json"))
    assert stored["runner_sha"] == "feedc0de" * 5 and stored["starter"] == "routine"
    assert stored["lease"] is None and stored["lease_on"] == "claude/lease"
    lease = json.loads(origin_file(repo, "claude/lease", "lease.json"))
    assert lease["lease"]["task"] == "K10" and lease["seq"] == 1
    assert "IN_PROGRESS" in origin_file(repo, "claude/kit-state", "PROGRESS.md")
    assert "## Running" in origin_file(repo, "claude/kit-state", "STATUS.md")
    assert "kind: gate" in origin_file(repo, "claude/kit-state", "outbox/T127.md")
    assert list(env.outbox_dir.iterdir()) == []
    log = git(repo["origin"], "log", "-1", "--format=%B", "claude/kit-state")
    assert "Runner-Sha: " + "feedc0de" * 5 in log and "Runner-Starter: routine" in log
    assert make_env(repo["b"], tmp_path, "b").read_state()["lease"]["session"] == "session-a"


def test_side_file_push_is_retried_not_a_lost_race(repo: dict[str, Path], tmp_path: Path) -> None:
    """K0 review N2: an inbox/outbox/nightly push in between is rebased over, never a lost lease."""
    env = make_env(repo["a"], tmp_path)
    base = env.read_state()
    push_side_file(repo, "inbox/6-1.md", "---\noutbox_id: \"T127\"\n---\naccounts done\n")
    assert env.push_state(st.with_task(base, "K10"), base) is True
    assert "accounts done" in origin_file(repo, "claude/kit-state", "inbox/6-1.md")
    assert any(t["id"] == "K10" for t in json.loads(origin_file(repo, "claude/kit-state", "kit/state.json"))["tasks"])


def test_lease_moved_by_another_runner_is_a_lost_race(repo: dict[str, Path], tmp_path: Path) -> None:
    first, second = make_env(repo["a"], tmp_path), make_env(repo["b"], tmp_path, "b")
    base_a, base_b = first.read_state(), second.read_state()
    assert first.push_state(st.claim(st.with_task(base_a, "K10"), "K10", "a", NOW), base_a) is True
    assert second.push_state(st.claim(st.with_task(base_b, "K10"), "K10", "b", NOW), base_b) is False
    assert json.loads(origin_file(repo, "claude/lease", "lease.json"))["lease"]["session"] == "a"


def test_state_file_changed_without_the_lease_is_a_lost_race(repo: dict[str, Path], tmp_path: Path) -> None:
    env = make_env(repo["a"], tmp_path)
    base = env.read_state()
    push_side_file(repo, "kit/state.json", json.dumps({"schema": 1, "lease": None, "tasks": [{"id": "X1"}]}))
    assert env.push_state(st.with_task(base, "K10"), base) is False


def test_inbox_read_and_acked_into_done(repo: dict[str, Path], tmp_path: Path) -> None:
    push_side_file(repo, "inbox/7-9.md", "---\nissue: 7\noutbox_id: \"T127\"\nevent: comment\n---\naccounts done\n")
    env = make_env(repo["a"], tmp_path)
    env.read_state()
    assert env.read_inbox() == [("7-9.md", "T127", "accounts done")]
    env.ack_inbox(["7-9.md"])
    names = git(repo["origin"], "ls-tree", "-r", "--name-only", "claude/kit-state")
    assert "inbox/done/7-9.md" in names and "inbox/7-9.md" not in names.splitlines()


def test_owner_answers_are_appended_to_the_jsonl(repo: dict[str, Path], tmp_path: Path) -> None:
    env = make_env(repo["a"], tmp_path)
    base = env.read_state()
    new = dict(base, owner_answers=[{"file": "7-9.md", "outbox_id": "T127", "note": "accounts done"}])
    assert env.push_state(new, base)
    line = json.loads(origin_file(repo, "claude/kit-state", "kit/owner_answers.jsonl").splitlines()[0])
    assert line["note"] == "accounts done"


def test_dry_run_pushes_nothing(repo: dict[str, Path], tmp_path: Path) -> None:
    env = make_env(repo["a"], tmp_path, dry_run=True)
    base = env.read_state()
    before = git(repo["origin"], "rev-parse", "claude/kit-state")
    assert env.push_state(st.with_task(base, "K10"), base) is True
    assert git(repo["origin"], "rev-parse", "claude/kit-state") == before and env.log
    assert env.smoke_main()[0] is True and env.run_worker(TASK, "m", "", lambda: None).outcome == "handed_off"


# ---- the whole hand-off over real git: claim, lease_live for a second session, --after, review gate


def worker_commit(root: Path, branch: str, path: str) -> str:
    git(root, "fetch", "-q", "origin", "main")
    git(root, "checkout", "-q", "-B", branch, "origin/main")
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text("practice\n")
    git(root, "add", "-A")
    git(root, "commit", "-qm", "work")
    return git(root, "rev-parse", "HEAD")


def transcript(tmp: Path, name: str, model: str) -> str:
    path = tmp / f"{name}.jsonl"
    rec = {"message": {"model": model, "usage": {"input_tokens": 10, "output_tokens": 5}}}
    path.write_text(json.dumps(rec) + "\n" + json.dumps({"message": {"model": "<synthetic>"}}) + "\n")
    return str(path)


def live_config(session: str, tmp: Path, env: GitHubEnv) -> cycle.Config:
    return cycle.Config([TASK], MODELS, [".github/", "runner/"], ["docs/decisions.md"], env.outbox_dir, session, lambda: NOW)


def test_rehearsal_handoff_lease_live_after_and_review_gate(repo: dict[str, Path], tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Acceptance (D-486): a session holding a live lease makes another exit with lease_live from code;
    --after records reviewed_sha under the lease before the branch push; auto-merge only for that head."""
    monkeypatch.delenv("RUNNER_WORKER_CMD", raising=False)
    env_a = make_env(repo["a"], tmp_path)
    env_a.smoke_main = lambda: (True, env_a.main_head())  # type: ignore[method-assign]  # reason: no smoke.sh in the fake repo
    merges: list[tuple[str, str]] = []
    env_a.enable_auto_merge = lambda b, h: merges.append((b, h)) or (True, "fake")  # type: ignore[method-assign]  # reason: no GitHub API offline
    assert cycle.run_cycle(live_config("session-a", tmp_path, env_a), env_a, NOW) == "handed_off"
    env_b = make_env(repo["b"], tmp_path, "b")
    assert cycle.run_cycle(live_config("session-b", tmp_path, env_b), env_b, NOW + timedelta(minutes=5)) == "lease_live"
    head = worker_commit(repo["a"], TASK["branch"], "docs/k10/note.md")
    work, review = transcript(tmp_path, "w", MODELS["sonnet"]), transcript(tmp_path, "r", MODELS["opus"])
    cfg = live_config("session-a", tmp_path, env_a)
    assert cycle.after_handoff(cfg, env_a, lambda t: env_a.collect_result(t, "submitted", [work], [review])) == st.MERGE_PENDING
    assert git(repo["origin"], "rev-parse", TASK["branch"]) == head
    state = json.loads(origin_file(repo, "claude/kit-state", "kit/state.json"))
    task = st.entry(state, "K10")
    assert task["reviewed_sha"] == head and task["review_lease"]["session"] == "session-a"
    assert task["history"][-1]["path_check_ok"] is True and task["history"][-1]["model_evidence_count"] == 1
    assert state["usage"]["by_task"]["K10"]["output_tokens"] == 10  # worker 5 + reviewer 5 (pacing counts both)
    lease = json.loads(origin_file(repo, "claude/lease", "lease.json"))["lease"]
    assert verify.auto_merge_allowed(state, TASK["branch"], head, lease)
    assert merges == [(TASK["branch"], head)] and task["auto_merge"]["enabled"] is True
    names = git(repo["origin"], "ls-tree", "-r", "--name-only", "claude/kit-state").splitlines()
    assert "README.md" in names and "kit/state.json" in names  # main was merged into claude/kit-state
    assert not verify.auto_merge_allowed(state, TASK["branch"], "0" * 40, lease)


def test_after_from_a_session_without_the_lease_is_refused(repo: dict[str, Path], tmp_path: Path) -> None:
    env = make_env(repo["a"], tmp_path)
    cfg = live_config("nobody", tmp_path, env)
    assert cycle.after_handoff(cfg, env, lambda t: cycle.WorkerResult("submitted")) == "not_lease_holder"


def test_worker_command_contract(repo: dict[str, Path], tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """RUNNER_WORKER_CMD gets the task, model alias and result path; the runner reads diff and transcripts itself."""
    script = tmp_path / "worker.py"
    work = transcript(tmp_path, "w2", MODELS["sonnet"])
    script.write_text(
        "import json, os, subprocess\n"
        "task = json.load(open(os.environ['RUNNER_TASK_FILE']))\n"
        "assert os.environ['RUNNER_MODEL_ALIAS'] == 'sonnet'\n"
        "b = os.environ['RUNNER_BRANCH']\n"
        "subprocess.run(['git', 'checkout', '-q', '-B', b, 'origin/main'], check=True)\n"
        "os.makedirs('docs/k10', exist_ok=True); open('docs/k10/a.md', 'w').write('x')\n"
        "subprocess.run(['git', 'add', '-A'], check=True); subprocess.run(['git', 'commit', '-qm', 'w'], check=True)\n"
        f"json.dump({{'outcome': 'submitted', 'transcripts': [{work!r}], 'claims': 'ignored'}}, open(os.environ['RUNNER_RESULT_FILE'], 'w'))\n"
    )
    git(repo["a"], "fetch", "-q", "origin", "main")
    monkeypatch.setenv("RUNNER_WORKER_CMD", f"{sys.executable} {script}")
    env = make_env(repo["a"], tmp_path)
    env.read_state()
    res = env.run_worker(TASK, MODELS["sonnet"], "", lambda: None)
    assert res.outcome == "submitted" and res.changed_paths == ["docs/k10/a.md"]
    assert res.model_evidence == [MODELS["sonnet"]] and res.branch == TASK["branch"] and len(res.head_sha) == 40


def test_worker_command_without_result_fails_the_attempt(repo: dict[str, Path], tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("RUNNER_WORKER_CMD", f"{sys.executable} -c pass")
    env = make_env(repo["a"], tmp_path)
    res = env.run_worker(TASK, MODELS["sonnet"], "", lambda: None)
    assert res.outcome == "tests_failed" and "without a result file" in res.log


# ---- REST through a fake command runner


class FakeRunner:
    def __init__(self, replies: dict[str, Any]) -> None:
        self.replies, self.calls = replies, []  # type: list[list[str]]

    def __call__(self, args: list[str], stdin: str | None, cwd: Path) -> tuple[int, str, str]:
        self.calls.append(args)
        if args[0] == "gh":
            path = args[-1]
            for key, value in self.replies.items():
                if path.startswith(key):
                    return 0, json.dumps(value), ""
            return 1, "", "not found"
        if args[:2] == ["git", "rev-parse"]:
            return 0, "tree-" + args[2].split("^")[0][:4], ""
        if args[:2] == ["git", "diff-tree"]:
            return 0, "app/x.rs\napp/y.rs\n", ""
        return 0, "", ""


def test_pr_for_uses_rest_and_reads_trees_and_files(tmp_path: Path) -> None:
    pulls = [{"number": 4, "head": {"sha": "aaaa1111"}, "merged_at": None},
             {"number": 9, "head": {"sha": "bbbb2222"}, "merged_at": "2026-11-01T10:00:00Z", "merge_commit_sha": "cccc3333"}]
    runs = {"check_runs": [{"name": n, "head_sha": "bbbb2222", "conclusion": "success"} for n in ("Ci", "Twin", "Ally")]}
    fake = FakeRunner({"repos/fake-owner/fake-repo/pulls?head=fake-owner:claude/task/K10-practice": pulls,
                       "repos/fake-owner/fake-repo/commits/bbbb2222/check-runs": runs})
    env = GitHubEnv(tmp_path, [TASK], tmp_path, repo="fake-owner/fake-repo", starter="actions", runner_sha="x", run=fake)
    pr = env.pr_for("K10")
    assert pr is not None and pr["number"] == 9 and pr["merged"] and pr["merged_sha"] == "cccc3333"
    assert verify.checks_state(pr) == "green" and pr["files"] == ["app/x.rs", "app/y.rs"]
    assert ["git", "diff-tree", "--no-commit-id", "--name-only", "-r", "--no-renames", "cccc3333^", "cccc3333"] in fake.calls
    assert not any(a[:2] == ["gh", "api"] and "graphql" in a for a in fake.calls)
    assert any("refs/pull/9/head" in " ".join(a) for a in fake.calls)


def test_pr_for_without_a_pr_is_none(tmp_path: Path) -> None:
    env = GitHubEnv(tmp_path, [TASK], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=FakeRunner({"repos/o/r/pulls": []}))
    assert env.pr_for("K10") is None


# ---- small pieces


def test_parse_inbox_file_reads_the_outbox_id_and_body() -> None:
    text = "---\nissue: 6\noutbox_id: \"GATE-P\"\nevent: comment\n---\napproved to build\n"
    assert parse_inbox_file(text) == ("GATE-P", "approved to build")
    assert parse_inbox_file("---\noutbox_id: null\n---\n") == ("", "")


def test_render_status_puts_progress_first_and_lists_what_needs_attention() -> None:
    state = st.with_task(st.new_state(), "T010", status=st.BLOCKED, reason="2 failed", stage="2")
    state["main_red"] = {"check": "nightly", "sha": "abc"}
    state["deviations"] = [{"id": "DV-1", "rule": "BUILD_BRIEF.md:69", "decision": "D-999", "expiry": "2026-12-01", "owner": "Claude"}]
    text = render_status(state, "## Progress\n3 of 400 requirements verified", NOW)
    assert text.index("## Progress") < text.index("## Tasks done")
    assert "MAIN IS RED: nightly" in text and "BLOCKED T010: 2 failed" in text and "DV-1" in text


def test_expired_deviations_are_not_open() -> None:
    state = {"deviations": [{"id": "a", "expiry": "2026-10-01"}, {"id": "b", "expiry": None}]}
    assert [d["id"] for d in st.open_deviations(state, NOW)] == ["b"]


def test_state_diff_lists_moves_and_keys() -> None:
    before = st.with_task(st.new_state(), "T001")
    after = dict(st.with_task(before, "T001", status=st.DONE), main_red=None, build_hold="x")
    assert state_diff(before, after) == {"tasks": {"T001": "QUEUED -> DONE"}, "keys": {"build_hold": "x"}}


def test_transcripts_found_by_agent_id_and_read_for_models_and_tokens(tmp_path: Path) -> None:
    folder = tmp_path / ".claude" / "projects" / "-repo" / "sess-1" / "subagents"
    folder.mkdir(parents=True)
    rec = {"message": {"model": MODELS["haiku"], "usage": {"input_tokens": 3, "output_tokens": 4, "cache_read_input_tokens": 5}}}
    (folder / "agent-abc.jsonl").write_text(json.dumps(rec) + "\nnot json\n")
    paths = find_transcripts(["abc", "missing"], home=tmp_path)
    assert len(paths) == 1
    models, usage = read_transcripts(paths)
    assert models == [MODELS["haiku"]] and usage["cache_read_input_tokens"] == 5 and usage["output_tokens"] == 4


def test_alias_for_full_model_ids() -> None:
    assert [alias_for(m) for m in MODELS.values()] == ["haiku", "sonnet", "opus"]


def test_run_command_returns_code_and_output(tmp_path: Path) -> None:
    assert run_command([sys.executable, "-c", "print('hi')"], None, tmp_path) == (0, "hi\n", "")


def test_entry_point_flags_starter_and_session(monkeypatch: pytest.MonkeyPatch) -> None:
    args = entry.parse_args(["--after", "--worker-agent", "w1", "--reviewer-agent", "r1", "--dry-run", "--state-ref", "x"])
    assert args.after and args.dry_run and args.state_ref == "x" and args.worker_agent == ["w1"]
    monkeypatch.setenv("RUNNER_STARTER", "actions")
    assert entry.starter_name() == "actions"
    monkeypatch.setenv("RUNNER_STARTER", "something-else")
    assert entry.starter_name() == "manual"
    monkeypatch.setenv("GITHUB_RUN_ID", "1234")
    monkeypatch.delenv("RUNNER_SESSION", raising=False)
    assert entry.session_id(entry.parse_args([]), "actions") == "actions-1234"
    assert entry.session_id(entry.parse_args(["--session", "s9"]), "actions") == "s9"


# ---- main merged into claude/kit-state, auto-merge by the runner, pacing lines


def test_merge_main_keeps_runner_files_and_makes_a_two_parent_merge(repo: dict[str, Path], tmp_path: Path) -> None:
    env = make_env(repo["a"], tmp_path)
    assert env.merge_main() == "merged"
    head = git(repo["origin"], "rev-parse", "claude/kit-state")
    parents = git(repo["origin"], "rev-list", "--parents", "-n", "1", head).split()
    assert len(parents) == 3 and parents[2] == git(repo["origin"], "rev-parse", "main")
    assert json.loads(origin_file(repo, "claude/kit-state", "kit/state.json"))["schema"] == 1
    assert origin_file(repo, "claude/kit-state", "README.md") == "fake repo"
    assert env.merge_main() == "up to date"
    seed = repo["seed"]
    git(seed, "checkout", "-q", "main")
    git(seed, "pull", "-q", "origin", "main")
    (seed / "README.md").write_text("fake repo v2\n")
    git(seed, "commit", "-qam", "main moves")
    git(seed, "push", "-q", "origin", "main")
    env.write_cycle_log({"result": "idle", "log": []})
    (seed / "README.md").write_text("fake repo v3\n")
    git(seed, "commit", "-qam", "main moves again")
    git(seed, "push", "-q", "origin", "main")
    assert env.merge_main() == "merged" and origin_file(repo, "claude/kit-state", "README.md") == "fake repo v3"
    assert origin_file(repo, "claude/kit-state", "PROGRESS.md") == "# Progress"
    assert json.loads(origin_file(repo, "claude/kit-state", "kit/runner_log.jsonl"))["result"] == "idle"  # D-499


class AutoMergeRunner(FakeRunner):
    def __init__(self, open_prs: list[Any], ccr_ok: bool, gh_ok: bool = True) -> None:
        super().__init__({})
        self.open_prs, self.ccr_ok, self.gh_ok = open_prs, ccr_ok, gh_ok

    def __call__(self, args: list[str], stdin: str | None, cwd: Path) -> tuple[int, str, str]:
        self.calls.append(args)
        if args[:3] == ["gh", "api", "-X"]:
            return (0, "{}", "") if self.ccr_ok else (1, "", "404")
        if args[:3] == ["gh", "pr", "merge"]:
            return (0, "", "") if self.gh_ok else (1, "", "not allowed")
        if args[:2] == ["gh", "api"] and args[2].endswith("/pulls"):
            self.open_prs = [{"number": 31, "head": {"sha": "h1"}}]
            return 0, json.dumps(self.open_prs[0]), ""
        if args[0] == "gh":
            return 0, json.dumps(self.open_prs), ""
        return 0, "", ""


def am_env(tmp_path: Path, fake: AutoMergeRunner) -> GitHubEnv:
    return GitHubEnv(tmp_path, [TASK], tmp_path, repo="o/r", starter="routine", runner_sha="x", run=fake)


def test_auto_merge_tries_the_ccr_route_then_gh_pr_merge(tmp_path: Path) -> None:
    ccr = AutoMergeRunner([{"number": 30, "head": {"sha": "h1"}}], ccr_ok=True)
    assert am_env(tmp_path, ccr).enable_auto_merge("claude/task/K10-practice", "h1") == (True, "auto-merge on #30 (ccr route)")
    assert ["gh", "api", "-X", "PUT", "repos/o/r/pulls/30/ccr/auto_merge", "-f", "merge_method=squash"] in ccr.calls
    fallback = AutoMergeRunner([{"number": 30, "head": {"sha": "h1"}}], ccr_ok=False)
    assert am_env(tmp_path, fallback).enable_auto_merge("claude/task/K10-practice", "h1")[0] is True
    assert ["gh", "pr", "merge", "--auto", "--squash", "30", "--repo", "o/r"] in fallback.calls
    neither = AutoMergeRunner([{"number": 30, "head": {"sha": "h1"}}], ccr_ok=False, gh_ok=False)
    assert am_env(tmp_path, neither).enable_auto_merge("claude/task/K10-practice", "h1") == (False, "auto-merge refused: not allowed")


def test_auto_merge_opens_the_pr_when_missing_and_refuses_another_head(tmp_path: Path) -> None:
    opener = AutoMergeRunner([], ccr_ok=True)
    assert am_env(tmp_path, opener).enable_auto_merge("claude/task/K10-practice", "h1") == (True, "auto-merge on #31 (ccr route)")
    assert any(a[:3] == ["gh", "api", "repos/o/r/pulls"] for a in opener.calls)
    moved = AutoMergeRunner([{"number": 30, "head": {"sha": "other"}}], ccr_ok=True)
    ok, why = am_env(tmp_path, moved).enable_auto_merge("claude/task/K10-practice", "h1")
    assert not ok and "not the reviewed head" in why and not any(a[:3] == ["gh", "api", "-X"] for a in moved.calls)


def test_status_and_progress_carry_the_pacing_line(repo: dict[str, Path], tmp_path: Path) -> None:
    import pacing

    env = make_env(repo["a"], tmp_path, pacing_line=pacing.status_line)
    base = env.read_state()
    stand = {"five_hour": {"used": 100, "budget": 900, "left": 800, "resets_at": "2026-11-01T17:00:00Z"},
             "week": {"used": 100, "budget": 9000, "left": 8900, "resets_at": None}}
    new = dict(base, pacing={"last": "K10 estimated 180k tokens; claimed", "standing": stand, "projected_finish": "2026-12-01T00:00:00Z"})
    assert env.push_state(new, base)
    assert "K10 estimated 180k tokens; claimed" in origin_file(repo, "claude/kit-state", "PROGRESS.md")
    status = origin_file(repo, "claude/kit-state", "STATUS.md")
    assert "## Pacing" in status and "projected finish 2026-12-01T00:00:00Z" in status and "resets 2026-11-01T17:00:00Z" in status


def test_cycle_log_is_appended_on_the_state_branch(repo: dict[str, Path], tmp_path: Path) -> None:
    """D-499: every cycle leaves its report line in kit/runner_log.jsonl so an unattended run can be read."""
    env = make_env(repo["a"], tmp_path)
    env.read_state()
    env.write_cycle_log({"result": "idle", "log": ["smoke on abc: exit 0"]})
    env.write_cycle_log({"result": "smoke_env", "log": []})
    lines = origin_file(repo, "claude/kit-state", "kit/runner_log.jsonl").splitlines()
    assert [json.loads(line)["result"] for line in lines] == ["idle", "smoke_env"] and "at" in json.loads(lines[0])


def test_fix_main_handoff_carries_its_goal_as_the_prompt() -> None:
    """D-501: FIX-MAIN has no prompt file, so the handoff hands the session the goal text itself."""
    from runner import __main__ as entry

    fix = {"goal": "Fix main. First option: git revert"}
    assert entry._prompt_for(fix, "FIX-MAIN") == fix["goal"]
    assert entry._prompt_for({"goal": "x"}, "T002") == "kit/prompts/T002.md"
    assert entry._prompt_for({}, "T999") == "kit/prompts/T999.md"


def test_check_run_reads_use_the_job_token(monkeypatch: pytest.MonkeyPatch) -> None:
    """D-502: only check-run reads swap in RUNNER_CHECKS_TOKEN, and only when it is set."""
    from runner import github_env

    path = ["gh", "api", "-H", "Accept: application/vnd.github+json", "repos/o/r/commits/abc/check-runs?per_page=100"]
    monkeypatch.delenv("RUNNER_CHECKS_TOKEN", raising=False)
    assert github_env._env_for(path) is None
    monkeypatch.setenv("RUNNER_CHECKS_TOKEN", "fake-job-token")
    env = github_env._env_for(path)
    assert env is not None and env["GH_TOKEN"] == "fake-job-token"
    assert github_env._env_for(["gh", "api", "repos/o/r/pulls"]) is None
    assert github_env._env_for(["git", "push"]) is None


def test_task_branch_push_uses_the_push_token(monkeypatch: pytest.MonkeyPatch) -> None:
    """D-509: with RUNNER_PUSH_TOKEN set, a push to the explicit GitHub URL gets a credential helper
    that reads the token from the environment; nothing else changes, and the token is never in argv."""
    from runner import github_env

    push = ["git", "push", "--porcelain", "https://github.com/o/r.git", "abc:refs/heads/claude/task/T1-x"]
    monkeypatch.delenv("RUNNER_PUSH_TOKEN", raising=False)
    assert github_env._env_for(push) is None
    monkeypatch.setenv("RUNNER_PUSH_TOKEN", "fake-push-token")
    env = github_env._env_for(push)
    assert env is not None and env["GIT_CONFIG_COUNT"] == "3"
    assert env["GIT_CONFIG_VALUE_0"] == "" and env["GIT_CONFIG_VALUE_1"] == ""
    assert "RUNNER_PUSH_TOKEN" in env["GIT_CONFIG_VALUE_2"] and "fake-push-token" not in env["GIT_CONFIG_VALUE_2"]
    assert github_env._env_for(["git", "push", "--porcelain", "origin", "abc:refs/heads/claude/kit-state"]) is None
    assert github_env._env_for(["git", "fetch", "https://github.com/o/r.git"]) is None


# ---- D-515: the lane map in lease.json, and update_state over real git


def push_lease_file(repo: dict[str, Path], text: str) -> None:
    """Write lease.json on claude/lease the way a runner from before D-515 would."""
    seed = repo["seed"]
    git(seed, "fetch", "-q", "origin", "claude/lease")
    git(seed, "checkout", "-q", "-B", "lease-edit", "FETCH_HEAD")
    (seed / "lease.json").write_text(text)
    git(seed, "commit", "-qam", "old lease file")
    git(seed, "push", "-q", "origin", "HEAD:claude/lease")
    git(seed, "checkout", "-q", "claude/kit-state")


def claimed_on_origin(repo: dict[str, Path], tmp_path: Path) -> GitHubEnv:
    env = make_env(repo["a"], tmp_path)
    base = env.read_state()
    assert env.push_state(st.claim(st.with_task(base, "K10"), "K10", "session-a", NOW), base)
    return env


def test_lease_file_carries_the_lane_map_and_an_old_file_still_reads(repo: dict[str, Path], tmp_path: Path) -> None:
    assert make_env(repo["a"], tmp_path).read_state()["leases"] == {"1": None}
    claimed_on_origin(repo, tmp_path)
    doc = json.loads(origin_file(repo, "claude/lease", "lease.json"))
    assert doc["leases"] == {"1": dict(doc["lease"], lane="1")}
    assert "leases" not in json.loads(origin_file(repo, "claude/kit-state", "kit/state.json"))
    push_lease_file(repo, json.dumps({"lease": doc["lease"], "seq": doc["seq"]}))
    state = make_env(repo["b"], tmp_path, "b").read_state()
    assert state["lease"]["session"] == "session-a" and state["leases"] == {"1": dict(doc["lease"], lane="1")}


def test_update_state_retries_after_a_lost_race(repo: dict[str, Path], tmp_path: Path) -> None:
    env, rival = make_env(repo["a"], tmp_path), make_env(repo["b"], tmp_path, "b")
    base = env.read_state()
    assert env.push_state(st.with_task(base, "K10"), base)
    waits: list[float] = []
    env.sleep = waits.append  # no real waiting in tests
    calls = {"n": 0}

    def claim_k10(fresh: st.State) -> st.State:
        calls["n"] += 1
        if calls["n"] == 1:  # a rival moves the state and the lease ref between this read and this push
            theirs = rival.read_state()
            assert rival.push_state(st.with_task(theirs, "K11"), theirs)
        return st.claim(fresh, "K10", "session-a", NOW)

    assert env.update_state(claim_k10) is not None
    assert calls["n"] == 2 and len(waits) == 1 and 0.5 <= waits[0] <= 2.0
    stored = json.loads(origin_file(repo, "claude/kit-state", "kit/state.json"))
    assert {t["id"] for t in stored["tasks"]} == {"K10", "K11"}
    assert json.loads(origin_file(repo, "claude/lease", "lease.json"))["lease"]["session"] == "session-a"


def test_update_state_gives_up_after_five_lost_races(repo: dict[str, Path], tmp_path: Path) -> None:
    env, rival = make_env(repo["a"], tmp_path), make_env(repo["b"], tmp_path, "b")
    waits: list[float] = []
    env.sleep = waits.append  # no real waiting in tests
    calls = {"n": 0}

    def always_beaten(fresh: st.State) -> st.State:
        calls["n"] += 1
        theirs = rival.read_state()
        assert rival.push_state(dict(theirs, rival_note=calls["n"]), theirs)
        return dict(fresh, mine=True)

    assert env.update_state(always_beaten) is None
    assert calls["n"] == 5 and len(waits) == 4
    assert all(0.5 * (i + 1) <= w <= 2.0 * (i + 1) for i, w in enumerate(waits))
    stored = json.loads(origin_file(repo, "claude/kit-state", "kit/state.json"))
    assert "mine" not in stored and stored["rival_note"] == 5
    assert env.update_state(lambda fresh: None) is None  # a transform that gives up pushes nothing


def test_renew_command_renews_on_the_fresh_state(repo: dict[str, Path], tmp_path: Path) -> None:
    env = claimed_on_origin(repo, tmp_path)
    rival = make_env(repo["b"], tmp_path, "b")
    theirs = rival.read_state()
    assert rival.push_state(st.with_task(theirs, "K11"), theirs)  # the state moved since session-a's claim
    later = NOW + timedelta(minutes=31)
    cfg = cycle.Config([TASK], MODELS, [], [], env.outbox_dir, "session-a", lambda: later)
    assert entry.renew(cfg, env) == "renewed"
    doc = json.loads(origin_file(repo, "claude/lease", "lease.json"))
    assert doc["lease"]["expires"] == "2026-11-01T15:31:00Z" and doc["leases"]["1"] == dict(doc["lease"], lane="1")
    stranger = cycle.Config([TASK], MODELS, [], [], env.outbox_dir, "nobody", lambda: later)
    assert entry.renew(stranger, env).startswith("renew_refused")


def test_update_state_survives_a_failed_read_and_restores_the_heads(repo: dict[str, Path], tmp_path: Path) -> None:
    """D-515 review, medium: a fetch or read failure is a lost try, never an exception."""
    env = make_env(repo["a"], tmp_path)
    env.read_state()
    heads = (env.state_head, env.lease_head)
    waits: list[float] = []
    env.sleep = waits.append  # no real waiting in tests
    env.state_ref = "claude/missing"  # kit/state.json not found: read_state raises RuntimeError
    assert env.update_state(lambda fresh: dict(fresh, mine=True)) is None
    assert len(waits) == 4 and (env.state_head, env.lease_head) == heads
    assert sum("read failed" in line for line in env.log) == 5


def test_update_state_gives_up_without_moving_the_base_a_stale_push_needs(repo: dict[str, Path], tmp_path: Path) -> None:
    """D-515 review, blocking: after update_state gives up or its transform raises, the heads are back
    where they were, so a push against the older base is still rejected as a lost race."""
    env, rival = claimed_on_origin(repo, tmp_path), make_env(repo["b"], tmp_path, "b")
    stale = env.read_state()
    theirs = rival.read_state()
    assert rival.push_state(st.with_task(theirs, "K11"), theirs)
    assert env.update_state(lambda fresh: None) is None
    with pytest.raises(st.LeaseError):
        env.update_state(lambda fresh: st.renew(fresh, "K10", "nobody", NOW))
    assert env.push_state(st.with_task(stale, "K10", note="stale"), stale) is False


def test_drill_finish_after_the_lease_was_reclaimed_writes_no_review_and_pushes_no_branch(repo: dict[str, Path], tmp_path: Path) -> None:
    """D-515 review, blocking, over real git: the runner stalls past three hours, another session releases
    the lease and claims the task again, the heartbeat's renewal fails, then the worker finishes."""
    env, rival = claimed_on_origin(repo, tmp_path), make_env(repo["b"], tmp_path, "b")
    stale = env.read_state()
    later = NOW + timedelta(hours=3, minutes=1)
    theirs = rival.read_state()
    assert rival.push_state(st.claim(st.release_expired(theirs, later), "K10", "session-b", later), theirs)
    with pytest.raises(st.LeaseError):  # what the heartbeat's renewal meets
        env.update_state(lambda fresh: st.renew(fresh, "K10", "session-a", later))
    head = worker_commit(repo["a"], TASK["branch"], "docs/k10/note.md")
    res = cycle.WorkerResult("submitted", ["docs/k10/note.md"], [MODELS["sonnet"]], branch=TASK["branch"], head_sha=head,
                             reviewer_evidence=[MODELS["opus"]])
    cfg = cycle.Config([TASK], MODELS, [], [], env.outbox_dir, "session-a", lambda: later)
    env.enable_auto_merge = lambda b, h: (_ for _ in ()).throw(AssertionError("no auto-merge"))  # type: ignore[method-assign]  # reason: must not be reached
    assert cycle.finish(cfg, env, stale, TASK, res, MODELS["sonnet"], later) == "lost_race"
    state = json.loads(origin_file(repo, "claude/kit-state", "kit/state.json"))
    assert "reviewed_sha" not in st.entry(state, "K10")
    assert json.loads(origin_file(repo, "claude/lease", "lease.json"))["lease"]["session"] == "session-b"
    assert git(repo["origin"], "branch", "--list", TASK["branch"]) == ""
