//! Live OSV advisory fetch for `mollify supply-chain`. Network I/O lives here so
//! `mollify-core` stays pure, offline, and deterministic. Best-effort: callers
//! fall back to the local advisory DB on any error.
//!
//! Uses OSV's `/v1/query` endpoint, which returns the vulnerabilities affecting
//! a specific `(package, version)` — so OSV does the version matching and we
//! pin each resulting advisory to the exact queried version.

use mollify_core::supplychain::{Advisory, PinnedDep};
use serde_json::Value;
use std::time::Duration;

const OSV_QUERYBATCH: &str = "https://api.osv.dev/v1/querybatch";
const OSV_VULN: &str = "https://api.osv.dev/v1/vulns/";

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
    // Distinct (name, version) pins, order preserved for result alignment.
    let mut seen = std::collections::HashSet::new();
    let distinct: Vec<&PinnedDep> = pins
        .iter()
        .filter(|p| seen.insert((p.name.clone(), p.version.clone())))
        .collect();

    // Batched discovery returns vuln IDs per query (aligned order). Large
    // result sets are paginated *per query* via `next_page_token`, so keep
    // re-querying the truncated entries until every page is merged (capped so
    // a pathological feed can't loop forever).
    let mut vuln_ids: Vec<Vec<String>> = vec![Vec::new(); distinct.len()];
    let mut pending: Vec<(usize, Option<String>)> =
        (0..distinct.len()).map(|i| (i, None)).collect();
    for _ in 0..MAX_BATCH_PAGES {
        if pending.is_empty() {
            break;
        }
        let queries: Vec<Value> = pending
            .iter()
            .map(|(i, token)| {
                let p = distinct[*i];
                let mut q = serde_json::json!({
                    "package": { "name": p.name, "ecosystem": "PyPI" },
                    "version": p.version,
                });
                if let Some(t) = token {
                    q["page_token"] = Value::String(t.clone());
                }
                q
            })
            .collect();
        let resp = agent
            .post(OSV_QUERYBATCH)
            .send_json(serde_json::json!({ "queries": queries }))?;
        let val: Value = resp.into_json()?;
        pending = merge_batch_page(&pending, &val, &mut vuln_ids);
    }

    // Fetch each unique advisory's details once (batch gives only IDs).
    let mut detail_cache: std::collections::HashMap<String, (String, Vec<String>)> =
        std::collections::HashMap::new();
    let mut out = Vec::new();
    for (pin, ids) in distinct.iter().zip(vuln_ids.iter()) {
        for id in ids {
            let (summary, aliases) = detail_cache
                .entry(id.clone())
                .or_insert_with(|| fetch_vuln_detail(&agent, id))
                .clone();
            out.push(Advisory {
                id: id.clone(),
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

/// Upper bound on querybatch pagination rounds (OSV pages hold 1000 vulns, so
/// 20 pages ≫ any real dependency's advisory count).
const MAX_BATCH_PAGES: usize = 20;

/// Merge one querybatch response into `vuln_ids` (per-query id lists, aligned
/// with the distinct pins). `pending` maps the response's result order back to
/// query indices; the return value is the queries that still have more pages,
/// as `(query index, page token)`.
fn merge_batch_page(
    pending: &[(usize, Option<String>)],
    response: &Value,
    vuln_ids: &mut [Vec<String>],
) -> Vec<(usize, Option<String>)> {
    let results = response
        .get("results")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let mut next = Vec::new();
    for ((idx, _), result) in pending.iter().zip(results.iter()) {
        if let Some(vulns) = result.get("vulns").and_then(|v| v.as_array()) {
            for v in vulns {
                let Some(id) = v.get("id").and_then(|x| x.as_str()) else {
                    continue;
                };
                if !vuln_ids[*idx].iter().any(|e| e == id) {
                    vuln_ids[*idx].push(id.to_string());
                }
            }
        }
        if let Some(tok) = result
            .get("next_page_token")
            .and_then(|t| t.as_str())
            .filter(|t| !t.is_empty())
        {
            next.push((*idx, Some(tok.to_string())));
        }
    }
    next
}

/// Fetch one OSV advisory's summary (first line) and CVE aliases by id.
/// Best-effort: returns empty strings on any failure.
fn fetch_vuln_detail(agent: &ureq::Agent, id: &str) -> (String, Vec<String>) {
    let url = format!("{OSV_VULN}{id}");
    let Ok(resp) = agent.get(&url).call() else {
        return (String::new(), Vec::new());
    };
    let Ok(v) = resp.into_json::<Value>() else {
        return (String::new(), Vec::new());
    };
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
    (summary, aliases)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_batch_page_collects_ids_and_reports_next_pages() {
        let pending = vec![(0usize, None), (1usize, None)];
        let mut vuln_ids: Vec<Vec<String>> = vec![Vec::new(), Vec::new()];
        let page1 = serde_json::json!({ "results": [
            { "vulns": [ { "id": "GHSA-a" }, { "id": "GHSA-b" } ], "next_page_token": "tok-1" },
            { "vulns": [ { "id": "GHSA-c" } ] },
        ]});
        let next = merge_batch_page(&pending, &page1, &mut vuln_ids);
        assert_eq!(vuln_ids[0], vec!["GHSA-a", "GHSA-b"]);
        assert_eq!(vuln_ids[1], vec!["GHSA-c"]);
        assert_eq!(next, vec![(0, Some("tok-1".to_string()))]);

        // The follow-up page merges into the *same* query's list (deduped)
        // and terminates when no token comes back.
        let page2 = serde_json::json!({ "results": [
            { "vulns": [ { "id": "GHSA-b" }, { "id": "GHSA-d" } ] },
        ]});
        let next = merge_batch_page(&next, &page2, &mut vuln_ids);
        assert_eq!(vuln_ids[0], vec!["GHSA-a", "GHSA-b", "GHSA-d"]);
        assert_eq!(vuln_ids[1], vec!["GHSA-c"]);
        assert!(next.is_empty());
    }

    #[test]
    fn merge_batch_page_ignores_empty_tokens_and_missing_results() {
        let pending = vec![(0usize, None)];
        let mut vuln_ids: Vec<Vec<String>> = vec![Vec::new()];
        let page = serde_json::json!({ "results": [
            { "vulns": [ { "id": "GHSA-x" } ], "next_page_token": "" },
        ]});
        assert!(merge_batch_page(&pending, &page, &mut vuln_ids).is_empty());
        assert!(merge_batch_page(&pending, &serde_json::json!({}), &mut vuln_ids).is_empty());
        assert_eq!(vuln_ids[0], vec!["GHSA-x"]);
    }
}
