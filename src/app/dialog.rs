//! Modal dialogs: messages, confirmations and forms.

use crate::app::input::{Edit, TextArea, TextInput};
use crate::domain::oid::Oid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Strong,
    Warn,
    Error,
    Code,
}

#[derive(Clone, Debug)]
pub enum Field {
    Text {
        label: String,
        input: TextInput,
    },
    Area {
        label: String,
        area: TextArea,
        rows: u16,
    },
    Check {
        label: String,
        value: bool,
    },
    Choice {
        label: String,
        options: Vec<String>,
        sel: usize,
    },
    List {
        label: String,
        items: Vec<String>,
        /// Hidden values associated with items (ref names, paths...).
        values: Vec<Vec<u8>>,
        sel: usize,
        top: usize,
        rows: u16,
        /// Incremental filter (`/` or `?` to start), when supported.
        search: Option<ListSearch>,
    },
    Label(String, Tone),
}

/// Filter state of a searchable list. `items`/`values` of the field hold the
/// filtered view; the full content is kept here.
#[derive(Clone, Debug)]
pub struct ListSearch {
    pub active: bool,
    pub input: TextInput,
    all_items: Vec<String>,
    all_values: Vec<Vec<u8>>,
    /// Text matched against the pattern for each item.
    keys: Vec<String>,
}

/// Case-insensitive match. Without wildcards: substring. With `*` (any
/// sequence) or `?` (one char): anchored on the whole key or on any part
/// following a `/` (so `ma*` matches `main` and `origin/master`).
pub fn search_match(pattern: &str, key: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let k = key.to_lowercase();
    if p.is_empty() {
        return true;
    }
    if !p.iter().any(|c| matches!(c, '*' | '?')) {
        return k.contains(&pattern.to_lowercase());
    }
    let kc: Vec<char> = k.chars().collect();
    std::iter::once(0)
        .chain(kc.iter().enumerate().filter(|(_, c)| **c == '/').map(|(i, _)| i + 1))
        .any(|start| glob(&p, &kc[start..]))
}

fn glob(p: &[char], s: &[char]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, si));
            pi += 1;
        } else if let Some((sp, ss)) = star {
            pi = sp + 1;
            si = ss + 1;
            star = Some((sp, ss + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

impl ListSearch {
    fn apply(&self, items: &mut Vec<String>, values: &mut Vec<Vec<u8>>, sel: &mut usize, top: &mut usize) {
        let pat = self.input.text.trim();
        let keep: Vec<usize> = (0..self.keys.len())
            .filter(|i| search_match(pat, &self.keys[*i]))
            .collect();
        let current = values.get(*sel).cloned();
        *items = keep.iter().map(|i| self.all_items[*i].clone()).collect();
        *values = keep.iter().map(|i| self.all_values[*i].clone()).collect();
        *sel = current
            .and_then(|v| values.iter().position(|x| *x == v))
            .unwrap_or(0);
        *top = 0;
    }
}

impl Field {
    pub fn text(label: &str, value: &str) -> Field {
        Field::Text {
            label: label.into(),
            input: TextInput::new(value),
        }
    }
    pub fn check(label: &str, value: bool) -> Field {
        Field::Check {
            label: label.into(),
            value,
        }
    }
    pub fn choice(label: &str, options: &[&str], sel: usize) -> Field {
        Field::Choice {
            label: label.into(),
            options: options.iter().map(|s| s.to_string()).collect(),
            sel,
        }
    }
    pub fn list(label: &str, items: Vec<String>, values: Vec<Vec<u8>>, sel: usize, rows: u16) -> Field {
        Field::List {
            label: label.into(),
            items,
            values,
            sel,
            top: 0,
            rows,
            search: None,
        }
    }
    /// List that can be filtered by typing `/` (or `?`) then a pattern
    /// matched against `keys` (see `search_match`).
    pub fn searchable_list(
        label: &str,
        items: Vec<String>,
        values: Vec<Vec<u8>>,
        keys: Vec<String>,
        rows: u16,
    ) -> Field {
        let search = ListSearch {
            active: false,
            input: TextInput::new(""),
            all_items: items.clone(),
            all_values: values.clone(),
            keys,
        };
        let mut f = Field::list(label, items, values, 0, rows);
        if let Field::List { search: s, .. } = &mut f {
            *s = Some(search);
        }
        f
    }
    pub fn area(label: &str, value: &str, rows: u16) -> Field {
        Field::Area {
            label: label.into(),
            area: TextArea::new(value),
            rows,
        }
    }
    pub fn label(s: &str, tone: Tone) -> Field {
        Field::Label(s.into(), tone)
    }

    pub fn focusable(&self) -> bool {
        !matches!(self, Field::Label(..))
    }

    /// Height in rows when rendered.
    pub fn height(&self) -> u16 {
        match self {
            Field::Area { rows, .. } => rows + 1,
            Field::List { rows, search, .. } => {
                rows + 1 + u16::from(search.as_ref().is_some_and(|s| s.active))
            }
            _ => 1,
        }
    }
}

/// What happens when the dialog is accepted.
pub enum Kind {
    /// Information only.
    Info,
    /// Confirmation; the pending operation runs on the first button.
    Confirm(Box<crate::app::Pending>),
    /// Form handled by `App::submit_form`.
    Form(FormKind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormKind {
    ViewEdit(Option<usize>),
    Preferences,
    CreateTag(Oid),
    CreateBranchAt(Oid),
    RenameBranch(Vec<u8>),
    ResetHere(Oid),
    MakePatch(Oid, Oid),
    WriteCommit(Oid),
    ListRefs,
    GuiCreateBranch,
    GuiCheckout,
    GuiRename,
    GuiDelete,
    GuiMerge,
    Push,
    AddRemote,
    DeleteRemoteBranch,
    BrowseBranch,
    Browser(Oid),
    QuitBusy,
    CherryPickMerge(Oid),
    UntrackedStage(Vec<Vec<u8>>),
}

pub struct Dialog {
    pub title: String,
    pub body: Vec<(String, Tone)>,
    pub fields: Vec<Field>,
    pub buttons: Vec<String>,
    /// Focus index: fields first, then buttons.
    pub focus: usize,
    pub kind: Kind,
    pub scroll: usize,
    pub width: u16,
    /// Button triggered by Esc (None = close without action).
    pub cancel: Option<usize>,
    pub viewport_height: usize,
    pub content_height: usize,
    pub last_focus: Option<usize>,
    pub reveal_focus: bool,
}

pub enum DialogResult {
    None,
    Close,
    Button(usize),
}

impl Dialog {
    pub fn new(title: &str, kind: Kind) -> Dialog {
        Dialog {
            title: title.into(),
            body: Vec::new(),
            fields: Vec::new(),
            buttons: vec!["OK".into()],
            focus: 0,
            kind,
            scroll: 0,
            width: 70,
            cancel: None,
            viewport_height: 1,
            content_height: 0,
            last_focus: None,
            reveal_focus: true,
        }
    }

    pub fn info(title: &str, text: &str) -> Dialog {
        let mut d = Dialog::new(title, Kind::Info);
        d.body = text.lines().map(|l| (l.to_string(), Tone::Normal)).collect();
        d.buttons = vec!["Close".into()];
        d.width = 78;
        d
    }

    pub fn error(title: &str, text: &str) -> Dialog {
        let mut d = Dialog::info(title, text);
        for l in &mut d.body {
            l.1 = Tone::Error;
        }
        d.buttons = vec!["OK".into()];
        d
    }

    pub fn with_body(mut self, lines: &[(&str, Tone)]) -> Dialog {
        self.body = lines.iter().map(|(s, t)| (s.to_string(), *t)).collect();
        self
    }

    pub fn with_buttons(mut self, b: &[&str]) -> Dialog {
        self.buttons = b.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_fields(mut self, f: Vec<Field>) -> Dialog {
        self.fields = f;
        self.focus = self.fields.iter().position(Field::focusable).unwrap_or(self.fields.len());
        self
    }

    pub fn n_focus(&self) -> usize {
        self.fields.len() + self.buttons.len()
    }

    pub fn focus_next(&mut self, dir: isize) {
        let n = self.n_focus() as isize;
        if n == 0 {
            return;
        }
        let mut f = self.focus as isize;
        for _ in 0..n {
            f = (f + dir).rem_euclid(n);
            let idx = f as usize;
            if idx >= self.fields.len() || self.fields[idx].focusable() {
                self.focus = idx;
                return;
            }
        }
    }

    pub fn focused_field(&mut self) -> Option<&mut Field> {
        self.fields.get_mut(self.focus)
    }

    pub fn scroll_by(&mut self, delta: isize) {
        self.scroll = self.scroll.saturating_add_signed(delta)
            .min(self.content_height.saturating_sub(self.viewport_height));
        self.reveal_focus = false;
        self.last_focus = Some(self.focus);
    }

    pub fn text(&self, i: usize) -> String {
        match self.fields.get(i) {
            Some(Field::Text { input, .. }) => input.text.clone(),
            Some(Field::Area { area, .. }) => area.text(),
            _ => String::new(),
        }
    }

    pub fn checked(&self, i: usize) -> bool {
        matches!(self.fields.get(i), Some(Field::Check { value: true, .. }))
    }

    pub fn chosen(&self, i: usize) -> usize {
        match self.fields.get(i) {
            Some(Field::Choice { sel, .. }) | Some(Field::List { sel, .. }) => *sel,
            _ => 0,
        }
    }

    pub fn list_value(&self, i: usize) -> Option<Vec<u8>> {
        match self.fields.get(i) {
            Some(Field::List { values, sel, .. }) => values.get(*sel).cloned(),
            _ => None,
        }
    }

    pub fn list_item(&self, i: usize) -> Option<String> {
        match self.fields.get(i) {
            Some(Field::List { items, sel, .. }) => items.get(*sel).cloned(),
            _ => None,
        }
    }

    /// Handles a key. Returns what the caller must do.
    pub fn on_key(&mut self, k: crossterm::event::KeyEvent) -> DialogResult {
        use crossterm::event::{KeyCode as C, KeyModifiers as M};
        let ctrl = k.modifiers.contains(M::CONTROL);
        let alt = k.modifiers.contains(M::ALT);
        let shift = k.modifiers.contains(M::SHIFT);
        if alt && matches!(k.code, C::PageUp | C::PageDown | C::Home | C::End) {
            let page = self.viewport_height.max(1) as isize;
            self.scroll_by(match k.code {
                C::PageUp => -page,
                C::PageDown => page,
                C::Home => -(self.content_height as isize),
                _ => self.content_height as isize,
            });
            return DialogResult::None;
        }
        self.reveal_focus = true;
        if k.code == C::Esc
            && let Some(Field::List { items, values, sel, top, search: Some(s), .. }) =
                self.fields.get_mut(self.focus)
            && s.active
        {
            s.active = false;
            s.input.clear();
            s.apply(items, values, sel, top);
            return DialogResult::None;
        }
        match k.code {
            C::Esc => {
                return match self.cancel {
                    Some(b) => DialogResult::Button(b),
                    None => DialogResult::Close,
                };
            }
            C::Tab => {
                self.focus_next(1);
                return DialogResult::None;
            }
            C::BackTab => {
                self.focus_next(-1);
                return DialogResult::None;
            }
            _ => {}
        }
        let nf = self.fields.len();
        if self.focus >= nf {
            let b = self.focus - nf;
            match k.code {
                C::Enter | C::Char(' ') => return DialogResult::Button(b),
                C::Left | C::Up => self.focus_next(-1),
                C::Right | C::Down => self.focus_next(1),
                C::PageUp => self.scroll_by(-(self.viewport_height as isize)),
                C::PageDown => self.scroll_by(self.viewport_height as isize),
                _ => {}
            }
            return DialogResult::None;
        }
        let field = &mut self.fields[self.focus];
        match field {
            Field::Text { input, .. } => match k.code {
                C::Enter => return DialogResult::Button(0),
                C::Up => self.focus_next(-1),
                C::Down => self.focus_next(1),
                _ => edit_input(input, k),
            },
            Field::Area { area, rows, .. } => {
                let h = *rows as usize;
                match k.code {
                    C::Enter if ctrl || alt => return DialogResult::Button(0),
                    C::Enter => area.newline(),
                    C::Char(c) if !ctrl && !alt => area.insert_char(c),
                    C::Backspace => area.backspace(),
                    C::Delete => area.delete(),
                    C::Left => area.move_cursor(crate::app::input::Move::Left, shift),
                    C::Right => area.move_cursor(crate::app::input::Move::Right, shift),
                    C::Up => area.move_cursor(crate::app::input::Move::Up, shift),
                    C::Down => area.move_cursor(crate::app::input::Move::Down, shift),
                    C::Home => area.move_cursor(crate::app::input::Move::Home, shift),
                    C::End => area.move_cursor(crate::app::input::Move::End, shift),
                    _ => {}
                }
                area.scroll_to_cursor(h, 40);
            }
            Field::Check { value, .. } => match k.code {
                C::Char(' ') => *value = !*value,
                C::Enter => return DialogResult::Button(0),
                C::Up | C::Left => self.focus_next(-1),
                C::Down | C::Right => self.focus_next(1),
                _ => {}
            },
            Field::Choice { options, sel, .. } => match k.code {
                C::Left => *sel = sel.saturating_sub(1),
                C::Right | C::Char(' ') => *sel = (*sel + 1) % options.len().max(1),
                C::Enter => return DialogResult::Button(0),
                C::Up => self.focus_next(-1),
                C::Down => self.focus_next(1),
                _ => {}
            },
            Field::List {
                items, values, sel, top, rows, search,
                ..
            } => {
                let h = *rows as usize;
                if let Some(s) = search {
                    match k.code {
                        C::Char('/') | C::Char('?') if !s.active && !ctrl && !alt => {
                            s.active = true;
                            return DialogResult::None;
                        }
                        C::Backspace if s.active && s.input.text.is_empty() => {
                            s.active = false;
                            return DialogResult::None;
                        }
                        C::Char(_) | C::Backspace | C::Delete | C::Left | C::Right
                            if s.active =>
                        {
                            edit_input(&mut s.input, k);
                            s.apply(items, values, sel, top);
                            return DialogResult::None;
                        }
                        _ => {}
                    }
                }
                let n = items.len();
                match k.code {
                    C::Up => *sel = sel.saturating_sub(1),
                    C::Down => *sel = (*sel + 1).min(n.saturating_sub(1)),
                    C::PageUp => *sel = sel.saturating_sub(h),
                    C::PageDown => *sel = (*sel + h).min(n.saturating_sub(1)),
                    C::Home => *sel = 0,
                    C::End => *sel = n.saturating_sub(1),
                    C::Enter => return DialogResult::Button(0),
                    _ => {}
                }
                if *sel < *top {
                    *top = *sel;
                } else if *sel >= *top + h {
                    *top = *sel + 1 - h;
                }
            }
            Field::Label(..) => {}
        }
        DialogResult::None
    }

    /// Bracketed paste into the focused field.
    pub fn paste(&mut self, s: &str) {
        self.reveal_focus = true;
        match self.focused_field() {
            Some(Field::Text { input, .. }) => input.insert_str(s),
            Some(Field::Area { area, .. }) => area.insert_str(s),
            Some(Field::List { items, values, sel, top, search: Some(ls), .. }) if ls.active => {
                ls.input.insert_str(s);
                ls.apply(items, values, sel, top);
            }
            _ => {}
        }
    }
}

/// Common single-line editing keys.
pub fn edit_input(input: &mut TextInput, k: crossterm::event::KeyEvent) {
    use crossterm::event::{KeyCode as C, KeyModifiers as M};
    let ctrl = k.modifiers.contains(M::CONTROL);
    match k.code {
        C::Char('u') if ctrl => input.apply(Edit::ClearToStart),
        C::Char('k') if ctrl => input.apply(Edit::ClearToEnd),
        C::Char('w') if ctrl => input.apply(Edit::DeleteWordLeft),
        C::Char('a') if ctrl => input.apply(Edit::Home),
        C::Char('e') if ctrl => input.apply(Edit::End),
        C::Char(c) if !ctrl && !k.modifiers.contains(M::ALT) => input.apply(Edit::Insert(c)),
        C::Backspace if ctrl => input.apply(Edit::DeleteWordLeft),
        C::Backspace => input.apply(Edit::Backspace),
        C::Delete => input.apply(Edit::Delete),
        C::Left if ctrl => input.apply(Edit::WordLeft),
        C::Right if ctrl => input.apply(Edit::WordRight),
        C::Left => input.apply(Edit::Left),
        C::Right => input.apply(Edit::Right),
        C::Home => input.apply(Edit::Home),
        C::End => input.apply(Edit::End),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn search_patterns() {
        assert!(search_match("ma*", "main"));
        assert!(search_match("ma*", "master"));
        assert!(search_match("ma*", "origin/main"));
        assert!(!search_match("ma*", "feature/xmain"));
        assert!(search_match("*main", "feature/xmain"));
        assert!(search_match("MA*", "main"));
        assert!(search_match("ai", "main"));
        assert!(search_match("v1.?", "v1.2"));
        assert!(!search_match("v1.?", "v1.23"));
        assert!(search_match("", "anything"));
    }

    #[test]
    fn list_filtering_keys() {
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let items: Vec<String> = ["H main", "H master", "H dev", "R origin/main"].map(String::from).into();
        let keys = items.iter().map(|s| s[2..].to_string()).collect();
        let values = items.iter().map(|s| s.as_bytes().to_vec()).collect();
        let mut d = Dialog::new("t", Kind::Info)
            .with_fields(vec![Field::searchable_list("l", items, values, keys, 5)]);
        d.on_key(key('?'));
        for c in "ma*".chars() {
            d.on_key(key(c));
        }
        let Some(Field::List { items, .. }) = d.fields.first() else { panic!() };
        assert_eq!(items, &["H main", "H master", "R origin/main"]);
        d.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        let Some(Field::List { items, search, .. }) = d.fields.first() else { panic!() };
        assert_eq!(items.len(), 4);
        assert!(!search.as_ref().unwrap().active);
    }
}
