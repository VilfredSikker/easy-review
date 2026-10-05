import { untrack } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { DocumentPreviewCache, type PreviewResponse } from '$lib/documentPreviewCache';
import type { AppSnapshot, FileSnapshot } from '$lib/types';
let revision = $state(0);
const cache = new DocumentPreviewCache((path, context) => invoke<PreviewResponse>('request_file_preview', {
  path, expectedPreviewContextKey: context,
}), () => { revision++; });
export const diffPreview = {
  get revision() { return revision; },
  requestKey: (snap: AppSnapshot, file: FileSnapshot) => cache.requestKey(snap, file),
  sync: (snap: AppSnapshot | null) => untrack(() => cache.sync(snap)),
  paths: (snap: AppSnapshot | null) => { revision; return cache.paths(snap); },
  state: (key: string) => { revision; return cache.state(key); },
  setMode: (snap: AppSnapshot, path: string, preview: boolean) => untrack(() => cache.setMode(snap, path, preview)),
  ensureRaw: (snap: AppSnapshot, path: string) => untrack(() => cache.ensureRaw(snap, path)),
  load: (snap: AppSnapshot, file: FileSnapshot, retry = false) => untrack(() => cache.load(snap, file, retry)),
};
