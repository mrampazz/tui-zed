use std::io::Stdout;

use anyhow::Result;
use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::{Frame, Terminal};

use crate::editor::TuiEditor;
use crate::event::{self, Action};

pub struct App {
    editor: TuiEditor,
    should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            editor: TuiEditor::new(),
            should_quit: false,
        }
    }

    pub fn open_file(&mut self, path: &std::path::Path) -> Result<()> {
        self.editor = TuiEditor::open(path)?;
        Ok(())
    }

    pub async fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> Result<()> {
        let mut event_stream = EventStream::new();

        loop {
            terminal.draw(|frame| self.render(frame))?;

            tokio::select! {
                Some(Ok(event)) = event_stream.next() => {
                    self.handle_terminal_event(event);
                }
            }

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn handle_terminal_event(&mut self, event: Event) {
        if let Event::Key(key) = event {
            if let Some(action) = event::map_key_event(key) {
                self.dispatch_action(action);
            }
        }
    }

    fn dispatch_action(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Save => {
                if let Err(error) = self.editor.save() {
                    log::error!("Failed to save: {error}");
                }
            }

            // Movement
            Action::MoveUp => self.editor.move_up(),
            Action::MoveDown => self.editor.move_down(),
            Action::MoveLeft => self.editor.move_left(),
            Action::MoveRight => self.editor.move_right(),
            Action::MoveToLineStart => self.editor.move_to_line_start(),
            Action::MoveToLineEnd => self.editor.move_to_line_end(),
            Action::MoveToDocStart => self.editor.move_to_doc_start(),
            Action::MoveToDocEnd => self.editor.move_to_doc_end(),
            Action::PageUp => self.editor.page_up(),
            Action::PageDown => self.editor.page_down(),

            // Editing
            Action::InsertChar(c) => self.editor.insert_char(c),
            Action::NewLine => self.editor.insert_newline(),
            Action::Backspace => self.editor.backspace(),
            Action::Delete => self.editor.delete(),
            Action::Tab => self.editor.insert_tab(),
            Action::Undo => self.editor.undo(),
            Action::Redo => self.editor.redo(),
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let vertical = Layout::vertical([
            Constraint::Min(1),    // editor
            Constraint::Length(1), // status bar
        ]);
        let [editor_area, status_area] = vertical.areas(area);

        self.render_editor(frame, editor_area);
        self.render_status_bar(frame, status_area);
    }

    fn render_editor(&mut self, frame: &mut Frame, area: Rect) {
        if area.height < 1 || area.width < 2 {
            return;
        }

        let max_line_num = self.editor.row_count();
        let gutter_width = digit_count(max_line_num) as u16 + 2; // digits + "│" + space

        let text_width = area.width.saturating_sub(gutter_width);
        let text_height = area.height;

        self.editor.set_viewport(text_height as u32, text_width as u32);

        let visible = self.editor.visible_rows();
        let cursor = self.editor.cursor();

        for (screen_row, buffer_row) in visible.clone().enumerate() {
            let y = area.y + screen_row as u16;
            if y >= area.y + area.height {
                break;
            }

            // Gutter: line number
            let line_num = format!(
                "{:>width$}",
                buffer_row + 1,
                width = gutter_width as usize - 2
            );
            let is_cursor_line = buffer_row == cursor.row;
            let num_style = if is_cursor_line {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            frame.render_widget(
                Paragraph::new(Span::styled(line_num, num_style)),
                Rect::new(area.x, y, gutter_width - 2, 1),
            );
            // Separator
            frame.render_widget(
                Paragraph::new(Span::styled("│", Style::default().fg(Color::DarkGray))),
                Rect::new(area.x + gutter_width - 2, y, 1, 1),
            );

            // Text content
            let line_text = self.editor.line_text(buffer_row);
            let scroll_col = self.editor.scroll_col() as usize;
            let visible_text: String = line_text
                .chars()
                .skip(scroll_col)
                .take(text_width as usize)
                .collect();

            let text_style = if is_cursor_line {
                Style::default()
            } else {
                Style::default()
            };

            frame.render_widget(
                Paragraph::new(Span::styled(visible_text, text_style)),
                Rect::new(area.x + gutter_width, y, text_width, 1),
            );
        }

        // Position the terminal cursor
        let cursor_screen_row = cursor.row.saturating_sub(self.editor.scroll_row());
        let cursor_screen_col =
            cursor.column.saturating_sub(self.editor.scroll_col()) as u16 + gutter_width;

        if cursor_screen_row < text_height as u32 {
            frame.set_cursor_position((
                area.x + cursor_screen_col,
                area.y + cursor_screen_row as u16,
            ));
        }
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let cursor = self.editor.cursor();
        let modified = if self.editor.is_dirty() { " [+]" } else { "" };

        let status = Line::from(vec![
            Span::styled(
                " NORMAL ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("{}{}", self.editor.file_name(), modified),
                Style::default().fg(Color::White),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{}:{}", cursor.row + 1, cursor.column + 1),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{} lines", self.editor.row_count()),
                Style::default().fg(Color::DarkGray),
            ),
        ]);

        frame.render_widget(
            Paragraph::new(status).style(Style::default().bg(Color::Rgb(30, 30, 30))),
            area,
        );
    }
}

fn digit_count(n: u32) -> u32 {
    if n == 0 {
        1
    } else {
        (n as f64).log10().floor() as u32 + 1
    }
}
