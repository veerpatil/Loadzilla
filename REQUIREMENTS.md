# Requirements: perftest

Product requirements for the Rust HTTP performance-testing CLI.

## Goal

Give developers a single binary to load-test HTTP services and see throughput, latency percentiles, and error rates without a heavyweight test platform.

## In scope (v0.1)

| ID | Requirement |
|----|-------------|
| R1 | CLI accepts a target URL and runs concurrent HTTP requests against it |
| R2 | Support methods: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS |
| R3 | Configure concurrency (`-c`) |
| R4 | Stop by request count (`-n`) **or** wall-clock duration (`-d`) |
| R5 | Optional sustained rate limit (`-r` req/s) |
| R6 | Custom headers (`-H`) and request body (`-b` / `--body-file`) |
| R7 | Per-request timeout (`-t`) |
| R8 | Report ok/error counts, req/s, bytes transferred |
| R9 | Report latency min / avg / p50 / p90 / p95 / p99 / max |
| R10 | Report HTTP status-code histogram |
| R11 | Human-readable output (default) and `--json` for scripts/CI |
| R12 | Treat non-2xx as errors by default (`--fail-on-status`) |
| R13 | TLS via rustls (no OpenSSL system dependency) |

## Out of scope (for now)

- Browser / WebSocket / gRPC protocols
- Distributed multi-node runners
- Scenario scripting / DSL
- Auth helpers beyond raw headers
- Persistent result store / dashboards
- Server-side instrumentation (APM)

## Non-functional

- Fast enough for local and CI use (async Tokio worker pool)
- Zero config beyond CLI flags
- Deterministic exit: non-zero if every request failed
- Runs on Linux/macOS with a recent stable Rust toolchain

## Success criteria

1. `cargo build --release` produces `perftest`
2. `perftest http://127.0.0.1:<port> -n 200 -c 20` completes and prints a summary
3. JSON mode is valid single-object JSON on stdout
4. Unit tests for duration and header parsing pass
