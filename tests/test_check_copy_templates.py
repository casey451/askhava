"""check_copy's template exemptions: Spanish and data-fixed text (TD-WORD-31; WORD-029, T519, D-3878, D-3905)."""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import check_copy as cc  # noqa: E402

WEB_005 = ("Launch offer: your first win is free, on any job size, when you join in the first 90 days "
           "or within 90 days after your trade opens.")
SPANISH_HARD = ("La documentación administrativa correspondiente de los procedimientos institucionales es "
                "responsabilidad de la organización para la coordinación interdepartamental que usted solicita "
                "en su comunicación electrónica.")
ENGLISH_BAD = "Best pros in La Paz County, fast."  # a Spanish list word, no English one


def run(tmp_path: Path, body: str, name: str = "page.html") -> list[str]:
    """TD-WORD-31: check_copy's problems for one planted fake template, with the real website spec."""
    (tmp_path / "docs/spec").mkdir(parents=True, exist_ok=True)
    shutil.copy(REPO_ROOT / "docs/spec/website.md", tmp_path / "docs/spec/website.md")
    (tmp_path / "app/site/templates").mkdir(parents=True, exist_ok=True)
    (tmp_path / "app/site/templates" / name).write_text(body, encoding="utf-8")
    return cc.run(tmp_path)


def test_repository_templates_pass() -> None:
    """TD-WORD-31: the real page templates pass."""
    assert cc.template_problems(REPO_ROOT) == [] and cc.run(REPO_ROOT) == []


def test_fixed_wording(tmp_path: Path) -> None:
    """TD-WORD-31: 27 unmarked words fail; data-fixed="WEB-005" passes, an expression standing for one word."""
    assert any("27-word sentence" in p for p in run(tmp_path, f"<p>{WEB_005}</p>"))
    assert run(tmp_path, f'<p data-fixed="WEB-005">{WEB_005}</p>') == []
    note = "{{ brand }} isn't an emergency service and doesn't judge what needs an <strong>emergency</strong> response."
    assert run(tmp_path, f'<p class="n" data-fixed="WEB-022">{note}</p>') == []


def test_bad_markers_fail(tmp_path: Path) -> None:
    """TD-WORD-31: text its requirement does not hold, an unknown ID or an unclean element fails and is measured."""
    problems = run(tmp_path, f'<p data-fixed="WEB-005">{WEB_005} We are the best.</p>')
    assert any('"WEB-005" text is not word for word' in p for p in problems)
    assert any("claim word 'best'" in p for p in problems)
    assert any('"WEB-999" names no requirement' in p for p in run(tmp_path, f'<p data-fixed="WEB-999">{WEB_005}</p>'))
    assert any("own end tag" in p for p in run(tmp_path, f'<div data-fixed="WEB-005"><p>{WEB_005}</p></div>'))


def test_spanish_is_skipped(tmp_path: Path) -> None:
    """TD-WORD-31: an over-grade Spanish passage fails unmarked, passes in lang="es" and in es_x.html; English
    after the lang="es" element closes, or in a nested lang="en", is measured; lang="es" on 5 words or more
    with no common Spanish word fails."""
    assert any("reading grade" in p for p in run(tmp_path, f"<p>{SPANISH_HARD}</p>"))
    assert run(tmp_path, f'<p lang="es">{SPANISH_HARD}</p>') == []
    assert run(tmp_path, f"<p>{SPANISH_HARD}</p>", "es_x.html") == []
    after = run(tmp_path, f'<p lang="es">{SPANISH_HARD}</p>\n<p>The best fee.</p>')
    assert after == ["app/site/templates/page.html · text 4: claim word 'best' (say the fact instead)"]
    nested = run(tmp_path, f'<p lang="es">{SPANISH_HARD} <span lang="en">The best fee.</span></p>')
    assert any("claim word 'best'" in p for p in nested)
    problems = run(tmp_path, '<p lang="es">Send us your job today, it is quick.</p>')
    assert any('lang="es" text holds no common Spanish word' in p for p in problems)
    assert run(tmp_path, '<p lang="es">Gracias, vecino.</p>') == []


def test_no_branch_or_implied_end_tag_stretches_an_exemption(tmp_path: Path) -> None:
    """TD-WORD-31: the T519 attempt-1 review's inputs, and English holding a Spanish word, are measured."""
    branch = ('{% if es %}<div lang="es">{% else %}<div>{% endif %}<p>Bienvenido a la casa</p></div>'
              f"<p>{ENGLISH_BAD}</p>")
    implied = f'<dl><dd lang="es">uno de dos<dd>{ENGLISH_BAD}</dl>'
    same_name = f'<span lang="es">uno de dos<span>{ENGLISH_BAD}</span></span>'
    for body in (branch, implied, same_name, '<p lang="es">We send the best pros in La Paz County.</p>'):
        assert any("claim word 'best'" in p for p in run(tmp_path, body)), body
