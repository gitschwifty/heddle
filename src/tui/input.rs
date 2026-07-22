use ratatui::layout::Position;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InputBuffer {
    pub(super) lines: Vec<String>,
    pub(super) row: usize,
    pub(super) col: usize,
}

impl Default for InputBuffer {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
        }
    }
}

impl InputBuffer {
    pub(super) fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn insert_char(&mut self, c: char) {
        let idx = char_to_byte(&self.lines[self.row], self.col);
        self.lines[self.row].insert(idx, c);
        self.col += 1;
    }

    pub(super) fn insert_newline(&mut self) {
        let idx = char_to_byte(&self.lines[self.row], self.col);
        let tail = self.lines[self.row].split_off(idx);
        self.lines.insert(self.row + 1, tail);
        self.row += 1;
        self.col = 0;
    }

    pub(super) fn backspace(&mut self) {
        if self.col > 0 {
            let end = char_to_byte(&self.lines[self.row], self.col);
            let start = char_to_byte(&self.lines[self.row], self.col - 1);
            self.lines[self.row].replace_range(start..end, "");
            self.col -= 1;
        } else if self.row > 0 {
            let current = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.lines[self.row].chars().count();
            self.lines[self.row].push_str(&current);
        }
    }

    pub(super) fn consume_trailing_backslash(&mut self) -> bool {
        if self.col == 0 {
            return false;
        }
        let line = &self.lines[self.row];
        let mut chars = line.chars();
        if chars.nth(self.col - 1) != Some('\\') {
            return false;
        }
        self.backspace();
        true
    }

    pub(super) fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.lines[self.row].chars().count();
        }
    }

    pub(super) fn move_right(&mut self) {
        let line_len = self.lines[self.row].chars().count();
        if self.col < line_len {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    pub(super) fn move_up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.clamp_col();
        }
    }

    pub(super) fn move_down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.clamp_col();
        }
    }

    pub(super) fn move_line_start(&mut self) {
        self.col = 0;
    }

    pub(super) fn move_line_end(&mut self) {
        self.col = self.lines[self.row].chars().count();
    }

    fn clamp_col(&mut self) {
        self.col = self.col.min(self.lines[self.row].chars().count());
    }

    fn visual_cursor(&self, width: u16) -> (usize, usize) {
        let inner_width = width.max(1) as usize;
        let row = self
            .lines
            .iter()
            .take(self.row)
            .map(|line| visual_line_count(line, inner_width) as usize)
            .sum::<usize>()
            + (self.col / inner_width);
        let col = self.col % inner_width;
        (row, col)
    }

    pub(super) fn input_scroll(&self, width: u16, height: u16) -> usize {
        let visible_height = height.max(1) as usize;
        let (row, _) = self.visual_cursor(width);
        row.saturating_sub(visible_height.saturating_sub(1))
    }

    pub(super) fn cursor_position(
        &self,
        origin: Position,
        width: u16,
        height: u16,
        scroll: usize,
    ) -> Position {
        let visible_height = height.max(1);
        let (row, col) = self.visual_cursor(width);
        let visible_row = row.saturating_sub(scroll);
        Position::new(
            origin.x.saturating_add(col as u16),
            origin
                .y
                .saturating_add((visible_row as u16).min(visible_height.saturating_sub(1))),
        )
    }

    pub(super) fn visual_height(&self, width: u16) -> u16 {
        let width = width.max(1) as usize;
        self.lines
            .iter()
            .map(|line| visual_line_count(line, width))
            .sum::<u16>()
            .max(1)
    }
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| s.len())
}

fn visual_line_count(line: &str, width: usize) -> u16 {
    let chars = line.chars().count();
    ((chars / width) + 1).max(1) as u16
}
