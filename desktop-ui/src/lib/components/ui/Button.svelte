<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    children: Snippet;
    variant?: "primary" | "secondary" | "ghost" | "danger";
    size?: "sm" | "md";
    type?: "button" | "submit" | "reset";
    disabled?: boolean;
    onclick?: (e: MouseEvent) => void;
    class?: string;
    title?: string;
    "aria-label"?: string;
  }

  const {
    children,
    variant = "secondary",
    size = "sm",
    type = "button",
    disabled = false,
    onclick,
    class: extra = "",
    title,
    "aria-label": ariaLabel,
  }: Props = $props();

  const base = "rounded-md font-medium uppercase tracking-wider transition-colors";

  const VARIANT_CLASS: Record<NonNullable<Props["variant"]>, string> = {
    primary: "bg-accent text-on-accent hover:bg-accent-hover",
    ghost: "text-fg-3 hover:bg-hover",
    danger: "bg-risk-high/15 text-risk-high border border-risk-high/40 hover:bg-risk-high/25 disabled:opacity-40",
    secondary: "border border-border text-fg-2 hover:bg-hover",
  };

  const variantClass = $derived(VARIANT_CLASS[variant]);

  const sizeClass = $derived(size === "md" ? "px-3 py-2 text-xs" : "px-3 py-1.5 text-xs");
</script>

<button
  {type}
  {disabled}
  {onclick}
  {title}
  aria-label={ariaLabel}
  class="{base} {variantClass} {sizeClass} {extra}"
>
  {@render children()}
</button>
