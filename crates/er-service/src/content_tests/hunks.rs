//! A lazy stub is named by its section of the raw diff, so a stub rewritten
//! after the page read its hash answers `Stale` instead of the new text.

use super::*;
use crate::hunks::{read_hunks, row_hash, HunkRead};

/// More files than an eager parse takes, past the lazy size; `f1` reads `text`.
fn lazy_raw(text: &str) -> String {
    use std::fmt::Write as _;

    let filler = "x".repeat(400);
    let mut raw = String::new();
    for n in 0..520 {
        let new = if n == 1 { text } else { "new" };
        let _ = write!(
            raw,
            "diff --git a/src/f{n}.rs b/src/f{n}.rs\n--- a/src/f{n}.rs\n+++ b/src/f{n}.rs\n\
             @@ -1,2 +1,2 @@\n-old {filler}\n+{new} {filler}\n ctx\n"
        );
    }
    raw
}

#[test]
fn a_stub_rewritten_after_its_hash_was_read_answers_stale() -> anyhow::Result<()> {
    with_root(|repo| {
        let key = TabKey::next();
        let read = actor_over(git(&lazy_raw("first")));
        let rewritten = actor_over(git(&lazy_raw("second")));
        load(&read, key, repo)?;
        load(&rewritten, key, repo)?;
        let before = content_of(&read.view(), key)?;
        let after = content_of(&rewritten.view(), key)?;
        let stub = |content: &crate::TabContent| {
            let file = content.diff.files.iter().find(|f| f.path == "src/f1.rs");
            file.map(|file| row_hash(content, file)).unwrap_or_default()
        };
        let asked = stub(&before);
        assert!(before.diff.meta.lazy, "the fixture did not load lazily");

        assert_eq!(read_hunks(&before, "src/f1.rs", &asked), HunkRead::Unparsed);
        assert_eq!(read_hunks(&after, "src/f1.rs", &asked), HunkRead::Stale);
        assert_eq!(
            read_hunks(&after, "src/f1.rs", &stub(&after)),
            HunkRead::Unparsed
        );
        Ok(())
    })
}
