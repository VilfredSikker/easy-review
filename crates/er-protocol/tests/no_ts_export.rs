//! No wire type exports itself.
//!
//! A `ts` attribute with `export` makes ts-rs add a test that writes the
//! type's TypeScript whenever `cargo test` runs: into `bindings/` under the
//! crate, or into `TS_RS_EXPORT_DIR`. Either way the committed files would
//! depend on which test ran, so export goes only through `protocol-gen`.
//! This file sits outside `src/`, so the scan never reads its own needle.

use std::path::{Path, PathBuf};

#[test]
fn no_type_carries_ts_export() -> std::io::Result<()> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files)?;
    assert!(!files.is_empty(), "no source files under {}", src.display());

    let mut offenders = Vec::new();
    for file in &files {
        if exports_itself(&std::fs::read_to_string(file)?) {
            offenders.push(file.display().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "a ts attribute with `export` in {offenders:?}; run `just protocol-gen` instead"
    );
    Ok(())
}

#[test]
fn the_scan_finds_export_in_any_position() {
    assert!(exports_itself("#[ts(export)]\nstruct A;"));
    assert!(exports_itself("#[ts(export_to = \"a.ts\")]\nstruct A;"));
    assert!(exports_itself("#[ts(rename = \"B\", export)]\nstruct A;"));
    assert!(exports_itself("#[ts(\n    export,\n)]\nstruct A;"));
}

#[test]
fn the_scan_passes_other_ts_attributes() {
    assert!(!exports_itself(
        "struct A(#[ts(type = \"number\")] pub u64);"
    ));
    assert!(!exports_itself("fn f() { parts(exported); }"));
    assert!(!exports_itself("let ts = 1;"));
}

/// Whether `source` holds a `ts(...)` attribute with an `export` key.
fn exports_itself(source: &str) -> bool {
    let mut rest = source;
    while let Some(at) = rest.find("ts(") {
        let before = rest.get(..at).unwrap_or_default();
        let after = rest.get(at + 3..).unwrap_or_default();
        let preceded_by_ident = before
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let args = after.split(')').next().unwrap_or_default();
        if !preceded_by_ident
            && args
                .split(',')
                .any(|arg| arg.trim_start().starts_with("export"))
        {
            return true;
        }
        rest = after;
    }
    false
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            rust_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}
