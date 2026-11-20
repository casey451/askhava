"""A failed attempt's kept work on a `claude/wip/**` ref (D-4720, T726): the path rules, the per-commit scan with
main's scanners, the job-token-only push and delete, `prepare_branch` continuing from a WIP ref, the
`wip_continue` switch and the workflows a WIP ref must never start. Over real git in a temporary clone with a
bare origin and a fake `gitleaks` on PATH; the token cases plant the command runner. Fake data only.
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
from pathlib import Path
from typing import Any

import pytest
import yaml

from runner import github_env as ge, wip
from runner.__main__ import branch_note
from runner.github_env import GitHubEnv

ROOT = Path(__file__).resolve().parent.parent
MAIN_FILES = ("scripts/pii_scan.py", "scripts/brand.py", "app/core/data/brand.json", "scripts/check_plan_lock.py")
REF = "claude/wip/T009-a1"
TASK_BRANCH = "claude/task/T009-wip"
FAKE_GITLEAKS = """#!/bin/bash
echo "$@" >> "$FAKE_GITLEAKS_LOG"
[ -n "$FAKE_GITLEAKS_SLEEP" ] && sleep "$FAKE_GITLEAKS_SLEEP"
[ -n "$FAKE_GITLEAKS_EXIT" ] && exit "$FAKE_GITLEAKS_EXIT"
src="${@: -1}"
if [ "$1" = git ]; then git -C "$src" log -p $3 | grep -q "^+.*FAKESECRET" && exit 1
else grep -rq --exclude=.git FAKESECRET "$src" && exit 1; fi
exit 0
"""
# A street address the PII scanner flags, built at run time so this file itself stays clean.
ADDRESS = "40" + "21 Quartzite " + "Drive"


def git(cwd: Path, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


def commit(root: Path, files: dict[str, str | bytes | None], message: str = "Fake change") -> str:
    for path, body in files.items():
        target = root / path
        if body is None:
            target.unlink()
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(body) if isinstance(body, bytes) else target.write_text(body, encoding="utf-8")
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", message)
    return git(root, "rev-parse", "HEAD")


@pytest.fixture
def world(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> dict[str, Any]:
    """A clone of a bare origin whose main has main's scanners, on a `work` branch; a fake gitleaks on PATH."""
    origin, root, fake = tmp_path / "origin.git", tmp_path / "clone", tmp_path / "bin"
    subprocess.run(["git", "init", "-q", "--bare", "-b", "main", str(origin)], check=True)
    root.mkdir()
    git(root, "init", "-q", "-b", "main")
    git(root, "config", "user.name", "Fake Runner")
    git(root, "config", "user.email", "runner@example.com")
    git(root, "remote", "add", "origin", str(origin))
    protected = json.loads((ROOT / "kit/tasks.json").read_text(encoding="utf-8"))["protected_paths"]
    files: dict[str, str | bytes | None] = {p: (ROOT / p).read_text(encoding="utf-8") for p in MAIN_FILES}
    files.update({"kit/tasks.json": json.dumps({"protected_paths": protected}), ".github/workflows/x.yml": "name: x\n",
                  "app/demo/notes.txt": "start\n"})
    main = commit(root, files, "Fake main")
    git(root, "push", "-q", "origin", "main")
    git(root, "switch", "-q", "-c", "work")
    fake.mkdir()
    (fake / "gitleaks").write_text(FAKE_GITLEAKS, encoding="utf-8")
    (fake / "gitleaks").chmod(0o755)
    monkeypatch.setenv("PATH", f"{fake}:{Path(shutil.which('git') or '/usr/bin/git').parent}:/usr/bin:/bin")
    monkeypatch.setenv("FAKE_GITLEAKS_LOG", str(tmp_path / "gitleaks.log"))
    for name in ("RUNNER_PUSH_TOKEN", "FAKE_GITLEAKS_EXIT", "FAKE_GITLEAKS_SLEEP", "GITHUB_ACTIONS"):
        monkeypatch.delenv(name, raising=False)
    env = GitHubEnv(root, [{"id": "T009", "branch": TASK_BRANCH}], tmp_path, repo="o/r", starter="actions",
                    runner_sha="x", run=no_gh)
    return {"root": root, "main": main, "env": env, "log": tmp_path / "gitleaks.log"}


def no_gh(args: list[str], stdin: str | None, cwd: Path) -> tuple[int, str, str]:
    """The real command runner, but no `gh` call ever leaves the test."""
    return (1, "", "no gh in tests") if args[:1] == ["gh"] else ge.run_command(args, stdin, cwd)


def scanned(w: dict[str, Any]) -> list[str]:
    return w["log"].read_text(encoding="utf-8").splitlines() if w["log"].exists() else []


# ---- wip_check and scan


def test_a_clean_range_passes_and_gitleaks_scans_exactly_the_range_and_the_tree(world: dict[str, Any]) -> None:
    head = commit(world["root"], {"app/demo/notes.txt": "start\nmore\n"})
    assert world["env"].wip_check(head) == ""
    runs = scanned(world)
    assert len(runs) == 2 and runs[0].startswith(f"git --log-opts {' '.join(wip.NO_COLOR)} --diff-merges=first-parent {world['main']}..{head} "
                                         "--no-banner --redact --exit-code 1")
    assert runs[1].startswith("dir --no-banner --redact --exit-code 1")


@pytest.mark.parametrize("path", ["runner/extra.py", "stand_ins/x.py", ".gitattributes", "app/demo/.gitattributes",
                                  "scripts/new_check.py", "scripts/smoke.sh", "kit/runner.json", ".gitleaksignore"])
def test_a_protected_locked_or_attributes_path_is_refused_before_any_scanner(world: dict[str, Any], path: str) -> None:
    head = commit(world["root"], {path: "fake\n"})
    assert "protected or locked path" in world["env"].wip_check(head)
    assert scanned(world) == []


def test_a_github_tree_change_is_refused(world: dict[str, Any]) -> None:
    head = commit(world["root"], {".github/workflows/x.yml": "name: y\n"})
    assert "changes .github" in world["env"].wip_check(head)


@pytest.mark.parametrize("path, body", [("app/demo/blob.bin", b"\x00\x01fake"), ("app/demo/pic.svg", "<svg/>\n"),
                                        ("app/demo/pic.png", b"\x89PNG\r\n\x1a\n\x00")])
def test_a_binary_or_an_image_is_refused(world: dict[str, Any], path: str, body: str | bytes) -> None:
    head = commit(world["root"], {path: body})
    assert "binary or an image" in world["env"].wip_check(head)
    assert scanned(world) == []


def test_an_added_gitleaks_allow_is_refused(world: dict[str, Any]) -> None:
    head = commit(world["root"], {"app/demo/notes.txt": "start\nvalue = 1  # gitleaks:" + "allow\n"})
    assert "gitleaks:allow" in world["env"].wip_check(head)
    assert scanned(world) == []


def test_a_leak_in_a_middle_commit_with_a_clean_head_tree_is_refused(world: dict[str, Any]) -> None:
    commit(world["root"], {"app/demo/notes.txt": "start\nFAKESECRET\n"})
    head = commit(world["root"], {"app/demo/notes.txt": "start\n"})
    assert world["env"].wip_check(head) == "gitleaks git: a leak was found"


def merge_main_adding(w: dict[str, Any], body: str) -> str:
    """A `_merge_main`-style merge of a newer main into `work` whose resolution also adds `body` (D-4798)."""
    root = w["root"]
    commit(root, {"app/demo/branch.txt": "branch\n"})
    git(root, "switch", "-q", "main")
    w["main"] = commit(root, {"app/demo/main.txt": "newer main\n"})
    git(root, "switch", "-q", "work")
    git(root, "merge", "-q", "--no-ff", "--no-commit", "main")
    return commit(root, {"app/demo/merged.txt": body}, "Merge main")


def test_a_leak_added_in_a_merge_then_removed_is_refused(world: dict[str, Any]) -> None:
    merge_main_adding(world, "FAKESECRET\n")
    head = commit(world["root"], {"app/demo/merged.txt": None})
    assert world["env"].wip_check(head) == "gitleaks git: a leak was found"


def test_an_allow_mark_added_in_a_merge_is_refused_but_one_from_main_is_not(world: dict[str, Any]) -> None:
    mark = "gitleaks:" + "allow"
    head = merge_main_adding(world, f"value = 1  # {mark}\n")
    assert mark in world["env"].wip_check(head)
    log = f"@@@ -1,1 -1,1 +1,2 @@@\n+ from main {mark}\n +from the branch {mark}\n++merge {mark}\n@@ -1 +1 @@\n+plain {mark}\n"
    assert list(wip.added_lines(log)) == [f"++merge {mark}", f"+plain {mark}"]


def test_a_pii_hit_in_a_changed_file_is_refused(world: dict[str, Any]) -> None:
    commit(world["root"], {"app/demo/visit.txt": f"Visit {ADDRESS} soon.\n"})
    head = commit(world["root"], {"app/demo/visit.txt": None})
    assert world["env"].wip_check(head).startswith("PII scan: app/demo/visit.txt in ")


def test_a_pii_hit_in_a_commit_message_is_refused(world: dict[str, Any]) -> None:
    head = commit(world["root"], {"app/demo/notes.txt": "start\nok\n"}, f"Fix the page for {ADDRESS}")
    assert world["env"].wip_check(head).startswith("PII scan: a commit message")


def test_a_missing_gitleaks_is_refused(world: dict[str, Any], monkeypatch: pytest.MonkeyPatch) -> None:
    head = commit(world["root"], {"app/demo/notes.txt": "start\nok\n"})
    monkeypatch.setenv("PATH", f"{Path(shutil.which('git') or '/usr/bin/git').parent}")
    assert shutil.which("gitleaks") is None
    assert world["env"].wip_check(head) == "gitleaks is not installed"


def test_a_scanner_exit_of_2_is_refused(world: dict[str, Any], monkeypatch: pytest.MonkeyPatch) -> None:
    head = commit(world["root"], {"app/demo/notes.txt": "start\nok\n"})
    monkeypatch.setenv("FAKE_GITLEAKS_EXIT", "2")
    assert world["env"].wip_check(head) == "gitleaks git: exit 2"


def test_the_time_bound_is_a_refusal(world: dict[str, Any], monkeypatch: pytest.MonkeyPatch) -> None:
    head = commit(world["root"], {"app/demo/notes.txt": "start\nok\n"})
    monkeypatch.setenv("FAKE_GITLEAKS_SLEEP", "5")
    assert "bound" in wip.scan(world["root"], world["main"], head, seconds=1)
    ticks = iter([0.0, 0.0, 500.0])
    assert "bound" in wip.scan(world["root"], world["main"], head, clock=lambda: next(ticks, 999.0))


def test_a_branch_edited_pii_scanner_in_the_checkout_is_not_the_one_run(world: dict[str, Any]) -> None:
    neutered = "def scan_text(path, text):\n    return []\n\n\ndef main(argv=None):\n    return 0\n"
    head = commit(world["root"], {"scripts/pii_scan.py": neutered, "app/demo/visit.txt": f"Visit {ADDRESS}.\n"})
    assert (world["root"] / "scripts/pii_scan.py").read_text(encoding="utf-8") == neutered
    assert wip.scan(world["root"], world["main"], head).startswith("PII scan: app/demo/visit.txt in ")


# ---- the clone's own git state can neither hide a line nor run code (D-4799)

FAKE_GPG = """#!/bin/bash
case "$*" in *--verify*) touch "$FAKE_MARKER" ;; *) echo "[GNUPG:] SIG_CREATED " >&2
  printf -- '-----BEGIN PGP SIGNATURE-----\\nZmFrZQ==\\n-----END PGP SIGNATURE-----\\n' ;; esac
"""


def middle_leak(w: dict[str, Any]) -> str:
    """A FAKESECRET added in a middle commit and removed by the head."""
    commit(w["root"], {"app/demo/notes.txt": "start\nFAKESECRET\n"})
    return commit(w["root"], {"app/demo/notes.txt": "start\n"})


@pytest.mark.parametrize("key, value", [("color.ui", "always"), ("log.showSignature", "true"), ("core.hooksPath", "hooks"),
                                        ("diff.fake.textconv", "cat"), ("include.path", "extra"), ("core.fsmonitor", "true"),
                                        ("core.sparseCheckout", "true"), ("extensions.worktreeConfig", "true")])
def test_a_local_config_key_outside_the_allow_list_is_refused(world: dict[str, Any], key: str, value: str) -> None:
    head = commit(world["root"], {"app/demo/notes.txt": "start\nok\n"})
    git(world["root"], "config", key, value)
    assert "does not allow" in world["env"].wip_check(head)
    assert "does not allow" in wip.scan(world["root"], world["main"], head)
    assert scanned(world) == []


def test_local_attributes_are_refused(world: dict[str, Any]) -> None:
    head = middle_leak(world)
    (world["root"] / ".git/info").mkdir(exist_ok=True)
    (world["root"] / ".git/info/attributes").write_text("* -diff\n", encoding="utf-8")
    assert "info/attributes" in world["env"].wip_check(head)


def test_a_planted_shallow_boundary_is_refused(world: dict[str, Any]) -> None:
    head = middle_leak(world)
    (world["root"] / ".git/shallow").write_text(git(world["root"], "rev-parse", "HEAD~1") + "\n", encoding="utf-8")
    assert "shallow" in world["env"].wip_check(head)


def test_a_replace_ref_does_not_hide_a_middle_commit(world: dict[str, Any]) -> None:
    head = middle_leak(world)
    middle = git(world["root"], "rev-parse", "HEAD~1")
    clean = git(world["root"], "commit-tree", f"{world['main']}^{{tree}}", "-p", world["main"], "-m", "Fake")
    git(world["root"], "replace", middle, clean)
    assert "FAKESECRET" not in git(world["root"], "log", "-p", f"{world['main']}..{head}")
    assert world["env"].wip_check(head) == "gitleaks git: a leak was found"


def test_global_colour_signatures_and_hooks_neither_hide_a_leak_nor_run(world: dict[str, Any], tmp_path: Path,
                                                                        monkeypatch: pytest.MonkeyPatch) -> None:
    root, marker, gpg, hooks = world["root"], tmp_path / "ran", tmp_path / "fake-gpg", tmp_path / "hooks"
    gpg.write_text(FAKE_GPG, encoding="utf-8")
    gpg.chmod(0o755)
    hooks.mkdir()
    for hook in (hooks / "post-checkout", root / ".git/hooks/post-checkout"):
        hook.write_text(f"#!/bin/bash\ntouch {marker}\n", encoding="utf-8")
        hook.chmod(0o755)
    monkeypatch.setenv("FAKE_MARKER", str(marker))
    commit(root, {"app/demo/notes.txt": "start\nFAKESECRET\n"})
    git(root, "-c", f"gpg.program={gpg}", "commit", "-q", "-S", "--allow-empty", "-m", "Fake signed")
    head = commit(root, {"app/demo/notes.txt": "start\n"})
    (tmp_path / "home").mkdir()
    (tmp_path / "home/.gitconfig").write_text(f"[color]\n\tui = always\n[log]\n\tshowSignature = true\n"
                                              f"[gpg]\n\tprogram = {gpg}\n[core]\n\thooksPath = {hooks}\n", encoding="utf-8")
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    monkeypatch.setenv("GIT_CONFIG_GLOBAL", str(tmp_path / "home/.gitconfig"))
    assert "\x1b[" in subprocess.run(["git", "log", "-p", "-1", "HEAD~2"], cwd=root, capture_output=True, text=True).stdout
    assert world["env"].wip_check(head) == "gitleaks git: a leak was found"
    assert not marker.exists()


def test_no_commits_ahead_or_an_unreadable_range_is_refused(world: dict[str, Any]) -> None:
    assert "no commits ahead" in world["env"].wip_check(world["main"])
    assert wip.scan(world["root"], "0" * 40, world["main"]) != ""


def test_the_path_rules_on_their_own() -> None:
    protected, locked = ["runner/", "kit/layers.json"], ["kit/runner.json", "scripts/*.py"]
    paths = ["runner/a.py", "kit/layers.json", "kit/layers.json.bak", "scripts/x.py", "app/.gitleaks.toml", "app/ok.rs"]
    assert wip.blocked_paths(paths, protected, locked) == ["runner/a.py", "kit/layers.json", "scripts/x.py", "app/.gitleaks.toml"]
    assert wip.binary_paths("1\t0\tapp/a.rs\0-\t-\tapp/b.bin\0" + "2\t0\tapp/C.JPG\0") == ["app/b.bin", "app/C.JPG"]
    assert wip.wip_ref("T009", 2) == "claude/wip/T009-a2"
    with pytest.raises(ValueError):
        wip.wip_ref("T009/../main", 1)
    assert "scripts/*.sh" in wip.locked_patterns((ROOT / "scripts/check_plan_lock.py").read_text(encoding="utf-8"))


# ---- push_wip, delete_wip_ref, wip_refs: the job token only


def token_env(monkeypatch: pytest.MonkeyPatch, **overrides: str | None) -> None:
    values = {"GITHUB_ACTIONS": "true", "RUNNER_STARTER": "actions", "RUNNER_CHECKS_TOKEN": "fake-job-token",
              "RUNNER_PUSH_TOKEN": "fake-pat-token", "GITHUB_REPOSITORY": "o/r", **overrides}
    for name, value in values.items():
        if value is None:
            monkeypatch.delenv(name, raising=False)
        else:
            monkeypatch.setenv(name, value)


def routed(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> tuple[GitHubEnv, list[tuple[list[str], dict[str, str] | None]]]:
    """A GitHubEnv over the real run_command whose process launcher is planted: ls-remote says absent, the rest succeed."""
    calls: list[tuple[list[str], dict[str, str] | None]] = []

    def launch(args: list[str], stdin: str | None, cwd: Path, env: dict[str, str] | None, timeout: int = 600) -> Any:
        calls.append((args, env))
        return subprocess.CompletedProcess(args, 2 if "ls-remote" in args else 0, "", "")

    monkeypatch.setattr(ge, "_run", launch)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    return GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=ge.run_command), calls


def test_push_and_delete_use_the_job_tokens_credential_helper_never_the_pat(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    token_env(monkeypatch)
    env, calls = routed(tmp_path, monkeypatch)
    assert env.push_wip(REF, "a" * 40) and env.delete_wip_ref(REF)
    pushes = [(args, e) for args, e in calls if args[:2] == ["git", "push"]]
    assert [args[-1] for args, _ in pushes] == [f"{'a' * 40}:refs/heads/{REF}", f":refs/heads/{REF}"]
    for args, e in pushes:
        assert e is not None and "RUNNER_PUSH_TOKEN" not in e and "GH_TOKEN" not in e
        config = {e[f"GIT_CONFIG_KEY_{i}"]: e[f"GIT_CONFIG_VALUE_{i}"] for i in range(int(e["GIT_CONFIG_COUNT"]))}
        assert config["credential.https://github.com.helper"].endswith('echo "password=$RUNNER_CHECKS_TOKEN"; }; f')
        assert config["core.hooksPath"] == "/dev/null" and e["GIT_CONFIG_GLOBAL"] == "/dev/null" and "--no-verify" in args
        assert not any("fake-" in a or a.startswith("+") or a.startswith("--force") for a in args)


@pytest.mark.parametrize("key", ["url.https://example.invalid/.insteadOf", "credential.helper", "core.hooksPath"])
def test_a_clone_whose_config_could_misuse_the_job_token_makes_no_push(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, key: str) -> None:
    token_env(monkeypatch)
    env, calls = routed(tmp_path, monkeypatch)
    git(tmp_path, "config", key, "fake")
    assert not env.push_wip(REF, "a" * 40) and not env.delete_wip_ref(REF) and calls == []
    assert "does not allow" in env.log[-1]


@pytest.mark.parametrize("name", ["claude/task/T009-wip", "claude/wip/T009-a1/x", "claude/wip/foo", "main", None])
def test_a_task_branch_or_a_name_that_is_not_a_wip_ref_is_refused_with_no_call(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, name: Any) -> None:
    token_env(monkeypatch)
    env, calls = routed(tmp_path, monkeypatch)
    assert not env.push_wip(name, "a" * 40) and not env.delete_wip_ref(name) and calls == []


@pytest.mark.parametrize("overrides", [{"RUNNER_CHECKS_TOKEN": None}, {"RUNNER_CHECKS_TOKEN": ""}, {"GITHUB_ACTIONS": None},
                                       {"RUNNER_STARTER": "routine"}, {"RUNNER_STARTER": None}])
def test_no_job_token_outside_actions_or_another_starter_makes_no_call(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                                                         overrides: dict[str, str | None]) -> None:
    token_env(monkeypatch, **overrides)
    env, calls = routed(tmp_path, monkeypatch)
    assert not env.push_wip(REF, "a" * 40) and not env.delete_wip_ref(REF) and calls == []
    assert ge.run_command(["git", "push", "--porcelain", "https://github.com/o/r.git", f":refs/heads/{REF}"], None, tmp_path)[0] == 1
    assert calls == []


def test_an_existing_ref_or_a_rejected_push_is_false(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    token_env(monkeypatch)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    env = GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=lambda a, s, c: (0, "", ""))
    assert not env.push_wip(REF, "a" * 40)
    env._run = lambda a, s, c: (2 if "ls-remote" in a else 1, "", "rejected by a rule")  # type: ignore[method-assign]
    assert not env.push_wip(REF, "a" * 40) and "rejected" in env.log[-1]


def test_wip_refs_lists_only_wip_ref_names(tmp_path: Path) -> None:
    out = f"{'a' * 40}\trefs/heads/{REF}\n{'b' * 40}\trefs/heads/claude/wip/odd\n"
    env = GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x", run=lambda a, s, c: (0, out, ""))
    assert env.wip_refs() == {REF: "a" * 40}
    env._run = lambda a, s, c: (128, "", "no network")  # type: ignore[method-assign]
    assert env.wip_refs() is None


# ---- prepare_branch


@pytest.fixture
def kept(world: dict[str, Any]) -> dict[str, Any]:
    """An earlier attempt's commit on origin's REF; main has moved on since."""
    root = world["root"]
    world["wip"] = commit(root, {"app/demo/notes.txt": "start\nkept\n"})
    git(root, "push", "-q", "origin", f"HEAD:refs/heads/{REF}")
    git(root, "switch", "-q", "main")
    world["main"] = commit(root, {"app/demo/other.txt": "main moved\n"})
    git(root, "push", "-q", "origin", "main")
    return world


def prepare(w: dict[str, Any], record: dict[str, str] | None) -> dict[str, Any]:
    task = {"id": "T009", **({"wip_continue": record} if record is not None else {})}
    return w["env"].prepare_branch(task, 2, "session-1")


def test_prepare_continues_from_a_matching_wip_ref_with_main_merged_in(kept: dict[str, Any]) -> None:
    got = prepare(kept, {"ref": REF, "sha": kept["wip"]})
    tip = git(kept["root"], "rev-parse", TASK_BRANCH)
    assert {k: got[k] for k in ("branch", "base", "continued_from", "base_sha")} == {
        "branch": TASK_BRANCH, "base": "wip", "continued_from": REF, "base_sha": tip}
    assert git(kept["root"], "rev-parse", f"{tip}^1", f"{tip}^2").split() == [kept["wip"], kept["main"]]
    assert "wip_dropped" not in got


@pytest.mark.parametrize("record, why", [({"ref": REF, "sha": "b" * 40}, "not " + "b" * 12),
                                         ({"ref": "claude/wip/T009-a7", "sha": "b" * 40}, "is not on origin")])
def test_prepare_starts_fresh_on_a_sha_mismatch_or_a_missing_ref(kept: dict[str, Any], record: dict[str, str], why: str) -> None:
    got = prepare(kept, record)
    assert got["base"] == "main" and got["base_sha"] == kept["main"] and "continued_from" not in got
    assert why in got["wip_dropped"]


def test_prepare_starts_fresh_when_main_conflicts(kept: dict[str, Any]) -> None:
    kept["main"] = commit(kept["root"], {"app/demo/notes.txt": "start\nother\n"})
    git(kept["root"], "push", "-q", "origin", "main")
    got = prepare(kept, {"ref": REF, "sha": kept["wip"]})
    assert got["base"] == "main" and "conflicted" in got["wip_dropped"] and "continued_from" not in got


def test_prepare_ignores_the_wip_ref_when_origin_has_the_task_branch(kept: dict[str, Any]) -> None:
    git(kept["root"], "push", "-q", "origin", f"{kept['wip']}:refs/heads/{TASK_BRANCH}")
    got = prepare(kept, {"ref": REF, "sha": kept["wip"]})
    assert got["wip_dropped"] == "origin has the task branch" and "continued_from" not in got


def test_prepare_without_a_wip_record_is_unchanged(kept: dict[str, Any]) -> None:
    got = prepare(kept, None)
    assert got == {"branch": TASK_BRANCH, "branch_ready": True, "base": "main", "base_sha": kept["main"]}


def test_the_branch_note_tells_the_worker_what_it_continues() -> None:
    note = branch_note({"branch_ready": True, "branch": TASK_BRANCH, "base": "wip", "continued_from": REF})
    assert REF in note and "last_failure" in note and "git revert" in note and f"`git switch {TASK_BRANCH}`" in note


# ---- the switch and the workflows


@pytest.mark.parametrize("text, on", [('{"wip_continue": true}', True), ('{"wip_continue": "true"}', False),
                                      ('{"wip_continue": 1}', False), ("{}", False), ("[true]", False), ("{not json", False),
                                      (None, False)])
def test_wip_continue_is_off_unless_exactly_true(tmp_path: Path, text: str | None, on: bool) -> None:
    if text is not None:
        (tmp_path / "kit").mkdir()
        (tmp_path / "kit/runner.json").write_text(text, encoding="utf-8")
    assert GitHubEnv(tmp_path, [], tmp_path, repo="o/r", starter="actions", runner_sha="x").wip_continue() is on
    assert GitHubEnv(ROOT, [], tmp_path, repo="o/r", starter="actions", runner_sha="x").wip_continue() is True


def _matches(pattern: str, name: str) -> bool:
    """A GitHub branch filter glob: `**` matches anything, `*` anything but `/`."""
    rx = "".join(".*" if part == "**" else "[^/]*" if part == "*" else re.escape(part) for part in re.split(r"(\*\*|\*)", pattern))
    return re.fullmatch(rx, name) is not None


def test_no_push_workflow_can_start_on_a_wip_ref() -> None:
    for path in sorted((ROOT / ".github/workflows").glob("*.yml")):
        on = yaml.safe_load(path.read_text(encoding="utf-8")).get(True) or {}
        push = on.get("push", None) if isinstance(on, dict) else ({} if "push" in on else None)
        if isinstance(on, dict) and "push" in on and push is None:
            push = {}
        if push is None:
            continue
        branches = (push or {}).get("branches")
        assert branches, f"{path.name}: a push trigger with no branch filter would match claude/wip/x"
        if any(_matches(b, "claude/wip/x") for b in branches if not b.startswith("!")):
            assert "!claude/wip/**" in branches, path.name
    auto = yaml.safe_load((ROOT / ".github/workflows/auto-pr.yml").read_text(encoding="utf-8"))[True]["push"]["branches"]
    assert auto == ["claude/task/**"] and not _matches(auto[0], "claude/wip/x")
