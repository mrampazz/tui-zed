use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::git::{GitFileStatus, GitStatusEntry};

/// A single entry in the file tree.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
    pub git_status: Option<GitFileStatus>,
}

/// An interactive file tree panel.
pub struct FileTree {
    root: PathBuf,
    entries: Vec<FileEntry>,
    expanded: HashSet<PathBuf>,
    selected: usize,
    visible: bool,
}

/// Result of handling a key event in the file tree.
pub enum FileTreeAction {
    None,
    OpenFile(PathBuf),
}

impl FileTree {
    /// Build a file tree rooted at the given directory.
    pub fn new(root: &Path) -> Self {
        let mut tree = Self {
            root: root.to_path_buf(),
            expanded: HashSet::new(),
            entries: Vec::new(),
            selected: 0,
            visible: true,
        };
        tree.expanded.insert(root.to_path_buf());
        tree.refresh(&[]);
        tree
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
    }

    /// Refresh the file tree entries, respecting `.gitignore`.
    pub fn refresh(&mut self, git_statuses: &[GitStatusEntry]) {
        self.entries.clear();
        self.walk_dir(&self.root.clone(), 0, git_statuses);
        if self.selected >= self.entries.len() && !self.entries.is_empty() {
            self.selected = self.entries.len() - 1;
        }
    }

    fn walk_dir(&mut self, dir: &Path, depth: usize, git_statuses: &[GitStatusEntry]) {
        let walker = ignore::WalkBuilder::new(dir)
            .max_depth(Some(1))
            .sort_by_file_name(std::cmp::Ord::cmp)
            .build();

        let mut dirs = Vec::new();
        let mut files = Vec::new();

        for entry in walker.flatten() {
            let path = entry.path().to_path_buf();
            if path == dir {
                continue;
            }

            let name = entry
                .file_name()
                .to_string_lossy()
                .into_owned();

            let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());

            let git_status = git_statuses.iter().find_map(|gs| {
                let relative = path.strip_prefix(&self.root).ok()?;
                if gs.path == relative {
                    Some(gs.status)
                } else {
                    None
                }
            });

            let entry = FileEntry {
                path,
                name,
                is_dir,
                depth,
                git_status,
            };

            if is_dir {
                dirs.push(entry);
            } else {
                files.push(entry);
            }
        }

        // Directories first, then files
        for dir_entry in dirs {
            let is_expanded = self.expanded.contains(&dir_entry.path);
            let child_path = dir_entry.path.clone();
            self.entries.push(dir_entry);
            if is_expanded {
                self.walk_dir(&child_path, depth + 1, git_statuses);
            }
        }
        for file_entry in files {
            self.entries.push(file_entry);
        }
    }

    // -- Navigation --

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    /// Handle Enter key: toggle dir or open file.
    pub fn select(&mut self) -> FileTreeAction {
        let Some(entry) = self.entries.get(self.selected) else {
            return FileTreeAction::None;
        };

        if entry.is_dir {
            let path = entry.path.clone();
            if self.expanded.contains(&path) {
                self.expanded.remove(&path);
            } else {
                self.expanded.insert(path);
            }
            self.refresh(&[]);
            FileTreeAction::None
        } else {
            FileTreeAction::OpenFile(entry.path.clone())
        }
    }

    /// Collapse current dir or move to parent.
    pub fn collapse_or_parent(&mut self) {
        let Some(entry) = self.entries.get(self.selected) else {
            return;
        };

        if entry.is_dir && self.expanded.contains(&entry.path) {
            let path = entry.path.clone();
            self.expanded.remove(&path);
            self.refresh(&[]);
        } else if self.selected > 0 {
            // Move to parent directory
            let current_depth = entry.depth;
            for i in (0..self.selected).rev() {
                if self.entries[i].is_dir && self.entries[i].depth < current_depth {
                    self.selected = i;
                    break;
                }
            }
        }
    }

    /// Expand directory or move into first child.
    pub fn expand_or_enter(&mut self) {
        let Some(entry) = self.entries.get(self.selected) else {
            return;
        };

        if entry.is_dir {
            if !self.expanded.contains(&entry.path) {
                let path = entry.path.clone();
                self.expanded.insert(path);
                self.refresh(&[]);
            }
            // Move to first child if it exists
            if self.selected + 1 < self.entries.len() {
                self.selected += 1;
            }
        }
    }

    // -- Rendering --

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if area.height < 1 || area.width < 2 {
            return;
        }

        let scroll = if self.selected >= area.height as usize {
            self.selected - area.height as usize + 1
        } else {
            0
        };

        for (screen_row, entry) in self
            .entries
            .iter()
            .skip(scroll)
            .take(area.height as usize)
            .enumerate()
        {
            let y = area.y + screen_row as u16;
            let is_selected = scroll + screen_row == self.selected;

            let indent = "  ".repeat(entry.depth);
            let icon = if entry.is_dir {
                if self.expanded.contains(&entry.path) {
                    "▼ "
                } else {
                    "▶ "
                }
            } else {
                "  "
            };

            let name_color = match entry.git_status {
                Some(GitFileStatus::Added) | Some(GitFileStatus::Untracked) => Color::Green,
                Some(GitFileStatus::Modified) => Color::Yellow,
                Some(GitFileStatus::Deleted) => Color::Red,
                Some(GitFileStatus::Renamed) => Color::Cyan,
                None => {
                    if entry.is_dir {
                        Color::Blue
                    } else {
                        Color::White
                    }
                }
            };

            let mut style = Style::default().fg(name_color);
            if is_selected {
                style = style.bg(Color::Rgb(50, 50, 80)).add_modifier(Modifier::BOLD);
            }

            let line_text = format!("{indent}{icon}{}", entry.name);
            let truncated: String = line_text.chars().take(area.width as usize).collect();

            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(truncated, style))),
                Rect::new(area.x, y, area.width, 1),
            );
        }
    }
}
