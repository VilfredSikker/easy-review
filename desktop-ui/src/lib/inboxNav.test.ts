import { describe, expect, it } from "bun:test";
import { inboxReturnView } from "./inboxNav";

describe("inboxReturnView", () => {
  it("goes back to the view the Inbox was opened from", () => {
    expect(inboxReturnView("diff", true)).toBe("diff");
    expect(inboxReturnView("settings", true)).toBe("settings");
  });
  it("falls back to the Overview, or the Diff when the Overview is unavailable", () => {
    expect(inboxReturnView(null, true)).toBe("overview");
    expect(inboxReturnView("inbox", false)).toBe("diff");
  });
});
