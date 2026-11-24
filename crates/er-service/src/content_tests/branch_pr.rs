//! A working tab's branch PR (`LoadBranchPr`): the PR section fills from the
//! probed number while the tab stays a working tab, and a load carries it.

use super::*;

fn overview(title: &str) -> er_engine::github::PrOverviewData {
    er_engine::github::PrOverviewData {
        number: 7,
        title: title.to_string(),
        body: String::new(),
        state: "OPEN".to_string(),
        author: "you".to_string(),
        url: String::new(),
        base_branch: "main".to_string(),
        head_branch: "feature".to_string(),
        checks: Vec::new(),
        reviewers: Vec::new(),
    }
}

fn title(content: &crate::TabContent) -> Option<String> {
    content.pr.overview.as_ref().map(|pr| pr.title.clone())
}

/// The read runs off the actor (ADR 0067): one made on the actor thread
/// trips the guard, and the reply never arrives.
#[test]
fn a_branch_pr_fills_the_pr_section_and_keeps_the_working_modes() -> anyhow::Result<()> {
    with_root(|repo| {
        let github = Arc::new(FakeGitHub::default().with_overview(Ok(Some(overview("One")))));
        let ports = Ports {
            git: git(RAW),
            github: github.clone(),
        };
        let handle = spawn(WorkspaceDeps::new(ports));
        let key = TabKey::next();
        load(&handle, key, repo)?;
        let send = |command| handle.send_and_wait_with(command, WAIT);

        let reply = send(Command::LoadBranchPr { key, number: 7 })?;
        assert!(matches!(reply, Reply::BranchPrLoaded { .. }), "{reply:?}");
        let content = content_of(&handle.view(), key)?;
        assert_eq!(content.pr.pr_number, Some(7));
        assert_eq!(title(&content).as_deref(), Some("One"));
        assert_eq!(content.tab.pr_number, None, "the tab became a PR tab");
        assert_eq!(content.tab.mode, DiffMode::Branch);

        // A refresh carries the section; a second read moves it.
        send(refresh(key))?;
        let content = content_of(&handle.view(), key)?;
        assert_eq!(title(&content).as_deref(), Some("One"), "a load dropped it");
        github.set_overview(Ok(Some(overview("Two"))));
        send(Command::LoadBranchPr { key, number: 7 })?;
        let content = content_of(&handle.view(), key)?;
        assert_eq!(title(&content).as_deref(), Some("Two"));
        let reads = github.calls();
        let reads: Vec<&String> = reads
            .iter()
            .filter(|c| c.starts_with("overview("))
            .collect();
        assert_eq!(reads.len(), 2, "{reads:?}");
        Ok(())
    })
}

fn branch_pr(key: TabKey, branch: &str) -> ContentPatch {
    ContentPatch::BranchPr {
        key,
        number: 7,
        branch: branch.to_string(),
        overview: Box::new(overview("One")),
    }
}

/// A reload that began before the branch PR landed keeps the section when
/// it applies; a load on another branch drops it.
#[test]
fn a_load_keeps_the_branch_pr_only_while_the_branch_holds() -> Result<(), PatchRefused> {
    let mut state = WorkspaceState::new();
    let key = TabKey::next();
    let loaded = |since| ContentPatch::Loaded {
        key,
        since,
        loaded: loaded_tab("/repo"),
    };
    let (since, _) = state.apply_patch(loaded(None))?;
    state.apply_patch(branch_pr(key, "feature"))?;
    state.apply_patch(loaded(Some(since)))?;
    let pr = &state.view().tabs[&key].pr;
    let kept = pr.overview.as_ref().map(|pr| pr.title.as_str());
    assert_eq!(
        kept,
        Some("One"),
        "the in-flight reload dropped the branch PR"
    );
    assert_eq!(pr.pr_number, Some(7));

    let since = state.view().tabs[&key].generation();
    let mut other = loaded_tab("/repo");
    other.tab.current_branch = "other".to_string();
    state.apply_patch(ContentPatch::Loaded {
        key,
        since: Some(since),
        loaded: other,
    })?;
    let pr = &state.view().tabs[&key].pr;
    assert!(pr.overview.is_none(), "another branch kept the branch PR");
    assert_eq!(pr.pr_number, None);
    Ok(())
}
