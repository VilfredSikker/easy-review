import js from "@eslint/js";
import svelte from "eslint-plugin-svelte";
import globals from "globals";
import tseslint from "typescript-eslint";
import classCohesion from "./scripts/eslint-rules/class-cohesion.js";
import moduleCohesion from "./scripts/eslint-rules/module-cohesion.js";

// Lint is a gate, in two tiers (docs/adr/0038-lint-is-a-ratcheted-gate.md):
//
// - error: correctness, best practice and this repo's conventions. Violations
//   that predate the gate are recorded in eslint-suppressions.json (ESLint bulk
//   suppressions); new code must be clean. Fixing a baselined site means
//   running `bun run lint:prune` so the baseline shrinks with it.
// - warn: maintainability signals (complexity, nesting, cohesion). Counted per
//   file and rule against eslint-warning-budget.json by `bun run lint`; a change
//   may not raise a count, and lowering one means `bun run lint:prune`.

const hexClass = String.raw`/(^|[\s:])[a-z-]+-\[#[0-9a-fA-F]{3,8}\]/`;
const hexMessage =
  "No hard-coded hex in classes. Use an @theme token from app.css (desktop-ui/agent.md, Rules with consequences).";
const snakeArgMessage =
  "Tauri command args are camelCase in JS; a snake_case key silently binds to None on the Rust side.";

/** Conventions from desktop-ui/agent.md, expressed as AST selectors. */
const projectRestrictions = [
  {
    selector: `SvelteAttribute[key.name='class'] SvelteLiteral[value=${hexClass}]`,
    message: hexMessage,
  },
  { selector: `Literal[value=${hexClass}]`, message: hexMessage },
  { selector: `TemplateElement[value.raw=${hexClass}]`, message: hexMessage },
  {
    selector: `CallExpression[callee.name='invoke'] > ObjectExpression.arguments:nth-child(2) > Property > Identifier.key[name=/_/]`,
    message: snakeArgMessage,
  },
  {
    selector: `CallExpression[callee.property.name='cmd'] > ObjectExpression.arguments:nth-child(2) > Property > Identifier.key[name=/_/]`,
    message: snakeArgMessage,
  },
];

/** Local rules with no off-the-shelf equivalent. */
const er = {
  rules: {
    "module-cohesion": moduleCohesion,
    "class-cohesion": classCohesion,
  },
};

export default tseslint.config(
  {
    ignores: ["node_modules", "dist", "storybook-static", ".tmp"],
  },
  {
    linterOptions: {
      reportUnusedDisableDirectives: "error",
      reportUnusedInlineConfigs: "error",
    },
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...svelte.configs.recommended,
  {
    languageOptions: {
      globals: {
        ...globals.browser,
        ...globals.node,
      },
    },
  },
  {
    files: ["**/*.svelte", "**/*.svelte.ts", "**/*.svelte.js"],
    languageOptions: {
      parserOptions: {
        parser: tseslint.parser,
        extraFileExtensions: [".svelte"],
      },
    },
  },

  // ── error tier: correctness, best practice, project conventions ──────────
  {
    rules: {
      // TypeScript already reports undefined names; no-undef misfires on
      // type-only names, ambient globals and Svelte 5 runes.
      "no-undef": "off",

      // Correctness
      eqeqeq: ["error", "smart"],
      "array-callback-return": "error",
      "default-case-last": "error",
      "no-alert": "error",
      "no-constructor-return": "error",
      "no-eval": "error",
      "no-implied-eval": "error",
      "no-new-func": "error",
      "no-param-reassign": "error",
      "no-self-compare": "error",
      "no-template-curly-in-string": "error",
      "no-throw-literal": "error",
      "no-unreachable-loop": "error",
      "prefer-promise-reject-errors": "error",
      radix: "error",
      "no-console": ["error", { allow: ["warn", "error", "info", "debug"] }],
      "no-unmodified-loop-condition": "error",
      // Probed and left off, because every hit was an idiom they misread:
      // no-return-assign (`onclick={() => (open = false)}`),
      // no-promise-executor-return (`new Promise((r) => requestAnimationFrame(r))`),
      // svelte/no-top-level-browser-globals (a Tauri webview has no SSR, so
      // top-level `window` is always defined).

      // Readability / idiom
      curly: ["error", "multi-line"],
      "no-else-return": "error",
      "no-lonely-if": "error",
      "no-useless-computed-key": "error",
      "no-useless-concat": "error",
      "no-useless-rename": "error",
      "no-useless-return": "error",
      "no-var": "error",
      "object-shorthand": "error",
      "prefer-object-has-own": "error",
      yoda: "error",

      // TypeScript
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
          caughtErrorsIgnorePattern: "^_",
          destructuredArrayIgnorePattern: "^_",
          ignoreRestSiblings: true,
        },
      ],
      // Off: a bare read (`diffFileCollapse.revision;`) is how a Svelte 5
      // effect or $derived.by subscribes to state it does not otherwise use.
      "@typescript-eslint/no-unused-expressions": "off",
      "@typescript-eslint/consistent-type-imports": [
        "error",
        { fixStyle: "inline-type-imports", disallowTypeAnnotations: false },
      ],
      "@typescript-eslint/no-import-type-side-effects": "error",
      "@typescript-eslint/no-shadow": "error",
      "@typescript-eslint/no-use-before-define": [
        "error",
        { functions: false, classes: false, variables: true, typedefs: false },
      ],
      "@typescript-eslint/prefer-for-of": "error",

      // Svelte
      "svelte/no-at-html-tags": "error",
      "svelte/prefer-svelte-reactivity": "error",
      "svelte/no-unused-svelte-ignore": "error",
      "svelte/require-each-key": "error",
      "svelte/prefer-writable-derived": "error",
      "svelte/no-useless-mustaches": ["error", { ignoreStringEscape: true }],
      "svelte/block-lang": ["error", { script: ["ts"], style: [null, "postcss"] }],
      "svelte/button-has-type": "error",
      "svelte/no-dom-manipulating": "error",
      "svelte/no-extra-reactive-curlies": "error",
      "svelte/no-ignored-unsubscribe": "error",
      "svelte/no-reactive-reassign": "error",
      "svelte/no-svelte-internal": "error",
      "svelte/no-target-blank": "error",
      "svelte/no-unnecessary-state-wrap": "error",
      "svelte/no-useless-children-snippet": "error",
      "svelte/require-store-reactive-access": "error",
      "svelte/require-stores-init": "error",
      "svelte/shorthand-attribute": "error",
      "svelte/shorthand-directive": "error",
      "svelte/valid-compile": "error",

      // Project conventions (desktop-ui/agent.md)
      "no-restricted-syntax": ["error", ...projectRestrictions],
    },
  },
  {
    // Svelte props destructure with `let`; svelte/prefer-const understands runes.
    files: ["**/*.svelte", "**/*.svelte.ts", "**/*.svelte.js"],
    rules: {
      "prefer-const": "off",
      "svelte/prefer-const": "error",
    },
  },
  {
    // Dev CLIs print to stdout by design, and the dev loggers are the one
    // place app code reaches console.log (gated by ER_LOG groups).
    files: ["scripts/**", "src/lib/dev/log.ts", "src/lib/arena/log.ts"],
    rules: { "no-console": "off" },
  },
  {
    // The one markdown renderer: markdown.ts escapes every text run before it
    // reaches {@html}. New {@html} anywhere else stays an error.
    files: ["src/lib/components/ui/MarkdownText.svelte"],
    rules: { "svelte/no-at-html-tags": "off" },
  },

  // ── warn tier: maintainability budget (eslint-warning-budget.json) ────────
  {
    // A file or class is flagged when its code falls into groups that never
    // reference each other, and the warning names the groups: that list is the
    // split (scripts/eslint-rules/). There is deliberately no file-length cap,
    // because a length cap splits one coherent idea across two files.
    plugins: { er },
    rules: {
      "er/module-cohesion": ["warn", { maxGroups: 2, minGroupLines: 60 }],
      "er/class-cohesion": ["warn", { maxGroups: 1, minGroupLines: 40 }],
      complexity: ["warn", 20],
      "max-depth": ["warn", 4],
      "max-nested-callbacks": ["warn", 4],
      "max-params": ["warn", 5],
      "no-nested-ternary": "warn",
      "@typescript-eslint/no-non-null-assertion": "warn",
    },
  },
  {
    // Tests and stories probe shapes loosely, stub freely, and group by
    // describe block rather than by responsibility.
    files: ["**/*.test.ts", "src/lib/stories/**"],
    rules: {
      "@typescript-eslint/no-non-null-assertion": "off",
      "no-nested-ternary": "off",
      "er/module-cohesion": "off",
      "max-nested-callbacks": "off",
    },
  },
);
