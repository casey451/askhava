"""The stand-in's email log route over HTTP (TXT-021, TXT-034; D-491, D-3263). Fake addresses only."""

from __future__ import annotations

from tests.contract.test_stand_in_server import base, call  # noqa: F401  (the `base` fixture)

AUTH = {"authorization": "Bearer re_stand_in_fake"}
LOG = "/_stand_in/resend/emails"


def send(base: str, to: str, subject: str = "Your quote", **headers: str) -> int:
    body = {"from": "hello@example.com", "to": to, "subject": subject, "text": "Plain", "html": "<p>Rich</p>"}
    status, _, _ = call(base, "POST", "/resend/emails", body=body, **AUTH, **headers)
    return status


def test_empty_log_answers_an_empty_list(base: str) -> None:
    assert call(base, "GET", LOG)[:2] == (200, {"emails": []})


def test_a_send_is_listed_once_with_its_fields(base: str) -> None:
    assert send(base, "ann@example.com") == 200
    emails = call(base, "GET", LOG)[1]["emails"]
    assert len(emails) == 1
    assert set(emails[0]) == {"id", "from", "to", "subject", "text", "html"}
    assert emails[0]["from"] == "hello@example.com"
    assert emails[0]["to"] == ["ann@example.com"]
    assert (emails[0]["subject"], emails[0]["text"], emails[0]["html"]) == ("Your quote", "Plain", "<p>Rich</p>")


def test_a_replayed_idempotent_send_is_listed_once(base: str) -> None:
    assert send(base, "ann@example.com", idempotency_key="k1") == 200
    assert send(base, "ann@example.com", idempotency_key="k1") == 200
    assert len(call(base, "GET", LOG)[1]["emails"]) == 1


def test_missing_html_lists_as_empty_html(base: str) -> None:
    body = {"from": "hello@example.com", "to": ["ann@example.com"], "subject": "S", "text": "Only text"}
    assert call(base, "POST", "/resend/emails", body=body, **AUTH)[0] == 200
    assert call(base, "GET", LOG)[1]["emails"][0]["html"] == ""


def test_to_filter_keeps_only_that_address_oldest_first(base: str) -> None:
    send(base, "ann@example.com", "First")
    send(base, "bob@example.com", "Second")
    send(base, "ann@example.com", "Third")
    mine = call(base, "GET", LOG + "?to=ann@example.com")[1]["emails"]
    assert [e["subject"] for e in mine] == ["First", "Third"]
    assert call(base, "GET", LOG + "?to=none@example.com")[1] == {"emails": []}
    assert call(base, "GET", LOG + "?to=example.com")[1] == {"emails": []}
    assert len(call(base, "GET", LOG)[1]["emails"]) == 3


def test_to_filter_matches_the_address_inside_a_display_name(base: str) -> None:
    send(base, "Ann Example <ann@example.com>")
    mine = call(base, "GET", LOG + "?to=ann@example.com")[1]["emails"]
    assert [e["to"] for e in mine] == [["Ann Example <ann@example.com>"]]


def test_to_filter_ignores_case(base: str) -> None:
    send(base, "Bob@Example.com")
    assert len(call(base, "GET", LOG + "?to=bob@example.com")[1]["emails"]) == 1
    assert len(call(base, "GET", LOG + "?to=BOB@EXAMPLE.COM")[1]["emails"]) == 1


def test_empty_to_filter_lists_every_email(base: str) -> None:
    send(base, "ann@example.com")
    send(base, "bob@example.com")
    assert len(call(base, "GET", LOG + "?to=")[1]["emails"]) == 2
