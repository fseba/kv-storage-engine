# Key-Value Storage Engine

This project implements a key-value storage engine in Rust. Based on [Build Your Own Key-Value Storage Engine](https://read.thecoder.cafe/p/build-your-own-kv-engine) by [The Coder Cafe](https://thecoder.cafe/).

## Assumptions

- Keys are lowercase ASCII strings.
- Values are ASCII strings.
- Implementation is single-threaded (Will be revisited later).
> NOTE: Assumptions persist for the rest of the series unless explicitly discarded.

## Architecture

### Storage Engine Module

The `StorageEngine` struct provides the core key-value storage functionality:

- **WAL**: Every write is first appended to a write-ahead log (`wal.db`) and synced to disk for durability. On startup, unflushed WAL entries are replayed into the memtable to recover the last session
- **Memtable**: Writes are buffered in an in-memory `HashMap<String, MemtableEntry>` for fast key-value operations. Deletions are represented as tombstones (`MemtableEntry::Deleted`) rather than immediate removals
- **SST files**: When the memtable reaches 2000 entries, it is flushed to a sorted JSON SST file under the `l0/` directory and the WAL is cleared. Tombstones are written to SST files so deletions survive a restart
- **Leveled compaction**: Once the L0 directory accumulates 5 SST files, all L0 files and any existing L1 files are merged via a k-way min-heap merge into new key-range-partitioned SST files under the `l1/` directory. Duplicate keys are resolved by keeping the newest value; tombstones are dropped entirely. Old L0/L1 files are removed and the manifest is updated atomically
- **Manifest**: Tracks SST files in two sections — a flat `[L0]` list and a `[L1]` list of key-range-to-file mappings. Kept in memory on the `StorageEngine` and updated atomically via a temp file rename on each flush and compaction
- **Negative cache**: An LRU cache of recently queried absent or deleted keys to avoid redundant SST scans
- **Thread-safe access**: Wrapped in `Arc<Mutex<>>` for concurrent access across HTTP handlers
- **Simple interface**: Provides `get()`, `set()`, `delete()`, and `scan()` methods for basic operations

#### Methods

- `new(path)` - Creates a storage engine rooted at `path`, creating the `l0/`/`l1/` directories if needed, recovering the SST counter and in-memory manifest from the `MANIFEST` file, and replaying any unflushed WAL entries into the memtable
- `set(key: String, value: String) -> Result<()>` - Appends to the WAL, inserts into the memtable, flushes to disk if the memtable is full, and triggers compaction if L0 has reached 5 files
- `get(key: &str) -> Option<String>` - Retrieves a value by key; checks the memtable, then the negative cache, then L0 SST files newest-to-oldest, then the single L1 SST file whose key range contains the key. Returns `None` for missing or deleted keys
- `delete(key: &str) -> Result<()>` - Appends a delete record to the WAL, inserts a tombstone into the memtable, flushes to disk if the memtable is full, and triggers compaction if L0 has reached 5 files
- `scan(start: &str, end: &str) -> Result<String>` - Returns a comma-separated, sorted list of all live (non-deleted) keys in the half-open range `[start, end)`, merging results from the memtable, L0 SST files, and any overlapping L1 SST files

### HTTP API

The server exposes a REST API on `127.0.0.1:8080` with the following endpoints:

#### GET /{key}
Retrieves the value associated with the given key.

**Response:**
- `200 OK` - Returns the value as plain text
- `404 Not Found` - Key does not exist

**Example:**
```bash
curl http://127.0.0.1:8080/mykey
```

#### PUT /{key}
Sets or updates the value for the given key.

**Response:**
- `200 OK` - Value successfully stored
- `500 Internal Server Error` - Failed to write to WAL or flush memtable to disk

**Example:**
```bash
curl -X PUT http://127.0.0.1:8080/mykey \
  -H "Content-Type: text/plain" \
  -d 'Hello, World!'
```

#### DELETE /{key}
Marks the given key as deleted.

**Response:**
- `202 Accepted` - Key successfully deleted
- `500 Internal Server Error` - Failed to write to WAL

**Example:**
```bash
curl -X DELETE http://127.0.0.1:8080/mykey
```

#### GET /scan?start={start}&end={end}
Returns all live keys in the half-open range `[start, end)`.

**Response:**
- `200 OK` - Returns a comma-separated, sorted list of keys as plain text
- `500 Internal Server Error` - Failed to read SST files

**Example:**
```bash
curl "http://127.0.0.1:8080/scan?start=a&end=m"
```

### Client

The `client` crate is a load-testing and consistency-checking tool for the HTTP API. It reads a sequence of operations from `put.txt` or `put-delete.txt` (PUT/GET/DELETE requests), replays them against the running server with retry-on-transient-failure logic, and reports:

- Consistency checks: if a request takes longer than 1 second, it verifies that the last successful `PUT` is actually readable back from the server
- Latency metrics: p50/p95/p99 latency (in ms) for each request type

Run it with:
```bash
cargo run -p client
```

## Usage

1. Start the server:
   ```bash
   cargo run
   ```

2. Store a value:
   ```bash
   curl -X PUT http://127.0.0.1:8080/greeting \
     -H "Content-Type: text/plain" \
     -d 'Hello, World!'
   ```

3. Retrieve the value:
   ```bash
   curl http://127.0.0.1:8080/greeting
   # Output: Hello, World!
   ```

4. Try to get a non-existent key:
   ```bash
   curl http://127.0.0.1:8080/nonexistent
   # Output: 404 Not Found
   ```

## Agenda

- [x] Week 1: [In-Memory Store](https://read.thecoder.cafe/p/build-your-own-kv-engine-1)
- [x] Week 2: [LSM Tree Foundations](https://read.thecoder.cafe/p/build-your-own-kv-engine-2)
- [x] Week 3: [Durability with Write-Ahead Logging](https://read.thecoder.cafe/p/build-your-own-kv-engine-3)
- [x] Week 4: [Deletes, Tombstones, and Compaction](https://read.thecoder.cafe/p/build-your-own-kv-engine-4)
- [x] Week 5: [Leveling and Key-Range Partitioning](https://read.thecoder.cafe/p/build-your-own-kv-engine-5)
- [ ] Week 6: [Block-Based SSTables and Indexing](https://read.thecoder.cafe/p/build-your-own-kv-engine-6)
- [ ] Week 7: [Bloom Filters and Trie Memtable](https://read.thecoder.cafe/p/build-your-own-kv-engine-7)
- [ ] Week 8: [Concurrency](https://read.thecoder.cafe/p/build-your-own-kv-engine-8)

