# GitHub access shells out to the `gh` CLI

Every GitHub operation — resolving a PR URL, checking out a PR branch, pulling review comments, pushing local comments back, replying, deleting — spawns `gh` from `crates/er-engine/src/github.rs`. The engine has no HTTP client and no GitHub token, in config or in code. Reusing the authentication the user already configured with `gh auth login` deletes a whole subsystem: nothing to store, scope, refresh, or leak, and no re-auth UI to build. `gh` stays optional for the tool as a whole; PR features are the only thing that needs it, and they fail with an explicit "run `gh auth login`" rather than a 401.

## Considered Options

A REST or GraphQL client with a stored PAT or an OAuth device flow would give typed responses, real pagination, and rate-limit headers the app can read. Rejected: it moves credential handling into a tool whose appeal is being one binary that needs no setup, and it means maintaining a second copy of GitHub's API surface next to a CLI users already have installed.

## Consequences

- Rate limits arrive opaquely, so call volume has to be gated inside `er` itself — the desktop's GitHub polling loops carry their own TTLs for exactly this reason. A new periodic `gh` call is not free, and nothing in the engine reports when it starts failing on quota.
- The CLI is not portable to web or mobile. A future hosted front end can share `er-engine` only up to the GitHub boundary, where it needs its own adapter behind the same interface.
