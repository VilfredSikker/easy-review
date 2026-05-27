// chrome.jsx — top app chrome: traffic lights, branch tabs row, and the
// context/branch-info bar that sits underneath. In "combined" mode the
// branch tabs share the row with the traffic lights (saves vertical space).

const { useState } = React;

// Tiny svg icon set — used in places Phosphor's stroke weight is too heavy.
function Glyph({ d, size = 12, stroke = 1.5 }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor"
         strokeWidth={stroke} strokeLinecap="round" strokeLinejoin="round">
      <path d={d} />
    </svg>
  );
}

function TrafficLights() {
  return (
    <div style={{ display: 'flex', gap: 8, alignItems: 'center', paddingLeft: 4 }}>
      <span style={dotStyle('#ff5f57')} />
      <span style={dotStyle('#febc2e')} />
      <span style={dotStyle('#28c840')} />
    </div>
  );
}
const dotStyle = (bg) => ({
  width: 12, height: 12, borderRadius: '50%', background: bg,
  boxShadow: '0 0 0 0.5px rgba(0,0,0,0.25) inset',
});

// One branch tab in the strip. Active tab gets the orange under-rule + brighter bg.
function BranchTab({ tab, active, onSelect, onClose, compact }) {
  const [hover, setHover] = useState(false);
  return (
    <div
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      onClick={() => onSelect(tab.id)}
      style={{
        position: 'relative',
        display: 'flex', alignItems: 'center', gap: 6,
        height: compact ? 28 : 32,
        padding: compact ? '0 8px 0 10px' : '0 10px 0 12px',
        background: active ? 'var(--bg-1)' : (hover ? 'rgba(255,255,255,0.03)' : 'transparent'),
        borderRadius: compact ? '6px 6px 0 0' : 8,
        cursor: 'pointer',
        minWidth: 0,
        maxWidth: 240,
        color: active ? 'var(--fg)' : 'var(--fg-muted)',
        transition: 'background var(--d-fast) var(--ease)',
      }}
    >
      <i className="ph ph-git-branch" style={{ fontSize: 12, color: active ? 'var(--orange)' : 'var(--fg-subtle)' }} />
      <span style={{
        fontSize: var12(), fontWeight: active ? 500 : 400,
        overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
      }}>{tab.label}</span>
      {tab.dirty && (
        <span title="Uncommitted changes" style={{
          width: 6, height: 6, borderRadius: '50%', background: 'var(--periwinkle)', flexShrink: 0,
        }} />
      )}
      {tab.comments > 0 && (
        <span style={{
          minWidth: 14, height: 14, padding: '0 4px',
          borderRadius: 999, fontSize: 9, fontWeight: 600,
          background: active ? 'var(--orange-tint-strong)' : 'rgba(255,255,255,0.08)',
          color: active ? 'var(--orange)' : 'var(--fg-muted)',
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          flexShrink: 0,
        }}>{tab.comments}</span>
      )}
      {(hover || active) && (
        <button
          onClick={(e) => { e.stopPropagation(); onClose(tab.id); }}
          aria-label="Close tab"
          style={{
            border: 0, background: 'transparent', color: 'var(--fg-subtle)',
            padding: 2, marginLeft: 2, borderRadius: 4, display: 'inline-flex',
          }}
          onMouseOver={(e) => { e.currentTarget.style.background = 'rgba(255,255,255,0.10)'; e.currentTarget.style.color = 'var(--fg)'; }}
          onMouseOut={(e)  => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--fg-subtle)'; }}
        >
          <i className="ph ph-x" style={{ fontSize: 10 }} />
        </button>
      )}
      {/* Active under-rule — orange, the brand's "current focus" signal */}
      {active && (
        <span style={{
          position: 'absolute', left: 8, right: 8, bottom: -1, height: 2,
          background: 'var(--orange)', borderRadius: '2px 2px 0 0',
        }} />
      )}
    </div>
  );
}
const var12 = () => 'var(--t-sm)';

// The strip of branch tabs.
function BranchTabStrip({ tabs, activeId, onSelect, onClose, onNew, compact, fill }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'flex-end',
      gap: compact ? 2 : 4,
      minWidth: 0, flex: fill ? 1 : 'unset',
      overflow: 'hidden',
    }}>
      {tabs.map((t) => (
        <BranchTab
          key={t.id} tab={t}
          active={t.id === activeId}
          onSelect={onSelect} onClose={onClose}
          compact={compact}
        />
      ))}
      <button
        onClick={onNew}
        title="Open another branch (⌘T)"
        style={{
          border: 0, background: 'transparent', color: 'var(--fg-subtle)',
          width: 24, height: compact ? 24 : 28, borderRadius: 6,
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          marginLeft: 2, flexShrink: 0,
        }}
        onMouseOver={(e) => { e.currentTarget.style.background = 'rgba(255,255,255,0.06)'; e.currentTarget.style.color = 'var(--fg)'; }}
        onMouseOut={(e)  => { e.currentTarget.style.background = 'transparent';            e.currentTarget.style.color = 'var(--fg-subtle)'; }}
      >
        <i className="ph ph-plus" style={{ fontSize: 12 }} />
      </button>
    </div>
  );
}

// macOS-style window chrome bar.
//   mode "combined":  [lights] [tab1 tab2 tab3 +]                    [window-buttons]
//   mode "separate":  [lights]                                       [window-buttons]
function WindowChrome({ mode, tabs, activeId, onSelect, onClose, onNew, onToggleLeftRail, onToggleRightRail, onCommandK }) {
  return (
    <div style={{
      height: 36,
      display: 'flex', alignItems: 'center', gap: 12,
      padding: '0 10px 0 12px',
      background: 'linear-gradient(180deg, #161c2b 0%, #131927 100%)',
      borderBottom: mode === 'combined' ? '1px solid var(--border)' : '1px solid var(--rule)',
      flexShrink: 0,
    }}>
      <TrafficLights />

      {/* Sidebar toggle — only in combined mode (lives in left-rail otherwise) */}
      {mode === 'combined' && (
        <button onClick={onToggleLeftRail} title="Toggle sidebar" style={chromeBtn}>
          <i className="ph ph-sidebar-simple" style={{ fontSize: 13 }} />
        </button>
      )}

      {mode === 'combined' ? (
        <BranchTabStrip
          tabs={tabs} activeId={activeId}
          onSelect={onSelect} onClose={onClose} onNew={onNew}
          compact fill
        />
      ) : (
        <div style={{ flex: 1, display: 'flex', justifyContent: 'center' }}>
          {/* In separate mode the chrome bar carries just the active branch label */}
          <SingleBranchPill tab={tabs.find((t) => t.id === activeId)} />
        </div>
      )}

      {/* Right cluster: command-k + right-rail toggle */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
        <button onClick={onCommandK} title="Command palette (⌘K)" style={{ ...chromeBtn, padding: '0 8px', gap: 6, width: 'auto' }}>
          <i className="ph ph-magnifying-glass" style={{ fontSize: 12 }} />
          <span className="kbd">⌘K</span>
        </button>
        <button onClick={onToggleRightRail} title="Toggle right panel" style={chromeBtn}>
          <i className="ph ph-sidebar-simple" style={{ fontSize: 13, transform: 'scaleX(-1)' }} />
        </button>
      </div>
    </div>
  );
}

const chromeBtn = {
  height: 24, width: 24, borderRadius: 6, border: 0, background: 'transparent',
  color: 'var(--fg-subtle)', display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
};

function SingleBranchPill({ tab }) {
  if (!tab) return null;
  return (
    <div style={{
      display: 'inline-flex', alignItems: 'center', gap: 6,
      height: 24, padding: '0 10px',
      background: 'var(--bg-1)', borderRadius: 999,
      border: '1px solid var(--border-strong)',
      fontSize: var12(),
    }}>
      <i className="ph ph-git-branch" style={{ fontSize: 12, color: 'var(--orange)' }} />
      <span>{tab.label}</span>
    </div>
  );
}

// Context bar — branch name (long), quick actions, view-mode toggles.
// Shown:  combined chrome  → this is the only place branch context lives below the strip
//         separate chrome  → renders as its own slimmer row
function ContextBar({ branch, onSplit, splitOpen, onTerminal, terminalOpen, viewSource, onViewSource, diffLayout, onDiffLayout }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 8,
      height: 36, padding: '0 12px',
      background: 'var(--bg-0)',
      borderBottom: '1px solid var(--border)',
      flexShrink: 0,
    }}>
      {/* Branch identity */}
      <i className="ph ph-git-branch" style={{ fontSize: 13, color: 'var(--orange)' }} />
      <span style={{ fontSize: var12(), fontWeight: 500, color: 'var(--fg)' }}>{branch.name}</span>
      <span style={{ fontSize: var12(), color: 'var(--fg-subtle)' }}>·</span>
      <span style={{ fontSize: var12(), color: 'var(--fg-subtle)' }}>
        base <span style={{ color: 'var(--fg-muted)' }}>{branch.base}</span>
      </span>
      <span style={{
        fontFamily: 'var(--font-mono)', fontSize: 10,
        color: 'var(--add-fg)', marginLeft: 6,
      }}>+{branch.changes.add}</span>
      <span style={{
        fontFamily: 'var(--font-mono)', fontSize: 10,
        color: 'var(--del-fg)',
      }}>−{branch.changes.del}</span>

      {/* Quick-action affordances — icon-only with tooltips for density. */}
      <div style={{ display: 'flex', gap: 2, marginLeft: 12 }}>
        <QuickAction icon="ph-copy"        label="Copy branch name" onClick={() => window.toast && window.toast.success('Branch name copied to clipboard')} />
        <QuickAction icon="ph-folder-open" label="Reveal worktree in Finder" />
        <QuickAction icon="ph-github-logo" label="Open PR #1137" badge="#1137" />
        <QuickAction icon="ph-terminal-window" label={terminalOpen ? 'Hide terminal' : 'Show terminal'} active={terminalOpen} onClick={onTerminal} />
        <QuickAction icon="ph-browser" label={splitOpen ? 'Close split view' : 'Open split view'} active={splitOpen} onClick={onSplit} />
      </div>

      <div style={{ flex: 1 }} />

      {/* Settings — view-mode preferences (unified/split, whitespace, wrap, etc.) for the whole diff */}
      <button title="Diff settings" style={{
        height: 24, width: 24, borderRadius: 6, border: 0,
        background: 'transparent', color: 'var(--fg-muted)',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
      }}
        onMouseOver={(e) => { e.currentTarget.style.background = 'rgba(255,255,255,0.06)'; e.currentTarget.style.color = 'var(--fg)'; }}
        onMouseOut={(e)  => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--fg-muted)'; }}>
        <i className="ph ph-gear" style={{ fontSize: 13 }} />
      </button>

      {/* Diff source toggle — pulled out of the right rail and made visible inline. */}
      <div role="tablist" style={{
        display: 'inline-flex', background: 'var(--bg-1)',
        border: '1px solid var(--border)', borderRadius: 6,
        padding: 2,
      }}>
        <SegBtn active={viewSource === 'pr'}    onClick={() => onViewSource('pr')}>PR diff</SegBtn>
        <SegBtn active={viewSource === 'local'} onClick={() => onViewSource('local')}>Local branch</SegBtn>
      </div>
    </div>
  );
}

function SegBtn({ active, onClick, children, title }) {
  return (
    <button onClick={onClick} title={title} style={{
      border: 0, height: 22, padding: '0 10px',
      borderRadius: 4,
      fontSize: 11, fontWeight: 500,
      background: active ? 'var(--bg-3)' : 'transparent',
      color: active ? 'var(--fg)' : 'var(--fg-muted)',
      transition: 'all var(--d-fast) var(--ease)',
      display: 'inline-flex', alignItems: 'center',
    }}>{children}</button>
  );
}

function QuickAction({ icon, label, badge, active, onClick }) {
  const [hover, setHover] = useState(false);
  return (
    <button
      title={label}
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: 'relative',
        height: 24, padding: badge ? '0 6px 0 6px' : 0, width: badge ? 'auto' : 24,
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 4,
        border: 0, borderRadius: 6,
        background: active ? 'var(--bg-3)' : (hover ? 'rgba(255,255,255,0.06)' : 'transparent'),
        color: active ? 'var(--periwinkle)' : 'var(--fg-muted)',
        transition: 'all var(--d-fast) var(--ease)',
      }}
    >
      <i className={`ph ${icon}`} style={{ fontSize: 13 }} />
      {badge && <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>{badge}</span>}
    </button>
  );
}

// Fix the broken ContextBar use of var.r2 — replace with inline string
// (kept above as the demonstration; here's the actually-used token)
Object.assign(window, { WindowChrome, ContextBar, BranchTabStrip, TrafficLights });
