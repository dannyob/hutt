# Codebase Assessment & Refactoring Suggestions

## Executive Summary

`hutt` is a mature TUI application with a solid foundation. It uses standard, high-quality Rust crates (`tokio`, `ratatui`, `crossterm`, `anyhow`, `serde`). The architecture follows a typical Elm-like model (State -> View -> Update), but it has outgrown its initial structure.

The primary technical debt lies in `src/tui/mod.rs`, which has become a monolithic "God object" handling state, logic, and rendering. This makes maintenance difficult, testing nearly impossible for UI logic, and increases the cognitive load for new contributors.

## 1. Architectural Improvements

### Deconstruct the `App` Monolith (`src/tui/mod.rs`)
The `App` struct holds over 60 fields, mixing long-lived configuration, transient UI state, and core data.

**Recommendation:** Group related fields into sub-structs or state machines.

```rust
// Current
pub struct App {
    pub search_input: String,
    pub search_textarea: TextArea<'static>,
    pub vim_sub_mode: VimSubMode,
    pub previous_folder: Option<String>,
    pub search_history: Vec<String>,
    // ... 50 more fields
}

// Proposed Refactoring
struct SearchState {
    input: TextArea<'static>,
    history: Vec<String>,
    mode: VimSubMode,
}

pub struct App {
    pub search: SearchState,
    pub compose: ComposeState,
    pub navigation: NavigationState,
    // ...
}
```

### Formalize State Transitions
Currently, modes and transient states are tracked via `InputMode` enum and various boolean flags (e.g., `creating_split`, `editing_folder`). This allows invalid states (e.g., `InputMode::Normal` but `creating_split = true`).

**Recommendation:** Use Rust's type system to enforce mutually exclusive states.

```rust
enum AppState {
    Browsing(BrowserState),
    Composing(ComposeState),
    InputPopup(PopupState), // Covers split/smart folder creation
}
```

### Separating View from Logic
The `run` function in `src/tui/mod.rs` mixes event loop logic with rendering logic. The `measure` closure and widget construction happens inline.

**Recommendation:** Extract a pure rendering function.
Create a `src/tui/view.rs` module that takes `&App` and returns/renders the UI. This isolates layout logic from business logic.

## 2. Abstraction & Code Quality

### Action Handler Extraction
The `run` loop contains a massive `match` statement (or series of `if let`) handling key events and IPC commands.

**Recommendation:** Implement the Command pattern.
Move the logic into `App::update(&mut self, action: Action) -> Result<Vec<Effect>>`. This allows unit testing actions without spinning up a TUI or Tokio runtime.

```rust
impl App {
    fn update(&mut self, action: Action) -> Result<()> {
        match action {
            Action::MoveDown => self.list.next(),
            Action::Compose => self.state = AppState::Composing(ComposeState::new()),
            // ...
        }
    }
}
```

### Generic Component for Lists/Pickers
The project implements similar list selection logic for:
- Folder Picker
- Command Palette
- Account Picker
- Move-to-Folder

**Recommendation:** Create a generic `SelectableList<T>` struct or trait that handles selection, filtering, and scrolling. This would remove duplicated logic in `tui/mod.rs` and the rendering code.

### Strong Typing for Primitives
The code relies heavily on `String` for identifiers (folders, message IDs, accounts).

**Recommendation:** Introduce newtypes to prevent mix-ups.
- `struct MessageId(String)`
- `struct FolderPath(String)`
- `struct AccountName(String)`

## 3. Specific Refactorings (Low Hanging Fruit)

1.  **Extract `src/tui/mod.rs` submodules**:
    - Move `App` struct definition to `src/tui/state.rs`.
    - Move `run` loop to `src/tui/runtime.rs`.
    - Move rendering to `src/tui/ui.rs`.

2.  **Unify Smart Folder & Split Logic**:
    Since both are essentially "saved queries," create a shared `SavedQuery` trait or struct. This handles the duplication noted in `CLAUDE.md`.

3.  **Config Logic Separation**:
    The `resolve_tabs` function and complex default logic in `src/tui/mod.rs` should move to `src/config.rs` or a `src/tabs.rs` module. The UI layer shouldn't be resolving configuration precedence.

## 4. Testing Strategy

Currently, testing `App` behavior is hard because of the I/O coupling (terminal, filesystem, mu subprocess).

**Recommendation:**
1.  **Mocking:** Abstract the `MuClient` behind a trait `MailBackend`. This allows testing the UI logic with in-memory data.
2.  **Headless UI Tests:** With the `update` function split from `run`, you can write tests like:
    ```rust
    let mut app = App::new_test();
    app.update(Action::Compose).unwrap();
    assert!(matches!(app.state, AppState::Composing(_)));
    ```

## 5. Error Handling

The project uses `anyhow::Result` everywhere. This is acceptable for the top-level application, but library-like modules (e.g., `mu_client`, `config`) should definitely define their own `Error` enums (using `thiserror`) to allow callers to handle specific failure cases recoverably.
