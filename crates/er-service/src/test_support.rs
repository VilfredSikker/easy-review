//! A fake agent for task tests: `/bin/sh` that prints its pid and two
//! lines, waiting on a gate file between them, so a test can look at a task
//! mid-run and stop it.

use std::path::{Path, PathBuf};

/// The files a gated agent reads and writes.
pub struct Agent {
    pub pid: PathBuf,
    pub first: PathBuf,
    pub second: PathBuf,
    pub settings: PathBuf,
}

impl Agent {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            pid: dir.join("agent.pid"),
            first: dir.join("gate-1"),
            second: dir.join("gate-2"),
            settings: dir.join("settings"),
        }
    }

    /// Prints its pid to a file and `line-a`, waits for the first gate,
    /// prints `line-b`, waits for the second. Each wait gives up after
    /// about thirty seconds, or once the test's settings are gone, so a
    /// failed test leaves no shell behind holding a slot.
    pub fn config(&self, cap: usize) -> String {
        let pid = self.pid.display();
        // The settings directory, not the test's own: a shell that starts as
        // the directory is removed writes its pid file into it, so the
        // removal fails and the directory stays, without its gates.
        let settings = self.settings.display();
        let wait = |gate: &Path| {
            let gate = gate.display();
            format!(
                r#"n=0; while [ ! -e "{gate}" ] && [ -d "{settings}" ] && [ $n -lt 1500 ]; do sleep 0.02; n=$((n+1)); done"#
            )
        };
        let script = format!(
            r#"echo $$ > "{pid}"; printf 'line-a\n'; {}; printf 'line-b\n'; {}"#,
            wait(&self.first),
            wait(&self.second)
        );
        provider(cap, &script)
    }

    /// Writes [`Self::config`] into the settings directory and answers its
    /// path.
    pub fn write_config(&self, cap: usize) -> PathBuf {
        self.write(&self.config(cap))
    }

    /// Like [`Self::write_config`], for an agent that runs `script` with
    /// the prompt as `$1`.
    pub fn write_script_config(&self, script: &str) -> PathBuf {
        self.write(&provider(50, script))
    }

    fn write(&self, config: &str) -> PathBuf {
        let path = self.settings.join("config.toml");
        std::fs::create_dir_all(&self.settings).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&path, config).unwrap_or_else(|e| panic!("config: {e}"));
        path
    }
}

/// The fake provider's config, running `script` with the prompt as `$1`.
fn provider(cap: usize, script: &str) -> String {
    format!(
        r#"
[ai_hub]
default_provider = "gated"
default_model = "one"
max_concurrent_reviews = {cap}

[ai_hub.providers.gated]
label = "Gated"
command = "/bin/sh"
args = ["-c", '''{script}''', "gated-agent", "{{prompt}}"]

[[ai_hub.providers.gated.models]]
id = "one"
"#
    )
}

pub fn open_gate(gate: &Path) {
    std::fs::write(gate, "").unwrap_or_else(|e| panic!("gate: {e}"));
}

#[cfg(test)]
pub fn pid_alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
