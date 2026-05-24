<script lang="ts">
  interface Props {
    name: "sparkle";
    class?: string;
    size?: number;
  }
  const { name, class: className = "", size = 10 }: Props = $props();
</script>

{#if name === "sparkle"}
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="currentColor"
    class="shrink-0 {className}"
    aria-hidden="true"
  >
    <path d="M12 2l1.5 5.5L19 9l-5.5 1.5L12 16l-1.5-5.5L5 9l5.5-1.5L12 2z" />
    <path d="M19 14l.8 2.8L22.5 18l-2.7.7L19 21.5l-.8-2.8L15.5 18l2.7-.7L19 14z" opacity="0.85" />
  </svg>
{/if}
