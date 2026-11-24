//! The flag-on file watcher on the active tab's root. One refresh per tab is
//! out at a time; er-engine's `FileWatcher` owns the threads.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use er_engine::watch::{FileWatcher, WatchEvent};
use er_engine::TabKey;
use er_service::TabContent;

/// How long the changes must stop before the refresh, as the App's.
const DEBOUNCE: Duration = Duration::from_millis(200);
/// The watcher's own debounce, as the App's.
const WATCH_DEBOUNCE_MS: u64 = 500;

pub struct Watch {
    /// On at the start, as the App's; `w` turns it off and on.
    pub on: bool,
    watching: Option<(String, FileWatcher)>,
    /// The active tab, which a change counted now belongs to.
    key: Option<TabKey>,
    tx: Sender<WatchEvent>,
    rx: Receiver<WatchEvent>,
    /// Per tab, the changes counted and when their refresh is due.
    due: HashMap<TabKey, (Instant, usize)>,
    out: HashSet<TabKey>,
    seen: HashSet<TabKey>,
}

impl Default for Watch {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            on: true,
            watching: None,
            key: None,
            tx,
            rx,
            due: HashMap::new(),
            out: HashSet::new(),
            seen: HashSet::new(),
        }
    }
}

/// A tab's checkout, else its repo root; a remote tab has none.
fn root_of(content: &TabContent) -> Option<&str> {
    let tab = &content.tab;
    if tab.identity().is_remote() {
        return None;
    }
    Some(tab.checkout_root.as_deref().unwrap_or(&tab.repo_root))
}

impl Watch {
    pub fn watching(&self) -> bool {
        self.watching.is_some()
    }

    /// Watches the active tab's root. A tab followed before is refreshed once
    /// when shown again; a watcher that cannot open turns watching off.
    pub fn follow(&mut self, active: Option<(TabKey, &TabContent)>, now: Instant) {
        let root = active.and_then(|(key, content)| Some((key, root_of(content)?)));
        let Some((key, root)) = root else {
            self.watching = None;
            return;
        };
        if self.key != Some(key) {
            if !self.seen.insert(key) && self.on {
                self.due.entry(key).or_insert((now, 0));
            }
            self.key = Some(key);
        }
        if !self.on {
            self.watching = None;
            return;
        }
        if self.watching.as_ref().is_some_and(|(at, _)| at == root) {
            return;
        }
        self.watching = None;
        match FileWatcher::new(root.as_ref(), WATCH_DEBOUNCE_MS, self.tx.clone()) {
            Ok(watcher) => self.watching = Some((root.to_string(), watcher)),
            Err(_) => self.on = false,
        }
    }

    /// `w`: stops or starts watching the active tab, with the App's notices.
    pub fn toggle(&mut self, key: TabKey, content: &TabContent) -> String {
        if content.tab.identity().is_remote() {
            return "Watch not available in remote mode".into();
        }
        if self.on {
            self.on = false;
            self.watching = None;
            return "Watch stopped".into();
        }
        self.on = true;
        let Some(root) = root_of(content) else {
            return "Watching for changes...".into();
        };
        match FileWatcher::new(root.as_ref(), WATCH_DEBOUNCE_MS, self.tx.clone()) {
            Ok(watcher) => {
                self.watching = Some((root.to_string(), watcher));
                self.key = Some(key);
                "Watching for changes...".into()
            }
            Err(error) => {
                self.on = false;
                format!("Watch error: {error}")
            }
        }
    }

    /// Counts the changes that arrived for the tab watched when they came.
    pub fn count(&mut self, now: Instant) {
        while let Ok(WatchEvent::FilesChanged(paths)) = self.rx.try_recv() {
            let Some(key) = self.key else { continue };
            let files = self.due.get(&key).map_or(0, |(_, files)| *files) + paths.len();
            self.due.insert(key, (now + DEBOUNCE, files));
        }
    }

    /// A tab past the debounce with no refresh out; it is out until `landed`.
    pub fn ready(&mut self, now: Instant) -> Option<(TabKey, usize)> {
        let (&key, &(_, files)) = self
            .due
            .iter()
            .find(|(key, (at, _))| *at <= now && !self.out.contains(key))?;
        self.due.remove(&key);
        self.out.insert(key);
        Some((key, files))
    }

    pub fn landed(&mut self, key: TabKey) {
        self.out.remove(&key);
    }

    #[cfg(test)]
    pub fn root(&self) -> Option<&str> {
        self.watching.as_ref().map(|(root, _)| root.as_str())
    }

    #[cfg(test)]
    pub fn sender(&self) -> Sender<WatchEvent> {
        self.tx.clone()
    }
}
