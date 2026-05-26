import { invoke } from "@tauri-apps/api/core";

/** Open an http(s) URL in the system browser (Tauri) or a new tab (web/Storybook). */
export async function openExternalUrl(url: string): Promise<void> {
  const trimmed = url.trim();
  if (!trimmed) return;
  try {
    await invoke<void>("open_url_in_browser", { url: trimmed });
  } catch {
    window.open(trimmed, "_blank", "noopener,noreferrer");
  }
}

/** True when href is an absolute http(s) URL suitable for external open. */
export function isExternalHttpUrl(href: string | null | undefined): boolean {
  if (!href) return false;
  try {
    const u = new URL(href, window.location.href);
    return u.protocol === "http:" || u.protocol === "https:";
  } catch {
    return false;
  }
}

/** Capture-phase handler: prevent in-app navigation for external links. */
export function onExternalLinkClick(e: MouseEvent): void {
  if (e.defaultPrevented || e.button !== 0) return;
  const anchor = (e.target as HTMLElement | null)?.closest?.("a[href]");
  if (!anchor || !(anchor instanceof HTMLAnchorElement)) return;
  const href = anchor.getAttribute("href");
  if (!isExternalHttpUrl(href)) return;
  e.preventDefault();
  e.stopPropagation();
  void openExternalUrl(href!);
}

/** Install a document-level capture listener (runs for welcome + review UI). */
export function installExternalLinkGuard(): () => void {
  document.addEventListener("click", onExternalLinkClick, true);
  return () => document.removeEventListener("click", onExternalLinkClick, true);
}
