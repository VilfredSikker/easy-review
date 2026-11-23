import { untrack } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { DocumentPreviewCache, type DocumentViewMode, type PreviewResponse } from '$lib/documentPreviewCache';
import type { AppSnapshot, FileSnapshot } from '$lib/pageTypes';
let revision = $state(0);
/** Reads from the tab's content when it answers; null leaves the read to the old command. */
let routed: (path: string, context: string) => Promise<PreviewResponse> | null = () => null;
const cache = new DocumentPreviewCache((path, context) => routed(path, context) ?? invoke<PreviewResponse>('request_file_preview', {
  path, expectedPreviewContextKey: context,
}), () => { revision++; });
export const diffPreview = {
  route: (read: typeof routed) => { routed = read; },
  get revision() { return revision; },
  requestKey: (snap: AppSnapshot, file: FileSnapshot) => cache.requestKey(snap, file),
  sync: (snap: AppSnapshot | null) => untrack(() => cache.sync(snap)),
  paths: (snap: AppSnapshot | null) => { revision; return cache.paths(snap); },
  sidePaths: (snap: AppSnapshot | null) => { revision; return cache.sidePaths(snap); },
  state: (key: string) => { revision; return cache.state(key); },
  setMode: (snap: AppSnapshot, path: string, mode: DocumentViewMode) => untrack(() => cache.setMode(snap, path, mode)),
  ensureRaw: (snap: AppSnapshot, path: string) => untrack(() => cache.ensureRaw(snap, path)),
  load: (snap: AppSnapshot, file: FileSnapshot, retry = false) => untrack(() => cache.load(snap, file, retry)),
};
