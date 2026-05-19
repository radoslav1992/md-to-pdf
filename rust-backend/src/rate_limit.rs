//! Per-identity token-bucket rate limiter.
//!
//! Identity is determined per request, in this order:
//!   1. API key id (when `Authorization: Bearer …` is used)
//!   2. User id   (when the session cookie is used)
//!   3. Peer IP   (anonymous)
//!
//! Each identity gets a bucket sized by tier: free / anonymous = small,
//! premium = large, admin = very large. Buckets refill linearly. When a
//! caller is over the limit we return `429 Too Many Requests` with a
//! `Retry-After` header.
//!
//! The bucket map is in-process (a `Mutex<HashMap>`). Restarting the API
//! resets all buckets — fine for our scale; a SQLite-backed store would
//! be the next step if we ever ran multiple replicas.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Identity key for the bucket map. We keep these as small enum variants
/// so the HashMap key stays cheap.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Identity {
    ApiKey(i64),
    User(i64),
    Ip(String),
}

#[derive(Debug, Clone, Copy)]
pub struct Tier {
    /// Max burst — the bucket starts full at this many tokens.
    pub capacity: u32,
    /// Refill rate in tokens per second.
    pub refill_per_sec: f64,
}

impl Tier {
    pub const ANON: Tier = Tier {
        capacity: 30,
        refill_per_sec: 30.0 / 60.0, // 30/min
    };
    pub const FREE: Tier = Tier {
        capacity: 60,
        refill_per_sec: 60.0 / 60.0, // 60/min
    };
    pub const PREMIUM: Tier = Tier {
        capacity: 600,
        refill_per_sec: 600.0 / 60.0, // 600/min
    };
    pub const ADMIN: Tier = Tier {
        capacity: 6000,
        refill_per_sec: 6000.0 / 60.0,
    };

    pub fn for_role(role: &str) -> Tier {
        match role {
            "admin" => Tier::ADMIN,
            "premium" => Tier::PREMIUM,
            "free" => Tier::FREE,
            _ => Tier::ANON,
        }
    }
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
    tier: Tier,
}

impl Bucket {
    fn new(tier: Tier) -> Self {
        Bucket {
            tokens: tier.capacity as f64,
            last_refill: Instant::now(),
            tier,
        }
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.last_refill).as_secs_f64();
        if elapsed <= 0.0 {
            return;
        }
        self.tokens =
            (self.tokens + elapsed * self.tier.refill_per_sec).min(self.tier.capacity as f64);
        self.last_refill = now;
    }

    /// Try to consume one token. On success returns `Decision::Allow` with
    /// the remaining tokens. On failure returns `Decision::Limited` with
    /// how many seconds until the next token is available.
    fn try_consume(&mut self) -> Decision {
        let now = Instant::now();
        self.refill(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Decision::Allow {
                remaining: self.tokens.floor() as u32,
                capacity: self.tier.capacity,
            }
        } else {
            let need = 1.0 - self.tokens;
            let retry = (need / self.tier.refill_per_sec).ceil().max(1.0) as u64;
            Decision::Limited {
                retry_after_secs: retry,
                capacity: self.tier.capacity,
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Decision {
    Allow { remaining: u32, capacity: u32 },
    Limited { retry_after_secs: u64, capacity: u32 },
}

pub struct RateLimiter {
    buckets: Mutex<HashMap<Identity, Bucket>>,
    /// Soft cap on how many buckets we keep around. Older idle ones get
    /// evicted in bulk on the next insert past this threshold — cheap LRU
    /// approximation via a per-bucket `last_refill` timestamp.
    max_buckets: usize,
}

impl RateLimiter {
    pub fn new(max_buckets: usize) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            max_buckets,
        }
    }

    pub fn check(&self, identity: Identity, tier: Tier) -> Decision {
        let mut buckets = self.buckets.lock().expect("rate-limit mutex poisoned");
        if buckets.len() >= self.max_buckets && !buckets.contains_key(&identity) {
            self.evict_oldest(&mut buckets);
        }
        let bucket = buckets.entry(identity).or_insert_with(|| Bucket::new(tier));
        // Tier may have shifted (user upgraded mid-session) — apply it.
        bucket.tier = tier;
        bucket.try_consume()
    }

    fn evict_oldest(&self, buckets: &mut HashMap<Identity, Bucket>) {
        // Evict ~10% of buckets, oldest first. Cheap and avoids thrashing
        // when the map sits right at capacity.
        let target = self.max_buckets / 10;
        let mut victims: Vec<(Identity, Instant)> = buckets
            .iter()
            .map(|(k, b)| (k.clone(), b.last_refill))
            .collect();
        victims.sort_by_key(|(_, t)| *t);
        for (k, _) in victims.into_iter().take(target.max(1)) {
            buckets.remove(&k);
        }
    }

    pub fn stats(&self) -> RateLimiterStats {
        let buckets = self.buckets.lock().expect("rate-limit mutex poisoned");
        RateLimiterStats {
            buckets: buckets.len(),
            max_buckets: self.max_buckets,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RateLimiterStats {
    pub buckets: usize,
    pub max_buckets: usize,
}

pub type SharedRateLimiter = std::sync::Arc<RateLimiter>;

/// Bucket lifetime over which `Retry-After` measures, fed into the
/// `X-RateLimit-*` response headers. Hard-coded to the bucket window
/// because every tier currently refills over 60 seconds.
pub const WINDOW_SECS: u64 = 60;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_within_capacity() {
        let rl = RateLimiter::new(8);
        let id = Identity::User(1);
        for _ in 0..Tier::FREE.capacity {
            assert!(matches!(rl.check(id.clone(), Tier::FREE), Decision::Allow { .. }));
        }
    }

    #[test]
    fn blocks_once_drained() {
        let rl = RateLimiter::new(8);
        let id = Identity::Ip("1.2.3.4".into());
        for _ in 0..Tier::ANON.capacity {
            let _ = rl.check(id.clone(), Tier::ANON);
        }
        let d = rl.check(id, Tier::ANON);
        assert!(matches!(d, Decision::Limited { .. }));
    }

    #[test]
    fn admin_bucket_is_huge() {
        let rl = RateLimiter::new(8);
        let id = Identity::ApiKey(42);
        // Burst 1000 requests for admin — should all pass.
        for _ in 0..1000 {
            assert!(matches!(rl.check(id.clone(), Tier::ADMIN), Decision::Allow { .. }));
        }
    }

    #[test]
    fn evicts_when_full() {
        let rl = RateLimiter::new(10);
        for i in 0..20 {
            rl.check(Identity::User(i), Tier::FREE);
        }
        assert!(rl.stats().buckets <= 10);
    }
}
