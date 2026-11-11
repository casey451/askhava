//! Photo asks, the on-server checks, the preview, look pictures and the concept image (TD-INT-27,
//! TD-INT-28, TD-SEC-20 media case, TD-CARD-04 confirmation cases, TD-CARD-12 photo and concept
//! cases, TD-A11Y-01 upload input; INT-044, INT-045, INT-046, PRIV-015, SEC-041, A11Y-027; D-432,
//! D-433, D-491, D-492).
//!
//! Every photo is a made-up solid-colour PNG built here; the texting provider and the image
//! maker are fakes that record each call. Fake data only.

use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, FetchError, IdempotencyKey, LocalStorage};
use askhava_core::api::IdSecret;
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    AfterQuestions, CONCEPT_QUESTION, ConceptStep, LEAVE_OUT_LINE, NO_PHOTO_LINE, PhotoStep,
    PhotoText, TextIn, after_photo, after_questions, concept_step, is_design_category,
    left_off_note, photo_answer, photo_body, photo_text, receive_text,
};
use askhava_integration::set_app_role;
use askhava_messaging::api::Language;
use askhava_portals::api::{
    JobPageLinks, REASK_ERROR, TOO_BIG_ERROR, TOO_MANY_PIXELS_ERROR, UploadScreen,
    job_details_page, photo_upload_page, upload_screen,
};
use askhava_privacy::api::{
    CONCEPT_LABEL, CleanPhoto, CommonWords, Concept, ConceptImageAdapter, DEFAULT_MIN_SHORT_SIDE,
    Known, MediaState, NameMemory, PhotoSeal, PhotoTaken, PictureTextMedia, ProjectFile,
    fetch_twilio_media, issue_link, make_concept, open_link, scrub, take_picture_text, take_upload,
};
use askhava_quotes::api::{
    CardParts, CardPhotoOutcome, DEFAULT_MIN_WORDS, HomeownerAnswer, LeftOff, PageParts,
    PhotoAiGate, PhotoScreener, Place, ScreenCheck, ScreenFailed, ScreenVerdict, build_card,
    card_photo, job_page,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const JOB: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0212);
const MEDIA_URL: &str = "https://api.twilio.com/2010-04-01/Accounts/AC0/Messages/MM0/Media/ME0";

struct Dir(std::path::PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _gone = std::fs::remove_dir_all(&self.0);
    }
}

struct Kit {
    _dir: Dir,
    storage: LocalStorage,
    seal: PhotoSeal,
    secret: SecretString,
    ids: IdSecret,
}

fn kit(test: &str) -> Kit {
    let root = std::env::temp_dir().join(format!("askhava-t212-{test}-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary directory");
    let key = SecretString::from(format!("1:{}", STANDARD.encode([5u8; 32])));
    let secret = SecretString::from("fake-id-secret-for-intake-photos-0123456789");
    Kit {
        _dir: Dir(root.clone()),
        storage: LocalStorage::new(&format!("file://{}", root.display())).expect("storage"),
        seal: PhotoSeal::new(&key).expect("seal"),
        ids: IdSecret::new(1, secret.clone()).expect("id secret"),
        secret,
    }
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::RgbImage::from_pixel(width, height, image::Rgb([120, 130, 140]))
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("png");
    out
}

fn at() -> Timestamp {
    Timestamp::from_second(1_800_000_000).expect("time")
}

async fn app(pool: &PgPool) -> PgConnection {
    let mut conn = pool.acquire().await.expect("connection").detach();
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

/// The fake texting provider's media: serves `bytes`, or with none runs the real guarded fetch;
/// records every delete.
struct FakeMedia {
    bytes: Option<Vec<u8>>,
    deleted: Mutex<Vec<String>>,
}

impl FakeMedia {
    fn new(bytes: Option<Vec<u8>>) -> Self {
        let deleted = Mutex::new(Vec::new());
        Self { bytes, deleted }
    }
}

impl PictureTextMedia for FakeMedia {
    async fn fetch_media(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        match &self.bytes {
            Some(bytes) => Ok(bytes.clone()),
            None => fetch_twilio_media(url).await,
        }
    }

    async fn delete_media(&self, _: &IdempotencyKey, url: &str) -> Result<(), AdapterError> {
        if url.contains("fail-delete") {
            return Err(AdapterError::NotSent);
        }
        self.deleted.lock().expect("deleted").push(url.to_owned());
        Ok(())
    }
}

/// A screener that must never be called before zero retention.
struct NoScreen(Mutex<usize>);

impl PhotoScreener for NoScreen {
    async fn screen(&self, _: ScreenCheck, _: &CleanPhoto) -> Result<ScreenVerdict, ScreenFailed> {
        *self.0.lock().expect("calls") += 1;
        Ok(ScreenVerdict::default())
    }
}

/// The stand-in image maker: records each request and answers a made-up picture.
struct StandInImages(Mutex<Vec<serde_json::Value>>);

impl ConceptImageAdapter for StandInImages {
    async fn make_image(
        &self,
        _: &IdempotencyKey,
        request: &serde_json::Value,
    ) -> Result<Vec<u8>, AdapterError> {
        self.0.lock().expect("requests").push(request.clone());
        Ok(png(400, 400))
    }
}

/// An image maker that fails (`None`) or answers bytes that are not a picture.
struct BrokenImages(Option<Vec<u8>>);

impl ConceptImageAdapter for BrokenImages {
    async fn make_image(
        &self,
        _: &IdempotencyKey,
        _: &serde_json::Value,
    ) -> Result<Vec<u8>, AdapterError> {
        self.0.clone().ok_or(AdapterError::NotSent)
    }
}

async fn reopened(conn: &mut PgConnection, kit: &Kit, id: Uuid) -> CleanPhoto {
    let token = issue_link(conn, &kit.ids, id, at()).await.expect("link");
    let deps = (&kit.seal, &kit.storage);
    open_link(conn, deps, &token, at()).await.expect("open")
}

fn text_in(words: &str) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x2120_0001),
        contact: Uuid::from_u128(0x2120_0002),
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(),
    }
}

/// TD-INT-27, INT-043, INT-044, PRIV-015: the photo ask follows the trade questions, carries at
/// most three asks, "No photo is fine too." and PRIV-015's line, and the name comes after it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_27_the_photo_ask_follows_the_questions_with_at_most_three_asks(pool: PgPool) {
    let steps = after_questions("plumbing_repair", false, (false, false));
    assert_eq!(steps, [AfterQuestions::Photos, AfterQuestions::NameAddress]);
    let mut db = pool.acquire().await.expect("connection");
    let secret = SecretString::from("test-only-id-secret-not-a-real-key");
    let msg = text_in("my kitchen sink is leaking under the cabinet");
    let handled = receive_text(&mut db, &secret, &msg, Settings::default()).await;
    let id = handled.expect("open").conversation.expect("request");
    let asks: Vec<String> = ["Ask one.", "Ask two.", "Ask three.", "Ask four."]
        .map(str::to_owned)
        .to_vec();
    let link = "https://askhava.com/h/requests/1/photos";
    let ask = PhotoText::Ask { asks: &asks, link };
    let queued = photo_text(&mut db, &secret, (&msg, id), ask, false).await;
    let outbox = queued.expect("queued").expect("a photo ask").outbox_id;
    let body: String = sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE id = $1")
        .bind(outbox)
        .fetch_one(&mut *db)
        .await
        .expect("body");
    assert!(
        body.contains(NO_PHOTO_LINE) && body.contains(LEAVE_OUT_LINE),
        "{body}"
    );
    assert!(
        body.contains("Ask three.") && !body.contains("Ask four."),
        "{body}"
    );
    assert!(body.contains(link));
}

/// TD-INT-27, INT-044: no photo ask, and no look-pictures ask in a design category, while a 911
/// text waits for SAFE.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_27_no_photo_ask_during_a_911_list_situation(pool: PgPool) {
    assert!(
        !after_questions("plumbing_repair", true, (false, false)).contains(&AfterQuestions::Photos)
    );
    let held = after_questions("painting", true, (false, true));
    assert!(
        !held.contains(&AfterQuestions::Photos) && !held.contains(&AfterQuestions::LookPictures)
    );
    let mut db = pool.acquire().await.expect("connection");
    let secret = SecretString::from("test-only-id-secret-not-a-real-key");
    let msg = text_in("I smell gas by the water heater");
    let handled = receive_text(&mut db, &secret, &msg, Settings::default()).await;
    let id = handled.expect("open").conversation.expect("request");
    let ask = PhotoText::Ask {
        asks: &[],
        link: "https://askhava.com/h",
    };
    let queued = photo_text(&mut db, &secret, (&msg, id), ask, false).await;
    assert_eq!(queued.expect("checked"), None);
}

/// TD-INT-27, TD-CARD-04, INT-044, D-491: a picture text and an upload are both checked on the
/// server and reach the card on her YES with no AI call; the picture text's media is deleted at
/// the fake provider once stored.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_27_a_picture_text_and_an_upload_both_land_on_the_card(pool: PgPool) {
    let kit = kit("land");
    let mut conn = app(&pool).await;
    let keys = (&kit.seal, &kit.storage, &kit.secret);
    let media = FakeMedia::new(Some(png(480, 320)));
    let texted = take_picture_text(&mut conn, keys, &media, (JOB, MEDIA_URL), (320, at()))
        .await
        .expect("picture text");
    assert_eq!(texted.media, MediaState::Deleted);
    assert_eq!(*media.deleted.lock().expect("deleted"), [MEDIA_URL]);
    let upload = take_upload(&mut conn, keys, (JOB, png(320, 640)), 320, at())
        .await
        .expect("upload");
    let screener = NoScreen(Mutex::new(0));
    for taken in [texted.taken, upload] {
        let PhotoStep::Preview(id) = after_photo(taken, false) else {
            panic!("stored: {taken:?}");
        };
        let photo = reopened(&mut conn, &kit, id).await;
        let gate = PhotoAiGate::ON_SERVER_ONLY;
        let out = card_photo(photo, photo_answer("Yes"), gate, &screener).await;
        assert!(matches!(out, CardPhotoOutcome::OnCard(_)));
    }
    assert_eq!(*screener.0.lock().expect("calls"), 0);
}

/// TD-INT-27, TD-CARD-04, INT-044: one re-ask for an unreadable file or a 319-pixel side, none
/// for 320 (the setting) or a plain readable photo; a second failure is left off; no confirmation
/// or her removal keeps it off with a note.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_27_one_re_ask_only_for_unreadable_or_small_files(pool: PgPool) {
    let kit = kit("reask");
    let mut conn = app(&pool).await;
    let keys = (&kit.seal, &kit.storage, &kit.secret);
    let mut take = async |bytes: Vec<u8>| {
        let min = DEFAULT_MIN_SHORT_SIDE;
        take_upload(&mut conn, keys, (JOB, bytes), min, at())
            .await
            .expect("take")
    };
    assert_eq!(
        take(b"not a photo at all".to_vec()).await,
        PhotoTaken::ReAsk
    );
    assert_eq!(take(png(500, 319)).await, PhotoTaken::ReAsk);
    assert!(matches!(take(png(320, 320)).await, PhotoTaken::Stored(_)));
    assert!(matches!(take(png(900, 700)).await, PhotoTaken::Stored(_)));
    assert_eq!(after_photo(PhotoTaken::ReAsk, false), PhotoStep::ReAsk);
    assert_eq!(after_photo(PhotoTaken::ReAsk, true), PhotoStep::LeftOff);
    assert_eq!(photo_answer("maybe"), HomeownerAnswer::NotConfirmed);
    assert_eq!(photo_answer("REMOVE"), HomeownerAnswer::Removed);
    assert!(!left_off_note(LeftOff::NotConfirmed).is_empty());
    let screen = upload_screen(PhotoTaken::ReAsk, String::new);
    assert_eq!(screen, UploadScreen::Ask(Some(REASK_ERROR)));
}

/// TD-SEC-20, SEC-041: media from a non-Twilio host is refused before any request, nothing is
/// stored and nothing is deleted.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_20_picture_text_media_from_another_host_is_refused(pool: PgPool) {
    let kit = kit("host");
    let mut conn = app(&pool).await;
    let keys = (&kit.seal, &kit.storage, &kit.secret);
    let media = FakeMedia::new(None);
    let url = "https://media.example.com/photo.jpg";
    assert_eq!(
        fetch_twilio_media(url).await,
        Err(FetchError::HostNotAllowed)
    );
    let texted = take_picture_text(&mut conn, keys, &media, (JOB, url), (320, at()))
        .await
        .expect("refused");
    assert_eq!(texted.taken, PhotoTaken::Refused);
    assert_eq!(texted.media, MediaState::NothingToDelete);
    assert!(media.deleted.lock().expect("deleted").is_empty());
}

/// TD-INT-27, TD-A11Y-01, PRIV-015, A11Y-027: the upload page says PRIV-015's line and uses a
/// native file input with a visible label; the photo ask texts say it too.
#[test]
fn td_int_27_the_upload_page_and_photo_ask_say_leave_people_out() {
    let asks = vec!["A photo of where the water is coming from.".to_owned()];
    let page =
        photo_upload_page("1042", &asks, &UploadScreen::Ask(None), "fake-csrf").expect("page");
    assert!(page.contains(LEAVE_OUT_LINE) && page.contains(NO_PHOTO_LINE));
    assert!(page.contains(r#"<label for="photo">"#));
    assert!(page.contains(r#"<input type="file" id="photo""#));
    for language in [Language::En, Language::Es] {
        let ask = PhotoText::Ask {
            asks: &asks,
            link: "https://askhava.com/h",
        };
        let (_, body) = photo_body(ask, language, false);
        assert!(body.starts_with("Ask Hava: ") && body.contains(LEAVE_OUT_LINE));
    }
}

/// TD-INT-28, INT-045, D-433: the look-pictures ask follows the photos in a `design: true`
/// category; a range category without the flag never asks it.
#[test]
fn td_int_28_only_design_categories_ask_for_look_pictures() {
    assert!(is_design_category("painting") && !is_design_category("electrical"));
    let steps = after_questions("painting", false, (false, false));
    let want = [
        AfterQuestions::Photos,
        AfterQuestions::LookPictures,
        AfterQuestions::DescribeLook,
        AfterQuestions::NameAddress,
    ];
    assert_eq!(steps, want);
    let range = after_questions("electrical", false, (false, true));
    assert!(!range.contains(&AfterQuestions::LookPictures));
    let (_, ask) = photo_body(PhotoText::LookAsk, Language::En, false);
    assert_eq!(
        ask,
        "Ask Hava: Have any pictures of the look you want? Screenshots are fine."
    );
}

fn project() -> ProjectFile {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let words = scrub(
        "Standard 5 by 8 bath, matte gray tile",
        &known,
        &mut NameMemory::default(),
    );
    ProjectFile::from_scrubbed(&words).expect("project file")
}

/// TD-INT-28, INT-046, D-492: with "Concept images" off no image call is made and SKIP skips.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_28_concept_images_off_makes_no_call(pool: PgPool) {
    let kit = kit("off");
    let mut conn = app(&pool).await;
    let maker = StandInImages(Mutex::new(Vec::new()));
    let deps = ((&kit.seal, &kit.storage, &kit.secret), &maker);
    let made = make_concept(&mut conn, deps, (JOB, 0, false), &project(), at()).await;
    assert_eq!(made, Ok(Concept::Off));
    assert!(maker.0.lock().expect("requests").is_empty());
    assert_eq!(concept_step("SKIP", 0), ConceptStep::Skip);
    assert!(
        !after_questions("painting", false, (false, false)).contains(&AfterQuestions::ConceptImage)
    );
}

/// TD-INT-28, INT-046, D-492: switched on against the stand-in, the request holds only the written
/// project file, the image and its beside-text carry the label, 3 change rounds are allowed and a
/// fourth refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_28_concept_image_from_the_project_file_only_three_rounds(pool: PgPool) {
    let kit = kit("on");
    let mut conn = app(&pool).await;
    let maker = StandInImages(Mutex::new(Vec::new()));
    let deps = ((&kit.seal, &kit.storage, &kit.secret), &maker);
    for changes in 0..=3 {
        let made = make_concept(&mut conn, deps, (JOB, changes, true), &project(), at()).await;
        assert!(matches!(made, Ok(Concept::Made(_))), "{made:?}");
    }
    let fourth = make_concept(&mut conn, deps, (JOB, 4, true), &project(), at()).await;
    assert_eq!(fourth, Ok(Concept::RoundsUsed));
    let requests = maker.0.lock().expect("requests").clone();
    assert_eq!(requests.len(), 4);
    for request in requests {
        let keys: Vec<&String> = request.as_object().expect("object").keys().collect();
        assert_eq!(keys, ["label", "project_file"]);
        assert_eq!(request["label"], CONCEPT_LABEL);
        assert_eq!(request["project_file"], project().as_str());
    }
    assert_eq!(concept_step("darker please", 2), ConceptStep::Remake(3));
    assert_eq!(concept_step("darker please", 3), ConceptStep::NoMoreChanges);
    assert_eq!(concept_step("yes", 3), ConceptStep::Accept);
    let link = PhotoText::Concept {
        link: "https://askhava.com/photos/x",
    };
    let (_, body) = photo_body(link, Language::En, false);
    assert!(
        body.contains(CONCEPT_LABEL) && body.contains(CONCEPT_QUESTION),
        "{body}"
    );
}

/// TD-CARD-12, CARD-023, INT-045, INT-046: the job page shows the photos, the project file and
/// the concept image with its label beside it; the page reads the concept image off the card
/// without the caller naming it, and photo links that do not match its photos show none (D-2874).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_12_the_job_page_shows_photos_project_file_and_labeled_concept(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (photo, concept) = (Uuid::from_u128(0x212a), Uuid::from_u128(0x212b));
    let card = build_card(
        &CardParts {
            category: "Painting".to_owned(),
            town: "Lake Havasu City".to_owned(),
            place: Place::House,
            description: "Repaint the small bathroom in a light gray".to_owned(),
            answers: Vec::new(),
            timing: None,
            danger: None,
            photos: vec![photo],
            concept: Some((concept, CONCEPT_LABEL.to_owned())),
        },
        DEFAULT_MIN_WORDS,
    );
    let business = Uuid::from_u128(0x212c);
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
        .bind(JOB)
        .bind(business)
        .bind(at().to_sqlx())
        .execute(&mut *db)
        .await
        .expect("card");
    let parts = PageParts {
        card,
        answers: Vec::new(),
        later: Vec::new(),
    };
    let page = job_page(&mut db, (JOB, business), &parts)
        .await
        .expect("page");
    let page = page.expect("holder");
    assert_eq!(
        page.photos(),
        [photo],
        "the card itself names its concept image"
    );
    assert_eq!(page.concept(), Some((concept, CONCEPT_LABEL)));
    let both = [
        "/photos/fake-photo-token".to_owned(),
        "/photos/fake-concept-token".to_owned(),
    ];
    let unsorted = JobPageLinks {
        job_number: "1042",
        photos: &both,
        concept: None,
    };
    let html = job_details_page(&page, &unsorted).expect("render");
    assert!(!html.contains("from the homeowner"), "{html}");
    let page = page.with_design(Some("Standard bath, matte gray"), None);
    assert_eq!(page.concept(), Some((concept, CONCEPT_LABEL)));
    let photos = ["/photos/fake-photo-token".to_owned()];
    let links = JobPageLinks {
        job_number: "1042",
        photos: &photos,
        concept: Some("/photos/fake-concept-token"),
    };
    let html = job_details_page(&page, &links).expect("render");
    assert!(
        html.contains("/photos/fake-photo-token") && html.contains("Standard bath, matte gray")
    );
    assert!(
        html.contains(&format!("<figcaption>{CONCEPT_LABEL}</figcaption>")),
        "{html}"
    );
}

/// TD-INT-27, TD-INT-28, PRIV-025, INT-046: a HEIC file gets the PRIV-025 reply, a file over the
/// byte or pixel limits is left off with its own error, a broken decode gets the re-ask, a failed
/// provider delete is pending for a retry, and a failed or unreadable concept image is no image;
/// an empty project file is none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_27_refusals_and_failures_keep_nothing(pool: PgPool) {
    let kit = kit("refusals");
    let mut conn = app(&pool).await;
    let keys = (&kit.seal, &kit.storage, &kit.secret);
    let mut heic = vec![0, 0, 0, 24];
    heic.extend_from_slice(b"ftypheic\0\0\0\0mif1heic");
    let mut broken = png(400, 400);
    broken.truncate(60);
    let huge = vec![0xFF; 16 * 1024 * 1024];
    let wide = png(8_001, 1);
    let want = [
        PhotoTaken::Heic,
        PhotoTaken::ReAsk,
        PhotoTaken::Refused,
        PhotoTaken::TooManyPixels,
    ];
    for (bytes, want) in [heic, broken, huge, wide].into_iter().zip(want) {
        let taken = take_upload(&mut conn, keys, (JOB, bytes), 320, at()).await;
        assert_eq!(taken, Ok(want));
    }
    assert_eq!(after_photo(PhotoTaken::Heic, false), PhotoStep::Heic);
    assert_eq!(
        after_photo(PhotoTaken::TooManyPixels, false),
        PhotoStep::LeftOff
    );
    let bytes = upload_screen(PhotoTaken::Refused, String::new);
    let pixels = upload_screen(PhotoTaken::TooManyPixels, String::new);
    assert_eq!(bytes, UploadScreen::Ask(Some(TOO_BIG_ERROR)));
    assert_eq!(pixels, UploadScreen::Ask(Some(TOO_MANY_PIXELS_ERROR)));
    let media = FakeMedia::new(Some(png(400, 400)));
    let url = "https://api.twilio.com/fail-delete";
    let texted = take_picture_text(&mut conn, keys, &media, (JOB, url), (320, at())).await;
    assert_eq!(texted.expect("taken").media, MediaState::DeletePending);
    for maker in [
        BrokenImages(None),
        BrokenImages(Some(b"not an image".to_vec())),
    ] {
        let deps = (keys, &maker);
        let made = make_concept(&mut conn, deps, (JOB, 0, true), &project(), at()).await;
        assert_eq!(made, Ok(Concept::Failed));
    }
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let blank = scrub("   ", &known, &mut NameMemory::default());
    assert_eq!(ProjectFile::from_scrubbed(&blank), None);
    assert!(format!("{:?}", project()).starts_with("ProjectFile("));
}
