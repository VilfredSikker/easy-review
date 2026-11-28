/* Canvas engine: one editing surface for two contexts.
   · map   — Map › Diagram at any level: the parts and references as built, plus a design layer drawn on top.
             The design is stored as a diff against the map (by part id and layer name), so it survives a re-import.
   · blank — Patterns › Blank canvas: design without code.
   Pattern instances bind library roles to real parts and are checked by the pattern's rules; fits are deterministic
   suggestions with a Lean ledger preview; every instance is a decision record with its door and sources.
   Claude edits through the same operations as the toolbar. */
const BK = { c: 'call', e: 'event', m: 'command', n: 'network', d: 'data' };
const BNK = { part: 'Part', new: 'New part', ext: 'Outside system', store: 'Store', queue: 'Channel / queue' };
const B = { ctx: null, ctxKey: '', m: null, sel: [], selEdge: null, selLayer: null, hist: [], fut: [], kind: 'c', side: 'checks', busy: false, ctl: null, hl: null, hlKey: null, drag: null, conn: null, place: null, q: '', aiUndo: null, turnEdits: [], live: '', status: '' };
const RH = 96, GUT = 118, NH = 34;
const uid = p => p + Math.random().toString(36).slice(2, 8);
const nW = n => Math.min(210, Math.max(84, n.name.length * 7.1 + 26));
const clone = o => JSON.parse(JSON.stringify(o));
const DEP = e => e.kind !== 'e';

/* ───────── design documents ───────── */
let D = null, Dsnap = null;
const dsKey = () => 'er-design:' + state.snap, blankKey = () => 'er-bld:' + state.snap;
function dsDoc() { if (Dsnap !== state.snap || !D) { Dsnap = state.snap; D = null; try { D = JSON.parse(localStorage.getItem(dsKey()) || 'null'); } catch (_) {} D = D || { levels: {}, chat: [] }; } return D; }
let dsT; function dsPersist() { clearTimeout(dsT); dsT = setTimeout(() => { try { localStorage.setItem(dsKey(), JSON.stringify(D)); } catch (_) {} }, 200); }
function dsLevels() { const d = dsDoc(); return Object.keys(d.levels).filter(l => byId[l] && dsHasChanges(d.levels[l])); }
function dsHasChanges(x) { return !!x && ((x.add || []).length || (x.del || []).length || Object.keys(x.move || {}).length || Object.keys(x.ren || {}).length || (x.eAdd || []).length || (x.eDel || []).length || (x.inst || []).length || (x.layers && x.layersChanged)); }

/* the as-built model of one map level */
function bFromMap(level) {
  const V = byId[level]; if (!V || !V.children) return bBlankModel();
  const kids = V.children.filter(c => !c.virtual), layers = (V.layers || ['Layer']).map(n => ({ id: 'L:' + n, name: n }));
  const byRow = {}; kids.forEach(c => { const r = Math.max(0, Math.min(layers.length - 1, c.layer || 0)); (byRow[r] = byRow[r] || []).push(c); });
  const nodes = [];
  Object.entries(byRow).forEach(([r, cs]) => { let x = GUT + 14; cs.sort((a, b) => (b.loc || 0) - (a.loc || 0)).forEach(c => { const n = { id: 'p:' + c.id, name: c.name, layer: +r, x, kind: c.kind === 'external' ? 'ext' : 'part', ref: c.id, by: 'map', open: !!(c.children && c.children.length) }; nodes.push(n); x += nW(n) + 22; }); });
  const v = view(level, null), K = { import: 'c', net: 'n', ipc: 'm', event: 'e', spawn: 'n', fs: 'd' }, edges = [];
  for (const it of v.items.values()) {
    if (it.w <= 0 || it.boundary || it.from.startsWith('g:') || it.to.startsWith('g:')) continue;
    const top = Object.entries(it.kinds || {}).sort((a, b) => b[1] - a[1])[0], k = top ? (K[top[0]] || 'c') : 'c';
    edges.push({ id: 'b:' + it.from + '>' + it.to, a: 'p:' + it.from, b: 'p:' + it.to, kind: k, w: it.w, by: 'map' });
  }
  return { layers, nodes, edges, inst: [] };
}
function bBlankModel() { return { layers: ['Front ends', 'Application', 'Domain', 'Adapters', 'Outside'].map(n => ({ id: uid('l'), name: n })), nodes: [], edges: [], inst: [] }; }
const ek = e => e.a + '>' + e.b + ':' + e.kind;
/* design = what differs from the as-built model */
function dsDiff(m) {
  const base = m.base, bn = new Map(base.nodes.map(n => [n.id, n])), lid = i => m.layers[i] && m.layers[i].id, blid = i => base.layers[i] && base.layers[i].id;
  const be = new Map(base.edges.map(e => [ek(e), e])), ce = new Set(m.edges.map(ek));
  const x = { layers: m.layers.map(l => ({ id: l.id, name: l.name })), layersChanged: JSON.stringify(m.layers.map(l => [l.id, l.name])) !== JSON.stringify(base.layers.map(l => [l.id, l.name])),
    add: m.nodes.filter(n => !bn.has(n.id)).map(n => Object.assign({}, n, { lid: lid(n.layer) })), del: base.nodes.filter(n => !m.nodes.some(c => c.id === n.id)).map(n => n.id),
    move: {}, ren: {}, kind: {}, note: {}, by: {}, pos: {}, eAdd: m.edges.filter(e => !be.has(ek(e))).map(e => Object.assign({}, e)), eDel: base.edges.filter(e => !ce.has(ek(e))).map(ek), lab: {}, inst: m.inst || [] };
  m.nodes.forEach(n => { x.pos[n.id] = Math.round(n.x); const b = bn.get(n.id); if (!b) return; if (lid(n.layer) !== blid(b.layer)) x.move[n.id] = lid(n.layer); if (n.name !== b.name) x.ren[n.id] = n.name; if (n.kind !== b.kind) x.kind[n.id] = n.kind; if (n.note) x.note[n.id] = n.note; if (n.by !== 'map') x.by[n.id] = n.by; });
  m.edges.forEach(e => { const b = be.get(ek(e)); if (b && e.label) x.lab[ek(e)] = e.label; });
  return x;
}
function dsApply(base, x) {
  const m = clone(base); m.base = clone(base); if (!x) return m;
  if (x.layers && x.layers.length) { const ids = new Set(x.layers.map(l => l.id)); m.layers = x.layers.map(l => ({ id: l.id, name: l.name })).concat(base.layers.filter(l => !ids.has(l.id))); }
  const li = id => m.layers.findIndex(l => l.id === id), remap = n => { const old = base.layers[n.layer]; const i = old ? li(old.id) : -1; return i < 0 ? Math.min(n.layer, m.layers.length - 1) : i; };
  m.nodes.forEach(n => { n.layer = remap(n); });
  const del = new Set(x.del || []); m.nodes = m.nodes.filter(n => !del.has(n.id));
  m.nodes.forEach(n => { if (x.move && x.move[n.id] != null && li(x.move[n.id]) >= 0) n.layer = li(x.move[n.id]); if (x.ren && x.ren[n.id]) n.name = x.ren[n.id]; if (x.kind && x.kind[n.id]) n.kind = x.kind[n.id]; if (x.note && x.note[n.id]) n.note = x.note[n.id]; if (x.by && x.by[n.id]) n.by = x.by[n.id]; if (x.pos && x.pos[n.id] != null) n.x = x.pos[n.id]; });
  (x.add || []).forEach(n => { const c = Object.assign({}, n); const i = li(n.lid); c.layer = i >= 0 ? i : Math.min(n.layer || 0, m.layers.length - 1); delete c.lid; if (x.pos && x.pos[c.id] != null) c.x = x.pos[c.id]; m.nodes.push(c); });
  const ids = new Set(m.nodes.map(n => n.id)), edel = new Set(x.eDel || []);
  m.edges = m.edges.filter(e => !edel.has(ek(e)) && ids.has(e.a) && ids.has(e.b));
  m.edges.forEach(e => { if (x.lab && x.lab[ek(e)]) e.label = x.lab[ek(e)]; });
  (x.eAdd || []).forEach(e => { if (ids.has(e.a) && ids.has(e.b) && !m.edges.some(o => ek(o) === ek(e))) m.edges.push(Object.assign({}, e)); });
  m.inst = (x.inst || []).map(i => Object.assign({}, i, { roles: Object.fromEntries(Object.entries(i.roles).map(([k, v]) => [k, v.filter(id => ids.has(id))])) }));
  return m;
}
/* open a context: builds B.m from the map plus the stored design, or from the blank canvas */
function bOpen(ctx) {
  const key = ctx.kind === 'map' ? 'map|' + state.snap + '|' + ctx.level + '|' + state.func : 'blank|' + state.snap;
  if (B.ctxKey === key && B.m) return;
  B.ctx = ctx; B.ctxKey = key; B.sel = []; B.selEdge = null; B.selLayer = null; B.hist = []; B.fut = []; B.hl = null; B.place = null; B.aiUndo = null;
  if (ctx.kind === 'map') { const d = dsDoc(); B.m = dsApply(bFromMap(ctx.level), d.levels[ctx.level]); B.m.origin = { kind: 'map', level: ctx.level, name: fullName(ctx.level) || (S.repo ? S.repo.name : S.label || 'whole system') }; }
  else { let m = null; try { m = JSON.parse(localStorage.getItem(blankKey()) || 'null'); } catch (_) {} if (m && !m.base) m = null; B.m = m; if (B.m) { B.m.inst = B.m.inst || []; B.m.origin = { kind: 'blank', name: 'blank canvas' }; } }
}
function bChat() { if (B.ctx && B.ctx.kind === 'map') return dsDoc().chat; B.m.chat = B.m.chat || []; return B.m.chat; }
let bSaveT; function bSave() { clearTimeout(bSaveT); const ctx = B.ctx, m = B.m; bSaveT = setTimeout(() => bSaveNow(ctx, m), 150); }
function bSaveNow(ctx, m) { ctx = ctx || B.ctx; m = m || B.m; if (!ctx || !m) return; if (ctx.kind === 'map') { const d = dsDoc(); d.levels[ctx.level] = dsDiff(m); if (!dsHasChanges(d.levels[ctx.level])) delete d.levels[ctx.level]; dsPersist(); } else { try { localStorage.setItem(blankKey(), JSON.stringify(m)); } catch (_) {} } }
function bPush() { B.hist.push(JSON.stringify(Object.assign({}, B.m, { chat: undefined }))); if (B.hist.length > 120) B.hist.shift(); B.fut = []; }
function bRestore(t) { const chat = B.m.chat; B.m = JSON.parse(t); if (chat) B.m.chat = chat; }
function bUndo() { if (!B.hist.length) return; B.fut.push(JSON.stringify(Object.assign({}, B.m, { chat: undefined }))); bRestore(B.hist.pop()); B.sel = []; B.selEdge = null; bSave(); bDraw(); }
function bRedo() { if (!B.fut.length) return; B.hist.push(JSON.stringify(Object.assign({}, B.m, { chat: undefined }))); bRestore(B.fut.pop()); bSave(); bDraw(); }

/* ───────── checks ───────── */
function bCheck(m) {
  const N = new Map(m.nodes.map(n => [n.id, n])), parts = m.nodes.filter(n => n.kind !== 'ext'), dep = m.edges.filter(e => DEP(e) && N.has(e.a) && N.has(e.b));
  const up = dep.filter(e => N.get(e.b).layer < N.get(e.a).layer && N.get(e.b).kind !== 'ext');
  const loops = sccOf(m.nodes.map(n => n.id), dep), inLoop = new Set(loops.flat()), loopEdges = dep.filter(e => loops.some(l => l.includes(e.a) && l.includes(e.b)));
  const doors = m.nodes.filter(n => n.kind === 'ext').map(x => ({ id: x.id, from: [...new Set(m.edges.filter(e => e.b === x.id && N.has(e.a)).map(e => e.a))] })).filter(d => d.from.length > 1);
  const hubs = parts.length >= 5 ? parts.filter(n => new Set(dep.filter(e => e.a === n.id && N.get(e.b).kind !== 'ext').map(e => e.b)).size >= Math.max(4, Math.ceil((parts.length - 1) * .6))) : [];
  const rules = (m.inst || []).map(i => instCheck(m, i));
  return { up, loops, inLoop, loopEdges, doors, hubs, rules, broken: rules.filter(r => r.viol.length).length, parts: parts.length, edges: m.edges.length };
}
function sccOf(ids, edges) {
  const adj = new Map(ids.map(id => [id, []])); edges.forEach(e => { if (adj.has(e.a) && adj.has(e.b)) adj.get(e.a).push(e.b); });
  let idx = 0; const st = [], on = new Set(), ix = new Map(), lo = new Map(), out = [];
  const sc = v => { ix.set(v, idx); lo.set(v, idx); idx++; st.push(v); on.add(v); for (const w of adj.get(v)) { if (!ix.has(w)) { sc(w); lo.set(v, Math.min(lo.get(v), lo.get(w))); } else if (on.has(w)) lo.set(v, Math.min(lo.get(v), ix.get(w))); } if (lo.get(v) === ix.get(v)) { const c = []; let w; do { w = st.pop(); on.delete(w); c.push(w); } while (w !== v); if (c.length > 1) out.push(c); } };
  ids.forEach(id => { if (!ix.has(id)) sc(id); }); return out;
}
const nm = (m, id) => { const n = m.nodes.find(x => x.id === id) || (m.base && m.base.nodes.find(x => x.id === id)); return n ? n.name : id; };
const names = (m, ids) => { const a = (ids || []).map(id => nm(m, id)); return a.length > 3 ? a.slice(0, 3).join(', ') + ` and ${a.length - 3} more` : a.join(', ') || '(none)'; };
function instCheck(m, inst) {
  const spec = PL_ROLES[inst.pat], R = k => inst.roles[k] || [], dep = m.edges.filter(DEP), viol = [], N = new Map(m.nodes.map(n => [n.id, n]));
  if (!spec) return { inst, viol, ok: true };
  const fill = s => s.replace(/\{(\w+)\}/g, (_, k) => names(m, R(k)));
  for (const r of spec.rules) {
    let bad = [], miss = false;
    if (r.t === 'no') { const A = new Set(R(r.from)), Bs = new Set(R(r.to)); bad = dep.filter(e => A.has(e.a) && Bs.has(e.b) && e.a !== e.b); }
    else if (r.t === 'only') { const W = new Set(R(r.who)), T = new Set(R(r.to)); bad = dep.filter(e => T.has(e.b) && !W.has(e.a) && !T.has(e.a)); }
    else if (r.t === 'has') { const A = R(r.from), Bs = new Set(R(r.to)); miss = A.length > 0 && Bs.size > 0 && A.some(a => !m.edges.some(e => e.a === a && Bs.has(e.b))); }
    else if (r.t === 'noup') { const S2 = new Set(R(r.in)); bad = dep.filter(e => S2.has(e.a) && S2.has(e.b) && N.get(e.b) && N.get(e.a) && N.get(e.b).layer < N.get(e.a).layer); }
    else if (r.t === 'acyclic') { const S2 = R(r.in), loops = sccOf(S2, dep); bad = dep.filter(e => loops.some(l => l.includes(e.a) && l.includes(e.b))); }
    if (bad.length || miss) viol.push({ why: fill(r.why), edges: bad.map(e => e.id), miss });
  }
  return { inst, viol, ok: !viol.length };
}
function bDiff(m) {
  const base = m.base, bn = new Map(base.nodes.map(n => [n.id, n])), cn = new Set(m.nodes.map(n => n.id)), be = new Set(base.edges.map(ek)), ce = new Set(m.edges.map(ek));
  const lidOf = (mm, i) => (mm.layers[i] || {}).id, L = i => (m.layers[i] || {}).name || '?', BL = i => (base.layers[i] || {}).name || '?';
  return {
    added: m.nodes.filter(n => !bn.has(n.id)), removed: base.nodes.filter(n => !cn.has(n.id)),
    renamed: m.nodes.filter(n => bn.has(n.id) && bn.get(n.id).name !== n.name).map(n => [bn.get(n.id).name, n]),
    moved: m.nodes.filter(n => bn.has(n.id) && lidOf(base, bn.get(n.id).layer) !== lidOf(m, n.layer)).map(n => [BL(bn.get(n.id).layer), n]),
    eAdded: m.edges.filter(e => !be.has(ek(e))), eRemoved: base.edges.filter(e => !ce.has(ek(e))),
    layers: m.layers.filter(l => !base.layers.some(b => b.id === l.id)),
  };
}
/* the Lean ledger: inventory added against structural exceptions removed */
function bLedger(m) {
  const b = Object.assign({}, m, { layers: m.base.layers, nodes: m.base.nodes, edges: m.base.edges, inst: m.inst }), c0 = bCheck(b), c1 = bCheck(m), d = bDiff(m);
  const add = { parts: d.added.filter(n => n.kind !== 'ext').length, layers: d.layers.length };
  const rem = { up: c0.up.length - c1.up.length, loops: c0.loops.length - c1.loops.length, doors: c0.doors.length - c1.doors.length, hubs: c0.hubs.length - c1.hubs.length, rules: c0.broken - c1.broken };
  const removed = Math.max(0, rem.up) + Math.max(0, rem.loops) * 2 + Math.max(0, rem.doors) + Math.max(0, rem.hubs), added = add.parts + add.layers, worse = Math.max(0, -rem.up) + Math.max(0, -rem.loops) * 2 + Math.max(0, -rem.doors) + Math.max(0, -rem.hubs);
  const verdict = !added && !removed && !worse ? 'none' : worse > removed ? 'worse' : removed >= added && removed > 0 ? 'earns' : 'inventory';
  return { add, rem, removed, added, worse, verdict, c0, c1, d };
}
function ledgerText(L) {
  const bits = [], r = L.rem, plus = (n, w) => `${n} ${w}${n === 1 ? '' : 's'}`;
  if (r.up) bits.push(`${r.up > 0 ? '−' : '+'}${plus(Math.abs(r.up), 'upward reference')}`); if (r.loops) bits.push(`${r.loops > 0 ? '−' : '+'}${plus(Math.abs(r.loops), 'loop')}`);
  if (r.doors) bits.push(`${r.doors > 0 ? '−' : '+'}${plus(Math.abs(r.doors), 'extra door')}`); if (r.hubs) bits.push(`${r.hubs > 0 ? '−' : '+'}${plus(Math.abs(r.hubs), 'hub')}`);
  const inv = [L.add.parts ? plus(L.add.parts, 'part') : '', L.add.layers ? plus(L.add.layers, 'layer') : ''].filter(Boolean).join(' and ');
  return { inv: inv || 'nothing', ex: bits.join(' · ') || 'no structural change' };
}

/* ───────── operations: toolbar, palette, fits and Claude all go through here ───────── */
function bFindNode(ref) { const r = String(ref || '').trim().toLowerCase(); return B.m.nodes.find(n => n.id.toLowerCase() === r) || B.m.nodes.find(n => n.name.toLowerCase() === r) || B.m.nodes.find(n => n.ref && n.ref.toLowerCase() === r) || null; }
function bLayerIx(l) { if (typeof l === 'number' || /^\d+$/.test(String(l))) return Math.max(0, Math.min(B.m.layers.length - 1, +l)); const r = String(l || '').toLowerCase(); return B.m.layers.findIndex(x => x.name.toLowerCase() === r); }
function bFreeX(layer) { const xs = B.m.nodes.filter(n => n.layer === layer).map(n => n.x + nW(n)); return Math.max(GUT + 14, ...xs.map(x => x + 22)); }
function bOp(o, by) {
  const m = B.m, ok = s => ({ ok: true, msg: s }), err = s => { throw new Error(s); };
  const mark = n => { if (by === 'ai' && n.by === 'map') n.by = 'ai-edit'; };
  switch (o.op) {
    case 'add_layer': { const name = String(o.name || 'New layer'); const at = o.at != null ? bLayerIx(o.at) : m.layers.length; const i = at < 0 ? m.layers.length : at; m.layers.splice(i, 0, { id: uid('l'), name }); m.nodes.forEach(n => { if (n.layer >= i) n.layer++; }); return ok(`Added layer ${name}`); }
    case 'rename_layer': { const i = bLayerIx(o.layer); if (i < 0) err('No such layer: ' + o.layer); const old = m.layers[i].name; m.layers[i].name = String(o.name); return ok(`Renamed layer ${old} to ${o.name}`); }
    case 'add_part': { let li = bLayerIx(o.layer ?? 0); if (li < 0) { m.layers.push({ id: uid('l'), name: String(o.layer) }); li = m.layers.length - 1; } const n = { id: uid('n'), name: String(o.name || 'New part').slice(0, 60), layer: li, x: 0, kind: BNK[o.kind] ? o.kind : 'new', by, note: o.note ? String(o.note).slice(0, 400) : '' }; const near = o.near && bFindNode(o.near); n.x = near && near.layer === li ? near.x + nW(near) + 22 : (o.x != null ? +o.x : bFreeX(li)); if (near && near.layer === li) m.nodes.filter(x => x.layer === li && x.x >= n.x).forEach(x => { x.x += nW(n) + 22; }); m.nodes.push(n); return Object.assign(ok(`Added ${n.name} in ${m.layers[li].name}`), { id: n.id }); }
    case 'remove_part': { const n = bFindNode(o.id || o.name); if (!n) err('No such part: ' + (o.id || o.name)); m.nodes = m.nodes.filter(x => x !== n); m.edges = m.edges.filter(e => e.a !== n.id && e.b !== n.id); (m.inst || []).forEach(i => Object.keys(i.roles).forEach(k => { i.roles[k] = i.roles[k].filter(id => id !== n.id); })); return ok(`Removed ${n.name}`); }
    case 'rename_part': { const n = bFindNode(o.id); if (!n) err('No such part: ' + o.id); const old = n.name; n.name = String(o.name).slice(0, 60); mark(n); return ok(`Renamed ${old} to ${n.name}`); }
    case 'move_part': { const n = bFindNode(o.id); if (!n) err('No such part: ' + o.id); const li = bLayerIx(o.layer); if (li < 0) err('No such layer: ' + o.layer); n.layer = li; n.x = bFreeX(li); mark(n); return ok(`Moved ${n.name} to ${m.layers[li].name}`); }
    case 'set_note': { const n = bFindNode(o.id); if (!n) err('No such part: ' + o.id); n.note = String(o.text || '').slice(0, 400); return ok(`Noted on ${n.name}`); }
    case 'add_edge': { const a = bFindNode(o.from), b = bFindNode(o.to); if (!a || !b) err(`Unknown part in ${o.from} → ${o.to}`); if (a === b) err('An edge needs two different parts'); const kind = BK[o.kind] ? o.kind : ({ call: 'c', event: 'e', command: 'm', network: 'n', data: 'd' })[o.kind] || 'c'; if (m.edges.some(e => e.a === a.id && e.b === b.id && e.kind === kind)) return ok('Already there'); m.edges.push({ id: uid('e'), a: a.id, b: b.id, kind, label: o.label ? String(o.label).slice(0, 40) : '', by }); return ok(`${a.name} → ${b.name} (${BK[kind]})`); }
    case 'remove_edge': { const a = bFindNode(o.from), b = bFindNode(o.to); if (!a || !b) err(`Unknown part in ${o.from} → ${o.to}`); const before = m.edges.length; m.edges = m.edges.filter(e => !(e.a === a.id && e.b === b.id)); if (before === m.edges.length) err(`No edge ${a.name} → ${b.name}`); return ok(`Removed ${a.name} → ${b.name}`); }
    case 'reroute_edge': { const a = bFindNode(o.from), b = bFindNode(o.to), v = bFindNode(o.via); if (!a || !b || !v) err('Unknown part in reroute'); const es = m.edges.filter(x => x.a === a.id && x.b === b.id); if (!es.length) err(`No edge ${a.name} → ${b.name}`); m.edges = m.edges.filter(x => !es.includes(x)); bOp({ op: 'add_edge', from: a.id, to: v.id, kind: es[0].kind }, by); if (!m.edges.some(x => x.a === v.id && x.b === b.id)) bOp({ op: 'add_edge', from: v.id, to: b.id, kind: es[0].kind }, by); return ok(`Rerouted ${a.name} → ${b.name} through ${v.name}`); }
    case 'stamp_pattern': { const p = PL_BY[o.pattern] || plFind(o.pattern); if (!p) err('No such pattern: ' + o.pattern); return ok(bStamp(p, o.layer, by)); }
    case 'place_pattern': { const p = PL_BY[o.pattern] || plFind(o.pattern); if (!p || !PL_ROLES[p.id]) err('Not a checkable pattern: ' + o.pattern); const roles = {}; for (const [k, , many] of PL_ROLES[p.id].roles) { const v = (o.roles || {})[k]; const arr = (Array.isArray(v) ? v : v ? [v] : []).map(x => bFindNode(x)); if (arr.some(x => !x)) err(`Unknown part in role ${k}`); roles[k] = arr.map(x => x.id).slice(0, many ? 99 : 1); } m.inst = m.inst || []; const inst = { id: uid('i'), pat: p.id, roles, by, at: new Date().toISOString().slice(0, 10), note: o.note ? String(o.note).slice(0, 400) : '' }; m.inst.push(inst); const r = instCheck(m, inst); return ok(`Placed ${p.name}: ${r.ok ? 'all rules hold' : r.viol.length + ' rule' + (r.viol.length > 1 ? 's' : '') + ' broken'}`); }
    case 'remove_pattern': { const i = (m.inst || []).findIndex(x => x.id === o.id || (PL_BY[x.pat] && PL_BY[x.pat].name.toLowerCase() === String(o.id).toLowerCase())); if (i < 0) err('No such placed pattern'); const [x] = m.inst.splice(i, 1); return ok(`Removed the ${PL_BY[x.pat].name} instance`); }
    case 'apply_fit': { const f = bFits(m).find(x => x.id === o.fit); if (!f) err('No such fit (call find_sites again)'); return ok(f.apply(m, by)); }
    default: err('Unknown op: ' + o.op);
  }
}
function bApply(ops, by) { const res = []; for (const o of ops) { try { res.push(bOp(o, by).msg); } catch (e) { res.push('Error: ' + e.message); } } bSave(); return res; }
function bStamp(p, layer, by) {
  const m = B.m, K = { n: 'new', o: 'ext', s: 'store', q: 'queue', '': 'part' };
  let start = layer != null ? bLayerIx(layer) : (B.sel.length ? (m.nodes.find(n => n.id === B.sel[0]) || {}).layer ?? 0 : 0); if (start < 0) start = 0;
  while (m.layers.length < start + p.dg.rows.length) m.layers.push({ id: uid('l'), name: p.dg.rows[m.layers.length - start] || 'Layer' });
  const base = Math.max(...Array.from({ length: p.dg.rows.length }, (_, r) => bFreeX(start + r))), ids = {}, made = [], rowX = {};
  p.dg.n.slice().sort((a, b) => a[2] - b[2] || a[3] - b[3]).forEach(([id, label, row, x, kind]) => { const n = { id: uid('n'), name: label, layer: start + row, x: 0, kind: K[kind || ''], by: by === 'ai' ? 'ai' : 'you', stamp: p.id }; n.x = Math.max(rowX[row] || base, base + x * 200); rowX[row] = n.x + nW(n) + 18; ids[id] = n.id; m.nodes.push(n); made.push(n.id); });
  p.dg.e.filter(e => e[2] !== 'x').forEach(([a, b, k, lab]) => m.edges.push({ id: uid('e'), a: ids[a], b: ids[b], kind: k, label: lab || '', by: by === 'ai' ? 'ai' : 'you' }));
  B.sel = made; return `Stamped ${p.name} (${made.length} parts) from layer ${m.layers[start].name}`;
}

/* ───────── fits: where a pattern would remove an exception, computed, not guessed ───────── */
function bFits(m) {
  const out = [], N = new Map(m.nodes.map(n => [n.id, n])), dep = m.edges.filter(e => DEP(e) && N.has(e.a) && N.has(e.b)), ck = bCheck(m);
  const placed = (pat, role, id) => (m.inst || []).some(i => i.pat === pat && (i.roles[role] || []).includes(id));
  // Gateway: an outside system reached from several parts
  for (const d of ck.doors) { if (placed('gateway', 'outside', d.id)) continue; const x = N.get(d.id);
    const why = `${x.name} is reached from ${d.from.length} parts: ${names(m, d.from)}.`;
    out.push({ id: 'gw:' + d.id, pat: 'gateway', title: `Put ${x.name} behind one gateway`, why, focus: [d.id, ...d.from],
      apply: (mm, by) => { const callers = d.from.filter(id => mm.nodes.some(n => n.id === id)), low = Math.max(...callers.map(id => mm.nodes.find(n => n.id === id).layer)); const kinds = mm.edges.filter(e => e.b === d.id && callers.includes(e.a)).map(e => e.kind); const g = bOpOn(mm, { op: 'add_part', name: `${x.name} gateway`, layer: low, kind: 'new', note: `The only part that talks to ${x.name}.` }, by).id;
        mm.edges = mm.edges.filter(e => !(e.b === d.id && callers.includes(e.a))); callers.forEach(c => bOpOn(mm, { op: 'add_edge', from: c, to: g, kind: 'c' }, by)); bOpOn(mm, { op: 'add_edge', from: g, to: d.id, kind: kinds.sort((a, b) => kinds.filter(k => k === b).length - kinds.filter(k => k === a).length)[0] || 'n' }, by);
        bOpOn(mm, { op: 'place_pattern', pattern: 'gateway', roles: { callers, gateway: [g], outside: [d.id] }, note: why }, by); return `Put ${x.name} behind a new gateway; ${callers.length} parts now go through it`; } });
  }
  // Separated interface: break an upward reference, or the edge whose inversion opens a loop, by owning the interface below
  const mkSi = e => { const a = N.get(e.a), b = N.get(e.b); if (!a || !b) return null;
    const why = ck.up.includes(e) ? `${a.name} sits below ${b.name} but depends on it${e.w ? ` (${e.w} references)` : ''}.` : `${a.name} → ${b.name} closes a loop${e.w ? ` (${e.w} references)` : ''}.`;
    return { id: 'si:' + e.a + '>' + e.b, pat: 'separated-interface', title: `Let ${a.name} own an interface instead of calling ${b.name}`, why, focus: [e.a, e.b],
      apply: (mm, by) => { const i = bOpOn(mm, { op: 'add_part', name: `${b.name} port`, layer: a.layer, near: a.id, kind: 'new', note: `Interface owned by ${a.name}; ${b.name} implements it.` }, by).id; mm.edges = mm.edges.filter(x => !(x.a === e.a && x.b === e.b)); bOpOn(mm, { op: 'add_edge', from: e.a, to: i, kind: 'c' }, by); bOpOn(mm, { op: 'add_edge', from: e.b, to: i, kind: 'c', label: 'implements' }, by);
        bOpOn(mm, { op: 'place_pattern', pattern: 'separated-interface', roles: { user: [e.a], iface: [i], impl: [e.b] }, note: why }, by); return `${a.name} now calls an interface it owns; ${b.name} implements it`; } }; };
  ck.up.slice(0, 6).forEach(e => { const f = mkSi(e); if (f) out.push(f); });
  ck.loops.slice(0, 4).forEach(l => { const es = ck.loopEdges.filter(e => l.includes(e.a) && l.includes(e.b) && !ck.up.includes(e)).slice(0, 14); let best = null, bestV = -1; for (const e of es) { const f = mkSi(e); if (!f) continue; const L = fitPreview(m, f), v = L.removed * 100 - (e.w || 1); if (L.removed > 0 && v > bestV) { best = f; bestV = v; } } if (best) out.push(best); });
  // Facade: several callers reaching into the same lower parts
  const byLayer = {}; dep.forEach(e => { const a = N.get(e.a), b = N.get(e.b); if (b.kind === 'ext' || b.layer <= a.layer) return; const k = b.layer; ((byLayer[k] = byLayer[k] || {})[e.a] = byLayer[k][e.a] || new Set()).add(e.b); });
  for (const [L, callers] of Object.entries(byLayer)) {
    const many = Object.entries(callers).filter(([, t]) => t.size >= 2); if (many.length < 2) continue;
    const count = {}; many.forEach(([, t]) => t.forEach(x => { count[x] = (count[x] || 0) + 1; })); const sub = Object.keys(count).filter(x => count[x] >= 2); if (sub.length < 2) continue;
    const cs = many.map(([c]) => c); if (placed('facade', 'subsystem', sub[0])) continue;
    const why = `${cs.length} callers each reach two or more of them directly.`;
    out.push({ id: 'fa:' + L + ':' + sub.join(','), pat: 'facade', title: `Give ${names(m, sub)} one front door`, why, focus: [...cs, ...sub],
      apply: (mm, by) => { const f = bOpOn(mm, { op: 'add_part', name: `${mm.layers[+L].name} facade`, layer: +L, kind: 'new', note: 'The one entry point callers use.' }, by).id; mm.edges = mm.edges.filter(e => !(cs.includes(e.a) && sub.includes(e.b) && DEP(e))); cs.forEach(c => bOpOn(mm, { op: 'add_edge', from: c, to: f, kind: 'c' }, by)); sub.forEach(s => bOpOn(mm, { op: 'add_edge', from: f, to: s, kind: 'c' }, by));
        bOpOn(mm, { op: 'place_pattern', pattern: 'facade', roles: { callers: cs, facade: [f], subsystem: sub }, note: why }, by); return `Added a facade in front of ${sub.length} parts for ${cs.length} callers`; } });
  }
  // Single writer: a store written from several parts
  for (const s of m.nodes.filter(n => n.kind === 'store' || (n.kind === 'ext' && /disk|stor|file|database|db\b/i.test(n.name)))) {
    const ws = m.edges.filter(e => e.b === s.id && e.kind === 'd'); const writers = [...new Set(ws.map(e => e.a))]; if (writers.length < 2 || placed('single-writer', 'store', s.id)) continue;
    const wsum = id => ws.filter(e => e.a === id).reduce((t, e) => t + (e.w || 1), 0), owner = writers.slice().sort((a, b) => wsum(b) - wsum(a))[0], others = writers.filter(w => w !== owner);
    const why = `${writers.length} parts write it directly: ${names(m, writers)}.`;
    out.push({ id: 'sw:' + s.id, pat: 'single-writer', title: `Give ${s.name} one writer`, why, focus: [s.id, ...writers],
      apply: (mm, by) => { mm.edges = mm.edges.filter(e => !(e.b === s.id && others.includes(e.a) && e.kind === 'd')); others.forEach(o => bOpOn(mm, { op: 'add_edge', from: o, to: owner, kind: 'm', label: 'request' }, by)); bOpOn(mm, { op: 'place_pattern', pattern: 'single-writer', roles: { owner: [owner], store: [s.id], others }, note: why }, by); return `${nm(mm, owner)} is now the only writer of ${s.name}`; } });
  }
  return out.filter(f => f.id.startsWith('si:') || fitPreview(m, f).removed > 0);
}
/* run an op against a model other than B.m (fit previews work on a clone) */
function bOpOn(mm, o, by) { const keep = B.m; B.m = mm; try { return bOp(o, by); } finally { B.m = keep; } }
function fitPreview(m, f) { const c = clone(m); c.base = m.base; f.apply(c, 'you'); return bLedger(c); }

/* ───────── rendering ───────── */
function renderDiagram(host) { bOpen({ kind: 'map', level: state.path }); bShell(host); }
function renderBuilder(host) {
  bOpen({ kind: 'blank' });
  if (!B.m) { host.innerHTML = bStartCard(); return; }
  bShell(host);
}
function bShell(host) {
  host.innerHTML = `<div class="bl-bar">${bToolbar()}</div><div class="bl-body"><div class="bl-stage" id="blStage"><svg id="blSvg" xmlns="http://www.w3.org/2000/svg"></svg></div><aside class="bl-side" aria-label="Design panel"><div class="bl-insp" id="blInsp" hidden></div><div class="seg bl-sseg" role="group">${[['checks', 'Checks'], ['pat', 'Patterns'], ['ai', 'Claude'], ['changes', 'Changes']].map(t => `<button type="button" data-bside="${t[0]}" aria-pressed="${B.side === t[0]}">${t[1]}</button>`).join('')}</div><div id="blSideBody" class="bl-sbody"></div></aside></div>`;
  bDraw();
  if (LIB.pendingStamp) { const id = LIB.pendingStamp; LIB.pendingStamp = null; if (PL_ROLES[id]) { B.side = 'pat'; B.place = bPlaceStart(id); } else if (PL_BY[id]) { bPush(); bStamp(PL_BY[id], null, 'you'); bSave(); } bDraw(); }
  if (LIB.pendingAsk) { const id = LIB.pendingAsk; LIB.pendingAsk = null; B.side = 'ai'; bDrawSide(); const p = PL_BY[id]; bAsk(id === 'lean' ? 'Read this level with the Lean lens. What structure is not earning its keep? Remove or simplify it on the canvas, and say what each change saves.' : `Where would ${p ? p.name : id} fit at this level? Use find_sites first; if a site fits, apply it, otherwise place the pattern on the parts it should cover. Say what it changes.`); }
}
function bStartCard() {
  return `<div class="arch"><div class="sheet" style="max-width:760px"><header><div class="eyebrow">Blank canvas</div><h1 class="lead">Design without code</h1><p class="para">A canvas for a system that does not exist yet: five empty layers, the pattern library, the checks and Claude. To design on your own code, open <b>Map › Diagram</b>: the design is drawn on top of the parts as built, at any level.</p></header>
    <div class="row"><button type="button" class="openbtn" data-bstart="blank">Start blank</button><button type="button" class="openbtn" data-bgo="dgm">Open Map › Diagram</button></div></div></div>`;
}
function bToolbar() {
  const kinds = Object.entries(BK).map(([k, l]) => `<button type="button" data-bkind="${k}" aria-pressed="${B.kind === k}" title="New edges are ${l}s"><i class="bl-kd" style="border-top-color:${PD_EDGE[k][0]};border-top-style:${PD_EDGE[k][1] ? 'dashed' : 'solid'}"></i>${l}</button>`).join('');
  const d = B.m.base ? bDiff(B.m) : null, n = d ? d.added.length + d.removed.length + d.moved.length + d.renamed.length + d.eAdded.length + d.eRemoved.length + d.layers.length : 0;
  return `<span class="bl-org">${B.ctx.kind === 'map' ? `<span class="bl-pill ${n ? 'on' : ''}">${n ? n + ' design change' + (n > 1 ? 's' : '') : 'As built'}</span>` : '<b>Blank canvas</b>'}</span>
    <button type="button" class="bl-btn" data-bact="part" title="Add a part (or double-click a layer)">+ Part</button><button type="button" class="bl-btn" data-bact="layer">+ Layer</button>
    <span class="bl-lbl">New edges</span><div class="seg bl-kinds" role="group" aria-label="Kind of new edges">${kinds}</div>
    <button type="button" class="bl-btn" data-bact="undo" ${B.hist.length ? '' : 'disabled'} title="Undo (Ctrl+Z)">↶ Undo</button><button type="button" class="bl-btn" data-bact="redo" ${B.fut.length ? '' : 'disabled'} title="Redo (Ctrl+Shift+Z)">↷</button>
    <span class="grow"></span>${B.ctx.kind === 'map' ? `<button type="button" class="bl-btn" data-bact="reset" ${n || (B.m.inst || []).length ? '' : 'disabled'} title="Drop the design at this level">Reset this level</button>` : '<button type="button" class="bl-btn" data-bact="reset">New canvas</button>'}`;
}
function bDraw() {
  const svg = $('blSvg'); if (!svg || !B.m) return;
  const m = B.m, ck = bCheck(m), N = new Map(m.nodes.map(n => [n.id, n])), upIds = new Set(ck.up.map(e => e.id)), loopIds = new Set(ck.loopEdges.map(e => e.id)), ruleIds = new Set(ck.rules.flatMap(r => r.viol.flatMap(v => v.edges)));
  const stage = $('blStage'), Wv = Math.max(stage ? stage.clientWidth : 900, 320);
  const W = Math.max(Wv, ...m.nodes.map(n => n.x + nW(n) + 40)), H = m.layers.length * RH + 16;
  const k = W > Wv ? Math.max(.75, Wv / W) : 1; svg.setAttribute('viewBox', `0 0 ${W} ${H}`); svg.setAttribute('width', Math.floor(W * k)); svg.setAttribute('height', Math.floor(H * k));
  const base = m.base ? { n: new Map(m.base.nodes.map(n => [n.id, n])), e: new Set(m.base.edges.map(ek)) } : null, isMap = B.ctx.kind === 'map';
  const pos = id => { const n = N.get(id), w = nW(n), y = 8 + n.layer * RH + (RH - NH) / 2; return { x: n.x, y, w, h: NH, cx: n.x + w / 2, cy: y + NH / 2, row: n.layer }; };
  const bands = m.layers.map((l, i) => { const c = m.nodes.filter(n => n.layer === i).length; return `<g class="bl-band ${B.selLayer === i ? 'sel' : ''}" data-blayer="${i}"><rect x="2" y="${8 + i * RH + 3}" width="${W - 4}" height="${RH - 6}" rx="10"/><text x="14" y="${8 + i * RH + 22}">${esc(l.name)}</text><text x="14" y="${8 + i * RH + 38}" class="bl-bn">${c} part${c === 1 ? '' : 's'}</text></g>`; }).join('');
  const hulls = (m.inst || []).map((inst, i) => { const ids = Object.values(inst.roles).flat().filter(id => N.has(id)); if (!ids.length) return ''; const ps = ids.map(pos), g = i % 3 * 4, x1 = Math.min(...ps.map(p => p.x)) - 10 - g, y1 = Math.min(...ps.map(p => p.y)) - 18 - g, x2 = Math.max(...ps.map(p => p.x + p.w)) + 10 + g, y2 = Math.max(...ps.map(p => p.y + p.h)) + 10 + g, r = ck.rules[i], p = PL_BY[inst.pat], lab = `${p.name} · ${r.ok ? 'holds' : r.viol.length + ' broken'}`;
    return `<g class="bl-hull ${r.ok ? 'ok' : 'bad'} ${B.hl && B.hlKey !== 'i:' + inst.id ? 'dim' : ''}" data-binst="${inst.id}"><rect x="${x1}" y="${y1}" width="${x2 - x1}" height="${y2 - y1}" rx="14"/><g transform="translate(${x1 + 10},${y1 - 7})"><rect width="${lab.length * 6.2 + 12}" height="15" rx="4" class="bl-hl"/><text x="6" y="11">${esc(lab)}</text></g></g>`; }).join('');
  const has = new Set(m.edges.map(e => e.a + '>' + e.b));
  const edges = m.edges.filter(e => N.has(e.a) && N.has(e.b)).map(e => {
    const s = pos(e.a), t = pos(e.b), bad = upIds.has(e.id) || loopIds.has(e.id) || ruleIds.has(e.id), col = bad ? 'var(--break)' : PD_EDGE[e.kind][0], off = has.has(e.b + '>' + e.a) ? (e.a < e.b ? -8 : 8) : 0;
    let d, lx, ly;
    if (t.row !== s.row) { const down = t.row > s.row, x1 = s.cx + off + (t.cx - s.cx) * .1, x2 = t.cx + off - (t.cx - s.cx) * .1, y1 = down ? s.y + s.h : s.y, y2 = down ? t.y - 4 : t.y + t.h + 4, c = (y2 - y1) * .5; d = `M${x1},${y1} C${x1},${y1 + c} ${x2},${y2 - c} ${x2},${y2}`; lx = (x1 + x2) / 2; ly = (y1 + y2) / 2; }
    else { const sg = Math.sign(t.cx - s.cx) || 1, x1 = s.cx + sg * s.w / 2, x2 = t.cx - sg * (t.w / 2 + 4), y = s.cy + off, adj = Math.abs(x2 - x1) < 60, bow = adj ? 0 : -Math.min(40, 14 + Math.abs(x2 - x1) * .05); d = `M${x1},${y} C${x1 + sg * 24},${y + bow} ${x2 - sg * 24},${y + bow} ${x2},${y}`; lx = (x1 + x2) / 2; ly = y + bow * .75; }
    const label = e.label || (e.w ? String(e.w) : ''), designed = isMap && base && !base.e.has(ek(e));
    return `<g class="bl-e ${B.selEdge === e.id ? 'sel' : ''} ${e.by === 'ai' ? 'ai' : ''} ${designed ? 'dz' : ''} ${B.hl && !B.hl.has(e.a + '>' + e.b) ? 'dim' : ''}" data-bedge="${e.id}"><path d="${d}" class="bl-hit"/><path d="${d}" class="bl-ep" style="stroke:${col}${PD_EDGE[e.kind][1] && !bad ? ';stroke-dasharray:' + PD_EDGE[e.kind][1] : ''}" marker-end="url(#pdm-${bad ? 'x' : e.kind})"/>${label ? `<g class="pd-el"><rect x="${lx - label.length * 3.1 - 5}" y="${ly - 8}" width="${label.length * 6.2 + 10}" height="16" rx="4" style="stroke:${col}"/><text x="${lx}" y="${ly + 3.5}" text-anchor="middle" style="fill:${bad ? 'var(--break)' : 'var(--tx)'}">${esc(label)}</text></g>` : ''}</g>`;
  }).join('');
  const ghosts = isMap && base ? m.base.nodes.filter(n => !N.has(n.id)).map(n => { const y = 8 + Math.min(n.layer, m.layers.length - 1) * RH + (RH - NH) / 2; return `<g class="bl-gone"><rect x="${n.x}" y="${y}" width="${nW(n)}" height="${NH}" rx="6"/><text x="${n.x + nW(n) / 2}" y="${y + NH / 2 + 4}" text-anchor="middle">${esc(n.name)}</text></g>`; }).join('') : '';
  const nodes = m.nodes.map(n => { const p = pos(n.id), kk = { new: 'n', ext: 'o', store: 's', queue: 'q', part: '' }[n.kind] || '', sel = B.sel.includes(n.id), b0 = base && base.n.get(n.id), isNew = isMap && !b0, moved = isMap && b0 && (m.base.layers[b0.layer] || {}).id !== (m.layers[n.layer] || {}).id;
    const tags = [n.by === 'ai' || n.by === 'ai-edit' ? 'AI' : '', isNew ? 'new' : '', moved ? 'moved' : '', ck.inLoop.has(n.id) ? 'loop' : '', ck.hubs.some(h => h.id === n.id) ? 'hub' : ''].filter(Boolean);
    const pl = B.place && Object.entries(B.place.roles).find(([, ids]) => ids.includes(n.id));
    return `<g class="bl-n ${sel ? 'sel' : ''} ${n.by === 'ai' || n.by === 'ai-edit' ? 'ai' : ''} ${isNew ? 'dz' : ''} ${B.hl && !B.hl.has(n.id) ? 'dim' : ''} ${pl ? 'role' : ''}" data-bnode="${n.id}" transform="translate(${p.x},${p.y})"><title>${esc(n.name + (n.note ? ' — ' + n.note : '') + (n.ref ? '\nmap part: ' + n.ref : '') + (n.open ? '\nDouble-click to open its level' : ''))}</title><rect width="${p.w}" height="${p.h}" rx="${kk === 's' ? 12 : kk === 'q' ? 17 : 6}" class="pd-n ${kk}"/><text x="${p.w / 2 - (n.open ? 6 : 0)}" y="${p.h / 2 + 4}" text-anchor="middle" class="pd-nt ${kk}">${esc(n.name)}</text>${n.open ? `<text x="${p.w - 13}" y="${p.h / 2 + 5}" class="bl-open" data-bopen="${esc(n.ref)}">›</text>` : ''}${pl ? `<g transform="translate(0,${p.h + 3})"><rect width="${pl[0].length * 6 + 10}" height="13" rx="3" class="bl-rl"/><text x="5" y="10" class="bl-rt">${esc(pl[0])}</text></g>` : ''}${tags.map((t, i) => `<g class="bl-tag ${t}" transform="translate(${p.w - 4 - (i + 1) * 40},-8)"><rect width="38" height="14" rx="3"/><text x="19" y="10.5" text-anchor="middle">${t}</text></g>`).join('')}<circle cx="${p.w}" cy="${p.h / 2}" r="6" class="bl-h" data-bhandle="${n.id}"><title>Drag to another part to connect</title></circle></g>`; }).join('');
  const defs = `<defs>${Object.entries(PD_EDGE).map(([k2, v]) => `<marker id="pdm-${k2}" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,1 L9,5 L0,9 z" style="fill:${v[0]}"/></marker>`).join('')}</defs>`;
  svg.innerHTML = defs + bands + `<g>${ghosts}</g><g>${hulls}</g><g id="blEdges">${edges}</g><g id="blNodes">${nodes}</g><path id="blTmp" class="bl-tmp" d=""/>`;
  const bar = document.querySelector('.bl-bar'); if (bar) bar.innerHTML = bToolbar();
  bDrawInsp(); bDrawSide(ck);
}
function bDrawInsp() {
  const el = $('blInsp'); if (!el) return; const m = B.m;
  const n = B.sel.length === 1 ? m.nodes.find(x => x.id === B.sel[0]) : null, e = B.selEdge ? m.edges.find(x => x.id === B.selEdge) : null, L = B.selLayer != null ? m.layers[B.selLayer] : null;
  el.hidden = !(n || e || L || B.sel.length > 1) || !!B.place;
  if (n) { const p = n.stamp && PL_BY[n.stamp], inI = (m.inst || []).filter(i => Object.values(i.roles).some(v => v.includes(n.id)));
    el.innerHTML = `<button type="button" class="x" data-bclose aria-label="Close">✕</button><div class="eyebrow">${n.by === 'map' ? 'Part as built' : n.by === 'ai' ? 'Added by Claude' : n.by === 'ai-edit' ? 'Changed by Claude' : 'Designed part'}</div>
      <label class="bl-f">Name<input data-bf="name" value="${esc(n.name)}"></label><div class="bl-2"><label class="bl-f">Kind<select data-bf="kind">${Object.entries(BNK).map(([k, l]) => `<option value="${k}" ${n.kind === k ? 'selected' : ''}>${l}</option>`).join('')}</select></label><label class="bl-f">Layer<select data-bf="layer">${m.layers.map((l, i) => `<option value="${i}" ${n.layer === i ? 'selected' : ''}>${esc(l.name)}</option>`).join('')}</select></label></div>
      <label class="bl-f">Note<textarea data-bf="note" rows="2" placeholder="What it owns, why it is here">${esc(n.note || '')}</textarea></label>
      ${n.ref ? `<p class="note mono">map part: ${esc(n.ref)}${n.open ? ` · <button type="button" class="linkbtn" data-bopen="${esc(n.ref)}">open its level ›</button>` : ''}</p>` : ''}${p ? `<p class="note">Stamped from ${libLink(p.id, p.name)}</p>` : ''}${inI.length ? `<p class="note">In ${inI.map(i => `<b>${esc(PL_BY[i.pat].name)}</b> as ${Object.entries(i.roles).filter(([, v]) => v.includes(n.id)).map(([k]) => k).join(', ')}`).join('; ')}</p>` : ''}
      <div class="bl-f">Connect to<div class="row"><select id="bConnTo" class="bl-sel">${m.nodes.filter(x => x !== n).map(x => `<option value="${x.id}">${esc(x.name)}</option>`).join('')}</select><button type="button" class="bl-btn" data-bact="connect">Add ${BK[B.kind]}</button></div></div>
      <div class="row">${n.by === 'ai' || n.by === 'ai-edit' ? '<button type="button" class="bl-btn" data-bact="keep">Keep</button>' : ''}<button type="button" class="bl-btn danger" data-bact="del">${n.by === 'map' ? 'Remove in design' : 'Delete part'}</button></div>`; return; }
  if (B.sel.length > 1) { el.innerHTML = `<button type="button" class="x" data-bclose aria-label="Close">✕</button><div class="eyebrow">${B.sel.length} parts selected</div><p class="note">Drag any of them to move all. Shift-click to add or remove. Open Patterns to place a pattern on them.</p><div class="row"><button type="button" class="bl-btn" data-bside="pat">Place a pattern…</button><button type="button" class="bl-btn danger" data-bact="del">Delete ${B.sel.length} parts</button></div>`; return; }
  if (e) { const up = bCheck(m).up.some(x => x.id === e.id);
    el.innerHTML = `<button type="button" class="x" data-bclose aria-label="Close">✕</button><div class="eyebrow">${e.by === 'map' ? 'Reference as built' : e.by === 'ai' ? 'Added by Claude' : 'Designed edge'}</div><p class="mono" style="color:var(--tx)">${esc(nm(m, e.a))} → ${esc(nm(m, e.b))}</p>${e.w ? `<p class="note">${e.w} reference${e.w > 1 ? 's' : ''} in the code today.</p>` : ''}${up ? '<p style="color:var(--break);font-size:12.5px">Points up: the lower part depends on a higher one.</p>' : ''}
      <label class="bl-f">Kind<select data-be="kind">${Object.entries(BK).map(([k, l]) => `<option value="${k}" ${e.kind === k ? 'selected' : ''}>${l}</option>`).join('')}</select></label><label class="bl-f">Label<input data-be="label" value="${esc(e.label || '')}" placeholder="${e.w || ''}"></label>
      <div class="row"><button type="button" class="bl-btn" data-bact="flip">Reverse</button><button type="button" class="bl-btn danger" data-bact="delEdge">${e.by === 'map' ? 'Remove in design' : 'Delete edge'}</button></div>`; return; }
  if (L) el.innerHTML = `<button type="button" class="x" data-bclose aria-label="Close">✕</button><div class="eyebrow">Layer ${B.selLayer + 1} of ${m.layers.length}</div><label class="bl-f">Name<input data-bl="name" value="${esc(L.name)}"></label><div class="row"><button type="button" class="bl-btn" data-bact="lup" ${B.selLayer ? '' : 'disabled'}>Move up</button><button type="button" class="bl-btn" data-bact="ldown" ${B.selLayer < m.layers.length - 1 ? '' : 'disabled'}>Move down</button><button type="button" class="bl-btn danger" data-bact="ldel" ${m.nodes.some(n => n.layer === B.selLayer) ? 'disabled title="Move its parts first"' : ''}>Delete</button></div><p class="note">Double-click inside the layer to add a part there.</p>`;
}
function bDrawSide(ck) {
  const el = $('blSideBody'); if (!el || !B.m) return; const m = B.m; ck = ck || bCheck(m);
  document.querySelectorAll('.bl-sseg [data-bside]').forEach(b => b.setAttribute('aria-pressed', b.dataset.bside === B.side));
  if (B.side === 'ai') { el.innerHTML = bChatHtml(); const lg = el.querySelector('.bl-chat'); if (lg) lg.scrollTop = lg.scrollHeight; return; }
  if (B.side === 'changes') { el.innerHTML = bChangesHtml(); return; }
  if (B.side === 'pat') { el.innerHTML = bPatHtml(ck); return; }
  const L = bLedger(m), c0 = L.c0;
  const delta = (l, a, b, good) => { const better = good === 'down' ? b < a : b > a; return `<div class="bl-m"><span>${l}</span><b class="${a === b || good === 'none' ? '' : better ? 'good' : 'bad'}">${a === b ? b : a + ' → ' + b}</b></div>`; };
  const list = (t, items, fn) => items.length ? `<h3>${t} <b>${items.length}</b></h3><div class="stack" style="gap:2px">${items.slice(0, 12).map(fn).join('')}</div>` : '';
  const lt = ledgerText(L);
  el.innerHTML = `<div class="bl-ms">${delta('References pointing up', c0.up.length, ck.up.length, 'down')}${delta('Loops', c0.loops.length, ck.loops.length, 'down')}${delta('Outside systems with several doors', c0.doors.length, ck.doors.length, 'down')}${delta('Hubs', c0.hubs.length, ck.hubs.length, 'down')}${delta('Broken pattern rules', c0.broken, ck.broken, 'down')}${delta('Parts', c0.parts, ck.parts, 'none')}${delta('Edges', c0.edges, ck.edges, 'none')}</div>
    <div class="bl-lean ${L.verdict}"><b>Lean ledger.</b> Adds ${lt.inv}; ${lt.ex}. <span class="bl-v">${{ none: 'No design yet.', earns: 'This earns its keep.', inventory: 'So far this is inventory: what does it buy?', worse: 'This makes the structure worse.' }[L.verdict]}</span></div>
    ${list('Pointing up', ck.up, e => `<button type="button" class="bl-row" data-bhl="${e.a}>${e.b}">${esc(nm(m, e.a))} → ${esc(nm(m, e.b))}</button>`)}
    ${list('Loops', ck.loops, l => `<button type="button" class="bl-row" data-bhl="${l.join(',')}">${esc(l.map(id => nm(m, id)).join(' ⇄ '))}</button>`)}
    ${list('Reached from several parts', ck.doors, x => `<button type="button" class="bl-row" data-bhl="${[x.id, ...x.from].join(',')}">${esc(nm(m, x.id))} <span class="note">from ${x.from.length}</span></button>`)}
    ${list('Hubs', ck.hubs, h => `<button type="button" class="bl-row" data-bhl="${h.id}">${esc(h.name)}</button>`)}
    ${list('Broken pattern rules', ck.rules.filter(r => !r.ok), r => `<button type="button" class="bl-row" data-bhl="i:${r.inst.id}">${esc(PL_BY[r.inst.pat].name)}: ${esc(r.viol[0].why)}</button>`)}
    ${!ck.up.length && !ck.loops.length && !ck.doors.length && !ck.broken ? '<p class="note" style="color:var(--good)">Every reference points down or sideways, there are no loops, each outside system has one door, and every placed pattern holds.</p>' : ''}
    <p class="note">Same rules as the map: layers run top to bottom; a dependency may point down or sideways; events may go up. Click a row to highlight it.</p>`;
}
function bPatHtml(ck) {
  const m = B.m;
  if (B.place) return bPlaceHtml();
  const fits = bFits(m);
  const inst = (m.inst || []).map((i, ix) => { const r = ck.rules[ix], p = PL_BY[i.pat]; return `<div class="bl-inst ${r.ok ? 'ok' : 'bad'}"><div class="row" style="justify-content:space-between"><button type="button" class="bl-row" data-bhl="i:${i.id}"><b>${esc(p.name)}</b></button><span class="stt s-${r.ok ? 'holds' : 'against'}">${r.ok ? 'Holds' : r.viol.length + ' broken'}</span></div><div class="note">${PL_ROLES[i.pat].roles.map(([k, l]) => `${l}: <span class="mono">${esc(names(m, i.roles[k]))}</span>`).join('<br>')}</div>${r.viol.map(v => `<div class="bl-viol">✕ ${esc(v.why)}${v.miss ? ' (missing)' : ''}</div>`).join('')}<div class="row">${libLink(i.pat, 'Read')}<button type="button" class="linkbtn" data-binstdel="${i.id}">Remove</button></div></div>`; }).join('');
  const det = B.ctx.kind === 'map' ? (() => { try { return patterns(view(B.ctx.level, null)).list; } catch (_) { return []; } })() : [];
  const fitRows = fits.map(f => { const L = fitPreview(m, f), t = ledgerText(L); return `<div class="bl-fit"><div class="row" style="justify-content:space-between;align-items:baseline"><b>${esc(f.title)}</b>${libLink(f.pat, PL_BY[f.pat].name)}</div><p class="note">${esc(f.why)}</p><p class="bl-led ${L.verdict}">Adds ${esc(t.inv)} · ${esc(t.ex)}</p><div class="row"><button type="button" class="bl-btn" data-bhl="${f.focus.join(',')}">Show</button><button type="button" class="bl-btn" data-bfit="${esc(f.id)}">Apply</button></div></div>`; }).join('');
  const q = B.q.toLowerCase(), cand = PL.filter(p => !p.anti && p.lvl !== 'read' && (!q || (p.name + ' ' + (p.aka || '')).toLowerCase().includes(q)));
  return `${(m.inst || []).length ? `<h3>Placed here <b>${m.inst.length}</b></h3><div class="stack">${inst}</div>` : ''}
    <h3>Where patterns fit <b>${fits.length}</b></h3>${fits.length ? `<div class="stack">${fitRows}</div>` : '<p class="note">Nothing here that a pattern would fix: no extra doors, upward references, loops, shared reach into lower parts, or stores with several writers.</p>'}
    ${det.length ? `<h3>Found by the map here</h3><div class="stack" style="gap:4px">${det.slice(0, 8).map(p => `<div class="bl-det"><span class="stt s-${p.st}">${PL_STL[p.st] || p.st}</span> <span>${esc(p.name)}</span> ${libLink(p.id, 'Read')}</div>`).join('')}</div>` : ''}
    <h3>Place or stamp a pattern</h3><div class="find"><input id="bq" type="search" placeholder="Search patterns…" value="${esc(B.q)}" aria-label="Search patterns" style="width:100%"></div>
    <p class="note">${B.sel.length ? `Your ${B.sel.length} selected part${B.sel.length > 1 ? 's' : ''} will be offered for its roles.` : 'Select parts first (shift-click) to bind them to the pattern’s roles.'} <b>Place</b> checks a pattern on parts you have; <b>Stamp</b> draws its boxes as new parts.</p>
    <div class="bl-pl">${cand.map(p => `<div class="bl-pi"><div class="bl-pd">${plDiagram({ rows: p.dg.rows, n: p.dg.n, e: p.dg.e.filter(e => e[2] !== 'x') }, { compact: true, w: 300, rh: 34, label: p.name })}</div><div class="row" style="justify-content:space-between"><span>${esc(p.name)}</span><span class="row" style="gap:4px">${PL_ROLES[p.id] ? `<button type="button" class="bl-btn sm" data-bplace="${p.id}">Place</button>` : ''}<button type="button" class="bl-btn sm" data-bstamp="${p.id}">Stamp</button></span></div></div>`).join('')}</div>`;
}
function bPlaceStart(pid) {
  const spec = PL_ROLES[pid], sel = B.sel.map(id => B.m.nodes.find(n => n.id === id)).filter(Boolean).sort((a, b) => a.layer - b.layer || a.x - b.x), roles = {};
  spec.roles.forEach(([k]) => { roles[k] = []; });
  if (spec.roles.length === 1) roles[spec.roles[0][0]] = sel.map(n => n.id);
  else if (sel.length) { const rows = [...new Set(sel.map(n => n.layer))], used = new Set(); spec.roles.forEach(([k, , many], i) => { const r = rows[Math.min(i, rows.length - 1)]; const pick = sel.filter(n => n.layer === r && !used.has(n.id)); const take = (many ? pick : pick.slice(0, 1)).map(n => n.id); take.forEach(id => used.add(id)); roles[k] = take; }); }
  return { pid, roles, cur: spec.roles[0][0] };
}
function bPlaceHtml() {
  const P = B.place, p = PL_BY[P.pid], spec = PL_ROLES[P.pid], m = B.m, tmp = { id: 'tmp', pat: P.pid, roles: P.roles }, r = instCheck(m, tmp), filled = spec.roles.every(([k]) => P.roles[k].length);
  const fill = s => s.replace(/\{(\w+)\}/g, (_, k) => P.roles[k] && P.roles[k].length ? names(m, P.roles[k]) : '[' + k + ']');
  return `<div class="eyebrow">Place a pattern</div><h3 style="font-size:14px;text-transform:none;letter-spacing:0;color:var(--tx)">${esc(p.name)}</h3><p class="note">Pick a role, then click parts on the canvas to add or remove them.</p>
    <div class="stack" style="gap:4px">${spec.roles.map(([k, l, many]) => `<button type="button" class="bl-role ${P.cur === k ? 'on' : ''}" data-brole="${k}"><span>${esc(l)}${many ? '' : ' (one)'}</span><span class="mono">${esc(names(m, P.roles[k]))}</span></button>`).join('')}</div>
    <h3>Its rules</h3><div class="stack" style="gap:3px">${spec.rules.map(ru => { const txt = fill(ru.why), v = filled && r.viol.some(x => x.why === txt); return `<div class="bl-rule ${!filled ? '' : v ? 'bad' : 'ok'}">${!filled ? '·' : v ? '✕' : '✓'} ${esc(txt)}</div>`; }).join('')}</div>
    <p class="note">A broken rule is fine to start with: placing the pattern records the decision, and the checks show what still has to change.</p>
    <div class="row"><button type="button" class="openbtn" data-bact="placeok" ${filled ? '' : 'disabled'}>Place it</button><button type="button" class="bl-btn" data-bact="placecancel">Cancel</button>${libLink(p.id, 'Read')}</div>`;
}
function bChangesHtml() {
  const m = B.m, steps = bSteps(m, bDiff(m)), L = bLedger(m), lt = ledgerText(L), all = B.ctx.kind === 'map' ? dsLevels() : [];
  const decs = (m.inst || []).map(i => { const p = PL_BY[i.pat], r = instCheck(m, i); return `<div class="pstep" style="padding:10px 12px"><div class="t"><b>${esc(p.name)}</b><span class="stt s-${p.door === 'one' ? 'against' : 'holds'}">${p.door === 'one' ? 'One-way door' : 'Two-way door'}</span><span class="stt s-${r.ok ? 'holds' : 'against'}">${r.ok ? 'Holds' : 'Not yet true'}</span></div><p class="note">${esc(p.doorWhy)}</p><p class="note">Sources: ${esc(p.src.map(x => PL_SRC[x[0]].a.split(' and ')[0].split(',')[0] + ', ' + x[1]).join(' · '))}</p></div>`; }).join('');
  return `<div class="bl-lean ${L.verdict}"><b>Lean ledger.</b> Adds ${lt.inv}; ${lt.ex}.</div>
    <h3>Steps at this level <b>${steps.length}</b></h3>${steps.length ? `<ol class="steps">${steps.map((s, i) => `<li><i>${i + 1}</i><span>${esc(s)}</span></li>`).join('')}</ol>` : '<p class="note">No design changes here yet.</p>'}
    <h3>Decisions <b>${(m.inst || []).length}</b></h3>${decs || '<p class="note">Each pattern you place becomes a decision here, with its door, the ledger and its sources.</p>'}
    ${B.ctx.kind === 'map' ? `<p class="note">${all.length ? `The design has changes at ${all.length} level${all.length > 1 ? 's' : ''}. The Plan tab lists them all, first.` : 'The Plan tab lists every designed level, first.'}</p>` : ''}
    <div class="row"><button type="button" class="openbtn" data-bact="copy" ${steps.length ? '' : 'disabled'}>Copy steps</button><button type="button" class="openbtn" data-bact="adr" ${(m.inst || []).length ? '' : 'disabled'}>Copy ADR drafts</button>${B.ctx.kind === 'map' ? `<button type="button" class="openbtn" data-bact="target" ${all.length || (m.inst || []).length || steps.length ? '' : 'disabled'}>Copy architecture.json target</button>` : ''}</div><div id="bCopied" class="note"></div>`;
}
function bSteps(m, d) {
  const L = i => (m.layers[i] || {}).name, out = [];
  d.layers.forEach(l => out.push(`Add a layer: ${l.name}.`));
  d.added.filter(n => n.kind !== 'ext').forEach(n => out.push(`Create ${n.name} in ${L(n.layer)}${n.stamp ? ` (${PL_BY[n.stamp].name})` : ''}${n.note ? ': ' + n.note.replace(/\.$/, '') : ''}.`));
  d.added.filter(n => n.kind === 'ext').forEach(n => out.push(`Start using ${n.name} (outside system).`));
  d.renamed.forEach(([o, n]) => out.push(`Rename ${o} to ${n.name}.`));
  d.moved.forEach(([o, n]) => out.push(`Move ${n.name} from ${o} to ${L(n.layer)}.`));
  d.eAdded.forEach(e => out.push(`Route ${nm(m, e.a)} → ${nm(m, e.b)} as a ${BK[e.kind]}${e.label ? ' (' + e.label + ')' : ''}.`));
  d.eRemoved.forEach(e => out.push(`Remove ${e.w ? 'the ' + e.w + ' reference' + (e.w > 1 ? 's' : '') : 'the dependency'} from ${nm(m, e.a)} to ${nm(m, e.b)}.`));
  d.removed.forEach(n => out.push(`Retire ${n.name}.`));
  return out;
}
/* exports: ADR drafts and the architecture.json target section */
function adrMd(m) {
  const L = bLedger(m), lt = ledgerText(L);
  return (m.inst || []).map(i => { const p = PL_BY[i.pat], r = instCheck(m, i), R = PL_ROLES[i.pat];
    return `# ${p.name} at ${m.origin.name}\n\nStatus: Proposed\n\n## Context\n\n${i.note || p.need}\n\n## Decision\n\n${p.sum}\n\n${R.roles.map(([k, l]) => `- ${l}: ${names(m, i.roles[k])}`).join('\n')}\n\nRules this commits to:\n\n${R.rules.map(ru => `- ${ru.why.replace(/\{(\w+)\}/g, (_, k) => names(m, i.roles[k] || []))}`).join('\n')}\n\n## Consequences\n\n- ${p.door === 'one' ? 'One-way door' : 'Two-way door'}: ${p.doorWhy}\n- Cost: ${p.cost}\n- Lean ledger for this level's design: adds ${lt.inv}; ${lt.ex}.\n- Today: ${r.ok ? 'every rule holds.' : r.viol.map(v => v.why).join('; ') + ': not yet true.'}\n\n## Sources\n\n${p.src.map(x => `- ${PL_SRC[x[0]].a}${PL_SRC[x[0]].y ? ' (' + PL_SRC[x[0]].y + ')' : ''}, ${PL_SRC[x[0]].t}: ${x[1]}${x[2] ? ' ' + x[2] : ''}`).join('\n')}\n`; }).join('\n---\n\n');
}
function targetJson() {
  const d = dsDoc(), parts = [], pats = [], slug = s => String(s).toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
  for (const lv of dsLevels()) { const m = dsApply(bFromMap(lv), d.levels[lv]), df = bDiff(m);
    df.added.filter(n => n.kind !== 'ext').forEach(n => parts.push({ id: slug(n.name), name: n.name, layer: m.layers[n.layer].name, level: lv || '(root)', status: 'proposed', ...(n.note ? { description: n.note } : {}) }));
    (m.inst || []).forEach(i => pats.push({ pattern: i.pat, level: lv || '(root)', roles: Object.fromEntries(Object.entries(i.roles).map(([k, v]) => [k, v.map(id => { const n = m.nodes.find(x => x.id === id); return n ? (n.ref || slug(n.name)) : id; })])) }));
  }
  return JSON.stringify({ target: { note: 'Drawn in er Map. Proposed parts, and the patterns the checker should hold the code to.', parts, patterns: pats } }, null, 2);
}
/* the Plan tab reads the design */
function bldPlanHtml() {
  if (typeof state === 'undefined' || !S) return '';
  const d = dsDoc(), secs = [];
  for (const l of dsLevels()) { const m = dsApply(bFromMap(l), d.levels[l]), st = bSteps(m, bDiff(m)), lt = ledgerText(bLedger(m)); if (!st.length && !(m.inst || []).length) continue;
    secs.push(`<div class="pstep"><div class="t"><b>${esc(fullName(l) || (S.repo ? S.repo.name : S.label) + ' · whole system')}</b><span class="note">${st.length} step${st.length === 1 ? '' : 's'}${(m.inst || []).length ? ` · ${m.inst.length} pattern${m.inst.length > 1 ? 's' : ''}: ${esc(m.inst.map(i => PL_BY[i.pat].name).join(', '))}` : ''}</span><button type="button" class="linkbtn" data-dsopen="${esc(l)}">Open in Diagram</button></div><p class="note">Adds ${esc(lt.inv)}; ${esc(lt.ex)}.</p>${st.length ? `<ol class="steps">${st.map((s, i) => `<li><i>${i + 1}</i><span style="color:var(--tx)">${esc(s)}</span></li>`).join('')}</ol>` : ''}</div>`); }
  if (!secs.length) return '';
  return `<section class="phase"><div class="phase-h"><span class="no" style="background:var(--accent);color:var(--on-accent)">D</span><div class="stack" style="gap:2px;max-width:62ch"><h2>From your design <span class="note" style="font-weight:400">· ${secs.length} level${secs.length > 1 ? 's' : ''}</span></h2><p>Drawn in Map › Diagram. The steps come from the difference between the design and the code as built, so they shrink as the code catches up.</p></div></div>${secs.join('')}</section>`;
}
/* the Map shows placed patterns as outlines around their tiles */
function dsMapTags(L, level) {
  const d = dsDoc(), x = d.levels[level]; if (!x || !L) return '';
  const out = [], all = Object.values(L.pos || {}), bottom = all.length ? Math.max(...all.map(r => r.y + r.h)) : 0, left = all.length ? Math.min(...all.map(r => r.x)) : 10;
  (x.inst || []).forEach((i, k) => { const rs = Object.values(i.roles).flat().map(id => id.startsWith('p:') ? L.pos[id.slice(2)] : null).filter(Boolean); if (!rs.length) return; const p = PL_BY[i.pat], pad = 4 + k * 4, lab = p.name + ' · design', r0 = rs.slice().sort((a, b) => a.y - b.y || a.x - b.x)[0];
    out.push(`<g class="ds-hull">${rs.map(r => `<rect x="${r.x - pad}" y="${r.y - pad}" width="${r.w + pad * 2}" height="${r.h + pad * 2}" rx="10"/>`).join('')}<g transform="translate(${r0.x - pad + 8},${r0.y - pad - 8 - k * 17})"><rect width="${lab.length * 6.3 + 12}" height="16" rx="4"/><text x="6" y="11.5">${esc(lab)}</text></g></g>`); });
  return out.join('');
}

/* the Map's reading column says when this level has a design */
function dsReadBanner() {
  const el = $('read'); if (!el) return; const x = dsDoc().levels[state.path]; if (!x || !dsHasChanges(x)) return;
  const m = dsApply(bFromMap(state.path), x), d = bDiff(m), n = d.added.length + d.removed.length + d.moved.length + d.renamed.length + d.eAdded.length + d.eRemoved.length, lt = ledgerText(bLedger(m));
  const box = document.createElement('div'); box.className = 'ds-read';
  box.innerHTML = `<div class="eyebrow" style="color:var(--accent)">Your design at this level</div><p style="color:var(--tx)">${n} change${n === 1 ? '' : 's'}${(m.inst || []).length ? ` · ${m.inst.map(i => esc(PL_BY[i.pat].name)).join(', ')}` : ''}</p><p class="note">Adds ${esc(lt.inv)}; ${esc(lt.ex)}. Dashed outlines on the map mark placed patterns; new parts and rerouted edges show in the Diagram.</p><button type="button" class="linkbtn" data-dsopen="${esc(state.path)}" style="text-align:left">Open in Diagram →</button>`;
  el.prepend(box);
}

/* ───────── Claude as co-creator ───────── */
let bSample = null, bSampleReady = false;
async function bInitAI() { if (bSampleReady) return bSample; bSampleReady = true; try { bSample = window.claude && window.claude.use ? await window.claude.use('sample') : null; } catch (_) { bSample = null; } return bSample; }
function bChatHtml() {
  const chat = bChat();
  const avail = bSample ? '' : bSampleReady ? '<p class="note">Claude is not available in this view. Open the artifact in Claude to design with it; everything else works without it.</p>' : '<p class="note">Connecting to Claude…</p>';
  const sugg = ['Where do patterns fit here?', 'Remove the loops with the smallest change', 'What would you remove? Use the Lean lens', 'Turn this level into ports and adapters'];
  return `<div class="bl-chat">${chat.length ? chat.map(t => `<div class="bl-msg ${t.role}">${t.role === 'user' ? esc(t.content) : bMd(t.content)}${t.edits && t.edits.length ? `<div class="bl-edits">${t.edits.map(x => `<span class="${/^Error/.test(x) ? 'err' : ''}">${esc(x)}</span>`).join('')}</div>` : ''}</div>`).join('') : `<div class="bl-msg assistant"><p>I can design on this level with you: add, move, connect and remove parts, place library patterns on real parts, apply the fits the map computes, and explain the trade-offs with sources. My changes are marked <span class="bl-tag ai inline">AI</span> and can be undone.</p></div>`}${B.busy ? `<div class="bl-msg assistant"><p class="note">${esc(B.status || 'Thinking…')}</p>${B.live ? bMd(B.live) : ''}</div>` : ''}</div>
    ${avail}${B.aiUndo != null && !B.busy ? `<div class="row" style="margin:6px 0"><button type="button" class="bl-btn" data-bact="aiundo">Undo Claude’s last changes</button><button type="button" class="bl-btn" data-bact="keepall">Keep all</button></div>` : ''}
    ${!chat.length && bSample ? `<div class="kinds" style="margin:6px 0">${sugg.map(s => `<button type="button" class="kind" data-bsugg="${esc(s)}">${esc(s)}</button>`).join('')}</div>` : ''}
    <form class="bl-ask" id="bAskF"><textarea id="bAsk" rows="2" placeholder="${bSample ? 'Ask Claude to change the design or explain a choice…' : 'Claude is unavailable here'}" ${bSample && !B.busy ? '' : 'disabled'}></textarea><div class="row">${B.busy ? '<button type="button" class="bl-btn" data-bact="stop">Stop</button>' : `<button type="submit" class="openbtn" ${bSample ? '' : 'disabled'}>Send</button>`}${chat.length && !B.busy ? '<button type="button" class="bl-btn" data-bact="clearchat">Clear chat</button>' : ''}</div></form>`;
}
function bMd(t) { return String(t).split(/\n{2,}/).map(p => /^\s*[-*] /m.test(p) ? `<ul>${p.split('\n').filter(l => l.trim()).map(l => `<li>${bInline(l.replace(/^\s*[-*] /, ''))}</li>`).join('')}</ul>` : `<p>${bInline(p)}</p>`).join(''); }
function bInline(s) { return esc(s).replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>').replace(/`([^`]+)`/g, '<code>$1</code>').replace(/\n/g, '<br>'); }
function bCanvasText(m) {
  const ck = bCheck(m), N = id => nm(m, id);
  return `LAYERS (top to bottom): ${m.layers.map((l, i) => `${i}: ${l.name}`).join(' | ')}
PARTS (id · name · layer · kind · origin):
${m.nodes.map(n => `- ${n.id} · ${n.name} · ${m.layers[n.layer].name} · ${n.kind} · ${n.by === 'map' ? 'as built' : n.by === 'ai' ? 'added by you' : 'designed'}${n.note ? ' · note: ' + n.note : ''}`).join('\n') || '(none)'}
EDGES (from → to · kind · references in today's code):
${m.edges.map(e => `- ${N(e.a)} → ${N(e.b)} · ${BK[e.kind]}${e.w ? ' · ' + e.w : ' · designed'}${e.label ? ' · "' + e.label + '"' : ''}`).join('\n') || '(none)'}
PLACED PATTERNS: ${(m.inst || []).map((i, k) => `${PL_BY[i.pat].name} [${Object.entries(i.roles).map(([r, v]) => r + '=' + names(m, v)).join('; ')}] ${ck.rules[k].ok ? 'holds' : 'broken: ' + ck.rules[k].viol.map(v => v.why).join('; ')}`).join(' | ') || 'none'}
CHECKS NOW: ${ck.up.length} pointing up${ck.up.length ? ' (' + ck.up.map(e => N(e.a) + '→' + N(e.b)).join(', ') + ')' : ''}; ${ck.loops.length} loops${ck.loops.length ? ' (' + ck.loops.map(l => l.map(N).join('⇄')).join('; ') + ')' : ''}; ${ck.doors.length} outside systems reached from several parts${ck.doors.length ? ' (' + ck.doors.map(d => N(d.id) + ' from ' + d.from.length).join(', ') + ')' : ''}; ${ck.hubs.length} hubs.`;
}
function bRules() {
  const repo = S.repo ? `${S.repo.name}${S.repo.branch ? ' (' + S.repo.branch + ')' : ''}` : S.label || 'a codebase';
  return `You are co-designing a software architecture with an engineer, inside "er Map", a tool that maps ${repo}.
${B.ctx.kind === 'map' ? `You are working on the level "${B.m.origin.name}". Parts marked "as built" are the real code, with counted references. Changes you make are a design layered on top; the code is not changed.` : 'This is a blank canvas: a system that does not exist yet.'}

Conventions (the same rules the map checks):
- Layers run top to bottom. A dependency (call, command, network, data) should point down or sideways. Events may point up.
- A loop is a defect. Each outside system should be reached from one part.
- A placed pattern binds library roles to parts and is checked by the pattern's rules.

How to work:
- When asked what to improve, call find_sites first: it returns deterministic fits with their Lean ledger. Apply one with apply_edits op apply_fit{fit}.
- Change the canvas only through apply_edits; batch the operations for one idea into one call. Use part ids or exact names.
- To record a pattern on existing parts use place_pattern{pattern, roles:{role:[parts]}}; to draw a pattern's boxes as new parts use stamp_pattern.
- Prefer the smallest change. Reuse parts before adding new ones; name new parts in the codebase's own terms.
- Lean lens (Poppendieck, Lean Software Development, 2003): every part you add is inventory. Say what each addition removes. If nothing, do not add it.
- Say whether each structural decision is a one-way or two-way door, and name the pattern and its source (for example "Gateway, Fowler PoEAA").
- After editing, answer in at most 120 words. No headings. Do not restate the canvas.
- If the engineer only asks a question, answer it without editing.

CHECKABLE PATTERNS (id: roles): ${Object.entries(PL_ROLES).map(([id, s]) => `${id}: ${s.roles.map(r => r[0] + (r[2] ? '[]' : '')).join(', ')}`).join(' · ')}

PATTERN LIBRARY (id: name — summary):
${PL.filter(p => !p.anti).map(p => `${p.id}: ${p.name} — ${p.sum}`).join('\n')}`;
}
const B_TOOLS = () => [
  { name: 'apply_edits', description: 'Apply a list of edits in order; returns what happened and the canvas afterwards. Ops: add_part{name,layer,kind?(part|new|ext|store|queue),near?,note?}, remove_part{id}, rename_part{id,name}, move_part{id,layer}, set_note{id,text}, add_edge{from,to,kind(call|event|command|network|data),label?}, remove_edge{from,to}, reroute_edge{from,to,via}, add_layer{name,at?}, rename_layer{layer,name}, stamp_pattern{pattern,layer?}, place_pattern{pattern,roles:{role:[parts]},note?}, remove_pattern{id}, apply_fit{fit}.',
    inputSchema: { type: 'object', properties: { ops: { type: 'array', items: { type: 'object' } } }, required: ['ops'] },
    execute: async (input, ctx) => {
      const ops = Array.isArray(input.ops) ? input.ops.slice(0, 40) : []; if (ctx.signal.aborted) throw new Error('stopped');
      const res = bApply(ops.map(o => Object.assign({}, o, { op: String(o.op || '') })), 'ai'); B.turnEdits.push(...res); B.status = res.filter(r => !/^Error/.test(r)).slice(-1)[0] || 'Editing…'; bDraw();
      return { results: res, canvas: bCanvasText(B.m) };
    } },
  { name: 'find_sites', description: 'Find where library patterns would remove a structural exception at this level: extra doors to outside systems, upward references, loops, several callers reaching into the same lower parts, stores with several writers. Returns each fit with its id, pattern, why, and Lean ledger preview. Deterministic and cheap.',
    execute: () => { B.status = 'Looking for fits…'; bDrawSide(); return bFits(B.m).map(f => { const t = ledgerText(fitPreview(B.m, f)); return { fit: f.id, pattern: f.pat, title: f.title, why: f.why, ledger: `adds ${t.inv}; ${t.ex}` }; }); } },
  { name: 'read_pattern', description: 'Read one library pattern in full: how it works, when to use it, when it is overkill, its costs, its door, its Lean check, its roles and rules, and its sources.',
    inputSchema: { type: 'object', properties: { id: { type: 'string' } }, required: ['id'] },
    execute: input => { const p = PL_BY[String(input.id)] || plFind(String(input.id)); if (!p) throw new Error('No such pattern'); B.status = 'Reading ' + p.name + '…'; bDrawSide(); return { name: p.name, how: p.how, flow: p.flow, use: p.use, avoid: p.avoid, cost: p.cost, door: p.door + '-way: ' + p.doorWhy, lean: p.lean, roles: PL_ROLES[p.id] ? PL_ROLES[p.id].roles.map(r => r[0]) : null, rules: PL_ROLES[p.id] ? PL_ROLES[p.id].rules.map(r => r.why) : null, sources: p.src.map(r => PL_SRC[r[0]].a + ', ' + PL_SRC[r[0]].t + ': ' + r[1]) }; } },
];
async function bAsk(text) {
  text = String(text || '').trim(); if (!text || B.busy) return;
  await bInitAI(); if (!bSample) { bDrawSide(); return; }
  const chat = bChat(); chat.push({ role: 'user', content: text });
  B.aiUndo = JSON.stringify(Object.assign({}, B.m, { chat: undefined })); bPush(); B.turnEdits = []; B.busy = true; B.status = 'Thinking…'; B.live = ''; B.side = 'ai'; bDrawSide();
  const turns = chat.slice(-10).map(t => ({ role: t.role, content: t.content }));
  const input = [{ role: 'user', content: bRules() + '\n\nCURRENT LEVEL\n' + bCanvasText(B.m) + '\n\n(The conversation follows.)' }, ...turns];
  while (input[1] && input[1].role === 'assistant') input.splice(1, 1);
  B.ctl = new AbortController();
  try {
    const r = await bSample(input, { tools: B_TOOLS(), signal: B.ctl.signal, onText: ({ text: t }) => { B.live = t; const el = document.querySelector('.bl-chat .bl-msg:last-child'); if (el) el.innerHTML = `<p class="note">${esc(B.status || '')}</p>` + bMd(t); } });
    chat.push({ role: 'assistant', content: r.text.trim() + (r.truncated ? '\n\n(Cut short.)' : ''), edits: B.turnEdits });
  } catch (e) {
    const msg = { cancelled: 'Stopped.', not_granted: 'Claude was not allowed for this page, so the design stays yours alone.', rate_limited: 'Too many requests just now. Try again in a moment.', tools_unavailable: 'This view cannot let Claude edit the canvas.', refused: 'Claude declined that request.' }[e && e.code] || 'Something went wrong talking to Claude. Try again.';
    chat.push({ role: 'assistant', content: (e && e.text ? e.text.trim() + '\n\n' : '') + msg, edits: B.turnEdits });
    if (e && (e.code === 'not_granted' || e.code === 'sampling_disabled')) bSample = null;
  }
  if (!B.turnEdits.length) { B.aiUndo = null; B.hist.pop(); }
  B.busy = false; B.live = ''; B.ctl = null; bSave(); if (B.ctx.kind === 'map') dsPersist(); bDraw();
}

/* ───────── interaction ───────── */
function bPt(ev) { const svg = $('blSvg'), p = svg.createSVGPoint(); p.x = ev.clientX; p.y = ev.clientY; return p.matrixTransform(svg.getScreenCTM().inverse()); }
const rowAt = y => Math.max(0, Math.min(B.m.layers.length - 1, Math.floor((y - 8) / RH)));
document.addEventListener('pointerdown', ev => {
  const svg = $('blSvg'); if (!svg || !svg.contains(ev.target) || ev.button !== 0) return;
  if (ev.target.closest('[data-bopen]')) return;
  const h = ev.target.closest('[data-bhandle]'), g = ev.target.closest('[data-bnode]');
  if (!h && !B.place && !ev.shiftKey && (g || !ev.target.closest('[data-bedge],[data-binst]'))) { const now = performance.now(), key = g ? g.dataset.bnode : 'bg', last = B.lastTap; B.lastTap = { key, t: now, x: ev.clientX, y: ev.clientY };
    if (last && last.key === key && now - last.t < 420 && Math.hypot(ev.clientX - last.x, ev.clientY - last.y) < 8) { ev.preventDefault(); B.lastTap = null; B.drag = null; bDouble(g ? key : null, bPt(ev)); return; } }
  if (h && !B.place) { ev.preventDefault(); B.conn = { from: h.dataset.bhandle }; svg.setPointerCapture(ev.pointerId); return; }
  if (g) {
    ev.preventDefault(); const id = g.dataset.bnode;
    if (B.place) { const r = B.place.roles[B.place.cur], many = PL_ROLES[B.place.pid].roles.find(x => x[0] === B.place.cur)[2]; B.place.roles[B.place.cur] = r.includes(id) ? r.filter(x => x !== id) : many ? [...r, id] : [id]; bDraw(); return; }
    if (ev.shiftKey) { B.sel = B.sel.includes(id) ? B.sel.filter(x => x !== id) : [...B.sel, id]; B.selEdge = null; B.selLayer = null; bDraw(); return; }
    if (!B.sel.includes(id)) { B.sel = [id]; B.selEdge = null; B.selLayer = null; }
    const p = bPt(ev); B.drag = { ids: B.sel.slice(), x0: p.x, y0: p.y, start: B.m.nodes.filter(n => B.sel.includes(n.id)).map(n => [n.id, n.x, n.layer]), moved: false };
    svg.setPointerCapture(ev.pointerId); bDraw(); return;
  }
});
document.addEventListener('pointermove', ev => {
  if (B.conn) { const n = B.m.nodes.find(x => x.id === B.conn.from), p = bPt(ev), y = 8 + n.layer * RH + RH / 2, x = n.x + nW(n); const t = $('blTmp'); if (t) t.setAttribute('d', `M${x},${y} L${p.x},${p.y}`); return; }
  if (!B.drag) return; const p = bPt(ev), dx = p.x - B.drag.x0, dy = p.y - B.drag.y0;
  if (!B.drag.moved && Math.hypot(dx, dy) < 4) return; if (!B.drag.moved) { bPush(); B.drag.moved = true; }
  const dr = Math.round(dy / RH);
  for (const [id, x, l] of B.drag.start) { const n = B.m.nodes.find(q => q.id === id); n.x = Math.max(GUT, x + dx); n.layer = Math.max(0, Math.min(B.m.layers.length - 1, l + dr)); }
  bDrawFast();
});
document.addEventListener('pointerup', ev => {
  if (B.conn) { const el = document.elementFromPoint(ev.clientX, ev.clientY), g = el && el.closest && el.closest('[data-bnode]'); const from = B.conn.from; B.conn = null; if (g && g.dataset.bnode !== from) { bPush(); bOp({ op: 'add_edge', from, to: g.dataset.bnode, kind: B.kind }, 'you'); bSave(); } bDraw(); return; }
  if (B.drag) { const moved = B.drag.moved, ids = B.drag.ids; B.drag = null; if (moved) { bUnoverlap(ids); bSave(); bDraw(); } }
});
function bUnoverlap(ids) {
  for (const id of ids) { const n = B.m.nodes.find(x => x.id === id); if (!n) continue;
    for (let guard = 0; guard < 40; guard++) { const hit = B.m.nodes.find(o => o !== n && !ids.includes(o.id) && o.layer === n.layer && n.x < o.x + nW(o) + 8 && o.x < n.x + nW(n) + 8); if (!hit) break; n.x = hit.x + nW(hit) + 14; } }
}
let bFastT = 0; function bDrawFast() { if (bFastT) return; bFastT = requestAnimationFrame(() => { bFastT = 0; bDraw(); }); }
function bGoLevel(id) { clearTimeout(bSaveT); bSaveNow(); B.ctxKey = ''; state.path = id; state.sel = null; render(); }
/* double-clicks are detected on pointer-down: the first click redraws the canvas, so the browser never sees two clicks on one element */
function bDouble(id, p) {
  if (id) { const n = B.m.nodes.find(x => x.id === id); if (n && n.open && B.ctx.kind === 'map') { bGoLevel(n.ref); return; } B.sel = [id]; bDraw(); const i = document.querySelector('[data-bf="name"]'); if (i) { i.focus(); i.select(); } return; }
  bPush(); bOp({ op: 'add_part', name: 'New part', layer: rowAt(p.y), x: Math.max(GUT + 4, p.x - 40) }, 'you'); const n = B.m.nodes[B.m.nodes.length - 1]; B.sel = [n.id]; bSave(); bDraw(); const i = document.querySelector('[data-bf="name"]'); if (i) { i.focus(); i.select(); }
}
const goDiagram = () => { state.mode = 'map'; state.view = 'dgm'; try { localStorage.setItem('er-map-view', 'dgm'); } catch (_) {} };
document.addEventListener('click', ev => {
  const q = s => ev.target.closest && ev.target.closest(s); let el;
  if ((el = q('[data-dsopen]'))) { goDiagram(); bGoLevel(el.dataset.dsopen); return; }
  if ((el = q('[data-bgo]'))) { goDiagram(); render(); return; }
  const host = $('bld') && $('bld').contains(ev.target) ? $('bld') : $('dgm') && $('dgm').contains(ev.target) ? $('dgm') : null; if (!host) return;
  if ((el = q('[data-bopen]'))) { bGoLevel(el.dataset.bopen); return; }
  if ((el = q('[data-bstart]'))) { const m = Object.assign(bBlankModel(), { origin: { kind: 'blank', name: 'blank canvas' }, chat: [] }); m.base = clone({ layers: m.layers, nodes: [], edges: [], inst: [] }); B.m = m; B.ctx = { kind: 'blank' }; B.ctxKey = 'blank|' + state.snap; B.hist = []; B.fut = []; bSaveNow(); bShell(host); return; }
  if ((el = q('[data-bside]'))) { B.side = el.dataset.bside; if (B.side === 'ai') bInitAI().then(() => bDrawSide()); bDrawSide(); return; }
  if ((el = q('[data-bkind]'))) { B.kind = el.dataset.bkind; bDraw(); return; }
  if ((el = q('[data-bstamp]'))) { bPush(); bStamp(PL_BY[el.dataset.bstamp], null, 'you'); bSave(); bDraw(); return; }
  if ((el = q('[data-bplace]'))) { B.place = bPlaceStart(el.dataset.bplace); B.side = 'pat'; bDraw(); return; }
  if ((el = q('[data-brole]'))) { B.place.cur = el.dataset.brole; bDrawSide(); return; }
  if ((el = q('[data-bfit]'))) { const f = bFits(B.m).find(x => x.id === el.dataset.bfit); if (f) { bPush(); f.apply(B.m, 'you'); B.hl = null; bSave(); bDraw(); } return; }
  if ((el = q('[data-binstdel]'))) { bPush(); B.m.inst = B.m.inst.filter(i => i.id !== el.dataset.binstdel); B.hl = null; bSave(); bDraw(); return; }
  if ((el = q('[data-bsugg]'))) { bAsk(el.dataset.bsugg); return; }
  if ((el = q('[data-bhl]'))) { const key = el.dataset.bhl, s = new Set(); if (key.startsWith('i:')) { const i = B.m.inst.find(x => x.id === key.slice(2)); if (i) Object.values(i.roles).flat().forEach(id => s.add(id)); } else key.split(',').forEach(x => { if (x.includes('>')) { const [a, b] = x.split('>'); s.add(a); s.add(b); s.add(x); } else s.add(x); }); B.m.edges.forEach(e => { if (s.has(e.a) && s.has(e.b)) s.add(e.a + '>' + e.b); }); B.hl = B.hl && B.hlKey === key ? null : s; B.hlKey = B.hl ? key : null; bDraw(); return; }
  if ((el = q('[data-bclose]'))) { B.sel = []; B.selEdge = null; B.selLayer = null; bDraw(); return; }
  if ((el = q('[data-bact]'))) { bAct(el.dataset.bact); return; }
  const svg = $('blSvg'); if (svg && svg.contains(ev.target)) {
    if (B.drag || B.conn || B.place) return; const e = q('[data-bedge]'), l = q('[data-blayer]'), n = q('[data-bnode]'), h = q('[data-binst]');
    if (n) return;
    if (e) { B.selEdge = e.dataset.bedge; B.sel = []; B.selLayer = null; }
    else if (h) { B.side = 'pat'; const i = B.m.inst.find(x => x.id === h.dataset.binst), s = new Set(Object.values(i.roles).flat()); B.m.edges.forEach(x => { if (s.has(x.a) && s.has(x.b)) s.add(x.a + '>' + x.b); }); B.hl = s; B.hlKey = 'i:' + i.id; }
    else if (l && ev.target.tagName === 'text') { B.selLayer = +l.dataset.blayer; B.sel = []; B.selEdge = null; }
    else { B.sel = []; B.selEdge = null; B.selLayer = null; B.hl = null; }
    bDraw();
  }
});
async function bCopy(text, label) { let ok = false; try { await navigator.clipboard.writeText(text); ok = true; } catch (_) {} const n = $('bCopied'); if (!n) return; n.textContent = ok ? `${label} copied to the clipboard.` : 'The browser blocked the clipboard. Copy from the box below.'; if (!ok) n.innerHTML += `<textarea class="bl-out" readonly>${esc(text)}</textarea>`; }
function bAct(a) {
  const m = B.m, n = B.sel.length === 1 ? m.nodes.find(x => x.id === B.sel[0]) : null;
  const done = () => { bSave(); bDraw(); };
  switch (a) {
    case 'part': bPush(); bOp({ op: 'add_part', name: 'New part', layer: B.selLayer ?? (n ? n.layer : 0) }, 'you'); B.sel = [m.nodes[m.nodes.length - 1].id]; done(); { const i = document.querySelector('[data-bf="name"]'); if (i) { i.focus(); i.select(); } } return;
    case 'layer': bPush(); bOp({ op: 'add_layer', name: 'New layer', at: B.selLayer != null ? B.selLayer + 1 : m.layers.length }, 'you'); B.selLayer = B.selLayer != null ? B.selLayer + 1 : m.layers.length - 1; B.sel = []; done(); return;
    case 'undo': return bUndo(); case 'redo': return bRedo();
    case 'reset': if (B.ctx.kind === 'map') { if (!confirm('Drop the design at this level and go back to the code as built?')) return; bPush(); B.m = Object.assign(dsApply(bFromMap(B.ctx.level), null), { origin: m.origin }); B.sel = []; return done(); }
      if (confirm('Throw this canvas away and start again?')) { B.m = null; B.ctxKey = ''; try { localStorage.removeItem(blankKey()); } catch (_) {} renderBuilder($('bld')); } return;
    case 'del': bPush(); m.nodes = m.nodes.filter(x => !B.sel.includes(x.id)); m.edges = m.edges.filter(e => !B.sel.includes(e.a) && !B.sel.includes(e.b)); (m.inst || []).forEach(i => Object.keys(i.roles).forEach(k => { i.roles[k] = i.roles[k].filter(id => !B.sel.includes(id)); })); B.sel = []; return done();
    case 'delEdge': bPush(); m.edges = m.edges.filter(e => e.id !== B.selEdge); B.selEdge = null; return done();
    case 'flip': { bPush(); const e = m.edges.find(x => x.id === B.selEdge); [e.a, e.b] = [e.b, e.a]; e.id = uid('e'); e.by = e.by === 'map' ? 'you' : e.by; B.selEdge = e.id; return done(); }
    case 'connect': { const to = $('bConnTo').value; bPush(); bOp({ op: 'add_edge', from: n.id, to, kind: B.kind }, 'you'); return done(); }
    case 'keep': n.by = n.by === 'ai-edit' ? 'map' : 'you'; return done();
    case 'keepall': m.nodes.forEach(x => { if (x.by === 'ai') x.by = 'you'; if (x.by === 'ai-edit') x.by = 'map'; }); m.edges.forEach(e => { if (e.by === 'ai') e.by = 'you'; }); B.aiUndo = null; return done();
    case 'aiundo': { bRestore(B.aiUndo); bChat().push({ role: 'assistant', content: 'Undone: the design is back to how it was before my last changes.' }); B.aiUndo = null; B.sel = []; if (B.ctx.kind === 'map') dsPersist(); return done(); }
    case 'stop': if (B.ctl) B.ctl.abort(); return;
    case 'clearchat': bChat().splice(0); B.aiUndo = null; if (B.ctx.kind === 'map') dsPersist(); return done();
    case 'placeok': { bPush(); bOp({ op: 'place_pattern', pattern: B.place.pid, roles: B.place.roles }, 'you'); B.place = null; return done(); }
    case 'placecancel': B.place = null; return bDraw();
    case 'lup': case 'ldown': { const i = B.selLayer, j = a === 'lup' ? i - 1 : i + 1; bPush(); [m.layers[i], m.layers[j]] = [m.layers[j], m.layers[i]]; m.nodes.forEach(x => { if (x.layer === i) x.layer = j; else if (x.layer === j) x.layer = i; }); B.selLayer = j; return done(); }
    case 'ldel': { const i = B.selLayer; bPush(); m.layers.splice(i, 1); m.nodes.forEach(x => { if (x.layer > i) x.layer--; }); B.selLayer = null; return done(); }
    case 'copy': return bCopy(`# ${m.origin.name}: architecture changes\n\nStatus: needs-triage\n\n${bSteps(m, bDiff(m)).map((s, i) => `${i + 1}. ${s}`).join('\n')}\n`, 'Steps');
    case 'adr': return bCopy(adrMd(m), 'ADR drafts');
    case 'target': clearTimeout(bSaveT); bSaveNow(); return bCopy(targetJson(), 'The architecture.json target section');
  }
}
document.addEventListener('change', ev => {
  const t = ev.target, inHost = ($('bld') && $('bld').contains(t)) || ($('dgm') && $('dgm').contains(t)); if (!inHost || !B.m) return;
  const m = B.m, n = B.sel.length === 1 ? m.nodes.find(x => x.id === B.sel[0]) : null, e = B.selEdge && m.edges.find(x => x.id === B.selEdge);
  if (t.dataset.bf && n) { bPush(); const f = t.dataset.bf; n[f] = f === 'layer' ? +t.value : t.value; bSave(); bDraw(); }
  if (t.dataset.be && e) { bPush(); e[t.dataset.be] = t.value; bSave(); bDraw(); }
  if (t.dataset.bl && B.selLayer != null) { bPush(); m.layers[B.selLayer].name = t.value; bSave(); bDraw(); }
});
document.addEventListener('input', ev => {
  const t = ev.target;
  if (t.id === 'bq') { B.q = t.value; bDrawSide(); const i = $('bq'); if (i) { i.focus(); i.setSelectionRange(B.q.length, B.q.length); } return; }
  if (t.dataset && t.dataset.bf === 'name' && B.sel.length === 1 && B.m) { const n = B.m.nodes.find(x => x.id === B.sel[0]); if (n) { n.name = t.value; bSave(); const g = document.querySelector(`[data-bnode="${n.id}"] text`); if (g) g.textContent = t.value; } }
});
document.addEventListener('focusin', ev => { if (ev.target.dataset && ev.target.dataset.bf === 'name') bPush(); });
document.addEventListener('focusout', ev => { if (ev.target.dataset && ev.target.dataset.bf === 'name' && B.m) bDraw(); });
document.addEventListener('submit', ev => { if (ev.target.id !== 'bAskF') return; ev.preventDefault(); const t = $('bAsk'); const v = t.value; t.value = ''; bAsk(v); });
const bActive = () => typeof state !== 'undefined' && B.m && ((state.mode === 'pat' && typeof LIB !== 'undefined' && LIB.tab === 'build' && $('bld')) || (state.mode === 'map' && state.view === 'dgm' && $('dgm') && !$('dgm').hidden));
document.addEventListener('keydown', ev => {
  if (!bActive()) return;
  const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement && document.activeElement.tagName);
  if (ev.target.id === 'bAsk' && ev.key === 'Enter' && !ev.shiftKey) { ev.preventDefault(); $('bAskF').requestSubmit(); return; }
  if (typing) return;
  if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'z') { ev.preventDefault(); ev.stopPropagation(); ev.shiftKey ? bRedo() : bUndo(); return; }
  if (ev.key === 'Delete' || ev.key === 'Backspace') { if (B.sel.length) { ev.preventDefault(); bAct('del'); } else if (B.selEdge) { ev.preventDefault(); bAct('delEdge'); } return; }
  if (ev.key === 'Escape') { ev.stopPropagation(); if (B.place) { B.place = null; bDraw(); return; } if (B.sel.length || B.selEdge || B.selLayer != null || B.hl) { B.sel = []; B.selEdge = null; B.selLayer = null; B.hl = null; bDraw(); return; } if (state.mode === 'map' && state.path) bGoLevel(parentOf(state.path)); }
}, true);
new ResizeObserver(() => { if (bActive() && !B.drag) bDrawFast(); }).observe(document.body);

(function () {
  const css = `
#pat.pl-full .bld,#dgm{flex:1;min-height:0;display:flex;flex-direction:column}
.bl-bar{display:flex;align-items:center;gap:8px;flex-wrap:wrap;padding:7px 16px;border-bottom:1px solid var(--line);font-size:12.5px}
.bl-org{color:var(--tx3);margin-right:6px} .bl-org b{color:var(--tx);font-family:var(--mono);font-weight:600}
.bl-pill{font:600 11px var(--mono);padding:3px 8px;border-radius:999px;border:1px solid var(--line2);color:var(--tx2)} .bl-pill.on{border-color:var(--accent);color:var(--accent);background:var(--accent-tint)}
.bl-lbl{color:var(--tx3);font-size:11.5px;margin-left:6px}
.bl-btn{border:1px solid var(--line2);background:var(--bg1);border-radius:7px;padding:4px 10px;cursor:pointer;font-size:12.5px;color:var(--tx2)}
.bl-btn.sm{padding:2px 8px;font-size:11.5px}
.bl-btn:hover:not(:disabled){color:var(--tx);border-color:var(--tx3)} .bl-btn:disabled{opacity:.45;cursor:default}
.bl-btn.danger{color:var(--break);border-color:color-mix(in srgb,var(--break) 40%,transparent)}
.bl-kinds{max-width:100%;min-width:0;overflow-x:auto}
.bl-kinds button{display:inline-flex;align-items:center;gap:6px;font-size:11.5px}
.bl-kd{display:inline-block;width:14px;border-top:2px solid}
.bl-body{flex:1;min-height:0;display:grid;grid-template-columns:minmax(0,1fr) 360px}
.bl-pl{display:flex;flex-direction:column;gap:6px}
.bl-pi{border:1px solid var(--line);background:var(--bg1);border-radius:8px;padding:6px 8px 7px;display:flex;flex-direction:column;gap:4px;color:var(--tx);font-size:12px;font-weight:600}
.bl-pd{opacity:.85} .bl-pi .pd-nt{font-size:9.5px} .bl-pi .pd-e{stroke-width:1.3}
.bl-stage{position:relative;overflow:auto;min-height:0;background:var(--bg)}
#blSvg{display:block;user-select:none;touch-action:none}
.bl-band rect{fill:var(--band);stroke:transparent} .bl-band.sel rect{stroke:var(--line2)}
.bl-band text{font:600 10px var(--sans);letter-spacing:.09em;text-transform:uppercase;fill:var(--tx3);cursor:pointer} .bl-band text.bl-bn{font:500 10px var(--mono);letter-spacing:0;text-transform:none;opacity:.7}
.bl-n{cursor:grab} .bl-n:active{cursor:grabbing}
.bl-n.sel .pd-n{stroke:var(--tx);stroke-width:2}
.bl-n.dz .pd-n{stroke:var(--accent);stroke-width:1.6}
.bl-n.ai .pd-n{stroke:var(--sanc);stroke-dasharray:5 3;stroke-width:1.6}
.bl-n.role .pd-n{stroke:var(--good);stroke-width:2.2}
.bl-n.dim,.bl-e.dim,.bl-hull.dim{opacity:.18}
.bl-open{font:600 15px var(--mono);fill:var(--tx3);cursor:pointer} .bl-open:hover{fill:var(--accent)}
.bl-h{fill:var(--bg2);stroke:var(--tx3);opacity:0;cursor:crosshair;transition:opacity .1s} .bl-n:hover .bl-h,.bl-n.sel .bl-h{opacity:1} .bl-h:hover{fill:var(--accent);stroke:var(--accent)}
.bl-tag rect{fill:var(--bg3)} .bl-tag text{font:700 8.5px var(--mono);fill:var(--tx2)}
.bl-tag.AI rect{fill:var(--sanc)} .bl-tag.AI text{fill:#fff} .bl-tag.new rect{fill:var(--accent)} .bl-tag.new text{fill:var(--on-accent)} .bl-tag.moved rect{fill:var(--side)} .bl-tag.moved text{fill:#fff}
.bl-tag.loop rect,.bl-tag.hub rect{fill:var(--break)} .bl-tag.loop text,.bl-tag.hub text{fill:#fff}
.bl-tag.ai.inline{display:inline-block;background:var(--sanc);color:#fff;font:700 9px var(--mono);padding:1px 4px;border-radius:3px}
.bl-rl{fill:var(--good)} .bl-rt{font:700 9px var(--mono);fill:var(--bg)}
.bl-gone rect{fill:none;stroke:var(--tx3);stroke-dasharray:3 4} .bl-gone text{font:500 11px var(--mono);fill:var(--tx3);text-decoration:line-through}
.bl-hull > rect{fill:color-mix(in srgb,var(--good) 5%,transparent);stroke:var(--good);stroke-dasharray:6 4;stroke-width:1.4;cursor:pointer}
.bl-hull.bad > rect{fill:color-mix(in srgb,var(--break) 5%,transparent);stroke:var(--break)}
.bl-hull .bl-hl{fill:var(--good)} .bl-hull.bad .bl-hl{fill:var(--break)} .bl-hull text{font:700 9.5px var(--mono);fill:var(--bg)}
.ds-hull > rect{fill:none;stroke:var(--accent);stroke-dasharray:6 4;stroke-width:1.5} .ds-hull g rect{fill:var(--accent)} .ds-hull text{font:700 10px var(--mono);fill:var(--on-accent)}
.ds-read{border:1px solid color-mix(in srgb,var(--accent) 45%,transparent);background:var(--accent-tint);border-radius:8px;padding:10px 12px;display:flex;flex-direction:column;gap:4px}
.bl-ep{fill:none;stroke-width:1.7;stroke-linecap:round} .bl-hit{fill:none;stroke:transparent;stroke-width:12;cursor:pointer}
.bl-e.sel .bl-ep{stroke-width:3} .bl-e.ai .bl-ep{filter:drop-shadow(0 0 2px var(--sanc))} .bl-e.dz .bl-ep{stroke-width:2.4}
.bl-tmp{stroke:var(--accent);stroke-width:2;stroke-dasharray:4 3;fill:none;pointer-events:none}
.bl-insp{position:relative;margin:10px 12px 0;background:var(--bg1);border:1px solid var(--line2);border-radius:10px;padding:12px;display:flex;flex-direction:column;gap:9px;font-size:12.5px;flex:none;max-height:52%;overflow:auto}
.bl-insp .x{position:absolute;top:8px;right:8px;border:0;background:none;color:var(--tx3);cursor:pointer;padding:4px 8px;border-radius:5px}
.bl-f{display:flex;flex-direction:column;gap:4px;font-size:11px;color:var(--tx3)}
.bl-f input,.bl-f select,.bl-f textarea,.bl-sel{background:var(--bg2);border:1px solid var(--line2);border-radius:6px;padding:5px 8px;font:12.5px var(--sans);color:var(--tx);min-width:0}
.bl-f textarea{resize:vertical}
.bl-2{display:grid;grid-template-columns:1fr 1fr;gap:8px}
.bl-side{border-left:1px solid var(--line);display:flex;flex-direction:column;min-height:0}
.bl-sseg{margin:10px 12px 0;align-self:flex-start} .bl-sseg button{font-family:var(--sans);font-size:12.5px}
.bl-sbody{flex:1;overflow:auto;padding:12px;display:flex;flex-direction:column;gap:10px;min-height:0}
.bl-ms{display:grid;grid-template-columns:1fr;gap:4px}
.bl-m{display:flex;justify-content:space-between;gap:8px;font-size:12.5px;color:var(--tx2);border-bottom:1px solid var(--line);padding:4px 0}
.bl-m b{font:600 12.5px var(--mono);color:var(--tx)} .bl-m b.good{color:var(--good)} .bl-m b.bad{color:var(--break)}
.bl-lean{font-size:12.5px;color:var(--tx2);border-left:2px solid var(--tx3);padding-left:10px} .bl-lean.earns{border-color:var(--good)} .bl-lean.inventory{border-color:var(--bend)} .bl-lean.worse{border-color:var(--break)}
.bl-lean .bl-v{color:var(--tx)}
.bl-led{font:11.5px var(--mono);color:var(--tx2);margin:0} .bl-led.earns{color:var(--good)} .bl-led.worse{color:var(--break)}
.bl-fit,.bl-inst{border:1px solid var(--line);border-radius:8px;background:var(--bg1);padding:8px 10px;display:flex;flex-direction:column;gap:5px;font-size:12.5px}
.bl-inst.bad{border-color:color-mix(in srgb,var(--break) 40%,transparent)}
.bl-viol{font-size:12px;color:var(--break)}
.bl-det{display:flex;gap:8px;align-items:baseline;font-size:12.5px;flex-wrap:wrap}
.bl-role{display:flex;justify-content:space-between;gap:10px;border:1px solid var(--line2);background:var(--bg1);border-radius:7px;padding:6px 9px;cursor:pointer;text-align:left;font-size:12.5px;color:var(--tx2)}
.bl-role.on{border-color:var(--good);color:var(--tx)} .bl-role .mono{font-size:11.5px;color:var(--tx);overflow-wrap:anywhere;text-align:right}
.bl-rule{font-size:12px;color:var(--tx2)} .bl-rule.ok{color:var(--good)} .bl-rule.bad{color:var(--break)}
.bl-row{border:0;background:none;text-align:left;cursor:pointer;padding:4px 6px;border-radius:5px;font:12px var(--mono);color:var(--tx);overflow-wrap:anywhere}
.bl-row:hover{background:var(--bg1)}
.bl-sbody h3{margin-top:4px}
.bl-out{width:100%;min-height:120px;background:var(--bg);color:var(--tx);border:1px solid var(--line2);border-radius:6px;font:11px var(--mono);margin-top:6px}
.bl-chat{display:flex;flex-direction:column;gap:10px}
.bl-msg{font-size:13px;line-height:1.55;color:var(--tx2);display:flex;flex-direction:column;gap:6px}
.bl-msg p,.bl-msg ul{margin:0} .bl-msg ul{padding-left:18px}
.bl-msg.user{align-self:flex-end;background:var(--bg3);color:var(--tx);border-radius:10px 10px 2px 10px;padding:7px 10px;max-width:90%;white-space:pre-wrap}
.bl-msg.assistant b{color:var(--tx)} .bl-msg code{font:12px var(--mono);color:var(--tx)}
.bl-edits{display:flex;flex-direction:column;gap:1px;border-left:2px solid var(--sanc);padding-left:8px;font:11.5px var(--mono);color:var(--tx3)}
.bl-edits .err{color:var(--break)}
.bl-ask{display:flex;flex-direction:column;gap:6px;margin-top:auto;padding-top:6px}
.bl-ask textarea{background:var(--bg1);border:1px solid var(--line2);border-radius:8px;padding:8px 10px;font:13px var(--sans);color:var(--tx);resize:vertical}
@media (max-width:1100px){.bl-body{grid-template-columns:minmax(0,1fr) 310px}}
@media (max-width:900px){#dgm{display:block}.bl-body{grid-template-columns:minmax(0,1fr);grid-template-rows:60vh auto}.bl-stage{height:60vh}.bl-side{border-left:0;border-top:1px solid var(--line)}.bl-sbody{overflow:visible}}
`;
  const s = document.createElement('style'); s.id = 'bl-css'; s.textContent = css; document.head.appendChild(s);
})();
