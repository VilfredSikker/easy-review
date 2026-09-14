import type { NotificationSnapshot } from "./types";

/**
 * Whether a snapshot's notification should raise a toast, given the one last
 * shown.
 *
 * Keyed on `seq`, which the engine advances on every notify. Comparing the
 * message text instead would swallow a repeat: the backend leaves the last
 * notification set for the process lifetime, so a second "review started..."
 * arrives byte-identical to the first and would be treated as a redelivery of
 * a message already on screen.
 */
export function shouldShowNotification(
  next: NotificationSnapshot | null,
  lastShown: NotificationSnapshot | null,
): boolean {
  if (next === null) return false;
  return next.seq !== lastShown?.seq;
}
