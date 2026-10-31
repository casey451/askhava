"""Tests for scripts/pii_scan.py. Bad samples are assembled at run time so this file stays clean."""

from __future__ import annotations

import struct
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


def test_growing_log_warns_past_three_quarters_of_the_limit(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    """D-1075: docs/decisions.md over 75% of MAX_BYTES warns while it is still scanned; it does not fail."""
    monkeypatch.setattr(pii_scan, "MAX_BYTES", 1_000)
    (tmp_path / "docs").mkdir()
    log = tmp_path / "docs" / "decisions.md"
    log.write_text("y" * 750, encoding="utf-8")
    assert pii_scan.size_warnings(tmp_path) == []
    log.write_text("y" * 751, encoding="utf-8")
    assert pii_scan.size_warnings(tmp_path) == [
        "docs/decisions.md is 751 bytes, over 75% of MAX_BYTES (1,000, scripts/pii_scan.py); "
        "raise the limit or split the file before the scanner skips it and fails"
    ]
    assert pii_scan.main(["--root", str(tmp_path)]) == 0
    assert "WARNING docs/decisions.md is 751 bytes" in capsys.readouterr().out


# --- D-485: email addresses, image metadata, skipped files ---------------------------------


def test_emails_only_at_reserved_or_own_domains() -> None:
    assert rules("to = 'canary@example.com'") == []
    assert rules("to = 'pat@mail.example.org'") == []
    assert rules("to = 'a@b.test', 'c@d.example', 'e@f.invalid'") == []
    assert rules("help@askhava.com and jobs@notify.askhava.com") == []
    assert rules("delivered@resend.dev") == []
    assert rules("runner@users.noreply.github.com") == []
    assert rules("`you+poola@gmail.com`") == []
    assert rules("texture plaster@2x.jpg") == []


def test_real_looking_emails_fail() -> None:
    assert rules("to = 'jordan.w" + "@gmail.com'") == ["email"]
    assert rules("reach me at pat" + "@havasu-mail.net") == ["email"]
    assert rules("x@example.com" + ".evil.io") == ["email"]
    assert rules("anyone" + "@resend.dev") == ["email"]


def tiff(tags: list[int], sub_tags: list[int] | None = None) -> bytes:
    """A little-endian TIFF block with IFD0 holding `tags` and an optional Exif sub-IFD."""
    entries = list(tags) + ([0x8769] if sub_tags is not None else [])
    ifd0 = 8
    sub = ifd0 + 2 + 12 * len(entries) + 4
    out = bytearray(b"II*\x00" + struct.pack("<I", ifd0))
    out += struct.pack("<H", len(entries))
    for tag in entries:
        value = sub if tag == 0x8769 else 0
        out += struct.pack("<HHII", tag, 4, 1, value)
    out += struct.pack("<I", 0)
    if sub_tags is not None:
        out += struct.pack("<H", len(sub_tags))
        for tag in sub_tags:
            out += struct.pack("<HHII", tag, 2, 1, 0)
        out += struct.pack("<I", 0)
    return bytes(out)


def jpeg_with(block: bytes) -> bytes:
    app1 = b"Exif\x00\x00" + block
    return b"\xff\xd8\xff\xe1" + struct.pack(">H", len(app1) + 2) + app1 + b"\xff\xd9"


def png_with(block: bytes) -> bytes:
    return b"\x89PNG\r\n\x1a\n" + struct.pack(">I", len(block)) + b"eXIf" + block + b"\x00" * 4


def webp_with(block: bytes) -> bytes:
    return b"RIFF\x00\x00\x00\x00WEBPEXIF" + struct.pack("<I", len(block)) + block


def test_image_gps_and_device_metadata_are_found() -> None:
    assert pii_scan.image_metadata_rules(jpeg_with(tiff([0x8825]))) == {"image-gps"}
    assert pii_scan.image_metadata_rules(jpeg_with(tiff([0x010F, 0x0110]))) == {"image-device"}
    assert pii_scan.image_metadata_rules(png_with(tiff([0x8825]))) == {"image-gps"}
    assert pii_scan.image_metadata_rules(webp_with(tiff([], [0xA431]))) == {"image-device"}
    xmp = b"\xff\xd8<x:xmpmeta><rdf:Description exif:GPSLatitude='34,28N'/></x:xmpmeta>\xff\xd9"
    assert pii_scan.image_metadata_rules(xmp) == {"image-gps"}


def test_clean_images_pass() -> None:
    assert pii_scan.image_metadata_rules(jpeg_with(tiff([0x0112]))) == set()  # orientation only
    assert pii_scan.image_metadata_rules(b"\xff\xd8\xff\xdb" + b"\x00" * 64 + b"\xff\xd9") == set()
    assert pii_scan.image_metadata_rules(b"Exif\x00\x00II*\x00\xff\xff\xff\xff") == set()  # broken TIFF


def test_images_under_tests_and_design_are_checked(tmp_path: Path) -> None:
    (tmp_path / "tests").mkdir()
    (tmp_path / "docs" / "design").mkdir(parents=True)
    (tmp_path / "tests" / "photo.jpg").write_bytes(jpeg_with(tiff([0x8825])))
    (tmp_path / "docs" / "design" / "mock.png").write_bytes(png_with(tiff([0x0110])))
    (tmp_path / "other.jpg").write_bytes(jpeg_with(tiff([0x8825])))  # outside the image roots
    paths = ["tests/photo.jpg", "docs/design/mock.png", "other.jpg"]
    found = pii_scan.scan_files(tmp_path, paths)
    assert ("tests/photo.jpg", 0, "image-gps") in found
    assert ("docs/design/mock.png", 0, "image-device") in found
    assert not [f for f in found if f[0] == "other.jpg" and f[2].startswith("image")]
    assert pii_scan.main(["--root", str(tmp_path)]) == 1


def test_skipped_file_fails_unless_allow_listed(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    (tmp_path / "big.txt").write_text("x" * (pii_scan.MAX_BYTES + 1), encoding="utf-8")
    assert pii_scan.main(["--root", str(tmp_path)]) == 1
    assert "fails, not on SKIP_ALLOWED" in capsys.readouterr().out
    monkeypatch.setattr(pii_scan, "SKIP_ALLOWED", {"big.txt": "fixture too large to scan, reviewed"})
    assert pii_scan.main(["--root", str(tmp_path)]) == 0
    assert "allow-listed" in capsys.readouterr().out


def test_twin_generator_literals_pass_only_in_their_own_file() -> None:
    line = "text += f\" I'm at 412 Palo Verde " + "Dr\"; phone = \"+1928" + "0000000\""
    assert pii_scan.scan_text("twin/twin.py", line) == []
    assert {rule for _, _, rule in pii_scan.scan_text("twin/app.py", line)} == {"address", "phone"}


def test_twin_file_still_flags_other_real_looking_data() -> None:
    line = "contact_phone = \"+1928" + "7001234\""
    assert [rule for _, _, rule in pii_scan.scan_text("twin/twin.py", line)] == ["phone"]


def test_growing_log_stops_a_plan_change_past_nine_tenths_of_the_limit(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """D-1075: over 90% of MAX_BYTES is a problem for the plan-change check, while the scanner itself still passes."""
    monkeypatch.setattr(pii_scan, "MAX_BYTES", 1_000)
    (tmp_path / "docs").mkdir()
    log = tmp_path / "docs" / "decisions.md"
    log.write_text("y" * 900, encoding="utf-8")
    assert pii_scan.size_stops(tmp_path) == []
    log.write_text("y" * 901, encoding="utf-8")
    assert pii_scan.size_stops(tmp_path) == [
        "docs/decisions.md is 901 bytes, over 90% of MAX_BYTES (1,000); raise the limit or split the file in this plan change"
    ]
    assert pii_scan.main(["--root", str(tmp_path)]) == 0
