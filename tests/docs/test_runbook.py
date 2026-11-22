"""The runbook holds every step its requirements name (T117).

TD-OPS-05 (OPS-012, OPS-014, OPS-020, OPS-021, OPS-022), TD-OPS-29 (OPS-054, OPS-020) and
TD-PRIV-09's inventory case (PRIV-024), with plan changes D-3144 (safe mode), D-3844
(complaints), D-3889 (inventory names come from PRIV-024's list, never from `settings.rs`),
D-4932 (the operator's restore off the host) and D-5412 items (1) to (6): key format, start
check, rotation order, an export older than a rotation, keeping retired keys 30 days, and the
opt-out check after a restore. Each check reads one section, so a phrase moved to the wrong
section fails. Fake data only.

Run: python3 -m pytest tests/docs/test_runbook.py -q -p no:cacheprovider
"""

from __future__ import annotations

import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
RUNBOOK = ROOT / "docs" / "runbook.md"
WRAP = 80


def text() -> str:
    return RUNBOOK.read_text(encoding="utf-8")


def flat(body: str) -> str:
    """The text with line breaks and repeated spaces folded, so a phrase may wrap."""
    return re.sub(r"\s+", " ", body)


def raw_section(title: str, body: str | None = None) -> str:
    """The body of the `##` or `###` heading starting with `title`, as written."""
    source = text() if body is None else body
    match = re.search(rf"^(#{{2,3}}) {re.escape(title)}.*$", source, re.M)
    assert match, f"no section {title!r}"
    level = len(match.group(1))
    rest = source[match.end():]
    end = re.search(rf"^#{{1,{level}}} ", rest, re.M)
    return rest[: end.start()] if end else rest


def section(title: str, body: str | None = None) -> str:
    """The folded body of the section starting with `title`."""
    return flat(raw_section(title, body))


def has_all(body: str, phrases: list[str]) -> list[str]:
    return [p for p in phrases if p not in body]


def numbered(title: str, body: str | None = None) -> dict[int, str]:
    """A section's numbered steps (a number starting a line), each folded, by number."""
    parts = re.split(r"^(\d{1,2})\. ", raw_section(title, body), flags=re.M)
    return {int(n): flat(step) for n, step in zip(parts[1::2], parts[2::2])}


def test_runbook_lines_keep_the_wrap_width() -> None:
    long = [i for i, line in enumerate(text().splitlines(), 1) if len(line) > WRAP]
    assert not long, f"lines over {WRAP}: {long}"


def test_td_ops_05_host_choice_restore_switch_and_no_hosting_key() -> None:
    assert not has_all(section("1. Host choice"), ["Railway Pro", "point-in-time", "Render"])
    restore = section("8. Restore")
    assert not has_all(restore, ["scripts/bootstrap_roles.sql", "pg_restore --single-transaction",
                                 "Switch web and worker to the restored database", "`/healthz`",
                                 "OPS-014", "data-loss window", "no-double-charge"])
    intro = flat(text().split("## 1.")[0])
    assert not has_all(intro, ["Claude never holds a hosting key", "presses Deploy at Gate C"])


def test_td_ops_05_breach_starts_by_pausing_texting_and_charging() -> None:
    steps = numbered("10. Breach")
    assert not has_all(steps[1], ["Pause texting and charging first", "preserve the logs"])
    assert "45-day notice" in section("10. Breach")


def test_td_ops_29_fix_forward_and_breach_notice_paths() -> None:
    forward = section("Fix forward")
    assert not has_all(forward, ["fix goes forward", "never roll the service back",
                                 "from before the rotation", "rollback restores"])
    breach = section("10. Breach")
    steps = numbered("10. Breach")
    assert not has_all(steps[2], ["fix forward", "never roll back"]), "secret-exposed step"
    assert not has_all(breach, [
        "letter, email or a live phone call", "never by text alone", "Substitute notice",
        "Attorney General", "laftly.com for at least 45 days", "Law-enforcement pause",
        "hamper an investigation", "online notice form", "as soon as practicable",
    ])


PRIV_024_SECRETS = ["`VAULT_KEY`", "`VAULT_INDEX_KEY`", "`ID_SECRET`", "the photo-link key",
                    "the export key", "vendor keys"]


def test_td_priv_09_inventory_names_every_priv_024_secret_with_escrow_and_trigger() -> None:
    raw = section("Key inventory")
    header = re.search(r"\| Secret \| Owner \| Storage \| Escrow \| Rotation trigger \|", raw)
    assert header, "inventory columns"
    rows = {}
    for line in RUNBOOK.read_text(encoding="utf-8").splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if cells[0] in PRIV_024_SECRETS:
            rows[cells[0]] = cells
    assert sorted(rows) == sorted(PRIV_024_SECRETS)
    for name, cells in rows.items():
        assert len(cells) == 5 and all(cells), name
        assert cells[3] == "paper", f"{name} escrow"
    assert not has_all(raw, ["sealed on paper", "apart from the password manager",
                             "private half never sits on the host", "AES-256-GCM",
                             "`homeowner-phone|v1`", "`sender-dir|v1`", "`email|v1`"])


def test_d_3144_safe_mode_release_waits_and_presses_are_refused() -> None:
    body = section("3. Safe mode")
    assert not has_all(body, ["A release waits while safe mode is on", "FEE-017 check",
                              '"Confirm and pay" press is refused',
                              "charges are refused until it ends", "T707"])


def test_d_3844_complaints_reach_casey_and_say_what_he_does() -> None:
    body = section("6. Complaints")
    assert not has_all(body, ["How a complaint reaches Casey", "What Casey does with it",
                              "weekly digest", "safety word", "3 complaints in 90 days"])


def test_outages_refunds_disputes_and_stop_issues_are_covered() -> None:
    assert "/healthz" in section("2. Outages")
    assert "disputed charge" in section("4. Refunds")
    assert "evidence" in section("5. Disputes")
    assert not has_all(section("7. STOP issues"), ['"cancel"', "START", "21610", "/owner/opt-out"])


def test_d_4932_restore_is_the_operators_off_the_host() -> None:
    steps = numbered("8. Restore")
    order = ["read key the host never holds", "`age -d`", "server replay",
             "Delete the plain dump file"]
    at = [next(n for n, s in sorted(steps.items()) if phrase in s) for phrase in order]
    assert at == sorted(at), f"restore order {at}"
    assert "escrowed private export key" in steps[4]
    assert "`ASKHAVA_MIGRATION_DATABASE_URL`" in section("8. Restore")


def test_d_3889_inbound_mail_retention_is_recorded() -> None:
    assert not has_all(section("11. Inbound mail"), ["deletes each", "7 days or less", "D-4914"])


def test_d_5412_1_key_format() -> None:
    body = section("Key format")
    assert not has_all(body, ["`<version>:<base64 of 32 bytes>`", "highest version active",
                              "app/privacy/src/vault.rs:69"])


def test_d_5412_2_start_check_refusals() -> None:
    body = section("Key format")
    assert not has_all(body, ["`check_index_keys`", "app/server/src/main.rs:55",
                              "app/server/src/bin/worker.rs:46", "app/privacy/src/vault.rs:985",
                              "app/messaging/src/suppression_versions.rs:39"])


def test_d_5412_3_rotation_order_and_refused_retirement() -> None:
    steps = numbered("Rotation")
    assert "add v2 to both lists" in steps[1]
    assert "`server rotate-keys` until every batch is 0" in steps[2]
    assert "`server retire-index-version 1`" in steps[3]
    assert not has_all(steps[4], ["Keep v1 on both lists", "note the count", "ask the developer",
                                  "Never delete those rows by hand"])
    assert "stale v1 phone digests" in steps[5]
    assert not has_all(steps[6], ["Drop v1", "0 moved and 0 skipped", "none was refused"])


def test_d_5412_4_old_export_puts_v1_back_before_start() -> None:
    steps = numbered("8. Restore")
    back = next(n for n, s in steps.items() if "older than a rotation" in s)
    assert "before starting the worker or web" in steps[back]
    start = next(n for n, s in steps.items() if "start the worker" in s)
    redo = next(n for n, s in steps.items() if "redo the rotation" in s)
    assert back < start < redo and "drop v1" in steps[redo]


def test_d_5412_5_retired_keys_kept_30_days_after_last_export() -> None:
    body = section("Keeping retired keys")
    assert not has_all(body, ["retired key's paper copy", "the old export key",
                              "30 days after the last export made under it",
                              "restart after a restore", "OPS-013"])


def test_d_5412_6_web_stopped_and_opt_outs_checked_before_the_pause_ends() -> None:
    steps = numbered("8. Restore")
    assert "keep web stopped during the restore" in steps[1]
    check = next(n for n, s in steps.items() if "Opt-out check" in s)
    assert not has_all(steps[check], ["Twilio's inbound log", "email and web opt-outs",
                                      "suppression list", "never opt-outs", "CON-035"])
    end = next(n for n, s in steps.items() if "end the pauses" in s)
    assert check < end


@pytest.mark.parametrize("word", ["commission", "vetted", "Ask Hava", "Havasu Home Quotes"])
def test_runbook_avoids_banned_words_and_old_brand(word: str) -> None:
    assert word.lower() not in text().lower()


def test_section_reader_catches_a_phrase_moved_out_of_its_section() -> None:
    planted = "## A\nalpha\n## B\nbeta\n"
    assert "beta" not in section("A", planted)
    assert numbered("A", "## A\n1. one is 2. not\n2. two\n")[2] == "two "
