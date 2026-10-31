import type { Meta, StoryObj } from "@storybook/svelte";
import OverviewLinks from "$lib/components/overview/OverviewLinks.svelte";

const VERCEL = `| Name | Status | Preview | Updated (UTC) |
| :--- | :----- | :------ | :------ |
| **rdp** | ✅ Ready ([Inspect](https://vercel.com/acme/rdp/AbC)) | [Visit Preview](https://rdp-git-sul-fix-migration-order.vercel.app) | Oct 30, 2026 |
| **docs** | ❌ Failed ([Inspect](https://vercel.com/acme/docs/XyZ)) | [Visit Preview](https://docs-git-sul-fix-migration-order.vercel.app) | Oct 30, 2026 |`;

const meta = {
  title: "Overview/Links",
  component: OverviewLinks,
  parameters: { layout: "padded" },
  args: {
    owner: "acme",
    repo: "web",
    number: 1756,
    body: [
      "Fixes [DEV-298](https://linear.app/acme/issue/DEV-298/deploy-db-fails-on-staging).",
      "Caused by https://github.com/acme/web/pull/1745, follow-up #1758.",
      "Failing run: https://github.com/acme/web/actions/runs/412",
      "Crash: https://acme.sentry.io/issues/5521/",
      "Runbook: [Deploy errors](https://docs.example.com/runbooks/deploy-db)",
    ].join("\n"),
    comments: [{ author: "vercel[bot]", body: VERCEL, created_at: "2026-10-30T10:00:00Z", url: "" }],
    onAction: () => {},
  },
} satisfies Meta<typeof OverviewLinks>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Previews with their bot-reported status, a Linear issue, PRs, a CI run,
 *  Sentry and a generic link, each with its quick actions. */
export const FromDescriptionAndBots: Story = {};

/** No links: the section draws nothing. */
export const None: Story = {
  args: { body: "Plain description.", comments: [] },
};
