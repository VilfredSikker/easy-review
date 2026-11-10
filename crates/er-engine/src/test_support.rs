//! Thread-scoped environment overrides for tests.
//!
//! `set_var` rewrites the process environment while parallel tests read it,
//! which is a data race. A test that needs an isolated value overrides it for
//! its own thread instead, and the production reader consults [`var`].

use std::cell::RefCell;
use std::collections::HashMap;

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
