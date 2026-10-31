import { describe, expect, it } from "bun:test";
import type { LineCommits } from "./lineCommits";
import {
  attributionWanted,
  commitSlots,
  fileCommitChip,
  fileLocalState,
  lineCommitColor,
  markLinesActive,
} from "./lineCommitMarks";

const A = "a".repeat(40);
const B = "b".repeat(40);
const C = "c".repeat(40);

const commits = [
  { sha: A, title: "First", unpushed: false },
  { sha: B, title: "Second", unpushed: true },
];

function attribution(over: Partial<LineCommits> = {}): LineCommits {
  return {
    path: "src/a.ts",
    cache_key: "k",
    unavailable: null,
    commits: [A, B],
    added: [
      { start: 1, len: 2, commit: 0 },
      { start: 5, len: 1, commit: 1 },
    ],
    deleted: [{ start: 3, len: 1, commit: 1 }],
    deleted_attributed: true,
    ...over,
  };
}

describe("markLinesActive", () => {
  it("is on only for a range in the Diff", () => {
    expect(markLinesActive(true, null, false)).toBe(true);
    expect(markLinesActive(false, null, false)).toBe(false);
    expect(markLinesActive(true, A, false)).toBe(false);
    expect(markLinesActive(true, null, true)).toBe(false);
  });
});

describe("lineCommitColor", () => {
  const slots = commitSlots(commits);

  it("colours added lines by the new side and deleted lines by the old side", () => {
    const r = attribution();
    expect(lineCommitColor(r, slots, { kind: "add", old_num: null, new_num: 2 })).toBe("var(--color-commit-1)");
    expect(lineCommitColor(r, slots, { kind: "add", old_num: null, new_num: 5 })).toBe("var(--color-commit-2)");
    expect(lineCommitColor(r, slots, { kind: "del", old_num: 3, new_num: null })).toBe("var(--color-commit-2)");
  });

  it("leaves context, unattributed and unlisted lines alone", () => {
    const r = attribution();
    expect(lineCommitColor(r, slots, { kind: "context", old_num: 1, new_num: 1 })).toBeNull();
    expect(lineCommitColor(r, slots, { kind: "add", old_num: null, new_num: 9 })).toBeNull();
    expect(lineCommitColor(attribution({ commits: [C, B] }), slots, { kind: "add", old_num: null, new_num: 1 })).toBeNull();
    expect(lineCommitColor(null, slots, { kind: "add", old_num: null, new_num: 1 })).toBeNull();
  });
});

describe("fileCommitChip", () => {
  it("names the oldest listed commit that touched the file", () => {
    expect(fileCommitChip(attribution(), commits)).toEqual({
      sha: A.slice(0, 7),
      title: "First",
      color: "var(--color-commit-1)",
    });
    expect(fileCommitChip(attribution({ commits: [C, B] }), commits)?.title).toBe("Second");
  });

  it("is null without attribution or a listed commit", () => {
    expect(fileCommitChip(null, commits)).toBeNull();
    expect(fileCommitChip(attribution({ commits: [C] }), commits)).toBeNull();
  });
});

describe("fileLocalState", () => {
  it("flags a file an unpushed commit touched", () => {
    expect(fileLocalState(attribution(), commits)).toBe("unpushed");
    expect(fileLocalState(attribution({ commits: [A] }), commits)).toBeNull();
    expect(fileLocalState(null, commits)).toBeNull();
  });
});

describe("attributionWanted", () => {
  it("asks while lines are marked or a commit is unpushed, never in the Tour", () => {
    const pushed = [{ unpushed: false }];
    expect(attributionWanted(true, pushed, false)).toBe(true);
    expect(attributionWanted(false, pushed, false)).toBe(false);
    expect(attributionWanted(false, [{ unpushed: true }], false)).toBe(true);
    expect(attributionWanted(true, [{ unpushed: true }], true)).toBe(false);
  });
});
