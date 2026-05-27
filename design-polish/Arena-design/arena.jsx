// arena.jsx — AI Review Arena: full-screen overlay launched from the Review tab.
//
// Layout: header (agents · rounds · stats · close) over a 2-pane body:
//   • LEFT (60%) — "The Process" — switchable between 3 layouts:
//        bracket : rounds as columns; findings as cards that move/transform across rounds
//        matrix  : agents × findings vote table
//        funnel  : 4-stage vertical funnel (proposed → cross-checked → resolved → final)
//   • RIGHT (40%) — "The Final Truth" — the consolidated finding list (always visible)
//
// Selecting any finding on the left highlights it on the right and vice-versa.

const { useState: useAR, useMemo: useARMemo, useEffect: useAREff } = React;

function ArenaOverlay({ open, onClose, layoutMode = 'bracket', onLayoutMode, onNewRun }) {
  const [selected, setSelected] = useAR(null);

  // Close on ESC
  useAREff(() => {
    if (!open) return;
    const onKey = (e) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  if (!open) return null;
  const run = window.DATA_ARENA_RUN;
  const findings = window.DATA_ARENA_FINDINGS;
  const stats = window.ARENA_STATS;

  return (
    <div style={{
      position: 'fixed', inset: 0, zIndex: 100,
      background: 'rgba(8,12,20,0.78)',
      backdropFilter: 'blur(6px)', WebkitBackdropFilter: 'blur(6px)',
      display: 'flex', flexDirection: 'column',
    }}>
      <style>{`
        .arena-link-row { transition: background var(--d-fast) var(--ease); }
        .arena-link-row:hover { background: var(--bg-3); }
      `}</style>

      <ArenaHeader run={run} stats={stats}
        layoutMode={layoutMode} onLayoutMode={onLayoutMode}
        onClose={onClose} onNewRun={onNewRun} />

      <div style={{
        flex: 1, display: 'grid', gridTemplateColumns: 'minmax(0, 1fr) 420px',
        minHeight: 0, overflow: 'hidden',
      }}>
        <ArenaProcess layoutMode={layoutMode} findings={findings} run={run}
          selected={selected} onSelect={setSelected} />
        <ArenaFinalTruth findings={findings} stats={stats}
          selected={selected} onSelect={setSelected} />
      </div>
    </div>
  );
}

// ─── Header ───────────────────────────────────────────────────────────────
function ArenaHeader({ run, stats, layoutMode, onLayoutMode, onClose, onNewRun }) {
  const agents = run.agents.map((id) => window.AGENT_BY_ID[id]);
  return (
    <div style={{
      flexShrink: 0,
      padding: '14px 20px 12px',
      background: 'var(--bg-1)',
      borderBottom: '1px solid var(--border)',
      display: 'flex', flexDirection: 'column', gap: 10,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <i className="ph-fill ph-trophy" style={{ fontSize: 18, color: 'var(--periwinkle)' }} />
        <div style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
          <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--fg)' }}>AI Review Arena</div>
          <div style={{ fontSize: 11, color: 'var(--fg-subtle)' }}>
            {run.id} · started {run.startedAt} · {(run.durationMs/1000).toFixed(1)}s · {run.rounds.length} rounds
          </div>
        </div>
        <div style={{ flex: 1 }} />

        {/* Layout switcher */}
        <LayoutToggle value={layoutMode} onChange={onLayoutMode} />

        <button onClick={onNewRun} title="Start a new arena run" style={{
          display: 'inline-flex', alignItems: 'center', gap: 6,
          height: 30, padding: '0 12px', borderRadius: 6,
          background: 'var(--periwinkle)', color: '#fff',
          border: 0, fontSize: 11, fontWeight: 600, fontFamily: 'inherit', cursor: 'pointer',
        }}>
          <i className="ph-bold ph-plus" style={{ fontSize: 11 }} />
          New run
        </button>

        <button onClick={onClose} title="Close arena (Esc)" style={{
          width: 30, height: 30, borderRadius: 6, border: '1px solid var(--border)',
          background: 'var(--bg-2)', color: 'var(--fg-muted)',
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center', cursor: 'pointer',
        }}>
          <i className="ph ph-x" style={{ fontSize: 12 }} />
        </button>
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: 16, flexWrap: 'wrap' }}>
        {/* Agents involved */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: 10, color: 'var(--fg-subtle)', fontWeight: 600, letterSpacing: '0.06em', textTransform: 'uppercase' }}>Agents</span>
          <div style={{ display: 'flex', gap: 6 }}>
            {agents.map((a) => (
              <span key={a.id} title={`${a.name} · ${a.desc}`} style={{
                display: 'inline-flex', alignItems: 'center', gap: 5,
                padding: '3px 8px 3px 5px',
                background: 'var(--bg-0)', border: `1px solid ${a.color}55`,
                borderRadius: 999, fontSize: 10, fontWeight: 600, color: 'var(--fg)',
              }}>
                <AgentDot agent={a} size={12} />{a.name}
              </span>
            ))}
          </div>
        </div>

        {/* Stat bar */}
        <div style={{ flex: 1 }} />
        <StatChip label="Proposed" value={stats.proposed} color="var(--fg-muted)" />
        <StatChip label="Kept"      value={stats.verdicts.kept}      color="var(--ok)" dot />
        <StatChip label="Escalated" value={stats.verdicts.escalated} color="var(--err)" dot />
        <StatChip label="Merged"    value={stats.verdicts.merged}    color="var(--periwinkle)" dot />
        <StatChip label="Dropped"   value={stats.verdicts.dropped}   color="var(--fg-subtle)" dot strike />
      </div>
    </div>
  );
}

function StatChip({ label, value, color, dot, strike }) {
  return (
    <div style={{
      display: 'inline-flex', alignItems: 'center', gap: 6,
      padding: '4px 10px', borderRadius: 6,
      background: 'var(--bg-0)', border: '1px solid var(--border)',
    }}>
      {dot && <span style={{ width: 6, height: 6, borderRadius: '50%', background: color }} />}
      <span style={{ fontSize: 10, color: 'var(--fg-subtle)', fontWeight: 600, letterSpacing: '0.06em', textTransform: 'uppercase' }}>{label}</span>
      <span style={{
        fontSize: 13, fontWeight: 700, color, fontFamily: 'var(--font-mono)',
        textDecoration: strike ? 'line-through' : 'none',
      }}>{value}</span>
    </div>
  );
}

function LayoutToggle({ value, onChange }) {
  const opts = [
    { id: 'bracket', label: 'Bracket', icon: 'ph-tree-structure' },
    { id: 'matrix',  label: 'Matrix',  icon: 'ph-grid-four' },
    { id: 'funnel',  label: 'Funnel',  icon: 'ph-funnel' },
  ];
  return (
    <div style={{
      display: 'inline-flex', padding: 3, borderRadius: 6,
      background: 'var(--bg-0)', border: '1px solid var(--border)',
    }}>
      {opts.map((o) => (
        <button key={o.id} onClick={() => onChange(o.id)}
          title={o.label}
          style={{
            display: 'inline-flex', alignItems: 'center', gap: 5,
            padding: '4px 9px', borderRadius: 4, border: 0,
            background: value === o.id ? 'var(--bg-3)' : 'transparent',
            color: value === o.id ? 'var(--fg)' : 'var(--fg-muted)',
            fontSize: 11, fontWeight: 500, fontFamily: 'inherit', cursor: 'pointer',
          }}>
          <i className={`ph ${o.icon}`} style={{ fontSize: 11 }} />{o.label}
        </button>
      ))}
    </div>
  );
}

// ─── Process panel — switches between bracket/matrix/funnel ────────────────
function ArenaProcess({ layoutMode, findings, run, selected, onSelect }) {
  return (
    <div style={{
      minHeight: 0, overflowY: 'auto', overflowX: 'auto',
      padding: 20, background: 'var(--bg-0)',
    }}>
      {layoutMode === 'bracket' && <BracketLayout findings={findings} run={run} selected={selected} onSelect={onSelect} />}
      {layoutMode === 'matrix'  && <MatrixLayout  findings={findings} run={run} selected={selected} onSelect={onSelect} />}
      {layoutMode === 'funnel'  && <FunnelLayout  findings={findings} run={run} selected={selected} onSelect={onSelect} />}
    </div>
  );
}

// ═══ Variant A: BRACKET ════════════════════════════════════════════════════
// Rounds as columns. Each round shows the findings as they existed after that
// round, with arrows between matching findings showing what happened.
function BracketLayout({ findings, run, selected, onSelect }) {
  // For each round, compute the set of findings + their state after that round.
  const rounds = run.rounds.map((r) => {
    return {
      ...r,
      cards: findings.map((f) => {
        const sev = f.severityByRound[r.n] || f.severityByRound[r.n - 1];
        // Determine state at end of this round
        const roundLog = f.rounds.find((rr) => rr.n === r.n);
        let state = 'present';
        if (r.n === 1) state = 'proposed';
        if (roundLog) {
          const votes = roundLog.log.map((l) => l.vote);
          if (votes.every((v) => v === 'drop' || v === 'abstain') && votes.includes('drop')) state = 'dropped';
          else if (votes.includes('merge')) state = 'merged';
          else if (votes.includes('escalate')) state = 'escalated';
          else if (votes.includes('lower')) state = 'lowered';
          else if (votes.includes('keep')) state = 'kept';
        }
        // After final round, force the verdict
        if (r.n === run.rounds.length) state = f.verdict;
        return { f, state, sev };
      }),
    };
  });

  return (
    <div style={{ display: 'flex', gap: 32, minHeight: '100%' }}>
      {rounds.map((round, idx) => (
        <div key={round.n} style={{ flex: '1 1 0', minWidth: 240, display: 'flex', flexDirection: 'column', gap: 12 }}>
          <RoundHeader round={round} />
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8, flex: 1 }}>
            {round.cards.map(({ f, state, sev }, i) => (
              <BracketCard key={f.id} f={f} state={state} sev={sev} round={round.n}
                selected={selected === f.id} onClick={() => onSelect(selected === f.id ? null : f.id)} />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

function RoundHeader({ round }) {
  return (
    <div style={{
      display: 'flex', flexDirection: 'column', gap: 4,
      padding: '0 0 10px',
      borderBottom: '1px solid var(--border)',
    }}>
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
        <span style={{
          fontFamily: 'var(--font-mono)', fontSize: 10,
          color: 'var(--fg-subtle)', fontWeight: 600,
        }}>R{round.n}</span>
        <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--fg)' }}>{round.name}</span>
      </div>
      <span style={{ fontSize: 11, color: 'var(--fg-muted)' }}>{round.desc}</span>
    </div>
  );
}

function BracketCard({ f, state, sev, round, selected, onClick }) {
  const stateMeta = {
    proposed:  { color: 'var(--fg-muted)',    label: 'PROPOSED',  bd: 'var(--border)' },
    kept:      { color: 'var(--ok)',          label: 'KEPT',      bd: 'rgba(78,201,164,0.34)' },
    escalated: { color: 'var(--err)',         label: 'ESCALATED', bd: 'rgba(255,107,107,0.40)' },
    lowered:   { color: 'var(--warn)',        label: 'LOWERED',   bd: 'rgba(255,196,87,0.36)' },
    merged:    { color: 'var(--periwinkle)',  label: 'MERGED',    bd: 'rgba(127,135,255,0.40)' },
    dropped:   { color: 'var(--fg-subtle)',   label: 'DROPPED',   bd: 'rgba(255,255,255,0.06)' },
    present:   { color: 'var(--fg-muted)',    label: '',          bd: 'var(--border)' },
  };
  const meta = stateMeta[state] || stateMeta.present;
  const dropped = state === 'dropped';
  const roundLog = f.rounds.find((r) => r.n === round);

  return (
    <div
      className="arena-card"
      onClick={onClick}
      style={{
        background: selected ? 'var(--bg-3)' : (dropped ? 'transparent' : 'var(--bg-1)'),
        border: `1px solid ${selected ? 'var(--periwinkle)' : meta.bd}`,
        borderRadius: 8, padding: 10,
        opacity: dropped ? 0.45 : 1,
        cursor: 'pointer',
        display: 'flex', flexDirection: 'column', gap: 6,
        position: 'relative',
        textDecoration: dropped ? 'line-through' : 'none',
      }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <span style={{ width: 6, height: 6, borderRadius: '50%', background: window.SEV_COLOR[sev], flexShrink: 0 }} />
        <span style={{
          fontSize: 9, fontWeight: 700, letterSpacing: '0.06em',
          color: meta.color,
        }}>{meta.label}</span>
        <div style={{ flex: 1 }} />
        <AgentStack ids={roundLog ? roundLog.log.map((l) => l.agent) : f.raisedBy} max={5} size={14} />
      </div>
      <div style={{
        fontSize: 12, color: 'var(--fg)', lineHeight: 1.4,
        textDecoration: dropped ? 'line-through' : 'none',
      }}>{f.title}</div>
      <div style={{
        fontSize: 10, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)',
        overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
      }}>{f.file}<span style={{ color: 'var(--periwinkle)' }}>:{f.line}</span></div>

      {/* Per-round votes inline */}
      {roundLog && round > 1 && (
        <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap', marginTop: 2, paddingTop: 6, borderTop: '1px dashed var(--rule)' }}>
          {roundLog.log.map((l, i) => <VoteChip key={i} log={l} />)}
        </div>
      )}
    </div>
  );
}

function VoteChip({ log }) {
  const a = window.AGENT_BY_ID[log.agent];
  if (!a) return null;
  const voteMeta = {
    propose:  { color: 'var(--fg-muted)',   icon: 'ph-plus-circle' },
    keep:     { color: 'var(--ok)',         icon: 'ph-check' },
    drop:     { color: 'var(--err)',        icon: 'ph-x' },
    merge:    { color: 'var(--periwinkle)', icon: 'ph-arrows-merge' },
    escalate: { color: 'var(--err)',        icon: 'ph-arrow-up' },
    lower:    { color: 'var(--warn)',       icon: 'ph-arrow-down' },
    abstain:  { color: 'var(--fg-faint)',   icon: 'ph-minus' },
    flag:     { color: 'var(--warn)',       icon: 'ph-flag' },
  };
  const m = voteMeta[log.vote] || voteMeta.abstain;
  return (
    <span title={`${a.name}: ${log.vote}${log.note ? ' — ' + log.note : ''}`} style={{
      display: 'inline-flex', alignItems: 'center', gap: 3,
      padding: '1px 5px 1px 2px', borderRadius: 999,
      background: 'var(--bg-2)', border: '1px solid var(--border)',
      fontSize: 9, color: m.color, fontWeight: 600,
    }}>
      <AgentDot agent={a} size={10} />
      <i className={`ph-bold ${m.icon}`} style={{ fontSize: 9 }} />
    </span>
  );
}

// ═══ Variant B: MATRIX ═════════════════════════════════════════════════════
// Findings as rows. Agents as columns. Cells show vote per agent (final round).
// Last column is the verdict.
function MatrixLayout({ findings, run, selected, onSelect }) {
  const agents = run.agents.map((id) => window.AGENT_BY_ID[id]);
  return (
    <div style={{
      background: 'var(--bg-1)', border: '1px solid var(--border)', borderRadius: 10,
      overflow: 'hidden',
    }}>
      <table style={{
        width: '100%', borderCollapse: 'collapse',
        fontSize: 11, color: 'var(--fg)',
      }}>
        <thead style={{ background: 'var(--bg-2)' }}>
          <tr>
            <th style={matrixH}>Finding</th>
            {agents.map((a) => (
              <th key={a.id} style={{ ...matrixH, textAlign: 'center', width: 64 }}>
                <div style={{ display: 'inline-flex', flexDirection: 'column', alignItems: 'center', gap: 3 }}>
                  <AgentDot agent={a} size={14} />
                  <span style={{ fontSize: 9, color: 'var(--fg-muted)' }}>{a.short}</span>
                </div>
              </th>
            ))}
            <th style={{ ...matrixH, textAlign: 'right', width: 130 }}>Verdict</th>
          </tr>
        </thead>
        <tbody>
          {findings.map((f) => {
            const isSel = selected === f.id;
            // Aggregate votes per agent across all rounds (latest wins)
            const lastVote = {};
            f.rounds.forEach((r) => r.log.forEach((l) => { lastVote[l.agent] = l; }));
            const sev = f.severityByRound[Math.max(...Object.keys(f.severityByRound).map(Number))];
            return (
              <tr key={f.id} className="arena-link-row"
                onClick={() => onSelect(isSel ? null : f.id)}
                style={{
                  cursor: 'pointer',
                  background: isSel ? 'var(--bg-3)' : 'transparent',
                  borderTop: '1px solid var(--rule)',
                  opacity: f.verdict === 'dropped' ? 0.55 : 1,
                }}>
                <td style={{ padding: '10px 12px' }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 2 }}>
                    <span style={{ width: 6, height: 6, borderRadius: '50%', background: window.SEV_COLOR[sev] }} />
                    <span style={{
                      fontSize: 12, color: 'var(--fg)', fontWeight: 500,
                      textDecoration: f.verdict === 'dropped' ? 'line-through' : 'none',
                    }}>{f.title}</span>
                  </div>
                  <div style={{ fontSize: 10, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)' }}>
                    {f.file}<span style={{ color: 'var(--periwinkle)' }}>:{f.line}</span>
                  </div>
                </td>
                {agents.map((a) => (
                  <td key={a.id} style={{ padding: '8px 4px', textAlign: 'center' }}>
                    <MatrixCell vote={lastVote[a.id]} />
                  </td>
                ))}
                <td style={{ padding: '8px 12px', textAlign: 'right' }}>
                  <VerdictPill verdict={f.verdict} />
                  <div style={{ fontSize: 9, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)', marginTop: 4 }}>
                    {(f.confidence * 100).toFixed(0)}% conf
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
const matrixH = {
  padding: '10px 12px', textAlign: 'left',
  fontSize: 9, color: 'var(--fg-subtle)', fontWeight: 700,
  letterSpacing: '0.08em', textTransform: 'uppercase',
  borderBottom: '1px solid var(--border)',
};

function MatrixCell({ vote }) {
  if (!vote) return <span style={{ color: 'var(--fg-faint)' }}>—</span>;
  const map = {
    propose:  { ch: '＋', color: 'var(--fg-muted)' },
    keep:     { ch: '✓',  color: 'var(--ok)' },
    drop:     { ch: '✕',  color: 'var(--err)' },
    merge:    { ch: '⊕',  color: 'var(--periwinkle)' },
    escalate: { ch: '↑',  color: 'var(--err)' },
    lower:    { ch: '↓',  color: 'var(--warn)' },
    abstain:  { ch: '·',  color: 'var(--fg-faint)' },
    flag:     { ch: '⚑',  color: 'var(--warn)' },
  };
  const m = map[vote.vote] || map.abstain;
  return (
    <span title={`${vote.vote}${vote.note ? ' — ' + vote.note : ''}`} style={{
      display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
      width: 24, height: 24, borderRadius: 6,
      background: 'var(--bg-2)', color: m.color,
      fontSize: 13, fontWeight: 700,
    }}>{m.ch}</span>
  );
}

function VerdictPill({ verdict }) {
  const m = window.VERDICT_COLOR[verdict];
  return (
    <span style={{
      display: 'inline-flex', alignItems: 'center', gap: 4,
      padding: '2px 7px', borderRadius: 999,
      background: m.bg, border: `1px solid ${m.bd}`,
      color: m.fg, fontSize: 9, fontWeight: 700,
      letterSpacing: '0.06em', textTransform: 'uppercase',
    }}>{window.VERDICT_LABEL[verdict]}</span>
  );
}

// ═══ Variant C: FUNNEL ═════════════════════════════════════════════════════
// 4 horizontal bands: proposed → cross-checked → resolved → final.
// Each band shows count + the findings that flowed through it. Dropped/merged
// findings peel off to the sides.
function FunnelLayout({ findings, run, selected, onSelect }) {
  const proposed = findings;
  const afterRound1 = findings.filter((f) => !(f.rounds.length === 1 && f.verdict === 'dropped'));
  const afterRound2 = findings.filter((f) => f.verdict !== 'dropped');
  const final = findings.filter((f) => f.verdict !== 'dropped');

  const bands = [
    { id: 'r1', label: 'Round 1 · Proposed',     desc: 'Each agent reviewed independently',  items: proposed,    peel: [] },
    { id: 'r2', label: 'Round 2 · Cross-checked', desc: 'Agents validated or challenged',     items: afterRound2, peel: findings.filter((f) => f.verdict === 'dropped') },
    { id: 'r3', label: 'Round 3 · Resolved',      desc: 'Conflicts arbitrated; severities set', items: final,      peel: findings.filter((f) => f.verdict === 'merged') },
    { id: 'final', label: 'Final Truth',          desc: 'Ships to your review',               items: final,       peel: [] },
  ];

  // Funnel widths (decreasing)
  const widths = ['100%', '88%', '76%', '66%'];

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 18, alignItems: 'center', paddingBottom: 24 }}>
      {bands.map((b, i) => (
        <div key={b.id} style={{ width: widths[i], display: 'flex', flexDirection: 'column', gap: 8 }}>
          <div style={{
            display: 'flex', alignItems: 'baseline', gap: 10,
            padding: '4px 12px',
            borderLeft: '2px solid var(--periwinkle)',
          }}>
            <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--fg)' }}>{b.label}</span>
            <span style={{ fontSize: 11, color: 'var(--fg-muted)' }}>{b.desc}</span>
            <div style={{ flex: 1 }} />
            <span style={{
              fontSize: 14, fontFamily: 'var(--font-mono)', fontWeight: 700,
              color: i === bands.length - 1 ? 'var(--periwinkle)' : 'var(--fg)',
            }}>{b.items.length}</span>
            <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>findings</span>
          </div>

          <div style={{
            display: 'flex', flexWrap: 'wrap', gap: 6,
            background: i === bands.length - 1 ? 'rgba(127,135,255,0.06)' : 'var(--bg-1)',
            border: `1px solid ${i === bands.length - 1 ? 'rgba(127,135,255,0.30)' : 'var(--border)'}`,
            borderRadius: 10, padding: 10,
          }}>
            {b.items.map((f) => (
              <FunnelChip key={f.id} f={f} kind={i === bands.length - 1 ? 'final' : 'flow'}
                selected={selected === f.id} onClick={() => onSelect(selected === f.id ? null : f.id)} />
            ))}
          </div>

          {b.peel.length > 0 && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, paddingLeft: 14, color: 'var(--fg-subtle)', fontSize: 10 }}>
              <i className="ph ph-arrow-elbow-down-right" style={{ fontSize: 11 }} />
              <span style={{ fontWeight: 600, letterSpacing: '0.06em', textTransform: 'uppercase' }}>
                {b.peel.length} peeled off →
              </span>
              {b.peel.map((f) => (
                <FunnelChip key={f.id} f={f} kind="peel"
                  selected={selected === f.id} onClick={() => onSelect(selected === f.id ? null : f.id)} />
              ))}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

function FunnelChip({ f, kind, selected, onClick }) {
  const sev = f.severityByRound[Math.max(...Object.keys(f.severityByRound).map(Number))];
  const dropped = f.verdict === 'dropped';
  return (
    <button
      onClick={onClick}
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 5,
        padding: '4px 9px 4px 6px',
        background: selected ? 'var(--periwinkle)' :
                    kind === 'final' ? 'var(--bg-2)' :
                    kind === 'peel'  ? 'transparent' : 'var(--bg-2)',
        border: `1px solid ${selected ? 'var(--periwinkle)' : kind === 'peel' ? 'rgba(255,255,255,0.08)' : 'var(--border)'}`,
        borderRadius: 999,
        color: selected ? '#0e1420' : (dropped ? 'var(--fg-subtle)' : 'var(--fg)'),
        fontSize: 11, fontWeight: 500, fontFamily: 'inherit',
        cursor: 'pointer',
        textDecoration: dropped && !selected ? 'line-through' : 'none',
        opacity: dropped && !selected ? 0.6 : 1,
      }}>
      <span style={{ width: 6, height: 6, borderRadius: '50%', background: window.SEV_COLOR[sev] }} />
      <AgentStack ids={f.raisedBy} max={2} size={12} />
      <span style={{ maxWidth: 200, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
        {f.title}
      </span>
    </button>
  );
}

// ─── RIGHT PANE — Final Truth ─────────────────────────────────────────────
function ArenaFinalTruth({ findings, stats, selected, onSelect }) {
  const finalFindings = findings.filter((f) => f.verdict !== 'dropped');
  const grouped = {
    high: finalFindings.filter((f) => f.severityByRound[3] === 'high' || f.severityByRound[2] === 'high' && !f.severityByRound[3]),
    med:  finalFindings.filter((f) => (f.severityByRound[3] || f.severityByRound[2]) === 'med'),
    low:  finalFindings.filter((f) => (f.severityByRound[3] || f.severityByRound[2]) === 'low'),
  };

  return (
    <div style={{
      borderLeft: '1px solid var(--border)',
      background: 'var(--bg-1)',
      display: 'flex', flexDirection: 'column', minHeight: 0,
    }}>
      <div style={{
        padding: '14px 18px 12px',
        borderBottom: '1px solid var(--border)',
        display: 'flex', flexDirection: 'column', gap: 8,
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <i className="ph-fill ph-check-circle" style={{ fontSize: 14, color: 'var(--ok)' }} />
          <span style={{ fontSize: 12, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--fg)' }}>
            Final Truth
          </span>
          <span style={{ fontSize: 11, color: 'var(--fg-muted)' }}>{finalFindings.length} findings</span>
          <div style={{ flex: 1 }} />
          <span className="pill pill--ok" style={{ fontSize: 9 }}>READY</span>
        </div>
        <div style={{ fontSize: 11, color: 'var(--fg-muted)', lineHeight: 1.5 }}>
          The consolidated review after {window.DATA_ARENA_RUN.rounds.length} rounds of consensus across {window.DATA_AGENTS.length} agents.
          Confidence shown per finding.
        </div>
        <div style={{ display: 'flex', gap: 6 }}>
          <button style={btnPrimary}><i className="ph-bold ph-check" style={{ fontSize: 11 }} />Accept all</button>
          <button style={btnSecondary}><i className="ph ph-arrow-clockwise" style={{ fontSize: 11 }} />Re-run arena</button>
        </div>
      </div>

      <div style={{ flex: 1, overflowY: 'auto', padding: '8px 0' }}>
        <FinalGroup label="HIGH"  color="var(--err)"  items={grouped.high} selected={selected} onSelect={onSelect} />
        <FinalGroup label="MED"   color="var(--warn)" items={grouped.med}  selected={selected} onSelect={onSelect} />
        <FinalGroup label="LOW"   color="var(--info)" items={grouped.low}  selected={selected} onSelect={onSelect} />
      </div>
    </div>
  );
}

function FinalGroup({ label, color, items, selected, onSelect }) {
  if (items.length === 0) return null;
  return (
    <div style={{ marginBottom: 6 }}>
      <div style={{
        padding: '6px 18px',
        display: 'flex', alignItems: 'center', gap: 6,
        fontSize: 9, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase',
      }}>
        <span style={{ width: 6, height: 6, borderRadius: '50%', background: color }} />
        <span style={{ color }}>{label}</span>
        <span style={{ color: 'var(--fg-subtle)' }}>· {items.length}</span>
      </div>
      {items.map((f) => (
        <FinalRow key={f.id} f={f} selected={selected === f.id} onClick={() => onSelect(selected === f.id ? null : f.id)} />
      ))}
    </div>
  );
}

function FinalRow({ f, selected, onClick }) {
  const sev = f.severityByRound[3] || f.severityByRound[2] || f.severityByRound[1];
  const verdict = f.verdict;
  return (
    <div className="arena-link-row"
      onClick={onClick}
      style={{
        padding: '8px 18px',
        background: selected ? 'var(--bg-3)' : 'transparent',
        borderLeft: selected ? `2px solid var(--periwinkle)` : '2px solid transparent',
        cursor: 'pointer',
        display: 'flex', flexDirection: 'column', gap: 4,
      }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <span style={{ width: 6, height: 6, borderRadius: '50%', background: window.SEV_COLOR[sev], flexShrink: 0 }} />
        <a style={{
          fontSize: 10, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)',
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', minWidth: 0, flex: 1,
        }}>
          {f.file}<span style={{ color: 'var(--periwinkle)' }}>:{f.line}</span>
        </a>
        <VerdictPill verdict={verdict} />
      </div>
      <div style={{ fontSize: 12, color: 'var(--fg)', lineHeight: 1.4, paddingLeft: 12 }}>{f.text}</div>
      <div style={{ paddingLeft: 12, display: 'flex', alignItems: 'center', gap: 8, fontSize: 10, color: 'var(--fg-subtle)' }}>
        <AgentStack ids={f.raisedBy} max={5} size={13} />
        <span>raised · </span>
        <span style={{ fontFamily: 'var(--font-mono)' }}>{(f.confidence * 100).toFixed(0)}% confidence</span>
      </div>
      {selected && (
        <div style={{
          marginTop: 6, marginLeft: 12,
          padding: '8px 10px', background: 'var(--bg-0)',
          border: '1px solid var(--border)', borderRadius: 6,
          fontSize: 11, color: 'var(--fg-muted)', lineHeight: 1.5,
        }}>
          <div style={{ fontSize: 9, fontWeight: 700, color: 'var(--fg-subtle)', letterSpacing: '0.08em', textTransform: 'uppercase', marginBottom: 4 }}>
            Why this verdict
          </div>
          {f.rationale}
        </div>
      )}
    </div>
  );
}

const btnPrimary = {
  display: 'inline-flex', alignItems: 'center', gap: 6,
  height: 28, padding: '0 12px', borderRadius: 6,
  background: 'var(--periwinkle)', color: '#fff',
  border: 0, fontSize: 11, fontWeight: 600, fontFamily: 'inherit', cursor: 'pointer',
};
const btnSecondary = {
  display: 'inline-flex', alignItems: 'center', gap: 6,
  height: 28, padding: '0 12px', borderRadius: 6,
  background: 'var(--bg-2)', color: 'var(--fg)',
  border: '1px solid var(--border)', fontSize: 11, fontWeight: 500, fontFamily: 'inherit', cursor: 'pointer',
};

Object.assign(window, { ArenaOverlay });
