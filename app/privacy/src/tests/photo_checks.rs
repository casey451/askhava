//! Photo checks, decode limits, metadata stripping and the photo seal (TD-PRIV-04, TD-PRIV-08,
//! TD-PRIV-10 unit cases; PRIV-013, PRIV-014, PRIV-023, PRIV-025).
//!
//! Every fixture is made here from flat colours, with fake metadata sentinels; no real photo is
//! used. The storage, route and link cases are `tests/integration/tests/photo_links.rs`.

use std::io::Cursor;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use image::codecs::jpeg::{JpegDecoder, JpegEncoder};
use image::codecs::webp::WebPEncoder;
use image::{ImageDecoder, Rgb, RgbImage};
use proptest::prelude::*;
use secrecy::SecretString;
use sqlx::types::Uuid;

use crate::api::{
    CleanPhoto, DECODE_PERMITS, MAX_ALLOC, MAX_PHOTO_BYTES, PhotoError, PhotoKind, PhotoSeal,
    clean, sniff,
};
use crate::photos::clean_within;

const GPS: &[u8] = b"FAKE-GPS-34.4839N-114.3225W";
const XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta>FAKE-XMP</x:xmpmeta>";
const IPTC: &[u8] = b"Photoshop 3.0\08BIM\x04\x04FAKE-IPTC";
const MAKER: &[u8] = b"FAKE-MAKERNOTE-CAMERA";

/// A `width` by `height` JPEG of one colour.
fn jpeg(width: u32, height: u32, colour: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    let image = RgbImage::from_pixel(width, height, Rgb(colour));
    JpegEncoder::new(&mut out).encode_image(&image).unwrap();
    out
}

/// `jpeg` with the APPn segments `(marker, payload)` put right after its start marker.
fn with_segments(jpeg: &[u8], segments: &[(u8, &[u8])]) -> Vec<u8> {
    let mut out = jpeg[..2].to_vec();
    for (marker, payload) in segments {
        let len = u16::try_from(payload.len() + 2).unwrap();
        out.extend([0xFF, *marker]);
        out.extend(len.to_be_bytes());
        out.extend(*payload);
    }
    out.extend(&jpeg[2..]);
    out
}

/// An EXIF block with an orientation entry and the fake GPS and maker-note bytes after it.
fn exif(orientation: u16) -> Vec<u8> {
    let mut block = b"Exif\0\0MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01".to_vec();
    block.extend(orientation.to_be_bytes());
    block.extend([0, 0, 0, 0, 0, 0]);
    block.extend(GPS);
    block.extend(MAKER);
    block
}

/// A PNG chunk with its CRC-32.
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut crc = !0u32;
    for byte in kind.iter().chain(data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
        }
    }
    let len = u32::try_from(data.len()).unwrap().to_be_bytes();
    [&len[..], kind, data, &(!crc).to_be_bytes()].concat()
}

/// A PNG whose header claims `width` by `height` pixels, with an empty data chunk.
fn png_header(width: u32, height: u32) -> Vec<u8> {
    let ihdr = [
        &width.to_be_bytes()[..],
        &height.to_be_bytes(),
        &[8, 2, 0, 0, 0],
    ]
    .concat();
    [
        b"\x89PNG\r\n\x1a\n".to_vec(),
        chunk(b"IHDR", &ihdr),
        chunk(b"IDAT", &[]),
        chunk(b"IEND", &[]),
    ]
    .concat()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// A cleaned photo has no APP1 to APP15 segment, no sentinel and nothing after its end marker.
fn assert_bare(photo: &CleanPhoto, sentinels: &[&[u8]]) {
    let bytes = photo.as_bytes();
    assert!(bytes.ends_with(&[0xFF, 0xD9]));
    for marker in 0xE1..=0xEF {
        assert!(
            !contains(bytes, &[0xFF, marker]),
            "APP segment {marker:#x} kept"
        );
    }
    for sentinel in sentinels {
        assert!(!contains(bytes, sentinel));
    }
    let mut decoder = JpegDecoder::new(Cursor::new(bytes)).unwrap();
    assert_eq!(decoder.exif_metadata().unwrap(), None);
}

fn run<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

/// TD-PRIV-04, PRIV-013: the type comes from the bytes; a PDF named `.jpg` and an oversize upload
/// are refused.
#[test]
fn the_type_and_size_are_read_from_the_bytes() {
    assert_eq!(sniff(b"%PDF-1.7 fake.jpg"), Err(PhotoError::NotAPhoto));
    assert_eq!(sniff(&[]), Err(PhotoError::NotAPhoto));
    assert_eq!(sniff(&jpeg(2, 2, [9, 9, 9])), Ok(PhotoKind::Jpeg));
    assert_eq!(sniff(&png_header(2, 2)), Ok(PhotoKind::Png));
    let mut big = jpeg(2, 2, [9, 9, 9]);
    big.resize(MAX_PHOTO_BYTES + 1, 0);
    assert_eq!(sniff(&big), Err(PhotoError::TooLarge));
    assert_eq!(run(clean(big)), Err(PhotoError::TooLarge));
}

/// TD-PRIV-10, TD-PRIV-08, PRIV-025: a HEIC upload is refused before any decoder runs.
#[test]
fn a_heic_upload_is_refused_without_decoding() {
    let heic = b"\0\0\0\x18ftypheic\0\0\0\0mif1heic".to_vec();
    assert_eq!(sniff(&heic), Err(PhotoError::Heic));
    assert_eq!(run(clean(heic)), Err(PhotoError::Heic));
}

/// TD-PRIV-10, PRIV-025: headers claiming 100,000 px a side or 50 megapixels are refused before
/// the pixels are allocated, and broken bytes do not decode.
#[test]
fn oversized_headers_are_refused_before_allocation() {
    for (width, height) in [(100_000, 100_000), (7_500, 7_000), (8_001, 2)] {
        assert_eq!(
            run(clean(png_header(width, height))),
            Err(PhotoError::TooManyPixels)
        );
    }
    let mut sof = jpeg(8, 8, [1, 2, 3]);
    let at = sof.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap() + 5;
    sof[at..at + 4].copy_from_slice(&[0xFD, 0xE8, 0xFD, 0xE8]);
    assert_eq!(run(clean(sof)), Err(PhotoError::TooManyPixels));
    let broken = jpeg(8, 8, [1, 2, 3])[..40].to_vec();
    assert_eq!(run(clean(broken)), Err(PhotoError::Undecodable));
}

/// TD-PRIV-08, TD-PRIV-04, PRIV-014: EXIF with fake GPS and a maker note, XMP and IPTC all come
/// out stripped, and a rotated photo comes out upright (TD-PRIV-10).
#[test]
fn metadata_is_stripped_and_orientation_applied() {
    let base = jpeg(16, 8, [200, 30, 30]);
    let exif = exif(6);
    let upload = with_segments(&base, &[(0xE1, &exif), (0xE1, XMP), (0xED, IPTC)]);
    let photo = run(clean(upload)).unwrap();
    assert_bare(&photo, &[GPS, XMP, IPTC, MAKER, b"Exif"]);
    let decoder = JpegDecoder::new(Cursor::new(photo.as_bytes())).unwrap();
    assert_eq!(decoder.dimensions(), (8, 16));
}

/// TD-PRIV-08, PRIV-014: a PNG's text and EXIF chunks do not survive.
#[test]
fn png_metadata_is_stripped() {
    let mut png = Vec::new();
    let image = RgbImage::from_pixel(4, 4, Rgb([10, 200, 10]));
    image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let iend = png.len() - 12;
    let text = chunk(b"tEXt", &[b"GPS\0", GPS].concat());
    let upload = [
        &png[..iend],
        &text,
        &chunk(b"eXIf", &exif(1)[6..]),
        &png[iend..],
    ]
    .concat();
    assert_bare(&run(clean(upload)).unwrap(), &[GPS, MAKER]);
}

/// TD-PRIV-10, PRIV-025: a ZIP or HTML payload after the image is dropped.
#[test]
fn appended_payloads_are_dropped() {
    let mut upload = jpeg(8, 8, [5, 5, 200]);
    upload.extend(b"PK\x03\x04fake.zip<html><script>x</script></html>");
    assert_bare(&run(clean(upload)).unwrap(), &[b"PK\x03\x04", b"<html"]);
}

/// A WebP chunk with its even padding.
fn riff_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let len = u32::try_from(data.len()).unwrap().to_le_bytes();
    let pad: &[u8] = if data.len() % 2 == 1 { &[0] } else { &[] };
    [&kind[..], &len, data, pad].concat()
}

/// One lossless 2 by 2 frame of `colour` as an ANMF chunk.
fn frame(colour: [u8; 3]) -> Vec<u8> {
    let mut still = Vec::new();
    let image = RgbImage::from_pixel(2, 2, Rgb(colour));
    WebPEncoder::new_lossless(&mut still)
        .encode(&image, 2, 2, image::ExtendedColorType::Rgb8)
        .unwrap();
    let head = [0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 100, 0, 0, 0];
    riff_chunk(b"ANMF", &[&head[..], &still[12..]].concat())
}

/// TD-PRIV-10, PRIV-025: an animated WebP comes out as its first frame, one still JPEG.
#[test]
fn an_animated_webp_keeps_its_first_frame() {
    let vp8x = riff_chunk(b"VP8X", &[0x02, 0, 0, 0, 1, 0, 0, 1, 0, 0]);
    let body = [
        &b"WEBP"[..],
        &vp8x,
        &riff_chunk(b"ANIM", &[0; 6]),
        &frame([250, 0, 0]),
        &frame([0, 0, 250]),
    ]
    .concat();
    let len = u32::try_from(body.len()).unwrap().to_le_bytes();
    let upload = [&b"RIFF"[..], &len, &body].concat();
    assert_eq!(sniff(&upload), Ok(PhotoKind::WebP));
    let photo = run(clean(upload)).unwrap();
    let decoded = image::load_from_memory(photo.as_bytes()).unwrap().to_rgb8();
    assert_eq!(decoded.dimensions(), (2, 2));
    assert!(decoded.get_pixel(0, 0)[0] > 200 && decoded.get_pixel(0, 0)[2] < 50);
}

/// TD-PRIV-10, PRIV-025: ten uploads at once all finish through the decode gate, whose permits
/// cap decode memory at 512 MiB.
#[test]
fn ten_concurrent_uploads_stay_under_the_memory_cap() {
    assert!(u64::try_from(DECODE_PERMITS).unwrap() * MAX_ALLOC <= 512 * 1024 * 1024);
    let done = run(async {
        let uploads = (0..10).map(|_| tokio::spawn(clean(jpeg(1_000, 1_000, [7, 7, 7]))));
        let mut done = 0;
        for upload in uploads.collect::<Vec<_>>() {
            done += usize::from(upload.await.unwrap().is_ok());
        }
        done
    });
    assert_eq!(done, 10);
}

/// PRIV-025: a decode that outlasts its time limit is refused as busy.
#[test]
fn a_decode_past_its_time_limit_is_refused() {
    let upload = jpeg(2_000, 2_000, [3, 3, 3]);
    let outcome = run(clean_within(upload, std::time::Duration::ZERO));
    assert_eq!(outcome, Err(PhotoError::Busy));
}

fn seal() -> PhotoSeal {
    let key = format!("1:{}", STANDARD.encode([7u8; 32]));
    PhotoSeal::new(&SecretString::from(key)).unwrap()
}

/// TD-PRIV-04, PRIV-023: a sealed photo opens only under its own job and photo ID.
#[test]
fn a_sealed_photo_opens_only_under_its_own_ids() {
    let (job, a, b) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
    let sealed = seal().seal(job, a, b"fake photo").unwrap();
    assert!(!contains(&sealed, b"fake photo"));
    assert_eq!(seal().open(job, a, &sealed).unwrap(), b"fake photo");
    assert_eq!(seal().open(job, b, &sealed), Err(PhotoError::Sealed));
    assert_eq!(seal().open(b, a, &sealed), Err(PhotoError::Sealed));
    assert_eq!(seal().open(job, a, &sealed[..5]), Err(PhotoError::Sealed));
    let key = STANDARD.encode([7u8; 32]);
    let twice = format!("1:{key},1:{key}");
    for bad in [
        "",
        "1",
        "x:AAAA",
        "1:AAAA",
        "1:!!",
        &format!("0:{key}"),
        &twice,
    ] {
        assert!(PhotoSeal::new(&SecretString::from(bad)).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// TD-PRIV-08, PRIV-014: whatever fake metadata an upload carries, none comes out.
    #[test]
    fn generated_metadata_never_survives(tag in "[A-Z]{12,24}", marker in 0xE1u8..=0xEF) {
        let sentinel = format!("FAKE-{tag}");
        let upload = with_segments(&jpeg(8, 8, [90, 90, 90]), &[(marker, sentinel.as_bytes())]);
        assert_bare(&run(clean(upload)).unwrap(), &[sentinel.as_bytes()]);
    }
}
