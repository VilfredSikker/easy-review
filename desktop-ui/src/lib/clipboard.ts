import { writeText as tauriWriteText } from "@tauri-apps/plugin-clipboard-manager";

export async function copyToClipboard(text: string): Promise<void> {
  try {
    await tauriWriteText(text);
  } catch (e) {
    // Fallback for storybook / non-Tauri contexts.
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      throw e;
    }
  }
}
