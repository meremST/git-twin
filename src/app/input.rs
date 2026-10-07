//! Text editing widgets state: single-line input and multi-line editor.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

fn byte_at(s: &str, ch: usize) -> usize {
    s.char_indices().nth(ch).map(|(i, _)| i).unwrap_or(s.len())
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Previous grapheme boundary (in chars) before `ch`.
pub(crate) fn prev_boundary(s: &str, ch: usize) -> usize {
    let b = byte_at(s, ch);
    let mut last = 0;
    for (i, _) in s.grapheme_indices(true) {
        if i >= b {
            break;
        }
        last = i;
    }
    s[..last].chars().count()
}

pub(crate) fn next_boundary(s: &str, ch: usize) -> usize {
    let b = byte_at(s, ch);
    for (i, g) in s.grapheme_indices(true) {
        if i >= b {
            return s[..i + g.len()].chars().count();
        }
    }
    char_len(s)
}

fn word_left(s: &str, ch: usize) -> usize {
    let chars: Vec<char> = s.chars().collect();
    let mut i = ch.min(chars.len());
    while i > 0 && !chars[i - 1].is_alphanumeric() {
        i -= 1;
    }
    while i > 0 && chars[i - 1].is_alphanumeric() {
        i -= 1;
    }
    i
}

fn word_right(s: &str, ch: usize) -> usize {
    let chars: Vec<char> = s.chars().collect();
    let mut i = ch;
    while i < chars.len() && !chars[i].is_alphanumeric() {
        i += 1;
    }
    while i < chars.len() && chars[i].is_alphanumeric() {
        i += 1;
    }
    i
}

/// Display width of the first `ch` chars.
pub fn width_upto(s: &str, ch: usize) -> usize {
    s[..byte_at(s, ch)].width()
}

/// Char index at display column `col`.
pub fn char_at_col(s: &str, col: usize) -> usize {
    let mut w = 0;
    for (i, g) in s.grapheme_indices(true) {
        let cw = g.width();
        if w + cw > col {
            return s[..i].chars().count();
        }
        w += cw;
    }
    char_len(s)
}

#[derive(Clone, Debug, Default)]
pub struct TextInput {
    pub text: String,
    /// Cursor position in chars.
    pub cursor: usize,
    /// First visible display column.
    pub scroll: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Insert(char),
    Backspace,
    Delete,
    Left,
    Right,
    WordLeft,
    WordRight,
    Home,
    End,
    DeleteWordLeft,
    ClearToStart,
    ClearToEnd,
}

impl TextInput {
    pub fn new(s: &str) -> TextInput {
        TextInput {
            text: s.to_string(),
            cursor: char_len(s),
            scroll: 0,
        }
    }

    pub fn set(&mut self, s: &str) {
        self.text = s.replace(['\n', '\r'], " ");
        self.cursor = char_len(&self.text);
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.scroll = 0;
    }

    pub fn insert_str(&mut self, s: &str) {
        let clean: String = s
            .chars()
            .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
            .filter(|c| !c.is_control())
            .collect();
        let b = byte_at(&self.text, self.cursor);
        self.text.insert_str(b, &clean);
        self.cursor += char_len(&clean);
    }

    pub fn apply(&mut self, e: Edit) {
        let len = char_len(&self.text);
        self.cursor = self.cursor.min(len);
        match e {
            Edit::Insert(c) => {
                if !c.is_control() {
                    let b = byte_at(&self.text, self.cursor);
                    self.text.insert(b, c);
                    self.cursor += 1;
                }
            }
            Edit::Backspace => {
                if self.cursor > 0 {
                    let p = prev_boundary(&self.text, self.cursor);
                    let (a, b) = (byte_at(&self.text, p), byte_at(&self.text, self.cursor));
                    self.text.replace_range(a..b, "");
                    self.cursor = p;
                }
            }
            Edit::Delete => {
                if self.cursor < len {
                    let n = next_boundary(&self.text, self.cursor);
                    let (a, b) = (byte_at(&self.text, self.cursor), byte_at(&self.text, n));
                    self.text.replace_range(a..b, "");
                }
            }
            Edit::Left => self.cursor = prev_boundary(&self.text, self.cursor),
            Edit::Right => self.cursor = next_boundary(&self.text, self.cursor),
            Edit::WordLeft => self.cursor = word_left(&self.text, self.cursor),
            Edit::WordRight => self.cursor = word_right(&self.text, self.cursor),
            Edit::Home => self.cursor = 0,
            Edit::End => self.cursor = len,
            Edit::DeleteWordLeft => {
                let p = word_left(&self.text, self.cursor);
                let (a, b) = (byte_at(&self.text, p), byte_at(&self.text, self.cursor));
                self.text.replace_range(a..b, "");
                self.cursor = p;
            }
            Edit::ClearToStart => {
                let b = byte_at(&self.text, self.cursor);
                self.text.replace_range(..b, "");
                self.cursor = 0;
            }
            Edit::ClearToEnd => {
                let b = byte_at(&self.text, self.cursor);
                self.text.truncate(b);
            }
        }
    }

    /// Adjusts horizontal scroll for a field of `width` columns and returns
    /// (visible text start char, cursor column within the field).
    pub fn view(&mut self, width: usize) -> (usize, usize) {
        let width = width.max(1);
        let cur_col = width_upto(&self.text, self.cursor);
        if cur_col < self.scroll {
            self.scroll = cur_col;
        }
        if cur_col >= self.scroll + width {
            self.scroll = cur_col + 1 - width;
        }
        let start = char_at_col(&self.text, self.scroll);
        (start, cur_col - width_upto(&self.text, start))
    }

    pub fn click(&mut self, col: usize) {
        let start = char_at_col(&self.text, self.scroll);
        let base = width_upto(&self.text, start);
        self.cursor = char_at_col(&self.text, base + col);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Debug)]
struct Snapshot {
    lines: Vec<String>,
    cursor: Pos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditGroup {
    Typing,
    Deleting,
    Other,
}

/// Multi-line editor used for commit messages.
#[derive(Clone, Debug)]
pub struct TextArea {
    pub lines: Vec<String>,
    pub cursor: Pos,
    pub anchor: Option<Pos>,
    pub top: usize,
    pub left: usize,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_group: Option<EditGroup>,
    /// Desired display column when moving vertically.
    goal_col: Option<usize>,
    pub modified: bool,
}

impl Default for TextArea {
    fn default() -> Self {
        TextArea::new("")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    Home,
    End,
    PageUp(usize),
    PageDown(usize),
    DocStart,
    DocEnd,
}

impl TextArea {
    pub fn new(s: &str) -> TextArea {
        let mut t = TextArea {
            lines: vec![String::new()],
            cursor: Pos::default(),
            anchor: None,
            top: 0,
            left: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            last_group: None,
            goal_col: None,
            modified: false,
        };
        t.set_text_raw(s);
        t
    }

    fn set_text_raw(&mut self, s: &str) {
        let s = s.replace("\r\n", "\n");
        self.lines = s.split('\n').map(str::to_string).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor = Pos::default();
        self.anchor = None;
        self.top = 0;
        self.left = 0;
    }

    /// Replaces the whole text (undoable).
    pub fn set_text(&mut self, s: &str) {
        self.snapshot(EditGroup::Other);
        self.set_text_raw(s);
        self.modified = true;
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.trim().is_empty())
    }

    fn snapshot(&mut self, g: EditGroup) {
        if g != EditGroup::Other && self.last_group == Some(g) {
            return;
        }
        self.undo.push(Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
        });
        if self.undo.len() > 500 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.last_group = Some(g);
    }

    pub fn break_undo_group(&mut self) {
        self.last_group = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(Snapshot {
                lines: std::mem::take(&mut self.lines),
                cursor: self.cursor,
            });
            self.lines = s.lines;
            self.cursor = s.cursor;
            self.anchor = None;
            self.last_group = None;
            self.modified = true;
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(Snapshot {
                lines: std::mem::take(&mut self.lines),
                cursor: self.cursor,
            });
            self.lines = s.lines;
            self.cursor = s.cursor;
            self.anchor = None;
            self.last_group = None;
            self.modified = true;
        }
    }

    fn clamp(&mut self, p: Pos) -> Pos {
        let row = p.row.min(self.lines.len() - 1);
        Pos {
            row,
            col: p.col.min(char_len(&self.lines[row])),
        }
    }

    pub fn selection(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        if a == self.cursor {
            return None;
        }
        Some(if a < self.cursor {
            (a, self.cursor)
        } else {
            (self.cursor, a)
        })
    }

    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection()?;
        Some(self.range_text(a, b))
    }

    fn range_text(&self, a: Pos, b: Pos) -> String {
        if a.row == b.row {
            let l = &self.lines[a.row];
            return l[byte_at(l, a.col)..byte_at(l, b.col)].to_string();
        }
        let mut s = String::new();
        let first = &self.lines[a.row];
        s.push_str(&first[byte_at(first, a.col)..]);
        for r in a.row + 1..b.row {
            s.push('\n');
            s.push_str(&self.lines[r]);
        }
        s.push('\n');
        let last = &self.lines[b.row];
        s.push_str(&last[..byte_at(last, b.col)]);
        s
    }

    fn delete_range(&mut self, a: Pos, b: Pos) {
        let tail = {
            let l = &self.lines[b.row];
            l[byte_at(l, b.col)..].to_string()
        };
        let head = {
            let l = &self.lines[a.row];
            l[..byte_at(l, a.col)].to_string()
        };
        self.lines.drain(a.row + 1..=b.row);
        self.lines[a.row] = head + &tail;
        self.cursor = a;
        self.anchor = None;
    }

    /// Deletes the selection, if any. Returns whether something was deleted.
    pub fn delete_selection(&mut self) -> bool {
        if let Some((a, b)) = self.selection() {
            self.snapshot(EditGroup::Other);
            self.delete_range(a, b);
            self.modified = true;
            true
        } else {
            self.anchor = None;
            false
        }
    }

    pub fn insert_char(&mut self, c: char) {
        if c == '\n' {
            self.newline();
            return;
        }
        if c.is_control() && c != '\t' {
            return;
        }
        if self.selection().is_some() {
            self.delete_selection();
        }
        self.anchor = None;
        self.snapshot(if c == ' ' {
            EditGroup::Other
        } else {
            EditGroup::Typing
        });
        if c == ' ' {
            self.last_group = Some(EditGroup::Typing);
        }
        let l = &mut self.lines[self.cursor.row];
        l.insert(byte_at(l, self.cursor.col), c);
        self.cursor.col += 1;
        self.goal_col = None;
        self.modified = true;
    }

    pub fn newline(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
        }
        self.snapshot(EditGroup::Other);
        let l = &mut self.lines[self.cursor.row];
        let tail = l.split_off(byte_at(l, self.cursor.col));
        self.lines.insert(self.cursor.row + 1, tail);
        self.cursor = Pos {
            row: self.cursor.row + 1,
            col: 0,
        };
        self.anchor = None;
        self.goal_col = None;
        self.modified = true;
    }

    pub fn insert_str(&mut self, s: &str) {
        if self.selection().is_some() {
            self.delete_selection();
        }
        self.snapshot(EditGroup::Other);
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        let clean: String = s
            .chars()
            .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
            .collect();
        let l = &mut self.lines[self.cursor.row];
        let tail = l.split_off(byte_at(l, self.cursor.col));
        let mut parts = clean.split('\n');
        let first = parts.next().unwrap_or("");
        self.lines[self.cursor.row].push_str(first);
        let mut row = self.cursor.row;
        let mut col = char_len(&self.lines[row]);
        for p in parts {
            row += 1;
            self.lines.insert(row, p.to_string());
            col = char_len(p);
        }
        self.lines[row].push_str(&tail);
        self.cursor = Pos { row, col };
        self.anchor = None;
        self.last_group = None;
        self.goal_col = None;
        self.modified = true;
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor.col > 0 {
            self.snapshot(EditGroup::Deleting);
            let l = &mut self.lines[self.cursor.row];
            let p = prev_boundary(l, self.cursor.col);
            let (a, b) = (byte_at(l, p), byte_at(l, self.cursor.col));
            l.replace_range(a..b, "");
            self.cursor.col = p;
        } else if self.cursor.row > 0 {
            self.snapshot(EditGroup::Other);
            let l = self.lines.remove(self.cursor.row);
            self.cursor.row -= 1;
            self.cursor.col = char_len(&self.lines[self.cursor.row]);
            self.lines[self.cursor.row].push_str(&l);
        }
        self.goal_col = None;
        self.modified = true;
    }

    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        let len = char_len(&self.lines[self.cursor.row]);
        if self.cursor.col < len {
            self.snapshot(EditGroup::Deleting);
            let l = &mut self.lines[self.cursor.row];
            let n = next_boundary(l, self.cursor.col);
            let (a, b) = (byte_at(l, self.cursor.col), byte_at(l, n));
            l.replace_range(a..b, "");
        } else if self.cursor.row + 1 < self.lines.len() {
            self.snapshot(EditGroup::Other);
            let l = self.lines.remove(self.cursor.row + 1);
            self.lines[self.cursor.row].push_str(&l);
        }
        self.modified = true;
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(Pos::default());
        let row = self.lines.len() - 1;
        self.cursor = Pos {
            row,
            col: char_len(&self.lines[row]),
        };
    }

    pub fn move_cursor(&mut self, m: Move, select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.last_group = None;
        let line = &self.lines[self.cursor.row];
        let cur_w = width_upto(line, self.cursor.col);
        let goal = self.goal_col.unwrap_or(cur_w);
        let mut keep_goal = false;
        let p = match m {
            Move::Left => {
                if self.cursor.col > 0 {
                    Pos {
                        col: prev_boundary(line, self.cursor.col),
                        ..self.cursor
                    }
                } else if self.cursor.row > 0 {
                    let r = self.cursor.row - 1;
                    Pos {
                        row: r,
                        col: char_len(&self.lines[r]),
                    }
                } else {
                    self.cursor
                }
            }
            Move::Right => {
                if self.cursor.col < char_len(line) {
                    Pos {
                        col: next_boundary(line, self.cursor.col),
                        ..self.cursor
                    }
                } else if self.cursor.row + 1 < self.lines.len() {
                    Pos {
                        row: self.cursor.row + 1,
                        col: 0,
                    }
                } else {
                    self.cursor
                }
            }
            Move::WordLeft => Pos {
                col: word_left(line, self.cursor.col),
                ..self.cursor
            },
            Move::WordRight => Pos {
                col: word_right(line, self.cursor.col),
                ..self.cursor
            },
            Move::Home => Pos {
                col: 0,
                ..self.cursor
            },
            Move::End => Pos {
                col: char_len(line),
                ..self.cursor
            },
            Move::Up | Move::Down | Move::PageUp(_) | Move::PageDown(_) => {
                keep_goal = true;
                let row = match m {
                    Move::Up => self.cursor.row.saturating_sub(1),
                    Move::Down => (self.cursor.row + 1).min(self.lines.len() - 1),
                    Move::PageUp(n) => self.cursor.row.saturating_sub(n.max(1)),
                    Move::PageDown(n) => (self.cursor.row + n.max(1)).min(self.lines.len() - 1),
                    _ => unreachable!(),
                };
                Pos {
                    row,
                    col: char_at_col(&self.lines[row], goal),
                }
            }
            Move::DocStart => Pos::default(),
            Move::DocEnd => {
                let r = self.lines.len() - 1;
                Pos {
                    row: r,
                    col: char_len(&self.lines[r]),
                }
            }
        };
        self.cursor = self.clamp(p);
        self.goal_col = if keep_goal { Some(goal) } else { None };
    }

    /// Places the cursor at a display position relative to the viewport.
    pub fn click(&mut self, vrow: usize, vcol: usize, select: bool) {
        let row = (self.top + vrow).min(self.lines.len() - 1);
        let col = char_at_col(&self.lines[row], self.left + vcol);
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = Pos { row, col };
        self.goal_col = None;
        self.last_group = None;
    }

    /// Keeps the cursor visible in a viewport of the given size.
    pub fn scroll_to_cursor(&mut self, height: usize, width: usize) {
        let h = height.max(1);
        if self.cursor.row < self.top {
            self.top = self.cursor.row;
        } else if self.cursor.row >= self.top + h {
            self.top = self.cursor.row + 1 - h;
        }
        let w = width.max(1);
        let cx = width_upto(&self.lines[self.cursor.row], self.cursor.col);
        if cx < self.left {
            self.left = cx;
        } else if cx >= self.left + w {
            self.left = cx + 1 - w;
        }
    }

    pub fn scroll(&mut self, delta: isize, height: usize) {
        let max = self.lines.len().saturating_sub(height.max(1));
        self.top = (self.top as isize + delta).clamp(0, max as isize) as usize;
    }

    /// Appends a `Signed-off-by` trailer like git gui's Sign Off button.
    pub fn sign_off(&mut self, ident: &str) {
        let line = format!("Signed-off-by: {ident}");
        if self.lines.iter().any(|l| l.trim_end() == line) {
            return;
        }
        self.snapshot(EditGroup::Other);
        while self.lines.len() > 1 && self.lines.last().is_some_and(|l| l.trim().is_empty()) {
            self.lines.pop();
        }
        let last_is_trailer = self
            .lines
            .last()
            .is_some_and(|l| l.contains(": ") && !l.starts_with(' ') && l.split(": ").next().is_some_and(|k| !k.contains(' ')));
        if !(self.lines.len() == 1 && self.lines[0].is_empty()) && !last_is_trailer {
            self.lines.push(String::new());
        }
        if self.lines.len() == 1 && self.lines[0].is_empty() {
            self.lines[0] = line;
        } else {
            self.lines.push(line);
        }
        let r = self.lines.len() - 1;
        self.cursor = Pos {
            row: r,
            col: char_len(&self.lines[r]),
        };
        self.anchor = None;
        self.modified = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_editing_unicode() {
        let mut t = TextInput::new("héllo");
        t.apply(Edit::Backspace);
        assert_eq!(t.text, "héll");
        t.apply(Edit::Home);
        t.apply(Edit::Right);
        t.apply(Edit::Delete);
        assert_eq!(t.text, "hll");
        t.insert_str("a\nb\x1b");
        assert_eq!(t.text, "ha bll");
        let mut e = TextInput::new("e\u{301}x");
        e.apply(Edit::End);
        e.apply(Edit::Left);
        e.apply(Edit::Backspace);
        assert_eq!(e.text, "x");
    }

    #[test]
    fn input_view_scrolls() {
        let mut t = TextInput::new("abcdefghij");
        let (start, col) = t.view(4);
        assert_eq!((start, col), (7, 3));
        t.apply(Edit::Home);
        assert_eq!(t.view(4), (0, 0));
        let mut w = TextInput::new("日本語");
        assert_eq!(w.view(10).1, 6);
    }

    #[test]
    fn area_undo_redo_groups() {
        let mut a = TextArea::new("");
        for c in "abc".chars() {
            a.insert_char(c);
        }
        a.insert_char(' ');
        for c in "de".chars() {
            a.insert_char(c);
        }
        assert_eq!(a.text(), "abc de");
        a.undo();
        assert_eq!(a.text(), "abc");
        a.undo();
        assert_eq!(a.text(), "");
        a.redo();
        assert_eq!(a.text(), "abc");
    }

    #[test]
    fn area_selection_and_paste() {
        let mut a = TextArea::new("line1\nline2\nline3");
        a.move_cursor(Move::Down, false);
        a.move_cursor(Move::End, true);
        assert_eq!(a.selected_text().unwrap(), "line2");
        a.insert_str("x\ny");
        assert_eq!(a.text(), "line1\nx\ny\nline3");
        a.select_all();
        a.backspace();
        assert_eq!(a.text(), "");
        a.undo();
        assert_eq!(a.text(), "line1\nx\ny\nline3");
    }

    #[test]
    fn sign_off_appends_trailer() {
        let mut a = TextArea::new("Subject\n\nBody\n");
        a.sign_off("A U Thor <a@b>");
        assert_eq!(a.text(), "Subject\n\nBody\n\nSigned-off-by: A U Thor <a@b>");
        a.sign_off("A U Thor <a@b>");
        assert_eq!(a.text().matches("Signed-off-by").count(), 1);
        a.sign_off("Other <o@b>");
        assert!(a.text().ends_with("<a@b>\nSigned-off-by: Other <o@b>"));
        let mut e = TextArea::new("");
        e.sign_off("X <x@y>");
        assert_eq!(e.text(), "Signed-off-by: X <x@y>");
    }
}
