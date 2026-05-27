// arena-launcher.jsx — How to START an AI Review Arena.
//
// Two layout variants exposed via Tweaks:
//   • modal  — centered overlay dialog (default; matches the polish of the arena itself)
//   • inline — slide-over panel from the right
//
// Mode toggle inside the launcher:
//   • models — pick 2–3 general LLMs (Opus 4.7, Sonnet 4.5, GPT-5.5, …)
//   • agents — pick 2–5 specialized roles (Security, Performance, …)
//
// Also exports a running/live state for after the user presses Start.

const { useState: useAL, useEffect: useALE, useMemo: useALM } = React;

function ArenaLauncher({ open, variant, onClose, onStart }) {
  if (!open) return null;
  if (variant === 'inline') return <LauncherInline onClose={onClose} onStart={onStart} />;
  return <LauncherModal onClose={onClose} onStart={onStart} />;
}

function useLauncherState() {
  const [mode, setMode] = useAL('models');
  const [selectedModels, setSelectedModels] = useAL(['opus-47', 'sonnet-45', 'gpt-55']);
  const [selectedAgents, setSelectedAgents] = useAL(['general', 'security', 'professor']);
  const [rounds, setRounds] = useAL(3);
  const [autoAccept, setAutoAccept] = useAL(0.75);
  const [scope, setScope] = useAL('branch');
  const [selectedFiles, setSelectedFiles] = useAL([]);
  const [filePickerOpen, setFilePickerOpen] = useAL(false);
  const [title, setTitle] = useAL('');

  const list   = mode === 'models' ? window.DATA_MODELS : window.DATA_AGENTS;
  const picked = mode === 'models' ? selectedModels : selectedAgents;
  const toggle = (id) => {
    if (mode === 'models') {
      setSelectedModels(picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id]);
    } else {
      setSelectedAgents(picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id]);
    }
  };

  const estimate = useALM(() => {
    if (mode === 'models') {
      const ms = (Math.max(0, ...picked.map((id) => window.MODEL_BY_ID[id]?.avgMs || 0))) * rounds * 0.85;
      const totalCost = picked.reduce((sum, id) => {
        const c = parseFloat(String(window.MODEL_BY_ID[id]?.costPer1k || '$0').replace('$',''));
        return sum + c;
      }, 0) * rounds * 8;
      return { ms, costStr: '$' + totalCost.toFixed(2) };
    }
    return { ms: 18000 * rounds, costStr: '$' + (picked.length * rounds * 0.18).toFixed(2) };
  }, [mode, picked, rounds]);

  const canStart = picked.length >= 1 && picked.length <= 6;

  // Smart title suggestion based on roster, scope, and rounds
  const suggestedTitle = useALM(() => {
    if (picked.length === 0) return 'New review run';
    const lookup = mode === 'models' ? window.MODEL_BY_ID : window.AGENT_BY_ID;
    const names = picked.map((id) => lookup[id]?.name).filter(Boolean);
    let body;
    if (mode === 'models') {
      body = names.length <= 3 ? names.join(' × ') : `${names.slice(0,2).join(' × ')} +${names.length - 2}`;
    } else {
      // Agents: emphasize the flavour
      if (names.length === 2) body = `${names[0]} + ${names[1]} review`;
      else if (names.length === 3) body = `${names.slice(0,2).join(' + ')} + ${names[2]}`;
      else body = `${names[0]} sweep (+${names.length - 1})`;
    }
    const prefix = rounds >= 3 ? 'Deep' : rounds === 2 ? 'Standard' : 'Quick';
    return `${prefix} · ${body}`;
  }, [mode, picked, rounds]);

  return {
    mode, setMode, list, picked, toggle, rounds, setRounds,
    autoAccept, setAutoAccept, scope, setScope, estimate, canStart,
    selectedFiles, setSelectedFiles, filePickerOpen, setFilePickerOpen,
    title, setTitle, suggestedTitle,
  };
}

// ═══ MODAL ═════════════════════════════════════════════════════════════════
function LauncherModal({ onClose, onStart }) {
  const s = useLauncherState();
  useALE(() => {
    const onKey = (e) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div onClick={onClose} style={{
      position: 'fixed', inset: 0, zIndex: 200,
      background: 'rgba(8,12,20,0.66)',
      backdropFilter: 'blur(4px)', WebkitBackdropFilter: 'blur(4px)',
      display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 24,
    }}>
      <div onClick={(e) => e.stopPropagation()} style={{
        width: 720, maxHeight: '90vh',
        display: 'flex', flexDirection: 'column',
        background: 'var(--bg-1)',
        border: '1px solid var(--border-strong)',
        borderRadius: 14,
        boxShadow: '0 24px 64px rgba(0,0,0,0.5), 0 0 0 1px rgba(127,135,255,0.10)',
        overflow: 'hidden',
      }}>
        <LauncherHeader onClose={onClose} compact={false} />
        <ModeToggle mode={s.mode} setMode={s.setMode} />
        <div style={{ flex: 1, overflowY: 'auto', padding: '12px 20px 16px' }}>
          <TitleField value={s.title} suggested={s.suggestedTitle} onChange={s.setTitle} />
          <ReviewerGrid {...s} />
          <Settings {...s} />
        </div>
        <Footer {...s} onClose={onClose} onStart={() => onStart({
          mode: s.mode, ids: s.picked, rounds: s.rounds,
          autoAccept: s.autoAccept, scope: s.scope,
          selectedFiles: s.scope === 'selected' ? s.selectedFiles : undefined,
          title: s.title.trim() || s.suggestedTitle,
        })} />
      </div>
      {window.FilePicker && React.createElement(window.FilePicker, {
        open: s.filePickerOpen,
        initialSelected: s.selectedFiles,
        onCancel: () => {
          s.setFilePickerOpen(false);
          if (s.scope === 'selected' && s.selectedFiles.length === 0) s.setScope('changed');
        },
        onConfirm: (ids) => {
          s.setSelectedFiles(ids);
          s.setFilePickerOpen(false);
          s.setScope('selected');
        },
      })}
    </div>
  );
}

// ═══ INLINE (slide-over) ═══════════════════════════════════════════════════
function LauncherInline({ onClose, onStart }) {
  const s = useLauncherState();
  useALE(() => {
    const onKey = (e) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <React.Fragment>
      <div onClick={onClose} style={{
        position: 'fixed', inset: 0, zIndex: 199,
        background: 'rgba(8,12,20,0.5)',
      }} />
      <div style={{
        position: 'fixed', top: 0, right: 0, bottom: 0, zIndex: 200,
        width: 420,
        background: 'var(--bg-1)',
        borderLeft: '1px solid var(--border-strong)',
        boxShadow: '-12px 0 32px rgba(0,0,0,0.32)',
        display: 'flex', flexDirection: 'column',
      }}>
        <LauncherHeader onClose={onClose} compact />
        <ModeToggle mode={s.mode} setMode={s.setMode} compact />
        <div style={{ flex: 1, overflowY: 'auto', padding: '8px 18px 14px' }}>
          <TitleField value={s.title} suggested={s.suggestedTitle} onChange={s.setTitle} compact />
          <ReviewerGrid {...s} compact />
          <Settings {...s} compact />
        </div>
        <Footer {...s} compact onClose={onClose} onStart={() => onStart({
          mode: s.mode, ids: s.picked, rounds: s.rounds,
          autoAccept: s.autoAccept, scope: s.scope,
          selectedFiles: s.scope === 'selected' ? s.selectedFiles : undefined,
          title: s.title.trim() || s.suggestedTitle,
        })} />
      </div>
      {window.FilePicker && React.createElement(window.FilePicker, {
        open: s.filePickerOpen,
        initialSelected: s.selectedFiles,
        onCancel: () => {
          s.setFilePickerOpen(false);
          if (s.scope === 'selected' && s.selectedFiles.length === 0) s.setScope('changed');
        },
        onConfirm: (ids) => {
          s.setSelectedFiles(ids);
          s.setFilePickerOpen(false);
          s.setScope('selected');
        },
      })}
    </React.Fragment>
  );
}

function LauncherHeader({ onClose, compact }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 10,
      padding: compact ? '14px 18px' : '16px 20px',
      borderBottom: '1px solid var(--border)',
      background: compact ? 'transparent' : 'linear-gradient(180deg, rgba(127,135,255,0.06), transparent)',
    }}>
      <i className="ph-fill ph-trophy" style={{ fontSize: compact ? 14 : 16, color: 'var(--periwinkle)' }} />
      <div style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
        <span style={{ fontSize: compact ? 12 : 14, fontWeight: 600, color: 'var(--fg)' }}>
          {compact ? 'New Arena run' : 'Start AI Review Arena'}
        </span>
        {!compact && (
          <span style={{ fontSize: 11, color: 'var(--fg-muted)' }}>
            Pick reviewers, let them debate, ship the consensus.
          </span>
        )}
      </div>
      <div style={{ flex: 1 }} />
      <button onClick={onClose} title="Close (Esc)" style={iconBtnClose}>
        <i className="ph ph-x" style={{ fontSize: compact ? 11 : 12 }} />
      </button>
    </div>
  );
}

function TitleField({ value, suggested, onChange, compact }) {
  const [focused, setFocused] = useAL(false);
  return (
    <div style={{ marginBottom: 14 }}>
      <div style={{
        fontSize: 10, fontWeight: 700, letterSpacing: '0.08em',
        textTransform: 'uppercase', color: 'var(--fg-subtle)',
        margin: '0 0 6px',
      }}>Title</div>
      <div style={{
        display: 'flex', alignItems: 'center', gap: 8,
        padding: '8px 12px',
        background: 'var(--bg-0)',
        border: '1px solid ' + (focused ? 'var(--periwinkle)' : 'var(--border)'),
        borderRadius: 8,
        transition: 'border-color var(--d-fast) var(--ease)',
      }}>
        <i className="ph ph-tag" style={{ fontSize: 12, color: 'var(--fg-subtle)', flexShrink: 0 }} />
        <input
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          placeholder={suggested}
          style={{
            flex: 1, background: 'transparent', border: 0, outline: 0,
            color: 'var(--fg)', fontSize: 13, fontFamily: 'inherit',
            minWidth: 0,
          }}
        />
        {!value && (
          <button
            type="button"
            onClick={() => onChange(suggested)}
            title="Use suggestion"
            style={{
              display: 'inline-flex', alignItems: 'center', gap: 4,
              padding: '2px 7px', borderRadius: 4,
              background: 'transparent', border: '1px solid var(--border)',
              color: 'var(--fg-muted)', fontSize: 10, fontFamily: 'inherit', cursor: 'pointer',
              whiteSpace: 'nowrap',
            }}>
            <i className="ph ph-sparkle" style={{ fontSize: 9 }} />
            Use suggestion
          </button>
        )}
        {value && (
          <button
            type="button"
            onClick={() => onChange('')}
            title="Clear"
            style={{
              display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
              width: 18, height: 18, borderRadius: 4,
              background: 'transparent', border: 0,
              color: 'var(--fg-subtle)', cursor: 'pointer',
            }}>
            <i className="ph ph-x" style={{ fontSize: 10 }} />
          </button>
        )}
      </div>
      <div style={{ fontSize: 10, color: 'var(--fg-subtle)', marginTop: 6, paddingLeft: 2 }}>
        Shown in run history. Leave blank to use the auto-generated title.
      </div>
    </div>
  );
}

function ModeToggle({ mode, setMode, compact }) {
  const opts = [
    { id: 'models', label: 'General models',     icon: 'ph-cube',           desc: 'Same prompt across frontier LLMs' },
    { id: 'agents', label: 'Specialized agents', icon: 'ph-graduation-cap', desc: 'A different agent for each lens' },
  ];
  return (
    <div style={{
      padding: compact ? '8px 18px' : '10px 20px',
      borderBottom: '1px solid var(--border)',
      background: 'var(--bg-0)',
    }}>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 6 }}>
        {opts.map((o) => {
          const active = mode === o.id;
          return (
            <button key={o.id} onClick={() => setMode(o.id)} style={{
              textAlign: 'left',
              padding: compact ? '8px 10px' : '10px 12px',
              background: active ? 'var(--bg-2)' : 'transparent',
              border: '1px solid ' + (active ? 'var(--periwinkle)' : 'var(--border)'),
              borderRadius: 8,
              color: active ? 'var(--fg)' : 'var(--fg-muted)',
              fontFamily: 'inherit', cursor: 'pointer',
              display: 'flex', flexDirection: 'column', gap: 2,
            }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12, fontWeight: 600, whiteSpace: 'nowrap' }}>
                <i className={'ph ' + o.icon} style={{ fontSize: 12, color: active ? 'var(--periwinkle)' : 'inherit' }} />
                {o.label}
                {active && <i className="ph-fill ph-check-circle" style={{ fontSize: 11, color: 'var(--periwinkle)', marginLeft: 'auto' }} />}
              </div>
              <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>{o.desc}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function ReviewerGrid({ mode, list, picked, toggle, compact }) {
  return (
    <div>
      <div style={{
        fontSize: 10, fontWeight: 700, letterSpacing: '0.08em',
        textTransform: 'uppercase', color: 'var(--fg-subtle)',
        margin: '0 0 8px',
      }}>
        {mode === 'models' ? 'Choose models' : 'Choose agents'}
        <span style={{ color: 'var(--fg-subtle)', fontWeight: 500, letterSpacing: 0, textTransform: 'none', marginLeft: 6 }}>
          {picked.length} selected
        </span>
      </div>
      <div style={{
        display: 'grid',
        gridTemplateColumns: compact ? '1fr' : '1fr 1fr',
        gap: 6,
      }}>
        {list.map((item) => (
          <ReviewerCard key={item.id} item={item} mode={mode}
            picked={picked.includes(item.id)} onToggle={() => toggle(item.id)} />
        ))}
      </div>
    </div>
  );
}

function ReviewerCard({ item, mode, picked, onToggle }) {
  return (
    <button onClick={onToggle} style={{
      textAlign: 'left',
      padding: '10px 12px',
      background: picked ? (item.color + '1A') : 'var(--bg-0)',
      border: '1px solid ' + (picked ? item.color + '88' : 'var(--border)'),
      borderRadius: 8,
      color: 'var(--fg)', fontFamily: 'inherit', cursor: 'pointer',
      display: 'flex', alignItems: 'center', gap: 10,
    }}>
      <span style={{
        width: 32, height: 32, borderRadius: 8,
        background: picked ? item.color : 'var(--bg-2)',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        flexShrink: 0,
      }}>
        <i className={'ph-bold ' + item.icon} style={{ fontSize: 16, color: picked ? '#0e1420' : item.color }} />
      </span>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 1, minWidth: 0, flex: 1 }}>
        <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--fg)' }}>{item.name}</span>
        <span style={{ fontSize: 10, color: 'var(--fg-subtle)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {item.tagline}
        </span>
        <span style={{ fontSize: 10, color: 'var(--fg-faint)', fontFamily: 'var(--font-mono)' }}>
          {mode === 'models' ? (item.vendor + ' · ' + item.costPer1k + '/1k') : item.model}
        </span>
      </div>
      <span style={{
        width: 16, height: 16, borderRadius: 4,
        background: picked ? item.color : 'transparent',
        border: '1px solid ' + (picked ? item.color : 'var(--border-strong)'),
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        flexShrink: 0,
      }}>
        {picked && <i className="ph-bold ph-check" style={{ fontSize: 10, color: '#0e1420' }} />}
      </span>
    </button>
  );
}

function Settings({ rounds, setRounds, autoAccept, setAutoAccept, scope, setScope, selectedFiles, setSelectedFiles, filePickerOpen, setFilePickerOpen, compact }) {
  const fileCount = selectedFiles.length;
  const scopeHint =
    scope === 'branch'    ? 'Review every file on this branch · in repo context' :
    fileCount > 0         ? `${fileCount} file${fileCount === 1 ? '' : 's'} chosen · still reviewed in branch/repo context` :
                            'Narrow to specific files — still in branch/repo context';
  return (
    <div style={{ marginTop: 14 }}>
      <div style={{
        fontSize: 10, fontWeight: 700, letterSpacing: '0.08em',
        textTransform: 'uppercase', color: 'var(--fg-subtle)',
        margin: '0 0 8px',
      }}>Run settings</div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <SettingRow label="Rounds" hint={
          rounds === 1 ? 'Propose only — no cross-check' :
          rounds === 2 ? 'Propose + cross-check' :
          rounds === 3 ? 'Full debate + resolve' :
          rounds + ' rounds (deep)'
        }>
          <SegmentedNumber value={rounds} onChange={setRounds} min={1} max={5} />
        </SettingRow>

        <SettingRow label="Auto-accept" hint={'Findings ≥ ' + (autoAccept * 100).toFixed(0) + '% confidence ship without review'}>
          <input type="range" min={0.5} max={0.95} step={0.05}
            value={autoAccept} onChange={(e) => setAutoAccept(parseFloat(e.target.value))}
            style={{ width: 140, accentColor: 'var(--periwinkle)' }} />
          <span style={{ fontSize: 11, fontFamily: 'var(--font-mono)', color: 'var(--fg)', minWidth: 32 }}>
            {(autoAccept * 100).toFixed(0)}%
          </span>
        </SettingRow>

        <SettingRow label="Scope" hint={scopeHint}>
          <ScopeSegmented
            value={scope}
            onChange={(v) => {
              setScope(v);
              if (v === 'selected' && selectedFiles.length === 0) setFilePickerOpen(true);
            }}
            onEditSelection={() => setFilePickerOpen(true)}
            fileCount={fileCount}
          />
        </SettingRow>
      </div>
    </div>
  );
}

function SettingRow({ label, hint, children }) {
  return (
    <div style={{
      display: 'grid', gridTemplateColumns: '120px 1fr', gap: 12,
      padding: '8px 10px',
      background: 'var(--bg-0)', border: '1px solid var(--border)', borderRadius: 8,
      alignItems: 'center',
    }}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
        <span style={{ fontSize: 11, color: 'var(--fg)', fontWeight: 500 }}>{label}</span>
        <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>{hint}</span>
      </div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>{children}</div>
    </div>
  );
}

function SegmentedNumber({ value, onChange, min, max }) {
  const opts = [];
  for (let i = min; i <= max; i++) opts.push(i);
  return (
    <div style={{
      display: 'inline-flex', padding: 2, borderRadius: 6,
      background: 'var(--bg-2)', border: '1px solid var(--border)',
    }}>
      {opts.map((v) => (
        <button key={v} onClick={() => onChange(v)} style={{
          width: 28, height: 24, borderRadius: 4, border: 0,
          background: value === v ? 'var(--periwinkle)' : 'transparent',
          color: value === v ? '#0e1420' : 'var(--fg-muted)',
          fontSize: 11, fontWeight: 600, fontFamily: 'var(--font-mono)', cursor: 'pointer',
        }}>{v}</button>
      ))}
    </div>
  );
}

function ScopeSegmented({ value, onChange, onEditSelection, fileCount }) {
  const opts = [
    { id: 'branch',   label: 'Branch',   icon: 'ph-git-branch' },
    { id: 'selected', label: fileCount > 0 ? `Selected (${fileCount})` : 'Selected files…', icon: 'ph-list-checks' },
  ];
  return (
    <div style={{
      display: 'inline-flex', padding: 2, borderRadius: 6,
      background: 'var(--bg-2)', border: '1px solid var(--border)',
    }}>
      {opts.map((o) => {
        const active = value === o.id;
        return (
          <button key={o.id} onClick={() => {
            if (active && o.id === 'selected') onEditSelection();
            else onChange(o.id);
          }}
            style={{
              display: 'inline-flex', alignItems: 'center', gap: 5,
              padding: '4px 10px', borderRadius: 4, border: 0,
              background: active ? 'var(--bg-4)' : 'transparent',
              color: active ? 'var(--fg)' : 'var(--fg-muted)',
              fontSize: 11, fontWeight: 500, fontFamily: 'inherit', cursor: 'pointer',
              whiteSpace: 'nowrap',
            }}>
            {o.icon && <i className={'ph ' + o.icon} style={{ fontSize: 11 }} />}
            {o.label}
            {active && o.id === 'selected' && (
              <i className="ph ph-pencil-simple" style={{ fontSize: 9, opacity: 0.7, marginLeft: 2 }} />
            )}
          </button>
        );
      })}
    </div>
  );
}

function SegmentedText({ value, onChange, options }) {
  return (
    <div style={{
      display: 'inline-flex', padding: 2, borderRadius: 6,
      background: 'var(--bg-2)', border: '1px solid var(--border)',
    }}>
      {options.map((o) => (
        <button key={o.id} onClick={() => onChange(o.id)} style={{
          padding: '4px 10px', borderRadius: 4, border: 0,
          background: value === o.id ? 'var(--bg-4)' : 'transparent',
          color: value === o.id ? 'var(--fg)' : 'var(--fg-muted)',
          fontSize: 11, fontWeight: 500, fontFamily: 'inherit', cursor: 'pointer',
        }}>{o.label}</button>
      ))}
    </div>
  );
}

function Footer({ canStart, picked, estimate, mode, onClose, onStart, compact }) {
  const dur = (estimate.ms / 1000).toFixed(0);
  const isArena = picked.length >= 2;
  return (
    <div style={{
      padding: compact ? '12px 18px' : '14px 20px',
      borderTop: '1px solid var(--border)',
      background: 'var(--bg-0)',
      display: 'flex', alignItems: 'center', gap: 10,
    }}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0, flex: 1 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <ReviewerStack ids={picked} mode={mode} max={6} size={18} />
          {!canStart && (
            <span style={{ fontSize: 10, color: 'var(--err)' }}>Pick at least 1</span>
          )}
          {canStart && (
            <span style={{ fontSize: 10, color: 'var(--fg-subtle)', fontWeight: 600, letterSpacing: '0.06em', textTransform: 'uppercase' }}>
              {isArena ? `Arena · ${picked.length} reviewers` : 'Single review'}
            </span>
          )}
        </div>
        <div style={{ fontSize: 10, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)' }}>
          ~{dur}s · est. {estimate.costStr}
        </div>
      </div>
      <button onClick={onClose} style={btnSec}>Cancel</button>
      <button onClick={canStart ? onStart : undefined} style={{
        ...btnPri,
        opacity: canStart ? 1 : 0.5,
        cursor: canStart ? 'pointer' : 'not-allowed',
        whiteSpace: 'nowrap',
      }}>
        <i className="ph-bold ph-play" style={{ fontSize: 11 }} />
        {isArena ? 'Start arena' : 'Start review'}
        <span style={{ marginLeft: 4, opacity: 0.7 }} className="kbd">⏎</span>
      </button>
    </div>
  );
}

function ReviewerStack({ ids, mode, max, size }) {
  const lookup = mode === 'models' ? window.MODEL_BY_ID : window.AGENT_BY_ID;
  const shown = ids.slice(0, max);
  const overflow = ids.length - shown.length;
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center' }}>
      {shown.map((id, i) => {
        const a = lookup[id]; if (!a) return null;
        return (
          <span key={id} title={a.name} style={{
            width: size, height: size, borderRadius: '50%',
            background: a.color, marginLeft: i === 0 ? 0 : -5,
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            border: '2px solid var(--bg-0)', zIndex: shown.length - i,
          }}>
            <i className={'ph-bold ' + a.icon} style={{ fontSize: size * 0.5, color: '#0e1420' }} />
          </span>
        );
      })}
      {overflow > 0 && (
        <span style={{
          width: size, height: size, borderRadius: '50%', background: 'var(--bg-3)',
          color: 'var(--fg-muted)', fontSize: 9, fontWeight: 700,
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          border: '2px solid var(--bg-0)', marginLeft: -5,
        }}>+{overflow}</span>
      )}
    </span>
  );
}

const iconBtnClose = {
  width: 28, height: 28, borderRadius: 6,
  border: '1px solid var(--border)', background: 'var(--bg-2)',
  color: 'var(--fg-muted)', cursor: 'pointer',
  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
};
const btnPri = {
  display: 'inline-flex', alignItems: 'center', gap: 6,
  height: 32, padding: '0 14px', borderRadius: 6,
  background: 'var(--periwinkle)', color: '#fff',
  border: 0, fontSize: 12, fontWeight: 600, fontFamily: 'inherit',
};
const btnSec = {
  display: 'inline-flex', alignItems: 'center', gap: 6,
  height: 32, padding: '0 14px', borderRadius: 6,
  background: 'var(--bg-2)', color: 'var(--fg)',
  border: '1px solid var(--border)', fontSize: 12, fontWeight: 500, fontFamily: 'inherit', cursor: 'pointer',
};

// ─── Running state ─────────────────────────────────────────────────────────
function ArenaRunningPanel({ config, minimized, onCancel, onComplete, onMinimize, onRestore }) {
  const lookup = config.mode === 'models' ? window.MODEL_BY_ID : window.AGENT_BY_ID;
  const reviewers = config.ids.map((id) => lookup[id]).filter(Boolean);
  const isArena = config.ids.length >= 2;
  const totalRounds = isArena ? config.rounds : 1;
  const roundDur = isArena ? 2.4 : 4.0;

  const [progress, setProgress] = useAL({ round: 1, reviewerIndex: 0, t: 0 });
  useALE(() => {
    const start = Date.now();
    const id = setInterval(() => {
      const t = (Date.now() - start) / 1000;
      const round = Math.floor(t / roundDur) + 1;
      const inRound = (t % roundDur) / roundDur;
      const reviewerIndex = Math.min(reviewers.length - 1, Math.floor(inRound * reviewers.length));
      if (round > totalRounds) {
        clearInterval(id);
        if (onComplete) onComplete();
        return;
      }
      setProgress({ round, reviewerIndex, t });
    }, 140);
    return () => clearInterval(id);
  }, []);

  const pct = Math.min(1, progress.t / (totalRounds * roundDur));

  if (minimized) {
    return <ArenaRunningPill
      reviewers={reviewers} isArena={isArena}
      totalRounds={totalRounds} progress={progress} pct={pct}
      onRestore={onRestore} onCancel={onCancel}
    />;
  }

  return (
    <div style={{
      position: 'fixed', inset: 0, zIndex: 150,
      background: 'rgba(8,12,20,0.82)',
      backdropFilter: 'blur(6px)', WebkitBackdropFilter: 'blur(6px)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
    }}>
      <style>{`@keyframes spin{to{transform:rotate(360deg)}} @keyframes pulse-dot{0%,100%{opacity:1}50%{opacity:.3}}`}</style>
      <div style={{
        width: 520,
        background: 'var(--bg-1)',
        border: '1px solid var(--border-strong)',
        borderRadius: 14,
        boxShadow: '0 24px 64px rgba(0,0,0,0.5), 0 0 0 1px rgba(127,135,255,0.20)',
        padding: 24,
        display: 'flex', flexDirection: 'column', gap: 18,
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <span style={{
            width: 36, height: 36, borderRadius: '50%',
            background: 'rgba(127,135,255,0.16)',
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            animation: 'spin 2s linear infinite',
          }}>
            <i className={`ph-fill ${isArena ? 'ph-trophy' : 'ph-magic-wand'}`} style={{ fontSize: 16, color: 'var(--periwinkle)' }} />
          </span>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0, flex: 1 }}>
            <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--fg)', whiteSpace: 'nowrap' }}>
              {isArena
                ? `Arena in progress · Round ${Math.min(progress.round, totalRounds)} of ${totalRounds}`
                : 'Review in progress'}
            </span>
            <span style={{ fontSize: 11, color: 'var(--fg-muted)' }}>
              {isArena
                ? (progress.round === 1 ? 'Each reviewer proposing independently' :
                   progress.round === 2 ? 'Cross-checking findings' :
                   progress.round === 3 ? 'Resolving conflicts' :
                   'Final pass')
                : `${reviewers[0]?.name || 'Reviewer'} is reading the diff`}
            </span>
          </div>
          <button onClick={onMinimize} title="Run in background"
            style={{
              display: 'inline-flex', alignItems: 'center', gap: 4,
              padding: '4px 10px', borderRadius: 6,
              background: 'var(--bg-2)', border: '1px solid var(--border)',
              color: 'var(--fg-muted)', fontSize: 11, fontFamily: 'inherit', cursor: 'pointer',
              whiteSpace: 'nowrap',
            }}
            onMouseOver={(e) => { e.currentTarget.style.borderColor = 'var(--border-strong)'; e.currentTarget.style.color = 'var(--fg)'; }}
            onMouseOut ={(e) => { e.currentTarget.style.borderColor = 'var(--border)'; e.currentTarget.style.color = 'var(--fg-muted)'; }}
          >
            <i className="ph ph-arrow-down-right" style={{ fontSize: 11 }} />
            Run in background
          </button>
        </div>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          {reviewers.map((r, i) => {
            const active = i === progress.reviewerIndex;
            const done = i < progress.reviewerIndex || progress.round > 1;
            return (
              <div key={r.id} style={{
                display: 'flex', alignItems: 'center', gap: 10,
                padding: '8px 10px', borderRadius: 6,
                background: active ? (r.color + '1A') : 'var(--bg-0)',
                border: '1px solid ' + (active ? r.color + '88' : 'var(--border)'),
              }}>
                <span style={{
                  width: 22, height: 22, borderRadius: 6,
                  background: r.color,
                  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
                }}>
                  <i className={'ph-bold ' + r.icon} style={{ fontSize: 12, color: '#0e1420' }} />
                </span>
                <span style={{ fontSize: 12, color: 'var(--fg)', fontWeight: 500, flex: 1 }}>{r.name}</span>
                {active ? (
                  <span style={{ fontSize: 10, color: r.color, fontWeight: 600, display: 'inline-flex', gap: 4, alignItems: 'center' }}>
                    <span style={{ width: 6, height: 6, borderRadius: '50%', background: r.color, animation: 'pulse-dot 1.2s infinite' }} />
                    Thinking…
                  </span>
                ) : done ? (
                  <i className="ph-fill ph-check-circle" style={{ fontSize: 14, color: 'var(--ok)' }} />
                ) : (
                  <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>Queued</span>
                )}
              </div>
            );
          })}
        </div>

        {isArena && (
          <div style={{ display: 'flex', gap: 4 }}>
            {Array.from({ length: totalRounds }, (_, i) => (
              <span key={i} style={{
                flex: 1, height: 4, borderRadius: 2,
                background: i + 1 < progress.round ? 'var(--periwinkle)' :
                            i + 1 === progress.round ? 'rgba(127,135,255,0.4)' : 'var(--bg-3)',
              }} />
            ))}
          </div>
        )}

        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: 10, color: 'var(--fg-subtle)', fontFamily: 'var(--font-mono)', flex: 1 }}>
            {progress.t.toFixed(1)}s elapsed
          </span>
          <button onClick={onCancel} style={{ ...btnSec, whiteSpace: 'nowrap' }}>Cancel</button>
          <button onClick={onComplete} style={{ ...btnSec, color: 'var(--fg)', whiteSpace: 'nowrap' }}>Skip to results</button>
        </div>
      </div>
    </div>
  );
}

// ─── Floating pill (minimized running state) ───────────────────────────────
function ArenaRunningPill({ reviewers, isArena, totalRounds, progress, pct, onRestore, onCancel }) {
  const radius = 14;
  const circ = 2 * Math.PI * radius;
  const offset = circ * (1 - pct);
  const active = reviewers[progress.reviewerIndex];

  return (
    <div style={{
      position: 'fixed', bottom: 80, right: 20, zIndex: 150,
      animation: 'pill-in 240ms var(--ease) both',
    }}>
      <style>{`
        @keyframes pill-in { from { opacity: 0; transform: translateY(8px) scale(0.95); } to { opacity: 1; transform: none; } }
        @keyframes pulse-dot { 0%, 100% { opacity: 1; } 50% { opacity: .3; } }
      `}</style>
      <div
        onClick={onRestore}
        style={{
          display: 'flex', alignItems: 'center', gap: 10,
          padding: '8px 8px 8px 8px',
          background: 'var(--bg-1)',
          border: '1px solid var(--border-strong)',
          borderRadius: 999,
          boxShadow: '0 10px 28px rgba(0,0,0,0.45), 0 0 0 1px rgba(127,135,255,0.18)',
          color: 'var(--fg)', fontFamily: 'inherit', cursor: 'pointer',
          maxWidth: 320, minWidth: 240,
        }}
        onMouseOver={(e) => { e.currentTarget.style.borderColor = 'rgba(127,135,255,0.55)'; }}
        onMouseOut ={(e) => { e.currentTarget.style.borderColor = 'var(--border-strong)'; }}
        title="Open progress"
      >
        <span style={{
          position: 'relative', width: 34, height: 34, flexShrink: 0,
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        }}>
          <svg width="34" height="34" viewBox="0 0 34 34" style={{ position: 'absolute', inset: 0, transform: 'rotate(-90deg)' }}>
            <circle cx="17" cy="17" r={radius} fill="none" stroke="var(--bg-3)" strokeWidth="2.5" />
            <circle cx="17" cy="17" r={radius} fill="none"
              stroke="var(--periwinkle)" strokeWidth="2.5"
              strokeDasharray={circ} strokeDashoffset={offset}
              strokeLinecap="round"
              style={{ transition: 'stroke-dashoffset 200ms linear' }}
            />
          </svg>
          <i className={`ph-fill ${isArena ? 'ph-trophy' : 'ph-magic-wand'}`}
             style={{ fontSize: 12, color: 'var(--periwinkle)', position: 'relative', zIndex: 1 }} />
        </span>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 1, minWidth: 0, flex: 1 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            <span style={{ fontSize: 11, color: 'var(--fg)', fontWeight: 600, whiteSpace: 'nowrap' }}>
              {isArena ? `Arena · Round ${Math.min(progress.round, totalRounds)} / ${totalRounds}` : 'Review running'}
            </span>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 10, color: 'var(--fg-subtle)' }}>
            {active ? (
              <React.Fragment>
                <span style={{
                  width: 8, height: 8, borderRadius: '50%',
                  background: active.color, flexShrink: 0,
                  animation: 'pulse-dot 1.2s infinite',
                }} />
                <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {active.name} thinking…
                </span>
              </React.Fragment>
            ) : (
              <span>{progress.t.toFixed(1)}s elapsed</span>
            )}
          </div>
        </div>

        <span
          onClick={(e) => { e.stopPropagation(); onCancel(); }}
          title="Cancel run"
          style={{
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            width: 24, height: 24, borderRadius: 6,
            background: 'transparent',
            color: 'var(--fg-subtle)', cursor: 'pointer', flexShrink: 0,
            transition: 'background var(--d-fast) var(--ease), color var(--d-fast) var(--ease)',
          }}
          onMouseOver={(e) => { e.currentTarget.style.background = 'var(--bg-2)'; e.currentTarget.style.color = 'var(--err)'; }}
          onMouseOut ={(e) => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--fg-subtle)'; }}
        >
          <i className="ph ph-x" style={{ fontSize: 11 }} />
        </span>
      </div>
    </div>
  );
}

Object.assign(window, { ArenaLauncher, ArenaRunningPanel });
