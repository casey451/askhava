//! One process-global log capture for the integration tests (D-4419, D-4681; R5, R6, PRIV-016).
//!
//! `tracing` caches each callsite's interest the first time any thread reaches it. A per-thread
//! capture (`tracing::subscriber::set_default`) leaves a callsite another test reached first, with
//! no subscriber in place or while the cache was being rebuilt, cached as `Interest::never`, and
//! the capture then never sees its event. Here one subscriber is installed once per test binary
//! with `set_global_default`, answers `Interest::always()` and `enabled` true for every callsite,
//! and the interest cache is rebuilt after it is in place, so no callsite stays cached as never.
//! It appends each event's fields, each new span's fields and each value later recorded on a span
//! (` name=value`, the value's `Debug`) to the buffer of the capture [`start`]ed on the current
//! thread, if any; a capture never sees another thread's events.

use std::cell::RefCell;
use std::sync::{Arc, Mutex, OnceLock};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Metadata, Subscriber};

/// The buffer a capture appends to, and the target prefix it skips, if any.
#[derive(Clone)]
struct Sink {
    lines: Arc<Mutex<Vec<String>>>,
    skip: Option<&'static str>,
}

thread_local! {
    /// The capture started on this thread, if any.
    static CURRENT: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

/// A running capture of the current thread's log lines; dropping it ends the capture.
pub struct Capture {
    sink: Sink,
    previous: Option<Sink>,
}

impl Capture {
    /// Every captured line so far, one per event or span, in order.
    ///
    /// # Panics
    ///
    /// When a thread panicked while appending to this capture's buffer.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.sink.lines.lock().expect("captured lines").clone()
    }

    /// Every captured line so far, joined with newlines.
    #[must_use]
    pub fn all(&self) -> String {
        self.lines().join("\n")
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT.with(|current| *current.borrow_mut() = previous);
    }
}

/// Starts capturing every log line of the current thread until the guard drops (D-4419).
///
/// # Panics
///
/// When another global subscriber was set in this test binary before the first capture.
#[must_use]
pub fn start() -> Capture {
    begin(None)
}

/// As [`start`], but skips lines whose target starts with `prefix` (for example `"sqlx"`).
#[must_use]
pub fn start_skipping(prefix: &'static str) -> Capture {
    begin(Some(prefix))
}

/// Installs the global subscriber once, then makes a new capture this thread's current one.
fn begin(skip: Option<&'static str>) -> Capture {
    install();
    let sink = Sink {
        lines: Arc::default(),
        skip,
    };
    let previous = CURRENT.with(|current| current.borrow_mut().replace(sink.clone()));
    Capture { sink, previous }
}

/// Sets [`Global`] as the process's subscriber once and rebuilds every callsite's interest.
fn install() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        tracing::subscriber::set_global_default(Global::default())
            .expect("no other global subscriber in an integration test binary");
        tracing::callsite::rebuild_interest_cache();
    });
}

/// Appends `fields` (written by `record`) to this thread's capture, unless it skips `meta`'s target.
fn append(meta: &Metadata<'_>, record: impl FnOnce(&mut dyn Visit)) {
    let sink = CURRENT.with(|current| current.borrow().clone());
    let Some(sink) = sink else { return };
    if sink
        .skip
        .is_some_and(|prefix| meta.target().starts_with(prefix))
    {
        return;
    }
    let mut line = String::new();
    record(&mut Line(&mut line));
    sink.lines.lock().expect("captured lines").push(line);
}

/// Writes each field as ` name=value`, the value's `Debug`.
struct Line<'a>(&'a mut String);

impl Visit for Line<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}

/// The process-global subscriber: interested in every callsite, forwarding to [`CURRENT`].
///
/// It keeps each span's metadata, the span's `Id` being its index plus one, so a value recorded
/// on a span later is appended under the span's target, and skipped like the span itself.
#[derive(Default)]
struct Global {
    spans: Mutex<Vec<&'static Metadata<'static>>>,
}

impl Global {
    /// The metadata of the span `id` names, if this subscriber issued it.
    fn span(&self, id: &Id) -> Option<&'static Metadata<'static>> {
        let index = usize::try_from(id.into_u64()).ok()?.checked_sub(1)?;
        self.spans
            .lock()
            .expect("span metadata")
            .get(index)
            .copied()
    }
}

impl Subscriber for Global {
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::always()
    }
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        append(attributes.metadata(), |v| attributes.record(v));
        let mut spans = self.spans.lock().expect("span metadata");
        spans.push(attributes.metadata());
        Id::from_u64(u64::try_from(spans.len()).expect("span count"))
    }
    fn record(&self, span: &Id, values: &Record<'_>) {
        if let Some(meta) = self.span(span) {
            append(meta, |v| values.record(v));
        }
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        append(event.metadata(), |v| event.record(v));
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

#[cfg(test)]
mod tests {
    use super::{start, start_skipping};

    /// The callsite only the other-thread-first case reaches, first on another thread.
    fn first_elsewhere(n: u32) {
        tracing::warn!(probe = n, "log capture probe reached first elsewhere");
    }

    /// The callsite the other cases reach.
    fn probe(n: u32) {
        tracing::warn!(probe = n, "log capture probe");
    }

    /// Every case in one test, in a fixed order: this is the only test in the crate's library
    /// binary, so nothing has called [`start`] when `first_elsewhere` is first reached on another
    /// thread, with no subscriber in place (the case D-4419 missed), and no other test can reach
    /// that callsite or install the subscriber earlier, whatever the runner's thread order.
    #[test]
    fn every_line_of_this_thread_is_captured_and_nothing_else() {
        a_callsite_reached_first_on_another_thread_is_still_captured();
        a_dropped_capture_sees_nothing_more_and_skipping_drops_the_target();
        a_value_recorded_on_a_span_later_is_captured();
    }

    fn a_callsite_reached_first_on_another_thread_is_still_captured() {
        std::thread::spawn(|| first_elsewhere(1))
            .join()
            .expect("other thread");
        let capture = start();
        first_elsewhere(2);
        std::thread::spawn(|| first_elsewhere(3))
            .join()
            .expect("other thread");
        let lines = capture.lines();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("probe=2"), "{lines:?}");
    }

    fn a_dropped_capture_sees_nothing_more_and_skipping_drops_the_target() {
        let first = start();
        probe(4);
        let lines = {
            let skipping = start_skipping(module_path!());
            probe(5);
            skipping.lines()
        };
        probe(6);
        let kept = first.lines();
        assert!(lines.is_empty(), "{lines:?}");
        assert_eq!(kept.len(), 2, "{kept:?}");
        assert!(kept[0].contains("probe=4") && kept[1].contains("probe=6"));
    }

    fn a_value_recorded_on_a_span_later_is_captured() {
        let capture = start();
        let span = tracing::warn_span!("probe span", contact = tracing::field::Empty);
        span.record("contact", "fake-contact");
        let lines = capture.lines();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[1].contains("contact=\"fake-contact\""), "{lines:?}");
        let skipped = {
            let skipping = start_skipping(module_path!());
            span.record("contact", "other-fake-contact");
            skipping.lines()
        };
        assert!(skipped.is_empty(), "{skipped:?}");
    }
}
