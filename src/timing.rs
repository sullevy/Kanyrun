use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static ENABLED: AtomicBool = AtomicBool::new(false);

pub(crate) struct Span {
    inner: Option<SpanInner>,
}

struct SpanInner {
    start: Instant,
    last: Instant,
    label: &'static str,
}

impl Span {
    pub(crate) fn new(label: &'static str) -> Self {
        if !enabled() {
            return Self { inner: None };
        }

        let now = Instant::now();
        Self {
            inner: Some(SpanInner {
                start: now,
                last: now,
                label,
            }),
        }
    }

    pub(crate) fn mark(&mut self, stage: &str, detail: impl AsRef<str>) {
        let Some(inner) = self.inner.as_mut() else {
            return;
        };

        let now = Instant::now();
        let delta_us = now.duration_since(inner.last).as_micros();
        let total_us = now.duration_since(inner.start).as_micros();
        inner.last = now;

        let now_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_micros())
            .unwrap_or_default();
        let request = std::env::var("KANYRUN_TIMING_REQUEST_ID")
            .or_else(|_| std::env::var("KANYRUN_OPEN_REQUEST_ID"))
            .unwrap_or_else(|_| format!("p{}", std::process::id()));
        let detail = timing_value(detail.as_ref());

        let path = std::env::var("KANYRUN_TIMING_LOG")
            .unwrap_or_else(|_| "/tmp/kanyrun-timing.log".into());
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(
                file,
                "now_us={now_us} req={request} label={} stage={stage} delta_us={delta_us} total_us={total_us} {detail}",
                inner.label
            );
        }
    }
}

pub(crate) fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

fn timing_value(value: &str) -> String {
    value
        .replace(['\n', '\r'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
}
