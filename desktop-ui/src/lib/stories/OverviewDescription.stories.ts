import type { Meta, StoryObj } from "@storybook/svelte";
import OverviewDescription from "$lib/components/overview/OverviewDescription.svelte";

// Stands in for the backend's authenticated attachment fetch.
const screenshot =
  "data:image/svg+xml;utf8," +
  encodeURIComponent(
    "<svg xmlns='http://www.w3.org/2000/svg' width='640' height='160'><rect width='640' height='160' rx='8' fill='#111318'/>" +
      "<text x='18' y='48' font-family='monospace' font-size='13' fill='#ef5f5b'>Error: migration `20261027120000` is out of order</text></svg>",
  );

const BODY = `\`deploy-staging / deploy-db\` has been failing on main since #1745:

\`\`\`sql
-- out of order with already applied migrations
SELECT version FROM schema_migrations ORDER BY version;
\`\`\`

This renumbers the three Hyperscale migrations into the gap and regenerates **schema.sql**.

![deploy-db run 412 failing](https://github.com/user-attachments/assets/0b7c-screenshot)

> [!WARNING]
> Anyone who ran the Hyperscale PR locally must run \`just reseed\` before pulling this.

> [!NOTE]
> The SQL bodies are unchanged; only versions move.

### Checklist

- [x] Reproduced the staging error against a DB at \`20261029093502\`
- [x] Applied cleanly under \`--strict\`
- [ ] Deploy to staging and confirm deploy-db passes
- [ ] Ping @martin to reseed the shared dev DB

| Migration | Old version | New version |
|---|---|---|
| hyperscale_threads | \`20261027120000\` | \`20261029093600\` |
| hyperscale_hypotheses | \`20261027150000\` | \`20261029093700\` |
| hyperscale_thread_answers | \`20261027180000\` | \`20261029093800\` |

<details><summary>Why not squash into one migration?</summary>

Squashing would rewrite versions that staging has not applied yet but some local DBs have.
</details>

Fixes DEV-298 · Follow-up: #1758`;

const meta = {
  title: "Overview/Description",
  component: OverviewDescription,
  parameters: { layout: "padded" },
  args: {
    body: BODY,
    owner: "acme",
    repo: "web",
    loadImage: () => Promise.resolve(screenshot),
    onOpenRef: () => {},
  },
} satisfies Meta<typeof OverviewDescription>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Alerts, a task list, a scrolling table, <details>, highlighted SQL, a
 *  mention and #refs, and an attachment loaded through the backend. */
export const Rich: Story = {};

/** The attachment fetch fails: the image becomes a link to open it on GitHub. */
export const ImageUnavailable: Story = {
  args: { loadImage: () => Promise.reject(new Error("404")) },
};

export const Empty: Story = {
  args: { body: "" },
};
