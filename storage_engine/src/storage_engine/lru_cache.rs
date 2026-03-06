use linked_hash_map::LinkedHashMap;

/// A fixed-capacity LRU cache for tracking set membership.
///
/// Internally backed by a [`LinkedHashMap`] which provides O(1) insert,
/// lookup, and removal while maintaining insertion/access order for eviction.
/// When the cache is full, the least recently used entry is evicted on insert.
#[derive(Debug)]
pub struct LRUCache {
    capacity: usize,
    map: LinkedHashMap<String, ()>,
}

impl LRUCache {
    /// Creates a new `LRUCache` with the given capacity.
    ///
    /// # Panics
    /// Panics if `capacity` is 0.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "Capacity must be greater than 0");
        LRUCache {
            capacity,
            map: LinkedHashMap::new(),
        }
    }

    /// Returns `true` if the key is in the cache, promoting it to most-recently-used.
    pub fn contains(&mut self, key: &str) -> bool {
        self.map.get_refresh(key).is_some()
    }

    /// Inserts a key. If the cache is at capacity, the least recently used entry is evicted.
    pub fn insert(&mut self, key: String) {
        self.map.insert(key, ());
        if self.map.len() > self.capacity {
            self.map.pop_front();
        }
    }

    /// Removes a key from the cache. No-op if the key is not present.
    pub fn remove(&mut self, key: &str) {
        self.map.remove(key);
    }
}
