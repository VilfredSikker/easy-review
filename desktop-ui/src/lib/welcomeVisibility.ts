import type { MainViewMode } from "$lib/stores/app.svelte";

export interface WelcomeInputs {
  /** The snapshot has no repo, no diff and no saved projects. */
  naturalEmpty: boolean;
  /** The user asked for the welcome and no tabs are open. */
  explicitFullWelcome: boolean;
  loading: boolean;
  mainView: MainViewMode;
}

/**
 * Whether the full-screen welcome replaces the app shell.
 *
 * The welcome links to settings, and with no repo open `naturalEmpty` stays
 * true, so the settings view must win or those links do nothing.
 */
export function showFullWelcome(s: WelcomeInputs): boolean {
  return (s.naturalEmpty || s.explicitFullWelcome) && !s.loading && s.mainView !== "settings";
}
