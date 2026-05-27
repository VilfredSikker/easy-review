// review-empty-states.jsx — early / partial states for the Review panel.
//
// Three states beyond the "full" multi-agent run:
//   • empty            — nothing has reviewed this branch yet
//   • general-only     — only the general model ran (no arena)
//   • specialized-only — exactly one specialized agent ran (no arena)
//
// All three focus on the *next action*: starting a richer review.
// All paths can reach either (a) another singular review or (b) the Arena.

const { useState: useES } = React;

// ─── Shared visual atoms ──────────────────────────────────────────────────

function StateHero({ icon, iconBg, title, body }) {
  return (
    <div style={{ display: 'flex', alignItems: 'flex-start', gap: 10, marginBottom: 4 }}>
      <span style={{
        width: 28, height: 28, borderRadius: 8,
        background: iconBg, flexShrink: 0,
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
      }}>
        <i className={`ph-fill ${icon}`} style={{ fontSize: 14, color: '#0e1420' }} />
      </span>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2, paddingTop: 1 }}>
        <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--fg)' }}>{title}</div>
        <div style={{ fontSize: 11, color: 'var(--fg-muted)', lineHeight: 1.5 }}>{body}</div>
      </div>
    </div>
  );
}

function StartCard({ tone, icon, eyebrow, title, blurb, meta, cta, onClick, primary }) {
  // tone: 'primary' (arena/periwinkle) | 'neutral' (review/dark) | 'ghost' (subtle)
  const palette = tone === 'primary' ? {
    bg: 'linear-gradient(135deg, rgba(127,135,255,0.10), rgba(255,122,43,0.06))',
    bd: 'rgba(127,135,255,0.34)',
    iconColor: 'var(--periwinkle)',
    iconBg: 'rgba(127,135,255,0.16)',
    eyebrow: 'var(--periwinkle)',
    cta: 'var(--periwinkle)',
  } : tone === 'ghost' ? {
    bg: 'transparent',
    bd: 'var(--border)',
    iconColor: 'var(--fg-muted)',
    iconBg: 'rgba(255,255,255,0.04)',
    eyebrow: 'var(--fg-subtle)',
    cta: 'var(--fg)',
  } : {
    bg: 'var(--bg-0)',
    bd: 'var(--border)',
    iconColor: 'var(--fg)',
    iconBg: 'var(--bg-3)',
    eyebrow: 'var(--fg-subtle)',
    cta: 'var(--fg)',
  };

  return (
    <button onClick={onClick} style={{
      textAlign: 'left',
      background: palette.bg,
      border: `1px solid ${palette.bd}`,
      borderRadius: 8,
      padding: 12,
      color: 'var(--fg)', fontFamily: 'inherit', cursor: 'pointer',
      display: 'flex', flexDirection: 'column', gap: 8,
      transition: 'border-color var(--d-fast) var(--ease), background var(--d-fast) var(--ease)',
    }}
      onMouseOver={(e) => { e.currentTarget.style.borderColor = tone === 'primary' ? 'rgba(127,135,255,0.6)' : 'var(--border-strong)'; }}
      onMouseOut ={(e) => { e.currentTarget.style.borderColor = palette.bd; }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <span style={{
          width: 22, height: 22, borderRadius: 6,
          background: palette.iconBg,
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          flexShrink: 0,
        }}>
          <i className={`ph-fill ${icon}`} style={{ fontSize: 12, color: palette.iconColor }} />
        </span>
        <span style={{
          fontSize: 9, fontWeight: 700, letterSpacing: '0.08em',
          textTransform: 'uppercase', color: palette.eyebrow,
        }}>{eyebrow}</span>
        <div style={{ flex: 1 }} />
        {meta && <span style={{ fontSize: 10, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)' }}>{meta}</span>}
      </div>
      <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--fg)', lineHeight: 1.3 }}>{title}</div>
      <div style={{ fontSize: 11, color: 'var(--fg-muted)', lineHeight: 1.5 }}>{blurb}</div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginTop: 2 }}>
        <span style={{
          display: 'inline-flex', alignItems: 'center', gap: 5,
          fontSize: 11, fontWeight: 600, color: palette.cta,
        }}>
          {cta}
          <i className="ph-bold ph-arrow-right" style={{ fontSize: 10 }} />
        </span>
      </div>
    </button>
  );
}

function SectionRule({ label }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '2px 0' }}>
      <span style={{
        fontSize: 9, fontWeight: 700, letterSpacing: '0.08em',
        textTransform: 'uppercase', color: 'var(--fg-subtle)',
      }}>{label}</span>
      <span style={{ flex: 1, height: 1, background: 'var(--rule)' }} />
    </div>
  );
}

// Small agent / model chip for the "pick a specialist" rows
function PickerPill({ item, onClick, color }) {
  return (
    <button onClick={onClick} style={{
      display: 'inline-flex', alignItems: 'center', gap: 5,
      padding: '4px 9px 4px 5px',
      background: 'var(--bg-0)',
      border: '1px solid var(--border)',
      borderRadius: 999,
      color: 'var(--fg)', fontSize: 10, fontWeight: 500, fontFamily: 'inherit',
      cursor: 'pointer', whiteSpace: 'nowrap',
      transition: 'border-color var(--d-fast) var(--ease), background var(--d-fast) var(--ease)',
    }}
      onMouseOver={(e) => { e.currentTarget.style.borderColor = (color || 'var(--fg-muted)'); e.currentTarget.style.background = 'var(--bg-2)'; }}
      onMouseOut ={(e) => { e.currentTarget.style.borderColor = 'var(--border)'; e.currentTarget.style.background = 'var(--bg-0)'; }}
    >
      <span style={{
        width: 12, height: 12, borderRadius: '50%',
        background: color || 'var(--fg-muted)',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
      }}>
        <i className={`ph-bold ${item.icon}`} style={{ fontSize: 6, color: '#0e1420' }} />
      </span>
      {item.name}
    </button>
  );
}

// ─── State: EMPTY (no reviews yet) ─────────────────────────────────────────
function ReviewEmptyState({ onNewArena, onRunQuick, onRunSpecialist }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <StateHero
        icon="ph-sparkle"
        iconBg="rgba(127,135,255,0.18)"
        title="No reviews yet"
        body="AI reviewers can audit this branch for bugs, regressions, and design issues. Start with a quick pass or run a full debate."
      />

      <StartCard
        tone="neutral"
        icon="ph-magic-wand"
        eyebrow="Quick review"
        title="One model · one pass"
        blurb="Sonnet 4.5 reads the changed files and reports findings. Fast and cheap — good for in-progress branches."
        meta="~15s · ≈ $0.04"
        cta="Run quick review"
        onClick={onRunQuick}
      />

      <StartCard
        tone="primary"
        icon="ph-trophy"
        eyebrow="AI Review Arena"
        title="Multiple reviewers debate · ship consensus"
        blurb="2–6 models or specialized agents review independently, cross-check each other, and resolve conflicts. Higher signal, fewer false positives."
        meta="~45s · ≈ $0.30"
        cta="Configure & start"
        onClick={onNewArena}
      />

      <SectionRule label="Or pick a specialist" />
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 5 }}>
        {window.DATA_AGENTS.filter((a) => a.id !== 'general').map((a) => (
          <PickerPill key={a.id} item={a} color={a.color} onClick={() => onRunSpecialist(a.id)} />
        ))}
      </div>
    </div>
  );
}

// ─── Top toolbar for non-empty states ─────────────────────────────────────
function ReviewToolbar({ subtitle, onNewRun }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 8,
      paddingBottom: 4,
    }}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 1, minWidth: 0, flex: 1 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <i className="ph-fill ph-sparkle" style={{ fontSize: 12, color: 'var(--periwinkle)' }} />
          <span style={{ fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--fg)' }}>
            AI Review
          </span>
        </div>
        {subtitle && (
          <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>{subtitle}</span>
        )}
      </div>
      <button onClick={onNewRun} title="Start another review" style={{
        display: 'inline-flex', alignItems: 'center', gap: 5,
        padding: '6px 10px', borderRadius: 6,
        background: 'var(--periwinkle)', color: '#fff',
        border: 0, fontSize: 11, fontWeight: 600, fontFamily: 'inherit', cursor: 'pointer',
        whiteSpace: 'nowrap',
      }}
        onMouseOver={(e) => { e.currentTarget.style.background = '#3F38C8'; }}
        onMouseOut ={(e) => { e.currentTarget.style.background = 'var(--periwinkle)'; }}
      >
        <i className="ph-bold ph-plus" style={{ fontSize: 10 }} />
        New review
      </button>
    </div>
  );
}

// ─── State: GENERAL-ONLY ──────────────────────────────────────────────────
// Show the general agent's findings (using existing FindingRow), then an
// "upgrade" zone for adding more reviewers or running the Arena.
function ReviewGeneralOnlyState({ onNewArena, onAddAgent }) {
  const agent = window.AGENT_BY_ID['general'];
  const review = window.DATA_AGENT_REVIEWS['general'];
  const counts = { high: 0, med: 0, low: 0 };
  review.findings.forEach((f) => counts[f.severity]++);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <ReviewToolbar subtitle="1 review on this branch" onNewRun={onNewArena} />
      {/* The single review (compact) */}
      <div style={{
        background: 'var(--bg-0)', border: '1px solid var(--border)', borderRadius: 8,
        overflow: 'hidden',
      }}>
        <div style={{
          display: 'flex', alignItems: 'center', gap: 8,
          padding: '8px 10px',
          background: 'var(--bg-2)',
          borderBottom: '1px solid var(--border)',
        }}>
          <span style={{
            width: 16, height: 16, borderRadius: '50%',
            background: agent.color,
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          }}>
            <i className={`ph-bold ${agent.icon}`} style={{ fontSize: 8, color: '#0e1420' }} />
          </span>
          <span style={{ fontSize: 11, color: 'var(--fg)', fontWeight: 600 }}>{agent.name}</span>
          <span style={{
            fontSize: 9, fontWeight: 700,
            padding: '1px 5px', borderRadius: 999,
            background: 'rgba(255,255,255,0.07)', color: 'var(--fg-muted)',
          }}>{review.findings.length}</span>
          <span style={{ display: 'inline-flex', gap: 5, marginLeft: 4 }}>
            {counts.high > 0 && <SevCount c={counts.high} color="var(--err)" />}
            {counts.med  > 0 && <SevCount c={counts.med}  color="var(--warn)" />}
            {counts.low  > 0 && <SevCount c={counts.low}  color="var(--info)" />}
          </span>
          <div style={{ flex: 1 }} />
          <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>{review.ranAt}</span>
        </div>
        <div style={{ padding: '4px 12px 10px' }}>
          {review.findings.slice(0, 3).map((f, i) => <CompactFinding key={f.id + i} f={f} />)}
          {review.findings.length > 3 && (
            <div style={{ paddingTop: 8, fontSize: 10, color: 'var(--fg-subtle)' }}>
              + {review.findings.length - 3} more
            </div>
          )}
        </div>
      </div>

      {/* Upgrade zone */}
      <SectionRule label="Level up this review" />

      <StartCard
        tone="primary"
        icon="ph-trophy"
        eyebrow="Run as Arena"
        title="Get a second, third, fourth opinion"
        blurb="Run the same review through 2–6 reviewers and surface only what they agree on. Your General findings become the baseline."
        meta="~45s · ≈ $0.30"
        cta="Open Arena setup"
        onClick={onNewArena}
      />

      <SectionRule label="Or add a single perspective" />
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 5 }}>
        {window.DATA_AGENTS.filter((a) => a.id !== 'general').map((a) => (
          <PickerPill key={a.id} item={a} color={a.color} onClick={() => onAddAgent(a.id)} />
        ))}
      </div>
    </div>
  );
}

// ─── State: SPECIALIZED-ONLY ──────────────────────────────────────────────
function ReviewSpecializedOnlyState({ agentId = 'security', onNewArena, onAddAgent, onAddGeneral }) {
  const agent = window.AGENT_BY_ID[agentId];
  const review = window.DATA_AGENT_REVIEWS[agentId];
  const counts = { high: 0, med: 0, low: 0 };
  review.findings.forEach((f) => counts[f.severity]++);
  const others = window.DATA_AGENTS.filter((a) => a.id !== agentId);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <ReviewToolbar subtitle={`1 review by ${agent.name}`} onNewRun={onNewArena} />
      <div style={{
        background: 'var(--bg-0)', border: '1px solid var(--border)', borderRadius: 8,
        overflow: 'hidden',
      }}>
        <div style={{
          display: 'flex', alignItems: 'center', gap: 8,
          padding: '8px 10px',
          background: `linear-gradient(180deg, ${agent.color}14, transparent)`,
          borderBottom: '1px solid var(--border)',
        }}>
          <span style={{
            width: 16, height: 16, borderRadius: '50%',
            background: agent.color,
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          }}>
            <i className={`ph-bold ${agent.icon}`} style={{ fontSize: 8, color: '#0e1420' }} />
          </span>
          <span style={{ fontSize: 11, color: 'var(--fg)', fontWeight: 600 }}>{agent.name}</span>
          <span style={{
            fontSize: 9, fontWeight: 700,
            padding: '1px 5px', borderRadius: 999,
            background: 'rgba(255,255,255,0.07)', color: 'var(--fg-muted)',
          }}>{review.findings.length}</span>
          <span style={{ display: 'inline-flex', gap: 5, marginLeft: 4 }}>
            {counts.high > 0 && <SevCount c={counts.high} color="var(--err)" />}
            {counts.med  > 0 && <SevCount c={counts.med}  color="var(--warn)" />}
            {counts.low  > 0 && <SevCount c={counts.low}  color="var(--info)" />}
          </span>
          <div style={{ flex: 1 }} />
          <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>{review.ranAt}</span>
        </div>
        <div style={{ padding: '4px 12px 10px' }}>
          {review.findings.slice(0, 3).map((f, i) => <CompactFinding key={f.id + i} f={f} />)}
          {review.findings.length > 3 && (
            <div style={{ paddingTop: 8, fontSize: 10, color: 'var(--fg-subtle)' }}>
              + {review.findings.length - 3} more
            </div>
          )}
        </div>
      </div>

      {/* Single, dismissible "blind spots" callout */}
      <div style={{
        padding: 10,
        background: 'rgba(255,196,87,0.06)',
        border: '1px solid rgba(255,196,87,0.20)',
        borderRadius: 8,
        display: 'flex', gap: 8, alignItems: 'flex-start',
      }}>
        <i className="ph-fill ph-warning" style={{ fontSize: 12, color: 'var(--warn)', marginTop: 1 }} />
        <div style={{ fontSize: 11, color: 'var(--fg-muted)', lineHeight: 1.5 }}>
          <strong style={{ color: 'var(--fg)' }}>Only {agent.name.toLowerCase()} reviewed this branch.</strong>
          {' '}You may be missing {others.slice(0, 3).map((a) => a.name.toLowerCase()).join(', ')} issues.
        </div>
      </div>

      <StartCard
        tone="primary"
        icon="ph-trophy"
        eyebrow="Cover the blind spots"
        title="Run as Arena"
        blurb={`Re-run with multiple reviewers — keep ${agent.name}, add others, debate, ship consensus.`}
        meta="~45s · ≈ $0.30"
        cta="Configure & start"
        onClick={onNewArena}
      />

      <SectionRule label="Or add one more lens" />
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 5 }}>
        <PickerPill item={window.AGENT_BY_ID['general']} color={window.AGENT_BY_ID['general'].color} onClick={onAddGeneral} />
        {others.filter((a) => a.id !== 'general').map((a) => (
          <PickerPill key={a.id} item={a} color={a.color} onClick={() => onAddAgent(a.id)} />
        ))}
      </div>
    </div>
  );
}

// ─── Small atoms used inside the state cards ───────────────────────────────
function SevCount({ c, color }) {
  return (
    <span style={{
      fontSize: 9, color, fontWeight: 700,
      display: 'inline-flex', alignItems: 'center', gap: 2,
    }}>
      <span style={{ width: 5, height: 5, borderRadius: '50%', background: color }} />{c}
    </span>
  );
}

function CompactFinding({ f }) {
  const sevColor = f.severity === 'high' ? 'var(--err)' : f.severity === 'med' ? 'var(--warn)' : 'var(--info)';
  return (
    <div style={{
      padding: '6px 0', borderTop: '1px solid var(--rule)',
      display: 'flex', flexDirection: 'column', gap: 3,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <span style={{ width: 5, height: 5, borderRadius: '50%', background: sevColor, flexShrink: 0 }} />
        <span style={{
          fontSize: 10, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)',
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1, minWidth: 0,
        }}>
          {f.file}<span style={{ color: 'var(--periwinkle)' }}>:{f.line}</span>
        </span>
      </div>
      <div style={{
        fontSize: 11, color: 'var(--fg)', lineHeight: 1.4, paddingLeft: 11,
        overflow: 'hidden', display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical',
      }}>{f.text}</div>
    </div>
  );
}

Object.assign(window, {
  ReviewEmptyState, ReviewGeneralOnlyState, ReviewSpecializedOnlyState,
  ReviewMultiSpecialistsState, ReviewMultiRunsState,
});

// ─── Mock: multiple separate review sessions ───────────────────────────────
// Each "run" is one independent review session with its own roster.
const DATA_REVIEW_RUNS = [
  {
    id: 'run-04', ranAt: '2 min ago', durationMs: 18400, label: 'Pre-merge sweep',
    agentIds: ['general', 'security'],
    findingIds: ['F-undef-sev', 'F-mapkey', 'F-injection', 'F-log-secret'],
  },
  {
    id: 'run-03', ranAt: '14 min ago', durationMs: 22600, label: 'Deep audit',
    agentIds: ['professor', 'security'],
    findingIds: ['F-prof-coupling', 'F-prof-invariant', 'F-mapkey', 'F-injection', 'F-log-secret', 'F-prof-naming'],
  },
  {
    id: 'run-02', ranAt: '1 hr ago', durationMs: 8200, label: 'Quick lint',
    agentIds: ['style'],
    findingIds: ['F-opt-prefix', 'F-shadow-opts', 'F-deadcomment', 'F-trailing-ws'],
  },
  {
    id: 'run-01', ranAt: '3 hr ago', durationMs: 14100, label: 'Initial pass',
    agentIds: ['general', 'tests'],
    findingIds: ['F-undef-sev', 'F-no-test', 'F-opt-prefix', 'F-shadow-opts'],
  },
];

// Build a fast lookup: id -> finding (any finding from any agent's review)
const FINDING_BY_ID = (() => {
  const map = {};
  Object.values(window.DATA_AGENT_REVIEWS || {}).forEach((review) => {
    review.findings.forEach((f) => {
      if (!map[f.id]) map[f.id] = f;
    });
  });
  return map;
})();
window.FINDING_BY_ID = FINDING_BY_ID;
window.DATA_REVIEW_RUNS = DATA_REVIEW_RUNS;

// ─── State: MULTIPLE SEPARATE RUNS ─────────────────────────────────────────
// Each card is one past review run. Expand to see findings. Top: "Promote
// to Arena" CTA that pre-fills the launcher with the union of all rosters.
function ReviewMultiRunsState({ onNewArena, onAddAgent }) {
  const runs = window.DATA_REVIEW_RUNS;
  const [expanded, setExpanded] = useES(() => new Set([runs[0].id]));

  // Roll-up across all runs
  const allFindingIds = new Set();
  const allAgentIds = new Set();
  runs.forEach((r) => {
    r.agentIds.forEach((id) => allAgentIds.add(id));
    r.findingIds.forEach((id) => allFindingIds.add(id));
  });
  const allTotals = { high: 0, med: 0, low: 0 };
  allFindingIds.forEach((fid) => {
    const f = window.FINDING_BY_ID[fid];
    if (f) allTotals[f.severity]++;
  });

  const toggle = (id) => {
    const next = new Set(expanded);
    next.has(id) ? next.delete(id) : next.add(id);
    setExpanded(next);
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <ReviewToolbar subtitle={`${runs.length} reviews on this branch`} onNewRun={onNewArena} />
      <StateHero
        icon="ph-stack"
        iconBg="rgba(127,135,255,0.18)"
        title={`${runs.length} review runs on this branch`}
        body="Different specialists, different moments — none have been reconciled yet. Each run stands alone, with overlapping and possibly conflicting findings."
      />

      {/* Roll-up bar */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: 10,
        padding: '8px 10px',
        background: 'var(--bg-0)', border: '1px solid var(--border)', borderRadius: 6,
      }}>
        <span style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--fg-subtle)' }}>
          Across all runs
        </span>
        <div style={{ flex: 1 }} />
        <SevCount c={allTotals.high} color="var(--err)" />
        <SevCount c={allTotals.med}  color="var(--warn)" />
        <SevCount c={allTotals.low}  color="var(--info)" />
        <span style={{ width: 1, height: 12, background: 'var(--rule)', margin: '0 4px' }} />
        <span style={{ fontSize: 10, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)' }}>
          {allFindingIds.size} unique · {allAgentIds.size} agents
        </span>
      </div>

      {/* Run cards */}
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        {runs.map((run, idx) => {
          const isLatest = idx === 0;
          return (
            <RunCard
              key={run.id}
              run={run}
              isLatest={isLatest}
              expanded={expanded.has(run.id)}
              onToggle={() => toggle(run.id)}
            />
          );
        })}
      </div>

      <SectionRule label="Reconcile all runs" />
      <StartCard
        tone="primary"
        icon="ph-trophy"
        eyebrow="Promote to Arena"
        title="Resolve the overlap between runs"
        blurb={`Replay with the union of ${allAgentIds.size} agents from prior runs. Duplicates collapse, conflicts get arbitrated, and you ship one consolidated list.`}
        meta={`~45s · ≈ $0.30`}
        cta="Configure & start"
        onClick={onNewArena}
      />
    </div>
  );
}

function RunCard({ run, isLatest, expanded, onToggle }) {
  const agents = run.agentIds.map((id) => window.AGENT_BY_ID[id]).filter(Boolean);
  const findings = run.findingIds.map((id) => window.FINDING_BY_ID[id]).filter(Boolean);
  const counts = { high: 0, med: 0, low: 0 };
  findings.forEach((f) => counts[f.severity]++);

  return (
    <div style={{
      background: 'var(--bg-0)',
      border: `1px solid ${isLatest ? 'rgba(127,135,255,0.30)' : 'var(--border)'}`,
      borderRadius: 8,
      overflow: 'hidden',
    }}>
      <button
        onClick={onToggle}
        style={{
          width: '100%', textAlign: 'left',
          padding: '8px 10px',
          background: isLatest ? 'linear-gradient(180deg, rgba(127,135,255,0.06), transparent)' : 'var(--bg-2)',
          border: 0, color: 'var(--fg)', fontFamily: 'inherit', cursor: 'pointer',
          display: 'flex', alignItems: 'center', gap: 8,
        }}
      >
        <i className={`ph ph-caret-${expanded ? 'down' : 'right'}`}
           style={{ fontSize: 10, color: 'var(--fg-subtle)', flexShrink: 0 }} />
        <span style={{ fontSize: 11, color: 'var(--fg)', fontWeight: 600 }}>{run.label}</span>
        {isLatest && (
          <span style={{
            fontSize: 8, fontWeight: 700, letterSpacing: '0.06em',
            padding: '1px 5px', borderRadius: 3,
            background: 'rgba(127,135,255,0.18)', color: 'var(--periwinkle)',
          }}>LATEST</span>
        )}
        <span style={{ fontSize: 9, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)' }}>
          {run.ranAt}
        </span>
        <div style={{ flex: 1 }} />
        <AgentDotStack agents={agents} size={14} />
        <span style={{
          fontSize: 9, fontWeight: 700,
          padding: '1px 5px', borderRadius: 999,
          background: 'rgba(255,255,255,0.07)', color: 'var(--fg-muted)',
        }}>{findings.length}</span>
        <span style={{ display: 'inline-flex', gap: 4 }}>
          {counts.high > 0 && <SevCount c={counts.high} color="var(--err)" />}
          {counts.med  > 0 && <SevCount c={counts.med}  color="var(--warn)" />}
          {counts.low  > 0 && <SevCount c={counts.low}  color="var(--info)" />}
        </span>
      </button>
      {expanded && (
        <div style={{ padding: '4px 12px 10px' }}>
          {findings.map((f, i) => (
            <CompactFinding key={f.id + i} f={f} />
          ))}
        </div>
      )}
    </div>
  );
}

function AgentDotStack({ agents, size }) {
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center' }}>
      {agents.map((a, i) => (
        <span key={a.id} title={a.name} style={{
          width: size, height: size, borderRadius: '50%',
          background: a.color, marginLeft: i === 0 ? 0 : -4,
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          border: '2px solid var(--bg-0)', zIndex: 10 - i,
        }}>
          <i className={`ph-bold ${a.icon}`} style={{ fontSize: size * 0.46, color: '#0e1420' }} />
        </span>
      ))}
    </span>
  );
}

// ─── State: MULTI-SPECIALISTS (3 ran independently, no arena) ─────────────
// User picks: one specialist at a time, OR severity-merged across all.
function ReviewMultiSpecialistsState({ onNewArena, onAddAgent }) {
  const ROSTER = ['general', 'security', 'professor'];
  const agents = ROSTER.map((id) => window.AGENT_BY_ID[id]).filter(Boolean);
  // Filters: which agent (or 'all'), and severity
  const [activeAgent, setActiveAgent] = useES('all');
  const [severity, setSeverity] = useES('high');   // 'all' | 'high' | 'med' | 'low'

  // Per-agent severity rollup (for chip badges)
  const perAgent = {};
  ROSTER.forEach((id) => {
    const r = window.DATA_AGENT_REVIEWS[id] || { findings: [] };
    const c = { high: 0, med: 0, low: 0 };
    r.findings.forEach((f) => c[f.severity]++);
    perAgent[id] = { total: r.findings.length, counts: c };
  });
  const allTotals = ROSTER.reduce((acc, id) => {
    const c = perAgent[id].counts;
    acc.high += c.high; acc.med += c.med; acc.low += c.low; acc.total += perAgent[id].total;
    return acc;
  }, { high: 0, med: 0, low: 0, total: 0 });

  // Build the merged feed — every finding carries its raising-agent id.
  const merged = [];
  ROSTER.forEach((agentId) => {
    (window.DATA_AGENT_REVIEWS[agentId]?.findings || []).forEach((f) => {
      merged.push({ ...f, agentId });
    });
  });
  // Order: high → med → low; within tier preserve agent grouping order
  const sevRank = (s) => s === 'high' ? 3 : s === 'med' ? 2 : 1;
  merged.sort((a, b) => sevRank(b.severity) - sevRank(a.severity));

  // Apply filters
  const visible = merged.filter((f) => {
    if (activeAgent !== 'all' && f.agentId !== activeAgent) return false;
    if (severity !== 'all' && f.severity !== severity) return false;
    return true;
  });

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <ReviewToolbar subtitle="3 independent reviews" onNewRun={onNewArena} />
      <StateHero
        icon="ph-users-three"
        iconBg="rgba(255,196,87,0.20)"
        title="3 specialists reviewed independently"
        body="No consensus run yet — each agent's findings stand on their own. Filter to compare, or run as an Arena to resolve overlaps."
      />

      {/* Agent picker row */}
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
        <AgentFilterChip
          label="All"
          icon="ph-stack"
          color="var(--periwinkle)"
          active={activeAgent === 'all'}
          count={allTotals.total}
          counts={allTotals}
          onClick={() => setActiveAgent('all')}
        />
        {agents.map((a) => (
          <AgentFilterChip
            key={a.id}
            label={a.name}
            agent={a}
            active={activeAgent === a.id}
            count={perAgent[a.id].total}
            counts={perAgent[a.id].counts}
            onClick={() => setActiveAgent(a.id)}
          />
        ))}
      </div>

      {/* Severity filter */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 10 }}>
        <span style={{ fontSize: 9, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--fg-subtle)', marginRight: 4 }}>
          Showing
        </span>
        <SeverityPill label="all"  count={allTotals.total} color={null}        active={severity === 'all'}  onClick={() => setSeverity('all')} />
        <SeverityPill label="high" count={allTotals.high}  color="var(--err)"  active={severity === 'high'} onClick={() => setSeverity('high')} />
        <SeverityPill label="med"  count={allTotals.med}   color="var(--warn)" active={severity === 'med'}  onClick={() => setSeverity('med')} />
        <SeverityPill label="low"  count={allTotals.low}   color="var(--info)" active={severity === 'low'}  onClick={() => setSeverity('low')} />
        <div style={{ flex: 1 }} />
        <span style={{ fontSize: 10, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)' }}>
          {visible.length} of {merged.length}
        </span>
      </div>

      {/* Merged feed */}
      {visible.length === 0 ? (
        <div style={{
          padding: 16, textAlign: 'center',
          fontSize: 11, color: 'var(--fg-subtle)',
          background: 'var(--bg-0)', border: '1px dashed var(--border)', borderRadius: 8,
        }}>
          No findings match the current filter.
        </div>
      ) : (
        <div>
          {visible.map((f, i) => (
            <MergedFindingRow key={f.id + f.agentId + i} f={f} agent={window.AGENT_BY_ID[f.agentId]} showAttribution={activeAgent === 'all'} />
          ))}
        </div>
      )}

      <SectionRule label="Resolve overlaps & disagreements" />
      <StartCard
        tone="primary"
        icon="ph-trophy"
        eyebrow="Promote to Arena"
        title="Make them debate it out"
        blurb="Same 3 reviewers — they cross-check, drop duplicates, and arbitrate severity. Final truth ships as one consolidated list."
        meta="~45s · ≈ $0.30"
        cta="Configure & start"
        onClick={onNewArena}
      />
    </div>
  );
}

function AgentFilterChip({ label, agent, active, count, counts, onClick, icon, color }) {
  const accent = color || agent?.color || 'var(--fg-muted)';
  const highest = counts.high > 0 ? 'high' : counts.med > 0 ? 'med' : counts.low > 0 ? 'low' : null;
  return (
    <button onClick={onClick} style={{
      display: 'inline-flex', alignItems: 'center', gap: 6,
      padding: '5px 9px',
      background: active ? `${accent}24` : 'var(--bg-2)',
      border: `1px solid ${active ? accent + '88' : 'var(--border)'}`,
      borderRadius: 999,
      color: active ? 'var(--fg)' : 'var(--fg-muted)',
      fontSize: 11, fontWeight: active ? 600 : 500, fontFamily: 'inherit',
      whiteSpace: 'nowrap', cursor: 'pointer',
      transition: 'all var(--d-fast) var(--ease)',
    }}>
      {agent ? (
        <span style={{
          width: 12, height: 12, borderRadius: '50%',
          background: agent.color, display: 'inline-flex',
          alignItems: 'center', justifyContent: 'center',
          boxShadow: `0 0 0 1px ${agent.color}33`,
        }}>
          <i className={`ph-bold ${agent.icon}`} style={{ fontSize: 6, color: '#0e1420' }} />
        </span>
      ) : (
        <i className={`ph-bold ${icon}`} style={{ fontSize: 10, color: accent }} />
      )}
      <span>{label}</span>
      <span style={{
        minWidth: 14, height: 14, padding: '0 4px', borderRadius: 999,
        background: active ? accent : 'rgba(255,255,255,0.07)',
        color: active ? '#0e1420' : 'var(--fg-muted)',
        fontSize: 9, fontWeight: 700,
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        lineHeight: 1,
      }}>{count}</span>
      {!active && highest && (
        <span style={{
          width: 5, height: 5, borderRadius: '50%',
          background: highest === 'high' ? 'var(--err)' : highest === 'med' ? 'var(--warn)' : 'var(--info)',
        }} />
      )}
    </button>
  );
}

function SeverityPill({ label, count, color, active, onClick }) {
  return (
    <button onClick={onClick} style={{
      display: 'inline-flex', alignItems: 'center', gap: 4,
      padding: '3px 8px', borderRadius: 4, border: 0,
      background: active ? 'rgba(255,255,255,0.06)' : 'transparent',
      color: active ? 'var(--fg)' : 'var(--fg-subtle)',
      fontSize: 10, fontFamily: 'inherit', cursor: 'pointer',
      borderBottom: active ? `1px solid ${color || 'var(--fg)'}` : '1px solid transparent',
    }}>
      {color && <span style={{ width: 5, height: 5, borderRadius: '50%', background: color }} />}
      {label}
      <span style={{ fontFamily: 'var(--font-mono)', fontWeight: 600 }}>{count}</span>
    </button>
  );
}

function MergedFindingRow({ f, agent, showAttribution }) {
  const sevColor = f.severity === 'high' ? 'var(--err)' : f.severity === 'med' ? 'var(--warn)' : 'var(--info)';
  return (
    <div style={{
      padding: '8px 0', borderTop: '1px solid var(--rule)',
      display: 'flex', flexDirection: 'column', gap: 4,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <span style={{ width: 6, height: 6, borderRadius: '50%', background: sevColor, flexShrink: 0 }} />
        <a href="#" style={{
          fontSize: 10, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)',
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', minWidth: 0, flex: 1,
        }}>
          {f.file}<span style={{ color: 'var(--periwinkle)' }}>:{f.line}</span>
        </a>
        {showAttribution && agent && (
          <span title={agent.name} style={{
            display: 'inline-flex', alignItems: 'center', gap: 4,
            padding: '1px 6px 1px 3px', borderRadius: 999,
            background: `${agent.color}1A`, border: `1px solid ${agent.color}44`,
          }}>
            <span style={{
              width: 10, height: 10, borderRadius: '50%', background: agent.color,
              display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            }}>
              <i className={`ph-bold ${agent.icon}`} style={{ fontSize: 5, color: '#0e1420' }} />
            </span>
            <span style={{ fontSize: 9, fontWeight: 600, color: 'var(--fg)' }}>{agent.name}</span>
          </span>
        )}
      </div>
      <div style={{ fontSize: 12, color: 'var(--fg)', lineHeight: 1.4, paddingLeft: 12 }}>{f.text}</div>
    </div>
  );
}
