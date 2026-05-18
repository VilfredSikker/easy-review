import type { StorybookConfig } from "@storybook/svelte-vite";
import { fileURLToPath, URL } from "node:url";

const config: StorybookConfig = {
  stories: ["../src/**/*.stories.@(ts|svelte)"],
  addons: [],
  framework: {
    name: "@storybook/svelte-vite",
    options: {},
  },
  typescript: { check: false },
  viteFinal: async (config) => {
    // Reuse the same $lib alias as the main Vite build.
    config.resolve = config.resolve ?? {};
    config.resolve.alias = {
      ...(config.resolve.alias ?? {}),
      $lib: fileURLToPath(new URL("../src/lib", import.meta.url)),
    };
    return config;
  },
};

export default config;
