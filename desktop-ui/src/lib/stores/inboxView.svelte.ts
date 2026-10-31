import type { InboxCategoryId } from "$lib/inboxCategories";
import type { InboxReadFilter } from "$lib/inboxThreads";

/**
 * The Inbox page's filters and selection. Held outside the component so that
 * opening a review and coming back lands on the same row with the same
 * filters, rather than resetting to "All" at the top.
 */
class InboxViewState {
  read = $state<InboxReadFilter>("all");
  projectId = $state<"all" | string>("all");
  category = $state<"all" | InboxCategoryId>("all");
  selectedKey = $state<string | null>(null);
}

export const inboxView = new InboxViewState();
