# T15 — er-mcp browser-annotations tool

Status: ready-for-agent
Depends on: T12 (v2 storage and assets), T14 (prompt renderer)
Size: 1–2 days
Risk: ordinary (read-only tool; must not leak paths outside managed storage)

> **For the executor:** Build exactly this, in order.

## In plain terms

- **The problem.** Agents can read a PR's review questions and notes through the
  er-mcp server, but not the reviewer's browser annotations, and nothing can hand
  them the screenshots.
- **The fix.** A new read-only er-mcp tool that returns a PR's (or the current
  branch's) browser annotations in the shared prompt format, with each screenshot
  as an image the agent can see, each preceded by its caption.
- **TL;DR.** Agents fetch browser notes and their pictures through er-mcp.

## Definition of done

- [ ] Tool `pr_browser_annotations` registered in `crates/er-mcp/src/server.rs`
      with arguments `{ ...PrRefFields, bucket: Option<String>, include_images: Option<bool> /* default true */ }`.
- [ ] Response content blocks, in order: one text block with the T14 prompt text;
      then for each image a text block with its caption followed by an image block
      (`image/png`, base64); then one text block with the v2 records as JSON.
- [ ] Response never contains an absolute filesystem path.
- [ ] A load error returns a tool error naming the problem; it never returns an
      empty success.
- [ ] Tests in Step 4 pass.

## Before you start

- **Existing pattern to copy:** `pr_feedback_get` at `crates/er-mcp/src/server.rs:1066`:
  ```rust
  async fn pr_feedback_get(&self, Parameters(args): Parameters<PrFeedbackGetArgs>) -> Result<CallToolResult, McpError> {
      let bucket = feedback_bucket(args.bucket.as_deref())?;
      let pr = resolve_feedback_target(&args.target, bucket)?;
      …
      let er_dir = pr.bucket_path.clone();
      let feedback = tokio::task::spawn_blocking(move || { … }).await …;
  ```
  Args struct `PrFeedbackGetArgs` at line 168 (`#[serde(flatten)] target: PrRefFields`,
  `bucket`, `include_resolved`). Helpers: `text_json` (line 234, builds
  `ContentBlock::text`), `tool_err`, `feedback_bucket` (258), `resolve_feedback_target`
  (300). Tests at the bottom of the file (`#[cfg(test)]` at line 1428), for example
  `feedback_tools_advertise_bucket_selector`.
- **rmcp:** version 3.1 (`crates/er-mcp/Cargo.toml`). Image content:
  `TODO(executor): confirm the image constructor — search the rmcp 3.1 source for
  "fn image" in the ContentBlock / Content type (likely
  ContentBlock::image(base64_data, "image/png"))`.
- **Engine functions:** `load_browser_annotations(dir)` and
  `read_browser_asset(dir, asset_id)` (T12), `render_browser_annotations` (T14).
- **Bucket directory:** `pr.bucket_path` from `resolve_feedback_target`.
  `TODO(executor): confirm bucket_path equals TabState::comments_dir() for the same
  view (crates/er-engine/src/app/state/mod.rs:2314 says comments_dir() is er_dir());
  open a PR tab in the app, save an annotation, and check the file lands in
  bucket_path.` If they differ, stop and ask.

## Scope

- **In scope:** the tool, its args struct, tests, the tool description string.
- **Out of scope:** reply/resolve actions for browser annotations; changes to the
  er-mcp skills under `skills/` (note in the PR that the `er-get-feedback` skill
  could mention the new tool; do not edit it here).

## Steps

### Step 1 — Args and registration

- **Do this:** `PrBrowserAnnotationsArgs` like `PrFeedbackGetArgs`, with
  `include_images`. Add the `#[tool(description = "Read the reviewer's browser annotations for a PR, with screenshots. bucket=local reads the current checked-out branch's local branch bucket.")]`
  method beside `pr_feedback_get`.

### Step 2 — Body

- **Do this:** resolve bucket and target as `pr_feedback_get` does; in
  `spawn_blocking`: load v2 annotations (map `Err` to `tool_err`), render with T14,
  read each referenced asset by ID. Build content blocks as in the definition of
  done. Cap total image bytes at 16 MiB per response; beyond that, emit captions
  with "image omitted: response size limit" instead of images.

### Step 3 — Path hygiene

- **Do this:** the JSON records include `asset_id`, never a path. Scan the final
  text blocks in a test for the bucket directory string and assert it is absent.

### Step 4 — Tests

- Registration test like `feedback_tools_advertise_bucket_selector` for the new
  tool's `bucket` and `include_images` properties.
- Integration test with a temp bucket directory: two annotations (one with a PNG
  asset) → content order text, caption, image, JSON; image bytes equal the asset;
  no absolute path in any text block; malformed file → error; `include_images=false`
  → no image blocks.
- Run `cargo test -p er-mcp` (through `locked.sh`).

## Rollback

Remove the tool and its tests.

## When to stop and ask

- `bucket_path` and `comments_dir()` differ for the same view.
- rmcp 3.1 has no image content type.
