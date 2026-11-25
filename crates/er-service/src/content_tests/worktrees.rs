//! The worktree list (`ListWorktrees`, P75.2): read off the actor for the
//! TUI's Open hub, which never reads it on its loop.

use super::*;

fn worktree(path: &str, branch: &str) -> er_git::Worktree {
    er_git::Worktree {
        path: path.to_string(),
        branch: branch.to_string(),
        head: "abc".to_string(),
    }
}

/// The read runs off the actor (ADR 0067): one made on the actor thread
/// trips the guard, and the reply never arrives.
#[test]
fn the_worktree_list_is_read_off_the_actor() -> anyhow::Result<()> {
    with_root(|repo| {
        let listed = vec![worktree(repo, "feature"), worktree("/tmp/wt", "other")];
        let fake = Arc::new(
            FakeGit::default()
                .with_is_merge_in_progress(false)
                .with_current_branch(Ok("feature".to_string()))
                .with_diff_shortstat((0, 0, 0))
                .with_diff_raw(Ok(RAW.to_string()))
                .with_list_worktrees(Ok(listed.clone())),
        );
        let handle = actor_over(Arc::clone(&fake));
        let key = TabKey::next();
        load(&handle, key, repo)?;

        let reply = handle.send_and_wait_with(Command::ListWorktrees { key }, WAIT)?;
        assert_eq!(
            reply,
            Reply::Worktrees {
                key,
                worktrees: listed
            }
        );
        let read = format!("list_worktrees(repo_root={repo:?})");
        assert!(fake.calls().contains(&read), "{:?}", fake.calls());

        let unknown = TabKey::next();
        let refused = handle.send_and_wait_with(Command::ListWorktrees { key: unknown }, WAIT);
        assert!(refused.is_err(), "{refused:?}");
        Ok(())
    })
}
