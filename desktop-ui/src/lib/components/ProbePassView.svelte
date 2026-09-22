<script lang="ts">
  import { tick } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { probePass } from "$lib/stores/probePass.svelte";
  import { diffFileCollapse } from "$lib/stores/diffFileCollapse.svelte";
  import { diffNav } from "$lib/stores/diffNav.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import type { ProbeSnapshot } from "$lib/types";

  const probes = $derived(probePass.probes);
  const filesClosed = $derived(probePass.filesClosed);
  const probeTask = $derived(
    (app.snapshot?.background_tasks ?? []).find(
      (t) => t.kind === "probes" || t.kind === "probe-answers",
    ),
  );
  const running = $derived(probeTask?.status === "running");
  const failError = $derived(
    probeTask?.status === "failed"
      ? (probeTask.error ? probeTask.error : "Probe pass failed")
      : "",
  );
  const unstamped = $derived(probes.filter((p) => !p.stamp));

  let lastOpened = "";

  $effect(() => {
    const open = probes.find((p) => p.stamp === "fail" || p.stamp === "empty");
    if (!open || filesClosed) return;
    if (lastOpened === open.id) return;
    lastOpened = open.id;
    void openClaim(open);
  });

  async function openClaim(probe: ProbeSnapshot) {
    const snap = app.snapshot;
    if (!snap) return;
    const f = snap.files.find((file) => file.path === probe.file);
    if (!f) return;
    await app.cmd("select_file", { idx: f.source_index });
    await tick();
    diffFileCollapse.expand(probe.file);
    diffNav.scrollToFile(probe.file);
  }

  function stampClass(stamp: ProbeSnapshot["stamp"]) {
    if (stamp === "pass") return "text-add-fg";
    if (stamp === "fail") return "text-del-fg";
    if (stamp === "empty") return "text-warning";
    return "text-muted";
  }

  function stampLabel(stamp: ProbeSnapshot["stamp"]) {
    if (stamp === "pass") return "pass";
    if (stamp === "fail") return "fail";
    if (stamp === "empty") return "empty";
    return "unstamped";
  }
</script>

<div class="flex flex-col min-h-0 {filesClosed ? 'flex-1' : 'w-80 shrink-0'} border-r border-hairline bg-ink-870">
  <div class="px-3 py-2 border-b border-hairline flex items-center gap-2">
    <SectionLabel>Probe pass</SectionLabel>
    <span class="text-[10px] text-muted ml-auto">PoC · cap 5</span>
    <button
      type="button"
      class="text-[10px] uppercase tracking-wider text-fg-3 hover:text-fg-1"
      onclick={() => probePass.exit()}
    >Close</button>
  </div>

  <p class="px-3 py-2 text-[11px] text-fg-3 leading-snug">
    The Hub writes Questions. You pick which to run. Files stay closed unless a probe fails or comes back empty.
  </p>

  <div class="px-3 pb-2 flex flex-wrap gap-1">
    <Button
      variant="primary"
      disabled={running}
      onclick={() => void app.cmd("run_probe_pass")}
    >Write probes</Button>
    <Button
      disabled={running}
      onclick={() => void app.cmd("seed_probe_pass")}
    >Seed probes</Button>
    <Button
      disabled={running || probePass.selectedIds.length === 0}
      onclick={() => void app.cmd("answer_probes", { ids: probePass.selectedIds })}
    >Answer selected</Button>
    <Button
      disabled={unstamped.length === 0}
      onclick={() => probePass.selectAll()}
    >Select unstamped</Button>
  </div>
  <div class="px-3 pb-2 flex flex-wrap gap-1">
    <Button
      disabled={running || probePass.selectedIds.length === 0}
      onclick={() => void app.cmd("seed_probe_answers", { ids: probePass.selectedIds, stamp: "pass" })}
    >Seed pass</Button>
    <Button
      disabled={running || probePass.selectedIds.length === 0}
      onclick={() => void app.cmd("seed_probe_answers", { ids: probePass.selectedIds, stamp: "fail" })}
    >Seed fail</Button>
    <Button
      disabled={running || probePass.selectedIds.length === 0}
      onclick={() => void app.cmd("seed_probe_answers", { ids: probePass.selectedIds, stamp: "empty" })}
    >Seed empty</Button>
  </div>

  {#if running}
    <p class="px-3 pb-2 text-[11px] text-accent">Hub running…</p>
  {:else if failError}
    <p class="px-3 pb-2 text-[11px] text-del-fg">{failError}</p>
  {/if}

  {#if filesClosed}
    <p class="px-3 pb-2 text-[11px] text-muted">Diff closed. No fail or empty stamp.</p>
  {:else}
    <p class="px-3 pb-2 text-[11px] text-del-fg">A probe failed or came back empty. Opening that claim.</p>
  {/if}

  <div class="flex-1 min-h-0 overflow-y-auto">
    {#if probes.length === 0}
      <p class="px-3 py-4 text-[12px] text-muted">No probes yet. Write probes (AI Hub) or Seed probes (PoC).</p>
    {:else}
      <ul class="divide-y divide-hairline">
        {#each probes as probe (probe.id)}
          <li class="px-3 py-2">
            <label class="flex items-start gap-2 text-[12px] text-fg-1">
              <input
                type="checkbox"
                class="mt-0.5"
                checked={probePass.selected.has(probe.id)}
                disabled={!!probe.stamp}
                onchange={() => probePass.toggle(probe.id)}
              />
              <span class="min-w-0 flex-1">
                <span class="block text-[10px] uppercase tracking-wider {stampClass(probe.stamp)}">{stampLabel(probe.stamp)}</span>
                <span class="block leading-snug">{probe.text}</span>
                {#if probe.stamp === "fail" || probe.stamp === "empty"}
                  <button
                    type="button"
                    class="mt-1 text-[10px] text-fg-3 hover:text-fg-1"
                    onclick={() => void openClaim(probe)}
                  >{probe.file}{probe.line ? `:${probe.line}` : ""}</button>
                {:else}
                  <span class="mt-1 block text-[10px] text-muted">{probe.file}</span>
                {/if}
              </span>
            </label>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>
