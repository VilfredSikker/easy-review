<script lang="ts">
  import type { Snippet } from "svelte";

  // The page's own button: sentence case at row height. The shared ui/Button
  // is uppercase chrome for toolbars, which reads as shouting in a form.
  interface Props {
    children: Snippet;
    variant?: "default" | "ghost" | "danger" | "ai";
    type?: "button" | "submit";
    disabled?: boolean;
    title?: string;
    onclick?: (e: MouseEvent) => void;
  }

  const { children, variant = "default", type = "button", disabled = false, title, onclick }: Props = $props();

  const VARIANT: Record<NonNullable<Props["variant"]>, string> = {
    default: "border-border bg-card text-fg hover:border-fg-3",
    ghost: "border-transparent text-fg-3 hover:bg-hover hover:text-fg",
    danger: "border-border bg-card text-risk-high hover:border-risk-high/60",
    ai: "border-accent-border bg-card text-accent hover:bg-accent-soft",
  };
</script>

<button
  {type}
  {disabled}
  {title}
  class="inline-flex items-center gap-1.5 h-[26px] px-2.5 rounded-[7px] border text-xs font-medium whitespace-nowrap transition-colors disabled:opacity-50 disabled:cursor-not-allowed {VARIANT[variant]}"
  {onclick}
>
  {@render children()}
</button>
