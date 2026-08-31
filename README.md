# perftest

A fast HTTP load-testing CLI written in Rust. Point it at a URL, set concurrency and request count (or duration), and get latency percentiles, throughput, and error rates.

## Requirements

- Rust 1.75+ (edition 2021)
- Network access to the target under test

## Install / build

```bash
cargo build --release
```

Binary: `./target/release/perftest`

Or run without installing:

```bash
cargo run --release -- --help
```

## Usage

```bash
# 100 GET requests, 10 workers (defaults)
perftest https://httpbin.org/get

# Fixed request count + concurrency
perftest https://example.com -n 1000 -c 50

# Time-boxed run
perftest https://example.com -d 30s -c 20

# POST with body and headers
perftest https://httpbin.org/post -X POST \
  -H "Content-Type: application/json" \
  -b '{"hello":"world"}' \
  -n 200 -c 10

# Cap throughput at 100 req/s
perftest https://example.com -d 10s -c 20 -r 100

# Machine-readable output
perftest https://example.com -n 500 -c 25 --json
```

### Flags

| Flag | Description | Default |
|------|-------------|---------|
| `-X, --method` | HTTP method | `GET` |
| `-c, --concurrency` | Concurrent workers | `10` |
| `-n, --requests` | Total requests | `100` (if no `-d`) |
| `-d, --duration` | Run for duration (`10s`, `2m`, …) | — |
| `-t, --timeout` | Per-request timeout | `10s` |
| `-r, --rate` | Max requests/second (`0` = unlimited) | `0` |
| `-H, --header` | Extra header (`Name: Value`), repeatable | — |
| `-b, --body` | Request body string | — |
| `--body-file` | Request body from file | — |
| `--fail-on-status` | Treat non-2xx as errors | `true` |
| `--json` | Print JSON summary | off |
| `--no-keepalive` | Disable connection reuse | off |

## What it reports

- Request counts (ok / error)
- Throughput (req/s)
- Bytes transferred
- Latency: min, avg, p50, p90, p95, p99, max
- Status code histogram

## Example output

```
perftest → GET https://example.com/  concurrency=10  requests=100

Summary
  URL           https://example.com/
  Method        GET
  Concurrency   10
  Duration      0.842s
  Requests      100 (ok=100 err=0)
  Throughput    118.76 req/s
  Transfer      12.45 KB

Latency (ms)
  min        45.120
  avg        78.331
  p50        72.000
  p90        98.000
  p95       110.000
  p99       140.000
  max       156.000

Status codes
  200     100
```

## Project layout

```
src/
  main.rs     CLI parsing and entrypoint
  runner.rs   Async load generator
  report.rs   Human + JSON reporters
```

## License

MIT
