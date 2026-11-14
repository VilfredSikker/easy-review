//! The desktop's artifact registry: the engine's shared kinds plus the five
//! app records this front end alone writes.
//!
//! The five are desktop-only state (D20), so they stay out of `er-engine` and
//! out of the TUI and MCP compositions.

use er_store::Registry;

/// The desktop-only kinds, as `(file, kind)`.
#[cfg(test)]
pub const DESKTOP_KINDS: &[(&str, &str)] = &[
    ("gh-status-cache.json", "gh-status-cache"),
    ("inbox.json", "inbox"),
    ("pr-cache.json", "pr-cache"),
    ("pr-open-cache.json", "pr-open-cache"),
    ("tabs.json", "tabs"),
];

/// The registry every desktop store read and write goes through.
pub fn registry() -> Registry {
    let mut registry = er_engine::store_registry::registry();
    #[expect(
        clippy::expect_used,
        reason = "every kind is statically registered once; a duplicate fails the register test"
    )]
    {
        register_desktop_kinds(&mut registry).expect("the desktop kinds register once");
    }
    registry
}

fn register_desktop_kinds(registry: &mut Registry) -> Result<(), er_store::StoreError> {
    crate::gh_status_cache::register(registry)?;
    crate::inbox::register(registry)?;
    crate::pr_cache::register(registry)?;
    crate::pr_open_cache::register(registry)?;
    crate::tabs::register(registry)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The desktop composition is the shared table plus its five records, and
    /// each kind resolves to its file.
    #[test]
    fn the_desktop_composition_adds_its_five_records() {
        let registry = registry();
        for (file, kind) in er_engine::store_registry::expected_kinds() {
            assert_eq!(
                registry.kind_for(std::path::Path::new(file)),
                Some(*kind),
                "{file}"
            );
        }
        for (file, kind) in DESKTOP_KINDS {
            assert_eq!(
                registry.kind_for(std::path::Path::new(file)),
                Some(*kind),
                "{file}"
            );
        }
    }

    /// The desktop kinds are not in the shared composition: the TUI and MCP
    /// must not claim them.
    #[test]
    fn the_shared_composition_does_not_claim_desktop_kinds() {
        let shared = er_engine::store_registry::registry();
        for (file, _) in DESKTOP_KINDS {
            assert_eq!(shared.kind_for(std::path::Path::new(file)), None, "{file}");
        }
    }
}
