# Stores

`desktop-ui/src/lib/stores/` — one store per file plus the pure helpers they import. The directory is the map. This file holds only the rules that span it.

## State ownership

- The resource store (`resources`) is backend truth at rest. Each store reads its keys from it, and the page view (`pageView`) is built from those stores alone, each laying its fields over the tab strip's rows; it holds no state of its own. Render from it, read loading state from the resources that carry it, and never infer backend state from toasts.
- A store shows a key while something holds it (`show` returns the release). A key nobody holds is dropped with its content, so a store that reads a key it never showed reads nothing.
- While a write is in flight the page runs ahead of the server on purpose; see pending writes below.
- localStorage holds frontend preferences only: diff view mode, comment visibility, drawer sizes, scroll offsets. Never review data, and never a wire key.

## Pending writes

- A thread write (add, reply, promote) paints a pending row under the id it sends as `clientId`, so the confirming row lands on the same id and is not drawn twice (`withPending`). This is ADR 0017's `client_id` rule, which ADR 0072 keeps.
- The paint is dropped once every version the `Ack` names is held (`whenAt`), or when the send fails. A refused write takes its paint back with no rollback code; dropping it any earlier flashes the row out until the refetch lands.
- A reviewed checkmark is pending the same way, until its `Ack`'s versions are held.
- Every other write paints nothing: the push of the resource it moved paints the result.
- Composers fire `void app.cmd(...)` and clear their draft after, never `await`.

## Commands: `app.cmd` and raw `invoke`

`app.cmd` sends a command through the first route a store handed it (`routeCmds`); each store routes the commands it owns to `er_send`. A name no route takes is an error, and every failure goes to the banner, the toast and the log. What a command changed reaches the page through the resources, never through its return.

Some stores send through the resource store directly rather than through `app.cmd`, the settings and session adapters among them. Their sends reject with the `ErError`, and the caller reports it; nothing else will.

Raw `invoke` is for the direct commands, which are not published state: the exports, the terminal, the browser webview, the OS launches and line commits. The protocol client (`desktop-ui/src/lib/protocol/client.ts`) is the one place the three resource commands are invoked. A raw `invoke` reports its own failure; nothing else will.

## Ordering guards

Answers arrive out of order (a push, an `Ack`, a version check), and the resource store defends itself:

- A key has at most one fetch in flight. A version announced while it is in flight is remembered, and the key is fetched again once the fetch lands if that version is still ahead.
- An older reply never replaces a newer one: a fetch whose version is at or below the held one is dropped.
- A store that needs a key at a version waits on `whenAt`, which also resolves when the key is released or its fetch fails, so nothing waits on a key no one shows.

Any new path that holds content outside the resource store needs the same guards, or it is the stale copy.

## Push model (ADR 0072)

The backend pushes `er://resources` with the keys whose version moved; the store refetches those it shows. The 30s timer asks for the versions of every shown key, for a push that was lost or one sent before the listener attached. A stale-looking UI points at a writer that did not stamp a version, or a relay that did not hand it on; shortening the interval hides that.

## Browser URL identity

Annotation matching keys on a canonical URL: use the `browserUrl.ts` helpers, never hand-parsed URLs in components. Dev URLs must stay on `localhost` — `127.0.0.1` is a different origin for cookies and for the proxy's same-origin redirect.

## Keyboard

One global handler in `keyboard.ts`; components register no global shortcuts and stop propagation in their own inputs. Guard scope: focus inside the terminal (`.xterm`) swallows everything except Cmd/Ctrl+T; bare keys stop at fields (`INPUT`/`TEXTAREA`/`SELECT`, contentEditable) and at an open modal; a few named chords opt out of the field guard. Escape is owned by the handler, first match wins: palette → modal stack → popover / search bar → diff selection → identifier highlight → annotation composer → blur the field.
