//! Mouse handling. Every gesture is resolved through the hit zones recorded
//! by the renderer, then mapped to the same actions as the keyboard.

use std::time::{Duration, Instant};

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use super::App;
use super::action::{Action, FileList};
use super::dialog::{DialogResult, Field};
use super::history::HFocus;
use super::menu::{Ctx, MenuItem};
use super::worktree::WFocus;
use crate::domain::refs::RefKind;
use crate::ui::hit::{Hit, PaneId, SplitId};

const WHEEL_LINES: isize = 3;

/// Detects double clicks (same cell, within the configured delay).
#[derive(Clone, Debug)]
pub struct ClickTracker {
    delay: Duration,
    last: Option<(Instant, u16, u16)>,
    count: u8,
}

impl ClickTracker {
    pub fn new(delay: Duration) -> ClickTracker {
        ClickTracker {
            delay,
            last: None,
            count: 0,
        }
    }

    /// Registers a press and returns the click count (1, 2, 3...).
    pub fn press(&mut self, x: u16, y: u16, now: Instant) -> u8 {
        match self.last {
            Some((t, lx, ly)) if lx == x && ly == y && now.duration_since(t) <= self.delay => {
                self.count = self.count.saturating_add(1);
            }
            _ => self.count = 1,
        }
        self.last = Some((now, x, y));
        self.count
    }
}

/// Gesture in progress while the left button is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drag {
    /// Splitter being moved; the rect is the container being split.
    Split(SplitId, Rect),
    /// Line selection in a diff pane (rect = text area).
    DiffSel(PaneId, Rect),
    /// Text selection in the commit message (rect = text area).
    MsgSel(Rect),
}

fn ratio(pos: u16, start: u16, len: u16) -> u16 {
    if len == 0 {
        return 500;
    }
    let v = (u32::from(pos.saturating_sub(start)) * 1000 / u32::from(len)) as u16;
    v.clamp(100, 900)
}

impl App {
    pub fn on_mouse(&mut self, m: MouseEvent) {
        if !self.mouse_enabled {
            return;
        }
        let (x, y) = (m.column, m.row);
        let shift = m.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = m.modifiers.contains(KeyModifiers::CONTROL);
        if matches!(m.kind, MouseEventKind::Down(_)) {
            self.dismiss_error();
        }
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let n = self.clicks.press(x, y, Instant::now());
                self.left_click(x, y, n, shift, ctrl);
            }
            MouseEventKind::Down(MouseButton::Right) => self.right_click(x, y),
            MouseEventKind::Drag(MouseButton::Left) => self.on_drag(x, y),
            MouseEventKind::Moved => self.menu_hover(x, y),
            MouseEventKind::Up(_) => {
                if matches!(self.drag, Some(Drag::Split(..))) {
                    self.save_state();
                }
                self.drag = None;
            }
            MouseEventKind::ScrollUp => self.wheel(x, y, -WHEEL_LINES, shift),
            MouseEventKind::ScrollDown => self.wheel(x, y, WHEEL_LINES, shift),
            MouseEventKind::ScrollLeft => self.wheel(x, y, -WHEEL_LINES, true),
            MouseEventKind::ScrollRight => self.wheel(x, y, WHEEL_LINES, true),
            _ => {}
        }
    }

    fn pane_rect(&self, p: PaneId) -> Option<Rect> {
        self.hits
            .zones
            .iter()
            .find(|(_, h)| *h == Hit::Pane(p))
            .map(|(r, _)| *r)
    }

    fn focus_pane(&mut self, p: PaneId) {
        self.wt.filter_focus = None;
        match p {
            PaneId::HList => self.hist.focus = HFocus::List,
            PaneId::HDiff => self.hist.focus = HFocus::Diff,
            PaneId::HFiles => self.hist.focus = HFocus::Files,
            PaneId::WUnstaged => self.wt.focus = WFocus::Unstaged,
            PaneId::WStaged => self.wt.focus = WFocus::Staged,
            PaneId::WDiff => self.wt.focus = WFocus::Diff,
            PaneId::WMessage => self.wt.focus = WFocus::Message,
            PaneId::Dialog => {}
        }
    }

    fn left_click(&mut self, x: u16, y: u16, n: u8, shift: bool, ctrl: bool) {
        let Some((r, hit)) = self.hits.at(x, y).map(|(r, h)| (r, h.clone())) else {
            // Outside any zone: clicking off a menu closes it.
            self.menu = None;
            return;
        };
        let col = usize::from(x.saturating_sub(r.x));
        if self.refs_window_open() && self.menu.is_none() {
            self.dialogs[0].background_focus = !hit.is_dialog();
        }
        if !matches!(
            hit,
            Hit::WtFilter(_) | Hit::Button(Action::ClearFileFilter(_))
        ) {
            self.wt.filter_focus = None;
        }
        if let Some(d) = self.dialogs.last_mut()
            && let Some((field, _)) = d.open_choice
            && !matches!(hit, Hit::DlgField(i) | Hit::DlgChoice(i, _) if i == field)
        {
            d.open_choice = None;
        }
        match hit {
            Hit::MenuTitle(i) => {
                if self.menu.as_ref().and_then(|m| m.bar) == Some(i) {
                    self.menu = None;
                } else {
                    self.open_menu(i);
                }
            }
            Hit::MenuItem { depth, idx } => self.menu_click(depth, idx),
            Hit::MenuBackdrop => self.menu = None,
            Hit::ViewTab(v) => self.switch_view(v),
            Hit::StatusMsg => self.show_message_log(),
            Hit::Split(id, rect) => self.drag = Some(Drag::Split(id, rect)),
            Hit::Pane(p) => {
                self.focus_pane(p);
                if p == PaneId::WMessage {
                    self.drag = Some(Drag::MsgSel(r));
                }
            }
            Hit::PaneTab(p) => {
                self.focus_pane(p);
                if self.maximized.is_some() {
                    self.maximized = Some(p);
                }
            }
            Hit::CommitRow(i) => {
                self.hist.focus = HFocus::List;
                self.dispatch(Action::SelectRow(i));
            }
            Hit::CutArrow(row, lane) => self.follow_cut(row, lane),
            Hit::ResumeArrow { node, .. } => self.follow_resume(node),
            Hit::RefToggle(i) => {
                self.hist.focus = HFocus::List;
                if let Some(oid) = self.hist.commits.get(i).map(|c| c.oid)
                    && !self.hist.expanded_refs.remove(&oid)
                {
                    self.hist.expanded_refs.insert(oid);
                }
            }
            Hit::RefLabel(i, full) => {
                self.hist.focus = HFocus::List;
                if self
                    .refs
                    .find_full(&full)
                    .is_some_and(|r| r.kind == RefKind::Tag)
                {
                    self.show_tag_info(full);
                } else {
                    self.dispatch(Action::SelectRow(i));
                }
            }
            Hit::ShaEntry => {
                self.hist.focus = HFocus::Sha;
                self.hist.sha.click(col);
            }
            Hit::FindEntry => {
                self.hist.focus = HFocus::Find;
                self.hist.find.click(col);
            }
            Hit::SearchEntry => {
                self.hist.focus = HFocus::Search;
                self.hist.search.click(col);
            }
            Hit::Button(a) => self.dispatch(a),
            Hit::ShowBtn(s) => self.dispatch(Action::SetShow(s)),
            Hit::HFileRow(i) => {
                if n >= 2 {
                    self.dispatch(Action::OpenFileRow(i));
                } else {
                    self.dispatch(Action::SelectFileRow(i));
                }
            }
            Hit::DiffLine(p, idx) => {
                self.focus_pane(p);
                let dv = match p {
                    PaneId::WDiff => &mut self.wt.diff,
                    _ => &mut self.hist.diff,
                };
                if shift {
                    dv.set_cursor(idx, true);
                } else {
                    dv.text_click(idx, col, false);
                }
                if p == PaneId::HDiff {
                    self.hist.sync_file_selection(idx);
                }
                if let Some(rect) = self.pane_rect(p) {
                    self.drag = Some(Drag::DiffSel(p, rect));
                }
            }
            Hit::WtFile(l, i, icon) => {
                self.wt.focus = match l {
                    FileList::Unstaged => WFocus::Unstaged,
                    FileList::Staged => WFocus::Staged,
                };
                if icon {
                    // A second press must not stage a new row after a refresh.
                    if n == 1 {
                        self.dispatch(Action::StageToggle(l, i));
                    }
                } else if ctrl {
                    self.dispatch(Action::ToggleFileSel(l, i));
                } else if shift {
                    self.dispatch(Action::ExtendFile(l, i));
                } else {
                    self.dispatch(Action::SelectFile(l, i));
                }
            }
            Hit::WtFilter(l) => {
                self.dispatch(Action::FilterFiles(l));
                self.wt.filter_mut(l).click(col);
            }
            Hit::Message => {
                self.wt.focus = WFocus::Message;
                self.wt.message.click(usize::from(y - r.y), col, shift);
                self.drag = Some(Drag::MsgSel(r));
            }
            Hit::DlgField(i) => self.dialog_field_click(i, usize::from(y - r.y), col, shift),
            Hit::DlgArea(i, offset) => {
                self.dialog_field_click(i, offset + usize::from(y - r.y), col, shift)
            }
            Hit::DlgChoice(i, opt) => {
                if let Some(d) = self.dialogs.last_mut() {
                    d.select_choice(i, opt);
                }
            }
            Hit::DlgListRow(i, row) => {
                let mut accept = false;
                let mut preview = false;
                if let Some(d) = self.dialogs.last_mut() {
                    d.focus = i;
                    if let Some(Field::List { sel, items, .. }) = d.fields.get_mut(i)
                        && row < items.len()
                    {
                        accept = n >= 2 && *sel == row;
                        *sel = row;
                        preview = d.is_modeless();
                    }
                }
                if preview {
                    self.preview_list_ref();
                }
                if accept {
                    self.on_dialog_result(DialogResult::Button(0));
                }
            }
            Hit::DlgButton(b) => self.on_dialog_result(DialogResult::Button(b)),
            Hit::DlgBackdrop => {}
        }
    }

    fn dialog_field_click(&mut self, i: usize, row: usize, col: usize, shift: bool) {
        let Some(d) = self.dialogs.last_mut() else {
            return;
        };
        if !d.field_visible(i) {
            return;
        }
        d.focus = i;
        d.reveal_focus = true;
        match d.fields.get_mut(i) {
            Some(Field::Text { input, .. }) => input.click(col),
            Some(Field::Area { area, .. }) => area.click(row, col, shift),
            Some(Field::List {
                search: Some(s), ..
            }) => s.input.click(col),
            Some(Field::Check { value, .. }) => *value = !*value,
            Some(Field::Choice { dropdown: true, .. }) => d.toggle_dropdown(i),
            Some(Field::Choice { options, sel, .. }) => *sel = (*sel + 1) % options.len().max(1),
            _ => {}
        }
    }

    fn menu_click(&mut self, depth: usize, idx: usize) {
        let Some(m) = &mut self.menu else { return };
        if depth >= m.levels.len() {
            return;
        }
        m.levels.truncate(depth + 1);
        let l = m.top();
        if !l.items.get(idx).is_some_and(MenuItem::selectable) {
            return;
        }
        l.sel = idx;
        self.menu_activate();
    }

    /// Highlights the menu item under the pointer, opening its submenu if any.
    fn menu_hover(&mut self, x: u16, y: u16) {
        let Some(Hit::MenuItem { depth, idx }) = self.hits.at(x, y).map(|(_, h)| h.clone()) else {
            return;
        };
        let Some(m) = &mut self.menu else { return };
        let has_child = m.levels.len() > depth + 1;
        let Some(l) = m.levels.get_mut(depth) else {
            return;
        };
        let Some(item) = l.items.get(idx) else { return };
        if !item.selectable() || (l.sel == idx && has_child) {
            return;
        }
        let is_sub = matches!(item, MenuItem::Sub { .. });
        l.sel = idx;
        m.levels.truncate(depth + 1);
        if is_sub {
            m.open_sub();
        }
    }

    fn right_click(&mut self, x: u16, y: u16) {
        self.drag = None;
        if !self.dialogs.is_empty() && !self.refs_window_open() {
            return;
        }
        let Some(hit) = self.hits.at(x, y).map(|(_, h)| h.clone()) else {
            return;
        };
        self.wt.filter_focus = None;
        if self.refs_window_open() {
            self.dialogs[0].background_focus = !hit.is_dialog();
        }
        let ctx = match hit {
            Hit::DlgListRow(i, row) if self.refs_window_open() => {
                let d = &mut self.dialogs[0];
                let Some(Field::List { values, sel, .. }) = d.fields.get_mut(i) else {
                    return;
                };
                let Some(full) = values.get(row).cloned() else {
                    return;
                };
                *sel = row;
                d.focus = i;
                Ctx::Ref(full)
            }
            Hit::CommitRow(i) | Hit::CutArrow(i, _) | Hit::ResumeArrow { row: i, .. } => {
                self.hist.focus = HFocus::List;
                Ctx::Commit(i)
            }
            Hit::RefLabel(_, full) => Ctx::Ref(full),
            Hit::HFileRow(i) => {
                self.hist.focus = HFocus::Files;
                Ctx::HFile(i)
            }
            Hit::DiffLine(PaneId::WDiff, idx) => {
                self.wt.focus = WFocus::Diff;
                if !self.wt.diff.is_selected(idx) && self.wt.diff.text_range().is_none() {
                    self.wt.diff.set_cursor(idx, false);
                }
                Ctx::WtDiff(idx)
            }
            Hit::DiffLine(_, idx) => {
                self.hist.focus = HFocus::Diff;
                if !self.hist.diff.is_selected(idx) && self.hist.diff.text_range().is_none() {
                    self.hist.diff.set_cursor(idx, false);
                }
                self.hist.sync_file_selection(idx);
                Ctx::HDiff(idx)
            }
            Hit::WtFile(l, i, _) => {
                let path = self.wt.list(l).get(i).map(|f| f.path.clone());
                if path.is_some_and(|p| !self.wt.sel(l).contains(&p)) {
                    self.dispatch(Action::SelectFile(l, i));
                }
                Ctx::WtFile(l, i)
            }
            Hit::MenuBackdrop | Hit::MenuItem { .. } | Hit::MenuTitle(_) => {
                self.menu = None;
                return;
            }
            _ => return,
        };
        self.open_context(ctx, x, y);
    }

    fn on_drag(&mut self, x: u16, y: u16) {
        if !self.dialogs.is_empty()
            && (!self.refs_window_open()
                || self.hits.at(x, y).is_some_and(|(_, hit)| hit.is_dialog()))
        {
            return;
        }
        let Some(d) = self.drag else { return };
        match d {
            Drag::Split(id, r) => {
                let s = &mut self.state.splits;
                match id {
                    SplitId::HTop => s.history_top = ratio(y, r.y, r.height),
                    SplitId::HBottom => s.history_bottom_left = ratio(x, r.x, r.width),
                    SplitId::WLeft => s.worktree_left = ratio(x, r.x, r.width),
                    SplitId::WFiles => s.worktree_files = ratio(y, r.y, r.height),
                    SplitId::WDiff => s.worktree_diff = ratio(y, r.y, r.height),
                    SplitId::HDate => {
                        let right = r.x + r.width;
                        s.history_date_col = right.saturating_sub(x).saturating_sub(1).clamp(5, 60);
                    }
                    SplitId::HAuthor => {
                        let date_start = (r.x + r.width).saturating_sub(s.history_date_col + 1);
                        s.history_author_col =
                            date_start.saturating_sub(x).saturating_sub(1).clamp(5, 60);
                    }
                }
            }
            Drag::DiffSel(p, r) => {
                let dv = match p {
                    PaneId::WDiff => &mut self.wt.diff,
                    _ => &mut self.hist.diff,
                };
                if y < r.y {
                    dv.scroll(-1);
                } else if y >= r.y + r.height {
                    dv.scroll(1);
                }
                let row = usize::from(y.clamp(r.y, (r.y + r.height).saturating_sub(1)) - r.y);
                let idx = (dv.top + row).min(dv.lines.len().saturating_sub(1));
                if !dv.lines.is_empty() {
                    if dv.text_selection.is_some() {
                        if x < r.x {
                            dv.scroll_h(-1);
                        } else if x >= r.right() {
                            dv.scroll_h(1);
                        }
                        dv.text_click(
                            idx,
                            usize::from(x.saturating_sub(r.x).min(r.width.saturating_sub(1))),
                            true,
                        );
                    } else {
                        dv.set_cursor(idx, true);
                    }
                }
                if p == PaneId::HDiff {
                    self.hist.sync_file_selection(idx);
                }
            }
            Drag::MsgSel(r) => {
                let h = usize::from(r.height);
                if y < r.y {
                    self.wt.message.scroll(-1, h);
                } else if y >= r.y + r.height {
                    self.wt.message.scroll(1, h);
                }
                let row = y.clamp(r.y, (r.y + r.height).saturating_sub(1)) - r.y;
                let col = x.saturating_sub(r.x);
                self.wt
                    .message
                    .click(usize::from(row), usize::from(col), true);
            }
        }
    }

    fn wheel(&mut self, x: u16, y: u16, d: isize, horizontal: bool) {
        if self.menu.is_some() {
            return;
        }
        let Some(p) = self.hits.pane_at(x, y) else {
            return;
        };
        match p {
            PaneId::HList => self.hist.scroll(d),
            PaneId::HFiles => self.hist.scroll_files(d),
            PaneId::HDiff if horizontal => self.hist.diff.scroll_h(d * 2),
            PaneId::HDiff => self.dispatch(Action::DiffScroll(d)),
            PaneId::WUnstaged => self.wt.scroll_list(FileList::Unstaged, d),
            PaneId::WStaged => self.wt.scroll_list(FileList::Staged, d),
            PaneId::WDiff if horizontal => self.wt.diff.scroll_h(d * 2),
            PaneId::WDiff => self.wt.diff.scroll(d),
            PaneId::WMessage => {
                let h = self.wt.msg_height;
                self.wt.message.scroll(d, h);
            }
            PaneId::Dialog => {
                if let Some(dl) = self.dialogs.last_mut() {
                    let target = match self.hits.at(x, y).map(|(_, h)| h.clone()) {
                        Some(Hit::DlgListRow(i, _)) => Some(i),
                        _ => None,
                    };
                    let visible_rows = target.map(|i| {
                        self.hits
                            .zones
                            .iter()
                            .filter(
                                |(_, hit)| matches!(hit, Hit::DlgListRow(field, _) if *field == i),
                            )
                            .count()
                    });
                    match target.and_then(|i| dl.fields.get_mut(i)) {
                        Some(Field::List {
                            items, top, rows, ..
                        }) => {
                            let max = items
                                .len()
                                .saturating_sub(visible_rows.unwrap_or(usize::from(*rows)).max(1));
                            *top = (*top as isize + d).clamp(0, max as isize) as usize;
                        }
                        _ => dl.scroll_by(d),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_click_detection() {
        let mut c = ClickTracker::new(Duration::from_millis(400));
        let t = Instant::now();
        assert_eq!(c.press(3, 4, t), 1);
        assert_eq!(c.press(3, 4, t + Duration::from_millis(100)), 2);
        assert_eq!(c.press(3, 4, t + Duration::from_millis(900)), 1);
        assert_eq!(c.press(5, 4, t + Duration::from_millis(950)), 1);
    }

    #[test]
    fn ratio_is_clamped() {
        assert_eq!(ratio(0, 0, 100), 100);
        assert_eq!(ratio(50, 0, 100), 500);
        assert_eq!(ratio(99, 0, 100), 900);
    }
}
