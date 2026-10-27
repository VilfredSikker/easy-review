import type { StorybookConfig } from "@storybook/svelte-vite";
import { fileURLToPath, URL } from "node:url";
import { realpathSync } from "node:fs";
import { searchForWorkspaceRoot } from "vite";

const config: StorybookConfig = {
  stories: ["../src/**/*.stories.@(ts|svelte)"],
  addons: [],
  framework: {
    name: "@storybook/svelte-vite",
    options: {},
  },
  typescript: { check: false },
  viteFinal: async (viteConfig) => {
    viteConfig.server ??= {};
    viteConfig.server.fs ??= {};
    viteConfig.server.fs.allow = [
      ...(viteConfig.server.fs.allow ?? [searchForWorkspaceRoot(fileURLToPath(new URL("..", import.meta.url)))]),
      realpathSync(new URL("../node_modules", import.meta.url)),
    ];
    // Reuse the same $lib alias as the main Vite build.
    viteConfig.resolve = viteConfig.resolve ?? {};
    viteConfig.resolve.alias = {
      ...(viteConfig.resolve.alias ?? {}),
      $lib: fileURLToPath(new URL("../src/lib", import.meta.url)),
    };
    return viteConfig;
  },
};

export default config;
