use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const DEFAULT_CAPACITY: usize = 1000;
pub const DEFAULT_TTL: Duration = Duration::from_secs(1800);

// 24-char blake3 key for content-addressable caching.
pub fn compute_key(payload: &[u8]) -> String {
    let h = blake3::hash(payload);
    let hex = h.to_hex();
    hex.as_str()[..24].to_string()
}

pub struct InMemoryCcrStore {
    map: Mutex<HashMap<String, Entry>>,
    ttl: Duration,
    capacity: usize,
}

#[derive(Clone)]
struct Entry {
    payload: String,
    version: u8,
    inserted: Instant,
}

impl InMemoryCcrStore {
    pub fn new() -> Self {
        Self::with_capacity_and_ttl(DEFAULT_CAPACITY, DEFAULT_TTL)
    }

    pub fn with_capacity_and_ttl(capacity: usize, ttl: Duration) -> Self {
        Self {
            map: Mutex::new(HashMap::with_capacity(capacity)),
            ttl,
            capacity,
        }
    }

    fn evict_until_under_capacity(&self) {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        while map.len() >= self.capacity {
            let oldest_key = map
                .iter()
                .min_by_key(|(_, e)| e.inserted)
                .map(|(k, _)| k.clone());
            if let Some(key) = oldest_key {
                map.remove(&key);
            } else {
                break;
            }
        }
    }

    pub fn put(&self, hash: &str, payload: &str) {
        self.put_with_version(hash, payload, 0);
    }

    pub fn put_with_version(&self, hash: &str, payload: &str, schema_version: u8) {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = map.get_mut(hash) {
            entry.payload = payload.to_string();
            entry.version = schema_version;
            entry.inserted = Instant::now();
            return;
        }
        if map.len() >= self.capacity {
            drop(map);
            self.evict_until_under_capacity();
            map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        }
        let entry = Entry {
            payload: payload.to_string(),
            version: schema_version,
            inserted: Instant::now(),
        };
        map.insert(hash.to_string(), entry);
    }

    pub fn get(&self, hash: &str) -> Option<String> {
        self.get_with_version(hash).map(|(s, _)| s)
    }

    pub fn get_with_version(&self, hash: &str) -> Option<(String, u8)> {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(hash) {
            Some(entry) if entry.inserted.elapsed() <= self.ttl => {
                Some((entry.payload.clone(), entry.version))
            }
            Some(entry) if entry.inserted.elapsed() > self.ttl => {
                map.remove(hash);
                None
            }
            _ => None,
        }
    }

    pub fn len(&self) -> usize {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for InMemoryCcrStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_then_get_returns_payload() {
        let store = InMemoryCcrStore::new();
        store.put("abc123", r#"[{"id":1}]"#);
        assert_eq!(store.get("abc123"), Some(r#"[{"id":1}]"#.to_string()));
    }

    #[test]
    fn missing_hash_returns_none() {
        let store = InMemoryCcrStore::new();
        assert_eq!(store.get("never_stored"), None);
    }

    #[test]
    fn put_overwrites_under_same_hash() {
        let store = InMemoryCcrStore::new();
        store.put("h", "first");
        store.put("h", "second");
        assert_eq!(store.get("h"), Some("second".to_string()));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn capacity_evicts_oldest() {
        let store = InMemoryCcrStore::with_capacity_and_ttl(2, DEFAULT_TTL);
        store.put("a", "1");
        store.put("b", "2");
        store.put("c", "3");
        assert_eq!(store.len(), 2);
        assert_eq!(store.get("a"), None);
        assert_eq!(store.get("b"), Some("2".to_string()));
        assert_eq!(store.get("c"), Some("3".to_string()));
    }

    #[test]
    fn expired_entries_are_dropped_on_get() {
        let store = InMemoryCcrStore::with_capacity_and_ttl(10, Duration::from_millis(10));
        store.put("a", "1");
        std::thread::sleep(Duration::from_millis(25));
        assert_eq!(store.get("a"), None);
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn store_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<InMemoryCcrStore>();
    }

    #[test]
    fn basic_put_get_is_empty() {
        let store = InMemoryCcrStore::new();
        store.put("h", "v");
        assert_eq!(store.get("h"), Some("v".to_string()));
        assert!(!store.is_empty());
    }
}
