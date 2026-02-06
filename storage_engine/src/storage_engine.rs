use std::collections::HashMap;

/// A in-memory key-value storage engine.
/// `StorageEngine` provides basic operations for storing and retrieving
/// string key-value pairs using a HashMap as the underlying storage mechanism.
/// # Examples
/// ```
/// use storage_engine::StorageEngine;
///
/// let mut engine = StorageEngine::new();
/// engine.set("key1".to_string(), "value1".to_string());
///
/// assert_eq!(engine.get("key1"), Some(&"value1".to_string()));
/// assert_eq!(engine.get("nonexistent"), None);
/// ```
#[derive(Debug, Clone)]
pub struct StorageEngine {
    store: HashMap<String, String>,
}

impl StorageEngine {
    /// Creates a new empty storage engine.
    /// # Examples
    /// ```
    /// use storage_engine::StorageEngine;
    ///
    /// let engine = StorageEngine::new();
    /// ```
    pub fn new() -> Self {
        Self {
            store: HashMap::new(),
        }
    }

    /// Inserts a key-value pair into the storage engine.
    /// If the key already exists, the old value is replaced and returned.
    /// # Arguments
    /// * `key` - The key to insert
    /// * `value` - The value to associate with the key
    /// # Examples
    /// ```
    /// use storage_engine::StorageEngine;
    ///
    /// let mut engine = StorageEngine::new();
    /// engine.set("key1".to_string(), "value1".to_string());
    /// ```
    pub fn set(&mut self, key: String, value: String) {
        self.store.insert(key, value);
    }

    /// Retrieves a reference to the value associated with the given key.
    /// Returns `None` if the key is not found in the storage engine.
    /// # Arguments
    /// * `key` - The key to look up
    /// # Returns
    /// An `Option<&String>` containing a reference to the value if found,
    /// or `None` if the key doesn't exist.
    /// # Examples
    /// ```
    /// use storage_engine::StorageEngine;
    ///
    /// let mut engine = StorageEngine::new();
    /// engine.set("key1".to_string(), "value1".to_string());
    ///
    /// assert_eq!(engine.get("key1"), Some(&"value1".to_string()));
    /// assert_eq!(engine.get("key2"), None);
    /// ```
    pub fn get(&self, key: &str) -> Option<&String> {
        self.store.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get() {
        let mut engine = StorageEngine::new();

        engine.set("key1".to_string(), "value1".to_string());
        assert_eq!(engine.get("key1"), Some(&"value1".to_string()));
    }

    #[test]
    fn get_nonexistent_key_return_none() {
        let engine = StorageEngine::new();
        assert_eq!(engine.get("nonexistent"), None);
    }

    #[test]
    fn set_overwrites_existing_key() {
        let mut engine = StorageEngine::new();

        engine.set("key1".to_string(), "value1".to_string());
        engine.set("key1".to_string(), "new_value".to_string());

        assert_eq!(engine.get("key1"), Some(&"new_value".to_string()));
    }

    #[test]
    fn multiple_keys_are_stored() {
        let mut engine = StorageEngine::new();

        engine.set("key1".to_string(), "value1".to_string());
        engine.set("key2".to_string(), "value2".to_string());
        engine.set("key3".to_string(), "value3".to_string());

        assert_eq!(engine.get("key1"), Some(&"value1".to_string()));
        assert_eq!(engine.get("key2"), Some(&"value2".to_string()));
        assert_eq!(engine.get("key3"), Some(&"value3".to_string()));
    }

    #[test]
    fn empty_strings_are_handled() {
        let mut engine = StorageEngine::new();

        engine.set("".to_string(), "".to_string());
        assert_eq!(engine.get(""), Some(&"".to_string()));

        engine.set("key".to_string(), "".to_string());
        assert_eq!(engine.get("key"), Some(&"".to_string()));

        engine.set("".to_string(), "value".to_string());
        assert_eq!(engine.get(""), Some(&"value".to_string()));
    }
}
