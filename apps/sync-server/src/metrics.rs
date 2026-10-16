//! Counters and rate limiting. Nothing here ever records document contents or identifiers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Default)]
pub struct Metrics {
    pub requests: AtomicU64,
    pub auth_failures: AtomicU64,
    pub rate_limited: AtomicU64,
    pub server_errors: AtomicU64,
    pub ops_pushed: AtomicU64,
    pub ops_duplicate: AtomicU64,
    pub ops_pulled: AtomicU64,
    pub op_bytes_in: AtomicU64,
    pub blob_chunks_in: AtomicU64,
    pub blob_bytes_in: AtomicU64,
    pub blob_chunks_out: AtomicU64,
    pub workspaces_created: AtomicU64,
    pub devices_revoked: AtomicU64,
}

impl Metrics {
    pub fn add(counter: &AtomicU64, n: u64) {
        counter.fetch_add(n, Ordering::Relaxed);
    }

    /// Prometheus text exposition format.
    pub fn render(&self) -> String {
        let rows: [(&str, &str, &AtomicU64); 13] = [
            ("requests_total", "HTTP requests handled", &self.requests),
            (
                "auth_failures_total",
                "Requests rejected by authentication",
                &self.auth_failures,
            ),
            (
                "rate_limited_total",
                "Requests rejected by rate limiting",
                &self.rate_limited,
            ),
            (
                "server_errors_total",
                "Responses with a 5xx status",
                &self.server_errors,
            ),
            ("ops_pushed_total", "Operations accepted", &self.ops_pushed),
            (
                "ops_duplicate_total",
                "Operations ignored as already stored",
                &self.ops_duplicate,
            ),
            ("ops_pulled_total", "Operations delivered", &self.ops_pulled),
            (
                "op_bytes_in_total",
                "Operation ciphertext bytes accepted",
                &self.op_bytes_in,
            ),
            (
                "blob_chunks_in_total",
                "Attachment chunks accepted",
                &self.blob_chunks_in,
            ),
            (
                "blob_bytes_in_total",
                "Attachment ciphertext bytes accepted",
                &self.blob_bytes_in,
            ),
            (
                "blob_chunks_out_total",
                "Attachment chunks served",
                &self.blob_chunks_out,
            ),
            (
                "workspaces_created_total",
                "Workspaces created",
                &self.workspaces_created,
            ),
            (
                "devices_revoked_total",
                "Devices revoked",
                &self.devices_revoked,
            ),
        ];
        let mut out = String::new();
        for (name, help, v) in rows {
            out.push_str(&format!(
                "# HELP notercrux_{name} {help}\n# TYPE notercrux_{name} counter\nnotercrux_{name} {}\n",
                v.load(Ordering::Relaxed)
            ));
        }
        out
    }
}

/// Fixed one-minute windows, keyed by device or client address.
#[derive(Default)]
pub struct Limiter {
    windows: Mutex<HashMap<String, (i64, u32)>>,
}

impl Limiter {
    pub fn allow(&self, key: &str, limit: u32, now: i64) -> bool {
        if limit == 0 {
            return true;
        }
        let window = now / 60;
        let mut map = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() > 20_000 {
            map.retain(|_, (w, _)| *w >= window);
        }
        let entry = map.entry(key.to_string()).or_insert((window, 0));
        if entry.0 != window {
            *entry = (window, 0);
        }
        entry.1 += 1;
        entry.1 <= limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_resets_each_minute_and_isolates_keys() {
        let l = Limiter::default();
        assert!(l.allow("a", 2, 0) && l.allow("a", 2, 1));
        assert!(!l.allow("a", 2, 2));
        assert!(l.allow("b", 2, 2));
        assert!(l.allow("a", 2, 60));
        assert!(l.allow("a", 0, 61), "a zero limit disables limiting");
    }

    #[test]
    fn metrics_render_as_prometheus_text() {
        let m = Metrics::default();
        Metrics::add(&m.ops_pushed, 3);
        let text = m.render();
        assert!(text.contains("notercrux_ops_pushed_total 3"));
        assert!(text.contains("# TYPE notercrux_requests_total counter"));
    }
}
