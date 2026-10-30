<script lang="ts">
  import type { ConfigFieldValue, ConfigHubField } from "$lib/types";
  import SettingsSection from "./SettingsSection.svelte";
  import SettingsRow from "./SettingsRow.svelte";
  import SettingsButton from "./SettingsButton.svelte";
  import Switch from "./Switch.svelte";
  import { groupAllOn, groupInboxFields, groupPatches, inboxOnCount, type InboxGroup } from "./inboxGroups";
  import { matchesQuery } from "./settingsModel";
  import { useSettingsQuery } from "./settingsContext";
  import { GENERAL_ROWS, rowTerms } from "./settingsSearch";

  interface Props {
    fields: ConfigHubField[];
    testBusy: boolean;
    onpatch: (key: string, value: ConfigFieldValue) => Promise<void>;
    ontest: () => void;
  }

  const { fields, testBusy, onpatch, ontest }: Props = $props();

  const query = useSettingsQuery();
  const groups = $derived(groupInboxFields(fields));
  const count = $derived(inboxOnCount(groups));
  // The group headers are the only text outside a row.
  const groupTitles = $derived(groups.map((g) => g.title));
  const testVisible = $derived(matchesQuery(query(), "Inbox", ...groupTitles, ...rowTerms(GENERAL_ROWS.testNotification)));
  let busyGroup = $state<string | null>(null);

  function visibleRows(group: InboxGroup) {
    if (matchesQuery(query(), "Inbox", group.title)) return group.rows;
    return group.rows.filter((r) => matchesQuery(query(), r.label, r.description));
  }

  // The engine has no batch key, so All/None is one patch per switch that has
  // to change, sent in order so each write sees the one before it.
  async function setGroup(group: InboxGroup) {
    busyGroup = group.title;
    try {
      for (const [key, value] of groupPatches(group, !groupAllOn(group))) {
        await onpatch(key, value);
      }
    } finally {
      busyGroup = null;
    }
  }
</script>

<SettingsSection title="Inbox" bare keywords={groupTitles}>
  {#snippet aside()}{count.on} of {count.total} on{/snippet}
  {#if testVisible}
    <div class="bg-card border border-line rounded-[10px] overflow-hidden">
      <SettingsRow {...GENERAL_ROWS.testNotification} always>
        <SettingsButton disabled={testBusy} onclick={ontest}>Send test</SettingsButton>
      </SettingsRow>
    </div>
  {/if}
  <div class="grid grid-cols-1 min-[880px]:grid-cols-2 gap-2.5 mt-2.5">
    {#each groups as group (group.title)}
      {@const rows = visibleRows(group)}
      {#if rows.length > 0}
        <div class="bg-card border border-line rounded-[10px] overflow-hidden">
          <div class="flex items-center gap-2 px-3.5 py-[9px] border-b border-line text-xs">
            <b class="font-semibold text-fg">{group.title}</b>
            <button
              type="button"
              class="ml-auto text-[11px] text-fg-3 px-1.5 py-0.5 rounded-[5px] border border-border hover:text-fg disabled:opacity-50"
              disabled={busyGroup !== null}
              onclick={() => void setGroup(group)}
            >{groupAllOn(group) ? "None" : "All"}</button>
            <span class="flex gap-3 text-[10px] uppercase tracking-[.06em] text-muted" aria-hidden="true">
              <span class="w-[38px] text-center" title="Listed in the inbox">List</span>
              <span class="w-[38px] text-center" title="Raises a system notification">Notify</span>
            </span>
          </div>
          {#each rows as row (row.kind)}
            <SettingsRow label={row.label} description={row.description} layout="tight" always>
              <span class="flex gap-3">
                {#each [row.show, row.notify] as field, i (i)}
                  <span class="w-[38px] flex justify-center">
                    {#if field}
                      <Switch
                        label="{i === 0 ? 'List' : 'Notify on'} {row.label}"
                        checked={field.value}
                        onchange={(v) => void onpatch(field.key, v)}
                      />
                    {/if}
                  </span>
                {/each}
              </span>
            </SettingsRow>
          {/each}
        </div>
      {/if}
    {/each}
  </div>
</SettingsSection>
