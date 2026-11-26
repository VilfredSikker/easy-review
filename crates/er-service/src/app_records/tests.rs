use std::path::{Path, PathBuf};

/// Only the desktop's composition registers the app records' kinds; the TUI
/// and MCP build their registries from the shared table alone, which their
/// "does not claim" tests read.
#[test]
fn only_the_desktop_registers_the_app_record_kinds() {
    let call = concat!("register_desktop", "_kinds(");
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut callers = Vec::new();
    for krate in std::fs::read_dir(&crates).unwrap() {
        let krate = krate.unwrap().path();
        let mut dirs: Vec<PathBuf> = ["src", "tests"].map(|dir| krate.join(dir)).into();
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs")
                    && std::fs::read_to_string(&path).unwrap().contains(call)
                {
                    let name = path.strip_prefix(&crates).unwrap();
                    callers.push(name.to_string_lossy().into_owned());
                }
            }
        }
    }
    callers.sort();
    assert_eq!(
        callers,
        [
            "er-desktop/src/store_registry.rs",
            "er-service/src/app_records.rs"
        ]
    );
}
