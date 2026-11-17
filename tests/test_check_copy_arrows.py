"""check_copy's arrow rule: one aria-hidden arrow inside a primary button, every other arrow fails (WEB-029, D-4246)."""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import check_copy as cc  # noqa: E402

ALLOWED = '<button class="button button-wide">Get quotes <span aria-hidden="true">→</span></button>'


def plant(root: Path, rel: str, text: str) -> list[str]:
    """WEB-029: check_copy's arrow problems for one planted fake file in an empty root."""
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return cc.arrow_problems(root)


def test_allowed_button_passes(tmp_path: Path) -> None:
    """WEB-029: the primary button's one aria-hidden arrow after its words passes, in es_*.html too."""
    assert plant(tmp_path, "app/site/templates/home.html", f"<form>\n  {ALLOWED}\n</form>") == []
    assert plant(tmp_path, "app/site/templates/es_home.html", "<p>«Hola, vecino»</p>") == []
    assert cc.run(tmp_path) == []


@pytest.mark.parametrize("body", [
    '<a href="/request">Get quotes →</a>',
    '<button class="button">Get quotes <span>→</span></button>',
    '<p>Get quotes <span aria-hidden="true">→</span></p>',
    '<button class="button">Get quotes <span aria-hidden="true">→</span><span aria-hidden="true">→</span></button>',
    '<button class="button"><span aria-hidden="true">→</span> Get quotes</button>',
    '<button class="button button-quiet">Back <span aria-hidden="true">→</span></button>',
    '<button>Send <span aria-hidden="true">→</span></button>',
    '<button class="button">Send <span aria-hidden="true">→→</span></button>',
    "<p>Your request -> a quote</p>",
    "<p>Pick =&gt; pay</p>",
    '<p>Ir al inicio <span aria-hidden="true">←</span></p>',
])
def test_template_arrows_fail(tmp_path: Path, body: str) -> None:
    """WEB-029: an arrow in a link, a bare or out-of-button span, a second or leading arrow, a quiet or
    class-less button, or typed text fails, and the problem names the file."""
    problems = plant(tmp_path, "app/site/templates/es_page.html", body)
    assert problems and all(p.startswith("app/site/templates/es_page.html: arrow") for p in problems), problems


def test_arrow_in_a_tag_fails(tmp_path: Path) -> None:
    """WEB-029: an arrow in an attribute (an accessible name) fails."""
    problems = plant(tmp_path, "app/portals/templates/page.html", '<a href="/x" aria-label="Next →">Next</a>')
    assert problems == ['app/portals/templates/page.html: arrow in a tag: <a href="/x" aria-label="Next →">']


def test_site_copy_arrow_fails(tmp_path: Path) -> None:
    """WEB-029: an arrow in a site-copy line fails, fixed blocks included."""
    copy = "# Copy\n\n## Home · hero (homeowner)\n\n> Tell us the job → we text local pros.\n"
    problems = plant(tmp_path, "docs/copy/site-copy.md", copy)
    assert problems == ["docs/copy/site-copy.md · Home · hero: arrow in copy: Tell us the job → we text local pros."]
    assert plant(tmp_path, "docs/copy/site-copy.md", copy.replace("→", "&") + "## A · b (homeowner, fixed)\n\n> x -> y\n")
    assert plant(tmp_path, "docs/copy/site-copy.md", copy.replace("→", "&rarr;"))
    assert plant(tmp_path, "docs/copy/site-copy.md", copy.replace("→", "-&gt;"))


@pytest.mark.parametrize("style", ['<style>a::after{content:"→"}</style>',
                                   r'<style media="all">a::after{content:"\2192"}</style>'])
def test_template_style_block_arrow_fails(tmp_path: Path, style: str) -> None:
    """WEB-029: an arrow or its CSS escape in a content: value inside a template's <style> block fails."""
    problems = plant(tmp_path, "app/site/templates/a.html", f"<head>{style}</head><p>Hi</p>")
    assert problems and problems[0].startswith("app/site/templates/a.html: arrow in CSS"), problems


@pytest.mark.parametrize("css", ['a::after{content:"→"}', r'a::after { content: "\2192"; }',
                                 r"a::before{content:'\2190'}", 'a::after{content:" ->"}'])
def test_stylesheet_content_arrow_fails(tmp_path: Path, css: str) -> None:
    """WEB-029: an arrow or its CSS escape in a content: value fails; other content passes."""
    assert plant(tmp_path, "app/site/static/site.css", css)[0].startswith("app/site/static/site.css: arrow in CSS")
    assert plant(tmp_path, "app/site/static/site.css", 'a::after{content:"";justify-content:space-between}') == []


@pytest.mark.parametrize("literal", ['"a::after{content:\\"→\\"}\\\n"', '"a::after{content:\\"\\u{2192}\\"}"',
                                     '"a::after{content:\\"\\\\2192\\"}"', 'r#"a::after{content:"\\2192"}"#'])
def test_rust_css_constant_arrow_fails(tmp_path: Path, literal: str) -> None:
    """WEB-029: a FORMS_CSS-style Rust constant with an arrow in a content: value fails; the real ones pass."""
    rust = f"//! Fake.\nconst FORMS_CSS: &str = {literal};\n"
    assert plant(tmp_path, "app/portals/src/feedback.rs", rust) == [
        next(p for p in cc.arrow_problems(tmp_path) if p.startswith("app/portals/src/feedback.rs: arrow in CSS"))]
    assert cc.style_arrow_problems(REPO_ROOT) == []


def test_repository_passes() -> None:
    """WEB-029: the real copy, templates and stylesheets hold no arrow outside the allowed place."""
    assert cc.arrow_problems(REPO_ROOT) == []
