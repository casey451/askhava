"""Build the Stage 0.5 static page set from a running server (WEB-013, WEB-017, TD-WEB-02, D-3155, D-4258).

Before Gate C the site shows every public page of the final site as static files: no texting number
and nothing that takes a request. The pages are Askama-rendered by the `web` process, so this script
fetches each public page (the `site` GET rows of docs/architecture.md section 13, GET only) and the
/static/site/ assets they use from a server run by hand on Casey's go (a local `cargo run` against the
stand-ins), then applies the Stage 0.5 filters, which are pure functions:

- `strip_requests` removes every form (any method), every field outside a form, every script, every
  event attribute, every frame, and every link that sends or calls a person, replacing a link by its text;
  `strip_numbers` removes the texting number; `reduce_request_page` keeps the request page's full consent
  text and adds an "opening soon" line.
- `check_page` is the closed List B of D-4258: a filtered page that still matches any pattern fails the build.
- A `.js` asset fails the build outright; a `site` row with a `{param}` path fails, named.
- /categories and /events (served with the home page, T102) are left out of the static set.

Publishing waits for Casey's separate go; this script never uploads anything.

Usage: python3 scripts/build_stage05.py --base-url http://127.0.0.1:8080 --out <folder>
Exit status 1 names each failure.
"""

from __future__ import annotations

import argparse
import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(Path(__file__).resolve().parent))
import brand  # noqa: E402  (the scripts folder must be on the path first)

LEFT_OUT = ("/categories", "/events")
OPENING_SOON = "<p>Requests open soon. This page takes no requests yet.</p>"
FLAGS = re.IGNORECASE | re.DOTALL

# Copied exactly from tests/docs/test_no_phone_numbers_in_templates.py (T152); test_stage05.py asserts equality.
TEN_DIGIT = re.compile(r"(?<![\w.])(?:\+?1[\s.-]?)?\(?\d{3}\)?[\s.-]?\d{3}[\s.-]?\d{4}(?![\w])")
SEVEN_DIGIT = re.compile(r"(?<![\w.-])\d{3}[\s.-]\d{4}(?![\w-])")
TEL_LINK = re.compile(r"\btel:\s*(?!911\b)[+\d(]", re.IGNORECASE)

SCHEMES = "sms|smsto|mms|callto|facetime|facetime-audio|whatsapp|skype|tg|viber|imessage"
SERVICES = (
    r"(?:wa\.me|api\.whatsapp\.com|m\.me/|forms\.gle|docs\.google\.com/forms|typeform\.com|"
    r"jotform\.com|calendly\.com|tally\.so|formspree\.io|airtable\.com/shr)"
)


def link_patterns(domain: str) -> list[re.Pattern[str]]:
    """The List B patterns a link can match: the ones a filtered link is replaced by its text for."""
    help_mail = re.escape(f"help@{domain}")
    sources = [
        rf"\b(?:href|action|src)\s*=\s*[\"']?\s*(?:{SCHEMES}):",
        r"\b(?:href|action|src)\s*=\s*[\"']?\s*tel:(?!911[\"'\s>])",
        SERVICES,
        rf"\bhref\s*=\s*[\"']?\s*mailto:(?!{help_mail}[\"'\s>])",
        r"javascript:",
    ]
    return [re.compile(s, FLAGS) for s in sources]


def list_b(domain: str) -> list[tuple[str, re.Pattern[str]]]:
    """Every closed List B pattern of D-4258 (2), each with its name; all case-insensitive and dot-all."""
    sources = {
        "form element": r"<form\b",
        "form attribute": r"\bform(?:action|method|enctype|target)\s*=",
        "field": r"<(?:input|textarea|select|datalist|output)\b",
        "button not type=button": r"<button\b(?![^>]*\btype\s*=\s*[\"']?button\b)",
        "button with form": r"<button\b[^>]*\bform\s*=",
        "script": r"<script\b",
        "event attribute": r"\son[a-z]+\s*=",
        "frame": r"<(?:iframe|frame|embed|object|portal)\b",
        "network call": r"\bfetch\s*\(|XMLHttpRequest|\bsendBeacon\b|\bEventSource\b|\bWebSocket\b|\bimport\s*\(",
    }
    named = [(name, re.compile(src, FLAGS)) for name, src in sources.items()]
    link_names = ["message or call link", "phone link", "form service", "mailto other than help", "javascript link"]
    named += list(zip(link_names, link_patterns(domain)))
    named += [("texting number", TEN_DIGIT), ("texting number", SEVEN_DIGIT), ("texting number", TEL_LINK)]
    return named


COMMENT = re.compile(r"<!--.*?-->", FLAGS)
PAIRED = ("form", "textarea", "select", "datalist", "output", "script", "iframe", "object", "portal")
VOID = ("input", "embed", "frame")
EVENT_ATTR = re.compile(r"\son[a-z]+\s*=\s*(?:\"[^\"]*\"|'[^']*'|[^\s>]+)", FLAGS)
BUTTON_KEEP = re.compile(r"<button\b(?![^>]*\b(?:form|formaction)\s*=)[^>]*\btype\s*=\s*[\"']?button\b", FLAGS)
ANCHOR = re.compile(r"<a\b[^>]*>(.*?)</a\s*>", FLAGS)


def _drop_paired(html: str, tag: str) -> str:
    """Remove each `<tag>...</tag>` element, innermost-first until none is left, then stray tags."""
    element = re.compile(rf"<{tag}\b[^>]*>(?:(?!<{tag}\b).)*?</{tag}\s*>", FLAGS)
    while True:
        html, count = element.subn("", html)
        if not count:
            break
    return re.sub(rf"</?{tag}\b[^>]*>", "", html, flags=FLAGS)


def _drop_buttons(html: str) -> str:
    """Remove every button element except `type="button"` without `form` or `formaction`."""

    def swap(match: re.Match[str]) -> str:
        return match.group(0) if BUTTON_KEEP.match(match.group(0)) else ""

    return re.sub(r"<button\b[^>]*>.*?</button\s*>", swap, html, flags=FLAGS)


def _unlink(html: str, domain: str) -> str:
    """Replace each link that sends, calls or opens a form service by its text (the help address stays)."""
    patterns = link_patterns(domain)

    def swap(match: re.Match[str]) -> str:
        opening = match.group(0)[: match.group(0).index(">") + 1]
        return match.group(1) if any(p.search(opening) for p in patterns) else match.group(0)

    return ANCHOR.sub(swap, html)


def strip_requests(html: str, domain: str) -> str:
    """Remove everything on a page that takes a request (D-4258 (1)); comments are left as they are."""
    held: list[str] = []

    def hold(match: re.Match[str]) -> str:
        held.append(match.group(0))
        return f"\x00{len(held) - 1}\x00"

    html = COMMENT.sub(hold, html)
    html = _drop_buttons(html)
    for tag in PAIRED:
        html = _drop_paired(html, tag)
    html = re.sub(rf"<(?:{'|'.join(VOID)})\b[^>]*>", "", html, flags=FLAGS)
    html = EVENT_ATTR.sub("", html)
    html = _unlink(html, domain)
    return re.sub(r"\x00(\d+)\x00", lambda m: held[int(m.group(1))], html)


def strip_numbers(html: str) -> str:
    """Remove the texting number, in any written form (WEB-017: none shows until Gate C)."""
    for pattern in (TEN_DIGIT, SEVEN_DIGIT, TEL_LINK):
        html = pattern.sub("", html)
    return html


def reduce_request_page(html: str) -> str:
    """Keep the request page's heading, full consent text and age line, add the opening-soon line (WEB-017)."""
    start, end = html.find("<h1"), html.find('<footer class="site-footer"')
    if start < 0 or end < start:
        raise ValueError("request page has no <h1> before its footer")
    body = html[start:end]
    keep = [
        re.search(r"<h1\b.*?</h1>", body, FLAGS),
        re.search(r"<h2>How we.ll text you</h2>", body, FLAGS),
        re.search(r"<div lang=\"es\">.*?</div>", body, FLAGS),
        re.search(r"<p data-fixed=\"WEB-014\">.*?</p>", body, FLAGS),
        re.search(r"<p>You must be 18 or older[^<]*</p>", body, FLAGS),
    ]
    if not all(keep):
        raise ValueError("request page lacks its heading, consent text or age line")
    kept = "\n".join(m.group(0) for m in keep if m)
    return html[:start] + kept + "\n" + OPENING_SOON + "\n" + html[end:]


def check_page(html: str, domain: str) -> list[str]:
    """List B (D-4258 (2)): the name of each pattern a filtered page still matches, comments removed."""
    text = COMMENT.sub("", html)
    return [name for name, pattern in list_b(domain) if pattern.search(text)]


def filter_page(path: str, html: str, domain: str) -> str:
    """Filter one fetched page, then fail when anything List B names is left."""
    if path == "/request":
        html = reduce_request_page(html)
    html = strip_numbers(strip_requests(html, domain))
    left = check_page(html, domain)
    if left:
        raise ValueError(f"{path} still holds: {', '.join(left)}")
    return html


def site_paths(architecture: str) -> list[str]:
    """The `site` GET rows of docs/architecture.md section 13, minus assets and the left-out pages."""
    section = architecture.split("## 13. Routes", 1)[1].split("\n## ", 1)[0]
    paths = []
    for row in re.finditer(r"^\| GET \| (\S+) \| site \|", section, re.MULTILINE):
        path = row.group(1)
        if "{" in path:
            raise ValueError(f"site row {path} has a {{param}} path: no static page can be built for it")
        if not path.startswith("/static/") and path not in LEFT_OUT:
            paths.append(path)
    return paths


def assets_in(html: str) -> list[str]:
    """The /static/site/ files a page or stylesheet points at."""
    found = re.findall(r"(?:href|src|url)\s*[=(]\s*[\"']?(/static/site/[^\"'#?)\s>]+)", html, FLAGS)
    return sorted(set(found))


def page_file(out: Path, path: str) -> Path:
    """Where a page lands: `/` is index.html, `/terms` is terms/index.html."""
    return out / path.strip("/") / "index.html"


def fetch(base_url: str, path: str) -> bytes:
    """GET one path from the running server."""
    with urllib.request.urlopen(base_url.rstrip("/") + path, timeout=30) as response:  # noqa: S310  (a local server)
        return response.read()


def build(base_url: str, out: Path, architecture: str, domain: str) -> list[str]:
    """Fetch, filter and write every page and asset; return each failure."""
    failures: list[str] = []
    assets = {"/static/site/site.css"}
    for path in site_paths(architecture):
        try:
            html = filter_page(path, fetch(base_url, path).decode("utf-8"), domain)
        except (ValueError, OSError) as error:
            failures.append(str(error) if isinstance(error, ValueError) else f"{path}: {error}")
            continue
        assets.update(assets_in(html))
        target = page_file(out, path)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(html, encoding="utf-8")
    for asset in sorted(assets):
        if asset.lower().endswith(".js"):
            failures.append(f"{asset} is a script: the Stage 0.5 set ships none")
            continue
        try:
            data = fetch(base_url, asset)
        except OSError as error:
            failures.append(f"{asset}: {error}")
            continue
        target = out / asset.lstrip("/")
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    return failures


def main(argv: list[str] | None = None) -> int:
    """Command line: build the set, print each failure, exit 1 on any."""
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--base-url", required=True, help="a running server, e.g. http://127.0.0.1:8080")
    parser.add_argument("--out", required=True, type=Path, help="the folder to write the static set into")
    args = parser.parse_args(argv)
    architecture = (ROOT / "docs" / "architecture.md").read_text(encoding="utf-8")
    try:
        failures = build(args.base_url, args.out, architecture, brand.load()["domain"])
    except ValueError as error:
        failures = [str(error)]
    for failure in failures:
        print(f"stage 0.5: {failure}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
