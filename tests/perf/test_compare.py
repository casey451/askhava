"""Offline checks of Ci's performance report (T418, TD-OPS-43, D-3292): planted bench, vitals and
baseline files; no app, no browser. A non-fake-looking value is assembled at run time, never stored."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))

from perf import compare


def bench(p95: float, verdict: str = "met") -> dict:
    entry = {"p95_ms": p95, "mean_queries": 12.5, "verdicts": {"error_rate_under_0.1%": verdict}}
    return {"routes": {"quote_page": {"1": entry}, "home": "not exposed: no HTTP route on main"}}


def vitals(lcp: float = 900.0) -> dict:
    return {"label": "lab, not field", "pages": {"quote_page": {"lcp_ms": lcp, "inp_ms": 40.0, "cls": 0.0}}}


def run(tmp_path: Path, monkeypatch, capsys, bench_data, vitals_data, baseline) -> tuple[int, str, list[str]]:
    """Runs the report on planted files; returns (exit code, step summary, warning lines)."""
    files = {}
    for name, data in (("bench", bench_data), ("vitals", vitals_data), ("baseline", baseline)):
        path = tmp_path / f"{name}.json"
        if data is not None:
            path.write_text(data if isinstance(data, str) else json.dumps(data), encoding="utf-8")
        files[name] = path
    summary = tmp_path / "summary.md"
    monkeypatch.setenv("GITHUB_STEP_SUMMARY", str(summary))
    code = compare.main(["--bench", str(files["bench"]), "--vitals", str(files["vitals"]), "--baseline", str(files["baseline"])])
    out = capsys.readouterr().out
    return code, summary.read_text(encoding="utf-8"), [line for line in out.splitlines() if line.startswith("::warning")]


def stored(p95: float) -> dict:
    return {"routes": {"quote_page": {"1": {"p95_ms": p95}}}, "vitals": {"quote_page": {"lcp_ms": 1000.0}}}


def test_td_ops_43_p95_25_percent_above_baseline_gives_no_warning(tmp_path, monkeypatch, capsys) -> None:
    code, summary, warnings = run(tmp_path, monkeypatch, capsys, bench(125.0), vitals(), stored(100.0))
    assert (code, warnings) == (0, [])
    assert "| quote_page | 1 | 125 | 100 | +25.0% | 12.5 |" in summary


def test_td_ops_43_p95_26_percent_above_baseline_gives_one_warning(tmp_path, monkeypatch, capsys) -> None:
    code, _, warnings = run(tmp_path, monkeypatch, capsys, bench(126.0), vitals(), stored(100.0))
    assert code == 0
    assert len(warnings) == 1 and "quote_page at 1 users: p95 126 ms" in warnings[0]


@pytest.mark.parametrize(("bench_data", "vitals_data", "says"), [
    (bench(10.0, "missed"), vitals(), "goal error_rate_under_0.1% missed"),
    (bench(10.0), vitals(lcp=2600.0), "LCP ms 2600 (lab, not field) misses the goal 2500"),
])
def test_td_ops_43_a_missed_goal_gives_one_warning(tmp_path, monkeypatch, capsys, bench_data, vitals_data, says) -> None:
    code, _, warnings = run(tmp_path, monkeypatch, capsys, bench_data, vitals_data, stored(10.0))
    assert code == 0
    assert len(warnings) == 1 and says in warnings[0]


@pytest.mark.parametrize(("baseline", "says"), [
    (None, "No stored baseline: nothing compared."),
    ({"routes": {}, "vitals": {}}, "The stored baseline is empty: nothing compared."),
])
def test_td_ops_43_a_missing_or_empty_baseline_compares_nothing(tmp_path, monkeypatch, capsys, baseline, says) -> None:
    code, summary, warnings = run(tmp_path, monkeypatch, capsys, bench(500.0), vitals(), baseline)
    assert (code, warnings) == (0, [])
    assert says in summary and "| quote_page | 1 | 500 | none | no baseline | 12.5 |" in summary


@pytest.mark.parametrize("which", ["bench", "vitals", "baseline"])
def test_td_ops_43_a_malformed_input_exits_0_with_one_warning(tmp_path, monkeypatch, capsys, which) -> None:
    inputs = {"bench": bench(10.0), "vitals": vitals(), "baseline": stored(10.0), which: "{not json"}
    code, summary, warnings = run(tmp_path, monkeypatch, capsys, inputs["bench"], inputs["vitals"], inputs["baseline"])
    assert code == 0
    assert len(warnings) == 1 and "not readable JSON" in warnings[0]
    assert "## Performance" in summary


def test_td_ops_43_the_summary_holds_only_figures_names_and_goals(tmp_path, monkeypatch, capsys) -> None:
    phone = "+1" + "928" + "4" + "41" + "7302"
    planted = bench(10.0)
    planted["routes"]["home"] = f"not exposed: caller {phone}"
    planted["routes"][f"route {phone}"] = {"1": {"p95_ms": 5.0}}
    planted["routes"]["quote_page"]["1"]["note"] = phone
    page = vitals()
    page["pages"]["home"] = f"not exposed: {phone}"
    page["pages"]["quote_page"]["inp_ms"] = phone
    code, summary, warnings = run(tmp_path, monkeypatch, capsys, planted, page, stored(10.0))
    assert code == 0 and phone not in summary and phone not in "".join(warnings)
    assert "| home | - | not measured |" in summary
    assert "| quote_page | INP ms (lab stand-in) (lab, not field) | not measured | 200 | 150 |" in summary


def test_td_ops_43_every_vital_is_labelled_lab_not_field(tmp_path, monkeypatch, capsys) -> None:
    _, summary, _ = run(tmp_path, monkeypatch, capsys, bench(10.0), vitals(), None)
    vital_rows = [line for line in summary.splitlines() if line.count("|") == 8 and not line.startswith(("| Page", "|---"))]
    assert len(vital_rows) == 3 and all("(lab, not field)" in line for line in vital_rows)
    assert "### Core Web Vitals (lab, not field)" in summary


def test_td_ops_43_write_baseline_keeps_figures_alone(tmp_path, monkeypatch) -> None:
    for name, data in (("b", bench(42.0)), ("v", vitals())):
        (tmp_path / f"{name}.json").write_text(json.dumps(data), encoding="utf-8")
    monkeypatch.delenv("GITHUB_STEP_SUMMARY", raising=False)
    out = tmp_path / "base.json"
    args = ["--bench", str(tmp_path / "b.json"), "--vitals", str(tmp_path / "v.json"), "--baseline", str(tmp_path / "none.json")]
    assert compare.main([*args, "--write-baseline", str(out)]) == 0
    written = json.loads(out.read_text(encoding="utf-8"))
    assert written["routes"] == {"quote_page": {"1": {"p95_ms": 42.0, "mean_queries": 12.5}}}
    assert written["vitals"] == {"quote_page": {"lcp_ms": 900.0, "inp_ms": 40.0, "cls": 0.0}}


def test_td_ops_43_the_stored_ci_baseline_is_readable_and_never_fails(tmp_path) -> None:
    baseline = json.loads((ROOT / "docs/audit/perf-baseline-ci.json").read_text(encoding="utf-8"))
    assert isinstance(baseline["routes"], dict) and isinstance(baseline["vitals"], dict)
    summary = tmp_path / "summary"
    summary.mkdir()
    done = subprocess.run([sys.executable, str(ROOT / "perf/compare.py"), "--bench", str(tmp_path / "absent.json")],
                          capture_output=True, text=True, check=False, env={"GITHUB_STEP_SUMMARY": str(summary)})
    assert done.returncode == 0
    assert done.stdout.count("::warning") == 1 and "the report stopped early (IsADirectoryError)" in done.stdout
