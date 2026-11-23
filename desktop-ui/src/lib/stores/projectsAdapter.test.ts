import { describe, expect, it } from "bun:test";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { Command } from "$lib/protocol/generated/Command";
import type { AppSnapshot, ProjectSnapshot, TabSummary } from "$lib/pageTypes";
import { ProjectsAdapter, branchRows, branchesKey, isActiveProject, remoteKey } from "./projectsAdapter";
import { prsKey } from "./prsAdapter";
import { ResourceStore } from "./resourceStore";

function tab(o: Partial<TabSummary> = {}): TabSummary {
  return {
    idx: 0,
    label: "t",
    kind: "working",
    branch: null,
    pr_number: null,
    remote: null,
    repo_root: "/src/b",
    is_active: true,
    change_token: "",
    stale: false,
    ...o,
  };
}

function row(id: string, o: Partial<ProjectSnapshot> = {}): ProjectSnapshot {
  return {
    id,
    name: id,
    root_path: `/src/${id}`,
    remote: null,
    is_active: false,
    local_branches: [],
    auto_branches: [],
    saved_prs: [],
    my_prs: [],
    prs_to_review: [],
    recent_prs: [],
    recently_merged: [],
    ...o,
  };
}

const pr = { number: 5, title: "Five", head_ref: "feature", state: "OPEN", is_draft: false, author: "me" };

/** The snapshot calls `a` active and holds the PR lists; the resources hold `b` first. */
const snapshot = {
  projects: [row("a", { is_active: true, my_prs: [pr as never] }), row("b")],
};

const wireProjects = [
  { id: "b", name: "b", rootPath: "/src/b", remote: null, remoteOnly: false, autoTriageMaxDiffKb: 0, reviewIgnoreGlobs: [] },
  { id: "a", name: "a", rootPath: "/src/a", remote: "Acme/A", remoteOnly: false, autoTriageMaxDiffKb: 9, reviewIgnoreGlobs: ["*.lock"] },
];

const wireBranches = (project: string) => ({
  kind: "branches",
  currentBranch: "main",
  localBranches: [
    { name: "main", upstream: "origin/main", isMerged: false, worktreePath: null, prNumber: null },
    { name: "feature", upstream: null, isMerged: false, worktreePath: `/wt/${project}`, prNumber: 5 },
  ],
  autoBranches: [],
});

function prRow(number: number, o: Record<string, unknown> = {}) {
  return {
    number,
    title: `PR ${number}`,
    headRef: `branch-${number}`,
    baseRef: "main",
    headOid: "",
    state: "OPEN",
    isDraft: false,
    author: "me",
    assignees: [],
    reviewers: [],
    checksState: null,
    reviewDecision: null,
    mergedAt: null,
    updatedAt: "",
    approvedByMe: false,
    ...o,
  };
}

/** `Acme/A`'s PR lists, as the server builds them; `saved` follows the pins sent. */
function wirePrs(saved: number[]) {
  return {
    kind: "prs",
    rows: [prRow(5, { title: "Five", headRef: "feature" }), prRow(9, { title: "Nine" })],
    fetchedAtMs: 1_000,
    projects: [{ project: "a", mine: [5], toReview: [], recentlyMerged: [], saved, recent: [], dismissed: [] }],
  };
}

function backend() {
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  let version = 3;
  const saved: number[] = [];
  const run: Invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args });
    if (command === "er_fetch") {
      // After a send the fetch answers a task later, as IPC does, so a write
      // that returned before it would see the old version.
      if (version > 3) await new Promise((r) => setTimeout(r, 5));
      const keys = args.keys as { kind: string; project?: string; remote?: string }[];
      return keys.map((key) => {
        if (key.kind === "projects") return { key, version, value: { kind: "projects", projects: wireProjects } };
        if (key.kind === "prs") {
          const value = key.remote === "Acme/A" ? wirePrs([...saved]) : { kind: "prs", rows: [], fetchedAtMs: null, projects: [] };
          return { key, version, value };
        }
        if (key.kind === "available_branches") return { key, version: 0, value: { kind: "available_branches", branches: ["spare"] } };
        return { key, version, value: wireBranches(key.project ?? "") };
      }) as T;
    }
    if (command === "er_send") {
      version += 1;
      const sent = args.command as Command;
      if (sent.kind === "save_pr") {
        saved.unshift(sent.number);
        return { changed: [{ key: prsKey("Acme/A"), version }], created: null } as T;
      }
      return { changed: [{ key: { kind: "projects" }, version }], created: null } as T;
    }
    throw Object.assign(new Error(`unexpected ${command}`), { kind: "unsupported" });
  };
  const client = resourceClient(run, async () => () => {});
  return { calls, client, store: new ResourceStore(client, 60_000) };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

/** What the old `patch_project_review_settings` answers. */
const patchedSnapshot = { projects: [] } as unknown as AppSnapshot;

function adapter(flagOn: boolean, active: TabSummary | null = tab(), invokeFails: string | null = null) {
  const { calls, client, store } = backend();
  const old: { name: string; args: Record<string, unknown> }[] = [];
  const commanded: string[] = [];
  const ingested: AppSnapshot[] = [];
  const reported: { command: string; message: string }[] = [];
  const projects = new ProjectsAdapter({
    flagOn,
    store,
    client,
    snapshot: () => snapshot,
    activeTab: () => active,
    command: async (name, args) => {
      old.push({ name, args });
      commanded.push(name);
    },
    invoke: async <T,>(name: string, args: Record<string, unknown>) => {
      old.push({ name, args });
      if (invokeFails !== null) throw invokeFails;
      return (name === "patch_project_review_settings" ? patchedSnapshot : ["old-path"]) as T;
    },
    report: (command, e) => {
      reported.push({ command, message: e.message });
    },
    ingest: (s) => {
      ingested.push(s);
    },
    now: () => 61_000,
    sleep: settle,
  });
  return { calls, old, commanded, ingested, reported, projects, store };
}

describe("ProjectsAdapter", () => {
  it("reads app.snapshot with the flag off and sends the old commands", async () => {
    const { calls, old, projects } = adapter(false);
    await settle();
    expect(projects.projects).toBe(snapshot.projects);
    await projects.reorder(["b", "a"]);
    await projects.addTrackedBranch("a", "feature");
    expect(await projects.availableBranches("a")).toEqual(["old-path"]);
    expect(old.map((c) => c.name)).toEqual(["reorder_projects", "add_tracked_branch", "list_available_branches"]);
    expect(calls).toEqual([]);
  });

  it("reads the projects and each shown project's branches through er_fetch with the flag on", async () => {
    const { calls, projects } = adapter(true);
    await settle();
    projects.showBranches(projects.projectIds);
    projects.showPrs(projects.remotes);
    await settle();
    const rows = projects.projects;
    expect(rows.map((p) => p.id)).toEqual(["b", "a"]);
    // The active tab is on /src/b, whatever the snapshot's flag says.
    expect(rows.map((p) => p.is_active)).toEqual([true, false]);
    const a = rows[1];
    // The lists come from `prs(Acme/A)`, which says nothing of the snapshot's row.
    expect(a.my_prs.map((p) => [p.number, p.title, p.head_ref])).toEqual([[5, "Five", "feature"]]);
    expect([a.pr_cache_stale, a.pr_cache_age_ms]).toEqual([false, 60_000]);
    expect(rows[0].my_prs).toEqual([]);
    expect(a.review_ignore_globs).toEqual(["*.lock"]);
    expect(a.local_branches.map((b) => [b.name, b.is_current, b.worktree_path, b.pr_number])).toEqual([
      ["main", true, null, null],
      ["feature", false, "/wt/a", 5],
    ]);
    const fetched = calls.flatMap((c) => (c.command === "er_fetch" ? (c.args.keys as unknown[]) : []));
    expect(fetched).toEqual([
      { kind: "projects" },
      { kind: "branches", project: "b" },
      { kind: "branches", project: "a" },
      { kind: "prs", remote: "Acme/A" },
    ]);
  });

  it("shows each named remote's PR lists once, keyed by the remote as the project writes it", async () => {
    const { calls, projects } = adapter(true);
    await settle();
    expect(projects.remotes).toEqual(["Acme/A"]);
    projects.showPrs(["Acme/A", "Acme/A"]);
    projects.showPrs(["Acme/A"]);
    await settle();
    const prsFetches = calls
      .flatMap((c) => (c.command === "er_fetch" ? (c.args.keys as { kind: string }[]) : []))
      .filter((k) => k.kind === "prs");
    expect(prsFetches).toEqual([{ kind: "prs", remote: "Acme/A" }]);
    expect(projects.heldPrs.map((p) => p.rows.map((r) => r.number))).toEqual([[5, 9]]);
  });

  it("sends the PR writes through the old commands with the flag off", async () => {
    const { calls, old, projects } = adapter(false);
    await projects.savePr("a", 9, "Nine");
    await projects.unsavePr("a", 9);
    await projects.dismissPr("a", 5);
    await projects.undismissPr("a", 5);
    await projects.refreshProjectPrList("a");
    await projects.refreshPrList();
    expect(old).toEqual([
      { name: "save_pr", args: { projectId: "a", prNumber: 9, title: "Nine" } },
      { name: "unsave_pr", args: { projectId: "a", prNumber: 9 } },
      { name: "dismiss_remote_pr", args: { projectId: "a", prNumber: 5 } },
      { name: "undismiss_remote_pr", args: { projectId: "a", prNumber: 5 } },
      { name: "refresh_project_pr_list", args: { projectId: "a" } },
      { name: "refresh_pr_list", args: {} },
    ]);
    expect(calls).toEqual([]);
  });

  it("with the flag on a pin paints nothing and shows once the prs push lands", async () => {
    const { calls, old, projects } = adapter(true);
    await settle();
    projects.showPrs(projects.remotes);
    await settle();
    const before = snapshot.projects[0].saved_prs;
    await projects.savePr("a", 9, "Nine");
    const sent = calls.filter((c) => c.command === "er_send").map((c) => c.args.command);
    expect(sent).toEqual([{ kind: "save_pr", project: "a", number: 9, title: "Nine" }]);
    expect(old).toEqual([]);
    // No paint on the snapshot: the row comes from the server's list.
    expect(snapshot.projects[0].saved_prs).toBe(before);
    expect(snapshot.projects[0].saved_prs).toEqual([]);
    const a = projects.projects.find((p) => p.id === "a");
    expect(a?.saved_prs.map((p) => [p.number, p.title])).toEqual([[9, "Nine"]]);
  });

  it("sends the review-settings change with every other field left as it is", async () => {
    const { calls, projects } = adapter(true);
    await projects.patchReviewSettings("a", { autoTriageMaxDiffKb: 12 });
    expect(calls.find((c) => c.command === "er_send")?.args.command).toEqual({
      kind: "patch_project_review_settings",
      project: "a",
      patch: { autoTriageMaxDiffKb: 12, reviewIgnoreGlobAdd: null, reviewIgnoreGlobRemove: null },
    });
  });

  // `app.cmd` drops a non-tab command while a tab switch is in flight, with
  // no toast; SettingsPage called the command directly before this adapter.
  it("with the flag off sends the review-settings change straight to the old command and applies its snapshot", async () => {
    const { calls, old, commanded, ingested, projects } = adapter(false);
    await projects.patchReviewSettings("a", { reviewIgnoreGlobAdd: "*.snap" });
    expect(old).toEqual([
      {
        name: "patch_project_review_settings",
        args: {
          projectId: "a",
          patch: { autoTriageMaxDiffKb: null, reviewIgnoreGlobAdd: "*.snap", reviewIgnoreGlobRemove: null },
        },
      },
    ]);
    expect(commanded).toEqual([]);
    expect(ingested).toEqual([patchedSnapshot]);
    expect(calls).toEqual([]);
  });

  it("with the flag off reports a failed review-settings change as the old toast did", async () => {
    const { ingested, reported, projects } = adapter(false, tab(), "no such project");
    await projects.patchReviewSettings("a", { autoTriageMaxDiffKb: 12 });
    expect(reported).toEqual([{ command: "patch_project_review_settings", message: "no such project" }]);
    expect(ingested).toEqual([]);
  });

  it("marks the branch the active tab views as current in the active project", async () => {
    const { projects } = adapter(true, tab({ branch: "feature" }));
    await settle();
    projects.showBranches(projects.projectIds);
    await settle();
    const [b, a] = projects.projects;
    expect(b.local_branches.filter((br) => br.is_current).map((br) => br.name)).toEqual(["feature"]);
    expect(a.local_branches.filter((br) => br.is_current).map((br) => br.name)).toEqual(["main"]);
  });

  it("sends the project commands through er_send and returns once the Ack's versions are held", async () => {
    const { calls, old, projects, store } = adapter(true);
    await settle();
    const held = () => store.get({ kind: "projects" })?.version;
    expect(held()).toBe(3);
    await projects.reorder(["a", "b"]);
    expect(held()).toBe(4);
    await projects.deleteProject("a");
    expect(held()).toBe(5);
    await projects.removeTrackedBranch("b", "feature");
    expect(held()).toBe(6);
    const sent = calls.filter((c) => c.command === "er_send").map((c) => c.args.command as Command);
    expect(sent).toEqual([
      { kind: "reorder_projects", ordered: ["a", "b"] },
      { kind: "delete_project", project: "a" },
      { kind: "remove_tracked_branch", project: "b", name: "feature" },
    ]);
    // Each send moved `projects`; the write returned after its refetch.
    const fetches = calls.filter((c) => c.command === "er_fetch").length;
    expect(fetches).toBe(1 + sent.length);
    expect(old).toEqual([]);
  });

  it("reads the available branches once through er_fetch with the flag on", async () => {
    const { calls, projects } = adapter(true);
    expect(await projects.availableBranches("a")).toEqual(["spare"]);
    expect(calls.at(-1)?.args.keys).toEqual([{ kind: "available_branches", project: "a" }]);
  });

  it("releases the branches of a project that leaves the list, and shows them again when it returns", async () => {
    const { calls, projects, store } = adapter(true);
    await settle();
    projects.showBranches(["a", "b"]);
    await settle();
    projects.showBranches(["a"]);
    expect(store.get(branchesKey("b"))).toBeUndefined();
    expect(store.get(branchesKey("a"))).toBeDefined();
    projects.showBranches(["a", "b"]);
    await settle();
    const branchFetches = calls
      .flatMap((c) => (c.command === "er_fetch" ? (c.args.keys as { kind: string; project?: string }[]) : []))
      .filter((k) => k.kind === "branches")
      .map((k) => k.project);
    expect(branchFetches).toEqual(["a", "b", "b"]);
  });
});

describe("sidebar derivations", () => {
  it("compares remotes as lower-case owner/repo", () => {
    expect(remoteKey("https://github.com/Acme/Widgets.git")).toBe("acme/widgets");
    expect(remoteKey("git@github.com:Acme/Widgets.git")).toBe("acme/widgets");
    expect(remoteKey("Acme/Widgets")).toBe("acme/widgets");
    expect(remoteKey(null)).toBeNull();
  });

  it("finds the active project by root, or by remote for a remote-only one", () => {
    const local = wireProjects[0];
    const remoteOnly = { ...wireProjects[1], rootPath: "", remoteOnly: true };
    expect(isActiveProject(local, tab())).toBe(true);
    expect(isActiveProject(local, tab({ repo_root: "/src/other" }))).toBe(false);
    expect(isActiveProject(remoteOnly, tab({ repo_root: "/tmp/x", remote: "acme/a" }))).toBe(true);
    expect(isActiveProject(remoteOnly, tab({ remote: null }))).toBe(false);
    expect(isActiveProject(local, null)).toBe(false);
  });

  it("derives the current branch from the checkout outside the active project", () => {
    const branches = wireBranches("x") as never;
    const viewed = branchRows(branches, tab({ branch: "feature" }), false);
    expect(viewed.local.filter((b) => b.is_current).map((b) => b.name)).toEqual(["main"]);
  });
});
