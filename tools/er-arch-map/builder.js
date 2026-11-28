/* Builder: a free-form architecture canvas, live structure checks against where you started, and Claude as co-creator.
   The canvas keeps the map's grammar — layers top to bottom, references should point down — so every drawing can be checked.
   Claude edits through the same operations the toolbar uses (apply_edits), so its changes are visible, marked and undoable. */
const BK = { c: 'call', e: 'event', m: 'command', n: 'network', d: 'data' };
const BNK = { part: 'Part', new: 'New part', ext: 'Outside system', store: 'Store', queue: 'Channel / queue' };
const B = { m: null, sel: [], selEdge: null, selLayer: null, hist: [], fut: [], kind: 'c', side: 'checks', ai: null, busy: false, ctl: null, log: [], q: '', hl: null, drag: null, conn: null, lastSnap: null };
const RH = 96, GUT = 118, NH = 34;
const bKey = () => 'er-bld:' + state.snap;
const uid = p => p + Math.random().toString(36).slice(2, 8);
const nW = n => Math.min(210, Math.max(84, n.name.length * 7.1 + 26));

function bLoad() {
  if (B.lastSnap === state.snap && B.m) return;
  B.lastSnap = state.snap; B.m = null; B.sel = []; B.hist = []; B.fut = []; B.log = [];
  try { const t = localStorage.getItem(bKey()); if (t) B.m = JSON.parse(t); } catch (_) {}
}
let bSaveT; function bSave() { clearTimeout(bSaveT); bSaveT = setTimeout(() => { try { localStorage.setItem(bKey(), JSON.stringify(B.m)); } catch (_) {} }, 250); }
function bPush() { B.hist.push(JSON.stringify(B.m)); if (B.hist.length > 120) B.hist.shift(); B.fut = []; }
function bUndo() { if (!B.hist.length) return; B.fut.push(JSON.stringify(B.m)); B.m = JSON.parse(B.hist.pop()); B.sel = []; B.selEdge = null; bSave(); bDraw(); }
function bRedo() { if (!B.fut.length) return; B.hist.push(JSON.stringify(B.m)); B.m = JSON.parse(B.fut.pop()); bSave(); bDraw(); }
const clone = o => JSON.parse(JSON.stringify(o));

/* ───────── starting points ───────── */
function bFromMap(level) {
  const V = byId[level]; if (!V || !V.children) return bBlank();
  const kids = V.children.filter(c => !c.virtual), layers = (V.layers || ['Layer']).map(n => ({ id: uid('l'), name: n }));
  const byRow = {}; kids.forEach(c => { const r = Math.max(0, Math.min(layers.length - 1, c.layer || 0)); (byRow[r] = byRow[r] || []).push(c); });
  const nodes = [];
  Object.entries(byRow).forEach(([r, cs]) => { let x = GUT + 14; cs.sort((a, b) => (b.loc || 0) - (a.loc || 0)).forEach(c => { const n = { id: 'p:' + c.id, name: c.name, layer: +r, x, kind: c.kind === 'external' ? 'ext' : 'part', ref: c.id, by: 'map' }; nodes.push(n); x += nW(n) + 22; }); });
  const v = view(level, null), K = { import: 'c', net: 'n', ipc: 'm', event: 'e', spawn: 'n', fs: 'd' }, edges = [];
  for (const it of v.items.values()) {
    if (it.w <= 0 || it.boundary || it.from.startsWith('g:') || it.to.startsWith('g:')) continue;
    const top = Object.entries(it.kinds || {}).sort((a, b) => b[1] - a[1])[0], k = top ? (K[top[0]] || 'c') : 'c';
    edges.push({ id: uid('e'), a: 'p:' + it.from, b: 'p:' + it.to, kind: k, w: it.w, by: 'map' });
  }
  const m = { origin: { kind: 'map', level, name: fullName(level) || (S.repo ? S.repo.name : 'whole system') }, layers, nodes, edges, steps: [], chat: [] };
  m.base = { layers: clone(layers), nodes: clone(nodes), edges: clone(edges) };
  return m;
}
function bBlank() {
  const layers = ['Front ends', 'Application', 'Domain', 'Adapters', 'Outside'].map(n => ({ id: uid('l'), name: n }));
  const m = { origin: { kind: 'blank', name: 'blank canvas' }, layers, nodes: [], edges: [], steps: [], chat: [] };
  m.base = { layers: clone(layers), nodes: [], edges: [] };
  return m;
}

/* ───────── checks: the map's rules applied to the drawing ───────── */
function bCheck(m) {
  const N = new Map(m.nodes.map(n => [n.id, n])), parts = m.nodes.filter(n => n.kind !== 'ext'), dep = m.edges.filter(e => e.kind !== 'e' && N.has(e.a) && N.has(e.b));
  const up = m.edges.filter(e => e.kind !== 'e' && N.has(e.a) && N.has(e.b) && N.get(e.b).layer < N.get(e.a).layer && N.get(e.b).kind !== 'ext');
  const side = dep.filter(e => N.get(e.b).layer === N.get(e.a).layer).length;
  // loops: strongly connected sets (Tarjan) over dependency edges
  const adj = new Map(m.nodes.map(n => [n.id, []])); dep.forEach(e => adj.get(e.a).push(e.b));
  let idx = 0; const st = [], on = new Set(), ix = new Map(), lo = new Map(), loops = [];
  const sc = v => { ix.set(v, idx); lo.set(v, idx); idx++; st.push(v); on.add(v); for (const w of adj.get(v)) { if (!ix.has(w)) { sc(w); lo.set(v, Math.min(lo.get(v), lo.get(w))); } else if (on.has(w)) lo.set(v, Math.min(lo.get(v), ix.get(w))); } if (lo.get(v) === ix.get(v)) { const c = []; let w; do { w = st.pop(); on.delete(w); c.push(w); } while (w !== v); if (c.length > 1) loops.push(c); } };
  m.nodes.forEach(n => { if (!ix.has(n.id)) sc(n.id); });
  const inLoop = new Set(loops.flat()), loopEdges = dep.filter(e => loops.some(l => l.includes(e.a) && l.includes(e.b)));
  const doors = m.nodes.filter(n => n.kind === 'ext').map(x => ({ id: x.id, from: [...new Set(m.edges.filter(e => e.b === x.id).map(e => e.a))] })).filter(d => d.from.length > 1);
  const hubs = parts.length >= 5 ? parts.filter(n => new Set(dep.filter(e => e.a === n.id).map(e => e.b)).size >= Math.ceil((parts.length - 1) * .6)) : [];
  return { up, side, loops, inLoop, loopEdges, doors, hubs, parts: parts.length, outside: m.nodes.length - parts.length, edges: m.edges.length };
}
function bDiff(m) {
  const bn = new Map(m.base.nodes.map(n => [n.id, n])), cn = new Map(m.nodes.map(n => [n.id, n])), ek = e => e.a + '>' + e.b + ':' + e.kind;
  const be = new Set(m.base.edges.map(ek)), ce = new Set(m.edges.map(ek)), L = i => (m.layers[i] || {}).name || '?', BL = i => (m.base.layers[i] || {}).name || '?';
  return {
    added: m.nodes.filter(n => !bn.has(n.id)), removed: m.base.nodes.filter(n => !cn.has(n.id)),
    renamed: m.nodes.filter(n => bn.has(n.id) && bn.get(n.id).name !== n.name).map(n => [bn.get(n.id).name, n]),
    moved: m.nodes.filter(n => bn.has(n.id) && BL(bn.get(n.id).layer) !== L(n.layer)).map(n => [BL(bn.get(n.id).layer), n]),
    eAdded: m.edges.filter(e => !be.has(ek(e))), eRemoved: m.base.edges.filter(e => !ce.has(ek(e))),
  };
}
const nm = (m, id) => { const n = m.nodes.find(x => x.id === id) || (m.base.nodes || []).find(x => x.id === id); return n ? n.name : id; };

/* ───────── operations: shared by the toolbar, the palette and Claude ───────── */
function bFindNode(ref) { const r = String(ref || '').trim().toLowerCase(); return B.m.nodes.find(n => n.id.toLowerCase() === r) || B.m.nodes.find(n => n.name.toLowerCase() === r) || null; }
function bLayerIx(l) { if (typeof l === 'number' || /^\d+$/.test(String(l))) return Math.max(0, Math.min(B.m.layers.length - 1, +l)); const r = String(l || '').toLowerCase(), i = B.m.layers.findIndex(x => x.name.toLowerCase() === r); return i; }
function bFreeX(layer, w) { const xs = B.m.nodes.filter(n => n.layer === layer).map(n => n.x + nW(n)); return Math.max(GUT + 14, ...xs.map(x => x + 22)); }
function bOp(o, by) {
  const m = B.m, ok = s => ({ ok: true, msg: s }), err = s => { throw new Error(s); };
  switch (o.op) {
    case 'add_layer': { const name = String(o.name || 'New layer'); const at = o.at != null ? bLayerIx(o.at) : m.layers.length; const i = at < 0 ? m.layers.length : at; m.layers.splice(i, 0, { id: uid('l'), name }); m.nodes.forEach(n => { if (n.layer >= i) n.layer++; }); return ok(`Added layer ${name}`); }
    case 'rename_layer': { const i = bLayerIx(o.layer); if (i < 0) err('No such layer: ' + o.layer); const old = m.layers[i].name; m.layers[i].name = String(o.name); return ok(`Renamed layer ${old} to ${o.name}`); }
    case 'add_part': { let li = bLayerIx(o.layer ?? 0); if (li < 0) { m.layers.push({ id: uid('l'), name: String(o.layer) }); li = m.layers.length - 1; } const n = { id: uid('n'), name: String(o.name || 'New part').slice(0, 60), layer: li, x: 0, kind: BNK[o.kind] ? o.kind : 'new', by, note: o.note ? String(o.note).slice(0, 400) : '' }; const near = o.near && bFindNode(o.near); n.x = near && near.layer === li ? near.x + nW(near) + 22 : (o.x != null ? +o.x : bFreeX(li)); if (near && near.layer === li) m.nodes.filter(x => x.layer === li && x.x >= n.x).forEach(x => { x.x += nW(n) + 22; }); m.nodes.push(n); return ok(`Added ${n.name} in ${m.layers[li].name}`); }
    case 'remove_part': { const n = bFindNode(o.id || o.name); if (!n) err('No such part: ' + (o.id || o.name)); m.nodes = m.nodes.filter(x => x !== n); m.edges = m.edges.filter(e => e.a !== n.id && e.b !== n.id); return ok(`Removed ${n.name}`); }
    case 'rename_part': { const n = bFindNode(o.id); if (!n) err('No such part: ' + o.id); const old = n.name; n.name = String(o.name).slice(0, 60); if (by === 'ai') n.by = n.by === 'map' ? 'ai-edit' : n.by; return ok(`Renamed ${old} to ${n.name}`); }
    case 'move_part': { const n = bFindNode(o.id); if (!n) err('No such part: ' + o.id); const li = bLayerIx(o.layer); if (li < 0) err('No such layer: ' + o.layer); n.layer = li; n.x = bFreeX(li); if (by === 'ai' && n.by === 'map') n.by = 'ai-edit'; return ok(`Moved ${n.name} to ${m.layers[li].name}`); }
    case 'set_note': { const n = bFindNode(o.id); if (!n) err('No such part: ' + o.id); n.note = String(o.text || '').slice(0, 400); return ok(`Noted on ${n.name}`); }
    case 'add_edge': { const a = bFindNode(o.from), b = bFindNode(o.to); if (!a || !b) err(`Unknown part in ${o.from} → ${o.to}`); if (a === b) err('An edge needs two different parts'); const kind = BK[o.kind] ? o.kind : ({ call: 'c', event: 'e', command: 'm', network: 'n', data: 'd' })[o.kind] || 'c'; if (m.edges.some(e => e.a === a.id && e.b === b.id && e.kind === kind)) return ok('Already there'); m.edges.push({ id: uid('e'), a: a.id, b: b.id, kind, label: o.label ? String(o.label).slice(0, 40) : '', by }); return ok(`${a.name} → ${b.name} (${BK[kind]})`); }
    case 'remove_edge': { const a = bFindNode(o.from), b = bFindNode(o.to); if (!a || !b) err(`Unknown part in ${o.from} → ${o.to}`); const before = m.edges.length; m.edges = m.edges.filter(e => !(e.a === a.id && e.b === b.id)); if (before === m.edges.length) err(`No edge ${a.name} → ${b.name}`); return ok(`Removed ${a.name} → ${b.name}`); }
    case 'reroute_edge': { const a = bFindNode(o.from), b = bFindNode(o.to), v = bFindNode(o.via); if (!a || !b || !v) err('Unknown part in reroute'); const e = m.edges.find(x => x.a === a.id && x.b === b.id); if (!e) err(`No edge ${a.name} → ${b.name}`); m.edges = m.edges.filter(x => x !== e); bOp({ op: 'add_edge', from: a.id, to: v.id, kind: e.kind }, by); bOp({ op: 'add_edge', from: v.id, to: b.id, kind: e.kind }, by); return ok(`Rerouted ${a.name} → ${b.name} through ${v.name}`); }
    case 'stamp_pattern': { const p = PL_BY[o.pattern] || plFind(o.pattern); if (!p) err('No such pattern: ' + o.pattern); return ok(bStamp(p, o.layer, by)); }
    default: err('Unknown op: ' + o.op);
  }
}
function bApply(ops, by) { const res = []; for (const o of ops) { try { res.push(bOp(o, by).msg); } catch (e) { res.push('Error: ' + e.message); } } bSave(); return res; }
function bStamp(p, layer, by) {
  const m = B.m, K = { n: 'new', o: 'ext', s: 'store', q: 'queue', '': 'part' };
  let start = layer != null ? bLayerIx(layer) : (B.sel.length ? m.nodes.find(n => n.id === B.sel[0])?.layer ?? 0 : 0); if (start < 0) start = 0;
  while (m.layers.length < start + p.dg.rows.length) m.layers.push({ id: uid('l'), name: p.dg.rows[m.layers.length - start] || 'Layer' });
  const base = Math.max(...Array.from({ length: p.dg.rows.length }, (_, r) => bFreeX(start + r))), ids = {}, made = [];
  const rowX = {};
  p.dg.n.slice().sort((a, b) => a[2] - b[2] || a[3] - b[3]).forEach(([id, label, row, x, kind]) => { const n = { id: uid('n'), name: label, layer: start + row, x: 0, kind: K[kind || ''], by: by === 'ai' ? 'ai' : 'you', stamp: p.id }; n.x = Math.max(rowX[row] || base, base + x * 200); rowX[row] = n.x + nW(n) + 18; ids[id] = n.id; m.nodes.push(n); made.push(n.id); });
  p.dg.e.filter(e => e[2] !== 'x').forEach(([a, b, k, lab]) => m.edges.push({ id: uid('e'), a: ids[a], b: ids[b], kind: k, label: lab || '', by: by === 'ai' ? 'ai' : 'you' }));
  B.sel = made; return `Stamped ${p.name} (${made.length} parts) from layer ${m.layers[start].name}`;
}

/* ───────── render ───────── */
function renderBuilder(host) {
  bLoad();
  if (!B.m) { host.innerHTML = bStartCard(); return; }
  host.innerHTML = `<div class="bl-bar">${bToolbar()}</div><div class="bl-body"><aside class="bl-pal" aria-label="Pattern palette">${bPalette()}</aside><div class="bl-stage" id="blStage"><svg id="blSvg" xmlns="http://www.w3.org/2000/svg"></svg></div><aside class="bl-side" aria-label="Checks and Claude"><div class="bl-insp" id="blInsp" hidden></div><div class="seg bl-sseg" role="group"><button type="button" data-bside="checks" aria-pressed="${B.side === 'checks'}">Checks</button><button type="button" data-bside="ai" aria-pressed="${B.side === 'ai'}">Claude</button><button type="button" data-bside="plan" aria-pressed="${B.side === 'plan'}">Change list</button></div><div id="blSideBody" class="bl-sbody"></div></aside></div>`;
  bDraw();
  if (LIB.pendingStamp) { const p = PL_BY[LIB.pendingStamp]; LIB.pendingStamp = null; if (p) { bPush(); bStamp(p, null, 'you'); bSave(); bDraw(); } }
  if (LIB.pendingAsk) { const id = LIB.pendingAsk; LIB.pendingAsk = null; B.side = 'ai'; bDrawSide(); const p = PL_BY[id]; bAsk(id === 'lean' ? 'Read this architecture with the Lean lens. What structure is not earning its keep? Remove or simplify it on the canvas, and say what each change saves.' : `Where would ${p ? p.name : id} fit in this architecture? Draw it on the canvas, connected to the parts it should sit between, and say what it would change.`); }
}
function bStartCard() {
  const lv = sysLevels().map(l => [l, fullName(l) || ((S.repo ? S.repo.name : S.label) + ' · whole system')]);
  return `<div class="arch"><div class="sheet" style="max-width:760px"><header><div class="eyebrow">Builder</div><h1 class="lead">Draw the architecture you want, from where you are</h1><p class="para">Start from a level of the map, with its parts and references as they are today, or from a blank canvas. Every change is checked live against the map's rules, compared with where you started, and can become steps in the Plan. Claude can draw with you.</p></header>
    <div class="pstep"><b>Start from the map</b><p>Parts become boxes in their layers; references become edges, counted. Outside systems come along.</p><div class="row"><select id="bLevel" class="bl-sel">${lv.map(([l, n]) => `<option value="${esc(l)}" ${l === state.path ? 'selected' : ''}>${esc(n)}</option>`).join('')}</select><button type="button" class="openbtn" data-bstart="map">Start from this level</button></div></div>
    <div class="pstep"><b>Start blank</b><p>Five empty layers: front ends, application, domain, adapters, outside.</p><div class="row"><button type="button" class="openbtn" data-bstart="blank">Start blank</button></div></div></div></div>`;
}
function bToolbar() {
  const kinds = Object.entries(BK).map(([k, l]) => `<button type="button" data-bkind="${k}" aria-pressed="${B.kind === k}" title="New edges are ${l}s"><i class="bl-kd" style="border-top-color:${PD_EDGE[k][0]};border-top-style:${PD_EDGE[k][1] ? 'dashed' : 'solid'}"></i>${l}</button>`).join('');
  return `<span class="bl-org" title="Where this canvas started">${B.m.origin.kind === 'map' ? 'From the map · ' : ''}<b>${esc(B.m.origin.name)}</b></span>
    <button type="button" class="bl-btn" data-bact="part" title="Add a part (or double-click a layer)">+ Part</button><button type="button" class="bl-btn" data-bact="layer">+ Layer</button>
    <span class="bl-lbl">New edges</span><div class="seg bl-kinds" role="group" aria-label="Kind of new edges">${kinds}</div>
    <button type="button" class="bl-btn" data-bact="undo" ${B.hist.length ? '' : 'disabled'} title="Undo (Ctrl+Z)">↶ Undo</button><button type="button" class="bl-btn" data-bact="redo" ${B.fut.length ? '' : 'disabled'} title="Redo (Ctrl+Shift+Z)">↷</button>
    <span class="grow"></span><button type="button" class="bl-btn" data-bact="reset" title="Throw this canvas away and choose a new start">New canvas</button>`;
}
function bPalette() {
  const q = B.q.toLowerCase(), ps = PL.filter(p => p.lvl !== 'read' && !p.anti && (!q || (p.name + ' ' + (p.aka || '')).toLowerCase().includes(q)));
  return `<div class="find"><input id="bq" type="search" placeholder="Stamp a pattern…" value="${esc(B.q)}" aria-label="Search patterns to stamp" style="width:100%"></div><p class="note">Click to stamp it on the canvas${B.sel.length ? ', starting at the selected part’s layer' : ''}. Rename the boxes to your parts, then connect them.</p>
    <div class="bl-pl">${ps.map(p => `<button type="button" class="bl-pi" data-bstamp="${p.id}" title="${esc(p.sum)}">${plDiagram({ rows: p.dg.rows, n: p.dg.n, e: p.dg.e.filter(e => e[2] !== 'x') }, { compact: true, w: 300, rh: 34, label: p.name })}<span>${esc(p.name)}</span></button>`).join('')}</div>`;
}
function bDraw() {
  const svg = $('blSvg'); if (!svg || !B.m) return;
  const m = B.m, ck = bCheck(m), N = new Map(m.nodes.map(n => [n.id, n])), upIds = new Set(ck.up.map(e => e.id)), loopIds = new Set(ck.loopEdges.map(e => e.id));
  const stage = $('blStage'), Wv = Math.max(stage ? stage.clientWidth : 900, 600);
  const W = Math.max(Wv, ...m.nodes.map(n => n.x + nW(n) + 40)), H = m.layers.length * RH + 16;
  const k = W > Wv ? Math.max(.75, Wv / W) : 1; svg.setAttribute('viewBox', `0 0 ${W} ${H}`); svg.setAttribute('width', Math.floor(W * k)); svg.setAttribute('height', Math.floor(H * k));
  const pos = id => { const n = N.get(id), w = nW(n), y = 8 + n.layer * RH + (RH - NH) / 2; return { x: n.x, y, w, h: NH, cx: n.x + w / 2, cy: y + NH / 2, row: n.layer }; };
  const bands = m.layers.map((l, i) => `<g class="bl-band ${B.selLayer === i ? 'sel' : ''}" data-blayer="${i}"><rect x="2" y="${8 + i * RH + 3}" width="${W - 4}" height="${RH - 6}" rx="10"/><text x="14" y="${8 + i * RH + 22}">${esc(l.name)}</text><text x="14" y="${8 + i * RH + 38}" class="bl-bn">${m.nodes.filter(n => n.layer === i).length} part${m.nodes.filter(n => n.layer === i).length === 1 ? '' : 's'}</text></g>`).join('');
  const has = new Set(m.edges.map(e => e.a + '>' + e.b));
  const edges = m.edges.filter(e => N.has(e.a) && N.has(e.b)).map(e => {
    const s = pos(e.a), t = pos(e.b), bad = upIds.has(e.id) || loopIds.has(e.id), col = bad ? 'var(--break)' : PD_EDGE[e.kind][0], off = has.has(e.b + '>' + e.a) ? (e.a < e.b ? -8 : 8) : 0;
    let d, lx, ly;
    if (t.row !== s.row) { const down = t.row > s.row, x1 = s.cx + off + (t.cx - s.cx) * .1, x2 = t.cx + off - (t.cx - s.cx) * .1, y1 = down ? s.y + s.h : s.y, y2 = down ? t.y - 4 : t.y + t.h + 4, c = (y2 - y1) * .5; d = `M${x1},${y1} C${x1},${y1 + c} ${x2},${y2 - c} ${x2},${y2}`; lx = (x1 + x2) / 2; ly = (y1 + y2) / 2; }
    else { const sg = Math.sign(t.cx - s.cx) || 1, x1 = s.cx + sg * s.w / 2, x2 = t.cx - sg * (t.w / 2 + 4), y = s.cy + off, adj = Math.abs(x2 - x1) < 60, bow = adj ? 0 : -Math.min(40, 14 + Math.abs(x2 - x1) * .05); d = `M${x1},${y} C${x1 + sg * 24},${y + bow} ${x2 - sg * 24},${y + bow} ${x2},${y}`; lx = (x1 + x2) / 2; ly = y + bow * .75; }
    const label = e.label || (e.w ? String(e.w) : ''), sel = B.selEdge === e.id, ai = e.by === 'ai';
    return `<g class="bl-e ${sel ? 'sel' : ''} ${ai ? 'ai' : ''} ${B.hl && !B.hl.has(e.a + '>' + e.b) ? 'dim' : ''}" data-bedge="${e.id}"><path d="${d}" class="bl-hit"/><path d="${d}" class="bl-ep" style="stroke:${col}${PD_EDGE[e.kind][1] && !bad ? ';stroke-dasharray:' + PD_EDGE[e.kind][1] : ''}" marker-end="url(#pdm-${bad ? 'x' : e.kind})"/>${label ? `<g class="pd-el"><rect x="${lx - label.length * 3.1 - 5}" y="${ly - 8}" width="${label.length * 6.2 + 10}" height="16" rx="4" style="stroke:${col}"/><text x="${lx}" y="${ly + 3.5}" text-anchor="middle" style="fill:${bad ? 'var(--break)' : 'var(--tx)'}">${esc(label)}</text></g>` : ''}</g>`;
  }).join('');
  const nodes = m.nodes.map(n => { const p = pos(n.id), k = { new: 'n', ext: 'o', store: 's', queue: 'q', part: '' }[n.kind] || '', sel = B.sel.includes(n.id), tags = [n.by === 'ai' || n.by === 'ai-edit' ? 'AI' : '', ck.inLoop.has(n.id) ? 'loop' : '', ck.hubs.some(h => h.id === n.id) ? 'hub' : ''].filter(Boolean);
    return `<g class="bl-n ${sel ? 'sel' : ''} ${n.by === 'ai' || n.by === 'ai-edit' ? 'ai' : ''} ${B.hl && !B.hl.has(n.id) ? 'dim' : ''}" data-bnode="${n.id}" transform="translate(${p.x},${p.y})"><title>${esc(n.name + (n.note ? ' — ' + n.note : '') + (n.ref ? '\nmap part: ' + n.ref : ''))}</title><rect width="${p.w}" height="${p.h}" rx="${k === 's' ? 12 : k === 'q' ? 17 : 6}" class="pd-n ${k}"/><text x="${p.w / 2}" y="${p.h / 2 + 4}" text-anchor="middle" class="pd-nt ${k}">${esc(n.name)}</text>${tags.map((t, i) => `<g class="bl-tag ${t}" transform="translate(${p.w - 4 - (i + 1) * 30},-8)"><rect width="28" height="14" rx="3"/><text x="14" y="10.5" text-anchor="middle">${t}</text></g>`).join('')}<circle cx="${p.w}" cy="${p.h / 2}" r="6" class="bl-h" data-bhandle="${n.id}"><title>Drag to another part to connect</title></circle></g>`; }).join('');
  const defs = `<defs>${Object.entries(PD_EDGE).map(([k, v]) => `<marker id="pdm-${k}" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,1 L9,5 L0,9 z" style="fill:${v[0]}"/></marker>`).join('')}</defs>`;
  svg.innerHTML = defs + bands + `<g id="blEdges">${edges}</g><g id="blNodes">${nodes}</g><path id="blTmp" class="bl-tmp" d=""/>`;
  const bar = document.querySelector('.bl-bar'); if (bar) bar.innerHTML = bToolbar();
  bDrawInsp(); bDrawSide(ck);
}
function bDrawInsp() {
  const el = $('blInsp'); if (!el) return; const m = B.m;
  const n = B.sel.length === 1 ? m.nodes.find(x => x.id === B.sel[0]) : null, e = B.selEdge ? m.edges.find(x => x.id === B.selEdge) : null, L = B.selLayer != null ? m.layers[B.selLayer] : null;
  el.hidden = !(n || e || L || B.sel.length > 1);
  if (n) { const p = n.stamp && PL_BY[n.stamp];
    el.innerHTML = `<button type="button" class="x" data-bclose aria-label="Close">✕</button><div class="eyebrow">${n.by === 'map' ? 'Part from the map' : n.by === 'ai' ? 'Added by Claude' : n.by === 'ai-edit' ? 'Changed by Claude' : 'Part'}</div>
      <label class="bl-f">Name<input data-bf="name" value="${esc(n.name)}"></label><div class="bl-2"><label class="bl-f">Kind<select data-bf="kind">${Object.entries(BNK).map(([k, l]) => `<option value="${k}" ${n.kind === k ? 'selected' : ''}>${l}</option>`).join('')}</select></label><label class="bl-f">Layer<select data-bf="layer">${m.layers.map((l, i) => `<option value="${i}" ${n.layer === i ? 'selected' : ''}>${esc(l.name)}</option>`).join('')}</select></label></div>
      <label class="bl-f">Note<textarea data-bf="note" rows="2" placeholder="What it owns, why it is here">${esc(n.note || '')}</textarea></label>
      ${n.ref ? `<p class="note mono">map part: ${esc(n.ref)}</p>` : ''}${p ? `<p class="note">Stamped from ${libLink(p.id, p.name)}</p>` : ''}
      <div class="bl-f">Connect to<div class="row"><select id="bConnTo" class="bl-sel">${m.nodes.filter(x => x !== n).map(x => `<option value="${x.id}">${esc(x.name)}</option>`).join('')}</select><button type="button" class="bl-btn" data-bact="connect">Add ${BK[B.kind]}</button></div></div>
      <div class="row">${n.by === 'ai' || n.by === 'ai-edit' ? '<button type="button" class="bl-btn" data-bact="keep">Keep</button>' : ''}<button type="button" class="bl-btn danger" data-bact="del">Delete part</button></div>`; return; }
  if (B.sel.length > 1) { el.innerHTML = `<button type="button" class="x" data-bclose aria-label="Close">✕</button><div class="eyebrow">${B.sel.length} parts selected</div><p class="note">Drag any of them to move all. Shift-click to add or remove.</p><div class="row"><button type="button" class="bl-btn danger" data-bact="del">Delete ${B.sel.length} parts</button></div>`; return; }
  if (e) { const up = bCheck(m).up.some(x => x.id === e.id);
    el.innerHTML = `<button type="button" class="x" data-bclose aria-label="Close">✕</button><div class="eyebrow">${e.by === 'map' ? 'Reference from the map' : e.by === 'ai' ? 'Added by Claude' : 'Edge'}</div><p class="mono" style="color:var(--tx)">${esc(nm(m, e.a))} → ${esc(nm(m, e.b))}</p>${e.w ? `<p class="note">${e.w} reference${e.w > 1 ? 's' : ''} in the code today.</p>` : ''}${up ? '<p style="color:var(--break);font-size:12.5px">Points up: the lower part depends on a higher one.</p>' : ''}
      <label class="bl-f">Kind<select data-be="kind">${Object.entries(BK).map(([k, l]) => `<option value="${k}" ${e.kind === k ? 'selected' : ''}>${l}</option>`).join('')}</select></label><label class="bl-f">Label<input data-be="label" value="${esc(e.label || '')}" placeholder="${e.w || ''}"></label>
      <div class="row"><button type="button" class="bl-btn" data-bact="flip">Reverse</button><button type="button" class="bl-btn danger" data-bact="delEdge">Delete edge</button></div>`; return; }
  if (L) el.innerHTML = `<button type="button" class="x" data-bclose aria-label="Close">✕</button><div class="eyebrow">Layer ${B.selLayer + 1} of ${m.layers.length}</div><label class="bl-f">Name<input data-bl="name" value="${esc(L.name)}"></label><div class="row"><button type="button" class="bl-btn" data-bact="lup" ${B.selLayer ? '' : 'disabled'}>Move up</button><button type="button" class="bl-btn" data-bact="ldown" ${B.selLayer < m.layers.length - 1 ? '' : 'disabled'}>Move down</button><button type="button" class="bl-btn danger" data-bact="ldel" ${m.nodes.some(n => n.layer === B.selLayer) ? 'disabled title="Move its parts first"' : ''}>Delete</button></div><p class="note">Double-click inside the layer to add a part there.</p>`;
}
function bDrawSide(ck) {
  const el = $('blSideBody'); if (!el) return; const m = B.m; ck = ck || bCheck(m);
  document.querySelectorAll('[data-bside]').forEach(b => b.setAttribute('aria-pressed', b.dataset.bside === B.side));
  if (B.side === 'ai') { el.innerHTML = bChatHtml(); const lg = el.querySelector('.bl-chat'); if (lg) lg.scrollTop = lg.scrollHeight; return; }
  if (B.side === 'plan') { el.innerHTML = bPlanHtml(); return; }
  const b0 = bCheck(Object.assign({}, m, { layers: m.base.layers, nodes: m.base.nodes, edges: m.base.edges })), d = bDiff(m);
  const delta = (l, a, b, good) => { const better = good === 'down' ? b < a : b > a; return `<div class="bl-m"><span>${l}</span><b class="${a === b ? '' : better ? 'good' : 'bad'}">${a === b ? b : a + ' → ' + b}</b></div>`; };
  const lean = d.added.length + d.eAdded.length, removedEx = (b0.up.length - ck.up.length) + (b0.loops.length - ck.loops.length) * 2 + (b0.doors.length - ck.doors.length);
  const list = (t, items, fn) => items.length ? `<h3>${t} <b>${items.length}</b></h3><div class="stack" style="gap:2px">${items.slice(0, 12).map(fn).join('')}</div>` : '';
  el.innerHTML = `<div class="bl-ms">${delta('References pointing up', b0.up.length, ck.up.length, 'down')}${delta('Loops', b0.loops.length, ck.loops.length, 'down')}${delta('Outside systems with several doors', b0.doors.length, ck.doors.length, 'down')}${delta('Hubs', b0.hubs.length, ck.hubs.length, 'down')}${delta('Parts', b0.parts, ck.parts, 'none')}${delta('Edges', b0.edges, ck.edges, 'none')}</div>
    <div class="bl-lean"><b>Lean check.</b> ${lean ? `Adds ${d.added.length} part${d.added.length === 1 ? '' : 's'} and ${d.eAdded.length} edge${d.eAdded.length === 1 ? '' : 's'}; ` : 'Adds nothing; '}${removedEx > 0 ? `removes ${removedEx} structural exception${removedEx === 1 ? '' : 's'}.` : removedEx < 0 ? `adds ${-removedEx} structural exception${removedEx === -1 ? '' : 's'}.` : 'removes no structural exception yet.'} ${lean && removedEx <= 0 ? 'So far this is inventory: what does it buy?' : ''}</div>
    ${list('Pointing up', ck.up, e => `<button type="button" class="bl-row" data-bhl="${e.a}>${e.b}">${esc(nm(m, e.a))} → ${esc(nm(m, e.b))}</button>`)}
    ${list('Loops', ck.loops, l => `<button type="button" class="bl-row" data-bhl="${l.join(',')}">${esc(l.map(id => nm(m, id)).join(' ⇄ '))}</button>`)}
    ${list('Reached from several parts', ck.doors, x => `<button type="button" class="bl-row" data-bhl="${[x.id, ...x.from].join(',')}">${esc(nm(m, x.id))} <span class="note">from ${x.from.length}</span></button>`)}
    ${list('Hubs', ck.hubs, h => `<button type="button" class="bl-row" data-bhl="${h.id}">${esc(h.name)}</button>`)}
    ${!ck.up.length && !ck.loops.length && !ck.doors.length ? '<p class="note" style="color:var(--good)">Every reference points down or sideways, there are no loops, and each outside system has one door.</p>' : ''}
    <p class="note">Same rules as the map: layers run top to bottom; a dependency may point down or sideways; events may go up. Click a row to highlight it.</p>`;
}
function bPlanHtml() {
  const m = B.m, d = bDiff(m), L = i => (m.layers[i] || {}).name, steps = bSteps(m, d);
  return `${steps.length ? `<ol class="steps">${steps.map((s, i) => `<li><i>${i + 1}</i><span>${esc(s)}</span></li>`).join('')}</ol>` : '<p class="note">No changes yet. Draw something: every difference from where you started becomes a step here.</p>'}
    <div class="row" style="margin-top:10px"><button type="button" class="openbtn" data-bact="toplan" ${steps.length ? '' : 'disabled'}>Add to the Plan</button><button type="button" class="openbtn" data-bact="copy" ${steps.length ? '' : 'disabled'}>Copy as markdown</button></div>${m.steps && m.steps.length ? `<p class="note">${m.steps.length} step${m.steps.length > 1 ? 's' : ''} from this canvas are in the Plan tab.</p>` : ''}`;
}
function bSteps(m, d) {
  const L = i => (m.layers[i] || {}).name, out = [];
  d.added.filter(n => n.kind !== 'ext').forEach(n => out.push(`Create ${n.name} in ${L(n.layer)}${n.stamp ? ` (${PL_BY[n.stamp].name})` : ''}${n.note ? ': ' + n.note : ''}.`));
  d.added.filter(n => n.kind === 'ext').forEach(n => out.push(`Start using ${n.name} (outside system).`));
  d.renamed.forEach(([o, n]) => out.push(`Rename ${o} to ${n.name}.`));
  d.moved.forEach(([o, n]) => out.push(`Move ${n.name} from ${o} to ${L(n.layer)}.`));
  d.eAdded.forEach(e => out.push(`Route ${nm(m, e.a)} → ${nm(m, e.b)} as a ${BK[e.kind]}${e.label ? ' (' + e.label + ')' : ''}.`));
  d.eRemoved.forEach(e => out.push(`Remove ${e.w ? 'the ' + e.w + ' reference' + (e.w > 1 ? 's' : '') : 'the dependency'} from ${nm(m, e.a)} to ${nm(m, e.b)}.`));
  d.removed.forEach(n => out.push(`Retire ${n.name}.`));
  return out;
}
/* the Plan tab shows what the builder sent it */
function bldPlanHtml() {
  let m = null; try { const t = localStorage.getItem(bKey()); if (t) m = JSON.parse(t); } catch (_) {}
  if (!m || !m.steps || !m.steps.length) return '';
  return `<section class="phase"><div class="phase-h"><span class="no" style="background:var(--accent);color:var(--on-accent)">B</span><div class="stack" style="gap:2px;max-width:62ch"><h2>From the builder <span class="note" style="font-weight:400">· ${m.steps.length} step${m.steps.length > 1 ? 's' : ''} · ${esc(m.origin.name)}</span></h2><p>Drawn on the Patterns › Builder canvas and added here. They are not simulated on the map.</p></div><div class="after"><button type="button" class="openbtn" data-lib-build>Open the canvas</button></div></div><div class="pstep"><ol class="steps">${m.steps.map((s, i) => `<li><i>${i + 1}</i><span style="color:var(--tx)">${esc(s)}</span></li>`).join('')}</ol></div></section>`;
}

/* ───────── Claude as co-creator ───────── */
let bSample = null, bSampleReady = false;
async function bInitAI() { if (bSampleReady) return bSample; bSampleReady = true; try { bSample = window.claude && window.claude.use ? await window.claude.use('sample') : null; } catch (_) { bSample = null; } return bSample; }
function bChatHtml() {
  const m = B.m, chat = m.chat || [];
  const avail = bSample ? '' : bSampleReady ? '<p class="note">Claude is not available in this view. Open the artifact in Claude to draw with it; everything else works without it.</p>' : '<p class="note">Connecting to Claude…</p>';
  const sugg = ['Where should a gateway go?', 'Remove the loops with the smallest change', 'What would you remove? Use the Lean lens', 'Turn this into ports and adapters'];
  return `<div class="bl-chat">${chat.length ? chat.map(t => `<div class="bl-msg ${t.role}">${t.role === 'user' ? esc(t.content) : bMd(t.content)}${t.edits && t.edits.length ? `<div class="bl-edits">${t.edits.map(x => `<span class="${/^Error/.test(x) ? 'err' : ''}">${esc(x)}</span>`).join('')}</div>` : ''}</div>`).join('') : `<div class="bl-msg assistant"><p>I can draw on this canvas with you: add, move, connect and remove parts, stamp patterns from the library, and explain the trade-offs. Every change I make is marked <span class="bl-tag ai inline">AI</span> and can be undone.</p></div>`}${B.busy ? `<div class="bl-msg assistant"><p class="note">${esc(B.status || 'Thinking…')}</p>${B.live ? bMd(B.live) : ''}</div>` : ''}</div>
    ${avail}${B.aiUndo != null && !B.busy ? `<div class="row" style="margin:6px 0"><button type="button" class="bl-btn" data-bact="aiundo">Undo Claude’s last changes</button><button type="button" class="bl-btn" data-bact="keepall">Keep all</button></div>` : ''}
    ${!chat.length && bSample ? `<div class="kinds" style="margin:6px 0">${sugg.map(s => `<button type="button" class="kind" data-bsugg="${esc(s)}">${esc(s)}</button>`).join('')}</div>` : ''}
    <form class="bl-ask" id="bAskF"><textarea id="bAsk" rows="2" placeholder="${bSample ? 'Ask Claude to change the canvas or explain a choice…' : 'Claude is unavailable here'}" ${bSample && !B.busy ? '' : 'disabled'}></textarea><div class="row">${B.busy ? '<button type="button" class="bl-btn" data-bact="stop">Stop</button>' : `<button type="submit" class="openbtn" ${bSample ? '' : 'disabled'}>Send</button>`}${chat.length && !B.busy ? '<button type="button" class="bl-btn" data-bact="clearchat">Clear chat</button>' : ''}</div></form>`;
}
function bMd(t) { return String(t).split(/\n{2,}/).map(p => /^\s*[-*] /m.test(p) ? `<ul>${p.split('\n').filter(l => l.trim()).map(l => `<li>${bInline(l.replace(/^\s*[-*] /, ''))}</li>`).join('')}</ul>` : `<p>${bInline(p)}</p>`).join(''); }
function bInline(s) { return esc(s).replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>').replace(/`([^`]+)`/g, '<code>$1</code>').replace(/\n/g, '<br>'); }
function bCanvasText(m) {
  const ck = bCheck(m), N = id => (m.nodes.find(n => n.id === id) || {}).name || id;
  return `LAYERS (top to bottom): ${m.layers.map((l, i) => `${i}: ${l.name}`).join(' | ')}
PARTS (id · name · layer · kind${m.origin.kind === 'map' ? ' · from map' : ''}):
${m.nodes.map(n => `- ${n.id} · ${n.name} · ${m.layers[n.layer].name} · ${n.kind}${n.by === 'ai' ? ' · added by you' : ''}${n.note ? ' · note: ' + n.note : ''}`).join('\n') || '(none)'}
EDGES (from → to · kind · references in today's code):
${m.edges.map(e => `- ${N(e.a)} → ${N(e.b)} · ${BK[e.kind]}${e.w ? ' · ' + e.w : ''}${e.label ? ' · "' + e.label + '"' : ''}`).join('\n') || '(none)'}
CHECKS NOW: ${ck.up.length} pointing up${ck.up.length ? ' (' + ck.up.map(e => N(e.a) + '→' + N(e.b)).join(', ') + ')' : ''}; ${ck.loops.length} loops${ck.loops.length ? ' (' + ck.loops.map(l => l.map(N).join('⇄')).join('; ') + ')' : ''}; ${ck.doors.length} outside systems reached from several parts${ck.doors.length ? ' (' + ck.doors.map(d => N(d.id) + ' from ' + d.from.length).join(', ') + ')' : ''}; ${ck.hubs.length} hubs.`;
}
function bRules() {
  const repo = S.repo ? `${S.repo.name}${S.repo.branch ? ' (' + S.repo.branch + ')' : ''}` : S.label || 'a codebase';
  return `You are co-designing a software architecture with an engineer on a shared canvas, inside "er Map", a tool that maps ${repo}.
The canvas started from ${B.m.origin.kind === 'map' ? 'the real code at the level "' + B.m.origin.name + '": parts and counted references as they are today' : 'a blank page'}.

Conventions (the same rules the map checks):
- Layers run top to bottom. A dependency (call, command, network, data) should point down or sideways. Events may point up.
- A loop (parts that reach each other) is a defect. Each outside system should be reached from one part (one door).
- Edge kinds: call, event, command, network, data.

How to work:
- Change the canvas only through apply_edits. Batch all the operations for one idea into a single apply_edits call. Use part ids or exact names.
- Prefer the smallest change that fixes the problem. Reuse existing parts before adding new ones. Name new parts in the codebase's own terms.
- To add a known pattern, use stamp_pattern or add parts yourself and name the pattern; call read_pattern if you need its details.
- Apply the Lean lens (Poppendieck, Lean Software Development, 2003): every part you add is inventory. Say what each addition removes (a loop, an upward reference, duplicated access to an outside system). If nothing, do not add it.
- Say whether each structural decision is a one-way or two-way door.
- After editing, answer in at most 120 words: what you changed, why, and which pattern it is. No headings. Do not restate the whole canvas.
- If the engineer only asks a question, answer it without editing.

PATTERN LIBRARY (id: name — summary):
${PL.filter(p => !p.anti).map(p => `${p.id}: ${p.name} — ${p.sum}`).join('\n')}`;
}
const B_TOOLS = () => [
  { name: 'apply_edits', description: 'Apply a list of edits to the canvas in order and return what happened plus the checks afterwards. Ops: add_part{name,layer,kind?(part|new|ext|store|queue),near?,note?}, remove_part{id}, rename_part{id,name}, move_part{id,layer}, set_note{id,text}, add_edge{from,to,kind(call|event|command|network|data),label?}, remove_edge{from,to}, reroute_edge{from,to,via}, add_layer{name,at?}, rename_layer{layer,name}, stamp_pattern{pattern,layer?}. Layers by name or index; parts by id or exact name.',
    inputSchema: { type: 'object', properties: { ops: { type: 'array', items: { type: 'object' } } }, required: ['ops'] },
    execute: async (input, ctx) => {
      const ops = Array.isArray(input.ops) ? input.ops.slice(0, 40) : []; if (ctx.signal.aborted) throw new Error('stopped');
      const res = bApply(ops.map(o => Object.assign({}, o, { op: String(o.op || '') })), 'ai'); B.turnEdits.push(...res); B.status = res.filter(r => !/^Error/.test(r)).slice(-1)[0] || 'Editing…'; bDraw();
      return { results: res, canvas: bCanvasText(B.m) };
    } },
  { name: 'read_pattern', description: 'Read one library pattern in full: how it works, when to use it, when it is overkill, its costs, its door, its Lean check and its sources. Input: id from the library list.',
    inputSchema: { type: 'object', properties: { id: { type: 'string' } }, required: ['id'] },
    execute: input => { const p = PL_BY[String(input.id)] || plFind(String(input.id)); if (!p) throw new Error('No such pattern'); B.status = 'Reading ' + p.name + '…'; bDrawSide(); return { name: p.name, how: p.how, flow: p.flow, use: p.use, avoid: p.avoid, cost: p.cost, door: p.door + '-way: ' + p.doorWhy, lean: p.lean, related: p.rel, sources: p.src.map(r => PL_SRC[r[0]].a + ', ' + PL_SRC[r[0]].t + ': ' + r[1]) }; } },
];
async function bAsk(text) {
  text = String(text || '').trim(); if (!text || B.busy) return;
  await bInitAI(); if (!bSample) { bDrawSide(); return; }
  const m = B.m; m.chat = m.chat || []; m.chat.push({ role: 'user', content: text });
  B.aiUndo = JSON.stringify(Object.assign({}, m, { chat: undefined })); bPush(); B.turnEdits = []; B.busy = true; B.status = 'Thinking…'; B.live = ''; B.side = 'ai'; bDrawSide();
  const turns = m.chat.slice(-10).map(t => ({ role: t.role, content: t.content }));
  const input = [{ role: 'user', content: bRules() + '\n\nCURRENT CANVAS\n' + bCanvasText(m) + '\n\n(The conversation follows.)' }, ...turns];
  if (input[1] && input[1].role === 'assistant') input.splice(1, 1);
  B.ctl = new AbortController();
  try {
    const r = await bSample(input, { tools: B_TOOLS(), signal: B.ctl.signal, onText: ({ text }) => { B.live = text; const el = document.querySelector('.bl-chat .bl-msg:last-child'); if (el) { el.innerHTML = `<p class="note">${esc(B.status || '')}</p>` + bMd(text); } } });
    m.chat.push({ role: 'assistant', content: r.text.trim() + (r.truncated ? '\n\n(Cut short.)' : ''), edits: B.turnEdits });
  } catch (e) {
    const msg = { cancelled: 'Stopped.', not_granted: 'Claude was not allowed for this page, so the canvas stays yours alone.', rate_limited: 'Too many requests just now. Try again in a moment.', tools_unavailable: 'This view cannot let Claude edit the canvas.', refused: 'Claude declined that request.' }[e && e.code] || 'Something went wrong talking to Claude. Try again.';
    m.chat.push({ role: 'assistant', content: (e && e.text ? e.text.trim() + '\n\n' : '') + msg, edits: B.turnEdits });
    if (e && (e.code === 'not_granted' || e.code === 'sampling_disabled')) bSample = null;
  }
  if (!B.turnEdits.length) { B.aiUndo = null; B.hist.pop(); }
  B.busy = false; B.live = ''; B.ctl = null; bSave(); bDraw();
}

/* ───────── interaction ───────── */
function bPt(ev) { const svg = $('blSvg'), p = svg.createSVGPoint(); p.x = ev.clientX; p.y = ev.clientY; return p.matrixTransform(svg.getScreenCTM().inverse()); }
const rowAt = y => Math.max(0, Math.min(B.m.layers.length - 1, Math.floor((y - 8) / RH)));
document.addEventListener('pointerdown', ev => {
  const svg = $('blSvg'); if (!svg || !svg.contains(ev.target) || ev.button !== 0) return;
  const h = ev.target.closest('[data-bhandle]'), g = ev.target.closest('[data-bnode]');
  if (h) { ev.preventDefault(); B.conn = { from: h.dataset.bhandle }; svg.setPointerCapture(ev.pointerId); return; }
  if (g) {
    ev.preventDefault(); const id = g.dataset.bnode;
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
document.addEventListener('dblclick', ev => {
  const svg = $('blSvg'); if (!svg || !svg.contains(ev.target)) return;
  const g = ev.target.closest('[data-bnode]'); if (g) { B.sel = [g.dataset.bnode]; bDraw(); const i = document.querySelector('[data-bf="name"]'); if (i) { i.focus(); i.select(); } return; }
  const p = bPt(ev); bPush(); bOp({ op: 'add_part', name: 'New part', layer: rowAt(p.y), x: Math.max(GUT + 4, p.x - 40) }, 'you'); const n = B.m.nodes[B.m.nodes.length - 1]; B.sel = [n.id]; bSave(); bDraw(); const i = document.querySelector('[data-bf="name"]'); if (i) { i.focus(); i.select(); }
});
document.addEventListener('click', ev => {
  const host = $('bld'); if (!host || !host.contains(ev.target)) { if (ev.target.closest && ev.target.closest('[data-lib-build]')) { LIB.tab = 'build'; state.mode = 'pat'; render(); } return; }
  const q = s => ev.target.closest(s); let el;
  if ((el = q('[data-bstart]'))) { B.m = el.dataset.bstart === 'map' ? bFromMap($('bLevel').value) : bBlank(); B.lastSnap = state.snap; B.hist = []; B.fut = []; bSave(); renderBuilder(host); return; }
  if ((el = q('[data-bside]'))) { B.side = el.dataset.bside; if (B.side === 'ai') bInitAI().then(() => bDrawSide()); bDrawSide(); return; }
  if ((el = q('[data-bkind]'))) { B.kind = el.dataset.bkind; bDraw(); return; }
  if ((el = q('[data-bstamp]'))) { bPush(); bStamp(PL_BY[el.dataset.bstamp], null, 'you'); bSave(); bDraw(); return; }
  if ((el = q('[data-bsugg]'))) { bAsk(el.dataset.bsugg); return; }
  if ((el = q('[data-bhl]'))) { const ids = el.dataset.bhl.split(','), s = new Set(); ids.forEach(x => { if (x.includes('>')) { const [a, b] = x.split('>'); s.add(a); s.add(b); s.add(x); } else s.add(x); }); if (ids.length > 1 && !ids[0].includes('>')) B.m.edges.forEach(e => { if (s.has(e.a) && s.has(e.b)) s.add(e.a + '>' + e.b); }); B.hl = B.hl && B.hlKey === el.dataset.bhl ? null : s; B.hlKey = B.hl ? el.dataset.bhl : null; bDraw(); return; }
  if ((el = q('[data-bclose]'))) { B.sel = []; B.selEdge = null; B.selLayer = null; bDraw(); return; }
  if ((el = q('[data-bact]'))) { bAct(el.dataset.bact); return; }
  const svg = $('blSvg'); if (svg && svg.contains(ev.target)) {
    if (B.drag || B.conn) return; const e = ev.target.closest('[data-bedge]'), l = ev.target.closest('[data-blayer]'), n = ev.target.closest('[data-bnode]');
    if (n) return; if (e) { B.selEdge = e.dataset.bedge; B.sel = []; B.selLayer = null; } else if (l && ev.target.tagName === 'text') { B.selLayer = +l.dataset.blayer; B.sel = []; B.selEdge = null; } else { B.sel = []; B.selEdge = null; B.selLayer = null; B.hl = null; }
    bDraw();
  }
});
function bAct(a) {
  const m = B.m, n = B.sel.length === 1 ? m.nodes.find(x => x.id === B.sel[0]) : null;
  const done = () => { bSave(); bDraw(); };
  switch (a) {
    case 'part': bPush(); bOp({ op: 'add_part', name: 'New part', layer: B.selLayer ?? (n ? n.layer : 0) }, 'you'); B.sel = [m.nodes[m.nodes.length - 1].id]; done(); { const i = document.querySelector('[data-bf="name"]'); if (i) { i.focus(); i.select(); } } return;
    case 'layer': bPush(); bOp({ op: 'add_layer', name: 'New layer', at: B.selLayer != null ? B.selLayer + 1 : m.layers.length }, 'you'); B.selLayer = B.selLayer != null ? B.selLayer + 1 : m.layers.length - 1; B.sel = []; done(); return;
    case 'undo': return bUndo(); case 'redo': return bRedo();
    case 'reset': if (confirm('Throw this canvas away and start again? Steps already added to the Plan go too.')) { B.m = null; try { localStorage.removeItem(bKey()); } catch (_) {} renderBuilder($('bld')); } return;
    case 'del': bPush(); m.nodes = m.nodes.filter(x => !B.sel.includes(x.id)); m.edges = m.edges.filter(e => !B.sel.includes(e.a) && !B.sel.includes(e.b)); B.sel = []; return done();
    case 'delEdge': bPush(); m.edges = m.edges.filter(e => e.id !== B.selEdge); B.selEdge = null; return done();
    case 'flip': { bPush(); const e = m.edges.find(x => x.id === B.selEdge); [e.a, e.b] = [e.b, e.a]; return done(); }
    case 'connect': { const to = $('bConnTo').value; bPush(); bOp({ op: 'add_edge', from: n.id, to, kind: B.kind }, 'you'); return done(); }
    case 'keep': n.by = 'you'; return done();
    case 'keepall': m.nodes.forEach(x => { if (x.by === 'ai' || x.by === 'ai-edit') x.by = 'you'; }); m.edges.forEach(e => { if (e.by === 'ai') e.by = 'you'; }); B.aiUndo = null; return done();
    case 'aiundo': { const chat = m.chat; B.m = Object.assign(JSON.parse(B.aiUndo), { chat }); B.m.chat.push({ role: 'assistant', content: 'Undone: the canvas is back to how it was before my last changes.' }); B.aiUndo = null; B.sel = []; return done(); }
    case 'stop': if (B.ctl) B.ctl.abort(); return;
    case 'clearchat': m.chat = []; B.aiUndo = null; return done();
    case 'lup': case 'ldown': { const i = B.selLayer, j = a === 'lup' ? i - 1 : i + 1; bPush(); [m.layers[i], m.layers[j]] = [m.layers[j], m.layers[i]]; m.nodes.forEach(x => { if (x.layer === i) x.layer = j; else if (x.layer === j) x.layer = i; }); B.selLayer = j; return done(); }
    case 'ldel': { const i = B.selLayer; bPush(); m.layers.splice(i, 1); m.nodes.forEach(x => { if (x.layer > i) x.layer--; }); B.selLayer = null; return done(); }
    case 'toplan': m.steps = bSteps(m, bDiff(m)); bSave(); try { localStorage.setItem(bKey(), JSON.stringify(m)); } catch (_) {} B.side = 'plan'; bDrawSide(); return;
    case 'copy': { const md = `# ${m.origin.name}: architecture changes\n\nStatus: needs-triage\n\n${bSteps(m, bDiff(m)).map((s, i) => `${i + 1}. ${s}`).join('\n')}\n`; try { navigator.clipboard.writeText(md); } catch (_) {} const b = document.querySelector('[data-bact="copy"]'); if (b) b.textContent = 'Copied'; return; }
  }
}
document.addEventListener('change', ev => {
  const t = ev.target; if (!$('bld') || !$('bld').contains(t)) return;
  const m = B.m, n = B.sel.length === 1 ? m.nodes.find(x => x.id === B.sel[0]) : null, e = B.selEdge && m.edges.find(x => x.id === B.selEdge);
  if (t.dataset.bf && n) { bPush(); const f = t.dataset.bf; n[f] = f === 'layer' ? +t.value : t.value; bSave(); bDraw(); }
  if (t.dataset.be && e) { bPush(); e[t.dataset.be] = t.value; bSave(); bDraw(); }
  if (t.dataset.bl && B.selLayer != null) { bPush(); m.layers[B.selLayer].name = t.value; bSave(); bDraw(); }
});
document.addEventListener('input', ev => {
  const t = ev.target;
  if (t.id === 'bq') { B.q = t.value; const pal = document.querySelector('.bl-pal'); if (pal) { pal.innerHTML = bPalette(); const i = $('bq'); i.focus(); i.setSelectionRange(B.q.length, B.q.length); } return; }
  if (t.dataset && t.dataset.bf === 'name' && B.sel.length === 1) { const n = B.m.nodes.find(x => x.id === B.sel[0]); if (n) { n.name = t.value; bSave(); const g = document.querySelector(`[data-bnode="${n.id}"] text`); if (g) g.textContent = t.value; } }
});
document.addEventListener('focusin', ev => { if (ev.target.dataset && ev.target.dataset.bf === 'name') bPush(); });
document.addEventListener('focusout', ev => { if (ev.target.dataset && ev.target.dataset.bf === 'name') bDraw(); });
document.addEventListener('submit', ev => { if (ev.target.id !== 'bAskF') return; ev.preventDefault(); const t = $('bAsk'); const v = t.value; t.value = ''; bAsk(v); });
document.addEventListener('keydown', ev => {
  if (!$('bld') || state.mode !== 'pat' || LIB.tab !== 'build' || !B.m) return;
  const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement && document.activeElement.tagName);
  if (ev.target.id === 'bAsk' && ev.key === 'Enter' && !ev.shiftKey) { ev.preventDefault(); $('bAskF').requestSubmit(); return; }
  if (typing) return;
  if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'z') { ev.preventDefault(); ev.stopPropagation(); ev.shiftKey ? bRedo() : bUndo(); return; }
  if (ev.key === 'Delete' || ev.key === 'Backspace') { if (B.sel.length) { ev.preventDefault(); bAct('del'); } else if (B.selEdge) { ev.preventDefault(); bAct('delEdge'); } return; }
  if (ev.key === 'Escape') { ev.stopPropagation(); B.sel = []; B.selEdge = null; B.selLayer = null; B.hl = null; bDraw(); }
}, true);
new ResizeObserver(() => { if (typeof state !== 'undefined' && state.mode === 'pat' && LIB.tab === 'build' && B.m && !B.drag) bDrawFast(); }).observe(document.body);

(function () {
  const css = `
#pat.pl-full .bld{flex:1;min-height:0;display:flex;flex-direction:column}
.bl-bar{display:flex;align-items:center;gap:8px;flex-wrap:wrap;padding:7px 16px;border-bottom:1px solid var(--line);font-size:12.5px}
.bl-org{color:var(--tx3);margin-right:6px} .bl-org b{color:var(--tx);font-family:var(--mono);font-weight:600}
.bl-lbl{color:var(--tx3);font-size:11.5px;margin-left:6px}
.bl-btn{border:1px solid var(--line2);background:var(--bg1);border-radius:7px;padding:4px 10px;cursor:pointer;font-size:12.5px;color:var(--tx2)}
.bl-btn:hover:not(:disabled){color:var(--tx);border-color:var(--tx3)} .bl-btn:disabled{opacity:.45;cursor:default}
.bl-btn.danger{color:var(--break);border-color:color-mix(in srgb,var(--break) 40%,transparent)}
.bl-kinds button{display:inline-flex;align-items:center;gap:6px;font-size:11.5px}
.bl-kd{display:inline-block;width:14px;border-top:2px solid}
.bl-body{flex:1;min-height:0;display:grid;grid-template-columns:230px minmax(0,1fr) 330px}
.bl-pal{overflow:auto;border-right:1px solid var(--line);padding:12px;display:flex;flex-direction:column;gap:8px;min-height:0}
.bl-pl{display:flex;flex-direction:column;gap:6px}
.bl-pi{border:1px solid var(--line);background:var(--bg1);border-radius:8px;padding:6px 6px 7px;cursor:pointer;text-align:left;display:flex;flex-direction:column;gap:4px;color:var(--tx)}
.bl-pi:hover{border-color:var(--accent)} .bl-pi span{font-size:12px;font-weight:600;padding-left:2px}
.bl-pi .pd-nt{font-size:9.5px} .bl-pi .pd-e{stroke-width:1.3}
.bl-stage{position:relative;overflow:auto;min-height:0;background:var(--bg)}
#blSvg{display:block;user-select:none;touch-action:none}
.bl-band rect{fill:var(--band);stroke:transparent} .bl-band.sel rect{stroke:var(--line2)}
.bl-band text{font:600 10px var(--sans);letter-spacing:.09em;text-transform:uppercase;fill:var(--tx3);cursor:pointer} .bl-band text.bl-bn{font:500 10px var(--mono);letter-spacing:0;text-transform:none;opacity:.7}
.bl-n{cursor:grab} .bl-n:active{cursor:grabbing}
.bl-n.sel .pd-n{stroke:var(--tx);stroke-width:2}
.bl-n.ai .pd-n{stroke:var(--sanc);stroke-dasharray:5 3;stroke-width:1.6}
.bl-n.dim,.bl-e.dim{opacity:.18}
.bl-h{fill:var(--bg2);stroke:var(--tx3);opacity:0;cursor:crosshair;transition:opacity .1s} .bl-n:hover .bl-h,.bl-n.sel .bl-h{opacity:1} .bl-h:hover{fill:var(--accent);stroke:var(--accent)}
.bl-tag rect{fill:var(--bg3)} .bl-tag text{font:700 8.5px var(--mono);fill:var(--tx2)}
.bl-tag.AI rect{fill:var(--sanc)} .bl-tag.AI text{fill:#fff} .bl-tag.loop rect,.bl-tag.hub rect{fill:var(--break)} .bl-tag.loop text,.bl-tag.hub text{fill:#fff}
.bl-tag.ai.inline{display:inline-block;background:var(--sanc);color:#fff;font:700 9px var(--mono);padding:1px 4px;border-radius:3px}
.bl-ep{fill:none;stroke-width:1.7;stroke-linecap:round} .bl-hit{fill:none;stroke:transparent;stroke-width:12;cursor:pointer}
.bl-e.sel .bl-ep{stroke-width:3} .bl-e.ai .bl-ep{filter:drop-shadow(0 0 2px var(--sanc))}
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
.bl-lean{font-size:12.5px;color:var(--tx2);border-left:2px solid var(--good);padding-left:10px}
.bl-row{border:0;background:none;text-align:left;cursor:pointer;padding:4px 6px;border-radius:5px;font:12px var(--mono);color:var(--tx);overflow-wrap:anywhere}
.bl-row:hover{background:var(--bg1)}
.bl-chat{display:flex;flex-direction:column;gap:10px}
.bl-msg{font-size:13px;line-height:1.55;color:var(--tx2);display:flex;flex-direction:column;gap:6px}
.bl-msg p,.bl-msg ul{margin:0} .bl-msg ul{padding-left:18px}
.bl-msg.user{align-self:flex-end;background:var(--bg3);color:var(--tx);border-radius:10px 10px 2px 10px;padding:7px 10px;max-width:90%;white-space:pre-wrap}
.bl-msg.assistant b{color:var(--tx)} .bl-msg code{font:12px var(--mono);color:var(--tx)}
.bl-edits{display:flex;flex-direction:column;gap:1px;border-left:2px solid var(--sanc);padding-left:8px;font:11.5px var(--mono);color:var(--tx3)}
.bl-edits .err{color:var(--break)}
.bl-ask{display:flex;flex-direction:column;gap:6px;margin-top:auto;padding-top:6px}
.bl-ask textarea{background:var(--bg1);border:1px solid var(--line2);border-radius:8px;padding:8px 10px;font:13px var(--sans);color:var(--tx);resize:vertical}
@media (max-width:1100px){.bl-body{grid-template-columns:180px minmax(0,1fr) 290px}}
@media (max-width:900px){.bl-body{grid-template-columns:minmax(0,1fr);grid-template-rows:auto 60vh auto}.bl-stage{height:60vh}.bl-pal{max-height:150px;border-right:0;border-bottom:1px solid var(--line)}.bl-pl{flex-direction:row;overflow-x:auto}.bl-pi{min-width:150px}.bl-side{border-left:0;border-top:1px solid var(--line);max-height:46vh}}
`;
  const s = document.createElement('style'); s.id = 'bl-css'; s.textContent = css; document.head.appendChild(s);
})();
