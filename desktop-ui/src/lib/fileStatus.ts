import type { FileSnapshot } from "$lib/types";

export type FileStatusIconKind = "pencil" | "plus-circle" | "minus-circle" | "alert";

export interface FileStatusDisplay {
  /** Single-char TUI symbol (crates/er-tui/src/ui/file_tree.rs). */
  glyph: string;
  icon: FileStatusIconKind;
  className: string;
  title: string;
}

/** TUI glyphs + desktop tree status SVG kinds. */
export function fileStatusDisplay(status: FileSnapshot["status"]): FileStatusDisplay {
  switch (status) {
    case "added":
      return { glyph: "+", icon: "plus-circle", className: "text-add-fg", title: "New file" };
    case "deleted":
      return { glyph: "−", icon: "minus-circle", className: "text-del-fg", title: "Deleted" };
    case "modified":
      return { glyph: "~", icon: "pencil", className: "text-risk-med", title: "Modified" };
    case "renamed":
      return { glyph: "R", icon: "pencil", className: "text-risk-med", title: "Renamed" };
    case "copied":
      return { glyph: "C", icon: "plus-circle", className: "text-add-fg", title: "Copied" };
    case "unmerged":
      return { glyph: "!", icon: "alert", className: "text-risk-high", title: "Unmerged" };
    default:
      return { glyph: "·", icon: "pencil", className: "text-muted", title: "Changed" };
  }
}
