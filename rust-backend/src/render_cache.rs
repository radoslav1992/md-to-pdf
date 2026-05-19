//! In-memory LRU-ish cache for expensive conversions (PDF / DOCX / EPUB /
//! ODT / PNG / JPG / Markdown — anything that spawns a subprocess).
//!
//! The cache is keyed by a SHA-256 of every input that affects the output
//! (input type, raw content, theme, custom CSS, PDF options, enrichments,
//! target format, *and* the calling user — image inlining is user-scoped,
//! so cross-user reuse would be unsound).
//!
//! Eviction is "kick the oldest until we fit" with a coarse `last_used`
//! timestamp. Good enough — the worst case is a handful of extra cache
//! misses under churn, never a correctness issue.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Default cap on total bytes stored. Chosen so the API container's
/// resident set doesn't balloon on a Hetzner CX23 (2 GB RAM): 128 MiB is
/// roughly 400–800 cached PDFs.
pub const DEFAULT_MAX_BYTES: usize = 128 * 1024 * 1024;

/// Items larger than this fraction of the cache are never inserted —
/// otherwise a single huge render would evict the entire warm set.
const MAX_ITEM_FRACTION: usize = 4;

/// One cached output.
#[derive(Clone)]
pub struct CachedRender {
    pub bytes: Vec<u8>,
    pub content_type: String,
    /// Mirror of the input_type that produced this render. Lets the API
    /// return the same value on cache hits without recomputing.
    pub input_type: String,
}

struct Entry {
    render: CachedRender,
    last_used: Instant,
    size: usize,
}

pub struct RenderCache {
    entries: HashMap<String, Entry>,
    total_bytes: usize,
    max_bytes: usize,
    // Diagnostic counters. Cheap to read; useful for sanity-checking
    // hit rate from /admin or a future /metrics endpoint.
    hits: u64,
    misses: u64,
}

impl RenderCache {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            total_bytes: 0,
            max_bytes,
            hits: 0,
            misses: 0,
        }
    }

    pub fn get(&mut self, key: &str) -> Option<CachedRender> {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_used = Instant::now();
            self.hits += 1;
            Some(entry.render.clone())
        } else {
            self.misses += 1;
            None
        }
    }

    pub fn insert(&mut self, key: String, render: CachedRender) {
        let size = render.bytes.len() + render.content_type.len() + key.len();
        // Don't pollute the cache with a single oversized object.
        if size > self.max_bytes / MAX_ITEM_FRACTION {
            return;
        }
        while self.total_bytes + size > self.max_bytes && !self.entries.is_empty() {
            let victim = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone());
            if let Some(k) = victim {
                if let Some(old) = self.entries.remove(&k) {
                    self.total_bytes = self.total_bytes.saturating_sub(old.size);
                }
            } else {
                break;
            }
        }
        self.entries.insert(
            key,
            Entry {
                render,
                last_used: Instant::now(),
                size,
            },
        );
        self.total_bytes += size;
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.entries.len(),
            total_bytes: self.total_bytes,
            max_bytes: self.max_bytes,
            hits: self.hits,
            misses: self.misses,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CacheStats {
    pub entries: usize,
    pub total_bytes: usize,
    pub max_bytes: usize,
    pub hits: u64,
    pub misses: u64,
}

/// Shared handle the rest of the app holds. A `Mutex<RenderCache>` is
/// fine because the critical section is just a `HashMap` lookup plus a
/// timestamp update — well under the cost of the work we're caching.
pub type SharedRenderCache = std::sync::Arc<Mutex<RenderCache>>;

/// Build the cache key from every input that can change the output.
/// Order matters — feed fields in deterministically.
#[allow(clippy::too_many_arguments)]
pub fn key(
    user_id: Option<i64>,
    input_type: &str,
    output: &str,
    content: &str,
    theme: &str,
    custom_css: Option<&str>,
    pdf_options_json: Option<&str>,
    enrichments_json: Option<&str>,
    title: &str,
    template_id: Option<i64>,
) -> String {
    let mut h = Sha256::new();
    h.update(b"udc-cache-v1\0");
    h.update(user_id.unwrap_or(0).to_le_bytes());
    h.update(input_type.as_bytes());
    h.update(b"\0");
    h.update(output.as_bytes());
    h.update(b"\0");
    h.update(theme.as_bytes());
    h.update(b"\0");
    h.update(custom_css.unwrap_or("").as_bytes());
    h.update(b"\0");
    h.update(pdf_options_json.unwrap_or("").as_bytes());
    h.update(b"\0");
    h.update(enrichments_json.unwrap_or("").as_bytes());
    h.update(b"\0");
    h.update(title.as_bytes());
    h.update(b"\0");
    h.update(template_id.unwrap_or(0).to_le_bytes());
    h.update(b"\0");
    h.update(content.as_bytes());
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(b: usize) -> CachedRender {
        CachedRender {
            bytes: vec![0u8; b],
            content_type: "application/pdf".into(),
            input_type: "markdown".into(),
        }
    }

    #[test]
    fn round_trip() {
        let mut cache = RenderCache::new(1024);
        cache.insert("a".into(), make(10));
        assert!(cache.get("a").is_some());
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn evicts_oldest_when_full() {
        // Cap at 1 KiB so each 200-byte entry (plus small overhead) fits
        // and stays well under the MAX_ITEM_FRACTION threshold.
        let mut cache = RenderCache::new(1024);
        cache.insert("a".into(), make(200));
        std::thread::sleep(std::time::Duration::from_millis(2));
        cache.insert("b".into(), make(200));
        // Touch 'b' so 'a' is older.
        let _ = cache.get("b");
        std::thread::sleep(std::time::Duration::from_millis(2));
        cache.insert("c".into(), make(200));
        std::thread::sleep(std::time::Duration::from_millis(2));
        cache.insert("d".into(), make(200));
        // Cumulative size ≈ 800+ bytes — pushing past the 1 KiB cap on
        // the next insert is what forces eviction of the oldest.
        cache.insert("e".into(), make(200));
        assert!(cache.get("a").is_none(), "a should have been evicted");
    }

    #[test]
    fn rejects_oversized_items() {
        let mut cache = RenderCache::new(100);
        // 80 bytes plus overhead > max_bytes/4 = 25 — must be rejected.
        cache.insert("huge".into(), make(80));
        assert!(cache.get("huge").is_none());
    }

    #[test]
    fn key_changes_with_inputs() {
        let a = key(Some(1), "markdown", "pdf", "x", "default", None, None, None, "t", None);
        let b = key(Some(1), "markdown", "pdf", "y", "default", None, None, None, "t", None);
        let c = key(Some(2), "markdown", "pdf", "x", "default", None, None, None, "t", None);
        assert_ne!(a, b, "content change should change key");
        assert_ne!(a, c, "user change should change key");
    }
}
