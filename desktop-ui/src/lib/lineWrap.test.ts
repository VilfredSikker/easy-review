import { describe, expect, it } from "bun:test";
import {
  CELL_HPAD_PX,
  CODE_PREFIX_COLS,
  MIN_CONTINUATION_COLS,
  MIN_WRAP_COLS,
  codeCellWidthPx,
  hangIndentCols,
  hangingIndentStyle,
  leadingWhitespaceChars,
  lineTotalCols,
  wrapColsForWidth,
  wrappedLineCount,
} from "./lineWrap";

describe("leadingWhitespaceChars", () => {
  it("counts leading spaces and tabs", () => {
    expect(leadingWhitespaceChars("")).toBe(0);
    expect(leadingWhitespaceChars("foo")).toBe(0);
    expect(leadingWhitespaceChars("    foo")).toBe(4);
    expect(leadingWhitespaceChars("\t\tfoo")).toBe(2);
    expect(leadingWhitespaceChars(" \t foo")).toBe(3);
  });
});

describe("lineTotalCols", () => {
  it("adds the marker prefix to plain text length", () => {
    expect(lineTotalCols("")).toBe(CODE_PREFIX_COLS);
    expect(lineTotalCols("abc")).toBe(CODE_PREFIX_COLS + 3);
  });

  it("expands tabs to 8-column stops measured from the line start", () => {
    // Prefix is 2 cols, so a leading tab advances to column 8 (6 cols wide).
    expect(lineTotalCols("\tx")).toBe(9);
    // Tab at an exact stop advances a full 8 columns.
    expect(lineTotalCols("abcdef\tx")).toBe(CODE_PREFIX_COLS + 6 + 8 + 1);
  });
});

describe("hangIndentCols", () => {
  it("is leading whitespace + prefix when unclamped", () => {
    expect(hangIndentCols("    foo", null)).toBe(4 + CODE_PREFIX_COLS);
    expect(hangIndentCols("    foo", 120)).toBe(4 + CODE_PREFIX_COLS);
  });

  it("clamps so continuation lines keep MIN_CONTINUATION_COLS", () => {
    const deep = " ".repeat(60) + "x";
    expect(hangIndentCols(deep, 40)).toBe(40 - MIN_CONTINUATION_COLS);
  });

  it("never goes negative", () => {
    expect(hangIndentCols("x", MIN_CONTINUATION_COLS)).toBe(0);
  });
});

describe("hangingIndentStyle", () => {
  it("mirrors hangIndentCols in the emitted ch values", () => {
    expect(hangingIndentStyle("  foo", null)).toBe(
      "padding-left: calc(0.75rem + 4ch); text-indent: -4ch;",
    );
    const deep = " ".repeat(60) + "x";
    const clamped = 40 - MIN_CONTINUATION_COLS;
    expect(hangingIndentStyle(deep, 40)).toBe(
      `padding-left: calc(0.75rem + ${clamped}ch); text-indent: -${clamped}ch;`,
    );
  });
});

describe("wrappedLineCount", () => {
  it("is 1 for lines that fit", () => {
    expect(wrappedLineCount("short", 80)).toBe(1);
    // Exactly at capacity still fits on one line.
    expect(wrappedLineCount("x".repeat(78), 80)).toBe(1);
  });

  it("wraps overflow into ceil(rest / continuation) extra lines", () => {
    // 100 chars + 2 prefix = 102 cols at 80 → 22 overflow, no indent → 1 + ceil(22/80)
    expect(wrappedLineCount("x".repeat(100), 80)).toBe(2);
    // 300 chars + 2 = 302 at 80 → 222 overflow → 1 + ceil(222/80) = 4
    expect(wrappedLineCount("x".repeat(300), 80)).toBe(4);
  });

  it("accounts for the hanging indent on continuation lines", () => {
    // 20 leading spaces → indent 22; capacity 40 → continuation width 18.
    const text = " ".repeat(20) + "y".repeat(60);
    // total = 2 + 80 = 82; first line 40; rest 42 → 1 + ceil(42/18) = 4
    expect(wrappedLineCount(text, 40)).toBe(4);
  });

  it("always makes progress under extreme indents", () => {
    const text = " ".repeat(200) + "y".repeat(10);
    const n = wrappedLineCount(text, 40);
    expect(Number.isFinite(n)).toBe(true);
    expect(n).toBeGreaterThan(1);
  });
});

describe("codeCellWidthPx", () => {
  it("takes one gutter in unified and one per panel in split", () => {
    expect(codeCellWidthPx(1000, 40, false)).toBe(1000 - 40 - CELL_HPAD_PX);
    expect(codeCellWidthPx(1000, 40, true)).toBe((1000 - 80) / 2 - CELL_HPAD_PX);
  });

  it("is 0 before the band is measured", () => {
    expect(codeCellWidthPx(0, 40, false)).toBe(0);
  });

  it("widens the wrap capacity when the gutter narrows", () => {
    const withNumbers = wrapColsForWidth(codeCellWidthPx(800, 48, true), 8);
    const without = wrapColsForWidth(codeCellWidthPx(800, 16, true), 8);
    expect(without).not.toBeNull();
    expect(without!).toBeGreaterThan(withNumbers!);
  });
});

describe("wrapColsForWidth", () => {
  it("is null until both widths are known", () => {
    expect(wrapColsForWidth(0, 8)).toBeNull();
    expect(wrapColsForWidth(400, 0)).toBeNull();
  });

  it("never drops below the minimum wrap", () => {
    expect(wrapColsForWidth(10, 8)).toBe(MIN_WRAP_COLS);
    expect(wrapColsForWidth(800, 8)).toBe(100);
  });
});
