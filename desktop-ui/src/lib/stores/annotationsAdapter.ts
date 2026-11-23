import type { AiAnnotations } from "$lib/protocol/generated/AiAnnotations";
import type { Annotations } from "$lib/protocol/generated/Annotations";
import type { Jobs } from "$lib/protocol/generated/Jobs";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { UiAnnotation as WireUiAnnotation } from "$lib/protocol/generated/UiAnnotation";
import type {
  AiAction,
  AiSnapshot,
  AppSnapshot,
  FindingResponseSnapshot,
  FlatFinding,
  OtherBucketSnapshot,
  ThreadMessage,
  ThreadSnapshot,
  TourSnapshot,
  UiAnnotation,
} from "$lib/types";
import { withPending, type PendingPaint } from "./annotationWrites";
import type { ResourceStore } from "./resourceStore";

export const JOBS: ResourceKey = { kind: "jobs" };

export function annotationsKey(tab: TabKey): ResourceKey {
  return { kind: "annotations", tab };
}

export function uiAnnotationsKey(tab: TabKey): ResourceKey {
  return { kind: "ui_annotations", tab };
}

/** The reply a thread shows while an ask-AI run writes one, as the snapshot draws it. */
function thinkingReply(aiAction: string | null): ThreadMessage {
  const reply: ThreadMessage = { id: "", author: "ai", kind: "ai", timestamp: "", body_markdown: "…thinking" };
  return { ...reply, origin: "thread_reply", ...(aiAction ? { ai_action: aiAction as AiAction } : {}) };
}

const THINKING_RESPONSE: FindingResponseSnapshot = {
  id: "",
  author: "AI",
  kind: "ai",
  timestamp: "",
  body_markdown: "…thinking",
  origin: "finding_response",
  editable: false,
  deletable: false,
};

/**
 * `annotations(tab)`'s AI slice with the in-flight replies of `jobs` joined
 * on: a "…thinking" reply after a thread's last, and a "…thinking" response
 * after a finding's, keyed as `jobs` keys them.
 */
export function joinJobs(ai: AiAnnotations, jobs: Jobs | null): AiSnapshot {
  const pending = new Map((jobs?.ai_replies ?? []).map((job) => [job.target, job]));
  const thread = (t: ThreadSnapshot): ThreadSnapshot => {
    const job = pending.get(t.id);
    return job ? { ...t, replies: [...t.replies, thinkingReply(job.ai_action)] } : t;
  };
  const finding = (f: FlatFinding): FlatFinding =>
    pending.has(`finding:${f.id}`) ? { ...f, responses: [...(f.responses ?? []), THINKING_RESPONSE] } : f;
  const wire = ai as unknown as AiSnapshot;
  return {
    ...wire,
    threads: wire.threads.map(thread),
    findings: wire.findings.map(finding),
    resolved_findings: wire.resolved_findings.map(finding),
    dropped_findings: wire.dropped_findings.map(finding),
  };
}

/** The wire rows as the snapshot's, with the DOM context parsed back. */
export function uiRows(rows: readonly WireUiAnnotation[]): UiAnnotation[] {
  return rows.map(({ dom_context_json, ...row }) => ({
    ...row,
    dom_context: dom_context_json ? (JSON.parse(dom_context_json) as UiAnnotation["dom_context"]) : null,
  }));
}

interface AnnotationsAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get">;
  /** The registry's active tab, whose annotations the page shows. */
  activeKey: () => TabKey | null;
  /** The writes the server has not acknowledged yet, drawn over the threads. */
  paints?: () => readonly PendingPaint[];
}

/**
 * The active tab's annotations: `app.snapshot`'s `ai`, `tour`,
 * `other_bucket` and `ui_annotations` with the flag off, and with it on
 * `annotations(tab)` joined with `jobs` and `ui_annotations(tab)` in every
 * view, since the workspace loads them all. The App's mode is not read: with
 * the flag on a view switch never reaches the App, so its mode goes stale.
 */
export class AnnotationsAdapter {
  constructor(private readonly deps: AnnotationsAdapterDeps) {}

  /** The active tab's key while the page reads its annotations from the resources; null with the flag off. */
  fetchedTab(): TabKey | null {
    return this.deps.flagOn ? this.deps.activeKey() : null;
  }

  private held(): Annotations | null {
    const key = this.fetchedTab();
    if (key === null) return null;
    const held = this.deps.store.get(annotationsKey(key))?.value;
    return held?.kind === "annotations" ? held : null;
  }

  private jobs(): Jobs | null {
    const held = this.deps.store.get(JOBS)?.value;
    return held?.kind === "jobs" ? held : null;
  }

  /** `held`'s AI slice with the in-flight replies and the pending writes on it. */
  private ai(held: Annotations): AiSnapshot {
    return withPending(joinJobs(held.ai, this.jobs()), this.deps.paints?.() ?? []);
  }

  /** The joined threads DiffView draws on its hunks; null while the snapshot serves them. */
  threads(): ThreadSnapshot[] | null {
    const held = this.held();
    return held ? this.ai(held).threads : null;
  }

  /** `raw` with the resources' fields in place of the App's, when the page holds them. */
  overlay(raw: AppSnapshot): AppSnapshot {
    const held = this.held();
    if (!held) return raw;
    const key = this.deps.activeKey();
    const ui = key === null ? undefined : this.deps.store.get(uiAnnotationsKey(key))?.value;
    return {
      ...raw,
      ai: this.ai(held),
      tour: held.tour as TourSnapshot,
      other_bucket: held.other_bucket as OtherBucketSnapshot | null,
      ...(ui?.kind === "ui_annotations" ? { ui_annotations: uiRows(ui.rows) } : {}),
    };
  }
}
