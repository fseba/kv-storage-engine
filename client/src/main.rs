use std::{
    fs::File,
    io::{BufRead, BufReader},
    time::Instant,
};

use indicatif::ProgressBar;

use reqwest::StatusCode;

struct Request {
    pub method: String,
    pub key: String,
    pub value: String,
    pub success: bool,
}

fn calc_percentile(percentile: f64, latencies: &[u128]) -> f64 {
    let idx = ((latencies.len() as f64 * percentile) as usize).saturating_sub(1);
    latencies[idx] as f64 / 1000.0
}

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let file = BufReader::new(File::open("put.txt").unwrap());
    let mut requests: Vec<Request> = file
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 3 {
                Some(Request {
                    method: parts[0].to_string(),
                    key: parts[1].to_string(),
                    value: parts[2].to_string(),
                    success: false,
                })
            } else {
                None
            }
        })
        .collect();

    let client = reqwest::Client::new();
    let base_url = "http://127.0.0.1:8080";
    let mut get_latencies: Vec<u128> = Vec::new();
    let mut put_latencies: Vec<u128> = Vec::new();

    let bar = ProgressBar::new(requests.len() as u64);
    let timer_all = Instant::now();
    for request in &mut requests {
        let timer_request = Instant::now();
        let url = format!("{base_url}/{}", request.key);
        if request.method == "PUT" {
            let res = client.put(url).body(request.value.clone()).send().await?;
            put_latencies.push(timer_request.elapsed().as_micros());
            request.success = res.status() == StatusCode::OK;
            let msg = format!("PUT request successful: {}", request.success);
            bar.println(msg);
        } else {
            let res = client.get(url).send().await?;
            get_latencies.push(timer_request.elapsed().as_micros());
            if request.value == "NOT_FOUND" {
                request.success = res.status() == StatusCode::NOT_FOUND;
            } else {
                match res.text().await {
                    Ok(v) => request.success = request.value == v,
                    Err(_) => request.success = false,
                }
            }
            let msg = format!("GET request successful: {}", request.success);
            bar.println(msg);
        };
        bar.inc(1);
    }
    bar.finish();
    let duration = timer_all.elapsed().as_millis();

    println!("\n--- Latency Metrics ---");
    println!("Overall duration: {duration} ms");

    let successful = requests.iter().filter(|r| r.success).count();
    println!("Successful requests: {successful}/{}", requests.len());

    if !get_latencies.is_empty() {
        get_latencies.sort_unstable();

        let p50 = calc_percentile(0.50, &get_latencies);
        let p95 = calc_percentile(0.95, &get_latencies);
        let p99 = calc_percentile(0.99, &get_latencies);

        println!("GET requests: {}", get_latencies.len());
        println!("  p50: {:.3} ms", p50);
        println!("  p95: {:.3} ms", p95);
        println!("  p99: {:.3} ms", p99);
    }

    if !put_latencies.is_empty() {
        put_latencies.sort_unstable();
        let p50 = calc_percentile(0.50, &put_latencies);
        let p95 = calc_percentile(0.95, &put_latencies);
        let p99 = calc_percentile(0.99, &put_latencies);

        println!("PUT requests: {}", put_latencies.len());
        println!("  p50: {:.3} ms", p50);
        println!("  p95: {:.3} ms", p95);
        println!("  p99: {:.3} ms", p99);
    }

    Ok(())
}
