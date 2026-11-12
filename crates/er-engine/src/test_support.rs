//! Test-only helpers shared across the engine's test modules.
//!
//! `set_var` rewrites the process environment while parallel tests read it,
//! which is a data race. A test that needs an isolated value overrides it for
//! its own thread instead, and the production reader consults [`var`].

use std::cell::RefCell;
use std::collections::HashMap;
#[cfg(unix)]
use std::io::Write;
use std::path::Path;
#[cfg(unix)]
use std::process::{Command, Stdio};

thread_local! {
    static OVERRIDES: RefCell<HashMap<String, Option<String>>> = RefCell::new(HashMap::new());
}

enum Previous {
    NoOverride,
    Value(Option<String>),
}

/// Run `f` with `name` set to `value` on this thread.
pub fn with_var<R>(name: &str, value: impl Into<String>, f: impl FnOnce() -> R) -> R {
    with_override(name, Some(value.into()), f)
}

/// Run `f` with `name` unset on this thread, even when the process environment
/// has a value.
pub fn with_var_unset<R>(name: &str, f: impl FnOnce() -> R) -> R {
    with_override(name, None, f)
}

fn with_override<R>(name: &str, value: Option<String>, f: impl FnOnce() -> R) -> R {
    struct Restore {
        name: String,
        previous: Previous,
    }

    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = std::mem::replace(&mut self.previous, Previous::NoOverride);
            OVERRIDES.with(|overrides| {
                let mut overrides = overrides.borrow_mut();
                match previous {
                    Previous::NoOverride => {
                        overrides.remove(&self.name);
                    }
                    Previous::Value(value) => {
                        overrides.insert(self.name.clone(), value);
                    }
                }
            });
        }
    }

    let previous =
        OVERRIDES.with(
            |overrides| match overrides.borrow_mut().insert(name.to_string(), value) {
                Some(value) => Previous::Value(value),
                None => Previous::NoOverride,
            },
        );
    let _restore = Restore {
        name: name.to_string(),
        previous,
    };
    f()
}

/// The variable's value: this thread's override when present, else the process environment.
pub fn var(name: &str) -> Option<String> {
    match explicit_var(name) {
        Some(value) => value,
        None => std::env::var(name).ok(),
    }
}

/// This thread's override for `name`, without falling back to the process
/// environment. `Some(None)` means the test explicitly unset it.
pub fn explicit_var(name: &str) -> Option<Option<String>> {
    OVERRIDES.with(|overrides| overrides.borrow().get(name).cloned())
}

/// Write `body` as an executable script at `path`.
///
/// The bytes go through a child `sh`, so this process never holds a writable
/// descriptor for the script. On Linux the kernel refuses to exec a file while
/// any process holds it open for writing (ETXTBSY); a script written in-process
/// can be caught by a fork on another test thread before that child execs. The
/// child writer owns every write descriptor and is reaped before this returns,
/// so a later exec cannot race a concurrent fork.
#[cfg(unix)]
/// Rewrite a fake provider's script so its output follows the job's staging
/// directory: `'__BUCKET__/x'` becomes `"${ER_JOB_STAGING:-__BUCKET__}/x"`,
/// with the fallback replaced by `bucket`.
///
/// Jobs stage their output, and the fake provider runs as a child that is told
/// the directory through `ER_JOB_STAGING`. A script that writes the bucket
/// path directly is exactly what the host no longer commits, so tests must
/// follow the env var.
#[expect(
    clippy::string_slice,
    reason = "every index comes from an ASCII needle match, so each is a char boundary"
)]
pub fn staged_script(script: &str, bucket: &str) -> String {
    let token = "__BUCKET__";
    let mut out = String::new();
    let mut rest = script;
    while let Some(start) = rest.find("'__BUCKET__") {
        let (before, tail) = rest.split_at(start);
        out.push_str(before);
        let Some(relative_end) = tail[1..].find('\'') else {
            break;
        };
        let end = 1 + relative_end;
        let word = &tail[1..end];
        let suffix = &word[token.len()..];
        out.push_str(&format!("\"${{ER_JOB_STAGING:-{token}}}{suffix}\""));
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out.replace(token, bucket)
}

/// Shell snippet: make `$out` the job's staging dir and `$h` the diff hash,
/// creating a diff when the job was not given a prepared one. Every fake
/// provider that writes a sidecar starts with this.
pub const STAGING_PREAMBLE: &str = r#"out="${ER_JOB_STAGING:?}"; [ -f "$out/diff-tmp" ] || printf 'test diff\n' > "$out/diff-tmp"; h=$(sha256sum "$out/diff-tmp" 2>/dev/null || shasum -a 256 "$out/diff-tmp"); h=${h%% *}"#;

/// Shell snippet: write a complete, hash-matching review sidecar set.
pub const WRITE_REVIEW_SIDECARS: &str = r#"printf '{"version":1,"diff_hash":"%s","files":{}}' "$h" > "$out/review.json"; printf '{"version":1,"diff_hash":"%s","order":[],"groups":{}}' "$h" > "$out/order.json"; printf '{"version":1,"diff_hash":"%s","items":[]}' "$h" > "$out/checklist.json"; printf 'summary\n' > "$out/summary.md""#;

/// Shell snippet: write a complete, hash-matching triage sidecar.
pub const WRITE_TRIAGE_SIDECAR: &str = r#"printf '{"version":1,"diff_hash":"%s","verdict":{"primary":"arena","experts":[]}}' "$h" > "$out/triage.json""#;

pub fn write_executable_script(path: &Path, body: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut child = Command::new("sh")
        .arg("-c")
        .arg("cat > \"$1\" && chmod 755 \"$1\"")
        .arg("sh")
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()?;
    let written = child
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(body.as_bytes()));
    let status = child.wait()?;
    written.transpose()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "writing {} failed: {status}",
            path.display()
        )));
    }
    Ok(())
}

/// Windows has no ETXTBSY and no execute bit, so a plain write is enough.
#[cfg(not(unix))]
pub fn write_executable_script(path: &Path, body: &str) -> std::io::Result<()> {
    std::fs::write(path, body)
}

// ── Store fixtures ──

/// Write one bucket artifact the way the host does, for a test fixture.
///
/// The write goes through `er_store` with the bucket as its scope, so a
/// fixture follows the kind's storage mode now and its envelope when P30b.2
/// deletes the bare mode. A fixture that must stay unreadable (a malformed
/// file, a bare payload proving rejection) keeps its raw write in the test.
pub fn write_sidecar<T: er_store::Artifact<Slot = ()>>(dir: &Path, value: &T) {
    write_sidecar_slot(dir, (), value);
}

/// [`write_sidecar`] for a kind with a slot, such as `experts/<id>.json`.
pub fn write_sidecar_slot<T: er_store::Artifact>(dir: &Path, slot: T::Slot, value: &T) {
    let store = er_store::ArtifactStore::new(dir);
    let scope = match er_store::Scope::dir(dir) {
        Ok(scope) => scope,
        Err(error) => panic!("test fixture bucket: {error}"),
    };
    let key = match er_store::ArtifactKey::<T>::new(scope, slot) {
        Ok(key) => key,
        Err(error) => panic!("test fixture key: {error}"),
    };
    store
        .write(&key, value)
        .unwrap_or_else(|error| panic!("test fixture write: {error}"));
}

/// [`write_sidecar`] for a fixture still held as `json!`. It is parsed into the
/// kind first, so a fixture that drifts from the type fails here.
pub fn write_sidecar_json<T: er_store::Artifact<Slot = ()>>(
    dir: &Path,
    payload: serde_json::Value,
) {
    write_sidecar(dir, &typed_fixture::<T>(payload));
}

/// [`write_sidecar_slot`] for a fixture still held as `json!`.
pub fn write_sidecar_json_slot<T: er_store::Artifact>(
    dir: &Path,
    slot: T::Slot,
    payload: serde_json::Value,
) {
    write_sidecar_slot(dir, slot, &typed_fixture::<T>(payload));
}

fn typed_fixture<T: er_store::Artifact>(payload: serde_json::Value) -> T {
    serde_json::from_value(payload).unwrap_or_else(|error| panic!("test fixture payload: {error}"))
}

/// Read one bucket artifact the way the host does, for a test assertion.
///
/// Use this where a test raw-parsed a fixture it wrote; the bytes are the
/// kind's stored form, which is a payload today and an envelope after P30b.2.
pub fn read_sidecar<T: er_store::Artifact<Slot = ()>>(dir: &Path) -> T {
    read_sidecar_slot(dir, ())
}

/// [`read_sidecar`] for a kind with a slot.
pub fn read_sidecar_slot<T: er_store::Artifact>(dir: &Path, slot: T::Slot) -> T {
    let store = er_store::ArtifactStore::new(dir);
    let scope = match er_store::Scope::dir(dir) {
        Ok(scope) => scope,
        Err(error) => panic!("test fixture bucket: {error}"),
    };
    let key = match er_store::ArtifactKey::<T>::new(scope, slot) {
        Ok(key) => key,
        Err(error) => panic!("test fixture key: {error}"),
    };
    match store.read(&key) {
        Ok(er_store::Loaded::Found(stored)) => stored.value,
        Ok(er_store::Loaded::Absent) => panic!("expected a stored fixture, found none"),
        Ok(er_store::Loaded::Unreadable(reason)) => {
            panic!("expected a stored fixture, read as unreadable: {reason:?}")
        }
        Err(error) => panic!("test fixture read: {error}"),
    }
}
