mod storage_engine;

use std::{
    io,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, put},
};

use serde::Deserialize;

use storage_engine::StorageEngine;

#[tokio::main]
async fn main() -> io::Result<()> {
    let engine = Arc::new(Mutex::new(
        StorageEngine::new("./").expect("Failed to initialize storage engine"),
    ));
    let app = Router::new()
        .route("/{key}", get(get_value))
        .route("/{key}", put(set_value))
        .route("/{key}", delete(delete_value))
        .route("/scan", get(scan_range))
        .with_state(engine);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await?;

    axum::serve(listener, app).await?;
    Ok(())
}

/// Handles `GET /{key}` — returns the value or 404 if not found.
async fn get_value(
    engine: State<Arc<Mutex<StorageEngine>>>,
    Path(key): Path<String>,
) -> Result<String, StatusCode> {
    match engine.lock().expect("mutex was poisoned").get(&key) {
        Some(v) => Ok(v),
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Handles `PUT /{key}` — stores the request body as the value. Returns 500 on flush failure.
async fn set_value(
    engine: State<Arc<Mutex<StorageEngine>>>,
    Path(key): Path<String>,
    body: String,
) -> StatusCode {
    // println!("Storing key: {} with value: {}", &key, &body);
    match engine.lock().expect("mutex was poisoned").set(key, body) {
        Ok(_) => StatusCode::OK,
        Err(e) => {
            eprintln!("Error setting value: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

/// Handles `DELETE /{key}` — marks the key as deleted. Returns 500 on WAL write failure.
async fn delete_value(
    engine: State<Arc<Mutex<StorageEngine>>>,
    Path(key): Path<String>,
) -> StatusCode {
    // println!("Deleted key: {}", &key);
    match engine.lock().expect("mutex was poisoned").delete(&key) {
        Ok(_) => StatusCode::ACCEPTED,
        Err(e) => {
            eprintln!("Error setting value: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

async fn scan_range(
    engine: State<Arc<Mutex<StorageEngine>>>,
    range: Query<ScanRange>,
) -> Result<String, StatusCode> {
    let range = range.0;
    match engine
        .lock()
        .expect("mutex was poisoned")
        .scan(&range.start, &range.end)
    {
        Ok(v) => Ok(v),
        Err(e) => {
            eprintln!("Error scanning keys: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Deserialize, Debug)]
struct ScanRange {
    start: String,
    end: String,
}
