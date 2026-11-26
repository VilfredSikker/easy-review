//! The MCP contract as a skill sees it: the `er-mcp` binary is started for
//! each scenario with its own storage root and home under a temp dir, driven
//! over stdio, and its JSON-RPC replies are compared with the goldens in
//! `tests/contract/goldens/`.
//!
//! The policy these goldens hold (README, "Stability policy") lets a reply
//! gain keys and a tool gain optional arguments. A golden that changes for
//! any other reason is a break of the contract. `ER_UPDATE_GOLDENS=1`
//! records them; a re-record is a reviewed change, named up front in its PR.
//!
//! Every child runs with a fake `gh` first on its `PATH`. It serves the
//! fixtures listed in `tests/contract/gh/index.tsv` and logs any other call,
//! and a scenario that made a logged call fails even when the tool swallowed
//! the error.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{json, Map, Value};

const PROTOCOL_VERSION: &str = "2026-07-28";
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

fn contract_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/contract")
}

/// One running `er-mcp`, with the temp root it owns.
pub struct Mcp {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
    root: tempfile::TempDir,
}

impl Mcp {
    /// Start the binary with an empty storage root, home and legacy config
    /// dir, and the fake `gh` first on `PATH`.
    pub fn start() -> Self {
        Self::start_with_fixtures(&contract_dir().join("gh"))
    }

    /// [`Mcp::start`] with the fake `gh` serving `fixtures`.
    pub fn start_with_fixtures(fixtures: &Path) -> Self {
        let root = tempfile::tempdir().expect("temp root");
        let dirs = ["storage", "home", "legacy-config", "work"];
        for dir in dirs {
            std::fs::create_dir_all(root.path().join(dir)).expect("temp subdir");
        }
        let fake_bin = contract_dir().join("bin");
        let path = std::env::var_os("PATH").unwrap_or_default();
        let path =
            std::env::join_paths(std::iter::once(fake_bin).chain(std::env::split_paths(&path)))
                .expect("PATH");
        let stderr = std::fs::File::create(root.path().join("stderr.log")).expect("stderr log");
        let mut child = Command::new(env!("CARGO_BIN_EXE_er-mcp"))
            .env_clear()
            .env("PATH", path)
            .env("HOME", root.path().join("home"))
            .env("ER_STORAGE_ROOT", root.path().join("storage"))
            .env("ER_LEGACY_CONFIG_DIR", root.path().join("legacy-config"))
            .env("ER_FAKE_GH_FIXTURES", fixtures)
            .env("ER_FAKE_GH_LOG", root.path().join("gh-calls.log"))
            .env("TMPDIR", std::env::temp_dir())
            .current_dir(root.path().join("work"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()
            .expect("start er-mcp");
        let stdout = child.stdout.take().expect("stdout");
        let (send, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        Self {
            child,
            stdin,
            lines,
            next_id: 1,
            root,
        }
    }

    /// The temp dir this child's storage, home and working dir live under.
    pub fn root(&self) -> &Path {
        self.root.path()
    }

    /// Send one request and wait for its reply: the `result`, or the
    /// `error` under an `error` key.
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let mut params = match params {
            Value::Object(map) => map,
            Value::Null => Map::new(),
            other => panic!("params must be an object, got {other}"),
        };
        params.insert(
            "_meta".into(),
            json!({
                "io.modelcontextprotocol/protocolVersion": PROTOCOL_VERSION,
                "io.modelcontextprotocol/clientCapabilities": {},
                "io.modelcontextprotocol/clientInfo": { "name": "er-mcp-contract", "version": "0" },
            }),
        );
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{line}").expect("write request");
        stdin.flush().expect("flush request");
        loop {
            let line = self
                .lines
                .recv_timeout(REPLY_TIMEOUT)
                .unwrap_or_else(|error| {
                    panic!("no reply to {method} ({error}); stderr:\n{}", self.stderr())
                });
            let reply: Value = serde_json::from_str(&line)
                .unwrap_or_else(|error| panic!("stdout line is not JSON ({error}): {line}"));
            if reply.get("id") != Some(&json!(id)) {
                continue;
            }
            if let Some(error) = reply.get("error") {
                return json!({ "error": error });
            }
            return reply.get("result").cloned().unwrap_or(Value::Null);
        }
    }

    /// Call one tool with `arguments` as a skill would send them.
    pub fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )
    }

    fn stderr(&self) -> String {
        std::fs::read_to_string(self.root().join("stderr.log")).unwrap_or_default()
    }

    /// Close stdin, wait for the child, and fail on any `gh` call the fake
    /// had no fixture for.
    pub fn finish(mut self) {
        drop(self.stdin.take());
        let status = self.child.wait().expect("wait for er-mcp");
        assert!(
            status.success(),
            "er-mcp exited {status}; stderr:\n{}",
            self.stderr()
        );
        let stray = std::fs::read_to_string(self.root().join("gh-calls.log")).unwrap_or_default();
        assert!(
            stray.is_empty(),
            "er-mcp called gh with no fixture for it:\n{stray}"
        );
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `text` with this child's temp root, as given and as resolved, written as
/// `<root>`, so a golden reads the same from any temp dir.
pub fn mask_root(text: &str, root: &Path) -> String {
    let mut roots: Vec<String> = [Some(root.to_path_buf()), std::fs::canonicalize(root).ok()]
        .into_iter()
        .flatten()
        .map(|path| path.to_string_lossy().trim_end_matches('/').to_string())
        .collect();
    roots.sort_by_key(|root| std::cmp::Reverse(root.len()));
    roots.iter().fold(text.to_string(), |out, root| {
        out.replace(root.as_str(), "<root>")
    })
}

/// `value` with every object's keys in sorted order. serde_json keeps
/// insertion order when another crate in the build turns on
/// `preserve_order`, so the goldens fix one order for both builds.
pub fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), sorted(&map[key])))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// `value`, keys sorted and pretty-printed, equals `goldens/<name>.json`.
/// `ER_UPDATE_GOLDENS=1` writes the file instead.
pub fn assert_golden(name: &str, value: &Value) {
    let mut text = serde_json::to_string_pretty(&sorted(value)).expect("golden JSON");
    text.push('\n');
    let path = contract_dir().join("goldens").join(format!("{name}.json"));
    if std::env::var_os("ER_UPDATE_GOLDENS").is_some() {
        assert!(
            std::env::var_os("CI").is_none(),
            "ER_UPDATE_GOLDENS is refused under CI: record goldens locally"
        );
        std::fs::create_dir_all(path.parent().expect("goldens dir")).expect("goldens dir");
        std::fs::write(&path, &text).expect("write golden");
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {} ({error})", path.display()));
    if want == text {
        return;
    }
    let line = want
        .lines()
        .zip(text.lines())
        .position(|(want, got)| want != got)
        .unwrap_or_else(|| want.lines().count().min(text.lines().count()));
    panic!(
        "{name} differs from {} at line {}:\n  golden: {}\n  got:    {}",
        path.display(),
        line + 1,
        want.lines().nth(line).unwrap_or("<end>"),
        text.lines().nth(line).unwrap_or("<end>"),
    );
}
