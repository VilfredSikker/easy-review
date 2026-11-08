//! The process seam below the port.

use std::process::{Command, Output, Stdio};
use std::time::Duration;

use er_git::proc::CommandTimeoutExt;

/// One `gh` invocation.
#[derive(Debug, Clone)]
pub struct GhCall<'a> {
    pub args: &'a [String],
    pub dir: Option<&'a str>,
    pub timeout: Duration,
    /// Body cap; the process runner adds the header allowance.
    pub max_stdout: Option<usize>,
}

pub trait GhRunner: Send + Sync {
    fn run(&self, call: &GhCall<'_>) -> std::io::Result<Output>;
    fn spawn_detached(&self, args: &[String], dir: Option<&str>) -> std::io::Result<()>;
}

/// The runner that executes the real binary.
pub struct ProcessRunner;

impl GhRunner for ProcessRunner {
    fn run(&self, call: &GhCall<'_>) -> std::io::Result<Output> {
        let mut cmd = Command::new("gh");
        cmd.args(call.args);
        if let Some(dir) = call.dir {
            cmd.current_dir(dir);
        }
        match call.max_stdout {
            Some(cap) => er_git::proc::run_with_bounded_stdout(
                &mut cmd,
                call.timeout,
                cap.saturating_add(16 * 1024),
            ),
            None => cmd.output_timed(call.timeout),
        }
    }

    fn spawn_detached(&self, args: &[String], dir: Option<&str>) -> std::io::Result<()> {
        let mut cmd = Command::new("gh");
        cmd.args(args);
        if let Some(dir) = dir {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(())
    }
}

/// A runner for tests and for downstream crates' tests. Unconditional, so
/// #382 and #383 can use it.
pub mod testing {
    use super::{GhCall, GhRunner};
    use std::collections::VecDeque;
    use std::io;
    use std::process::{ExitStatus, Output};
    use std::sync::{Mutex, PoisonError};

    #[derive(Default)]
    pub struct ScriptedRunner {
        scripts: Mutex<VecDeque<(Vec<String>, io::Result<Output>)>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl ScriptedRunner {
        pub fn new() -> Self {
            Self::default()
        }

        /// Queue `result` for the first call whose argv starts with `args_prefix`.
        /// The prefix is matched against the final argv (`gh` first), including
        /// an inserted `--include`.
        pub fn script(&self, args_prefix: &[&str], result: io::Result<Output>) -> &Self {
            let prefix: Vec<String> = args_prefix.iter().map(ToString::to_string).collect();
            self.scripts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push_back((prefix, result));
            self
        }

        pub fn calls(&self) -> Vec<Vec<String>> {
            self.calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }

        pub fn output(exit: i32, stdout: impl Into<Vec<u8>>, stderr: impl Into<Vec<u8>>) -> Output {
            Output {
                status: exit_status(exit),
                stdout: stdout.into(),
                stderr: stderr.into(),
            }
        }
    }

    impl GhRunner for ScriptedRunner {
        fn run(&self, call: &GhCall<'_>) -> io::Result<Output> {
            let argv = argv(call.args);
            self.calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(argv.clone());
            let mut scripts = self.scripts.lock().unwrap_or_else(PoisonError::into_inner);
            let index = scripts.iter().position(|(prefix, _)| {
                argv.len() >= prefix.len()
                    && prefix
                        .iter()
                        .zip(argv.iter())
                        .all(|(expected, actual)| expected == actual)
            });
            match index.and_then(|index| scripts.remove(index)) {
                Some((_, result)) => result,
                None => Err(io::Error::other(format!("unscripted gh call: {argv:?}"))),
            }
        }

        fn spawn_detached(&self, args: &[String], _dir: Option<&str>) -> io::Result<()> {
            self.calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(argv(args));
            Ok(())
        }
    }

    /// The argv the process runner would execute, with the program first.
    fn argv(args: &[String]) -> Vec<String> {
        let mut argv = vec!["gh".to_string()];
        argv.extend(args.iter().cloned());
        argv
    }

    #[cfg(unix)]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code as u32)
    }
}
