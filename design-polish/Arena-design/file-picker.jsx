// file-picker.jsx — Selected-files picker, modeled on the existing
// "Review selected files" modal. Opens from the Arena launcher when the
// user picks Scope: Selected files.

const { useState: useFP, useMemo: useFPMemo, useEffect: useFPE, useRef: useFPRef } = React;

// Build a tree from window.DATA_FILES (flat list with depth + dir markers).
function buildFileTree() {
  const flat = window.DATA_FILES || [];
  const nodes = [];
  let currentDir1 = null;
  let currentDir2 = null;
  flat.forEach((row, i) => {
    if (row.kind === 'dir') {
      const node = { type: 'dir', id: 'd' + i, path: row.path, depth: row.depth, children: [] };
      if (row.depth === 0) { currentDir1 = node; nodes.push(node); currentDir2 = null; }
      else if (row.depth === 1) { currentDir2 = node; (currentDir1?.children || nodes).push(node); }
    } else {
      const node = {
        type: 'file', id: 'f' + i,
        name: row.name, ext: row.ext,
        add: row.add || 0, del: row.del || 0,
        comments: row.comments || 0,
      };
      (currentDir2?.children || currentDir1?.children || nodes).push(node);
    }
  });
  return nodes;
}

// Collect all file ids in a tree
function collectFileIds(nodes) {
  const out = [];
  const walk = (n) => { if (n.type === 'file') out.push(n.id); n.children?.forEach(walk); };
  nodes.forEach(walk);
  return out;
}

function FilePicker({ open, initialSelected = [], onCancel, onConfirm }) {
  const tree = useFPMemo(buildFileTree, []);
  const allIds = useFPMemo(() => collectFileIds(tree), [tree]);

  const [selected, setSelected] = useFP(() => new Set(initialSelected.length ? initialSelected : allIds));
  const [filter, setFilter] = useFP('');
  const [expanded, setExpanded] = useFP(() => new Set(['d0', 'd1', 'd5']));
  const filterRef = useFPRef(null);

  useFPE(() => {
    if (!open) return;
    const onKey = (e) => {
      if (e.key === 'Escape') { onCancel(); }
      if (e.key === '/' && document.activeElement !== filterRef.current) {
        e.preventDefault();
        filterRef.current?.focus();
      }
    };
    window.addEventListener('keydown', onKey);
    setTimeout(() => filterRef.current?.focus(), 50);
    return () => window.removeEventListener('keydown', onKey);
  }, [open]);

  if (!open) return null;

  const toggleFile = (id) => {
    const next = new Set(selected);
    next.has(id) ? next.delete(id) : next.add(id);
    setSelected(next);
  };

  const toggleDir = (node) => {
    const ids = collectFileIds([node]);
    const allOn = ids.every((id) => selected.has(id));
    const next = new Set(selected);
    ids.forEach((id) => allOn ? next.delete(id) : next.add(id));
    setSelected(next);
  };

  const markAll = () => setSelected(new Set(allIds));
  const unmarkAll = () => setSelected(new Set());
  const toggleExpand = (id) => {
    const next = new Set(expanded);
    next.has(id) ? next.delete(id) : next.add(id);
    setExpanded(next);
  };

  const fLower = filter.trim().toLowerCase();
  const matchesFilter = (node) => {
    if (!fLower) return true;
    if (node.type === 'file') return node.name.toLowerCase().includes(fLower);
    return node.children?.some(matchesFilter);
  };

  const totalFiles = allIds.length;
  const selCount = selected.size;

  return (
    <div onClick={onCancel} style={{
      position: 'fixed', inset: 0, zIndex: 250,
      background: 'rgba(8,12,20,0.72)',
      backdropFilter: 'blur(6px)', WebkitBackdropFilter: 'blur(6px)',
      display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 24,
    }}>
      <div onClick={(e) => e.stopPropagation()} style={{
        width: 760, maxHeight: '88vh',
        display: 'flex', flexDirection: 'column',
        background: 'var(--bg-1)',
        border: '1px solid var(--border-strong)',
        borderRadius: 12,
        boxShadow: '0 24px 64px rgba(0,0,0,0.5)',
        overflow: 'hidden',
      }}>
        {/* Header */}
        <div style={{
          padding: '18px 22px 14px',
          display: 'flex', alignItems: 'flex-start', gap: 10,
        }}>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 2, flex: 1 }}>
            <div style={{ display: 'flex', alignItems: 'baseline', gap: 10 }}>
              <span style={{ fontSize: 18, fontWeight: 600, color: 'var(--fg)' }}>Review selected files</span>
              <span style={{ fontSize: 11, color: 'var(--fg-subtle)' }}>All changes · {totalFiles} files</span>
            </div>
          </div>
          <button onClick={onCancel} style={{
            display: 'inline-flex', alignItems: 'center', gap: 4,
            padding: '4px 8px', borderRadius: 4,
            background: 'var(--bg-2)', color: 'var(--fg-muted)',
            border: '1px solid var(--border)',
            fontSize: 10, fontWeight: 600, letterSpacing: '0.04em', fontFamily: 'var(--font-mono)', cursor: 'pointer',
          }}>Esc</button>
        </div>

        {/* Mark / Unmark / count */}
        <div style={{
          padding: '0 22px 12px',
          display: 'flex', alignItems: 'center', gap: 14,
          fontSize: 12, color: 'var(--fg-muted)',
        }}>
          <button onClick={markAll}   style={lnkBtn}>Mark all</button>
          <span style={{ color: 'var(--fg-faint)' }}>·</span>
          <button onClick={unmarkAll} style={lnkBtn}>Unmark all</button>
          <div style={{ flex: 1 }} />
          <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--fg-muted)' }}>{selCount} selected</span>
        </div>

        {/* Filter */}
        <div style={{ padding: '0 22px 10px', position: 'relative' }}>
          <div style={{
            display: 'flex', alignItems: 'center', gap: 8,
            padding: '8px 12px',
            background: 'var(--bg-0)', border: '1px solid var(--border)', borderRadius: 6,
          }}>
            <i className="ph ph-magnifying-glass" style={{ fontSize: 12, color: 'var(--fg-subtle)' }} />
            <input
              ref={filterRef}
              value={filter} onChange={(e) => setFilter(e.target.value)}
              placeholder="Filter files…"
              style={{
                flex: 1, background: 'transparent', border: 0, outline: 0,
                color: 'var(--fg)', fontSize: 12, fontFamily: 'inherit',
              }}
            />
            <span style={{
              fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--fg-subtle)',
              padding: '2px 6px', borderRadius: 4, border: '1px solid var(--border)',
            }}>/</span>
          </div>
        </div>

        {/* Tree header */}
        <div style={{
          padding: '8px 22px',
          display: 'flex', alignItems: 'center',
          fontSize: 11, color: 'var(--fg-subtle)',
          borderTop: '1px solid var(--rule)',
        }}>
          <span>{totalFiles} files</span>
          <div style={{ flex: 1 }} />
          <span style={{ fontFamily: 'var(--font-mono)' }}>{selCount} selected</span>
        </div>

        {/* Tree */}
        <div style={{
          flex: 1, overflowY: 'auto',
          padding: '0 12px 8px',
        }}>
          {tree.map((node) => matchesFilter(node) && (
            <TreeNode key={node.id} node={node}
              selected={selected} expanded={expanded}
              onToggleFile={toggleFile}
              onToggleDir={toggleDir}
              onToggleExpand={toggleExpand}
              filter={fLower} />
          ))}
        </div>

        {/* Footer */}
        <div style={{
          padding: '14px 22px',
          borderTop: '1px solid var(--border)',
          background: 'var(--bg-0)',
          display: 'flex', alignItems: 'center', gap: 10,
        }}>
          <div style={{ flex: 1 }} />
          <button onClick={onCancel} style={fpBtnSec}>CANCEL</button>
          <button
            onClick={() => onConfirm(Array.from(selected))}
            disabled={selCount === 0}
            style={{
              ...fpBtnPri,
              opacity: selCount === 0 ? 0.5 : 1,
              cursor: selCount === 0 ? 'not-allowed' : 'pointer',
            }}>
            <i className="ph-bold ph-check" style={{ fontSize: 11 }} />
            USE SELECTION
          </button>
        </div>
      </div>
    </div>
  );
}

function TreeNode({ node, selected, expanded, onToggleFile, onToggleDir, onToggleExpand, filter }) {
  if (node.type === 'dir') {
    const ids = collectFileIds([node]);
    const allOn = ids.length > 0 && ids.every((id) => selected.has(id));
    const someOn = !allOn && ids.some((id) => selected.has(id));
    const isOpen = expanded.has(node.id);
    return (
      <div>
        <div style={{
          display: 'flex', alignItems: 'center', gap: 8,
          padding: '6px 10px',
          paddingLeft: 10 + node.depth * 16,
          borderRadius: 4,
          cursor: 'pointer',
        }}
          onClick={() => onToggleExpand(node.id)}
        >
          <Checkbox state={allOn ? 'on' : someOn ? 'indeterminate' : 'off'} onClick={(e) => { e.stopPropagation(); onToggleDir(node); }} />
          <i className={`ph ph-caret-${isOpen ? 'down' : 'right'}`} style={{ fontSize: 10, color: 'var(--fg-subtle)' }} />
          <i className="ph ph-folder" style={{ fontSize: 12, color: 'var(--fg-muted)' }} />
          <span style={{ fontSize: 12, color: 'var(--fg)', fontFamily: 'var(--font-mono)' }}>{node.path}</span>
        </div>
        {isOpen && node.children?.map((child) => (
          <TreeNode key={child.id} node={child}
            selected={selected} expanded={expanded}
            onToggleFile={onToggleFile}
            onToggleDir={onToggleDir}
            onToggleExpand={onToggleExpand}
            filter={filter} />
        ))}
      </div>
    );
  }

  // File row
  const on = selected.has(node.id);
  const extColor = node.ext === 'svelte' ? '#ff7a00' :
                   node.ext === 'ts'     ? '#5750ee' :
                   'var(--fg-subtle)';
  const extLabel = node.ext === 'svelte' ? 'sv' : node.ext;
  return (
    <div
      onClick={() => onToggleFile(node.id)}
      onMouseOver={(e) => { e.currentTarget.style.background = 'var(--bg-2)'; }}
      onMouseOut={(e)  => { e.currentTarget.style.background = 'transparent'; }}
      style={{
        display: 'flex', alignItems: 'center', gap: 8,
        padding: '6px 10px',
        paddingLeft: 10 + 2 * 16 + 22,    // file rows always indent past the dir caret
        borderRadius: 4,
        cursor: 'pointer',
        transition: 'background var(--d-fast) var(--ease)',
      }}>
      <Checkbox state={on ? 'on' : 'off'} />
      <span style={{
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        width: 18, height: 18, borderRadius: 4,
        background: extColor + '22', color: extColor,
        fontSize: 9, fontWeight: 700, fontFamily: 'var(--font-mono)',
        textTransform: 'lowercase',
      }}>{extLabel}</span>
      <span style={{
        flex: 1, fontSize: 12, color: 'var(--fg)', fontFamily: 'var(--font-mono)',
        whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
      }}>{node.name}</span>
      {node.comments > 0 && (
        <span title={`${node.comments} comment`} style={{
          width: 16, height: 16, borderRadius: '50%',
          background: 'rgba(127,135,255,0.18)', color: 'var(--periwinkle)',
          fontSize: 9, fontWeight: 700,
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        }}>?</span>
      )}
      <span style={{ display: 'inline-flex', gap: 6, fontSize: 11, fontFamily: 'var(--font-mono)' }}>
        {node.add > 0 && <span style={{ color: 'var(--ok)' }}>+{node.add}</span>}
        {node.del > 0 && <span style={{ color: 'var(--err)' }}>-{node.del}</span>}
      </span>
    </div>
  );
}

function Checkbox({ state, onClick }) {
  // Reshape brand uses Orange for selection checkboxes in the existing file picker.
  const on = state === 'on';
  const ind = state === 'indeterminate';
  const bg = on || ind ? 'var(--orange, #ff7a00)' : 'transparent';
  const bd = on || ind ? 'var(--orange, #ff7a00)' : 'var(--border-strong)';
  return (
    <span onClick={onClick} style={{
      width: 16, height: 16, borderRadius: 4,
      background: bg, border: '1.5px solid ' + bd,
      display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
      flexShrink: 0, cursor: onClick ? 'pointer' : 'inherit',
    }}>
      {on && <i className="ph-bold ph-check" style={{ fontSize: 10, color: '#fff' }} />}
      {ind && <span style={{ width: 8, height: 2, background: '#fff', borderRadius: 1 }} />}
    </span>
  );
}

const lnkBtn = {
  background: 'transparent', border: 0,
  color: 'var(--fg-muted)', fontSize: 12, fontFamily: 'inherit', cursor: 'pointer',
  padding: 0,
};
const fpBtnPri = {
  display: 'inline-flex', alignItems: 'center', gap: 6,
  height: 32, padding: '0 14px', borderRadius: 6,
  background: 'var(--orange, #ff7a00)', color: '#fff',
  border: 0, fontSize: 11, fontWeight: 700, letterSpacing: '0.06em', fontFamily: 'inherit',
};
const fpBtnSec = {
  display: 'inline-flex', alignItems: 'center', gap: 6,
  height: 32, padding: '0 14px', borderRadius: 6,
  background: 'transparent', color: 'var(--fg-muted)',
  border: 0, fontSize: 11, fontWeight: 700, letterSpacing: '0.06em', fontFamily: 'inherit', cursor: 'pointer',
};

Object.assign(window, { FilePicker });
