# Agent Panel — AI Terminal in the Side Panel

## Overview

A new tab inside the existing AI Side Panel that lets you prompt any CLI agent with auto-injected context from your current position in the diff. Navigate to a hunk, press `a`, ask a question — the agent sees exactly what you're looking at.

```
┌─ Files ──┬─ Diff ──────────────┬─ AI Panel ──────────────────┐
│          │                     │ [Review] [Agent]             │
│ auth.rs  │ @@ -45,8 +45,12    │                              │
│ routes.  │ - old_code()        │ ┌ context ─────────────────┐ │
│ tests/   │ + new_code()        │ │ auth.rs › hunk#2 › L45   │ │
│          │ + validate()        │ │ 🔴 Token expiry not set  │ │
│          │                     │ └──────────────────────────┘ │
│          │                     │                              │
│          │                     │ you: Why was this changed?   │
│          │                     │                              │
│          │                     │ agent: The JWT handling was  │
│          │                     │ refactored because the old   │
│          │                     │ opaque token approach had    │
│          │                     │ no expiry mechanism...       │
│          │                     │                              │
│          │                     │ you: Is this safe?           │
│          │                     │                              │
│          │                     │ agent: ██ (streaming...)     │
│          │                     ├──────────────────────────────┤
│          │                     │ > _                          │
└──────────┴─────────────────────┴──────────────────────────────┘
```

---

## Architecture

```
User navigates diff
       │
       ▼
AgentContext updated         ┌──────────────────┐
(file, hunk, line, finding)  │ .er-agent.toml   │
       │                     │ (config)         │
       ▼                     └────────┬─────────┘
User types prompt ──► er builds command from config
       │                     │
       ▼                     ▼
er writes context ──► spawns child process (claude/aider/custom)
to temp JSON file           │
                            ▼
                    stdout piped back
                    non-blocking reads on tick
                            │
                            ▼
                    rendered in Agent tab
                    (streaming, then finalized)
```

---

## Step 1: PanelTab enum + tab switching

**Files:** `src/ai/review.rs`, `src/ui/ai_panel.rs`

Add panel tab state:

```rust
// src/ai/review.rs

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelTab {
    Review,
    Agent,
}
```

Add to `AiState`:

```rust
pub panel_tab: PanelTab,  // default: PanelTab::Review
```

Render a tab bar at the top of the AI Side Panel column:

```rust
// src/ui/ai_panel.rs

fn render_panel_tab_bar(area: Rect, buf: &mut Buffer, active: PanelTab) {
    // Layout: single row at top of panel
    // [Review]  [Agent]
    // Active tab: bold, bright fg, underline
    // Inactive tab: dim fg
    // Separator line below
}
```

Split the existing `render_ai_panel()`:
- First row: tab bar (1 line)
- Remaining area: delegate to `render_review_content()` or `render_agent_content()` based on `panel_tab`

The current side panel rendering becomes `render_review_content()` — no logic changes, just extracted into its own function.

**Keybind:** In SidePanel ViewMode, `Tab` toggles `panel_tab` between Review and Agent.

---

## Step 2: AgentState + data structures

**New file:** `src/ai/agent.rs`

```rust
use std::process::Child;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MessageRole {
    User,
    Agent,
    System,
}

#[derive(Debug, Clone)]
pub struct AgentMessage {
    pub role: MessageRole,
    pub text: String,
    pub timestamp: String,
}

/// Context snapshot attached to each prompt
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AgentContext {
    pub file: Option<String>,
    pub hunk_index: Option<usize>,
    pub hunk_header: Option<String>,
    pub hunk_diff: Option<String>,
    pub line_number: Option<usize>,
    pub line_content: Option<String>,
    pub finding_title: Option<String>,
    pub finding_severity: Option<String>,
    pub finding_description: Option<String>,
    pub finding_suggestion: Option<String>,
    pub base_branch: String,
    pub head_branch: String,
}

pub struct AgentState {
    /// Conversation history
    pub messages: Vec<AgentMessage>,

    /// Current text in the prompt input
    pub input: String,

    /// Scroll offset in conversation view
    pub scroll: u16,

    /// Running child process (None = idle)
    pub child: Option<Child>,

    /// Whether agent is currently producing output
    pub is_running: bool,

    /// Buffer for streaming output (not yet committed to messages)
    pub partial_response: String,

    /// Current context (updates as user navigates)
    pub context: AgentContext,
}

impl AgentState {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            scroll: 0,
            child: None,
            is_running: false,
            partial_response: String::new(),
            context: AgentContext::default(),
        }
    }
}
```

Add `pub agent: AgentState` to `AiState` (in `review.rs`), initialized in `AiState::default()` / constructor.

Update `reload_ai_state()` in `state.rs` to preserve `agent` across reloads (same pattern as `review_focus` and `review_cursor`).

---

## Step 3: Context auto-update on navigation

**File:** `src/app/state.rs`

Add a method that rebuilds `AgentContext` from current navigation state:

```rust
impl TabState {
    pub fn update_agent_context(&mut self) {
        let ctx = &mut self.ai.agent.context;

        // Always set branch info
        ctx.base_branch = self.base_branch.clone();
        ctx.head_branch = self.current_branch.clone();

        // File
        let file = match self.files.get(self.selected_file) {
            Some(f) => f,
            None => {
                *ctx = AgentContext {
                    base_branch: ctx.base_branch.clone(),
                    head_branch: ctx.head_branch.clone(),
                    ..Default::default()
                };
                return;
            }
        };
        ctx.file = Some(file.path.clone());

        // Hunk
        if let Some(hunk) = file.hunks.get(self.current_hunk) {
            ctx.hunk_index = Some(self.current_hunk);
            ctx.hunk_header = Some(hunk.header.clone());
            ctx.hunk_diff = Some(
                hunk.lines.iter()
                    .map(|l| l.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            );

            // Line (if line-level navigation is active)
            if let Some(line_idx) = self.current_line {
                if let Some(line) = hunk.lines.get(line_idx) {
                    ctx.line_number = line.new_num;
                    ctx.line_content = Some(line.content.clone());
                } else {
                    ctx.line_number = None;
                    ctx.line_content = None;
                }
            } else {
                ctx.line_number = None;
                ctx.line_content = None;
            }
        } else {
            ctx.hunk_index = None;
            ctx.hunk_header = None;
            ctx.hunk_diff = None;
            ctx.line_number = None;
            ctx.line_content = None;
        }

        // Finding (if AI review data exists for this hunk)
        let finding = self.ai.review.as_ref()
            .and_then(|r| r.files.get(&file.path))
            .and_then(|f| f.findings.iter()
                .find(|f| Some(f.hunk_index) == ctx.hunk_index));

        if let Some(f) = finding {
            ctx.finding_title = Some(f.title.clone());
            ctx.finding_severity = Some(f.severity.clone());
            ctx.finding_description = Some(f.description.clone());
            ctx.finding_suggestion = f.suggestion.clone();
        } else {
            ctx.finding_title = None;
            ctx.finding_severity = None;
            ctx.finding_description = None;
            ctx.finding_suggestion = None;
        }
    }
}
```

Call `update_agent_context()` after any navigation that changes file/hunk/line:
- `select_file()`, `next_file()`, `prev_file()`
- `next_hunk()`, `prev_hunk()`
- `next_line()`, `prev_line()`

Lightweight — just copies a few strings. No I/O.

---

## Step 4: Agent configuration

**File:** `src/ai/agent.rs` (add config parsing)

Config file search order:
1. `{repo_root}/.er-agent.toml`
2. `~/.config/er/agent.toml`
3. Built-in defaults

```toml
# .er-agent.toml

[agent]
command = "claude"
args = ["--print", "-p", "{prompt}"]

# Placeholders:
#   {prompt}        — user's typed message
#   {context_file}  — path to temp JSON with full context
#   {file}          — current file path
#   {hunk}          — current hunk diff text
#   {line}          — current line content
```

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    pub agent: AgentConfigInner,
}

#[derive(Debug, Deserialize)]
pub struct AgentConfigInner {
    pub command: String,
    pub args: Vec<String>,
}

impl Default for AgentConfigInner {
    fn default() -> Self {
        Self {
            command: "claude".to_string(),
            args: vec![
                "--print".to_string(),
                "-p".to_string(),
                "{prompt}".to_string(),
            ],
        }
    }
}

pub fn load_agent_config(repo_root: &str) -> AgentConfigInner {
    // Try repo-local, then global, then default
    let local = format!("{repo_root}/.er-agent.toml");
    let global = dirs::config_dir()
        .map(|d| d.join("er/agent.toml").to_string_lossy().to_string());

    for path in [Some(local), global].into_iter().flatten() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = toml::from_str::<AgentConfig>(&content) {
                return config.agent;
            }
        }
    }

    AgentConfigInner::default()
}

pub fn resolve_command(
    config: &AgentConfigInner,
    prompt: &str,
    context_file: &str,
    context: &AgentContext,
) -> (String, Vec<String>) {
    let cmd = config.command.clone();
    let args = config.args.iter().map(|a| {
        a.replace("{prompt}", prompt)
         .replace("{context_file}", context_file)
         .replace("{file}", context.file.as_deref().unwrap_or(""))
         .replace("{hunk}", context.hunk_diff.as_deref().unwrap_or(""))
         .replace("{line}", context.line_content.as_deref().unwrap_or(""))
    }).collect();
    (cmd, args)
}
```

Add `toml` to Cargo.toml dependencies:
```toml
toml = "0.8"
dirs = "5"
```

---

## Step 5: Process spawning + streaming output

**File:** `src/app/state.rs` (or `src/ai/agent.rs`)

### Spawning

```rust
use std::process::{Command, Stdio};
use std::os::unix::io::AsRawFd;

impl TabState {
    pub fn spawn_agent(&mut self, config: &AgentConfigInner) -> Result<()> {
        let prompt = self.ai.agent.input.trim().to_string();
        if prompt.is_empty() { return Ok(()); }

        // Write context to temp file
        let ctx_path = format!("{}/.er-agent-context.json", self.repo_root);
        let ctx_json = serde_json::to_string_pretty(&self.ai.agent.context)?;
        std::fs::write(&ctx_path, &ctx_json)?;

        // Build full prompt with context preamble
        let full_prompt = build_full_prompt(&prompt, &self.ai.agent.context);

        // Resolve command + args from config
        let (cmd, args) = resolve_command(config, &full_prompt, &ctx_path, &self.ai.agent.context);

        // Spawn
        let mut child = Command::new(&cmd)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(&self.repo_root)
            .spawn()
            .with_context(|| format!("Failed to run agent: {cmd}"))?;

        // Set stdout to non-blocking
        if let Some(ref stdout) = child.stdout {
            set_nonblocking(stdout.as_raw_fd())?;
        }

        // Record in state
        self.ai.agent.messages.push(AgentMessage {
            role: MessageRole::User,
            text: prompt.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
        self.ai.agent.child = Some(child);
        self.ai.agent.is_running = true;
        self.ai.agent.partial_response.clear();
        self.ai.agent.input.clear();

        Ok(())
    }
}

fn build_full_prompt(user_prompt: &str, ctx: &AgentContext) -> String {
    let mut parts = Vec::new();

    if let Some(ref file) = ctx.file {
        let mut loc = format!("File: {file}");
        if let Some(idx) = ctx.hunk_index {
            loc.push_str(&format!(", hunk #{idx}"));
        }
        if let Some(ln) = ctx.line_number {
            loc.push_str(&format!(", line {ln}"));
        }
        parts.push(loc);
    }

    if let Some(ref diff) = ctx.hunk_diff {
        parts.push(format!("Diff:\n{diff}"));
    }

    if let Some(ref title) = ctx.finding_title {
        let sev = ctx.finding_severity.as_deref().unwrap_or("?");
        parts.push(format!("Finding: [{sev}] {title}"));
    }

    if parts.is_empty() {
        user_prompt.to_string()
    } else {
        format!("{}\n\nQuestion: {}", parts.join("\n"), user_prompt)
    }
}

fn set_nonblocking(fd: i32) -> Result<()> {
    use libc::{fcntl, F_GETFL, F_SETFL, O_NONBLOCK};
    unsafe {
        let flags = fcntl(fd, F_GETFL);
        fcntl(fd, F_SETFL, flags | O_NONBLOCK);
    }
    Ok(())
}
```

### Polling (in event loop)

Add to `run_app()` in `main.rs`, called every tick:

```rust
fn poll_agent(app: &mut App) {
    use std::io::Read;

    let agent = &mut app.tab_mut().ai.agent;
    if !agent.is_running { return; }

    let child = match agent.child.as_mut() {
        Some(c) => c,
        None => return,
    };

    // Try reading stdout
    if let Some(ref mut stdout) = child.stdout {
        let mut buf = [0u8; 4096];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) => {
                    // EOF — process done
                    finalize_agent_response(agent);
                    return;
                }
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]);
                    agent.partial_response.push_str(&chunk);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    break; // No more data right now
                }
                Err(_) => {
                    finalize_agent_response(agent);
                    return;
                }
            }
        }
    }

    // Check if process exited
    if let Ok(Some(_status)) = child.try_wait() {
        // Drain remaining stdout
        if let Some(ref mut stdout) = child.stdout {
            let mut remaining = String::new();
            let _ = stdout.read_to_string(&mut remaining);
            agent.partial_response.push_str(&remaining);
        }
        finalize_agent_response(agent);
    }
}

fn finalize_agent_response(agent: &mut AgentState) {
    let text = std::mem::take(&mut agent.partial_response)
        .trim()
        .to_string();

    if !text.is_empty() {
        agent.messages.push(AgentMessage {
            role: MessageRole::Agent,
            text,
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    }

    agent.is_running = false;
    agent.child = None;
}
```

Place `poll_agent(app)` in the main event loop, runs every tick (same place as `check_ai_files_changed`).

---

## Step 6: Agent panel rendering

**File:** `src/ui/ai_panel.rs`

### Layout

```
┌─ [Review] [Agent] ──────────┐  ← tab bar (1 line)
│ ┌ context ─────────────────┐ │  ← context badge (2-3 lines)
│ │ auth.rs › hunk#2 › L45   │ │
│ │ 🔴 Token expiry not set  │ │
│ └──────────────────────────┘ │
│                              │  ← conversation area (fills remaining)
│ you: Why was this changed?   │
│                              │
│ agent: The JWT handling was  │
│ refactored because...        │
│                              │
│ you: Is this safe?           │
│                              │
│ agent: ██                    │  ← streaming partial response
├──────────────────────────────┤
│ > _                          │  ← prompt input (2 lines)
└──────────────────────────────┘
```

### Render functions

```rust
fn render_agent_content(area: Rect, buf: &mut Buffer, app: &App) {
    let agent = &app.tab().ai.agent;

    // Split area: context badge | conversation | prompt input
    let chunks = Layout::vertical([
        Constraint::Length(3),   // context badge
        Constraint::Min(4),     // conversation
        Constraint::Length(2),   // prompt input
    ]).split(area);

    render_context_badge(chunks[0], buf, &agent.context);
    render_conversation(chunks[1], buf, agent);
    render_prompt_input(chunks[2], buf, &agent.input, agent.is_running);
}

fn render_context_badge(area: Rect, buf: &mut Buffer, ctx: &AgentContext) {
    // Bordered box with compact breadcrumb:
    //   auth.rs › hunk#2 › L45
    //   🔴 Token expiry not enforced
    //
    // If no file selected: "Navigate to a file to set context"
    // Dim border, Rgb(60,60,80) background
}

fn render_conversation(area: Rect, buf: &mut Buffer, agent: &AgentState) {
    // Scrollable list of messages
    // Each message:
    //   "you:" prefix   — Color::Rgb(130, 170, 255) (blue-ish)
    //   "agent:" prefix — Color::Rgb(170, 220, 170) (green-ish)
    //   "system:" prefix — dim italic
    //
    // Message text wraps within the panel width
    // If is_running, append partial_response with "██" block cursor
    //
    // Auto-scroll to bottom when new content arrives
    // Manual scroll with j/k when not in input mode
}

fn render_prompt_input(area: Rect, buf: &mut Buffer, input: &str, is_running: bool) {
    // Top border line
    // If is_running: show "⏳ running..." instead of input
    // Otherwise: "> {input}_" with blinking cursor
    // If input is empty: dim placeholder "Ask about this code..."
}
```

### Message rendering detail

Messages support basic formatting:
- `**bold**` → Bold style
- `` `code` `` → Inline code with different bg
- ` ```block``` ` → Code block with border + syntax coloring (reuse existing highlighter)
- Everything else → plain wrap

This doesn't need full markdown — just these three patterns cover 90% of agent output.

---

## Step 7: Keybindings

**File:** `src/main.rs`

### New InputMode variant

```rust
pub enum InputMode {
    Normal,
    Search,
    Comment,
    AgentPrompt,  // NEW
}
```

### Global keybinds (in `handle_normal_input`)

| Key | Condition | Action |
|-----|-----------|--------|
| `a` | Any view mode | Switch to SidePanel + Agent tab + AgentPrompt input mode |

### SidePanel-specific keybinds

| Key | Condition | Action |
|-----|-----------|--------|
| `Tab` | SidePanel mode, Normal input | Toggle `panel_tab` between Review/Agent |

### AgentPrompt input mode

```rust
fn handle_agent_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Enter => {
            // Submit prompt, spawn agent
            let config = load_agent_config(&app.tab().repo_root);
            app.tab_mut().spawn_agent(&config)?;
            // Stay in AgentPrompt mode (input clears, watch streaming output)
        }
        KeyCode::Esc => {
            // Exit input, back to Normal mode
            // Stay on Agent tab, can scroll conversation
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char(c) => {
            app.tab_mut().ai.agent.input.push(c);
        }
        KeyCode::Backspace => {
            app.tab_mut().ai.agent.input.pop();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Kill running agent
            if let Some(ref mut child) = app.tab_mut().ai.agent.child {
                let _ = child.kill();
            }
            app.tab_mut().ai.agent.is_running = false;
            app.tab_mut().ai.agent.child = None;
            app.tab_mut().ai.agent.messages.push(AgentMessage {
                role: MessageRole::System,
                text: "— cancelled —".to_string(),
                timestamp: now_iso8601(),
            });
        }
        _ => {}
    }
    Ok(())
}
```

### Agent tab in Normal input mode (viewing conversation)

| Key | Action |
|-----|--------|
| `j/k` | Scroll conversation up/down |
| `a` or `i` or `Enter` | Focus prompt input (switch to AgentPrompt mode) |
| `Ctrl+L` | Clear conversation history |
| `Tab` | Switch to Review tab |
| `v/V` | Cycle view mode (leave panel) |
| `Esc` | Back to Default view mode |

---

## Step 8: Quick prompts (slash commands)

When the user types a `/` prefix in the prompt, intercept and expand:

```rust
fn expand_slash_command(input: &str) -> Option<String> {
    match input.trim() {
        "/explain" => Some("Explain this change. Why was it made and what does it do?".into()),
        "/safe" | "/security" => Some("Review this change for security issues. Is it safe?".into()),
        "/test" => Some("Write a unit test for this change.".into()),
        "/bug" | "/issues" => Some("What could go wrong with this change? Any edge cases?".into()),
        "/suggest" => Some("Suggest improvements to this code.".into()),
        "/review" => None, // Special: triggers er-review skill (generates er-* files)
        _ => None,
    }
}
```

`/review` is special — instead of prompting the agent conversationally, it runs the configured review command (equivalent to running `/er-review` in Claude Code). The agent config could have a separate `review_command` for this:

```toml
[agent]
command = "claude"
args = ["--print", "-p", "{prompt}"]

[agent.review]
command = "claude"
args = ["-p", "/er-review"]
```

Show available commands when user types `/` with nothing after it — render a small popup or inline hint listing the available slash commands.

---

## Step 9: Conversation persistence (optional)

**File:** `.er-agent-history.json` in repo root

```json
{
  "diff_hash": "abc123...",
  "messages": [
    {
      "role": "user",
      "text": "Why was this changed?",
      "timestamp": "2026-02-24T10:45:00Z",
      "context_file": "src/auth.rs",
      "context_hunk": 2
    },
    {
      "role": "agent",
      "text": "The JWT handling was refactored...",
      "timestamp": "2026-02-24T10:45:15Z"
    }
  ]
}
```

Load on startup if `diff_hash` matches current diff. Discard if stale.

Add to `.gitignore`:
```
.er-agent-context.json
.er-agent-history.json
```

---

## New dependencies

```toml
# Cargo.toml additions
toml = "0.8"       # config file parsing
dirs = "5"         # ~/.config/er/ path resolution
chrono = "0.4"     # timestamps (or use std::time if preferred)
libc = "0.2"       # set_nonblocking on unix fd
```

---

## Implementation order

1. **PanelTab enum + tab bar UI** — wiring only, Review tab = existing content
2. **AgentState struct + add to AiState** — data structures, preserve across reloads
3. **Agent panel rendering** — context badge, conversation view, prompt input
4. **InputMode::AgentPrompt keybindings** — typing, enter to submit, esc to exit
5. **Config loading** — .er-agent.toml parsing with defaults
6. **Context auto-update** — hook into navigation methods
7. **Process spawning + non-blocking stdout** — the core integration
8. **Streaming display** — partial_response rendering with block cursor
9. **Slash commands** — expand shortcuts, `/review` triggers er-review
10. **Persistence** — save/load conversation history (optional)
