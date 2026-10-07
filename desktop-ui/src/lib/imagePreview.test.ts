import { describe, expect, test } from 'bun:test';
import { ImagePreviewCache, imagePanes, isImagePath, isImagePreview, stepPane, type ImagePreviewResponse } from './imagePreview';
import type { AppSnapshot, FileSnapshot } from './types';
import { richSnapshot } from './stories/fixtures';

function fixture(context = 'one') {
  const snap = structuredClone(richSnapshot) as AppSnapshot;
  snap.preview_context_key = context;
  const file = { ...snap.files[0], path: 'logo.png', preview_key: 'blob-a' };
  return { snap, file };
}
function response(snap: AppSnapshot, file: FileSnapshot, extra: Partial<ImagePreviewResponse> = {}): ImagePreviewResponse {
  return {
    path: file.path, before: null, after: 'data:image/png;base64,AA==',
    preview_context_key: snap.preview_context_key!, preview_key: file.preview_key!, ...extra,
  };
}

describe('image paths', () => {
  test('matches image extensions case-insensitively', () => {
    for (const path of ['a.png', 'b.JPG', 'c.jpeg', 'd.gif', 'e.webp', 'f.avif', 'g.bmp', 'h.ico', 'dir.v2/i.svg']) {
      expect(isImagePath(path)).toBe(true);
    }
    for (const path of ['readme.md', 'notes.txt', 'png', 'dir.png/file', 'image.tiff']) {
      expect(isImagePath(path)).toBe(false);
    }
  });
  test('needs a preview key from the backend', () => {
    const { file } = fixture();
    expect(isImagePreview(file)).toBe(true);
    expect(isImagePreview({ ...file, preview_key: '' })).toBe(false);
    expect(isImagePreview({ ...file, preview_key: undefined })).toBe(false);
  });
});

describe('image preview cache', () => {
  test('drops a response whose keys do not match the request', async () => {
    const { snap, file } = fixture();
    for (const extra of [{ path: 'other.png' }, { preview_key: 'blob-b' }, { preview_context_key: 'two' }]) {
      const cache = new ImagePreviewCache(async () => response(snap, file, extra));
      expect(await cache.load(snap, file)).toBeNull();
    }
    const cache = new ImagePreviewCache(async () => response(snap, file));
    expect(await cache.load(snap, file)).toEqual({ status: 'ready', before: null, after: 'data:image/png;base64,AA==' });
  });

  test('reuses a ready image across contexts and shares one request', async () => {
    const { snap, file } = fixture();
    let calls = 0;
    const cache = new ImagePreviewCache(async (_path, context) => { calls++; return response({ ...snap, preview_context_key: context }, file); });
    const [a, b] = await Promise.all([cache.load(snap, file), cache.load(snap, file)]);
    expect(a).toEqual(b);
    expect(calls).toBe(1);
    expect((await cache.load({ ...snap, preview_context_key: 'two' }, file))?.status).toBe('ready');
    expect(calls).toBe(1);
    await cache.load(snap, file, true);
    expect(calls).toBe(2);
  });

  test('retries an error once the context moves', async () => {
    const { snap, file } = fixture();
    let fail = true;
    const cache = new ImagePreviewCache(async (_path, context) => {
      if (fail) throw new Error('Review context changed');
      return response({ ...snap, preview_context_key: context }, file);
    });
    expect(await cache.load(snap, file)).toEqual({ status: 'error', message: 'Error: Review context changed' });
    fail = false;
    expect((await cache.load(snap, file))?.status).toBe('error');
    expect((await cache.load({ ...snap, preview_context_key: 'two' }, file))?.status).toBe('ready');
  });

  test('keeps at most 16 results', async () => {
    const { snap, file } = fixture();
    let calls = 0;
    const cache = new ImagePreviewCache(async (path) => { calls++; return response(snap, { ...file, path }); });
    for (let i = 0; i < 17; i++) await cache.load(snap, { ...file, path: `${i}.png` });
    await cache.load(snap, { ...file, path: '16.png' });
    expect(calls).toBe(17);
    await cache.load(snap, { ...file, path: '0.png' });
    expect(calls).toBe(18);
  });

  test('evicts the oldest images once their bytes pass the budget', async () => {
    const { snap, file } = fixture();
    let calls = 0;
    const image = 'x'.repeat(40);
    const cache = new ImagePreviewCache(async (path) => {
      calls++;
      return response(snap, { ...file, path }, { before: image, after: image });
    }, 200);
    for (const path of ['a.png', 'b.png', 'c.png']) await cache.load(snap, { ...file, path });
    await cache.load(snap, { ...file, path: 'c.png' });
    await cache.load(snap, { ...file, path: 'b.png' });
    expect(calls).toBe(3);
    await cache.load(snap, { ...file, path: 'a.png' });
    expect(calls).toBe(4);
  });

  test('keeps the newest image even when it alone is over budget', async () => {
    const { snap, file } = fixture();
    let calls = 0;
    const cache = new ImagePreviewCache(async (path) => {
      calls++;
      return response(snap, { ...file, path }, { after: 'x'.repeat(100) });
    }, 50);
    await cache.load(snap, { ...file, path: 'small.png' });
    await cache.load(snap, { ...file, path: 'huge.png' });
    await cache.load(snap, { ...file, path: 'huge.png' });
    expect(calls).toBe(2);
    await cache.load(snap, { ...file, path: 'small.png' });
    expect(calls).toBe(3);
  });

  test('clears when the last review closes, including reads still in flight', async () => {
    const { snap, file } = fixture();
    let calls = 0;
    const resolves: Array<() => void> = [];
    const cache = new ImagePreviewCache((path) => {
      calls++;
      return new Promise(resolve => resolves.push(() => resolve(response(snap, { ...file, path }))));
    });
    const first = cache.load(snap, file);
    resolves[0]();
    await first;
    cache.sync(snap);
    const late = cache.load(snap, { ...file, path: 'late.png' });
    cache.sync({ ...snap, tabs: [] });
    resolves[1]();
    expect(await late).toBeNull();
    const again = cache.load(snap, file);
    resolves[2]();
    await again;
    expect(calls).toBe(3);
    const lateAgain = cache.load(snap, { ...file, path: 'late.png' });
    resolves[3]();
    await lateAgain;
    expect(calls).toBe(4);
  });
});

describe('image panes', () => {
  const ready = (before: string | null, after: string | null) => ({ status: 'ready' as const, before, after });
  const summary = (before: string | null, after: string | null, status: FileSnapshot['status']) =>
    imagePanes(ready(before, after), status).map(p => [p.label, p.note]);

  test('flags a changed image whose earlier side could not be read', () => {
    expect(summary(null, 'b', 'modified')).toEqual([['After', 'Before unavailable']]);
    expect(summary(null, 'b', 'renamed')).toEqual([['After', 'Before unavailable']]);
    expect(summary(null, 'b', 'added')).toEqual([['Added', '']]);
  });
  test('shows both sides of a change and the old side of a deletion', () => {
    expect(summary('a', 'b', 'modified')).toEqual([['Before', ''], ['After', '']]);
    expect(summary('a', null, 'deleted')).toEqual([['Deleted', '']]);
    expect(summary(null, null, 'modified')).toEqual([]);
  });
});

describe('full-screen pane stepping', () => {
  test('arrows move between Before and After and stop at the ends', () => {
    expect(stepPane(0, 'ArrowRight', 2)).toBe(1);
    expect(stepPane(1, 'ArrowRight', 2)).toBe(1);
    expect(stepPane(1, 'ArrowLeft', 2)).toBe(0);
    expect(stepPane(0, 'ArrowLeft', 2)).toBe(0);
  });
  test('a single pane and other keys leave the index alone', () => {
    expect(stepPane(0, 'ArrowRight', 1)).toBe(0);
    expect(stepPane(1, 'j', 2)).toBe(1);
  });
});
