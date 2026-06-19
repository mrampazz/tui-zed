use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Top-level actions the TUI can perform.
pub enum Action {
    Quit,
}

/// Map a crossterm key event to an application action.
pub fn map_key_event(key: KeyEvent) -> Option<Action> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match (key.code, ctrl) {
        (KeyCode::Char('c'), true) => Some(Action::Quit),
        (KeyCode::Char('q'), false) => Some(Action::Quit),
        _ => None,
    }
}
