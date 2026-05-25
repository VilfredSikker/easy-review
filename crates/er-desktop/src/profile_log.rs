//! Opt-in idle/CPU profiling: `ER_DESKTOP_PROFILE_POLL=1`.
//! Logs to stderr with wall `ts_ms` and per-`kind` `since_last_ms` for cadence analysis.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::Hasher;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static LAST_BY_KIND: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
static LAST_REV_BUMP_BY_SOURCE: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

pub fn profile_enabled() -> bool {
    std::env::var("ER_DESKTOP_PROFILE_POLL").as_deref() == Ok("1")
}

fn wall_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn since_last_ms(kind: &str) -> u64 {
    let map = LAST_BY_KIND.get_or_init(|| Mutex::new(HashMap::new()));
    let now = Instant::now();
    let mut g = map.lock().expect("profile_log mutex");
    let since = g
        .get(kind)
        .map(|t| now.duration_since(*t).as_millis() as u64)
        .unwrap_or(0);
    g.insert(kind.to_string(), now);
    since
}

/// Single-line stderr log: `er-desktop kind=… ts_ms=… since_last_ms=… key=value …`
pub fn profile_log(kind: &str, fields: &[(&str, String)]) {
    if !profile_enabled() {
        return;
    }
    let since = since_last_ms(kind);
    let ts = wall_ms();
    let mut parts = vec![
        format!("kind={kind}"),
        format!("ts_ms={ts}"),
        format!("since_last_ms={since}"),
    ];
    for (k, v) in fields {
        parts.push(format!("{k}={v}"));
    }
    eprintln!("er-desktop {}", parts.join(" "));
}

/// Bump desktop revision and log `rev_bump` (throttled to one line per source per 200ms).
pub fn bump_desktop_revision(counter: &AtomicU64, source: &'static str) -> u64 {
    let new = counter.fetch_add(1, Ordering::Relaxed) + 1;
    if !profile_enabled() {
        return new;
    }
    let map = LAST_REV_BUMP_BY_SOURCE.get_or_init(|| Mutex::new(HashMap::new()));
    let now = Instant::now();
    let mut g = map.lock().expect("profile_log rev_bump mutex");
    let should_log = g
        .get(source)
        .map(|t| now.duration_since(*t).as_millis() >= 200)
        .unwrap_or(true);
    if should_log {
        g.insert(source.to_string(), now);
        profile_log(
            "rev_bump",
            &[
                ("source", source.to_string()),
                ("desktop_rev", new.to_string()),
            ],
        );
    }
    new
}

/// Combine hashed parts into a single fingerprint (caller feeds fields into `h`).
pub fn finish_hash(h: DefaultHasher) -> u64 {
    h.finish()
}
