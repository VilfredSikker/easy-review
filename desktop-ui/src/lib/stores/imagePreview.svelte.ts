import { untrack } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { ImagePreviewCache, type ImagePreviewResponse } from '$lib/imagePreview';
import type { AppSnapshot, FileSnapshot } from '$lib/types';

const cache = new ImagePreviewCache((path, context) => invoke<ImagePreviewResponse>('request_image_preview', {
  path, expectedPreviewContextKey: context,
}));
export const imagePreview = {
  sync: (snap: AppSnapshot | null) => untrack(() => cache.sync(snap)),
  requestKey: (snap: AppSnapshot, file: FileSnapshot) => cache.requestKey(snap, file),
  load: (snap: AppSnapshot, file: FileSnapshot, retry = false) => untrack(() => cache.load(snap, file, retry)),
};
