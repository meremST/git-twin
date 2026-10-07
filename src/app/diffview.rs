//! Display model for diff panes (gitk bottom-left pane, git gui diff).

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::domain::diff::{Diff, LineKind};
use crate::domain::sanitize;
use crate::domain::textmatch::Matcher;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DKind {
    /// Commit header (Author:, Committer:, ...).
    Header,
    /// Commit message line.
    Message,
    /// Highlighted file title.
    FileTitle,
    /// `diff --git`, `index`, `---`, `+++`, mode lines.
    Meta,
    Hunk,
    Ctx,
    Add,
    Del,
    /// Word diff line (segments carry the colours).
    Words,
    Info,
    Error,
}

#[derive(Clone, Debug)]
pub struct DLine {
    pub kind: DKind,
    pub text: String,
    pub segs: Option<Vec<(DKind, String)>>,
    pub file: Option<usize>,
    pub hunk: Option<usize>,
    /// Index of the line inside the hunk.
    pub line: Option<usize>,
}

impl DLine {
    pub fn plain(kind: DKind, text: impl Into<String>) -> DLine {
        DLine {
            kind,
            text: text.into(),
            segs: None,
            file: None,
            hunk: None,
            line: None,
        }
    }
}

/// gitk "Diff / Old version / New version".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Show {
    #[default]
    Diff,
    Old,
    New,
}

impl Show {
    pub fn label(self) -> &'static str {
        match self {
            Show::Diff => "Diff",
            Show::Old => "Old version",
            Show::New => "New version",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WordStyle {
    #[default]
    Markup,
    Color,
}

#[derive(Clone, Debug, Default)]
pub struct DiffView {
    pub lines: Vec<DLine>,
    pub diff: Option<Arc<Diff>>,
    /// First display line of each file.
    pub file_starts: Vec<usize>,
    pub top: usize,
    pub left: usize,
    pub cursor: usize,
    pub anchor: Option<usize>,
    /// Character-boundary selection for copying, independent of patch-line selection.
    pub text_selection: Option<(super::input::Pos, super::input::Pos)>,
    pub search: Option<Matcher>,
    pub search_hits: Vec<usize>,
    /// Last known viewport height (updated at render time).
    pub height: usize,
    pub width: usize,
}

pub struct BuildOpts {
    pub tab: usize,
    pub show: Show,
    pub words: WordStyle,
    /// Hide `diff --git`/`index` noise like git gui does.
    pub compact_headers: bool,
}

fn seg_kind(k: LineKind) -> DKind {
    match k {
        LineKind::Add => DKind::Add,
        LineKind::Del => DKind::Del,
        LineKind::Context => DKind::Ctx,
    }
}

impl DiffView {
    pub fn clear(&mut self) {
        *self = DiffView {
            height: self.height,
            width: self.width,
            search: self.search.take(),
            ..Default::default()
        };
    }

    pub fn set_message(&mut self, kind: DKind, msg: &str) {
        self.clear();
        for l in msg.lines() {
            self.lines.push(DLine::plain(kind, sanitize::sanitize(l)));
        }
    }

    /// Replaces the content; `prefix` lines (commit header) come first.
    pub fn set(&mut self, prefix: Vec<DLine>, diff: Option<Arc<Diff>>, o: &BuildOpts) {
        let keep_top = self.top;
        self.clear();
        self.lines = prefix;
        if let Some(d) = &diff {
            for (fi, f) in d.files.iter().enumerate() {
                self.file_starts.push(self.lines.len());
                let mut title = DLine::plain(DKind::FileTitle, f.display_path());
                title.file = Some(fi);
                self.lines.push(title);
                for h in &f.header {
                    let skip = o.compact_headers
                        && (h.starts_with(b"diff ")
                            || h.starts_with(b"index ")
                            || h.starts_with(b"--- ")
                            || h.starts_with(b"+++ "));
                    if skip {
                        continue;
                    }
                    let mut l = DLine::plain(DKind::Meta, sanitize::display_bytes(h));
                    l.file = Some(fi);
                    self.lines.push(l);
                }
                if f.binary {
                    let mut l = DLine::plain(DKind::Info, "Binary files differ");
                    l.file = Some(fi);
                    self.lines.push(l);
                }
                for (hi, h) in f.hunks.iter().enumerate() {
                    let mut hl = DLine::plain(DKind::Hunk, sanitize::display_bytes(&h.header));
                    hl.file = Some(fi);
                    hl.hunk = Some(hi);
                    self.lines.push(hl);
                    if !h.words.is_empty() {
                        for w in &h.words {
                            let segs: Vec<(DKind, String)> = w
                                .iter()
                                .filter(|(k, _)| match o.show {
                                    Show::Diff => true,
                                    Show::Old => *k != LineKind::Add,
                                    Show::New => *k != LineKind::Del,
                                })
                                .map(|(k, t)| {
                                    let mut s = sanitize::display_bytes_tabs(t, o.tab);
                                    if o.words == WordStyle::Markup {
                                        s = match k {
                                            LineKind::Add => format!("{{+{s}+}}"),
                                            LineKind::Del => format!("[-{s}-]"),
                                            LineKind::Context => s,
                                        };
                                    }
                                    (seg_kind(*k), s)
                                })
                                .collect();
                            let text: String = segs.iter().map(|(_, s)| s.as_str()).collect();
                            self.lines.push(DLine {
                                kind: DKind::Words,
                                text,
                                segs: Some(segs),
                                file: Some(fi),
                                hunk: Some(hi),
                                line: None,
                            });
                        }
                        continue;
                    }
                    for (li, dl) in h.lines.iter().enumerate() {
                        let keep = match o.show {
                            Show::Diff => true,
                            Show::Old => dl.kind != LineKind::Add,
                            Show::New => dl.kind != LineKind::Del,
                        };
                        if !keep {
                            continue;
                        }
                        let mut text = if o.show == Show::Diff {
                            String::from_utf8_lossy(&dl.prefix).into_owned()
                        } else {
                            String::new()
                        };
                        text.push_str(&sanitize::display_bytes_tabs(&dl.text, o.tab));
                        self.lines.push(DLine {
                            kind: seg_kind(dl.kind),
                            text,
                            segs: None,
                            file: Some(fi),
                            hunk: Some(hi),
                            line: Some(li),
                        });
                        if dl.no_eol && o.show == Show::Diff {
                            let mut n = DLine::plain(DKind::Meta, "\\ No newline at end of file");
                            n.file = Some(fi);
                            n.hunk = Some(hi);
                            self.lines.push(n);
                        }
                    }
                }
                if f.truncated {
                    self.lines
                        .push(DLine::plain(DKind::Error, "[diff truncated]"));
                }
            }
            if d.truncated {
                self.lines.push(DLine::plain(
                    DKind::Error,
                    "[output truncated: diff too large to display completely]",
                ));
            }
        }
        self.diff = diff;
        self.top = keep_top.min(self.lines.len().saturating_sub(1));
        self.recompute_search();
    }

    pub fn reset_scroll(&mut self) {
        self.top = 0;
        self.left = 0;
        self.cursor = 0;
        self.anchor = None;
        self.text_selection = None;
    }

    pub fn max_top(&self) -> usize {
        self.lines.len().saturating_sub(self.height.max(1))
    }

    pub fn scroll(&mut self, delta: isize) {
        self.top = (self.top as isize + delta).clamp(0, self.max_top() as isize) as usize;
    }

    pub fn scroll_h(&mut self, delta: isize) {
        use unicode_width::UnicodeWidthStr;
        let max = self.lines.iter().map(|l| l.text.width()).max().unwrap_or(0);
        self.left = (self.left as isize + delta).clamp(0, max as isize) as usize;
    }

    pub fn ensure_visible(&mut self, idx: usize) {
        let h = self.height.max(1);
        if idx < self.top {
            self.top = idx;
        } else if idx >= self.top + h {
            self.top = idx + 1 - h;
        }
    }

    pub fn set_cursor(&mut self, idx: usize, extend: bool) {
        self.text_selection = None;
        if self.lines.is_empty() {
            return;
        }
        let idx = idx.min(self.lines.len() - 1);
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = idx;
        self.ensure_visible(idx);
    }

    /// Selected display line range (inclusive), defaulting to the cursor.
    pub fn sel_range(&self) -> (usize, usize) {
        match self.anchor {
            Some(a) => (a.min(self.cursor), a.max(self.cursor)),
            None => (self.cursor, self.cursor),
        }
    }

    pub fn has_range(&self) -> bool {
        self.anchor.is_some_and(|a| a != self.cursor)
    }

    pub fn is_selected(&self, idx: usize) -> bool {
        let (a, b) = self.sel_range();
        self.anchor.is_some() && idx >= a && idx <= b
    }

    pub fn selected_text(&self) -> String {
        if let Some((a, b)) = self.text_range() {
            return self.lines[a.row..=b.row]
                .iter()
                .enumerate()
                .map(|(offset, line)| {
                    let row = a.row + offset;
                    let start = if row == a.row { a.col } else { 0 };
                    let end = if row == b.row {
                        b.col
                    } else {
                        line.text.chars().count()
                    };
                    line.text
                        .chars()
                        .skip(start)
                        .take(end.saturating_sub(start))
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
        let (a, b) = self.sel_range();
        self.lines
            .get(a..=b.min(self.lines.len().saturating_sub(1)))
            .unwrap_or_default()
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// File/hunk under a display line.
    pub fn hunk_at(&self, idx: usize) -> Option<(usize, usize)> {
        let l = self.lines.get(idx)?;
        Some((l.file?, l.hunk?))
    }

    /// (file, {(hunk, line)}) for the selected changed lines. Lines from a
    /// single file only (the first one in the selection).
    pub fn selected_lines(&self) -> Option<(usize, BTreeSet<(usize, usize)>)> {
        if self.text_range().is_some() {
            return None;
        }
        let (a, b) = self.sel_range();
        let mut file = None;
        let mut set = BTreeSet::new();
        for l in self
            .lines
            .get(a..=b.min(self.lines.len().saturating_sub(1)))?
        {
            if let (Some(f), Some(h), Some(li)) = (l.file, l.hunk, l.line) {
                if !matches!(l.kind, DKind::Add | DKind::Del) {
                    continue;
                }
                match file {
                    None => file = Some(f),
                    Some(ff) if ff != f => continue,
                    _ => {}
                }
                set.insert((h, li));
            }
        }
        Some((file?, set)).filter(|(_, s)| !s.is_empty())
    }

    pub fn text_range(&self) -> Option<(super::input::Pos, super::input::Pos)> {
        let (a, b) = self.text_selection?;
        (a != b && a.row < self.lines.len() && b.row < self.lines.len())
            .then_some((a.min(b), a.max(b)))
    }

    pub fn text_click(&mut self, row: usize, column: usize, extend: bool) {
        use super::input::{Pos, char_at_col};
        let Some(line) = self.lines.get(row) else {
            return;
        };
        let point = Pos {
            row,
            col: char_at_col(&line.text, self.left + column),
        };
        let anchor = if extend {
            self.text_selection.map_or(point, |(a, _)| a)
        } else {
            point
        };
        self.text_selection = Some((anchor, point));
        self.anchor = None;
        self.cursor = row;
        self.ensure_visible(row);
    }

    pub fn text_move(&mut self, right: bool) {
        use super::input::{Pos, next_boundary, prev_boundary};
        if self.lines.is_empty() {
            return;
        }
        let (anchor, mut point) = self.text_selection.unwrap_or_else(|| {
            let p = Pos {
                row: self.cursor.min(self.lines.len() - 1),
                col: 0,
            };
            (p, p)
        });
        let text = &self.lines[point.row].text;
        if right {
            if point.col < text.chars().count() {
                point.col = next_boundary(text, point.col);
            } else if point.row + 1 < self.lines.len() {
                point.row += 1;
                point.col = 0;
            }
        } else if point.col > 0 {
            point.col = prev_boundary(text, point.col);
        } else if point.row > 0 {
            point.row -= 1;
            point.col = self.lines[point.row].text.chars().count();
        }
        self.text_selection = Some((anchor, point));
        self.anchor = None;
        self.cursor = point.row;
        self.ensure_visible(point.row);
        let column = super::input::width_upto(&self.lines[point.row].text, point.col);
        if column < self.left {
            self.left = column;
        } else if column >= self.left + self.width.max(1) {
            self.left = column + 1 - self.width.max(1);
        }
    }

    pub fn file_at(&self, idx: usize) -> Option<usize> {
        self.file_starts.iter().rposition(|s| *s <= idx)
    }

    pub fn next_file(&mut self) {
        let cur = self.top;
        if let Some(s) = self.file_starts.iter().find(|s| **s > cur) {
            self.top = (*s).min(self.max_top().max(*s));
        }
    }

    pub fn prev_file(&mut self) {
        let cur = self.top;
        if let Some(s) = self.file_starts.iter().rev().find(|s| **s < cur) {
            self.top = *s;
        }
    }

    pub fn goto_file(&mut self, fi: usize) {
        if let Some(s) = self.file_starts.get(fi) {
            self.top = *s;
            self.cursor = *s;
            self.anchor = None;
            self.text_selection = None;
        }
    }

    pub fn set_search(&mut self, m: Option<Matcher>) {
        self.search = m;
        self.recompute_search();
    }

    fn recompute_search(&mut self) {
        self.search_hits = match &self.search {
            Some(m) => self
                .lines
                .iter()
                .enumerate()
                .filter(|(_, l)| m.is_match(&l.text))
                .map(|(i, _)| i)
                .collect(),
            None => Vec::new(),
        };
    }

    /// Moves to the next (or previous) search hit; returns false if none.
    pub fn search_step(&mut self, forward: bool) -> bool {
        if self.search_hits.is_empty() {
            return false;
        }
        let cur = self.cursor;
        let hit = if forward {
            self.search_hits
                .iter()
                .find(|h| **h > cur)
                .or(self.search_hits.first())
        } else {
            self.search_hits
                .iter()
                .rev()
                .find(|h| **h < cur)
                .or(self.search_hits.last())
        };
        if let Some(h) = hit.copied() {
            self.cursor = h;
            self.anchor = None;
            self.text_selection = None;
            let half = self.height / 2;
            self.top = h.saturating_sub(half).min(self.max_top());
        }
        true
    }

    /// New-side line number of a display line (for blame / origin).
    pub fn new_line_number(&self, idx: usize) -> Option<(usize, u32)> {
        let l = self.lines.get(idx)?;
        let (f, h, li) = (l.file?, l.hunk?, l.line?);
        let d = self.diff.as_ref()?;
        let hunk = d.files.get(f)?.hunks.get(h)?;
        let mut n = hunk.new_start;
        for (i, dl) in hunk.lines.iter().enumerate() {
            if i == li {
                return if dl.kind == LineKind::Del {
                    None
                } else {
                    Some((f, n))
                };
            }
            if dl.kind != LineKind::Del {
                n += 1;
            }
        }
        None
    }

    /// Old-side line number of a display line.
    pub fn old_line_number(&self, idx: usize) -> Option<(usize, u32)> {
        let l = self.lines.get(idx)?;
        let (f, h, li) = (l.file?, l.hunk?, l.line?);
        let d = self.diff.as_ref()?;
        let hunk = d.files.get(f)?.hunks.get(h)?;
        let mut n = hunk.old_start;
        for (i, dl) in hunk.lines.iter().enumerate() {
            if i == li {
                return if dl.kind == LineKind::Add {
                    None
                } else {
                    Some((f, n))
                };
            }
            if dl.kind != LineKind::Add {
                n += 1;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diff::{ParseMode, parse_diff};
    use crate::domain::textmatch::MatchKind;

    const D: &[u8] = b"diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\ndiff --git a/g b/g\n--- a/g\n+++ b/g\n@@ -1 +1 @@\n-x\n+y\n";

    fn opts(show: Show) -> BuildOpts {
        BuildOpts {
            tab: 8,
            show,
            words: WordStyle::Markup,
            compact_headers: false,
        }
    }

    #[test]
    fn builds_and_selects() {
        let d = Arc::new(parse_diff(D, ParseMode::Normal, false));
        let mut v = DiffView {
            height: 10,
            ..Default::default()
        };
        v.set(Vec::new(), Some(d), &opts(Show::Diff));
        assert_eq!(v.file_starts.len(), 2);
        let del = v.lines.iter().position(|l| l.text == "-b").unwrap();
        v.set_cursor(del, false);
        v.set_cursor(del + 1, true);
        let (f, set) = v.selected_lines().unwrap();
        assert_eq!(f, 0);
        assert_eq!(set.into_iter().collect::<Vec<_>>(), vec![(0, 1), (0, 2)]);
        assert_eq!(v.new_line_number(del + 1), Some((0, 2)));
        assert_eq!(v.old_line_number(del), Some((0, 2)));
        assert_eq!(v.new_line_number(del), None);
        v.set_search(Some(Matcher::new(MatchKind::Exact, "y").unwrap()));
        assert!(v.search_step(true));
        assert_eq!(v.lines[v.cursor].text, "+y");
    }

    #[test]
    fn old_new_filters() {
        let d = Arc::new(parse_diff(D, ParseMode::Normal, false));
        let mut v = DiffView::default();
        v.set(Vec::new(), Some(d.clone()), &opts(Show::Old));
        assert!(v.lines.iter().any(|l| l.text == "b"));
        assert!(!v.lines.iter().any(|l| l.text == "B"));
        v.set(Vec::new(), Some(d), &opts(Show::New));
        assert!(v.lines.iter().any(|l| l.text == "B"));
        assert!(!v.lines.iter().any(|l| l.text == "b"));
    }

    #[test]
    fn hostile_content_is_sanitized() {
        let raw = b"diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -0,0 +1 @@\n+\x1b]52;c;AAAA\x07evil\n";
        let d = Arc::new(parse_diff(raw, ParseMode::Normal, false));
        let mut v = DiffView::default();
        v.set(Vec::new(), Some(d), &opts(Show::Diff));
        assert!(
            v.lines
                .iter()
                .all(|l| !l.text.contains('\x1b') && !l.text.contains('\x07'))
        );
    }
}
