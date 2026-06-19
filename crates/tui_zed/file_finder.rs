use std::path::{Path, PathBuf};

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Result of interacting with the file finder overlay.
pub enum FileFinderResult {
    /// The finder consumed the key event, stay open.
    Consumed,
    /// The user selected a file. Close the finder.
    Selected(PathBuf),
    /// The user dismissed the finder. Close it.
    Dismissed,
}

/// A fuzzy file finder overlay (Ctrl+P).
pub struct FileFinder {
    query: String,
    all_paths: Vec<PathBuf>,
    filtered: Vec<(usize, PathBuf)>, // (score, path)
    selected: usize,
}

impl FileFinder {
    /// Create a new file finder scanning from the given root.
    pub fn new(root: &Path) -> Self {
        let mut all_paths = Vec::new();

        let walker = ignore::WalkBuilder::new(root)
            .hidden(true)
            .build();

        for entry in walker.flatten() {
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                if let Ok(relative) = entry.path().strip_prefix(root) {
                    all_paths.push(relative.to_path_buf());
                }
            }
        }

        all_paths.sort();

        let filtered = all_paths
            .iter()
            .enumerate()
            .map(|(i, p)| (i, p.clone()))
            .collect();

        Self {
            query: String::new(),
            all_paths,
            filtered,
            selected: 0,
        }
    }

    /// Handle a key event. Returns what the app should do.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> FileFinderResult {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Esc => FileFinderResult::Dismissed,
            KeyCode::Enter => {
                if let Some((_, path)) = self.filtered.get(self.selected) {
                    FileFinderResult::Selected(path.clone())
                } else {
                    FileFinderResult::Dismissed
                }
            }
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                FileFinderResult::Consumed
            }
            KeyCode::Down => {
                if self.selected + 1 < self.filtered.len() {
                    self.selected += 1;
                }
                FileFinderResult::Consumed
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.update_filter();
                FileFinderResult::Consumed
            }
            KeyCode::Char(c) => {
                self.query.push(c);
                self.update_filter();
                FileFinderResult::Consumed
            }
            _ => FileFinderResult::Consumed,
        }
    }

    fn update_filter(&mut self) {
        if self.query.is_empty() {
            self.filtered = self
                .all_paths
                .iter()
                .enumerate()
                .map(|(i, p)| (i, p.clone()))
                .collect();
        } else {
            let query_lower = self.query.to_lowercase();
            self.filtered = self
                .all_paths
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    let path_str = p.to_string_lossy().to_lowercase();
                    fuzzy_match(&path_str, &query_lower)
                })
                .map(|(i, p)| (i, p.clone()))
                .collect();
        }

        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }

    /// Render the file finder as a centered overlay.
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let width = (area.width * 3 / 5).max(30).min(area.width - 4);
        let height = 15u16.min(area.height - 4);
        let x = area.x + (area.width - width) / 2;
        let y = area.y + (area.height - height) / 3;
        let popup_area = Rect::new(x, y, width, height);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Open File ")
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height < 2 || inner.width < 2 {
            return;
        }

        // Input line
        let input_line = Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(&self.query),
            Span::styled("│", Style::default().fg(Color::DarkGray)),
        ]);
        frame.render_widget(
            Paragraph::new(input_line),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );

        // Results
        let results_area = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
        let max_visible = results_area.height as usize;

        let scroll = if self.selected >= max_visible {
            self.selected - max_visible + 1
        } else {
            0
        };

        for (screen_row, (_, path)) in self
            .filtered
            .iter()
            .skip(scroll)
            .take(max_visible)
            .enumerate()
        {
            let is_selected = scroll + screen_row == self.selected;
            let display = path.to_string_lossy();
            let truncated: String = display.chars().take(results_area.width as usize).collect();

            let style = if is_selected {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(50, 50, 80))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            frame.render_widget(
                Paragraph::new(Span::styled(truncated, style)),
                Rect::new(
                    results_area.x,
                    results_area.y + screen_row as u16,
                    results_area.width,
                    1,
                ),
            );
        }
    }
}

/// Simple fuzzy match: all characters in the query appear in order in the target.
fn fuzzy_match(target: &str, query: &str) -> bool {
    let mut target_chars = target.chars();
    for query_char in query.chars() {
        loop {
            match target_chars.next() {
                Some(c) if c == query_char => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match() {
        assert!(fuzzy_match("src/main.rs", "main"));
        assert!(fuzzy_match("src/main.rs", "smr"));
        assert!(fuzzy_match("src/main.rs", "src/main.rs"));
        assert!(!fuzzy_match("src/main.rs", "xyz"));
        assert!(fuzzy_match("hello", ""));
    }
}
