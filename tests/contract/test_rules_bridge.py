"""The Python side of the rules_bridge contract (D-481, D-491, T074).

Every request the twin's adapters (`twin/rules_bridge.py`) send and every answer they read must fit
`app/server/rules_bridge.schema.json`, judged by the same schema subset the Rust side uses
(`app/server/tests/rules_bridge_contract.rs`); no JSON-schema package is needed. The live cases need the binary:
`ASKHAVA_RULES_BRIDGE` (twin.yml sets it after `cargo build -p askhava-server --bin rules_bridge --locked`), else
`target/debug/rules_bridge`. Ci's pytest step runs before any Rust build, so with neither the live cases skip;
with the variable set and no binary they fail. The schema and validator cases always run.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = json.loads((ROOT / "app" / "server" / "rules_bridge.schema.json").read_text(encoding="utf-8"))
sys.path.insert(0, str(ROOT / "twin"))
import rules_bridge as RB  # noqa: E402  # the twin's adapters, from twin/
import twin as twin_sim  # noqa: E402  # the oracle's fee

TYPES = {"object": dict, "array": list, "string": str, "boolean": bool, "null": type(None)}


def has_type(value: object, name: str) -> bool:
    if name == "integer":
        return isinstance(value, int) and not isinstance(value, bool)
    return isinstance(value, TYPES[name])


def problems(value: object, schema: dict, at: str = "$") -> list[str]:
    """The schema-subset problems of `value`: $ref, type, enum, required, properties, additionalProperties false,
    items, minimum, minItems, maxItems."""
    if "$ref" in schema:
        node: object = SCHEMA
        for part in schema["$ref"].removeprefix("#/").split("/"):
            node = node[part]  # type: ignore[index]
        return problems(value, node, at)  # type: ignore[arg-type]
    out = []
    types = schema.get("type")
    types = [types] if isinstance(types, str) else types or []
    if types and not any(has_type(value, t) for t in types):
        out.append(f"{at}: not {types}")
    if "enum" in schema and value not in schema["enum"]:
        out.append(f"{at}: not one of {schema['enum']}")
    if "minimum" in schema and has_type(value, "integer") and value < schema["minimum"]:  # type: ignore[operator]
        out.append(f"{at}: below the minimum")
    if isinstance(value, dict):
        props = schema.get("properties", {})
        out += [f"{at}: missing {k}" for k in schema.get("required", []) if k not in value]
        for k, v in value.items():
            if k in props:
                out += problems(v, props[k], f"{at}.{k}")
            elif schema.get("additionalProperties") is False:
                out.append(f"{at}: unexpected {k}")
    if isinstance(value, list):
        if len(value) < schema.get("minItems", 0) or len(value) > schema.get("maxItems", len(value)):
            out.append(f"{at}: item count")
        out += [p for i, item in enumerate(value) for p in problems(item, schema.get("items", {}), f"{at}[{i}]")]
    return out


def binary() -> str:
    path = os.environ.get("ASKHAVA_RULES_BRIDGE") or str(ROOT / "target" / "debug" / "rules_bridge")
    if not Path(path).is_file():
        if os.environ.get("ASKHAVA_RULES_BRIDGE"):
            pytest.fail(f"ASKHAVA_RULES_BRIDGE names no binary: {path}")
        pytest.skip("no rules_bridge binary built yet (twin.yml builds it and runs this file)")
    return path


class Checked(RB.RulesBridge):
    """The twin's bridge with every line checked against the schema."""

    def call(self, call: str, **args: object) -> dict:
        request = {"id": self.n + 1, "call": call, "args": args}
        defs = SCHEMA["$defs"]
        assert problems(request, defs["request"]) + problems(args, defs["calls"][call]["args"]) == []
        result = super().call(call, **args)
        envelope = {"ok": {"id": self.n, "result": result}}
        assert problems(envelope, defs["ok"]) + problems(result, defs["calls"][call]["result"]) == []
        return result


@pytest.fixture(name="bridge")
def fixture_bridge():
    b = Checked(binary())
    yield b
    assert b.close() == 0


def test_schema_names_args_and_result_for_every_call() -> None:
    calls = SCHEMA["$defs"]["calls"]
    assert sorted(SCHEMA["$defs"]["request"]["properties"]["call"]["enum"]) == sorted(calls)
    assert all(set(c) == {"args", "result"} for c in calls.values())


def test_validator_refuses_off_schema_lines() -> None:
    defs = SCHEMA["$defs"]
    assert problems({"id": 1, "call": "fee", "args": {}}, defs["request"]) == []
    assert problems({"id": -1, "call": "fee", "args": {}}, defs["request"]) != []
    assert problems({"id": 1, "call": "x", "args": {}, "more": 1}, defs["request"]) != []
    assert problems({"cents": 14900, "basis": None}, defs["calls"]["fee"]["result"]) != []
    assert problems({"job_types": []}, defs["calls"]["job_types"]["result"]) != []


@pytest.mark.parametrize(
    "quote",
    [
        {"type": "firm", "amount": 499.99},
        {"type": "firm", "amount": 500},
        {"type": "firm", "amount": 1500.01},
        {"type": "range", "amount": 400, "high": 700},
        {"type": "trip", "amount": 95},
        {"type": "firm", "amount": 150, "period": "month"},
        {"type": "firm", "amount": 124.99, "period": "month"},
        {"type": "firm", "amount": 375.01, "period": "month"},
    ],
)
def test_fee_through_the_bridge_is_the_oracles(bridge: Checked, quote: dict) -> None:
    """FEE-001, FEE-003, FEE-004, REC-002: the app's fee equals the oracle's ($150 a month: $75)."""
    assert RB.fee(bridge, quote) == twin_sim.fee_by_job_size(quote)


def test_quiet_hours_through_the_bridge(bridge: Checked) -> None:
    """QH-001, QH-006: sim 19:59 goes, 20:00 waits for 8:00 the next morning."""
    assert RB.quiet(bridge, 19 * 60 + 59) == (True, 19 * 60 + 59)
    assert RB.quiet(bridge, 20 * 60) == (False, 1440 + 8 * 60)


def test_status_moves_and_cards_through_the_bridge(bridge: Checked) -> None:
    """JOB-002 and CARD-001: the twin's moves under the app's arrows; an AC-only license gets AC work only."""
    assert RB.move_allowed(bridge, "OPEN", "QUOTES_READY")
    assert RB.move_allowed(bridge, "QUOTES_READY", "AWAITING_YES")
    assert not RB.move_allowed(bridge, "OPEN", "DONE")
    co = {"license_class": "R-39R-ac", "checklist": {"ac_repair", "heating_repair"}}
    assert RB.card(bridge, co, ["ac_repair"])
    assert not RB.card(bridge, co, ["heating_repair"])
    assert RB.card(bridge, {"license_class": "HANDYMAN", "checklist": {"drywall"}}, ["drywall"])


def test_envelope_errors_fit_the_schema() -> None:
    lines = ["not json", '{"id": 1, "call": "fee"}', '{"id": 2, "call": "none", "args": {}}']
    out = subprocess.run([binary()], input="\n".join(lines) + "\n", capture_output=True, text=True, check=True)
    answers = [json.loads(line) for line in out.stdout.splitlines()]
    assert [a["error"]["code"] for a in answers] == ["bad_json", "bad_request", "unknown_call"]
    assert all(problems(a, SCHEMA["$defs"]["error"]) == [] for a in answers)
