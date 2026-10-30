# Browser proof fixtures

Run `bun tests/fixtures/browser/serve.ts`. The server listens on ports 5173 and
3000 at both `localhost` and `127.0.0.1`. Stop it with Ctrl-C. Each document
shows its origin, port, and a fresh document ID; a same-document navigation
retains the ID. Use `localhost` for the cross-origin iframe test, which points
to `127.0.0.1` on the same port.

| Page | Scenario |
| --- | --- |
| `landing.html` | Links to the fixtures |
| `spa.html` | Push, replace, and hash navigation |
| `form.html` | Input, textarea, and long document |
| `frames.html` | Two same-origin frames with matching button selectors |
| `cross-origin.html` | Frame on the numeric host |
| `shadow.html` | Open and closed shadow roots |
| `nested-scroll.html` | Scroll container with 50 buttons |
| `responsive.html` | Layout changes at 600px |
| `dialog.html` | Native modal dialog |
| `popup.html` | Activated popups and an unactivated timer popup |
| `hostile.html` | Forged invocation, synthetic events, and keyboard listeners |
| `canvas.html` | Drawn 600 by 400 canvas |
| `password.html` | Password, hidden value, and token attribute |

Routes include `/redirect/path`, `/redirect/origin`, `/redirect/chain`,
`/slow?ms=N` (integer 0–10000), `/status/404`, `/status/500`, `/set-cookie`,
and `/read-cookie`. The server is intended for local testing only.

Run `bun test tests/fixtures/browser/serve.test.ts` for routing checks. These
checks do not replace Safari or native-window observations.
