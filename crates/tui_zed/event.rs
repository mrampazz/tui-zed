use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Top-level actions the TUI can perform.
pub enum Action {
    // Movement
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    MoveToLineStart,
    MoveToLineEnd,
    MoveToDocStart,
    MoveToDocEnd,
    PageUp,
    PageDown,

    // Editing
    InsertChar(char),
    NewLine,
    Backspace,
    Delete,
    Tab,
    Undo,
    Redo,

    // File
    Save,

    // App
    Quit,
}

/// Map a crossterm key event to an application action.
pub fn map_key_event(key: KeyEvent) -> Option<Action> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match (key.code, ctrl, shift) {
        // Quit
        (KeyCode::Char('c'), true, _) => Some(Action::Quit),
        (KeyCode::Char('q'), true, _) => Some(Action::Quit),

        // Save
        (KeyCode::Char('s'), true, _) => Some(Action::Save),

        // Undo / Redo
        (KeyCode::Char('z'), true, false) => Some(Action::Undo),
        (KeyCode::Char('z'), true, true) => Some(Action::Redo),
        (KeyCode::Char('y'), true, false) => Some(Action::Redo),

        // Movement
        (KeyCode::Up, false, _) => Some(Action::MoveUp),
        (KeyCode::Down, false, _) => Some(Action::MoveDown),
        (KeyCode::Left, false, _) => Some(Action::MoveLeft),
        (KeyCode::Right, false, _) => Some(Action::MoveRight),
        (KeyCode::Home, false, _) => Some(Action::MoveToLineStart),
        (KeyCode::End, false, _) => Some(Action::MoveToLineEnd),
        (KeyCode::Home, true, _) => Some(Action::MoveToDocStart),
        (KeyCode::End, true, _) => Some(Action::MoveToDocEnd),
        (KeyCode::PageUp, _, _) => Some(Action::PageUp),
        (KeyCode::PageDown, _, _) => Some(Action::PageDown),

        // Editing
        (KeyCode::Enter, false, _) => Some(Action::NewLine),
        (KeyCode::Backspace, false, _) => Some(Action::Backspace),
        (KeyCode::Delete, false, _) => Some(Action::Delete),
        (KeyCode::Tab, false, _) => Some(Action::Tab),

        // Character input (only when ctrl is not held, except for ctrl combos above)
        (KeyCode::Char(c), false, _) => Some(Action::InsertChar(c)),

        _ => None,
    }
}
