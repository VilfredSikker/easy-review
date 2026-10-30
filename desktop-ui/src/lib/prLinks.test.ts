import { describe, expect, it } from "bun:test";
import {
  bareRefs,
  classifyLink,
  deployStatus,
  extractPrLinks,
  githubRefUrl,
  isBotAuthor,
  linearKeys,
  parseGithubRef,
} from "./prLinks";

const self = { owner: "acme", repo: "web", number: 1756 };

const VERCEL_COMMENT = `[vc]: #abc
**The latest updates on your projects**. Learn more about [Vercel for Git ↗︎](https://vercel.link/github-learn-more)

| Name | Status | Preview | Comments | Updated (UTC) |
| :--- | :----- | :------ | :------- | :------ |
| **rdp** | ✅ Ready ([Inspect](https://vercel.com/acme/rdp/AbC)) | [Visit Preview](https://rdp-git-sul-fix-migration-order.vercel.app) | 💬 [**Add feedback**](https://vercel.live/open-feedback/rdp-git-sul.vercel.app) | Oct 30, 2026 10:00am |
| **docs** | 🔄 Building ([Inspect](https://vercel.com/acme/docs/XyZ)) | [Visit Preview](https://docs-git-sul-fix-migration-order.vercel.app) | | Oct 30, 2026 10:00am |
`;

const NETLIFY_COMMENT = `### <span aria-hidden="true">✅</span> Deploy Preview for *acme-site* ready!

|  Name | Link |
|:-:|------------------------|
|<span aria-hidden="true">🔨</span> Latest commit | 7be0d14 |
|<span aria-hidden="true">😎</span> Deploy Preview | [https://deploy-preview-1756--acme-site.netlify.app](https://deploy-preview-1756--acme-site.netlify.app) |
`;

describe("parseGithubRef", () => {
  it("reads short refs against the PR's repo", () => {
    expect(parseGithubRef("#12", self)).toEqual({ owner: "acme", repo: "web", number: 12, kind: "unknown" });
    expect(parseGithubRef("other/lib#3", self)).toEqual({ owner: "other", repo: "lib", number: 3, kind: "unknown" });
    expect(parseGithubRef("#12")).toBeNull();
  });

  it("reads pull and issue URLs, including sub-pages", () => {
    expect(parseGithubRef("https://github.com/acme/web/pull/1758/files")?.kind).toBe("pull");
    expect(parseGithubRef("https://github.com/acme/web/issues/9")?.kind).toBe("issue");
    expect(parseGithubRef("https://github.com/acme/web/actions/runs/1")).toBeNull();
    expect(parseGithubRef("https://gitlab.com/acme/web/pull/1")).toBeNull();
  });

  it("builds a URL GitHub redirects for either kind", () => {
    expect(githubRefUrl({ owner: "a", repo: "b", number: 4, kind: "unknown" })).toBe("https://github.com/a/b/issues/4");
    expect(githubRefUrl({ owner: "a", repo: "b", number: 4, kind: "pull" })).toBe("https://github.com/a/b/pull/4");
  });
});

describe("classifyLink", () => {
  it("names a Vercel preview after its project and keeps the host as detail", () => {
    const link = classifyLink("https://rdp-git-sul-fix.vercel.app", { source: "description" });
    expect(link).toMatchObject({ kind: "preview", title: "Vercel preview · rdp", detail: "rdp-git-sul-fix.vercel.app", chip: "Vercel" });
    expect(link?.actions).toEqual(["browser-split", "copy"]);
  });

  it("reads a Linear issue URL's key and slug", () => {
    expect(classifyLink("https://linear.app/acme/issue/DEV-298/deploy-db-fails-on-staging", { source: "description" })).toMatchObject({
      kind: "linear",
      key: "linear:DEV-298",
      title: "DEV-298 · Deploy db fails on staging",
      actions: ["open-issue", "copy"],
    });
  });

  it("offers a PR in a tab and an issue in the browser", () => {
    expect(classifyLink("https://github.com/acme/web/pull/1758", { source: "description", self })).toMatchObject({
      kind: "pull",
      detail: "pull request",
      actions: ["open-tab", "copy"],
    });
    expect(classifyLink("https://github.com/other/lib/issues/3", { source: "description", self })).toMatchObject({
      kind: "issue",
      detail: "other/lib · issue",
      actions: ["open-issue", "copy"],
    });
  });

  it("does not list the PR under review", () => {
    expect(classifyLink("https://github.com/acme/web/pull/1756", { source: "description", self })).toBeNull();
  });

  it("recognises Actions runs and Sentry issues", () => {
    expect(classifyLink("https://github.com/acme/web/actions/runs/412/job/9", { source: "description" })).toMatchObject({
      kind: "actions",
      title: "Actions run 412 · job 9",
      actions: ["view-log", "copy"],
    });
    expect(classifyLink("https://acme.sentry.io/issues/5521/", { source: "description" })).toMatchObject({
      kind: "sentry",
      title: "Sentry issue 5521",
      detail: "Sentry · acme",
    });
  });

  it("keeps generic links from the description but not from bots", () => {
    expect(classifyLink("https://docs.example.com/guide", { source: "description", label: "Guide" })).toMatchObject({
      kind: "generic",
      title: "Guide",
      actions: ["open", "copy"],
    });
    expect(classifyLink("https://docs.example.com/guide", { source: "bot" })).toBeNull();
  });

  it("drops badge, avatar, attachment and dashboard links", () => {
    for (const url of [
      "https://img.shields.io/badge/x-y-green",
      "https://avatars.githubusercontent.com/u/1",
      "https://github.com/user-attachments/assets/abc",
      "https://vercel.com/acme/rdp/AbC",
      "https://vercel.live/open-feedback/x",
    ]) {
      expect(classifyLink(url, { source: "description" })).toBeNull();
    }
  });
});

describe("deployStatus", () => {
  it("reads the table row first, then the comment headline", () => {
    expect(deployStatus("| **rdp** | ✅ Ready |", "")).toBe("ready");
    expect(deployStatus("| **rdp** | 🔄 Building |", "")).toBe("building");
    expect(deployStatus("| **rdp** | ❌ Failed |", "")).toBe("failed");
    expect(deployStatus("| Deploy Preview | url |", "Deploy Preview for site ready!")).toBe("ready");
    expect(deployStatus("nothing", "nothing")).toBeNull();
  });
});

describe("bare keys and refs", () => {
  it("takes Linear keys only after a magic word", () => {
    expect(linearKeys("Fixes DEV-298. Uses SHA-256 and UTF-8. Part of ENG-12")).toEqual(["DEV-298", "ENG-12"]);
    expect(linearKeys("`fixes DEV-1`")).toEqual([]);
  });

  it("finds #refs in prose but not in code, URLs or headings", () => {
    const md = "# 12 things\nFollow-up: #1758 and other/lib#3\n```\n#99\n```\n`#98` see https://x.dev/#97 x#96";
    expect(bareRefs(md, self).map((r) => `${r.owner}/${r.repo}#${r.number}`)).toEqual(["acme/web#1758", "other/lib#3"]);
  });
});

describe("extractPrLinks", () => {
  it("merges the description and bot comments into ordered, de-duplicated rows", () => {
    const body = [
      "Fixes [DEV-298](https://linear.app/acme/issue/DEV-298/deploy-db-fails-on-staging).",
      "Preview: https://rdp-git-sul-fix-migration-order.vercel.app.",
      "Caused by https://github.com/acme/web/pull/1745 · follow-up #1758",
      "Failing run: https://github.com/acme/web/actions/runs/412",
      "![shot](https://example.com/shot.png)",
      "<!-- https://hidden.example.com -->",
    ].join("\n");
    const rows = extractPrLinks({
      body,
      ...self,
      comments: [
        { author: "vercel[bot]", body: VERCEL_COMMENT, created_at: "", url: "" },
        { author: "martin", body: "see https://human.example.com", created_at: "", url: "" },
      ],
    });
    expect(rows.map((r) => [r.kind, r.title, r.chip])).toEqual([
      ["preview", "Vercel preview · rdp", "Ready"],
      ["preview", "Vercel preview · docs", "Building"],
      ["linear", "DEV-298 · Deploy db fails on staging", "Linear"],
      ["pull", "#1745", "PR"],
      ["ref", "#1758", "Ref"],
      ["actions", "Actions run 412", "CI"],
    ]);
    expect(rows[0].source).toBe("description");
    expect(rows[0].status).toBe("ready");
  });

  it("reads a Netlify preview and its status from the bot's headline", () => {
    const rows = extractPrLinks({ body: "", comments: [{ author: "netlify[bot]", body: NETLIFY_COMMENT, created_at: "", url: "" }] });
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ kind: "preview", title: "Netlify preview · acme-site", chip: "Ready", status: "ready" });
  });

  it("links a bare Linear key once a bot names the workspace", () => {
    const rows = extractPrLinks({
      body: "Closes ENG-7",
      comments: [{ author: "linear[bot]", body: "[DEV-1](https://linear.app/acme/issue/DEV-1/x)", created_at: "", url: "" }],
    });
    expect(rows.map((r) => r.key)).toEqual(["linear:DEV-1", "linear:ENG-7"]);
    expect(rows[1].url).toBe("https://linear.app/acme/issue/ENG-7");
  });

  it("knows bots by their app suffix or a known login", () => {
    expect(isBotAuthor("vercel[bot]")).toBe(true);
    expect(isBotAuthor("vercel")).toBe(true);
    expect(isBotAuthor("martin")).toBe(false);
  });
});
