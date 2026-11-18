"""scripts/ci_proven.py, the proven-parent pass of `Ci` (D-4281, T638). Each case builds a real temporary git
repository shaped like the runner's update merge (HEAD = merge of the reviewed head C and main's tip) and fakes
only the GitHub REST answers; every planted case must print `unproven:` for its own reason, while the clean
case (main moved only by docs) prints `proven=<C>`. Fake data only; no network.
"""

from __future__ import annotations

import importlib.util
import io
import json
import subprocess
import urllib.error
from email.message import Message
from pathlib import Path
from typing import Any

import pytest
import yaml

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("ci_proven", ROOT / "scripts/ci_proven.py")
assert SPEC is not None and SPEC.loader is not None
ci_proven = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci_proven)

REPO = "example-org/example-repo"
ENV = {"GITHUB_REPOSITORY": REPO, "GH_TOKEN": "fake-token-not-a-secret", "GITHUB_API_URL": "https://api.test"}
STEPS = {  # (job name in ci.yml, flag) -> the `changes` step's job id
    ("Checks", "rust"): "checks", ("Checks", "npm"): "checks", ("Lint and slow tests", "rust"): "lint",
    ("Coverage", "rust"): "coverage", ("E2e", "run"): "e2e"}
# One path each job's own rule matches and no other proof input does.
COMPILED = {("Checks", "rust"): "tests/mutation/mutants.toml", ("Checks", "npm"): "eslint.config.js",
            ("Lint and slow tests", "rust"): "clippy.toml", ("Coverage", "rust"): "kit/layers.json",
            ("E2e", "run"): "perf/bench.py"}


def git(cwd: Path, *args: str) -> str:
    done = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, check=True)
    return done.stdout.strip()


def commit(cwd: Path, message: str, changes: dict[str, str | None]) -> str:
    """Write (or delete, for None) each file, then commit everything."""
    for name, text in changes.items():
        path = cwd / name
        if text is None:
            path.unlink()
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")
    git(cwd, "add", "-A")
    git(cwd, "commit", "-q", "--allow-empty", "-m", message)
    return git(cwd, "rev-parse", "HEAD")


def merge(cwd: Path, *parents: str) -> str:
    """A merge commit of `parents` (the first one's tree with the others merged in), checked out detached."""
    git(cwd, "checkout", "-q", "--detach", parents[0])
    git(cwd, "merge", "-q", "--no-ff", "--no-edit", *parents[1:])
    return git(cwd, "rev-parse", "HEAD")


class Repo:
    """base -> main tip M (main moves by `main_change`); base -> C (the branch); HEAD = merge(C, M)."""

    def __init__(self, root: Path, main_change: dict[str, str | None]) -> None:
        self.root = root
        git(root, "init", "-q", "-b", "main")
        git(root, "config", "user.email", "ci@example.test")
        git(root, "config", "user.name", "Fake Tester")
        git(root, "config", "commit.gpgsign", "false")
        base = commit(root, "base", {"docs/readme.md": "a\n", "app/core/src/lib.rs": "// fake\n",
                                     "docs/notes.md": "n\n"})
        self.main = commit(root, "main moves", main_change)
        git(root, "update-ref", "refs/remotes/origin/main", self.main)
        git(root, "checkout", "-q", "-b", "branch", base)
        self.c = commit(root, "branch work", {"app/core/src/work.rs": "// fake work\n"})
        self.head = merge(root, self.c, self.main)


def step_script(tmp: Path, job: str, flag: str) -> str:
    """The real `changes` step script of that job in ci.yml, written where `$0` would point."""
    ci = yaml.safe_load((ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8"))
    (step,) = [s for s in ci["jobs"][STEPS[(job, flag)]]["steps"] if s.get("id") == "changes"]
    path = tmp / f"step-{STEPS[(job, flag)]}.sh"
    path.write_text(step["run"], encoding="utf-8")
    return str(path)


def run(sha: str, job: str = "Checks", **over: Any) -> dict[str, Any]:
    """A fake ci.yml push run on `sha` from this repository."""
    return {"id": over.pop("id", 11), "workflow_id": 7, "path": ".github/workflows/ci.yml", "event": "push",
            "status": "completed", "head_sha": sha, "repository": {"full_name": REPO},
            "head_repository": {"full_name": REPO}, **over}


def job(name: str, conclusion: str | None = "success", status: str = "completed") -> dict[str, Any]:
    return {"name": name, "status": status, "conclusion": conclusion, "run_attempt": 1}


class Api:
    """Routes the script's GET calls to fake answers; an answer may be an exception or raw bytes."""

    def __init__(self, runs: list[dict[str, Any]], jobs: dict[int, list[dict[str, Any]]]) -> None:
        self.answers: dict[str, Any] = {
            "/actions/workflows/ci.yml": {"id": 7, "path": ".github/workflows/ci.yml"},
            "/actions/runs?": {"total_count": len(runs), "workflow_runs": runs}}
        for run_id, listed in jobs.items():
            self.answers[f"/actions/runs/{run_id}/jobs"] = {"total_count": len(listed), "jobs": listed}

    def __call__(self, request: Any, timeout: float) -> io.BytesIO:
        assert timeout == 10
        assert request.get_header("Authorization") == "Bearer fake-token-not-a-secret"
        url = request.full_url
        assert url.startswith(f"https://api.test/repos/{REPO}/")
        key = next(k for k in self.answers if k in url)
        answer = self.answers[key]
        if isinstance(answer, BaseException):
            raise answer
        return io.BytesIO(answer if isinstance(answer, bytes) else json.dumps(answer).encode())


def decide(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, api: Api, job_flag: tuple[str, str] = ("Checks", "rust"),
           env: dict[str, str] = ENV) -> str:
    monkeypatch.chdir(tmp_path / "repo")
    monkeypatch.setattr(ci_proven.urllib.request, "urlopen", api)
    return ci_proven.decide(job_flag[0], job_flag[1], step_script(tmp_path, *job_flag), env)


def green(repo: Repo, name: str = "Checks") -> Api:
    return Api([run(repo.c)], {11: [job(name)]})


def make(tmp_path: Path, main_change: dict[str, str | None] | None = None) -> Repo:
    (tmp_path / "repo").mkdir()
    return Repo(tmp_path / "repo", main_change or {"docs/readme.md": "b\n"})


@pytest.mark.parametrize("job_flag", list(STEPS))
def test_a_docs_only_move_of_main_is_proven_for_every_flag(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                                            job_flag: tuple[str, str]) -> None:
    repo = make(tmp_path)
    assert decide(tmp_path, monkeypatch, green(repo, job_flag[0]), job_flag) == f"proven={repo.c}"


@pytest.mark.parametrize("job_flag", list(STEPS))
def test_a_path_of_the_jobs_own_rule_is_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                                  job_flag: tuple[str, str]) -> None:
    repo = make(tmp_path, {COMPILED[job_flag]: "changed\n"})
    line = decide(tmp_path, monkeypatch, green(repo, job_flag[0]), job_flag)
    assert line == f"unproven: '{COMPILED[job_flag]}' changed since C and matches the job's rule"


@pytest.mark.parametrize(("change", "path"), [
    ({"docs/notes.md": None, "app/notes.md": "n\n"}, "app/notes.md"),  # a rename from docs/ into app/
    ({"app/core/src/lib.rs": None}, "app/core/src/lib.rs"),  # a deleted compiled input
    ({"app/ü x.rs": "// fake\n"}, "app/ü x.rs"),  # a path with a space and a non-ASCII byte
])
def test_renames_deletions_and_odd_paths_are_seen(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                                  change: dict[str, str | None], path: str) -> None:
    repo = make(tmp_path, change)
    assert decide(tmp_path, monkeypatch, green(repo)) == f"unproven: {path!r} changed since C and matches the job's rule"


def test_an_odd_docs_path_still_proves(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path, {"docs/a b ü.md": "x\n"})
    assert decide(tmp_path, monkeypatch, green(repo)) == f"proven={repo.c}"


@pytest.mark.parametrize("path", ["tests/coverage_floor.json", ".github/workflows/twin.yml", "scripts/ci_proven.py",
                                  ".cargo/audit.toml", "kit/tasks.json", "scripts/npm_audit.py"])
def test_a_proof_input_outside_the_rule_is_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, path: str) -> None:
    repo = make(tmp_path, {path: "changed\n"})
    line = decide(tmp_path, monkeypatch, green(repo, "E2e"), ("E2e", "run"))
    assert line == f"unproven: {path!r} changed since C and is a proof input"


@pytest.mark.parametrize(("path", "job"), [("tests/integration/tests/capability_setup.rs", ("E2e", "run")),
                                           ("scripts/brand.py", ("E2e", "run")),
                                           ("scripts/brand.py", ("Checks", "rust")),
                                           ("scripts/brand.py", ("Lint and slow tests", "rust")),
                                           ("scripts/brand.py", ("Coverage", "rust"))])
def test_a_runtime_input_outside_the_e2e_or_rust_rule_is_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                                                  path: str, job: tuple[str, str]) -> None:
    repo = make(tmp_path, {path: "changed\n"})
    line = decide(tmp_path, monkeypatch, green(repo, job[0]), job)
    assert line == f"unproven: {path!r} changed since C and is a proof input"


@pytest.mark.parametrize(("path", "job"), [("tests/fixtures/src/lib.rs", ("E2e", "run")),
                                           ("tests/recovery/Cargo.toml", ("E2e", "run")),
                                           ("tests/recovery/Cargo.toml", ("Checks", "rust")),
                                           ("tools/nested/Cargo.toml", ("E2e", "run")),
                                           ("tools/nested/Cargo.lock", ("Coverage", "rust"))])
def test_any_workspace_member_or_cargo_manifest_is_a_proof_input(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                                                  path: str, job: tuple[str, str]) -> None:
    repo = make(tmp_path, {path: "changed\n"})
    line = decide(tmp_path, monkeypatch, green(repo, job[0]), job)
    assert line.startswith(f"unproven: {path!r} changed since C and ")


def test_a_top_level_python_test_alone_still_proves_e2e(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path, {"tests/test_something.py": "changed\n"})
    assert decide(tmp_path, monkeypatch, green(repo, "E2e"), ("E2e", "run")) == f"proven={repo.c}"


@pytest.mark.parametrize("path", ["tests/perf/web_vitals.spec.ts", "tests/perf/fixture.json",
                                  "sources/03_business_and_design/website_prototype/p/assets/js/app.js",
                                  "tools/x.mjs", "a/b.cjs", "c.mts", "d.cts", "e.jsx", "f.tsx"])
def test_a_tsc_or_eslint_input_outside_the_npm_rule_is_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                                                 path: str) -> None:
    repo = make(tmp_path, {path: "changed\n"})
    line = decide(tmp_path, monkeypatch, green(repo), ("Checks", "npm"))
    assert line == f"unproven: {path!r} changed since C and is a proof input"


def test_head_with_one_parent_is_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path)
    commit(repo.root, "on top", {"docs/x.md": "x\n"})
    assert decide(tmp_path, monkeypatch, green(repo)) == "unproven: HEAD has 1 parent(s), not 2"


def test_head_with_three_parents_is_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path)
    git(repo.root, "checkout", "-q", "-b", "side", repo.main)
    side = commit(repo.root, "side", {"docs/side.md": "s\n"})
    three = git(repo.root, "commit-tree", f"{repo.head}^{{tree}}", "-p", repo.c, "-p", repo.main, "-p", side, "-m", "3")
    git(repo.root, "checkout", "-q", "--detach", three)
    assert decide(tmp_path, monkeypatch, green(repo)) == "unproven: HEAD has 3 parent(s), not 2"


def test_a_second_parent_off_mains_first_parent_line_is_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path)
    git(repo.root, "checkout", "-q", "-b", "side", repo.main)
    side = commit(repo.root, "side", {"docs/side.md": "s\n"})
    merge(repo.root, repo.c, side)
    line = decide(tmp_path, monkeypatch, green(repo))
    assert line == "unproven: HEAD^2 is not on origin/main's first-parent line (or beyond the fetch)"


@pytest.mark.parametrize(("listed", "why"), [
    ([job("Checks", "failure")], "Checks on C is completed/failure"),
    ([job("Checks", "cancelled")], "Checks on C is completed/cancelled"),
    ([job("Checks", "skipped")], "Checks on C is completed/skipped"),
    ([job("Checks", None, "in_progress")], "Checks on C is in_progress/None"),
    ([job("Checks", "success"), job("Checks", "failure")], "Checks on C is completed/failure"),  # attempts 1 and 2
    ([job("Coverage")], "no Checks job in a ci.yml push run on C"),
])
def test_a_job_that_is_not_green_or_missing_is_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                                        listed: list[dict[str, Any]], why: str) -> None:
    repo = make(tmp_path)
    assert decide(tmp_path, monkeypatch, Api([run(repo.c)], {11: listed})) == f"unproven: {why}"


def test_a_run_still_in_progress_is_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path)
    api = Api([run(repo.c, status="in_progress")], {11: [job("Checks")]})
    assert decide(tmp_path, monkeypatch, api) == "unproven: a ci.yml run on C is in_progress"


def test_two_runs_one_red_is_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path)
    api = Api([run(repo.c), run(repo.c, id=12)], {11: [job("Checks")], 12: [job("Checks", "failure")]})
    assert decide(tmp_path, monkeypatch, api) == "unproven: Checks on C is completed/failure"


@pytest.mark.parametrize("over", [
    {"workflow_id": 8, "path": ".github/workflows/twin.yml"}, {"event": "workflow_dispatch"},
    {"repository": {"full_name": "someone/fork"}}, {"head_repository": {"full_name": "someone/fork"}},
])
def test_only_this_repositorys_ci_push_runs_count(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                                  over: dict[str, Any]) -> None:
    repo = make(tmp_path)
    api = Api([run(repo.c, **over)], {11: [job("Checks")]})
    assert decide(tmp_path, monkeypatch, api) == "unproven: no Checks job in a ci.yml push run on C"


def http(code: int) -> urllib.error.HTTPError:
    return urllib.error.HTTPError("https://api.test/x", code, "fake", Message(), None)


@pytest.mark.parametrize(("answer", "why"), [
    (http(403), "HTTP 403"), (http(404), "HTTP 404"), (http(500), "HTTP 500"),
    (TimeoutError("timed out"), "request failed: TimeoutError"), (b"{not json", "malformed JSON"),
    ({"total_count": 101, "workflow_runs": []}, "a page of workflow_runs may be incomplete"),
])
def test_rest_failures_and_doubtful_pages_are_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                                       answer: Any, why: str) -> None:
    repo = make(tmp_path)
    api = green(repo)
    api.answers["/actions/runs?"] = answer
    assert decide(tmp_path, monkeypatch, api) == f"unproven: {why}"


def test_a_full_page_of_jobs_is_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path)
    api = Api([run(repo.c)], {11: [job("Checks")] * 100})
    assert decide(tmp_path, monkeypatch, api) == "unproven: a page of jobs may be incomplete"


def test_a_git_failure_is_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path)
    git(repo.root, "update-ref", "-d", "refs/remotes/origin/main")
    assert decide(tmp_path, monkeypatch, green(repo)) == "unproven: git rev-list failed (exit 128)"


def test_missing_token_is_unproven_and_main_always_exits_zero(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                                              capsys: pytest.CaptureFixture[str]) -> None:
    repo = make(tmp_path)
    assert decide(tmp_path, monkeypatch, green(repo), env={}) == "unproven: GITHUB_REPOSITORY or GH_TOKEN is not set"
    monkeypatch.delenv("GH_TOKEN", raising=False)
    assert ci_proven.main(["--job", "Checks", "--flag", "rust", "--step-script", "/nonexistent"]) == 0
    assert ci_proven.main([]) == 0
    lines = capsys.readouterr().out.splitlines()
    assert lines == ["unproven: GITHUB_REPOSITORY or GH_TOKEN is not set", "unproven: bad arguments"]


def test_the_rule_comes_from_the_step_script(tmp_path: Path) -> None:
    script = tmp_path / "s.sh"
    script.write_text("echo x | grep -Eq '^(app/)' && rust=true\n", encoding="utf-8")
    assert ci_proven.rule_of(str(script), "rust") == "^(app/)"
    with pytest.raises(ci_proven.Unproven, match="0 rules for npm"):
        ci_proven.rule_of(str(script), "npm")
    with pytest.raises(ci_proven.Unproven, match="cannot read the step script"):
        ci_proven.rule_of(str(tmp_path / "missing.sh"), "rust")



# The merge train (D-4281, T640): X, the runner's update merge of this task's reviewed head with the turn holder's
# head, sits on a throwaway ref `claude/train/T###-<12 hex>` in the remote only; the local clone lacks it.
BRANCH = "claude/task/T640-ci-merge-train-refs"


def train_env(tmp_path: Path, ref_name: str = BRANCH) -> dict[str, str]:
    return {**ENV, "GITHUB_REF_NAME": ref_name, "GITHUB_SERVER_URL": f"file://{tmp_path / 'remote'}"}


def train(tmp_path: Path, repo: Repo, ref: str) -> str:
    """Push X (HEAD's tree, a different commit made in another clone) to the remote as `ref`; return X."""
    bare, other = tmp_path / "remote" / REPO, tmp_path / "other"
    if not bare.exists():
        git(tmp_path, "init", "-q", "--bare", str(bare))
        git(repo.root, "push", "-q", str(bare), f"{repo.main}:refs/heads/main", f"{repo.head}:refs/heads/{BRANCH}")
        git(tmp_path, "clone", "-q", str(bare), str(other))
        git(other, "fetch", "-q", "origin", BRANCH)
    x = git(other, "-c", "user.name=Fake Tester", "-c", "user.email=ci@example.test", "commit-tree",
            f"{repo.head}^{{tree}}", "-p", repo.main, "-m", f"train {ref}")
    git(other, "push", "-q", "origin", f"{x}:refs/heads/{ref}")
    assert subprocess.run(["git", "cat-file", "-e", x], cwd=repo.root, capture_output=True, check=False).returncode != 0
    return x


def by_sha(answers: dict[str, list[dict[str, Any]]], jobs: dict[int, list[dict[str, Any]]]) -> Api:
    """An Api whose run list depends on the head_sha asked for."""
    api = Api([], jobs)
    api.answers = {**{f"head_sha={sha}&": {"total_count": len(runs), "workflow_runs": runs}
                      for sha, runs in answers.items()}, **api.answers}
    return api


def red_c_green_x(repo: Repo, x: str, x_job: dict[str, Any] | None = None) -> Api:
    return by_sha({repo.c: [run(repo.c)], x: [run(x, id=12)]}, {11: [job("Checks", "failure")],
                                                                 12: [x_job or job("Checks")]})


def name(repo: Repo, task: str = "T640") -> str:
    return f"claude/train/{task}-{repo.main[:12]}"


def test_a_train_tip_with_the_same_compiled_inputs_proves_the_head(tmp_path: Path,
                                                                   monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path)
    x = train(tmp_path, repo, name(repo))
    calls: list[tuple[list[str], dict[str, str]]] = []
    real = subprocess.run

    def spy(args: list[str], **kw: Any) -> Any:
        calls.append((args, kw.get("env") or {}))
        return real(args, **kw)

    monkeypatch.setattr(ci_proven.subprocess, "run", spy)
    assert decide(tmp_path, monkeypatch, red_c_green_x(repo, x), env=train_env(tmp_path)) == f"proven={x}"
    assert all("fake-token-not-a-secret" not in " ".join(args) for args, _env in calls)
    fetch_env = next(env for args, env in calls if args[1] == "fetch")
    assert "--depth=1" in next(args for args, _env in calls if args[1] == "fetch")
    assert fetch_env["GIT_CONFIG_KEY_0"] == "http.extraheader" and "fake-token" not in fetch_env["GIT_CONFIG_VALUE_0"]


def test_another_tasks_train_tip_and_a_malformed_name_are_ignored(tmp_path: Path,
                                                                  monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path)
    xs = [train(tmp_path, repo, ref) for ref in (name(repo, "T641"), name(repo, "T64"), name(repo) + "0",
                                                 f"claude/train/T640-{repo.main[:12].upper()}", "claude/train/T640-x")]
    api = by_sha({repo.c: [run(repo.c)], **{x: [run(x, id=12)] for x in xs}}, {11: [job("Checks", "failure")],
                                                                              12: [job("Checks")]})
    assert decide(tmp_path, monkeypatch, api, env=train_env(tmp_path)) == "unproven: Checks on C is completed/failure"


@pytest.mark.parametrize(("x_job", "why"), [(job("Checks", "failure"), "completed/failure"),
                                            (job("Checks", "cancelled"), "completed/cancelled"),
                                            (job("Checks", None, "in_progress"), "in_progress/None")])
def test_a_red_cancelled_or_pending_train_run_is_unproven(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                                          x_job: dict[str, Any], why: str) -> None:
    repo = make(tmp_path)
    x = train(tmp_path, repo, name(repo))
    line = decide(tmp_path, monkeypatch, red_c_green_x(repo, x, x_job), env=train_env(tmp_path))
    assert line == f"unproven: Checks on C is completed/failure; {name(repo)}: Checks on C is {why}"


def test_a_listing_error_drops_the_train_and_head_parent_is_still_judged(tmp_path: Path,
                                                                         monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path)
    env = train_env(tmp_path)  # no remote at that path: ls-remote fails
    line = decide(tmp_path, monkeypatch, Api([run(repo.c)], {11: [job("Checks", "failure")]}), env=env)
    assert line == "unproven: Checks on C is completed/failure; train refs: git ls-remote failed (exit 128)"
    assert decide(tmp_path, monkeypatch, green(repo), env=env) == f"proven={repo.c}"


def test_nothing_is_proven_on_a_train_ref(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path)
    line = decide(tmp_path, monkeypatch, green(repo), env=train_env(tmp_path, name(repo)))
    assert line == "unproven: a claude/train/ ref proves nothing"


def test_a_failed_fetch_drops_only_that_candidate(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = make(tmp_path)
    first, second = name(repo), f"claude/train/T640-{'f' * 12}"
    x1, x2 = train(tmp_path, repo, first), train(tmp_path, repo, second)
    real = subprocess.run

    def flaky(args: list[str], **kw: Any) -> Any:
        if args[1] == "fetch" and args[-1] == f"refs/heads/{first}":
            return subprocess.CompletedProcess(args, 1, b"", b"fatal: secret-looking stderr")
        return real(args, **kw)

    monkeypatch.setattr(ci_proven.subprocess, "run", flaky)
    api = by_sha({repo.c: [run(repo.c)], x1: [run(x1, id=12)], x2: [run(x2, id=12)]},
                 {11: [job("Checks", "failure")], 12: [job("Checks")]})
    assert decide(tmp_path, monkeypatch, api, env=train_env(tmp_path)) == f"proven={x2}"
    git(tmp_path / "other", "push", "-q", "origin", f":refs/heads/{second}")
    line = decide(tmp_path, monkeypatch, api, env=train_env(tmp_path))
    assert line == f"unproven: Checks on C is completed/failure; {first}: fetch failed (exit 1)"
