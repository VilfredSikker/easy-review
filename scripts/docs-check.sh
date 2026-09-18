#!/usr/bin/env bash
# Verify the agent-facing docs still point at things that exist.
#
# The failure this exists for is not a typo — it is a citation that was correct
# when written and silently stopped being true: a config.rs that became a
# directory, an er_storage.rs that was deleted, a symbol that was never written
# at all. Nothing regenerates those sentences, so they decay into confident
# fiction. An audit found several; this catches the next one.
#
# Checks, over the docs an agent actually loads:
#   1. Path-like code citations (`crates/…/foo.rs`, `diff.rs`) resolve against
#      the repo root, the citing file's directory, or the crate src root.
#   2. Relative markdown links resolve.
#   3. ADR references name an ADR that exists.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Docs an agent loads to work on the code. User-facing guide pages are excluded:
# they are how-to by design (see docs/agents/writing-docs.md).
DOCS="
CLAUDE.md
AGENTS.md
CONTEXT.md
docs/agents/domain.md
docs/agents/issue-tracker.md
docs/agents/triage-labels.md
docs/agents/writing-docs.md
docs/config-reference.md
docs/DEVELOPMENT.md
docs/quality-checks.md
crates/er-engine/src/CLAUDE.md
crates/er-engine/src/ai/CLAUDE.md
crates/er-engine/src/app/CLAUDE.md
crates/er-engine/src/git/CLAUDE.md
crates/er-engine/src/watch/CLAUDE.md
crates/er-tui/src/ui/CLAUDE.md
crates/er-desktop/agent.md
crates/er-engine/src/app/state/agent.md
desktop-ui/agent.md
desktop-ui/src/lib/stores/agent.md
desktop-ui/src/lib/components/agent.md
"
# Extra paths to check, space-separated. Used by the script's own test.
DOCS="$DOCS ${DOCS_EXTRA:-}"

# Citations a doc names deliberately as EXAMPLES of a citation that went wrong.
# They are supposed to not resolve.
ILLUSTRATIVE="config.rs er-api compute_poll_revision ExportModal.svelte er_storage.rs"

fail=0
note() { printf '  %s\n' "$1"; fail=1; }
is_illustrative() {
  for x in $ILLUSTRATIVE; do [ "$1" = "$x" ] && return 0; done
  return 1
}

echo "docs-check: scanning agent-facing docs"
echo

# ── 1: code citations ─────────────────────────────────────────────────────────
# Two kinds, checked differently:
#   - path-like (`crates/…/foo.rs`) must resolve exactly, from one of a few bases
#   - basename-only (`storage.rs`) is accepted if a file of that name exists
#     anywhere, which is a weaker check but catches a file being deleted
# Runtime artifacts are skipped: review.json and config.toml live in managed
# storage and are created by the app, so they are correctly absent from the tree.
RUNTIME_ARTIFACTS="config.toml review.json questions.json notes.json order.json checklist.json github-comments.json session.json tour.json triage.json professor.json summary.md reviewed"
is_runtime() {
  for x in $RUNTIME_ARTIFACTS; do [ "$1" = "$x" ] && return 0; done
  return 1
}

echo "Citations:"
cited=0
for doc in $DOCS; do
  [ -f "$doc" ] || { note "MISSING DOC: $doc"; continue; }
  ddir="$(dirname "$doc")"
  bases="$ddir ."
  case "$doc" in
    crates/*/src/*) bases="$ddir crates/$(echo "$doc" | cut -d/ -f2)/src ." ;;
  esac

  while IFS= read -r cit; do
    [ -n "$cit" ] || continue
    is_illustrative "$cit" && continue
    is_runtime "$cit" && continue
    cited=$((cited + 1))

    case "$cit" in
      */*)
        ok=0
        for b in $bases; do
          [ -e "$b/$cit" ] && { ok=1; break; }
        done
        [ "$ok" -eq 1 ] || note "$doc cites '$cit' — does not resolve"
        ;;
      *)
        find crates desktop-ui scripts docs brand -name "$cit" \
          -not -path '*/node_modules/*' -not -path '*/target/*' 2>/dev/null | grep -q . \
          || note "$doc cites '$cit' — no file with that name anywhere"
        ;;
    esac
  done < <(grep -oE '`[A-Za-z0-9_][A-Za-z0-9_/.-]*\.(rs|ts|svelte|toml|sh|js|css)`' "$doc" 2>/dev/null \
             | tr -d '`' | sort -u)
done
echo "  $cited citations checked"
echo

# ── 2: relative markdown links ────────────────────────────────────────────────
echo "Links:"
links=0
for doc in $DOCS; do
  [ -f "$doc" ] || continue
  dir="$(dirname "$doc")"
  while IFS= read -r link; do
    [ -n "$link" ] || continue
    case "$link" in
      http*|\#*) continue ;;
      /*) target=".$link" ;;
      *)  target="$dir/$link" ;;
    esac
    links=$((links + 1))
    [ -e "$target" ] || note "$doc links to '$link' — does not resolve"
  done < <(grep -oE '\]\([^)#]+\)' "$doc" 2>/dev/null | sed 's/](//;s/)$//' | sort -u)
done
echo "  $links links checked"
echo

# ── 3: ADR references ─────────────────────────────────────────────────────────
echo "ADR references:"
refs=0
for doc in $DOCS docs/adr/*.md; do
  [ -f "$doc" ] || continue
  while IFS= read -r n; do
    [ -n "$n" ] || continue
    refs=$((refs + 1))
    find docs/adr -name "$n-*" 2>/dev/null | grep -q . || note "$doc references ADR $n — no such ADR"
  done < <(grep -oE '(ADR|adr/)[- ]?0[0-9]{3}' "$doc" 2>/dev/null | grep -oE '0[0-9]{3}' | sort -u)
done
echo "  $refs ADR references checked"
echo

# ── 4: user-facing pages ──────────────────────────────────────────────────────
# docs/guide/ and the landing page are how-to by design, so the citation rules
# above do not apply. What does apply is that a link has to go somewhere: a
# reader clicking a dead cross-reference is the user-facing version of the same
# rot. Only local targets are checked; anchors and external URLs are skipped.
echo "User-facing links:"
ulinks=0
for page in docs/index.html docs/guide/*.html README.md ${HTML_EXTRA:-}; do
  [ -f "$page" ] || continue
  pdir="$(dirname "$page")"
  while IFS= read -r link; do
    [ -n "$link" ] || continue
    case "$link" in
      http*|mailto:*|\#*|data:*) continue ;;
    esac
    target="${link%%#*}"
    [ -n "$target" ] || continue
    ulinks=$((ulinks + 1))
    [ -e "$pdir/$target" ] || note "$page links to '$link' — does not resolve"
  done < <(grep -oE '(href|src)="[^"]+"' "$page" 2>/dev/null | sed 's/^[a-z]*="//;s/"$//' | sort -u)
done
echo "  $ulinks user-facing links checked"
echo

# ── 5: the guide's navigation model ───────────────────────────────────────────
# The sidebar is injected by assets/docs.js, so it is invisible to the href scan
# above: a NAV entry pointing at a deleted page would 404 silently, and nothing
# else in the tree would notice.
echo "Guide navigation:"
nave=0
if [ -f docs/guide/assets/docs.js ]; then
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    nave=$((nave + 1))
    [ -f "docs/guide/$f" ] || note "docs/guide/assets/docs.js NAV lists '$f' — no such page"
  done < <(grep -oE "file: *'[a-zA-Z0-9_.-]+\.html'" docs/guide/assets/docs.js | sed "s/.*'\(.*\)'/\1/" | sort -u)
fi
echo "  $nave navigation entries checked"
echo

# ── 6: the shape that rots ────────────────────────────────────────────────────
# The clean removed file tables, module inventories and type listings from the
# agent docs, because they restate the code and then drift from it. Nothing
# regenerates them, so a new one is a new source of rot — and this shape, not a
# broken citation, is what produced the 39% error rate the clean was about.
#
# Scoped to the docs an agent reads as INSTRUCTIONS — CLAUDE.md, agent.md,
# CONTEXT.md — because those are the ones that must stay why-shaped. A reference
# doc (config-reference, DEVELOPMENT, quality-checks) legitimately holds tables:
# a reader looks things up there, and looking it up is the point. An ADR table is
# a decision, not an inventory. Neither is checked.
echo "Doc shape:"
shapes=0
for doc in $DOCS; do
  [ -f "$doc" ] || continue
  case "$doc" in
    *CLAUDE.md | *agent.md | CONTEXT.md) ;;
    *) continue ;;
  esac
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    shapes=$((shapes + 1))
    note "$doc has an inventory table — restating the tree, which rots: $line"
  done < <(grep -nE '^\| *(File|Module|Key file|Key type|Type|Field|Struct) ' "$doc" 2>/dev/null)
  while IFS= read -r h; do
    [ -n "$h" ] || continue
    shapes=$((shapes + 1))
    note "$doc has an inventory heading: $h"
  done < <(grep -nE '^#{2,3} +(Files|Module Map|Type Reference|Structs|Key Types)$' "$doc" 2>/dev/null)
done
[ "$shapes" -eq 0 ] && echo "  no inventory shapes (the rot pattern) found"
echo

if [ "$fail" -ne 0 ]; then
  echo "docs-check: FAILED — fix the citations above."
  echo "See docs/agents/writing-docs.md. A citation that no longer resolves is a"
  echo "doc bug: the sentence now asserts something untrue."
  exit 1
fi
echo "docs-check: ok"
