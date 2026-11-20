"""The Stage 0.5 static set and the DNS records file (TD-WEB-02, TD-SEC-29, TD-OPS-21; WEB-013, WEB-016, WEB-017, SEC-053, SEC-054, OPS-045).

`scripts/build_stage05.py` fetches pages from a running server, so this test runs its filters and its
closed List B check (D-4258) on planted fake HTML, checks that the copied phone patterns equal T152's,
reads `docs/dns-records.md`, and checks the D-4914 decision that names inbound mail and every sender.
The help address's domain comes from the brand file. Planted numbers are fake 555-01xx numbers.

Run: python3 -m pytest tests/docs/test_stage05.py -q -p no:cacheprovider
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(ROOT / "scripts"))
import brand  # noqa: E402
import build_stage05 as stage  # noqa: E402
import test_no_phone_numbers_in_templates as t152  # noqa: E402

DOMAIN = brand.load()["domain"]
HELP = f"help@{DOMAIN}"
DNS = (ROOT / "docs" / "dns-records.md").read_text(encoding="utf-8")

# Must fail, and the filter must clear (D-4258 (3)).
FILTERED = [
    '<form method="post" action="/help"></form>',
    '<form method="get" action="/search"></form>',
    "<form></form>",
    '<button formaction="/request">Send</button>',
    '<input type="text" name="words">',
    '<textarea name="message"></textarea>',
    '<select name="trade"></select>',
    "<button>Send</button>",
    '<button type="submit">Send</button>',
    '<button type="button" form="f1">Send</button>',
    '<script src="/static/site/app.js"></script>',
    '<script>fetch("/request", {method: "POST"})</script>',
    '<a href="#" onclick="send()">Send</a>',
    '<a href="javascript:send()">Send</a>',
    '<iframe src="https://example.com/form"></iframe>',
    '<object data="/form.html"></object>',
    '<embed src="/form.html">',
    '<a href="sms:+19285550142">Text us</a>',
    '<a href="sms:?&body=hi">Text us</a>',
    '<a href="smsto:+19285550142">Text</a>',
    '<a href="tel:+19285550142">Call</a>',
    '<a href="https://wa.me/19285550142">WhatsApp</a>',
    '<a href="https://forms.gle/abc">Request</a>',
    '<a href="https://calendly.com/x">Book</a>',
    f'<a href="mailto:requests@{DOMAIN}">Email a request</a>',
    f'<a href="mailto:{HELP}?subject=Request">Email</a>',
    "Text us at 928-555-0142.",
    "Text 555-0142.",
    "Call +1 928 555 0142.",
]

# Must fail the check; bare text the filter cannot tell from copy.
CHECK_ONLY = [
    'fetch("/request")',
    "new XMLHttpRequest()",
    'navigator.sendBeacon("/request", data)',
    'new EventSource("/events")',
    'new WebSocket("wss://example.com")',
]

# Must pass the check and stay unchanged by the filter.
CLEAN = [
    f'<a href="mailto:{HELP}">{HELP}</a>',
    '<a href="/request">Start a request</a>',
    '<a href="/privacy">Privacy</a>',
    '<link rel="stylesheet" href="/static/site/site.css">',
    '<details><summary>Menu</summary><a href="/help">Help</a></details>',
    '<button type="button">Menu</button>',
    "Call 911.",
    '<a href="tel:911">911</a>',
    '<!-- <form method="post"> -->',
    "Version 0.2 (draft) · November 13, 2026 · Not yet in effect",
]


def cleared(html: str) -> str:
    return stage.strip_numbers(stage.strip_requests(html, DOMAIN))


@pytest.mark.parametrize("html", FILTERED + CHECK_ONLY)
def test_td_web_02_check_fails_each_planted_page(html: str) -> None:
    assert stage.check_page(f"<main>{html}</main>", DOMAIN), html


@pytest.mark.parametrize("html", FILTERED)
def test_td_web_02_filter_leaves_nothing_the_check_flags(html: str) -> None:
    assert stage.check_page(f"<main>{cleared(html)}</main>", DOMAIN) == [], html


@pytest.mark.parametrize("html", CLEAN)
def test_td_web_02_clean_pages_pass_unchanged(html: str) -> None:
    assert stage.check_page(html, DOMAIN) == []
    assert cleared(html) == html


def test_td_web_02_link_is_replaced_by_its_text() -> None:
    assert cleared('<p><a href="sms:+19285550142">Text us</a></p>') == "<p>Text us</p>"


def test_td_web_02_each_list_b_pattern_has_a_planted_case() -> None:
    for name, pattern in stage.list_b(DOMAIN):
        assert any(pattern.search(h) for h in FILTERED + CHECK_ONLY), name


@pytest.mark.parametrize("name", ["TEN_DIGIT", "SEVEN_DIGIT", "TEL_LINK"])
def test_td_web_02_phone_patterns_equal_t152s(name: str) -> None:
    ours, theirs = getattr(stage, name), getattr(t152, name)
    assert (ours.pattern, ours.flags) == (theirs.pattern, theirs.flags)


REQUEST_PAGE = """<html><body><main><h1>Tell us the job.</h1><h2>How we'll text you</h2>
<div lang="es"><p>Texto de consentimiento.</p></div>
<p data-fixed="WEB-014">Full consent text.</p>
<p>You must be 18 or older to use Laftly.</p>
<div id="chat-notice" role="status"></div>
<form method="post" action="/request"><input type="hidden" name="csrf" value="x">
<textarea name="words"></textarea><button type="submit">Send</button></form>
<footer class="site-footer"><p>Footer</p></footer></main></body></html>"""


def test_td_web_02_request_page_keeps_consent_and_adds_opening_soon() -> None:
    page = stage.filter_page("/request", REQUEST_PAGE, DOMAIN)
    for kept in ("Full consent text.", "Texto de consentimiento.", "You must be 18", "Requests open soon", "Footer"):
        assert kept in page
    assert "chat-notice" not in page and "<form" not in page


def test_td_web_02_filter_page_fails_when_a_page_still_holds_a_form(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(stage, "strip_requests", lambda html, domain: html)
    with pytest.raises(ValueError, match="form element"):
        stage.filter_page("/help", "<form></form>", DOMAIN)


def test_td_web_02_site_paths_come_from_the_site_get_rows() -> None:
    table = "## 13. Routes\n| GET | / | site | yes |\n| POST | /request | site | yes |\n| GET | /events | site | yes |\n"
    table += "| GET | /static/site/site.css | site | yes |\n| GET | /q/{token} | portals | yes |\n## 14. Next\n"
    assert stage.site_paths(table) == ["/"]
    with pytest.raises(ValueError, match=r"\{id\}"):
        stage.site_paths("## 13. Routes\n| GET | /x/{id} | site | yes |\n")


def test_td_web_02_architecture_site_rows_have_no_param() -> None:
    paths = stage.site_paths((ROOT / "docs" / "architecture.md").read_text(encoding="utf-8"))
    assert "/" in paths and "/request" in paths and not any(p in paths for p in stage.LEFT_OUT)


def test_td_web_02_a_js_asset_fails_the_build(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(stage, "fetch", lambda base, path: b'<link href="/static/site/app.js">')
    failures = stage.build("http://x", tmp_path, "## 13. Routes\n| GET | / | site | yes |\n", DOMAIN)
    assert any("app.js" in f for f in failures)


def test_td_web_02_build_writes_pages_and_assets(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(stage, "fetch", lambda base, path: b'<link href="/static/site/site.css"><p>Hi</p>')
    arch = "## 13. Routes\n| GET | / | site | yes |\n| GET | /terms | site | yes |\n"
    assert stage.build("http://x", tmp_path, arch, DOMAIN) == []
    assert (tmp_path / "index.html").exists() and (tmp_path / "terms" / "index.html").exists()
    assert (tmp_path / "static" / "site" / "site.css").exists()


def test_td_web_02_dns_file_keeps_search_console_verification() -> None:
    assert "google-site-verification" in DNS


def test_td_sec_29_every_web_hostname_is_dns_only() -> None:
    rows = {re.match(r"`([^`]+)`", r[1]).group(1): r for r in (_cells(l) for l in DNS.splitlines() if l.startswith("| CNAME"))}
    for host in (DOMAIN, f"www.{DOMAIN}", f"hooks.{DOMAIN}"):
        assert host in rows, host
        assert rows[host][-1] == "DNS-only", host
    assert not re.search(r"\|\s*Proxied\s*\|", DNS)


def _cells(line: str) -> list[str]:
    return [c.strip() for c in line.strip().strip("|").split("|")]


def test_td_sec_29_caa_allows_only_lets_encrypt() -> None:
    caa = [_cells(l) for l in DNS.splitlines() if l.startswith("| CAA")]
    assert caa and all('"letsencrypt.org"' in row[2] for row in caa)
    assert len(re.findall(r"\bissue(?:wild)?\s+\"", DNS)) == len(caa)


def test_td_sec_29_hsts_stays_off_and_no_preload() -> None:
    flat = " ".join(DNS.split())
    assert "Cloudflare's HSTS setting stays off" in flat
    assert f"{DOMAIN} is not submitted to the HSTS preload list before the pilot ends" in flat


def test_td_ops_21_decision_names_inbound_placement_and_every_sender() -> None:
    log = (ROOT / "docs" / "decisions.md").read_text(encoding="utf-8")
    entry = next(l for l in log.splitlines() if l.startswith("- **D-4914**"))
    assert "OPS-045" in entry and f"inbound.{DOMAIN}" in entry
    assert "received" in entry and "subdomain" in entry
    for sender in ("Resend", f"notify.{DOMAIN}", f"help@{DOMAIN}"):
        assert sender in entry, sender
    assert f"inbound.{DOMAIN}" in DNS and f"notify.{DOMAIN}" in DNS
