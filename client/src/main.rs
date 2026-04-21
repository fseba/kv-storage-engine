use std::{
    fs::File,
    io::{BufRead, BufReader},
    time::Instant,
};

use indicatif::ProgressBar;

use reqwest::StatusCode;
use reqwest_middleware::ClientBuilder;
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};

const FILE_NAME: &str = "put-delete.txt";
// const FILE_NAME: &str = "put.txt";

#[derive(Debug)]
enum Method {
    Put(String),
    Get(String),
    Delete,
}

#[derive(Debug)]
struct Request {
    method: Method,
    key: String,
    success: bool,
}

fn calc_percentile(percentile: f64, latencies: &[u128]) -> f64 {
    let idx = ((latencies.len() as f64 * percentile) as usize).saturating_sub(1);
    latencies[idx] as f64 / 1000.0
}

fn print_latency_metrics(label: &str, latencies: &mut [u128]) {
    if latencies.is_empty() {
        return;
    }
    latencies.sort_unstable();
    let p50 = calc_percentile(0.50, latencies);
    let p95 = calc_percentile(0.95, latencies);
    let p99 = calc_percentile(0.99, latencies);
    println!("{label} requests: {}", latencies.len());
    println!("  p50: {p50:.3} ms");
    println!("  p95: {p95:.3} ms");
    println!("  p99: {p99:.3} ms");
}

async fn check_put_consistency(
    client: &reqwest_middleware::ClientWithMiddleware,
    base_url: &str,
    bar: &ProgressBar,
    request_key: &str,
    last_put: &(String, String),
) -> Result<(), reqwest_middleware::Error> {
    bar.println(format!(
        "----- Request for key {request_key} took longer than 1 second, checking for data consistency -----"
    ));
    let (key, expected_value) = last_put;
    let res = client.get(format!("{base_url}/{key}")).send().await?;
    match res.status() {
        StatusCode::OK => match res.text().await {
            Ok(v) if v == *expected_value => {
                bar.println(format!(
                    "PUT --- Data consistency verified for key {key} after retry"
                ));
            }
            Ok(v) => {
                bar.println(format!(
                    "PUT --- Data inconsistency detected for key {key}: expected '{expected_value}', got '{v}'"
                ));
            }
            Err(_) => {
                bar.println(format!(
                    "PUT --- Failed to retrieve value for key {key} after retry"
                ));
            }
        },
        status => {
            bar.println(format!(
                "PUT --- Data inconsistency detected for key {key}: expected OK, got {status}"
            ));
        }
    }
    Ok(())
}

async fn check_delete_consistency(
    client: &reqwest_middleware::ClientWithMiddleware,
    base_url: &str,
    bar: &ProgressBar,
    request_key: &str,
    last_delete_key: &str,
) -> Result<(), reqwest_middleware::Error> {
    bar.println(format!(
        "----- Request for key {request_key} took longer than 1 second, checking for data consistency -----"
    ));
    let res = client
        .get(format!("{base_url}/{last_delete_key}"))
        .send()
        .await?;
    match res.status() {
        StatusCode::NOT_FOUND => bar.println(format!(
            "DELETE --- Data consistency verified for key {last_delete_key} after retry"
        )),
        status => {
            bar.println(format!(
                "DELETE --- Data inconsistency detected for key {last_delete_key}: expected NOT_FOUND, got {status}"
            ));
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), reqwest_middleware::Error> {
    let file = BufReader::new(File::open(FILE_NAME).unwrap());
    let mut requests: Vec<Request> = file
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                return None;
            };
            Some(Request {
                method: match parts[0] {
                    "PUT" | "GET" if parts.len() < 3 => return None,
                    "PUT" => Method::Put(parts[2].to_string()),
                    "GET" => Method::Get(parts[2].to_string()),
                    "DELETE" => Method::Delete,
                    _ => return None,
                },
                key: parts[1].to_string(),
                success: false,
            })
        })
        .collect();

    let client = ClientBuilder::new(reqwest::Client::new())
        .with(RetryTransientMiddleware::new_with_policy(
            ExponentialBackoff::builder()
                .build_with_total_retry_duration(std::time::Duration::from_secs(10)),
        ))
        .build();
    let base_url = "http://127.0.0.1:8080";
    let mut get_latencies: Vec<u128> = Vec::new();
    let mut put_latencies: Vec<u128> = Vec::new();
    let mut delete_latencies: Vec<u128> = Vec::new();

    let bar = ProgressBar::new(requests.len() as u64);
    // let msg = format!("{} loaded, starting requests...", FILE_NAME);
    // bar.println(msg);
    let timer_all = Instant::now();

    let mut last_successful_put: Option<(String, String)> = None;
    let mut last_successful_delete: Option<String> = None;

    for request in &mut requests {
        let timer_request = Instant::now();
        let url = format!("{base_url}/{}", request.key);
        match &request.method {
            Method::Put(value) => {
                let res = client.put(&url).body(value.clone()).send().await?;
                put_latencies.push(timer_request.elapsed().as_micros());
                request.success = res.status() == StatusCode::OK;
                if !request.success {
                    let msg = format!("PUT request failed with status: {}", res.status());
                    bar.println(msg);
                }
            }
            Method::Get(expected) => {
                let res = client.get(&url).send().await?;
                get_latencies.push(timer_request.elapsed().as_micros());
                let result_status = res.status();
                if expected == "NOT_FOUND" {
                    request.success = result_status == StatusCode::NOT_FOUND;
                    if !request.success {
                        let msg = format!(
                            "GET request failed with status: {} - expected 'not_found'",
                            result_status
                        );
                        bar.println(msg);
                    }
                } else {
                    match res.text().await {
                        Ok(v) => {
                            request.success = *expected == v;
                            if !request.success {
                                let msg = format!(
                                    "GET request failed with status: {} - expected {}, got {}",
                                    result_status, *expected, v
                                );
                                bar.println(msg);
                            }
                        }
                        Err(_) => {
                            request.success = false;
                            let msg = format!("GET request failed with status: {}", result_status);
                            bar.println(msg);
                        }
                    }
                }
            }
            Method::Delete => {
                let res = client.delete(&url).send().await?;
                delete_latencies.push(timer_request.elapsed().as_micros());
                request.success = res.status() == StatusCode::ACCEPTED;
                if !request.success {
                    let msg = format!("DELETE request failed with status: {}", res.status());
                    bar.println(msg);
                }
            }
        }

        // If the request took longer than 1 second, we assume that the kv-storage-engine might have been
        // restarted during the request. We then check if the last successful PUT before the restart has been stored correctly
        if timer_request.elapsed().as_secs() > 1 && request.success {
            if let Some(last_put) = &last_successful_put {
                check_put_consistency(&client, base_url, &bar, &request.key, last_put).await?;
            }

            if let Some(last_delete) = &last_successful_delete {
                check_delete_consistency(&client, base_url, &bar, &request.key, last_delete)
                    .await?;
            }
        }

        if request.success {
            match &request.method {
                Method::Put(value) => {
                    last_successful_put = Some((request.key.clone(), value.clone()))
                }
                Method::Get(_) => (),
                Method::Delete => last_successful_delete = Some(request.key.clone()),
            }
        }
        bar.inc(1);
    }
    bar.finish();
    let duration = timer_all.elapsed().as_millis();

    println!("\n--- Latency Metrics ---");
    println!("Overall duration: {duration} ms");

    let successful = requests.iter().filter(|r| r.success).count();
    println!("Successful requests: {successful}/{}", requests.len());
    let failed = requests.iter().filter(|r| !r.success);
    for (i, fail) in failed.enumerate() {
        println!("#{i}: {:?}", fail);
    }

    print_latency_metrics("GET", &mut get_latencies);
    print_latency_metrics("PUT", &mut put_latencies);
    print_latency_metrics("DELETE", &mut delete_latencies);

    Ok(())
}
