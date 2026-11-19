import { listen } from "@tauri-apps/api/event";
import type { Notice } from "./generated/Notice";

/** The event the backend emits each notice on. */
export const NOTICE_EVENT = "er://notice";

/** Tauri's `listen`, narrowed to the notice event. */
export type ListenNotices = (
  event: string,
  handler: (event: { payload: Notice }) => void,
) => Promise<() => void>;

/** How many sequence numbers the feed remembers. */
const SEEN_LIMIT = 256;

/**
 * Shows each notice once. A notice happens once and has nothing to refetch,
 * so one delivered twice (a listener attached twice, say) would show twice
 * without the check. Remembers seen numbers rather than the highest, since
 * two commands' notices can arrive out of order.
 */
export class NoticeFeed {
  private readonly seen = new Set<number>();
  private readonly order: number[] = [];

  constructor(private readonly show: (notice: Notice) => void) {}

  /** Shows `notice` unless its number was seen; answers whether it showed. */
  accept(notice: Notice): boolean {
    if (this.seen.has(notice.seq)) return false;
    this.seen.add(notice.seq);
    this.order.push(notice.seq);
    if (this.order.length > SEEN_LIMIT) this.seen.delete(this.order.shift() ?? notice.seq);
    this.show(notice);
    return true;
  }

  /** Feeds every notice the backend emits; resolves once the listener is attached. */
  listen(listenTo: ListenNotices = listen<Notice>): Promise<() => void> {
    return listenTo(NOTICE_EVENT, (event) => void this.accept(event.payload));
  }
}
