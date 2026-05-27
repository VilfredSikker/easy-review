// diff-view.jsx — the main diff area, recognisable from the screenshot but
// with a few affordance fixes:
//   • File header carries collapse, prev/next-hunk, view-mode (unified/split)
//     and "mark as reviewed" together — not three loose icon buttons.
//   • Inline comment looks like a thread (anchored to the diff lines with a
//     spine line) rather than a yellow slab.
//   • Hovering a line reveals "+" to drop a new question / comment.

const { useState: useDV } = React;

function DiffView({ file, hunks, splitOpen, rightCollapsed, onToggleRightRail }) {
  return (
    <section style={{
      flex: 1, minWidth: 0,
      background: 'var(--bg-0)',
      display: 'flex', flexDirection: 'column',
      overflow: 'hidden',
    }}>
      {/* File header */}
      <DiffHeader file={file} rightCollapsed={rightCollapsed} onToggleRightRail={onToggleRightRail} />

      {/* Diff body */}
      <div style={{
        flex: 1, overflowY: 'auto',
        fontFamily: 'var(--font-mono)',
        fontSize: 'var(--code-font, 12px)', lineHeight: 'var(--code-line-h, 20px)',
      }}>
        {hunks.map((row, i) => {
          if (row.kind === 'meta')    return <MetaRow key={i} text={row.text} />;
          if (row.kind === 'comment') return <CommentRow key={i} row={row} />;
          return <CodeRow key={i} row={row} />;
        })}
      </div>
    </section>
  );
}

function DiffHeader({ file, rightCollapsed, onToggleRightRail }) {
  const [reviewed, setReviewed] = useDV(false);
  const [collapsed, setCollapsed] = useDV(false);
  return (
    <header style={{
      display: 'flex', alignItems: 'center', gap: 8,
      padding: '8px 12px',
      borderBottom: '1px solid var(--border)',
      background: 'var(--bg-1)',
      flexShrink: 0,
    }}>
      {/* Mark-reviewed checkbox — primary action */}
      <button
        onClick={() => setReviewed((v) => !v)}
        title={reviewed ? 'Marked reviewed' : 'Mark file reviewed'}
        style={{
          width: 16, height: 16, borderRadius: 4,
          border: reviewed ? '0' : '1px solid var(--border-strong)',
          background: reviewed ? 'var(--periwinkle)' : 'transparent',
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          padding: 0, color: '#fff',
        }}>
        {reviewed && <i className="ph-bold ph-check" style={{ fontSize: 10 }} />}
      </button>

      <button onClick={() => setCollapsed((v) => !v)} title="Collapse file" style={iconBtn}>
        <i className={`ph ph-caret-${collapsed ? 'right' : 'down'}`} style={{ fontSize: 11 }} />
      </button>

      {/* Path — monospace, prominent */}
      <div style={{
        fontFamily: 'var(--font-mono)', fontSize: 12,
        color: 'var(--fg-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
        flex: 1, minWidth: 0,
      }}>
        <span style={{ color: 'var(--fg-subtle)' }}>packages/discovery-platform/src/lib/components/property-editor/</span>
        <span style={{ color: 'var(--fg)', fontWeight: 500 }}>experiment-template-resolution.ts</span>
      </div>

      <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--add-fg)' }}>+90</span>
      <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--del-fg)' }}>−0</span>

      {/* Hunk nav */}
      <div style={{ display: 'inline-flex', gap: 2, marginLeft: 8 }}>
        <button title="Previous change (k)" style={iconBtn}><i className="ph ph-caret-up" style={{ fontSize: 11 }} /></button>
        <button title="Next change (j)"     style={iconBtn}><i className="ph ph-caret-down" style={{ fontSize: 11 }} /></button>
      </div>
      <button title="Open in VS Code" style={{ ...iconBtn, padding: '0 8px', width: 'auto', gap: 4, whiteSpace: 'nowrap' }}>
        <i className="ph ph-arrow-square-out" style={{ fontSize: 11 }} />
        <span style={{ fontSize: 11 }}>Open source</span>
      </button>
    </header>
  );
}

const iconBtn = {
  width: 22, height: 22, borderRadius: 5, border: 0,
  background: 'transparent', color: 'var(--fg-muted)',
  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
};
const miniSeg = {
  width: 22, height: 18, borderRadius: 3, border: 0,
  background: 'transparent', color: 'var(--fg-muted)',
  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
};

function CodeRow({ row }) {
  const [hover, setHover] = useDV(false);
  const sign = row.kind === 'add' ? '+' : row.kind === 'del' ? '−' : ' ';
  const bg =
    row.focus            ? 'var(--focus-bg)' :
    row.kind === 'add'   ? 'var(--add-bg)'   :
    row.kind === 'del'   ? 'var(--del-bg)'   : 'transparent';
  const gutter =
    row.focus            ? 'rgba(56,139,253,0.45)' :
    row.kind === 'add'   ? 'var(--add-gutter)'    :
    row.kind === 'del'   ? 'var(--del-gutter)'    : 'transparent';
  return (
    <div
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: 'relative',
        display: 'grid',
        gridTemplateColumns: '48px 14px 1fr',
        background: bg,
        borderLeft: `2px solid ${gutter}`,
      }}>
      <span style={{
        textAlign: 'right', padding: '0 8px',
        color: 'var(--fg-faint)', userSelect: 'none',
        background: row.kind === 'add' ? 'rgba(46,160,67,0.06)' :
                    row.kind === 'del' ? 'rgba(248,81,73,0.06)' : 'transparent',
      }}>{row.n}</span>
      <span style={{
        color: row.kind === 'add' ? 'var(--add-fg)' : row.kind === 'del' ? 'var(--del-fg)' : 'var(--fg-faint)',
        userSelect: 'none', textAlign: 'center',
      }}>{sign}</span>
      <span style={{
        whiteSpace: 'pre', overflow: 'hidden', color: 'var(--fg)',
        paddingRight: 16,
      }}>{colorize(row.text)}</span>

      {/* Hover affordance — click + to drop a comment on this line */}
      {hover && (
        <button title="Add comment on this line" style={{
          position: 'absolute', left: 38, top: '50%', transform: 'translateY(-50%)',
          width: 14, height: 14, padding: 0,
          border: 0, borderRadius: 3,
          background: 'var(--periwinkle)', color: '#fff',
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          boxShadow: '0 2px 6px rgba(0,0,0,0.4)',
          zIndex: 2,
        }}>
          <i className="ph-bold ph-plus" style={{ fontSize: 9 }} />
        </button>
      )}
    </div>
  );
}

function MetaRow({ text }) {
  return (
    <div style={{
      padding: '4px 12px 4px 64px',
      color: 'var(--fg-faint)',
      background: 'rgba(255,255,255,0.015)',
      borderTop: '1px solid var(--rule)',
      borderBottom: '1px solid var(--rule)',
      fontFamily: 'var(--font-mono)', fontSize: 11,
    }}>
      {text}
    </div>
  );
}

// Improved comment thread: anchored by a spine to the line range, action row
// reads left-to-right and only the destructive action is muted.
function CommentRow({ row }) {
  return (
    <div style={{
      display: 'grid',
      gridTemplateColumns: '48px 1fr',
      padding: '4px 0',
      borderLeft: '2px solid var(--comment-border)',
      background: 'var(--comment-bg)',
    }}>
      <span style={{ textAlign: 'right', padding: '4px 8px 0', color: 'var(--comment-fg)' }}>
        <i className="ph-fill ph-chat-circle" style={{ fontSize: 11 }} />
      </span>
      <div style={{ padding: '6px 16px 8px 6px', fontFamily: 'var(--font-ui)' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 11, color: 'var(--comment-fg)' }}>
          <span style={{ fontWeight: 600 }}>Local question</span>
          <span style={{ color: 'var(--fg-subtle)' }}>·</span>
          <span style={{ color: 'var(--fg-muted)' }}>lines {row.range}</span>
          <div style={{ flex: 1 }} />
          <span className="pill pill--warn" style={{ background: 'transparent', borderColor: 'rgba(255,196,87,0.4)' }}>
            <i className="ph ph-lock" style={{ fontSize: 9 }} /> Private
          </span>
          <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>won't push · {row.when || '3m'}</span>
        </div>
        <div style={{ display: 'flex', gap: 10, marginTop: 8 }}>
          <span style={{
            width: 22, height: 22, borderRadius: '50%',
            background: 'var(--orange)', color: '#fff',
            fontSize: 11, fontWeight: 700,
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            flexShrink: 0,
          }}>Y</span>
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 11, color: 'var(--fg-subtle)' }}>{row.author} · {row.when || '3m'}</div>
            <div style={{ fontSize: 13, color: 'var(--fg)', marginTop: 2 }}>{row.text}</div>
            <div style={{ display: 'flex', gap: 12, marginTop: 8, fontSize: 11, color: 'var(--fg-muted)' }}>
              <CommentAction icon="ph-arrow-bend-up-left" label="Reply" />
              <CommentAction icon="ph-sparkle" label="Ask AI" accent />
              <CommentAction icon="ph-check-circle" label="Validate with AI" />
              <CommentAction icon="ph-copy" label="Copy" />
              <CommentAction icon="ph-check" label="Resolve" />
              <CommentAction icon="ph-chat-circle" label="Promote to comment" />
              <div style={{ flex: 1 }} />
              <CommentAction icon="ph-trash" label="Delete" muted />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function CommentAction({ icon, label, accent, muted }) {
  return (
    <button style={{
      display: 'inline-flex', alignItems: 'center', gap: 4,
      border: 0, background: 'transparent', padding: '2px 4px',
      borderRadius: 4,
      color: accent ? 'var(--periwinkle)' : muted ? 'var(--fg-subtle)' : 'var(--fg-muted)',
      fontSize: 11,
    }}
      onMouseOver={(e) => { e.currentTarget.style.background = 'rgba(255,255,255,0.05)'; e.currentTarget.style.color = accent ? 'var(--periwinkle)' : 'var(--fg)'; }}
      onMouseOut={(e)  => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = accent ? 'var(--periwinkle)' : muted ? 'var(--fg-subtle)' : 'var(--fg-muted)'; }}>
      <i className={`ph ${icon}`} style={{ fontSize: 11 }} />
      <span>{label}</span>
    </button>
  );
}

function ReviewDoneRow() {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 10, justifyContent: 'center',
      padding: '14px 12px 20px',
      color: 'var(--fg-subtle)', fontSize: 12,
    }}>
      <span style={{ height: 1, flex: 1, background: 'var(--border)' }} />
      <span>End of file</span>
      <button style={{
        height: 26, padding: '0 12px', borderRadius: 6,
        background: 'var(--periwinkle)', color: '#fff',
        border: 0, fontSize: 11, fontWeight: 600,
        display: 'inline-flex', alignItems: 'center', gap: 6,
      }}>
        <i className="ph-bold ph-check" style={{ fontSize: 11 }} />
        Mark reviewed & next file
        <span className="kbd" style={{ background: 'rgba(0,0,0,0.25)', borderColor: 'rgba(0,0,0,0.3)' }}>U</span>
      </button>
      <span style={{ height: 1, flex: 1, background: 'var(--border)' }} />
    </div>
  );
}

// Very lightweight TS-flavoured tokenizer — just enough colour to match the
// look of the source screenshots without pulling Prism in.
function colorize(line) {
  if (!line) return line;
  const KW = /\b(interface|export|const|extends|as|return|import|from|type|function|class|let|var|new|if|else|for|while|in|of|implements|true|false|null|undefined)\b/g;
  const TYPE = /\b(TData|PropertyDataLike|BulkWellContext|ExperimentPropertyOption|PropertyData|PropertyType|SwapExperimentReferenceForGroupArgs|ApplyExperimentOptionToExistingGroupArgs|QUANTITY_PROPERTY_KEYS|resolveExperimentPropertyGroup|mergeExperimentVariantWithTemplateQuantity|swapExperimentReferenceForGroup|variableProperties|templateProperty)\b/g;
  const STR = /'[^']*'|"[^"]*"/g;
  const NUM = /\b\d+\b/g;
  const COMMENT = /(\/\/.*$)/g;

  // tokens with their color
  const parts = [];
  const tokens = [
    { re: COMMENT, color: 'var(--fg-subtle)', italic: true },
    { re: STR, color: '#9bd17f' },
    { re: KW, color: '#c490e6' },
    { re: TYPE, color: '#5fb7ff' },
    { re: NUM, color: '#ffa657' },
  ];
  let segments = [{ text: line, color: null }];
  for (const t of tokens) {
    const next = [];
    for (const seg of segments) {
      if (seg.color) { next.push(seg); continue; }
      let last = 0;
      seg.text.replace(t.re, (m, ...rest) => {
        const idx = rest[rest.length - 2];
        if (idx > last) next.push({ text: seg.text.slice(last, idx), color: null });
        next.push({ text: m, color: t.color, italic: t.italic });
        last = idx + m.length;
        return m;
      });
      if (last < seg.text.length) next.push({ text: seg.text.slice(last), color: null });
    }
    segments = next;
  }
  return segments.map((s, i) => (
    <span key={i} style={{ color: s.color || 'inherit', fontStyle: s.italic ? 'italic' : 'normal' }}>{s.text}</span>
  ));
}

Object.assign(window, { DiffView });
