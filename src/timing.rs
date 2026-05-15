use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static ENABLED: OnceLock<bool> = OnceLock::new();

pub(crate) struct Span {
    start: Instant,
    last: Instant,
    label: &'static str,
}

impl Span {
    pub(crate) fn new(label: &'static str) -> Self {
        Self {
            start: Instant::now(),
            last: Instant::now(),
            label,
        }
    }

    pub(crate) fn mark(&mut self, stage: &str, detail: impl AsRef<str>) {
        if !enabled() {
            return;
        }

        let now = Instant::now();
        let delta_us = now.duration_since(self.last).as_micros();
        let total_us = now.duration_since(self.start).as_micros();
        self.last = now;

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
                self.label
            );
        }
    }
}

fn enabled() -> bool {
    *ENABLED.get_or_init(|| {
        std::env::var("KANYRUN_TIMING")
            .map(|value| value != "0")
            .unwrap_or(false)
    })
}

fn timing_value(value: &str) -> String {
    value
        .replace(['\n', '\r'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
}
