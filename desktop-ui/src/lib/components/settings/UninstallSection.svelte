<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import SettingsRow from "./SettingsRow.svelte";
  import SettingsButton from "./SettingsButton.svelte";
  import { INPUT_CLASS } from "./settingsModel";
  import { GENERAL_ROWS } from "./settingsSearch";

  interface Props {
    /** Load the preview on mount: the `er.focusUninstall` deep link lands here. */
    autoPreview?: boolean;
    /** The deep link has been acted on; the page clears it so a later visit
     *  to General does not preview and scroll again. */
    onautopreviewed?: () => void;
  }

  const { autoPreview = false, onautopreviewed }: Props = $props();

  interface UninstallPreview {
    targets: { kind: string; path: string; exists: boolean; description: string }[];
    existingCount: number;
  }

  const fullUninstallRequest = {
    removeConfig: true,
    removeData: true,
    removeCache: true,
    removeBinaries: true,
    removeDesktopApp: true,
  };

  let preview = $state<UninstallPreview | null>(null);
  let loading = $state(false);
  let confirming = $state(false);
  let typed = $state("");
  let busy = $state(false);

  async function loadPreview() {
    loading = true;
    try {
      preview = await invoke<UninstallPreview>("preview_uninstall", { request: fullUninstallRequest });
    } catch (e) {
      app.showToast("error", `preview_uninstall: ${e}`);
    } finally {
      loading = false;
    }
  }

  async function confirmUninstall() {
    if (typed.trim() !== "uninstall") return;
    busy = true;
    try {
      await invoke("run_uninstall", { request: fullUninstallRequest });
      app.showToast("success", "Easy Review removed — quitting…");
    } catch (e) {
      app.showToast("error", String(e));
      busy = false;
      await loadPreview();
    }
  }

  $effect(() => {
    if (!autoPreview) return;
    untrack(() => onautopreviewed?.());
    void loadPreview().then(() => {
      queueMicrotask(() => {
        document.getElementById("settings-uninstall")?.scrollIntoView({ behavior: "smooth", block: "start" });
      });
    });
  });
</script>

<SettingsSection id="settings-uninstall" title="Danger zone" danger>
  <SettingsRow {...GENERAL_ROWS.uninstall}>
    {#if !confirming}
      <SettingsButton onclick={() => void loadPreview()}>
        {preview ? "Refresh list" : "Show what will be removed"}
      </SettingsButton>
      <SettingsButton
        variant="danger"
        disabled={!preview || preview.existingCount === 0 || busy}
        onclick={() => (confirming = true)}
      >Uninstall…</SettingsButton>
    {/if}
  </SettingsRow>
  {#if preview || loading}
    <SettingsRow always layout="col">
      {#if preview}
        <ul class="space-y-1 text-xs font-mono text-fg-3 max-h-40 overflow-y-auto">
          {#each preview.targets as t (t.path)}
            <li class="flex gap-2">
              <span class={t.exists ? "text-fg-2" : "text-muted"}>{t.exists ? "•" : "·"}</span>
              <span class="min-w-0 break-all">{t.description}{t.exists ? "" : " (not present)"}</span>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="text-xs text-muted">Scanning install locations…</p>
      {/if}
    </SettingsRow>
  {/if}
  {#if confirming}
    <SettingsRow always layout="col">
      <p class="text-[13px] text-fg">
        Type <span class="font-mono text-risk-high">uninstall</span> to confirm. The app quits afterwards.
      </p>
      <div class="flex gap-2">
        <input
          type="text"
          class={INPUT_CLASS}
          aria-label="Type uninstall to confirm"
          placeholder="uninstall"
          bind:value={typed}
          disabled={busy}
          onkeydown={(e) => e.stopPropagation()}
        />
        <SettingsButton
          variant="danger"
          disabled={typed.trim() !== "uninstall" || busy}
          onclick={() => void confirmUninstall()}
        >{busy ? "Removing…" : "Remove everything"}</SettingsButton>
        <SettingsButton
          variant="ghost"
          disabled={busy}
          onclick={() => {
            confirming = false;
            typed = "";
          }}
        >Cancel</SettingsButton>
      </div>
    </SettingsRow>
  {/if}
</SettingsSection>
