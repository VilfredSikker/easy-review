export interface SpanSnapshot {
  text: string;
  color: string; // "#RRGGBB" or ""
}

export interface LineSnapshot {
  old_num: number | null;
  new_num: number | null;
  kind: "context" | "add" | "del" | "fold";
  spans: SpanSnapshot[];
}

export interface ThreadMessage {
  author: string;
  kind: "you" | "human" | "ai";
  timestamp: string;
  body_markdown: string;
}

export interface ThreadSnapshot {
  id: string;
  kind: "comment" | "question";
  file: string;
  line: number;
  source: string;
  synced: boolean;
  stale: boolean;
  resolved: boolean;
  root: ThreadMessage;
  replies: ThreadMessage[];
}

export interface HunkSnapshot {
  header: string;
  old_start: number;
  old_count: number;
  new_start: number;
  new_count: number;
  lines: LineSnapshot[];
  threads: ThreadSnapshot[];
}

export interface FileSnapshot {
  path: string;
  status: "added" | "modified" | "deleted" | "renamed" | "copied" | "unmerged";
  additions: number;
  deletions: number;
  reviewed: boolean;
  compacted: boolean;
  risk: "high" | "med" | "low" | null;
  finding_count: number;
  comment_count: number;
  question_count: number;
  hunks: HunkSnapshot[];
}

export interface FlatFinding {
  id: string;
  file: string;
  line: number | null;
  severity: "high" | "med" | "low";
  title: string;
  message_markdown: string;
}

export interface AiSnapshot {
  fresh: boolean;
  summary_markdown: string | null;
  high: number;
  med: number;
  low: number;
  comments: number;
  questions: number;
  unpushed: number;
  threads: ThreadSnapshot[];
  findings: FlatFinding[];
}

export interface PrSnapshot {
  number: number;
  title: string;
  state: string;
  base: string;
  head: string;
}

export interface Panels {
  left: boolean;
  tree: boolean;
  right: boolean;
}

export interface WorktreeSnapshot {
  path: string;
  branch: string;
  is_current: boolean;
}

export interface AppSnapshot {
  mode: "branch" | "unstaged" | "staged" | "history";
  branch: string;
  base: string;
  input_mode: "normal" | "search" | "comment" | "filter" | "commit" | "confirm";
  files: FileSnapshot[];
  selected_file: number;
  current_hunk: number | null;
  filter: string | null;
  reviewed_count: number;
  total_count: number;
  ai: AiSnapshot;
  pr: PrSnapshot | null;
  panels: Panels;
  theme: string;
  watch_active: boolean;
  worktrees: WorktreeSnapshot[];
  notification: string | null;
}
