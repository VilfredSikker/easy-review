#!/usr/bin/env node
// er-arch-extract — reads a git repository and writes the dependency facts that
// er Map imports. No dependencies; needs git and Node 18+ or Bun (bun er-arch-extract.mjs …).
//
//   node er-arch-extract.mjs [repo-path] [-o out.json] [--model architecture.json] [--base <ref>] [--timings]
//
// --base main records what changed since the merge base with main (committed and uncommitted),
// including which dependencies the change adds or removes. Without it, uncommitted changes are
// recorded against HEAD.
//
// Writes <name>.er-arch.json (import this into er Map). When the repo has no
// architecture.json, it also writes <name>.architecture.draft.json: the parts and
// layers it inferred, ready to edit, copy into the repo as architecture.json, and
// rerun with.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const VERSION = '2026.11.28';
const SELF = (() => { try { return createHash('sha256').update(fs.readFileSync(fileURLToPath(import.meta.url))).digest('hex').slice(0, 10); } catch { return 'unknown'; } })();
const FEATURES = ['imports', 'processes', 'network', 'files', 'service-sdks', 'tauri-commands', 'tauri-events', 'tests-inline', 'generated-headers', 'entry-points', 'undeclared-modules', 'name-mentions', 'auto-tooling', 'model-validation', 'change', 'script-entries', 'import-meta-glob'];

const argv = process.argv.slice(2);
const opt = (name) => { const i = argv.indexOf(name); if (i < 0) return null; const v = argv[i + 1]; argv.splice(i, 2); return v; };
const TIMINGS = argv.includes('--timings') || !!process.env.ER_TIMINGS; if (argv.includes('--timings')) argv.splice(argv.indexOf('--timings'), 1);
const outArg = opt('-o') || opt('--out'), modelArg = opt('--model'), baseArg = opt('--base');
const __times = []; let __t = performance.now(), __sec = 'start';
const mark = (name) => { const t = performance.now(); __times.push([__sec, t - __t]); __t = t; __sec = name; };
const repoArg = argv[0] || '.';
const git = (...a) => execFileSync('git', ['-C', repoArg, ...a], { encoding: 'utf8', maxBuffer: 1 << 30 }).trim();
const tryGit = (...a) => { try { return git(...a); } catch { return ''; } };

let ROOT;
try { ROOT = git('rev-parse', '--show-toplevel'); } catch { console.error(`Not a git repository: ${path.resolve(repoArg)}`); process.exit(1); }
const NAME = path.basename(ROOT);
const COMMIT = tryGit('rev-parse', 'HEAD') || 'uncommitted';
const BRANCH = tryGit('rev-parse', '--abbrev-ref', 'HEAD') || '';
const DIRTY = tryGit('status', '--porcelain').length > 0;
const t0 = Date.now();

mark("files");
// ── files ───────────────────────────────────────────────────────────────────
const CODE = /\.(rs|ts|tsx|mts|cts|js|jsx|mjs|cjs|svelte|vue|py|go)$/;
const SKIP = /(^|\/)(node_modules|vendor|dist|build|out|target|\.next|\.nuxt|\.svelte-kit|\.output|coverage|__pycache__|\.venv|venv|env|site-packages|\.git)(\/|$)|\.min\.(js|mjs)$|\.bundle\.js$/;
const listed = git('ls-files', '-z', '--cached', '--others', '--exclude-standard').split('\0').filter(Boolean);
const all = listed.filter((p) => fs.existsSync(path.join(ROOT, p)));
const rels = all.filter((p) => CODE.test(p) && !SKIP.test(p));
const fileSet = new Map(rels.map((p, i) => [p, i]));
const langOf = (p) => /\.rs$/.test(p) ? 'rust' : /\.py$/.test(p) ? 'python' : /\.go$/.test(p) ? 'go' : 'js';
const F = rels.map((rel) => {
  let text = '';
  try { const st = fs.statSync(path.join(ROOT, rel)); if (st.size < 1_500_000) text = fs.readFileSync(path.join(ROOT, rel), 'utf8'); } catch {}
  return { rel, lang: langOf(rel), text, lines: text ? text.split('\n').length : 0 };
});

mark("model");
// ── model file ──────────────────────────────────────────────────────────────
const stripJsonc = (s) => { let o = '', q = null; for (let i = 0; i < s.length; i++) { const c = s[i], d = s[i + 1]; if (q) { o += c; if (c === '\\') { o += d; i++; } else if (c === q) q = null; continue; } if (c === '"' || c === "'") { q = c; o += c; continue; } if (c === '/' && d === '/') { while (i < s.length && s[i] !== '\n') i++; o += '\n'; continue; } if (c === '/' && d === '*') { i += 2; while (i < s.length && !(s[i] === '*' && s[i + 1] === '/')) i++; i++; continue; } o += c; } return o.replace(/,(\s*[}\]])/g, '$1'); };
const readJson = (p) => { try { return JSON.parse(stripJsonc(fs.readFileSync(p, 'utf8'))); } catch { return null; } };
const modelPath = modelArg ? path.resolve(modelArg) : ['architecture.json', '.architecture.json'].map((f) => path.join(ROOT, f)).find((p) => fs.existsSync(p));
const MODEL = modelPath ? readJson(modelPath) : null;
if (modelPath && !MODEL) { console.error(`Could not parse ${modelPath}. It must be JSON (comments allowed).`); process.exit(1); }
const warnings = [];

const globRe = (g, exact) => { let s = ''; for (let i = 0; i < g.length; i++) { const c = g[i]; if (c === '*') { if (g[i + 1] === '*') { if (g[i + 2] === '/') { s += '(?:.*/)?'; i += 2; } else { s += '.*'; i++; } } else s += '[^/]*'; } else if (c === '?') s += '[^/]'; else s += c.replace(/[.+^${}()|[\]\\]/g, '\\$&'); } return new RegExp('^' + s + (exact ? '$' : '(/.*)?$')); };
// {a,b} alternatives, expanded before matching (globRe treats braces as literal text)
const expandBraces = (g) => { const m = g.match(/\{([^{}]*)\}/); return m ? m[1].split(',').flatMap((x) => expandBraces(g.slice(0, m.index) + x + g.slice(m.index + m[0].length))) : [g]; };
const anyGlob = (globs) => { const rs = (globs || []).map(globRe); return (p) => rs.some((r) => r.test(p)); };

mark("non-functional");
// ── non-functional code ─────────────────────────────────────────────────────
const NF = MODEL && MODEL.nonFunctional ? MODEL.nonFunctional : {};
const userNf = { test: anyGlob(NF.test), generated: anyGlob(NF.generated), dev: anyGlob(NF.dev), tooling: anyGlob(NF.tooling) };
const RX = {
  test: /(^|\/)(tests?|__tests__|spec|specs|e2e|__mocks__|fixtures?|testdata|test[-_]utils|test[-_]support|testing)(\/|$)|\.(test|spec)\.[a-z]+$|[-.]fixtures\.[\w.]+$|-test-utils\.[\w.]+$|\.types\.svelte$|_test\.(go|py)$|(^|\/)test_[^/]*\.py$|(^|\/)conftest\.py$|_tests?\.rs$|(^|\/)tests?\.rs$|golden/,
  generated: /(^|\/)(generated|__generated__|gen|autogen)(\/|$)|\.(gen|generated|pb)\.[a-z]+$|_pb2(_grpc)?\.py$|\.pb\.go$|\.d\.ts$/,
  dev: /\.stories\.[a-z]+$|-story-data\.[\w.]+$|(^|\/)(stories|storybook|\.storybook|playgrounds?|examples?|demos?|benches|benchmarks?|sandbox)(\/|$)/,
  tooling: /(^|\/)\.github(\/|$)|(^|\/)[\w.-]+\.config\.(js|ts|mjs|cjs)$|(^|\/)(gulpfile|gruntfile|webpack\.[\w.]+|rollup\.[\w.]+|babel\.config)\.(js|ts|mjs|cjs)$|(^|\/)(setup|noxfile|fabfile|tasks)\.py$|(^|\/)build\.rs$/,
};
// A scripts/ or tools/ folder is build tooling, except under src/, where the name is the code's own
// word (an MCP server's tools/, say) and production code imports it.
const TOOL_DIRS = new Set(['scripts', 'tools', 'tooling', 'ci']);
const toolingDir = (rel) => { const s = rel.split('/'); for (let k = 0; k < s.length - 1; k++) { if (s[k] === 'src') return false; if (TOOL_DIRS.has(s[k])) return true; } return false; };
const category = (f) => {
  for (const k of ['test', 'generated', 'dev', 'tooling']) if (userNf[k](f.rel)) return k;
  if (/^\s*(\/\/|#|\/\*)[^\n]*(@generated|Code generated|DO NOT EDIT|this file (?:was|is) (?:auto-?)?generated|auto-?generated (?:file|code|by))/im.test(f.text.slice(0, 600))) return 'generated';
  for (const k of ['test', 'generated', 'dev', 'tooling']) if (RX[k].test(f.rel)) return k;
  return toolingDir(f.rel) ? 'tooling' : '';
};
F.forEach((f) => {
  f.cat = category(f);
  f.t0 = f.cat ? 0 : -1;
  if (!f.cat && f.lang === 'rust') { let pend = false; const ls = f.text.split('\n'); for (let i = 0; i < ls.length; i++) { const ln = ls[i]; if (/^#\[cfg\(test\)\]/.test(ln)) pend = true; else if (pend && /^(pub(\([a-z]+\))?\s+)?mod\s+\w+/.test(ln)) { f.t0 = i; break; } else if (pend && ln.trim() && !/^#\[/.test(ln)) pend = false; } }
  f.nf = f.t0 < 0 ? 0 : f.lines - f.t0;
});

mark("packages");
// ── packages and manifests ──────────────────────────────────────────────────
const dirOf = (p) => { const d = path.posix.dirname(p); return d === '.' ? '' : d; };
const manifests = all.filter((p) => !SKIP.test(p) && /(^|\/)(Cargo\.toml|package\.json|go\.mod|pyproject\.toml|setup\.py)$/.test(p));
const read = (p) => { try { return fs.readFileSync(path.join(ROOT, p), 'utf8'); } catch { return ''; } };
const crates = new Map(); // snake name → { dir, src }
const npm = new Map(); // package name → { dir, entry }
const gomods = []; // { mod, dir }
const pkgDirs = new Map(); // dir → display name
let tauriCrate = null;
for (const m of manifests) {
  const dir = dirOf(m), base = path.posix.basename(m), t = read(m);
  if (base === 'Cargo.toml') { const sec = t.split(/^\[/m).find((s) => s.startsWith('package]')); const nm = sec && (sec.match(/^\s*name\s*=\s*"([^"]+)"/m) || [])[1]; if (nm) { crates.set(nm.replace(/-/g, '_'), { dir, src: (dir ? dir + '/' : '') + 'src' }); pkgDirs.set(dir, nm); if (/^\s*tauri\s*=/m.test(t)) tauriCrate = nm.replace(/-/g, '_'); } }
  else if (base === 'package.json') { const j = readJson(path.join(ROOT, m)); if (j && j.name) { const ex = j.exports && (typeof j.exports === 'string' ? j.exports : typeof j.exports['.'] === 'string' ? j.exports['.'] : null); npm.set(j.name, { dir, entry: j.source || j.module || ex || j.main || '' }); if (!j.workspaces || dir) pkgDirs.set(dir, j.name); } }
  else if (base === 'go.mod') { const mm = t.match(/^module\s+(\S+)/m); if (mm) { gomods.push({ mod: mm[1], dir }); pkgDirs.set(dir, mm[1].split('/').pop()); } }
  else { const nm = (t.match(/^\s*name\s*=\s*["']([^"']+)/m) || [])[1]; pkgDirs.set(dir, nm || path.posix.basename(dir) || NAME); }
}

mark("build");
// ── files a package.json script names: [file, script name, run by a runner]. Every one is an entry point.
// The build tooling step below takes those a runner (node, bun, …) executes from a build, generate, seed or
// similar step; a file handed to a bundler or linter (`tsup src/index.ts`) is the product, not tooling.
const TOOL_STEPS = new Set(['gen', 'codegen', 'generate', 'build', 'lint', 'format', 'fmt', 'check', 'typecheck', 'migrate', 'seed', 'release', 'deploy', 'prepare', 'postinstall']);
const RUNNERS = new Set(['node', 'tsx', 'ts-node', 'bun', 'deno', 'python', 'python3', 'vite-node', 'esno', 'jiti']);
const scriptRuns = [];
for (const m of manifests.filter((x) => /package\.json$/.test(x))) {
  const j = readJson(path.join(ROOT, m)) || {}, d = dirOf(m);
  for (const [name, cmd] of Object.entries(j.scripts || {})) for (const seg of String(cmd).split(/&&|\|\||[;|]/)) {
    let runner = false, ran = false;
    for (const w of seg.split(/\s+/)) {
      const tok = w.replace(/^['"]|['"]$/g, ''); if (RUNNERS.has(tok)) runner = true; if (!CODE.test(tok)) continue;
      const rel = path.posix.normalize(path.posix.join(d, tok)); if (!fileSet.has(rel)) continue;
      scriptRuns.push([rel, name, runner && !ran]); if (runner) ran = true;
    }
  }
}
// ── build tooling by convention: code generators in a library crate, xtask crates, and files that
// package.json runs from a build, lint, codegen, migration or release script
let AUTO_TOOLING = 0;
{
  const auto = new Set();
  for (const [, c] of crates) {
    const hasLib = fileSet.has(c.src + '/lib.rs');
    for (const f of F) { if (hasLib && f.rel.startsWith(c.src + '/bin/') && /gen|codegen|export|schema|bindings|xtask/i.test(path.posix.basename(f.rel))) auto.add(f.rel); if (/(^|\/)xtask$/.test(c.dir) && f.rel.startsWith(c.dir + '/')) auto.add(f.rel); }
  }
  for (const [rel, name, run] of scriptRuns) if (run && name.split(/[^a-z0-9]+/i).some((w) => TOOL_STEPS.has(w.toLowerCase()))) auto.add(rel);
  F.forEach((f) => { if (!f.cat && auto.has(f.rel)) { f.cat = 'tooling'; f.nf = f.lines; f.t0 = 0; AUTO_TOOLING++; } });
}

mark("parts");
// ── parts ───────────────────────────────────────────────────────────────────
const slug = (s) => s.replace(/^\(|\)$/g, '').replace(/[^\w.-]+/g, '·').replace(/^·|·$/g, '') || 'root';
let partOf, partsMeta = [], partSource;
if (MODEL && Array.isArray(MODEL.parts) && MODEL.parts.length) {
  partSource = 'declared';
  const ms = MODEL.parts.map((p) => ({ id: slug(p.id || p.name), name: p.name || p.id, layer: p.layer, group: p.group ? slug(p.group) : '', desc: p.description || '', kind: p.kind || '', m: anyGlob(p.paths || p.globs || []), paths: p.paths || [] }));
  partOf = (rel) => { const hit = ms.find((p) => p.m(rel)); return hit ? hit.id : 'unassigned'; };
  partsMeta = ms.map(({ id, name, layer, group, desc, kind, paths }) => ({ id, name, layer, group, desc, kind, paths }));
  ms.forEach((p) => { if (!rels.some((r) => p.m(r))) warnings.push(`Part "${p.name}" matches no files: ${JSON.stringify(p.paths)}`); });
} else {
  partSource = 'inferred';
  const dirs = [...pkgDirs.keys()].filter((d) => d !== '' || pkgDirs.size === 1).sort((a, b) => b.length - a.length);
  const inPkg = (rel) => dirs.find((d) => d === '' || rel === d || rel.startsWith(d + '/'));
  const code = F.filter((f) => !f.cat);
  const lines = (pred) => code.filter((f) => pred(f.rel)).reduce((s, f) => s + f.lines, 0) || 0;
  const total = lines(() => true) || 1;
  if (dirs.filter((d) => d !== '').length >= 2) {
    partOf = (rel) => { const d = inPkg(rel); if (d != null && d !== '') return d; const s = rel.split('/'); return s.length > 1 ? s[0] : '(root files)'; };
  } else {
    const roots = ['src', 'lib', 'app', 'pkg', 'internal', 'cmd', 'source', 'packages', 'apps', 'services'].filter((r) => lines((p) => p.startsWith(r + '/')) / total >= 0.12);
    partOf = (rel) => { const r = roots.find((x) => rel.startsWith(x + '/')); if (r) { const rest = rel.slice(r.length + 1).split('/'); return rest.length > 1 ? `${r}/${rest[0]}` : `${r} (files)`; } const s = rel.split('/'); return s.length > 1 ? s[0] : '(root files)'; };
  }
  // split a part that holds most of the code, one level at a time
  for (let round = 0; round < 3; round++) {
    const sizes = new Map(); code.forEach((f) => { const p = partOf(f.rel); sizes.set(p, (sizes.get(p) || 0) + f.lines); });
    const [big, n] = [...sizes.entries()].sort((a, b) => b[1] - a[1])[0] || [];
    if (!big || n / total < 0.6 || big.endsWith('(files)') || big.startsWith('(')) break;
    const sub = new Set(code.filter((f) => partOf(f.rel) === big).map((f) => f.rel.slice(big.length + 1).split('/')).filter((s) => s.length > 1).map((s) => s[0]));
    if (sub.size < 2) break;
    const prev = partOf; partOf = (rel) => { const p = prev(rel); if (p !== big) return p; const rest = rel.slice(big.length + 1).split('/'); return rest.length > 1 ? `${big}/${rest[0]}` : `${big} (files)`; };
  }
  const ids = new Set(F.map((f) => partOf(f.rel)));
  // cap the number of parts
  if (ids.size > 28) { const sizes = new Map(); F.forEach((f) => { const p = partOf(f.rel); sizes.set(p, (sizes.get(p) || 0) + f.lines); }); const keep = new Set([...sizes.entries()].sort((a, b) => b[1] - a[1]).slice(0, 27).map((x) => x[0])); const prev = partOf; partOf = (rel) => { const p = prev(rel); return keep.has(p) ? p : '(small folders)'; }; }
  const raw = [...new Set(F.map((f) => partOf(f.rel)))];
  partsMeta = raw.map((p) => ({ id: slug(p), name: pkgDirs.get(p) || p, paths: [p.startsWith('(') ? '' : p.replace(/ \(files\)$/, '/*') + (p.endsWith('(files)') ? '' : '/**')] }));
  const prev = partOf; partOf = (rel) => slug(prev(rel));
}
F.forEach((f) => { f.part = partOf(f.rel); });
if (partSource === 'declared') { const un = F.filter((f) => f.part === 'unassigned' && !f.cat).length; if (un) { warnings.push(`${un} code files match no part in architecture.json; they are grouped as "unassigned".`); partsMeta.push({ id: 'unassigned', name: 'unassigned', desc: 'Files that match no part in architecture.json.' }); } }

mark("model");
// ── model validation: names in architecture.json that point at nothing
if (MODEL) {
  const gidOf = (g) => slug(g.id || g.name), gids = new Set((MODEL.groups || []).map(gidOf)), pids = new Set(partsMeta.map((p) => p.id)), known = (x) => pids.has(slug(x)) || gids.has(slug(x)) || /^x-/.test(String(x));
  const rootL = Array.isArray(MODEL.layers) ? MODEL.layers : [], gLayers = new Map((MODEL.groups || []).map((g) => [gidOf(g), g.layers || []]));
  const layersFor = (grp) => (grp && gLayers.has(slug(grp)) ? gLayers.get(slug(grp)) : rootL);
  for (const g of (MODEL.groups || [])) { if (g.group && !gids.has(slug(g.group))) warnings.push(`Group "${g.id || g.name}" sits in group "${g.group}", which is not declared.`); const ls = layersFor(g.group); if (g.layer && ls.length && !ls.includes(g.layer)) warnings.push(`Group "${g.id || g.name}" is in layer "${g.layer}", which ${g.group ? `group "${g.group}"` : 'the model'} does not declare.`); }
  for (const p of (MODEL.parts || [])) { if (p.group && !gids.has(slug(p.group))) warnings.push(`Part "${p.id || p.name}" sits in group "${p.group}", which is not declared.`); const ls = layersFor(p.group); if (p.layer && ls.length && !ls.includes(p.layer)) warnings.push(`Part "${p.id || p.name}" is in layer "${p.layer}", which ${p.group ? `group "${p.group}"` : 'the model'} does not declare.`); }
  const r = MODEL.rules || {}, chk = (x, where) => { if (x && !known(x)) warnings.push(`${where} names "${x}", which is not a part or group.`); };
  for (const [k, v] of Object.entries(r.allowed || {})) { chk(k, 'rules.allowed'); (v || []).forEach((x) => chk(x, `rules.allowed.${k}`)); }
  (r.independent || []).forEach((x) => chk(x, 'rules.independent'));
  (r.exceptions || []).forEach((x, n) => { chk(x.from, `rules.exceptions[${n}].from`); chk(x.to, `rules.exceptions[${n}].to`); });
  for (const [k, v] of Object.entries(r.ports || {})) { chk(v, `rules.ports["${k}"]`); if (!/^(network|disk|ipc|process:.+|service:.+)$/.test(k)) warnings.push(`rules.ports key "${k}" should be network, disk, ipc, process:<command> or service:<name>.`); }
  (MODEL.flows || []).forEach((fl) => (fl.steps || []).forEach((s) => { chk(s[0], `flow "${fl.name}"`); chk(s[1], `flow "${fl.name}"`); }));
}

mark("resolution");
// ── resolution indexes ──────────────────────────────────────────────────────
// Rust: crate → module path → file
const crateOfFile = (rel) => { let best = null; for (const [nm, c] of crates) if (rel.startsWith(c.src + '/') && (!best || c.src.length > crates.get(best).src.length)) best = nm; return best; };
const rsMod = {};
F.forEach((f, i) => { if (f.lang !== 'rust') return; const c = crateOfFile(f.rel); if (!c) return; const segs = f.rel.slice(crates.get(c).src.length + 1).replace(/\.rs$/, '').split('/'); if (['mod', 'lib', 'main'].includes(segs[segs.length - 1])) segs.pop(); f.crate = c; f.mod = segs; (rsMod[c] = rsMod[c] || {})[segs.join('::')] = i; });
// Comments always go; string contents go when blankStrings is set. Newlines and
// offsets are kept, so line numbers still point at the source.
function codeOnly(t, lang, blankStrings) {
  const out = [], n = t.length, blank = (s) => s.replace(/[^\n]/g, ' ');
  let i = 0, run = 0; // run: start of the plain text not yet copied
  const flush = () => { if (run < i) out.push(t.slice(run, i)); };
  while (i < n) {
    const c = t.charCodeAt(i);
    // fast path: anything that cannot start a comment or a string
    if (c !== 47 && c !== 35 && c !== 34 && c !== 39 && c !== 96 && c !== 114) { i++; continue; }
    const ch = t[i], d = t[i + 1];
    if (lang !== 'python' && ch === '/' && d === '/') { flush(); let j = t.indexOf('\n', i); if (j < 0) j = n; out.push(' '.repeat(j - i)); i = run = j; continue; }
    if (lang !== 'python' && ch === '/' && d === '*') { flush(); let j = t.indexOf('*/', i + 2); j = j < 0 ? n : j + 2; out.push(blank(t.slice(i, j))); i = run = j; continue; }
    if (lang === 'python' && ch === '#') { flush(); let j = t.indexOf('\n', i); if (j < 0) j = n; out.push(' '.repeat(j - i)); i = run = j; continue; }
    if (lang === 'rust' && ch === 'r' && /^r#*"/.test(t.slice(i, i + 12)) && !/\w/.test(t[i - 1] || '')) {
      flush(); const hs = t.slice(i + 1).match(/^#*/)[0], end = '"' + hs, j0 = i + 2 + hs.length;
      let j = t.indexOf(end, j0); j = j < 0 ? n : j + end.length;
      out.push(blankStrings ? t.slice(i, j0) + blank(t.slice(j0, Math.max(j0, j - end.length))) + t.slice(Math.max(j0, j - end.length), j) : t.slice(i, j)); i = run = j; continue;
    }
    const rustChar = lang === 'rust' && ch === "'" && /^'(\\.|[^\\'\n])'/.test(t.slice(i, i + 4));
    if (ch === '"' || rustChar || (lang !== 'rust' && (ch === "'" || ch === '`'))) {
      flush(); const triple = lang === 'python' && t.startsWith(ch.repeat(3), i), q = triple ? ch.repeat(3) : ch;
      let j = i + q.length;
      while (j < n) { if (t[j] === '\\') { j += 2; continue; } if (t.startsWith(q, j)) { j += q.length; break; } if (!triple && ch !== '`' && lang !== 'rust' && t[j] === '\n') break; j++; }
      const a = i + q.length, b = Math.max(a, j - q.length);
      out.push(blankStrings ? t.slice(i, a) + blank(t.slice(a, b)) + t.slice(b, j) : t.slice(i, j)); i = run = j; continue;
    }
    i++;
  }
  flush();
  return out.join('');
}
F.forEach((f) => { f.code = f.lang === 'rust' || f.lang === 'python' ? codeOnly(f.text, f.lang, true) : codeOnly(f.text, f.lang, false); });
// Rust inline modules: which `mod x { }` each line sits in, and whether that block is test code
function innerMods(f) {
  if (f.lang !== 'rust') return;
  const C = f.code, mods = [[]], test = [0]; let depth = 0, line = 0; const stack = [];
  for (let i = 0; i < C.length; i++) {
    const c = C[i];
    if (c === '\n') { line++; mods[line] = stack.map((s) => s.name); test[line] = stack.some((s) => s.test) ? 1 : 0; continue; }
    if (c === '{') { depth++; const pre = C.slice(Math.max(0, i - 160), i), m = pre.match(/\bmod\s+([A-Za-z_]\w*)\s*$/); if (m) { const attr = f.text.slice(Math.max(0, i - 160), i); stack.push({ name: m[1], depth, test: m[1] === 'tests' || /#\[cfg\(test\)\]\s*(pub(\([^)]*\))?\s*)?mod\s+\w+\s*$/.test(attr) }); } }
    else if (c === '}') { if (stack.length && stack[stack.length - 1].depth === depth) stack.pop(); depth--; }
  }
  f.modsAt = mods; f.testAt = test;
  if (!f.cat) f.nf = test.reduce((s, x) => s + x, 0);
}
F.forEach(innerMods);
const isTestLine = (f, line) => (f.cat ? true : f.testAt ? !!f.testAt[line - 1] : false);

const rsReexp = {}; // crate → module key → name → raw path
F.forEach((f) => { if (f.lang !== 'rust' || !f.crate) return; for (const m of f.code.matchAll(/\bpub(?:\([^)]*\))?\s+use\s+([^;]+);/g)) for (const p of expandUse(m[1])) { const nm = p.split('::').pop(); if (!nm || nm === '*') continue; const k = f.mod.join('::'); ((rsReexp[f.crate] = rsReexp[f.crate] || {})[k] = rsReexp[f.crate][k] || {})[nm] = { f, p }; } });
const resolveRs = (f, full, depth = 0, inner = null) => {
  const segs = full.split('::'); let head = segs.shift(), crate = f.crate, base = [];
  const here = (f.mod || []).concat(inner || []); // code in `mod tests` sits one module deeper
  if (head === 'crate') { if (!crate) return null; } else if (head === 'self') { base = here.slice(); } else if (head === 'super') { base = here.slice(0, -1); while (segs[0] === 'super') { segs.shift(); base.pop(); } } else if (rsMod[head]) crate = head; else return null;
  const idx = rsMod[crate]; if (!idx) return null; const all2 = base.concat(segs);
  for (let k = all2.length; k >= 0; k--) { const hit = idx[all2.slice(0, k).join('::')]; if (hit != null && k < all2.length && depth < 4) { const rx = ((rsReexp[crate] || {})[all2.slice(0, k).join('::')] || {})[all2[k]]; if (rx) { const p2 = rx.p.replace(/^::/, ''), r2 = resolveRs(rx.f, /^(crate|self|super)\b/.test(p2) || rsMod[p2.split('::')[0]] ? p2 : 'self::' + p2, depth + 1); if (r2) return r2; } } if (hit != null) return { i: hit, sym: all2.slice(Math.max(0, k - 1)).slice(-2).join('::') || '(module)' }; }
  return null;
};
function expandUse(s) { s = s.replace(/\s+/g, ''); const out = []; (function go(pre, str) { let depth = 0, cur = ''; const parts = []; for (const ch of str) { if (ch === '{') depth++; if (ch === '}') depth--; if (ch === ',' && depth === 0) { parts.push(cur); cur = ''; } else cur += ch; } if (cur) parts.push(cur); for (const p of parts) { const i = p.indexOf('{'); if (i < 0) { let q = p.split(/as(?=[A-Za-z_])/)[0]; if (q === 'self' || q === '*') q = ''; out.push(pre + (q ? (pre ? '::' : '') + q : '')); } else { const head = p.slice(0, i).replace(/::$/, ''), inner = p.slice(i + 1, p.lastIndexOf('}')); go(pre + (pre && head ? '::' : '') + head, inner); } } })('', s); return out.map((x) => x.replace(/::\*$/, '')); };

// JS/TS: tsconfig paths, svelte $lib, workspace packages
const tsconfigs = all.filter((p) => !SKIP.test(p) && /(^|\/)(tsconfig|jsconfig)(\.[\w-]+)?\.json$/.test(p)).map((p) => { const j = readJson(path.join(ROOT, p)) || {}; const co = j.compilerOptions || {}; return { dir: dirOf(p), base: co.baseUrl || '.', paths: co.paths || {} }; }).filter((t) => Object.keys(t.paths).length);
const svelteDirs = all.filter((p) => /(^|\/)svelte\.config\.(js|ts|mjs|cjs)$/.test(p)).map(dirOf);
// import.meta.glob('./x/*.ts') and its array form: every file the patterns match is an import. A leading /
// is the Vite root (the folder with vite.config or svelte.config); aliases such as $lib are not followed.
const viteDirs = all.filter((p) => /(^|\/)(vite|vitest|svelte)\.config\.(js|ts|mjs|cjs|mts)$/.test(p)).map(dirOf);
const globHits = new Map();
const resolveGlob = (rel, pats) => {
  const key = dirOf(rel) + '\0' + pats.join('\0'); if (globHits.has(key)) return globHits.get(key);
  const abs = (p) => p.startsWith('/') ? path.posix.join(near(viteDirs, rel) ?? '', p.slice(1)) : p.startsWith('.') ? path.posix.join(dirOf(rel), p) : null;
  const pos = [], neg = [];
  for (const p of pats) { const neg1 = p.startsWith('!'), a = abs(neg1 ? p.slice(1) : p); if (a == null) continue; for (const g of expandBraces(a)) (neg1 ? neg : pos).push(globRe(path.posix.normalize(g), true)); }
  const hits = pos.length ? rels.filter((r) => pos.some((x) => x.test(r)) && !neg.some((x) => x.test(r))).map((r) => fileSet.get(r)) : [];
  globHits.set(key, hits); return hits;
};
const JS_EXT = ['', '.ts', '.tsx', '.mts', '.cts', '.js', '.jsx', '.mjs', '.cjs', '.svelte', '.vue', '.svelte.ts', '.svelte.js'];
const findFile = (base) => { base = path.posix.normalize(base).replace(/^\.\//, ''); const cands = []; for (const e of JS_EXT) cands.push(base + e); for (const e of JS_EXT.slice(1)) cands.push(base + '/index' + e); if (/\.(js|jsx|mjs|cjs)$/.test(base)) { const s = base.replace(/\.(js|jsx|mjs|cjs)$/, ''); for (const e of ['.ts', '.tsx', '.mts', '.cts']) cands.push(s + e); } for (const c of cands) if (fileSet.has(c)) return fileSet.get(c); return null; };
const near = (list, rel) => list.filter((d) => d === '' || rel.startsWith(d + '/')).sort((a, b) => b.length - a.length)[0];
const resolveJs = (rel, spec) => {
  if (spec.startsWith('.')) return findFile(path.posix.join(dirOf(rel), spec));
  if (spec.startsWith('/')) return findFile(spec.slice(1));
  const tcs = tsconfigs.filter((t) => t.dir === '' || rel.startsWith(t.dir + '/')).sort((a, b) => b.dir.length - a.dir.length);
  for (const t of tcs) for (const [pat, targets] of Object.entries(t.paths)) { const star = pat.indexOf('*'); const pre = star < 0 ? pat : pat.slice(0, star); if (star < 0 ? spec !== pat : !spec.startsWith(pre)) continue; const rest = star < 0 ? '' : spec.slice(pre.length); for (const tg of targets) { const hit = findFile(path.posix.join(t.dir, t.base, tg.replace('*', rest))); if (hit != null) return hit; } }
  if (spec === '$lib' || spec.startsWith('$lib/')) { const d = near(svelteDirs, rel); if (d != null) return findFile(path.posix.join(d, 'src/lib', spec.slice(4))); }
  for (const [nm, p] of npm) if (spec === nm || spec.startsWith(nm + '/')) { const sub = spec.slice(nm.length + 1); return findFile(path.posix.join(p.dir, sub || p.entry.replace(/^\.\//, '') || 'src/index')) ?? findFile(path.posix.join(p.dir, 'index')); }
  return null;
};

// Python: module name → file, under every plausible root
const pyRoots = new Set(['']); for (const [d] of pkgDirs) { pyRoots.add(d); pyRoots.add(d ? d + '/src' : 'src'); }
const pyMod = new Map();
F.forEach((f, i) => { if (f.lang !== 'python') return; for (const r of pyRoots) { if (r && !f.rel.startsWith(r + '/')) continue; const m = f.rel.slice(r ? r.length + 1 : 0).replace(/\.py$/, '').replace(/\/__init__$/, '').split('/').join('.'); if (!pyMod.has(m)) pyMod.set(m, i); } });
const pyModOf = (f) => { for (const r of [...pyRoots].sort((a, b) => b.length - a.length)) if (!r || f.rel.startsWith(r + '/')) return f.rel.slice(r ? r.length + 1 : 0).replace(/\.py$/, '').split('/'); return f.rel.replace(/\.py$/, '').split('/'); };
const resolvePy = (mod) => { const s = mod.split('.'); for (let k = s.length; k > 0; k--) { const hit = pyMod.get(s.slice(0, k).join('.')); if (hit != null) return hit; } return null; };

// Go: import path → a representative file in that package
const goPkg = new Map();
F.forEach((f, i) => { if (f.lang !== 'go' || /_test\.go$/.test(f.rel)) return; const d = dirOf(f.rel); if (!goPkg.has(d)) goPkg.set(d, []); goPkg.get(d).push(i); });
const resolveGo = (imp) => { const m = gomods.filter((g) => imp === g.mod || imp.startsWith(g.mod + '/')).sort((a, b) => b.mod.length - a.mod.length)[0]; if (!m) return null; const d = path.posix.join(m.dir, imp.slice(m.mod.length + 1)).replace(/^\.\/?/, ''); const fs2 = goPkg.get(d === '.' ? '' : d); if (!fs2) return null; const base = path.posix.basename(d); return fs2.find((i) => path.posix.basename(F[i].rel) === base + '.go') ?? fs2.find((i) => /\/doc\.go$/.test(F[i].rel)) ?? fs2.slice().sort((a, b) => F[a].rel.localeCompare(F[b].rel))[0]; };

// in-repo IPC targets (Tauri / Electron)
const tauriEntry = tauriCrate && (rsMod[tauriCrate] ? rsMod[tauriCrate][''] : null);
const electronMain = F.findIndex((f) => f.lang === 'js' && /\bipcMain\.(handle|on)\(/.test(f.text));

mark("Rust");
// ── Rust module declarations: `mod x;` and `#[path = "..."] mod x;`. A module declared inside
// test code is test code, and so is everything it declares in turn.
const declared = new Set(), declaredFiles = new Set();
{
  const decls = []; // [declaring file, line, target file]
  F.forEach((f, i) => {
    if (f.lang !== 'rust' || !f.crate) return;
    for (const m of f.code.matchAll(/(?:^|[;{}\s])(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_]\w*)\s*;/g)) {
      const ln = f.code.slice(0, m.index + m[0].length - 1).split('\n').length, inner = (f.modsAt && f.modsAt[ln - 1]) || [], key = (f.mod || []).concat(inner, [m[1]]).join('::');
      declared.add(f.crate + '::' + key);
      const attr = f.text.slice(Math.max(0, m.index - 240), m.index + 1).match(/#\[path\s*=\s*"([^"]+)"\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?$/);
      let to = null;
      if (attr) { to = fileSet.get(path.posix.normalize(path.posix.join(dirOf(f.rel), attr[1]))); if (to != null) declaredFiles.add(to); }
      else to = (rsMod[f.crate] || {})[key];
      const attrs = (f.text.slice(Math.max(0, m.index - 240), m.index + 1).match(/((?:#\[[^\]]*\]\s*)+)(?:pub(?:\([^)]*\))?\s+)?$/) || [])[1] || '';
      if (to != null) decls.push([i, ln, to, /#\[cfg\(test\)\]/.test(attrs)]);
    }
  });
  for (let changed = true, n = 0; changed && n < 10; n++) {
    changed = false;
    for (const [i, ln, to, cfgTest] of decls) { const g = F[to]; const fromTest = F[i].cat === 'test' || (!F[i].cat && F[i].testAt && !!F[i].testAt[ln - 1]);
      if ((cfgTest || fromTest) && !g.cat) { g.cat = 'test'; g.nf = g.lines; changed = true; } }
  }
}

mark("scan");
// ── scan ────────────────────────────────────────────────────────────────────
const EXT = new Map();
const ext = (id, name, kind) => { if (!EXT.has(id)) EXT.set(id, { id, name, ext: kind }); return id; };
const OPENER = /^(open|xdg-open|explorer(\.exe)?|start|gio|wslview|rundll32(\.exe)?)$/;
const KILLER = /^(kill|pkill|killall|taskkill(\.exe)?)$/;
const procId = (cmd) => OPENER.test(cmd || '') ? ext('x-proc-os-open', 'OS opener', 'subprocess') : KILLER.test(cmd || '') ? ext('x-proc-os-kill', 'OS process control', 'subprocess') : ext('x-proc-' + (cmd || 'dynamic').replace(/[^\w.-]/g, '_').slice(0, 40), cmd ? cmd : 'other processes', 'subprocess');
const E = new Map();
const lineIndex = (text) => { const a = [0]; for (let i = 0; i < text.length; i++) if (text.charCodeAt(i) === 10) a.push(i + 1); return a; };
const lineAt = (idx, pos) => { let lo = 0, hi = idx.length - 1; while (lo < hi) { const m = (lo + hi + 1) >> 1; if (idx[m] <= pos) lo = m; else hi = m - 1; } return lo + 1; };
let CUR = null;
const add = (i, to, kind, sym, line) => {
  if (to == null || to === i) return; const f = F[i], nf = isTestLine(f, line) ? 1 : 0, M = CUR || E;
  const k = `${i}|${to}|${kind}|${nf}`; let e = M.get(k); if (!e) { e = { i, to, kind, nf, w: 0, s: new Map(), l: [] }; M.set(k, e); }
  e.w++; e.s.set(sym, (e.s.get(sym) || 0) + 1); if (e.l.length < 3) e.l.push(line);
};
const writers = new Map();
const write = (i, line) => { const f = F[i], nf = isTestLine(f, line); if (!CUR) { const w = writers.get(i) || [0, 0]; w[nf ? 1 : 0]++; writers.set(i, w); } add(i, ext('x-disk', 'files on disk', 'on disk'), 'fs', 'file write', line); };
const net = (i, line, sym) => add(i, ext('x-net', 'network', 'network'), 'net', sym, line);
// Outside services reached through an SDK. A bare import of one of these packages is a dependency on that service.
const SDK = [
  [/^@supabase\/|^supabase$/, 'supabase', 'Supabase'], [/^stripe$|^@stripe\//, 'stripe', 'Stripe'], [/^firebase(-admin)?$|^@firebase\//, 'firebase', 'Firebase'],
  [/^@aws-sdk\/|^aws-sdk$|^boto3$|^botocore$|^aws_sdk_|^github\.com\/aws\//, 'aws', 'AWS'], [/^@google-cloud\/|^google\.cloud|^cloud\.google\.com\//, 'gcp', 'Google Cloud'],
  [/^@prisma\/client$|^drizzle-orm|^pg$|^postgres$|^mysql2?$|^mongodb$|^mongoose$|^better-sqlite3$|^kysely$|^typeorm$|^sequelize$|^knex$|^psycopg2?$|^sqlalchemy$|^asyncpg$|^pymongo$|^sqlx$|^diesel$|^tokio_postgres$|^rusqlite$|^sea_orm$|^database\/sql$|^gorm\.io\/|^github\.com\/jackc\/pgx/, 'database', 'database'],
  [/^redis$|^ioredis$|^@upstash\/redis$/, 'redis', 'Redis'], [/^openai$|^@anthropic-ai\/|^anthropic$|^@google\/generative-ai$|^async_openai$/, 'llm', 'AI model APIs'],
  [/^resend$|^@sendgrid\/|^nodemailer$|^postmark$|^mailgun|^lettre$/, 'email', 'email'], [/^twilio$/, 'sms', 'Twilio'],
  [/^@sentry\/|^sentry_sdk$|^sentry$/, 'sentry', 'Sentry'], [/^posthog|^@posthog\/|^mixpanel|^@segment\//, 'analytics', 'analytics'], [/^@octokit\/|^octokit$/, 'github-api', 'GitHub API'],
];
const sdkOf = (spec) => { for (const [re, id, name] of SDK) if (re.test(spec)) return ext('x-svc-' + id, name, 'service'); return null; };


function scanFile(f, i) {
  if (!f.text) return; const T = f.text, LI = lineIndex(T), at = (pos) => lineAt(LI, pos);
  const C = f.code, CL = C.split('\n'), OL = T.split('\n');
  // detection runs on the original line (string literals intact) but only where the comment-free text still has code
  const eachLine = (fn) => OL.forEach((ln, li) => { if (!(CL[li] || '').trim()) return; fn(ln, li + 1, CL[li]); });
  if (f.lang === 'rust') {
    const tst = (L) => (f.modsAt && f.modsAt[L - 1]) || [];
    for (const m of C.matchAll(/\buse\s+([^;]+);/g)) { const ln = at(m.index); { const head = m[1].trim().split('::')[0]; if (!rsMod[head] && !['crate', 'self', 'super', 'std', 'core', 'alloc'].includes(head)) { const svc = sdkOf(head); if (svc) add(i, svc, 'net', head, ln); } } for (const p of expandUse(m[1])) { const r = resolveRs(f, p.replace(/^::/, ''), 0, tst(ln)); if (r) add(i, r.i, 'import', r.sym, ln); } }
    const masked = C.replace(/\buse\s+[^;]+;/g, (s) => s.replace(/[^\n]/g, ' '));
    masked.split('\n').forEach((ln, li) => { const L = li + 1, orig = OL[li] || '';
      for (const m of ln.matchAll(/\b(crate|super|self|[a-z_][a-z0-9_]*)::((?:[A-Za-z_]\w*::)*[A-Za-z_]\w*)/g)) { if (m[1] !== 'crate' && m[1] !== 'super' && m[1] !== 'self' && !rsMod[m[1]]) continue; if (m.index > 0 && ln[m.index - 1] === ':') continue; const r = resolveRs(f, m[1] + '::' + m[2], 0, tst(L)); if (r) add(i, r.i, 'import', r.sym, L); }
      const c = /Command::new\(/.test(ln) && orig.match(/Command::new\(\s*"([^"]+)"/); if (c) add(i, procId(path.posix.basename(c[1])), 'spawn', `Command::new("${c[1]}")`, L); else if (/Command::new\(/.test(ln)) add(i, procId(''), 'spawn', 'Command::new(…)', L);
      if (/\b(reqwest|ureq|hyper|surf|isahc|tungstenite|tonic)::/.test(ln)) net(i, L, (ln.match(/\b(reqwest|ureq|hyper|surf|isahc|tungstenite|tonic)::/) || [])[1]);
      if (/\b(fs::(write|rename|remove_file|remove_dir_all|create_dir_all|copy)|File::create|OpenOptions::new|tokio::fs::write)\b/.test(ln)) write(i, L);
    });
  } else if (f.lang === 'js') {
    for (const m of C.matchAll(/(?:^|[^\w$.])(?:import|export)\s+(?:type\s+)?(?:([\w$*{}\s,]+?)\s+from\s+)?['"]([^'"\n]+)['"]|(?:import|require)\(\s*['"]([^'"\n]+)['"]\s*\)|new\s+URL\(\s*['"](\.[^'"\n]+)['"]\s*,\s*import\.meta\.url/g)) {
      const spec = m[2] || m[3] || m[4]; const to = resolveJs(f.rel, spec); if (to == null) { const svc = !spec.startsWith('.') && sdkOf(spec); if (svc) add(i, svc, 'net', spec, at(m.index)); continue; } const L = at(m.index);
      const names = (m[1] || '').replace(/[{}]/g, ',').split(',').map((s) => s.trim().split(/\s+as\s+/)[0].replace(/^type\s+/, '')).filter((s) => s && s !== '*');
      (names.length ? names.slice(0, 6) : [path.posix.basename(spec)]).forEach((nm) => add(i, to, 'import', nm, L));
    }
    for (const m of C.matchAll(/\bimport\.meta\.glob(?:<[^>]*>)?\(\s*(\[[^\]]*\]|'[^'\n]*'|"[^"\n]*"|`[^`$]*`)/g)) {
      const pats = [...m[1].matchAll(/'([^'\n]*)'|"([^"\n]*)"|`([^`$]*)`/g)].map((x) => x[1] ?? x[2] ?? x[3]), L = at(m.index);
      for (const to of resolveGlob(f.rel, pats)) add(i, to, 'import', 'import.meta.glob', L);
    }
    const usesCp = /['"](node:)?child_process['"]|['"]execa['"]|@tauri-apps\/(plugin-shell|api\/shell)/.test(T), usesFs = /['"](node:)?fs(\/promises)?['"]|['"]fs-extra['"]/.test(T), tauri = /@tauri-apps\/api/.test(T), electron = /ipcRenderer/.test(T);
    eachLine((ln, L) => {
      if (usesCp) { const c = ln.match(/\b(?:spawn|spawnSync|exec|execSync|execFile|execFileSync|fork|execa|execaSync|execaCommand)\(\s*['"`]([^'"`\s)]+)/) || ln.match(/Command\.(?:create|sidecar)\(\s*['"]([^'"]+)/); if (c) add(i, procId(path.posix.basename(c[1])), 'spawn', c[0].slice(0, 40), L); else if (/\b(spawn|exec|execFile|fork|execa)\(/.test(ln)) add(i, procId(''), 'spawn', 'spawn(…)', L); }
      if (/(?:^|[^\w.])(fetch|axios|got|ky|ofetch|\$fetch)\s*[.(]|new (WebSocket|EventSource)\(|\bhttps?\.(request|get)\(/.test(ln) && !/function fetch|fetch:\s/.test(ln)) net(i, L, (ln.match(/(fetch|axios|got|ky|WebSocket|EventSource|http)/) || ['', 'network'])[1]);
      if (usesFs && /\b(writeFile|writeFileSync|appendFile|appendFileSync|createWriteStream|rename|renameSync|unlink|unlinkSync|rm|rmSync|outputFile|writeJson)\(/.test(ln)) write(i, L);
      if (tauri) { const c = ln.match(/\binvoke(?:<[^>]*>)?\(\s*['"]([\w:./-]+)/); if (c && !/^[a-z_][a-z0-9_]*$/.test(c[1])) add(i, tauriEntry ?? ext('x-ipc', 'native backend', 'ipc'), 'ipc', `invoke("${c[1]}")`, L); }
      if (electron) { const c = ln.match(/ipcRenderer\.(invoke|send|sendSync)\(\s*['"]([\w:./-]+)/); if (c) add(i, electronMain >= 0 ? electronMain : ext('x-ipc', 'main process', 'ipc'), 'ipc', `${c[1]}("${c[2]}")`, L); }
    });
  } else if (f.lang === 'python') {
    const pkg = pyModOf(f).slice(0, -1); const isInit = /__init__\.py$/.test(f.rel); const here = isInit ? pyModOf(f).slice(0, -1).concat([]) : pkg;
    for (const m of C.matchAll(/^[ \t]*from[ \t]+(\.*)([\w.]*)[ \t]+import[ \t]+(\([^)]*\)|[^\n#]+)/gm)) {
      const L = at(m.index), dots = m[1].length, names = m[3].replace(/[()\n\\]/g, ' ').split(',').map((s) => s.trim().split(/\s+as\s+/)[0]).filter(Boolean);
      let baseMod; if (dots) { const up = here.slice(0, Math.max(0, here.length - (dots - 1))); baseMod = up.concat(m[2] ? m[2].split('.') : []).join('.'); } else baseMod = m[2];
      if (!dots && m[2] && resolvePy(m[2]) == null) { const svc = sdkOf(m[2].split('.')[0]) || sdkOf(m[2]); if (svc) add(i, svc, 'net', m[2], L); }
      for (const nm of names.slice(0, 6)) { const to = (nm !== '*' && pyMod.get(baseMod ? baseMod + '.' + nm : nm)) ?? resolvePy(baseMod); if (to != null) add(i, to, 'import', nm, L); }
    }
    for (const m of C.matchAll(/^[ \t]*import[ \t]+([\w., \t]+)/gm)) { const L = at(m.index); for (const part of m[1].split(',')) { const mod = part.trim().split(/\s+as\s+/)[0]; const to = mod && resolvePy(mod); if (to == null && mod) { const svc = sdkOf(mod.split('.')[0]) || sdkOf(mod); if (svc) add(i, svc, 'net', mod, L); } if (to != null) add(i, to, 'import', mod.split('.').pop(), L); } }
    eachLine((ln, L) => {
      const c = ln.match(/\bsubprocess\.(?:run|Popen|call|check_output|check_call)\(\s*\[?\s*f?['"]([^'"\s]+)/); if (c) add(i, procId(path.posix.basename(c[1])), 'spawn', 'subprocess: ' + c[1], L); else if (/\b(subprocess\.(run|Popen|call|check_output|check_call)|os\.(system|popen)|asyncio\.create_subprocess_\w+)\(/.test(ln)) add(i, procId(''), 'spawn', 'subprocess(…)', L);
      if (/\b(requests|httpx|aiohttp|urllib\.request|urllib3|grpc)\.\w+\(/.test(ln)) net(i, L, (ln.match(/\b(requests|httpx|aiohttp|urllib|grpc)/) || ['', 'http'])[1]);
      if (/\bopen\([^)]*,\s*['"][wax]b?\+?['"]|\.write_(text|bytes)\(|\bshutil\.(copy\w*|move|rmtree)\(|\bos\.(remove|unlink|rename|replace|makedirs)\(/.test(ln)) write(i, L);
    });
  } else if (f.lang === 'go') {
    const imports = []; for (const m of C.matchAll(/^import\s*\(([\s\S]*?)^\)/gm)) for (const x of m[1].matchAll(/^\s*(?:[\w.]+\s+)?"([^"]+)"/gm)) imports.push([x[1], at(m.index + x.index)]);
    for (const m of C.matchAll(/^import\s+(?:[\w.]+\s+)?"([^"]+)"/gm)) imports.push([m[1], at(m.index)]);
    for (const [imp, L] of imports) { const to = resolveGo(imp); if (to == null) { const svc = sdkOf(imp); if (svc) add(i, svc, 'net', imp, L); } if (to != null) add(i, to, 'import', imp.split('/').pop(), L); }
    eachLine((ln, L) => {
      const c = ln.match(/exec\.Command(?:Context)?\(\s*(?:\w+,\s*)?"([^"]+)"/); if (c) add(i, procId(path.posix.basename(c[1])), 'spawn', `exec.Command("${c[1]}")`, L); else if (/exec\.Command(Context)?\(/.test(ln)) add(i, procId(''), 'spawn', 'exec.Command(…)', L);
      if (/\bhttp\.(Get|Post|PostForm|Head|NewRequest\w*)\(|&http\.Client\{|\bgrpc\.(Dial|NewClient)\w*\(/.test(ln)) net(i, L, 'http');
      if (/\bos\.(WriteFile|Create|Rename|Remove|RemoveAll|MkdirAll|OpenFile)\(|ioutil\.WriteFile\(/.test(ln)) write(i, L);
    });
  }
}
F.forEach(scanFile);

mark("change");
// ── change: what moved since a base. Changed files are re-parsed at the base commit, so this
// costs one extra parse per changed file. Without --base, uncommitted changes are compared with HEAD.
let CHANGE = null;
{
  const baseRef = baseArg || (DIRTY ? 'HEAD' : null);
  const mb = !baseRef ? '' : baseArg ? (tryGit('merge-base', baseArg, 'HEAD') || tryGit('rev-parse', '--verify', baseArg + '^{commit}')) : tryGit('rev-parse', 'HEAD');
  if (baseArg && !mb) warnings.push(`--base ${baseArg}: not a commit this repository knows.`);
  if (mb) {
    const raw = (...a) => { try { return execFileSync('git', ['-C', ROOT, ...a], { encoding: 'utf8', maxBuffer: 1 << 30 }); } catch { return null; } };
    const isCode = (p) => CODE.test(p) && !SKIP.test(p), entries2 = [];
    const status = new Map(), z = (raw('diff', '--name-status', '-M', '-z', mb) || '').split('\0'); for (let k = 0; k < z.length - 1;) { const st = z[k++]; if (!st) continue; if (st[0] === 'R' || st[0] === 'C') { const from = z[k++], to = z[k++]; status.set(to, ['R', from]); } else status.set(z[k++], [st[0], null]); }
    const nz = (raw('diff', '--numstat', '-M', '-z', mb) || '').split('\0'); for (let k = 0; k < nz.length - 1;) { const head = nz[k++]; if (!head) continue; const [a, d, p] = head.split('\t'); let pth = p; if (!p) { k++; pth = nz[k++]; } const s = status.get(pth); if (s) s.push(a === '-' ? 0 : +a, d === '-' ? 0 : +d); }
    for (const p of (raw('ls-files', '--others', '--exclude-standard', '-z') || '').split('\0').filter(Boolean)) if (!status.has(p)) { const i = fileSet.get(p); status.set(p, ['A', null, i != null ? F[i].lines - (F[i].text.endsWith('\n') ? 1 : 0) : 0, 0]); }
    const deps = [];
    for (const [p, [st, from, add = 0, del = 0]] of status) {
      if (!isCode(p) && !(from && isCode(from))) continue;
      const i = fileSet.get(p); entries2.push([i ?? -1, p, st, add, del, from]);
      if (i == null) continue;
      const cur = new Map(); for (const e of E.values()) if (e.i === i) cur.set(`${e.to}|${e.kind}|${e.nf}`, e);
      let bas = new Map();
      if (st !== 'A') {
        const text = raw('show', `${mb}:${from || p}`);
        if (text != null) {
          const fb = Object.assign({}, F[i], { text, lines: text.split('\n').length, modsAt: null, testAt: null });
          fb.code = codeOnly(text, fb.lang, fb.lang === 'rust' || fb.lang === 'python'); innerMods(fb);
          CUR = new Map(); const keep = F[i]; F[i] = fb; try { scanFile(fb, i); } finally { F[i] = keep; } bas = CUR; CUR = null;
          bas = new Map([...bas.values()].map((e) => [`${e.to}|${e.kind}|${e.nf}`, e]));
        }
      }
      for (const k of new Set([...cur.keys(), ...bas.keys()])) {
        const a = cur.get(k), b = bas.get(k), d = (a ? a.w : 0) - (b ? b.w : 0); if (!d) continue;
        const e = d > 0 ? a : b; deps.push([i, e.to, e.kind, e.nf, d, [...e.s.entries()].sort((x, y) => y[1] - x[1]).slice(0, 3)]);
      }
    }
    if (entries2.length) CHANGE = { base: baseArg || 'HEAD', mergeBase: mb, files: entries2, deps };
  }
}

mark("IPC");
// ── IPC from a web UI to a Tauri backend: a string in the UI that names a #[tauri::command]
{
  const cmds = new Map();
  F.forEach((f, i) => { if (f.lang !== 'rust') return; for (const m of f.code.matchAll(/#\[(?:tauri::)?command[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([a-z_][a-z0-9_]*)/g)) if (!cmds.has(m[1])) cmds.set(m[1], i); });
  if (cmds.size) F.forEach((f, i) => {
    if (f.lang !== 'js' || !/@tauri-apps\/api/.test(f.text) && !/\binvoke\b/.test(f.code)) return;
    const LI = lineIndex(f.text);
    for (const m of f.code.matchAll(/['"`]([a-z_][a-z0-9_]*)['"`]/g)) { const to = cmds.get(m[1]); if (to != null) add(i, to, 'ipc', `command "${m[1]}"`, lineAt(LI, m.index)); }
  });
}

mark("events");
// ── events pushed from a Rust backend to a web UI (Tauri emit → listen), matched by event name
{
  const consts = new Map();
  F.forEach((f) => { const rx = f.lang === 'rust' ? /const\s+([A-Z_][A-Z0-9_]*)\s*:\s*&(?:'static\s+)?str\s*=\s*"([^"]+)"/g : f.lang === 'js' ? /const\s+([A-Z_][A-Z0-9_]*)\s*=\s*['"`]([^'"`]+)['"`]/g : null; if (rx) for (const m of f.text.matchAll(rx)) consts.set(m[1], m[2]); });
  const emits = new Map(), listens = new Map(), push = (M, k, v) => { if (!M.has(k)) M.set(k, []); M.get(k).push(v); };
  F.forEach((f, i) => {
    if (f.lang === 'rust') { const LI = lineIndex(f.text); for (const m of f.text.matchAll(/\.emit(?:_to|_all|_filter)?\(\s*(?:[\w.&]+\s*,\s*)?(?:"([\w:./-]+)"|([A-Z_][A-Z0-9_]*))/g)) { if (f.code[m.index] === ' ') continue; const nm = m[1] || consts.get(m[2]); if (nm) push(emits, nm, [i, lineAt(LI, m.index)]); } }
    else if (f.lang === 'js' && /@tauri-apps\/api/.test(f.text)) { const LI = lineIndex(f.text); for (const m of f.code.matchAll(/\b(?:listen|once)(?:<[^>]*>)?\(\s*(?:['"`]([\w:./-]+)['"`]|([A-Z_][A-Z0-9_]*))/g)) { const nm = m[1] || consts.get(m[2]); if (nm) push(listens, nm, [i, lineAt(LI, m.index)]); } }
  });
  for (const [nm, ls] of listens) for (const [ei, eL] of (emits.get(nm) || [])) for (const [li] of ls) add(ei, li, 'event', `emit("${nm}")`, eL);
}

mark("fold");
// ── fold rarely used processes into one, so test-only commands don't crowd the map
{
  const use = new Map(); for (const e of E.values()) if (typeof e.to === 'string' && e.to.startsWith('x-proc-')) use.set(e.to, (use.get(e.to) || 0) + (e.nf ? 0 : e.w));
  const keep = new Set([...use.entries()].filter((x) => x[1] > 0 && x[0] !== 'x-proc-dynamic').sort((a, b) => b[1] - a[1]).slice(0, 6).map((x) => x[0]));
  const folded = [...use.keys()].filter((k) => !keep.has(k));
  if (folded.length) {
    const other = ext('x-proc-other', 'other processes', 'subprocess'); EXT.get(other).note = 'Folded: ' + folded.map((k) => EXT.get(k).name).join(', ');
    for (const [k, e] of [...E.entries()]) { if (!folded.includes(e.to)) continue; E.delete(k); const k2 = `${e.i}|${other}|${e.kind}|${e.nf}`, t = E.get(k2); if (t) { t.w += e.w; e.s.forEach((v, s) => t.s.set(s, (t.s.get(s) || 0) + v)); t.l = t.l.concat(e.l).slice(0, 3); } else E.set(k2, Object.assign(e, { to: other })); }
    folded.forEach((k) => { if (k !== other) EXT.delete(k); });
    if (CHANGE) CHANGE.deps.forEach((d) => { if (folded.includes(d[1])) d[1] = other; });
  }
}

mark("reachability");
// ── reachability facts: entry points, undeclared Rust files, name mentions
const entries = new Set();
{
  const tryJs = (dir, spec) => { if (!spec || typeof spec !== 'string') return; const i = findFile(path.posix.join(dir, spec.replace(/^\.\//, ''))); if (i != null) entries.add(i); };
  F.forEach((f, i) => {
    const b = path.posix.basename(f.rel);
    if (f.lang === 'rust' && (/(^|\/)src\/(main|lib)\.rs$/.test(f.rel) || /(^|\/)(build\.rs|src\/bin\/[^/]+\.rs)$/.test(f.rel) || /(^|\/)(tests|benches|examples)\/[^/]+\.rs$/.test(f.rel))) entries.add(i);
    if (f.lang === 'go' && /^\s*package\s+main\b/m.test(f.code)) entries.add(i);
    if (f.lang === 'python' && (b === '__main__.py' || /^if\s+__name__\s*==\s*['"]__main__['"]/m.test(f.text) || b === 'manage.py' || b === 'wsgi.py' || b === 'asgi.py')) entries.add(i);
    if (f.lang === 'js' && (/(^|\/)(routes|pages|app)\/.*(\+(page|layout|server|error)(@[^/.]*)?|page|layout|route|loading|error)(\.[\w]+)*\.(svelte|tsx?|jsx?|vue)$/.test(f.rel) || /(^|\/)(hooks\.(server|client)|service-worker|sw)\.(ts|js)$/.test(f.rel))) entries.add(i);
  });
  for (const [rel] of scriptRuns) entries.add(fileSet.get(rel));
  for (const p of all.filter((x) => /\.html$/.test(x) && !SKIP.test(x))) { const t = read(p); for (const m of t.matchAll(/<script[^>]*\bsrc=["']([^"']+)["']/g)) tryJs(dirOf(p), m[1].startsWith('/') ? m[1].slice(1) : m[1]); }
  for (const m of manifests.filter((x) => /package\.json$/.test(x))) { const j = readJson(path.join(ROOT, m)) || {}, d = dirOf(m); [j.main, j.module, j.source, j.browser, typeof j.bin === 'string' ? j.bin : null, ...(j.bin && typeof j.bin === 'object' ? Object.values(j.bin) : [])].forEach((s) => tryJs(d, s)); if (j.exports) JSON.stringify(j.exports).replace(/"(\.\/[^"]+)"/g, (_, s) => { tryJs(d, s); return ''; }); }
  for (const p of all.filter((x) => /(^|\/)(vite|vitest|svelte|astro|nuxt|next|webpack|rollup)\.config\.(js|ts|mjs|cjs)$/.test(x))) { const t = read(p); for (const m of t.matchAll(/['"](\.?\/?[\w./-]+\.(ts|tsx|js|jsx|svelte|vue))['"]/g)) tryJs(dirOf(p), m[1]); }
}
const orphans = []; F.forEach((f, i) => { if (f.lang === 'rust' && f.crate && f.mod && f.mod.length && !entries.has(i) && !declaredFiles.has(i) && !declared.has(f.crate + '::' + f.mod.join('::'))) orphans.push(i); });
const referenced = new Set(); for (const e of E.values()) if (typeof e.to === 'number' && e.to !== e.i) referenced.add(e.to);
const mentions = [];
{
  const esc = (s) => s.replace(/[.*+?^$|()[\]{}\\]/g, '\\$&');
  const cand = F.map((f, i) => i).filter((i) => !referenced.has(i) && !entries.has(i) && F[i].lang !== 'rust' && !F[i].cat && F[i].nf < F[i].lines);
  const stems = cand.map((i) => [i, path.posix.basename(F[i].rel).replace(/\.(svelte\.ts|svelte\.js|d\.ts|[a-z]+)$/, '')]).filter(([, s]) => s.length >= 4);
  if (stems.length) {
    const hay = all.filter((p) => !SKIP.test(p) && /\.(rs|ts|tsx|mts|cts|js|jsx|mjs|cjs|svelte|vue|py|go|html|json|toml|ya?ml|css)$/.test(p));
    // Split each file into words once and look them up in a set. A name counts only as a whole word
    // ([\w$-] on neither side). This avoids one pattern with hundreds of alternatives, whose speed
    // depends heavily on the JavaScript engine (V8 and JavaScriptCore each choke on it in different cases).
    const words = new Set(), dotted = [], where = new Map(), hit = (s, p) => { let z = where.get(s); if (!z) where.set(s, (z = new Set())); z.add(p); };
    for (const [, s] of stems) (/^[\w$-]+$/.test(s) ? words.add(s) : dotted.push(s));
    const dottedRe = dotted.map((s) => [s, new RegExp('(^|[^\\w$-])' + esc(s) + '([^\\w$-]|$)')]);
    for (const p of hay) {
      if (/(^|\/)(package-lock\.json|pnpm-lock\.yaml|yarn\.lock|Cargo\.lock|poetry\.lock|composer\.lock)$/.test(p)) continue;
      let t; if (fileSet.has(p)) t = F[fileSet.get(p)].text; else { try { if (fs.statSync(path.join(ROOT, p)).size > 2_000_000) continue; } catch { continue; } t = read(p); }
      if (words.size) for (const w of t.split(/[^\w$-]+/)) if (words.has(w)) hit(w, p);
      for (const [s, re] of dottedRe) if (t.includes(s) && re.test(t)) hit(s, p);
    }
    for (const [i, s] of stems) { const hits = [...(where.get(s) || [])].filter((p) => p !== F[i].rel).sort(); mentions.push([i, hits.length, hits.slice(0, 3)]); }
  }
}

mark("draft");
// ── draft model: inferred parts with layers from import direction ───────────
function inferLayers(ids, edges) {
  const adj = new Map(ids.map((k) => [k, new Set()])); for (const [a, b] of edges) if (a !== b && adj.has(a) && adj.has(b)) adj.get(a).add(b);
  let ix = 0; const idx = new Map(), low = new Map(), st = [], on = new Set(), comp = new Map(); let cn = 0;
  const go = (v) => { idx.set(v, ix); low.set(v, ix); ix++; st.push(v); on.add(v); for (const x of adj.get(v)) { if (!idx.has(x)) { go(x); low.set(v, Math.min(low.get(v), low.get(x))); } else if (on.has(x)) low.set(v, Math.min(low.get(v), idx.get(x))); } if (low.get(v) === idx.get(v)) { let x; do { x = st.pop(); on.delete(x); comp.set(x, cn); } while (x !== v); cn++; } };
  for (const k of ids) if (!idx.has(k)) go(k);
  const pred = new Map(); for (const [a, s] of adj) for (const b of s) { const ca = comp.get(a), cb = comp.get(b); if (ca !== cb) { if (!pred.has(cb)) pred.set(cb, new Set()); pred.get(cb).add(ca); } }
  const memo = new Map(), depth = (c) => { if (memo.has(c)) return memo.get(c); memo.set(c, 0); let d = 0; for (const p of (pred.get(c) || [])) d = Math.max(d, 1 + depth(p)); memo.set(c, d); return d; };
  const out = new Map(); let mx = 0; ids.forEach((k) => { const d = depth(comp.get(k)); out.set(k, d); mx = Math.max(mx, d); });
  if (mx > 5) ids.forEach((k) => out.set(k, Math.round(out.get(k) * 5 / mx)));
  return out;
}
const partEdges = []; for (const e of E.values()) { if (typeof e.to !== 'number' || e.nf) continue; const a = F[e.i].part, b = F[e.to].part; if (a !== b) partEdges.push([a, b]); }
const pIds = partsMeta.map((p) => p.id), inferred = inferLayers(pIds, partEdges);
const mxL = Math.max(0, ...inferred.values());
const autoLayerNames = Array.from({ length: mxL + 1 }, (_, k) => mxL === 0 ? 'All parts' : k === 0 ? 'Entry points' : k === mxL ? 'Foundation' : `Layer ${k + 1}`);

mark("write");
// ── write ───────────────────────────────────────────────────────────────────
const tooling = new Set(partsMeta.filter((p) => { const fs2 = F.filter((f) => f.part === p.id); const t = fs2.reduce((s, f) => s + f.lines, 0); return t && fs2.filter((f) => f.cat === 'tooling').reduce((s, f) => s + f.lines, 0) / t > 0.9; }).map((p) => p.id));
const out = {
  format: 'er-arch/1',
  extractor: { name: 'er-arch-extract', version: VERSION, sha: SELF, features: FEATURES },
  repo: { name: (MODEL && MODEL.name) || NAME, commit: COMMIT, branch: BRANCH, dirty: DIRTY, generatedAt: new Date().toISOString() },
  model: {
    source: MODEL && Array.isArray(MODEL.layers) && MODEL.layers.length ? 'declared' : 'inferred',
    file: modelPath ? path.relative(ROOT, modelPath) : null,
    layers: MODEL && Array.isArray(MODEL.layers) && MODEL.layers.length ? MODEL.layers : null,
    rules: (MODEL && MODEL.rules) || {},
    decisions: (MODEL && MODEL.decisions) || [],
    flows: (MODEL && MODEL.flows) || [],
    groups: ((MODEL && MODEL.groups) || []).map((g) => ({ id: slug(g.id || g.name), name: g.name || g.id, layer: g.layer, group: g.group ? slug(g.group) : '', layers: g.layers || [], description: g.description || '' })),
    description: (MODEL && MODEL.description) || '',
    nonFunctional: NF,
    autoTooling: AUTO_TOOLING,
  },
  parts: partsMeta.map((p) => ({ id: p.id, name: p.name, layer: p.layer ?? null, group: p.group || '', inferredLayer: inferred.get(p.id) ?? 0, description: p.desc || '', kind: p.kind || (tooling.has(p.id) ? 'tool' : ''), paths: p.paths || [] })),
  inferredLayers: autoLayerNames,
  files: F.map((f) => [f.rel, f.part, f.lines, f.nf, f.cat, f.lang]),
  externals: [...EXT.values()],
  edges: [...E.values()].map((e) => [e.i, e.to, e.kind, e.nf, e.w, [...e.s.entries()].sort((a, b) => b[1] - a[1]).slice(0, 5), e.l]),
  writers: [...writers.entries()].map(([i, w]) => [i, w[0], w[1]]),
  entries: [...entries].sort((a, b) => a - b),
  orphans,
  mentions,
  change: CHANGE,
  warnings,
};
const outPath = path.resolve(outArg || `${NAME}.er-arch.json`);
fs.writeFileSync(outPath, JSON.stringify(out));
let draftPath = null;
if (!MODEL && !outPath.startsWith('/dev/')) {
  draftPath = outPath.replace(/\.er-arch\.json$|\.json$/, '') + '.architecture.draft.json';
  const draft = {
    name: NAME,
    description: 'Draft written by er-arch-extract. Rename the layers, move parts between them, add rules, then save as architecture.json in the repo root and run the script again.',
    layers: autoLayerNames,
    parts: partsMeta.map((p) => ({ id: p.id, name: p.name, layer: autoLayerNames[inferred.get(p.id) ?? 0], paths: p.paths })),
    rules: { allowed: {}, ports: {}, exceptions: [] },
    nonFunctional: { test: [], generated: [], dev: [], tooling: [] },
    decisions: [],
    flows: [],
  };
  fs.writeFileSync(draftPath, JSON.stringify(draft, null, 2) + '\n');
}
const lines = F.reduce((s, f) => s + f.lines, 0), nfl = F.reduce((s, f) => s + f.nf, 0), langs = {}; F.forEach((f) => { langs[f.lang] = (langs[f.lang] || 0) + f.lines; });
console.log(`er-arch-extract ${VERSION} (${SELF}) · ${NAME} @ ${COMMIT.slice(0, 10)}${DIRTY ? ' (with local changes)' : ''}`);
console.log(`  ${F.length} code files, ${lines.toLocaleString()} lines (${Object.entries(langs).map(([k, v]) => `${k} ${Math.round(v / lines * 100)}%`).join(', ')})`);
console.log(`  ${Math.round(nfl / (lines || 1) * 100)}% non-functional (tests, generated, stories, tooling)`);
if (AUTO_TOOLING) console.log(`  ${AUTO_TOOLING} file${AUTO_TOOLING > 1 ? 's' : ''} recognised as build tooling (code generators, xtask, package.json build scripts)`);
if (CHANGE) { const fa = CHANGE.files.reduce((s, x) => s + x[3], 0), fd = CHANGE.files.reduce((s, x) => s + x[4], 0), up = CHANGE.deps.filter((d) => d[4] > 0 && !d[3]).length, dn = CHANGE.deps.filter((d) => d[4] < 0 && !d[3]).length; console.log(`  change since ${CHANGE.base === 'HEAD' ? 'HEAD (uncommitted)' : CHANGE.base} @ ${CHANGE.mergeBase.slice(0, 10)}: ${CHANGE.files.length} code files, +${fa} −${fd}; ${up} dependencies added, ${dn} removed`); }
console.log(`  ${partsMeta.length} parts (${partSource}), ${E.size} file-level dependencies, ${EXT.size} outside systems`);
for (const w of warnings) console.log(`  warning: ${w}`);
console.log(`  wrote ${outPath} (${Math.round(fs.statSync(outPath).size / 1024)} KB) in ${((Date.now() - t0) / 1000).toFixed(1)}s`);
if (draftPath) console.log(`  wrote ${draftPath}\n  No architecture.json found: parts and layers were inferred. Edit the draft and save it as architecture.json in the repo root to declare them.`);
if (TIMINGS) { mark('end'); console.log('\n  timings (' + (typeof Bun !== 'undefined' ? 'bun ' + Bun.version : 'node ' + process.versions.node) + ')'); for (const [s, ms] of __times) if (ms >= 1) console.log(`  ${String(Math.round(ms)).padStart(7)} ms  ${s}`); }
console.log('\nNext: open er Map, choose Import, and pick the .er-arch.json file.');
