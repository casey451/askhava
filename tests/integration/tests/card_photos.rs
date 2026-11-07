//! Card photos across `privacy` and `quotes` (TD-CARD-04, TD-AI-11, TD-PRIV-08 card and AI-input
//! cases; AI-016, CARD-013, CARD-014, INT-044, PRIV-014).
//!
//! A fake upload goes through `privacy::api::clean`, then `quotes::api::card_photo` with a
//! stand-in screener that records every call. Before zero retention the screener is never called
//! and only the homeowner's confirmation decides; after it, both checks run with different
//! instructions and any flag, refusal or failure keeps the photo off. The fixture is a made-up
//! 2 by 2 PNG with a fake GPS text chunk; the "face" and "plate" are the stand-in's flags.

use std::sync::Mutex;

use askhava_privacy::api::{CleanPhoto, clean};
use askhava_quotes::api::{
    CardPhotoOutcome, HomeownerAnswer, LeftOff, PhotoAiGate, PhotoScreener, ScreenCheck,
    ScreenFailed, ScreenVerdict, card_photo,
};

const PNG_HEX: &str = "89504e470d0a1a0a0000000d4948445200000002000000020802000000fdd49a73000000137445\
                       58744750530046414b452d4750532d33342e34384ea7dc82a60000001049444154789c6338a1\
                       a101440c100a00212e04611ee06dc20000000049454e44ae426082";
const GPS: &[u8] = b"FAKE-GPS-34.48N";

/// What the stand-in answers for one check.
#[derive(Clone, Copy)]
enum Reply {
    Pass,
    Flag(ScreenVerdict),
    Fail,
}

/// A stand-in AI photo screen: answers per check and records each call's check and input bytes.
struct StandIn {
    first: Reply,
    second: Reply,
    calls: Mutex<Vec<(ScreenCheck, Vec<u8>)>>,
}

impl StandIn {
    fn new(first: Reply, second: Reply) -> Self {
        Self {
            first,
            second,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<(ScreenCheck, Vec<u8>)> {
        self.calls.lock().expect("calls").clone()
    }
}

impl PhotoScreener for StandIn {
    async fn screen(
        &self,
        check: ScreenCheck,
        photo: &CleanPhoto,
    ) -> Result<ScreenVerdict, ScreenFailed> {
        self.calls
            .lock()
            .expect("calls")
            .push((check, photo.as_bytes().to_vec()));
        match if check == ScreenCheck::First {
            self.first
        } else {
            self.second
        } {
            Reply::Pass => Ok(ScreenVerdict::default()),
            Reply::Flag(verdict) => Ok(verdict),
            Reply::Fail => Err(ScreenFailed),
        }
    }
}

fn upload() -> Vec<u8> {
    let hex: String = PNG_HEX.split_whitespace().collect();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
        .collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

async fn photo() -> CleanPhoto {
    let upload = upload();
    assert!(contains(&upload, GPS), "the fixture carries fake GPS");
    clean(upload).await.expect("the fixture cleans")
}

fn after_zero_retention() -> PhotoAiGate {
    PhotoAiGate::from_records(true, true)
}

const FACE: ScreenVerdict = ScreenVerdict {
    face: true,
    plate: false,
    text: false,
    refused: false,
};
const PLATE: ScreenVerdict = ScreenVerdict {
    face: false,
    plate: true,
    text: false,
    refused: false,
};
const TEXT: ScreenVerdict = ScreenVerdict {
    face: false,
    plate: false,
    text: true,
    refused: false,
};
const REFUSED: ScreenVerdict = ScreenVerdict {
    face: false,
    plate: false,
    text: false,
    refused: true,
};

/// TD-CARD-04, TD-PRIV-08, CARD-013, CARD-014, INT-044, PRIV-014: before zero retention a
/// confirmed, cleaned photo goes on the card as a metadata-free JPEG with zero AI calls, even
/// with a stand-in that would flag it.
#[tokio::test]
async fn td_card_04_before_zero_retention_a_confirmed_photo_goes_on_with_no_ai_call() {
    let screen = StandIn::new(Reply::Flag(FACE), Reply::Flag(PLATE));
    let gate = PhotoAiGate::ON_SERVER_ONLY;
    let outcome = card_photo(photo().await, HomeownerAnswer::Confirmed, gate, &screen).await;
    let CardPhotoOutcome::OnCard(card) = outcome else {
        panic!("a confirmed photo goes on the card before zero retention");
    };
    assert!(
        card.as_bytes().starts_with(&[0xFF, 0xD8, 0xFF]),
        "re-encoded JPEG"
    );
    assert!(
        !contains(card.as_bytes(), GPS),
        "no metadata on the card image"
    );
    assert!(
        screen.calls().is_empty(),
        "the AI adapter is called zero times"
    );
}

/// TD-CARD-04, CARD-014, INT-044: a photo she did not confirm is left off with a note, and one
/// she removed before send stays off; neither reaches the screen, before or after zero retention.
#[tokio::test]
async fn td_card_04_unconfirmed_or_removed_photos_stay_off_with_no_ai_call() {
    for gate in [PhotoAiGate::ON_SERVER_ONLY, after_zero_retention()] {
        let screen = StandIn::new(Reply::Pass, Reply::Pass);
        let answers = [
            (HomeownerAnswer::NotConfirmed, LeftOff::NotConfirmed),
            (HomeownerAnswer::Removed, LeftOff::Removed),
        ];
        for (answer, reason) in answers {
            let outcome = card_photo(photo().await, answer, gate, &screen).await;
            assert_eq!(outcome, CardPhotoOutcome::LeftOff(reason));
        }
        assert!(screen.calls().is_empty(), "no AI call for a photo kept off");
    }
}

/// TD-AI-11, AI-016, CARD-013: AI screening turns on only with the zero-retention agreement
/// signed and the AI-018 photo sentence live; either alone keeps the screen uncalled.
#[tokio::test]
async fn td_ai_11_no_ai_call_until_zero_retention_and_the_photo_sentence() {
    for (signed, sentence) in [(false, false), (false, true), (true, false)] {
        let gate = PhotoAiGate::from_records(signed, sentence);
        assert!(!gate.ai_screening());
        let screen = StandIn::new(Reply::Flag(TEXT), Reply::Flag(TEXT));
        let outcome = card_photo(photo().await, HomeownerAnswer::Confirmed, gate, &screen).await;
        assert!(matches!(outcome, CardPhotoOutcome::OnCard(_)));
        assert!(screen.calls().is_empty(), "no call before zero retention");
    }
    assert!(after_zero_retention().ai_screening());
}

/// TD-AI-11, TD-PRIV-08, AI-016, CARD-014, PRIV-014: after zero retention a passing photo goes
/// through both checks, with different instructions, and the AI input carries no metadata.
#[tokio::test]
async fn td_ai_11_both_checks_run_with_different_instructions_on_clean_bytes() {
    let screen = StandIn::new(Reply::Pass, Reply::Pass);
    let gate = after_zero_retention();
    let outcome = card_photo(photo().await, HomeownerAnswer::Confirmed, gate, &screen).await;
    assert!(matches!(outcome, CardPhotoOutcome::OnCard(_)));
    let calls = screen.calls();
    let checks: Vec<ScreenCheck> = calls.iter().map(|(check, _)| *check).collect();
    assert_eq!(checks, ScreenCheck::BOTH);
    assert_ne!(
        ScreenCheck::First.instruction(),
        ScreenCheck::Second.instruction()
    );
    for (_, input) in &calls {
        assert!(
            input.starts_with(&[0xFF, 0xD8, 0xFF]),
            "the AI sees the clean JPEG"
        );
        assert!(!contains(input, GPS), "no metadata in any AI input");
    }
}

/// TD-AI-11, AI-016, CARD-014: a fake face, a house-number plate, readable text or a refusal
/// from either check, or a failed call, keeps the photo off with a note; both checks still run.
#[tokio::test]
async fn td_ai_11_any_flag_refusal_or_failure_keeps_the_photo_off() {
    let cases = [
        (Reply::Flag(FACE), Reply::Pass, LeftOff::Flagged),
        (Reply::Pass, Reply::Flag(PLATE), LeftOff::Flagged),
        (Reply::Flag(TEXT), Reply::Pass, LeftOff::Flagged),
        (Reply::Pass, Reply::Flag(REFUSED), LeftOff::Flagged),
        (Reply::Fail, Reply::Pass, LeftOff::NotScreened),
        (Reply::Pass, Reply::Fail, LeftOff::NotScreened),
    ];
    for (first, second, reason) in cases {
        let screen = StandIn::new(first, second);
        let gate = after_zero_retention();
        let outcome = card_photo(photo().await, HomeownerAnswer::Confirmed, gate, &screen).await;
        assert_eq!(outcome, CardPhotoOutcome::LeftOff(reason));
        assert_eq!(screen.calls().len(), 2, "both checks run on every photo");
    }
}
