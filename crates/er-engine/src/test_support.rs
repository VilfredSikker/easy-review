//! Test-only helpers shared across the engine's test modules.
//!
//! `set_var` rewrites the process environment while parallel tests read it,
//! which is a data race. A test that needs an isolated value overrides it for
//! its own thread instead, and the production reader consults [`var`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

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
pub fn write_executable_script(path: &Path, body: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, body)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// Windows has no ETXTBSY and no execute bit, so a plain write is enough.
#[cfg(not(unix))]
pub fn write_executable_script(path: &Path, body: &str) -> std::io::Result<()> {
    std::fs::write(path, body)
}
