"""Check the expert layer: docs/catalog/expert/<category>.yaml and scenarios/<category>.yaml.

What it checks (schema 1, docs/catalog/expert/schema.yaml):
- every enabled category in docs/catalog/categories.yaml has an expert file, and no file names an unknown category
- field shapes and limits; every bank question, photo, kb job type, category job type,
  follow-up id and source id it names exists
- the quote format matches the category (or its job_type_formats entry)
- customer-facing strings carry no banned word (WORD-003), no AI-smell hit (docs/ai_smell.yaml),
  no cause word, no dollar figure to a homeowner and no safety advice (AI-019, D-436)
- scenario files: every scenario names a real category and expected job type, covers the
  required scenario kinds, and holds no phone-shaped number or email (fake data only)

Each problem prints as path: message. Exit 1 on any problem.

Usage: python3 scripts/check_expert.py [--root REPO_ROOT]
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path
from typing import Any

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_ai_smell import load_lists, scan_text  # noqa: E402

EXPERT_DIR = Path("docs/catalog/expert")
SCENARIO_DIR = EXPERT_DIR / "scenarios"
KB_DIR = Path("docs/catalog/trade_kb/trades")

BANNED = [
    "vetted", "pre-screened", "prescreened", "pre-vetted", "leads", "lead generation", "exclusive",
    "best", "recommended", "background-checked", "trusted", "top-rated", "top rated", "#1",
    "referral network", "connect you with", "concierge", "commission",
]
BANNED_RE = re.compile(r"(?<![\w-])(" + "|".join(re.escape(b) for b in BANNED) + r")(?![\w-])", re.I)
CAUSE_RE = re.compile(
    r"\b(probably|likely|most likely|sounds like|the problem is|you need a new|you'll need|it's the|"
    r"it is the|that means|caused by|is failing|has failed|is broken|es probable|seguramente)\b",
    re.I,
)
DOLLAR_RE = re.compile(r"\$\s?\d|\b\d+\s?dollars\b|\bcost(s)? (about|around)\b", re.I)
ADVICE_RE = re.compile(
    r"\b(you should|make sure to|be sure to|call 911|get out of|evacuate|don't touch|do not touch|"
    r"turn off the (main|gas|power|breaker)|shut off the (main|gas|power|breaker))\b",
    re.I,
)
PHONE_RE = re.compile(r"(?:\d[\s.\-()]*){7,}")
EMAIL_RE = re.compile(r"[\w.+-]+@[\w-]+\.[\w.]+")

QUOTE_FORMATS = {"range", "monthly", "tiers", "firm", "diagnostic_visit"}
SOURCE_KINDS = {
    "statute", "regulator", "code_standard", "manufacturer", "trade_association",
    "trade_publication", "forum", "other",
}
SCENARIO_KINDS = {
    "vague", "wrong_terminology", "wrong_self_diagnosis", "misleading_photo", "no_photo",
    "elderly", "tenant", "spanish", "panicked_non_emergency", "calm_real_emergency",
    "two_trades", "outside_area", "no_businesses",
}
# A category whose kb file has no 911-list trigger may mark calm_real_emergency "not applicable".
LIMITS = {
    "price_drivers": (4, 8), "hidden_conditions": (3, 8), "misdiagnosis_checks": (2, 8),
    "must_know": (2, 5), "allowed_follow_ups": (1, 4), "photos_priority": (0, 3), "never_say": (2, 6),
}


class Problems:
    def __init__(self) -> None:
        self.items: list[str] = []

    def add(self, path: str, message: str) -> None:
        self.items.append(f"{path}: {message}")


def load_yaml(path: Path) -> Any:
    return yaml.safe_load(path.read_text(encoding="utf-8"))


def load_catalog(root: Path) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    cats = {c["id"]: c for c in load_yaml(root / "docs/catalog/categories.yaml")["categories"]}
    sets = load_yaml(root / "docs/catalog/question_bank.yaml")["sets"]
    kb: dict[str, Any] = {}
    for path in sorted((root / KB_DIR).glob("*.yaml")):
        kb[path.stem] = load_yaml(path)
    return cats, sets, kb


def within(problems: Problems, path: str, name: str, items: Any) -> None:
    low, high = LIMITS[name]
    count = len(items) if isinstance(items, list) else -1
    if count < low or count > high:
        problems.add(path, f"{name} has {count} items, expected {low} to {high}")


def check_copy(problems: Problems, lists: Any, path: str, where: str, text: str, homeowner: bool) -> None:
    """Customer-facing wording rules. homeowner=True adds the no-price and no-advice rules."""
    if not isinstance(text, str):
        problems.add(path, f"{where}: expected text")
        return
    for hit in scan_text(lists, path, 0, text):
        problems.add(path, f"{where}: AI smell {hit.rule}: \"{hit.snippet}\"")
    found = BANNED_RE.search(text)
    if found:
        problems.add(path, f"{where}: banned word '{found.group(1)}'")
    found = CAUSE_RE.search(text)
    if found:
        problems.add(path, f"{where}: cause word '{found.group(1)}' (AI-019)")
    if homeowner:
        if DOLLAR_RE.search(text):
            problems.add(path, f"{where}: a price or dollar figure to a homeowner (AI-019)")
        found = ADVICE_RE.search(text)
        if found:
            problems.add(path, f"{where}: safety advice '{found.group(0)}' (D-436)")
        if "!" in text:
            problems.add(path, f"{where}: exclamation mark")
    if PHONE_RE.search(text) or EMAIL_RE.search(text):
        problems.add(path, f"{where}: phone-shaped number or email (fake data only, and never contact details)")


def kb_ids(kb_file: dict[str, Any] | None) -> tuple[set[str], set[str], set[str]]:
    if not kb_file:
        return set(), set(), set()
    jobs = {j["id"] for j in kb_file.get("job_types", [])}
    photos = {p["id"] for p in kb_file.get("photos_to_request", [])}
    questions = {q["id"] for q in kb_file.get("questions", [])}
    return jobs, photos, questions


def check_trade_brief(problems: Problems, lists: Any, path: str, brief: Any, ctx: dict[str, Any]) -> None:
    if not isinstance(brief, dict):
        problems.add(path, "trade_brief missing")
        return
    for name in ("price_drivers", "hidden_conditions", "misdiagnosis_checks"):
        within(problems, path, name, brief.get(name))
    for item in brief.get("price_drivers") or []:
        if not item.get("driver") or not item.get("why"):
            problems.add(path, "price_drivers item needs driver and why")
        check_source(problems, path, "price_drivers", item, ctx)
    for item in brief.get("hidden_conditions") or []:
        if not item.get("condition") or not item.get("how_a_pro_spots_it"):
            problems.add(path, "hidden_conditions item needs condition and how_a_pro_spots_it")
        ref = item.get("question_or_photo")
        if ref and ref not in ctx["bank_q"] | ctx["photos"] | ctx["follow_ups"]:
            problems.add(path, f"hidden_conditions names unknown question or photo '{ref}'")
        check_source(problems, path, "hidden_conditions", item, ctx, judgment_ok=True)
    for item in brief.get("misdiagnosis_checks") or []:
        for key in ("sounds_like", "might_be", "one_question"):
            if not item.get(key):
                problems.add(path, f"misdiagnosis_checks item needs {key}")
        if item.get("one_question") and item["one_question"] not in ctx["follow_ups"] | ctx["bank_q"]:
            problems.add(path, f"misdiagnosis_checks one_question '{item['one_question']}' is not a follow-up or bank question")
        route = item.get("if_other_route_to") or {}
        if route and not route.get("category") in ctx["cats"]:
            problems.add(path, f"misdiagnosis_checks routes to unknown category '{route.get('category')}'")
        elif route and route.get("job_type") not in cat_job_names(ctx["cats"][route["category"]]):
            problems.add(path, f"misdiagnosis_checks routes to unknown job type '{route.get('job_type')}' in {route['category']}")
        check_source(problems, path, "misdiagnosis_checks", item, ctx, judgment_ok=True)
    for item in brief.get("photo_reading") or []:
        if item.get("photo_id") not in ctx["photos"]:
            problems.add(path, f"photo_reading names unknown photo '{item.get('photo_id')}'")
        if item.get("neutral_wording"):
            check_copy(problems, lists, path, "photo_reading.neutral_wording", item["neutral_wording"], True)
    for item in brief.get("local_factors") or []:
        if not item.get("fact"):
            problems.add(path, "local_factors item needs fact")
        check_source(problems, path, "local_factors", item, ctx)
    season = brief.get("seasonality") or {}
    if not season.get("busy") or not season.get("slow"):
        problems.add(path, "seasonality needs busy and slow")


def cat_job_names(cat: dict[str, Any]) -> set[str]:
    names = set(cat.get("job_types") or [])
    for row in cat.get("refused") or []:
        names.add(row.get("job"))
    return names


def check_source(problems: Problems, path: str, where: str, item: dict[str, Any], ctx: dict[str, Any], judgment_ok: bool = False) -> None:
    """A source id must exist. With no source, the item is trade reasoning: it must say [judgment]
    (or be marked unverified), except in sections where every row is reasoning by nature."""
    src = item.get("source")
    if src is None:
        text = " ".join(str(item.get(k, "")) for k in ("why", "how_a_pro_spots_it", "effect", "fact", "figure", "one_question"))
        if not judgment_ok and "[judgment]" not in text and not item.get("unverified"):
            problems.add(path, f"{where}: no source and no [judgment] mark: {str(item)[:60]}")
    elif src not in ctx["sources"]:
        problems.add(path, f"{where}: unknown source id '{src}'")


def check_job_type(problems: Problems, lists: Any, path: str, job: dict[str, Any], ctx: dict[str, Any]) -> None:
    jid = job.get("id")
    where = f"job_types[{jid}]"
    if ctx["kb_jobs"] and jid not in ctx["kb_jobs"] and not job.get("new_job_type"):
        problems.add(path, f"{where}: not a job type in trade_kb/{ctx['kb_trade']}.yaml (set new_job_type: true to add one)")
    if job.get("bank_job_type") not in cat_job_names(ctx["cat"]):
        problems.add(path, f"{where}: bank_job_type '{job.get('bank_job_type')}' is not in categories.yaml for {ctx['cat']['id']}")
    within(problems, path, "must_know", job.get("must_know"))
    within(problems, path, "allowed_follow_ups", job.get("allowed_follow_ups"))
    within(problems, path, "photos_priority", job.get("photos_priority") or [])
    within(problems, path, "never_say", job.get("never_say"))
    fu_ids = {f if isinstance(f, str) else f.get("id") for f in job.get("allowed_follow_ups") or []}
    for ref in (f for f in job.get("allowed_follow_ups") or [] if isinstance(f, str)):
        if ref not in ctx["follow_ups"]:
            problems.add(path, f"{where}: allowed_follow_ups refers to '{ref}', which no job type defines")
    for fact in job.get("must_know") or []:
        if fact.get("bank_question") and fact["bank_question"] not in ctx["bank_q"]:
            problems.add(path, f"{where}: must_know names unknown bank question '{fact['bank_question']}'")
        if fact.get("follow_up") and fact["follow_up"] not in fu_ids:
            problems.add(path, f"{where}: must_know names unknown follow_up '{fact['follow_up']}'")
        if fact.get("photo") and fact["photo"] not in ctx["photos"]:
            problems.add(path, f"{where}: must_know names unknown photo '{fact['photo']}'")
        if not fact.get("why_it_moves_price"):
            problems.add(path, f"{where}: must_know item needs why_it_moves_price")
    for fu in (f for f in job.get("allowed_follow_ups") or [] if isinstance(f, dict)):
        for key in ("id", "ask_when", "text_en", "text_es", "why"):
            if not fu.get(key):
                problems.add(path, f"{where}: follow-up needs {key}")
        for key in ("text_en", "text_es"):
            if fu.get(key):
                check_copy(problems, lists, path, f"{where}.{fu.get('id')}.{key}", fu[key], True)
                if "?" not in fu[key]:
                    problems.add(path, f"{where}.{fu.get('id')}.{key}: a follow-up is a question")
        opts_en, opts_es = fu.get("options_en") or [], fu.get("options_es") or []
        if len(opts_en) != len(opts_es):
            problems.add(path, f"{where}.{fu.get('id')}: options_en and options_es differ in count")
        if opts_en and not 2 <= len(opts_en) <= 6:
            problems.add(path, f"{where}.{fu.get('id')}: 2 to 6 options")
        for opt in opts_en + opts_es:
            check_copy(problems, lists, path, f"{where}.{fu.get('id')}.option", str(opt), True)
    for pid in job.get("photos_priority") or []:
        if pid not in ctx["photos"]:
            problems.add(path, f"{where}: photos_priority names unknown photo '{pid}'")
    urgency = job.get("urgency") or {}
    for trig in urgency.get("fire_dept_triggers") or []:
        if not re.search(r"gas|propane|hiss|CO alarm|carbon monoxide|smoke|fire|flame|spark|burning|power line", str(trig), re.I):
            problems.add(path, f"{where}: fire_dept_triggers must be on the D-436 list: '{trig}'")
    for key in ("scope_line_en", "scope_line_es", "card_template"):
        if not job.get(key):
            problems.add(path, f"{where}: needs {key}")
        else:
            check_copy(problems, lists, path, f"{where}.{key}", job[key], True)
    fmt = job.get("quote_format")
    expected = (ctx["cat"].get("job_type_formats") or {}).get(job.get("bank_job_type"))
    if expected is None:
        expected = "firm" if job.get("bank_job_type") in (ctx["cat"].get("one_time_job_types") or []) else ctx["cat"].get("quote_format")
    if fmt not in QUOTE_FORMATS:
        problems.add(path, f"{where}: quote_format '{fmt}' not allowed")
    elif fmt != expected:
        problems.add(path, f"{where}: quote_format '{fmt}' but the category says '{expected}'")
    if not job.get("business_fills"):
        problems.add(path, f"{where}: needs business_fills")


def check_business_side(problems: Problems, lists: Any, path: str, side: Any, ctx: dict[str, Any]) -> None:
    if not isinstance(side, dict):
        problems.add(path, "business_side missing")
        return
    for key in ("how_they_get_customers", "customer_acquisition_benchmarks", "what_they_dislike_about_lead_services",
                "what_makes_a_lead_worth_paying_for", "their_words", "signup_objections"):
        if not side.get(key):
            problems.add(path, f"business_side needs {key}")
    for item in side.get("customer_acquisition_benchmarks") or []:
        if not item.get("figure"):
            problems.add(path, "customer_acquisition_benchmarks item needs figure")
        check_source(problems, path, "customer_acquisition_benchmarks", item, ctx)
    if not 3 <= len(side.get("signup_objections") or []) <= 12:
        problems.add(path, "signup_objections: 3 to 12 items")
    for item in side.get("signup_objections") or []:
        if not item.get("objection") or not item.get("answer"):
            problems.add(path, "signup_objections item needs objection and answer")
        else:
            check_copy(problems, lists, path, "signup_objections.answer", item["answer"], False)
    for tip in side.get("quote_tips_for_card") or []:
        check_copy(problems, lists, path, "quote_tips_for_card", str(tip), False)


def check_expert_file(problems: Problems, lists: Any, root: Path, path: Path, cats: dict[str, Any], sets: dict[str, Any], kb: dict[str, Any]) -> str | None:
    rel = str(path.relative_to(root))
    data = load_yaml(path)
    if not isinstance(data, dict) or data.get("schema") != 1:
        problems.add(rel, "schema: 1 expected")
        return None
    cid = data.get("category")
    if cid not in cats:
        problems.add(rel, f"unknown category '{cid}'")
        return None
    if path.stem != cid:
        problems.add(rel, f"file name should be {cid}.yaml")
    qset = sets.get(data.get("question_set"))
    if qset is None:
        problems.add(rel, f"unknown question_set '{data.get('question_set')}'")
        return cid
    kb_trade = data.get("kb_trade")
    kb_file = kb.get(kb_trade) if kb_trade else None
    if kb_trade and kb_file is None:
        problems.add(rel, f"kb_trade '{kb_trade}' has no file in {KB_DIR}")
    if qset.get("kb_trade") and qset["kb_trade"] != kb_trade:
        problems.add(rel, f"kb_trade '{kb_trade}' differs from the question set's '{qset['kb_trade']}'")
    kb_jobs, kb_photos, _ = kb_ids(kb_file)
    sources = {s.get("id") for s in data.get("sources") or []}
    for src in data.get("sources") or []:
        for key in ("id", "title", "url", "checked"):
            if not src.get(key):
                problems.add(rel, f"source needs {key}: {src}")
        if src.get("kind") not in SOURCE_KINDS:
            problems.add(rel, f"source '{src.get('id')}' kind '{src.get('kind')}' not allowed")
    if not sources:
        problems.add(rel, "at least one source")
    defined = [f.get("id") for j in data.get("job_types") or [] for f in j.get("allowed_follow_ups") or [] if isinstance(f, dict)]
    follow_ups = set(defined)
    ctx = {
        "cat": cats[cid], "cats": cats, "kb_trade": kb_trade, "kb_jobs": kb_jobs, "photos": kb_photos,
        "bank_q": {q["id"] for q in qset.get("questions", [])}, "follow_ups": follow_ups, "sources": sources,
    }
    for key in ("as_of", "written_by"):
        if not data.get(key):
            problems.add(rel, f"needs {key}")
    check_trade_brief(problems, lists, rel, data.get("trade_brief"), ctx)
    jobs = data.get("job_types") or []
    if not jobs:
        problems.add(rel, "job_types empty")
    seen: set[str] = set()
    for job in jobs:
        if job.get("id") in seen:
            problems.add(rel, f"job type '{job.get('id')}' listed twice")
        seen.add(job.get("id"))
        check_job_type(problems, lists, rel, job, ctx)
    if len(follow_ups) != len(defined):
        problems.add(rel, "a follow-up is defined once; other job types list its id as a plain string to share it")
    mapped = {j.get("bank_job_type") for j in jobs}
    for name in cats[cid].get("job_types") or []:
        if name not in mapped:
            problems.add(rel, f"category job type '{name}' has no job_types entry")
    check_business_side(problems, lists, rel, data.get("business_side"), ctx)
    return cid


def check_scenarios(problems: Problems, root: Path, cats: dict[str, Any], kb: dict[str, Any], experts: dict[str, Any]) -> int:
    count = 0
    for path in sorted((root / SCENARIO_DIR).glob("*.yaml")):
        rel = str(path.relative_to(root))
        data = load_yaml(path)
        cid = data.get("category") if isinstance(data, dict) else None
        if cid not in cats:
            problems.add(rel, f"unknown category '{cid}'")
            continue
        expert = experts.get(cid) or {}
        job_ids = {j.get("id") for j in expert.get("job_types") or []}
        kinds_seen: set[str] = set()
        scenarios = data.get("scenarios") or []
        if len(scenarios) < 12:
            problems.add(rel, f"{len(scenarios)} scenarios, expected 12 or more")
        for sc in scenarios:
            count += 1
            sid = sc.get("id", "?")
            kind = sc.get("kind")
            if kind not in SCENARIO_KINDS:
                problems.add(rel, f"{sid}: kind '{kind}' not allowed")
            kinds_seen.add(kind)
            for key in ("opening_message", "hidden_facts", "expected"):
                if not sc.get(key):
                    problems.add(rel, f"{sid}: needs {key}")
            exp = sc.get("expected") or {}
            if kind not in ("outside_area", "no_businesses", "two_trades") and exp.get("job_type") is not None and exp.get("job_type") not in job_ids and exp.get("category") == cid:
                problems.add(rel, f"{sid}: expected.job_type '{exp.get('job_type')}' is not in the expert file")
            if exp.get("category") and exp["category"] not in cats:
                problems.add(rel, f"{sid}: expected.category '{exp['category']}' unknown")
            if exp.get("fire_dept_text") not in (True, False):
                problems.add(rel, f"{sid}: expected.fire_dept_text must be true or false")
            text = yaml.safe_dump(sc)
            if PHONE_RE.search(text) or EMAIL_RE.search(text):
                problems.add(rel, f"{sid}: phone-shaped number or email (fake data only)")
        missing = SCENARIO_KINDS - kinds_seen - set(data.get("not_applicable") or [])
        if missing:
            problems.add(rel, f"scenario kinds missing: {sorted(missing)}")
    return count


def run(root: Path) -> tuple[list[str], str]:
    problems = Problems()
    lists = load_lists(root)
    cats, sets, kb = load_catalog(root)
    experts: dict[str, Any] = {}
    files = sorted((root / EXPERT_DIR).glob("*.yaml"))
    for path in files:
        if path.name == "schema.yaml" or path.name.startswith("business_"):
            continue
        cid = check_expert_file(problems, lists, root, path, cats, sets, kb)
        if cid:
            experts[cid] = load_yaml(path)
    for cid, cat in cats.items():
        if cat.get("enabled") and cid not in experts:
            problems.add(str(EXPERT_DIR / f"{cid}.yaml"), "missing: every enabled category needs an expert file")
    scenarios = check_scenarios(problems, root, cats, kb, experts) if (root / SCENARIO_DIR).exists() else 0
    summary = f"Expert files: {len(experts)}; enabled categories: {sum(1 for c in cats.values() if c.get('enabled'))}; scenarios: {scenarios}"
    return problems.items, summary


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--root", default=".", help="repository root")
    args = parser.parse_args(argv)
    items, summary = run(Path(args.root).resolve())
    for item in items:
        print(item)
    print(summary)
    print(f"Errors: {len(items)}")
    return 1 if items else 0


if __name__ == "__main__":
    sys.exit(main())
