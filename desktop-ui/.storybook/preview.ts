import type { Preview } from "@storybook/svelte";

// Bundle the same fonts the app uses.
import "@fontsource/dm-sans/400.css";
import "@fontsource/dm-sans/500.css";
import "@fontsource/dm-sans/600.css";
import "@fontsource/dm-sans/700.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";

// Tailwind + theme tokens.
import "../src/app.css";

const preview: Preview = {
  parameters: {
    backgrounds: {
      default: "app",
      values: [
        { name: "app", value: "#0a0a0a" },
        { name: "rail", value: "#0c0c0c" },
        { name: "card", value: "#141414" },
      ],
    },
    layout: "fullscreen",
    controls: { matchers: { color: /(background|color)$/i, date: /Date$/i } },
  },
};

export default preview;
