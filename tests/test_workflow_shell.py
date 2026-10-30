"""Every `run:` step in `.github/workflows/` parses as bash (D-794).

The chain step's refill snippet is Python inside a single-quoted `python3 -c '...'`; one apostrophe
in a comment there ended the quote, and from D-723 on the step failed with a bash syntax error on
every run, so no freed lane was refilled. The snippet tests in tests/test_runner_two_lanes.py run
the Python alone and could not see it. Here each step's script goes through `bash -n` the way the
runner's shell reads it, with every `${{ ... }}` expression replaced by a plain word first.
"""

from __future__ import annotations

import re
import shutil
import subprocess
from pathlib import Path
from typing import Any

import pytest
import yaml

ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS = sorted([*(ROOT / ".github" / "workflows").glob("*.yml"), *(ROOT / ".github" / "workflows").glob("*.yaml")])
EXPRESSION = re.compile(r"\$\{\{.*?\}\}", re.DOTALL)


def _bash_steps() -> list[tuple[str, str]]:
    """(label, script) for every `run:` step whose shell is bash (the default on Linux runners)."""
    found: list[tuple[str, str]] = []
    for path in WORKFLOWS:
        doc: dict[str, Any] = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
        default = ((doc.get("defaults") or {}).get("run") or {}).get("shell", "bash")
        for job_id, job in (doc.get("jobs") or {}).items():
            job_shell = ((job.get("defaults") or {}).get("run") or {}).get("shell", default)
            for index, step in enumerate(job.get("steps") or []):
                if "run" in step and Path(str(step.get("shell", job_shell)).split()[0]).name == "bash":
                    label = f"{path.name}:{job_id}:{step.get('name', index)}"
                    found.append((label, str(step["run"])))
    return found


STEPS = _bash_steps()


def test_the_workflows_have_bash_steps_to_check() -> None:
    assert WORKFLOWS and STEPS
    assert any("Start the next cycle" in label for label, _ in STEPS)


@pytest.mark.skipif(shutil.which("bash") is None, reason="bash is not installed")
@pytest.mark.parametrize(("label", "script"), STEPS, ids=[label for label, _ in STEPS])
def test_every_run_step_parses_as_bash(label: str, script: str) -> None:
    plain = EXPRESSION.sub("EXPR", script)
    result = subprocess.run(["bash", "-n"], input=plain, capture_output=True, text=True, check=False)
    assert result.returncode == 0, f"{label}: {result.stderr.strip()}"


def test_the_check_catches_an_apostrophe_inside_a_single_quoted_snippet() -> None:
    broken = "x=\"$(python3 -c '\n# another lane's merge (D-723)\nprint(1)\n')\"\n"
    result = subprocess.run(["bash", "-n"], input=broken, capture_output=True, text=True, check=False)
    assert result.returncode != 0
