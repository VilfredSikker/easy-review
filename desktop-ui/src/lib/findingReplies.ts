import type { FlatFinding, ThreadSnapshot } from "$lib/types";

export type MergedReply = {
  /** Dedupe key, unique within one merge; also the `{#each}` key. */
  key: string;
  id: string;
  author: string;
  kind: "you" | "human" | "ai";
  timestamp: string;
  body_markdown: string;
  origin: "finding_response" | "thread_reply";
  source?: string;
  synced?: boolean;
  editable?: boolean;
  deletable?: boolean;
};

/// A finding's replies live in two places: responses stored on the finding
/// and replies on a legacy thread. The same reply can appear in both, so the
/// first copy wins, and the result is ordered by timestamp.
export function mergeFindingReplies(
  finding: FlatFinding,
  thread: ThreadSnapshot | null,
): MergedReply[] {
  const byKey = new Map<string, MergedReply>();
  const add = (r: Omit<MergedReply, "key">) => {
    const key = `${r.origin}:${r.id || r.timestamp}:${r.body_markdown}`;
    if (!byKey.has(key)) byKey.set(key, { key, ...r });
  };
  for (const r of finding.responses ?? []) {
    add({
      id: r.id,
      author: r.author,
      kind: r.kind,
      timestamp: r.timestamp,
      body_markdown: r.body_markdown,
      origin: "finding_response",
      editable: r.editable,
      deletable: r.deletable,
    });
  }
  for (const r of thread?.replies ?? []) {
    add({
      id: r.id,
      author: r.author,
      kind: r.kind,
      timestamp: r.timestamp,
      body_markdown: r.body_markdown,
      origin: r.origin ?? "thread_reply",
      source: r.source,
      synced: r.synced,
      editable: r.editable ?? r.kind === "you",
      deletable: r.deletable ?? true,
    });
  }
  return [...byKey.values()].sort((a, b) => a.timestamp.localeCompare(b.timestamp));
}
