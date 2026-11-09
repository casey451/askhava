"""The model window: a time-boxed switch of every model the runner starts (D-2225).

Casey, November 9, 2026: "on Thursday when the usage resets, I want to switch everything to fable and run
the usage all the way through so I can use one of my fable resets to bring usage back to zero", and "Back
to opus. Its just to get more tokens out of the week for the build series." The plan (kit/tasks.json) is
not touched: the window is one key of kit/runner.json, read at run time,

    "model_window": {"model": "claude-fable-5-1", "alias": "fable", "from": "2026-11-12T17:00:00Z",
                     "until": null, "run_to_limit": true, "end_on_limit": true}

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

With `end_on_limit`, the window closes by itself the first time the runner records the plan's limit inside
it: a session reading taken in [from, until) with the week at 100% or rejected (a `usage.limit_hit` alone,
which may be the five-hour limit, does not close it). The runner records `model_window_ended` = {model, from, at, why: "limit"} and the outbox item
`model-window-ended` (`close_on_limit`, at a cycle's claim and at `--usage-reading`); from then on that
window (the same model and `from`) changes no model and `run_to_limit` stops: pacing is back to its own
rules (headroom, the 97% stop), which still wait out the stale 100% reading until a fresh one shows Casey's
reset: the close drops the pacer's ledger, and the keeper bounds its waits by the reading's 6-hour expiry, so a
cycle starts within 6 hours of the last reading and refreshes it. Casey's words: Fable only until the week's usage runs out, then a reset, then back to Opus.

A model the CLI or the worker call refuses is not the task's failure: the session runs the attempt on the
task's own models and hands in with `--model-fallback "<error>"`, or the job's step after the session finds
the orchestrating session refused (`--model-refused FILE`); either way the state records
`model_window_refused` = {model, from, at, why} and an outbox item says so, and from then on that window
(the same model and `from`) changes no model, while `run_to_limit` still applies. A new model string or
`from` in kit/runner.json is a new window and is tried again.

Outside the window, or with no or a malformed `model_window` key, nothing here changes anything: every
function returns the value the runner used before D-2225.
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
ENDED = "model_window_ended"  # the state key a window closed by the limit leaves (kit/state_schema.json)
ENDED_OUTBOX_ID = "model-window-ended"
MARK = "window_model"  # the task-entry key a claim inside the window sets
OUTBOX_ID = "model-window-refused"
MODEL_ID = re.compile(r"^claude-[a-z]+(-[0-9]+)+$")
ALIAS = re.compile(r"^[a-z]+$")
# What a refused model looks like in the CLI's or the API's words: Claude Code's "There's an issue with the
# selected model (X). It may not exist or you may not have access to it", the API's 404 not_found_error on
# `model`, and the Agent tool's schema refusal of an alias it does not list.
REFUSAL = re.compile(
    r"issue with the selected model|not_found_error[^\n]{0,80}model|model[^\n]{0,80}(not found|does not exist|"
    r"may not exist|not available|is not supported|not have access|unknown|invalid)|(unknown|invalid) model|"
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
    run_to_limit, end_on_limit = raw.get("run_to_limit", False), raw.get("end_on_limit", False)
    if not isinstance(run_to_limit, bool) or not isinstance(end_on_limit, bool):
        return None
    return {"model": model, "alias": alias, "from": start, "until": end, "run_to_limit": run_to_limit, "end_on_limit": end_on_limit}


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


def _recorded(window: Window | None, state: st.State | None, key: str) -> bool:
    found = (state or {}).get(key)
    if window is None or not isinstance(found, dict):
        return False
    return found.get("model") == window["model"] and found.get("from") == st.iso(window["from"])


def refused(window: Window | None, state: st.State | None) -> bool:
    """Whether the state records this very window (model and `from`) as refused."""
    return _recorded(window, state, REFUSED)


def _during(window: Window, stamp: Any) -> datetime | None:
    moment = _instant(stamp)
    return moment if moment is not None and in_window(window, moment) else None


def _week_full(reading: dict[str, Any]) -> bool:
    """Whether a session reading's week is at the plan's limit: utilization at 100%, or the API's own
    `status` "rejected" for it (runner/usage_reading.py keeps that status)."""
    week = reading.get("week")
    if not isinstance(week, dict):
        return False
    if week.get("status") == "rejected":
        return True
    try:
        return float(week["utilization"]) >= 1.0
    except (KeyError, TypeError, ValueError):
        return False


def limit_inside(window: Window | None, state: st.State | None) -> str:
    """Why the state shows the week's limit reached inside the window ('' when it does not). Only positive
    evidence counts: a session reading taken in the window whose week is at 100% or rejected. A
    `usage.limit_hit` alone never counts (it may be the five-hour limit, or have no reading after it); the
    reading that follows a week's limit is what closes the window."""
    usage = (state or {}).get("usage") if isinstance((state or {}).get("usage"), dict) else {}
    if window is None or not isinstance(usage, dict):
        return ""
    reading = usage.get("session_reading") if isinstance(usage.get("session_reading"), dict) else {}
    read_at = _during(window, reading.get("at"))
    if read_at is None or not _week_full(reading):
        return ""
    return f"the week read at its limit at {st.iso(read_at)}"


def ended(window: Window | None, state: st.State | None) -> bool:
    """Whether the state records this very window as ended at the limit (`model_window_ended`)."""
    return _recorded(window, state, ENDED)


def closed(window: Window | None, state: st.State | None) -> bool:
    """Whether an `end_on_limit` window is over: recorded as ended, or the limit already shows inside it."""
    if window is None or not window["end_on_limit"]:
        return False
    return _recorded(window, state, ENDED) or bool(limit_inside(window, state))


def switched(window: Window | None, now: datetime, state: st.State | None) -> Window | None:
    """The window when it changes models now (inside it, not refused, not closed by the limit), else None."""
    return window if in_window(window, now) and not refused(window, state) and not closed(window, state) else None


def run_to_limit(window: Window | None, now: datetime, state: st.State | None = None) -> bool:
    """Whether the pacer and the keeper run to the real limit now: inside the window (refused or not) and,
    for an `end_on_limit` window, until the limit closes it."""
    return bool(window and window["run_to_limit"] and in_window(window, now) and not closed(window, state))


def close_on_limit(state: st.State, window: Window | None, now: datetime) -> tuple[st.State, str]:
    """(state, why): the state with `model_window_ended` recorded when an `end_on_limit` window shows its limit
    for the first time, and why; otherwise the state unchanged (the same object) and ''. The pacer's own
    ledger (`usage.windows`) is dropped in the same write: it was not read while the window ran to the limit,
    holds that whole spend against the placeholder budgets, and would otherwise hold the build for up to a
    week after Casey's reset once the last reading is stale (the next attempt's usage starts it again)."""
    if window is None or not window["end_on_limit"] or _recorded(window, state, ENDED):
        return state, ""
    why = limit_inside(window, state)
    if not why:
        return state, ""
    out = copy.deepcopy(state)
    out[ENDED] = {"model": window["model"], "from": st.iso(window["from"]), "at": st.iso(now), "why": "limit"}
    if isinstance(out.get("usage"), dict):
        out["usage"].pop("windows", None)
    return out, why


def ended_note(window: Window, why: str) -> tuple[str, str]:
    """(title, body) of the outbox item for a window the limit closed."""
    title = f"Runner: the model window ({window['model']}) ended at the usage limit; back on the plan's models"
    body = (f"The plan's usage limit was reached inside the model window: {why}.\n\n"
            "From now on every worker, reviewer and session runs on the plan's own models, and pacing is back to its "
            "usual rules (headroom, the 97% week stop). The build waits while the 100% reading holds; after the reset "
            "(Casey's own or the weekly one) a fresh reading shows the usage down and the build resumes by itself. "
            "kit/runner.json still holds the window; set `until` when convenient (D-2225).")
    return title, body


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
            "Check the model string, or set `until` to end the window (D-2225).")
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
