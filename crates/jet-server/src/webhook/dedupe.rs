//! Replay rejection for webhook deliveries.
//!
//! Providers redeliver. GitHub retries automatically when an endpoint is slow or
//! returns an error, operators click "Redeliver" while debugging, and a proxy in
//! front of the server can duplicate a request on its own. Without dedupe, each
//! of those starts a second identical run -- which at best wastes a build and at
//! worst deploys twice.
//!
//! Signature verification does not help here: a replayed delivery is a *valid*
//! delivery, byte for byte, with a valid signature. The delivery id is the only
//! thing that distinguishes the second copy from the first.
//!
//! # Bounded on purpose
//!
//! The set of seen ids is capped and time-limited. An unbounded set is a trivial
//! memory exhaustion vector on an internet-facing endpoint -- an attacker who can
//! reach the endpoint (even without a valid signature, if the check ran after
//! this one) would only need to send distinct ids. It is capped rather than
//! merely expired for the same reason: a burst can exceed any TTL.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a delivery id is remembered.
///
/// Comfortably longer than any provider's retry window (GitHub retries over
/// minutes, not hours) while keeping the set small.
pub const DEFAULT_TTL: Duration = Duration::from_secs(3600);

/// Hard cap on remembered ids. At the cap, the oldest are evicted.
pub const DEFAULT_CAPACITY: usize = 65_536;

pub struct DeliveryGuard {
    ttl: Duration,
    capacity: usize,
    seen: Mutex<HashMap<String, Instant>>,
}

/// Whether a delivery is new or a repeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Not seen before; proceed.
    Fresh,
    /// Already processed; acknowledge to the provider but do nothing.
    Duplicate,
    /// The provider sent no delivery id.
    ///
    /// Treated as fresh, because refusing would break GitLab event types that
    /// omit the header, but reported separately so it is visible that no replay
    /// protection was possible for this request.
    Unidentified,
}

impl Default for DeliveryGuard {
    fn default() -> Self {
        Self::new(DEFAULT_TTL, DEFAULT_CAPACITY)
    }
}

impl DeliveryGuard {
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        DeliveryGuard {
            ttl,
            capacity: capacity.max(1),
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// Record a delivery and report whether it is new.
    ///
    /// Insertion and the duplicate check are one atomic operation. Doing them
    /// separately would let two concurrent redeliveries both observe "not seen"
    /// and both start a run -- which is precisely the race a provider's parallel
    /// retry produces.
    pub fn check(&self, id: Option<&str>) -> Delivery {
        let Some(id) = id else {
            return Delivery::Unidentified;
        };
        // Scope the id so an absurd header cannot be stored verbatim.
        if id.is_empty() || id.len() > 200 {
            return Delivery::Unidentified;
        }

        let now = Instant::now();
        let mut seen = match self.seen.lock() {
            Ok(g) => g,
            // A poisoned mutex means a panic elsewhere. Failing closed here would
            // wedge every webhook; treating the delivery as fresh degrades to
            // "no dedupe" rather than "no CI".
            Err(e) => e.into_inner(),
        };

        // Drop expired entries opportunistically -- no background task needed, and
        // the work is proportional to the map only when it is large.
        if seen.len() > self.capacity / 2 {
            seen.retain(|_, at| now.duration_since(*at) < self.ttl);
        }

        if let Some(at) = seen.get(id)
            && now.duration_since(*at) < self.ttl
        {
            return Delivery::Duplicate;
        }
        // An expired entry falls through and has its timestamp refreshed.

        if seen.len() >= self.capacity {
            // Evict the oldest. O(n) but only at the cap, which a healthy
            // deployment never reaches.
            if let Some(oldest) = seen
                .iter()
                .min_by_key(|(_, at)| **at)
                .map(|(k, _)| k.clone())
            {
                seen.remove(&oldest);
            }
        }

        seen.insert(id.to_owned(), now);
        Delivery::Fresh
    }

    pub fn len(&self) -> usize {
        self.seen.lock().map(|s| s.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_delivery_is_fresh_and_the_second_is_a_duplicate() {
        let g = DeliveryGuard::default();
        assert_eq!(g.check(Some("delivery-1")), Delivery::Fresh);
        assert_eq!(g.check(Some("delivery-1")), Delivery::Duplicate);
        assert_eq!(g.check(Some("delivery-1")), Delivery::Duplicate);
    }

    #[test]
    fn distinct_deliveries_are_independent() {
        let g = DeliveryGuard::default();
        assert_eq!(g.check(Some("a")), Delivery::Fresh);
        assert_eq!(g.check(Some("b")), Delivery::Fresh);
        assert_eq!(g.check(Some("a")), Delivery::Duplicate);
    }

    #[test]
    fn missing_or_absurd_ids_are_unidentified() {
        let g = DeliveryGuard::default();
        assert_eq!(g.check(None), Delivery::Unidentified);
        assert_eq!(g.check(Some("")), Delivery::Unidentified);
        assert_eq!(g.check(Some(&"x".repeat(201))), Delivery::Unidentified);
        // And none of those were stored.
        assert!(g.is_empty());
    }

    #[test]
    fn unidentified_deliveries_never_dedupe_each_other() {
        // GitLab omits the header on some events; those must still be processed
        // rather than being collapsed into one.
        let g = DeliveryGuard::default();
        assert_eq!(g.check(None), Delivery::Unidentified);
        assert_eq!(g.check(None), Delivery::Unidentified);
    }

    #[test]
    fn entries_expire() {
        let g = DeliveryGuard::new(Duration::from_millis(30), 128);
        assert_eq!(g.check(Some("x")), Delivery::Fresh);
        assert_eq!(g.check(Some("x")), Delivery::Duplicate);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            g.check(Some("x")),
            Delivery::Fresh,
            "an id older than the TTL should no longer be remembered"
        );
    }

    #[test]
    fn capacity_is_enforced() {
        // The memory-exhaustion guard. Distinct ids must not grow the map without
        // bound on an internet-facing endpoint.
        let cap = 64;
        let g = DeliveryGuard::new(Duration::from_secs(3600), cap);
        for i in 0..cap * 10 {
            g.check(Some(&format!("id-{i}")));
        }
        assert!(
            g.len() <= cap,
            "guard grew to {} entries with a cap of {cap}",
            g.len()
        );
    }

    #[test]
    fn recent_ids_survive_eviction_pressure() {
        // Eviction must drop the oldest, so the ids most likely to be retried
        // right now are the ones still remembered.
        let g = DeliveryGuard::new(Duration::from_secs(3600), 8);
        for i in 0..8 {
            g.check(Some(&format!("old-{i}")));
        }
        let recent = "just-now";
        assert_eq!(g.check(Some(recent)), Delivery::Fresh);
        assert_eq!(
            g.check(Some(recent)),
            Delivery::Duplicate,
            "the newest id must not be the one evicted"
        );
    }

    #[test]
    fn concurrent_redeliveries_yield_exactly_one_fresh() {
        // The race a provider's parallel retry produces: check and insert must be
        // atomic, or two threads both see "not seen" and two runs start.
        use std::sync::Arc;
        let g = Arc::new(DeliveryGuard::default());
        let mut handles = Vec::new();
        for _ in 0..16 {
            let g = Arc::clone(&g);
            handles.push(std::thread::spawn(move || {
                matches!(g.check(Some("same-delivery")), Delivery::Fresh)
            }));
        }
        let fresh = handles
            .into_iter()
            .filter(|h| h.is_finished() || true)
            .map(|h| h.join().unwrap())
            .filter(|f| *f)
            .count();
        assert_eq!(fresh, 1, "exactly one caller may proceed");
    }
}
