"""TD-OPS-30 (FEE-034, PRIV-022, LIC-033; D-694, D-1759, T264): a missing coverage measurement is red.

`scripts/coverage_floor.py` judged a floor only when the report measured it, so a built crate left out of the
report, a report under another path prefix or an empty report passed. These cases run its `judge`, `future`
and `built_future` over synthetic reports in a fake tree, and check the real floor file: no key for a path
that is not on disk, and a nonzero total taken from a named `Ci` run.
"""

from __future__ import annotations

import importlib.util
import json
import re
from pathlib import Path
from types import ModuleType
from typing import Any

import pytest

ROOT = Path(__file__).resolve().parent.parent


def _load() -> ModuleType:
    spec = importlib.util.spec_from_file_location("coverage_floor", ROOT / "scripts" / "coverage_floor.py")
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cf = _load()
FLOORS = {"total": 50.0, "floors": {"app/pay/": 95.0, "app/lic/": 95.0, "app/cat/src/rules.rs": 95.0},
          "future": [{"path": "app/cat/src/rules.rs", "reason": "fake: built by a later task"}]}


@pytest.fixture
def tree(tmp_path: Path) -> str:
    """A fake checkout where app/pay/ and app/lic/ are built and app/cat/src/rules.rs is not."""
    for crate in ("pay", "lic"):
        (tmp_path / "app" / crate / "src").mkdir(parents=True)
    return f"{tmp_path}/"


def _row(name: str, branches: tuple[int, int], lines: tuple[int, int] = (10, 10)) -> dict[str, Any]:
    return {"filename": name, "summary": {"branches": {"covered": branches[0], "count": branches[1]},
                                          "lines": {"covered": lines[0], "count": lines[1]}}}


def _report(rows: list[dict[str, Any]]) -> dict[str, Any]:
    totals = {k: {"covered": sum(r["summary"][k]["covered"] for r in rows),
                  "count": sum(r["summary"][k]["count"] for r in rows)} for k in ("branches", "lines")}
    return {"files": rows, "totals": totals}


def _full(root: str) -> dict[str, Any]:
    return _report([_row(f"{root}app/pay/src/a.rs", (96, 100)), _row(f"{root}app/lic/src/b.rs", (97, 100))])


def test_a_full_report_of_every_built_floor_passes(tree: str) -> None:
    assert cf.judge(FLOORS, _full(tree), tree) == []
    assert cf.future(FLOORS, tree) == []


def test_a_built_crate_absent_from_a_full_report_fails(tree: str) -> None:
    data = _report([_row(f"{tree}app/pay/src/a.rs", (96, 100))])
    assert cf.judge(FLOORS, data, tree) == [f"app/lic/ is not measured by the report (no lines under {tree}app/lic/)"]


def test_a_built_crate_measured_with_zero_lines_fails(tree: str) -> None:
    data = _full(tree)
    data["files"].append(_row(f"{tree}app/lic/src/c.rs", (0, 0), (0, 0)))
    data["files"][1] = _row(f"{tree}app/lic/src/b.rs", (0, 0), (0, 0))
    assert cf.judge(FLOORS, data, tree) == [f"app/lic/ is not measured by the report (no lines under {tree}app/lic/)"]


def test_a_report_under_the_wrong_path_prefix_fails(tree: str) -> None:
    failures = cf.judge(FLOORS, _full("/elsewhere/"), tree)
    assert failures == [f"{p} is not measured by the report (no lines under {tree}{p})" for p in ("app/pay/", "app/lic/")]


def test_an_empty_report_or_zero_total_lines_fails(tree: str) -> None:
    assert "the report measured nothing (no files or zero total lines)" in cf.judge(FLOORS, _report([]), tree)
    zero = _full(tree)
    zero["totals"]["lines"]["count"] = 0
    assert "the report measured nothing (no files or zero total lines)" in cf.judge(FLOORS, zero, tree)
    assert cf.judge(FLOORS, _report([]), tree, ["app/pay/"])[0].startswith("the report measured nothing")


def test_a_future_path_passes_only_with_a_reason_and_is_shown(tree: str, capsys: pytest.CaptureFixture[str]) -> None:
    assert cf.future(FLOORS, tree) == []
    assert "future: app/cat/src/rules.rs has no file yet (fake: built by a later task)" in capsys.readouterr().out
    unlisted = {**FLOORS, "future": []}
    no_reason = {**FLOORS, "future": [{"path": "app/cat/src/rules.rs", "reason": " "}]}
    for floors in (unlisted, no_reason):
        assert cf.future(floors, tree) == [
            f'app/cat/src/rules.rs has no file on disk and no reason in the "future" list of {cf.FLOOR_FILE}']


def test_a_future_entry_added_for_a_built_path_is_refused(tree: str) -> None:
    built = {**FLOORS, "future": [*FLOORS["future"], {"path": "app/lic/", "reason": "fake"}]}
    assert cf.built_future(FLOORS, built, Path(tree)) == ['a "future" entry was added for app/lic/, which exists on disk']
    assert cf.built_future({"total": 0.0, "floors": {}}, FLOORS, Path(tree)) == [], "a path not on disk may be added"


def test_a_part_with_no_branches_is_judged_by_its_lines(tree: str) -> None:
    data = _full(tree)
    data["files"][0] = _row(f"{tree}app/pay/src/a.rs", (0, 0), (9, 10))
    assert cf.judge(FLOORS, data, tree) == ["app/pay/ 90.00 is below the floor 95.0"]
    data["files"][0] = _row(f"{tree}app/pay/src/a.rs", (0, 0), (10, 10))
    assert cf.judge(FLOORS, data, tree) == []


def test_an_only_run_fails_when_a_selected_prefix_is_absent(tree: str) -> None:
    data = _report([_row(f"{tree}app/pay/src/a.rs", (96, 100))])
    assert cf.judge(FLOORS, data, tree, ["app/pay/"]) == [], "a per-crate run judges only its own floors"
    assert cf.judge(FLOORS, data, tree, ["app/pay/", "app/lic/"]) == [
        f"app/lic/ is not measured by the report (no lines under {tree}app/lic/)"]
    assert cf.judge(FLOORS, data, tree, ["app/nope/"]) == [f"--only app/nope/ is not a floor in {cf.FLOOR_FILE}"]


def test_the_real_floor_file_has_no_missing_path_and_a_measured_total() -> None:
    floors = json.loads((ROOT / cf.FLOOR_FILE).read_text(encoding="utf-8"))
    assert "app/catalog/src/rules.rs" not in floors["floors"], "the fee rules are app/payments/src/rules.rs (D-1759)"
    assert (ROOT / "app" / "payments" / "src" / "rules.rs").exists()
    assert cf.future(floors, f"{ROOT}/") == [] and floors["future"] == [], "every floored path is built"
    printed = re.fullmatch(r"Ci run (\d+) on main \(\w+\), job ci / Coverage, printed total: ([\d.]+); .*",
                           floors["total_from"])
    assert printed, "the total names the Ci run it was measured in"
    assert floors["total"] == int(float(printed.group(2))) - 2 > 0, "rounded down to a whole percent, less 2 points"
    decisions = (ROOT / "docs" / "decisions.md").read_text(encoding="utf-8")
    assert printed.group(1) in decisions and "coverage floor" in decisions


# TD-OPS-35 (FEE-034, PRIV-022, LIC-033; D-2209, D-3974, T324): per-file floors for the money and privacy
# files outside app/payments/, app/privacy/ and app/licensing/.
FILE_FLOORS = {"total": 0.0, "floors": {"app/pay/": 50.0, "app/web/src/release.rs": 95.0}, "future": []}


def test_a_per_file_floor_is_judged_on_that_files_branches_alone(tree: str) -> None:
    rows = [_row(f"{tree}app/pay/src/a.rs", (60, 100)), _row(f"{tree}app/web/src/release.rs", (19, 20)),
            _row(f"{tree}app/web/src/page.rs", (0, 100))]
    (Path(tree) / "app" / "web" / "src").mkdir(parents=True)
    assert cf.judge(FILE_FLOORS, _report(rows), tree) == [], "the 0% sibling file does not count against release.rs"
    rows[1] = _row(f"{tree}app/web/src/release.rs", (0, 0), (10, 10))
    assert cf.judge(FILE_FLOORS, _report(rows), tree) == [], "a file with no branches is judged by its lines"


def test_a_report_measuring_a_floored_file_below_its_floor_fails(tree: str) -> None:
    (Path(tree) / "app" / "web" / "src").mkdir(parents=True)
    rows = [_row(f"{tree}app/pay/src/a.rs", (60, 100)), _row(f"{tree}app/web/src/release.rs", (18, 20))]
    assert cf.judge(FILE_FLOORS, _report(rows), tree) == ["app/web/src/release.rs 90.00 is below the floor 95.0"]
    only = cf.judge(FILE_FLOORS, _report(rows), tree, ["app/web/src/release.rs"])
    assert only == ["app/web/src/release.rs 90.00 is below the floor 95.0"]


def _workspace(root: Path) -> None:
    """A fake workspace: `web` depends on `pay`, `site` on `web`, `other` on neither."""
    (root / "Cargo.toml").write_text('[workspace]\nmembers = ["app/*"]\n', encoding="utf-8")
    deps = {"pay": "", "web": 'askhava-pay = { path = "../pay" }', "site": 'askhava-web = { path = "../web" }',
            "other": ""}
    for crate, dep in deps.items():
        (root / "app" / crate / "src").mkdir(parents=True, exist_ok=True)
        manifest = f'[package]\nname = "askhava-{crate}"\n\n[dependencies]\n{dep}\n'
        (root / "app" / crate / "Cargo.toml").write_text(manifest, encoding="utf-8")
    (root / "app" / "web" / "src" / "release.rs").write_text("", encoding="utf-8")
    (root / "tests").mkdir()
    (root / cf.FLOOR_FILE).write_text(json.dumps({"floors": {"app/web/src/release.rs": 95.0}}), encoding="utf-8")


def test_select_maps_a_floored_file_to_its_crate_and_its_dependents(tmp_path: Path) -> None:
    _workspace(tmp_path)
    picked = (["askhava-site", "askhava-web"], ["app/web/src/release.rs"])
    assert cf.select(tmp_path, ["app/web/src/release.rs"]) == picked
    assert cf.select(tmp_path, ["app/web/src/page.rs"]) == picked, "a change elsewhere in the crate measures it too"
    assert cf.select(tmp_path, [cf.FLOOR_FILE]) == picked
    assert cf.select(tmp_path, ["app/pay/src/a.rs", "app/other/src/b.rs"]) == ([], [])


def test_every_per_file_floor_is_at_or_below_its_ci_measurement_on_main() -> None:
    floors = json.loads((ROOT / cf.FLOOR_FILE).read_text(encoding="utf-8"))
    files = {p: f for p, f in floors["floors"].items() if not p.endswith("/")}
    measured = floors["files_measured"]
    assert files and set(files) == set(measured), "each per-file floor names its measurement, and only those"
    assert re.match(r"cargo llvm-cov on main \(\w+\)", floors["files_from"]), "the measurement names its commit"
    review = (ROOT / "docs" / "for-casey-review.md").read_text(encoding="utf-8")
    decision = next(line for line in (ROOT / "docs" / "decisions.md").read_text(encoding="utf-8").splitlines()
                    if line.startswith("- **D-3974** "))
    for path, floor in files.items():
        assert (ROOT / path).is_file() and path in decision, path
        assert floor <= measured[path], f"{path}: floor {floor} above the measured {measured[path]}"
        assert floor == (95.0 if measured[path] >= 95 else int(measured[path]) - 2), f"{path}: D-1759's rule"
        assert floor == 95.0 or f"`{path}`" in review, f"{path}: a floor under 95 is listed for Casey"
