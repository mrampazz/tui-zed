use std::path::{Path, PathBuf};

use anyhow::Result;
use clock::ReplicaId;
use text::{Bias, Buffer, BufferId, Point};

/// A single open file/buffer in the TUI editor.
pub struct TuiEditor {
    buffer: Buffer,
    file_path: Option<PathBuf>,
    dirty: bool,

    /// Cursor position in buffer coordinates (row, column in bytes).
    cursor: Point,

    /// Viewport scroll offset (first visible row, first visible col).
    scroll_row: u32,
    scroll_col: u32,

    /// Viewport dimensions (set by the renderer).
    viewport_rows: u32,
    viewport_cols: u32,
}

impl TuiEditor {
    /// Create a new empty editor (no file).
    pub fn new() -> Self {
        let buffer_id = BufferId::new(1).expect("nonzero buffer id");
        Self {
            buffer: Buffer::new(ReplicaId::LOCAL, buffer_id, ""),
            file_path: None,
            dirty: false,
            cursor: Point::zero(),
            scroll_row: 0,
            scroll_col: 0,
            viewport_rows: 24,
            viewport_cols: 80,
        }
    }

    /// Open a file and load its contents into a new editor.
    pub fn open(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let buffer_id = BufferId::new(1).expect("nonzero buffer id");
        Ok(Self {
            buffer: Buffer::new(ReplicaId::LOCAL, buffer_id, content),
            file_path: Some(path.to_path_buf()),
            dirty: false,
            cursor: Point::zero(),
            scroll_row: 0,
            scroll_col: 0,
            viewport_rows: 24,
            viewport_cols: 80,
        })
    }

    /// Save the buffer contents to the file.
    pub fn save(&mut self) -> Result<()> {
        if let Some(ref path) = self.file_path {
            std::fs::write(path, self.buffer.text())?;
            self.dirty = false;
        }
        Ok(())
    }

    // -- Accessors --

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn cursor(&self) -> Point {
        self.cursor
    }

    pub fn scroll_row(&self) -> u32 {
        self.scroll_row
    }

    pub fn scroll_col(&self) -> u32 {
        self.scroll_col
    }

    pub fn file_path(&self) -> Option<&Path> {
        self.file_path.as_deref()
    }

    pub fn file_name(&self) -> &str {
        self.file_path
            .as_deref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("[scratch]")
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn row_count(&self) -> u32 {
        self.buffer.max_point().row + 1
    }

    /// Set the viewport dimensions (called by the renderer before drawing).
    pub fn set_viewport(&mut self, rows: u32, cols: u32) {
        self.viewport_rows = rows;
        self.viewport_cols = cols;
    }

    /// Range of visible rows.
    pub fn visible_rows(&self) -> std::ops::Range<u32> {
        let end = (self.scroll_row + self.viewport_rows).min(self.row_count());
        self.scroll_row..end
    }

    // -- Cursor movement --

    pub fn move_up(&mut self) {
        if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.clamp_cursor_column();
            self.ensure_cursor_visible();
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor.row < self.buffer.max_point().row {
            self.cursor.row += 1;
            self.clamp_cursor_column();
            self.ensure_cursor_visible();
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor.column > 0 {
            let offset = self.cursor_offset();
            let prev_char_offset = self.buffer.clip_offset(offset.saturating_sub(1), Bias::Left);
            self.cursor = self.buffer.offset_to_point(prev_char_offset);
            self.ensure_cursor_visible();
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.cursor.column = self.current_line_len();
            self.ensure_cursor_visible();
        }
    }

    pub fn move_right(&mut self) {
        let line_len = self.current_line_len();
        if self.cursor.column < line_len {
            let offset = self.cursor_offset();
            // Move past the current character (may be multi-byte)
            let next_offset = offset + self.char_len_at(offset);
            self.cursor = self.buffer.offset_to_point(next_offset);
            self.ensure_cursor_visible();
        } else if self.cursor.row < self.buffer.max_point().row {
            self.cursor.row += 1;
            self.cursor.column = 0;
            self.ensure_cursor_visible();
        }
    }

    pub fn move_to_line_start(&mut self) {
        self.cursor.column = 0;
        self.ensure_cursor_visible();
    }

    pub fn move_to_line_end(&mut self) {
        self.cursor.column = self.current_line_len();
        self.ensure_cursor_visible();
    }

    pub fn move_to_doc_start(&mut self) {
        self.cursor = Point::zero();
        self.ensure_cursor_visible();
    }

    pub fn move_to_doc_end(&mut self) {
        self.cursor = self.buffer.max_point();
        self.ensure_cursor_visible();
    }

    pub fn page_up(&mut self) {
        let jump = self.viewport_rows.saturating_sub(1).max(1);
        self.cursor.row = self.cursor.row.saturating_sub(jump);
        self.clamp_cursor_column();
        self.scroll_row = self.scroll_row.saturating_sub(jump);
    }

    pub fn page_down(&mut self) {
        let jump = self.viewport_rows.saturating_sub(1).max(1);
        let max_row = self.buffer.max_point().row;
        self.cursor.row = (self.cursor.row + jump).min(max_row);
        self.clamp_cursor_column();
        self.scroll_row = (self.scroll_row + jump).min(max_row.saturating_sub(self.viewport_rows / 2));
    }

    // -- Text editing --

    pub fn insert_char(&mut self, ch: char) {
        let offset = self.cursor_offset();
        let text = ch.to_string();
        self.buffer.edit([(offset..offset, text.as_str())]);
        self.dirty = true;
        // Move cursor past the inserted character
        self.cursor = self.buffer.offset_to_point(offset + ch.len_utf8());
        self.ensure_cursor_visible();
    }

    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    pub fn backspace(&mut self) {
        let offset = self.cursor_offset();
        if offset > 0 {
            let prev_offset = self.buffer.clip_offset(offset - 1, Bias::Left);
            self.buffer.edit([(prev_offset..offset, "")]);
            self.dirty = true;
            self.cursor = self.buffer.offset_to_point(prev_offset);
            self.ensure_cursor_visible();
        }
    }

    pub fn delete(&mut self) {
        let offset = self.cursor_offset();
        if offset < self.buffer.len() {
            let next_offset = offset + self.char_len_at(offset);
            self.buffer.edit([(offset..next_offset, "")]);
            self.dirty = true;
            self.clamp_cursor_column();
        }
    }

    pub fn insert_tab(&mut self) {
        let offset = self.cursor_offset();
        self.buffer.edit([(offset..offset, "    ")]);
        self.dirty = true;
        self.cursor = self.buffer.offset_to_point(offset + 4);
        self.ensure_cursor_visible();
    }

    pub fn undo(&mut self) {
        if self.buffer.undo().is_some() {
            self.dirty = true;
            self.clamp_cursor();
            self.ensure_cursor_visible();
        }
    }

    pub fn redo(&mut self) {
        if self.buffer.redo().is_some() {
            self.dirty = true;
            self.clamp_cursor();
            self.ensure_cursor_visible();
        }
    }

    // -- Text content access --

    /// Get the text of a single row (without trailing newline).
    pub fn line_text(&self, row: u32) -> String {
        if row > self.buffer.max_point().row {
            return String::new();
        }
        let start = self.buffer.point_to_offset(Point::new(row, 0));
        let line_len = self.buffer.line_len(row);
        let end = start + line_len as usize;
        self.buffer.text_for_range(start..end).collect()
    }

    // -- Internal helpers --

    fn cursor_offset(&self) -> usize {
        self.buffer.point_to_offset(self.cursor)
    }

    fn current_line_len(&self) -> u32 {
        self.buffer.line_len(self.cursor.row)
    }

    fn char_len_at(&self, offset: usize) -> usize {
        self.buffer
            .chars_at(offset)
            .next()
            .map_or(1, |ch| ch.len_utf8())
    }

    /// Clamp cursor column to the current line length.
    fn clamp_cursor_column(&mut self) {
        let max_col = self.current_line_len();
        if self.cursor.column > max_col {
            self.cursor.column = max_col;
        }
    }

    /// Clamp cursor to valid buffer position.
    fn clamp_cursor(&mut self) {
        let max = self.buffer.max_point();
        if self.cursor.row > max.row {
            self.cursor = max;
        } else {
            self.clamp_cursor_column();
        }
    }

    /// Scroll the viewport so the cursor is visible.
    fn ensure_cursor_visible(&mut self) {
        // Vertical
        if self.cursor.row < self.scroll_row {
            self.scroll_row = self.cursor.row;
        } else if self.cursor.row >= self.scroll_row + self.viewport_rows {
            self.scroll_row = self.cursor.row - self.viewport_rows + 1;
        }

        // Horizontal
        if self.cursor.column < self.scroll_col {
            self.scroll_col = self.cursor.column;
        } else if self.cursor.column >= self.scroll_col + self.viewport_cols {
            self.scroll_col = self.cursor.column - self.viewport_cols + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor_with(text: &str) -> TuiEditor {
        let buffer_id = BufferId::new(1).expect("nonzero");
        TuiEditor {
            buffer: Buffer::new(ReplicaId::LOCAL, buffer_id, text),
            file_path: None,
            dirty: false,
            cursor: Point::zero(),
            scroll_row: 0,
            scroll_col: 0,
            viewport_rows: 24,
            viewport_cols: 80,
        }
    }

    #[test]
    fn test_insert_and_cursor_moves() {
        let mut editor = editor_with("");
        editor.insert_char('H');
        editor.insert_char('i');
        assert_eq!(editor.buffer.text(), "Hi");
        assert_eq!(editor.cursor, Point::new(0, 2));
    }

    #[test]
    fn test_backspace() {
        let mut editor = editor_with("Hello");
        editor.cursor = Point::new(0, 5);
        editor.backspace();
        assert_eq!(editor.buffer.text(), "Hell");
        assert_eq!(editor.cursor, Point::new(0, 4));
    }

    #[test]
    fn test_delete() {
        let mut editor = editor_with("Hello");
        editor.cursor = Point::new(0, 0);
        editor.delete();
        assert_eq!(editor.buffer.text(), "ello");
        assert_eq!(editor.cursor, Point::new(0, 0));
    }

    #[test]
    fn test_newline() {
        let mut editor = editor_with("Hello");
        editor.cursor = Point::new(0, 5);
        editor.insert_newline();
        assert_eq!(editor.buffer.text(), "Hello\n");
        assert_eq!(editor.cursor, Point::new(1, 0));
    }

    #[test]
    fn test_move_up_down() {
        let mut editor = editor_with("line1\nline2\nline3");
        editor.cursor = Point::new(1, 2);

        editor.move_up();
        assert_eq!(editor.cursor, Point::new(0, 2));

        editor.move_down();
        assert_eq!(editor.cursor, Point::new(1, 2));

        editor.move_down();
        assert_eq!(editor.cursor, Point::new(2, 2));

        editor.move_down(); // at last line, shouldn't move
        assert_eq!(editor.cursor, Point::new(2, 2));
    }

    #[test]
    fn test_move_left_right() {
        let mut editor = editor_with("AB");
        editor.cursor = Point::new(0, 0);

        editor.move_right();
        assert_eq!(editor.cursor, Point::new(0, 1));

        editor.move_right();
        assert_eq!(editor.cursor, Point::new(0, 2));

        editor.move_left();
        assert_eq!(editor.cursor, Point::new(0, 1));

        editor.move_left();
        assert_eq!(editor.cursor, Point::new(0, 0));

        editor.move_left(); // at beginning, shouldn't move
        assert_eq!(editor.cursor, Point::new(0, 0));
    }

    #[test]
    fn test_move_left_wraps_to_previous_line() {
        let mut editor = editor_with("AB\nCD");
        editor.cursor = Point::new(1, 0);

        editor.move_left();
        assert_eq!(editor.cursor, Point::new(0, 2));
    }

    #[test]
    fn test_move_right_wraps_to_next_line() {
        let mut editor = editor_with("AB\nCD");
        editor.cursor = Point::new(0, 2);

        editor.move_right();
        assert_eq!(editor.cursor, Point::new(1, 0));
    }

    #[test]
    fn test_clamp_cursor_on_shorter_line() {
        let mut editor = editor_with("long line\nhi");
        editor.cursor = Point::new(0, 9);

        editor.move_down();
        // "hi" is only 2 chars, cursor should clamp
        assert_eq!(editor.cursor, Point::new(1, 2));
    }

    #[test]
    fn test_line_start_end() {
        let mut editor = editor_with("Hello, world!");
        editor.cursor = Point::new(0, 5);

        editor.move_to_line_end();
        assert_eq!(editor.cursor.column, 13);

        editor.move_to_line_start();
        assert_eq!(editor.cursor.column, 0);
    }

    #[test]
    fn test_undo_redo() {
        let mut editor = editor_with("Hello");
        editor.cursor = Point::new(0, 5);
        editor.insert_char('!');
        assert_eq!(editor.buffer.text(), "Hello!");

        editor.undo();
        assert_eq!(editor.buffer.text(), "Hello");

        editor.redo();
        assert_eq!(editor.buffer.text(), "Hello!");
    }

    #[test]
    fn test_line_text() {
        let editor = editor_with("first\nsecond\nthird");
        assert_eq!(editor.line_text(0), "first");
        assert_eq!(editor.line_text(1), "second");
        assert_eq!(editor.line_text(2), "third");
        assert_eq!(editor.line_text(99), "");
    }

    #[test]
    fn test_scroll_follows_cursor_down() {
        let mut editor = editor_with(&"x\n".repeat(50));
        editor.set_viewport(10, 80);

        for _ in 0..15 {
            editor.move_down();
        }
        // Cursor is on row 15, viewport is 10 rows
        assert_eq!(editor.cursor.row, 15);
        assert!(editor.scroll_row > 0);
        assert!(editor.cursor.row < editor.scroll_row + editor.viewport_rows);
    }
}
