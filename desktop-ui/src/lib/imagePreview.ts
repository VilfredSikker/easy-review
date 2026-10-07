import type { AppSnapshot, FileSnapshot } from './types';

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'avif', 'bmp', 'ico', 'svg']);

/** Kept apart from `documentPreviewKind`: the document paths (Preview, Side by
 *  side, comments from Preview) all assume the file is text. */
export function isImagePath(path: string): boolean {
  const dot = path.lastIndexOf('.');
  return dot !== -1 && IMAGE_EXTENSIONS.has(path.slice(dot + 1).toLowerCase());
}

/** The backend sets `preview_key` only when it can serve the image. */
export function imagePreviewKey(file: FileSnapshot): string | null {
  return file.preview_key && isImagePath(file.path) ? file.preview_key : null;
}

export function isImagePreview(file: FileSnapshot): boolean {
  return imagePreviewKey(file) !== null;
}

/** `before`/`after` are `data:` URLs; null where that side does not exist or
 *  cannot be read (the old blob of a remote review). */
export interface ImagePreviewResponse {
  path: string;
  before: string | null;
  after: string | null;
  preview_context_key: string;
  preview_key: string;
}
export type ImagePreviewState =
  | { status: 'loading' }
  | { status: 'ready'; before: string | null; after: string | null }
  | { status: 'error'; message: string };
export type ImagePreviewLoader = (path: string, context: string) => Promise<ImagePreviewResponse>;

const CACHE_LIMIT = 16;
/** Images are base64 `data:` URLs of up to 8 MiB a side, so count limits alone could pin hundreds of MB. */
const CACHE_BYTE_BUDGET = 64 * 1024 * 1024;

/**
 * Results are keyed by path and `preview_key`, which already fingerprints the
 * repo, mode, commit and blob. The context key also changes when any other file
 * changes, so keying on it would refetch every image on each live refresh. It is
 * still checked on the response, so an answer for a stale request is dropped.
 */
export class ImagePreviewCache {
  private results = new Map<string, { context: string; state: ImagePreviewState; bytes: number }>();
  private pending = new Map<string, Promise<ImagePreviewState | null>>();
  private bytes = 0;
  /** Bumped on clear, so a read that started before it does not refill the cache. */
  private generation = 0;
  constructor(private loader: ImagePreviewLoader, private byteBudget = CACHE_BYTE_BUDGET) {}

  /** Drop everything once no review is open, as the document cache does. */
  sync(snap: AppSnapshot | null): void {
    if (snap && snap.tabs.length > 0) return;
    this.results.clear();
    this.pending.clear();
    this.bytes = 0;
    this.generation++;
  }

  requestKey(snap: AppSnapshot, file: FileSnapshot): string {
    return JSON.stringify([snap.preview_context_key ?? '', file.path, file.preview_key ?? '']);
  }

  load(snap: AppSnapshot, file: FileSnapshot, retry = false): Promise<ImagePreviewState | null> {
    const previewKey = file.preview_key;
    const context = snap.preview_context_key;
    if (!previewKey || !context) return Promise.resolve(null);
    const requestKey = this.requestKey(snap, file);
    const pending = this.pending.get(requestKey);
    if (pending) return pending;
    const key = JSON.stringify([file.path, previewKey]);
    const cached = this.results.get(key);
    // An error can come from the context moving under the request, so it is
    // only reused for the context it was raised in.
    if (cached && !retry && (cached.state.status === 'ready' || cached.context === context)) {
      return Promise.resolve(cached.state);
    }
    const result = this.read(key, file.path, previewKey, context).finally(() => {
      if (this.pending.get(requestKey) === result) this.pending.delete(requestKey);
    });
    this.pending.set(requestKey, result);
    return result;
  }

  private async read(key: string, path: string, previewKey: string, context: string): Promise<ImagePreviewState | null> {
    const generation = this.generation;
    try {
      const response = await this.loader(path, context);
      if (generation !== this.generation) return null;
      if (response.path !== path || response.preview_key !== previewKey || response.preview_context_key !== context) {
        return null;
      }
      return this.store(key, context, { status: 'ready', before: response.before, after: response.after });
    } catch (error) {
      if (generation !== this.generation) return null;
      const state: ImagePreviewState = { status: 'error', message: String(error) };
      // A request that lost a race with a newer context must not hide its result.
      return this.results.get(key)?.state.status === 'ready' ? state : this.store(key, context, state);
    }
  }

  private store(key: string, context: string, state: ImagePreviewState): ImagePreviewState {
    this.remove(key);
    const bytes = state.status === 'ready' ? (state.before?.length ?? 0) + (state.after?.length ?? 0) : 0;
    this.results.set(key, { context, state, bytes });
    this.bytes += bytes;
    // The newest entry always stays, even alone over budget: its row is waiting for it.
    while (this.results.size > 1 && (this.results.size > CACHE_LIMIT || this.bytes > this.byteBudget)) {
      const oldest = this.results.keys().next().value;
      if (oldest === undefined) break;
      this.remove(oldest);
    }
    return state;
  }

  private remove(key: string): void {
    this.bytes -= this.results.get(key)?.bytes ?? 0;
    this.results.delete(key);
  }
}

export interface ImagePane { label: string; src: string; note: string }

/** The panes a ready image shows, Before then After. */
export function imagePanes(
  { before, after }: Extract<ImagePreviewState, { status: 'ready' }>,
  status: FileSnapshot['status'],
): ImagePane[] {
  if (before && after) return [{ label: 'Before', src: before, note: '' }, { label: 'After', src: after, note: '' }];
  if (before) return [{ label: 'Deleted', src: before, note: '' }];
  if (!after) return [];
  if (status === 'added') return [{ label: 'Added', src: after, note: '' }];
  // A remote review or unfetched base cannot read the old blob. Saying so
  // keeps a changed image from reading as a new one.
  const changed = status === 'modified' || status === 'renamed';
  return [{ label: 'After', src: after, note: changed ? 'Before unavailable' : '' }];
}

/** ←/→ in the full-screen view. Stops at the ends so the key never jumps from After back to Before. */
export function stepPane(index: number, key: string, count: number): number {
  if (key === 'ArrowLeft') return Math.max(0, index - 1);
  if (key === 'ArrowRight') return Math.min(count - 1, index + 1);
  return index;
}
