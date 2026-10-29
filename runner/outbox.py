"""Outbox messages and owner gates (kit/outbox_format.md, BUILD_BRIEF.md:226-232, 531-556)."""

from __future__ import annotations

import re
from datetime import datetime
from pathlib import Path
from typing import Any

from runner.state import WAITING_FOR_OWNER, State, entry, with_task

KINDS = ("question", "approval", "gate", "blocked", "info")
ID_PATTERN = re.compile(r"^[A-Za-z0-9._-]+$")
GATE_P = "GATE-P"
GATE_P_PHRASE = "approved to build"
DEFAULT_WORDS = ("go", "yes", "approved", "done")  # gates that name no quoted phrase


def write_outbox(directory: Path, msg_id: str, title: str, kind: str, body: str) -> Path:
    if not ID_PATTERN.match(msg_id):
        raise ValueError(f"bad outbox id {msg_id!r}")
    if kind not in KINDS:
        raise ValueError(f"bad outbox kind {kind!r}")
    if "\n" in title or not title.strip():
        raise ValueError("title must be one non-empty line")
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"{msg_id}.md"
    path.write_text(f"---\nid: {msg_id}\ntitle: {title}\nkind: {kind}\n---\n{body}\n", encoding="utf-8")
    return path


def open_gate(state: State, gate: dict[str, Any], body: str, outbox_dir: Path) -> State:
    """Stop at an owner gate: mark it waiting and write the 'Your turn' message.

    Returns the new state; the caller commits it with the outbox file and then exits.
    """
    spec = gate["gate"]
    title = spec.get("outbox") or f"Your turn: {spec['kind']}"
    write_outbox(outbox_dir, gate["id"], title, "gate", body)
    waits = spec.get("waits_on_owner", True)
    return with_task(state, gate["id"], status=WAITING_FOR_OWNER if waits else "DONE")


def resume_phrase(gate: dict[str, Any]) -> str:
    """The quoted phrase that clears a gate, or '' when the gate names none."""
    quoted = re.search(r"(?<![A-Za-z])['\"]([^'\"]+)['\"]", gate["gate"].get("resume", ""))
    return quoted.group(1).lower() if quoted else ""


NEGATIONS = ("not", "don't", "dont", "do not", "never", "no", "isn't", "wait", "yet", "hold", "stop", "cancel")
MAX_ANSWER_WORDS = 12  # a go is a short reply; anything longer is read as discussion, never as the go


def _plain(text: str) -> str:
    return " ".join(re.sub(r"[^a-z0-9' ]+", " ", text.lower()).split())


def clears(gate: dict[str, Any], text: str) -> bool:
    """True only for a short, unnegated reply carrying the gate's phrase (D-479).

    "Accounts DONE, thanks" clears the account checkpoint; "I have NOT approved to build yet"
    and "don't go live yet" clear nothing: a negation anywhere in the reply, or a reply longer
    than MAX_ANSWER_WORDS, keeps the gate closed and the answer is only recorded.
    """
    plain = _plain(text)
    words = plain.split()
    if len(words) > MAX_ANSWER_WORDS or any(re.search(rf"\b{re.escape(n)}\b", plain) for n in NEGATIONS):
        return False
    phrase = resume_phrase(gate)
    if phrase:
        return _plain(phrase) in plain
    return any(w in words for w in DEFAULT_WORDS)


def apply_owner_answer(state: State, gate: dict[str, Any], text: str, now: datetime) -> State:
    """Clear a gate only on its exact phrase; any other answer is recorded and the gate stays."""
    answers = list(entry(state, gate["id"]).get("answers", [])) + [text]
    if clears(gate, text):
        return with_task(state, gate["id"], status="DONE", answers=answers, cleared=now.isoformat())
    return with_task(state, gate["id"], answers=answers)


def hold_for_gate_p(state: State, outbox_dir: Path) -> State:
    """K9 done: build is held until Casey says 'approved to build' (BUILD_BRIEF.md:538)."""
    body = f"Your presentation is ready. Reply '{GATE_P_PHRASE}' to start building."
    write_outbox(outbox_dir, GATE_P, "Gate P: your presentation is ready", "gate", body)
    out = with_task(state, GATE_P, status=WAITING_FOR_OWNER)
    out["build_hold"] = out.get("build_hold") or f"Gate P: build tasks wait for '{GATE_P_PHRASE}'"
    return out


def apply_gate_p_answer(state: State, text: str) -> State:
    out = with_task(state, GATE_P, answer=text)
    if clears({"gate": {"resume": f"'{GATE_P_PHRASE}'"}}, text):
        out = with_task(out, GATE_P, status="DONE")
        out["build_hold"] = None
    return out
