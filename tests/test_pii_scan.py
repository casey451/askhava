"""Tests for scripts/pii_scan.py. Bad samples are assembled at run time so this file stays clean."""

from __future__ import annotations

from pathlib import Path

import pii_scan
import pytest


def rules(text: str) -> list[str]:
    return [rule for _, _, rule in pii_scan.scan_text("x.py", text)]


def test_fictional_phones_pass() -> None:
    assert rules("phone = '+1 928 555 0142'") == []
    assert rules("call (928) 555-0199") == []
    assert rules("magic +15005550006") == []


def test_real_looking_phones_fail() -> None:
    assert rules("phone = '+1 928 " + "700 1234'") == ["phone"]
    assert rules("call (928) " + "555-0234") == ["phone"]
    assert rules("+1928" + "7001234") == ["phone"]
    assert rules("928." + "444.1212") == ["phone"]


def test_bare_ten_digit_run_starting_with_one_is_not_a_phone() -> None:
    assert rules("created_at = 1798400000") == []


def test_bare_ten_digit_run_that_could_be_a_phone_fails() -> None:
    assert rules("contact = 92870" + "01234") == ["phone"]


def test_addresses() -> None:
    assert rules("1 Test Way") == []
    assert rules("12 Example Street") == []
    assert rules("4521 Maple" + " Avenue") == ["address"]
    assert rules("987 Lake Havasu" + " Boulevard") == ["address"]
    assert rules("120 N " + "Smith Street") == ["address"]
    assert rules("120 NE " + "Smith Street") == ["address"]
    assert rules("77 " + "McDowell Road") == ["address"]
    assert rules("9 " + "MacArthur Drive") == ["address"]
    assert rules("120 N Test Street") == []


def test_names_only_from_the_fake_list() -> None:
    assert rules('first_name = "Zelda Canary"') == []
    assert rules('homeowner_name: "Test Homeowner"') == []
    assert rules('full_name = "' + 'Jordan Whitfield"') == ["name"]
    assert rules('name = "Jordan Whitfield"') == []  # unlabelled text is out of reach


def test_key_shaped_strings_fail() -> None:
    assert rules("k = 'sk_" + "live_" + "A" * 24 + "'") == ["stripe key"]
    assert rules("AKIA" + "ABCDEFGHIJKLMNOP") == ["aws key"]
    assert rules("ghp_" + "a" * 36) == ["github token"]
    assert rules("sk-ant-" + "x" * 30) == ["anthropic key"]
    assert rules("-----BEGIN " + "PRIVATE KEY-----") == ["private key"]
    assert rules("AC" + "a1" * 16) == ["twilio sid"]


def test_fake_zero_sid_and_short_secrets_pass() -> None:
    assert rules("sid = 'AC" + "0" * 32 + "'") == []
    assert rules("secret = 'whsec_fake'") == []


def test_main_scans_files_skips_sources_and_exits_nonzero(tmp_path: Path) -> None:
    (tmp_path / "sources").mkdir()
    (tmp_path / "sources" / "ok.txt").write_text("call 928 " + "700 1234", encoding="utf-8")
    (tmp_path / "bad.py").write_text("x = 1\nphone = '928-" + "700-1234'\n", encoding="utf-8")
    assert pii_scan.main(["--root", str(tmp_path)]) == 1
    (tmp_path / "bad.py").write_text("x = 1\n", encoding="utf-8")
    assert pii_scan.main(["--root", str(tmp_path)]) == 0


def test_output_never_echoes_the_match(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    secret = "sk_" + "live_" + "B" * 24
    (tmp_path / "a.py").write_text(f"k = '{secret}'\n", encoding="utf-8")
    pii_scan.main(["--root", str(tmp_path)])
    assert secret not in capsys.readouterr().out


def test_repository_is_clean() -> None:
    root = Path(__file__).resolve().parent.parent
    assert pii_scan.scan_files(root, pii_scan.tracked_files(root)) == []


def test_oversize_files_are_reported_and_bad_encoding_is_still_scanned(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    (tmp_path / "big.txt").write_text("x" * (pii_scan.MAX_BYTES + 1), encoding="utf-8")
    (tmp_path / "latin.txt").write_bytes(b"caf\xe9 phone 928-" + b"700-1234")
    skipped: list[str] = []
    found = pii_scan.scan_files(tmp_path, ["big.txt", "latin.txt"], skipped)
    assert [rule for _, _, rule in found] == ["phone"] and skipped == [f"big.txt (over {pii_scan.MAX_BYTES} bytes)"]
    pii_scan.main(["--root", str(tmp_path)])
    assert "skipped, not scanned: big.txt" in capsys.readouterr().out
