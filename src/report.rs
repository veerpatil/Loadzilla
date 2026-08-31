use crate::runner::Summary;
use anyhow::Result;
use std::io::{self, Write};

pub fn print_human(s: &Summary) {
    let total = s.ok_count + s.error_count;
    println!();
    println!("Summary");
    println!("  URL           {}", s.url);
    println!("  Method        {}", s.method);
    println!("  Concurrency   {}", s.concurrency);
    println!("  Duration      {:.3}s", s.wall_time.as_secs_f64());
    println!("  Requests      {total} (ok={} err={})", s.ok_count, s.error_count);
    println!("  Throughput    {:.2} req/s", s.requests_per_sec);
    println!(
        "  Transfer      {}",
        format_bytes(s.bytes_read)
    );
    println!();
    println!("Latency (ms)");
    println!("  min   {:>10.3}", s.latency_min_ms);
    println!("  avg   {:>10.3}", s.latency_avg_ms);
    println!("  p50   {:>10.3}", s.latency_p50_ms);
    println!("  p90   {:>10.3}", s.latency_p90_ms);
    println!("  p95   {:>10.3}", s.latency_p95_ms);
    println!("  p99   {:>10.3}", s.latency_p99_ms);
    println!("  max   {:>10.3}", s.latency_max_ms);

    if !s.status_counts.is_empty() {
        println!();
        println!("Status codes");
        for (code, count) in &s.status_counts {
            println!("  {code}     {count}");
        }
    }
    println!();
}

pub fn print_json(s: &Summary) -> Result<()> {
    let mut out = io::stdout().lock();
    write!(out, "{{")?;
    write!(out, "\"url\":{},", json_str(&s.url))?;
    write!(out, "\"method\":{},", json_str(&s.method))?;
    write!(out, "\"concurrency\":{},", s.concurrency)?;
    write!(out, "\"duration_secs\":{:.6},", s.wall_time.as_secs_f64())?;
    write!(out, "\"ok\":{},", s.ok_count)?;
    write!(out, "\"errors\":{},", s.error_count)?;
    write!(out, "\"requests_per_sec\":{:.6},", s.requests_per_sec)?;
    write!(out, "\"bytes_read\":{},", s.bytes_read)?;
    write!(out, "\"latency_ms\":{{")?;
    write!(out, "\"min\":{:.6},", s.latency_min_ms)?;
    write!(out, "\"avg\":{:.6},", s.latency_avg_ms)?;
    write!(out, "\"p50\":{:.6},", s.latency_p50_ms)?;
    write!(out, "\"p90\":{:.6},", s.latency_p90_ms)?;
    write!(out, "\"p95\":{:.6},", s.latency_p95_ms)?;
    write!(out, "\"p99\":{:.6},", s.latency_p99_ms)?;
    write!(out, "\"max\":{:.6}", s.latency_max_ms)?;
    write!(out, "}},")?;
    write!(out, "\"status_codes\":{{")?;
    for (i, (code, count)) in s.status_counts.iter().enumerate() {
        if i > 0 {
            write!(out, ",")?;
        }
        write!(out, "\"{code}\":{count}")?;
    }
    write!(out, "}}")?;
    writeln!(out, "}}")?;
    Ok(())
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} {}", UNITS[i])
    } else {
        format!("{v:.2} {}", UNITS[i])
    }
}
