mod storage_engine;

use std::{
    io,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, put},
};

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
