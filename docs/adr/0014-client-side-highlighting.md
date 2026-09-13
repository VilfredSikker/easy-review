# Snapshots carry plain text; highlighting is the consumer's job

Diff line snapshots ship line numbers, kind, and text — no spans. The desktop highlights client-side in a web worker (`desktop-ui/src/lib/highlightWorker.ts`) with a 50-file LRU cache, while the TUI keeps syntect in Rust. Serializing spans over IPC dominated snapshot cost: every poll paid to ship coloring the consumer could derive itself, and spans multiplied a payload already throttled by a line budget. Span generation must not return to the snapshot builder.

## Consequences

- The worker tokenizes each line with fresh grammar state. A diff hunk is not contiguous source, so a multi-line construct whose opener sits outside the hunk — a closing `"""` fence arriving as context, say — would otherwise flip the lexer into string or comment mode and miscolor every line after it. Losing cross-line context is the accepted cost of being immune to that poisoning.
- Two independent highlighters mean the TUI and desktop can color the same token differently. That is expected, not drift to reconcile: a wrong color is fixed in the worker's grammar or theme, never by pushing spans back over IPC.
