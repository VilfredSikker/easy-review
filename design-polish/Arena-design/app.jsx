// app.jsx — top-level layout. Composes WindowChrome → ContextBar → main row
// (LeftRail · FilesRail · DiffView · RightRail) → optional TerminalDrawer.

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "rightPanelMode":     "tabs",
  "rightTab":           "review",
  "chromeMode":         "combined",
  "terminalPlacement":  "drawer",
  "density":            "comfy",
  "showContextBar":     true,
  "leftCollapsed":      false,
  "rightCollapsed":     false,
  "reviewPickerMode":   "chips",
  "reviewState":        "full",
  "arenaLayout":        "bracket",
  "arenaOpen":          false,
  "launcherOpen":       false,
  "launcherVariant":    "modal",
  "runningOpen":        false,
  "runningMinimized":   false
}/*EDITMODE-END*/;

const { useState: useA, useEffect: useAE } = React;

function App() {
  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);

  // Local non-persisted state
  const [activeProject, setActiveProject] = useA('discovery');
  const [activeTabId, setActiveTabId] = useA('b1');
  const [branchTabs, setBranchTabs] = useA(window.DATA_BRANCH_TABS);
  const [splitOpen, setSplitOpen] = useA(false);
  const [terminalOpen, setTerminalOpen] = useA(true);
  const [viewSource, setViewSource] = useA('pr');
  const [diffLayout, setDiffLayout] = useA('unified');

  const terminalInRight = t.terminalPlacement === 'right';
  const terminalAsDrawer = t.terminalPlacement === 'drawer' && terminalOpen;

  const [runConfig, setRunConfig] = useA(null);

  const startArena = (config) => {
    setRunConfig(config);
    setTweak({ launcherOpen: false, runningOpen: true });
  };
  const completeRun = () => {
    setTweak({ runningOpen: false, runningMinimized: false, arenaOpen: true });
  };

  const onTabSelect = (id) => setActiveTabId(id);
  const onTabClose  = (id) => {
    setBranchTabs((tabs) => tabs.filter((x) => x.id !== id));
  };
  const onNewTab = () => {
    const id = 'b' + (branchTabs.length + 1);
    setBranchTabs([...branchTabs, { id, label: 'main', repo: 'main', comments: 0 }]);
    setActiveTabId(id);
  };

  return (
    <div
      className="app"
      data-density={t.density}
      style={{
        position: 'fixed', inset: 0,
        display: 'flex', flexDirection: 'column',
        background: 'var(--bg-0)',
        color: 'var(--fg)',
        overflow: 'hidden',
      }}>

      <WindowChrome
        mode={t.chromeMode}
        tabs={branchTabs}
        activeId={activeTabId}
        onSelect={onTabSelect}
        onClose={onTabClose}
        onNew={onNewTab}
        onToggleLeftRail={() => setTweak('leftCollapsed', !t.leftCollapsed)}
        onToggleRightRail={() => setTweak('rightCollapsed', !t.rightCollapsed)}
      />

      {/* When chrome is "separate" we still need the branch tab strip somewhere.
          Render it on its own slim row beneath the chrome. */}
      {t.chromeMode === 'separate' && (
        <div style={{
          display: 'flex', alignItems: 'flex-end', gap: 4,
          padding: '6px 12px 0',
          background: 'var(--bg-1)',
          borderBottom: '1px solid var(--border)',
          flexShrink: 0,
        }}>
          <BranchTabStrip
            tabs={branchTabs} activeId={activeTabId}
            onSelect={onTabSelect} onClose={onTabClose} onNew={onNewTab}
          />
        </div>
      )}

      {t.showContextBar && (
        <ContextBar
          branch={window.DATA_BRANCH}
          splitOpen={splitOpen} onSplit={() => setSplitOpen(!splitOpen)}
          terminalOpen={t.terminalPlacement === 'drawer' && terminalOpen}
          onTerminal={() => setTerminalOpen(!terminalOpen)}
          viewSource={viewSource} onViewSource={setViewSource}
          diffLayout={diffLayout} onDiffLayout={setDiffLayout}
        />
      )}

      <div style={{
        flex: 1, display: 'flex', minHeight: 0,
      }}>
        <LeftRail
          projects={window.DATA_PROJECTS}
          inbox={window.DATA_INBOX}
          activeProject={activeProject}
          onProject={setActiveProject}
          onSelectBranch={() => {}}
          collapsed={t.leftCollapsed}
        />
        <FilesRail
          files={window.DATA_FILES}
          density={t.density}
          viewSource={viewSource}
          onViewSource={setViewSource}
        />
        <DiffView
          file={null}
          hunks={window.DATA_DIFF}
          splitOpen={splitOpen}
          rightCollapsed={t.rightCollapsed}
          onToggleRightRail={() => setTweak('rightCollapsed', !t.rightCollapsed)}
        />
        {splitOpen && <SplitBrowser onClose={() => setSplitOpen(false)} />}
        {!t.rightCollapsed ? (
          <RightRail
            mode={t.rightPanelMode}
            branch={window.DATA_BRANCH}
            ai={window.DATA_AI_REVIEW}
            questions={window.DATA_QUESTIONS}
            density={t.density}
            terminalInRight={terminalInRight}
            activeTab={t.rightTab}
            onTab={(id) => setTweak('rightTab', id)}
            onAskAI={() => {}}
            reviewPickerMode={t.reviewPickerMode}
            reviewState={t.reviewState}
            onOpenArena={() => setTweak('arenaOpen', true)}
            onNewRun={() => setTweak('launcherOpen', true)}
          />
        ) : (
          <RightRail
            collapsed
            branch={window.DATA_BRANCH}
            ai={window.DATA_AI_REVIEW}
            questions={window.DATA_QUESTIONS}
            onExpand={() => setTweak('rightCollapsed', false)}
            onTab={(id) => setTweak('rightTab', id)}
          />
        )}
      </div>

      <ArenaOverlay
        open={t.arenaOpen}
        onClose={() => setTweak('arenaOpen', false)}
        layoutMode={t.arenaLayout}
        onLayoutMode={(v) => setTweak('arenaLayout', v)}
        onNewRun={() => setTweak({ arenaOpen: false, launcherOpen: true })}
      />

      {window.ArenaLauncher && React.createElement(window.ArenaLauncher, {
        open: t.launcherOpen,
        variant: t.launcherVariant,
        onClose: () => setTweak('launcherOpen', false),
        onStart: startArena,
      })}

      {t.runningOpen && runConfig && window.ArenaRunningPanel && React.createElement(window.ArenaRunningPanel, {
        config: runConfig,
        minimized: t.runningMinimized,
        onCancel: () => setTweak({ runningOpen: false, runningMinimized: false }),
        onComplete: completeRun,
        onMinimize: () => setTweak('runningMinimized', true),
        onRestore: () => setTweak('runningMinimized', false),
      })}

      {terminalAsDrawer && (
        <TerminalDrawer
          branch={window.DATA_BRANCH}
          onClose={() => setTerminalOpen(false)}
        />
      )}

      <ToastHost />

      <TweaksPanel title="Tweaks">
        <TweakSection label="AI Review">
          <TweakSelect
            label="Review state"
            value={t.reviewState}
            onChange={(v) => setTweak('reviewState', v)}
            options={[
              { value: 'empty',             label: 'Nothing run yet' },
              { value: 'general-only',      label: 'Only General reviewed' },
              { value: 'specialized-only',  label: 'Only one specialist' },
              { value: 'multi-specialists', label: '3 specialists, independent' },
              { value: 'multi-runs',        label: 'Multiple separate runs' },
              { value: 'full',              label: 'Full multi-agent + arena' },
            ]}
          />
          <TweakRadio
            label="Agent picker (in rail)"
            value={t.reviewPickerMode}
            onChange={(v) => setTweak('reviewPickerMode', v)}
            options={[
              { value: 'chips',   label: 'Chips' },
              { value: 'stacked', label: 'Stacked' },
              { value: 'merged',  label: 'Merged' },
            ]}
          />
          <TweakRadio
            label="Launcher style"
            value={t.launcherVariant}
            onChange={(v) => setTweak('launcherVariant', v)}
            options={[
              { value: 'modal',  label: 'Modal' },
              { value: 'inline', label: 'Slide-over' },
            ]}
          />
          <TweakRadio
            label="Arena layout"
            value={t.arenaLayout}
            onChange={(v) => setTweak('arenaLayout', v)}
            options={[
              { value: 'bracket', label: 'Bracket' },
              { value: 'matrix',  label: 'Matrix' },
              { value: 'funnel',  label: 'Funnel' },
            ]}
          />
          <TweakButton
            label="Start new arena…"
            onClick={() => setTweak('launcherOpen', true)}
          />
          <TweakButton
            label={t.arenaOpen ? 'Close Arena' : 'Open Arena'}
            onClick={() => setTweak('arenaOpen', !t.arenaOpen)}
          />
        </TweakSection>

        <TweakSection label="Layout">
          <TweakRadio
            label="Right panel"
            value={t.rightPanelMode}
            onChange={(v) => setTweak('rightPanelMode', v)}
            options={[
              { value: 'tabs',      label: 'Tabs' },
              { value: 'stacked',   label: 'Stacked' },
              { value: 'accordion', label: 'Accordion' },
            ]}
          />
          <TweakRadio
            label="Chrome / branch tabs"
            value={t.chromeMode}
            onChange={(v) => setTweak('chromeMode', v)}
            options={[
              { value: 'combined', label: 'Combined' },
              { value: 'separate', label: 'Separate' },
            ]}
          />
          <TweakRadio
            label="Terminal"
            value={t.terminalPlacement}
            onChange={(v) => setTweak('terminalPlacement', v)}
            options={[
              { value: 'drawer', label: 'Drawer' },
              { value: 'right',  label: 'In tabs' },
              { value: 'off',    label: 'Hidden' },
            ]}
          />
        </TweakSection>

        <TweakSection label="Density & visibility">
          <TweakRadio
            label="Density"
            value={t.density}
            onChange={(v) => setTweak('density', v)}
            options={[
              { value: 'compact', label: 'Compact' },
              { value: 'comfy',   label: 'Comfy' },
            ]}
          />
          <TweakToggle label="Context bar visible"  value={t.showContextBar} onChange={(v) => setTweak('showContextBar', v)} />
          <TweakToggle label="Left rail collapsed"  value={t.leftCollapsed}  onChange={(v) => setTweak('leftCollapsed', v)} />
          <TweakToggle label="Right rail collapsed" value={t.rightCollapsed} onChange={(v) => setTweak('rightCollapsed', v)} />
        </TweakSection>

        <TweakSection label="Toasts (demo)">
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 6 }}>
            <DemoToastBtn onClick={() => window.toast.success('Diff force refreshed')} kind="success">Success</DemoToastBtn>
            <DemoToastBtn onClick={() => window.toast.info('Worktree synced from origin/main')} kind="info">Info</DemoToastBtn>
            <DemoToastBtn onClick={() => window.toast.warn('1 file has unresolved comments before push')} kind="warn">Warn</DemoToastBtn>
            <DemoToastBtn onClick={() => window.toast.error('Failed to push: remote rejected non-fast-forward update', { action: { label: 'Retry', onClick: () => window.toast.info('Retrying push…') } })} kind="error">Error</DemoToastBtn>
          </div>
        </TweakSection>
      </TweaksPanel>
    </div>
  );
}

// A placeholder for the "split view" browser the user described — opens to
// the right of the diff, replacing nothing, just narrowing it.
function SplitBrowser({ onClose }) {  return (
    <section style={{
      flex: 1, minWidth: 360,
      background: '#fff', color: '#222',
      display: 'flex', flexDirection: 'column',
      borderLeft: '1px solid var(--border-strong)',
    }}>
      <div style={{
        display: 'flex', alignItems: 'center', gap: 6,
        padding: '6px 8px',
        background: '#1a1f2c', color: 'var(--fg)',
        borderBottom: '1px solid var(--border)',
      }}>
        <i className="ph ph-arrow-left" style={{ fontSize: 12, color: 'var(--fg-subtle)' }} />
        <i className="ph ph-arrow-right" style={{ fontSize: 12, color: 'var(--fg-subtle)' }} />
        <i className="ph ph-arrows-clockwise" style={{ fontSize: 12, color: 'var(--fg-subtle)' }} />
        <div style={{
          flex: 1, height: 22, background: '#0b0f17', borderRadius: 4,
          padding: '0 8px', display: 'flex', alignItems: 'center',
          fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--fg-muted)',
        }}>localhost:5173</div>
        <button onClick={onClose} title="Close split" style={{
          width: 22, height: 22, border: 0, background: 'transparent',
          color: 'var(--fg-subtle)', borderRadius: 4,
        }}>
          <i className="ph ph-x" style={{ fontSize: 11 }} />
        </button>
      </div>
      <div style={{
        flex: 1, padding: 24,
        display: 'flex', flexDirection: 'column',
        alignItems: 'center', justifyContent: 'center', gap: 12,
        background: 'linear-gradient(180deg, #f7f8fa 0%, #eef0f4 100%)',
        color: '#666',
      }}>
        <i className="ph ph-browser" style={{ fontSize: 32, color: '#9ea4af' }} />
        <div style={{ fontSize: 13, color: '#555' }}>Preview server running</div>
        <div style={{ fontSize: 11, color: '#9ea4af' }}>Hot-reload tied to <code>main</code> worktree</div>
      </div>
    </section>
  );
}

ReactDOM.createRoot(document.getElementById('root')).render(<App />);

function DemoToastBtn({ kind, onClick, children }) {
  const palette = {
    success: { color: 'var(--ok)',         border: 'rgba(78,201,164,0.30)'  },
    info:    { color: 'var(--periwinkle)', border: 'rgba(127,135,255,0.25)' },
    warn:    { color: 'var(--warn)',       border: 'rgba(255,196,87,0.32)'  },
    error:   { color: 'var(--err)',        border: 'rgba(255,107,107,0.35)' },
  }[kind] || { color: 'var(--fg)', border: 'var(--border)' };
  return (
    <button onClick={onClick} style={{
      height: 26, padding: '0 10px',
      border: `1px solid ${palette.border}`,
      borderRadius: 5,
      background: 'transparent', color: palette.color,
      fontSize: 11, fontWeight: 500,
      cursor: 'pointer',
    }}>{children}</button>
  );
}
