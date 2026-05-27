// left-rail.jsx — projects column with inbox, search, project tree, recents.
// Key affordance improvements vs. the source:
//   • Stronger hierarchy: section headers are uppercase eyebrows w/ counts
//   • PR rows reveal an inline action menu on hover (rename, hide, pin)
//   • Active branch row has a subtle background + a left orange tick
//   • Search bar gains a keyboard hint pill so the affordance is discoverable
//   • Inbox item visually distinct from PR rows (acts like a notification)

const { useState: useLR } = React;

function SectionHead({ label, count, action }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center',
      padding: '14px 12px 4px',
      fontSize: 10, letterSpacing: '0.06em',
      textTransform: 'uppercase', fontWeight: 600,
      color: 'var(--fg-subtle)',
    }}>
      <span>{label}</span>
      {count != null && (
        <span style={{
          marginLeft: 6, padding: '0 5px', borderRadius: 999,
          background: 'rgba(255,255,255,0.05)', color: 'var(--fg-muted)',
          fontSize: 9, lineHeight: '14px',
        }}>{count}</span>
      )}
      <div style={{ flex: 1 }} />
      {action}
    </div>
  );
}

function ProjectHeader({ proj, expanded, onToggle }) {
  return (
    <button
      onClick={onToggle}
      style={{
        width: '100%', border: 0, background: 'transparent',
        display: 'flex', alignItems: 'center', gap: 6,
        padding: '6px 10px', borderRadius: 6,
        color: 'var(--fg)', fontSize: 12, textAlign: 'left',
      }}
    >
      <i className={`ph ph-caret-${expanded ? 'down' : 'right'}`} style={{ fontSize: 10, color: 'var(--fg-subtle)' }} />
      <i className="ph ph-folder" style={{ fontSize: 12, color: 'var(--fg-muted)' }} />
      <span style={{ fontWeight: 500 }}>{proj.name}</span>
      <div style={{ flex: 1 }} />
      <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>{proj.count}</span>
    </button>
  );
}

function PRRow({ item, kind, onSelect }) {
  const [hover, setHover] = useLR(false);
  const active = !!item.active;
  const dot = !!item.dot;
  return (
    <div
      onClick={() => onSelect && onSelect(item)}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: 'relative',
        display: 'flex', alignItems: 'center', gap: 6,
        margin: '0 6px',
        padding: 'var(--pr-row-pad-y, 4px) 8px var(--pr-row-pad-y, 4px) 22px',
        borderRadius: 6,
        background: active ? 'var(--bg-3)' : (hover ? 'rgba(255,255,255,0.03)' : 'transparent'),
        cursor: 'pointer',
        fontSize: 12,
        color: active ? 'var(--fg)' : 'var(--fg-muted)',
        transition: 'background var(--d-fast) var(--ease)',
      }}
    >
      {/* Active orange tick — the unambiguous "you are here" affordance */}
      {active && (
        <span style={{
          position: 'absolute', left: 6, top: 6, bottom: 6, width: 2,
          background: 'var(--orange)', borderRadius: 2,
        }} />
      )}
      {dot && !active && (
        <span style={{
          position: 'absolute', left: 12, top: '50%', transform: 'translateY(-50%)',
          width: 5, height: 5, borderRadius: '50%', background: 'var(--orange)',
        }} className="dot-pulse" />
      )}
      <i className={`ph ${prIcon(kind)}`} style={{ fontSize: 11, color: item.prStatus ? prStatusColor(item.prStatus) : (active ? 'var(--orange)' : 'var(--fg-subtle)') }} />
      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}>
        {item.name}
      </span>
      {/* Inline actions appear on hover — discoverable without crowding */}
      {hover && !active ? (
        <span style={{ display: 'inline-flex', gap: 2 }}>
          <RowBtn icon="ph-push-pin" title="Pin" />
          <RowBtn icon="ph-dots-three" title="More" />
        </span>
      ) : (
        item.num && <span style={{ fontSize: 10, color: 'var(--fg-subtle)' }}>{item.num}</span>
      )}
    </div>
  );
}

function RowBtn({ icon, title }) {
  return (
    <button title={title} onClick={(e) => e.stopPropagation()} style={{
      border: 0, background: 'transparent', color: 'var(--fg-subtle)',
      width: 18, height: 18, borderRadius: 4, padding: 0,
      display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
    }}
      onMouseOver={(e) => { e.currentTarget.style.background = 'rgba(255,255,255,0.08)'; e.currentTarget.style.color = 'var(--fg)'; }}
      onMouseOut={(e)  => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--fg-subtle)'; }}>
      <i className={`ph ${icon}`} style={{ fontSize: 10 }} />
    </button>
  );
}

function TrackedRow({ item, onSelect }) {
  const [hover, setHover] = useLR(false);
  const active = !!item.active;
  return (
    <div
      onClick={() => onSelect && onSelect(item)}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: 'relative',
        margin: '0 6px',
        padding: 'var(--pr-row-pad-y, 5px) 8px var(--pr-row-pad-y, 5px) 22px',
        borderRadius: 6,
        background: active ? 'var(--bg-3)' : (hover ? 'rgba(255,255,255,0.03)' : 'transparent'),
        cursor: 'pointer',
        display: 'flex', flexDirection: 'column', gap: 1,
      }}
    >
      {active && (
        <span style={{
          position: 'absolute', left: 6, top: 6, bottom: 6, width: 2,
          background: 'var(--orange)', borderRadius: 2,
        }} />
      )}
      {item.dot && !active && (
        <span style={{
          position: 'absolute', left: 12, top: 9,
          width: 5, height: 5, borderRadius: '50%', background: 'var(--orange)',
        }} className="dot-pulse" />
      )}
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <i className="ph ph-git-branch" style={{ fontSize: 11, color: active ? 'var(--orange)' : 'var(--fg-subtle)' }} />
        <span style={{ fontSize: 12, color: active ? 'var(--fg)' : 'var(--fg-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {item.name}
        </span>
      </div>
      <div style={{ fontSize: 10, color: 'var(--fg-subtle)', paddingLeft: 17, fontFamily: 'var(--font-mono)', display: 'var(--show-sub, block)' }}>
        {item.repo}
      </div>
    </div>
  );
}

function prStatusColor(status) {
  switch (status) {
    case 'draft':    return 'var(--fg-faint)';      // greyed out
    case 'review':   return 'var(--fg-muted)';      // light grey
    case 'approved': return 'var(--ok)';            // green
    case 'declined': return 'var(--err)';           // red
    case 'merged':   return '#a371f7';              // purple
    case 'queue':    return 'var(--warn)';          // yellow
    default:         return 'var(--fg-subtle)';
  }
}

function prIcon(kind) {
  switch (kind) {
    case 'merged':   return 'ph-git-merge';
    case 'recent':   return 'ph-clock-counter-clockwise';
    case 'review':   return 'ph-eye';
    default:         return 'ph-git-pull-request';
  }
}

function InboxKindIcon({ kind }) {
  const map = {
    merged:  { icon: 'ph-git-merge',      color: 'var(--periwinkle)' },
    'ci-fail':{ icon: 'ph-x-circle',      color: 'var(--err)' },
    review:  { icon: 'ph-eye',            color: 'var(--orange)' },
    comment: { icon: 'ph-chat-circle',    color: 'var(--info)' },
    mention: { icon: 'ph-at',             color: 'var(--warn)' },
  };
  const m = map[kind] || { icon: 'ph-bell', color: 'var(--fg-muted)' };
  return <i className={`ph ${m.icon}`} style={{ fontSize: 13, color: m.color }} />;
}

function InboxPopover({ items, onClose, onMarkAll, onClearRead }) {
  const [filter, setFilter] = useLR('all');
  const ref = React.useRef(null);
  React.useEffect(() => {
    const onClick = (e) => { if (ref.current && !ref.current.contains(e.target)) onClose(); };
    const onKey = (e) => { if (e.key === 'Escape') onClose(); };
    document.addEventListener('mousedown', onClick);
    document.addEventListener('keydown', onKey);
    return () => { document.removeEventListener('mousedown', onClick); document.removeEventListener('keydown', onKey); };
  }, [onClose]);

  const filtered = items.filter((i) => filter === 'all' ? true : filter === 'unread' ? i.unread : !i.unread);
  const unread = items.filter((i) => i.unread).length;

  return (
    <div ref={ref} style={{
      position: 'absolute',
      top: 0, left: 8, right: 8,
      maxHeight: 'calc(100vh - 80px)',
      zIndex: 50,
      background: 'var(--bg-2)',
      border: '1px solid var(--border-strong)',
      borderRadius: 10,
      boxShadow: 'var(--shadow-pop)',
      display: 'flex', flexDirection: 'column',
      overflow: 'hidden',
    }}>
      {/* Header */}
      <div style={{
        padding: '10px 12px 8px',
        borderBottom: '1px solid var(--border)',
        display: 'flex', flexDirection: 'column', gap: 8,
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <i className="ph-fill ph-tray" style={{ fontSize: 14, color: 'var(--orange)' }} />
          <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--fg)' }}>Inbox</span>
          <span style={{ fontSize: 11, color: 'var(--fg-subtle)' }}>· Updated just now</span>
          <div style={{ flex: 1 }} />
          <button onClick={onClose} title="Close" style={{
            border: 0, background: 'transparent', color: 'var(--fg-subtle)',
            width: 20, height: 20, borderRadius: 4,
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          }}><i className="ph ph-x" style={{ fontSize: 11 }} /></button>
        </div>
        {/* Segmented filter — All / Unread / Read */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <div style={{
            display: 'inline-flex', background: 'var(--bg-1)',
            border: '1px solid var(--border)', borderRadius: 5,
            padding: 2,
          }}>
            <InboxSeg active={filter === 'all'}    onClick={() => setFilter('all')}>All <span style={{ color: 'var(--fg-subtle)', marginLeft: 2 }}>{items.length}</span></InboxSeg>
            <InboxSeg active={filter === 'unread'} onClick={() => setFilter('unread')}>Unread <span style={{ color: filter === 'unread' ? 'var(--orange)' : 'var(--fg-subtle)', marginLeft: 2 }}>{unread}</span></InboxSeg>
            <InboxSeg active={filter === 'read'}   onClick={() => setFilter('read')}>Read</InboxSeg>
          </div>
          <div style={{ flex: 1 }} />
          <button onClick={onMarkAll} style={inboxLinkBtn}>Read all</button>
          <button onClick={onClearRead} style={inboxLinkBtn}>Clear read</button>
        </div>
      </div>
      {/* List */}
      <div style={{ flex: 1, overflowY: 'auto', padding: 4 }}>
        {filtered.length === 0 ? (
          <div style={{ padding: 24, textAlign: 'center', color: 'var(--fg-subtle)', fontSize: 12 }}>
            No items
          </div>
        ) : filtered.map((i) => <InboxItem key={i.id} item={i} />)}
      </div>
    </div>
  );
}

function InboxSeg({ active, onClick, children }) {
  return (
    <button onClick={onClick} style={{
      border: 0, height: 22, padding: '0 8px',
      borderRadius: 3,
      fontSize: 11, fontWeight: 500,
      background: active ? 'var(--bg-3)' : 'transparent',
      color: active ? 'var(--fg)' : 'var(--fg-muted)',
      display: 'inline-flex', alignItems: 'center', gap: 2,
    }}>{children}</button>
  );
}
const inboxLinkBtn = {
  border: 0, background: 'transparent',
  color: 'var(--periwinkle)', fontSize: 11, padding: '2px 4px',
  borderRadius: 4, cursor: 'pointer',
};

function InboxItem({ item }) {
  const [hover, setHover] = useLR(false);
  return (
    <div
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: 'flex', alignItems: 'flex-start', gap: 10,
        padding: '8px 10px',
        borderRadius: 6,
        background: hover ? 'rgba(255,255,255,0.04)' : 'transparent',
        cursor: 'pointer',
        position: 'relative',
      }}>
      {/* Unread tick on the left */}
      <span style={{
        position: 'absolute', left: 0, top: 12, bottom: 12, width: 2,
        background: item.unread ? 'var(--orange)' : 'transparent',
        borderRadius: '0 2px 2px 0',
      }} />
      <InboxKindIcon kind={item.kind} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{
          fontSize: 12, fontWeight: item.unread ? 500 : 400,
          color: item.unread ? 'var(--fg)' : 'var(--fg-muted)',
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
        }}>{item.label}</div>
        <div style={{
          fontSize: 11, color: 'var(--fg-subtle)',
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
          marginTop: 1,
        }}>{item.sub}</div>
      </div>
      <span style={{ fontSize: 10, color: 'var(--fg-subtle)', whiteSpace: 'nowrap', flexShrink: 0, marginTop: 1 }}>
        {item.when}
      </span>
    </div>
  );
}

function LeftRail({ projects, inbox, density, onSelectBranch, activeProject, onProject, collapsed }) {
  const [expanded, setExpanded] = useLR({ easy: false, discovery: true, ink: false });
  const [inboxOpen, setInboxOpen] = useLR(false);
  const [items, setItems] = useLR(inbox);
  const proj = projects.find((p) => p.id === activeProject) || projects[1];

  const unreadCount = items.filter((i) => i.unread).length;

  if (collapsed) {
    return (
      <div style={{
        width: 44, background: 'var(--bg-1)', borderRight: '1px solid var(--border)',
        display: 'flex', flexDirection: 'column', alignItems: 'center', paddingTop: 8, gap: 4,
      }}>
        {projects.map((p) => (
          <button key={p.id} onClick={() => onProject(p.id)}
            title={p.name}
            style={{
              width: 28, height: 28, borderRadius: 7,
              border: 0, background: p.id === activeProject ? 'var(--bg-3)' : 'transparent',
              color: p.id === activeProject ? 'var(--fg)' : 'var(--fg-muted)',
              display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            }}>
            <i className="ph ph-folder" style={{ fontSize: 13 }} />
          </button>
        ))}
        <div style={{ flex: 1 }} />
        <button style={{
          width: 28, height: 28, borderRadius: 7, border: 0, background: 'transparent', color: 'var(--fg-muted)', marginBottom: 8,
        }}>
          <i className="ph ph-gear" style={{ fontSize: 13 }} />
        </button>
      </div>
    );
  }

  return (
    <aside style={{
      width: 240,
      background: 'var(--bg-1)',
      borderRight: '1px solid var(--border)',
      display: 'flex', flexDirection: 'column',
      flexShrink: 0,
    }}>
      {/* New-review CTA + Search */}
      <div style={{ padding: '10px 10px 4px', display: 'flex', flexDirection: 'column', gap: 6 }}>
        <button style={{
          height: 28, border: '1px dashed var(--border-strong)', borderRadius: 6,
          background: 'transparent', color: 'var(--fg-muted)',
          display: 'inline-flex', alignItems: 'center', gap: 6, justifyContent: 'center',
          fontSize: 12, fontWeight: 500,
        }}
          onMouseOver={(e) => { e.currentTarget.style.background = 'rgba(255,255,255,0.04)'; e.currentTarget.style.color = 'var(--fg)'; e.currentTarget.style.borderColor = 'var(--periwinkle)'; }}
          onMouseOut={(e)  => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--fg-muted)'; e.currentTarget.style.borderColor = 'var(--border-strong)'; }}>
          <i className="ph ph-plus" style={{ fontSize: 12 }} />
          <span>New review</span>
        </button>
        <SearchInput placeholder="Search projects, branches, PRs" shortcut="⌘P" />
      </div>

      <div style={{ flex: 1, overflowY: 'auto', paddingBottom: 8, position: 'relative' }}>
        {/* Inbox — header opens a popover with the full list */}
        <SectionHead
          label="Inbox"
          count={unreadCount > 0 ? unreadCount : null}
          action={
            <button onClick={() => setInboxOpen((v) => !v)} title="Open inbox" style={{
              border: 0, background: 'transparent',
              color: inboxOpen ? 'var(--orange)' : 'var(--fg-subtle)',
              padding: '2px 4px', borderRadius: 4,
              display: 'inline-flex', alignItems: 'center', gap: 4,
              fontSize: 10, cursor: 'pointer',
            }}>
              <i className="ph ph-arrows-out-simple" style={{ fontSize: 11 }} />
            </button>
          }
        />
        {/* Two most-recent unread items, shown in-rail as a teaser. Click anywhere to expand. */}
        <div style={{ padding: '0 6px', position: 'relative' }}>
          {items.slice(0, 2).map((i) => (
            <div key={i.id} onClick={() => setInboxOpen(true)} style={{
              display: 'flex', alignItems: 'flex-start', gap: 8,
              padding: '6px 8px', borderRadius: 6, cursor: 'pointer',
              position: 'relative',
            }}
              onMouseOver={(e) => { e.currentTarget.style.background = 'rgba(255,255,255,0.03)'; }}
              onMouseOut={(e)  => { e.currentTarget.style.background = 'transparent'; }}>
              {i.unread && (
                <span style={{
                  position: 'absolute', left: 0, top: 8, bottom: 8, width: 2,
                  background: 'var(--orange)', borderRadius: '0 2px 2px 0',
                }} />
              )}
              <span style={{ marginTop: 1, flexShrink: 0 }}>
                <InboxKindIcon kind={i.kind} />
              </span>
              <div style={{ minWidth: 0, flex: 1 }}>
                <div style={{
                  fontSize: 12, color: i.unread ? 'var(--fg)' : 'var(--fg-muted)',
                  fontWeight: i.unread ? 500 : 400,
                  overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                }}>{i.label}</div>
                <div style={{
                  fontSize: 10, color: 'var(--fg-subtle)',
                  overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                }}>{i.sub}</div>
              </div>
              <span style={{ fontSize: 10, color: 'var(--fg-subtle)', flexShrink: 0 }}>{i.when}</span>
            </div>
          ))}
          {items.length > 2 && (
            <button onClick={() => setInboxOpen(true)} style={{
              width: '100%', border: 0, background: 'transparent',
              padding: '4px 8px', color: 'var(--fg-subtle)',
              fontSize: 11, textAlign: 'left', cursor: 'pointer',
              display: 'flex', alignItems: 'center', gap: 4,
            }}
              onMouseOver={(e) => { e.currentTarget.style.color = 'var(--fg)'; }}
              onMouseOut={(e)  => { e.currentTarget.style.color = 'var(--fg-subtle)'; }}>
              <i className="ph ph-caret-down" style={{ fontSize: 10 }} />
              See {items.length - 2} more
            </button>
          )}
          {inboxOpen && (
            <InboxPopover
              items={items}
              onClose={() => setInboxOpen(false)}
              onMarkAll={() => setItems(items.map((i) => ({ ...i, unread: false })))}
              onClearRead={() => setItems(items.filter((i) => i.unread))}
            />
          )}
        </div>

        <SectionHead label="Projects" action={
          <RowBtn icon="ph-arrows-clockwise" title="Refresh" />
        } />
        <div style={{ padding: '0 6px' }}>
          {projects.map((p) => {
            const exp = p.id === proj.id;
            return (
              <div key={p.id}>
                <ProjectHeader proj={p} expanded={exp}
                  onToggle={() => onProject(p.id)} />
                {exp && p.tracked.length > 0 && (
                  <>
                    <SubHead label="Tracked" />
                    {p.tracked.map((t) => (
                      <TrackedRow key={t.id} item={t} onSelect={onSelectBranch} />
                    ))}
                  </>
                )}
                {exp && p.myPRs && (
                  <>
                    <SubHead label="My PRs" />
                    {p.myPRs.map((pr) => <PRRow key={pr.id} item={pr} kind="pr" onSelect={onSelectBranch} />)}
                  </>
                )}
                {exp && p.toReview && (
                  <>
                    <SubHead label="To review" />
                    {p.toReview.map((pr) => <PRRow key={pr.id} item={pr} kind="review" onSelect={onSelectBranch} />)}
                  </>
                )}
                {exp && p.recent && (
                  <>
                    <SubHead label="Recent" />
                    {p.recent.map((pr) => <PRRow key={pr.id} item={pr} kind="recent" onSelect={onSelectBranch} />)}
                  </>
                )}
                {exp && p.merged && (
                  <>
                    <SubHead label="Recently merged" />
                    {p.merged.map((pr) => <PRRow key={pr.id} item={pr} kind="merged" onSelect={onSelectBranch} />)}
                  </>
                )}
              </div>
            );
          })}
        </div>
      </div>

      {/* Settings strip */}
      <div style={{
        padding: '6px 10px',
        borderTop: '1px solid var(--border)',
        display: 'flex', alignItems: 'center', gap: 8,
        background: 'var(--bg-0)',
      }}>
        <span style={{
          width: 18, height: 18, borderRadius: 4, background: 'var(--orange-tint-strong)',
          color: 'var(--orange)', display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          fontSize: 9, fontWeight: 700, letterSpacing: '0.05em',
        }}>er</span>
        <span style={{ fontSize: 12, color: 'var(--fg-muted)' }}>Settings</span>
        <div style={{ flex: 1 }} />
        <i className="ph ph-gear" style={{ fontSize: 12, color: 'var(--fg-subtle)' }} />
      </div>
    </aside>
  );
}

function SubHead({ label }) {
  return (
    <div style={{
      padding: '10px 14px 2px',
      fontSize: 9, letterSpacing: '0.08em',
      textTransform: 'uppercase', fontWeight: 600,
      color: 'var(--fg-faint)',
    }}>{label}</div>
  );
}

function SearchInput({ placeholder, shortcut }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 6,
      height: 28, padding: '0 8px',
      background: 'var(--bg-0)', borderRadius: 6,
      border: '1px solid var(--border)',
    }}>
      <i className="ph ph-magnifying-glass" style={{ fontSize: 12, color: 'var(--fg-subtle)' }} />
      <input
        placeholder={placeholder}
        style={{
          flex: 1, minWidth: 0, border: 0, background: 'transparent',
          fontSize: 12, color: 'var(--fg)', outline: 'none',
        }}
      />
      {shortcut && <span className="kbd">{shortcut}</span>}
    </div>
  );
}

Object.assign(window, { LeftRail });
