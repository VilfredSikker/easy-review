import type { AppSnapshot, FileSnapshot } from './types';
import type { TabSummary } from './types';

/** How a document file shows in the diff. `side` keeps the raw rows and renders the document beside them. */
export type DocumentViewMode = 'raw' | 'preview' | 'side';
export type PreviewState = { status: 'loading' } | { status: 'ready'; text: string } | { status: 'error'; message: string };
export interface PreviewResponse { path: string; text: string; preview_context_key: string; preview_key: string }
export type PreviewLoader = (path: string, context: string) => Promise<PreviewResponse>;

export class DocumentPreviewCache {
  private choices = new Map<string, Map<string, Exclude<DocumentViewMode, 'raw'>>>();
  private documents = new Map<string, PreviewState>();
  private pending = new Map<string, { token: object; result: Promise<PreviewState | null> }>();
  private current: AppSnapshot | null = null;
  constructor(private loader: PreviewLoader, private changed: () => void = () => {}) {}

  private reviewKey(tab: TabSummary): string {
    return JSON.stringify([tab.repo_root, tab.remote, tab.kind, tab.pr_number == null ? tab.branch : null, tab.pr_number]);
  }
  private activeReviewKey(snap: AppSnapshot): string {
    const tab = snap.tabs.find(t => t.is_active) ?? snap.tabs[snap.active_tab];
    return tab ? this.reviewKey(tab) : '';
  }

  private choiceKey(snap: AppSnapshot): string {
    return `${this.activeReviewKey(snap)}\0${snap.mode}\0${snap.selected_commit_sha ?? ''}`;
  }
  sync(snap: AppSnapshot | null): void {
    if (!snap || !this.current || snap.preview_context_key !== this.current.preview_context_key ||
        this.choiceKey(snap) !== this.choiceKey(this.current)) {
      for (const key of this.pending.keys()) {
        if (this.documents.get(key)?.status === 'loading') this.documents.delete(key);
      }
      this.pending.clear();
    }
    this.current = snap;
    const open = new Set(snap?.tabs.map(tab => this.reviewKey(tab)));
    for (const key of this.choices.keys()) {
      if (!open.has(key.split('\0')[0])) this.choices.delete(key);
    }
    if (!snap || open.size === 0) { this.documents.clear(); this.pending.clear(); }
  }
  paths(snap: AppSnapshot | null): ReadonlySet<string> { return this.pathsIn(snap, 'preview'); }
  sidePaths(snap: AppSnapshot | null): ReadonlySet<string> { return this.pathsIn(snap, 'side'); }
  private pathsIn(snap: AppSnapshot | null, mode: DocumentViewMode): ReadonlySet<string> {
    const modes = snap ? this.choices.get(this.choiceKey(snap)) : undefined;
    return new Set([...modes ?? []].filter(([, m]) => m === mode).map(([path]) => path));
  }
  setMode(snap: AppSnapshot, path: string, mode: DocumentViewMode): void {
    const key = this.choiceKey(snap);
    const modes = new Map(this.choices.get(key));
    if (mode === 'raw') modes.delete(path); else modes.set(path, mode);
    this.choices.set(key, modes);
    this.changed();
  }
  ensureRaw(snap: AppSnapshot, path: string): boolean {
    if (!this.paths(snap).has(path)) return false;
    this.setMode(snap, path, 'raw');
    return true;
  }
  requestKey(snap: AppSnapshot, file: FileSnapshot): string {
    return JSON.stringify([this.choiceKey(snap), snap.preview_context_key, file.path, file.preview_key]);
  }
  state(key: string): PreviewState { return this.documents.get(key) ?? { status: 'loading' }; }
  load(snap: AppSnapshot, file: FileSnapshot, retry = false): Promise<PreviewState | null> {
    const key = file.preview_key;
    const context = snap.preview_context_key;
    if (!key || !context) return Promise.resolve(null);
    const pending = this.pending.get(key);
    if (pending) return pending.result;
    const cached = this.documents.get(key);
    if (!retry && cached) return Promise.resolve(cached);
    const token = {};
    // Start the read after registering its token, including for synchronous loaders.
    const result = Promise.resolve().then(() => this.read(snap, file, token, key, context));
    this.pending.set(key, { token, result });
    this.documents.set(key, { status: 'loading' });
    this.trim();
    this.changed();
    return result;
  }
  private async read(snap: AppSnapshot, file: FileSnapshot, token: object, key: string, context: string): Promise<PreviewState | null> {
    try {
      const result = await this.loader(file.path, context);
      if (!this.accepts(snap, file, token)) return null;
      if (result.path !== file.path || result.preview_key !== key || result.preview_context_key !== context) return null;
      const state: PreviewState = { status: 'ready', text: result.text };
      this.documents.set(key, state);
      this.trim();
      return state;
    } catch (error) {
      if (!this.accepts(snap, file, token)) return null;
      const state: PreviewState = { status: 'error', message: String(error) };
      this.documents.set(key, state);
      this.trim();
      return state;
    } finally {
      if (this.pending.get(key)?.token === token) {
        this.pending.delete(key);
        if (this.documents.get(key)?.status === 'loading') this.documents.delete(key);
      }
      this.changed();
    }
  }
  private accepts(snap: AppSnapshot, file: FileSnapshot, token: object): boolean {
    return this.current !== null && this.pending.get(file.preview_key ?? '')?.token === token &&
      this.current?.preview_context_key === snap.preview_context_key &&
      this.choiceKey(this.current) === this.choiceKey(snap) &&
      this.current.files.some(f => f.path === file.path && f.preview_key === file.preview_key);
  }
  private trim(): void {
    while (this.documents.size > 16) {
      const oldest = this.documents.keys().next().value;
      if (oldest === undefined) break;
      this.documents.delete(oldest);
    }
  }
}
