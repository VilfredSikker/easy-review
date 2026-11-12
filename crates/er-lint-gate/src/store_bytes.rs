//! `er::store_bytes` — a public byte-writing API on the artifact store.
//!
//! The store writes a kind through its typed value, so the version, envelope
//! and shape rules apply. Recovery needs to put back bytes it read, but a
//! public function taking both a byte buffer and a store selector
//! (`ArtifactKey`, `Scope`, `RelPath`, a snapshot) would let any caller put
//! arbitrary bytes at a registered kind's path. Recovery exposes snapshots and
//! backups instead, so this rule rejects the old shape if it grows back.
//!
//! A byte parameter named `expected` does not count: it is the comparison
//! value of a compare-and-swap, never the written value. Plain paths
//! (`&Path`) do not count either: `er_store::atomic::write_atomic` is the
//! store's low-level writer, and the engine's sidecar-fs deny list is what
//! keeps callers from aiming it at an artifact. Test code (`#[cfg(test)]` and
//! `#[test]`) is exempt.

use std::collections::BTreeMap;

use syn::visit::{self, Visit};
use syn::{Item, Pat, Signature};

use crate::attrs::{impl_item_attrs, is_test_item, item_attrs};
use crate::diagnostics::Diagnostic;

/// The code the gate reports these findings under.
pub const CODE: &str = "er::store_bytes";

/// Only files below this path are scanned.
pub const HOME: &str = "crates/er-store/src/";

const SELECTORS: [&str; 6] = [
    "ArtifactKey",
    "Scope",
    "RelPath",
    "BucketDir",
    "ArtifactSnapshot",
    "BackupSnapshot",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    pub message: String,
}

struct ByteFinder {
    findings: Vec<Finding>,
}

impl ByteFinder {
    fn check(&mut self, vis: &syn::Visibility, sig: &Signature) {
        if matches!(vis, syn::Visibility::Public(_)) && risky(sig) {
            self.findings.push(Finding {
                line: sig.ident.span().start().line,
                message: format!(
                    "`{}` takes raw bytes with a store path; recovery writes snapshots",
                    sig.ident
                ),
            });
        }
    }
}

impl<'ast> Visit<'ast> for ByteFinder {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_attrs(item).is_some_and(is_test_item) {
            return;
        }
        match item {
            Item::Fn(item) => self.check(&item.vis, &item.sig),
            Item::Impl(item) => {
                for member in &item.items {
                    if let syn::ImplItem::Fn(method) = member {
                        if is_test_item(impl_item_attrs(member)) {
                            continue;
                        }
                        self.check(&method.vis, &method.sig);
                    }
                }
            }
            _ => {}
        }
        visit::visit_item(self, item);
    }
}

/// A signature that pairs a written byte buffer with a way to name a store
/// path. `expected` is the compare-and-swap comparison value, not a write.
fn risky(sig: &Signature) -> bool {
    let mut bytes = false;
    let mut selector = false;
    for input in &sig.inputs {
        let syn::FnArg::Typed(parameter) = input else {
            continue;
        };
        let compared = matches!(
            parameter.pat.as_ref(),
            Pat::Ident(ident) if ident.ident == "expected"
        );
        if !compared && mentions(&parameter.ty, "u8") {
            bytes = true;
        }
        if SELECTORS.iter().any(|name| mentions(&parameter.ty, name)) {
            selector = true;
        }
    }
    bytes && selector
}

struct TypeNames {
    names: Vec<String>,
}

impl<'ast> Visit<'ast> for TypeNames {
    fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
        if let Some(segment) = path.path.segments.last() {
            self.names.push(segment.ident.to_string());
        }
        visit::visit_type_path(self, path);
    }
}

fn mentions(ty: &syn::Type, name: &str) -> bool {
    let mut names = TypeNames { names: Vec::new() };
    names.visit_type(ty);
    names.names.iter().any(|found| found == name)
}

/// Findings for one source file. A file that does not parse yields none.
pub fn findings(source: &str) -> Vec<Finding> {
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let mut finder = ByteFinder {
        findings: Vec::new(),
    };
    finder.visit_file(&file);
    finder.findings
}

/// Turn findings into gate diagnostics. The caller passes only `er-store`
/// files, so every finding is an error.
pub fn evaluate(findings_by_file: &BTreeMap<String, Vec<Finding>>, code: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (file, findings) in findings_by_file {
        for finding in findings {
            out.push(Diagnostic {
                level: "error".into(),
                code: Some(code.into()),
                file: Some(file.clone()),
                line: finding.line,
                column: 1,
                rendered: format!(
                    "error[{code}]: {}\n  --> {file}:{}\n\n",
                    finding.message, finding.line
                ),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_public_function_taking_a_key_and_bytes_is_flagged() {
        let found = findings(
            "pub fn write_bytes<T>(key: &ArtifactKey<T>, bytes: &[u8]) { let _ = (key, bytes); }",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].message.contains("write_bytes"), "{found:?}");
    }

    #[test]
    fn a_public_method_is_flagged() {
        let found = findings(
            "impl ArtifactStore { pub fn write_bytes(&self, key: &ArtifactKey<T>, bytes: Vec<u8>) {} }",
        );
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn a_private_or_crate_visible_helper_is_fine() {
        for source in [
            "fn write_bytes(key: &ArtifactKey<T>, bytes: &[u8]) {}",
            "pub(crate) fn write_bytes(key: &ArtifactKey<T>, bytes: &[u8]) {}",
            "impl ArtifactStore { fn write_bytes(&self, key: &ArtifactKey<T>, bytes: &[u8]) {} }",
        ] {
            assert!(findings(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn snapshot_and_backup_operations_are_fine() {
        for source in [
            "pub fn snapshot<T: Artifact>(&self, key: &ArtifactKey<T>) -> Result<()> { todo!() }",
            "pub fn restore_backup(&self, backup: &BackupSnapshot) -> Result<()> { todo!() }",
            "pub fn restore_backup_if_unchanged(&self, backup: &BackupSnapshot, expected: &[u8]) -> Result<bool> { todo!() }",
            "pub fn write_typed_if_unchanged<T: Artifact>(&self, expected: &ArtifactSnapshot<T>, value: &T) -> Result<()> { todo!() }",
        ] {
            assert!(findings(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn a_public_function_taking_only_bytes_is_fine() {
        assert!(findings("pub fn parse(bytes: &[u8]) -> Result<()> { todo!() }").is_empty());
    }

    #[test]
    fn the_low_level_atomic_writer_is_not_a_store_selector() {
        assert!(findings(
            "pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> { todo!() }"
        )
        .is_empty());
    }

    #[test]
    fn a_snapshot_with_a_written_byte_buffer_is_flagged() {
        let found = findings(
            "pub fn write_bytes(&self, backup: &BackupSnapshot, bytes: &[u8]) { let _ = (backup, bytes); }",
        );
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn test_items_are_skipped() {
        for source in [
            "#[cfg(test)] mod tests { pub fn write_bytes(key: &ArtifactKey<T>, bytes: &[u8]) {} }",
            "#[test] fn write_bytes(key: &ArtifactKey<T>, bytes: &[u8]) {}",
        ] {
            assert!(findings(source).is_empty(), "{source}");
        }
    }
}
