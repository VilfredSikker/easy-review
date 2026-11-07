//! A TTL cache with a generation guard.
//!
//! `get_or_fetch_for` stores only when nothing bumped the cache's own
//! generation while the fetch ran, so an invalidation or a write made during a
//! lookup cannot be overwritten by the older answer.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

struct Entry<V> {
    inserted_at: Instant,
    expires_at: Instant,
    value: V,
}

pub struct TtlCache<K: Hash + Eq + Clone, V: Clone> {
    entries: Mutex<HashMap<K, Entry<V>>>,
    default_ttl: Duration,
    cap: Option<usize>,
    generation: AtomicU64,
}

impl<K: Hash + Eq + Clone, V: Clone> TtlCache<K, V> {
    pub fn new(default_ttl: Duration, cap: Option<usize>) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            default_ttl,
            cap,
            generation: AtomicU64::new(0),
        }
    }

    fn lock(&self) -> Option<MutexGuard<'_, HashMap<K, Entry<V>>>> {
        self.entries.lock().ok()
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let mut entries = self.lock()?;
        let entry = entries.get(key)?;
        if Instant::now() >= entry.expires_at {
            entries.remove(key);
            return None;
        }
        Some(entry.value.clone())
    }

    pub fn put(&self, key: K, value: V) {
        self.put_for(key, value, self.default_ttl);
    }

    pub fn put_for(&self, key: K, value: V, ttl: Duration) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let Some(mut entries) = self.lock() else {
            return;
        };
        self.evict_if_full(&mut entries, &key);
        let now = Instant::now();
        entries.insert(
            key,
            Entry {
                inserted_at: now,
                expires_at: now + ttl,
                value,
            },
        );
    }

    fn evict_if_full(&self, entries: &mut HashMap<K, Entry<V>>, incoming: &K) {
        let Some(cap) = self.cap else {
            return;
        };
        if entries.len() < cap || entries.contains_key(incoming) {
            return;
        }
        let now = Instant::now();
        let expired = entries
            .iter()
            .find(|(_, entry)| now >= entry.expires_at)
            .map(|(key, _)| key.clone());
        let victim = expired.or_else(|| {
            entries
                .iter()
                .min_by_key(|(_, entry)| entry.inserted_at)
                .map(|(key, _)| key.clone())
        });
        if let Some(victim) = victim {
            entries.remove(&victim);
        }
    }

    pub fn get_or_fetch<E>(&self, key: K, fetch: impl FnOnce() -> Result<V, E>) -> Result<V, E> {
        let default_ttl = self.default_ttl;
        self.get_or_fetch_for(key, |_| default_ttl, fetch)
    }

    pub fn get_or_fetch_for<E>(
        &self,
        key: K,
        ttl_for: impl FnOnce(&V) -> Duration,
        fetch: impl FnOnce() -> Result<V, E>,
    ) -> Result<V, E> {
        if let Some(value) = self.get(&key) {
            return Ok(value);
        }
        let generation = self.generation.load(Ordering::SeqCst);
        let value = fetch()?;
        let ttl = ttl_for(&value);
        let Some(mut entries) = self.lock() else {
            return Ok(value);
        };
        if self.generation.load(Ordering::SeqCst) != generation {
            return Ok(value);
        }
        self.evict_if_full(&mut entries, &key);
        let now = Instant::now();
        entries.insert(
            key,
            Entry {
                inserted_at: now,
                expires_at: now + ttl,
                value: value.clone(),
            },
        );
        Ok(value)
    }

    pub fn invalidate(&self, key: &K) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(mut entries) = self.lock() {
            entries.remove(key);
        }
    }

    pub fn clear(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(mut entries) = self.lock() {
            entries.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: &str) -> String {
        value.to_string()
    }

    #[test]
    fn ttl_cache_entry_can_expire_sooner_than_the_default() {
        let cache: TtlCache<String, u32> = TtlCache::new(Duration::from_secs(60), None);
        cache.put_for(key("short"), 1, Duration::ZERO);
        cache.put(key("long"), 2);
        assert_eq!(cache.get(&key("short")), None);
        assert_eq!(cache.get(&key("long")), Some(2));
    }

    #[test]
    fn ttl_cache_serves_fresh_entries_and_skips_failures() {
        let cache: TtlCache<String, u32> = TtlCache::new(Duration::from_secs(60), None);
        let mut calls = 0;
        assert_eq!(
            cache.get_or_fetch(key("k"), || {
                calls += 1;
                Err::<u32, ()>(())
            }),
            Err(())
        );
        assert_eq!(
            cache.get_or_fetch(key("k"), || {
                calls += 1;
                Ok::<_, ()>(7)
            }),
            Ok(7)
        );
        assert_eq!(
            cache.get_or_fetch(key("k"), || {
                calls += 1;
                Ok::<_, ()>(8)
            }),
            Ok(7)
        );
        assert_eq!(calls, 2, "a failed lookup is retried, a fresh hit is not");
        cache.put(key("k"), 9);
        assert_eq!(cache.get_or_fetch(key("k"), || Ok::<_, ()>(10)), Ok(9));
    }

    #[test]
    fn ttl_cache_refetches_after_expiry() {
        let cache: TtlCache<String, u32> = TtlCache::new(Duration::ZERO, None);
        cache.put(key("k"), 1);
        assert_eq!(cache.get_or_fetch(key("k"), || Ok::<_, ()>(2)), Ok(2));
    }

    #[test]
    fn a_capped_cache_evicts_an_expired_entry_before_the_oldest() {
        let cache: TtlCache<String, u32> = TtlCache::new(Duration::from_secs(60), Some(2));
        cache.put(key("expired"), 1);
        cache.put_for(key("oldest"), 2, Duration::from_secs(60));
        cache.put_for(key("expired"), 9, Duration::ZERO);
        cache.put(key("new"), 3);
        assert_eq!(cache.get(&key("expired")), None);
        assert_eq!(cache.get(&key("oldest")), Some(2));
        assert_eq!(cache.get(&key("new")), Some(3));
    }

    #[test]
    fn clear_empties_every_entry() {
        let cache: TtlCache<String, u32> = TtlCache::new(Duration::from_secs(60), None);
        cache.put(key("a"), 1);
        cache.put(key("b"), 2);
        cache.clear();
        assert_eq!(cache.get(&key("a")), None);
        assert_eq!(cache.get(&key("b")), None);
    }

    #[test]
    fn get_or_fetch_never_stores_an_error() {
        let cache: TtlCache<String, u32> = TtlCache::new(Duration::from_secs(60), None);
        assert!(cache
            .get_or_fetch(key("k"), || Err::<u32, &str>("no"))
            .is_err());
        assert_eq!(cache.get(&key("k")), None);
    }

    #[test]
    fn an_invalidated_fetch_does_not_store_the_stale_value() {
        let cache: TtlCache<String, u32> = TtlCache::new(Duration::from_secs(60), None);
        let result = cache.get_or_fetch(key("k"), || {
            cache.clear();
            Ok::<_, ()>(7)
        });
        assert_eq!(result, Ok(7));
        assert_eq!(cache.get(&key("k")), None);
    }

    #[test]
    fn a_delete_during_a_branch_lookup_is_not_overwritten() {
        let cache: TtlCache<String, bool> = TtlCache::new(Duration::from_secs(300), None);
        let exists = cache.get_or_fetch_for(
            key("b"),
            |_| Duration::from_secs(300),
            || {
                cache.put(key("b"), false);
                Ok::<_, ()>(true)
            },
        );
        assert_eq!(exists, Ok(true));
        assert_eq!(cache.get(&key("b")), Some(false));
    }
}
