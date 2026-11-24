//! The Log panel and the status bar's running commands, read from the
//! service's task host for the tab's bucket. The host keeps each bucket's
//! log after its tasks leave the list (ADR 0074), as the App keeps a tab's.

use std::collections::{HashMap, HashSet};

use er_engine::agent_log::{AgentLogEntry, AgentLogSource};
use er_engine::TabKey;
use er_service::TaskHost;

use crate::screen::view;

/// What the frame draws from the task host for one bucket.
#[derive(Default)]
pub struct Jobs {
    pub log: Vec<view::AgentLogEntry>,
    /// The commands running on the bucket, by the name the App gives each.
    pub running: Vec<String>,
}

/// The bucket's log, read only when the Log panel shows it, and its
/// running commands.
pub fn read(tasks: &TaskHost, er_dir: &str, shows_log: bool) -> Jobs {
    let log = if shows_log {
        tasks.bucket_log(er_dir).into_iter().map(entry).collect()
    } else {
        Vec::new()
    };
    Jobs {
        log,
        running: running(tasks, er_dir),
    }
}

/// Whether a command runs on `er_dir`, so the open Log panel draws each
/// tick: an output line moves no row and sends no event.
pub fn any_running(tasks: &TaskHost, er_dir: &str) -> bool {
    !running(tasks, er_dir).is_empty()
}

fn running(tasks: &TaskHost, er_dir: &str) -> Vec<String> {
    tasks
        .rows()
        .into_iter()
        .filter(|row| row.er_dir == er_dir && row.status == "running")
        .map(|row| match row.kind.strip_prefix("expert:") {
            Some(id) => format!("expert-{id}"),
            None => row.kind,
        })
        .collect()
}

/// Forgets the log of each closed tab that ran something, unless an open
/// tab still shows that bucket. `ran` maps each tab that ran something to
/// its bucket, and the closed tabs leave it; `shown` is every open tab's
/// bucket.
pub fn forget_closed(
    tasks: &TaskHost,
    ran: &mut HashMap<TabKey, String>,
    open: &HashSet<TabKey>,
    shown: &HashSet<String>,
) {
    ran.retain(|key, er_dir| {
        if open.contains(key) {
            return true;
        }
        if !shown.contains(er_dir.as_str()) {
            tasks.drop_bucket_log(er_dir);
        }
        false
    });
}

fn entry(entry: AgentLogEntry) -> view::AgentLogEntry {
    view::AgentLogEntry {
        timestamp: entry.timestamp,
        command_name: entry.command_name,
        source: match entry.source {
            AgentLogSource::Stdout => view::AgentLogSource::Stdout,
            AgentLogSource::Stderr => view::AgentLogSource::Stderr,
            AgentLogSource::Status => view::AgentLogSource::Status,
        },
        text: entry.text,
    }
}
