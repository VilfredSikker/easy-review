//! Each tab's reviewed count, from er-service's reviewed builder. The builder
//! runs again only when the version it answers at moves, so a frame with
//! nothing new reads the count it already has.

use std::collections::HashMap;

use er_engine::TabKey;
use er_protocol::TabReviewed;
use er_service::{reviewed_resource, reviewed_version, TabContent};

#[derive(Default)]
pub struct ReviewedCache {
    tabs: HashMap<TabKey, (u64, TabReviewed)>,
    #[cfg(test)]
    pub builds: usize,
}

impl ReviewedCache {
    /// Forgets the tabs `open` no longer lists.
    pub fn keep_open(&mut self, open: &[TabKey]) {
        self.tabs.retain(|key, _| open.contains(key));
    }

    /// `key`'s reviewed and total file counts in `content`.
    pub fn count(&mut self, key: TabKey, content: &TabContent) -> (usize, usize) {
        let version = reviewed_version(content);
        let fresh = self.tabs.get(&key).is_some_and(|(at, _)| *at == version);
        if !fresh {
            #[cfg(test)]
            {
                self.builds += 1;
            }
            self.tabs.insert(key, (version, reviewed_resource(content)));
        }
        let reviewed = self.tabs.get(&key).map(|(_, reviewed)| reviewed);
        reviewed.map_or((0, 0), |reviewed| {
            let count = |n: u64| usize::try_from(n).unwrap_or(usize::MAX);
            (count(reviewed.reviewed_count), count(reviewed.total_count))
        })
    }
}
