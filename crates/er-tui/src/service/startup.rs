//! The flag-on start: the command line's targets become registry tabs, each
//! loaded on its workspace's actor.

use anyhow::Context;
use er_engine::github;
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_service::{AppCommand, Command, Reply, ReplyReceiver, ServiceHost};

/// The command line's targets and flags, as `main` parsed them.
#[derive(Debug, Default)]
pub struct Args {
    pub paths: Vec<String>,
    pub pr: Option<u64>,
    pub filter: Option<String>,
    pub remote: bool,
    pub target: Option<String>,
}

/// One tab the start opens. A local PR tab has the service fetch its head
/// and base refs once its first load lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartTab {
    pub tab: TabDescriptor,
    pub fetch_pr_refs: bool,
}

/// The tabs to open, in order, and what the status line says about flags
/// that have no service route yet.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Start {
    pub tabs: Vec<StartTab>,
    pub notices: Vec<String>,
    /// `--filter`, which the first tab applies before its session restores.
    pub filter: Option<String>,
}

/// Turns the arguments into tabs. Paths resolve to their repo roots; a PR
/// URL opens a remote PR with `--remote`, or this clone's PR without;
/// `--pr` makes the first tab that PR. Reads the repo roots, and for `--pr`
/// the origin's `owner/repo`, which the port parses from the remote URL.
/// A `--remote` or `--pr` start first checks that `gh` works, as the App's.
pub fn plan(args: &Args, ports: &er_engine::ports::Ports) -> anyhow::Result<Start> {
    let git = &*ports.git;
    let mut start = Start {
        filter: args.filter.clone(),
        ..Start::default()
    };
    if args.remote {
        ports.github.ensure_installed()?;
        for url in args
            .paths
            .iter()
            .filter(|path| github::is_github_pr_url(path))
        {
            let pr = github::parse_github_pr_url(url)
                .ok_or_else(|| anyhow::anyhow!("Invalid GitHub PR URL: {url}"))?;
            start.tabs.push(remote_pr(pr));
        }
        return Ok(start);
    }
    if args.paths.is_empty() {
        start.tabs.push(working(git.repo_root_cwd()?));
    }
    for path in &args.paths {
        if github::is_github_pr_url(path) {
            let pr = github::parse_github_pr_url(path)
                .ok_or_else(|| anyhow::anyhow!("Invalid GitHub PR URL: {path}"))?;
            let root = git
                .repo_root_cwd()
                .context("Cannot open PR URL: not in a git repository. Clone the repo first.")?;
            start
                .tabs
                .push(local_pr(root, pr.number, Some((pr.owner, pr.repo))));
        } else {
            let dir =
                std::fs::canonicalize(path).with_context(|| format!("Path not found: {path}"))?;
            let root = git
                .repo_root_in(&dir.to_string_lossy())
                .with_context(|| format!("Not a git repository: {path}"))?;
            start.tabs.push(working(root));
        }
    }
    if let (Some(number), Some(first)) = (args.pr, start.tabs.first_mut()) {
        ports.github.ensure_installed()?;
        let root = first.tab.repo_root.clone();
        // The repo names the PR, so an open of the same PR from the Open hub
        // finds this tab (`same_pr`) instead of listing a second one.
        let repo = ports.github.get_repo_info(git, &root).ok();
        *first = local_pr(root, number, repo);
    }
    // `--target` is each tab's base from its first load, as the App's.
    for start_tab in &mut start.tabs {
        start_tab.tab.base_ref.clone_from(&args.target);
    }
    Ok(start)
}

/// The `er <args>` line that reopens these tabs, as the App prints it on
/// exit: only with more than one tab or a remote PR tab, and with
/// `--remote` when every tab is one.
pub fn resume_hint(tabs: &[TabDescriptor]) -> Option<String> {
    let remote = |tab: &TabDescriptor| tab.kind == TabKind::RemotePr;
    if tabs.len() <= 1 && !tabs.iter().any(remote) {
        return None;
    }
    let args: Vec<String> = tabs
        .iter()
        .map(|tab| match (&tab.pr_owner, &tab.pr_repo, tab.pr_number) {
            (Some(owner), Some(repo), Some(number)) if remote(tab) => {
                format!("https://github.com/{owner}/{repo}/pull/{number}")
            }
            _ => tab.repo_root.clone(),
        })
        .collect();
    let prefix = if tabs.iter().all(remote) {
        "er --remote"
    } else {
        "er"
    };
    Some(format!("{prefix} {}", args.join(" ")))
}

fn descriptor(kind: TabKind, repo_root: String) -> TabDescriptor {
    TabDescriptor {
        kind,
        repo_root,
        branch: None,
        pr_owner: None,
        pr_repo: None,
        pr_number: None,
        pr_head_ref: None,
        base_ref: None,
        extra: serde_json::Map::new(),
    }
}

pub fn working(repo_root: String) -> StartTab {
    StartTab {
        tab: descriptor(TabKind::Working, repo_root),
        fetch_pr_refs: false,
    }
}

/// A remote PR tab, with no clone, as `--remote` and the Open hub open one.
pub fn remote_pr(pr: github::PrRef) -> StartTab {
    let mut tab = descriptor(TabKind::RemotePr, String::new());
    tab.pr_owner = Some(pr.owner);
    tab.pr_repo = Some(pr.repo);
    tab.pr_number = Some(pr.number);
    StartTab {
        tab,
        fetch_pr_refs: false,
    }
}

fn local_pr(repo_root: String, number: u64, repo: Option<(String, String)>) -> StartTab {
    let mut tab = descriptor(TabKind::LocalPr, repo_root);
    tab.pr_number = Some(number);
    if let Some((owner, name)) = repo {
        tab.pr_owner = Some(owner);
        tab.pr_repo = Some(name);
    }
    StartTab {
        tab,
        fetch_pr_refs: true,
    }
}

/// What to do once a reply lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    Nothing,
    /// Fetch the PR's head and base refs on the service, as a sync does.
    FetchPrRefs,
    /// A commit: once it lands, the commit input closes.
    Commit,
    /// A reviewed toggle. A mark made under unreviewed-only moves the
    /// selection to this place in the visible list once the marks publish.
    Reviewed {
        advance_from: Option<usize>,
    },
    /// A watched-file write the user asked for: the list is read again.
    Watched,
    /// The snapshot a first read found missing: its diff is read again, and
    /// nothing is announced.
    SnapshotSaved,
    /// A create, reply or edit: once it lands, the draft of the tab it was
    /// written on clears; a refusal keeps it.
    Composed,
    /// A GitHub post from the composer: as `Composed`, and a refusal that
    /// adopted an earlier body drops the draft's client id.
    Posted,
    /// A pull of the PR's comments.
    Synced,
    /// An approval.
    Approved,
    /// A thread delete: once it lands, the status line says so.
    Deleted,
    /// A checklist toggle: once it lands, the status line says whether the
    /// row is checked, as it read at the press.
    Checklist {
        checked: bool,
    },
    /// A read of the pending review for the review hub.
    ReviewRead,
    /// A remove from the pending review; its reply reads the review back.
    ReviewEdit,
    /// The review's submit: once it lands, the hub closes.
    ReviewSubmit,
    /// A mode switch; a refused PR diff says it is unavailable.
    ModeSwitched {
        pr_diff: bool,
    },
    Refreshed(Refresh),
    BaseChanged,
    /// A file's context-line count; an automatic pick's refusal is
    /// silent, as the App's.
    Context {
        auto: bool,
    },
    /// A History commit `k` or `j` picked: once it lands, the next queued
    /// press goes out.
    CommitSelected,
    /// History's next page: once it lands, the waiting `k` moves into it.
    MoreHistory,
    /// A clear: once it lands, the status line says so; a refusal says why.
    Cleared(super::clears::Clear),
    /// A stack lookup the Open hub asked for: once it lands, its rows follow.
    StackRead,
    /// The worktree list the Open hub's opening asked for.
    Worktrees,
    /// The PR section's forced refresh after a pull: once it lands, the
    /// pull's notice says the PR status was refreshed, as the App's does.
    PrRefreshed {
        github_count: usize,
        local_count: usize,
    },
    /// A rerun's clear: once it lands, the run is planned; a refusal says
    /// why and nothing runs.
    Rerun(super::ai_runs::Rerun),
}

impl After {
    /// A reload the selection follows.
    pub const fn follows(self) -> bool {
        matches!(
            self,
            Self::ModeSwitched { .. } | Self::Refreshed(_) | Self::BaseChanged
        )
    }
}

/// Why a refresh went out, for its notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refresh {
    Key,
    /// `m`: the sort's notice went at the press.
    Sort,
    Watched {
        files: usize,
    },
}

/// A command in flight. The loop drains its receiver each tick.
pub struct Pending {
    pub key: TabKey,
    pub workspace: String,
    pub replies: ReplyReceiver,
    pub after: After,
    /// A reload to resend if its tab moved, and its resends; for a context
    /// count, the command it sent.
    pub retry: Option<(Command, u8)>,
}

/// Lists every tab in the registry, makes the first one active, and loads
/// it. The others load when they are first shown. Answers the tab keys in
/// order and the load in flight.
pub fn open(host: &ServiceHost, start: &Start) -> anyhow::Result<(Vec<TabKey>, Vec<Pending>)> {
    let mut keys = Vec::new();
    for start_tab in &start.tabs {
        let reply = host.registry().send(AppCommand::NewTab {
            tab: start_tab.tab.clone(),
        })?;
        let Reply::Tabs(view) = reply else {
            anyhow::bail!("a new tab answered {reply:?}");
        };
        keys.extend(view.active);
    }
    let mut pending = Vec::new();
    if let (Some(&first), Some(start_tab)) = (keys.first(), start.tabs.first()) {
        host.registry().send(AppCommand::SelectTab { key: first })?;
        pending.push(load(host, first, start_tab));
    }
    Ok((keys, pending))
}

/// Sends a tab's first load without waiting for it.
pub fn load(host: &ServiceHost, key: TabKey, start_tab: &StartTab) -> Pending {
    let workspace = start_tab.tab.workspace_key();
    let replies = host.post(
        &workspace,
        Command::LoadTab {
            key,
            tab: start_tab.tab.clone(),
            mode: None,
        },
    );
    let after = if start_tab.fetch_pr_refs {
        After::FetchPrRefs
    } else {
        After::Nothing
    };
    Pending {
        key,
        workspace,
        replies,
        after,
        retry: None,
    }
}
