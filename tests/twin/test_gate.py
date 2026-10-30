"""Self-tests of twin/gate.py (D-107; docs/twin-warnings.md section 6.5) and of its license oracle (D-541).

Waiver logic is tested on synthetic instances; exit codes and the deliberate breaks of the twin's
rules run the real gate in a subprocess on a temporary copy of twin/ and docs/catalog/, so neither
ever changes. The oracle tests prove twin/oracle/license_scope.json is a fresh generation from the
catalog (CARD-001, LIC-001) and agrees with docs/catalog/matching_examples.yaml (D-110); the fee
tests pin the twin's introduction fee by job size (FEE-001, FEE-002, REC-001, REC-003; D-542).
"""

from __future__ import annotations

import importlib.util
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest
import yaml  # type: ignore[import-untyped]  # reason: PyYAML ships no stubs; only safe_load is used

ROOT = Path(__file__).resolve().parents[2]
TWIN = ROOT / "twin"
SPEC = importlib.util.spec_from_file_location("twin_gate", TWIN / "gate.py")
assert SPEC and SPEC.loader
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)

KNOWN = {"decisions": {"D-102", "D-106", "D-107"}, "checks": set(gate.REQUIRED_CHECKS), "scenarios": {"normal_fixed"}}


def pinned(ref: str = "request 1 (doors) not sent to X", **over: object) -> dict:
    entry = {
        "id": "WV-0001",
        "scenario": "normal_fixed",
        "seed": 1,
        "check": "qualified_business_left_out",
        "ref": ref,
        "cause": "C1",
        "decision": "D-102",
        "until": "T007",
    }
    return {**entry, **over}


def klass(ceiling: int = 1) -> dict:
    return {
        "id": "WV-HO-ANSWER",
        "checks": list(gate.MATCHING),
        "evidence": "homeowner_answered_wrong",
        "decision": "D-106",
        "max_per_seed": {"normal_fixed": ceiling},
    }


def instance(ref: str = "request 1 (doors) not sent to X", evidence: frozenset = frozenset()) -> dict:
    return {
        "scenario": "normal_fixed",
        "seed": 1,
        "check": "qualified_business_left_out",
        "ref": ref,
        "evidence": set(evidence),
    }


def judge(instances: list[dict], waivers: dict, no_pinned: bool = False) -> int:
    result = gate.apply_waivers(instances, waivers, {("normal_fixed", 1)}, no_pinned)
    return gate.verdict(result, [])


def test_instance_with_no_waiver_exits_1() -> None:
    assert judge([instance()], dict(version=1, **{"class": []}, pinned=[])) == 1


def test_pinned_entry_waives_exactly_one_instance() -> None:
    waivers = dict(version=1, **{"class": []}, pinned=[pinned()])
    assert judge([instance()], waivers) == 0
    assert judge([instance(), instance()], waivers) == 1
    assert judge([instance()], waivers, no_pinned=True) == 1


def test_stale_pinned_entry_exits_2() -> None:
    assert judge([], dict(version=1, **{"class": []}, pinned=[pinned()])) == 2


def test_stale_entry_for_a_scenario_not_run_is_ignored() -> None:
    result = gate.apply_waivers(
        [], dict(version=1, **{"class": []}, pinned=[pinned(seed=3)]), {("normal_fixed", 1)}, False
    )
    assert result["stale"] == []


def test_class_waiver_needs_evidence_per_instance() -> None:
    waivers = dict(version=1, **{"class": [klass()]}, pinned=[])
    assert judge([instance()], waivers) == 1
    assert judge([instance(evidence=frozenset({"homeowner_answered_wrong"}))], waivers) == 0


def test_class_ceiling_per_seed_exits_1() -> None:
    waivers = dict(version=1, **{"class": [klass(ceiling=1)]}, pinned=[])
    both = [instance(f"request {n} (doors) not sent to X", frozenset({"homeowner_answered_wrong"})) for n in (1, 2)]
    assert judge(both, waivers) == 1


def test_negative_control_that_reports_nothing_exits_1() -> None:
    class QuietTwin:
        """A stand-in twin whose scenario reports nothing."""

        @staticmethod
        def deep_update(a: dict, b: dict) -> dict:
            return a

        @staticmethod
        def build(cfg: dict, seed: int) -> object:
            return type("Sim", (), {"run": lambda self, days: None})()

        @staticmethod
        def check(sim: object) -> list:
            return []

    missing = gate.control_missing(QuietTwin(), ("normal_fixed", 1, gate.MATCHING))
    assert missing == list(gate.MATCHING)
    assert gate.verdict({"unwaived": [], "stale": [], "over_ceiling": []}, missing) == 1


@pytest.mark.parametrize(
    "change, message",
    [
        ({"decision": "D-999"}, "not in docs/decisions.md"),
        ({"check": "no_such_check"}, "unknown check key"),
        ({"cause": "C9"}, "unknown cause"),
        ({"extra": 1}, "malformed"),
        ({"seed": 0}, "seed outside 1-5"),
        ({"seed": 6}, "seed outside 1-5"),
        ({"seed": True}, "seed outside 1-5"),
    ],
)
def test_bad_pinned_entry_is_a_waiver_error(change: dict, message: str) -> None:
    with pytest.raises(gate.GateError, match=message):
        gate.validate_waivers(dict(version=1, **{"class": []}, pinned=[pinned(**change)]), KNOWN)


def test_bad_class_entry_is_a_waiver_error() -> None:
    with pytest.raises(gate.GateError, match="unknown evidence"):
        gate.validate_waivers(dict(version=1, **{"class": [{**klass(), "evidence": "trust_me"}]}, pinned=[]), KNOWN)
    with pytest.raises(gate.GateError, match="max_per_seed"):
        gate.validate_waivers(
            dict(version=1, **{"class": [{**klass(), "max_per_seed": {"normal_fixed": 1.5}}]}, pinned=[]), KNOWN
        )


def test_committed_waiver_file_is_valid_and_names_known_causes() -> None:
    names = [n for n in json.loads((TWIN / "twin_scenarios.json").read_text()) if n.endswith("_fixed")]
    known = {
        "decisions": gate.decision_ids(gate.DECISIONS),
        "checks": gate.declared_checks(TWIN),
        "scenarios": set(names),
    }
    data = gate.validate_waivers(json.loads((TWIN / "waivers.json").read_text()), known)
    assert data["class"] and not data["pinned"]  # stage 1 is done only with no pinned entry left (D-544)


def test_decision_missing_from_the_log_next_to_the_twin_exits_2(tmp_path: Path) -> None:
    twin_dir = copy_twin(tmp_path)
    log = tmp_path / "docs" / "decisions.md"
    # D-106: a decision the committed pins still name (T005 removed every D-102 pin; D-517)
    log.write_text(log.read_text().replace("- **D-106**", "- D-106"))
    out = run_gate(twin_dir)
    assert out.returncode == 2 and "D-106" in out.stdout


def test_twin_declares_every_required_check() -> None:
    assert gate.REQUIRED_CHECKS <= gate.declared_checks(TWIN)
    assert len(gate.REQUIRED_CHECKS) == 57


def copy_twin(tmp_path: Path) -> Path:
    dest = tmp_path / "twin"
    shutil.copytree(TWIN, dest, ignore=shutil.ignore_patterns("__pycache__", "results"))
    shutil.copytree(ROOT / "docs" / "catalog", tmp_path / "docs" / "catalog")  # the oracle's inputs (D-541)
    shutil.copy(ROOT / "docs" / "decisions.md", tmp_path / "docs" / "decisions.md")  # the gate reads ../docs only
    return dest


def run_gate(twin_dir: Path, *args: str, seed: str | None = "0") -> subprocess.CompletedProcess:
    env = {k: v for k, v in os.environ.items() if k != "PYTHONHASHSEED"}
    if seed is not None:
        env["PYTHONHASHSEED"] = seed
    cmd = [
        sys.executable,
        str(twin_dir / "gate.py"),
        "--report",
        str(twin_dir.parent / "report.json"),
        *args,
    ]
    return subprocess.run(cmd, cwd=twin_dir.parent, env=env, capture_output=True, text=True, timeout=600, check=False)


@pytest.mark.parametrize("seed", [None, "12345"])
def test_hash_seed_not_zero_exits_2(tmp_path: Path, seed: str | None) -> None:
    assert run_gate(copy_twin(tmp_path), seed=seed).returncode == 2


def test_required_check_removed_from_twin_exits_2(tmp_path: Path) -> None:
    twin_dir = copy_twin(tmp_path)
    source = (twin_dir / "twin.py").read_text()
    (twin_dir / "twin.py").write_text(source.replace('bad("double_charge"', 'bad("charged_twice"'))
    out = run_gate(twin_dir)
    assert out.returncode == 2 and "double_charge" in out.stdout


def test_unchanged_twin_passes_on_one_scenario(tmp_path: Path) -> None:
    out = run_gate(copy_twin(tmp_path), "--scenarios", "normal_fixed", "--seeds", "1")
    assert out.returncode == 0, out.stdout[-2000:]


def test_lowered_class_ceiling_turns_the_gate_red(tmp_path: Path) -> None:
    twin_dir = copy_twin(tmp_path)
    data = json.loads((twin_dir / "waivers.json").read_text())
    entry = next(e for e in data["class"] if e["id"] == "WV-HO-ANSWER")
    entry["max_per_seed"]["normal_fixed"] = 0
    (twin_dir / "waivers.json").write_text(json.dumps(data))
    out = run_gate(twin_dir, "--scenarios", "normal_fixed", "--seeds", "2")  # seed 2 has wrong homeowner answers
    assert out.returncode == 1 and "CEILING exceeded WV-HO-ANSWER normal_fixed seed 2" in out.stdout


def test_stale_pinned_entry_in_a_real_run_exits_2(tmp_path: Path) -> None:
    twin_dir = copy_twin(tmp_path)
    data = json.loads((twin_dir / "waivers.json").read_text())
    data["pinned"] = [pinned(ref="request 9999 (doors) not sent to Nobody", until="never")]
    (twin_dir / "waivers.json").write_text(json.dumps(data))
    out = run_gate(twin_dir, "--scenarios", "normal_fixed", "--seeds", "1")
    assert out.returncode == 2 and "STALE pinned waiver WV-0001" in out.stdout


def oracle() -> dict:
    return json.loads((TWIN / "oracle" / "license_scope.json").read_text())


def examples() -> list[dict]:
    return yaml.safe_load((ROOT / "docs" / "catalog" / "matching_examples.yaml").read_text())["examples"]


def test_license_oracle_is_a_fresh_generation_from_the_catalog() -> None:
    cmd = [sys.executable, str(TWIN / "oracle" / "gen_license_scope.py"), "--check"]
    out = subprocess.run(cmd, capture_output=True, text=True, timeout=60, check=False)
    assert out.returncode == 0, out.stdout
    assert gate.oracle_problems(oracle(), ROOT) == []


def test_oracle_agrees_with_every_matching_example_it_can_express() -> None:
    expressed = [e for e in examples() if [*e["business"]["roc"], *e["business"]["agencies"]]]
    assert len(expressed) >= 9 and all(n in oracle()["grants"] for e in expressed for n in e["business"]["roc"])
    assert gate.self_test_problems(oracle(), examples()) == []


def test_oracle_that_disagrees_with_an_example_fails_the_self_test() -> None:
    broken = oracle()
    broken["grants"]["R-39R Air Conditioning and Refrigeration"]["hvac_repair"].append("furnace repair")
    problems = gate.self_test_problems(broken, examples())
    assert [p.split(":")[0] for p in problems] == ["M02", "M15"]


def test_oracle_matches_the_twins_license_classes() -> None:
    twin_scope = oracle()["twin"]
    assert "heating_repair" not in twin_scope["R-39R-ac"] and "heating_repair" in twin_scope["R-39"]
    assert twin_scope["R-37R-drains"] == ["drain_sewer"] and "water_heater" in twin_scope["R-37"]


def test_stale_oracle_exits_2(tmp_path: Path) -> None:
    twin_dir = copy_twin(tmp_path)
    lmap = tmp_path / "docs" / "catalog" / "license_map.yaml"
    lmap.write_text(lmap.read_text() + "# a catalog change with no regenerated oracle\n")
    out = run_gate(twin_dir, "--scenarios", "normal_fixed", "--seeds", "1")
    assert out.returncode == 2 and "stale" in out.stdout


def test_twin_reading_the_apps_license_table_exits_2(tmp_path: Path) -> None:
    twin_dir = copy_twin(tmp_path)
    replace_in(twin_dir, "twin.py", "from app import App,", "from app import CLASS_ALLOWS, App,")
    out = run_gate(twin_dir, "--scenarios", "normal_fixed", "--seeds", "1")
    assert out.returncode == 2 and "CLASS_ALLOWS" in out.stdout


def fee_functions() -> list:
    sys.path.insert(0, str(TWIN))
    import app as twin_app
    import twin as twin_sim

    return [twin_app.fee_by_job_size, twin_sim.fee_by_job_size]


@pytest.mark.parametrize(
    "quote, fee",
    [
        ({"type": "firm", "amount": 499.99}, 25),
        ({"type": "firm", "amount": 500}, 75),
        ({"type": "firm", "amount": 1500}, 75),
        ({"type": "firm", "amount": 1500.01}, 199),
        ({"type": "firm", "amount": 9000}, 199),
        ({"type": "range", "amount": 400, "high": 700}, 75),
        ({"type": "range", "amount": 1400, "high": 1700}, 199),
        ({"type": "trip", "amount": 95}, 25),
        ({"type": "firm", "amount": 150, "period": "month"}, 75),
        ({"type": "firm", "amount": 124.99, "period": "month"}, 25),
    ],
)
def test_fee_by_job_size_in_the_app_and_the_oracle(quote: dict, fee: int) -> None:
    """FEE-001, FEE-002 (fixed tiers, never a percentage), FEE-003, FEE-004, REC-001, REC-003 ($150 a month: $75)."""
    assert [f(quote) for f in fee_functions()] == [fee, fee]


def replace_in(twin_dir: Path, name: str, old: str, new: str) -> None:
    text = (twin_dir / name).read_text()
    assert old in text, f"break no longer applies to {name}: {old!r}"
    (twin_dir / name).write_text(text.replace(old, new))


def no_job_matching(twin_dir: Path) -> None:
    path = twin_dir / "twin_scenarios.json"
    scenarios = json.loads(path.read_text())
    scenarios["normal_fixed"]["overrides"].setdefault("app", {})["job_matching"] = False
    path.write_text(json.dumps(scenarios))


CHARGE = '        res = self.sim.stripe.charge_off_session(c["stripe"], p["fee"], idem_key=key,'
TO_PENDING = '        if r["status"] == "AWAITING_CONFIRM":\n            self.set_status(r, "PAYMENT_PENDING")\n'
BREAKS = {
    "license check at card time removed": (
        lambda d: (
            replace_in(
                d, "app.py", "    def license_ok(self, co):\n", "    def license_ok(self, co):\n        return True\n"
            ),
            replace_in(
                d,
                "app.py",
                '> 12 * 60:\n                    self.check_license(c["id"])',
                "> 12 * 60:\n                    pass",
            ),
        ),
        "vendor_chaos_fixed",  # D-517: its seed 2 has a lapsing license; normal_fixed seeds 1-2 no longer do
        "2",
        "job_card_to_unlicensed_contractor",
    ),
    "job-type matching off": (no_job_matching, "normal_fixed", "1", "job_card_outside_license"),
    "AC-only sub-license given heating repair": (  # LIC-035: the break the old twin could not see (section 2)
        lambda d: replace_in(
            d,
            "app.py",
            '"R-39R-ac": {"ac_repair", "thermostat", "system_replacement"}',
            '"R-39R-ac": {"ac_repair", "heating_repair", "thermostat", "system_replacement"}',
        ),
        "normal_fixed",
        "1",
        "job_card_outside_license",
    ),
    "opt-out check removed from sending": (
        lambda d: replace_in(d, "app.py", 'if phone in self.stopped and kind not in ("stop_confirm",):', "if False:"),
        "busy_fixed",
        "1",  # D-544: after T007's changes seed 1 of busy_fixed is where a text reaches an opted-out homeowner
        "text_after_stop",
    ),
    "quiet-hours check removed": (
        lambda d: replace_in(
            d,
            "app.py",
            'if kind not in ("safety", "stop_confirm", "pick_code") and not conversational and not self.in_business():',
            "if False:",
        ),
        "normal_fixed",
        "1",
        "text_in_quiet_hours",
    ),
    "charge retried under a fresh idempotency key": (
        lambda d: replace_in(
            d,
            "app.py",
            CHARGE,
            CHARGE.replace("res = ", "").replace(
                "idem_key=key,", 'idem_key=key + "-retry", meta=dict(pick=p["id"], req=r["id"]))'
            )
            + "\n"
            + CHARGE,
        ),
        "normal_fixed",
        "1",
        "double_charge",
    ),
    "details released before payment succeeds": (
        lambda d: replace_in(d, "app.py", TO_PENDING, TO_PENDING + "        self.release(p)\n"),
        "normal_fixed",
        "1",
        "details_released_wrongly",
    ),
}


@pytest.mark.parametrize("name", sorted(BREAKS))
def test_deliberate_break_of_a_twin_rule_exits_1(tmp_path: Path, name: str) -> None:
    breaker, scenario, seeds, expected = BREAKS[name]
    twin_dir = copy_twin(tmp_path)
    breaker(twin_dir)
    out = run_gate(twin_dir, "--scenarios", scenario, "--seeds", seeds)
    assert out.returncode == 1
    assert any(line.startswith("UNWAIVED") and f" {expected}:" in line for line in out.stdout.splitlines()), out.stdout[
        -2000:
    ]
