import type { AiSnapshot, AppSnapshot } from "../pageTypes";

/** Lays one store's fields over the view, after the stores before it. */
export type ViewOverlay = (view: AppSnapshot) => AppSnapshot;

const EMPTY_AI: AiSnapshot = {
  fresh: false,
  stale_reason: null,
  summary_markdown: null,
  agent_summaries: {},
  high: 0,
  med: 0,
  low: 0,
  local_comment_count: 0,
  github_comment_count: 0,
  comments: 0,
  questions: 0,
  notes: 0,
  threads: [],
  findings: [],
  arbiter_dropped: 0,
  arbiter_merged: 0,
  arbiter_unmatched: 0,
  arbiter_regraded: 0,
  file_risks: [],
  resolved_findings: [],
  dropped_findings: [],
  min_trust_default: "" as AiSnapshot["min_trust_default"],
  has_review_json: false,
  eligible_comment_count: 0,
  triage: null,
  checklist: null,
  diagrams: [],
  diagram_presets: [],
};

/** What the page shows before any store has laid its fields down. */
function emptyView(tabs: Pick<AppSnapshot, "tabs" | "active_tab">): AppSnapshot {
  return {
    mode: "branch",
    branch: "",
    base: "",
    input_mode: "normal",
    files: [],
    selected_file: 0,
    current_hunk: null,
    filter: null,
    reviewed_count: 0,
    total_count: 0,
    ai: EMPTY_AI,
    review_bucket_er_dir: "",
    pr: null,
    panels: { left: true, tree: true, right: true },
    theme: "",
    watch_active: false,
    watch_status: { active: false, branch: null, root_path: null },
    worktrees: [],
    projects: [],
    local_branch: null,
    notification: null,
    ...tabs,
  };
}

/**
 * The active tab as the page reads it, built from the resources alone: the
 * tab strip's rows, then each store's fields laid over them in the order the
 * stores load. Null until the tab strip first arrives.
 */
class PageView {
  private tabs = $state<() => Pick<AppSnapshot, "tabs" | "active_tab"> | null>(() => null);
  private overlays = $state<ViewOverlay[]>([]);
  private pinned = $state<{ view: AppSnapshot | null } | null>(null);
  private readonly shown = $derived.by(() => {
    if (this.pinned) return this.pinned.view;
    const tabs = this.tabs();
    return tabs && this.overlays.reduce((view, overlay) => overlay(view), emptyView(tabs));
  });

  get current(): AppSnapshot | null {
    return this.shown;
  }

  /** Pins a fixed view in place of the resources' one; stories and tests set it, the app never does. */
  set current(view: AppSnapshot | null) {
    this.pinned = { view };
  }

  /** Where the tab rows come from; null until they first arrive. */
  readTabs(tabs: () => Pick<AppSnapshot, "tabs" | "active_tab"> | null): void {
    this.tabs = tabs;
  }

  overlay(overlay: ViewOverlay): void {
    this.overlays = [...this.overlays, overlay];
  }
}

export const pageView = new PageView();
