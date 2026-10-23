/**
 * Pure view-model for the GitHub card's merge box: what GitHub's PR page shows
 * under the checks (headline, merge button, update branch, delete/restore
 * branch) plus the overflow menu. Kept out of the component so every state is
 * unit-tested away from the DOM.
 */
import type { GithubStatusSnapshot } from "$lib/types";
import { countChecks } from "$lib/checkCounts";

export type MergeMethod = "merge" | "squash" | "rebase";
export type MergeBoxTone = "ok" | "warn" | "danger" | "muted" | "merged";

/** Mirrors `PrActionRequest` in `crates/er-desktop/src/gh_pr_actions.rs`. */
export type PrActionRequest =
  | {
      kind: "merge";
      method: MergeMethod;
      auto: boolean;
      /** Merge now past unmet requirements (`gh pr merge --admin`). */
      admin: boolean;
      expected_head: string;
      delete_branch: boolean;
    }
  | { kind: "disable_auto_merge" }
  | { kind: "update_branch"; rebase: boolean }
  | { kind: "close" }
  | { kind: "reopen" }
  | { kind: "mark_ready" }
  | { kind: "convert_to_draft" }
  | { kind: "delete_branch" }
  | { kind: "restore_branch" };

export type PrActionKind = PrActionRequest["kind"];

export interface MenuItem {
  action: Exclude<PrActionRequest, { kind: "merge" }>;
  label: string;
  /** Asks for a second click before it runs. */
  destructive: boolean;
}

export interface MergeBoxModel {
  phase: "open" | "merged" | "closed";
  tone: MergeBoxTone;
  headline: string;
  detail: string | null;
  /** False hides every write action (READ / TRIAGE access). */
  canWrite: boolean;
  /** Empty on a merge-queue base, where the queue picks the method. */
  methods: MergeMethod[];
  /** The base has a merge queue: merging only enqueues the PR. */
  mergeQueue: boolean;
  /** The merge button: absent when the PR is not open or is a draft. */
  merge: { enabled: boolean; auto: boolean; reason: string | null } | null;
  /** Offer "Merge without waiting for requirements" (bypass rules). */
  offerBypass: boolean;
  /** Method of a pending auto-merge. */
  autoMerge: MergeMethod | null;
  /** Update branch shown as a button (out of date) rather than only in the menu. */
  updateProminent: boolean;
  /** Offer "Delete branch after merge" — off when GitHub deletes it anyway. */
  offerDeleteOnMerge: boolean;
  /** Ready for review shown as the primary action for a draft. */
  readyPrimary: boolean;
  /** Delete / Restore / Reopen shown in the box after merge or close. */
  primaryActions: MenuItem[];
  menu: MenuItem[];
}

export const METHOD_ORDER: MergeMethod[] = ["merge", "squash", "rebase"];

export function methodLabel(method: MergeMethod): string {
  if (method === "squash") return "Squash and merge";
  if (method === "rebase") return "Rebase and merge";
  return "Create a merge commit";
}

export function mergeButtonLabel(method: MergeMethod, auto: boolean, queue = false): string {
  // On a merge-queue base the queue, not this PR, merges and picks the method.
  if (queue) return "Add to merge queue";
  if (auto) return `Enable auto-merge (${method})`;
  if (method === "merge") return "Merge pull request";
  return methodLabel(method);
}

export function allowedMethods(github: GithubStatusSnapshot): MergeMethod[] {
  const s = github.repo_merge;
  // Settings unknown: offer every method and let GitHub refuse.
  if (!s) return [...METHOD_ORDER];
  const allowed = METHOD_ORDER.filter(
    (m) =>
      (m === "merge" && s.merge_commit_allowed) ||
      (m === "squash" && s.squash_merge_allowed) ||
      (m === "rebase" && s.rebase_merge_allowed),
  );
  return allowed.length > 0 ? allowed : [...METHOD_ORDER];
}

/** The remembered method when it is still allowed, else the first allowed. */
export function pickMethod(allowed: MergeMethod[], remembered: string | null): MergeMethod {
  const hit = allowed.find((m) => m === remembered);
  return hit ?? allowed[0] ?? "merge";
}

export function canWrite(github: GithubStatusSnapshot): boolean {
  const p = github.repo_merge?.viewer_permission;
  // Unknown permission: show the actions and let GitHub decide.
  if (!p) return true;
  return p === "ADMIN" || p === "MAINTAIN" || p === "WRITE";
}

function toMethod(raw: string | null | undefined): MergeMethod | null {
  const m = raw?.toLowerCase();
  return m === "merge" || m === "squash" || m === "rebase" ? m : null;
}

/** Why GitHub blocks the merge, in the order its merge box lists reasons. */
function blockedReason(github: GithubStatusSnapshot): string {
  if (github.review_decision === "CHANGES_REQUESTED") return "Changes requested";
  if (github.review_decision === "REVIEW_REQUIRED") return "Review required";
  const { fail: failing, pending } = countChecks(github.checks);
  // The checks list does not say which are required, so neither does this.
  if (failing > 0) return `${failing} check${failing === 1 ? "" : "s"} failing`;
  if (pending > 0) return "Checks have not finished";
  return "Branch protection rules are not met";
}

/**
 * The headline detail names the branch, so the button does not. A fork's
 * branch is not ours to delete or restore (ADR 0040).
 */
function branchItem(github: GithubStatusSnapshot): MenuItem | null {
  if (github.is_cross_repository) return null;
  if (github.head_branch_exists === true) {
    return { action: { kind: "delete_branch" }, label: "Delete branch", destructive: true };
  }
  if (github.head_branch_exists === false && github.head_oid) {
    return { action: { kind: "restore_branch" }, label: "Restore branch", destructive: false };
  }
  return null;
}

function branchDetail(github: GithubStatusSnapshot): string | null {
  if (github.head_branch_exists === false) return `Branch ${github.head_ref} was deleted`;
  if (github.head_branch_exists === true) return `Branch ${github.head_ref} is still on GitHub`;
  return null;
}

function finishedModel(github: GithubStatusSnapshot, base: MergeBoxModel): MergeBoxModel {
  const merged = github.state === "MERGED";
  const branch = branchItem(github);
  const primaryActions: MenuItem[] = [];
  // GitHub refuses to reopen a PR whose head branch is gone.
  if (!merged && github.head_branch_exists !== false) {
    primaryActions.push({ action: { kind: "reopen" }, label: "Reopen", destructive: false });
  }
  if (branch) primaryActions.push(branch);
  return {
    ...base,
    phase: merged ? "merged" : "closed",
    tone: merged ? "merged" : "danger",
    headline: merged ? "Pull request merged" : "Closed without merging",
    detail: branchDetail(github),
    primaryActions: base.canWrite ? primaryActions : [],
  };
}

function openMenu(github: GithubStatusSnapshot): MenuItem[] {
  const draftItem: MenuItem = github.is_draft
    ? { action: { kind: "mark_ready" }, label: "Ready for review", destructive: false }
    : { action: { kind: "convert_to_draft" }, label: "Convert to draft", destructive: false };
  return [
    { action: { kind: "update_branch", rebase: false }, label: "Update branch (merge)", destructive: false },
    { action: { kind: "update_branch", rebase: true }, label: "Update branch (rebase)", destructive: false },
    draftItem,
    { action: { kind: "close" }, label: "Close pull request", destructive: true },
  ];
}

function unstableDetail(github: GithubStatusSnapshot): string | null {
  const { fail: failing, pending } = countChecks(github.checks);
  if (failing > 0) return `${failing} failing`;
  if (pending > 0) return `${pending} pending`;
  return null;
}

export function mergeBoxModel(github: GithubStatusSnapshot): MergeBoxModel {
  const write = canWrite(github);
  const base: MergeBoxModel = {
    phase: "open",
    tone: "muted",
    headline: "",
    detail: null,
    canWrite: write,
    methods: github.base_has_merge_queue ? [] : allowedMethods(github),
    mergeQueue: github.base_has_merge_queue === true,
    merge: null,
    autoMerge: null,
    offerBypass: false,
    updateProminent: false,
    offerDeleteOnMerge: false,
    readyPrimary: false,
    primaryActions: [],
    menu: [],
  };
  if (github.state === "MERGED" || github.state === "CLOSED") return finishedModel(github, base);

  const menu = openMenu(github);
  const open: MergeBoxModel = {
    ...base,
    menu: write ? menu : [],
    // A queued merge lands later, so a delete now would drop the PR from the
    // queue; the backend skips it, and offering it would promise a no-op.
    offerDeleteOnMerge:
      write &&
      !github.is_cross_repository &&
      !github.repo_merge?.delete_branch_on_merge &&
      !github.base_has_merge_queue,
  };

  const autoMerge = toMethod(github.auto_merge_method);
  if (autoMerge) {
    return {
      ...open,
      tone: "ok",
      headline: "Auto-merge enabled",
      detail: `Merges with ${methodLabel(autoMerge).toLowerCase()} once requirements are met`,
      // Disable auto-merge is the box's own button, so not repeated here.
      autoMerge,
    };
  }

  if (github.in_merge_queue) {
    // On a merge-queue branch `gh pr merge` only enqueues; the PR stays open
    // until the queue lands it, so offering Merge again would be wrong.
    return {
      ...open,
      tone: "ok",
      headline: "Queued to merge",
      detail: "GitHub merges it once the queue's checks pass",
    };
  }

  if (github.is_draft || github.merge_state_status === "DRAFT") {
    return {
      ...open,
      tone: "muted",
      headline: "This pull request is still a draft",
      detail: "Mark it ready for review to merge",
      readyPrimary: write,
    };
  }
  return mergeStateModel(github, open);
}

/** An open, ready-for-review PR without auto-merge: GitHub's merge state decides. */
function mergeStateModel(github: GithubStatusSnapshot, open: MergeBoxModel): MergeBoxModel {
  const write = open.canWrite;
  const status = github.merge_state_status ?? (github.mergeable === "CONFLICTING" ? "DIRTY" : null);
  const mergeOk = { enabled: true, auto: false, reason: null };
  switch (status) {
    case "CLEAN":
    case "HAS_HOOKS":
      return { ...open, tone: "ok", headline: "Ready to merge", merge: mergeOk };
    case "UNSTABLE":
      return {
        ...open,
        tone: "warn",
        headline: "Ready to merge, with non-required checks not passing",
        detail: unstableDetail(github),
        merge: mergeOk,
      };
    case "BEHIND":
      return {
        ...open,
        tone: "warn",
        headline: "This branch is out-of-date with the base branch",
        detail: `Merge the latest changes from ${github.base_ref} into this branch`,
        updateProminent: write,
        merge: { enabled: false, auto: false, reason: "Update the branch first" },
      };
    case "DIRTY":
      return {
        ...open,
        tone: "danger",
        headline: "This branch has conflicts that must be resolved",
        detail: "Resolve them locally or on GitHub",
        merge: { enabled: false, auto: false, reason: "Resolve conflicts first" },
      };
    case "BLOCKED":
      return {
        ...open,
        tone: "warn",
        headline: "Merging is blocked",
        detail: blockedReason(github),
        // Auto-merge lands once the block clears, but only on repos that
        // enabled it; elsewhere GitHub would refuse the button.
        merge: github.repo_merge?.auto_merge_allowed
          ? { enabled: true, auto: true, reason: null }
          : { enabled: false, auto: false, reason: "Auto-merge is not enabled for this repository" },
        // The backend asks GitHub only for a blocked PR on a base without a
        // merge queue, where `--admin` would also skip the queue.
        offerBypass: write && github.can_bypass_rules === true && !open.mergeQueue,
      };
    default:
      return {
        ...open,
        tone: "muted",
        headline: "Checking mergeability…",
        detail: "GitHub is still computing whether this can merge",
        merge: { enabled: false, auto: false, reason: "GitHub is still checking mergeability" },
      };
  }
}

export interface MergeRequestOptions {
  auto: boolean;
  deleteBranch: boolean;
  /**
   * Head of the PR diff on screen. The merge is pinned to it when there is
   * one, so a push after the review makes GitHub refuse the merge (ADR 0040).
   * A local-branch view has none; the live PR head is pinned.
   */
  prDiffHeadOid?: string | null;
  /** Merge now past unmet requirements (`--admin`). */
  bypass?: boolean;
}

export function mergeRequest(
  github: GithubStatusSnapshot,
  method: MergeMethod,
  { auto, deleteBranch, prDiffHeadOid, bypass = false }: MergeRequestOptions,
): PrActionRequest {
  // A bypass merge lands now, so it never waits as an auto-merge.
  const isAuto = auto && !bypass;
  return {
    kind: "merge",
    method,
    auto: isAuto,
    admin: bypass,
    expected_head: prDiffHeadOid || github.head_oid || "",
    delete_branch: deleteBranch && !isAuto,
  };
}

/** The merge button once the bypass checkbox is applied. */
export function effectiveMerge(model: MergeBoxModel, bypass: boolean): MergeBoxModel["merge"] {
  if (model.merge && bypass && model.offerBypass) return { enabled: true, auto: false, reason: null };
  return model.merge;
}

/** Mirrors `PrTarget` in `crates/er-desktop/src/gh_pr_actions.rs`. */
export interface PrTarget {
  owner: string;
  repo: string;
  number: number;
}

export function prTarget(github: GithubStatusSnapshot): PrTarget {
  return { owner: github.owner, repo: github.repo, number: github.number };
}

/**
 * What the merge lands relative to what is on screen. `blocksMerge` when the
 * PR moved past the reviewed diff: the merge is pinned to that diff's head
 * (see `mergeRequest`), so GitHub would refuse it.
 */
export function mergeHeadNote(
  github: GithubStatusSnapshot,
  prDiffHeadOid: string | null | undefined,
): { text: string; warn: boolean; blocksMerge: boolean } | null {
  const head = github.head_oid;
  if (!head) return null;
  const short = head.slice(0, 7);
  if (!prDiffHeadOid) {
    return {
      text: `This view shows your local branch. The merge lands PR head ${short}.`,
      warn: false,
      blocksMerge: false,
    };
  }
  if (prDiffHeadOid !== head) {
    return {
      // Either side can be the newer one (a Sync updates the diff before the
      // status refresh lands), so the text does not say which moved.
      text: `The PR head on GitHub (${short}) is not the diff on screen (${prDiffHeadOid.slice(0, 7)}). Sync the diff, or wait for the status to refresh.`,
      warn: true,
      blocksMerge: true,
    };
  }
  return null;
}

/** Text of the confirm step for an action that asks twice. */
export function confirmLabel(action: PrActionRequest, github: GithubStatusSnapshot): string {
  switch (action.kind) {
    case "merge":
      if (action.admin) {
        return `Bypass rules and ${methodLabel(action.method).toLowerCase()} #${github.number} into ${github.base_ref}?`;
      }
      if (github.base_has_merge_queue) return `Add #${github.number} to the ${github.base_ref} merge queue?`;
      return action.auto
        ? `Enable auto-merge (${action.method}) for #${github.number}?`
        : `${methodLabel(action.method)} #${github.number} into ${github.base_ref}?`;
    case "close":
      return `Close #${github.number} without merging?`;
    case "delete_branch":
      return `Delete ${github.head_ref} on GitHub?`;
    default:
      return "Confirm?";
  }
}
