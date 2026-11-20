"""Guard for deny.toml's RUSTSEC-2026-0173 ignore (T109, D-4707; SEC-021, PRIV-024).

An advisory id ignore covers the whole dependency graph, so this test pins the one path it was
granted for. It parses Cargo.lock (standard library only, no network, no database) and fails when:
- proc-macro-error2 is in the lock and any crate but i18n-embed-fl depends on it;
- i18n-embed-fl is in the lock and any crate but age depends on it;
- crypto-bigint below 0.7.6 is in the lock (RUSTSEC-2026-0387's unpatched range, never ignored);
- rsa is in the lock at any version (only age's forbidden `ssh` feature brings it; RUSTSEC-2023-0071).
A crate absent from the lock passes. Each rule has a planted failing and a planted passing lock below.

Run: python3 -m pytest tests/test_age_lock_guard.py -q -p no:cacheprovider
"""

from __future__ import annotations

import re
from pathlib import Path

import pytest

LOCK = Path(__file__).resolve().parent.parent / "Cargo.lock"
ONLY_DEPENDENT = {"proc-macro-error2": "i18n-embed-fl", "i18n-embed-fl": "age"}
CRYPTO_BIGINT_PATCHED = (0, 7, 6)


def packages(lock: str) -> list[dict[str, object]]:
    """Each [[package]] block's name, version and dependency names."""
    found = []
    for block in lock.split("[[package]]")[1:]:
        name = re.search(r'^name = "([^"]+)"', block, re.MULTILINE)
        version = re.search(r'^version = "([^"]+)"', block, re.MULTILINE)
        deps = re.search(r"^dependencies = \[(.*?)\]", block, re.MULTILINE | re.DOTALL)
        names = [d.split()[0] for d in re.findall(r'"([^"]+)"', deps.group(1))] if deps else []
        if name and version:
            found.append({"name": name.group(1), "version": version.group(1), "deps": names})
    return found


def version_tuple(version: str) -> tuple[int, ...]:
    """The numeric release part of a version, `0.5.5` -> (0, 5, 5)."""
    return tuple(int(part) for part in re.findall(r"\d+", version.split("-")[0])[:3])


def problems(lock: str) -> list[str]:
    """Every rule the lock text breaks, one line each."""
    pkgs = packages(lock)
    names = {p["name"] for p in pkgs}
    found = []
    for crate, allowed in ONLY_DEPENDENT.items():
        if crate in names:
            others = sorted({p["name"] for p in pkgs if crate in p["deps"]} - {allowed})
            if others:
                found.append(f"{crate} is reached through {', '.join(others)}, not only {allowed}")
    for p in pkgs:
        if p["name"] == "crypto-bigint" and version_tuple(p["version"]) < CRYPTO_BIGINT_PATCHED:
            found.append(f"crypto-bigint {p['version']} is in RUSTSEC-2026-0387's unpatched range")
        if p["name"] == "rsa":
            found.append(f"rsa {p['version']} is in the lock (age's ssh feature is forbidden)")
    return found


def lock_of(*entries: tuple[str, str, list[str]]) -> str:
    """A small Cargo.lock text of (name, version, dependencies) entries."""
    out = ["version = 4\n"]
    for name, version, deps in entries:
        out.append(f'\n[[package]]\nname = "{name}"\nversion = "{version}"\n')
        if deps:
            out.append("dependencies = [\n" + "".join(f' "{d}",\n' for d in deps) + "]\n")
    return "".join(out)


AGE_PATH = (
    ("age", "0.11.5", ["i18n-embed-fl", "rand 0.8.5"]),
    ("i18n-embed-fl", "0.9.4", ["proc-macro-error2"]),
    ("proc-macro-error2", "2.0.1", []),
)


def test_the_workspace_lock_passes() -> None:
    assert problems(LOCK.read_text(encoding="utf-8")) == []


@pytest.mark.parametrize(
    ("lock", "expected"),
    [
        (lock_of(), []),
        (lock_of(("serde", "1.0.0", [])), []),
        (lock_of(*AGE_PATH), []),
        (lock_of(*AGE_PATH, ("other-macro", "1.0.0", ["proc-macro-error2"])),
         ["proc-macro-error2 is reached through other-macro, not only i18n-embed-fl"]),
        (lock_of(("proc-macro-error2", "2.0.1", [])), []),
        (lock_of(*AGE_PATH, ("tool", "1.0.0", ["i18n-embed-fl 0.9.4"])),
         ["i18n-embed-fl is reached through tool, not only age"]),
        (lock_of(("crypto-bigint", "0.5.5", [])),
         ["crypto-bigint 0.5.5 is in RUSTSEC-2026-0387's unpatched range"]),
        (lock_of(("crypto-bigint", "0.7.5", [])),
         ["crypto-bigint 0.7.5 is in RUSTSEC-2026-0387's unpatched range"]),
        (lock_of(("crypto-bigint", "0.7.6", [])), []),
        (lock_of(("crypto-bigint", "0.8.0-rc.1", [])), []),
        (lock_of(("rsa", "0.10.0", [])), ["rsa 0.10.0 is in the lock (age's ssh feature is forbidden)"]),
        (lock_of(("rsa", "0.9.8", [])), ["rsa 0.9.8 is in the lock (age's ssh feature is forbidden)"]),
    ],
)
def test_each_rule_fails_its_planted_case_and_passes_its_clean_one(lock: str, expected: list[str]) -> None:
    assert problems(lock) == expected
