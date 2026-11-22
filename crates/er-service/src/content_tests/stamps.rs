//! The AI section's sidecar stamp: a workspace write moves it to what the
//! write left, the stamp is read before the state it vouches for, and a
//! reload that cannot start leaves no key behind.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;

use super::*;
use crate::content::read_stamped;
use crate::sidecar_reload::SidecarReloads;

fn reloaded(reply: Reply) -> anyhow::Result<bool> {
    match reply {
        Reply::SidecarsReloaded { reloaded, .. } => Ok(reloaded),
        other => anyhow::bail!("unexpected reply: {other:?}"),
    }
}

/// The question a service write stores is in the AI section and its stamp,
/// so the tick's next reload finds nothing new and the section moves once.
#[test]
fn a_service_question_moves_the_ai_section_once_across_the_next_reload() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(git(RAW));
        let key = TabKey::next();
        load(&handle, key, repo)?;
        let loaded = content_of(&handle.view(), key)?;
        let target = AnnotationTarget {
            file: "src/b.rs".to_string(),
            hunk_idx: 0,
            line_num: Some(2),
            line_num_end: None,
        };
        handle.send_and_wait_with(
            Command::AddQuestion {
                context: loaded.context.clone(),
                target,
                body: "why?".to_string(),
                side: Some("RIGHT".to_string()),
                client_id: Some("q-own".to_string()),
                reply_to: None,
                finding_ref: None,
            },
            WAIT,
        )?;
        let written = content_of(&handle.view(), key)?;
        assert!(written.ai.revision > loaded.ai.revision);

        let reply = handle.send_and_wait_with(Command::ReloadSidecars { key }, WAIT)?;

        assert!(
            !reloaded(reply)?,
            "the reload read the workspace's own write back"
        );
        let after = content_of(&handle.view(), key)?;
        assert_eq!(
            after.ai.revision, written.ai.revision,
            "the AI section moved twice"
        );
        Ok(())
    })
}

/// A write that lands while the state is read leaves the stamp behind the
/// files, so the next reload reads it instead of losing it.
#[test]
fn a_write_during_the_read_leaves_the_stamp_behind_it() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let bucket = dir.path().to_string_lossy().into_owned();
    std::fs::write(dir.path().join("questions.json"), "{}")?;

    let (stamp, ()) = read_stamped(vec![bucket.clone()], |_| {
        let later = std::time::SystemTime::now() + Duration::from_secs(60);
        let path = Path::new(&bucket).join("questions.json");
        let file = std::fs::File::options().write(true).open(path);
        let stamped = file.and_then(|file| file.set_modified(later));
        assert!(stamped.is_ok(), "{stamped:?}");
    });

    let (now, ()) = read_stamped(vec![bucket], |_| {});
    assert!(
        stamp.moved_to(&now),
        "the stamp took the write the read missed"
    );
    Ok(())
}

/// A reload whose read could not start answers an error and leaves no key
/// in `running`, so the next one reads.
#[test]
fn a_reload_that_cannot_start_lets_the_next_one_run() -> anyhow::Result<()> {
    let mut state = WorkspaceState::new();
    let key = TabKey::next();
    let first_load = ContentPatch::Loaded {
        key,
        since: None,
        loaded: loaded_tab("/repo"),
    };
    state
        .apply_patch(first_load)
        .map_err(|refused| anyhow::anyhow!("{refused}"))?;
    let github: Arc<dyn er_github::GitHub> = Arc::new(FakeGitHub::default());
    let deps = WorkspaceDeps::new(Ports {
        git: git(RAW),
        github,
    });
    static REFUSED: AtomicBool = AtomicBool::new(false);
    let refuse_once: crate::sidecar_reload::Spawn = |read| {
        if REFUSED.swap(true, Ordering::SeqCst) {
            std::thread::Builder::new().spawn(read).map(drop)
        } else {
            Err(std::io::Error::other("no threads"))
        }
    };
    let (tx, done) = channel();
    let mut reloads = SidecarReloads::with_spawn(refuse_once);
    let (reply, failed) = channel();
    reloads.start(&deps, &state, &tx, key, reply);
    assert!(
        matches!(failed.try_recv(), Ok(Err(_))),
        "the failure was not answered"
    );

    let (reply, next) = channel();
    reloads.start(&deps, &state, &tx, key, reply);

    let coalesced = Reply::SidecarsReloaded {
        key,
        reloaded: false,
    };
    assert_ne!(
        next.try_recv().ok(),
        Some(Ok(coalesced)),
        "the next reload was refused"
    );
    assert!(
        matches!(done.recv_timeout(WAIT), Ok(ActorMsg::Reloaded(_))),
        "the next reload read nothing"
    );
    Ok(())
}
