//! Lightweight in-process metrics + Prometheus text exporter.
//!
//! We hand-roll a small set of counters / gauges / sum-and-count
//! "summaries" instead of pulling in the `metrics` + `metrics-exporter-
//! prometheus` ecosystem. That keeps the dependency footprint flat
//! and lets the format match the rest of the project (one global
//! struct behind an `Arc`, no macros).
//!
//! Histograms here use fixed bucket boundaries so a request's latency
//! lands in exactly one bucket; the exporter emits the cumulative
//! counts Prometheus expects. The bucket set is biased toward typical
//! /convert latencies (50 ms – a few seconds).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const REQUEST_LATENCY_BUCKETS_SECS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];

#[derive(Default)]
pub struct Counter {
    value: AtomicU64,
}
impl Counter {
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }
    #[allow(dead_code)]
    pub fn add(&self, by: u64) {
        self.value.fetch_add(by, Ordering::Relaxed);
    }
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
#[allow(dead_code)]
pub struct Gauge {
    value: AtomicU64,
}
#[allow(dead_code)]
impl Gauge {
    pub fn set(&self, v: u64) {
        self.value.store(v, Ordering::Relaxed);
    }
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// Histogram: tracks per-bucket cumulative counts + a sum so Prometheus
/// can compute `_bucket`, `_sum`, `_count` lines and quantiles.
struct Histogram {
    bounds: &'static [f64],
    counts: Vec<AtomicU64>, // one per bucket plus the implicit +Inf
    sum_milli: AtomicU64,   // sum in milliseconds to stay integer-only
    total: AtomicU64,
}
impl Histogram {
    fn new(bounds: &'static [f64]) -> Self {
        let mut counts = Vec::with_capacity(bounds.len() + 1);
        for _ in 0..=bounds.len() {
            counts.push(AtomicU64::new(0));
        }
        Histogram {
            bounds,
            counts,
            sum_milli: AtomicU64::new(0),
            total: AtomicU64::new(0),
        }
    }
    fn observe(&self, secs: f64) {
        // Find the first bucket whose upper bound is >= the observation.
        // Anything past the last bound goes in the +Inf bucket.
        let idx = self
            .bounds
            .iter()
            .position(|b| secs <= *b)
            .unwrap_or(self.bounds.len());
        self.counts[idx].fetch_add(1, Ordering::Relaxed);
        self.sum_milli
            .fetch_add((secs * 1000.0).max(0.0) as u64, Ordering::Relaxed);
        self.total.fetch_add(1, Ordering::Relaxed);
    }
}

/// Bag of metrics shared across the whole app. Methods are cheap and
/// safe to call from any task.
pub struct Metrics {
    pub requests_total: Counter,
    pub responses_2xx: Counter,
    pub responses_4xx: Counter,
    pub responses_5xx: Counter,
    pub rate_limited_total: Counter,
    pub conversions_total: Counter,
    pub conversion_failures_total: Counter,
    pub pdf_renders_total: Counter,
    pub pdf_render_failures_total: Counter,
    pub url_watch_polls_total: Counter,
    pub url_watch_deliveries_total: Counter,
    pub url_watch_failures_total: Counter,
    pub started_at_seconds: AtomicU64,

    request_latency: Histogram,

    /// Per-path request counters, populated lazily. Keys are
    /// `"GET /convert"` etc; we cap the map so a misbehaving client
    /// scanning random paths can't blow it up.
    per_path: Mutex<HashMap<String, AtomicU64>>,
    per_path_cap: usize,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            requests_total: Counter::default(),
            responses_2xx: Counter::default(),
            responses_4xx: Counter::default(),
            responses_5xx: Counter::default(),
            rate_limited_total: Counter::default(),
            conversions_total: Counter::default(),
            conversion_failures_total: Counter::default(),
            pdf_renders_total: Counter::default(),
            pdf_render_failures_total: Counter::default(),
            url_watch_polls_total: Counter::default(),
            url_watch_deliveries_total: Counter::default(),
            url_watch_failures_total: Counter::default(),
            started_at_seconds: AtomicU64::new(now_seconds()),
            request_latency: Histogram::new(REQUEST_LATENCY_BUCKETS_SECS),
            per_path: Mutex::new(HashMap::new()),
            per_path_cap: 256,
        }
    }
}

fn now_seconds() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Metrics {
    pub fn observe_request(
        &self,
        method: &str,
        normalised_path: &str,
        status: u16,
        elapsed_secs: f64,
    ) {
        self.requests_total.inc();
        match status / 100 {
            2 => self.responses_2xx.inc(),
            4 => self.responses_4xx.inc(),
            5 => self.responses_5xx.inc(),
            _ => {}
        }
        if status == 429 {
            self.rate_limited_total.inc();
        }
        self.request_latency.observe(elapsed_secs);

        let key = format!("{method} {normalised_path}");
        let mut map = match self.per_path.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if let Some(c) = map.get(&key) {
            c.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if map.len() >= self.per_path_cap {
            // Drop the request-path label rather than grow unbounded;
            // the overall counters still capture the activity.
            return;
        }
        map.insert(key, AtomicU64::new(1));
    }

    /// Render the metrics as a Prometheus text-format response body.
    /// We pre-allocate a reasonable buffer because this gets scraped
    /// every few seconds in production and we want it fast.
    pub fn render(&self, gauges: &RuntimeGauges) -> String {
        let mut out = String::with_capacity(4096);

        meta(&mut out, "udc_requests_total", "counter", "Total HTTP requests served.");
        line(&mut out, "udc_requests_total", "", self.requests_total.get());

        meta(&mut out, "udc_responses_total", "counter", "HTTP responses by class.");
        line_labels(&mut out, "udc_responses_total", "class=\"2xx\"", self.responses_2xx.get());
        line_labels(&mut out, "udc_responses_total", "class=\"4xx\"", self.responses_4xx.get());
        line_labels(&mut out, "udc_responses_total", "class=\"5xx\"", self.responses_5xx.get());

        meta(&mut out, "udc_rate_limited_total", "counter", "Requests rejected by the rate limiter.");
        line(&mut out, "udc_rate_limited_total", "", self.rate_limited_total.get());

        meta(&mut out, "udc_conversions_total", "counter", "Successful conversions (any format).");
        line(&mut out, "udc_conversions_total", "", self.conversions_total.get());
        meta(&mut out, "udc_conversion_failures_total", "counter", "Conversions that failed.");
        line(&mut out, "udc_conversion_failures_total", "", self.conversion_failures_total.get());

        meta(&mut out, "udc_pdf_renders_total", "counter", "Successful Chromium PDF renders.");
        line(&mut out, "udc_pdf_renders_total", "", self.pdf_renders_total.get());
        meta(&mut out, "udc_pdf_render_failures_total", "counter", "Chromium PDF renders that failed.");
        line(&mut out, "udc_pdf_render_failures_total", "", self.pdf_render_failures_total.get());

        meta(&mut out, "udc_url_watch_polls_total", "counter", "URL-watch fetches the worker has run.");
        line(&mut out, "udc_url_watch_polls_total", "", self.url_watch_polls_total.get());
        meta(&mut out, "udc_url_watch_deliveries_total", "counter", "URL-watch deliveries the worker successfully POSTed.");
        line(&mut out, "udc_url_watch_deliveries_total", "", self.url_watch_deliveries_total.get());
        meta(&mut out, "udc_url_watch_failures_total", "counter", "URL-watch poll/delivery failures.");
        line(&mut out, "udc_url_watch_failures_total", "", self.url_watch_failures_total.get());

        // Histogram: emit cumulative counts per Prometheus convention.
        meta(
            &mut out,
            "udc_request_duration_seconds",
            "histogram",
            "End-to-end HTTP request latency, seconds.",
        );
        let mut cumulative = 0u64;
        for (i, bound) in self.request_latency.bounds.iter().enumerate() {
            cumulative += self.request_latency.counts[i].load(Ordering::Relaxed);
            let label = format!("le=\"{}\"", format_bucket(*bound));
            line_labels(&mut out, "udc_request_duration_seconds_bucket", &label, cumulative);
        }
        cumulative += self.request_latency.counts[self.request_latency.bounds.len()]
            .load(Ordering::Relaxed);
        line_labels(
            &mut out,
            "udc_request_duration_seconds_bucket",
            "le=\"+Inf\"",
            cumulative,
        );
        let sum_secs =
            self.request_latency.sum_milli.load(Ordering::Relaxed) as f64 / 1000.0;
        out.push_str(&format!("udc_request_duration_seconds_sum {sum_secs}\n"));
        out.push_str(&format!(
            "udc_request_duration_seconds_count {}\n",
            self.request_latency.total.load(Ordering::Relaxed)
        ));

        // Per-path counts. We sort for stable scrape output.
        meta(&mut out, "udc_requests_by_path_total", "counter", "Requests labelled by method + normalised path.");
        if let Ok(map) = self.per_path.lock() {
            let mut entries: Vec<(&String, u64)> = map
                .iter()
                .map(|(k, v)| (k, v.load(Ordering::Relaxed)))
                .collect();
            entries.sort_by_key(|(k, _)| (*k).clone());
            for (k, v) in entries {
                let (method, path) = k.split_once(' ').unwrap_or(("?", k));
                let label = format!("method=\"{method}\",path=\"{}\"", escape_label(path));
                line_labels(&mut out, "udc_requests_by_path_total", &label, v);
            }
        }

        // Runtime gauges sampled at scrape time.
        meta(&mut out, "udc_db_pool_size", "gauge", "Active connections in the SQLite pool.");
        line(&mut out, "udc_db_pool_size", "", gauges.db_pool_size);
        meta(&mut out, "udc_url_watches_active", "gauge", "Enabled URL watches.");
        line(&mut out, "udc_url_watches_active", "", gauges.url_watches_active);
        meta(&mut out, "udc_jobs_queued", "gauge", "Jobs waiting in the work queue.");
        line(&mut out, "udc_jobs_queued", "", gauges.jobs_queued);
        meta(&mut out, "udc_jobs_running", "gauge", "Jobs currently being processed.");
        line(&mut out, "udc_jobs_running", "", gauges.jobs_running);
        meta(&mut out, "udc_rate_limit_buckets", "gauge", "Currently tracked rate-limit buckets.");
        line(&mut out, "udc_rate_limit_buckets", "", gauges.rate_limit_buckets);

        meta(&mut out, "udc_uptime_seconds", "gauge", "Seconds since the API process started.");
        let up = now_seconds().saturating_sub(self.started_at_seconds.load(Ordering::Relaxed));
        line(&mut out, "udc_uptime_seconds", "", up);

        out
    }
}

/// Read-once gauge snapshot collected at scrape time so we don't have
/// to thread mutable state through every handler.
pub struct RuntimeGauges {
    pub db_pool_size: u64,
    pub url_watches_active: u64,
    pub jobs_queued: u64,
    pub jobs_running: u64,
    pub rate_limit_buckets: u64,
}

fn meta(out: &mut String, name: &str, kind: &str, help: &str) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push(' ');
    out.push_str(help);
    out.push('\n');
    out.push_str("# TYPE ");
    out.push_str(name);
    out.push(' ');
    out.push_str(kind);
    out.push('\n');
}

fn line(out: &mut String, name: &str, _labels: &str, value: u64) {
    out.push_str(name);
    out.push(' ');
    out.push_str(&value.to_string());
    out.push('\n');
}

fn line_labels(out: &mut String, name: &str, labels: &str, value: u64) {
    out.push_str(name);
    out.push('{');
    out.push_str(labels);
    out.push_str("} ");
    out.push_str(&value.to_string());
    out.push('\n');
}

fn format_bucket(b: f64) -> String {
    // Avoid Rust's default "5" → "5" but "5.0" → "5"; Prometheus wants
    // the exact strings so successive scrapes can match buckets.
    if b.fract() == 0.0 {
        format!("{b}")
    } else {
        format!("{b}")
    }
}

fn escape_label(s: &str) -> String {
    s.replace('\\', r"\\").replace('"', r#"\""#)
}

pub type SharedMetrics = Arc<Metrics>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_increment() {
        let m = Metrics::default();
        m.observe_request("GET", "/foo", 200, 0.05);
        m.observe_request("GET", "/foo", 500, 1.5);
        m.observe_request("POST", "/bar", 429, 0.01);
        assert_eq!(m.requests_total.get(), 3);
        assert_eq!(m.responses_2xx.get(), 1);
        assert_eq!(m.responses_5xx.get(), 1);
        assert_eq!(m.responses_4xx.get(), 1);
        assert_eq!(m.rate_limited_total.get(), 1);
    }

    #[test]
    fn histogram_buckets_are_cumulative_after_render() {
        let m = Metrics::default();
        m.observe_request("GET", "/x", 200, 0.005);
        m.observe_request("GET", "/x", 200, 0.5);
        m.observe_request("GET", "/x", 200, 50.0);
        let g = RuntimeGauges {
            db_pool_size: 0,
            url_watches_active: 0,
            jobs_queued: 0,
            jobs_running: 0,
            rate_limit_buckets: 0,
        };
        let s = m.render(&g);
        assert!(s.contains("udc_request_duration_seconds_count 3"));
        assert!(s.contains("le=\"+Inf\"} 3"));
        // 50.0 lands in +Inf only; the 5.0 bucket should hold the 0.5
        // and 0.005 observations.
        assert!(s.contains("le=\"5\"} 2"));
    }

    #[test]
    fn per_path_map_capped() {
        let m = Metrics::default();
        for i in 0..1000 {
            m.observe_request("GET", &format!("/p{i}"), 200, 0.001);
        }
        // The cap is 256; the test only asserts we didn't exceed it.
        let map = m.per_path.lock().unwrap();
        assert!(map.len() <= 256);
    }
}
