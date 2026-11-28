/* Patterns tab: the library (reading pages), the Lean lens, the sources, and the hand-off to the builder.
   Depends on pattern-lib.js and on er-map's globals (state, S, byId, sysLevels, view, patterns, render, esc, $). */
const LIB = { tab: 'lib', id: 'layered', q: '', filter: 'all', found: null, foundKey: '' };
const PL_ALIAS = { 'cyclic dependency': 'acyclic', 'layering': 'layered', 'service and state': 'single-writer', 'no port in between': 'ports-adapters', 'entangled pair': 'acyclic', 'high cohesion': 'bounded-context', 'declared dependencies': 'layered', 'no loops': 'acyclic', 'single core': 'ports-adapters', 'shared base': 'shared-kernel', 'toolbox': 'shared-kernel', 'several things under one name': 'bounded-context' };
function plFind(name) {
  if (!name) return null; const k = String(name).toLowerCase().trim();
  if (PL_BY[k]) return PL_BY[k];
  if (PL_ALIAS[k]) return PL_BY[PL_ALIAS[k]];
  return PL.find(p => p.name.toLowerCase() === k) || PL.find(p => (p.aka || '').toLowerCase().split(' · ').includes(k)) || null;
}
/* a small "about this pattern" link for other pages; '' when the library has no entry */
function libLink(nameOrPid, label) {
  const base = String(nameOrPid || '').split(':')[0], p = (PL_DET[base] && PL_BY[PL_DET[base][0]]) || plFind(nameOrPid);
  return p ? `<button type="button" class="linkbtn" data-lib="${p.id}" style="white-space:nowrap">${esc(label || 'About ' + p.name.toLowerCase().replace(/^cqrs$/, 'CQRS'))} →</button>` : '';
}

/* ───────── diagrams: one renderer for the library and the builder palette ───────── */
const PD_EDGE = { c: ['var(--follow)', '', 'call'], e: ['var(--side)', '5 4', 'event'], m: ['var(--sanc)', '', 'command / message'], n: ['var(--tx2)', '1 4', 'network'], d: ['var(--tx3)', '3 3', 'data / file'], x: ['var(--break)', '4 4', 'what it prevents'] };
function plDiagram(dg, o = {}) {
  const W = o.w || 600, RH = o.rh || 78, top = 8, padL = o.compact ? 10 : 96, padR = 16, H = top + dg.rows.length * RH + 6;
  const pos = {};
  dg.n.forEach(([id, label0, row, x, kind]) => {
    const label = o.compact && label0.length > 11 ? label0.split(/[ ·:(]/)[0].slice(0, 10) : label0;
    const w = Math.min(o.compact ? 76 : 190, Math.max(o.compact ? 40 : 78, label.length * (o.compact ? 6.2 : 6.6) + (o.compact ? 12 : 22))), h = o.compact ? 24 : 32;
    const cx = padL + w / 2 + x * (W - padL - padR - w), cy = top + row * RH + RH / 2;
    pos[id] = { x: cx - w / 2, y: cy - h / 2, w, h, cx, cy, row, kind: kind || '', label };
  });
  const has = new Set(dg.e.map(e => e[0] + '>' + e[1]));
  const bands = dg.rows.map((r, i) => `<rect x="0" y="${top + i * RH + 3}" width="${W}" height="${RH - 6}" rx="8" class="pd-band"/>${o.compact ? '' : `<text x="12" y="${top + i * RH + RH / 2 + 4}" class="pd-bl">${esc(r)}</text>`}`).join('');
  const edges = dg.e.map(([a, b, k, lab]) => {
    const s = pos[a], t = pos[b], st = PD_EDGE[k] || PD_EDGE.c, off = has.has(b + '>' + a) ? (a < b ? -9 : 9) : 0;
    let d, lx, ly;
    if (t.row !== s.row) {
      const down = t.row > s.row, x1 = s.cx + off + (t.cx - s.cx) * .12, x2 = t.cx + off - (t.cx - s.cx) * .12, y1 = down ? s.y + s.h : s.y, y2 = down ? t.y - 4 : t.y + t.h + 4, c = (y2 - y1) * .5;
      if (Math.abs(t.row - s.row) > 1) { const mid = (x1 + x2) / 2, bul = (mid < W / 2 ? -1 : 1) * (Math.max(s.w, t.w) * .5 + 60); d = `M${x1},${y1} C${x1 + bul},${y1 + c * .9} ${x2 + bul},${y2 - c * .9} ${x2},${y2}`; const tt = .32, u = 1 - tt; lx = u * u * u * x1 + 3 * u * u * tt * (x1 + bul) + 3 * u * tt * tt * (x2 + bul) + tt * tt * tt * x2; ly = u * u * u * y1 + 3 * u * u * tt * (y1 + c * .9) + 3 * u * tt * tt * (y2 - c * .9) + tt * tt * tt * y2; }
      else { d = `M${x1},${y1} C${x1},${y1 + c} ${x2},${y2 - c} ${x2},${y2}`; lx = (x1 + x2) / 2; ly = (y1 + y2) / 2; }
    } else {
      const sg = Math.sign(t.cx - s.cx) || 1, x1 = s.cx + sg * s.w / 2, x2 = t.cx - sg * (t.w / 2 + 4), y = s.cy + off, bow = k === 'x' || Math.abs(x2 - x1) > 260 ? -26 : 0;
      d = `M${x1},${y} C${x1 + sg * 20},${y + bow} ${x2 - sg * 20},${y + bow} ${x2},${y}`; lx = (x1 + x2) / 2; ly = y + bow * .75;
    }
    const lb = lab && !o.compact ? `<g class="pd-el"><rect x="${lx - lab.length * 3.05 - 5}" y="${ly - 8}" width="${lab.length * 6.1 + 10}" height="16" rx="4" style="stroke:${st[0]}"/><text x="${lx}" y="${ly + 3.5}" text-anchor="middle" style="fill:${k === 'x' ? 'var(--break)' : 'var(--tx)'}">${esc(lab)}</text></g>` : '';
    return `<path d="${d}" class="pd-e" style="stroke:${st[0]}${st[1] ? ';stroke-dasharray:' + st[1] : ''}" marker-end="url(#pdm-${k})"/>${lb}`;
  }).join('');
  const nodes = Object.values(pos).map(p => {
    const shape = p.kind === 's' ? `<rect x="${p.x}" y="${p.y}" width="${p.w}" height="${p.h}" rx="${p.h / 2.4}" class="pd-n s"/><path d="M${p.x + 6},${p.y + 7} H${p.x + p.w - 6}" class="pd-sl"/>`
      : p.kind === 'q' ? `<rect x="${p.x}" y="${p.y}" width="${p.w}" height="${p.h}" rx="${p.h / 2}" class="pd-n q"/>${[1, 2, 3].map(i => `<path d="M${p.x + p.w - 8 - i * 6},${p.y + 7} V${p.y + p.h - 7}" class="pd-sl"/>`).join('')}`
      : `<rect x="${p.x}" y="${p.y}" width="${p.w}" height="${p.h}" rx="6" class="pd-n ${p.kind}"/>`;
    return `<g>${shape}<text x="${p.kind === 'q' ? p.x + 12 : p.cx}" y="${p.cy + 4}" ${p.kind === 'q' ? '' : 'text-anchor="middle"'} class="pd-nt ${p.kind}">${esc(p.label)}</text></g>`;
  }).join('');
  const defs = `<defs>${Object.entries(PD_EDGE).map(([k, v]) => `<marker id="pdm-${k}" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,1 L9,5 L0,9 z" style="fill:${v[0]}"/></marker>`).join('')}</defs>`;
  return `<svg class="pd" viewBox="0 0 ${W} ${H}" role="img" aria-label="${esc(o.label || 'Pattern diagram')}">${defs}${bands}${edges}${nodes}</svg>`;
}
function plLegend(dg) {
  const kinds = [...new Set(dg.e.map(e => e[1] && e[2]))], nk = new Set(dg.n.map(n => n[4] || ''));
  return `<div class="pd-lg">${nk.has('n') ? '<span><i class="pd-sw n"></i>what the pattern adds</span>' : ''}${nk.has('o') ? '<span><i class="pd-sw o"></i>outside system</span>' : ''}${nk.has('s') ? '<span><i class="pd-sw s"></i>store</span>' : ''}${nk.has('q') ? '<span><i class="pd-sw q"></i>channel / queue</span>' : ''}${kinds.map(k => `<span><i class="pd-ln" style="border-top-color:${PD_EDGE[k][0]};border-top-style:${PD_EDGE[k][1] ? 'dashed' : 'solid'}"></i>${PD_EDGE[k][2]}</span>`).join('')}</div>`;
}

/* ───────── which library entries the map has found in this repo ───────── */
function plFound() {
  const key = state.snap + '|' + state.func;
  if (LIB.foundKey === key && LIB.found) return LIB.found;
  const out = {}, save = [VW, PT, state.path];
  try {
    for (const l of sysLevels()) {
      const v = view(l, null), list = patterns(v).list;
      for (const p of list) {
        const base = p.id.split(':')[0];
        for (const lid of (PL_DET[base] || [])) {
          if (lid === 'big-ball-of-mud' && base === 'tangle' && (p.nodes || []).length < 4) continue;
          (out[lid] = out[lid] || []).push({ level: l, pid: p.id, st: p.st, name: p.name, ev: p.ev });
        }
      }
    }
  } catch (e) { console.warn('pattern scan', e); }
  [VW, PT, state.path] = save;
  LIB.found = out; LIB.foundKey = key; return out;
}
const PL_STL = { holds: 'Holds', partly: 'Partly', against: 'Against the grain', neutral: 'Present' };

/* ───────── rendering ───────── */
function renderPatterns() {
  const el = $('pat'); if (!el) return;
  const tabs = [['lib', 'Library'], ['lean', 'Lean lens'], ['build', 'Builder'], ['src', 'Sources']];
  const head = `<div class="pl-top"><div class="seg" role="group" aria-label="Patterns section">${tabs.map(t => `<button type="button" data-pltab="${t[0]}" aria-pressed="${LIB.tab === t[0]}">${t[1]}</button>`).join('')}</div><span class="note">${LIB.tab === 'build' ? 'Draw the architecture you want. Claude can draw with you.' : `${PL.length} patterns · checked against ${esc(S.label || 'this repo')} ${esc(S.ver || '')}`}</span></div>`;
  if (LIB.tab === 'build') { el.innerHTML = head + '<div id="bld" class="bld"></div>'; el.classList.add('pl-full'); if (typeof renderBuilder === 'function') renderBuilder($('bld')); return; }
  el.classList.remove('pl-full');
  el.innerHTML = head + (LIB.tab === 'lean' ? leanPage() : LIB.tab === 'src' ? srcPage() : libPage());
  const sel = el.querySelector('.pl-item[aria-current="true"]'); if (sel && sel.scrollIntoViewIfNeeded) sel.scrollIntoViewIfNeeded(false);
}
function libPage() {
  const found = plFound(), q = LIB.q.toLowerCase();
  const match = p => (!q || (p.name + ' ' + (p.aka || '') + ' ' + p.sum).toLowerCase().includes(q)) && (LIB.filter === 'all' || (LIB.filter === 'found' && found[p.id]) || (LIB.filter === 'read' && p.lvl === 'read') || (LIB.filter === 'one' && p.door === 'one'));
  const fCount = PL.filter(p => found[p.id]).length;
  const list = PL_FAM.map(([fid, fname]) => { const ps = PL.filter(p => p.fam === fid && match(p)); if (!ps.length) return ''; return `<div class="pl-fam"><div class="pl-fh">${esc(fname)} <span>${ps.length}</span></div>${ps.map(p => { const f = found[p.id], st = f ? worstSt(f) : ''; return `<button type="button" class="pl-item" data-lib="${p.id}" aria-current="${LIB.id === p.id}"><span class="pl-in">${esc(p.name)}</span><span class="pl-im">${f ? `<i class="pl-dot s-${st}" title="Found here: ${PL_STL[st]}"></i>` : ''}${p.anti ? '<span class="pl-chip anti">anti</span>' : ''}${p.lvl === 'read' ? '<span class="pl-chip">read</span>' : ''}</span></button>`; }).join('')}</div>`; }).join('');
  const chip = (k, l) => `<button type="button" class="kind" data-plfilter="${k}" aria-pressed="${LIB.filter === k}">${l}</button>`;
  return `<div class="pl-lib"><aside class="pl-list" aria-label="Pattern list"><div class="find"><input id="plq" type="search" placeholder="Search patterns…" value="${esc(LIB.q)}" aria-label="Search patterns" style="width:100%"></div><div class="kinds">${chip('all', 'All <b>' + PL.length + '</b>')}${chip('found', 'Found here <b>' + fCount + '</b>')}${chip('one', 'One-way doors')}${chip('read', 'Read to understand')}</div>${list || '<p class="empty">No pattern matches.</p>'}</aside><article class="pl-doc" id="pldoc">${docOf(PL_BY[LIB.id] || PL[0], found)}</article></div>`;
}
function worstSt(f) { const r = { against: 3, partly: 2, neutral: 1, holds: 0 }; return f.reduce((a, x) => (r[x.st] || 0) > (r[a] || 0) ? x.st : a, f[0].st); }
function srcLine(r) { const s = PL_SRC[r[0]]; return `<li><span class="pl-st">${esc(s.a)}${s.y ? ' (' + s.y + ')' : ''}, <i>${esc(s.t)}</i>${s.p ? ', ' + esc(s.p) : ''}.</span> <span class="pl-sw">${esc(r[1])}</span>${r[2] ? ` <a href="${esc(r[2])}" target="_blank" rel="noopener noreferrer">${esc(r[2].replace(/^https?:\/\/(www\.)?/, '').replace(/\/$/, ''))} ↗</a>` : ''}</li>`; }
function docOf(p, found) {
  const f = found[p.id] || [], fam = PL_FAM.find(x => x[0] === p.fam);
  const sec = (t, body, cls) => body && body !== '—' ? `<section class="pl-sec ${cls || ''}"><h3>${t}</h3><p>${esc(body)}</p></section>` : '';
  const chips = ids => (ids || []).map(id => PL_BY[id]).filter(Boolean).map(x => `<button type="button" class="kind" data-lib="${x.id}">${esc(x.name)}</button>`).join('');
  const here = f.length ? `<section class="pl-sec wide"><h3>In ${esc(S.repo ? S.repo.name : S.label || 'this repo')}</h3><div class="stack">${f.slice(0, 8).map(x => `<div class="pl-here"><span class="stt s-${x.st}">${PL_STL[x.st] || x.st}</span><span><b>${esc(x.name)}</b> <span class="note mono">· ${esc(fullName(x.level) || 'whole system')}</span><br>${esc(x.ev || '')}</span><button type="button" class="linkbtn" data-archjump="${esc(x.level)}" data-pid="${esc(x.pid)}">Show on map</button></div>`).join('')}${f.length > 8 ? `<p class="note">and ${f.length - 8} more levels</p>` : ''}</div></section>`
    : p.det ? `<section class="pl-sec wide"><h3>In ${esc(S.repo ? S.repo.name : S.label || 'this repo')}</h3><p class="note">The map checks for this at every level and did not find it.</p></section>` : `<section class="pl-sec wide"><h3>In ${esc(S.repo ? S.repo.name : S.label || 'this repo')}</h3><p class="note">The map cannot see this pattern from references alone${p.have && p.have !== '—' ? ': ' + esc(p.have.charAt(0).toLowerCase() + p.have.slice(1)) : '.'}</p></section>`;
  return `<header class="pl-h"><div class="eyebrow">${esc(fam ? fam[1] : '')}${p.lvl === 'read' ? ' · read to understand' : ''}</div><h1>${esc(p.name)}</h1>${p.aka ? `<p class="note">${esc(p.aka)}</p>` : ''}
      <div class="row" style="margin-top:6px">${p.anti ? '<span class="stt s-against">Anti-pattern</span>' : ''}<span class="stt s-${p.door === 'one' ? 'against' : 'holds'}" title="${esc(p.doorWhy || '')}">${p.door === 'one' ? 'One-way door' : 'Two-way door'}</span>${f.length ? `<span class="stt s-${worstSt(f)}">Found here · ${PL_STL[worstSt(f)]}</span>` : ''}</div>
      <p class="pl-sum">${esc(p.sum)}</p></header>
    <figure class="pl-fig">${plDiagram(p.dg, { label: p.name })}${plLegend(p.dg)}</figure>
    <div class="row pl-acts"><button type="button" class="openbtn" data-plstamp="${p.id}">Stamp it in the builder</button><button type="button" class="openbtn" data-plask="${p.id}">Ask Claude where it fits</button></div>
    <div class="pl-grid">${sec('How it works', p.how, 'wide')}${sec('How information flows', p.flow)}${sec(p.door === 'one' ? 'One-way door' : 'Two-way door', p.doorWhy)}${sec('Use it when', p.use)}${sec(p.anti ? 'Get out of it' : 'Overkill when', p.avoid)}${sec('What it costs', p.cost)}${sec('Lean check', p.lean, 'lean')}${sec('You may need it when', p.need)}${sec('How the map sees it', p.have)}${here}
    ${(p.rel || []).length || (p.vs || []).length ? `<section class="pl-sec wide"><h3>Related</h3><div class="kinds">${chips(p.rel)}</div>${(p.vs || []).length ? `<h3 style="margin-top:10px">Alternatives and tensions</h3><div class="kinds">${chips(p.vs)}</div>` : ''}</section>` : ''}
    <section class="pl-sec wide"><h3>Sources</h3><ul class="pl-src">${p.src.map(srcLine).join('')}</ul></section></div>`;
}
function leanPage() {
  const L = PL_LEAN, found = plFound();
  const chips = ids => ids.map(id => PL_BY[id]).filter(Boolean).map(x => `<button type="button" class="kind" data-lib="${x.id}">${esc(x.name)}${found[x.id] ? ' <b>·</b>' : ''}</button>`).join('');
  const against = Object.entries(found).filter(([, f]) => f.some(x => x.st === 'against')).map(([id]) => PL_BY[id]);
  return `<div class="arch"><div class="sheet"><header><div class="eyebrow">Lean lens</div><h1 class="lead">Does each structure earn its keep?</h1><p class="para">${esc(L.intro)}</p></header>
    <section><h2>Four questions for any pattern</h2><ol class="steps">${L.questions.map((q, i) => `<li><i>${i + 1}</i><span style="color:var(--tx)">${esc(q)}</span></li>`).join('')}</ol></section>
    <section><h2>The seven principles, read as architecture</h2><p class="sub note">From Mary and Tom Poppendieck, <i>Lean Software Development: An Agile Toolkit</i> (Addison-Wesley, 2003).</p><div class="pl-lean">${L.principles.map((x, i) => `<div class="pstep"><div class="t"><span class="n">${i + 1}</span><b>${esc(x[0])}</b></div><p style="color:var(--tx)">${esc(x[1])}</p><p><span class="eyebrow">Look for</span><br>${esc(x[2])}</p><div class="kinds">${chips(x[3])}</div></div>`).join('')}</div></section>
    <section><h2>In ${esc(S.repo ? S.repo.name : S.label || 'this repo')}</h2><p class="sub">${against.length ? `The map finds ${against.length} pattern${against.length > 1 ? 's' : ''} working against the grain: ${against.map(p => `<button type="button" class="linkbtn" data-lib="${p.id}">${esc(p.name)}</button>`).join(', ')}. Each is either a structure to finish or one to remove. The Recommend tab lists what to remove.` : 'No detected pattern works against the grain here.'}</p><div class="row"><button type="button" class="openbtn" data-mode="rec">Open Recommend</button><button type="button" class="openbtn" data-plask="lean">Ask Claude what to remove</button></div></section>
    <section><h2>From Martin Fowler</h2><div class="two">${L.fowler.map(x => `<div class="flowcard"><b>${esc(x[0])}</b><p>${esc(x[1])}</p><a href="${x[2]}" target="_blank" rel="noopener noreferrer" class="note">${esc(x[2].replace('https://', ''))} ↗</a></div>`).join('')}</div></section></div></div>`;
}
function srcPage() {
  const by = {}; PL.forEach(p => p.src.forEach(r => { (by[r[0]] = by[r[0]] || new Set()).add(p.id); }));
  by.lean = new Set(PL_LEAN.principles.flatMap(x => x[3]));
  return `<div class="arch"><div class="sheet"><header><div class="eyebrow">Sources</div><h1 class="lead">Where each pattern comes from</h1><p class="para">Every entry cites the book or article that names it. Text is paraphrased; follow the links for the original. Links were checked when the library was written. Books without a free page are cited without a link.</p></header>
    <div class="tblwrap"><table><thead><tr><th>Source</th><th>Patterns it backs here</th></tr></thead><tbody>${Object.entries(PL_SRC).filter(([k]) => by[k]).map(([k, s]) => `<tr><td style="min-width:260px"><b>${esc(s.t)}</b><br><span class="note">${esc(s.a)}${s.y ? ', ' + s.y : ''}${s.p ? ' · ' + esc(s.p) : ''}</span>${s.u ? `<br><a href="${s.u}" target="_blank" rel="noopener noreferrer" class="note">${esc(s.u.replace(/^https?:\/\/(www\.)?/, ''))} ↗</a>` : ''}</td><td><div class="kinds">${[...by[k]].map(id => PL_BY[id]).filter(Boolean).map(p => `<button type="button" class="kind" data-lib="${p.id}">${esc(p.name)}</button>`).join('')}</div></td></tr>`).join('')}</tbody></table></div></div></div>`;
}

/* ───────── events ───────── */
document.addEventListener('click', ev => {
  const q = s => ev.target.closest(s); let el;
  if ((el = q('[data-lib]'))) { ev.stopPropagation(); LIB.id = el.dataset.lib; LIB.tab = 'lib'; if (state.mode !== 'pat') { state.mode = 'pat'; render(); } else renderPatterns(); const d = $('pldoc'); if (d) d.scrollTop = 0; return; }
  if ((el = q('[data-pltab]'))) { LIB.tab = el.dataset.pltab; renderPatterns(); return; }
  if ((el = q('[data-plfilter]'))) { LIB.filter = el.dataset.plfilter; renderPatterns(); return; }
  if ((el = q('[data-plstamp]'))) { LIB.tab = 'build'; LIB.pendingStamp = el.dataset.plstamp; renderPatterns(); return; }
  if ((el = q('[data-plask]'))) { LIB.tab = 'build'; LIB.pendingAsk = el.dataset.plask; renderPatterns(); return; }
}, true);
document.addEventListener('input', ev => { if (ev.target.id === 'plq') { LIB.q = ev.target.value; const pos = ev.target.selectionStart; renderPatterns(); const i = $('plq'); if (i) { i.focus(); i.setSelectionRange(pos, pos); } } });

/* ───────── styles ───────── */
(function () {
  const css = `
#pat{min-height:0;display:flex;flex-direction:column;overflow:hidden}
#pat .arch{flex:1}
.pl-top{display:flex;align-items:center;gap:14px;flex-wrap:wrap;padding:8px max(16px,env(safe-area-inset-right,0px)) 8px max(16px,env(safe-area-inset-left,0px));border-bottom:1px solid var(--line)}
.pl-top .seg button{font-family:var(--sans);font-size:13px}
.pl-lib{flex:1;min-height:0;display:grid;grid-template-columns:280px minmax(0,1fr)}
.pl-list{overflow:auto;border-right:1px solid var(--line);padding:14px 12px 30px;display:flex;flex-direction:column;gap:12px;min-height:0}
.pl-fam{display:flex;flex-direction:column;gap:1px}
.pl-fh{font-size:10.5px;letter-spacing:.09em;text-transform:uppercase;color:var(--tx3);font-weight:600;padding:4px 8px;display:flex;justify-content:space-between}
.pl-fh span{font-family:var(--mono);letter-spacing:0}
.pl-item{display:flex;justify-content:space-between;align-items:center;gap:8px;border:0;background:none;text-align:left;cursor:pointer;padding:5px 8px;border-radius:6px;color:var(--tx2)}
.pl-item:hover{background:var(--bg1);color:var(--tx)}
.pl-item[aria-current="true"]{background:var(--bg3);color:var(--tx)}
.pl-in{font-size:13px}
.pl-im{display:inline-flex;gap:5px;align-items:center;flex:none}
.pl-dot{width:7px;height:7px;border-radius:50%;background:currentColor;display:inline-block}
.pl-chip{font:600 9.5px/1 var(--sans);letter-spacing:.05em;text-transform:uppercase;color:var(--tx3);border:1px solid var(--line2);border-radius:4px;padding:2px 4px}
.pl-chip.anti{color:var(--break);border-color:color-mix(in srgb,var(--break) 45%,transparent)}
.pl-doc{overflow:auto;min-height:0;padding:24px max(24px,env(safe-area-inset-right,0px)) 60px 28px;display:flex;flex-direction:column;gap:18px}
.pl-doc > *{max-width:980px}
.pl-h h1{font:600 26px/1.15 var(--sans);letter-spacing:-.02em;margin-top:4px}
.pl-sum{font-size:15.5px;line-height:1.55;color:var(--tx);margin-top:10px;max-width:70ch}
.pl-fig{margin:0;border:1px solid var(--line);border-radius:12px;background:var(--bg1);padding:10px 12px;display:flex;flex-direction:column;gap:8px}
.pd{width:100%;height:auto;display:block}
.pl-fig .pd{max-width:680px}
.pd-band{fill:var(--band)}
.pd-bl{font:600 9.5px var(--sans);letter-spacing:.09em;text-transform:uppercase;fill:var(--tx3)}
.pd-n{fill:var(--bg2);stroke:var(--line2);stroke-width:1}
.pd-n.n{fill:var(--accent-tint);stroke:var(--accent);stroke-width:1.6}
.pd-n.o{fill:var(--bg);stroke-dasharray:3 3}
.pd-n.s,.pd-n.q{fill:var(--bg1)}
.pd-sl{stroke:var(--line2);fill:none}
.pd-nt{font:600 11.5px var(--mono);fill:var(--tx)}
.pd-nt.o{fill:var(--tx2);font-weight:500}
.pd-e{fill:none;stroke-width:1.6;stroke-linecap:round}
.pd-el rect{fill:var(--bg1);stroke-width:1} .pd-el text{font:600 9.5px var(--mono)}
.pd-lg{display:flex;gap:14px;flex-wrap:wrap;font-size:11.5px;color:var(--tx3)}
.pd-lg span{display:inline-flex;align-items:center;gap:6px}
.pd-ln{display:inline-block;width:18px;border-top:2px solid}
.pd-sw{display:inline-block;width:12px;height:10px;border-radius:3px;border:1px solid var(--line2);background:var(--bg2)}
.pd-sw.n{background:var(--accent-tint);border-color:var(--accent)} .pd-sw.o{background:var(--bg);border-style:dashed} .pd-sw.s{border-radius:5px} .pd-sw.q{border-radius:6px}
.pl-acts{gap:8px}
.pl-grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:16px 28px}
.pl-sec{display:flex;flex-direction:column;gap:5px}
.pl-sec.wide{grid-column:1 / -1}
.pl-sec p{font-size:13.5px;line-height:1.6;max-width:72ch}
.pl-sec.lean{border-left:2px solid var(--good);padding-left:10px}
.pl-here{display:grid;grid-template-columns:auto minmax(0,1fr) auto;gap:10px;align-items:start;font-size:12.5px;color:var(--tx2);border:1px solid var(--line);border-radius:8px;padding:8px 10px;background:var(--bg1)}
.pl-src{margin:0;padding-left:18px;display:flex;flex-direction:column;gap:6px;font-size:12.5px;color:var(--tx2)}
.pl-src a{color:var(--accent);text-decoration:none;font-family:var(--mono);font-size:11.5px;overflow-wrap:anywhere}
.pl-src a:hover{text-decoration:underline}
.pl-sw{color:var(--tx)}
.pl-lean{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,320px),1fr));gap:12px}
.sheet a{color:var(--accent);text-decoration:none} .sheet a:hover{text-decoration:underline}
@media (max-width:900px){#pat{overflow:visible}.pl-lib{display:block}.pl-list{max-height:34vh;border-right:0;border-bottom:1px solid var(--line)}.pl-grid{grid-template-columns:1fr}.pl-doc{padding:18px 16px 50px;overflow:visible}}
`;
  const s = document.createElement('style'); s.id = 'pl-css'; s.textContent = css; document.head.appendChild(s);
})();
