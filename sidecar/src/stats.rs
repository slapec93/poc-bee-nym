use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

#[derive(Default)]
pub struct Stats {
    pub conns_out: AtomicU64,
    pub conns_in: AtomicU64,
    pub active: AtomicU64,
    pub failed: AtomicU64,
    pub bytes_to_mix: AtomicU64,
    pub bytes_from_mix: AtomicU64,
}

impl Stats {
    pub fn json(&self) -> String {
        format!(
            r#"{{"event":"stats","conns_out":{},"conns_in":{},"active":{},"failed":{},"bytes_to_mix":{},"bytes_from_mix":{}}}"#,
            self.conns_out.load(Relaxed),
            self.conns_in.load(Relaxed),
            self.active.load(Relaxed),
            self.failed.load(Relaxed),
            self.bytes_to_mix.load(Relaxed),
            self.bytes_from_mix.load(Relaxed),
        )
    }
}
