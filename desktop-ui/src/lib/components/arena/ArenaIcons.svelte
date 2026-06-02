<script lang="ts">
  interface Props {
    name: "trophy" | "x" | "plus" | "bracket" | "grid" | "funnel";
    class?: string;
    size?: number;
  }
  const { name, class: className = "", size = 14 }: Props = $props();
</script>

{#if name === "trophy"}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" class="shrink-0 {className}" aria-hidden="true">
    <path d="M6 3h12v2a4 4 0 01-4 4h-1a4 4 0 01-4-4V3zm2 14h8v2H8v-2zm-1 2v2h10v-2H7zM5 7H3a2 2 0 002 2h.1A5 5 0 005 7zm16 0h-2a5 5 0 00.9 2H21a2 2 0 00-2-2z" />
  </svg>
{:else if name === "x"}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 {className}" aria-hidden="true">
    <path d="M6 6l12 12M18 6L6 18" />
  </svg>
{:else if name === "plus"}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="shrink-0 {className}" aria-hidden="true">
    <path d="M12 5v14M5 12h14" />
  </svg>
{:else if name === "bracket"}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="shrink-0 {className}" aria-hidden="true">
    <rect x="3" y="4" width="6" height="16" rx="1" /><rect x="11" y="4" width="4" height="16" rx="1" /><rect x="17" y="4" width="4" height="16" rx="1" />
  </svg>
{:else if name === "grid"}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" class="shrink-0 {className}" aria-hidden="true">
    <path d="M3 3h7v7H3V3zm11 0h7v7h-7V3zM3 14h7v7H3v-7zm11 0h7v7h-7v-7z" opacity="0.9" />
  </svg>
{:else if name === "funnel"}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" class="shrink-0 {className}" aria-hidden="true">
    <path d="M4 4h16l-6 7v5l-4 2v-7L4 4z" />
  </svg>
{/if}
