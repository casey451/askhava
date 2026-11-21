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


# One planted case per List B pattern (D-2506), in the order of the 17 patterns of D-4258 (2); each matches
# only its own pattern, so deleting a pattern from the script leaves a planted case unmatched.
SOLO = [
    ("form element", "<form></form>"),
    ("form attribute", '<div formaction="/x"></div>'),
    ("field", '<input type="text">'),
    ("button not type=button", "<button>Send</button>"),
    ("button with form", '<button type="button" form="f1">Send</button>'),
    ("script", "<script></script>"),
    ("event attribute", '<p onclick="x()">Hi</p>'),
    ("frame", '<iframe src="/x"></iframe>'),
    ("network call", "new XMLHttpRequest()"),
    ("message or call link", '<a href="sms:abc">Text</a>'),
    ("phone link", '<a href="tel:abc">Call</a>'),
    ("form service", "see forms.gle/abc"),
    ("mailto other than help", '<a href="mailto:x@example.com">Mail</a>'),
    ("javascript link", "javascript:void(0)"),
    ("texting number", "9285550142"),
    ("texting number", "555-0142"),
    ("texting number", "tel:+x"),
]


def test_td_web_02_each_list_b_pattern_has_a_planted_case() -> None:
    patterns = stage.list_b(DOMAIN)
    assert [name for name, _ in patterns] == [name for name, _ in SOLO]
    for index, (name, html) in enumerate(SOLO):
        matching = [i for i, (_, pattern) in enumerate(patterns) if pattern.search(html)]
        assert matching == [index], f"{name}: {html} matched patterns {matching}"


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
    page = (
        b'<link href="/static/site/site.css"><p>Hi</p>'
        b'<picture><source srcset="/static/site/photos/p.webp 1x, /static/site/photos/q.webp 2x"></picture>'
    )
    css = b'@font-face { src: url("fonts/f.woff2") format("woff2"); } a { background: url(data:image/png;base64,AA) }'
    monkeypatch.setattr(stage, "fetch", lambda base, path: css if path.endswith(".css") else page if path in "/terms" else b"x")
    arch = "## 13. Routes\n| GET | / | site | yes |\n| GET | /terms | site | yes |\n"
    assert stage.build("http://x", tmp_path, arch, DOMAIN) == []
    assert (tmp_path / "index.html").exists() and (tmp_path / "terms" / "index.html").exists()
    for asset in ("site.css", "fonts/f.woff2", "photos/p.webp", "photos/q.webp"):
        assert (tmp_path / "static" / "site" / asset).exists(), asset


def test_td_web_02_assets_in_reads_srcset_and_css_urls() -> None:
    assert stage.assets_in('<source srcset="/static/site/a.webp 1x,/static/site/b.webp 2x">') == [
        "/static/site/a.webp",
        "/static/site/b.webp",
    ]
    assert stage.css_assets('src: url("fonts/a.woff2"); b: url(/static/site/c.png); d: url(https://x/y)', "/static/site/site.css") == [
        "/static/site/c.png",
        "/static/site/fonts/a.woff2",
    ]


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
    assert "OPS-045" in entry and "PRIV-026" in entry and "webhook" in entry
    assert HELP in entry and f"notify.{DOMAIN}" in entry and "Resend" in entry
    assert "No human mailbox" in entry


def test_td_ops_21_root_domain_mail_reaches_the_app_and_has_records() -> None:
    rows = {(_cells(l)[0], _cells(l)[1]): _cells(l) for l in DNS.splitlines() if l.startswith(("| MX", "| TXT"))}
    mx = rows[("MX", f"`{DOMAIN}`")]
    assert "<from the email provider" in mx[2] and HELP in mx[3]
    assert ("TXT (SPF)", f"`{DOMAIN}`") in rows
    assert any(k[0] == "TXT (DKIM)" and f"_domainkey.{DOMAIN}" in k[1] for k in rows)
    assert ("MX, TXT (SPF, DKIM)", f"`notify.{DOMAIN}`") in rows
    assert f"inbound.{DOMAIN}" not in DNS


def test_td_web_02_assets_read_import_string_image_set_and_bare_srcset() -> None:
    css = '@import "extra.css"; a { background-image: image-set("a.webp" 1x, "b.webp" 2x) }'
    assert stage.css_assets(css, "/static/site/site.css") == [
        "/static/site/a.webp",
        "/static/site/b.webp",
        "/static/site/extra.css",
    ]
    assert stage.assets_in("<img srcset=/static/site/a.webp>") == ["/static/site/a.webp"]


def test_td_web_02_escaping_asset_path_raises() -> None:
    with pytest.raises(ValueError, match="leaves"):
        stage.assets_in('<img src="/static/site/../../../escaped.png">')
    with pytest.raises(ValueError, match="leaves"):
        stage.css_assets("a { background: url(../../x.png) }", "/static/site/site.css")


def test_td_web_02_escaping_asset_path_fails_the_build_and_writes_nothing(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    out = tmp_path / "a" / "out"
    monkeypatch.setattr(stage, "fetch", lambda base, path: b'<img src="/static/site/../../../escaped.png">')
    failures = stage.build("http://x", out, "## 13. Routes\n| GET | / | site | yes |\n", DOMAIN)
    assert any("leaves" in f for f in failures)
    assert not list(tmp_path.rglob("escaped.png"))


def test_td_web_02_write_inside_refuses_a_destination_outside_out(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="outside"):
        stage.write_inside(tmp_path / "out", "/../escaped.png", b"x")
    assert not (tmp_path / "escaped.png").exists()
