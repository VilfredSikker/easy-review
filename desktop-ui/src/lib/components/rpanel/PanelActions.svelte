<script module lang="ts">
  export interface PanelAction {
    glyph: string;
    label: string;
    title?: string;
    disabled?: boolean;
    run: () => void;
  }
</script>

<script lang="ts">
  interface Props {
    actions: PanelAction[];
  }

  const { actions }: Props = $props();
</script>

<div class="grid gap-0.5 border-t border-hairline pt-2">
  {#each actions as action (action.label)}
    <button
      type="button"
      title={action.title}
      disabled={action.disabled}
      onclick={action.run}
      class="text-left px-1.5 py-[5px] rounded-md mono text-[12px] text-fg-2 hover:bg-hover hover:text-fg
        disabled:opacity-40 disabled:pointer-events-none"
    ><span aria-hidden="true">{action.glyph}</span> {action.label}</button>
  {/each}
</div>
