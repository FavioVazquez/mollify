//! Live OSV advisory fetch for `mollify supply-chain`. Network I/O lives here so
//! `mollify-core` stays pure, offline, and deterministic. Best-effort: callers
//! fall back to the local advisory DB on any error.
//!
//! Uses OSV's `/v1/query` endpoint, which returns the vulnerabilities affecting
//! a specific `(package, version)` — so OSV does the version matching and we
//! pin each resulting advisory to the exact queried version.

use mollify_core::supplychain::{Advisory, PinnedDep};
use std::time::Duration;

const OSV_QUERY: &str = "https://api.osv.dev/v1/query";

fn build_agent() -> ureq::Agent {
    let mut b = ureq::AgentBuilder::new().timeout(Duration::from_secs(20));
    // Honor a corporate/CI proxy if one is configured.
    if let Ok(p) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("https_proxy")) {
        if !p.is_empty() {
            if let Ok(proxy) = ureq::Proxy::new(&p) {
                b = b.proxy(proxy);
            }
        }
    }
    b.build()
}

/// Query OSV for each distinct `(package, version)` pin and return advisories.
/// Each advisory's spec pins the exact queried version, so the offline matcher
/// in `mollify-core` reproduces the same result deterministically.
pub fn fetch_for_pins(pins: &[PinnedDep]) -> anyhow::Result<Vec<Advisory>> {
    if pins.is_empty() {
        return Ok(Vec::new());
    }
    let agent = build_agent();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for pin in pins {
        if !seen.insert((pin.name.clone(), pin.version.clone())) {
            continue;
        }
        let body = serde_json::json!({
            "package": { "name": pin.name, "ecosystem": "PyPI" },
            "version": pin.version,
        });
        let resp = agent.post(OSV_QUERY).send_json(body)?;
        let val: serde_json::Value = resp.into_json()?;
        let Some(vulns) = val.get("vulns").and_then(|v| v.as_array()) else {
            continue;
        };
        for v in vulns {
            let id = v
                .get("id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                continue;
            }
            let summary = v
                .get("summary")
                .and_then(|x| x.as_str())
                .or_else(|| v.get("details").and_then(|x| x.as_str()))
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(200)
                .collect::<String>();
            let aliases = v
                .get("aliases")
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .filter(|s| s.starts_with("CVE-"))
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
            out.push(Advisory {
                id,
                package: pin.name.clone(),
                specs: vec![format!("=={}", pin.version)],
                summary,
                aliases,
                severity: None,
            });
        }
    }
    Ok(out)
}

/// Serialize advisories into the `mollify-advisories/1` schema and write to
/// `path` (used by `--refresh` to cache the live feed for later offline runs).
pub fn write_db(path: &camino::Utf8Path, advisories: &[Advisory]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let db = serde_json::json!({
        "schema": "mollify-advisories/1",
        "source": "osv.dev /v1/query (live)",
        "advisories": advisories,
    });
    std::fs::write(path, serde_json::to_string_pretty(&db).unwrap())
}
