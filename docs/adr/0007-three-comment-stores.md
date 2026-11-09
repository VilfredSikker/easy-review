# Three separate comment stores, private by default

A review produces three kinds of annotation that look alike but have different audiences, so each lives in its own file: `questions.json` for things you want answered, `notes.json` for instructions to an agent, and `github-comments.json` for the shared, two-way-synced GitHub review threads. Questions and notes are private, and no push path can reach them: separate files make privacy a property of which file the data is in, so a private review note cannot reach a pull request even when someone later adds a new push path or forgets a filter. `ai/annotation_store.rs` holds the store-backed reader and writer for the three files. Promote-to-comment (`ai::promote::promote_to_github`, reached through the `promote_to_comment` command) is the single deliberate crossing from private to shared, and it is always an explicit act by the reviewer.

## Considered Options

**One store with a `type` discriminator.** The obvious shape, and the one a maintainer will reach for on seeing three files of near-identical JSON. It was rejected because privacy would then depend on a filter applied on the way out — one missed filter, or one new push path, and a private note lands on a pull request.

## Consequences

The split is at the file level only: notes reuse the `PrivateAnnotation` struct with an `n-` id prefix, and both private stores share the desktop Notes panel. One struct behind two stores is deliberate — do not collapse the stores on the assumption that the shared struct and shared panel already make them one thing.

`ai/promote.rs` is the only conversion from a private annotation to a GitHub write; there is no `From`/`Into` between `PrivateAnnotation` and the write types.
