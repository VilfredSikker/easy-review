//! A file's context-line count (P71.3): set off the actor, kept across a
//! refresh in the same view and base, dropped by a switch.

use super::*;

/// `src/a.rs` fetched with more context: one wider hunk.
const WIDE: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,4 +1,4 @@
 a-zero
 a-one
-a-old
+a-new
 a-two
";

fn fake() -> Arc<FakeGit> {
    Arc::new(
        FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_diff_shortstat((0, 0, 0))
            .with_diff_raw(Ok(RAW.to_string()))
            .with_diff_raw_file(Ok(WIDE.to_string())),
    )
}

fn set(handle: &WorkspaceHandle, key: TabKey, lines: usize) -> anyhow::Result<Reply> {
    let path = "src/a.rs".to_string();
    Ok(handle.send_and_wait_with(Command::SetFileContext { key, path, lines }, WAIT)?)
}

fn a_lines(handle: &WorkspaceHandle, key: TabKey) -> anyhow::Result<(usize, Option<usize>)> {
    let content = content_of(&handle.view(), key)?;
    let file = content.diff.files.iter().find(|f| f.path == "src/a.rs");
    let lines = file.map_or(0, |file| file.hunks.iter().map(|h| h.lines.len()).sum());
    Ok((lines, content.diff.context.get("src/a.rs").copied()))
}

#[test]
fn a_context_count_holds_across_a_refresh_and_drops_on_a_switch() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(fake());
        let key = TabKey::next();
        load(&handle, key, repo)?;
        assert_eq!(a_lines(&handle, key)?, (3, None));

        assert_eq!(set(&handle, key, 20)?, Reply::ContextSet { key });
        assert_eq!(a_lines(&handle, key)?, (5, Some(20)));

        handle.send_and_wait_with(super::refresh(key), WAIT)?;
        assert_eq!(a_lines(&handle, key)?, (5, Some(20)), "a refresh lost it");

        let switch = Command::SetDiffMode {
            key,
            mode: DiffMode::Unstaged,
            pr_number: None,
        };
        handle.send_and_wait_with(switch, WAIT)?;
        assert_eq!(a_lines(&handle, key)?, (3, None), "a switch kept it");
        Ok(())
    })
}

/// Back at the default count, the path leaves the map.
#[test]
fn a_count_back_at_the_default_leaves_the_map() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(fake());
        let key = TabKey::next();
        load(&handle, key, repo)?;
        set(&handle, key, 20)?;
        assert_eq!(a_lines(&handle, key)?.1, Some(20));
        set(&handle, key, er_engine::git::DEFAULT_CONTEXT_LINES)?;
        let content = content_of(&handle.view(), key)?;
        assert!(
            content.diff.context.is_empty(),
            "{:?}",
            content.diff.context
        );
        Ok(())
    })
}
