// Session-only, unlike the kind filter, and tied to the tab it was set on: it
// comes from one PR's reviewer list, and carrying a login to another PR would
// empty the panel there for no visible reason.
class CommentsAuthorFilter {
  private value = $state<{ author: string; tab: string | null } | null>(null);

  /** The author to narrow to on `tab`, or null when the filter belongs elsewhere. */
  authorFor(tab: string | null): string | null {
    return this.value && this.value.tab === tab ? this.value.author : null;
  }

  set(author: string | null, tab: string | null) {
    this.value = author ? { author, tab } : null;
  }
}

export const commentsAuthorFilter = new CommentsAuthorFilter();
