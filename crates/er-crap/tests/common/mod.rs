//! Shared helpers for the integration tests — one setup/opts source instead
//! of near-identical copies in every gate/error test file.
//!
//! Every integration-test binary compiles this module independently and only
//! uses a subset of the helpers, hence the dead-code allow.
#![allow(dead_code)]

use std::fs;
use std::path::Path;

use er_crap::report::OutputFormat;
use er_crap::Opts;

/// Write the `<name>.rs` + `<name>.lcov` fixture pair into `dir`.
pub fn write_fixture(dir: &Path, name: &str) {
    let (rs, lcov) = match name {
        "clean" => (
            include_str!("../fixtures/clean.rs"),
            include_str!("../fixtures/clean.lcov"),
        ),
        "crappy" => (
            include_str!("../fixtures/crappy.rs"),
            include_str!("../fixtures/crappy.lcov"),
        ),
        "boundary" => (
            include_str!("../fixtures/boundary.rs"),
            include_str!("../fixtures/boundary.lcov"),
        ),
        _ => panic!("unknown fixture: {name}"),
    };
    fs::write(dir.join(format!("{name}.rs")), rs).unwrap();
    fs::write(dir.join(format!("{name}.lcov")), lcov).unwrap();
}

/// Standard JSON, gate-on JSON options over `dir` with the given LCOV file.
pub fn opts(dir: &Path, lcov: &str, fail_above: bool) -> Opts {
    Opts {
        lcov_path: Some(dir.join(lcov)),
        path: vec![dir.to_path_buf()],
        threshold: 30.0,
        fail_above,
        format: OutputFormat::Json,
        summary: false,
    }
}
