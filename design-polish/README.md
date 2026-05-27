easy-review — Affordance & Polish Implementation Brief
This brief captures the agreed design direction for the next iteration of easy-review. It's scoped to the changes we want to ship — not an exploration of alternatives. Use it as your spec.

Window chrome (top bar)
Branch tabs live inside the title bar, alongside the macOS traffic lights. There is no separate row for branch tabs.

Active tab: orange under-rule, slightly lifted background, branch name in --fg.
Each tab supports: a dirty-indicator dot (periwinkle) for uncommitted changes, a comment-count badge, and a close-on-hover ×.
+ button at the end of the strip opens a new tab (⌘T).
Right cluster: ⌘K command palette button and a right-rail toggle. No "switch project" affordance — projects already live in the left rail.
Context bar (row beneath the chrome)
A single slim row carrying the active branch's context.

Branch glyph (orange) + full branch name (not truncated) + base main + +464 / −16 summary.
Quick-action icon row with tooltips:
Copy branch name → fires success toast
Reveal worktree in Finder
Open PR (icon + inline #1137 badge)
Terminal toggle (active state)
Open split view (browser glyph, active state)
Diff settings gear lives here, not on individual file headers — it's a whole-diff control (whitespace, wrap, etc).
Right side: PR diff | Local branch segmented control.
Left rail (projects)
Inbox
Section header shows unread count + an expand button.
In-rail teaser: top 2 unread items inline with kind-colored icon, title (bold if unread), one-line subtitle, age, and an orange unread tick on the left edge.
"See N more" caret opens the full inbox popover.
Inbox popover:
Header: Inbox · Updated just now, with Read all and Clear read link buttons.
All N · Unread N · Read segmented filter.
Each item: kind icon + title + subtitle + age + unread tick.
Closes on outside-click or Esc.
Kind icons + colors:
PR merged → periwinkle git-merge
CI failed → red x-circle
Review requested → orange eye
New comment → blue chat
Mention → amber @
Project tree
Strong eyebrow section heads: TRACKED, MY PRS, TO REVIEW, RECENT, RECENTLY MERGED.
Active branch row: orange left tick + lifted background.
Hover-reveal row actions (pin, more) — don't crowd the row by default.
PR status colors on branch icons (replace the current monochrome):
Draft → --fg-faint (greyed out)
Ready for review → --fg-muted (light grey)
Approved → green
Declined → red
Merged → purple
Merge queue → yellow
"New review" CTA + project/branch search with ⌘P keyboard hint.
Collapsible to a 44px-wide icon rail (project icons only).
Files rail
Filter input with / keyboard hint.
Review progress meter: a horizontal segmented bar showing 4/7 reviewed visually, not just numerically.
File rows: file-type chip (TS / SV / etc.), filename, comment-count badge, +90 / −0 add/del text. No proportional add/del bars — give the filename the room.
Commits panel at the bottom (collapsible):
All changes at the top (default scope).
Per-commit row: author avatar, message, short-SHA chip, age, +/−, local badge for un-pushed.
Selecting a commit scopes the diff: the files-header label flips to the SHA chip, and files not in that commit dim to ~35% opacity.
Diff view (main area)
File header
Review checkbox (primary action, leftmost).
Collapse caret.
Breadcrumb path with the filename emphasized (rest muted).
+90 / −0 totals (mono, green/red).
Prev/next hunk arrows.
Open source button (opens in VS Code).
No unified/split toggle here — that's a whole-diff setting and lives in the context bar's gear menu.
Code rows
Hover any code line to reveal a small + button for dropping a new comment on that line — primary affordance for the user to learn line-comments exist.
Lightweight TS-aware syntax colorization (keyword / type / string / number / comment).
Inline comment thread
Anchored to the line range by a left spine; not a yellow slab.

Header: Local question · lines 183–186 · Private · won't push · 3m.
Avatar + author + age + comment body.
Action row, left to right: Reply · Ask AI (periwinkle) · Validate with AI · Copy · Resolve · Promote to comment. Delete pushed to the right and muted.
Right rail — TABS layout
Tabs along the top of the rail, full-height content beneath.

Tabs: Branch · Review · Notes.
Each tab supports a unread/count badge (e.g. Review shows total finding count; Notes shows local question count).
Active tab: orange underline + orange icon + brighter label.
Branch panel (default tab)
Title row: branch icon + short branch name + reviewed count + bookmark button.
Base ref display: base main ← claude/organism-sub… with mono chips.
Changes meter: +464 / −16 with a horizontal proportional bar.
GitHub status card (consolidates the 3 floating pills from current design):
Header: GitHub glyph + PR link + sync button.
3-cell status grid: Status: Draft, Review: Required, Mergeable: Yes — semantic colors per cell.
CI row: ✓ 7/7 checks passing with caret to expand details.
Activity row: 1 comment · 0 reviews.
Description block with truncate + Show all link.
Comment or review… composer button (always visible at the bottom).
Review panel
3-cell severity grid: HIGH · MED · LOW with finding counts in mono numerals.
Empty state reframed: FRESH · 0 FINDINGS pill + a short explanation + two primary actions: Re-run review and Open .er/. Not a passive paragraph.
Footer: Copy findings JSON, Reveal review files.
Notes panel
Local-only / private framing made explicit in the intro line.
Each question card: clickable file:line link + question text.
Primary actions: Ask AI (periwinkle filled) + Promote to comment.
Right rail — collapsed (44px)
When the user collapses the rail, replace it with a vertical icon stack so they still get glanceable status:

Branch atom — git-branch (orange) + D badge for Draft.
GitHub atom — github logo + ✓ badge when CI is green (red ! when failing).
AI Review atom — sparkle + finding-count badge (color tracks worst severity).
Notes atom — chat bubble + local-question count.
Click any atom → expands the rail and jumps to that tab.
The collapse toggle lives in the window chrome's right cluster, not in the diff header.

Terminal
Bottom drawer (conditionally shown when the user opens it on the active branch). Not a tab in the right rail.

Top edge has a row-resize cursor handle.
Header strip: terminal icon + branch name + Insert: git checkout <branch> hint + split / clear / close actions.
Body: monospace, blinking caret, syntax-tinted prompt segments.
Toasts
Stacked bottom-right, newest on top. Slide-in animation. Single system for success / info / warn / error — same chrome, different palette and dismiss behavior.

Success / info / warn auto-dismiss in ~3.2s.
Errors persist until manually closed with the ×.
Pause-on-hover for non-persistent toasts.
Optional action: { label, onClick } (e.g. Retry on push-failure errors).
Palette: green / periwinkle / amber / red — each gets a filled icon and a 3px left rule in the kind color.
Long error/warn messages:
Card has a fixed width (~480px) so the layout doesn't blow out horizontally.
Message wraps with overflow-wrap: anywhere so URLs and long tokens break cleanly.
Collapsed to 3 lines by default with a Show more caret when the message overflows; expanded scrolls within a bounded max-height.
Footer row carries: optional action button (e.g. Retry), Show more, and a Copy button on the right.
The error icon sits top-left (not center) so the message body gets full horizontal room.
Density
Use the comfy baseline (current design). Specifically:

Code: 12px font / 22px line-height.
File and PR rows: ~5px vertical padding.
Tracked-branch rows: show the worktree subtitle underneath the branch name.
Section padding: 14px.
No need to ship a density toggle.

Source files for reference
The prototype is split into focused components — useful as scaffolding:

File	Responsibility
tokens.css	Palette + type + spacing tokens (matches the app's existing dark aesthetic)
chrome.jsx	Window chrome + branch tab strip + context bar
left-rail.jsx	Projects, inbox + popover, PR status colors
files-rail.jsx	File list, review progress meter, commits panel
diff-view.jsx	File header, code rows, inline comment thread, syntax colorization
right-rail.jsx	Tabs layout + collapsed icon stack
terminal.jsx	Bottom drawer terminal
toast.jsx	Toast system
Implement in the order: tokens → chrome → context bar → right rail (tabs + collapsed) → left rail (inbox popover, PR colors) → files rail (progress meter, commits) → diff view (inline comments, hover-to-comment) → toasts → terminal.