// right-rail.jsx — the right panel. Renders in one of three modes:
//   • tabs     — Branch / Review / Notes / Terminal as tabs (recommended default)
//   • stacked  — current behaviour, but with cleaner section chrome
//   • accordion— stacked + collapse-other so only one section is expanded at a time
//
// Inside each mode we render the same four blocks (BranchBlock, ReviewBlock,
// NotesBlock, ActionsBlock) so the content stays in lockstep.

const { useState: useRR } = React;

function RightRail({ mode, branch, ai, questions, density, terminalInRight, onTab, activeTab, onAskAI, collapsed, onExpand }) {
  if (collapsed) {
    return <RailCollapsed branch={branch} ai={ai} questions={questions} onExpand={onExpand} onTab={onTab} />;
  }
  if (mode === 'tabs') {
    return <RailTabs branch={branch} ai={ai} questions={questions} activeTab={activeTab} onTab={onTab} onAskAI={onAskAI} terminalInRight={terminalInRight} />;
  }
  if (mode === 'accordion') {
    return <RailAccordion branch={branch} ai={ai} questions={questions} onAskAI={onAskAI} terminalInRight={terminalInRight} />;
  }
  return <RailStacked branch={branch} ai={ai} questions={questions} onAskAI={onAskAI} />;
}

// ────────────────────────────────────────────────────────────────────────────
// COLLAPSED — 44px rail; status atoms stacked so you can still glance at the
// branch + CI + findings + notes without re-expanding the panel.
// ────────────────────────────────────────────────────────────────────────────
function RailCollapsed({ branch, ai, questions, onExpand, onTab }) {
  const totalFindings = ai.high + ai.med + ai.low;
  const ciAllPass = branch.ci.passed === branch.ci.total;
  return (
    <aside style={{
      width: 44, flexShrink: 0,
      background: 'var(--bg-1)',
      borderLeft: '1px solid var(--border)',
      display: 'flex', flexDirection: 'column',
      alignItems: 'center', padding: '8px 0', gap: 4,
    }}>
      {/* Status atoms — branch, github, ai review, notes — stacked top-to-bottom */}
      <CollapsedCell
        title={`Branch · ${branch.short}`}
        icon="ph-git-branch" iconColor="var(--orange)"
        badgeKind="info" badge={branch.status === 'draft' ? 'D' : null}
        onClick={() => { onExpand(); onTab('branch'); }}
      />

      {/* GitHub PR + CI + comments — one cell summarising github state */}
      <CollapsedCell
        title={`GitHub ${branch.pr} · ${branch.ci.passed}/${branch.ci.total} checks · ${branch.comments} comment${branch.comments !== 1 ? 's' : ''}`}
        icon="ph-github-logo"
        iconColor={ciAllPass ? 'var(--fg)' : 'var(--err)'}
        badgeKind={ciAllPass ? 'ok' : 'err'}
        badge={ciAllPass ? '✓' : '!'}
        sub={branch.comments > 0 ? branch.comments : null}
        onClick={() => { onExpand(); onTab('branch'); }}
      />

      {/* AI review findings */}
      <CollapsedCell
        title={`AI review · ${totalFindings} finding${totalFindings !== 1 ? 's' : ''}`}
        icon="ph-sparkle"
        iconColor={ai.high > 0 ? 'var(--err)' : ai.med > 0 ? 'var(--warn)' : 'var(--ok)'}
        badgeKind={ai.high > 0 ? 'err' : ai.med > 0 ? 'warn' : 'ok'}
        badge={totalFindings > 0 ? totalFindings : null}
        onClick={() => { onExpand(); onTab('review'); }}
      />

      {/* Notes */}
      <CollapsedCell
        title={`Notes · ${questions.length} local question${questions.length !== 1 ? 's' : ''}`}
        icon="ph-chat-circle"
        iconColor={questions.length > 0 ? 'var(--comment-fg)' : 'var(--fg-muted)'}
        badgeKind="warn"
        badge={questions.length > 0 ? questions.length : null}
        onClick={() => { onExpand(); onTab('notes'); }}
      />

      <div style={{ flex: 1 }} />

      <button title="Diff settings" style={collapsedHeaderBtn}>
        <i className="ph ph-gear" style={{ fontSize: 13 }} />
      </button>
    </aside>
  );
}

function CollapsedCell({ title, icon, iconColor, badge, badgeKind, sub, onClick }) {
  const [hover, setHover] = useRR(false);
  const badgeColor = ({
    ok:   { bg: 'rgba(78,201,164,0.18)', fg: 'var(--ok)' },
    warn: { bg: 'rgba(255,196,87,0.18)', fg: 'var(--warn)' },
    err:  { bg: 'rgba(255,107,107,0.18)', fg: 'var(--err)' },
    info: { bg: 'rgba(127,135,255,0.18)', fg: 'var(--periwinkle)' },
  })[badgeKind] || { bg: 'rgba(255,255,255,0.08)', fg: 'var(--fg-muted)' };
  return (
    <button
      title={title}
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: 'relative',
        width: 32, height: 32, borderRadius: 7,
        border: 0,
        background: hover ? 'var(--bg-3)' : 'transparent',
        color: 'var(--fg)',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        transition: 'background var(--d-fast) var(--ease)',
        cursor: 'pointer',
      }}>
      <i className={`ph ${icon}`} style={{ fontSize: 15, color: iconColor }} />
      {badge != null && (
        <span style={{
          position: 'absolute', top: -2, right: -2,
          minWidth: 14, height: 14, padding: '0 3px',
          borderRadius: 999, fontSize: 9, fontWeight: 700,
          background: badgeColor.bg, color: badgeColor.fg,
          border: '2px solid var(--bg-1)',
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          lineHeight: 1,
        }}>{badge}</span>
      )}
    </button>
  );
}

const collapsedHeaderBtn = {
  width: 32, height: 28, borderRadius: 7, border: 0,
  background: 'transparent', color: 'var(--fg-muted)',
  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
};

// ────────────────────────────────────────────────────────────────────────────
// Mode 1 — TABS  (the recommended default)
// ────────────────────────────────────────────────────────────────────────────
function RailTabs({ branch, ai, questions, activeTab, onTab, onAskAI, terminalInRight }) {
  const tabs = [
    { id: 'branch',   icon: 'ph-git-branch',      label: 'Branch',   badge: null },
    { id: 'review',   icon: 'ph-sparkle',         label: 'Review',   badge: ai.high + ai.med + ai.low > 0 ? ai.high + ai.med + ai.low : null },
    { id: 'notes',    icon: 'ph-chat-circle',     label: 'Notes',    badge: questions.length || null },
    ...(terminalInRight ? [{ id: 'terminal', icon: 'ph-terminal-window', label: 'Terminal', badge: null }] : []),
  ];

  return (
    <aside style={railStyle}>
      <div style={{
        display: 'flex',
        borderBottom: '1px solid var(--border)',
        background: 'var(--bg-1)',
        flexShrink: 0,
      }}>
        {tabs.map((t) => {
          const active = t.id === activeTab;
          return (
            <button key={t.id} onClick={() => onTab(t.id)} style={{
              position: 'relative', flex: 1,
              border: 0, background: 'transparent',
              padding: '10px 6px',
              color: active ? 'var(--fg)' : 'var(--fg-muted)',
              display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 5,
              fontSize: 11, fontWeight: 500,
              transition: 'color var(--d-fast) var(--ease)',
            }}
              onMouseOver={(e) => { if (!active) e.currentTarget.style.color = 'var(--fg)'; }}
              onMouseOut={(e)  => { if (!active) e.currentTarget.style.color = 'var(--fg-muted)'; }}>
              <i className={`ph ${t.icon}`} style={{ fontSize: 13, color: active ? 'var(--orange)' : 'inherit' }} />
              <span>{t.label}</span>
              {t.badge != null && (
                <span style={{
                  minWidth: 14, height: 14, padding: '0 4px',
                  borderRadius: 999, fontSize: 9, fontWeight: 700,
                  background: active ? 'var(--orange-tint-strong)' : 'rgba(255,255,255,0.08)',
                  color: active ? 'var(--orange)' : 'var(--fg-muted)',
                  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
                }}>{t.badge}</span>
              )}
              {active && (
                <span style={{
                  position: 'absolute', left: 12, right: 12, bottom: 0, height: 2,
                  background: 'var(--orange)', borderRadius: '2px 2px 0 0',
                }} />
              )}
            </button>
          );
        })}
      </div>
      <div style={{ flex: 1, overflowY: 'auto' }}>
        {activeTab === 'branch'   && <BranchPanel branch={branch} expanded />}
        {activeTab === 'review'   && <ReviewPanel ai={ai} expanded />}
        {activeTab === 'notes'    && <NotesPanel questions={questions} onAskAI={onAskAI} expanded />}
        {activeTab === 'terminal' && <TerminalEmbedded />}
      </div>
    </aside>
  );
}

// ────────────────────────────────────────────────────────────────────────────
// Mode 2 — STACKED (the current pattern, polished)
// ────────────────────────────────────────────────────────────────────────────
function RailStacked({ branch, ai, questions, onAskAI }) {
  return (
    <aside style={railStyle}>
      <div style={{ flex: 1, overflowY: 'auto' }}>
        <Section label="Branch" icon="ph-git-branch" defaultOpen>
          <BranchPanel branch={branch} compact />
        </Section>
        <Section label="AI Review" icon="ph-sparkle" badge="fresh" defaultOpen>
          <ReviewPanel ai={ai} compact />
        </Section>
        <Section label="Questions" icon="ph-chat-circle" badge="private" badgeKind="info" defaultOpen>
          <NotesPanel questions={questions} onAskAI={onAskAI} compact />
        </Section>
      </div>
    </aside>
  );
}

// ────────────────────────────────────────────────────────────────────────────
// Mode 3 — ACCORDION (one section expanded at a time)
// ────────────────────────────────────────────────────────────────────────────
function RailAccordion({ branch, ai, questions, onAskAI }) {
  const [open, setOpen] = useRR('branch');
  return (
    <aside style={railStyle}>
      <div style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column' }}>
        <Section label="Branch" icon="ph-git-branch" open={open === 'branch'} onToggle={() => setOpen('branch')} grow>
          <BranchPanel branch={branch} compact />
        </Section>
        <Section label="AI Review" icon="ph-sparkle" badge="fresh" open={open === 'review'} onToggle={() => setOpen('review')} grow>
          <ReviewPanel ai={ai} compact />
        </Section>
        <Section label="Questions" icon="ph-chat-circle" badge="private" badgeKind="info" open={open === 'notes'} onToggle={() => setOpen('notes')} grow>
          <NotesPanel questions={questions} onAskAI={onAskAI} compact />
        </Section>
      </div>
    </aside>
  );
}

const railStyle = {
  width: 320,
  background: 'var(--bg-1)',
  borderLeft: '1px solid var(--border)',
  display: 'flex', flexDirection: 'column',
  flexShrink: 0,
};

// Shared collapsible section used in stacked + accordion modes.
function Section({ label, icon, badge, badgeKind, defaultOpen, open: openProp, onToggle, grow, children }) {
  const [_open, _setOpen] = useRR(defaultOpen ?? true);
  const open = openProp == null ? _open : openProp;
  const setOpen = onToggle || _setOpen;
  return (
    <div style={{
      borderBottom: '1px solid var(--border)',
      display: 'flex', flexDirection: 'column',
      minHeight: open && grow ? 0 : undefined,
      flex: open && grow ? 1 : '0 0 auto',
    }}>
      <button
        onClick={() => setOpen(!open)}
        style={{
          display: 'flex', alignItems: 'center', gap: 8,
          padding: '10px 14px',
          background: 'transparent', border: 0,
          color: 'var(--fg)', cursor: 'pointer',
          width: '100%', textAlign: 'left',
        }}>
        <i className={`ph ${icon}`} style={{ fontSize: 12, color: 'var(--fg-subtle)' }} />
        <span style={{
          fontSize: 10, letterSpacing: '0.06em',
          fontWeight: 600, textTransform: 'uppercase',
          color: 'var(--fg-subtle)',
        }}>{label}</span>
        {badge && (
          <span className={`pill pill--${badgeKind || 'ok'}`}>{badge}</span>
        )}
        <div style={{ flex: 1 }} />
        <i className={`ph ph-caret-${open ? 'down' : 'right'}`} style={{ fontSize: 10, color: 'var(--fg-subtle)' }} />
      </button>
      {open && (
        <div style={{ padding: '0 14px 14px', flex: grow ? 1 : '0 0 auto', overflowY: grow ? 'auto' : 'visible', minHeight: 0 }}>
          {children}
        </div>
      )}
    </div>
  );
}

// ────────────────────────────────────────────────────────────────────────────
// Block 1 — Branch & Git status
// (consolidates 3 floating pills into one StatusGrid + tightens the GitHub block)
// ────────────────────────────────────────────────────────────────────────────
function BranchPanel({ branch, compact, expanded }) {
  const pad = expanded ? 14 : 0;
  return (
    <div style={{ padding: pad, display: 'flex', flexDirection: 'column', gap: 14 }}>
      {/* Title row */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <i className="ph ph-git-branch" style={{ fontSize: 12, color: 'var(--orange)', flexShrink: 0 }} />
        <span style={{ fontSize: 12, color: 'var(--fg)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1, minWidth: 0 }}>
          {branch.short}
        </span>
        <span style={{ fontSize: 10, color: 'var(--fg-subtle)', whiteSpace: 'nowrap', flexShrink: 0 }}>{branch.reviewed}</span>
        <button title="Bookmark branch" style={iconBtnTiny}>
          <i className="ph ph-bookmark-simple" style={{ fontSize: 11 }} />
        </button>
      </div>

      {/* Base ref */}
      <div style={{ fontSize: 11, color: 'var(--fg-muted)', display: 'flex', alignItems: 'center', gap: 6 }}>
        <span style={{ color: 'var(--fg-subtle)' }}>base</span>
        <span style={{
          fontFamily: 'var(--font-mono)', background: 'var(--bg-0)',
          border: '1px solid var(--border)', borderRadius: 4, padding: '1px 6px',
        }}>{branch.base}</span>
        <i className="ph ph-arrow-left" style={{ fontSize: 10 }} />
        <span style={{
          fontFamily: 'var(--font-mono)', background: 'var(--bg-0)',
          border: '1px solid var(--border)', borderRadius: 4, padding: '1px 6px',
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', minWidth: 0,
        }}>{branch.short}</span>
      </div>

      {/* Changes meter */}
      <div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, color: 'var(--fg-muted)', marginBottom: 6 }}>
          <i className="ph ph-arrows-down-up" style={{ fontSize: 11, color: 'var(--fg-subtle)' }} />
          <span>Changes</span>
          <div style={{ flex: 1 }} />
          <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--add-fg)' }}>+{branch.changes.add}</span>
          <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--del-fg)' }}>−{branch.changes.del}</span>
        </div>
        <div style={{ height: 4, display: 'flex', gap: 2, borderRadius: 2, overflow: 'hidden' }}>
          <span style={{ flex: branch.changes.add, background: 'var(--add-gutter)' }} />
          <span style={{ flex: branch.changes.del, background: 'var(--del-gutter)' }} />
        </div>
      </div>

      {/* GitHub status — consolidated. Was 3 pills + a heading + a CI line + counts. */}
      <div style={{
        background: 'var(--bg-0)', border: '1px solid var(--border)',
        borderRadius: 8, overflow: 'hidden',
      }}>
        <div style={{
          display: 'flex', alignItems: 'center', gap: 8,
          padding: '8px 10px',
          borderBottom: '1px solid var(--border)',
          background: 'var(--bg-2)',
        }}>
          <i className="ph ph-github-logo" style={{ fontSize: 13, color: 'var(--fg)' }} />
          <a href="#" style={{ fontSize: 12, color: 'var(--periwinkle)', fontWeight: 500 }}>{branch.pr}</a>
          <div style={{ flex: 1 }} />
          <button title="Sync" style={iconBtnTiny}><i className="ph ph-arrows-clockwise" style={{ fontSize: 11 }} /></button>
        </div>
        <div style={{ padding: '10px', display: 'flex', flexDirection: 'column', gap: 8 }}>
          {/* Status grid: state · review · mergeable */}
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 6 }}>
            <StatusCell icon="ph-pencil-simple" label="Status" value="Draft" kind="draft" />
            <StatusCell icon="ph-eye" label="Review" value="Required" kind="review" />
            <StatusCell icon="ph-git-merge" label="Mergeable" value="Yes" kind="ok" />
          </div>
          {/* CI status row */}
          <div style={{
            display: 'flex', alignItems: 'center', gap: 8,
            padding: '6px 8px', background: 'var(--bg-2)', borderRadius: 6,
          }}>
            <i className="ph-fill ph-check-circle" style={{ fontSize: 13, color: 'var(--ok)' }} />
            <span style={{ fontSize: 12, color: 'var(--fg)' }}>
              <strong style={{ fontWeight: 600 }}>{branch.ci.passed}/{branch.ci.total}</strong>
              <span style={{ color: 'var(--fg-muted)' }}> checks passing</span>
            </span>
            <div style={{ flex: 1 }} />
            <i className="ph ph-caret-down" style={{ fontSize: 10, color: 'var(--fg-subtle)' }} />
          </div>
          {/* Activity counts */}
          <div style={{ display: 'flex', gap: 12, fontSize: 11, color: 'var(--fg-muted)' }}>
            <span><i className="ph ph-chat-circle" style={{ fontSize: 11, marginRight: 4 }} />{branch.comments} comment</span>
            <span><i className="ph ph-eye" style={{ fontSize: 11, marginRight: 4 }} />{branch.reviews} reviews</span>
          </div>
        </div>
      </div>

      {/* Description (one-liner with a "Show description" toggle replaced by clear truncate+more) */}
      <div>
        <div style={{ fontSize: 10, letterSpacing: '0.06em', textTransform: 'uppercase', color: 'var(--fg-subtle)', fontWeight: 600, marginBottom: 4 }}>
          Description
        </div>
        <div style={{ fontSize: 12, color: 'var(--fg)', lineHeight: 1.4 }}>
          Adds <code style={{ fontFamily: 'var(--font-mono)', fontSize: 11, background: 'var(--bg-2)', padding: '0 4px', borderRadius: 3 }}>swapExperimentReferenceForGroup</code> for in-place display updates… <a href="#" style={{ color: 'var(--periwinkle)', whiteSpace: 'nowrap' }}>Show all</a>
        </div>
      </div>

      {/* New comment composer — primary affordance, not buried */}
      <button style={{
        display: 'flex', alignItems: 'center', gap: 8,
        padding: '8px 10px', borderRadius: 6,
        background: 'var(--bg-0)', border: '1px solid var(--border)',
        color: 'var(--fg-subtle)', fontSize: 12, fontFamily: 'inherit',
        cursor: 'text', width: '100%', textAlign: 'left',
      }}>
        <i className="ph ph-chat-text" style={{ fontSize: 13 }} />
        <span>Comment or review…</span>
      </button>
    </div>
  );
}

function StatusCell({ icon, label, value, kind }) {
  const map = {
    draft:  { color: 'var(--fg-muted)' },
    review: { color: 'var(--err)' },
    ok:     { color: 'var(--ok)' },
  };
  const c = (map[kind] || {}).color;
  return (
    <div style={{
      background: 'var(--bg-2)', borderRadius: 6,
      padding: '6px 8px',
      display: 'flex', flexDirection: 'column', gap: 2,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 9,
        letterSpacing: '0.06em', textTransform: 'uppercase', color: 'var(--fg-subtle)', fontWeight: 600 }}>
        <i className={`ph ${icon}`} style={{ fontSize: 10 }} />
        {label}
      </div>
      <div style={{ fontSize: 12, color: c || 'var(--fg)', fontWeight: 500 }}>{value}</div>
    </div>
  );
}

// ────────────────────────────────────────────────────────────────────────────
// Block 2 — AI Review (the empty state is reframed as "ready to run")
// ────────────────────────────────────────────────────────────────────────────
function ReviewPanel({ ai, compact, expanded }) {
  const pad = expanded ? 14 : 0;
  const totalFindings = ai.high + ai.med + ai.low;
  return (
    <div style={{ padding: pad, display: 'flex', flexDirection: 'column', gap: 14 }}>
      {/* Severity grid — always shown so users learn the categories */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 6 }}>
        <SeverityCell color="var(--err)"  label="HIGH" value={ai.high} />
        <SeverityCell color="var(--warn)" label="MED"  value={ai.med} />
        <SeverityCell color="var(--info)" label="LOW"  value={ai.low} />
      </div>

      {totalFindings === 0 ? (
        <div style={{
          background: 'var(--bg-0)', border: '1px dashed var(--border-strong)', borderRadius: 8,
          padding: 14, display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: 10,
        }}>
          <span className="pill pill--ok">FRESH · 0 FINDINGS</span>
          <p style={{ margin: 0, fontSize: 12, color: 'var(--fg-muted)', lineHeight: 1.5 }}>
            Last review was clean. Re-run the AI review skill after pushing more changes, or open the raw output.
          </p>
          <div style={{ display: 'flex', gap: 6, marginTop: 2 }}>
            <button style={btnPrimary}>
              <i className="ph-bold ph-sparkle" style={{ fontSize: 11 }} />
              Re-run review
            </button>
            <button style={btnSecondary}>
              <i className="ph ph-folder-open" style={{ fontSize: 11 }} />
              Open .er/
            </button>
          </div>
        </div>
      ) : (
        <FindingsList findings={[]} />
      )}

      <div style={{ display: 'flex', gap: 6, color: 'var(--fg-subtle)', fontSize: 11 }}>
        <button style={btnGhost}>
          <i className="ph ph-copy" style={{ fontSize: 10 }} />
          Copy findings JSON
        </button>
        <button style={btnGhost}>
          <i className="ph ph-folder" style={{ fontSize: 10 }} />
          Reveal review files
        </button>
      </div>
    </div>
  );
}

function SeverityCell({ color, label, value }) {
  return (
    <div style={{
      background: 'var(--bg-2)', borderRadius: 6,
      padding: '8px 10px', display: 'flex', flexDirection: 'column', gap: 2,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 5, fontSize: 9, letterSpacing: '0.08em', color, fontWeight: 700 }}>
        <span style={{ width: 6, height: 6, borderRadius: '50%', background: color }} />
        {label}
      </div>
      <div style={{ fontSize: 18, color: 'var(--fg)', fontWeight: 600, fontFamily: 'var(--font-mono)' }}>
        {value}
      </div>
    </div>
  );
}

function FindingsList() {
  return <div style={{ fontSize: 12, color: 'var(--fg-muted)' }}>(no findings)</div>;
}

// ────────────────────────────────────────────────────────────────────────────
// Block 3 — Notes / Questions (private)
// ────────────────────────────────────────────────────────────────────────────
function NotesPanel({ questions, onAskAI, compact, expanded }) {
  const pad = expanded ? 14 : 0;
  return (
    <div style={{ padding: pad, display: 'flex', flexDirection: 'column', gap: 10 }}>
      <p style={{ margin: 0, fontSize: 11, color: 'var(--fg-subtle)', lineHeight: 1.5 }}>
        Local-only review notes. Promote to a GitHub comment to share, or route through an AI assistant.
      </p>

      {questions.map((q) => (
        <div key={q.id} style={{
          background: 'var(--bg-0)', border: '1px solid var(--border)', borderRadius: 8,
          padding: 10, display: 'flex', flexDirection: 'column', gap: 6,
        }}>
          <a href="#" style={{
            fontSize: 10, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)',
            display: 'inline-flex', alignItems: 'center', gap: 4,
          }}>
            <i className="ph ph-file-ts" style={{ fontSize: 10 }} />
            {q.file}<span style={{ color: 'var(--periwinkle)' }}>:{q.lines}</span>
          </a>
          <div style={{ fontSize: 12, color: 'var(--fg)' }}>{q.text}</div>
        </div>
      ))}

      <div style={{ display: 'flex', gap: 6, marginTop: 4 }}>
        <button onClick={onAskAI} style={btnPrimary}>
          <i className="ph-bold ph-sparkle" style={{ fontSize: 11 }} />
          Ask AI
        </button>
        <button style={btnSecondary}>
          <i className="ph ph-chat-circle" style={{ fontSize: 11 }} />
          Promote to comment
        </button>
      </div>
    </div>
  );
}

function TerminalEmbedded() {
  return (
    <div style={{
      padding: 12, fontFamily: 'var(--font-mono)', fontSize: 12,
      color: 'var(--fg)', display: 'flex', flexDirection: 'column', gap: 4,
      height: '100%', background: '#0a0e17',
    }}>
      <div style={{ color: 'var(--fg-subtle)', fontSize: 10 }}>worktrees/pr1137-fixes</div>
      <div><span style={{ color: 'var(--ok)' }}>pr1137-fixes</span> <span style={{ color: 'var(--periwinkle)' }}>claude/organism-subproperty-mixup-LgMlO</span> $ git status</div>
      <div style={{ color: 'var(--fg-muted)' }}>On branch claude/organism-subproperty-mixup-LgMlO</div>
      <div style={{ color: 'var(--fg-muted)' }}>nothing to commit, working tree clean</div>
      <div><span style={{ color: 'var(--ok)' }}>pr1137-fixes</span> $ <span style={{ background: 'var(--fg)', color: '#000', display: 'inline-block', width: 7, height: 14, verticalAlign: 'middle' }} /></div>
    </div>
  );
}

const iconBtnTiny = {
  width: 20, height: 20, borderRadius: 4, border: 0,
  background: 'transparent', color: 'var(--fg-muted)',
  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
};

const btnPrimary = {
  display: 'inline-flex', alignItems: 'center', gap: 6,
  height: 28, padding: '0 12px', borderRadius: 6,
  background: 'var(--periwinkle)', color: '#fff',
  border: 0, fontSize: 11, fontWeight: 600, fontFamily: 'inherit',
};
const btnSecondary = {
  display: 'inline-flex', alignItems: 'center', gap: 6,
  height: 28, padding: '0 12px', borderRadius: 6,
  background: 'var(--bg-2)', color: 'var(--fg)',
  border: '1px solid var(--border)', fontSize: 11, fontWeight: 500, fontFamily: 'inherit',
};
const btnGhost = {
  display: 'inline-flex', alignItems: 'center', gap: 5,
  height: 22, padding: '0 6px', borderRadius: 4,
  background: 'transparent', color: 'var(--fg-subtle)',
  border: 0, fontSize: 11, fontFamily: 'inherit',
};

Object.assign(window, { RightRail });
