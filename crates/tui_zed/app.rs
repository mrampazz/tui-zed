use std::io::Stdout;
use std::path::Path;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::{Frame, Terminal};

use crate::editor::TuiEditor;
use crate::event::{self, Action};
use crate::file_finder::{FileFinder, FileFinderResult};
use crate::file_tree::{FileTree, FileTreeAction};
use crate::git::{DiffHunkStatus, GitRepo};

#[derive(PartialEq)]
enum Focus {
    Editor,
    FileTree,
}

pub struct App {
    editor: TuiEditor,
    file_tree: Option<FileTree>,
    file_finder: Option<FileFinder>,
    git_repo: Option<GitRepo>,
    git_branch: Option<String>,
    diff_hunks: Vec<crate::git::DiffHunk>,
    focus: Focus,
    should_quit: bool,
    workdir: std::path::PathBuf,
}

impl App {
    pub fn new() -> Self {
        let workdir = std::env::current_dir().unwrap_or_default();
        let git_repo = GitRepo::detect(&workdir);
        let git_branch = git_repo
            .as_ref()
            .and_then(|r| r.current_branch().ok());

        let file_tree = Some(FileTree::new(&workdir));

        Self {
            editor: TuiEditor::new(),
            file_tree,
            file_finder: None,
            git_repo,
            git_branch,
            diff_hunks: Vec::new(),
            focus: Focus::Editor,
            should_quit: false,
            workdir,
        }
    }

    pub fn open_file(&mut self, path: &Path) -> Result<()> {
        self.editor = TuiEditor::open(path)?;
        self.focus = Focus::Editor;
        self.refresh_diff();
        Ok(())
    }

    fn refresh_diff(&mut self) {
        self.diff_hunks.clear();
        if let (Some(repo), Some(path)) = (&self.git_repo, self.editor.file_path()) {
            if let Ok(Some(head_text)) = repo.head_text(path) {
                let current_text = self.editor.buffer().text();
                self.diff_hunks = GitRepo::diff_hunks(&head_text, &current_text);
            }
        }
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
                    self.handle_terminal_event(event)?;
                }
            }

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn handle_terminal_event(&mut self, event: Event) -> Result<()> {
        let Event::Key(key) = event else {
            return Ok(());
        };

        // File finder overlay gets priority
        if let Some(finder) = &mut self.file_finder {
            match finder.handle_key(key) {
                FileFinderResult::Consumed => return Ok(()),
                FileFinderResult::Dismissed => {
                    self.file_finder = None;
                    return Ok(());
                }
                FileFinderResult::Selected(path) => {
                    self.file_finder = None;
                    let full_path = self.workdir.join(&path);
                    self.open_file(&full_path)?;
                    return Ok(());
                }
            }
        }

        // File tree focus
        if self.focus == Focus::FileTree {
            if let Some(tree) = &mut self.file_tree {
                match key.code {
                    KeyCode::Up => tree.move_up(),
                    KeyCode::Down => tree.move_down(),
                    KeyCode::Enter => {
                        match tree.select() {
                            FileTreeAction::OpenFile(path) => {
                                self.open_file(&path)?;
                            }
                            FileTreeAction::None => {}
                        }
                    }
                    KeyCode::Left => tree.collapse_or_parent(),
                    KeyCode::Right => tree.expand_or_enter(),
                    KeyCode::Esc => self.focus = Focus::Editor,
                    _ => {
                        // Pass ctrl combos through
                        if let Some(action) = event::map_key_event(key) {
                            self.dispatch_action(action)?;
                        }
                    }
                }
                return Ok(());
            }
        }

        // Editor focus
        if let Some(action) = event::map_key_event(key) {
            self.dispatch_action(action)?;
        }

        Ok(())
    }

    fn dispatch_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Save => {
                self.editor.save().ok();
                self.refresh_diff();
            }

            Action::OpenFileFinder => {
                self.file_finder = Some(FileFinder::new(&self.workdir));
            }
            Action::ToggleFileTree => {
                if let Some(tree) = &mut self.file_tree {
                    tree.toggle_visible();
                }
                if self.focus == Focus::FileTree {
                    self.focus = Focus::Editor;
                } else {
                    self.focus = Focus::FileTree;
                }
            }
            Action::FocusEditor => {
                self.focus = Focus::Editor;
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
            Action::InsertChar(c) => {
                self.editor.insert_char(c);
                self.refresh_diff();
            }
            Action::NewLine => {
                self.editor.insert_newline();
                self.refresh_diff();
            }
            Action::Backspace => {
                self.editor.backspace();
                self.refresh_diff();
            }
            Action::Delete => {
                self.editor.delete();
                self.refresh_diff();
            }
            Action::Tab => {
                self.editor.insert_tab();
                self.refresh_diff();
            }
            Action::Undo => {
                self.editor.undo();
                self.refresh_diff();
            }
            Action::Redo => {
                self.editor.redo();
                self.refresh_diff();
            }
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let vertical = Layout::vertical([
            Constraint::Min(1),    // main content
            Constraint::Length(1), // status bar
        ]);
        let [main_area, status_area] = vertical.areas(area);

        // Horizontal split: file tree | editor
        let show_tree = self
            .file_tree
            .as_ref()
            .is_some_and(|t| t.is_visible());

        if show_tree {
            let horizontal = Layout::horizontal([
                Constraint::Length(30),
                Constraint::Min(1),
            ]);
            let [tree_area, editor_area] = horizontal.areas(main_area);

            if let Some(tree) = &self.file_tree {
                tree.render(frame, tree_area);
            }
            self.render_editor(frame, editor_area);
        } else {
            self.render_editor(frame, main_area);
        }

        self.render_status_bar(frame, status_area);

        // Overlays render last (on top)
        if let Some(finder) = &self.file_finder {
            finder.render(frame, area);
        }
    }

    fn render_editor(&mut self, frame: &mut Frame, area: Rect) {
        if area.height < 1 || area.width < 2 {
            return;
        }

        let max_line_num = self.editor.row_count();
        let gutter_width = digit_count(max_line_num) as u16 + 3; // diff + digits + "│"

        let text_width = area.width.saturating_sub(gutter_width);
        let text_height = area.height;

        self.editor
            .set_viewport(text_height as u32, text_width as u32);

        let visible = self.editor.visible_rows();
        let cursor = self.editor.cursor();

        for (screen_row, buffer_row) in visible.clone().enumerate() {
            let y = area.y + screen_row as u16;
            if y >= area.y + area.height {
                break;
            }

            let is_cursor_line = buffer_row == cursor.row;

            // Gutter: diff marker (1 char)
            let diff_status = GitRepo::line_diff_status(&self.diff_hunks, buffer_row);
            let (diff_char, diff_color) = match diff_status {
                Some(DiffHunkStatus::Added) => ("│", Color::Green),
                Some(DiffHunkStatus::Modified) => ("│", Color::Yellow),
                Some(DiffHunkStatus::Removed) => ("─", Color::Red),
                None => (" ", Color::DarkGray),
            };
            frame.render_widget(
                Paragraph::new(Span::styled(
                    diff_char,
                    Style::default().fg(diff_color),
                )),
                Rect::new(area.x, y, 1, 1),
            );

            // Gutter: line number
            let num_width = gutter_width as usize - 2; // minus diff char and separator
            let line_num = format!("{:>width$}", buffer_row + 1, width = num_width);
            let num_style = if is_cursor_line {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            frame.render_widget(
                Paragraph::new(Span::styled(line_num, num_style)),
                Rect::new(area.x + 1, y, gutter_width - 2, 1),
            );

            // Separator
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "│",
                    Style::default().fg(Color::Rgb(60, 60, 60)),
                )),
                Rect::new(area.x + gutter_width - 1, y, 1, 1),
            );

            // Text content
            let line_text = self.editor.line_text(buffer_row);
            let scroll_col = self.editor.scroll_col() as usize;
            let visible_text: String = line_text
                .chars()
                .skip(scroll_col)
                .take(text_width as usize)
                .collect();

            frame.render_widget(
                Paragraph::new(Span::raw(visible_text)),
                Rect::new(area.x + gutter_width, y, text_width, 1),
            );
        }

        // Position the terminal cursor (only if editor is focused and no overlay)
        if self.focus == Focus::Editor && self.file_finder.is_none() {
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
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let cursor = self.editor.cursor();
        let modified = if self.editor.is_dirty() { " [+]" } else { "" };

        let branch_span = if let Some(ref branch) = self.git_branch {
            Span::styled(
                format!("  {branch}"),
                Style::default().fg(Color::Magenta),
            )
        } else {
            Span::raw("")
        };

        let focus_indicator = match self.focus {
            Focus::Editor => "EDIT",
            Focus::FileTree => "TREE",
        };

        let status = Line::from(vec![
            Span::styled(
                format!(" {focus_indicator} "),
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
            branch_span,
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
