// toast.jsx — small system toasts at bottom-left.
// Matches the existing pattern from the screenshot: dark card, mono-feeling
// label, subtle border, stacked when multiple are live. Successes & info
// auto-dismiss after a few seconds; errors persist until manually closed.

const { useState: useT, useEffect: useTE, useCallback: useTC, useRef: useTR } = React;

// ─────────────────────────────────────────────────────────────
// Public API — window.toast.{success,info,warn,error}('message', opts?)
// Wire ToastHost once at app root.
// ─────────────────────────────────────────────────────────────
const _toastListeners = new Set();
let _nextToastId = 1;

function publishToast(t) {
  const toast = {
    id: _nextToastId++,
    kind: 'info',
    persist: false,
    durationMs: 3200,
    ...t,
  };
  // Errors persist by default; the kind override wins unless explicit.
  if (toast.kind === 'error' && t.persist == null) toast.persist = true;
  _toastListeners.forEach((fn) => fn({ type: 'add', toast }));
  return toast.id;
}
function dismissToast(id) {
  _toastListeners.forEach((fn) => fn({ type: 'remove', id }));
}

// Expose ergonomic helpers.
window.toast = {
  success: (msg, opts) => publishToast({ ...opts, kind: 'success', message: msg }),
  info:    (msg, opts) => publishToast({ ...opts, kind: 'info',    message: msg }),
  warn:    (msg, opts) => publishToast({ ...opts, kind: 'warn',    message: msg }),
  error:   (msg, opts) => publishToast({ ...opts, kind: 'error',   message: msg }),
  dismiss: dismissToast,
};

// ─────────────────────────────────────────────────────────────
// ToastHost — render at app root once.
// ─────────────────────────────────────────────────────────────
function ToastHost() {
  const [toasts, setToasts] = useT([]);

  useTE(() => {
    const handler = (event) => {
      if (event.type === 'add')    setToasts((t) => [...t, event.toast]);
      if (event.type === 'remove') setToasts((t) => t.filter((x) => x.id !== event.id));
    };
    _toastListeners.add(handler);
    return () => _toastListeners.delete(handler);
  }, []);

  return (
    <div style={{
      position: 'fixed', right: 16, bottom: 18,
      display: 'flex', flexDirection: 'column-reverse', gap: 8,
      zIndex: 60, pointerEvents: 'none',
      alignItems: 'flex-end',
    }}>
      {toasts.map((t) => (
        <ToastCard key={t.id} toast={t} onClose={() => dismissToast(t.id)} />
      ))}
      <style>{`
        @keyframes toast-in {
          from { opacity: 0; transform: translateX(8px) scale(0.98); }
          to   { opacity: 1; transform: none; }
        }
      `}</style>
    </div>
  );
}

function ToastCard({ toast, onClose }) {
  const [hover, setHover] = useT(false);
  const [expanded, setExpanded] = useT(false);
  const [copied, setCopied] = useT(false);
  const timerRef = useTR(null);
  const msgRef = useTR(null);
  const [overflowing, setOverflowing] = useT(false);

  // Detect when the (collapsed) message exceeds its allotted height so the
  // "Show more" affordance only appears when there's actually more to show.
  useTE(() => {
    if (!msgRef.current) return;
    const el = msgRef.current;
    setOverflowing(el.scrollHeight - 2 > el.clientHeight);
  }, [toast.message, expanded]);

  useTE(() => {
    if (toast.persist) return;
    if (hover) { if (timerRef.current) { clearTimeout(timerRef.current); timerRef.current = null; } return; }
    timerRef.current = setTimeout(onClose, toast.durationMs);
    return () => clearTimeout(timerRef.current);
  }, [hover, toast.persist, toast.durationMs, onClose]);

  const palette = TOAST_PALETTE[toast.kind] || TOAST_PALETTE.info;
  const isPersistent = toast.persist || toast.kind === 'error' || toast.kind === 'warn';
  const isMultiline = toast.kind === 'error' || toast.kind === 'warn';
  const collapsedMaxLines = toast.kind === 'error' ? 3 : 2;

  const copyMessage = async () => {
    try {
      await navigator.clipboard.writeText(toast.message);
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    } catch (e) {
      // fallback for environments without clipboard API
      const ta = document.createElement('textarea');
      ta.value = toast.message; document.body.appendChild(ta); ta.select();
      try { document.execCommand('copy'); setCopied(true); setTimeout(() => setCopied(false), 1400); }
      finally { document.body.removeChild(ta); }
    }
  };

  return (
    <div
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      role={toast.kind === 'error' || toast.kind === 'warn' ? 'alert' : 'status'}
      style={{
        pointerEvents: 'auto',
        display: 'flex',
        alignItems: isMultiline ? 'flex-start' : 'center',
        gap: 10,
        width: isMultiline ? 480 : 'auto',
        maxWidth: 'min(560px, calc(100vw - 32px))',
        padding: '8px 10px 8px 12px',
        background: palette.bg,
        color: 'var(--fg)',
        border: `1px solid ${palette.border}`,
        borderLeft: `3px solid ${palette.accent}`,
        borderRadius: 6,
        boxShadow: 'var(--shadow-pop)',
        fontFamily: 'var(--font-mono)',
        fontSize: 12,
        animation: 'toast-in 180ms var(--ease)',
      }}>
      <i className={`ph${palette.fill ? '-fill' : ''} ${palette.icon}`}
         style={{ fontSize: 14, color: palette.accent, flexShrink: 0, marginTop: isMultiline ? 2 : 0 }} />

      <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', gap: 6 }}>
        <div
          ref={msgRef}
          style={{
            fontFamily: 'var(--font-mono)',
            lineHeight: 1.45,
            wordBreak: 'break-word',
            overflowWrap: 'anywhere',
            whiteSpace: isMultiline ? 'normal' : 'nowrap',
            overflow: isMultiline ? 'hidden' : 'visible',
            maxHeight: isMultiline ? (expanded ? 280 : `calc(${collapsedMaxLines} * 1.45em)`) : undefined,
            overflowY: expanded ? 'auto' : 'hidden',
            display: '-webkit-box',
            WebkitBoxOrient: 'vertical',
            WebkitLineClamp: !isMultiline ? undefined : expanded ? 'unset' : collapsedMaxLines,
          }}>{toast.message}</div>

        {/* Footer row — only on multiline toasts: actions + show more + copy */}
        {(isMultiline || toast.action) && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            {toast.action && (
              <button onClick={toast.action.onClick} style={{
                border: 0, background: 'transparent', color: palette.accent,
                fontFamily: 'var(--font-mono)', fontSize: 12, fontWeight: 600,
                padding: 0, cursor: 'pointer',
              }}>{toast.action.label}</button>
            )}
            {isMultiline && overflowing && (
              <button onClick={() => setExpanded((v) => !v)} style={{
                border: 0, background: 'transparent', color: 'var(--fg-muted)',
                fontFamily: 'var(--font-mono)', fontSize: 11,
                padding: 0, cursor: 'pointer',
                display: 'inline-flex', alignItems: 'center', gap: 4,
              }}>
                <i className={`ph ph-caret-${expanded ? 'up' : 'down'}`} style={{ fontSize: 10 }} />
                {expanded ? 'Show less' : 'Show more'}
              </button>
            )}
            <div style={{ flex: 1 }} />
            {isMultiline && (
              <button onClick={copyMessage} title={copied ? 'Copied' : 'Copy message'} style={{
                border: 0, background: 'transparent',
                color: copied ? palette.accent : 'var(--fg-muted)',
                fontFamily: 'var(--font-mono)', fontSize: 11,
                padding: '2px 4px', borderRadius: 4, cursor: 'pointer',
                display: 'inline-flex', alignItems: 'center', gap: 4,
              }}
                onMouseOver={(e) => { e.currentTarget.style.background = 'rgba(255,255,255,0.06)'; }}
                onMouseOut={(e)  => { e.currentTarget.style.background = 'transparent'; }}>
                <i className={`ph ${copied ? 'ph-check' : 'ph-copy'}`} style={{ fontSize: 11 }} />
                {copied ? 'Copied' : 'Copy'}
              </button>
            )}
          </div>
        )}
      </div>

      {isPersistent && (
        <button
          onClick={onClose}
          title="Dismiss"
          style={{
            border: 0, background: 'transparent', color: 'var(--fg-subtle)',
            width: 20, height: 20, borderRadius: 4, padding: 0, flexShrink: 0,
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            cursor: 'pointer',
            marginTop: isMultiline ? 1 : 0,
          }}
          onMouseOver={(e) => { e.currentTarget.style.background = 'rgba(255,255,255,0.08)'; e.currentTarget.style.color = 'var(--fg)'; }}
          onMouseOut={(e)  => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--fg-subtle)'; }}>
          <i className="ph ph-x" style={{ fontSize: 11 }} />
        </button>
      )}
    </div>
  );
}

const TOAST_PALETTE = {
  success: { bg: '#0f1b16', border: 'rgba(78,201,164,0.30)', accent: 'var(--ok)',         icon: 'ph-check-circle', fill: true },
  info:    { bg: '#11161f', border: 'rgba(127,135,255,0.25)', accent: 'var(--periwinkle)', icon: 'ph-info',         fill: true },
  warn:    { bg: '#1c1812', border: 'rgba(255,196,87,0.32)',  accent: 'var(--warn)',       icon: 'ph-warning',      fill: true },
  error:   { bg: '#1c1112', border: 'rgba(255,107,107,0.35)', accent: 'var(--err)',        icon: 'ph-x-circle',     fill: true },
};

Object.assign(window, { ToastHost });
