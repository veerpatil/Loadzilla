use anyhow::{bail, Context, Result};
use clap::Parser;
use std::path::PathBuf;
use std::time::Duration;
use url::Url;

mod report;
mod runner;

/// HTTP load-testing CLI for performance testing.
#[derive(Debug, Parser)]
#[command(
    name = "perftest",
    version,
    about = "Run HTTP load tests and report latency, throughput, and error rates",
    long_about = None
)]
struct Cli {
    /// Target URL to load-test
    url: String,

    /// HTTP method
    #[arg(short = 'X', long, default_value = "GET", value_parser = parse_method)]
    method: Method,

    /// Number of concurrent workers
    #[arg(short, long, default_value_t = 10)]
    concurrency: usize,

    /// Total number of requests (mutually exclusive with --duration)
    #[arg(short = 'n', long, conflicts_with = "duration")]
    requests: Option<u64>,

    /// Test duration, e.g. 10s, 2m (mutually exclusive with --requests)
    #[arg(short = 'd', long, value_parser = parse_duration, conflicts_with = "requests")]
    duration: Option<Duration>,

    /// Per-request timeout
    #[arg(short = 't', long, default_value = "10s", value_parser = parse_duration)]
    timeout: Duration,

    /// Request rate limit in requests/second (0 = unlimited)
    #[arg(short = 'r', long, default_value_t = 0)]
    rate: u64,

    /// HTTP header, repeatable: -H "Authorization: Bearer tok"
    #[arg(short = 'H', long = "header", value_name = "HEADER")]
    headers: Vec<String>,

    /// Request body string (for POST/PUT/PATCH)
    #[arg(short, long, conflicts_with = "body_file")]
    body: Option<String>,

    /// Read request body from a file
    #[arg(long, conflicts_with = "body")]
    body_file: Option<PathBuf>,

    /// Treat non-2xx responses as errors (default: true)
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    fail_on_status: bool,

    /// Print results as JSON
    #[arg(long)]
    json: bool,

    /// Disable keep-alive / connection reuse
    #[arg(long)]
    no_keepalive: bool,
}

#[derive(Debug, Clone, Copy)]
enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl Method {
    fn as_reqwest(self) -> reqwest::Method {
        match self {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Put => reqwest::Method::PUT,
            Method::Patch => reqwest::Method::PATCH,
            Method::Delete => reqwest::Method::DELETE,
            Method::Head => reqwest::Method::HEAD,
            Method::Options => reqwest::Method::OPTIONS,
        }
    }
}

fn parse_method(s: &str) -> Result<Method, String> {
    match s.trim().to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::Get),
        "POST" => Ok(Method::Post),
        "PUT" => Ok(Method::Put),
        "PATCH" => Ok(Method::Patch),
        "DELETE" => Ok(Method::Delete),
        "HEAD" => Ok(Method::Head),
        "OPTIONS" => Ok(Method::Options),
        other => Err(format!(
            "unsupported method '{other}' (GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS)"
        )),
    }
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("duration cannot be empty".into());
    }

    let (num, unit) = if let Some(i) = s.find(|c: char| c.is_ascii_alphabetic()) {
        (&s[..i], &s[i..])
    } else {
        (s, "s")
    };

    let value: f64 = num
        .parse()
        .map_err(|_| format!("invalid duration number: {num}"))?;

    let secs = match unit.to_ascii_lowercase().as_str() {
        "ms" => value / 1000.0,
        "s" | "sec" | "secs" | "second" | "seconds" => value,
        "m" | "min" | "mins" | "minute" | "minutes" => value * 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => value * 3600.0,
        other => return Err(format!("unknown duration unit: {other}")),
    };

    if secs <= 0.0 {
        return Err("duration must be positive".into());
    }

    Ok(Duration::from_secs_f64(secs))
}

fn parse_header(raw: &str) -> Result<(String, String)> {
    let (name, value) = raw
        .split_once(':')
        .context(format!("invalid header (expected Name: Value): {raw}"))?;
    let name = name.trim();
    let value = value.trim();
    if name.is_empty() {
        bail!("header name cannot be empty: {raw}");
    }
    Ok((name.to_string(), value.to_string()))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let url = Url::parse(&cli.url).context("invalid target URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("URL scheme must be http or https");
    }

    if cli.concurrency == 0 {
        bail!("--concurrency must be at least 1");
    }

    let requests = cli.requests;
    let duration = cli.duration;
    if requests.is_none() && duration.is_none() {
        // Sensible default: 100 requests when neither is given
    }
    let requests = requests.or(if duration.is_none() { Some(100) } else { None });

    let mut headers = Vec::with_capacity(cli.headers.len());
    for h in &cli.headers {
        headers.push(parse_header(h)?);
    }

    let body = if let Some(path) = &cli.body_file {
        Some(
            tokio::fs::read(path)
                .await
                .with_context(|| format!("failed to read body file {}", path.display()))?,
        )
    } else {
        cli.body.map(|s| s.into_bytes())
    };

    let config = runner::Config {
        url: url.to_string(),
        method: cli.method.as_reqwest(),
        concurrency: cli.concurrency,
        requests,
        duration,
        timeout: cli.timeout,
        rate: cli.rate,
        headers,
        body,
        fail_on_status: cli.fail_on_status,
        keepalive: !cli.no_keepalive,
    };

    eprintln!(
        "perftest → {} {}  concurrency={}  {}",
        config.method,
        config.url,
        config.concurrency,
        match (config.requests, config.duration) {
            (Some(n), None) => format!("requests={n}"),
            (None, Some(d)) => format!("duration={d:?}"),
            (Some(n), Some(d)) => format!("requests={n} duration={d:?}"),
            (None, None) => "requests=100".to_string(),
        }
    );

    let summary = runner::run(config).await?;

    if cli.json {
        report::print_json(&summary)?;
    } else {
        report::print_human(&summary);
    }

    if summary.error_count > 0 && summary.ok_count == 0 {
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("10s").unwrap(), Duration::from_secs(10));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_duration("5").unwrap(), Duration::from_secs(5));
    }

    #[test]
    fn parses_headers() {
        let (n, v) = parse_header("Authorization: Bearer abc").unwrap();
        assert_eq!(n, "Authorization");
        assert_eq!(v, "Bearer abc");
    }
}
