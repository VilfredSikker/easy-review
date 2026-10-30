/**
 * Splits a toast message into a bold title and a muted detail line.
 * Backend and store messages are written as "Title · detail"; errors keep the
 * whole text as the body because their detail is a raw command error that
 * must stay copyable as one string.
 */
export function splitToast(
  kind: "success" | "error" | "info" | "warn",
  message: string,
): { title: string; detail: string } {
  if (kind === "error" || kind === "warn") return { title: message, detail: "" };
  const [title, ...rest] = message.split(" · ");
  return { title, detail: rest.join(" · ") };
}
