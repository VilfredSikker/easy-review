//! Linux-only checks for running test scripts beside concurrent forks.
//!
//! On Linux, `execve` refuses a file that any process holds open for writing
//! (ETXTBSY). A test that writes a script and runs it can be caught by a fork on
//! another test thread: the forked child inherits the write descriptor and keeps
//! it until it execs. macOS does not raise this, so these run on Linux CI.
#![cfg(target_os = "linux")]

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The helper's child writer leaves no writable descriptor behind, so scripts
/// it writes stay runnable while another thread forks continuously.
#[test]
fn scripts_written_by_the_helper_run_while_another_thread_forks() {
    let dir = tempfile::tempdir().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let forker = {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let _ = Command::new("true").status();
            }
        })
    };

    let mut failures = Vec::new();
    for i in 0..50 {
        let path = dir.path().join(format!("fake-{i}.sh"));
        er_engine::test_support::write_executable_script(&path, "#!/bin/sh\nexit 0\n")
            .expect("write script");
        match Command::new(&path).status() {
            Ok(status) if status.success() => {}
            other => failures.push(format!("iteration {i}: {other:?}")),
        }
    }

    stop.store(true, Ordering::Relaxed);
    forker.join().unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The failure mode itself: any open writer blocks the exec. The writer lives
/// in a child process, not this one: an in-process writer would be inherited by
/// a concurrent test's fork, which is the race the helper prevents.
#[test]
fn an_open_writer_makes_exec_fail_with_etxtbsy() {
    const ETXTBSY: i32 = 26;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("held.sh");
    let opened = dir.path().join("holder-opened");
    er_engine::test_support::write_executable_script(&path, "#!/bin/sh\nexit 0\n").unwrap();
    assert!(Command::new(&path).status().unwrap().success());

    // `exec` makes the shell become the sleeping holder, so killing it closes
    // the descriptor in every process that held it. The marker is written only
    // after fd 3 is open, so the test can wait for that event instead of
    // polling the deadline.
    let mut holder = Command::new("sh")
        .arg("-c")
        .arg("exec 3>>\"$1\"; : > \"$2\"; exec sleep 30")
        .arg("sh")
        .arg(&path)
        .arg(&opened)
        .spawn()
        .expect("start the writing holder");

    // A hang guard only: the pass path returns as soon as the holder opens the
    // file, and a loaded runner can take longer than a few seconds to start a
    // process.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !opened.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the holder never opened the script for writing"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // The descriptor is open, so this exec must fail at once.
    let error = Command::new(&path)
        .status()
        .expect_err("exec must fail while the script is open for writing");
    assert_eq!(error.raw_os_error(), Some(ETXTBSY), "{error:?}");

    holder.kill().expect("kill the writer");
    holder.wait().expect("reap the writer");
    assert!(Command::new(&path).status().unwrap().success());
}
