//! Best-effort "a newer mollify is available" nudge.
//!
//! Mirrors fallow's update check, scoped tightly so it never disrupts machine
//! use:
//! - Shown only on an **interactive human run** (Human format, stdout+stderr are
//!   TTYs, not `--quiet`-equivalent). CI, JSON/SARIF/etc., pipes, and agents
//!   never see it.
//! - Silenced by `MOLLIFY_UPDATE_CHECK=off` (also `DO_NOT_TRACK`) and on CI.
//! - The version fetch runs on a background thread with a bounded grace window,
//!   so it never blocks process exit; every error is swallowed so it can never
//!   change the exit code.
//! - The latest-version answer is cached once per day next to the user cache.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const UPDATE_CHECK_ENV: &str = "MOLLIFY_UPDATE_CHECK";
const DO_NOT_TRACK_ENV: &str = "DO_NOT_TRACK";
const CHECK_TTL_SECS: u64 = 24 * 60 * 60;
const FETCH_GRACE: Duration = Duration::from_millis(250);
const PYPI_JSON_URL: &str = "https://pypi.org/pypi/mollify/json";
const CHANGELOG_URL: &str = "https://github.com/FavioVazquez/mollify/blob/main/CHANGELOG.md";

/// Public entry point. Call once at the end of a successful human run.
pub fn maybe_nudge() {
    if !should_run() {
        return;
    }
    let Some(path) = cache_path() else { return };
    let cache = read_cache(&path).unwrap_or_default();

    let current = env!("CARGO_PKG_VERSION");
    if is_newer(current, &cache.latest_version) {
        eprintln!(
            "A newer mollify is available ({}, you have {current}). Changelog: {CHANGELOG_URL} (silence: {UPDATE_CHECK_ENV}=off)",
            cache.latest_version
        );
    }

    if cache_expired(cache.checked_at_secs) {
        refresh_bounded(path, cache);
    }
}

/// Pure gate: only an interactive, opted-in human run may nudge or fetch.
fn should_run() -> bool {
    !env_disabled() && std::io::stdout().is_terminal() && std::io::stderr().is_terminal()
}

/// Env / CI kill switches.
fn env_disabled() -> bool {
    update_check_off() || env_truthy(DO_NOT_TRACK_ENV) || is_ci()
}

fn update_check_off() -> bool {
    std::env::var(UPDATE_CHECK_ENV).ok().is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "off" | "false" | "no"
        )
    })
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn is_ci() -> bool {
    std::env::var("CI").is_ok_and(|v| !v.is_empty() && v != "0" && v != "false")
        || std::env::var("GITHUB_ACTIONS").is_ok()
}

/// Cache file path: `$XDG_CACHE_HOME/mollify/update-check.json` (or
/// `$HOME/.cache/mollify/...`). `None` if no home is resolvable.
fn cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("mollify").join("update-check.json"))
}

/// Persisted latest-version answer.
#[derive(Default)]
struct UpdateCache {
    latest_version: String,
    checked_at_secs: u64,
}

fn read_cache(path: &PathBuf) -> Option<UpdateCache> {
    let raw = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    Some(UpdateCache {
        latest_version: v
            .get("latest_version")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string(),
        checked_at_secs: v
            .get("checked_at_secs")
            .and_then(|x| x.as_u64())
            .unwrap_or(0),
    })
}

fn write_cache(path: &PathBuf, latest: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body = serde_json::json!({
        "schema_version": 1,
        "latest_version": latest,
        "checked_at_secs": now_secs(),
    });
    let _ = std::fs::write(path, body.to_string());
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_expired(checked_at_secs: u64) -> bool {
    now_secs().saturating_sub(checked_at_secs) >= CHECK_TTL_SECS
}

/// Fetch the latest version on a background thread, waiting at most
/// [`FETCH_GRACE`] before returning. Never blocks exit; errors are swallowed.
fn refresh_bounded(path: PathBuf, cache: UpdateCache) {
    let (tx, rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        if let Some(latest) = fetch_latest_version() {
            if latest != cache.latest_version || cache_expired(cache.checked_at_secs) {
                write_cache(&path, &latest);
            }
        }
        let _ = tx.send(());
    });
    // Bound the wait so a slow endpoint can't delay a sub-second run.
    let _ = rx.recv_timeout(FETCH_GRACE);
}

/// GET the PyPI JSON for `mollify` and return `info.version`. Tight timeouts.
fn fetch_latest_version() -> Option<String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(1))
        .timeout(Duration::from_secs(2))
        .build();
    let resp = agent.get(PYPI_JSON_URL).call().ok()?;
    let json: serde_json::Value = resp.into_json().ok()?;
    json.get("info")?
        .get("version")?
        .as_str()
        .map(|s| s.to_string())
}

/// True when `latest` is a strictly-newer release than `current`. Both must be
/// plain numeric `major.minor.patch` (pre-release / non-numeric → no nudge).
fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().split('.');
    let major = it.next()?.parse::<u64>().ok()?;
    let minor = it.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = it.next().unwrap_or("0").parse::<u64>().ok()?;
    if it.next().is_some() {
        return None; // more than 3 components → not a plain release
    }
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering() {
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.9.9", "1.0.0"));
        assert!(!is_newer("0.2.0", "0.1.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
    }

    #[test]
    fn non_numeric_or_empty_never_nudges() {
        assert!(!is_newer("0.1.0", ""));
        assert!(!is_newer("0.1.0", "0.2.0rc1"));
        assert!(!is_newer("abc", "0.2.0"));
    }

    #[test]
    fn parse_version_rejects_extra_components() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("1.2.3.4"), None);
    }
}
