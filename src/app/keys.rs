//! Keyboard handling. Priority: dialog > menu > global keys > focused view.
//! gitk and git gui shortcuts are kept where the terminal can report them.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode as C, KeyEvent, KeyEventKind, KeyModifiers as M};

use super::action::{Action, FileList};
use super::dialog::edit_input;
use super::history::HFocus;
use super::input::Move;
use super::menu::{Ctx, MenuItem, titles};
use super::worktree::WFocus;
use super::{App, StatusTone};
use crate::config::ViewKind;
use crate::ui::hit::PaneId;

impl App {
    pub fn on_key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        self.dismiss_error();
        if k.code == C::Char('c') && k.modifiers.contains(M::CONTROL) {
            let now = Instant::now();
            if self
                .last_ctrl_c
                .is_some_and(|t| now.duration_since(t) < Duration::from_millis(1200))
            {
                self.last_ctrl_c = None;
                self.request_quit();
                return;
            }
            self.last_ctrl_c = Some(now);
            self.set_status("Press Ctrl-C again to quit", StatusTone::Info);
            // Fall through so Ctrl-C still copies as usual on a single press.
        }
        if !self.dialogs.is_empty() {
            if k.code == C::Char('c') && k.modifiers.contains(M::CONTROL) {
                let d = self.dialogs.last().expect("dialog");
                if d.fields.is_empty() && !d.code.is_empty() {
                    let t: Vec<String> = d
                        .code
                        .iter()
                        .map(|line| line.iter().map(|(s, _)| s.as_str()).collect::<String>())
                        .collect();
                    self.copy_text(t.join("\n"));
                    return;
                }
                if d.fields.is_empty() {
                    let t: Vec<String> = d.body.iter().map(|(s, _)| s.clone()).collect();
                    self.copy_text(t.join("\n"));
                    return;
                }
            }
            let r = self.dialogs.last_mut().expect("dialog").on_key(k);
            self.on_dialog_result(r);
            return;
        }
        if self.menu.is_some() {
            self.menu_key(k);
            return;
        }
        if self.global_key(k) {
            return;
        }
        match self.view {
            ViewKind::History => self.history_key(k),
            ViewKind::Worktree => self.worktree_key(k),
        }
    }

    fn menu_key(&mut self, k: KeyEvent) {
        let Some(m) = &mut self.menu else { return };
        match k.code {
            C::Esc | C::F(10) => {
                if m.levels.len() > 1 {
                    m.levels.pop();
                } else {
                    self.menu = None;
                }
            }
            C::Up => m.move_sel(-1),
            C::Down => m.move_sel(1),
            C::Left => {
                if m.levels.len() > 1 {
                    m.levels.pop();
                } else if let Some(b) = m.bar {
                    let n = titles(self.view).len();
                    self.open_menu((b + n - 1) % n);
                }
            }
            C::Right => {
                if !m.open_sub()
                    && let Some(b) = m.bar
                {
                    self.open_menu(b + 1);
                }
            }
            C::Enter | C::Char(' ') => self.menu_activate(),
            C::Char(c) => {
                let c = c.to_ascii_lowercase();
                let l = m.top();
                let found = l.items.iter().position(|i| {
                    i.selectable()
                        && i.label().chars().next().map(|f| f.to_ascii_lowercase()) == Some(c)
                });
                if let Some(i) = found {
                    l.sel = i;
                    self.menu_activate();
                }
            }
            _ => {}
        }
    }

    /// Keys valid everywhere. Returns true when consumed.
    fn global_key(&mut self, k: KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(M::CONTROL);
        let alt = k.modifiers.contains(M::ALT);
        let shift = k.modifiers.contains(M::SHIFT);
        let a = match k.code {
            C::Char('q') if ctrl => Action::Quit,
            C::Char('w') if ctrl => Action::Quit,
            C::F(10) if shift => {
                self.context_from_keyboard();
                return true;
            }
            C::F(10) => Action::OpenMenu(0),
            C::F(1) => Action::KeyBindings,
            C::F(6) => Action::ToggleView,
            C::Tab if ctrl => Action::ToggleView,
            C::F(11) => Action::ToggleMaximize,
            C::Tab if !self.in_text_field() || self.view == ViewKind::Worktree => Action::FocusNext,
            C::BackTab => Action::FocusPrev,
            C::Esc if self.maximized.is_some() => Action::ToggleMaximize,
            C::Menu => {
                self.context_from_keyboard();
                return true;
            }
            C::Char(c) if alt && c.is_ascii_alphabetic() => {
                let c = c.to_ascii_lowercase();
                match titles(self.view)
                    .iter()
                    .position(|t| t.chars().next().map(|f| f.to_ascii_lowercase()) == Some(c))
                {
                    Some(i) => Action::OpenMenu(i),
                    None => return false,
                }
            }
            _ => return false,
        };
        self.dispatch(a);
        true
    }

    fn in_text_field(&self) -> bool {
        match self.view {
            ViewKind::History => {
                matches!(self.hist.focus, HFocus::Sha | HFocus::Find | HFocus::Search)
            }
            ViewKind::Worktree => self.wt.focus == WFocus::Message,
        }
    }

    /// Shift-F10 / Menu key: context menu for the focused element.
    fn context_from_keyboard(&mut self) {
        let (w, h) = self.size;
        let (x, y) = (w / 3, h / 3);
        let c = match self.focused_pane() {
            PaneId::HList => match self.hist.sel {
                Some(i) => Ctx::Commit(i),
                None => return,
            },
            PaneId::HFiles => Ctx::HFile(self.hist.file_sel),
            PaneId::HDiff => Ctx::HDiff(self.hist.diff.cursor),
            PaneId::WUnstaged | PaneId::WStaged => match self.wt.cursor_index() {
                Some((l, i)) => Ctx::WtFile(l, i),
                None => return,
            },
            PaneId::WDiff => Ctx::WtDiff(self.wt.diff.cursor),
            _ => return,
        };
        self.open_context(c, x, y);
    }

    // ------------------------------------------------------------------
    // History (gitk)

    fn history_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(M::CONTROL);
        let shift = k.modifiers.contains(M::SHIFT);
        // Text entries first.
        match self.hist.focus {
            HFocus::Sha | HFocus::Find | HFocus::Search => {
                if self.history_entry_key(k) {
                    return;
                }
            }
            _ => {}
        }
        let focus = self.hist.focus;
        let a = match k.code {
            C::F(5) if shift => Action::Reload,
            C::F(5) => Action::Update,
            C::F(2) => Action::ListRefs,
            C::F(4) if shift => Action::NewView,
            C::F(4) => Action::EditView,
            C::Char('f') if ctrl => Action::FocusFind,
            C::Char('g') if ctrl => Action::FindNext,
            C::Char('s') if ctrl => Action::DiffSearchNext,
            C::Char('r') if ctrl => Action::DiffSearchPrev,
            C::Char('c') if ctrl => Action::Copy,
            C::Char('+') | C::Char('=') if ctrl => Action::ContextMore,
            C::Char('-') if ctrl => Action::ContextLess,
            C::Char(c @ '1'..='9') if ctrl => Action::GotoParent(c as usize - '0' as usize),
            C::Left | C::Right if shift && focus == HFocus::Diff => {
                self.hist.diff.text_move(k.code == C::Right);
                return;
            }
            C::Up if shift && focus != HFocus::Diff => Action::FindPrev,
            C::Down if shift && focus != HFocus::Diff => Action::FindNext,
            C::Up if ctrl => Action::ScrollList(-1),
            C::Down if ctrl => Action::ScrollList(1),
            C::PageUp if ctrl => Action::ScrollList(-(self.hist.height as isize)),
            C::PageDown if ctrl => Action::ScrollList(self.hist.height as isize),
            C::Home if ctrl => Action::ScrollList(isize::MIN / 2),
            C::End if ctrl => Action::ScrollList(isize::MAX / 2),
            // Files pane: arrows move in the file list.
            C::Up if focus == HFocus::Files => {
                Action::SelectFileRow(self.hist.file_sel.saturating_sub(1))
            }
            C::Down if focus == HFocus::Files => Action::SelectFileRow(self.hist.file_sel + 1),
            C::Enter if focus == HFocus::Files => Action::OpenFileRow(self.hist.file_sel),
            // Diff pane: arrows move the line cursor (Shift extends).
            C::Up if focus == HFocus::Diff => {
                let c = self.hist.diff.cursor.saturating_sub(1);
                self.hist.diff.set_cursor(c, shift);
                return;
            }
            C::Down if focus == HFocus::Diff => {
                let c = self.hist.diff.cursor + 1;
                self.hist.diff.set_cursor(c, shift);
                return;
            }
            C::Char('c') if focus == HFocus::Diff => Action::Copy,
            C::Up | C::Char('k') | C::Char('p') => Action::MoveSel(-1),
            C::Down | C::Char('j') | C::Char('n') => Action::MoveSel(1),
            C::PageUp => Action::SelPage(-1),
            C::PageDown => Action::SelPage(1),
            C::Home => Action::SelFirst,
            C::End => Action::SelLast,
            C::Left | C::Char('z') | C::Char('h') => Action::GoBack,
            C::Right | C::Char('x') | C::Char('l') => Action::GoForward,
            C::Char(' ') => Action::DiffPage(1),
            C::Backspace | C::Delete | C::Char('b') => Action::DiffPage(-1),
            C::Char('d') => Action::DiffScroll(18),
            C::Char('u') => Action::DiffScroll(-18),
            C::Char('/') => Action::FocusFind,
            C::Enter => Action::FindNext,
            C::Char('?') => Action::FindPrev,
            C::Char('g') => Action::FocusSha,
            C::Char('f') => Action::DiffNextFile,
            C::Esc => {
                self.hist.diff.anchor = None;
                self.hist.diff.text_selection = None;
                return;
            }
            _ => return,
        };
        self.dispatch(a);
    }

    /// Keys for the SHA1 / Find / Search entries. Returns true when consumed.
    fn history_entry_key(&mut self, k: KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(M::CONTROL);
        let shift = k.modifiers.contains(M::SHIFT);
        let focus = self.hist.focus;
        match k.code {
            C::Esc => {
                self.hist.focus = HFocus::List;
                return true;
            }
            C::Enter => {
                let a = match focus {
                    HFocus::Sha => Action::GotoSha(self.hist.sha.text.clone()),
                    HFocus::Find => Action::FindNext,
                    _ => Action::DiffSearchNext,
                };
                self.dispatch(a);
                return true;
            }
            C::Up | C::Down if shift && focus == HFocus::Find => return false,
            C::Up | C::Down | C::PageUp | C::PageDown | C::F(_) => return false,
            C::Char('q' | 'w' | 'f' | 'g' | 's' | 'r') if ctrl => return false,
            C::Char('c') if ctrl => {
                let t = match focus {
                    HFocus::Sha => self.hist.sha.text.clone(),
                    HFocus::Find => self.hist.find.text.clone(),
                    _ => self.hist.search.text.clone(),
                };
                self.copy_text(t);
                return true;
            }
            C::Char('v') if ctrl => {
                let c = self.clipboard.clone();
                self.paste(&c);
                return true;
            }
            _ => {}
        }
        match focus {
            HFocus::Sha => edit_input(&mut self.hist.sha, k),
            HFocus::Find => {
                let before = self.hist.find.text.clone();
                edit_input(&mut self.hist.find, k);
                if self.hist.find.text != before {
                    self.find_changed();
                }
            }
            _ => {
                let before = self.hist.search.text.clone();
                edit_input(&mut self.hist.search, k);
                if self.hist.search.text != before {
                    self.search_changed();
                }
            }
        }
        true
    }

    // ------------------------------------------------------------------
    // Worktree (git gui)

    fn worktree_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(M::CONTROL);
        let alt = k.modifiers.contains(M::ALT);
        let shift = k.modifiers.contains(M::SHIFT);
        // Commands valid in every pane (including the message).
        let a = match k.code {
            C::Enter if ctrl || alt => Some(Action::Commit),
            C::F(5) => Some(Action::Rescan),
            C::F(4) => Some(Action::EditMessageExternally),
            C::Char('r') if ctrl => Some(Action::Rescan),
            C::Char('n') if ctrl => Some(Action::CreateBranch),
            C::Char('o') if ctrl => Some(Action::CheckoutBranch),
            C::Char('m') if ctrl => Some(Action::LocalMerge),
            C::Char('p') if ctrl => Some(Action::Push),
            C::Char('s') if ctrl => Some(Action::SignOff),
            C::Char('t') if ctrl => Some(Action::StageSelected),
            C::Char('u') if ctrl => Some(Action::UnstageSelected),
            C::Char('j') if ctrl => Some(Action::RevertSelected),
            C::Char('i') if ctrl => Some(Action::StageChanged),
            C::Char('e') if ctrl => Some(Action::ToggleAmend),
            C::Char('-') if ctrl => Some(Action::WtContextLess),
            C::Char('+') | C::Char('=') if ctrl => Some(Action::WtContextMore),
            C::Char(c @ '1'..='4') if alt => {
                self.wt.focus = [
                    WFocus::Unstaged,
                    WFocus::Staged,
                    WFocus::Diff,
                    WFocus::Message,
                ][c as usize - '1' as usize];
                if let (WFocus::Unstaged | WFocus::Staged, None) =
                    (self.wt.focus, self.wt.cursor_index())
                {
                    let l = if self.wt.focus == WFocus::Staged {
                        FileList::Staged
                    } else {
                        FileList::Unstaged
                    };
                    if !self.wt.list(l).is_empty() {
                        self.dispatch(Action::SelectFile(l, 0));
                    }
                }
                return;
            }
            _ => None,
        };
        if let Some(a) = a {
            self.dispatch(a);
            return;
        }
        match self.wt.focus {
            WFocus::Message => self.message_key(k),
            WFocus::Diff => self.wt_diff_key(k),
            WFocus::Unstaged | WFocus::Staged => {
                let l = if self.wt.focus == WFocus::Staged {
                    FileList::Staged
                } else {
                    FileList::Unstaged
                };
                let cur = self
                    .wt
                    .cursor_index()
                    .filter(|(cl, _)| *cl == l)
                    .map(|(_, i)| i);
                let a = match k.code {
                    C::Up | C::Char('k') if shift => {
                        cur.map(|i| Action::ExtendFile(l, i.saturating_sub(1)))
                    }
                    C::Down | C::Char('j') if shift => cur.map(|i| Action::ExtendFile(l, i + 1)),
                    C::Up | C::Char('k') => Some(Action::MoveFile(-1)),
                    C::Down | C::Char('j') => Some(Action::MoveFile(1)),
                    C::Home => Some(Action::SelectFile(l, 0)),
                    C::End => Some(Action::SelectFile(
                        l,
                        self.wt.list(l).len().saturating_sub(1),
                    )),
                    C::Char(' ') | C::Enter => cur.map(|i| Action::StageToggle(l, i)),
                    C::Right | C::Char('l') => {
                        self.wt.focus = WFocus::Diff;
                        None
                    }
                    C::Char('c') if ctrl => Some(Action::Copy),
                    _ => None,
                };
                if let Some(a) = a {
                    self.dispatch(a);
                }
            }
        }
    }

    fn wt_diff_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(M::CONTROL);
        let shift = k.modifiers.contains(M::SHIFT);
        let d = &mut self.wt.diff;
        let page = d.height.max(2) as isize - 1;
        match k.code {
            C::Up | C::Char('k') => {
                let c = d.cursor.saturating_sub(1);
                d.set_cursor(c, shift);
            }
            C::Down | C::Char('j') => {
                let c = d.cursor + 1;
                d.set_cursor(c, shift);
            }
            C::PageUp | C::Char('b') => {
                let c = d.cursor.saturating_sub(page as usize);
                d.set_cursor(c, shift);
            }
            C::PageDown | C::Char('f') => {
                let c = d.cursor + page as usize;
                d.set_cursor(c, shift);
            }
            C::Home => d.set_cursor(0, shift),
            C::End => {
                let n = d.lines.len().saturating_sub(1);
                d.set_cursor(n, shift);
            }
            C::Left | C::Right if shift => d.text_move(k.code == C::Right),
            C::Left | C::Char('h') => d.scroll_h(-4),
            C::Right | C::Char('l') => d.scroll_h(4),
            C::Esc => {
                d.anchor = None;
                d.text_selection = None;
            }
            C::Char('c') if ctrl => self.dispatch(Action::Copy),
            C::Char('a') if ctrl => self.dispatch(Action::SelectAll),
            C::Char('s') => {
                let a = if self.wt.diff.has_range() {
                    Action::StageLines
                } else {
                    Action::StageHunk(self.wt.diff.cursor)
                };
                self.dispatch(a);
            }
            C::Char('r') => {
                let a = if self.wt.diff.has_range() {
                    Action::RevertLines
                } else {
                    Action::RevertHunk(self.wt.diff.cursor)
                };
                self.dispatch(a);
            }
            C::Char('n') => {
                // Jump to the next hunk header.
                let cur = self.wt.diff.cursor;
                if let Some(i) = self
                    .wt
                    .diff
                    .lines
                    .iter()
                    .enumerate()
                    .skip(cur + 1)
                    .find(|(_, l)| l.kind == super::diffview::DKind::Hunk)
                    .map(|(i, _)| i)
                {
                    self.wt.diff.set_cursor(i, false);
                }
            }
            C::Char('p') => {
                let cur = self.wt.diff.cursor;
                if let Some(i) = self.wt.diff.lines[..cur.min(self.wt.diff.lines.len())]
                    .iter()
                    .rposition(|l| l.kind == super::diffview::DKind::Hunk)
                {
                    self.wt.diff.set_cursor(i, false);
                }
            }
            _ => {}
        }
    }

    fn message_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(M::CONTROL);
        let alt = k.modifiers.contains(M::ALT);
        let shift = k.modifiers.contains(M::SHIFT);
        let h = self.wt.msg_height.max(1);
        let w = self.wt.msg_width.max(1);
        let m = &mut self.wt.message;
        match k.code {
            C::Esc => {
                m.anchor = None;
                self.wt.focus = WFocus::Unstaged;
                return;
            }
            C::Char('z') if ctrl => m.undo(),
            C::Char('y') if ctrl => m.redo(),
            C::Char('a') if ctrl => m.select_all(),
            C::Char('c') if ctrl => {
                if let Some(t) = m.selected_text() {
                    self.copy_text(t);
                }
                return;
            }
            C::Char('x') if ctrl => {
                if let Some(t) = m.selected_text() {
                    m.delete_selection();
                    self.copy_text(t);
                }
                return;
            }
            C::Char('v') if ctrl => {
                let c = self.clipboard.clone();
                m.insert_str(&c);
            }
            C::Char(c) if !ctrl && !alt => m.insert_char(c),
            C::Enter => m.newline(),
            C::Backspace => m.backspace(),
            C::Delete => m.delete(),
            C::Left if ctrl => m.move_cursor(Move::WordLeft, shift),
            C::Right if ctrl => m.move_cursor(Move::WordRight, shift),
            C::Home if ctrl => m.move_cursor(Move::DocStart, shift),
            C::End if ctrl => m.move_cursor(Move::DocEnd, shift),
            C::Left => m.move_cursor(Move::Left, shift),
            C::Right => m.move_cursor(Move::Right, shift),
            C::Up => m.move_cursor(Move::Up, shift),
            C::Down => m.move_cursor(Move::Down, shift),
            C::Home => m.move_cursor(Move::Home, shift),
            C::End => m.move_cursor(Move::End, shift),
            C::PageUp => m.move_cursor(Move::PageUp(h), shift),
            C::PageDown => m.move_cursor(Move::PageDown(h), shift),
            _ => return,
        }
        m.scroll_to_cursor(h, w);
    }

    /// Mnemonic for menu items, used by the renderer to underline.
    pub fn menu_mnemonic(item: &MenuItem) -> Option<char> {
        item.label().chars().next()
    }

    pub fn status_hint(&mut self, s: &str) {
        self.set_status(s.to_string(), StatusTone::Info);
    }
}
