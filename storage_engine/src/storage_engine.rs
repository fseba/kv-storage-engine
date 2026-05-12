use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet},
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Result, Write},
    ops::Not,
    path::{Path, PathBuf},
};

mod lru_cache;

use serde_json::{Value, json};

use lru_cache::LRUCache;

const MAX_ENTRIES: usize = 2000;
const MANIFEST: &str = "MANIFEST";
const MANIFEST_TEMP: &str = "MANIFEST.tmp";
const WAL: &str = "wal.db";
const COMPACTION_TRIGGER_COUNT: usize = 10000;

/// An in-memory key-value storage engine backed by SST files on disk.
/// `StorageEngine` provides basic operations for storing and retrieving
/// string key-value pairs. Each write is first appended to a write-ahead log
/// (WAL) for durability, then inserted into an in-memory `HashMap` (memtable).
/// When the memtable reaches [`MAX_ENTRIES`], it is flushed to a sorted SST
/// file on disk and the WAL is cleared. On startup, any unflushed WAL entries
/// are replayed into the memtable to recover writes from the last session.
/// # Examples
/// ```
/// use storage_engine::StorageEngine;
///
/// let mut engine = StorageEngine::new("./").unwrap();
/// engine.set("key1".to_string(), "value1".to_string()).unwrap();
///
/// assert_eq!(engine.get("key1"), Some("value1".to_string()));
/// assert_eq!(engine.get("nonexistent"), None);
/// ```
#[derive(Debug)]
pub struct StorageEngine {
    memtable: HashMap<String, MemtableEntry>,
    directory_path: PathBuf,
    sst_file_counter: usize,
    negative_cache: LRUCache,
    update_counter: usize,
}

#[derive(Debug)]
enum MemtableEntry {
    Value(String),
    Deleted,
}

impl StorageEngine {
    /// Creates a new storage engine at the given directory path.
    /// Reads the [`MANIFEST`] file to recover the SST file counter. If the WAL
    /// exists, its entries are replayed into the memtable to recover any writes
    /// from the previous session that were not yet flushed. If either file does
    /// not exist, an empty one is created.
    /// # Arguments
    /// * `path` - Directory where SST, manifest, and WAL files are stored
    /// # Examples
    /// ```
    /// use storage_engine::StorageEngine;
    ///
    /// let engine = StorageEngine::new("./").unwrap();
    /// ```
    /// # Errors
    /// Returns an `io::Error` if any file cannot be read, created, or synced.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let mut engine = Self {
            memtable: HashMap::with_capacity(MAX_ENTRIES),
            directory_path: path.as_ref().to_path_buf(),
            sst_file_counter: 0,
            negative_cache: LRUCache::new(100),
            update_counter: 0,
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

        let wal_path = engine.directory_path.join(WAL);
        if wal_path.exists() {
            let wal = fs::read_to_string(wal_path)?;
            for wal_entry in wal.lines() {
                if let Ok(record) = serde_json::from_str::<WALRecord>(wal_entry) {
                    match record.op {
                        WALRecordType::Put(v) => {
                            engine.memtable.insert(record.key, MemtableEntry::Value(v))
                        }
                        WALRecordType::Delete => {
                            engine.memtable.insert(record.key, MemtableEntry::Deleted)
                        }
                    };
                }
                if engine.memtable.len() >= MAX_ENTRIES {
                    let flush_content = engine.create_flush_content();
                    engine.flush(flush_content)?;
                }
            }
        } else {
            let wal = File::create(wal_path)?;
            wal.sync_data()?;
            engine.sync_parent_dir()?;
        }
        Ok(engine)
    }

    /// Syncs the storage directory to disk, making any recently created directory
    /// entries durable. Call this after creating a new file in the storage directory.
    /// # Errors
    /// Returns an `io::Error` if there is an issue opening or syncing the directory.
    fn sync_parent_dir(&self) -> Result<()> {
        let dir = File::open(self.directory_path.as_path())?;
        dir.sync_all()
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
    /// Returns an `io::Error` if the WAL cannot be written or synced, or if
    /// flushing the memtable to disk fails when the entry threshold is reached.
    pub fn set(&mut self, key: String, value: String) -> Result<()> {
        let wal_record = WALRecord {
            op: WALRecordType::Put(value.clone()),
            key: key.clone(),
        };
        let mut wal = File::options()
            .append(true)
            .open(self.directory_path.join(WAL))?;
        writeln!(wal, "{}", json!(&wal_record))?;
        wal.sync_data()?;

        self.negative_cache.remove(&key);
        self.memtable.insert(key, MemtableEntry::Value(value));
        if self.memtable.len() >= MAX_ENTRIES {
            let flush_content = self.create_flush_content();
            self.flush(flush_content)?;
        }
        self.update_counter += 1;
        if self.update_counter.is_multiple_of(COMPACTION_TRIGGER_COUNT) {
            self.compact_sst_files()?;
        }
        Ok(())
    }

    /// Retrieves the value associated with the given key.
    /// Checks the in-memory memtable first, then the negative cache (keys confirmed absent).
    /// If not found in either, performs a linear scan across SST files from newest to oldest.
    /// On a miss, the key is added to the negative cache to skip SST scans on future lookups.
    /// Returns `None` if the key does not exist or has been deleted.
    /// # Arguments
    /// * `key` - The key to look up
    /// # Returns
    /// `Some(value)` if the key exists and has not been deleted, otherwise `None`.
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
    pub fn get(&mut self, key: &str) -> Option<String> {
        match self.memtable.get(key) {
            Some(MemtableEntry::Value(value)) => return Some(value.clone()),
            Some(MemtableEntry::Deleted) => return None,
            None => {}
        }
        if self.negative_cache.contains(key) {
            return None;
        }

        for n in (1..=self.sst_file_counter).rev() {
            let sst_file = format!("sst-{n}.json");
            if let Ok(file) = File::open(self.directory_path.join(sst_file)) {
                let Some(entries) =
                    serde_json::from_reader::<_, Vec<SSTEntry>>(BufReader::new(file)).ok()
                else {
                    continue;
                };

                let entry = entries.iter().find(|entry| entry.key == key);
                match entry {
                    Some(SSTEntry { value: Some(v), .. }) => return Some(v.clone()),
                    Some(SSTEntry { deleted: true, .. }) => {
                        self.negative_cache.insert(key.to_string());
                        return None;
                    }
                    _ => {}
                }
            }
        }
        self.negative_cache.insert(key.to_string());
        None
    }

    /// Marks a key as deleted in the storage engine.
    /// Appends a delete record to the WAL for durability, inserts a tombstone into
    /// the memtable, and adds the key to the negative cache so future lookups return
    /// immediately without scanning SST files. The deletion is persisted to SST during
    /// the next flush; tombstones are dropped entirely during compaction.
    /// # Arguments
    /// * `key` - The key to delete
    /// # Errors
    /// Returns an `io::Error` if the WAL cannot be written or synced, or if
    /// compaction triggered by the update count fails.
    pub fn delete(&mut self, key: &str) -> Result<()> {
        let wal_record = WALRecord {
            key: key.to_string(),
            op: WALRecordType::Delete,
        };
        let mut wal = File::options()
            .append(true)
            .open(self.directory_path.join(WAL))?;
        writeln!(wal, "{}", serde_json::to_string(&wal_record)?)?;
        wal.sync_data()?;

        self.negative_cache.insert(key.to_string());
        self.memtable
            .insert(key.to_string(), MemtableEntry::Deleted);
        self.update_counter += 1;
        if self.update_counter.is_multiple_of(COMPACTION_TRIGGER_COUNT) {
            self.compact_sst_files()?;
        }
        Ok(())
    }

    /// Writes pre-serialized memtable content to a new SST file, then clears the
    /// WAL and memtable. The SST file is named `sst-{N}.json` and the manifest is
    /// updated atomically via a temp-file rename. The WAL and memtable are only
    /// cleared after the SST write succeeds, so a failure leaves them intact.
    /// # Errors
    /// Returns an `io::Error` if there is an issue creating, writing, or syncing
    /// any of the SST, manifest, or WAL files.
    fn flush(&mut self, flush_content: Value) -> Result<()> {
        self.write_sst_file(flush_content)?;
        // Clear the WAL after a successful flush
        File::create(self.directory_path.join(WAL))?.sync_data()?;
        self.memtable.clear();
        Ok(())
    }

    /// Appends `sst_file_name` to the manifest and atomically replaces the manifest
    /// file via a temp-file rename. Both the updated manifest and the storage directory
    /// are synced before returning to guarantee durability.
    /// # Errors
    /// Returns an `io::Error` if any read, write, sync, or rename operation fails.
    fn update_manifest(&mut self, sst_file_name: String) -> Result<()> {
        let mut manifest_content = fs::read(self.directory_path.join(MANIFEST))?;
        writeln!(manifest_content, "{}", sst_file_name)?;
        let mut manifest_temp = File::options()
            .create(true)
            .write(true)
            .truncate(true)
            .open(self.directory_path.join(MANIFEST_TEMP))?;
        manifest_temp.write_all(&manifest_content)?;
        manifest_temp.sync_data()?;
        fs::rename(
            self.directory_path.join(MANIFEST_TEMP),
            self.directory_path.join(MANIFEST),
        )?;
        let manifest = File::open(self.directory_path.join(MANIFEST))?;
        manifest.sync_data()?;
        self.sync_parent_dir()?;
        Ok(())
    }

    /// Serializes the current memtable into a sorted JSON array suitable for writing
    /// to an SST file. Entries are sorted by key and include both live values and
    /// tombstones so that deletions are persisted correctly.
    fn create_flush_content(&self) -> Value {
        let mut entries: Vec<(&String, &MemtableEntry)> = self.memtable.iter().collect();
        entries.sort_by_key(|(k, _)| *k);
        let json_flush: Value = entries
            .iter()
            .take(MAX_ENTRIES)
            .map(|(k, e)| match e {
                MemtableEntry::Value(v) => {
                    json!(SSTEntry {
                        key: k.to_string(),
                        value: Some(v.to_string()),
                        deleted: false,
                    })
                }
                MemtableEntry::Deleted => {
                    json!(SSTEntry {
                        key: k.to_string(),
                        value: None,
                        deleted: true,
                    })
                }
            })
            .collect();
        json_flush
    }

    /// Merges all existing SST files into a minimal set of new SST files using a
    /// k-way min-heap merge. For duplicate keys across files, the newest file's value
    /// wins (files are ordered newest-first). Tombstones are dropped so that deleted
    /// keys do not appear in the compacted output.
    ///
    /// After writing the new SST files, the manifest is updated atomically to list only
    /// the new files, and the old SST files are deleted. Compaction runs synchronously
    /// and blocks all writes for its duration.
    /// # Errors
    /// Returns an `io::Error` if any SST, manifest, or directory operation fails.
    fn compact_sst_files(&mut self) -> Result<()> {
        println!("Starting compaction...");
        let mut min_heap = BinaryHeap::new();
        let manifest_path = self.directory_path.join(MANIFEST);
        if !manifest_path.exists() {
            eprintln!("Manifest file not found, skipping compaction.");
            return Ok(());
        }

        let old_manifest_content = fs::read_to_string(&manifest_path)?;
        let sst_file_names = old_manifest_content.lines().rev();
        let mut file_iters = Vec::new();
        for file_name in sst_file_names {
            let file = File::open(self.directory_path.join(file_name))?;
            let reader = BufReader::new(file);
            let file_iter = serde_json::from_reader::<_, Vec<SSTEntry>>(reader)?
                .into_iter()
                .peekable();
            file_iters.push(file_iter);
        }

        let mut sst_entries = Vec::with_capacity(MAX_ENTRIES);
        for (file_index, file_iter) in file_iters.iter_mut().enumerate() {
            if let Some(entry) = file_iter.peek() {
                min_heap.push(Reverse((
                    entry.key.clone(),
                    file_index,
                    entry.value.clone(),
                )));
            }
        }
        while let Some(Reverse((key, file_index, value))) = min_heap.pop() {
            if value.is_some() {
                let sst_entry = SSTEntry {
                    key: key.clone(),
                    value: value.clone(),
                    deleted: false,
                };
                sst_entries.push(sst_entry);
                self.negative_cache.remove(&key);
            }
            if sst_entries.len() >= MAX_ENTRIES {
                let content = json!(sst_entries);
                self.write_sst_file(content)?;
                sst_entries.clear();
            }

            file_iters[file_index].next();
            if let Some(next_entry) = file_iters[file_index].peek() {
                min_heap.push(Reverse((
                    next_entry.key.clone(),
                    file_index,
                    next_entry.value.clone(),
                )));
            }
            while min_heap.peek().is_some_and(|x| x.0.0 == key) {
                let Some(Reverse((_, dup_index, _))) = min_heap.pop() else {
                    break;
                };
                file_iters[dup_index].next();
                if let Some(next) = file_iters[dup_index].peek() {
                    min_heap.push(Reverse((next.key.clone(), file_index, next.value.clone())));
                }
            }
        }
        if !sst_entries.is_empty() {
            let content = json!(sst_entries);
            self.write_sst_file(content)?;
        }

        // INFO: Clean up section
        let mut manifest_content = String::new();
        {
            let mut manifest_file = File::open(&manifest_path)?;
            manifest_file.read_to_string(&mut manifest_content)?;
        }

        let old_lines: HashSet<&str> = old_manifest_content.lines().collect();
        manifest_content = manifest_content
            .lines()
            .filter(|line| !old_lines.contains(*line))
            .collect::<Vec<_>>()
            .join("\n");
        if !manifest_content.is_empty() {
            manifest_content.push('\n');
        }

        {
            let mut manifest_temp = File::options()
                .create(true)
                .write(true)
                .truncate(true)
                .open(self.directory_path.join(MANIFEST_TEMP))?;
            manifest_temp.write_all(manifest_content.as_bytes())?;
            manifest_temp.sync_data()?;
        }
        fs::rename(self.directory_path.join(MANIFEST_TEMP), &manifest_path)?;
        let manifest = File::open(&manifest_path)?;
        manifest.sync_data()?;
        self.sync_parent_dir()?;
        for line in old_lines {
            fs::remove_file(self.directory_path.join(line))?
        }
        self.sync_parent_dir()?;

        println!("Compaction complete.");
        Ok(())
    }

    /// Writes `sst_entries` as a JSON array to the next SST file (`sst-{N+1}.json`),
    /// syncs it to disk, updates the manifest atomically, and increments the SST file
    /// counter. The file is always created fresh (truncating any existing file at that path).
    /// # Errors
    /// Returns an `io::Error` if the file cannot be created, written, or synced, or if
    /// the manifest update fails.
    fn write_sst_file(&mut self, sst_entries: Value) -> Result<()> {
        let tmp_count = self.sst_file_counter + 1;
        let sst_file_name = format!("sst-{tmp_count}.json");
        // INFO: Truncates file
        let sst_file = File::create(self.directory_path.join(&sst_file_name))?;
        serde_json::to_writer(BufWriter::new(&sst_file), &sst_entries)?;
        sst_file.sync_data()?;
        self.update_manifest(sst_file_name)?;
        self.sst_file_counter += 1;
        Ok(())
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct SSTEntry {
    key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(skip_serializing_if = "<&bool>::not", default)]
    deleted: bool,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct WALRecord {
    key: String,
    op: WALRecordType,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
enum WALRecordType {
    Put(String),
    Delete,
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
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine::new(dir.path()).unwrap();

        engine
            .set("key1".to_string(), "value1".to_string())
            .unwrap();
        assert_eq!(engine.get("key1"), Some("value1".to_string()));
    }

    #[test]
    fn set_and_get_with_engine_restart() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine::new(dir.path()).unwrap();

        engine
            .set("key1".to_string(), "value1".to_string())
            .unwrap();

        let mut restarted_engine = StorageEngine::new(dir.path()).unwrap();

        assert_eq!(restarted_engine.get("key1"), Some("value1".to_string()));
    }

    #[test]
    fn get_nonexistent_key_return_none() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine::new(dir.path()).unwrap();
        assert_eq!(engine.get("nonexistent"), None);
    }

    #[test]
    fn set_overwrites_existing_key() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine::new(dir.path()).unwrap();

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
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine::new(dir.path()).unwrap();

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
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine::new(dir.path()).unwrap();

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
        let mut engine = StorageEngine::new(dir.path()).unwrap();

        engine
            .set("key1".to_string(), "value1".to_string())
            .unwrap();
        let flush_content = engine.create_flush_content();
        engine.flush(flush_content).unwrap();

        assert_eq!(engine.get("key1"), Some("value1".to_string()));
    }

    #[test]
    fn get_returns_none_if_value_not_found_in_sstabe_file() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine::new(dir.path()).unwrap();

        engine
            .set("key1".to_string(), "value1".to_string())
            .unwrap();
        let flush_content = engine.create_flush_content();
        engine.flush(flush_content).unwrap();

        assert_eq!(engine.get("nonexistent"), None);
    }

    #[test]
    fn two_thousand_or_more_entries_trigger_a_flush() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine::new(dir.path()).unwrap();
        engine.memtable = (0..1999u32)
            .map(|i| {
                (
                    format!("key_{i}"),
                    MemtableEntry::Value("value_{i}".to_string()),
                )
            })
            .collect();

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
        let mut engine = StorageEngine::new(dir.path()).unwrap();
        engine.memtable = HashMap::from([
            ("b".to_string(), MemtableEntry::Value("v_2".to_string())),
            ("c".to_string(), MemtableEntry::Value("v_3".to_string())),
            ("a".to_string(), MemtableEntry::Value("v_1".to_string())),
        ]);

        let flush_content = engine.create_flush_content();
        engine.flush(flush_content).unwrap();
        let content = fs::read_to_string(dir.path().join("sst-1.json")).unwrap();
        assert_eq!(
            content,
            "[{\"key\":\"a\",\"value\":\"v_1\"},{\"key\":\"b\",\"value\":\"v_2\"},{\"key\":\"c\",\"value\":\"v_3\"}]"
        );
    }

    #[test]
    fn memtable_is_cleared_after_successful_flush() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine::new(dir.path()).unwrap();
        engine.memtable = HashMap::from([
            ("a".to_string(), MemtableEntry::Value("v_1".to_string())),
            ("b".to_string(), MemtableEntry::Value("v_2".to_string())),
            ("c".to_string(), MemtableEntry::Value("v_3".to_string())),
        ]);

        let flush_content = engine.create_flush_content();
        engine.flush(flush_content).unwrap();

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
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine::new(dir.path()).unwrap();
        engine.memtable = HashMap::from([
            ("a".to_string(), MemtableEntry::Value("v_1".to_string())),
            ("b".to_string(), MemtableEntry::Value("v_2".to_string())),
            ("c".to_string(), MemtableEntry::Value("v_3".to_string())),
        ]);
        engine.directory_path = PathBuf::from("/non/existent/directory");

        let flush_content = engine.create_flush_content();
        let result = engine.flush(flush_content);

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
        let mut engine = StorageEngine::new(dir.path()).unwrap();
        engine.memtable = HashMap::from([
            ("a".to_string(), MemtableEntry::Value("v_1".to_string())),
            ("b".to_string(), MemtableEntry::Value("v_2".to_string())),
            ("c".to_string(), MemtableEntry::Value("v_3".to_string())),
        ]);

        let flush_content = engine.create_flush_content();
        engine.flush(flush_content).unwrap();
        let flush_content = engine.create_flush_content();
        engine.flush(flush_content).unwrap();
        let flush_content = engine.create_flush_content();
        engine.flush(flush_content).unwrap();
        assert!(fs::exists(dir.path().join("sst-1.json")).unwrap());
        assert!(fs::exists(dir.path().join("sst-2.json")).unwrap());
        assert!(fs::exists(dir.path().join("sst-3.json")).unwrap());
    }

    #[test]
    fn failed_flush_should_not_increase_counter() {
        let mut engine = StorageEngine {
            memtable: HashMap::from([
                ("a".to_string(), MemtableEntry::Value("v_1".to_string())),
                ("b".to_string(), MemtableEntry::Value("v_2".to_string())),
                ("c".to_string(), MemtableEntry::Value("v_3".to_string())),
            ]),
            directory_path: PathBuf::from("/non/existent/directory"),
            sst_file_counter: 0,
            negative_cache: LRUCache::new(100),
            update_counter: 0,
        };

        let flush_content = engine.create_flush_content();
        let result = engine.flush(flush_content);

        assert!(
            result.is_err(),
            "Flush should fail due to invalid directory"
        );
        assert_eq!(engine.sst_file_counter, 0, "Counter should still be zero");
    }

    #[test]
    fn new_should_set_counter_from_latest_sst_table_file_name_in_manifest() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(MANIFEST), "sst-1.json\n").unwrap();

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

        let flush_content = engine.create_flush_content();
        engine.flush(flush_content).unwrap();
        let flush_content = engine.create_flush_content();
        engine.flush(flush_content).unwrap();

        let manifest_content = fs::read_to_string(dir.path().join(MANIFEST)).unwrap();
        assert_eq!(
            manifest_content, "sst-1.json\nsst-2.json\n",
            "Manifest should contain the latest SST file name"
        );
    }

    #[test]
    fn wal_file_should_be_created_on_startup() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join(WAL);
        assert!(
            !wal_path.exists(),
            "WAL file should not exist before initialization"
        );

        let _engine = StorageEngine::new(dir.path()).unwrap();

        assert!(
            wal_path.exists(),
            "WAL file should be created during initialization"
        );
    }

    #[test]
    fn wal_content_should_be_insert_in_memtable_on_startup() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join(WAL);
        let wal_record = WALRecord {
            op: WALRecordType::Put("value1".to_string()),
            key: "key1".to_string(),
        };
        let wal_content = serde_json::to_string(&wal_record).unwrap();
        fs::write(wal_path, wal_content + "\n").unwrap();

        let mut engine = StorageEngine::new(dir.path()).unwrap();

        assert_eq!(engine.get("key1"), Some("value1".to_string()));
    }

    #[test]
    fn compact_sst_files_keeps_newest_record() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine::new(dir.path()).unwrap();
        for i in 0..(MAX_ENTRIES) {
            engine
                .set(format!("aaa{i}"), "value_1".to_string())
                .unwrap();
        }
        assert!(fs::exists(dir.path().join("sst-1.json")).unwrap());
        for i in 0..(MAX_ENTRIES) {
            engine
                .set(format!("aaa{i}"), "value_2".to_string())
                .unwrap();
        }
        assert!(fs::exists(dir.path().join("sst-2.json")).unwrap());

        engine.compact_sst_files().unwrap();

        assert!(fs::exists(dir.path().join("sst-3.json")).unwrap());
        assert_eq!(engine.get("aaa0").unwrap(), "value_2".to_string());
        assert_eq!(engine.get("aaa100").unwrap(), "value_2".to_string());
    }

    #[test]
    fn compact_sst_files_drops_tombstones() {
        let dir = tempdir().unwrap();
        let mut engine = StorageEngine::new(dir.path()).unwrap();
        for i in 0..(MAX_ENTRIES) {
            engine
                .set(format!("aaa{i}"), "value_1".to_string())
                .unwrap();
        }
        assert!(fs::exists(dir.path().join("sst-1.json")).unwrap());
        engine.delete("aaa0").unwrap();
        for i in 1..(MAX_ENTRIES) {
            engine
                .set(format!("aaa{i}"), "value_2".to_string())
                .unwrap();
        }
        assert!(fs::exists(dir.path().join("sst-2.json")).unwrap());

        engine.compact_sst_files().unwrap();

        assert!(fs::exists(dir.path().join("sst-3.json")).unwrap());
        assert!(engine.get("aaa0").is_none());
    }
}
