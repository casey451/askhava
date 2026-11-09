"""The model window: a time-boxed switch of every model the runner starts (D-2220).

Casey, November 9, 2026: "on Thursday when the usage resets, I want to switch everything to fable and run
the usage all the way through so I can use one of my fable resets to bring usage back to zero", and "Back
to opus. Its just to get more tokens out of the week for the build series." The plan (kit/tasks.json) is
not touched: the window is one key of kit/runner.json, read at run time,

    "model_window": {"model": "claude-fable-5-1", "alias": "fable", "from": "2026-11-12T17:00:00Z",
                     "until": null, "run_to_limit": true}

`model` is the full ID the session transcripts record (what verify.check_model compares), `alias` the name
the worker call (the Agent tool) takes (D-170); `from` and `until` are UTC instants, the window being
[from, until), `until` null meaning open. Going back is one data change: set `until` (or remove the key).

While `now` is inside the window:
- a claim marks the task's state entry `window_model` = {"model", "alias"}; that attempt's worker and its
  reviewer run on that model instead of the task's (the hand-off's `model_window` block names the alias and
  the fallback), and `--after` checks the evidence against it, so a claim made inside and finished outside
  (or the reverse) is judged by the model it was started on. The history and usage rows keep the model
  that ran;
- the orchestrating session's `--model` (backup-runner.yml, `python3 -m runner --session-model`);
- with `run_to_limit`, the pacer (scripts/pacing.py) and the keeper (runner/keeper.py) drop the headroom
  and the 97% week stop: they hold only for a recorded usage-limit hit or a real reading at 100%.

A model the CLI or the worker call refuses is not the task's failure: the session runs the attempt on the
task's own models and hands in with `--model-fallback "<error>"`, or the job's step after the session finds
the orchestrating session refused (`--model-refused FILE`); either way the state records
`model_window_refused` = {model, from, at, why} and an outbox item says so, and from then on that window
(the same model and `from`) changes no model, while `run_to_limit` still applies. A new model string or
`from` in kit/runner.json is a new window and is tried again.

Outside the window, or with no or a malformed `model_window` key, nothing here changes anything: every
function returns the value the runner used before D-2220.
"""

from __future__ import annotations

import copy
import json
import re
from datetime import datetime
from pathlib import Path
from typing import Any

from runner import state as st

SETTINGS = Path(__file__).resolve().parent.parent / "kit" / "runner.json"
SESSION_MODEL = "claude-sonnet-5-5"  # the orchestrating session's model outside a window (BUILD_BRIEF.md:500)
REFUSED = "model_window_refused"  # the state key a refused window leaves (kit/state_schema.json)
MARK = "window_model"  # the task-entry key a claim inside the window sets
OUTBOX_ID = "model-window-refused"
MODEL_ID = re.compile(r"^claude-[a-z]+(-[0-9]+)+$")
ALIAS = re.compile(r"^[a-z]+$")
# What a refused model looks like in the CLI's or the API's words: Claude Code's "There's an issue with the
# selected model (X). It may not exist or you may not have access to it", the API's 404 not_found_error on
# `model`, and the Agent tool's schema refusal of an alias it does not list.
REFUSAL = re.compile(
    r"issue with the selected model|not_found_error[^\n]{0,80}model|model[^\n]{0,80}(not found|does not exist|"
    r"may not exist|not available|is not supported|unknown|invalid)|(unknown|invalid) model|"
    r"invalid_enum_value[^\n]{0,200}model",
    re.IGNORECASE,
)

Window = dict[str, Any]


def _instant(value: Any) -> datetime | None:
    if not isinstance(value, str):
        return None
    try:
        return st.parse(value)
    except ValueError:
        return None


def parse_window(raw: Any) -> Window | None:
    """The window from the `model_window` value, or None when absent or malformed (nothing changes then).
    `until` may be null; a set `until` at or before `from` is an empty window, which is never active."""
    if not isinstance(raw, dict):
        return None
    model, alias, start = raw.get("model"), raw.get("alias"), _instant(raw.get("from"))
    if not isinstance(model, str) or not MODEL_ID.match(model) or not isinstance(alias, str) or not ALIAS.match(alias):
        return None
    until = raw.get("until")
    end = _instant(until)
    if start is None or (until is not None and end is None):
        return None
    run_to_limit = raw.get("run_to_limit", False)
    if not isinstance(run_to_limit, bool):
        return None
    return {"model": model, "alias": alias, "from": start, "until": end, "run_to_limit": run_to_limit}


def load(settings: Path = SETTINGS) -> Window | None:
    """The window in kit/runner.json, or None when the file or the key is missing or unreadable."""
    try:
        raw = json.loads(settings.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    return parse_window(raw.get("model_window")) if isinstance(raw, dict) else None


def in_window(window: Window | None, now: datetime) -> bool:
    """from <= now < until (until None: open)."""
    if window is None or now < window["from"]:
        return False
    return window["until"] is None or now < window["until"]


def refused(window: Window | None, state: st.State | None) -> bool:
    """Whether the state records this very window (model and `from`) as refused."""
    found = (state or {}).get(REFUSED)
    if window is None or not isinstance(found, dict):
        return False
    return found.get("model") == window["model"] and found.get("from") == st.iso(window["from"])


def switched(window: Window | None, now: datetime, state: st.State | None) -> Window | None:
    """The window when it changes models now (inside it and not refused), else None."""
    return window if in_window(window, now) and not refused(window, state) else None


def run_to_limit(window: Window | None, now: datetime) -> bool:
    """Whether the pacer and the keeper run to the real limit now (inside the window, refused or not)."""
    return bool(window and window["run_to_limit"] and in_window(window, now))


def session_model(window: Window | None, now: datetime, state: st.State | None, default: str = SESSION_MODEL) -> str:
    """The orchestrating session's `--model`: the window's model when it switches now, else `default`."""
    found = switched(window, now, state)
    return found["model"] if found else default


def mark_claim(state: st.State, tid: str, window: Window | None, now: datetime) -> st.State:
    """After a claim: the task's `window_model` mark set while the window switches models, removed when one
    is left from an earlier attempt, and the state returned unchanged (the same object) otherwise."""
    found = switched(window, now, state)
    if found:
        return st.with_task(state, tid, **{MARK: {"model": found["model"], "alias": found["alias"]}})
    if MARK in st.entry(state, tid):
        return unmark(state, tid)
    return state


def unmark(state: st.State, tid: str) -> st.State:
    out = copy.deepcopy(state)
    for task in out["tasks"]:
        if task["id"] == tid:
            task.pop(MARK, None)
    return out


def mark_of(entry: dict[str, Any]) -> dict[str, str] | None:
    """The {model, alias} an attempt was claimed on inside a window, or None."""
    found = entry.get(MARK)
    if isinstance(found, dict) and isinstance(found.get("model"), str) and isinstance(found.get("alias"), str):
        return {"model": found["model"], "alias": found["alias"]}
    return None


def record_refusal(state: st.State, window: Window, why: str, now: datetime, tid: str = "") -> st.State:
    """The state with the window recorded as refused and, when `tid` is given, that task's mark removed."""
    out = copy.deepcopy(state)
    out[REFUSED] = {"model": window["model"], "from": st.iso(window["from"]), "at": st.iso(now), "why": " ".join(why.split())[:300]}
    return unmark(out, tid) if tid else out


def refusal_note(window: Window, why: str, where: str) -> tuple[str, str]:
    """(title, body) of the outbox item for a refused window model."""
    title = f"Runner: the model window's model {window['model']} was refused; back on the plan's models"
    body = (f"The {where} could not run on {window['model']} (alias {window['alias']}): {' '.join(why.split())[:300]}\n\n"
            "No attempt was counted against the task for it. Until kit/runner.json names another model or `from`, "
            "every worker, reviewer and session runs on the plan's own models; `run_to_limit` still applies. "
            "Check the model string, or set `until` to end the window (D-2220).")
    return title, body


def is_refusal(text: str) -> bool:
    return bool(REFUSAL.search(text or ""))


def _strings(value: Any) -> list[str]:
    if isinstance(value, str):
        return [value]
    if isinstance(value, dict):
        return [s for v in value.values() for s in _strings(v)]
    if isinstance(value, list):
        return [s for v in value for s in _strings(v)]
    return []


def _used_a_tool(message: Any) -> bool:
    inner = message.get("message") if isinstance(message, dict) else None
    content = inner.get("content") if isinstance(inner, dict) else None
    return isinstance(content, list) and any(isinstance(c, dict) and c.get("type") == "tool_use" for c in content)


def session_refusal(messages: list[Any]) -> str:
    """The refusal line of a session that never got going: none of its messages used a tool (so no tool
    output, which may quote these very words, is read) and one says the model was refused; '' otherwise."""
    if any(_used_a_tool(m) for m in messages):
        return ""
    for text in (s for m in messages for s in _strings(m)):
        if is_refusal(text):
            return " ".join(text.split())[:300]
    return ""


def handoff_block(mark: dict[str, str], fallback_model: str | None, fallback_reviewer: str | None, alias_for: Any) -> dict[str, Any]:
    """The hand-off's `model_window` block for an attempt claimed inside the window (backup-runner.yml step 3d)."""
    return {
        "model": mark["model"], "alias": mark["alias"], "reviewer_alias": mark["alias"] if fallback_reviewer else None,
        "fallback_model": fallback_model, "fallback_alias": alias_for(fallback_model) if fallback_model else None,
        "fallback_reviewer_model": fallback_reviewer,
        "fallback_reviewer_alias": alias_for(fallback_reviewer) if fallback_reviewer else None,
    }
