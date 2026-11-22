import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { jumpToUnreviewed, nextFile, nextHunk, prevFile, prevHunk, type FileMove } from "./navigation";

/** The fixture the engine's navigation functions answer too (`crates/er-engine/tests/navigation_fixture.rs`). */
const FIXTURE = join(dirname(fileURLToPath(import.meta.url)), "../../../crates/er-engine/tests/fixtures/navigation_cases.json");

interface Cases {
  file_moves: {
    name: string;
    file: number;
    watched_sel: number | null;
    visible: number[];
    watched: number[];
    next: [string, number?];
    prev: [string, number?];
  }[];
  hunk_moves: { current: number; count: number; next: number | null; prev: number | null }[];
  unreviewed: { name: string; file: number; visible: [number, boolean][]; next: number | null }[];
}

const cases = JSON.parse(readFileSync(FIXTURE, "utf8")) as Cases;

/** A move as the fixture writes it. */
const encoded = (move: FileMove) => ("idx" in move ? [move.kind, move.idx] : [move.kind]);

describe("the shared navigation fixture", () => {
  it("holds for every file move", () => {
    expect(cases.file_moves.length).toBeGreaterThan(0);
    for (const c of cases.file_moves) {
      expect({ name: c.name, next: encoded(nextFile(c.file, c.watched_sel, c.visible, c.watched)) }).toEqual({
        name: c.name,
        next: c.next,
      });
      expect({ name: c.name, prev: encoded(prevFile(c.file, c.watched_sel, c.visible, c.watched)) }).toEqual({
        name: c.name,
        prev: c.prev,
      });
    }
  });

  it("holds for every hunk move", () => {
    for (const c of cases.hunk_moves) {
      expect([nextHunk(c.current, c.count), prevHunk(c.current)]).toEqual([c.next, c.prev]);
    }
  });

  it("holds for every jump to an unreviewed file", () => {
    for (const c of cases.unreviewed) {
      expect({ name: c.name, next: jumpToUnreviewed(c.file, c.visible) }).toEqual({ name: c.name, next: c.next });
    }
  });
});
