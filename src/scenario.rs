//! Scenario model: a weighted set of HTTP requests compiled into a
//! runtime-ready table that the runner samples from on each iteration.
//!
//! The single-URL CLI path is expressed as a one-step scenario via
//! [`Scenario::single`], so the runner only ever deals with a `Scenario`.

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use reqwest::Method;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Raw on-disk shape of a scenario TOML file.
#[derive(Debug, Deserialize)]
struct ScenarioFile {
    name: Option<String>,
    defaults: Option<Defaults>,
    #[serde(default, rename = "request")]
    requests: Vec<RawRequest>,
}

#[derive(Debug, Default, Deserialize)]
struct Defaults {
    #[serde(default)]
    headers: BTreeMap<String, String>,
    timeout: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawRequest {
    name: Option<String>,
    #[serde(default = "default_method")]
    method: String,
    url: String,
    weight: Option<u32>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    body: Option<String>,
    body_file: Option<String>,
    timeout: Option<String>,
}

fn default_method() -> String {
    "GET".to_string()
}

/// A single compiled request, ready to be issued with no further parsing.
#[derive(Debug, Clone)]
pub struct Step {
    // Read by per-step reporting in a later PR.
    #[allow(dead_code)]
    pub name: String,
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Bytes>,
    pub timeout: Option<Duration>,
}

/// A weighted collection of [`Step`]s sampled per request.
#[derive(Debug)]
pub struct Scenario {
    pub name: String,
    pub steps: Vec<Step>,
    dist: WeightedIndex<u32>,
}

impl Scenario {
    /// Pick the next step according to configured weights.
    #[inline]
    pub fn pick(&self, rng: &mut impl Rng) -> &Step {
        &self.steps[self.dist.sample(rng)]
    }

    /// Build a one-step scenario from the classic single-URL CLI inputs.
    pub fn single(
        url: String,
        method: Method,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    ) -> Self {
        let step = Step {
            name: url.clone(),
            method,
            url,
            headers,
            body: body.map(Bytes::from),
            timeout: None,
        };
        Scenario {
            name: "default".to_string(),
            steps: vec![step],
            dist: WeightedIndex::new([1u32]).expect("single weight is valid"),
        }
    }

    /// Parse and compile a scenario TOML file.
    ///
    /// `cli_headers` are merged in as defaults (below per-request headers but
    /// above file `[defaults]`), so a global `-H "Authorization: ..."` can be
    /// layered onto a checked-in scenario without editing it.
    pub fn from_file(path: &Path, cli_headers: &[(String, String)]) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read scenario file {}", path.display()))?;
        let file: ScenarioFile = toml::from_str(&text)
            .with_context(|| format!("failed to parse scenario file {}", path.display()))?;

        if file.requests.is_empty() {
            bail!("scenario {} has no [[request]] entries", path.display());
        }

        let defaults = file.defaults.unwrap_or_default();
        let default_timeout = match defaults.timeout.as_deref() {
            Some(s) => Some(parse_duration(s).map_err(anyhow::Error::msg)?),
            None => None,
        };
        let base_dir = path.parent();

        let mut steps = Vec::with_capacity(file.requests.len());
        let mut weights = Vec::with_capacity(file.requests.len());

        for (i, r) in file.requests.into_iter().enumerate() {
            let label = format!("request #{}", i + 1);

            if r.body.is_some() && r.body_file.is_some() {
                bail!("{}: {} sets both body and body_file", path.display(), label);
            }

            let method = parse_method(&r.method)
                .with_context(|| format!("{}: {}", path.display(), label))?;

            // Merge order: file defaults -> cli headers -> per-request (wins).
            let mut merged: BTreeMap<String, String> = defaults.headers.clone();
            for (k, v) in cli_headers {
                merged.insert(k.clone(), v.clone());
            }
            for (k, v) in r.headers {
                merged.insert(k, v);
            }

            let body = match (r.body, r.body_file) {
                (Some(b), None) => Some(Bytes::from(b.into_bytes())),
                (None, Some(f)) => {
                    let p = match base_dir {
                        Some(d) => d.join(&f),
                        None => PathBuf::from(&f),
                    };
                    let bytes = std::fs::read(&p).with_context(|| {
                        format!("{}: failed to read body_file {}", label, p.display())
                    })?;
                    Some(Bytes::from(bytes))
                }
                _ => None,
            };

            let timeout = match r.timeout.as_deref() {
                Some(s) => Some(parse_duration(s).map_err(anyhow::Error::msg)?),
                None => default_timeout,
            };

            let weight = r.weight.unwrap_or(1);
            if weight == 0 {
                bail!("{}: {} has weight 0 (must be >= 1)", path.display(), label);
            }

            steps.push(Step {
                name: r.name.unwrap_or_else(|| r.url.clone()),
                method,
                url: r.url,
                headers: merged.into_iter().collect(),
                body,
                timeout,
            });
            weights.push(weight);
        }

        let dist = WeightedIndex::new(&weights).context("invalid request weights")?;

        Ok(Scenario {
            name: file.name.unwrap_or_else(|| path.display().to_string()),
            steps,
            dist,
        })
    }
}

fn parse_method(s: &str) -> Result<Method> {
    match s.trim().to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::GET),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "PATCH" => Ok(Method::PATCH),
        "DELETE" => Ok(Method::DELETE),
        "HEAD" => Ok(Method::HEAD),
        "OPTIONS" => Ok(Method::OPTIONS),
        other => bail!(
            "unsupported method '{other}' (GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS)"
        ),
    }
}

use crate::parse_duration;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(name: &str, contents: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("perftest_scenario_{}_{}.toml", std::process::id(), name));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    #[test]
    fn single_is_one_step() {
        let s = Scenario::single(
            "https://example.com/".to_string(),
            Method::GET,
            vec![("Accept".into(), "*/*".into())],
            None,
        );
        assert_eq!(s.steps.len(), 1);
        assert_eq!(s.steps[0].url, "https://example.com/");
        assert_eq!(s.steps[0].method, Method::GET);
    }

    #[test]
    fn parses_and_merges_defaults() {
        let path = write_tmp(
            "merge",
            r#"
name = "flow"

[defaults]
headers = { "Accept" = "application/json" }
timeout = "5s"

[[request]]
name = "list"
url = "https://shop.test/api/products"
weight = 3

[[request]]
method = "POST"
url = "https://shop.test/api/cart"
headers = { "Content-Type" = "application/json" }
body = "{}"
"#,
        );
        let cli_headers = vec![("Authorization".to_string(), "Bearer t".to_string())];
        let s = Scenario::from_file(&path, &cli_headers).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(s.name, "flow");
        assert_eq!(s.steps.len(), 2);

        let list = &s.steps[0];
        assert_eq!(list.name, "list");
        assert_eq!(list.method, Method::GET);
        assert_eq!(list.timeout, Some(Duration::from_secs(5)));
        // default Accept + cli Authorization present
        assert!(list.headers.iter().any(|(k, v)| k == "Accept" && v == "application/json"));
        assert!(list.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer t"));

        let cart = &s.steps[1];
        assert_eq!(cart.method, Method::POST);
        assert_eq!(cart.name, "https://shop.test/api/cart"); // falls back to url
        assert_eq!(cart.body.as_deref(), Some(&b"{}"[..]));
    }

    #[test]
    fn rejects_empty_scenario() {
        let path = write_tmp("empty", "name = \"x\"\n");
        let err = Scenario::from_file(&path, &[]).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(err.to_string().contains("no [[request]]"));
    }

    #[test]
    fn rejects_body_and_body_file() {
        let path = write_tmp(
            "dup",
            r#"
[[request]]
url = "https://x.test"
body = "a"
body_file = "b.json"
"#,
        );
        let err = Scenario::from_file(&path, &[]).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(err.to_string().contains("body and body_file"));
    }

    #[test]
    fn weighted_sampling_respects_weights() {
        let path = write_tmp(
            "weights",
            r#"
[[request]]
name = "a"
url = "https://a.test"
weight = 9

[[request]]
name = "b"
url = "https://b.test"
weight = 1
"#,
        );
        let s = Scenario::from_file(&path, &[]).unwrap();
        std::fs::remove_file(&path).ok();

        let mut rng = SmallRng::seed_from_u64(42);
        let mut a = 0;
        let n = 10_000;
        for _ in 0..n {
            if s.pick(&mut rng).name == "a" {
                a += 1;
            }
        }
        let ratio = a as f64 / n as f64;
        // Expected ~0.9; allow generous slack for RNG variance.
        assert!(ratio > 0.85 && ratio < 0.95, "ratio was {ratio}");
    }
}
