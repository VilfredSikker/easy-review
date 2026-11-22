import { untrack } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { ImagePreviewCache, type ImagePreviewResponse } from '$lib/imagePreview';
import type { AppSnapshot, FileSnapshot } from '$lib/types';

/** Reads from the tab's content when it answers; null leaves the read to the old command. */
let routed: (path: string, context: string) => Promise<ImagePreviewResponse> | null = () => null;
const cache = new ImagePreviewCache((path, context) => routed(path, context) ?? invoke<ImagePreviewResponse>('request_image_preview', {
  path, expectedPreviewContextKey: context,
}));
export const imagePreview = {
  route: (read: typeof routed) => { routed = read; },
  sync: (snap: AppSnapshot | null) => untrack(() => cache.sync(snap)),
  requestKey: (snap: AppSnapshot, file: FileSnapshot) => cache.requestKey(snap, file),
  load: (snap: AppSnapshot, file: FileSnapshot, retry = false) => untrack(() => cache.load(snap, file, retry)),
};
