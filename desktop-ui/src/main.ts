import { mount } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.svelte";
import { app } from "$lib/stores/app.svelte";
import { assertV1ServiceFlagAgreement } from "$lib/v1ServiceFlag";
import { attachConsole, error as logError, warn as logWarn } from "@tauri-apps/plugin-log";

// Route Rust log output to the browser devtools console.
attachConsole().catch(() => {});

function fmt(args: unknown[]): string {
  return args
    .map((a) => {
      if (typeof a === "string") return a;
      try { return JSON.stringify(a); } catch { return String(a); }
    })
    .join(" ");
}

const origError = console.error.bind(console);
const origWarn = console.warn.bind(console);
console.error = (...args: unknown[]) => {
  origError(...args);
  const msg = fmt(args);
  try { app.pushLog("error", "console", msg); } catch { /* a console call must never throw */ }
  logError(msg).catch(() => {});
};
console.warn = (...args: unknown[]) => {
  origWarn(...args);
  const msg = fmt(args);
  try { app.pushLog("warn", "console", msg); } catch { /* a console call must never throw */ }
  logWarn(msg).catch(() => {});
};

window.addEventListener("error", (e) => {
  const msg = `${e.message} @ ${e.filename}:${e.lineno}:${e.colno}`;
  app.pushLog("error", "window", msg);
  logError(`[window] ${msg}`).catch(() => {});
});
window.addEventListener("unhandledrejection", (e) => {
  const msg = String(e.reason);
  app.pushLog("error", "promise", msg);
  logError(`[promise] ${msg}`).catch(() => {});
});

// Bundle fonts so they work without a network round-trip to Google Fonts.
// Tauri WKWebView sometimes fails the fetch silently; local imports are reliable.
import "@fontsource/dm-sans/400.css";
import "@fontsource/dm-sans/500.css";
import "@fontsource/dm-sans/600.css";
import "@fontsource/dm-sans/700.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";

import "./app.css";

const target = document.getElementById("app");
if (!target) throw new Error("index.html is missing the #app mount point");

async function boot(): Promise<void> {
  let backend: boolean | undefined;
  try {
    backend = await invoke<boolean>("v1_service_enabled");
  } catch {
    // No Tauri runtime (a browser dev server): there is no compiled backend
    // to disagree with, so mount the UI as before.
  }
  if (backend !== undefined) {
    // A mismatch throws before the mount, so a build for one path cannot run
    // against the other.
    assertV1ServiceFlagAgreement(backend, import.meta.env.VITE_ER_V1_SERVICE);
  }
  mount(App, { target });
}

void boot();
