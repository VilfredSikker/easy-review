// review-multi.jsx — Multi-agent AI Review panel for the right rail.
// Replaces the current single-agent dropdown. Three layout variants exposed
// via tweaks so the user can compare:
//   • chips    — agent chips at top, picked one's findings below (default)
//   • stacked  — every agent expanded, scroll through all
//   • merged   — single deduped feed with agent attribution per finding
//
// Also includes the entry point for the AI Review Arena (the multi-round
// consensus view) — a card at top of the panel with status + "Open Arena" CTA.

const { useState: useRM, useMemo: useRMemo } = React;

const SEV_COLOR = {
  high: 'var(--err)',
  med:  'var(--warn)',
  low:  'var(--info)',
};
const SEV_LABEL = { high: 'HIGH', med: 'MED', low: 'LOW' };

const VERDICT_COLOR = {
  kept:      { fg: 'var(--ok)',         bg: 'rgba(78,201,164,0.14)',  bd: 'rgba(78,201,164,0.32)' },
  escalated: { fg: 'var(--err)',        bg: 'rgba(255,107,107,0.14)', bd: 'rgba(255,107,107,0.34)' },
  merged:    { fg: 'var(--periwinkle)', bg: 'rgba(127,135,255,0.14)', bd: 'rgba(127,135,255,0.34)' },
  dropped:   { fg: 'var(--fg-subtle)',  bg: 'rgba(255,255,255,0.04)', bd: 'rgba(255,255,255,0.08)' },
};
const VERDICT_LABEL = {
  kept: 'Kept', escalated: 'Escalated', merged: 'Merged', dropped: 'Dropped',
};

// ─── Main entry: picks a variant ───────────────────────────────────────────
function ReviewMulti({ pickerMode = 'chips', reviewState = 'full', onOpenArena, onNewRun, onRunQuick, onRunSpecialist }) {
  // Empty / partial states focus on starting a review.
  if (reviewState === 'empty') {
    return (
      <window.ReviewEmptyState
        onNewArena={onNewRun}
        onRunQuick={onRunQuick || onNewRun}
        onRunSpecialist={onRunSpecialist || onNewRun}
      />
    );
  }
  if (reviewState === 'general-only') {
    return (
      <window.ReviewGeneralOnlyState
        onNewArena={onNewRun}
        onAddAgent={onRunSpecialist || onNewRun}
      />
    );
  }
  if (reviewState === 'specialized-only') {
    return (
      <window.ReviewSpecializedOnlyState
        agentId="security"
        onNewArena={onNewRun}
        onAddAgent={onRunSpecialist || onNewRun}
        onAddGeneral={onRunQuick || onNewRun}
      />
    );
  }
  if (reviewState === 'multi-specialists') {
    return (
      <window.ReviewMultiSpecialistsState
        onNewArena={onNewRun}
        onAddAgent={onRunSpecialist || onNewRun}
      />
    );
  }
  if (reviewState === 'multi-runs') {
    return (
      <window.ReviewMultiRunsState
        onNewArena={onNewRun}
        onAddAgent={onRunSpecialist || onNewRun}
      />
    );
  }
  const agents = window.DATA_AGENTS;
  const reviews = window.DATA_AGENT_REVIEWS;

  // Per-agent severity counts (for chip badges + summary)
  const summary = useRMemo(() => {
    return agents.map((a) => {
      const r = reviews[a.id] || { findings: [] };
      const counts = { high: 0, med: 0, low: 0 };
      r.findings.forEach((f) => { counts[f.severity]++; });
      return { agent: a, total: r.findings.length, counts, status: r.status, ranAt: r.ranAt };
    });
  }, []);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <ArenaCard onOpenArena={onOpenArena} onNewRun={onNewRun} />
      {pickerMode === 'chips'   && <ChipsVariant summary={summary} reviews={reviews} />}
      {pickerMode === 'stacked' && <StackedVariant summary={summary} reviews={reviews} />}
      {pickerMode === 'merged'  && <MergedVariant  summary={summary} reviews={reviews} />}
      <FooterActions />
    </div>
  );
}

// ─── Arena entry card (always visible) ────────────────────────────────────
function ArenaCard({ onOpenArena, onNewRun }) {
  const run = window.DATA_ARENA_RUN;
  const stats = window.ARENA_STATS;
  return (
    <div style={{
      background: 'linear-gradient(135deg, rgba(127,135,255,0.10), rgba(255,122,43,0.08))',
      border: '1px solid rgba(127,135,255,0.30)',
      borderRadius: 8,
      overflow: 'hidden',
    }}>
      <button
        onClick={onOpenArena}
        style={{
          width: '100%', textAlign: 'left',
          padding: 12, background: 'transparent', border: 0,
          color: 'var(--fg)', cursor: 'pointer', fontFamily: 'inherit',
          display: 'flex', flexDirection: 'column', gap: 8,
        }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <i className="ph-fill ph-trophy" style={{ fontSize: 14, color: 'var(--periwinkle)' }} />
          <span style={{ fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--fg)' }}>
            AI Review Arena
          </span>
          <span className="pill pill--info" style={{ fontSize: 9 }}>{run.status === 'complete' ? 'COMPLETE' : run.status.toUpperCase()}</span>
          <div style={{ flex: 1 }} />
          <i className="ph ph-arrow-up-right" style={{ fontSize: 12, color: 'var(--fg-muted)' }} />
        </div>
        <div style={{ fontSize: 12, color: 'var(--fg-muted)', lineHeight: 1.45 }}>
          <strong style={{ color: 'var(--fg)' }}>{stats.finalCount} final findings</strong> from {stats.proposed} proposals
          across {window.DATA_AGENTS.length} agents · {run.rounds.length} rounds.
        </div>
        <div style={{ display: 'flex', alignItems: 'center' }}>
          <AgentStack ids={run.agents} max={5} size={18} />
          <div style={{ flex: 1 }} />
          <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>{run.startedAt} · {(run.durationMs/1000).toFixed(1)}s</span>
        </div>
      </button>
      <div style={{
        display: 'flex', alignItems: 'stretch',
        borderTop: '1px solid rgba(127,135,255,0.18)',
      }}>
        <button onClick={onNewRun} style={{
          flex: 1, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 5,
          padding: '7px 10px', background: 'transparent', border: 0,
          color: 'var(--periwinkle)', fontSize: 11, fontWeight: 600, fontFamily: 'inherit', cursor: 'pointer',
          whiteSpace: 'nowrap',
        }}
          onMouseOver={(e) => { e.currentTarget.style.background = 'rgba(127,135,255,0.10)'; }}
          onMouseOut={(e)  => { e.currentTarget.style.background = 'transparent'; }}
        >
          <i className="ph-bold ph-plus" style={{ fontSize: 11 }} />
          New run
        </button>
        <span style={{ width: 1, background: 'rgba(127,135,255,0.18)' }} />
        <button onClick={onOpenArena} style={{
          flex: 1, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 5,
          padding: '7px 10px', background: 'transparent', border: 0,
          color: 'var(--fg-muted)', fontSize: 11, fontWeight: 600, fontFamily: 'inherit', cursor: 'pointer',
          whiteSpace: 'nowrap',
        }}
          onMouseOver={(e) => { e.currentTarget.style.background = 'rgba(255,255,255,0.04)'; }}
          onMouseOut={(e)  => { e.currentTarget.style.background = 'transparent'; }}
        >
          <i className="ph-bold ph-arrow-up-right" style={{ fontSize: 11 }} />
          Open last
        </button>
      </div>
    </div>
  );
}

// ─── Variant A: Chips at top, selected agent's findings below ─────────────
function ChipsVariant({ summary, reviews }) {
  const [active, setActive] = useRM(summary[0].agent.id);
  const review = reviews[active];
  const activeSummary = summary.find((s) => s.agent.id === active);
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
      <div style={{
        display: 'flex', flexWrap: 'wrap', gap: 4,
        margin: '0 -2px', padding: '0 2px 2px',
      }}>
        {summary.map(({ agent, total, counts }) => (
          <AgentChip
            key={agent.id} agent={agent}
            active={agent.id === active}
            total={total} counts={counts}
            onClick={() => setActive(agent.id)}
          />
        ))}
      </div>

      <RunMeta review={review} agent={activeSummary.agent} />
      <SeverityRow counts={activeSummary.counts} />
      <SeverityFilter />
      <MultiFindingsList findings={review.findings} />
    </div>
  );
}

function AgentChip({ agent, active, total, counts, onClick }) {
  const [hover, setHover] = useRM(false);
  const highest = counts.high > 0 ? 'high' : counts.med > 0 ? 'med' : counts.low > 0 ? 'low' : null;
  return (
    <button
      onClick={onClick}
      onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
      title={`${agent.name} · ${total} finding${total === 1 ? '' : 's'}`}
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 6,
        padding: '5px 9px',
        background: active ? `${agent.color}24` : (hover ? 'var(--bg-3)' : 'var(--bg-2)'),
        border: `1px solid ${active ? agent.color + '88' : 'var(--border)'}`,
        borderRadius: 999,
        color: active ? 'var(--fg)' : 'var(--fg-muted)',
        fontSize: 11, fontWeight: active ? 600 : 500, fontFamily: 'inherit',
        whiteSpace: 'nowrap', flexShrink: 0,
        transition: 'all var(--d-fast) var(--ease)',
        cursor: 'pointer',
      }}
    >
      <AgentDot agent={agent} size={10} />
      <span>{agent.name}</span>
      <span style={{
        minWidth: 14, height: 14, padding: '0 4px', borderRadius: 999,
        background: active ? agent.color : 'rgba(255,255,255,0.07)',
        color: active ? '#0e1420' : 'var(--fg-muted)',
        fontSize: 9, fontWeight: 700,
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        lineHeight: 1,
      }}>{total}</span>
      {!active && highest && (
        <span style={{ width: 5, height: 5, borderRadius: '50%', background: SEV_COLOR[highest] }} />
      )}
    </button>
  );
}

function RunMeta({ review, agent }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 8,
      padding: '6px 8px',
      background: 'var(--bg-0)', border: '1px solid var(--border)',
      borderRadius: 6,
    }}>
      <AgentDot agent={agent} size={14} />
      <div style={{ display: 'flex', flexDirection: 'column', gap: 1, minWidth: 0, flex: 1 }}>
        <div style={{ fontSize: 11, color: 'var(--fg)', fontWeight: 500 }}>{agent.name}</div>
        <div style={{ fontSize: 10, color: 'var(--fg-subtle)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{agent.desc}</div>
      </div>
      <span className="pill pill--ok" style={{ fontSize: 9 }}>{review.status.toUpperCase()}</span>
      <button title="Re-run this agent" style={{
        width: 22, height: 22, borderRadius: 4, border: 0,
        background: 'transparent', color: 'var(--fg-subtle)',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
      }}>
        <i className="ph ph-arrow-clockwise" style={{ fontSize: 11 }} />
      </button>
    </div>
  );
}

function SeverityRow({ counts }) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 6 }}>
      <SeverityCell color="var(--err)"  label="HIGH" value={counts.high} />
      <SeverityCell color="var(--warn)" label="MED"  value={counts.med} />
      <SeverityCell color="var(--info)" label="LOW"  value={counts.low} />
    </div>
  );
}
function SeverityCell({ color, label, value }) {
  return (
    <div style={{
      background: 'var(--bg-2)', borderRadius: 6, padding: '8px 10px',
      display: 'flex', flexDirection: 'column', gap: 2,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 5, fontSize: 9, letterSpacing: '0.08em', color, fontWeight: 700 }}>
        <span style={{ width: 6, height: 6, borderRadius: '50%', background: color }} />
        {label}
      </div>
      <div style={{ fontSize: 18, color: 'var(--fg)', fontWeight: 600, fontFamily: 'var(--font-mono)' }}>{value}</div>
    </div>
  );
}

function SeverityFilter() {
  const [filter, setFilter] = useRM('all');
  const opts = [
    { id: 'all',  label: 'all',  color: null },
    { id: 'high', label: 'high', color: SEV_COLOR.high },
    { id: 'med',  label: 'med',  color: SEV_COLOR.med },
    { id: 'low',  label: 'low',  color: SEV_COLOR.low },
  ];
  return (
    <div style={{ display: 'flex', gap: 4, fontSize: 10 }}>
      {opts.map((o) => (
        <button key={o.id} onClick={() => setFilter(o.id)}
          style={{
            display: 'inline-flex', alignItems: 'center', gap: 4,
            padding: '3px 7px', border: 0, borderRadius: 4,
            background: filter === o.id ? 'rgba(255,255,255,0.06)' : 'transparent',
            color: filter === o.id ? 'var(--fg)' : 'var(--fg-subtle)',
            fontSize: 10, fontFamily: 'inherit', cursor: 'pointer',
            borderBottom: filter === o.id ? `1px solid ${o.color || 'var(--fg)'}` : '1px solid transparent',
          }}>
          {o.color && <span style={{ width: 5, height: 5, borderRadius: '50%', background: o.color }} />}
          {o.label}
        </button>
      ))}
    </div>
  );
}

// Single finding row (compact, like the screenshot)
function FindingRow({ finding, attribution }) {
  return (
    <div style={{
      padding: '8px 0', borderTop: '1px solid var(--rule)',
      display: 'flex', flexDirection: 'column', gap: 4,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <span style={{ width: 6, height: 6, borderRadius: '50%', background: SEV_COLOR[finding.severity], flexShrink: 0 }} />
        <a href="#" style={{
          fontSize: 10, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)',
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', minWidth: 0, flex: 1,
        }}>
          {finding.file}<span style={{ color: 'var(--periwinkle)' }}>:{finding.line}</span>
        </a>
        {attribution && <AgentStack ids={attribution} size={14} max={3} />}
      </div>
      <div style={{ fontSize: 12, color: 'var(--fg)', lineHeight: 1.4, paddingLeft: 12 }}>{finding.text}</div>
    </div>
  );
}
function MultiFindingsList({ findings }) {
  return (
    <div>
      {findings.map((f, i) => <FindingRow key={f.id + i} finding={f} />)}
    </div>
  );
}

// ─── Variant B: Stacked — every agent expanded, scroll through all ────────
function StackedVariant({ summary, reviews }) {
  const [open, setOpen] = useRM(() => Object.fromEntries(summary.map((s) => [s.agent.id, true])));
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      {summary.map(({ agent, total, counts }) => {
        const isOpen = open[agent.id];
        return (
          <div key={agent.id} style={{
            background: 'var(--bg-0)', border: '1px solid var(--border)', borderRadius: 8, overflow: 'hidden',
          }}>
            <button
              onClick={() => setOpen({ ...open, [agent.id]: !isOpen })}
              style={{
                width: '100%', textAlign: 'left',
                display: 'flex', alignItems: 'center', gap: 8,
                padding: '8px 10px',
                background: 'var(--bg-2)', border: 0,
                color: 'var(--fg)', fontFamily: 'inherit', cursor: 'pointer',
              }}>
              <AgentDot agent={agent} size={14} />
              <span style={{ fontSize: 11, fontWeight: 600 }}>{agent.name}</span>
              <span style={{
                fontSize: 9, fontWeight: 700,
                padding: '1px 5px', borderRadius: 999,
                background: 'rgba(255,255,255,0.07)', color: 'var(--fg-muted)',
              }}>{total}</span>
              <span style={{ display: 'inline-flex', gap: 3, marginLeft: 4 }}>
                {counts.high > 0 && <SevDot c={counts.high} color={SEV_COLOR.high} />}
                {counts.med  > 0 && <SevDot c={counts.med}  color={SEV_COLOR.med} />}
                {counts.low  > 0 && <SevDot c={counts.low}  color={SEV_COLOR.low} />}
              </span>
              <div style={{ flex: 1 }} />
              <i className={`ph ph-caret-${isOpen ? 'down' : 'right'}`} style={{ fontSize: 10, color: 'var(--fg-subtle)' }} />
            </button>
            {isOpen && (
              <div style={{ padding: '4px 10px 10px' }}>
                {reviews[agent.id].findings.map((f, i) => <FindingRow key={f.id + i} finding={f} />)}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
function SevDot({ c, color }) {
  return (
    <span style={{
      fontSize: 9, color, fontWeight: 700,
      display: 'inline-flex', alignItems: 'center', gap: 2,
    }}>
      <span style={{ width: 5, height: 5, borderRadius: '50%', background: color }} />{c}
    </span>
  );
}

// ─── Variant C: Merged feed with agent attribution per row ────────────────
function MergedVariant({ summary, reviews }) {
  // Group findings by id, collect which agents raised each
  const merged = useRMemo(() => {
    const byId = new Map();
    Object.entries(reviews).forEach(([agentId, r]) => {
      r.findings.forEach((f) => {
        const existing = byId.get(f.id);
        if (existing) {
          existing.agents.push(agentId);
          // Take the highest severity seen
          if (sevRank(f.severity) > sevRank(existing.finding.severity)) existing.finding = { ...existing.finding, severity: f.severity };
        } else {
          byId.set(f.id, { finding: { ...f }, agents: [agentId] });
        }
      });
    });
    return Array.from(byId.values()).sort((a, b) => sevRank(b.finding.severity) - sevRank(a.finding.severity) || b.agents.length - a.agents.length);
  }, []);

  const counts = { high: 0, med: 0, low: 0 };
  merged.forEach((m) => counts[m.finding.severity]++);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
      <div style={{
        display: 'flex', alignItems: 'center', gap: 6,
        padding: '6px 8px',
        background: 'var(--bg-0)', border: '1px solid var(--border)', borderRadius: 6,
      }}>
        <AgentStack ids={summary.map((s) => s.agent.id)} size={16} max={5} />
        <span style={{ fontSize: 11, color: 'var(--fg)', fontWeight: 500 }}>{merged.length} unique</span>
        <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>· deduped across {summary.length} agents</span>
        <div style={{ flex: 1 }} />
        <span className="pill pill--ok" style={{ fontSize: 9 }}>FRESH</span>
      </div>
      <SeverityRow counts={counts} />
      <SeverityFilter />
      <div>
        {merged.map((m, i) => (
          <FindingRow key={m.finding.id + i} finding={m.finding} attribution={m.agents} />
        ))}
      </div>
    </div>
  );
}
function sevRank(s) { return s === 'high' ? 3 : s === 'med' ? 2 : 1; }

// ─── Shared atoms ─────────────────────────────────────────────────────────
function AgentDot({ agent, size = 12 }) {
  return (
    <span title={agent.name} style={{
      width: size, height: size, borderRadius: '50%',
      background: agent.color, flexShrink: 0,
      display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
      boxShadow: `0 0 0 1px ${agent.color}33`,
    }}>
      <i className={`ph-bold ${agent.icon}`} style={{ fontSize: size * 0.55, color: '#0e1420' }} />
    </span>
  );
}

function AgentStack({ ids, max = 4, size = 16 }) {
  const shown = ids.slice(0, max);
  const overflow = ids.length - shown.length;
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center' }}>
      {shown.map((id, i) => {
        const a = window.AGENT_BY_ID[id];
        if (!a) return null;
        return (
          <span key={id} style={{
            width: size, height: size, borderRadius: '50%',
            background: a.color,
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            border: '2px solid var(--bg-1)',
            marginLeft: i === 0 ? 0 : -5,
            zIndex: shown.length - i,
          }} title={a.name}>
            <i className={`ph-bold ${a.icon}`} style={{ fontSize: size * 0.5, color: '#0e1420' }} />
          </span>
        );
      })}
      {overflow > 0 && (
        <span style={{
          width: size, height: size, borderRadius: '50%',
          background: 'var(--bg-3)', color: 'var(--fg-muted)',
          fontSize: size * 0.45, fontWeight: 700,
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          border: '2px solid var(--bg-1)', marginLeft: -5,
        }}>+{overflow}</span>
      )}
    </span>
  );
}

function FooterActions() {
  return (
    <div style={{ display: 'flex', gap: 6, color: 'var(--fg-subtle)', fontSize: 11, paddingTop: 4 }}>
      <button style={btnGhost}><i className="ph ph-copy" style={{ fontSize: 10 }} />Copy JSON</button>
      <button style={btnGhost}><i className="ph ph-folder" style={{ fontSize: 10 }} />Reveal files</button>
      <div style={{ flex: 1 }} />
      <button style={btnGhost}><i className="ph ph-arrow-clockwise" style={{ fontSize: 10 }} />Re-run all</button>
    </div>
  );
}
const btnGhost = {
  display: 'inline-flex', alignItems: 'center', gap: 5,
  height: 22, padding: '0 6px', borderRadius: 4,
  background: 'transparent', color: 'var(--fg-subtle)',
  border: 0, fontSize: 11, fontFamily: 'inherit', cursor: 'pointer',
};

Object.assign(window, {
  ReviewMulti, AgentDot, AgentStack, SEV_COLOR, SEV_LABEL,
  VERDICT_COLOR, VERDICT_LABEL,
});
