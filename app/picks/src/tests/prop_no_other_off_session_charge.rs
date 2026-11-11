//! TD-FEE-30 property case and TD-FEE-31 pure cases (PICK-048, PICK-056, FEE-014; D-401, D-480):
//! the auto-confirm rule charges only a Verify-proven pick (quote-page code or portal session) of
//! a ticked quote while Stripe's written answer is filed, with the tick's author, card, fee and
//! version all unchanged; and no other code path in the workspace charges off-session: only
//! `payments`' `off_session.rs` calls the off-session confirm, only `picks`' `auto_confirm.rs`
//! calls `charge_off_session`, and no source asks Stripe for `off_session` on a charge.

use std::fs;
use std::path::{Path, PathBuf};

use proptest::prelude::*;

use crate::api::{AutoFacts, AutoPlan, PickProof, Skip, Void, auto_confirm_plan};

fn proof() -> impl Strategy<Value = PickProof> {
    prop_oneof![
        Just(PickProof::PageCode),
        Just(PickProof::PortalSession),
        Just(PickProof::Text),
    ]
}

prop_compose! {
    fn any_facts()(
        proof in proof(),
        flags in proptest::array::uniform7(any::<bool>()),
    ) -> AutoFacts {
        AutoFacts {
            proof,
            filed: flags[0],
            ticked: flags[1],
            fee_charged: flags[2],
            author_current: flags[3],
            card_same: flags[4],
            fee_same: flags[5],
            version_same: flags[6],
        }
    }
}

proptest! {
    /// TD-FEE-30, TD-FEE-31 (PICK-048, PICK-056, FEE-014): a charge exactly when every condition
    /// holds; a text pick, an unfiled answer or an unticked box never charges and voids nothing.
    #[test]
    fn td_fee_30_only_a_proven_ticked_unchanged_pick_charges(f in any_facts()) {
        let all = f.proof != PickProof::Text && f.filed && f.ticked && f.fee_charged
            && f.author_current && f.card_same && f.fee_same && f.version_same;
        let plan = auto_confirm_plan(f);
        prop_assert_eq!(plan == AutoPlan::Charge, all);
        if f.proof == PickProof::Text {
            prop_assert_eq!(plan, AutoPlan::Skip(Skip::NotProven));
        } else if !f.filed {
            prop_assert_eq!(plan, AutoPlan::Skip(Skip::NotFiled));
        } else if !f.ticked {
            prop_assert_eq!(plan, AutoPlan::Skip(Skip::NotTicked));
        }
    }
}

/// A pick whose every condition holds.
const GOOD: AutoFacts = AutoFacts {
    proof: PickProof::PortalSession,
    filed: true,
    ticked: true,
    fee_charged: true,
    author_current: true,
    card_same: true,
    fee_same: true,
    version_same: true,
};

/// TD-FEE-31 (PICK-056): each void condition alone voids the tick, in the rule's order; a $0
/// fee has nothing to charge.
#[test]
fn td_fee_31_each_change_voids_the_tick() {
    let good = GOOD;
    assert_eq!(auto_confirm_plan(good), AutoPlan::Charge);
    let author = AutoFacts {
        author_current: false,
        ..good
    };
    let card = AutoFacts {
        card_same: false,
        ..good
    };
    let fee = AutoFacts {
        fee_same: false,
        ..good
    };
    let version = AutoFacts {
        version_same: false,
        ..good
    };
    assert_eq!(auto_confirm_plan(author), AutoPlan::Void(Void::AuthorGone));
    assert_eq!(auto_confirm_plan(card), AutoPlan::Void(Void::CardChanged));
    assert_eq!(auto_confirm_plan(fee), AutoPlan::Void(Void::FeeChanged));
    assert_eq!(
        auto_confirm_plan(version),
        AutoPlan::Void(Void::VersionChanged)
    );
    let free = AutoFacts {
        fee_charged: false,
        ..good
    };
    assert_eq!(
        auto_confirm_plan(free),
        AutoPlan::Skip(Skip::NothingToCharge)
    );
}

/// Every Rust source under `app/` and `tests/`, test modules and this file included.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && !path.ends_with("target") {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Whether `path` is test code (its own tests may call a fake), never a production path.
fn is_test(rel: &str) -> bool {
    rel.starts_with("tests/") || rel.contains("/src/tests/") || rel.contains("/tests/")
}

/// The one file that defines and calls the off-session confirm and builds `OffSession`.
const OFF_SESSION: &str = "app/payments/src/off_session.rs";
/// The one production caller of `charge_off_session`.
const AUTO_CONFIRM: &str = "app/picks/src/auto_confirm.rs";
/// The files that define or re-export `charge_off_session` without calling it.
const CHARGE_NAMERS: [&str; 2] = [OFF_SESSION, "app/payments/src/api.rs"];

/// TD-FEE-30 (FEE-014, D-401): the only production callers of the off-session confirm and of
/// `charge_off_session` are `payments/src/off_session.rs` and `picks/src/auto_confirm.rs`; any
/// mention of the names counts, not only a call, so a qualified path, a `use … as` rename or a
/// function reference is caught, as is building the `OffSession(` wrapper anywhere else; and no
/// production source outside `adapters` (the Stripe transport T132 adds the form field to, reached
/// only through `OffSessionStripe`) posts `off_session=true`.
#[test]
fn td_fee_30_no_other_code_path_charges_off_session() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    sources(&root.join("app"), &mut files);
    assert!(files.len() > 50, "the workspace sources were found");
    let mut seen = (false, false);
    for path in files {
        let rel = relative(&path, &root);
        let text = fs::read_to_string(&path).expect("source reads");
        if is_test(&rel) {
            continue;
        }
        if text.contains("confirm_off_session") || text.contains("OffSession(") {
            assert_eq!(
                rel, OFF_SESSION,
                "the off-session confirm or its wrapper named"
            );
            seen.0 = true;
        }
        if text.contains("charge_off_session") && !CHARGE_NAMERS.contains(&rel.as_str()) {
            assert_eq!(rel, AUTO_CONFIRM, "charge_off_session named");
            seen.1 = true;
        }
        if rel.starts_with("app/adapters/") {
            continue;
        }
        for line in text.lines().filter(|l| !l.trim_start().starts_with("//")) {
            let asks = line.contains("off_session=true")
                || line.contains("(\"off_session\", \"true\")")
                || line.contains("\"off_session\": true");
            assert!(!asks, "{rel} asks Stripe for an off-session charge: {line}");
        }
    }
    assert_eq!(seen, (true, true), "both PICK-048 call sites were found");
}
