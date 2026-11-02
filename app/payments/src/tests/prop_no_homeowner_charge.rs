//! TD-FEE-07 property cases (FEE-009, FEE-014, FEE-048): whatever the business, the only Stripe
//! object `payments` asks for when saving a card is an off-session, cards-only SetupIntent on that
//! business's customer, with nothing else in the form, so no flow can charge anyone there; and
//! every payment method type but `card` is refused, bank debits included (D-073). The database
//! and route cases are `tests/integration/tests/setup_intent.rs` and
//! `tests/security/tests/no_homeowner_charge.rs`.

use proptest::prelude::*;

use crate::api::{CardError, CustomerId, SavedCard, SetupIntentRequest};

/// Payment method types Stripe offers that are not cards; each must be refused (D-073).
const NOT_CARDS: [&str; 8] = [
    "us_bank_account",
    "sepa_debit",
    "acss_debit",
    "bacs_debit",
    "au_becs_debit",
    "link",
    "cashapp",
    "ach_debit",
];

proptest! {
    /// TD-FEE-07 (FEE-009, FEE-048): the SetupIntent form names the business's customer, saves
    /// for off-session use, takes cards only, and has no amount, confirm or charge field.
    #[test]
    fn td_fee_07_setup_intent_never_charges(tail in "[A-Za-z0-9]{1,40}") {
        let customer = CustomerId::parse(&format!("cus_{tail}")).unwrap();
        let request = SetupIntentRequest::for_customer(customer.clone());
        let form = request.form();
        prop_assert_eq!(form[0], ("customer", customer.as_str()));
        prop_assert_eq!(form[1], ("usage", "off_session"));
        prop_assert_eq!(form[2], ("payment_method_types[]", "card"));
        for (name, _) in form {
            prop_assert!(!["amount", "confirm", "off_session", "payment_method"].contains(&name));
        }
    }

    /// TD-FEE-10 (FEE-014, D-073): any method type but `card` is `CardsOnly`, before any other
    /// field is looked at.
    #[test]
    fn td_fee_10_only_cards_are_saved(kind in "[a-z_]{0,20}", pick in 0usize..8) {
        for method in [kind.as_str(), NOT_CARDS[pick]] {
            let result = SavedCard::new("pm_Fake0001", method, "visa", "4242");
            if method == "card" {
                prop_assert!(result.is_ok());
            } else {
                prop_assert_eq!(result, Err(CardError::CardsOnly));
            }
        }
    }

    /// TD-FEE-07 (FEE-009): only a `cus_…` ID is a customer; nothing else parses, so no other
    /// party's ID can stand in for one.
    #[test]
    fn td_fee_07_only_customer_ids_parse(id in "[a-z]{0,4}_?[A-Za-z0-9]{0,12}") {
        let parsed = CustomerId::parse(&id);
        let want = id.strip_prefix("cus_").is_some_and(|t| !t.is_empty());
        prop_assert_eq!(parsed.is_ok(), want);
    }
}

/// TD-FEE-10 (FEE-014): the card's fields are checked; card data beyond brand and last four has
/// nowhere to go.
#[test]
fn td_fee_10_card_fields_are_checked() {
    let card = SavedCard::new("pm_Fake0002", "card", "mastercard", "4444").unwrap();
    assert_eq!(
        (card.payment_method(), card.brand(), card.last4()),
        ("pm_Fake0002", "mastercard", "4444")
    );
    for (pm, brand, last4) in [
        ("pi_Fake0002", "visa", "4242"),
        ("pm_", "visa", "4242"),
        ("pm_Fake0002", "Visa", "4242"),
        ("pm_Fake0002", "", "4242"),
        ("pm_Fake0002", "visa", "424"),
        ("pm_Fake0002", "visa", "42a2"),
    ] {
        assert_eq!(
            SavedCard::new(pm, "card", brand, last4),
            Err(CardError::BadField)
        );
    }
}

/// TD-FEE-10 (FEE-014): an ID's part after the prefix is at most 200 characters, as the table's
/// checks (`cus_` or `pm_` and `{1,200}`) allow.
#[test]
fn td_fee_10_vendor_ids_match_the_table_length() {
    let tail = "F".repeat(200);
    assert!(SavedCard::new(&format!("pm_{tail}"), "card", "visa", "4242").is_ok());
    assert_eq!(
        SavedCard::new(&format!("pm_{tail}F"), "card", "visa", "4242"),
        Err(CardError::BadField)
    );
    assert!(CustomerId::parse(&format!("cus_{tail}")).is_ok());
    assert_eq!(
        CustomerId::parse(&format!("cus_{tail}F")),
        Err(CardError::BadField)
    );
}
