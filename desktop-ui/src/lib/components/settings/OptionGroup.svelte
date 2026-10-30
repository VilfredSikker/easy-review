<script lang="ts">
  import SettingsRow from "./SettingsRow.svelte";
  import Segmented from "./Segmented.svelte";
  import Stepper from "./Stepper.svelte";
  import { isNumericCycle } from "./settingsModel";

  interface Props {
    label: string;
    description?: string;
    options: string[];
    value: string;
    onchange: (value: string) => void;
  }

  const { label, description = "", options, value, onchange }: Props = $props();
</script>

<SettingsRow {label} {description}>
  {#if isNumericCycle(options)}
    <Stepper {options} {value} {label} {onchange} />
  {:else}
    <Segmented {options} {value} ariaLabel={label} {onchange} />
  {/if}
</SettingsRow>
