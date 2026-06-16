<script lang="ts">
  /**
   * The Easy Review "Review List" app mark — severity badges leading tinted
   * finding rows. Replaces the old `er` text tile everywhere in the app.
   *
   * The tile is a fixed dark mark and intentionally does NOT recolour with the
   * theme (an app icon is a constant); it sits cleanly on dark and light
   * surfaces alike. Geometry mirrors `brand/app-icon/review-list-dark.svg`.
   */
  interface Props {
    /** Rendered size in px (square). */
    size?: number;
    class?: string;
  }
  const { size = 20, class: klass = "" }: Props = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 64 64"
  class={klass}
  role="img"
  aria-label="Easy Review"
>
  <defs>
    <linearGradient id="er-mark-tile" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#1d2331" />
      <stop offset="1" stop-color="#0c0e14" />
    </linearGradient>
  </defs>
  <rect width="64" height="64" rx="14.3" fill="url(#er-mark-tile)" />
  <rect x="0.7" y="0.7" width="62.6" height="62.6" rx="13.8" fill="none" stroke="#ffffff16" stroke-width="1" />
  <rect x="15" y="13.4" width="9.2" height="9.2" rx="2.9" fill="#5f9cea" />
  <rect x="27.5" y="14.8" width="22" height="6.4" rx="3.2" fill="#5f9cea" opacity="0.34" />
  <rect x="15" y="27.4" width="9.2" height="9.2" rx="2.9" fill="#46bd6c" />
  <rect x="27.5" y="28.8" width="22" height="6.4" rx="3.2" fill="#46bd6c" opacity="0.34" />
  <rect x="15" y="41.4" width="9.2" height="9.2" rx="2.9" fill="#ef5f5b" />
  <rect x="27.5" y="42.8" width="22" height="6.4" rx="3.2" fill="#ef5f5b" opacity="0.34" />
</svg>
