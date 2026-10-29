"""Roll every expert file's business_side into docs/catalog/expert/business_signup_knowledge.yaml.

The sign-up help page, the AI-drafted HELP replies (if OPS-036 is ever switched on) and the
recruiting posts read one file instead of 37. Re-run after any expert file changes.

Usage: python3 scripts/build_business_signup_knowledge.py [--root REPO_ROOT]
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import Any

import yaml

EXPERT_DIR = Path("docs/catalog/expert")
OUT = EXPERT_DIR / "business_signup_knowledge.yaml"

SHARED_ANSWERS = [
    {"objection": "What does it cost to join?",
     "answer": "Nothing. There is no sign-up fee and nothing monthly. You pay a set introduction fee only when a homeowner picks your quote and you confirm: $25, $75 or $199 by job size."},
    {"objection": "Do other businesses get the same job?",
     "answer": "Every business that covers the job type gets the card, and each one quotes in its own words. The homeowner sees every quote and picks one. Only the business she picks pays, and only that business gets her contact details."},
    {"objection": "Do you rank or rate us?",
     "answer": "No. Quotes are shown in the order they came in, with each business's name, license numbers and links to its own Google and Yelp pages. Ask Hava never scores, sorts or recommends a business."},
    {"objection": "What if the homeowner cancels after I pay?",
     "answer": "If you have not spoken with the homeowner yet, the fee is refunded."},
    {"objection": "Do I have to quote every card?",
     "answer": "No. Quote the ones you want. You pick the job types you take at sign-up and can change them any time, and you can pause with one word when you are busy."},
    {"objection": "Is Ask Hava a contractor or a middleman that marks up the job?",
     "answer": "Neither. Ask Hava is not a contractor and does not perform, bid on or supervise work. It passes the homeowner's request to you and shows her your quote as you wrote it. You and the homeowner deal with each other directly."},
    {"objection": "How do I know the request is real?",
     "answer": "Each request comes from a texted phone number we check, with the town, the answers to the questions a pro asks, and usually photos. Names and addresses are shared only with the business the homeowner picks."},
]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--root", default=".")
    args = parser.parse_args(argv)
    root = Path(args.root).resolve()
    cats = {c["id"]: c for c in yaml.safe_load((root / "docs/catalog/categories.yaml").read_text(encoding="utf-8"))["categories"]}
    trades: dict[str, Any] = {}
    for path in sorted((root / EXPERT_DIR).glob("*.yaml")):
        if path.name in ("schema.yaml", OUT.name):
            continue
        data = yaml.safe_load(path.read_text(encoding="utf-8"))
        cid = data.get("category")
        side = data.get("business_side") or {}
        if not cid or not side:
            continue
        sources = {s["id"]: s for s in data.get("sources") or []}
        benchmarks = []
        for row in side.get("customer_acquisition_benchmarks") or []:
            src = sources.get(row.get("source") or "")
            benchmarks.append({
                "channel": row.get("channel"),
                "figure": row.get("figure"),
                "source_url": src.get("url") if src else None,
                "unverified": bool(row.get("unverified")),
            })
        trades[cid] = {
            "name": cats.get(cid, {}).get("name", cid),
            "trade_brief_seasonality": (data.get("trade_brief") or {}).get("seasonality"),
            "how_they_get_customers": side.get("how_they_get_customers"),
            "customer_acquisition_benchmarks": benchmarks,
            "what_they_dislike_about_lead_services": side.get("what_they_dislike_about_lead_services"),
            "what_makes_a_lead_worth_paying_for": side.get("what_makes_a_lead_worth_paying_for"),
            "their_words": side.get("their_words"),
            "signup_objections": side.get("signup_objections"),
            "quote_tips_for_card": side.get("quote_tips_for_card"),
        }
    out = {
        "schema": 1,
        "generated_by": "scripts/build_business_signup_knowledge.py from docs/catalog/expert/<category>.yaml; do not edit by hand",
        "wording_rules": "Every answer keeps the rules: pay only on the pick, no monthly fee, fees $25/$75/$199 by job size, no ranking or rating, refund before contact, Ask Hava is not a contractor. Banned words and the AI-smell list apply (scripts/check_expert.py checks the source files).",
        "shared_answers": SHARED_ANSWERS,
        "trades": trades,
    }
    header = "# Ask Hava business sign-up knowledge: the business side of every trade, rolled up from the expert files.\n# Generated file. Fake data only. AI research, not legal advice.\n"
    (root / OUT).write_text(header + yaml.safe_dump(out, sort_keys=False, allow_unicode=True, width=120), encoding="utf-8")
    print(f"wrote {OUT} with {len(trades)} trades")
    return 0


if __name__ == "__main__":
    sys.exit(main())
