import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

// Force a full page reload when any Svelte component or CSS changes.
// Tailwind v4 + Tauri webview occasionally drops CSS HMR silently,
// so we trade a slight refresh delay for guaranteed-up-to-date styles.
const forceReload = {
  name: "force-full-reload-on-style-changes",
  handleHotUpdate({ file, server }: { file: string; server: { ws: { send: (msg: { type: "full-reload" }) => void } } }) {
    if (/\.css$/.test(file)) {
      server.ws.send({ type: "full-reload" });
      return [];
    }
  },
};

export default defineConfig({
  define: {
    "import.meta.env.VITE_ER_LOG": JSON.stringify(process.env.ER_LOG ?? ""),
    "import.meta.env.VITE_ER_DESKTOP_PROFILE_POLL": JSON.stringify(
      process.env.ER_DESKTOP_PROFILE_POLL ?? "",
    ),
  },
  plugins: [svelte(), tailwindcss(), forceReload],
  worker: {
    format: "es",
  },
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL("./src/lib", import.meta.url)),
    },
  },
  clearScreen: false,
  server: { port: 5183, strictPort: true },
  envPrefix: ["VITE_", "TAURI_"],
});
