//! T275's cases: write-once storage under competing writers (TD-SEC-39), and
//! object delete and list with the Twilio body delete (TD-PRIV-19; SEC-018,
//! PRIV-009, PRIV-027, TXT-020, D-1841). Fake data only.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use askhava_core::api::Settings;
use secrecy::SecretString;

use super::adapter_config::stand_in;
use crate::api::{
    AdapterError, IdempotencyKey, LocalStorage, SmsAdapter, SmsBodyDelete, StorageAdapter,
    StorageDelete, TwilioConfig, twilio_clients,
};
use crate::storage::{MAX_PART, Step, write_once_observed};

/// Large enough that a partly written object would be seen by the reader.
const SIZE: usize = 1 << 20;
const SID: &str = "AC00000000000000000000000000000000";

/// How long a lockstep attempt waits for the other before the case fails.
const PATIENCE: Duration = Duration::from_secs(30);

type Outcome = Result<(), AdapterError>;
type Temporaries = Arc<Mutex<[Option<PathBuf>; 2]>>;

fn key(action: &str, id: &str) -> IdempotencyKey {
    IdempotencyKey::derive(&SecretString::from("fake-id-secret"), action, &[id])
}

/// A fresh directory for one case.
fn fresh_dir(case: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("askhava-t275-{case}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Reads `path` continuously until `stop`; returns each distinct content seen,
/// in order. Once an object was seen it must never vanish.
fn watch(path: PathBuf, stop: Arc<AtomicBool>) -> JoinHandle<Vec<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut seen: Vec<Vec<u8>> = Vec::new();
        loop {
            let done = stop.load(Ordering::SeqCst);
            match std::fs::read(&path) {
                Ok(bytes) if seen.last() != Some(&bytes) => seen.push(bytes),
                Ok(_) => {}
                Err(error) => {
                    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
                    assert!(seen.is_empty(), "a published object vanished");
                }
            }
            if done {
                return seen;
            }
        }
    })
}

/// The names left in `dir`, sorted.
fn left_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A two-party barrier whose wait panics after [`PATIENCE`], so an assertion
/// that panics in one attempt fails the case instead of leaving the other
/// attempt waiting forever (a `std::sync::Barrier` never times out).
#[derive(Default)]
struct Lockstep {
    /// Arrivals at the current step and the step's generation.
    state: Mutex<(usize, u64)>,
    turn: Condvar,
}

impl Lockstep {
    fn wait(&self) {
        let mut state = self.state.lock().unwrap();
        let generation = state.1;
        state.0 += 1;
        if state.0 == 2 {
            *state = (0, generation + 1);
            self.turn.notify_all();
            return;
        }
        let (_state, waited) = self
            .turn
            .wait_timeout_while(state, PATIENCE, |s| s.1 == generation)
            .unwrap();
        assert!(
            !waited.timed_out(),
            "the other attempt never reached this step"
        );
    }
}

/// One lockstep attempt `me` (0 or 1): both stop after creating and after
/// writing, both try the link before either removes; attempt 1 removes only
/// after attempt 0 checked that removing its own file left attempt 1's.
fn lockstep(
    me: usize,
    bytes: Vec<u8>,
    path: PathBuf,
    barrier: Arc<Lockstep>,
    temporaries: Temporaries,
    (done, go): (Option<Sender<()>>, Option<Receiver<()>>),
) -> JoinHandle<Outcome> {
    std::thread::spawn(move || {
        let observe = &mut |step: Step, temporary: &Path| match step {
            Step::Created => {
                temporaries.lock().unwrap()[me] = Some(temporary.to_owned());
                barrier.wait();
            }
            Step::Written => {
                assert_eq!(std::fs::read(temporary).unwrap(), bytes);
                barrier.wait();
            }
            Step::Linked => {
                barrier.wait();
                if let Some(go) = &go {
                    go.recv().unwrap();
                }
            }
            Step::Removed => {
                assert!(!temporary.exists(), "an attempt left its own file");
                if let Some(done) = &done {
                    let other = temporaries.lock().unwrap()[1 - me].clone().unwrap();
                    assert!(other.exists(), "an attempt removed another's file");
                    done.send(()).unwrap();
                }
            }
        };
        write_once_observed(&path, &bytes, observe)
    })
}

/// Races two attempts in lockstep while a reader watches the final path;
/// returns both outcomes and what the reader saw.
fn race(case: &str, first: Vec<u8>, second: Vec<u8>) -> ([Outcome; 2], Vec<Vec<u8>>) {
    let dir = fresh_dir(case);
    let path = dir.join("object.bin");
    let stop = Arc::new(AtomicBool::new(false));
    let reader = watch(path.clone(), Arc::clone(&stop));
    let barrier = Arc::new(Lockstep::default());
    let temporaries: Temporaries = Arc::default();
    let (done, go) = mpsc::channel();
    let shared = |me, bytes, signal| {
        let parts = (Arc::clone(&barrier), Arc::clone(&temporaries));
        lockstep(me, bytes, path.clone(), parts.0, parts.1, signal)
    };
    let a = shared(0, first, (Some(done), None));
    let b = shared(1, second, (None, Some(go)));
    let outcomes = [a.join().unwrap(), b.join().unwrap()];
    stop.store(true, Ordering::SeqCst);
    let seen = reader.join().unwrap();
    assert_eq!(left_in(&dir), ["object.bin"], "only the object remains");
    let _ = std::fs::remove_dir_all(dir);
    (outcomes, seen)
}

/// TD-SEC-39; SEC-018, D-1841: two attempts with the same bytes in lockstep both
/// succeed; the reader sees nothing or the whole object, which never changes.
#[test]
fn same_bytes_writers_in_lockstep_publish_one_complete_object() {
    let (outcomes, seen) = race("same", vec![b'a'; SIZE], vec![b'a'; SIZE]);
    assert_eq!(outcomes, [Ok(()), Ok(())]);
    assert!(seen.len() <= 1 && seen.iter().all(|bytes| *bytes == vec![b'a'; SIZE]));
}

/// TD-SEC-39; SEC-018, D-1841: with different bytes one attempt wins, the loser
/// gets `ObjectExists`, and the published object is the winner's, never partial
/// and never changed.
#[test]
fn different_bytes_writers_in_lockstep_leave_the_winner_untouched() {
    let (first, second) = (vec![b'a'; SIZE], vec![b'b'; SIZE]);
    let (outcomes, seen) = race("different", first.clone(), second.clone());
    let winner = match outcomes {
        [Ok(()), Err(AdapterError::ObjectExists)] => first,
        [Err(AdapterError::ObjectExists), Ok(())] => second,
        other => panic!("one winner and one ObjectExists expected: {other:?}"),
    };
    assert!(seen.len() <= 1 && seen.iter().all(|bytes| *bytes == winner));
}

/// A late attempt that passed the existence check before `first` was
/// published, then wrote its own file only after: the published object
/// must not change (the B05 race, D-1841).
fn late_writer(case: &str, first: &[u8], late: Vec<u8>) -> Outcome {
    let dir = fresh_dir(case);
    let path = dir.join("object.bin");
    let stop = Arc::new(AtomicBool::new(false));
    let reader = watch(path.clone(), Arc::clone(&stop));
    let (created, at_created) = mpsc::channel();
    let (go, wait_go) = mpsc::channel::<()>();
    let late_path = path.clone();
    let published = first.to_vec();
    let attempt = std::thread::spawn(move || {
        let observe = &mut |step: Step, _: &Path| match step {
            Step::Created => {
                created.send(()).unwrap();
                wait_go.recv().unwrap();
            }
            Step::Written => assert_eq!(std::fs::read(&late_path).unwrap(), published),
            Step::Linked | Step::Removed => {}
        };
        write_once_observed(&late_path, &late, observe)
    });
    at_created.recv().unwrap();
    write_once_observed(&path, first, &mut |_, _| {}).unwrap();
    go.send(()).unwrap();
    let outcome = attempt.join().unwrap();
    stop.store(true, Ordering::SeqCst);
    let seen = reader.join().unwrap();
    assert!(seen.len() <= 1 && seen.iter().all(|bytes| bytes == first));
    assert_eq!(std::fs::read(&path).unwrap(), first);
    assert_eq!(left_in(&dir), ["object.bin"]);
    let _ = std::fs::remove_dir_all(dir);
    outcome
}

/// TD-SEC-39; SEC-018, D-1841: a retry that loses the race to a published
/// object never empties or changes it: other bytes are `ObjectExists`, the
/// same bytes a successful retry.
#[test]
fn a_late_writer_never_changes_a_published_object() {
    let first = vec![b'a'; SIZE];
    assert_eq!(
        late_writer("late-other", &first, vec![b'b'; 16]),
        Err(AdapterError::ObjectExists)
    );
    assert_eq!(late_writer("late-same", &first, first.clone()), Ok(()));
}

/// TD-PRIV-19; PRIV-009: delete removes an object and an absent one is success;
/// a listing covers its prefix only and never a temporary file.
#[tokio::test]
async fn delete_and_list_objects_by_prefix() {
    let dir = fresh_dir("delete");
    let store = LocalStorage::new(&format!("file://{}", dir.display())).unwrap();
    let k = key("storage.put", "photo-1");
    for name in [
        "photos/job-1/a.bin",
        "photos/job-2/b.bin",
        "stream/c.bin",
        "pho.bin",
    ] {
        store.put_object(&k, name, b"fake".to_vec()).await.unwrap();
    }
    std::fs::write(dir.join("photos/job-1/.a.bin.0f.part"), b"x").unwrap();
    let photos = store.list_objects("photos/").await.unwrap();
    assert_eq!(photos, ["photos/job-1/a.bin", "photos/job-2/b.bin"]);
    let job = store.list_objects("photos/job-1").await.unwrap();
    assert_eq!(job, ["photos/job-1/a.bin"]);
    assert_eq!(store.list_objects("pho").await.unwrap().len(), 3);
    assert_eq!(store.list_objects("missing/").await, Ok(Vec::new()));
    assert_eq!(
        store.list_objects("../").await,
        Err(AdapterError::InvalidId)
    );
    assert_eq!(
        store.list_objects("a/.x").await,
        Err(AdapterError::InvalidId)
    );
    let delete = key("storage.delete", "photo-1");
    store
        .delete_object(&delete, "photos/job-1/a.bin")
        .await
        .unwrap();
    store
        .delete_object(&delete, "photos/job-1/a.bin")
        .await
        .unwrap();
    assert!(store.read_object("photos/job-1/a.bin").await.is_err());
    assert_eq!(store.read_object("stream/c.bin").await.unwrap(), b"fake");
    let bad = store.delete_object(&delete, "../escape.bin").await;
    assert_eq!(bad, Err(AdapterError::InvalidId));
    let _ = std::fs::remove_dir_all(dir);
}

/// SEC-032, D-2158: a name part longer than `MAX_PART` bytes is `InvalidId`
/// before any write, since its temporary file name could never be created;
/// a part of exactly `MAX_PART` bytes is written and read back.
#[tokio::test]
async fn a_name_part_too_long_to_write_is_refused() {
    let dir = fresh_dir("long");
    let store = LocalStorage::new(&format!("file://{}", dir.display())).unwrap();
    let k = key("storage.put", "photo-long");
    let longest = format!("photos/{}", "a".repeat(MAX_PART));
    store
        .put_object(&k, &longest, b"fake".to_vec())
        .await
        .unwrap();
    assert_eq!(store.read_object(&longest).await.unwrap(), b"fake");
    let too_long = format!("photos/{}", "a".repeat(MAX_PART + 1));
    let refused = store.put_object(&k, &too_long, b"fake".to_vec()).await;
    assert_eq!(refused, Err(AdapterError::InvalidId));
    assert_eq!(left_in(&dir.join("photos")), ["a".repeat(MAX_PART)]);
    let _ = std::fs::remove_dir_all(dir);
}

fn stand_in_config(port: u16) -> TwilioConfig {
    let base = format!("http://127.0.0.1:{port}/twilio");
    let numbers: Vec<String> = (0..10).map(|i| format!("+1500555{:04}", 100 + i)).collect();
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "ci"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        ("ASKHAVA_TWILIO_ACCOUNT_SID", SID),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_NUMBERS", &numbers.join(",")),
        ("ASKHAVA_TWILIO_BASE_URL", &base),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", &base),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    TwilioConfig::from_settings(&Settings::from_pairs(pairs.clone()).unwrap(), pairs).unwrap()
}

/// TD-PRIV-19; PRIV-027, TXT-020: the body delete is Twilio's redaction over
/// HTTP; a repeat succeeds, an unknown message is 404, a bad SID never leaves.
#[tokio::test]
async fn message_body_delete_redacts_at_the_stand_in() {
    let server = stand_in();
    let (sms, _, _) = twilio_clients(&stand_in_config(server.1)).unwrap();
    let send = key("sms.send", "outbox-1");
    let sent = sms
        .send_text(&send, "+15005550010", "+15005550006", "fake text")
        .await
        .unwrap();
    let purge = key("sms.body_delete", "outbox-1");
    assert_eq!(sms.delete_message_body(&purge, &sent.sid).await, Ok(()));
    assert_eq!(sms.delete_message_body(&purge, &sent.sid).await, Ok(()));
    let url = format!(
        "http://127.0.0.1:{}/twilio/2010-04-01/Accounts/{SID}/Messages/{}.json",
        server.1, sent.sid
    );
    let request = reqwest::Client::new()
        .get(url)
        .basic_auth(SID, Some("fake-auth-token"));
    let message: serde_json::Value = request.send().await.unwrap().json().await.unwrap();
    assert_eq!(message["body"], "");
    let missing = format!("SM{}", "f".repeat(32));
    let gone = AdapterError::Vendor {
        status: 404,
        code: Some(20404),
    };
    assert_eq!(sms.delete_message_body(&purge, &missing).await, Err(gone));
    let bad = sms.delete_message_body(&purge, "SM../x").await;
    assert_eq!(bad, Err(AdapterError::InvalidId));
}
