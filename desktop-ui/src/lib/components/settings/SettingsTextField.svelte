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

<div class="py-3">
  <label class="block text-sm text-fg mb-0.5">{label}</label>
  {#if description}
    <p class="text-xs text-muted mb-1.5">{description}</p>
  {/if}
  <input
    type="text"
    class="w-full bg-ink-850 border border-hairline rounded-md px-2.5 py-1.5 text-sm text-fg outline-none font-mono transition-colors placeholder:text-ink-300 hover:border-border focus:border-accent/60"
    {placeholder}
    bind:value={draft}
    onblur={() => oncommit(draft)}
    onkeydown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
  />
  {#if strict && warning}
    <p class="flex items-center gap-1.5 text-xs text-risk-med mt-1.5">
      <span aria-hidden="true">⚠</span>
      {warning}
    </p>
  {/if}
</div>
