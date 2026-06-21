/** Format an ISO 8601 timestamp as a compact "time ago" string (e.g. "16m ago",
 *  "3h ago", "5d ago", "2w ago", "4mo ago", "1y ago"). Returns the input
 *  unchanged if it isn't a parseable timestamp. */
export function timeAgo(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return iso;

  const sec = Math.floor((Date.now() - t) / 1000);
  if (sec < 60) return "just now";

  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;

  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;

  const day = Math.floor(hr / 24);
  if (day < 7) return `${day}d ago`;

  const wk = Math.floor(day / 7);
  if (day < 30) return `${wk}w ago`;

  const mo = Math.floor(day / 30);
  if (mo < 12) return `${mo}mo ago`;

  const yr = Math.floor(day / 365);
  return `${yr}y ago`;
}
