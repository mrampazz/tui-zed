# TUI-Zed: Implementation Plans

Concrete, file-level actionable steps for each phase of the TUI-Zed project.
Each task is a single PR-sized unit of work. Tasks within a phase can often
be parallelized.

---

## Phase 0: Infrastructure

### Task 0.1 — Create the `text_style` crate

**Goal**: Extract pure data types from `gpui` into a standalone crate so that
`syntax_theme`, `language`, and `editor` can depend on style types without
pulling in GPUI.

**Steps**:

1. Create `crates/text_style/Cargo.toml`:
   ```toml
   [package]
   name = "text_style"
   edition = "2024"
   publish = false

   [lib]
   path = "text_style.rs"

   [dependencies]
   serde = { version = "1", features = ["derive"] }
   schemars = { workspace = true }
   ```

2. Create `crates/text_style/text_style.rs` — copy these structs from gpui
   **verbatim** (preserving derive macros and impls):

   | Type | Source file | Source line |
   |------|-----------|-------------|
   | `Pixels` | `crates/gpui/src/geometry.rs` | 2677 |
   | `Hsla` | `crates/gpui/src/color.rs` | 334 |
   | `Rgba` | `crates/gpui/src/color.rs` | 39 |
   | `FontWeight` | `crates/gpui/src/text_system.rs` | 887 |
   | `FontStyle` | `crates/gpui/src/text_system.rs` | 969 |
   | `UnderlineStyle` | `crates/gpui/src/style.rs` | 824 |
   | `StrikethroughStyle` | `crates/gpui/src/style.rs` | 839 |
   | `HighlightStyle` | `crates/gpui/src/style.rs` | 576 |

   Key changes from gpui originals:
   - Make `Pixels` inner field `pub` (it's `pub(crate)` in gpui at line 2677)
   - Remove any `Refineable` derive macros (those are gpui-specific)
   - Remove any methods that reference gpui runtime types
   - Keep `Serialize`, `Deserialize`, `Clone`, `Copy`, `Debug`, `Default`,
     `PartialEq` derives
   - Keep the color conversion impls (`Hsla → Rgba`, `Rgba → Hsla`)
   - Keep `HighlightStyle::highlight()` merge method (it's pure logic)

3. Add to workspace `Cargo.toml`:
   - Add `"crates/text_style"` to `[workspace.members]`
   - Add `text_style = { path = "crates/text_style" }` to
     `[workspace.dependencies]`

4. Update `crates/gpui/src/style.rs` — re-export from `text_style`:
   ```rust
   pub use text_style::{HighlightStyle, UnderlineStyle, StrikethroughStyle};
   ```
   Remove the original struct definitions. This keeps all existing gpui
   consumers working without changes.

5. Do the same for `color.rs` (`Hsla`, `Rgba`), `geometry.rs` (`Pixels`),
   `text_system.rs` (`FontWeight`, `FontStyle`).

6. Add `text_style.workspace = true` to `crates/gpui/Cargo.toml` dependencies.

7. Verify: `cargo check -p gpui` passes. `cargo check -p text_style` passes
   with zero gpui dependency.

**Validation**: `cargo tree -p text_style` shows no `gpui` anywhere.

---

### Task 0.2 — Create the `async_executor` crate

**Goal**: Define an executor trait that both GPUI and tokio can implement,
used by decoupled crates for spawning tasks and timers.

**Steps**:

1. Create `crates/async_executor/Cargo.toml`:
   ```toml
   [package]
   name = "async_executor"
   edition = "2024"
   publish = false

   [lib]
   path = "async_executor.rs"

   [dependencies]
   futures.workspace = true
   anyhow.workspace = true
   ```

2. Create `crates/async_executor/async_executor.rs`:
   ```rust
   use std::future::Future;
   use std::pin::Pin;
   use std::time::Duration;

   pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

   /// A handle to a spawned task. Dropping it should NOT cancel the task
   /// (unlike gpui::Task). Use cancel() explicitly.
   pub trait TaskHandle<T>: Future<Output = T> + Send + Unpin {
       fn cancel(&self);
       fn detach(self);
   }

   /// Abstract executor for spawning async work.
   pub trait Executor: Send + Sync + 'static {
       fn spawn<F>(&self, future: F) -> BoxFuture<()>
       where
           F: Future<Output = ()> + Send + 'static;

       fn spawn_labeled<F>(&self, label: &str, future: F) -> BoxFuture<()>
       where
           F: Future<Output = ()> + Send + 'static
       {
           let _ = label;
           self.spawn(future)
       }

       fn timer(&self, duration: Duration) -> BoxFuture<()>;
   }
   ```

3. Add to workspace `Cargo.toml`:
   - Add `"crates/async_executor"` to `[workspace.members]`
   - Add `async_executor = { path = "crates/async_executor" }` to
     `[workspace.dependencies]`

4. Create `crates/async_executor/tokio_impl.rs` (behind `tokio` feature):
   ```rust
   use crate::Executor;
   use tokio::runtime::Handle;

   #[derive(Clone)]
   pub struct TokioExecutor {
       handle: Handle,
   }

   impl TokioExecutor {
       pub fn new(handle: Handle) -> Self {
           Self { handle }
       }

       pub fn current() -> Self {
           Self { handle: Handle::current() }
       }
   }

   impl Executor for TokioExecutor {
       fn spawn<F>(&self, future: F) -> BoxFuture<()>
       where F: Future<Output = ()> + Send + 'static {
           let handle = self.handle.spawn(future);
           Box::pin(async move { handle.await.ok(); })
       }

       fn timer(&self, duration: Duration) -> BoxFuture<()> {
           Box::pin(tokio::time::sleep(duration))
       }
   }
   ```

5. Create `crates/async_executor/gpui_impl.rs` (behind `gpui` feature):
   ```rust
   use crate::Executor;
   use gpui::BackgroundExecutor;

   impl Executor for BackgroundExecutor {
       fn spawn<F>(&self, future: F) -> BoxFuture<()>
       where F: Future<Output = ()> + Send + 'static {
           let task = self.spawn(future);
           Box::pin(async move { task.await })
       }

       fn timer(&self, duration: Duration) -> BoxFuture<()> {
           let timer = self.timer(duration);
           Box::pin(async move { timer.await })
       }
   }
   ```

**Validation**: `cargo check -p async_executor` passes with no gpui
dependency when built without the `gpui` feature.

---

### Task 0.3 — Create the `tui_zed` binary crate

**Goal**: Skeleton binary with tokio event loop, crossterm raw mode, and
ratatui frame rendering.

**Steps**:

1. Create `crates/tui_zed/Cargo.toml` (see TUI_PLAN.md Phase 0.3 for full
   deps).

2. Create `crates/tui_zed/main.rs`:
   ```rust
   #[tokio::main]
   async fn main() -> anyhow::Result<()> {
       // 1. Parse CLI args (file path)
       // 2. Enter crossterm raw mode
       // 3. Create ratatui terminal
       // 4. Run event loop
       // 5. Restore terminal on exit
   }
   ```

3. Create `crates/tui_zed/tui_zed.rs` (lib root):
   ```rust
   pub mod app;
   pub mod event;
   pub mod ui;
   ```

4. Create `crates/tui_zed/app.rs` — the main event loop:
   ```rust
   pub struct App {
       terminal: Terminal<CrosstermBackend<Stdout>>,
       should_quit: bool,
   }

   impl App {
       pub async fn run(&mut self) -> Result<()> {
           loop {
               self.terminal.draw(|frame| self.render(frame))?;

               tokio::select! {
                   event = crossterm::event::EventStream::new().next() => {
                       self.handle_input(event?)?;
                   }
                   // Future: LSP events, fs events, timers
               }

               if self.should_quit { break; }
           }
           Ok(())
       }
   }
   ```

5. Add `"crates/tui_zed"` to workspace `Cargo.toml` members.

6. Verify: `cargo run -p tui_zed` opens a blank terminal window, handles
   Ctrl+C to quit, restores terminal properly on exit.

**Validation**: Binary starts, shows empty ratatui frame, exits cleanly.

---

## Phase 1: Decouple the LSP Crate

### Task 1.1 — Add feature flags to `lsp` Cargo.toml

**File**: `crates/lsp/Cargo.toml`

**Steps**:

1. At line 24, change `gpui.workspace = true` to:
   ```toml
   gpui = { workspace = true, optional = true }
   ```

2. Add new dependencies:
   ```toml
   async_executor = { workspace = true }
   gpui_shared_string.workspace = true
   tokio = { version = "1", features = ["process", "sync", "time"], optional = true }
   ```

3. Add feature flags:
   ```toml
   [features]
   default = ["gpui-backend"]
   gpui-backend = ["dep:gpui"]
   tui-backend = ["dep:tokio"]
   ```

4. Verify: `cargo check -p lsp` still passes (uses default `gpui-backend`).

---

### Task 1.2 — Replace `SharedString` imports

**File**: `crates/lsp/src/lsp.rs`

**Steps**:

1. At line 15, change:
   ```rust
   use gpui::{App, AppContext as _, AsyncApp, BackgroundExecutor, SharedString, Task};
   ```
   to:
   ```rust
   use gpui_shared_string::SharedString;

   #[cfg(feature = "gpui-backend")]
   use gpui::{App, AppContext as _, AsyncApp, BackgroundExecutor, Task};
   ```

2. Find all other uses of `SharedString` in the file (lines 105, 155, 177,
   187, 1075, 1309, 1314, 1328, 1330, 1332). These all work with
   `gpui_shared_string::SharedString` — no changes needed.

**Validation**: `cargo check -p lsp` passes.

---

### Task 1.3 — Abstract the executor

**File**: `crates/lsp/src/lsp.rs`

**Steps**:

1. Add import:
   ```rust
   use async_executor::Executor;
   ```

2. At line 120, change the `LanguageServer` field:
   ```rust
   // Before
   executor: BackgroundExecutor,
   // After
   executor: Arc<dyn Executor>,
   ```

3. At the `LanguageServer::new` signature (line 397-406), replace
   `cx: &mut AsyncApp` with `executor: Arc<dyn Executor>`:
   - Under `gpui-backend`, provide a helper that extracts the executor from
     `AsyncApp` and calls through
   - Under `tui-backend`, take the executor directly

4. At `new_internal` (line 465-484), same change: replace `cx: &mut AsyncApp`
   with `executor: Arc<dyn Executor>`.

5. Replace all `cx.background_spawn(...)` calls with
   `self.executor.spawn(...)`:
   - Line 495 (`cx.spawn(...)` for `handle_incoming_messages`) — this is a
     foreground spawn, needs special handling (see Task 1.4)
   - Line 539 (`cx.background_spawn(...)` for stderr)
   - Line 546 (`cx.background_spawn(...)` for io join)
   - Line 550 (`cx.background_spawn(...)` for outgoing messages)
   - Line 567 (`cx.background_spawn(...)` for notification forwarding)

6. Replace `executor.timer(duration)` calls (lines 1109, 1572) with
   `self.executor.timer(duration)`.

7. At line 600 (`cx.background_executor().clone()`), replace with the passed
   `executor.clone()`.

8. In `Drop for LanguageServer` (line 1739), replace
   `self.executor.spawn(shutdown).detach()` with
   `self.executor.spawn(shutdown)` (the BoxFuture return is sufficient to
   fire-and-forget).

**Validation**: `cargo check -p lsp --features gpui-backend` passes.

---

### Task 1.4 — Feature-gate `AsyncApp` in notification handlers

**File**: `crates/lsp/src/lsp.rs`

This is the trickiest part. The `NotificationHandler` type (line 63) takes
`&mut AsyncApp` so handlers can access GPUI app state. Under `tui-backend`,
handlers must close over their own state.

**Steps**:

1. Feature-gate the type aliases (lines 63-65):
   ```rust
   #[cfg(feature = "gpui-backend")]
   type NotificationHandler =
       Box<dyn Send + FnMut(Option<RequestId>, Value, &mut AsyncApp)>;
   #[cfg(feature = "tui-backend")]
   type NotificationHandler =
       Box<dyn Send + FnMut(Option<RequestId>, Value)>;

   // Same pattern for ResponseHandler and PendingRespondTasks,
   // replacing Task<()> with BoxFuture<()>
   ```

2. Feature-gate `handle_incoming_messages` (line 614-624):
   - `gpui-backend`: keep `cx: &mut AsyncApp` parameter, pass to handlers
   - `tui-backend`: remove `cx` parameter, handlers called without it

3. Feature-gate `on_notification` (line 1149-1152):
   ```rust
   #[cfg(feature = "gpui-backend")]
   pub fn on_notification<T, F>(&self, f: F) -> Subscription
   where
       T: notification::Notification,
       F: 'static + Send + FnMut(T::Params, &mut AsyncApp),
   { /* existing impl */ }

   #[cfg(feature = "tui-backend")]
   pub fn on_notification<T, F>(&self, f: F) -> Subscription
   where
       T: notification::Notification,
       F: 'static + Send + FnMut(T::Params),
   { /* same impl but handler doesn't receive cx */ }
   ```

4. Same pattern for `on_request` (line 1161-1166),
   `on_custom_notification` (line 1201), `on_custom_request` (line 1225).

5. Feature-gate `default_initialize_params` (line 746-751):
   ```rust
   #[cfg(feature = "gpui-backend")]
   pub fn default_initialize_params(&self, ..., cx: &App) -> InitializeParams {
       // reads release_channel::ReleaseChannel::try_global(cx)
       // reads release_channel::AppVersion::global(cx)
   }

   #[cfg(feature = "tui-backend")]
   pub fn default_initialize_params(
       &self, ..., client_info: Option<ClientInfo>,
   ) -> InitializeParams {
       // uses the passed client_info directly
   }
   ```

6. Feature-gate `initialize` (line 1055-1061):
   - `gpui-backend`: takes `cx: &App`, returns `Task<Result<Arc<Self>>>`
   - `tui-backend`: no `cx`, returns `BoxFuture<Result<Arc<Self>>>`

**Validation**: `cargo check -p lsp --features tui-backend --no-default-features`
passes with zero gpui dependency.

---

### Task 1.5 — Update `input_handler.rs`

**File**: `crates/lsp/src/input_handler.rs`

**Steps**:

1. At line 31, change `loop_handle: Task<Result<()>>` to:
   ```rust
   #[cfg(feature = "gpui-backend")]
   loop_handle: Task<Result<()>>,
   #[cfg(feature = "tui-backend")]
   loop_handle: BoxFuture<Result<()>>,
   ```

2. At `LspStdoutHandler::new` (line 53-58), replace
   `cx: BackgroundExecutor` with `executor: Arc<dyn Executor>`.

3. Replace `cx.spawn(Self::handler(...))` (line 63) with
   `executor.spawn(...)`.

**Validation**: Same as Task 1.4 — full `tui-backend` check passes.

---

### Task 1.6 — Write integration test under `tui-backend`

**Goal**: Prove the decoupled LSP crate works with tokio.

**Steps**:

1. Create `crates/tui_zed/tests/lsp_integration.rs`:
   ```rust
   use async_executor::tokio_impl::TokioExecutor;
   use lsp::LanguageServer;

   #[tokio::test]
   async fn test_lsp_lifecycle() {
       // 1. Create a TokioExecutor
       // 2. Start a mock LSP server (or use a real one like rust-analyzer)
       // 3. Send initialize, verify capabilities
       // 4. Send didOpen, verify no errors
       // 5. Shutdown
   }
   ```

**Validation**: Test passes under `cargo test -p tui_zed`.

---

## Phase 2: Decouple Syntax Highlighting

### Task 2.1 — Decouple `syntax_theme`

**File**: `crates/syntax_theme/Cargo.toml` and
`crates/syntax_theme/src/syntax_theme.rs`

**Steps**:

1. In `Cargo.toml`, replace `gpui.workspace = true` with:
   ```toml
   text_style.workspace = true
   gpui = { workspace = true, optional = true }

   [features]
   default = ["gpui-backend"]
   gpui-backend = ["dep:gpui"]
   ```

2. In `syntax_theme.rs` at line 8, change:
   ```rust
   // Before
   use gpui::HighlightStyle;
   // After
   use text_style::HighlightStyle;
   ```

3. At line 10 (test-only `Hsla` import):
   ```rust
   // Before
   #[cfg(any(test, feature = "test-support"))]
   use gpui::Hsla;
   // After
   use text_style::Hsla;  // no longer test-only, it's lightweight
   ```

4. For the `bundled-themes` feature code (~line 139+) that uses `gpui::rgb`,
   `gpui::Rgba`, `gpui::Hsla`, `gpui::FontWeight`, `gpui::FontStyle`:
   replace all with `text_style::*` imports.

5. For test code (~line 244+) that uses `gpui::red()`, `gpui::green()`, etc.:
   either feature-gate behind `gpui-backend` or define local test helpers.

**Validation**: `cargo check -p syntax_theme --no-default-features` passes
with zero gpui dependency.

---

### Task 2.2 — Decouple `syntax_map.rs` in the `language` crate

**File**: `crates/language/src/syntax_map.rs`

**Steps**:

1. At line 9, change:
   ```rust
   // Before
   use gpui::SharedString;
   // After
   use gpui_shared_string::SharedString;
   ```

That's it. The entire 2220-line file is now GPUI-free. The `SharedString` was
the only coupling point (used in `ParseStepLanguage::name()` at line 223).

**Validation**: The file compiles. `SyntaxSnapshot::reparse()`,
`SyntaxSnapshot::captures()`, `SyntaxMapCaptures`, `SyntaxMapMatches` all
work without GPUI.

---

### Task 2.3 — Feature-flag `language` crate Cargo.toml

**File**: `crates/language/Cargo.toml`

**Steps**:

1. At line 41, change `gpui.workspace = true` to:
   ```toml
   gpui = { workspace = true, optional = true }
   ```

2. Add:
   ```toml
   async_executor.workspace = true
   text_style.workspace = true
   gpui_shared_string.workspace = true
   tokio = { version = "1", features = ["sync"], optional = true }
   ```

3. Add features:
   ```toml
   [features]
   default = ["gpui-backend"]
   gpui-backend = ["dep:gpui", "settings/gpui-backend", "theme/gpui-backend"]
   tui-backend = ["dep:tokio"]
   ```

4. Feature-gate the `settings`, `theme`, and `fs` dependencies (these pull in
   gpui). Under `tui-backend`, these are optional or replaced.

**Validation**: `cargo check -p language --features gpui-backend` passes
(existing behavior).

---

### Task 2.4 — Decouple `LanguageRegistry`

**File**: `crates/language/src/language_registry.rs`

**Steps**:

1. At line 19, feature-gate the import:
   ```rust
   #[cfg(feature = "gpui-backend")]
   use gpui::{App, BackgroundExecutor};
   use async_executor::Executor;
   ```

2. At line 41, change the field:
   ```rust
   // Before
   executor: BackgroundExecutor,
   // After
   executor: Arc<dyn Executor>,
   ```

3. At line 141, update the constructor:
   ```rust
   // Before
   pub fn new(executor: BackgroundExecutor) -> Self
   // After
   pub fn new(executor: Arc<dyn Executor>) -> Self
   ```

4. At line 664, feature-gate `language_for_file`:
   ```rust
   #[cfg(feature = "gpui-backend")]
   pub fn language_for_file(
       self: &Arc<Self>, file: &Arc<dyn File>,
       content: Option<&Rope>, cx: &App,
   ) -> ... { /* existing: reads settings via cx */ }

   #[cfg(feature = "tui-backend")]
   pub fn language_for_file_with_settings(
       self: &Arc<Self>, file: &Arc<dyn File>,
       content: Option<&Rope>,
       language_settings: &AllLanguageSettings,
   ) -> ... { /* same logic, settings passed directly */ }
   ```

**Validation**: Compiles under both features.

---

### Task 2.5 — Extract `Buffer::reparse` pure logic

**File**: `crates/language/src/buffer.rs`

**Steps**:

1. Create a new public free function (place before the `impl Buffer` block):
   ```rust
   /// Pure reparse — no GPUI dependency. Runs synchronously.
   /// Call this on a background thread.
   pub fn reparse_syntax(
       text: &text::BufferSnapshot,
       syntax: SyntaxSnapshot,
       registry: Option<&Arc<LanguageRegistry>>,
       language: Option<&Arc<Language>>,
   ) -> SyntaxSnapshot {
       let mut syntax = syntax;
       syntax.reparse(text, registry, language);
       syntax
   }
   ```

2. Refactor `Buffer::reparse()` (line 1836) to call `reparse_syntax()`
   internally on the background thread. The existing logic already does
   essentially this — it's just wrapped in `cx.background_spawn()`.

3. Feature-gate the GPUI scheduling wrapper:
   ```rust
   #[cfg(feature = "gpui-backend")]
   pub fn reparse(&mut self, cx: &mut Context<Self>, may_block: bool) {
       // existing spawn/emit/notify logic, now calls reparse_syntax()
   }
   ```

4. Under `tui-backend`, the TUI app calls `reparse_syntax()` directly on a
   tokio task and applies the result to the buffer manually.

**Validation**: `reparse_syntax()` compiles with no GPUI imports.

---

### Task 2.6 — Feature-gate `Buffer` entity wrapper

**File**: `crates/language/src/buffer.rs`

**Steps**:

1. Feature-gate the GPUI imports at lines 31-34:
   ```rust
   #[cfg(feature = "gpui-backend")]
   use gpui::{App, AppContext as _, Context, Entity, EventEmitter,
              HighlightStyle, SharedString, StyledText, Task, TextStyle};
   use text_style::HighlightStyle;
   use gpui_shared_string::SharedString;
   ```

2. Feature-gate `impl EventEmitter<BufferEvent> for Buffer {}` (line 3460).

3. Feature-gate the `_subscriptions: Vec<gpui::Subscription>` field and the
   Task fields (`reload_task`, `pending_autoindent`, `reparse`) behind
   `gpui-backend`.

4. Feature-gate methods that take `&mut Context<Self>` behind `gpui-backend`.

5. For `tui-backend`, provide alternative methods that take no context:
   ```rust
   #[cfg(feature = "tui-backend")]
   impl Buffer {
       pub fn set_text_no_cx(&mut self, text: impl Into<String>) { ... }
       pub fn edit_no_cx(&mut self, edits: impl IntoIterator<...>) { ... }
   }
   ```

**Validation**: `cargo check -p language --features tui-backend
--no-default-features` passes.

---

### Task 2.7 — Feature-gate `language_settings.rs`

**File**: `crates/language/src/language_settings.rs`

**Steps**:

1. At line 14, feature-gate:
   ```rust
   #[cfg(feature = "gpui-backend")]
   use gpui::{App, Modifiers};
   use gpui_shared_string::SharedString;
   ```

2. For `all_language_settings` (line 30): this reads from
   `SettingsStore` (GPUI global). Under `tui-backend`, provide:
   ```rust
   #[cfg(feature = "tui-backend")]
   pub fn all_language_settings_direct(
       settings: &AllLanguageSettings,
   ) -> &AllLanguageSettings {
       settings
   }
   ```

3. For `LanguageSettings::for_buffer` (line 282) and `settings_at`
   (buffer.rs:3987), provide `tui-backend` variants that take
   `&AllLanguageSettings` instead of `&App`.

**Validation**: Compiles under `tui-backend`.

---

### Task 2.8 — Create `TuiTheme` mapping layer

**File**: `crates/tui_zed/src/theme.rs` (new file)

**Steps**:

1. Create the theme bridge:
   ```rust
   use syntax_theme::SyntaxTheme;
   use language_core::HighlightId;
   use text_style::{Hsla, HighlightStyle};
   use ratatui::style::{Color, Modifier, Style};

   pub struct TuiTheme {
       styles: Vec<Style>,
   }

   impl TuiTheme {
       pub fn from_syntax_theme(theme: &SyntaxTheme) -> Self {
           let styles = theme.highlights()
               .iter()
               .map(|hs| hsla_highlight_to_ratatui(hs))
               .collect();
           Self { styles }
       }

       pub fn style_for_highlight(&self, id: HighlightId) -> Style {
           self.styles.get(id.0.get() as usize - 1)
               .copied()
               .unwrap_or_default()
       }
   }

   fn hsla_highlight_to_ratatui(hs: &HighlightStyle) -> Style {
       let mut style = Style::default();
       if let Some(color) = hs.color {
           style = style.fg(hsla_to_rgb_color(color));
       }
       if let Some(bg) = hs.background_color {
           style = style.bg(hsla_to_rgb_color(bg));
       }
       if let Some(weight) = hs.font_weight {
           if weight.0 >= 700.0 {
               style = style.add_modifier(Modifier::BOLD);
           }
       }
       if let Some(font_style) = hs.font_style {
           if font_style == text_style::FontStyle::Italic {
               style = style.add_modifier(Modifier::ITALIC);
           }
       }
       if hs.underline.is_some() {
           style = style.add_modifier(Modifier::UNDERLINED);
       }
       style
   }

   fn hsla_to_rgb_color(hsla: Hsla) -> Color {
       let rgba = hsla.to_rgba();
       Color::Rgb(
           (rgba.r * 255.0) as u8,
           (rgba.g * 255.0) as u8,
           (rgba.b * 255.0) as u8,
       )
   }
   ```

2. Add a default built-in theme (ANSI-16 compatible for basic terminals).

3. Add Zed theme JSON loader that reads from `assets/themes/` and converts.

**Validation**: Unit test that loads a Zed theme JSON and produces valid
ratatui `Style` values.

---

## Phase 3: Decouple the Editor Core

### Task 3.1 — Extract `EditorState` struct

**File**: `crates/editor/src/editor.rs` (new struct, keep in same file)

**Steps**:

1. Define `EditorState` above the existing `Editor` struct (~line 920).
   Move these field categories into it (see TUI_PLAN.md Phase 3.3 for the
   full field list):
   - `selections: SelectionsCollection`
   - `selection_history`, `columnar_selection_state`, `add_selections_state`,
     `select_next_state`, `select_prev_state`
   - `autoclose_regions`, `snippet_stack`, `select_syntax_node_history`
   - `cursor_shape`, `mode`, `read_only`, `input_enabled`
   - All `show_*` booleans
   - `inline_diagnostics`, `lsp_document_symbols`, `linked_edit_ranges`
   - `change_list`
   - All `bool` config flags

2. Update `Editor` to hold `pub state: EditorState`.

3. Update all `self.selections` references to `self.state.selections`, etc.
   Use find-and-replace. This is mechanical but touches many lines.

4. Keep `Editor` compiling under `gpui-backend` — this is a refactor, not a
   feature change.

**Validation**: `cargo check -p editor` passes. All existing editor tests
pass (`cargo test -p editor`).

---

### Task 3.2 — Extract pure movement functions (already done)

**File**: `crates/editor/src/movement.rs`

**Steps**: None needed! This file is already ~100% GPUI-free. All functions
take `&DisplaySnapshot` and return `DisplayPoint`. The only GPUI type is
`Pixels` (in `TextLayoutDetails`), which will come from `text_style` after
Phase 0.

Just verify:
1. `movement.rs` only imports `Pixels` and `WindowTextSystem` from gpui.
2. After Task 0.1 (`text_style` crate), change `Pixels` import to
   `text_style::Pixels`.
3. `WindowTextSystem` is only used in `TextLayoutDetails` — for TUI, we
   provide a `TerminalTextMeasurer` instead (Task 3.5).

**Validation**: No code changes needed for extraction; will work after
`text_style` swap.

---

### Task 3.3 — Extract pure selection functions

**Files**: `crates/editor/src/selection.rs`,
`crates/editor/src/selections_collection.rs`

**Steps**:

1. `selections_collection.rs` is already GPUI-free (only uses `Pixels`).
   No changes needed beyond the `text_style::Pixels` import swap.

2. In `selection.rs`, identify the pure inner lambdas in each method and
   extract them as free functions in a new module
   `crates/editor/src/selection_ops.rs`:

   ```rust
   /// Pure: compute line-expanded selections
   pub fn select_line_ranges(
       snapshot: &DisplaySnapshot,
       selections: &[Selection<Point>],
   ) -> Vec<Selection<Point>> { ... }

   /// Pure: split one selection into per-line selections
   pub fn split_selection_into_lines(
       snapshot: &MultiBufferSnapshot,
       selection: &Selection<Point>,
   ) -> Vec<Selection<Point>> { ... }

   /// Pure: find all matches in buffer text
   pub fn find_all_matches(
       snapshot: &MultiBufferSnapshot,
       query: &str,
       case_sensitive: bool,
   ) -> Vec<Range<usize>> { ... }

   /// Pure: find enclosing bracket ranges
   pub fn enclosing_bracket_ranges(
       snapshot: &MultiBufferSnapshot,
       position: usize,
   ) -> Option<(Range<usize>, Range<usize>)> { ... }
   ```

3. Update the GPUI methods in `selection.rs` to call these free functions.

**Validation**: New functions compile standalone. Existing `selection.rs`
methods still work by calling through.

---

### Task 3.4 — Extract pure input functions

**File**: `crates/editor/src/input.rs`

**Steps**:

1. The free functions are already extracted (they exist at the bottom of
   `input.rs`):
   - `comment_delimiter_for_newline()`
   - `documentation_delimiter_for_newline()`
   - `list_delimiter_for_newline()`
   - `is_list_prefix_row()`
   - `NewlineConfig::insert_extra_newline_brackets()`

2. Extract the bracket matching core from `handle_input()` into a pure
   function:
   ```rust
   /// Given buffer state, selections, and input text, compute:
   /// - edits to apply
   /// - new autoclose regions
   /// - whether a closing bracket should be inserted
   pub fn compute_input_edits(
       snapshot: &MultiBufferSnapshot,
       selections: &[Selection<Anchor>],
       text: &str,
       autoclose_regions: &[AutocloseRegion],
       language_settings: &LanguageSettings,
   ) -> InputEditResult { ... }

   pub struct InputEditResult {
       pub edits: Vec<(Range<usize>, Arc<str>)>,
       pub new_autoclose_regions: Vec<AutocloseRegion>,
       pub new_selections: Option<Vec<Selection<Anchor>>>,
   }
   ```

3. Keep `handle_input()` as the GPUI orchestrator that calls
   `compute_input_edits()` and applies results via transactions.

**Validation**: `compute_input_edits()` compiles without GPUI. Existing
`handle_input()` works by calling through.

---

### Task 3.5 — Decouple `WrapMap` text measurement

**File**: `crates/editor/src/display_map/wrap_map.rs`

**Steps**:

1. Define a `TextMeasurer` trait:
   ```rust
   pub trait TextMeasurer: Send + Sync {
       fn wrap_line(
           &self, text: &str, wrap_width: f32,
       ) -> Vec<usize>;  // byte offsets of wrap points
   }
   ```

2. Under `gpui-backend`, implement it using `gpui::LineWrapper`:
   ```rust
   #[cfg(feature = "gpui-backend")]
   struct GpuiTextMeasurer { line_wrapper: LineWrapper }
   ```

3. Under `tui-backend`, implement it using `unicode-width`:
   ```rust
   #[cfg(feature = "tui-backend")]
   pub struct TerminalTextMeasurer;
   impl TextMeasurer for TerminalTextMeasurer {
       fn wrap_line(&self, text: &str, wrap_width: f32) -> Vec<usize> {
           // Use unicode_width::UnicodeWidthChar to measure each char
           // Insert wrap points when cumulative width exceeds wrap_width
       }
   }
   ```

4. Replace `WrapMap`'s `Font` and `LineWrapper` usage with the trait.

5. Feature-gate `WrapMap` as `Entity` vs plain struct:
   - `gpui-backend`: remains `Entity<WrapMap>` with `Task` for async
   - `tui-backend`: plain struct, wrap computation is synchronous (terminals
     are fast enough for char-width measurement)

**Validation**: `WrapMap` compiles under both features.

---

### Task 3.6 — Feature-gate `DisplayMap` entity wrapper

**File**: `crates/editor/src/display_map/display_map.rs`

**Steps**:

1. Feature-gate the entity fields:
   ```rust
   #[cfg(feature = "gpui-backend")]
   buffer: Entity<MultiBuffer>,
   #[cfg(feature = "gpui-backend")]
   wrap_map: Entity<WrapMap>,

   #[cfg(feature = "tui-backend")]
   buffer: Arc<Mutex<MultiBuffer>>,
   #[cfg(feature = "tui-backend")]
   wrap_map: WrapMap,
   ```

2. The `DisplaySnapshot` struct is already pure data (no entities). It only
   references snapshots from each pipeline stage. No changes needed.

3. Feature-gate methods that take `&mut Context<Self>`:
   - `gpui-backend`: keep existing signatures
   - `tui-backend`: take `&mut self` only, access buffer via
     `self.buffer.lock()`

4. The `snapshot()` method (line 603) needs both variants:
   ```rust
   #[cfg(feature = "gpui-backend")]
   pub fn snapshot(&mut self, cx: &mut Context<Self>) -> DisplaySnapshot

   #[cfg(feature = "tui-backend")]
   pub fn snapshot(&mut self) -> DisplaySnapshot
   ```

**Validation**: Compiles under both features. Existing tests pass under
`gpui-backend`.

---

### Task 3.7 — Build `TuiEditorShell`

**File**: `crates/tui_zed/src/editor.rs` (new)

**Steps**:

1. Define the TUI editor wrapper:
   ```rust
   use editor::EditorState;
   use language::Buffer;
   use multi_buffer::MultiBuffer;

   pub struct TuiEditorShell {
       pub state: EditorState,
       buffer: Arc<Mutex<MultiBuffer>>,
       display_map: DisplayMap,
       display_snapshot: DisplaySnapshot,
       // Debounce handles
       reparse_task: Option<tokio::task::JoinHandle<()>>,
       highlight_task: Option<tokio::task::JoinHandle<()>>,
   }

   impl TuiEditorShell {
       pub fn open_file(path: &Path, executor: Arc<dyn Executor>) -> Result<Self>;
       pub fn handle_input(&mut self, text: &str);
       pub fn handle_action(&mut self, action: Action);
       pub fn refresh_display(&mut self);
       pub fn visible_lines(&self, viewport: Range<u32>) -> Vec<HighlightedLine>;
   }
   ```

2. Implement `handle_action` by dispatching to the extracted pure functions:
   ```rust
   pub fn handle_action(&mut self, action: Action) {
       let snapshot = self.display_snapshot();
       match action {
           Action::MoveUp => {
               self.state.selections.move_cursors_with(&snapshot, |map, point, goal| {
                   movement::up(map, point, goal, false, &self.text_layout)
               });
           }
           Action::MoveDown => { /* similar */ }
           Action::Backspace => {
               let edits = compute_delete_edits(&snapshot, &self.state.selections, Backward);
               self.apply_edits(edits);
           }
           // ...
       }
       self.refresh_display();
   }
   ```

3. Implement `visible_lines` by walking the `DisplaySnapshot`:
   ```rust
   pub fn visible_lines(&self, rows: Range<u32>) -> Vec<HighlightedLine> {
       let snapshot = &self.display_snapshot;
       let chunks = snapshot.highlighted_chunks(rows, false, &self.theme);
       // Convert to Vec<HighlightedLine> where each line is Vec<(String, Style)>
   }
   ```

**Validation**: Unit tests that open a file, perform edits, and verify buffer
contents.

---

### Task 3.8 — Build the ratatui editor widget

**File**: `crates/tui_zed/src/ui/editor_widget.rs` (new)

**Steps**:

1. Implement `ratatui::Widget` for the editor:
   ```rust
   impl Widget for &TuiEditorShell {
       fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
           // 1. Compute gutter width
           // 2. Split area into gutter + text regions
           // 3. Render line numbers in gutter
           // 4. Render highlighted text lines
           // 5. Render cursors (set cursor style on cell)
           // 6. Render selections (set background on cells)
       }
   }
   ```

2. Handle scroll offset: only render lines in
   `scroll_offset .. scroll_offset + viewport_height`.

3. Handle horizontal scroll: offset each line by `scroll_col`.

**Validation**: Visual test — open a Rust file, see syntax-highlighted code
with line numbers.

---

## Phase 4: Git & Workspace (New Code)

### Task 4.1 — Git operations module

**File**: `crates/tui_zed/src/git.rs` (new)

**Steps**:

1. Implement `GitRepo` struct with `tokio::process::Command` for:
   - `status()` — parse `git status --porcelain=v1`
   - `current_branch()` — `git rev-parse --abbrev-ref HEAD`
   - `diff_file(path)` — `git diff HEAD -- <path>`
   - `blame_file(path)` — `git blame --porcelain <path>`
   - `head_text(path)` — `git show HEAD:<path>`
   - `stage_file(path)` — `git add <path>`
   - `unstage_file(path)` — `git restore --staged <path>`
   - `commit(message)` — `git commit -m "<message>"`

2. Implement `DiffComputer` using `imara-diff` to compare buffer text against
   HEAD text:
   ```rust
   pub fn compute_diff_hunks(head: &str, current: &str) -> Vec<DiffHunk> { ... }
   ```

**Validation**: Unit tests against a temp git repo.

---

### Task 4.2 — File tree module

**File**: `crates/tui_zed/src/file_tree.rs` (new)

**Steps**:

1. Implement `FileTree` struct:
   - Use `ignore::WalkBuilder` for `.gitignore`-aware traversal
   - Store entries in a flat `Vec<FileEntry>` with depth tracking
   - Track `expanded: HashSet<PathBuf>`, `selected: usize`

2. Implement keyboard navigation:
   - Up/Down: move selection
   - Enter on dir: toggle expand/collapse
   - Enter on file: emit `OpenFile(path)` event
   - Left: collapse / go to parent
   - Right: expand / enter dir

3. Implement ratatui `Widget` rendering:
   - Tree-style indentation with `├──` / `└──` connectors
   - Git status colors (integrate with Task 4.1)
   - Highlight selected entry

4. Set up `notify::RecommendedWatcher` to auto-refresh on FS changes.

**Validation**: Visual test — navigate a project directory.

---

### Task 4.3 — Fuzzy finder

**File**: `crates/tui_zed/src/file_finder.rs` (new)

**Steps**:

1. Use `nucleo` for fuzzy matching:
   ```rust
   pub struct FileFinder {
       query: String,
       nucleo: nucleo::Nucleo<PathData>,
       selected: usize,
       visible: bool,
   }
   ```

2. Collect file paths using `ignore::WalkBuilder` (reuse from file tree).

3. Render as centered overlay popup with:
   - Text input at top
   - Matched files below with highlighted match positions
   - Enter to open, Esc to cancel

**Validation**: Visual test — type partial filename, see matches.

---

### Task 4.4 — Status bar

**File**: `crates/tui_zed/src/ui/status_bar.rs` (new)

**Steps**:

1. Render a single-line bar at the bottom:
   ```
   [mode] [filename] [modified?] [line:col] [encoding] [line-ending] [language] [lsp-status] [branch]
   ```

2. Pull data from `TuiEditorShell` (cursor position, buffer name),
   `GitRepo` (branch), and LSP state.

**Validation**: Shows accurate cursor position that updates on movement.

---

### Task 4.5 — Tab bar

**File**: `crates/tui_zed/src/ui/tab_bar.rs` (new)

**Steps**:

1. Render open buffer names at the top:
   ```
   [Tab1] [Tab2*] [Tab3]
   ```
   - Active tab highlighted
   - Modified indicator (`*`)

2. Keyboard shortcuts: `Ctrl+Tab` / `Ctrl+Shift+Tab` to cycle.

**Validation**: Open multiple files, switch between tabs.

---

### Task 4.6 — Settings system

**File**: `crates/tui_zed/src/settings.rs` (new)

**Steps**:

1. Define TOML config structure:
   ```rust
   #[derive(Deserialize)]
   pub struct TuiSettings {
       pub editor: EditorSettings,
       pub theme: ThemeSettings,
       pub lsp: HashMap<String, LspConfig>,
       pub keybindings: Option<HashMap<String, String>>,
   }
   ```

2. Load from `~/.config/tui-zed/settings.toml` or
   `.tui-zed/settings.toml` in project root.

3. Provide sensible defaults.

**Validation**: Change a setting, restart, see effect.

---

## Phase 5: Integration & Polish

### Task 5.1 — Wire LSP into editor

**Steps**:

1. On file open: start LSP server (if configured for language), send
   `textDocument/didOpen`.

2. On buffer edit: send `textDocument/didChange` with incremental edits
   (use `text::Buffer`'s patch output).

3. On save: send `textDocument/didSave`.

4. On diagnostics notification: store in `EditorState.inline_diagnostics`,
   trigger re-render.

5. On completion request (triggered by typing `.` or `:`): send
   `textDocument/completion`, show popup.

6. On go-to-definition: send `textDocument/definition`, jump to result.

7. On hover: send `textDocument/hover`, show popup.

---

### Task 5.2 — Completion popup widget

**File**: `crates/tui_zed/src/ui/completion_menu.rs` (new)

**Steps**:

1. Render a bordered box below/above the cursor with completion items.
2. Filter as user types.
3. Enter to confirm, Esc to dismiss.
4. Tab to cycle.
5. Show documentation in a side panel if available.

---

### Task 5.3 — Diagnostic rendering

**Steps**:

1. Gutter markers: `E` (error, red), `W` (warning, yellow) next to line
   numbers.

2. Inline underlines: underline the diagnostic range with error/warning
   color.

3. Diagnostic panel (toggle-able bottom panel): list all diagnostics with
   file:line:col and message. Enter to jump.

4. Next/prev diagnostic navigation keybindings.

---

### Task 5.4 — Command palette

**File**: `crates/tui_zed/src/ui/command_palette.rs` (new)

**Steps**:

1. Register all available commands with names and keybindings.
2. Fuzzy filter using `nucleo`.
3. Render as centered overlay (same pattern as file finder).
4. Enter to execute, Esc to cancel.

---

### Task 5.5 — Search in file

**Steps**:

1. Ctrl+F opens search bar at bottom of editor.
2. Type query, highlight all matches in buffer.
3. Enter to jump to next match, Shift+Enter for previous.
4. Optional: regex mode toggle.
5. Optional: replace mode.

---

### Task 5.6 — Search in project

**Steps**:

1. Ctrl+Shift+F opens project search panel.
2. Uses `grep` / `ripgrep` or Rope search across all files.
3. Results panel with file:line:match preview.
4. Enter to jump to result.

---

## Verification Checklist

After each phase, verify:

- [ ] `cargo check -p tui_zed` compiles
- [ ] `cargo check -p lsp --features gpui-backend` still compiles (no regression)
- [ ] `cargo check -p language --features gpui-backend` still compiles
- [ ] `cargo check -p editor --features gpui-backend` still compiles
- [ ] `cargo test -p editor` still passes (Zed's existing 39k lines of tests)
- [ ] `cargo test -p lsp` still passes
- [ ] `cargo test -p language` still passes
- [ ] `cargo run -p tui_zed -- <file>` opens and displays the file correctly

---

## Dependency Graph After Decoupling

```
tui_zed (binary)
├── editor (feature: tui-backend)
│   ├── text_style (NEW, GPUI-free)
│   ├── async_executor (NEW, GPUI-free)
│   ├── movement.rs (already GPUI-free)
│   ├── selections_collection.rs (already GPUI-free)
│   ├── display_map pipeline (feature-gated)
│   ├── multi_buffer (feature: tui-backend)
│   └── language (feature: tui-backend)
│       ├── syntax_map.rs (already GPUI-free except 1 import)
│       ├── language_core (already GPUI-free)
│       ├── syntax_theme (depends on text_style, not gpui)
│       └── language_registry (executor trait)
├── lsp (feature: tui-backend)
│   ├── async_executor
│   ├── gpui_shared_string (GPUI-free)
│   └── lsp-types (GPUI-free)
├── ratatui + crossterm
├── tokio
├── sum_tree, rope, text, clock (all GPUI-free)
├── nucleo, ignore, notify, imara-diff (all GPUI-free)
└── tui_zed-specific modules (git, file_tree, settings, UI widgets)
```

**Zero GPUI in the final binary's dependency tree.**

---

## Appendix A: The Ratatui Editor in Detail

This section covers the actual TUI application -- the ratatui code that the
user sees and interacts with. Everything above was about decoupling Zed's
internals. This is about building the frontend.

### A.1 Application Architecture

The app follows an **Elm-like architecture**: a central event loop receives
events, updates state, and re-renders. Ratatui is immediate-mode -- every
frame redraws the entire UI from scratch (ratatui diffs internally).

```
                    ┌─────────────────────────┐
                    │      App (owns all       │
                    │      application state)  │
                    └──────────┬──────────────┘
                               │
                ┌──────────────┼──────────────┐
                │              │              │
         ┌──────▼──────┐ ┌────▼────┐ ┌───────▼───────┐
         │ EditorPane  │ │FileTree │ │ BottomPanel   │
         │ (Vec of     │ │ (left   │ │ (diagnostics, │
         │  EditorTab) │ │  panel) │ │  search)      │
         └──────┬──────┘ └─────────┘ └───────────────┘
                │
         ┌──────▼──────┐
         │ EditorTab   │
         │ (one per    │
         │  open file) │
         │ owns:       │
         │  - buffer   │
         │  - display  │
         │  - state    │
         │  - lsp conn │
         └─────────────┘
```

### A.2 File & Module Structure

```
crates/tui_zed/
├── main.rs                  # Entry point, CLI arg parsing
├── tui_zed.rs               # Lib root: pub mod declarations
├── app.rs                   # App struct, event loop, top-level state
├── event.rs                 # Event enum, input mapping, Action enum
├── keymap.rs                # Key → Action mapping, configurable
│
├── editor/
│   ├── mod.rs               # (avoid: use editor_tab.rs instead)
│   ├── editor_tab.rs        # EditorTab: one open file/buffer
│   ├── editor_pane.rs       # EditorPane: tabbed container of editors
│   ├── rendering.rs         # The core ratatui Widget impl
│   ├── gutter.rs            # Line numbers, git diff markers, diagnostics
│   ├── highlights.rs        # Syntax highlight → ratatui::Style mapping
│   └── cursor.rs            # Cursor rendering (block, bar, underline)
│
├── panels/
│   ├── file_tree.rs         # File tree widget
│   ├── diagnostics.rs       # Diagnostic list panel
│   └── search.rs            # Search results panel
│
├── overlays/
│   ├── completion_menu.rs   # LSP completion popup
│   ├── hover_popup.rs       # LSP hover documentation
│   ├── command_palette.rs   # Fuzzy command search
│   ├── file_finder.rs       # Fuzzy file open (Ctrl+P)
│   └── dialog.rs            # Confirmation dialogs (save?, quit?)
│
├── chrome/
│   ├── tab_bar.rs           # Buffer tabs at top
│   ├── status_bar.rs        # Status line at bottom
│   └── layout.rs            # Top-level layout composition
│
├── lsp_manager.rs           # LSP server lifecycle, dispatch
├── git.rs                   # Git operations (shell out to git)
├── theme.rs                 # Zed theme → ratatui Style conversion
└── settings.rs              # TOML settings loader
```

### A.3 The `App` Struct and Event Loop

```rust
// app.rs

use crossterm::event::{EventStream, KeyCode, KeyModifiers, KeyEvent, Event};
use futures::StreamExt;
use ratatui::{Terminal, Frame};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Layout, Constraint, Direction, Rect};
use std::io::Stdout;

pub struct App {
    // Layout
    file_tree: FileTree,
    editor_pane: EditorPane,
    status_bar: StatusBar,
    tab_bar: TabBar,

    // Overlays (at most one visible at a time)
    active_overlay: Option<Overlay>,

    // Panels
    bottom_panel: Option<BottomPanel>,

    // State
    focus: Focus,          // which component has keyboard focus
    should_quit: bool,
    show_file_tree: bool,

    // Services
    lsp_manager: LspManager,
    git_repo: Option<GitRepo>,
    settings: TuiSettings,
    theme: TuiTheme,

    // Async channels
    lsp_event_rx: mpsc::UnboundedReceiver<LspEvent>,
    fs_event_rx: mpsc::UnboundedReceiver<notify::Event>,
}

pub enum Focus {
    Editor,
    FileTree,
    BottomPanel,
    Overlay,
}

pub enum Overlay {
    CompletionMenu(CompletionMenu),
    HoverPopup(HoverPopup),
    CommandPalette(CommandPalette),
    FileFinder(FileFinder),
    Dialog(Dialog),
}

pub enum BottomPanel {
    Diagnostics(DiagnosticsPanel),
    Search(SearchPanel),
}

impl App {
    pub async fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> anyhow::Result<()> {
        let mut event_stream = EventStream::new();

        loop {
            // 1. RENDER
            terminal.draw(|frame| self.render(frame))?;

            // 2. WAIT FOR NEXT EVENT
            tokio::select! {
                // Terminal input
                Some(Ok(event)) = event_stream.next() => {
                    self.handle_terminal_event(event)?;
                }
                // LSP notifications/responses
                Some(lsp_event) = self.lsp_event_rx.recv() => {
                    self.handle_lsp_event(lsp_event)?;
                }
                // File system changes
                Some(fs_event) = self.fs_event_rx.recv() => {
                    self.handle_fs_event(fs_event)?;
                }
                // Cursor blink timer (every 500ms)
                _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                    self.editor_pane.toggle_cursor_blink();
                }
            }

            if self.should_quit {
                break;
            }
        }
        Ok(())
    }
}
```

### A.4 Top-Level Layout

```rust
// chrome/layout.rs

impl App {
    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        // Top-level vertical split: tab_bar | main | status_bar
        let vertical = Layout::vertical([
            Constraint::Length(1),   // tab bar
            Constraint::Min(1),     // main content
            Constraint::Length(1),   // status bar
        ]);
        let [tab_area, main_area, status_area] = vertical.areas(area);

        // Render tab bar
        self.tab_bar.render(frame, tab_area, &self.editor_pane);

        // Main area: optional file tree | editor | optional bottom panel
        let main_with_bottom = if self.bottom_panel.is_some() {
            let split = Layout::vertical([
                Constraint::Percentage(70),
                Constraint::Percentage(30),
            ]);
            let [editor_area, panel_area] = split.areas(main_area);
            if let Some(ref panel) = self.bottom_panel {
                panel.render(frame, panel_area);
            }
            editor_area
        } else {
            main_area
        };

        // Horizontal split: file tree | editor
        if self.show_file_tree {
            let horizontal = Layout::horizontal([
                Constraint::Length(30), // file tree width
                Constraint::Min(1),    // editor
            ]);
            let [tree_area, editor_area] = horizontal.areas(main_with_bottom);
            self.file_tree.render(frame, tree_area);
            self.editor_pane.render(frame, editor_area, &self.theme);
        } else {
            self.editor_pane.render(frame, main_with_bottom, &self.theme);
        }

        // Status bar
        self.status_bar.render(frame, status_area, &self);

        // Overlays render LAST (on top of everything)
        if let Some(ref overlay) = self.active_overlay {
            overlay.render(frame, area);
        }

        // Set terminal cursor position
        if self.focus == Focus::Editor {
            if let Some(cursor_pos) = self.editor_pane.cursor_screen_position() {
                frame.set_cursor_position(cursor_pos);
            }
        }
    }
}
```

### A.5 Editor Rendering -- The Core Widget

This is the most complex widget. It renders syntax-highlighted text with
line numbers, cursors, selections, diagnostics, and git diff markers.

```rust
// editor/rendering.rs

use ratatui::buffer::Buffer as RatatuiBuf;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Render a single editor tab into a ratatui area.
pub fn render_editor(
    editor: &EditorTab,
    theme: &TuiTheme,
    area: Rect,
    buf: &mut RatatuiBuf,
    is_focused: bool,
) {
    if area.width < 2 || area.height < 1 {
        return;
    }

    let snapshot = &editor.display_snapshot;
    let gutter_width = compute_gutter_width(editor, snapshot);
    let text_area = Rect {
        x: area.x + gutter_width,
        y: area.y,
        width: area.width.saturating_sub(gutter_width),
        height: area.height,
    };
    let gutter_area = Rect {
        x: area.x,
        y: area.y,
        width: gutter_width,
        height: area.height,
    };

    let scroll_row = editor.state.scroll_offset.row;
    let scroll_col = editor.state.scroll_offset.col;
    let visible_rows = scroll_row..(scroll_row + area.height as u32);

    // -- GUTTER --
    render_gutter(editor, snapshot, theme, gutter_area, buf, &visible_rows);

    // -- TEXT WITH SYNTAX HIGHLIGHTS --
    render_text(editor, snapshot, theme, text_area, buf,
                &visible_rows, scroll_col);

    // -- SELECTIONS (background highlight) --
    render_selections(editor, snapshot, text_area, buf,
                      &visible_rows, scroll_col, is_focused);

    // -- CURSORS --
    render_cursors(editor, snapshot, text_area, buf,
                   &visible_rows, scroll_col, is_focused);

    // -- DIAGNOSTICS (underlines) --
    render_diagnostics(editor, snapshot, theme, text_area, buf,
                       &visible_rows, scroll_col);
}
```

#### A.5.1 Gutter Rendering

```rust
// editor/gutter.rs

fn render_gutter(
    editor: &EditorTab,
    snapshot: &DisplaySnapshot,
    theme: &TuiTheme,
    area: Rect,
    buf: &mut RatatuiBuf,
    visible_rows: &Range<u32>,
) {
    let max_line = snapshot.max_buffer_row().0;
    let line_number_width = digit_count(max_line) as u16;

    for (screen_row, display_row) in visible_rows.clone().enumerate() {
        let y = area.y + screen_row as u16;
        if y >= area.y + area.height { break; }

        // Map display row → buffer row (accounts for folds, blocks)
        let buffer_row = snapshot
            .display_point_to_point(DisplayPoint::new(DisplayRow(display_row), 0), Bias::Left)
            .row;

        // 1. Git diff marker (1 char)
        let diff_char = match editor.diff_hunk_at_row(buffer_row) {
            Some(DiffHunkStatus::Added) => ("+", Color::Green),
            Some(DiffHunkStatus::Modified) => ("~", Color::Yellow),
            Some(DiffHunkStatus::Removed) => ("-", Color::Red),
            None => (" ", Color::DarkGray),
        };
        buf.set_string(area.x, y, diff_char.0,
                       Style::default().fg(diff_char.1));

        // 2. Diagnostic marker (1 char)
        let diag_char = match editor.severity_at_row(buffer_row) {
            Some(DiagnosticSeverity::ERROR) => ("E", Color::Red),
            Some(DiagnosticSeverity::WARNING) => ("W", Color::Yellow),
            Some(DiagnosticSeverity::INFORMATION) => ("I", Color::Blue),
            Some(DiagnosticSeverity::HINT) => ("H", Color::Cyan),
            _ => (" ", Color::DarkGray),
        };
        buf.set_string(area.x + 1, y, diag_char.0,
                       Style::default().fg(diag_char.1));

        // 3. Line number (right-aligned)
        let line_num = format!("{:>width$}", buffer_row.0 + 1,
                               width = line_number_width as usize);
        let is_cursor_line = editor.is_cursor_on_row(buffer_row);
        let num_style = if is_cursor_line {
            theme.active_line_number_style()
        } else {
            theme.line_number_style()
        };
        buf.set_string(area.x + 2, y, &line_num, num_style);

        // 4. Separator
        buf.set_string(area.x + 2 + line_number_width, y, "│",
                       Style::default().fg(Color::DarkGray));
    }
}

fn compute_gutter_width(editor: &EditorTab, snapshot: &DisplaySnapshot) -> u16 {
    let max_line = snapshot.max_buffer_row().0;
    let line_number_width = digit_count(max_line) as u16;
    // diff_marker(1) + diag_marker(1) + line_numbers + separator(1) + padding(1)
    1 + 1 + line_number_width + 1 + 1
}

fn digit_count(n: u32) -> u32 {
    if n == 0 { 1 } else { (n as f64).log10().floor() as u32 + 1 }
}
```

#### A.5.2 Text Rendering with Syntax Highlights

```rust
// editor/highlights.rs + rendering.rs

fn render_text(
    editor: &EditorTab,
    snapshot: &DisplaySnapshot,
    theme: &TuiTheme,
    area: Rect,
    buf: &mut RatatuiBuf,
    visible_rows: &Range<u32>,
    scroll_col: u32,
) {
    // Use the decoupled DisplaySnapshot to get highlighted chunks.
    // This calls through to Zed's SyntaxMapCaptures + HighlightMap.
    let mut chunks = snapshot.highlighted_chunks(
        DisplayRow(visible_rows.start)..DisplayRow(visible_rows.end),
        false, // include_highlights
        &editor.highlight_styles,
    );

    let mut screen_row: u16 = 0;
    let mut col: u32 = 0;

    for chunk in chunks {
        let style = theme.ratatui_style_for_highlight(&chunk.highlight_style);

        for ch in chunk.text.chars() {
            if ch == '\n' {
                screen_row += 1;
                col = 0;
                continue;
            }

            // Tab expansion
            let char_width = if ch == '\t' {
                let tab_size = editor.tab_size() as u32;
                tab_size - (col % tab_size)
            } else {
                unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as u32
            };

            // Apply horizontal scroll
            if col + char_width > scroll_col {
                let screen_col = (col - scroll_col) as u16;
                let y = area.y + screen_row;
                let x = area.x + screen_col;

                if x < area.x + area.width && y < area.y + area.height {
                    if ch == '\t' {
                        // Render tab as spaces
                        for i in 0..char_width {
                            let tx = area.x + (col - scroll_col + i) as u16;
                            if tx < area.x + area.width {
                                buf.set_string(tx, y, " ", style);
                            }
                        }
                    } else {
                        buf.set_string(x, y, ch.to_string(), style);
                    }
                }
            }

            col += char_width;
        }
    }
}
```

#### A.5.3 Cursor Rendering

```rust
// editor/cursor.rs

fn render_cursors(
    editor: &EditorTab,
    snapshot: &DisplaySnapshot,
    text_area: Rect,
    buf: &mut RatatuiBuf,
    visible_rows: &Range<u32>,
    scroll_col: u32,
    is_focused: bool,
) {
    if !is_focused || !editor.cursor_blink_visible {
        return;
    }

    for selection in editor.state.selections.all::<DisplayPoint>(snapshot) {
        let cursor = selection.head();
        let row = cursor.row().0;
        let col = cursor.column() as u32;

        if row < visible_rows.start || row >= visible_rows.end {
            continue;
        }

        let screen_row = (row - visible_rows.start) as u16;
        let screen_col = col.saturating_sub(scroll_col) as u16;
        let x = text_area.x + screen_col;
        let y = text_area.y + screen_row;

        if x >= text_area.x + text_area.width || y >= text_area.y + text_area.height {
            continue;
        }

        match editor.state.cursor_shape {
            CursorShape::Block => {
                // Invert colors at cursor position
                if let Some(cell) = buf.cell_mut((x, y)) {
                    let fg = cell.fg();
                    let bg = cell.bg();
                    cell.set_fg(bg);
                    cell.set_bg(fg);
                }
            }
            CursorShape::Bar => {
                // Use the terminal's native cursor (set via frame.set_cursor_position)
                // Just make sure the character is visible
            }
            CursorShape::Underline => {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_style(cell.style().add_modifier(Modifier::UNDERLINED));
                }
            }
        }
    }
}
```

#### A.5.4 Selection Rendering

```rust
// editor/rendering.rs (continued)

fn render_selections(
    editor: &EditorTab,
    snapshot: &DisplaySnapshot,
    text_area: Rect,
    buf: &mut RatatuiBuf,
    visible_rows: &Range<u32>,
    scroll_col: u32,
    is_focused: bool,
) {
    let selection_bg = if is_focused {
        Color::Rgb(68, 68, 120)  // blueish highlight
    } else {
        Color::Rgb(50, 50, 50)   // dim when unfocused
    };

    for selection in editor.state.selections.all::<DisplayPoint>(snapshot) {
        if selection.is_empty() { continue; }

        let start = selection.start.min(selection.end);
        let end = selection.start.max(selection.end);

        for display_row in start.row().0..=end.row().0 {
            if display_row < visible_rows.start || display_row >= visible_rows.end {
                continue;
            }

            let screen_row = (display_row - visible_rows.start) as u16;
            let y = text_area.y + screen_row;

            // Determine column range for this row
            let col_start = if display_row == start.row().0 {
                start.column() as u32
            } else { 0 };

            let col_end = if display_row == end.row().0 {
                end.column() as u32
            } else {
                // End of line
                snapshot.line_len(DisplayRow(display_row)) as u32
            };

            for col in col_start..col_end {
                if col < scroll_col { continue; }
                let x = text_area.x + (col - scroll_col) as u16;
                if x >= text_area.x + text_area.width { break; }

                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_bg(selection_bg);
                }
            }
        }
    }
}
```

### A.6 Input Handling

```rust
// event.rs

use crossterm::event::{KeyCode, KeyModifiers, KeyEvent};

pub enum Action {
    // -- Movement (delegates to Zed's movement.rs) --
    MoveUp, MoveDown, MoveLeft, MoveRight,
    MoveWordLeft, MoveWordRight,
    MoveToLineStart, MoveToLineEnd,
    MoveToDocStart, MoveToDocEnd,
    PageUp, PageDown,

    // -- Movement with selection --
    SelectUp, SelectDown, SelectLeft, SelectRight,
    SelectWordLeft, SelectWordRight,
    SelectToLineStart, SelectToLineEnd,
    SelectAll,

    // -- Editing (delegates to extracted input.rs functions) --
    InsertChar(char),
    Backspace, Delete,
    BackspaceWord, DeleteWord,
    NewLine, NewLineAbove, NewLineBelow,
    Tab, BackTab,
    Undo, Redo,

    // -- Multi-cursor --
    AddCursorAbove, AddCursorBelow,
    SelectNextMatch, SelectAllMatches,

    // -- Clipboard --
    Copy, Cut, Paste,

    // -- LSP --
    GoToDefinition, GoToReferences,
    Hover, Rename, CodeAction,
    ShowCompletions, NextDiagnostic, PrevDiagnostic,

    // -- Panels --
    ToggleFileTree, ToggleDiagnostics,
    FocusEditor, FocusFileTree,

    // -- Files --
    Save, SaveAll,
    OpenFileFinder, OpenCommandPalette,
    CloseTab, NextTab, PrevTab,

    // -- Search --
    SearchInFile, SearchInProject,

    // -- App --
    Quit, ForceQuit,
}

pub fn map_key_event(key: KeyEvent) -> Option<Action> {
    use KeyCode::*;
    use KeyModifiers as Mod;

    let ctrl = key.modifiers.contains(Mod::CONTROL);
    let shift = key.modifiers.contains(Mod::SHIFT);
    let alt = key.modifiers.contains(Mod::ALT);

    match (key.code, ctrl, shift, alt) {
        // Movement
        (Up, false, false, false)    => Some(Action::MoveUp),
        (Down, false, false, false)  => Some(Action::MoveDown),
        (Left, false, false, false)  => Some(Action::MoveLeft),
        (Right, false, false, false) => Some(Action::MoveRight),
        (Left, true, false, false)   => Some(Action::MoveWordLeft),
        (Right, true, false, false)  => Some(Action::MoveWordRight),
        (Home, false, false, false)  => Some(Action::MoveToLineStart),
        (End, false, false, false)   => Some(Action::MoveToLineEnd),
        (Home, true, false, false)   => Some(Action::MoveToDocStart),
        (End, true, false, false)    => Some(Action::MoveToDocEnd),
        (PageUp, _, _, _)            => Some(Action::PageUp),
        (PageDown, _, _, _)          => Some(Action::PageDown),

        // Selection
        (Up, false, true, false)     => Some(Action::SelectUp),
        (Down, false, true, false)   => Some(Action::SelectDown),
        (Left, false, true, false)   => Some(Action::SelectLeft),
        (Right, false, true, false)  => Some(Action::SelectRight),
        (Left, true, true, false)    => Some(Action::SelectWordLeft),
        (Right, true, true, false)   => Some(Action::SelectWordRight),
        (Home, false, true, false)   => Some(Action::SelectToLineStart),
        (End, false, true, false)    => Some(Action::SelectToLineEnd),
        (Char('a'), true, false, false) => Some(Action::SelectAll),

        // Editing
        (Char(c), false, _, false)   => Some(Action::InsertChar(c)),
        (Char(c), false, true, false) => Some(Action::InsertChar(c)),
        (Backspace, false, false, _) => Some(Action::Backspace),
        (Delete, false, false, _)    => Some(Action::Delete),
        (Backspace, true, false, _)  => Some(Action::BackspaceWord),
        (Delete, true, false, _)     => Some(Action::DeleteWord),
        (Enter, false, false, false) => Some(Action::NewLine),
        (Tab, false, false, false)   => Some(Action::Tab),
        (BackTab, _, _, _)           => Some(Action::BackTab),

        // Undo / Redo
        (Char('z'), true, false, false) => Some(Action::Undo),
        (Char('z'), true, true, false)  => Some(Action::Redo),
        (Char('y'), true, false, false) => Some(Action::Redo),

        // Multi-cursor
        (Up, true, true, false)      => Some(Action::AddCursorAbove),
        (Down, true, true, false)    => Some(Action::AddCursorBelow),
        (Char('d'), true, false, false) => Some(Action::SelectNextMatch),
        (Char('d'), true, true, false)  => Some(Action::SelectAllMatches),

        // Clipboard
        (Char('c'), true, false, false) => Some(Action::Copy),
        (Char('x'), true, false, false) => Some(Action::Cut),
        (Char('v'), true, false, false) => Some(Action::Paste),

        // LSP
        (F(12), false, false, false)    => Some(Action::GoToDefinition),
        (Char('.'), true, false, false)  => Some(Action::CodeAction),
        (F(2), false, false, false)      => Some(Action::Rename),
        (Char(' '), true, false, false)  => Some(Action::ShowCompletions),

        // Files & panels
        (Char('s'), true, false, false)  => Some(Action::Save),
        (Char('p'), true, false, false)  => Some(Action::OpenFileFinder),
        (Char('p'), true, true, false)   => Some(Action::OpenCommandPalette),
        (Char('b'), true, false, false)  => Some(Action::ToggleFileTree),
        (Char('w'), true, false, false)  => Some(Action::CloseTab),
        (Char('f'), true, false, false)  => Some(Action::SearchInFile),
        (Char('f'), true, true, false)   => Some(Action::SearchInProject),

        // Diagnostics
        (Char('m'), true, true, false)   => Some(Action::ToggleDiagnostics),
        (F(8), false, false, false)      => Some(Action::NextDiagnostic),
        (F(8), false, true, false)       => Some(Action::PrevDiagnostic),

        // Quit
        (Char('q'), true, false, false)  => Some(Action::Quit),

        _ => None,
    }
}
```

### A.7 Action Dispatch

```rust
// app.rs (continued)

impl App {
    fn handle_terminal_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Key(key) => {
                // Overlay gets first priority
                if let Some(ref mut overlay) = self.active_overlay {
                    let result = overlay.handle_key(key);
                    match result {
                        OverlayResult::Consumed => return Ok(()),
                        OverlayResult::Dismiss => {
                            self.active_overlay = None;
                            return Ok(());
                        }
                        OverlayResult::Action(action) => {
                            self.active_overlay = None;
                            return self.dispatch_action(action);
                        }
                        OverlayResult::Ignored => {}
                    }
                }

                // Map key to action
                if let Some(action) = map_key_event(key) {
                    self.dispatch_action(action)?;
                }
            }
            Event::Resize(_, _) => {
                // ratatui handles this automatically on next draw
            }
            Event::Mouse(mouse) => {
                // Optional: handle mouse clicks for cursor placement
            }
            _ => {}
        }
        Ok(())
    }

    fn dispatch_action(&mut self, action: Action) -> Result<()> {
        match action {
            // -- Movement: delegate to Zed's movement.rs --
            Action::MoveUp => {
                self.active_editor_mut()?.move_cursor(movement::up);
            }
            Action::MoveDown => {
                self.active_editor_mut()?.move_cursor(movement::down);
            }
            Action::MoveLeft => {
                self.active_editor_mut()?.move_cursor_simple(movement::left);
            }
            // ... all other movements follow the same pattern

            // -- Editing: delegate to extracted input functions --
            Action::InsertChar(c) => {
                let editor = self.active_editor_mut()?;
                let edits = compute_input_edits(
                    &editor.buffer_snapshot(),
                    &editor.state.selections,
                    &c.to_string(),
                    &editor.state.autoclose_regions,
                    &editor.language_settings(),
                );
                editor.apply_edits(edits);
                editor.schedule_reparse();
                self.notify_lsp_did_change()?;
            }

            // -- LSP actions --
            Action::GoToDefinition => {
                let editor = self.active_editor()?;
                let position = editor.cursor_lsp_position();
                let uri = editor.document_uri();
                self.lsp_manager.request_definition(uri, position);
            }

            // -- File operations --
            Action::Save => {
                let editor = self.active_editor_mut()?;
                editor.save()?;
                self.notify_lsp_did_save()?;
            }

            // -- Panels --
            Action::ToggleFileTree => {
                self.show_file_tree = !self.show_file_tree;
            }

            // -- Overlays --
            Action::OpenFileFinder => {
                let finder = FileFinder::new(&self.editor_pane.workspace_root());
                self.active_overlay = Some(Overlay::FileFinder(finder));
            }

            Action::Quit => {
                if self.editor_pane.has_unsaved_changes() {
                    self.active_overlay = Some(Overlay::Dialog(
                        Dialog::confirm_quit()
                    ));
                } else {
                    self.should_quit = true;
                }
            }

            // ... remaining actions
            _ => {}
        }
        Ok(())
    }
}
```

### A.8 EditorTab -- One Open File

```rust
// editor/editor_tab.rs

pub struct EditorTab {
    // From decoupled Zed crates
    pub state: EditorState,             // extracted from editor::Editor
    buffer: Arc<Mutex<MultiBuffer>>,    // the text data
    display_map: DisplayMap,            // the display pipeline
    display_snapshot: DisplaySnapshot,  // cached for rendering

    // Syntax
    syntax_map: SyntaxMap,              // from language crate (decoupled)
    language: Option<Arc<Language>>,     // detected language

    // File info
    file_path: Option<PathBuf>,
    dirty: bool,                        // unsaved changes

    // LSP state
    diagnostics: Vec<(Range<Anchor>, Diagnostic)>,
    document_version: i32,

    // Git diff
    diff_hunks: Vec<DiffHunk>,

    // UI state
    cursor_blink_visible: bool,
    highlight_styles: HighlightStyles,

    // Async handles
    reparse_handle: Option<tokio::task::JoinHandle<()>>,
}

impl EditorTab {
    pub fn open(path: &Path, registry: &LanguageRegistry) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let language = registry.language_for_path(path);

        let mut text_buffer = text::Buffer::new(0, text::BufferId::new(1).unwrap(), content);
        // ... wrap in MultiBuffer, create DisplayMap, parse syntax
    }

    pub fn move_cursor(
        &mut self,
        movement_fn: impl Fn(&DisplaySnapshot, DisplayPoint, SelectionGoal, bool, &TextLayoutDetails)
            -> (DisplayPoint, SelectionGoal),
    ) {
        let snapshot = &self.display_snapshot;
        self.state.selections.move_cursors_with(snapshot, |map, point, goal| {
            movement_fn(map, point, goal, false, &self.text_layout_details())
        });
        self.ensure_cursor_visible();
    }

    pub fn move_cursor_simple(
        &mut self,
        movement_fn: impl Fn(&DisplaySnapshot, DisplayPoint) -> DisplayPoint,
    ) {
        let snapshot = &self.display_snapshot;
        self.state.selections.move_cursors_with(snapshot, |map, point, _goal| {
            (movement_fn(map, point), SelectionGoal::None)
        });
        self.ensure_cursor_visible();
    }

    pub fn apply_edits(&mut self, result: InputEditResult) {
        let mut buffer = self.buffer.lock();
        buffer.start_transaction();
        for (range, text) in &result.edits {
            buffer.edit([(range.clone(), text.as_ref())]);
        }
        buffer.end_transaction();
        drop(buffer);

        self.dirty = true;
        if let Some(new_regions) = result.new_autoclose_regions {
            self.state.autoclose_regions = new_regions;
        }
        self.refresh_display_snapshot();
    }

    pub fn schedule_reparse(&mut self) {
        // Cancel any in-flight reparse
        if let Some(handle) = self.reparse_handle.take() {
            handle.abort();
        }

        let syntax = self.syntax_map.snapshot();
        let text = self.buffer_snapshot();
        let language = self.language.clone();
        let registry = None; // pass if you have one

        self.reparse_handle = Some(tokio::spawn(async move {
            // This calls Zed's SyntaxSnapshot::reparse -- pure algorithm
            let new_syntax = reparse_syntax(&text, syntax, registry.as_ref(), language.as_ref());
            // Send result back via channel (or store in Arc<Mutex<>>)
            // ...
        }));
    }

    fn ensure_cursor_visible(&mut self) {
        // Auto-scroll so the cursor is within the viewport
        let cursor = self.state.selections.newest_display_point(&self.display_snapshot);
        let row = cursor.row().0;
        let col = cursor.column() as u32;

        // Vertical
        if row < self.state.scroll_offset.row {
            self.state.scroll_offset.row = row;
        } else if row >= self.state.scroll_offset.row + self.viewport_height {
            self.state.scroll_offset.row = row - self.viewport_height + 1;
        }

        // Horizontal
        if col < self.state.scroll_offset.col {
            self.state.scroll_offset.col = col;
        } else if col >= self.state.scroll_offset.col + self.viewport_width {
            self.state.scroll_offset.col = col - self.viewport_width + 1;
        }
    }

    pub fn save(&mut self) -> Result<()> {
        if let Some(ref path) = self.file_path {
            let buffer = self.buffer.lock();
            let text = buffer.snapshot().text();
            std::fs::write(path, text)?;
            self.dirty = false;
        }
        Ok(())
    }

    pub fn cursor_lsp_position(&self) -> lsp_types::Position {
        let cursor = self.state.selections.newest_anchor();
        let snapshot = self.buffer_snapshot();
        let point = cursor.head().to_point(&snapshot);
        lsp_types::Position {
            line: point.row,
            character: point.column,
        }
    }
}
```

### A.9 Overlay Widgets

Overlays are rendered on top of the main content. They are positioned
relative to the full terminal area.

```rust
// overlays/completion_menu.rs

pub struct CompletionMenu {
    items: Vec<CompletionItem>,
    selected: usize,
    anchor_row: u16,    // screen row to anchor below
    anchor_col: u16,    // screen col to anchor at
}

impl CompletionMenu {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let max_visible = 10.min(self.items.len());
        let menu_width = self.items.iter()
            .map(|i| i.label.len())
            .max()
            .unwrap_or(20) as u16 + 4;
        let menu_height = max_visible as u16 + 2; // +2 for border

        let x = self.anchor_col.min(area.width.saturating_sub(menu_width));
        let y = if self.anchor_row + 1 + menu_height <= area.height {
            self.anchor_row + 1  // below cursor
        } else {
            self.anchor_row.saturating_sub(menu_height) // above cursor
        };

        let menu_area = Rect::new(x, y, menu_width, menu_height);

        // Clear background
        frame.render_widget(Clear, menu_area);

        // Build list items
        let items: Vec<ListItem> = self.items.iter().enumerate()
            .skip(self.scroll_offset())
            .take(max_visible)
            .map(|(i, item)| {
                let style = if i == self.selected {
                    Style::default().bg(Color::Blue).fg(Color::White)
                } else {
                    Style::default()
                };
                let icon = completion_icon(item.kind);
                ListItem::new(Line::from(vec![
                    Span::styled(icon, Style::default().fg(Color::Cyan)),
                    Span::raw(" "),
                    Span::styled(&item.label, style),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Completions"));

        frame.render_widget(list, menu_area);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayResult {
        match key.code {
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.items.len() - 1);
                OverlayResult::Consumed
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                OverlayResult::Consumed
            }
            KeyCode::Enter | KeyCode::Tab => {
                let item = &self.items[self.selected];
                OverlayResult::Action(Action::ConfirmCompletion(item.clone()))
            }
            KeyCode::Esc => OverlayResult::Dismiss,
            _ => OverlayResult::Ignored,
        }
    }
}

fn completion_icon(kind: Option<CompletionItemKind>) -> &'static str {
    match kind {
        Some(CompletionItemKind::FUNCTION) => "fn",
        Some(CompletionItemKind::VARIABLE) => "va",
        Some(CompletionItemKind::STRUCT) | Some(CompletionItemKind::CLASS) => "st",
        Some(CompletionItemKind::FIELD) | Some(CompletionItemKind::PROPERTY) => "fd",
        Some(CompletionItemKind::MODULE) => "md",
        Some(CompletionItemKind::KEYWORD) => "kw",
        Some(CompletionItemKind::SNIPPET) => "sn",
        _ => "  ",
    }
}
```

### A.10 LSP Manager

```rust
// lsp_manager.rs

use lsp::LanguageServer;
use async_executor::Executor;

pub struct LspManager {
    executor: Arc<dyn Executor>,
    servers: HashMap<LanguageName, Arc<LanguageServer>>,
    event_tx: mpsc::UnboundedSender<LspEvent>,
}

pub enum LspEvent {
    Diagnostics { uri: Url, diagnostics: Vec<Diagnostic> },
    CompletionResponse { items: Vec<CompletionItem> },
    DefinitionResponse { locations: Vec<Location> },
    HoverResponse { contents: HoverContents },
    Initialized { language: LanguageName },
    ServerError { language: LanguageName, error: String },
}

impl LspManager {
    /// Start an LSP server for a language if not already running.
    pub async fn ensure_server(
        &mut self,
        language: &LanguageName,
        root_path: &Path,
        settings: &TuiSettings,
    ) -> Result<()> {
        if self.servers.contains_key(language) {
            return Ok(());
        }

        let config = settings.lsp.get(language.as_ref())
            .ok_or_else(|| anyhow::anyhow!("No LSP config for {}", language))?;

        // Use the decoupled lsp crate with our tokio executor
        let server = LanguageServer::new(
            /* ... binary config from settings ... */
            self.executor.clone(),
        )?;

        // Register notification handlers
        let tx = self.event_tx.clone();
        server.on_notification::<lsp_types::notification::PublishDiagnostics, _>(
            move |params| {
                tx.send(LspEvent::Diagnostics {
                    uri: params.uri,
                    diagnostics: params.diagnostics,
                }).ok();
            }
        );

        let server = server.initialize(/* params */).await?;
        self.servers.insert(language.clone(), server);
        Ok(())
    }

    pub fn notify_did_open(&self, language: &LanguageName, uri: Url, text: String, version: i32) {
        if let Some(server) = self.servers.get(language) {
            server.notify::<lsp_types::notification::DidOpenTextDocument>(
                DidOpenTextDocumentParams {
                    text_document: TextDocumentItem {
                        uri,
                        language_id: language.to_string(),
                        version,
                        text,
                    },
                },
            );
        }
    }

    pub fn notify_did_change(
        &self, language: &LanguageName, uri: Url, version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) {
        if let Some(server) = self.servers.get(language) {
            server.notify::<lsp_types::notification::DidChangeTextDocument>(
                DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier { uri, version },
                    content_changes: changes,
                },
            );
        }
    }

    pub fn request_completion(
        &self, language: &LanguageName, uri: Url, position: Position,
    ) {
        if let Some(server) = self.servers.get(language) {
            let tx = self.event_tx.clone();
            let server = server.clone();
            tokio::spawn(async move {
                match server.request::<lsp_types::request::Completion>(
                    CompletionParams {
                        text_document_position: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier { uri },
                            position,
                        },
                        ..Default::default()
                    },
                ).await {
                    Ok(Some(response)) => {
                        let items = match response {
                            CompletionResponse::Array(items) => items,
                            CompletionResponse::List(list) => list.items,
                        };
                        tx.send(LspEvent::CompletionResponse { items }).ok();
                    }
                    Ok(None) => {}
                    Err(e) => log::error!("Completion error: {e}"),
                }
            });
        }
    }

    // Similar methods for definition, hover, rename, code_action, references
}
```

### A.11 Putting It All Together -- `main.rs`

```rust
// main.rs

use std::io;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Parse CLI arguments
    let args: Vec<String> = std::env::args().collect();
    let file_path = args.get(1).map(|s| std::path::PathBuf::from(s));

    // 2. Load settings
    let settings = TuiSettings::load()?;

    // 3. Load theme
    let theme = TuiTheme::load(&settings.theme)?;

    // 4. Initialize language registry (reusing Zed's language_core)
    let executor = Arc::new(TokioExecutor::current());
    let registry = LanguageRegistry::new(executor.clone());
    register_builtin_languages(&registry);

    // 5. Set up LSP
    let (lsp_tx, lsp_rx) = tokio::sync::mpsc::unbounded_channel();
    let lsp_manager = LspManager::new(executor.clone(), lsp_tx);

    // 6. Detect git repo
    let git_repo = GitRepo::detect(std::env::current_dir()?);

    // 7. Create the app
    let mut app = App::new(settings, theme, registry, lsp_manager, lsp_rx, git_repo);

    // 8. Open initial file if provided
    if let Some(path) = file_path {
        app.open_file(&path)?;
    }

    // 9. Run the terminal
    let mut terminal = ratatui::init();
    let result = app.run(&mut terminal).await;
    ratatui::restore();

    result
}
```
