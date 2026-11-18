#!/usr/bin/env python3
"""The proven-parent pass of `Ci` (D-4281, T638): may a `changes` step of `.github/workflows/ci.yml` set a
compile flag that D-489's rule set true back to false, because the same job already passed on a commit whose
compiled inputs for that job are byte-identical?

Usage (inside a `changes` step, after the rule ran): `python3 scripts/ci_proven.py --job <job name> --flag
<rust|npm|run> --step-script "$0"`. `$0` is the file the runner wrote the step's script to, so the rule is read
from the very `grep -Eq '<rule>' ... && <flag>=true` the step just ran, never from a second copy.

Proven only when ALL hold for C = HEAD^1: HEAD has exactly two parents and HEAD^2 is on origin/main's
first-parent line (the runner's update merge, `runner/update_merge.py` `commit`); every push run of this
repository's ci.yml on C has the job of that exact name completed with `success` in every attempt, and at least
one such job exists; and `git diff --no-renames C HEAD` touches neither the rule nor `EXTRA`. Anything else,
including any git, HTTP, timeout, JSON or paging doubt, is unproven and the flag stays true.

Prints exactly one line, `proven=<40-hex sha>` or `unproven: <why>`, and always exits 0. Standard library only;
the token (GH_TOKEN) is sent in a header and never printed.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.request
from typing import Any

WORKFLOW = ".github/workflows/ci.yml"
# Inputs of the skippable steps that no job rule names (D-4321 lists each step's inputs): the coverage floor and
# its script (Coverage compares against origin/main), cargo's config and cargo-audit's ignore list (`.cargo/`),
# every workflow (the steps and pins themselves; app/privacy's scrub test reads them), this script, the npm audit
# script, the feature-on security tests E2e builds, rustfmt's config, and the files the Rust tests read at run
# time: kit/tasks.json (route_wiring, architecture_migrations), the Dockerfile (image_pinned, static_files_route),
# tests/task_kinds_deferred.json (task_registry) and scripts/check_rust_rules.py (status_enums). For tsc and
# eslint: tests/perf/ (tsconfig includes its .ts) and every .js/.mjs/.cjs/.ts/.mts/.cts/.jsx/.tsx path anywhere
# (eslint parses each one its ignores do not drop, e.g. the prototype .js under sources/).
EXTRA = re.compile(r"^(tests/coverage_floor\.json|scripts/coverage_floor\.py|\.cargo/|\.github/workflows/"
                   r"|scripts/ci_proven\.py|scripts/npm_audit\.py|tests/security/|\.?rustfmt\.toml"
                   r"|kit/tasks\.json|Dockerfile|tests/task_kinds_deferred\.json|scripts/check_rust_rules\.py"
                   r"|tests/perf/|.*\.[cm]?[jt]sx?\Z)", re.DOTALL)
TIMEOUT = 10
PER_PAGE = 100
SHA = re.compile(r"^[0-9a-f]{40}$")


class Unproven(Exception):
    """Why the flag stays true."""


def git(*args: str) -> bytes:
    """Run git in the current directory; any failure is Unproven."""
    try:
        done = subprocess.run(["git", *args], capture_output=True, timeout=60, check=False)
    except (OSError, subprocess.SubprocessError) as err:
        raise Unproven(f"git failed: {type(err).__name__}") from err
    if done.returncode != 0:
        raise Unproven(f"git {args[0]} failed (exit {done.returncode})")
    return done.stdout


def candidate() -> tuple[str, str]:
    """(C = HEAD^1, HEAD) when HEAD is a two-parent merge whose second parent is on main's first-parent line."""
    head, *parents = git("rev-list", "--parents", "-n", "1", "HEAD").decode().split()
    if len(parents) != 2:
        raise Unproven(f"HEAD has {len(parents)} parent(s), not 2")
    main_line = set(git("rev-list", "--first-parent", "origin/main").decode().split())
    if parents[1] not in main_line:
        raise Unproven("HEAD^2 is not on origin/main's first-parent line (or beyond the fetch)")
    if not SHA.match(parents[0]):
        raise Unproven("HEAD^1 is not a full sha")
    return parents[0], head


def rule_of(script: str, flag: str) -> str:
    """The step's own rule for `flag`: the one `grep -Eq '<rule>' ... && <flag>=true` in its script file."""
    try:
        with open(script, encoding="utf-8") as handle:
            text = handle.read()
    except OSError as err:
        raise Unproven(f"cannot read the step script: {type(err).__name__}") from err
    found = re.findall(rf"grep -Eq '([^']+)'\s*&&\s*{re.escape(flag)}=true", text)
    if len(found) != 1:
        raise Unproven(f"{len(found)} rules for {flag} in the step script, not 1")
    return found[0]


def check_diff(parent: str, rule: str) -> None:
    """Unproven when a path changed between C and HEAD matches the job's rule or the extra paths."""
    try:
        pattern = re.compile(rule)
    except re.error as err:
        raise Unproven("the rule does not compile") from err
    out = git("-c", "core.quotepath=off", "diff", "-z", "--no-renames", "--name-only", parent, "HEAD")
    for path in out.decode("utf-8", "surrogateescape").split("\0"):
        if path and pattern.search(path):
            raise Unproven(f"{path!r} changed since C and matches the job's rule")
        if path and EXTRA.search(path):
            raise Unproven(f"{path!r} changed since C and is a proof input")


def get(url: str, token: str) -> dict[str, Any]:
    """One REST call (10 s); any HTTP error, timeout or non-object JSON is Unproven."""
    request = urllib.request.Request(url, headers={
        "Accept": "application/vnd.github+json", "Authorization": f"Bearer {token}",
        "X-GitHub-Api-Version": "2022-11-28", "User-Agent": "ci-proven"})
    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
            body = response.read()
    except urllib.error.HTTPError as err:
        raise Unproven(f"HTTP {err.code}") from err
    except (urllib.error.URLError, OSError) as err:
        raise Unproven(f"request failed: {type(err).__name__}") from err
    try:
        data = json.loads(body)
    except ValueError as err:
        raise Unproven("malformed JSON") from err
    if not isinstance(data, dict):
        raise Unproven("malformed JSON")
    return data


def listed(data: dict[str, Any], key: str) -> list[dict[str, Any]]:
    """The items of one page, Unproven when the page may be incomplete or is malformed."""
    items, total = data.get(key), data.get("total_count")
    if not isinstance(items, list) or not isinstance(total, int) or not all(isinstance(i, dict) for i in items):
        raise Unproven("malformed JSON")
    if total != len(items) or len(items) >= PER_PAGE:
        raise Unproven(f"a page of {key} may be incomplete")
    return items


def ours(run: dict[str, Any], workflow_id: Any, repo: str) -> bool:
    """A push run of this repository's ci.yml (by id or exact path), from this repository."""
    return ((run.get("workflow_id") == workflow_id or run.get("path") == WORKFLOW) and run.get("event") == "push"
            and (run.get("repository") or {}).get("full_name") == repo
            and (run.get("head_repository") or {}).get("full_name") == repo)


def green_jobs(api: str, repo: str, token: str, run: dict[str, Any], job: str) -> int:
    """How many jobs named `job` the run has over every attempt; Unproven unless each one succeeded."""
    if run.get("status") != "completed":
        raise Unproven(f"a ci.yml run on C is {run.get('status')}")
    jobs = listed(get(f"{api}/repos/{repo}/actions/runs/{run.get('id')}/jobs?filter=all&per_page={PER_PAGE}",
                      token), "jobs")
    named = [j for j in jobs if j.get("name") == job]
    for item in named:
        if item.get("status") != "completed" or item.get("conclusion") != "success":
            raise Unproven(f"{job} on C is {item.get('status')}/{item.get('conclusion')}")
    return len(named)


def proven_on(api: str, repo: str, token: str, sha: str, job: str) -> None:
    """Unproven unless every ci.yml push run on `sha` has `job` green and at least one such job exists."""
    workflow_id = get(f"{api}/repos/{repo}/actions/workflows/ci.yml", token).get("id")
    if not isinstance(workflow_id, int):
        raise Unproven("malformed JSON")
    runs = listed(get(f"{api}/repos/{repo}/actions/runs?head_sha={sha}&per_page={PER_PAGE}", token),
                  "workflow_runs")
    count = 0
    for run in runs:
        if ours(run, workflow_id, repo):
            if run.get("head_sha") != sha:
                raise Unproven("a run answered for another commit")
            count += green_jobs(api, repo, token, run, job)
    if count == 0:
        raise Unproven(f"no {job} job in a ci.yml push run on C")


def decide(job: str, flag: str, script: str, env: dict[str, str]) -> str:
    """The one line this script prints."""
    try:
        repo, token = env.get("GITHUB_REPOSITORY", ""), env.get("GH_TOKEN", "")
        if not repo or not token:
            raise Unproven("GITHUB_REPOSITORY or GH_TOKEN is not set")
        rule = rule_of(script, flag)
        parent, _head = candidate()
        check_diff(parent, rule)
        proven_on(env.get("GITHUB_API_URL", "https://api.github.com"), repo, token, parent, job)
    except Unproven as why:
        return f"unproven: {why}"
    except Exception as err:  # noqa: BLE001  # reason: any surprise must leave the flag true, never fail the step
        return f"unproven: unexpected {type(err).__name__}"
    return f"proven={parent}"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--job", required=True, help="the job's exact name, e.g. Checks")
    parser.add_argument("--flag", required=True, help="the flag the step's rule sets: rust, npm or run")
    parser.add_argument("--step-script", required=True, help='the step\'s own script file ("$0")')
    try:
        args = parser.parse_args(argv)
    except SystemExit:
        print("unproven: bad arguments")
        return 0
    print(decide(args.job, args.flag, args.step_script, dict(os.environ)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
