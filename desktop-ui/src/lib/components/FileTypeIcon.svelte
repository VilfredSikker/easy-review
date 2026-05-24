<script lang="ts">
  import type { FileTypeIconSpec } from "$lib/fileTypeIcon";

  interface Props {
    spec: FileTypeIconSpec;
    size?: number;
  }
  const { spec, size = 13 }: Props = $props();
  const jsonGlyph = "{}";
</script>

{#if spec.icon === "typescript"}
  <svg width={size} height={size} viewBox="0 0 16 16" class="shrink-0" aria-hidden="true">
    <rect width="16" height="16" rx="2" fill="#3178c6" opacity="0.9" />
    <text x="8" y="11" text-anchor="middle" fill="white" font-size="7" font-weight="700" font-family="system-ui,sans-serif">TS</text>
  </svg>
{:else if spec.icon === "javascript"}
  <svg width={size} height={size} viewBox="0 0 16 16" class="shrink-0" aria-hidden="true">
    <rect width="16" height="16" rx="2" fill="#c9a227" opacity="0.95" />
    <text x="8" y="11" text-anchor="middle" fill="#1a1a1a" font-size="7" font-weight="700" font-family="system-ui,sans-serif">JS</text>
  </svg>
{:else if spec.icon === "svelte"}
  <svg width={size} height={size} viewBox="0 0 16 16" class="shrink-0" aria-hidden="true">
    <rect width="16" height="16" rx="2" fill="#ff3e00" opacity="0.9" />
    <text x="8" y="11" text-anchor="middle" fill="white" font-size="6" font-weight="700" font-family="system-ui,sans-serif">SV</text>
  </svg>
{:else if spec.icon === "rust"}
  <svg width={size} height={size} viewBox="0 0 16 16" class="shrink-0" aria-hidden="true">
    <rect width="16" height="16" rx="2" fill="#dea584" opacity="0.95" />
    <text x="8" y="11" text-anchor="middle" fill="#3d1f0f" font-size="7" font-weight="700" font-family="system-ui,sans-serif">RS</text>
  </svg>
{:else if spec.icon === "json"}
  <svg width={size} height={size} viewBox="0 0 16 16" class="shrink-0" aria-hidden="true">
    <rect width="16" height="16" rx="2" fill="#cbcb41" opacity="0.85" />
    <text x="8" y="11" text-anchor="middle" fill="#1a1a1a" font-size="7" font-weight="700" font-family="monospace">{jsonGlyph}</text>
  </svg>
{:else if spec.icon === "markdown"}
  <svg width={size} height={size} viewBox="0 0 16 16" class="shrink-0" aria-hidden="true">
    <rect width="16" height="16" rx="2" fill="#519aba" opacity="0.85" />
    <text x="8" y="11" text-anchor="middle" fill="white" font-size="7" font-weight="700" font-family="system-ui,sans-serif">M</text>
  </svg>
{:else if spec.icon === "docker"}
  <svg width={size} height={size} viewBox="0 0 16 16" class="shrink-0" aria-hidden="true">
    <rect width="16" height="16" rx="2" fill="#2496ed" opacity="0.9" />
    <text x="8" y="11" text-anchor="middle" fill="white" font-size="7" font-weight="700" font-family="system-ui,sans-serif">D</text>
  </svg>
{:else if spec.icon === "generic" || !spec.icon}
  <span
    class="shrink-0 inline-flex items-center justify-center rounded-sm font-bold mono leading-none {spec.monogramClass}"
    style="width: {size}px; height: {size}px; font-size: {size <= 13 ? 7 : 8}px;"
    aria-hidden="true"
  >{spec.label}</span>
{:else}
  <!-- go, python, ruby, css, html, yaml, toml, shell, image, lock, config -->
  <span
    class="shrink-0 inline-flex items-center justify-center rounded-sm font-bold mono leading-none {spec.monogramClass}"
    style="width: {size}px; height: {size}px; font-size: {size <= 13 ? 7 : 8}px;"
    aria-hidden="true"
  >{spec.label}</span>
{/if}
