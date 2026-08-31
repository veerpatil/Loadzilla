use anyhow::{Context, Result};
use hdrhistogram::Histogram;
use reqwest::{Client, Method};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};

#[derive(Debug, Clone)]
pub struct Config {
    pub url: String,
    pub method: Method,
    pub concurrency: usize,
    pub requests: Option<u64>,
    pub duration: Option<Duration>,
    pub timeout: Duration,
    pub rate: u64,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub fail_on_status: bool,
    pub keepalive: bool,
}

#[derive(Debug, Clone)]
pub struct Sample {
    pub latency: Duration,
    pub status: Option<u16>,
    pub ok: bool,
    #[allow(dead_code)]
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct Summary {
    pub url: String,
    pub method: String,
    pub concurrency: usize,
    pub wall_time: Duration,
    pub ok_count: u64,
    pub error_count: u64,
    pub status_counts: Vec<(u16, u64)>,
    pub bytes_read: u64,
    pub latency_min_ms: f64,
    pub latency_avg_ms: f64,
    pub latency_max_ms: f64,
    pub latency_p50_ms: f64,
    pub latency_p90_ms: f64,
    pub latency_p95_ms: f64,
    pub latency_p99_ms: f64,
    pub requests_per_sec: f64,
}

pub async fn run(config: Config) -> Result<Summary> {
    let mut builder = Client::builder()
        .timeout(config.timeout)
        .pool_max_idle_per_host(if config.keepalive {
            config.concurrency
        } else {
            0
        })
        .tcp_nodelay(true);

    if !config.keepalive {
        builder = builder.pool_max_idle_per_host(0);
    }

    let client = builder.build().context("failed to build HTTP client")?;

    let stop = Arc::new(AtomicBool::new(false));
    let issued = Arc::new(AtomicU64::new(0));
    let bytes_read = Arc::new(AtomicU64::new(0));

    if let Some(d) = config.duration {
        let stop_flag = Arc::clone(&stop);
        tokio::spawn(async move {
            tokio::time::sleep(d).await;
            stop_flag.store(true, Ordering::Relaxed);
        });
    }

    let rate_gate = if config.rate > 0 {
        let (tx, rx) = mpsc::channel::<()>(config.concurrency.max(1) * 2);
        let rate = config.rate;
        let stop_flag = Arc::clone(&stop);
        tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs_f64(1.0 / rate as f64));
            tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
            while !stop_flag.load(Ordering::Relaxed) {
                tick.tick().await;
                if tx.send(()).await.is_err() {
                    break;
                }
            }
        });
        Some(Arc::new(tokio::sync::Mutex::new(rx)))
    } else {
        None
    };

    let (tx, mut rx) = mpsc::channel::<Sample>(config.concurrency.max(1) * 4);
    let wall_start = Instant::now();
    let mut joins = Vec::with_capacity(config.concurrency);
    let total_target = config.requests;
    let timed_run = config.duration.is_some();

    for _ in 0..config.concurrency {
        let client = client.clone();
        let url = config.url.clone();
        let method = config.method.clone();
        let headers = config.headers.clone();
        let body = config.body.clone();
        let fail_on_status = config.fail_on_status;
        let stop = Arc::clone(&stop);
        let issued = Arc::clone(&issued);
        let bytes_counter = Arc::clone(&bytes_read);
        let tx = tx.clone();
        let rate_gate = rate_gate.clone();

        joins.push(tokio::spawn(async move {
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                if let Some(n) = total_target {
                    let prev = issued.fetch_add(1, Ordering::Relaxed);
                    if prev >= n {
                        break;
                    }
                } else if !timed_run {
                    break;
                }

                if let Some(gate) = &rate_gate {
                    if gate.lock().await.recv().await.is_none() {
                        break;
                    }
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                }

                let sample = one_request(
                    &client,
                    &url,
                    &method,
                    &headers,
                    body.as_deref(),
                    fail_on_status,
                    &bytes_counter,
                )
                .await;

                if tx.send(sample).await.is_err() {
                    break;
                }
            }
        }));
    }

    drop(tx);

    let mut samples = Vec::new();
    while let Some(sample) = rx.recv().await {
        samples.push(sample);
    }

    for join in joins {
        let _ = join.await;
    }

    let wall_time = wall_start.elapsed();
    Ok(build_summary(
        &config,
        wall_time,
        &samples,
        bytes_read.load(Ordering::Relaxed),
    ))
}

async fn one_request(
    client: &Client,
    url: &str,
    method: &Method,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    fail_on_status: bool,
    bytes_counter: &AtomicU64,
) -> Sample {
    let started = Instant::now();
    let mut req = client.request(method.clone(), url);
    for (name, value) in headers {
        req = req.header(name.as_str(), value.as_str());
    }
    if let Some(b) = body {
        req = req.body(b.to_vec());
    }

    match req.send().await {
        Ok(resp) => {
            let status = resp.status();
            let status_code = status.as_u16();
            match resp.bytes().await {
                Ok(bytes) => {
                    bytes_counter.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                    let ok = if fail_on_status {
                        status.is_success()
                    } else {
                        true
                    };
                    Sample {
                        latency: started.elapsed(),
                        status: Some(status_code),
                        ok,
                        error: if ok {
                            None
                        } else {
                            Some(format!("HTTP {status_code}"))
                        },
                    }
                }
                Err(e) => Sample {
                    latency: started.elapsed(),
                    status: Some(status_code),
                    ok: false,
                    error: Some(e.to_string()),
                },
            }
        }
        Err(e) => {
            let status = e.status().map(|s| s.as_u16());
            Sample {
                latency: started.elapsed(),
                status,
                ok: false,
                error: Some(e.to_string()),
            }
        }
    }
}

fn build_summary(
    config: &Config,
    wall_time: Duration,
    samples: &[Sample],
    bytes_read: u64,
) -> Summary {
    let mut hist = Histogram::<u64>::new_with_bounds(1, 60_000_000, 3).expect("histogram");
    let mut ok_count = 0u64;
    let mut error_count = 0u64;
    let mut status_map: std::collections::BTreeMap<u16, u64> = std::collections::BTreeMap::new();
    let mut sum_ms = 0.0f64;

    for s in samples {
        let micros = s.latency.as_micros().min(u128::from(u64::MAX)) as u64;
        let micros = micros.max(1);
        let _ = hist.record(micros);

        let ms = s.latency.as_secs_f64() * 1000.0;
        sum_ms += ms;

        if s.ok {
            ok_count += 1;
        } else {
            error_count += 1;
        }
        if let Some(code) = s.status {
            *status_map.entry(code).or_default() += 1;
        }
    }

    let total = samples.len() as u64;
    let wall_secs = wall_time.as_secs_f64().max(f64::EPSILON);
    let value_at = |pct: f64| -> f64 {
        if total == 0 {
            0.0
        } else {
            hist.value_at_percentile(pct) as f64 / 1000.0
        }
    };

    Summary {
        url: config.url.clone(),
        method: config.method.to_string(),
        concurrency: config.concurrency,
        wall_time,
        ok_count,
        error_count,
        status_counts: status_map.into_iter().collect(),
        bytes_read,
        latency_min_ms: if total == 0 {
            0.0
        } else {
            hist.min() as f64 / 1000.0
        },
        latency_avg_ms: if total == 0 {
            0.0
        } else {
            sum_ms / total as f64
        },
        latency_max_ms: if total == 0 {
            0.0
        } else {
            hist.max() as f64 / 1000.0
        },
        latency_p50_ms: value_at(50.0),
        latency_p90_ms: value_at(90.0),
        latency_p95_ms: value_at(95.0),
        latency_p99_ms: value_at(99.0),
        requests_per_sec: total as f64 / wall_secs,
    }
}
