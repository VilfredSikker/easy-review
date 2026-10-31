import type { GithubStatusSnapshot } from "$lib/types";
import { githubStatusOpen } from "./fixtures";

// Comment bodies as deploy and tracker bots post them on GitHub, kept close to
// the real thing: a link-reference line carrying a long base64 payload, raw
// HTML (anchors, <picture>, <details>, tables) and bookkeeping comments. They
// are what pushed the Overview sideways when shown as plain markdown text.

const VERCEL_STATE =
  "eyJpc01vbm9yZXBvIjp0cnVlLCJ0eXBlIjoiZ2l0aHViIiwicHJvamVjdHMiOlt7Im5hbWUiOiJ3ZWIiLCJyb290RGlyZWN0b3J5IjoiYXBwcy93ZWIiLCJpbnNwZWN0b3JVcmwiOiJodHRwczovL3ZlcmNlbC5jb20vYWNtZS93ZWIvM0ZxY1h5UjdKZ2hWbTR0Wk5rWGRQcVc4c0x5RSIsInByZXZpZXdVcmwiOiJ3ZWItZ2l0LWZpeC1taWdyYXRpb24tb3JkZXItYWNtZS52ZXJjZWwuYXBwIiwibmV4dENvbW1pdFN0YXR1cyI6IkRFUExPWUVEIiwibGl2ZUZlZWRiYWNrIjp7InJlc29sdmVkIjowLCJ1bnJlc29sdmVkIjowLCJ0b3RhbCI6MCwibGluayI6IndlYi1naXQtZml4LW1pZ3JhdGlvbi1vcmRlci1hY21lLnZlcmNlbC5hcHAifX1dfQ==";

/** Vercel's deployment summary: a hidden state line, a markdown table, a skipped-deploys <details>. */
export const VERCEL_COMMENT_BODY = `[vc]: #cZBX/0gIGmMnCZ8tBQyy2cnfu4PFkFnS5ztHZPpj7ZU=:${VERCEL_STATE}
**The latest updates on your projects**. Learn more about [Vercel for GitHub ↗︎](https://vercel.link/github-learn-more)

| Name | Status | Preview | Comments | Updated (UTC) |
| :--- | :----- | :------ | :------- | :------ |
| **web** | ✅ Ready ([Inspect](https://vercel.com/acme/web/3FqcXyR7JghVm4tZNkXdPqW8sLyE)) | [Visit Preview](https://web-git-fix-migration-order-acme.vercel.app) | 💬 [**Add feedback**](https://vercel.live/open-feedback/web-git-fix-migration-order-acme.vercel.app?via=pr-comment-feedback-link) | Oct 30, 2026 2:14pm |

<details><summary>2 Skipped Deployments</summary>

| Name | Status | Preview | Comments | Updated (UTC) |
| :--- | :----- | :------ | :------- | :------ |
| **docs** | ⬜️ Ignored ([Inspect](https://vercel.com/acme/docs/9kQwErTyUiOp)) | [Visit Preview](https://docs-git-fix-migration-order-acme.vercel.app) | | Oct 30, 2026 2:14pm |
| **storybook** | ⬜️ Ignored ([Inspect](https://vercel.com/acme/storybook/7hGfDsAzXcVb)) | [Visit Preview](https://storybook-git-fix-migration-order-acme.vercel.app) | | Oct 30, 2026 2:14pm |

</details>
`;

/** Vercel's newer layout: everything in raw HTML, with theme-switching <picture> badges. */
export const VERCEL_HTML_COMMENT_BODY = `[vc]: #Qm9vdHN0cmFw/aGVsbG8=:${VERCEL_STATE}
<!-- vercel-deployment-summary -->
<p><strong>The latest updates on your projects</strong>. Learn more about <a href="https://vercel.link/github-learn-more">Vercel for GitHub</a>.</p>
<table>
<thead><tr><th align="left">Project</th><th align="left">Deployment</th><th align="left">Preview</th><th align="left">Comments</th><th align="left">Updated (UTC)</th></tr></thead>
<tbody><tr>
<td><a href="https://vercel.com/acme/web">web</a></td>
<td><a href="https://vercel.com/acme/web/3FqcXyR7JghVm4tZNkXdPqW8sLyE"><picture><source media="(prefers-color-scheme: dark)" srcset="https://vercel.com/static/status/ready-dark.svg"><img src="https://vercel.com/static/status/ready.svg" alt="Ready" width="16" height="16"></picture> Ready</a></td>
<td><a href="https://web-git-fix-migration-order-acme.vercel.app">Preview</a>, <a href="https://vercel.live/open-feedback/web-git-fix-migration-order-acme.vercel.app?via=pr-comment-feedback-link">Comment</a></td>
<td>0 unresolved</td>
<td>Oct 30, 2026 2:14pm</td>
</tr></tbody>
</table>
`;

/** Linear's link-back: a marker comment and an HTML paragraph, plus the issue's description folded away. */
export const LINEAR_COMMENT_BODY = `<!-- linear-linkback -->
<p><a href="https://linear.app/acme/issue/DEV-298/deploy-db-fails-on-out-of-order-migrations">DEV-298 deploy-db fails on out-of-order migrations</a></p>

<details>
<summary>Issue description</summary>

<p>Since #1745 merged, <code>deploy-staging / deploy-db</code> fails with <code>migration 20261027120000 is out of order with already applied migrations</code>. Staging logs: https://grafana.acme.internal/explore?orgId=1&left=%7B%22datasource%22%3A%22loki%22%2C%22queries%22%3A%5B%7B%22expr%22%3A%22%7Bapp%3D%5C%22deploy-db%5C%22%7D%22%7D%5D%7D</p>

</details>
`;

/** A person pasting a log line and a long URL: the cases that wrap or scroll in place. */
export const HUMAN_LONG_COMMENT_BODY = `Still red on my machine after \`just reseed\`:

\`\`\`text
Error: migration 20261029093600_hyperscale_threads is out of order with already applied migrations [20261029093502_hyperscale_hypotheses, 20261029093700_hyperscale_thread_answers] (see https://docs.acme.dev/db/migrations#ordering)
\`\`\`

Run: https://github.com/acme/web/actions/runs/11847562930/job/33012345678?pr=1756#step:7:142-the-rest-of-this-very-long-anchor-that-never-breaks

Could we also re-check the down migrations? I went through each of them by hand and two still drop the column before the index, which fails on Postgres 15. Happy to pair on it tomorrow morning if that helps get this out before the release cut.

1. \`hyperscale_threads\` drops \`thread_id\` first
2. \`hyperscale_thread_answers\` does the same
3. The rest look fine

Last thing: the seed script still references the old version numbers in two places.`;

const minutesAgo = (m: number) => new Date(Date.now() - m * 60_000).toISOString();
const comment = (author: string, body: string, m: number, id: string) => ({
  author,
  body,
  created_at: minutesAgo(m),
  url: `https://github.com/acme/web/pull/1756#issuecomment-${id}`,
});

/** An open PR whose conversation is mostly bots, then one long human reply. */
export const botConversation: GithubStatusSnapshot = {
  ...githubStatusOpen,
  owner: "acme",
  repo: "web",
  number: 1756,
  created_at: minutesAgo(21),
  comments_count: 4,
  recent_comments: [
    comment("linear", LINEAR_COMMENT_BODY, 20, "1"),
    comment("vercel", VERCEL_COMMENT_BODY, 19, "2"),
    comment("vercel", VERCEL_HTML_COMMENT_BODY, 17, "3"),
    comment("maria-c", HUMAN_LONG_COMMENT_BODY, 14, "4"),
  ],
  recent_reviews: [{ author: "martin", state: "APPROVED", body: "", submitted_at: minutesAgo(12) }],
};
