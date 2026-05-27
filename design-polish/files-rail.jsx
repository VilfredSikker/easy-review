// files-rail.jsx — file tree for the active branch.
// Improvements: filter bar with hint, sticky breadcrumb at the bottom showing
// the active file's path, file rows with diff bars (proportional add/del),
// review-progress meter at the top to make "4/7 reviewed" tangible.

const { useState: useFR } = React;

function FilesRail({ files, density, viewSource, onViewSource }) {
  const [filter, setFilter] = useFR('');
  const [selectedCommit, setSelectedCommit] = useFR('all');
  const reviewed = 4, total = 7;
  return (
    <aside style={{
      width: 280,
      background: 'var(--bg-1)',
      borderRight: '1px solid var(--border)',
      display: 'flex', flexDirection: 'column',
      flexShrink: 0,
    }}>
      {/* Filter */}
      <div style={{ padding: '10px 10px 8px' }}>
        <div style={{
          display: 'flex', alignItems: 'center', gap: 6,
          height: 28, padding: '0 8px',
          background: 'var(--bg-0)', borderRadius: 6,
          border: '1px solid var(--border)',
        }}>
          <i className="ph ph-funnel" style={{ fontSize: 12, color: 'var(--fg-subtle)' }} />
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter files…"
            style={{
              flex: 1, minWidth: 0, border: 0, background: 'transparent',
              fontSize: 12, color: 'var(--fg)', outline: 'none',
            }}
          />
          <span className="kbd">/</span>
        </div>
      </div>

      {/* Files header — scope label + review-progress strip */}
      <div style={{ padding: '0 14px 8px', display: 'flex', flexDirection: 'column', gap: 4 }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
          <span style={{ fontSize: 11, color: 'var(--fg-muted)', display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            {selectedCommit === 'all' ? (
              <><i className="ph ph-stack" style={{ fontSize: 11, color: 'var(--fg-subtle)' }} />{total} files</>
            ) : (
              <>
                <span style={commitChipStyle(true)}>{selectedCommit.slice(0, 7)}</span>
                <span>{(window.DATA_COMMITS.find((c) => c.sha === selectedCommit) || {}).files} files</span>
              </>
            )}
          </span>
          <span style={{ fontSize: 11, color: 'var(--fg-subtle)' }}>{reviewed}/{total} reviewed</span>
        </div>
        <div style={{ display: 'flex', gap: 2 }}>
          {Array.from({ length: total }).map((_, i) => (
            <span key={i} style={{
              flex: 1, height: 3, borderRadius: 2,
              background: i < reviewed ? 'var(--periwinkle)' : 'rgba(255,255,255,0.06)',
              transition: 'background var(--d-base) var(--ease)',
            }} />
          ))}
        </div>
      </div>

      {/* File list */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '4px 6px', minHeight: 80 }}>
        {files
          .filter((f) => !filter || (f.name||f.path||'').toLowerCase().includes(filter.toLowerCase()))
          .map((f, idx) => (
            f.kind === 'dir'
              ? <DirRow key={idx} f={f} />
              : <FileRow key={idx} f={f} dimmed={selectedCommit !== 'all' && !shouldShowInCommit(f, selectedCommit)} />
          ))
        }
      </div>

      {/* Commits — pick "All changes" or a specific commit to filter the diff scope */}
      <CommitsPanel
        commits={window.DATA_COMMITS}
        selected={selectedCommit}
        onSelect={setSelectedCommit}
      />
    </aside>
  );
}

// Loose heuristic — in the mock, alternate files between commits so the dim
// state has something to do.
function shouldShowInCommit(file, sha) {
  if (file.kind !== 'file') return true;
  const i = file.name.charCodeAt(0) % 5;
  const list = window.DATA_COMMITS.map((c) => c.sha);
  return list.indexOf(sha) === i % list.length;
}

const commitChipStyle = (active) => ({
  fontFamily: 'var(--font-mono)',
  fontSize: 10,
  color: active ? 'var(--orange)' : 'var(--fg-muted)',
  background: active ? 'var(--orange-tint)' : 'var(--bg-2)',
  border: `1px solid ${active ? 'rgba(255,122,43,0.3)' : 'var(--border)'}`,
  borderRadius: 4, padding: '1px 5px', lineHeight: 1.3,
});

function CommitsPanel({ commits, selected, onSelect }) {
  const [collapsed, setCollapsed] = useFR(false);
  return (
    <div style={{
      borderTop: '1px solid var(--border)',
      background: 'var(--bg-0)',
      display: 'flex', flexDirection: 'column',
      maxHeight: collapsed ? 30 : 280,
      flexShrink: 0,
      transition: 'max-height var(--d-base) var(--ease)',
      overflow: 'hidden',
    }}>
      <button
        onClick={() => setCollapsed((v) => !v)}
        style={{
          display: 'flex', alignItems: 'center', gap: 6,
          padding: '6px 12px', border: 0, background: 'transparent',
          color: 'var(--fg-subtle)', textAlign: 'left', cursor: 'pointer',
        }}>
        <i className="ph ph-git-commit" style={{ fontSize: 11 }} />
        <span style={{ fontSize: 10, letterSpacing: '0.06em', fontWeight: 600, textTransform: 'uppercase' }}>
          Commits
        </span>
        <span style={{
          fontSize: 9, padding: '0 5px', borderRadius: 999,
          background: 'rgba(255,255,255,0.06)', color: 'var(--fg-muted)',
        }}>{commits.length}</span>
        <div style={{ flex: 1 }} />
        <i className={`ph ph-caret-${collapsed ? 'up' : 'down'}`} style={{ fontSize: 10 }} />
      </button>
      <div style={{ overflowY: 'auto', paddingBottom: 4 }}>
        <CommitRow
          isAll
          selected={selected === 'all'}
          onSelect={() => onSelect('all')}
        />
        {commits.map((c) => (
          <CommitRow key={c.sha} commit={c}
            selected={selected === c.sha}
            onSelect={() => onSelect(c.sha)}
          />
        ))}
      </div>
    </div>
  );
}

function CommitRow({ commit, isAll, selected, onSelect }) {
  const [hover, setHover] = useFR(false);
  return (
    <div
      onClick={onSelect}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: 'relative',
        display: 'flex', alignItems: 'center', gap: 8,
        margin: '0 6px',
        padding: '5px 8px 5px 10px',
        borderRadius: 5,
        background: selected ? 'var(--bg-3)' : (hover ? 'rgba(255,255,255,0.03)' : 'transparent'),
        cursor: 'pointer',
        color: selected ? 'var(--fg)' : 'var(--fg-muted)',
        transition: 'background var(--d-fast) var(--ease)',
      }}>
      {selected && (
        <span style={{
          position: 'absolute', left: 0, top: 5, bottom: 5, width: 2,
          background: 'var(--orange)', borderRadius: '0 2px 2px 0',
        }} />
      )}
      {isAll ? (
        <>
          <i className="ph ph-stack" style={{ fontSize: 11, color: selected ? 'var(--orange)' : 'var(--fg-subtle)', flexShrink: 0 }} />
          <span style={{ fontSize: 12, fontWeight: selected ? 500 : 400 }}>All changes</span>
          <div style={{ flex: 1 }} />
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--add-fg)' }}>+464</span>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--del-fg)' }}>−16</span>
        </>
      ) : (
        <>
          <div style={{
            width: 18, height: 18, borderRadius: '50%',
            background: 'var(--orange)', color: '#fff',
            fontSize: 9, fontWeight: 700,
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            flexShrink: 0,
          }}>{commit.author}</div>
          <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', gap: 1 }}>
            <span style={{
              fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}>{commit.msg}</span>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <span style={commitChipStyle(selected)}>{commit.sha.slice(0, 7)}</span>
              <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>{commit.when}</span>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, color: 'var(--add-fg)' }}>+{commit.add}</span>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, color: 'var(--del-fg)' }}>−{commit.del}</span>
              {!commit.pushed && (
                <span title="Not pushed yet" style={{
                  fontSize: 9, color: 'var(--warn)',
                  border: '1px solid rgba(255,196,87,0.3)',
                  background: 'rgba(255,196,87,0.08)',
                  padding: '0 4px', borderRadius: 3,
                }}>local</span>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}

function DirRow({ f }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 6,
      padding: '4px 6px', paddingLeft: 6 + f.depth * 12,
      fontSize: 11, color: 'var(--fg-subtle)',
    }}>
      <i className="ph ph-caret-down" style={{ fontSize: 9 }} />
      <i className="ph ph-folder" style={{ fontSize: 11 }} />
      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{f.path}</span>
    </div>
  );
}

function FileRow({ f, dimmed }) {
  const [hover, setHover] = useFR(false);
  const active = !!f.active;
  const total = (f.add || 0) + (f.del || 0);
  return (
    <div
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: 'relative',
        display: 'flex', alignItems: 'center', gap: 6,
        padding: 'var(--row-pad-y, 4px) 6px',
        paddingLeft: 6 + f.depth * 12,
        borderRadius: 5,
        background: active ? 'var(--bg-4)' : (hover ? 'rgba(255,255,255,0.03)' : 'transparent'),
        cursor: 'pointer',
        color: active ? 'var(--fg)' : 'var(--fg-muted)',
        opacity: dimmed ? 0.35 : 1,
        transition: 'background var(--d-fast) var(--ease), opacity var(--d-base) var(--ease)',
      }}
    >
      {active && (
        <span style={{
          position: 'absolute', left: 0, top: 4, bottom: 4, width: 2,
          background: 'var(--periwinkle)', borderRadius: '0 2px 2px 0',
        }} />
      )}
      {/* file-type glyph */}
      <FileIcon ext={f.ext} />
      <span style={{
        fontSize: 12, flex: 1, minWidth: 0,
        overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
      }}>{f.name}</span>

      {/* comment count — a question mark badge */}
      {f.comments > 0 && (
        <span title={`${f.comments} local comment`} style={{
          width: 13, height: 13, borderRadius: 999,
          background: 'var(--comment-bg)', color: 'var(--comment-fg)',
          fontSize: 9, fontWeight: 700,
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          border: '1px solid var(--comment-border)',
        }}>?</span>
      )}

      {/* +X / -Y — text only; the bars were stealing space from the file name */}
      <span style={{
        display: 'inline-flex', alignItems: 'center', gap: 4,
        fontFamily: 'var(--font-mono)', fontSize: 10, flexShrink: 0,
      }}>
        <span style={{ color: 'var(--add-fg)' }}>+{f.add || 0}</span>
        <span style={{ color: 'var(--del-fg)' }}>−{f.del || 0}</span>
      </span>
    </div>
  );
}

function FileIcon({ ext }) {
  const map = {
    ts:     { color: '#5db4ff', label: 'TS' },
    tsx:    { color: '#5db4ff', label: 'TSX' },
    js:     { color: '#ffce5b', label: 'JS' },
    svelte: { color: '#ff7a2b', label: 'SV' },
    css:    { color: '#7f87ff', label: 'CSS' },
    md:     { color: '#aab0bd', label: 'MD' },
  };
  const m = map[ext] || { color: 'var(--fg-subtle)', label: '·' };
  return (
    <span style={{
      width: 14, height: 14, borderRadius: 3,
      background: `${m.color}22`, color: m.color,
      fontSize: 7, fontWeight: 700, letterSpacing: 0,
      display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
      flexShrink: 0,
    }}>{m.label}</span>
  );
}

Object.assign(window, { FilesRail });
