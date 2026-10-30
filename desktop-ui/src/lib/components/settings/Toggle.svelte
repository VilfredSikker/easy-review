<script lang="ts">
  import SettingsRow from "./SettingsRow.svelte";
  import Switch from "./Switch.svelte";

  interface Props {
    checked: boolean;
    label: string;
    description?: string;
    disabled?: boolean;
    layout?: "row" | "tight";
    onchange: (checked: boolean) => void;
  }

  const { checked, label, description = "", disabled = false, layout = "row", onchange }: Props = $props();
</script>

<SettingsRow {label} {description} {layout}>
  <Switch {checked} {label} {disabled} {onchange} />
</SettingsRow>
