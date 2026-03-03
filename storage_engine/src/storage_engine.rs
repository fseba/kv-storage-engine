use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufReader, BufWriter, Result, Write},
    path::{Path, PathBuf},
};

use serde_json::{Value, json};

const MAX_ENTRIES: usize = 2000;
const MANIFEST: &str = "MANIFEST";

/// An in-memory key-value storage engine backed by SST files on disk.
/// `StorageEngine` provides basic operations for storing and retrieving
/// string key-value pairs. Writes go to an in-memory `HashMap` (memtable)
/// that is flushed to a sorted SST file on disk once it reaches [`MAX_ENTRIES`].
/// # Examples
/// ```
/// use storage_engine::StorageEngine;
///
/// let mut engine = StorageEngine::new("./").unwrap();
/// engine.set("key1".to_string(), "value1".to_string()).unwrap();
///
/// assert_eq!(engine.get("key1"), Some(&"value1".to_string()));
/// assert_eq!(engine.get("nonexistent"), None);
/// ```
#[derive(Debug)]
pub struct StorageEngine {
    memtable: HashMap<String, String>,
    directory_path: PathBuf,
    sst_file_counter: usize,
}

impl StorageEngine {
    /// Creates a new storage engine at the given directory path.
    /// Reads the [`MANIFEST`] file to recover the SST file counter. If no
    /// manifest exists, an empty one is created.
    /// # Arguments
    /// * `path` - Directory where SST and manifest files are stored
    /// # Examples
    /// ```
    /// use storage_engine::StorageEngine;
    ///
    /// let engine = StorageEngine::new("./").unwrap();
    /// ```
    /// # Errors
    /// Returns an `io::Error` if the manifest file cannot be read or created.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let mut engine = Self {
            memtable: HashMap::with_capacity(MAX_ENTRIES),
            directory_path: path.as_ref().to_path_buf(),
            sst_file_counter: 0,
        };

        let manifest_path = engine.directory_path.join(MANIFEST);
        if manifest_path.exists() {
            let content = fs::read_to_string(manifest_path)?;
            if let Some(latest_file) = content.lines().last()
                && let Some(counter) = parse_sst_filename(latest_file)
            {
                engine.sst_file_counter = counter;
            }
        } else {
            File::create(manifest_path)?;
        }
        Ok(engine)
    }

    /// Inserts a key-value pair into the storage engine.
    /// If the key already exists, the old value is replaced.
    /// When the memtable reaches [`MAX_ENTRIES`], it is automatically flushed to disk.
    /// # Arguments
    /// * `key` - The key to insert
    /// * `value` - The value to associate with the key
    /// # Examples
    /// ```
    /// use storage_engine::StorageEngine;
    ///
    /// let mut engine = StorageEngine::new("./").unwrap();
    /// engine.set("key1".to_string(), "value1".to_string()).unwrap();
    /// ```
    /// # Errors
    /// Returns an `io::Error` if there is an issue flushing the memtable to disk when the number
    /// of entries exceeds the threshold.
    pub fn set(&mut self, key: String, value: String) -> Result<()> {
        self.memtable.insert(key, value);
        if self.memtable.len() >= MAX_ENTRIES {
            self.flush()?;
        }
        Ok(())
    }

    /// Retrieves the value associated with the given key.
    /// Checks the in-memory memtable first. If not found, performs a linear scan
    /// across SST files listed in the MANIFEST from newest to oldest.
    /// Returns `None` if the key is not found in either the memtable or any SST file.
    /// # Arguments
    /// * `key` - The key to look up
    /// # Returns
    /// An `Option<String>` containing the value if found, or `None` if the key doesn't exist.
    /// # Examples
    /// ```
    /// use storage_engine::StorageEngine;
    ///
    /// let mut engine = StorageEngine::new("./").unwrap();
    /// engine.set("key1".to_string(), "value1".to_string()).unwrap();
    ///
    /// assert_eq!(engine.get("key1"), Some("value1".to_string()));
    /// assert_eq!(engine.get("key2"), None);
    /// ```
    pub fn get(&self, key: &str) -> Option<String> {
        if let Some(value) = self.memtable.get(key) {
            return Some(value.clone());
        }

        for n in (1..=self.sst_file_counter).rev() {
            let sst_file = format!("sst-{n}.json");
            if let Ok(file) = File::open(self.directory_path.join(sst_file)) {
                let value = serde_json::from_reader::<_, Vec<Value>>(BufReader::new(file))
                    .ok()?
                    .iter()
                    .filter_map(|entry| entry.as_object())
                    .find_map(|obj| obj.get(key).and_then(|v| v.as_str().map(|s| s.to_string())));
                if value.is_some() {
                    return value;
                }
            }
        }
        None
    }

    /// Flushes the in-memory key-value pairs to disk as a sorted JSON array.
    /// The key-value pairs are written to a file named `sst-{N}.json` (where N is an
    /// incrementing counter) in the configured directory.
    /// After a successful flush, the memtable is cleared and the counter is incremented.
    /// # Errors
    /// Returns an `io::Error` if there is an issue creating or writing to the file.
    fn flush(&mut self) -> Result<()> {
        let tmp_count = self.sst_file_counter + 1;
        let file_name = format!("sst-{tmp_count}.json");
        // INFO: Truncates file
        let file = File::create(self.directory_path.join(file_name))?;
        let json_flush = self.sort_memtable();
        serde_json::to_writer(BufWriter::new(file), &json_flush)?;
        // TODO: how to add to file and not repalce?
        let mut manifest = File::options()
            .create(true)
            .append(true)
            .open(self.directory_path.join(MANIFEST))?;
        writeln!(manifest, "sst-{}.json", tmp_count)?;
        self.sst_file_counter += 1;
        self.memtable.clear();
        Ok(())
    }

    fn sort_memtable(&mut self) -> Value {
        let mut entries: Vec<(&String, &String)> = self.memtable.iter().collect();
        entries.sort_by_key(|(k, _)| *k);
        let json_flush: Value = entries
            .iter()
            .map(|(k, v)| {
                let key = k.to_string();
                let value = v.to_string();
                json!({key: value})
            })
            .collect();
        json_flush
    }
}

fn parse_sst_filename(filename: &str) -> Option<usize> {
    filename
        .strip_prefix("sst-")?
        .strip_suffix(".json")?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn set_and_get() {
        let mut engine = StorageEngine::new("./").unwrap();

        engine
            .set("key1".to_string(), "value1".to_string())
            .unwrap();
    }

    #[test]
    fn get_nonexistent_key_return_none() {
        let engine = StorageEngine::new("./").unwrap();
        assert_eq!(engine.get("nonexistent"), None);
    }

    #[test]
    fn set_overwrites_existing_key() {
        let mut engine = StorageEngine::new("./").unwrap();

        engine
            .set("key1".to_string(), "value1".to_string())
            .unwrap();
        engine
            .set("key1".to_string(), "new_value".to_string())
            .unwrap();

        assert_eq!(engine.get("key1"), Some("new_value".to_string()));
    }

    #[test]
    fn multiple_keys_are_stored() {
        let mut engine = StorageEngine::new("./").unwrap();

        engine
            .set("key1".to_string(), "value1".to_string())
            .unwrap();
        engine
            .set("key2".to_string(), "value2".to_string())
            .unwrap();
        engine
            .set("key3".to_string(), "value3".to_string())
            .unwrap();

        assert_eq!(engine.get("key1"), Some("value1".to_string()));
        assert_eq!(engine.get("key2"), Some("value2".to_string()));
        assert_eq!(engine.get("key3"), Some("value3".to_string()));
    }

    #[test]
    fn empty_strings_are_handled() {
        let mut engine = StorageEngine::new("./").unwrap();

        engine.set("".to_string(), "".to_string()).unwrap();
        assert_eq!(engine.get(""), Some("".to_string()));

        engine.set("key".to_string(), "".to_string()).unwrap();
        assert_eq!(engine.get("key"), Some("".to_string()));

        engine.set("".to_string(), "value".to_string()).unwrap();
        assert_eq!(engine.get(""), Some("value".to_string()));
    }

    #[test]
    fn get_finds_values_in_sstable_files() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine {
            memtable: HashMap::new(),
            directory_path: dir.path().to_path_buf(),
            sst_file_counter: 0,
        };

        engine
            .set("key1".to_string(), "value1".to_string())
            .unwrap();
        engine.flush().unwrap();

        assert_eq!(engine.get("key1"), Some("value1".to_string()));
    }

    #[test]
    fn get_returns_none_if_value_not_found_in_sstabe_file() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine {
            memtable: HashMap::new(),
            directory_path: dir.path().to_path_buf(),
            sst_file_counter: 0,
        };

        engine
            .set("key1".to_string(), "value1".to_string())
            .unwrap();
        engine.flush().unwrap();

        assert_eq!(engine.get("nonexistent"), None);
    }

    #[test]
    fn two_thousand_or_more_entries_trigger_a_flush() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine {
            memtable: (0..1999u32)
                .map(|i| (format!("key_{i}"), format!("value_{i}")))
                .collect(),
            directory_path: dir.path().to_path_buf(),
            sst_file_counter: 0,
        };

        engine
            .set("key_2000".to_string(), "value_2000".to_string())
            .unwrap();

        assert!(
            fs::exists(dir.path().join("sst-1.json")).unwrap(),
            "File does not exist"
        );
    }

    #[test]
    fn flush_creates_a_sorted_json_array_sst_file() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine {
            memtable: HashMap::from([
                ("a".to_string(), "v_1".to_string()),
                ("b".to_string(), "v_2".to_string()),
                ("c".to_string(), "v_3".to_string()),
            ]),
            directory_path: dir.path().to_path_buf(),
            sst_file_counter: 0,
        };

        engine.flush().unwrap();
        let content = fs::read_to_string(dir.path().join("sst-1.json")).unwrap();
        assert_eq!(content, "[{\"a\":\"v_1\"},{\"b\":\"v_2\"},{\"c\":\"v_3\"}]");
    }

    #[test]
    fn memtable_is_cleared_after_successful_flush() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine {
            memtable: HashMap::from([
                ("a".to_string(), "v_1".to_string()),
                ("b".to_string(), "v_2".to_string()),
                ("c".to_string(), "v_3".to_string()),
            ]),
            directory_path: dir.path().to_path_buf(),
            sst_file_counter: 0,
        };

        engine.flush().unwrap();

        assert!(
            fs::exists(dir.path().join("sst-1.json")).unwrap(),
            "File does not exist"
        );
        assert!(
            engine.memtable.is_empty(),
            "Memtable should be empty after flush"
        );
    }

    #[test]
    fn memtable_is_not_cleared_after_failed_flush() {
        let mut engine = StorageEngine {
            memtable: HashMap::from([
                ("a".to_string(), "v_1".to_string()),
                ("b".to_string(), "v_2".to_string()),
                ("c".to_string(), "v_3".to_string()),
            ]),
            directory_path: PathBuf::from("/non/existent/directory"),
            sst_file_counter: 0,
        };

        let result = engine.flush();

        assert!(
            result.is_err(),
            "Flush should fail due to invalid directory"
        );
        assert!(
            !engine.memtable.is_empty(),
            "Memtable should be empty after flush"
        );
    }

    #[test]
    fn flush_should_increase_counter_on_success() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine {
            memtable: HashMap::from([
                ("a".to_string(), "v_1".to_string()),
                ("b".to_string(), "v_2".to_string()),
                ("c".to_string(), "v_3".to_string()),
            ]),
            directory_path: dir.path().to_path_buf(),
            sst_file_counter: 0,
        };

        engine.flush().unwrap();
        engine.flush().unwrap();
        engine.flush().unwrap();
        assert!(fs::exists(dir.path().join("sst-1.json")).unwrap());
        assert!(fs::exists(dir.path().join("sst-2.json")).unwrap());
        assert!(fs::exists(dir.path().join("sst-3.json")).unwrap());
    }

    #[test]
    fn failed_flush_should_not_increase_counter() {
        let mut engine = StorageEngine {
            memtable: HashMap::from([
                ("a".to_string(), "v_1".to_string()),
                ("b".to_string(), "v_2".to_string()),
                ("c".to_string(), "v_3".to_string()),
            ]),
            directory_path: PathBuf::from("/non/existent/directory"),
            sst_file_counter: 0,
        };

        let result = engine.flush();

        assert!(
            result.is_err(),
            "Flush should fail due to invalid directory"
        );
        assert_eq!(engine.sst_file_counter, 0, "Counter should still be zero");
    }

    #[test]
    fn new_should_set_counter_from_latest_sst_table_file_name_in_manifest() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(MANIFEST), "sst-1.json").unwrap();

        let engine = StorageEngine::new(dir.path()).unwrap();

        assert_eq!(
            engine.sst_file_counter, 1,
            "File counter not correctly parsed from manifest"
        );
    }

    #[test]
    fn new_should_create_manifest_file() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join(MANIFEST);

        assert!(
            !manifest_path.exists(),
            "Manifest file should not exist before initialization"
        );

        let _engine = StorageEngine::new(dir.path()).unwrap();

        assert!(
            manifest_path.exists(),
            "Manifest file should be created during initialization"
        );
    }

    #[test]
    fn flush_should_write_sst_table_file_name_to_manifest() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine::new(dir.path()).unwrap();

        engine.flush().unwrap();
        engine.flush().unwrap();

        let manifest_content = fs::read_to_string(dir.path().join(MANIFEST)).unwrap();
        assert_eq!(
            manifest_content, "sst-1.json\nsst-2.json\n",
            "Manifest should contain the latest SST file name"
        );
    }
}
