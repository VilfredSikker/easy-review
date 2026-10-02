import { describe, expect, test } from 'bun:test';
import { DocumentPreviewCache, type PreviewResponse } from './documentPreviewCache';
import type { AppSnapshot, FileSnapshot, TabSummary } from './types';
import { richSnapshot } from './stories/fixtures';

function fixture(context = 'one') {
  const snap = structuredClone(richSnapshot) as AppSnapshot;
  snap.preview_context_key = context;
  const file = snap.files[0];
  file.preview_key = context + '-file';
  return { snap, file };
}
function response(snap: AppSnapshot, file: FileSnapshot): PreviewResponse {
  return {path:file.path,text:'# full document',preview_context_key:snap.preview_context_key!,preview_key:file.preview_key!};
}
describe('complete document preview cache', () => {
  test('retains choices through content refresh and clears closed reviews', () => {
    const {snap,file} = fixture();
    const cache = new DocumentPreviewCache(async () => response(snap,file));
    cache.sync(snap);
    cache.setMode(snap,file.path,true);
    const refreshed = {...snap, preview_context_key:'two'};
    cache.sync(refreshed);
    expect(cache.paths(refreshed).has(file.path)).toBe(true);
    expect(cache.paths({...snap,mode:'staged'}).size).toBe(0);
    cache.sync({...snap,tabs:[]});
    expect(cache.paths(snap).size).toBe(0);
  });
  test('discards delayed responses after switching views', async () => {
    const {snap,file} = fixture();
    let resolve!: (value: PreviewResponse) => void;
    const cache = new DocumentPreviewCache(() => new Promise(r => {resolve=r;}));
    cache.sync(snap);
    const loading=cache.load(snap,file);
    await Promise.resolve();
    cache.sync({...snap,mode:'staged',preview_context_key:'other'});
    resolve(response(snap,file));
    expect(await loading).toBeNull();
    expect(cache.state(file.preview_key!).status).toBe('loading');
  });
  test('refreshing other content replaces pending work without deleting the new response', async () => {
    const {snap,file} = fixture();
    const resolves: Array<(value: PreviewResponse) => void> = [];
    const cache = new DocumentPreviewCache(() => new Promise(resolve => resolves.push(resolve)));
    cache.sync(snap);
    const first = cache.load(snap,file);
    await Promise.resolve();
    const refreshed = {...snap,preview_context_key:'new-context'};
    cache.sync(refreshed);
    const second = cache.load(refreshed,file);
    await Promise.resolve();
    resolves[1](response(refreshed,file));
    await second;
    resolves[0](response(snap,file));
    await first;
    expect(cache.state(file.preview_key!)).toEqual({status:'ready',text:'# full document'});
  });
  test('closing an earlier tab preserves remaining review choices', () => {
    const {snap,file} = fixture();
    const cache = new DocumentPreviewCache(async () => response(snap,file));
    const active = {...snap.tabs[0],idx:1,is_active:true};
    const earlier = {...active,idx:0,is_active:false,repo_root:'/other'};
    const before = {...snap,tabs:[earlier,active],active_tab:1};
    cache.sync(before);
    cache.setMode(before,file.path,true);
    const after = {...snap,tabs:[{...active,idx:0}],active_tab:0};
    cache.sync(after);
    expect(cache.paths(after).has(file.path)).toBe(true);
  });
  test('isolates remote reviews with the same PR number and clears only the closed review', () => {
    const {snap,file} = fixture();
    const first: TabSummary = {...snap.tabs[0],idx:0,kind:'remote_pr',remote:'owner/first',pr_number:42,is_active:true};
    const second = {...first,idx:1,remote:'owner/second',is_active:false};
    const before = {...snap,tabs:[first,second],active_tab:0};
    const other = {...before,tabs:[{...first,is_active:false},{...second,is_active:true}],active_tab:1};
    const cache = new DocumentPreviewCache(async () => response(snap,file));
    cache.sync(before);
    cache.setMode(before,file.path,true);
    expect(cache.paths(other).has(file.path)).toBe(false);
    cache.setMode(other,file.path,true);
    const closed = {...other,tabs:[{...second,idx:0,is_active:true}],active_tab:0};
    cache.sync(closed);
    expect(cache.paths(closed).has(file.path)).toBe(true);
    expect(cache.paths(before).has(file.path)).toBe(false);
  });
  test('loads once and permits retry after failure', async () => {
    const {snap,file}=fixture(); let calls=0;
    const cache=new DocumentPreviewCache(async () => { if (++calls===1) throw new Error('unavailable'); return response(snap,file); });
    cache.sync(snap);
    await cache.load(snap,file);
    expect(cache.state(file.preview_key!).status).toBe('error');
    await cache.load(snap,file);
    expect(calls).toBe(1);
    await cache.load(snap,file,true);
    expect(cache.state(file.preview_key!)).toEqual({status:'ready',text:'# full document'});
  });
  test('mounted request identity ignores chrome refresh but tracks scope and complete content', () => {
    const {snap,file} = fixture();
    const cache = new DocumentPreviewCache(async () => response(snap,file));
    const key = cache.requestKey(snap,file);
    expect(cache.requestKey({...snap,chrome_revision:snap.chrome_revision + 1},{...file})).toBe(key);
    expect(cache.requestKey({...snap,mode:'staged'},file)).not.toBe(key);
    expect(cache.requestKey({...snap,preview_context_key:'two'},file)).not.toBe(key);
    expect(cache.requestKey(snap,{...file,preview_key:'changed'})).not.toBe(key);
  });
  test('mounted consumers retain all concurrent results while shared cache stays bounded', async () => {
    const {snap,file}=fixture();
    let calls = 0;
    const cache=new DocumentPreviewCache(async (path,context) => {
      calls++;
      return response({...snap,preview_context_key:context},snap.files.find(f=>f.path===path)!);
    });
    snap.files=Array.from({length:24},(_,i)=>({...file,path:`${i}.md`,preview_key:`key-${i}`}));
    cache.sync(snap);
    const pending = snap.files.map(f => cache.load(snap,f));
    expect(cache.load(snap,snap.files[0])).toBe(pending[0]);
    const mounted = await Promise.all(pending);
    expect(mounted.every(result => result?.status === 'ready')).toBe(true);
    expect(calls).toBe(24);
    expect(snap.files.filter(f => cache.state(f.preview_key!).status === 'ready')).toHaveLength(16);
    expect(cache.state('key-0').status).toBe('loading');
    expect(mounted[0]).toEqual({status:'ready',text:'# full document'});
    await cache.load(snap,snap.files[23]);
    expect(calls).toBe(24);
    await cache.load(snap,snap.files[0]);
    expect(calls).toBe(25);
    expect(snap.files.filter(f => cache.state(f.preview_key!).status === 'ready')).toHaveLength(16);
  });
  test('keys complete text independently of unchanged hunks and caps documents at 16', async () => {
    const {snap,file}=fixture();
    const cache=new DocumentPreviewCache(async (path,context) => response({...snap,preview_context_key:context},snap.files.find(f=>f.path===path)!));
    snap.files=Array.from({length:17},(_,i)=>({...file,path:`${i}.md`,preview_key:`key-${i}`}));
    cache.sync(snap);
    for(const f of snap.files) await cache.load(snap,f);
    expect(cache.state('key-0').status).toBe('loading');
    expect(cache.state('key-16').status).toBe('ready');
    cache.setMode(snap,file.path,true);
    expect(cache.ensureRaw(snap,file.path)).toBe(true);
    expect(cache.paths(snap).size).toBe(0);
  });
});
