import { untrack } from 'svelte';
import { ImagePreviewCache, type ImagePreviewResponse } from '$lib/imagePreview';
import type { AppSnapshot, FileSnapshot } from '$lib/pageTypes';

/** Reads from the active tab's content; null when no tab is open. */
let routed: (path: string, context: string) => Promise<ImagePreviewResponse> | null = () => null;
const cache = new ImagePreviewCache((path, context) => routed(path, context) ?? Promise.reject(new Error('no tab is open')));
export const imagePreview = {
  route: (read: typeof routed) => { routed = read; },
  sync: (snap: AppSnapshot | null) => untrack(() => cache.sync(snap)),
  requestKey: (snap: AppSnapshot, file: FileSnapshot) => cache.requestKey(snap, file),
  load: (snap: AppSnapshot, file: FileSnapshot, retry = false) => untrack(() => cache.load(snap, file, retry)),
};
