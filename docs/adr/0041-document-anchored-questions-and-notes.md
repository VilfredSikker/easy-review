# Questions and notes can anchor on a document line outside the diff

The desktop Preview shows a whole Markdown or text file, and a reviewer highlights passages in it to ask a question or leave a note. Most of what Preview shows is unchanged, so most highlights land on lines no hunk holds. Until now every anchor was a hunk line: the engine filled `line_content` and context from the hunk, and relocation searched hunks. An anchor on an unchanged line came out empty and was marked lost on the next relocation pass.

A question or note made from a line outside the diff is stored with `hunk_index: None` and `line_start: Some`, with `line_content` and three lines of context taken from the file's full text. `ReviewQuestion::is_document_anchor` names that combination, and relocation skips it the same way it skips file-level items. No field was added: the two stores already serialize both fields as optional, and the TUI already lists a hunkless item with the file's unanchored comments, so older builds read these sidecars without change.

A highlight whose lines all sit in one hunk is stored as an ordinary hunk anchor. It shows in the diff view as well, and it can be a GitHub comment.

## Consequences

- Only questions and notes can be document anchors. GitHub accepts review comments on diff lines only, so the composer offers Comment only when every highlighted line is in a hunk, and the engine refuses a document-anchored GitHub comment. ADR 0007 already keeps the private stores apart from the pushed one.
- A document anchor does not follow edits. Relocation works on hunks, so a document anchor keeps its line number when lines are added above it, and it is never marked stale. The saved text starts with the highlighted words as a `>` quote, so the reader can still tell what it referred to. Relocating it against the file text would mean reading every such file on each diff refresh, a network fetch for a remote review.
- Promoting a document-anchored question to a GitHub comment is refused before anything is written, for the reason above. Promoting it to a note, and replying to it, keep the document anchor.
- The desktop command reads the file text outside the app lock, as the preview does, and refuses the save if the file changed in between.
