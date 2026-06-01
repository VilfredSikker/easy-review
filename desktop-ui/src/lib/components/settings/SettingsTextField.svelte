<script lang="ts">
  interface Props {
    label: string;
    description?: string;
    placeholder?: string;
    value: string;
    strict?: boolean;
    warning?: string | null;
    oncommit: (value: string) => void;
  }

  let {
    label,
    description = "",
    placeholder = "",
    value,
    strict = false,
    warning = null,
    oncommit,
  }: Props = $props();

  let draft = $state(value);

  $effect(() => {
    draft = value;
  });
</script>

<div class="py-2">
  <label class="block text-sm text-fg mb-1">{label}</label>
  {#if description}
    <p class="text-xs text-muted mb-1.5">{description}</p>
  {/if}
  <input
    type="text"
    class="w-full bg-surface border border-hairline rounded-md px-2.5 py-1.5 text-sm text-fg outline-none focus:border-accent/60 font-mono"
    {placeholder}
    bind:value={draft}
    onblur={() => oncommit(draft)}
    onkeydown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
  />
  {#if strict && warning}
    <p class="text-xs text-amber-400/90 mt-1">{warning}</p>
  {/if}
</div>
