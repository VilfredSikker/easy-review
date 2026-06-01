//! Wrapper so you can run: `cargo er-dev -- --logs arena`
//! (sets `ER_LOG` then invokes `cargo tauri dev` from `crates/er-desktop`).

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut logs: Option<String> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--logs" {
            logs = args.get(i + 1).cloned();
            i += 2;
            continue;
        }
        if let Some(v) = args[i].strip_prefix("--logs=") {
            logs = Some(v.to_string());
            i += 1;
            continue;
        }
        rest.push(args[i].clone());
        i += 1;
    }

    if let Some(spec) = logs {
        std::env::set_var("ER_LOG", spec);
    }

    let manifest_dir = concat!(env!("CARGO_MANIFEST_DIR"));
    let status = Command::new("cargo")
        .arg("tauri")
        .arg("dev")
        .args(&rest)
        .current_dir(manifest_dir)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("er-tauri-dev: failed to spawn cargo tauri dev: {e}");
            std::process::exit(1);
        });

    ExitCode::from(status.code().unwrap_or(1) as u8)
}
