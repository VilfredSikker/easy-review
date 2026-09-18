import { describe, expect, it } from "bun:test";
import { shouldShowNotification } from "./snapshotNotification";
import type { NotificationSnapshot } from "./types";

function note(
  message: string,
  seq: number,
  long = false,
): NotificationSnapshot {
  return { message, seq, long };
}

describe("shouldShowNotification", () => {
  it("shows a notification it has not seen", () => {
    expect(shouldShowNotification(note("review started...", 1), null)).toBe(true);
  });

  it("ignores a redelivery of the same seq", () => {
    // The backend re-sends the same notification on every snapshot until the
    // next one replaces it, so the same seq must not raise a second toast.
    const shown = note("review started...", 4);
    expect(shouldShowNotification(note("review started...", 4), shown)).toBe(false);
  });

  it("shows the same text again when the seq advanced", () => {
    // The regression: the backend never clears the message, so running the
    // same command twice delivers two snapshots whose text is identical. The
    // seq is the only thing that distinguishes them.
    const shown = note("review started...", 1);
    expect(shouldShowNotification(note("review started...", 2), shown)).toBe(true);
  });

  it("shows nothing when the backend has no notification", () => {
    expect(shouldShowNotification(null, note("review started...", 1))).toBe(false);
  });
});
