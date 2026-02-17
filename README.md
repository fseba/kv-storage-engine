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

- **In-memory storage**: Uses a `HashMap<String, String>` for fast key-value operations
- **Thread-safe access**: Wrapped in `Arc<Mutex<>>` for concurrent access across HTTP handlers
- **Simple interface**: Provides `get()` and `set()` methods for basic operations

#### Methods

- `new()` - Creates a new empty storage engine instance
- `set(key: String, value: String)` - Stores a key-value pair
- `get(key: &str) -> Option<&String>` - Retrieves a value by key, returns `None` if not found

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

**Example:**
```bash
curl -X PUT http://127.0.0.1:8080/mykey \
  -H "Content-Type: text/plain" \
  -d 'Hello, World!'
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
- [ ] Week 2: [LSM Tree Foundations](https://read.thecoder.cafe/p/build-your-own-kv-engine-2)
- [ ] Week 3: [Durability with Write-Ahead Logging](https://read.thecoder.cafe/p/build-your-own-kv-engine-3)
- [ ] Week 4: [Deletes, Tombstones, and Compaction](https://read.thecoder.cafe/p/build-your-own-kv-engine-4)
- [ ] Week 5: [Leveling and Key-Range Partitioning](https://read.thecoder.cafe/p/build-your-own-kv-engine-5)
- [ ] Week 6: [Block-Based SSTables and Indexing](https://read.thecoder.cafe/p/build-your-own-kv-engine-6)
- [ ] Week 7: Bloom Filters and Trie Memtable
- [ ] Week 8: Concurrency

