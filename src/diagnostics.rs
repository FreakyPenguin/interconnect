//! Opt-in counters: no sampling timer, message contents, or Matrix identifiers.
use matrix_sdk_ui::timeline::Timeline;
use std::sync::{
    Arc, Mutex, OnceLock, Weak,
    atomic::{AtomicU64, Ordering},
};

static REDRAWS: AtomicU64 = AtomicU64::new(0);
static UPDATES: AtomicU64 = AtomicU64::new(0);
pub fn enabled() -> bool {
    *OnceLock::get_or_init(&ENABLED, || {
        std::env::var_os("INTERCONNECT_METRICS").is_some_and(|v| v == "1")
    })
}
static ENABLED: OnceLock<bool> = OnceLock::new();
pub fn redraw() {
    REDRAWS.fetch_add(1, Ordering::Relaxed);
}
pub fn update() {
    if enabled() {
        UPDATES.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Default)]
struct Counts {
    starts: [u64; 2],
    stops: [u64; 2],
    peak: [u64; 2],
    timelines: Vec<Weak<Timeline>>,
}
#[derive(Debug)]
pub struct Diagnostics {
    enabled: bool,
    counts: Mutex<Counts>,
}
impl Default for Diagnostics {
    fn default() -> Self {
        Self::new(enabled())
    }
}
impl Diagnostics {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            counts: Mutex::default(),
        }
    }
    pub fn view(self: &Arc<Self>, thread: bool) -> ViewLifetime {
        let kind = usize::from(thread);
        if self.enabled {
            let mut counts = self.counts.lock().unwrap();
            counts.starts[kind] += 1;
            counts.peak[kind] = counts.peak[kind].max(counts.starts[kind] - counts.stops[kind]);
        }
        ViewLifetime {
            diagnostics: self.clone(),
            kind,
        }
    }
    pub fn timeline(&self, timeline: &Arc<Timeline>) {
        if self.enabled {
            let mut counts = self.counts.lock().unwrap();
            counts.timelines.retain(|t| t.strong_count() > 0);
            counts.timelines.push(Arc::downgrade(timeline));
        }
    }
    pub fn snapshot(&self) -> serde_json::Value {
        let mut c = self.counts.lock().unwrap();
        c.timelines.retain(|t| t.strong_count() > 0);
        serde_json::json!({"metrics": true, "view_starts": c.starts, "view_stops": c.stops,
            "active_views": [c.starts[0]-c.stops[0], c.starts[1]-c.stops[1]], "peak_views": c.peak,
            "live_timelines": c.timelines.len(), "redraws": REDRAWS.load(Ordering::Relaxed),
            "ui_updates": UPDATES.load(Ordering::Relaxed)})
    }
    /// SIGUSR1 is installed only in opt-in runs. No periodic task wakes the UI.
    pub async fn reports(&self) {
        if !self.enabled {
            std::future::pending::<()>().await;
        }
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined1()) {
            Ok(mut signals) => {
                eprintln!("{}", self.snapshot());
                while signals.recv().await.is_some() {
                    eprintln!("{}", self.snapshot());
                }
            }
            Err(error) => eprintln!("metrics signal unavailable: {error}"),
        }
        std::future::pending::<()>().await;
    }
}
pub struct ViewLifetime {
    diagnostics: Arc<Diagnostics>,
    kind: usize,
}
impl Drop for ViewLifetime {
    fn drop(&mut self) {
        if self.diagnostics.enabled {
            self.diagnostics.counts.lock().unwrap().stops[self.kind] += 1;
        }
        tracing::debug!(target: "interconnect", thread = self.kind == 1, "connected presentation view stopped");
    }
}
