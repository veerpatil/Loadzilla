use crate::runner::Summary;
use anyhow::{Context, Result};
use std::io::{self, Write};
use std::path::Path;

pub fn print_human(s: &Summary) {
    let total = s.ok_count + s.error_count;
    println!();
    println!("Summary");
    println!("  URL           {}", s.url);
    println!("  Method        {}", s.method);
    println!("  Concurrency   {}", s.concurrency);
    println!("  Duration      {:.3}s", s.wall_time.as_secs_f64());
    println!(
        "  Requests      {total} (ok={} err={})",
        s.ok_count, s.error_count
    );
    println!("  Throughput    {:.2} req/s", s.requests_per_sec);
    println!("  Transfer      {}", format_bytes(s.bytes_read));
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

pub fn write_html(s: &Summary, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report directory {}", parent.display()))?;
    }

    std::fs::write(path, render_html(s))
        .with_context(|| format!("failed to write report file {}", path.display()))
}

fn render_html(s: &Summary) -> String {
    let total = s.ok_count + s.error_count;
    let success_rate = if total == 0 {
        0.0
    } else {
        s.ok_count as f64 * 100.0 / total as f64
    };
    let error_rate = if total == 0 {
        0.0
    } else {
        s.error_count as f64 * 100.0 / total as f64
    };
    let status_rows = if s.status_counts.is_empty() {
        String::from("<tr><td colspan=\"2\">No HTTP status codes recorded</td></tr>")
    } else {
        s.status_counts
            .iter()
            .map(|(code, count)| format!("<tr><td>{code}</td><td>{count}</td></tr>"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let latency_chart = latency_chart(s);
    let outcome_chart = outcome_chart(s.ok_count, s.error_count);
    let status_chart = status_chart(&s.status_counts);

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>perftest report</title>
  <style>
    :root {{
      color-scheme: light;
      --bg: #f7f8fb;
      --panel: #ffffff;
      --ink: #172033;
      --muted: #5f6d83;
      --line: #dbe2ee;
      --accent: #0f9f8f;
      --danger: #c2410c;
      --blue: #2563eb;
      --amber: #d97706;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      background: var(--bg);
      color: var(--ink);
      font: 14px/1.5 system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }}
    main {{
      width: min(1120px, calc(100vw - 32px));
      margin: 32px auto;
    }}
    header {{
      margin-bottom: 22px;
    }}
    h1 {{
      margin: 0 0 8px;
      font-size: 30px;
      letter-spacing: 0;
    }}
    .target {{
      color: var(--muted);
      overflow-wrap: anywhere;
    }}
    .grid {{
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
      gap: 14px;
      margin-bottom: 18px;
    }}
    .metric, section {{
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 8px;
      box-shadow: 0 1px 2px rgba(23, 32, 51, 0.04);
    }}
    .metric {{
      padding: 16px;
    }}
    .metric span {{
      display: block;
      color: var(--muted);
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.06em;
    }}
    .metric strong {{
      display: block;
      margin-top: 6px;
      font-size: 24px;
    }}
    section {{
      padding: 18px;
      margin-top: 18px;
    }}
    h2 {{
      margin: 0 0 12px;
      font-size: 18px;
    }}
    table {{
      width: 100%;
      border-collapse: collapse;
    }}
    th, td {{
      padding: 10px 8px;
      border-bottom: 1px solid var(--line);
      text-align: left;
    }}
    th {{
      color: var(--muted);
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.06em;
    }}
    tr:last-child td {{ border-bottom: 0; }}
    .ok {{ color: var(--accent); }}
    .err {{ color: var(--danger); }}
    .charts {{
      display: grid;
      grid-template-columns: minmax(0, 1.25fr) minmax(280px, 0.75fr);
      gap: 18px;
      margin-top: 18px;
    }}
    .chart {{
      background: linear-gradient(180deg, #ffffff 0%, #fbfcff 100%);
      border: 1px solid var(--line);
      border-radius: 8px;
      padding: 18px;
      box-shadow: 0 1px 2px rgba(23, 32, 51, 0.04);
    }}
    .chart h2 {{
      margin-bottom: 16px;
    }}
    .bar-row {{
      display: grid;
      grid-template-columns: 52px minmax(90px, 1fr) 84px;
      align-items: center;
      gap: 12px;
      margin: 10px 0;
    }}
    .bar-label, .bar-value {{
      color: var(--muted);
      font-variant-numeric: tabular-nums;
    }}
    .bar-value {{
      text-align: right;
    }}
    .bar-track {{
      height: 12px;
      overflow: hidden;
      background: #e9eef7;
      border-radius: 999px;
    }}
    .bar-fill {{
      display: block;
      height: 100%;
      min-width: 2px;
      border-radius: inherit;
      background: linear-gradient(90deg, var(--accent), var(--blue));
    }}
    .status .bar-fill {{
      background: linear-gradient(90deg, var(--amber), var(--danger));
    }}
    .donut-wrap {{
      display: grid;
      grid-template-columns: 132px minmax(0, 1fr);
      gap: 18px;
      align-items: center;
    }}
    .donut {{
      width: 132px;
      height: 132px;
      border-radius: 50%;
      background: conic-gradient(var(--accent) 0 {success_rate:.3}%, var(--danger) {success_rate:.3}% 100%);
      position: relative;
    }}
    .donut::after {{
      content: "";
      position: absolute;
      inset: 18px;
      background: var(--panel);
      border-radius: 50%;
      border: 1px solid var(--line);
    }}
    .legend-row {{
      display: flex;
      justify-content: space-between;
      gap: 12px;
      padding: 7px 0;
      border-bottom: 1px solid var(--line);
      font-variant-numeric: tabular-nums;
    }}
    .legend-row:last-child {{
      border-bottom: 0;
    }}
    .swatch {{
      display: inline-block;
      width: 10px;
      height: 10px;
      border-radius: 50%;
      margin-right: 8px;
    }}
    .swatch.ok-bg {{ background: var(--accent); }}
    .swatch.err-bg {{ background: var(--danger); }}
    @media (max-width: 820px) {{
      .charts {{
        grid-template-columns: 1fr;
      }}
      .donut-wrap {{
        grid-template-columns: 1fr;
      }}
    }}
  </style>
</head>
<body>
  <main>
    <header>
      <h1>perftest report</h1>
      <div class="target">{method} {url}</div>
    </header>

    <div class="grid">
      <div class="metric"><span>Total requests</span><strong>{total}</strong></div>
      <div class="metric"><span>Throughput</span><strong>{rps:.2} req/s</strong></div>
      <div class="metric"><span>Duration</span><strong>{duration:.3}s</strong></div>
      <div class="metric"><span>Transfer</span><strong>{transfer}</strong></div>
      <div class="metric"><span>Success</span><strong class="ok">{success_rate:.2}%</strong></div>
      <div class="metric"><span>Errors</span><strong class="err">{error_rate:.2}%</strong></div>
    </div>

    <div class="charts">
      <section class="chart">
        <h2>Latency Profile</h2>
        {latency_chart}
      </section>

      <section class="chart">
        <h2>Request Outcomes</h2>
        <div class="donut-wrap">
          <div class="donut" role="img" aria-label="Success {success_rate:.2} percent, errors {error_rate:.2} percent"></div>
          {outcome_chart}
        </div>
      </section>
    </div>

    <section class="chart status">
      <h2>Status Distribution</h2>
      {status_chart}
    </section>

    <section>
      <h2>Run Summary</h2>
      <table>
        <tr><th>Field</th><th>Value</th></tr>
        <tr><td>URL</td><td>{url}</td></tr>
        <tr><td>Method</td><td>{method}</td></tr>
        <tr><td>Concurrency</td><td>{concurrency}</td></tr>
        <tr><td>OK requests</td><td>{ok}</td></tr>
        <tr><td>Error requests</td><td>{errors}</td></tr>
      </table>
    </section>

    <section>
      <h2>Latency</h2>
      <table>
        <tr><th>Percentile</th><th>Milliseconds</th></tr>
        <tr><td>min</td><td>{lat_min:.3}</td></tr>
        <tr><td>avg</td><td>{lat_avg:.3}</td></tr>
        <tr><td>p50</td><td>{lat_p50:.3}</td></tr>
        <tr><td>p90</td><td>{lat_p90:.3}</td></tr>
        <tr><td>p95</td><td>{lat_p95:.3}</td></tr>
        <tr><td>p99</td><td>{lat_p99:.3}</td></tr>
        <tr><td>max</td><td>{lat_max:.3}</td></tr>
      </table>
    </section>

    <section>
      <h2>Status Codes</h2>
      <table>
        <tr><th>Status</th><th>Count</th></tr>
        {status_rows}
      </table>
    </section>
  </main>
</body>
</html>
"#,
        method = html_escape(&s.method),
        url = html_escape(&s.url),
        total = total,
        rps = s.requests_per_sec,
        duration = s.wall_time.as_secs_f64(),
        transfer = format_bytes(s.bytes_read),
        success_rate = success_rate,
        error_rate = error_rate,
        concurrency = s.concurrency,
        ok = s.ok_count,
        errors = s.error_count,
        lat_min = s.latency_min_ms,
        lat_avg = s.latency_avg_ms,
        lat_p50 = s.latency_p50_ms,
        lat_p90 = s.latency_p90_ms,
        lat_p95 = s.latency_p95_ms,
        lat_p99 = s.latency_p99_ms,
        lat_max = s.latency_max_ms,
        status_rows = status_rows,
        latency_chart = latency_chart,
        outcome_chart = outcome_chart,
        status_chart = status_chart
    )
}

fn latency_chart(s: &Summary) -> String {
    let points = [
        ("min", s.latency_min_ms),
        ("avg", s.latency_avg_ms),
        ("p50", s.latency_p50_ms),
        ("p90", s.latency_p90_ms),
        ("p95", s.latency_p95_ms),
        ("p99", s.latency_p99_ms),
        ("max", s.latency_max_ms),
    ];
    let max = points.iter().map(|(_, value)| *value).fold(0.0, f64::max);

    points
        .iter()
        .map(|(label, value)| {
            let width = percent_of(*value, max);
            format!(
                r#"<div class="bar-row"><div class="bar-label">{label}</div><div class="bar-track"><span class="bar-fill" style="width: {width:.3}%"></span></div><div class="bar-value">{value:.3}</div></div>"#
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn outcome_chart(ok: u64, errors: u64) -> String {
    format!(
        r#"<div>
  <div class="legend-row"><span><span class="swatch ok-bg"></span>OK</span><strong>{ok}</strong></div>
  <div class="legend-row"><span><span class="swatch err-bg"></span>Errors</span><strong>{errors}</strong></div>
</div>"#
    )
}

fn status_chart(status_counts: &[(u16, u64)]) -> String {
    if status_counts.is_empty() {
        return String::from("<p class=\"target\">No HTTP status codes recorded.</p>");
    }

    let max = status_counts
        .iter()
        .map(|(_, count)| *count)
        .max()
        .unwrap_or(0);
    status_counts
        .iter()
        .map(|(code, count)| {
            let width = percent_of(*count as f64, max as f64);
            format!(
                r#"<div class="bar-row"><div class="bar-label">{code}</div><div class="bar-track"><span class="bar-fill" style="width: {width:.3}%"></span></div><div class="bar-value">{count}</div></div>"#
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn percent_of(value: f64, max: f64) -> f64 {
    if max <= 0.0 {
        0.0
    } else {
        (value * 100.0 / max).clamp(0.0, 100.0)
    }
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

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn html_report_escapes_summary_fields() {
        let summary = Summary {
            url: "https://example.com/?q=<script>".to_string(),
            method: "GET".to_string(),
            concurrency: 1,
            wall_time: Duration::from_secs(1),
            ok_count: 1,
            error_count: 0,
            status_counts: vec![(200, 1)],
            bytes_read: 42,
            latency_min_ms: 1.0,
            latency_avg_ms: 1.0,
            latency_max_ms: 1.0,
            latency_p50_ms: 1.0,
            latency_p90_ms: 1.0,
            latency_p95_ms: 1.0,
            latency_p99_ms: 1.0,
            requests_per_sec: 1.0,
        };

        let html = render_html(&summary);

        assert!(html.contains("https://example.com/?q=&lt;script&gt;"));
        assert!(html.contains("Latency Profile"));
        assert!(html.contains("Request Outcomes"));
        assert!(html.contains("Status Distribution"));
        assert!(!html.contains("<script>"));
    }
}
