# TUI-Zed: Plan to Build a Terminal-Based Code Editor from Zed

## Goal

Build a terminal UI (TUI) code editor using **ratatui** that reuses Zed's core logic -- editor, LSP, and syntax highlighting -- by decoupling them from GPUI. For git, workspace, and file tree we build fresh implementations.

This is NOT a port of the Zed GUI. It is a new TUI frontend that surgically decouples and reuses Zed's battle-tested internals.

---

## Architecture Overview

```
┌──────────────────────────────────────────────────────────────┐
│                       tui_zed (binary)                        │
│  ┌────────────────────────────────────────────────────────┐  │
│  │              ratatui rendering layer                    │  │
│  │  ┌──────────┐ ┌────────┐ ┌──────┐ ┌───────────┐       │  │
│  │  │ EditorUI │ │FileTree│ │GitUI │ │StatusBar  │       │  │
│  │  └────┬─────┘ └───┬────┘ └──┬───┘ └─────┬─────┘       │  │
│  └───────┼────────────┼─────────┼───────────┼─────────────┘  │
│          │            │         │           │                 │
│  ┌───────┴────────────┴─────────┴───────────┴─────────────┐  │
│  │  tui_app (event loop, state, tokio async runtime)       │  │
│  └───────┬────────────┬─────────┬───────────┬─────────────┘  │
│          │            │         │           │                 │
│  ════════╪════════════╪═════════╪═══════════╪═════════════════│
│          │            │         │           │                 │
│  ┌───────┴──┐ ┌───────┴──┐ ┌───┴────┐ ┌────┴──────┐        │
│  │ editor   │ │ lsp      │ │tui_git │ │tui_project│        │
│  │(decoupled│ │(decoupled│ │(new)   │ │(new)      │        │
│  │ from Zed)│ │ from Zed)│ │        │ │           │        │
│  └───────┬──┘ └───────┬──┘ └────────┘ └───────────┘        │
│          │            │                                      │
│  ┌───────┴────────────┴──────────────────────────────────┐  │
│  │    Zed crates (decoupled or used directly)             │  │
│  │                                                        │  │
│  │  USED DIRECTLY (already GPUI-free):                    │  │
│  │    sum_tree, rope, text, clock, collections, util,     │  │
│  │    language_core, gpui_shared_string, snippet           │  │
│  │                                                        │  │
│  │  DECOUPLED (GPUI removed via feature flags / traits):  │  │
│  │    lsp, language, syntax_theme, editor (core logic),   │  │
│  │    multi_buffer, buffer_diff, settings                  │  │
│  └────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

---

## The Decoupling Strategy

The central insight from analyzing the codebase: Zed's GPUI coupling falls into a small number of repeating patterns. Instead of rewriting thousands of lines of battle-tested logic, we introduce abstraction boundaries that let the same code run under GPUI or tokio.

### The Five GPUI Coupling Patterns

| Pattern | GPUI Type | Occurrences | Replacement Strategy |
|---------|-----------|-------------|---------------------|
| **Async tasks** | `Task<T>`, `BackgroundExecutor`, `cx.spawn()`, `cx.background_spawn()` | ~50 sites across lsp + language + editor | Trait: `trait Executor { fn spawn<F>(...) -> TaskHandle<T>; fn background_spawn<F>(...) -> TaskHandle<T>; fn timer(d: Duration) -> impl Future; }` |
| **App context** | `&App`, `&mut AsyncApp`, `&mut Context<T>` | ~40 sites | Trait: `trait AppContext { fn global<T>() -> &T; }` or pass data directly |
| **Entity handles** | `Entity<T>`, `WeakEntity<T>`, `EntityId` | ~30 fields | Replace with `Arc<Mutex<T>>` / `Weak<Mutex<T>>` behind a type alias |
| **Reactivity** | `EventEmitter`, `cx.notify()`, `cx.observe()`, `Subscription` | ~20 sites | Channel-based: `mpsc::Sender<Event>` / callback registration |
| **Style types** | `HighlightStyle`, `Hsla`, `Pixels`, `Font`, `SharedString` | ~40 sites | Extract into standalone `text_style` crate |

### Implementation: Feature Flags

The cleanest approach is to add a `no-gpui` (or `tui`) feature flag to each decoupled crate. Under this flag, GPUI types are replaced with standalone equivalents. This avoids forking and lets us track upstream.

```toml
# Example: crates/lsp/Cargo.toml
[features]
default = ["gpui-backend"]
gpui-backend = ["gpui"]
tui-backend = ["tokio"]

[dependencies]
gpui = { workspace = true, optional = true }
tokio = { version = "1", features = ["full"], optional = true }
```

---

## Phase 0: Foundations

### 0.1 Create the `text_style` Crate

Before touching any existing crate, extract the style types that form the coupling boundary between syntax highlighting and GPUI into a new standalone crate.

**Types to extract from `gpui`:**

| Type | Current Location | Nature |
|------|-----------------|--------|
| `HighlightStyle` | `gpui/src/style.rs:576` | Struct: `{ color: Option<Hsla>, font_weight: Option<FontWeight>, font_style: Option<FontStyle>, background_color: Option<Hsla>, underline: Option<UnderlineStyle>, strikethrough: Option<StrikethroughStyle>, fade_out: Option<f32> }` |
| `Hsla` | `gpui/src/color.rs` | `{ h: f32, s: f32, l: f32, a: f32 }` |
| `Rgba` | `gpui/src/color.rs` | `{ r: f32, g: f32, b: f32, a: f32 }` |
| `FontWeight` | `gpui/src/text_system.rs` | Newtype: `FontWeight(pub f32)` |
| `FontStyle` | `gpui/src/text_system.rs` | Enum: `{ Normal, Italic, Oblique }` |
| `UnderlineStyle` | `gpui/src/style.rs:824` | `{ thickness: Pixels, color: Option<Hsla>, wavy: bool }` |
| `StrikethroughStyle` | `gpui/src/style.rs:839` | `{ thickness: Pixels, color: Option<Hsla> }` |
| `Pixels` | `gpui/src/geometry.rs` | Newtype: `Pixels(f32)` |

These are all **pure data structs** with zero runtime GPUI dependency. The new crate:

```toml
# crates/text_style/Cargo.toml
[package]
name = "text_style"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
```

Then update `gpui` to re-export from `text_style` (backward-compatible), and update `syntax_theme` to depend on `text_style` instead of `gpui`.

### 0.2 Create the `executor` Abstraction Crate

Create `crates/async_executor` that defines the executor trait used by all decoupled crates:

```rust
// crates/async_executor/async_executor.rs

pub trait Executor: Send + Sync + Clone + 'static {
    type TaskHandle<T: Send + 'static>: Future<Output = T> + Send;

    fn spawn<F, T>(&self, future: F) -> Self::TaskHandle<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static;

    fn spawn_blocking<F, T>(&self, func: F) -> Self::TaskHandle<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static;

    fn timer(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}
```

Two implementations:
- `GpuiExecutor` -- wraps `BackgroundExecutor`, `Task<T>` (in `gpui` crate)
- `TokioExecutor` -- wraps `tokio::runtime::Handle`, `JoinHandle<T>` (in `tui_zed`)

### 0.3 Create the TUI Binary Crate

```toml
# crates/tui_zed/Cargo.toml
[package]
name = "tui_zed"
edition = "2024"

[lib]
path = "tui_zed.rs"

[[bin]]
name = "tui-zed"
path = "main.rs"

[dependencies]
# TUI framework
ratatui = { version = "0.29", features = ["crossterm"] }
crossterm = "0.28"
tokio = { version = "1", features = ["full"] }

# Decoupled Zed crates (with tui-backend feature)
lsp = { path = "../lsp", default-features = false, features = ["tui-backend"] }
language = { path = "../language", default-features = false, features = ["tui-backend"] }
editor_core = { path = "../editor_core" }  # new crate, extracted logic
syntax_theme = { path = "../syntax_theme" }
multi_buffer = { path = "../multi_buffer", default-features = false, features = ["tui-backend"] }

# Zed crates used directly (already GPUI-free)
sum_tree.workspace = true
rope.workspace = true
text.workspace = true
clock.workspace = true
collections.workspace = true
util.workspace = true
language_core.workspace = true
gpui_shared_string.workspace = true
snippet.workspace = true
text_style = { path = "../text_style" }
async_executor = { path = "../async_executor" }

# External
anyhow = "1"
log = "0.4"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
nucleo.workspace = true
ignore.workspace = true
notify = "6"
imara-diff.workspace = true
```

### 0.4 Application Event Loop

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Terminal     │────>│  Event       │────>│  App State   │
│  Input        │     │  Loop        │     │  Update      │
│  (crossterm)  │     │  (tokio      │     │              │
│               │     │   select!)   │     │              │
└──────────────┘     └──────┬───────┘     └──────┬───────┘
                            │                     │
                     ┌──────┴───────┐     ┌──────┴───────┐
                     │  Async       │     │  Render      │
                     │  Events      │     │  (ratatui)   │
                     │  (LSP, git,  │     │              │
                     │   fs watch)  │     │              │
                     └──────────────┘     └──────────────┘
```

Use `tokio::select!` to multiplex:
1. Terminal input events (crossterm `EventStream`)
2. LSP server responses/notifications
3. File system change events (notify)
4. Git status updates
5. Render tick timer (cursor blink, debounced highlights)

---

## Phase 1: Decouple the LSP Crate

The `lsp` crate is ~2300 lines. The GPUI coupling is concentrated in **5 patterns across ~50 call sites**. The core protocol logic (JSON-RPC framing, request/response matching, cancellation, timeout) is sound and worth preserving.

### 1.1 GPUI Coupling Inventory

| Pattern | Count | Sites |
|---------|-------|-------|
| `BackgroundExecutor` / `cx.background_spawn()` | 8 | `new_internal`, `handle_stderr`, `handle_outgoing_messages`, `notification_tx` forwarding, `initialize`, `request_internal_with_timer`, `request_timeout_future`, `LspStdoutHandler::new` |
| `AsyncApp` in callbacks | 7 | `NotificationHandler`, `on_notification`, `on_request`, `on_custom_notification`, `on_custom_request`, `handle_incoming_messages` |
| `Task<T>` in struct fields | 5 | `LanguageServer::io_tasks`, `PendingRespondTasks`, `ResponseHandler` return, `LspStdoutHandler::loop_handle` |
| `SharedString` | 6 | `LanguageServer::version`, `LanguageServerName`, constructors, accessors |
| `&App` for globals | 1 | `default_initialize_params` reads `ReleaseChannel` and `AppVersion` |

### 1.2 Decoupling Changes

**Step 1: Replace `BackgroundExecutor` with `Executor` trait**

```rust
// Before (lsp.rs:120)
executor: BackgroundExecutor,

// After
executor: Arc<dyn Executor>,
```

All `executor.spawn(...)`, `executor.timer(...)`, `cx.background_spawn(...)` become `self.executor.spawn(...)`, `self.executor.timer(...)`.

**8 call sites** change from `cx.background_spawn(async { ... })` to `self.executor.spawn(async { ... })`.

**Step 2: Remove `AsyncApp` from notification/request handler signatures**

The `&mut AsyncApp` in handlers is used so handlers can access GPUI app state. Replace with a generic context or remove entirely:

```rust
// Before
type NotificationHandler = Box<dyn Send + FnMut(Option<RequestId>, Value, &mut AsyncApp)>;

// After (feature-gated)
#[cfg(feature = "gpui-backend")]
type NotificationHandler = Box<dyn Send + FnMut(Option<RequestId>, Value, &mut AsyncApp)>;

#[cfg(feature = "tui-backend")]
type NotificationHandler = Box<dyn Send + FnMut(Option<RequestId>, Value)>;
```

The `handle_incoming_messages` loop (line 614) dispatches to these handlers. Under `tui-backend`, handlers close over any state they need via `Arc<Mutex<T>>` instead of receiving `&mut AsyncApp`.

**~12 call sites** need `#[cfg]` gating on the callback signatures.

**Step 3: Replace `Task<T>` with a generic task handle**

```rust
// Before
io_tasks: Mutex<Option<(Task<Option<()>>, Task<Option<()>>)>>,

// After
io_tasks: Mutex<Option<(Box<dyn Future<Output = Option<()>> + Send>, ...)>>,
// Or simpler: use the Executor trait's associated TaskHandle type
```

Alternatively, define `type LspTask<T> = Pin<Box<dyn Future<Output = T> + Send>>` and use that uniformly.

**Step 4: Replace `SharedString` with `gpui_shared_string::SharedString`**

`gpui_shared_string` is already GPUI-free. The `lsp` crate currently imports `SharedString` from `gpui`, but can import it from `gpui_shared_string` directly. **Zero logic changes needed.**

**Step 5: Replace `&App` in `default_initialize_params`**

This function reads `ReleaseChannel` and `AppVersion` via GPUI globals. Replace with parameters:

```rust
// Before
fn default_initialize_params(cx: &App) -> InitializeParams { ... }

// After
fn default_initialize_params(client_info: Option<ClientInfo>) -> InitializeParams { ... }
```

**1 call site.**

### 1.3 Estimated Effort

| Change | Lines Modified | Complexity |
|--------|---------------|------------|
| Executor trait replacement | ~30 | Low (mechanical) |
| Handler signature cfg-gating | ~50 | Medium (must thread through dispatch) |
| Task type replacement | ~20 | Low |
| SharedString import swap | ~5 | Trivial |
| `default_initialize_params` | ~10 | Trivial |
| Feature flag wiring | ~15 | Low |
| **Total** | **~130** | **Low-Medium** |

The core protocol logic (~1800 lines of JSON-RPC framing, request matching, cancellation, backpressure, timeout) remains **completely untouched**.

---

## Phase 2: Decouple Syntax Highlighting

The syntax highlighting pipeline spans three crates: `language_core` (already GPUI-free), `syntax_theme` (shallow coupling), and `language` (moderate coupling). The actual parsing/highlighting algorithm is already GPUI-free -- the coupling is in the wrappers.

### 2.1 `syntax_theme` -- Extract Style Types

**Current coupling**: Imports `HighlightStyle`, `Hsla`, `Rgba`, `FontWeight`, `FontStyle` from `gpui`. These are pure data types.

**Change**: Depend on `text_style` (created in Phase 0.1) instead of `gpui`.

```toml
# Before
gpui.workspace = true

# After
text_style = { path = "../text_style" }
```

Update `~15 import lines`. Zero logic changes.

### 2.2 `language` Crate -- The SyntaxMap Pipeline

The critical file is `syntax_map.rs` (~2220 lines). Analysis shows it is **~99% algorithmic**:

| Component | Lines | GPUI Usage |
|-----------|-------|------------|
| `SyntaxSnapshot::interpolate()` | 118 | None |
| `SyntaxSnapshot::reparse()` | 451 | None |
| `SyntaxSnapshot::captures()` | 43 | None |
| `SyntaxSnapshot::matches()` | 41 | None |
| `SyntaxMapCaptures` iterator | 270 | None |
| `SyntaxMapMatches` iterator | 167 | None |
| `parse_text()` | 52 | None |
| `get_injections()` | 130 | None |
| `ParseStepLanguage::name()` | 6 | `SharedString` (1 line) |

**Total GPUI coupling in syntax_map.rs: 1 line** (a `SharedString` conversion in a logging helper).

**Fix**: Replace `gpui::SharedString` with `gpui_shared_string::SharedString` (already standalone). Done.

### 2.3 `language::Buffer` -- Decouple Reparse Scheduling

`Buffer::reparse()` (~70 lines) is the bridge between GPUI's async scheduling and the algorithmic `SyntaxSnapshot::reparse()`. It:
1. Takes `&mut Context<Self>` (GPUI entity context)
2. Spawns a background task via `cx.background_spawn()`
3. Calls `syntax_snapshot.reparse(...)` on the background thread (this is **pure**)
4. Spawns a foreground task to apply the result via `cx.spawn()`
5. Calls `cx.emit(BufferEvent::Reparsed)` and `cx.notify()`

**Decoupling approach**: Extract the pure reparse into a standalone function, add a scheduling wrapper behind a feature flag:

```rust
// Pure function -- no GPUI, no feature flag
pub fn reparse_syntax(
    text: &BufferSnapshot,
    syntax: SyntaxSnapshot,
    language_registry: Option<&Arc<LanguageRegistry>>,
    language: Option<&Arc<Language>>,
) -> SyntaxSnapshot {
    let mut syntax = syntax;
    syntax.reparse(text, language_registry, language);
    syntax
}

// GPUI scheduling (behind feature flag)
#[cfg(feature = "gpui-backend")]
impl Buffer {
    pub fn reparse(&mut self, cx: &mut Context<Self>) {
        // ... existing spawn/emit/notify logic ...
    }
}
```

Under `tui-backend`, the TUI app calls `reparse_syntax()` on a tokio background task and applies the result itself.

### 2.4 `LanguageRegistry` -- Replace Executor

`LanguageRegistry` stores a `BackgroundExecutor` (1 field) used for spawning grammar loading. Replace with the `Executor` trait:

```rust
// Before
executor: BackgroundExecutor,

// After
executor: Arc<dyn Executor>,
```

The `language_for_file()` method takes `&App` solely to read settings. Replace with a direct parameter:

```rust
// Before
fn language_for_file(&self, ..., cx: &App) -> ...

// After (tui-backend)
fn language_for_file(&self, ..., language_settings: &AllLanguageSettings) -> ...
```

### 2.5 `BufferSnapshot` Settings Access

Several methods (`settings_at`, `language_indent_size_at`) take `&App` to read from `SettingsStore` (a GPUI global). Replace with direct parameter passing:

```rust
// Before
fn settings_at<D>(&self, position: D, cx: &App) -> &LanguageSettings

// After (tui-backend)
fn settings_at<D>(&self, position: D, settings: &AllLanguageSettings) -> &LanguageSettings
```

### 2.6 Highlight Pipeline Summary

After decoupling, the full highlighting pipeline for the TUI is:

```
1. LanguageRegistry::language_for_path(path)     →  Arc<Language>
2. language.grammar().highlight_query()            →  tree_sitter::Query
3. SyntaxSnapshot::reparse(buffer_snapshot, ...)   →  updated trees (background task)
4. SyntaxSnapshot::captures(range, buffer_text)    →  Iterator<(range, HighlightId)>
5. HighlightMap::get(capture_index)                →  HighlightId
6. TuiTheme::style_for(HighlightId)               →  ratatui::Style
```

Steps 1-5 are **all Zed code, reused directly**. Step 6 is a thin mapping layer we write:

```rust
struct TuiTheme {
    highlight_styles: Vec<ratatui::style::Style>,
}

impl TuiTheme {
    fn style_for(&self, id: HighlightId) -> ratatui::style::Style {
        self.highlight_styles.get(id.0 as usize)
            .copied()
            .unwrap_or_default()
    }

    fn from_zed_theme(syntax_theme: &SyntaxTheme) -> Self {
        // Convert Hsla → ratatui::Color (truecolor RGB)
    }
}
```

### 2.7 Estimated Effort

| Crate | Lines Modified | Complexity |
|-------|---------------|------------|
| `syntax_theme` | ~15 (import swap) | Trivial |
| `language/syntax_map.rs` | ~1 (SharedString import) | Trivial |
| `language/buffer.rs` | ~40 (extract reparse, feature-flag scheduling) | Medium |
| `language/language_registry.rs` | ~20 (executor trait, settings param) | Low |
| `language/language_settings.rs` | ~30 (remove `&App` from public API) | Low-Medium |
| `language/language.rs` | ~15 (LspAdapter feature-gating) | Low |
| **Total** | **~120** | **Low-Medium** |

---

## Phase 3: Decouple the Editor Core

The `editor` crate is ~127k lines. We do NOT decouple all of it. We extract the **algorithmic core** -- the parts that compute editing operations on snapshots -- into a new `editor_core` crate (or feature-flag the existing one).

### 3.1 What's Already GPUI-Free (Reuse Directly)

| Component | File | Lines | Description |
|-----------|------|-------|-------------|
| **All movement functions** | `movement.rs` | 1509 | `left()`, `right()`, `up()`, `down()`, `word_start()`, `word_end()`, `line_beginning()`, `line_end()`, paragraph, etc. All take `&DisplaySnapshot` → `DisplayPoint`. |
| **SelectionsCollection** | `selections_collection.rs` | 1470 | Multi-cursor selection state. `select()`, `move_with()`, `move_cursors_with()`. Pure data structure. |
| **Rewrap algorithm** | `rewrap.rs` | 782 | `wrap_with_prefix()`, `WordBreakingTokenizer`. 90% pure standalone functions. |
| **Display pipeline: InlayMap** | `display_map/inlay_map.rs` | 2574 | ~95% algorithmic SumTree transforms. |
| **Display pipeline: FoldMap** | `display_map/fold_map.rs` | 2482 | ~90% algorithmic. Rendering callbacks are the only GPUI part. |
| **Display pipeline: TabMap** | `display_map/tab_map.rs` | 1700 | **100% GPUI-free.** Tab expansion arithmetic. |
| **Display pipeline: dimensions** | `display_map/dimensions.rs` | 100 | **100% GPUI-free.** |
| **Display pipeline: invisibles** | `display_map/invisibles.rs` | 133 | **100% GPUI-free.** |
| **Display pipeline: custom_highlights** | `display_map/custom_highlights.rs` | 421 | ~95% algorithmic. Uses `HighlightStyle` (data type only). |
| **Bracket colorization** | `bracket_colorization.rs` | — | Pure algorithm on syntax tree. |
| **Comment logic** | `input.rs` free functions | ~200 | `comment_delimiter_for_newline`, `documentation_delimiter_for_newline`, `is_list_prefix_row`, etc. |
| **Markdown paste** | `clipboard.rs` free functions | ~80 | `edit_for_markdown_paste`, `is_standalone_url`. Pure. |

**Total directly reusable: ~11,000+ lines of editing algorithms.**

### 3.2 What Needs Decoupling (Moderate Effort)

| Component | File | Lines | GPUI Coupling | Decoupling Strategy |
|-----------|------|-------|---------------|---------------------|
| **DisplayMap orchestrator** | `display_map.rs` | 4269 | 60% GPUI -- `Entity<MultiBuffer>`, `Entity<WrapMap>`, `cx.observe()`, `cx.notify()` | Feature-flag the entity wrapper. The snapshot assembly logic is pure. |
| **WrapMap** | `display_map/wrap_map.rs` | 1700 | 25% GPUI -- `Entity<Self>`, `Task`, `cx.background_spawn()`, `Font`, `LineWrapper` | Replace `Entity` with direct ownership under TUI. Replace async wrapping with tokio tasks. Provide a `trait TextMeasurer` for line width measurement. |
| **BlockMap** | `display_map/block_map.rs` | 4963 | 10% GPUI -- `BlockContext` (rendering), `EntityId`, `Pixels`, `AnyElement` | Feature-gate the rendering callback types. The core block insertion/tracking logic is pure. |
| **Selection operations** | `selection.rs` | 2398 | 30% GPUI -- `change_selections(window, cx)` wrapper | Extract inner lambdas into pure functions; keep the `change_selections` orchestration as a thin adapter. |
| **Input handling** | `input.rs` | 3077 | 35% GPUI -- transactions via entities, completion triggers | Extract the bracket matching / comment continuation algorithms as pure functions. Keep transaction orchestration as adapter. |

### 3.3 The Editor Split: `EditorState` vs `EditorShell`

Split the 255-field `Editor` struct into two layers:

```rust
/// Pure editing state -- no GPUI types.
/// ~80 fields: selections, modes, config, diagnostics data, etc.
pub struct EditorState {
    // Selections (already GPUI-free)
    pub selections: SelectionsCollection,
    pub selection_history: SelectionHistory,
    pub columnar_selection_state: Option<ColumnarSelectionState>,
    pub add_selections_state: Option<AddSelectionsState>,
    pub select_next_state: Option<SelectNextState>,
    pub select_prev_state: Option<SelectNextState>,

    // Editing state
    pub autoclose_regions: Vec<AutocloseRegion>,
    pub snippet_stack: InvalidationStack<SnippetState>,
    pub cursor_shape: CursorShape,
    pub mode: EditorMode,
    pub read_only: bool,
    pub input_enabled: bool,
    pub autoindent_mode: Option<AutoindentMode>,

    // Display config
    pub show_gutter: bool,
    pub show_line_numbers: Option<bool>,
    pub show_git_diff_gutter: Option<bool>,
    pub show_code_actions: Option<bool>,
    pub show_wrap_guides: Option<bool>,
    pub show_indent_guides: Option<bool>,
    pub current_line_highlight: Option<CurrentLineHighlight>,

    // Scroll
    pub scroll_offset: ScrollOffset,

    // LSP data (pure data, no entities)
    pub inline_diagnostics: Vec<(Anchor, InlineDiagnostic)>,
    pub lsp_document_symbols: HashMap<BufferId, Vec<OutlineItem<Anchor>>>,
    pub linked_edit_ranges: LinkedEditingRanges,

    // Git config
    pub show_git_blame_gutter: bool,
    pub show_git_blame_inline: bool,

    // ... remaining pure state fields
}
```

The `EditorShell` is framework-specific:

```rust
/// GPUI shell (for Zed GUI)
#[cfg(feature = "gpui-backend")]
pub struct GpuiEditorShell {
    pub state: EditorState,
    pub buffer: Entity<MultiBuffer>,
    pub display_map: Entity<DisplayMap>,
    pub focus_handle: FocusHandle,
    pub blink_manager: Entity<BlinkManager>,
    // ... ~50 Task<()> fields, Subscriptions, etc.
}

/// Ratatui shell (for TUI)
#[cfg(feature = "tui-backend")]
pub struct TuiEditorShell {
    pub state: EditorState,
    pub buffer: Arc<Mutex<MultiBuffer>>,
    pub display_snapshot: DisplaySnapshot, // refreshed on demand
    // ... tokio JoinHandles for debounced tasks
}
```

### 3.4 Pure Editing Operations

Extract the core editing operations as free functions operating on snapshots + state:

```rust
// movement -- already free functions in movement.rs, no changes needed
pub fn move_left(map: &DisplaySnapshot, point: DisplayPoint) -> DisplayPoint;
pub fn move_right(map: &DisplaySnapshot, point: DisplayPoint) -> DisplayPoint;
pub fn previous_word_start(map: &DisplaySnapshot, point: DisplayPoint) -> DisplayPoint;
// ... etc

// selection -- extract from selection.rs inner lambdas
pub fn select_line(
    snapshot: &DisplaySnapshot,
    selections: &[Selection<Point>],
) -> Vec<Selection<Point>>;

pub fn select_word(
    snapshot: &MultiBufferSnapshot,
    position: Point,
) -> Range<Point>;

pub fn split_selection_into_lines(
    snapshot: &MultiBufferSnapshot,
    selection: &Selection<Point>,
) -> Vec<Selection<Point>>;

// input -- extract from input.rs
pub fn compute_autoclose(
    snapshot: &MultiBufferSnapshot,
    selections: &[Selection<Anchor>],
    text: &str,
    autoclose_regions: &[AutocloseRegion],
) -> (Vec<(Range<usize>, String)>, Vec<AutocloseRegion>);

pub fn compute_bracket_pair_edit(
    snapshot: &MultiBufferSnapshot,
    position: Anchor,
    text: &str,
) -> Option<(String, String)>;  // (text_to_insert, closing_bracket)
```

### 3.5 The Display Pipeline for TUI

For the TUI, we reuse the display pipeline stages but replace `WrapMap`'s text measurement:

```rust
/// Trait replacing GPUI's WindowTextSystem for line width measurement.
/// In a terminal, every character is 1 or 2 cells wide (CJK).
pub trait TextMeasurer: Send + Sync {
    fn measure_line(&self, text: &str) -> u32;  // width in columns
}

/// Terminal implementation -- every char is 1 cell (2 for CJK)
pub struct TerminalTextMeasurer;
impl TextMeasurer for TerminalTextMeasurer {
    fn measure_line(&self, text: &str) -> u32 {
        unicode_width::UnicodeWidthStr::width(text) as u32
    }
}
```

The pipeline becomes:
```
Buffer → InlayMap → FoldMap → TabMap → WrapMap(TerminalTextMeasurer) → BlockMap
                                                                         ↓
                                                              DisplaySnapshot
                                                                         ↓
                                                              ratatui Widget
```

### 3.6 Estimated Effort

| Component | Lines Modified/Written | Complexity |
|-----------|----------------------|------------|
| `EditorState` extraction | ~200 (new struct, move fields) | Medium |
| Feature-flag `DisplayMap` entity wrapper | ~100 | Medium |
| `WrapMap` `TextMeasurer` trait | ~60 | Low |
| `BlockMap` feature-gate rendering | ~30 | Low |
| Extract pure selection functions | ~150 | Medium |
| Extract pure input functions | ~200 | Medium-High |
| `TuiEditorShell` implementation | ~300 | Medium |
| **Total** | **~1040** | **Medium** |

---

## Phase 4: Build the TUI Application

With the decoupled crates, we build the TUI application itself.

### 4.1 Keybinding System

Build a simple keybinding layer (Zed's action system is too GPUI-coupled):

```rust
enum Action {
    // Movement (delegates to movement.rs functions)
    MoveUp, MoveDown, MoveLeft, MoveRight,
    MoveWordLeft, MoveWordRight,
    MoveToLineStart, MoveToLineEnd,
    MoveToDocStart, MoveToDocEnd,
    PageUp, PageDown,

    // Editing (delegates to input.rs extracted functions)
    InsertChar(char), Backspace, Delete,
    NewLine, Tab, BackTab, Undo, Redo,

    // Selection (delegates to selection.rs extracted functions)
    SelectAll, SelectLine, SelectWord,
    AddCursorAbove, AddCursorBelow,

    // LSP (dispatches to decoupled lsp crate)
    GoToDefinition, Hover, Rename, CodeAction,
    ShowCompletions,

    // Workspace
    Save, Open, Quit, ToggleFileTree,
    NextBuffer, PrevBuffer, CommandPalette,
    FuzzyFileFinder,
}
```

### 4.2 Editor Widget (ratatui)

```rust
impl Widget for &TuiEditorShell {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let snapshot = &self.display_snapshot;
        let theme = &self.theme;

        // 1. Compute visible rows from scroll_offset + area.height
        let visible_rows = self.visible_row_range(area);

        // 2. Render gutter (line numbers + git diff markers)
        let gutter_width = self.gutter_width();
        for (display_row, buffer_row) in visible_rows.clone() {
            self.render_gutter_row(display_row, buffer_row, gutter_width, buf);
        }

        // 3. Render text with syntax highlights
        //    Uses: DisplaySnapshot::highlighted_chunks(range)
        //    → Iterator<HighlightedChunk { text, highlight_style, ... }>
        for (display_row, chunks) in snapshot.highlighted_chunks(visible_rows) {
            let spans: Vec<Span> = chunks.map(|chunk| {
                let style = theme.to_ratatui_style(&chunk.highlight_style);
                Span::styled(chunk.text, style)
            }).collect();
            let line = Line::from(spans);
            // render at (gutter_width + col_offset, row)
        }

        // 4. Render cursors and selections
        for selection in &self.state.selections {
            // Convert anchor → display point → screen position
            // Set cell style for selection background
            // Set cursor cell (block/bar/underline)
        }

        // 5. Render scrollbar
    }
}
```

### 4.3 Application Layout

```
┌──────────────────────────────────────────────────────────┐
│ [Tab1] [Tab2] [Tab3]                     [branch: main] │
├────────────────┬─────────────────────────────────────────┤
│ src/           │  1 │ fn main() {                        │
│ ├── main.rs    │  2 │     let x = 42;                    │
│ ├── editor.rs  │  3 │     println!("{x}");               │
│ └── lib.rs     │  4 │ }                                  │
│ Cargo.toml     │  5 │                                    │
│ README.md      │    │                                    │
│                │    │                                    │
├────────────────┴─────────────────────────────────────────┤
│ NORMAL  main.rs  3:15  UTF-8  LF  rust  rust-analyzer ✓ │
└──────────────────────────────────────────────────────────┘
```

### 4.4 Popups (Overlays)

- **Completion menu**: Renders below cursor, uses LSP completion items
- **Hover docs**: Renders in a floating box, Markdown → styled text
- **Command palette**: Centered overlay, fuzzy-filtered via `nucleo`
- **File finder**: Same as command palette but with file paths
- **Diagnostic panel**: Bottom panel, toggleable

---

## Phase 5: Git Integration (New Code)

Git and workspace are built fresh -- they're simpler and the existing Zed code is too GPUI-coupled to be worth decoupling.

### 5.1 Git Operations

```rust
pub struct GitRepo {
    repo_path: PathBuf,
}

impl GitRepo {
    pub async fn status(&self) -> Result<Vec<GitFileStatus>>;
    pub async fn diff_file(&self, path: &Path) -> Result<Vec<DiffHunk>>;
    pub async fn blame_file(&self, path: &Path) -> Result<Vec<BlameLine>>;
    pub async fn current_branch(&self) -> Result<String>;
    pub async fn stage_file(&self, path: &Path) -> Result<()>;
    pub async fn unstage_file(&self, path: &Path) -> Result<()>;
    pub async fn commit(&self, message: &str) -> Result<()>;
    pub async fn head_text(&self, path: &Path) -> Result<Option<String>>;
}
```

Shell out to the `git` binary (same approach as Zed's `RealGitRepository`).

### 5.2 Gutter Diff Markers

Use `imara-diff` (already a workspace dep) to diff buffer text against HEAD:
- `+` green: added lines
- `~` yellow: modified lines
- `-` red: deleted lines (shown as marker on adjacent line)

### 5.3 Git Status in File Tree

Color/icon file entries by status: Modified (yellow), Added (green), Deleted (red), Untracked (gray).

---

## Phase 6: Workspace & Project (New Code)

### 6.1 File Tree

```rust
pub struct FileTree {
    root: PathBuf,
    entries: Vec<FileEntry>,
    expanded: HashSet<PathBuf>,
    selected: usize,
    watcher: notify::RecommendedWatcher,
}
```

Use `ignore` crate for `.gitignore`-aware traversal. Use `notify` for live updates.

### 6.2 Buffer Management

```rust
pub struct BufferManager {
    buffers: Vec<TuiEditorShell>,
    active: usize,
}
```

### 6.3 Fuzzy Finder

Use `nucleo` (already a workspace dep) directly for fuzzy matching:

```rust
pub struct FileFinder {
    query: String,
    nucleo: nucleo::Nucleo<PathBuf>,
    selected: usize,
}
```

### 6.4 Settings

Simple TOML-based settings (not reusing Zed's `SettingsStore` which is a GPUI global):

```toml
[editor]
tab_size = 4
show_line_numbers = true
soft_wrap = "off"  # "off", "editor_width", "preferred_line_length"
preferred_line_length = 80

[theme]
name = "One Dark"  # or path to Zed theme JSON

[keybindings]
# Override defaults

[lsp.rust-analyzer]
command = "rust-analyzer"

[lsp.typescript]
command = "typescript-language-server"
args = ["--stdio"]
```

---

## Crate Reuse Map (Updated)

| Zed Crate | Strategy | Effort |
|-----------|----------|--------|
| `sum_tree` | **Use directly** -- GPUI-free | None |
| `rope` | **Use directly** -- GPUI-free | None |
| `text` | **Use directly** -- GPUI-free | None |
| `clock` | **Use directly** -- GPUI-free | None |
| `collections` | **Use directly** -- GPUI-free | None |
| `util` | **Use directly** -- GPUI-free | None |
| `language_core` | **Use directly** -- GPUI-free | None |
| `gpui_shared_string` | **Use directly** -- GPUI-free | None |
| `snippet` | **Use directly** -- GPUI-free | None |
| `syntax_theme` | **Decouple** -- swap `gpui` dep for `text_style` | ~15 lines |
| `lsp` | **Decouple** -- feature-flag executor + handler signatures | ~130 lines |
| `language` | **Decouple** -- feature-flag Buffer scheduling + settings access | ~120 lines |
| `editor` (core logic) | **Decouple** -- extract `EditorState` + pure functions | ~1040 lines |
| `multi_buffer` | **Decouple** -- feature-flag `Entity<Buffer>` → `Arc<Mutex<Buffer>>` | ~200 lines |
| `buffer_diff` | **Decouple** -- feature-flag entity wrapper | ~100 lines |
| `git` | **Build new** -- thin wrapper over git CLI | ~300 lines |
| `workspace` | **Build new** -- ratatui layout | ~500 lines |
| `project` | **Build new** -- simplified orchestrator | ~400 lines |
| `worktree` | **Build new** -- `notify` + `ignore` | ~300 lines |
| `settings` | **Build new** -- TOML-based | ~200 lines |
| `fs` | **Build new** -- thin async wrapper | ~100 lines |

**Total new code: ~1800 lines. Total decoupling modifications: ~1600 lines.**

Compare: rewriting LSP alone from scratch would be ~800 lines, and you'd lose Zed's timeout handling, cancellation, backpressure, IO logging, and test infrastructure. Rewriting the editor core would be 5000+ lines and you'd lose years of battle-tested multi-cursor, bracket matching, and display pipeline logic.

---

## Implementation Order & Milestones

### Milestone 0: "Infrastructure" (1 week)
- [ ] Create `text_style` crate (extract style types from `gpui`)
- [ ] Create `async_executor` crate (executor trait + tokio impl)
- [ ] Create `tui_zed` binary crate skeleton
- [ ] Set up tokio event loop with crossterm input

### Milestone 1: "Decoupled Editor" (2-3 weeks)
- [ ] Decouple `syntax_theme` (swap imports)
- [ ] Decouple `language` crate (feature-flag SyntaxMap scheduling)
- [ ] Decouple `lsp` crate (feature-flag executor + handlers)
- [ ] Extract `EditorState` from `editor::Editor`
- [ ] Extract pure movement/selection/input functions
- [ ] Feature-flag `DisplayMap` entity wrapper
- [ ] Implement `TuiEditorShell` with `TerminalTextMeasurer`
- [ ] Basic text rendering via ratatui (visible lines + line numbers)
- [ ] Cursor movement using Zed's `movement.rs` functions
- [ ] Text input using extracted `input.rs` functions
- [ ] File open/save

### Milestone 2: "Syntax & LSP" (2-3 weeks)
- [ ] Syntax highlighting using decoupled `language` + `syntax_theme`
- [ ] Tree-sitter parsing on tokio background tasks
- [ ] Theme loading (convert Zed themes → terminal colors)
- [ ] LSP client via decoupled `lsp` crate + tokio executor
- [ ] Document sync (didOpen/didChange/didSave/didClose)
- [ ] Diagnostics rendering (gutter + inline + panel)
- [ ] Completion popup
- [ ] Go to definition
- [ ] Hover documentation

### Milestone 3: "Git & Workspace" (2 weeks)
- [ ] Git repo detection + branch display
- [ ] Git diff in editor gutter (using `imara-diff`)
- [ ] Git status in file tree
- [ ] File tree panel (using `ignore` + `notify`)
- [ ] Fuzzy file finder (using `nucleo`)
- [ ] Tab bar / buffer management
- [ ] Status bar

### Milestone 4: "Polish" (2 weeks)
- [ ] Command palette
- [ ] Multi-cursor (add cursor above/below, select all matches)
- [ ] Search in file / project
- [ ] Rename symbol, code actions
- [ ] Configurable keybindings (TOML)
- [ ] Configurable theme
- [ ] Git blame
- [ ] Diagnostic navigation (next/prev error)

### Milestone 5: "Advanced" (ongoing)
- [ ] Vim mode
- [ ] Split panes
- [ ] Code folding (using decoupled FoldMap)
- [ ] Soft wrapping (using decoupled WrapMap)
- [ ] Language injections
- [ ] Mouse support
- [ ] Snippet expansion

---

## Technical Decisions

### Why decouple instead of rewrite?

| Approach | Lines of Code | What You Get | What You Lose |
|----------|--------------|--------------|---------------|
| **Decouple** | ~1600 modified | All of Zed's battle-tested logic: multi-cursor, bracket matching, incremental tree-sitter, LSP timeout/cancellation/backpressure, display pipeline, 39k lines of editor tests | Nothing -- full Zed parity on extracted features |
| **Rewrite** | ~8000+ new | Clean code, no GPUI vestiges | Years of edge-case handling, test coverage, LSP quirk workarounds |

The decoupling approach is **5x less code** and preserves **all of Zed's test suite** (which can run under the `gpui-backend` feature flag).

### Why feature flags instead of forking?

Feature flags (`#[cfg(feature = "tui-backend")]`) let us:
1. **Track upstream Zed** -- merge new features/fixes from Zed with minimal conflicts
2. **Run Zed's existing tests** -- the `gpui-backend` feature runs everything as before
3. **Gradual migration** -- decouple one crate at a time, verify nothing breaks
4. **Dual-target** -- the same codebase can build both Zed GUI and tui-zed

### Why tokio?

Zed uses `smol` internally, but since we're replacing GPUI's executor (not augmenting it), we choose the ecosystem with the widest library support:
- `tokio::process` for LSP server management
- `tokio::sync` for channels/mutexes
- `tokio::select!` for event multiplexing
- Widest async Rust ecosystem compatibility

### Why ratatui?

Most actively maintained TUI framework in Rust. Provides:
- Constraint-based layout (similar to flexbox)
- Rich styled text (Span/Line/Text)
- Widget trait for composable components
- Multiple backends (crossterm, termion, termwiz)
- Large ecosystem of examples and community widgets

### Terminal text measurement vs GPUI text measurement

GPUI uses GPU font shaping (`LineWrapper`, `WindowTextSystem`) for variable-width font measurement. In a terminal, every character is either 1 cell (ASCII, most Unicode) or 2 cells (CJK fullwidth). This makes `WrapMap` dramatically simpler in TUI mode -- the `TextMeasurer` trait implementation is ~5 lines using `unicode-width`.

---

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Feature-flag conditionals make code harder to read | Maintenance burden | Keep cfg blocks small and at module boundaries, not inside functions |
| Upstream Zed changes break our feature flags | Merge conflicts | Run CI for both backends; keep decoupling changes minimal |
| `text` or other core crates add GPUI deps | Build breaks | Pin workspace deps; CI checks that `tui-backend` builds without `gpui` |
| `multi_buffer` decoupling is harder than estimated | Timeline slip | Start with single-buffer support; add multi-buffer later |
| Terminal color limitations | Poor theme rendering | Support truecolor (most modern terminals); fallback to 256-color |
| LSP server quirks across languages | Broken features | Test with rust-analyzer, typescript-language-server, pyright first |
| Performance with large files | Slow rendering | Rope + tree-sitter already handle this; only render visible viewport |
| Crossterm input quirks | Broken keybindings | Test on iTerm2, Alacritty, WezTerm, kitty; use enhanced key detection |
