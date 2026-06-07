import type { Meta, StoryObj } from "@storybook/svelte";
import BrowserView from "$lib/components/BrowserView.svelte";
import { browser } from "$lib/stores/browser.svelte";
import { app } from "$lib/stores/app.svelte";
import type { UiAnnotation } from "$lib/types";

// Minimal HTML loaded into the iframe so the stories render something visible
// without depending on a running dev server.
const SAMPLE_HTML = `data:text/html;charset=utf-8,${encodeURIComponent(`
<!doctype html>
<html><head><title>Sample</title>
<style>
  body { font: 14px system-ui; padding: 24px; color: #222; }
  button { padding: 8px 14px; border: 1px solid #888; border-radius: 6px; background: #fff; cursor: pointer; margin-right: 8px; }
  .card { border: 1px solid #ddd; border-radius: 8px; padding: 16px; margin-top: 16px; }
</style></head>
<body>
  <h1>Demo page</h1>
  <p>Annotate any element by clicking it while annotate-mode is on.</p>
  <button>Primary action</button>
  <button>Secondary</button>
  <div class="card">
    <h3>A card</h3>
    <p>With some body copy that reviewers might call out.</p>
  </div>
</body></html>`)}`;

const meta = {
  title: "Browser/BrowserView",
  component: BrowserView,
  parameters: { layout: "fullscreen", backgrounds: { default: "app" } },
} satisfies Meta<typeof BrowserView>;

export default meta;
type Story = StoryObj<typeof meta>;

function setupAnnotations(items: UiAnnotation[], annotateMode = false) {
  // Force a snapshot shaped just enough for the overlay/right-panel to render.
  app.snapshot = {
    mode: "branch",
    branch: "demo",
    base: "main",
    input_mode: "normal",
    files: [],
    selected_file: 0,
    current_hunk: null,
    filter: null,
    reviewed_count: 0,
    total_count: 0,
    ai: {
      fresh: true,
      stale_reason: null,
      summary_markdown: null,
      agent_summaries: {},
      high: 0,
      med: 0,
      low: 0,
      local_comment_count: 0,
      github_comment_count: 0,
      comments: 0,
      questions: 0,
      unpushed: 0,
      threads: [],
      findings: [],
      has_review_json: false,
      eligible_comment_count: 0,
      triage: null,
    },
    pr: null,
    panels: { left: true, tree: true, right: true },
    theme: "dark",
    watch_active: false,
    watch_status: { active: false, branch: null, root_path: null },
    worktrees: [],
    projects: [],
    local_branch: null,
    notification: null,
    tabs: [],
    active_tab: 0,
    bg_loading: { pr_list: false, gh_status: false, gh_comments: false },
    ui_annotations: items,
    browser: {
      url: SAMPLE_HTML,
      layout: "split",
      split_ratio: 0.45,
      annotate_mode: annotateMode,
      show_tooltips: false,
    },
  };
}

export const EmptyState: Story = {
  render: () => ({
    Component: BrowserView,
    onMount: () => {
      setupAnnotations([]);
    },
  }),
};

export const WithAnnotations: Story = {
  render: () => ({
    Component: BrowserView,
    onMount: () => {
      setupAnnotations([
        {
          id: "ann-1",
          url: "/",
          selector: "button:nth-of-type(1)",
          box_x: 24,
          box_y: 80,
          box_w: 120,
          box_h: 32,
          viewport_w: 1024,
          viewport_h: 600,
          text: "Primary button copy reads awkwardly — try 'Continue'.",
          timestamp: "0",
          author: "You",
          screenshot_path: null,
          stale: false,
        },
        {
          id: "ann-2",
          url: "/",
          selector: "button:nth-of-type(2)",
          box_x: 160,
          box_y: 80,
          box_w: 120,
          box_h: 32,
          viewport_w: 1024,
          viewport_h: 600,
          text: "Secondary action lacks visual hierarchy.",
          timestamp: "0",
          author: "You",
          screenshot_path: null,
          stale: false,
        },
        {
          id: "ann-3",
          url: "/",
          selector: ".card h3",
          box_x: 48,
          box_y: 200,
          box_w: 100,
          box_h: 24,
          viewport_w: 1024,
          viewport_h: 600,
          text: "Card heading needs better hierarchy.",
          timestamp: "0",
          author: "You",
          screenshot_path: null,
          stale: true,
        },
      ]);
    },
  }),
};

export const AnnotatingHover: Story = {
  render: () => ({
    Component: BrowserView,
    onMount: () => {
      setupAnnotations([], true);
      setTimeout(() => {
        window.dispatchEvent(new MessageEvent("message", {
          data: {
            __er_ready: true,
            href: SAMPLE_HTML,
          },
        }));
        window.dispatchEvent(new MessageEvent("message", {
          data: {
            __er_hover_result: true,
            selector: "button:nth-of-type(1)",
            rect: { left: 24, top: 92, width: 122, height: 36 },
            element_context: "button: Primary action",
            dom_context: {
              selector: "button:nth-of-type(1)",
              summary: "button: Primary action",
              node: {
                tag: "button",
                text: "Primary action",
                role: "button",
                classes: [],
                attrs: {},
              },
              rect: { left: 24, top: 92, width: 122, height: 36 },
              parent_chain: [{ tag: "body", text: "Demo page Annotate any element by clicking it while annotate-mode is on.", classes: [], attrs: {} }],
              nearby_text: "Demo page Annotate any element by clicking it while annotate-mode is on.",
              outer_html: "<button>Primary action</button>",
            },
          },
        }));
      }, 100);
    },
  }),
};
