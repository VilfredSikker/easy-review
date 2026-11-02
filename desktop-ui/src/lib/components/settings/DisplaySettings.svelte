<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { kindShown, toggleKindShown, type CommentVisibility } from "$lib/commentVisibility";
  import SettingsSection from "./SettingsSection.svelte";
  import SettingsRow from "./SettingsRow.svelte";
  import Switch from "./Switch.svelte";
  import { DISPLAY_ANNOTATION_ROWS, DISPLAY_LAYOUT_ROWS } from "./settingsSearch";
  import { settingsSectionDomId } from "$lib/settingsLink";

  /*
   * The desktop's diff display, the same values the diff's display menu flips.
   * They live in this app only: the Terminal tab's display rows are the TUI's
   * (`config.toml`), and only the theme is shared between the two.
   */

  const vis = $derived(app.commentVisibility);

  type ShowKey = Parameters<typeof kindShown>[1];
  const annotationKeys: Record<keyof typeof DISPLAY_ANNOTATION_ROWS, ShowKey> = {
    comments: "hideComments",
    findings: "hideFindings",
    questions: "hideQuestions",
    notes: "hideNotes",
    resolved: "hideResolved",
    outdated: "hideOutdated",
  };
  const annotationRows = Object.entries(annotationKeys) as [keyof typeof DISPLAY_ANNOTATION_ROWS, ShowKey][];

  function setShown(key: ShowKey, on: boolean, current: CommentVisibility) {
    if (kindShown(current, key) === on) return;
    app.setCommentVisibility(toggleKindShown(current, key));
  }
</script>

<SettingsSection id={settingsSectionDomId("display")} title="Diff display">
  <SettingsRow {...DISPLAY_LAYOUT_ROWS.split}>
    <Switch
      label={DISPLAY_LAYOUT_ROWS.split.label}
      checked={app.diffViewMode === "split"}
      onchange={(on) => app.setDiffViewMode(on ? "split" : "unified")}
    />
  </SettingsRow>
  <SettingsRow {...DISPLAY_LAYOUT_ROWS.wrap}>
    <Switch
      label={DISPLAY_LAYOUT_ROWS.wrap.label}
      checked={app.wrapLines}
      onchange={(on) => {
        if (on !== app.wrapLines) app.toggleWrapLines();
      }}
    />
  </SettingsRow>
  <SettingsRow {...DISPLAY_LAYOUT_ROWS.lineNumbers}>
    <Switch
      label={DISPLAY_LAYOUT_ROWS.lineNumbers.label}
      checked={app.lineNumbers}
      onchange={(on) => app.setLineNumbers(on)}
    />
  </SettingsRow>
  <SettingsRow {...DISPLAY_LAYOUT_ROWS.hideAll}>
    <Switch
      label={DISPLAY_LAYOUT_ROWS.hideAll.label}
      checked={vis.hideAll}
      onchange={(on) => app.setCommentVisibility({ hideAll: on })}
    />
  </SettingsRow>
  {#each annotationRows as [row, key] (row)}
    <SettingsRow {...DISPLAY_ANNOTATION_ROWS[row]}>
      <Switch
        label={DISPLAY_ANNOTATION_ROWS[row].label}
        checked={kindShown(vis, key)}
        onchange={(on) => setShown(key, on, vis)}
      />
    </SettingsRow>
  {/each}
</SettingsSection>
