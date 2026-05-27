// terminal.jsx — bottom drawer terminal. Shown when the user opens it on
// the active branch. Drag-handle on top for resize affordance, header strip
// with branch name and clear/close actions.

const { useState: useTM } = React;

function TerminalDrawer({ branch, onClose, height = 240 }) {
  return (
    <div style={{
      height,
      background: '#0a0e17',
      borderTop: '1px solid var(--border-strong)',
      display: 'flex', flexDirection: 'column',
      flexShrink: 0,
      position: 'relative',
    }}>
      {/* Drag-resize handle */}
      <div style={{
        position: 'absolute', top: -3, left: 0, right: 0, height: 6,
        cursor: 'row-resize',
      }} />

      {/* Header */}
      <div style={{
        height: 28,
        display: 'flex', alignItems: 'center', gap: 8,
        padding: '0 10px',
        background: 'var(--bg-1)',
        borderBottom: '1px solid var(--border)',
        fontSize: 11,
      }}>
        <i className="ph ph-terminal-window" style={{ fontSize: 12, color: 'var(--fg-subtle)' }} />
        <span style={{ color: 'var(--fg-muted)', fontFamily: 'var(--font-mono)' }}>
          {branch.short}
        </span>
        <span style={{ color: 'var(--fg-subtle)' }}>·</span>
        <span style={{ color: 'var(--fg-subtle)' }}>Insert: <code style={{ fontFamily: 'var(--font-mono)' }}>git checkout {branch.short}</code></span>
        <div style={{ flex: 1 }} />
        <button title="Split terminal" style={hdrBtn}>
          <i className="ph ph-columns" style={{ fontSize: 11 }} />
        </button>
        <button title="Clear" style={hdrBtn}>
          <i className="ph ph-eraser" style={{ fontSize: 11 }} />
        </button>
        <button title="Close terminal" onClick={onClose} style={hdrBtn}>
          <i className="ph ph-x" style={{ fontSize: 11 }} />
        </button>
      </div>

      {/* Body */}
      <div style={{
        flex: 1, overflowY: 'auto',
        padding: '10px 12px',
        fontFamily: 'var(--font-mono)', fontSize: 12,
        color: 'var(--fg)', lineHeight: 1.55,
      }}>
        <div style={{ color: 'var(--fg-muted)' }}>/Users/vilfredsikker/.claude/hooks/peon-ping/completions.bash:28: command not found: complete</div>
        <div style={{ color: 'var(--fg-muted)' }}>complete:13: command not found: compdef</div>
        <div style={{ height: 8 }} />
        <div>
          <span style={{ color: '#ff7eb9' }}>pr1137-fixes</span>{' '}
          <span style={{ color: 'var(--periwinkle)' }}>{branch.short}</span>{' '}
          <span style={{ color: 'var(--fg-muted)' }}>$ via</span>{' '}
          <span style={{ color: 'var(--ok)' }}>v1.3.11</span>
        </div>
        <div>
          <span style={{ color: 'var(--ok)' }}>{'>'}</span>{' '}
          <span style={{
            display: 'inline-block', width: 8, height: 14,
            background: 'var(--fg)', verticalAlign: 'middle',
            animation: 'caret-blink 1s steps(2) infinite',
          }} />
        </div>
      </div>

      <style>{`
        @keyframes caret-blink { 0%,49% { opacity: 1; } 50%,100% { opacity: 0; } }
      `}</style>
    </div>
  );
}

const hdrBtn = {
  width: 22, height: 20, border: 0, background: 'transparent',
  color: 'var(--fg-subtle)', borderRadius: 4,
  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
};

Object.assign(window, { TerminalDrawer });
