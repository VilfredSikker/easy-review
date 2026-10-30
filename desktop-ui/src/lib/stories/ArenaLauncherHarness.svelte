<script lang="ts">
  import { onDestroy } from "svelte";
  import ArenaLauncher from "$lib/components/arena/ArenaLauncher.svelte";
  import type { AiProviderInfo, ReviewerInfo } from "$lib/types";
  import type { ArenaEstimate } from "$lib/types/arena";

  const PROVIDERS: AiProviderInfo[] = [
    {
      id: "claude",
      label: "Claude",
      is_selected: true,
      models: [
        { id: "sonnet", label: "Sonnet", is_selected: true, description: "Broad correctness", effort_levels: ["low", "medium", "high"] },
        { id: "opus", label: "Opus", is_selected: false, description: "Deep general pass", effort_levels: ["low", "medium", "high", "max"], cost_per_1k_in: 0.015, cost_per_1k_out: 0.075 },
      ],
    },
    {
      id: "codex",
      label: "Codex",
      is_selected: false,
      models: [{ id: "gpt", label: "GPT", is_selected: true, effort_levels: [] }],
    },
  ];

  const REVIEWERS: ReviewerInfo[] = [
    { kind: "expert:testing", label: "Tests", description: "Coverage gaps" },
    { kind: "expert:performance", label: "Performance", description: "Hot paths" },
    { kind: "expert:security", label: "Security", description: "Authn, injection, secrets" },
  ];

  const ESTIMATE: ArenaEstimate = { diff_bytes: 18_000, cost_usd: 0.42, latency_sec: 140, cost_limit_usd: 25, max_rounds: 3 };

  // Set before the launcher mounts: its effect asks for providers on first run.
  const previous = Reflect.get(window, "__TAURI_INTERNALS__");
  Reflect.set(window, "__TAURI_INTERNALS__", {
    invoke: async (command: string) => {
      if (command === "list_ai_providers") return PROVIDERS;
      if (command === "list_ai_reviewers") return REVIEWERS;
      if (command === "arena_estimate") return ESTIMATE;
      throw new Error(`${command} is not mocked in this story`);
    },
  });
  onDestroy(() => Reflect.set(window, "__TAURI_INTERNALS__", previous));
</script>

<div class="h-screen bg-bg">
  <ArenaLauncher open onClose={() => {}} />
</div>
