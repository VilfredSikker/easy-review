//! `pr(tab)`: a loaded tab's PR card and stack, built from its PR section.

use er_engine::gh_stack::StackInfo;
use er_protocol::{PrCard, Stack, StackLayer, TabPr};

use crate::content::TabContent;

/// The version `pr(tab)` answers at: the PR section's revision, which every
/// load and every stack lookup moves.
pub fn pr_version(content: &TabContent) -> u64 {
    content.pr.revision
}

/// `content`'s PR card and stack, as the snapshot's `pr` and `stack` show
/// them.
pub fn pr_resource(content: &TabContent) -> TabPr {
    let pr = content.pr.overview.as_ref().map(|pr| PrCard {
        number: pr.number,
        title: pr.title.clone(),
        state: pr.state.clone(),
        base: pr.base_branch.clone(),
        head: pr.head_branch.clone(),
        url: pr.url.clone(),
        author: pr.author.clone(),
    });
    let stack = crate::tab_pr::stack_source(&content.tab, content.pr.overview.as_ref()).map(|_| {
        content
            .pr
            .stack
            .as_ref()
            .map_or_else(Stack::default, |read| stack(&read.info))
    });
    TabPr { pr, stack }
}

fn stack(info: &StackInfo) -> Stack {
    match info {
        // A definitive "no stack here" hides the control; a failed lookup
        // keeps it so the page can retry.
        StackInfo::Unavailable(reason) => Stack {
            unavailable: Some(reason.clone()),
            ..Stack::default()
        },
        StackInfo::Failed(reason) => Stack {
            unavailable: Some(reason.clone()),
            retryable: true,
            ..Stack::default()
        },
        StackInfo::Stack(stack) => Stack {
            trunk: stack.trunk.clone(),
            layers: stack
                .entries
                .iter()
                .map(|entry| StackLayer {
                    branch: entry.branch.clone(),
                    pr_number: entry.pr_number,
                    pr_url: entry.pr_url.clone(),
                    state: entry.status_label(),
                    is_current: entry.is_current,
                    enabled: entry.is_openable(),
                    needs_rebase: entry.needs_rebase,
                })
                .collect(),
            // 1-based from the top of the stack: the badge reads `2 / 4`.
            position: stack
                .entries
                .iter()
                .position(|entry| entry.is_current)
                .and_then(|index| u64::try_from(index + 1).ok()),
            size: u64::try_from(stack.entries.len()).unwrap_or(u64::MAX),
            unavailable: None,
            retryable: false,
            loading: false,
        },
    }
}
