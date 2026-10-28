//! Menu bar and context menus. Menus are rebuilt from the application state
//! every time they open, so enabled/checked states are always current.

use super::App;
use super::action::{Action, FileList};
use super::diffview::Show;
use super::history::HistoryState;
use crate::config::ViewKind;
use crate::domain::refs::RefKind;
use crate::git::read::WordMode;

#[derive(Clone, Debug)]
pub enum MenuItem {
    Entry {
        label: String,
        key: String,
        action: Action,
        enabled: bool,
        /// Some(state) for check / radio entries.
        check: Option<bool>,
    },
    Sub {
        label: String,
        items: Vec<MenuItem>,
    },
    Sep,
}

impl MenuItem {
    pub fn label(&self) -> &str {
        match self {
            MenuItem::Entry { label, .. } | MenuItem::Sub { label, .. } => label,
            MenuItem::Sep => "",
        }
    }
    pub fn selectable(&self) -> bool {
        match self {
            MenuItem::Entry { enabled, .. } => *enabled,
            MenuItem::Sub { items, .. } => !items.is_empty(),
            MenuItem::Sep => false,
        }
    }
}

/// Collects enabled leaf entries below `items`, prefixing each label with its menu path.
fn flatten_menu(prefix: &str, items: &[MenuItem], out: &mut Vec<(String, Action, bool)>) {
    for item in items {
        match item {
            MenuItem::Entry {
                label,
                key,
                action,
                enabled,
                ..
            } => {
                let mut shown = format!("{prefix} > {label}");
                if !key.is_empty() {
                    shown.push_str(&format!("  ({key})"));
                }
                out.push((shown, action.clone(), *enabled));
            }
            MenuItem::Sub { label, items } => {
                flatten_menu(&format!("{prefix} > {label}"), items, out);
            }
            _ => {}
        }
    }
}

fn e(label: &str, key: &str, action: Action) -> MenuItem {
    MenuItem::Entry {
        label: label.into(),
        key: key.into(),
        action,
        enabled: true,
        check: None,
    }
}

fn en(label: &str, key: &str, action: Action, enabled: bool) -> MenuItem {
    MenuItem::Entry {
        label: label.into(),
        key: key.into(),
        action,
        enabled,
        check: None,
    }
}

fn chk(label: &str, key: &str, action: Action, on: bool) -> MenuItem {
    MenuItem::Entry {
        label: label.into(),
        key: key.into(),
        action,
        enabled: true,
        check: Some(on),
    }
}

fn sub(label: &str, items: Vec<MenuItem>) -> MenuItem {
    MenuItem::Sub {
        label: label.into(),
        items,
    }
}

#[derive(Clone, Debug)]
pub struct Level {
    pub items: Vec<MenuItem>,
    pub sel: usize,
    /// First visible item when the box is too short for all items.
    pub scroll: usize,
    /// Top-left corner where the box is drawn.
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Debug)]
pub struct MenuState {
    /// Index of the menu bar title, None for context menus.
    pub bar: Option<usize>,
    pub levels: Vec<Level>,
    pub title: Option<String>,
}

impl MenuState {
    fn new(bar: Option<usize>, items: Vec<MenuItem>, x: u16, y: u16) -> MenuState {
        let sel = items.iter().position(MenuItem::selectable).unwrap_or(0);
        MenuState {
            bar,
            levels: vec![Level {
                items,
                sel,
                scroll: 0,
                x,
                y,
            }],
            title: None,
        }
    }

    pub fn top(&mut self) -> &mut Level {
        self.levels.last_mut().expect("menu has a level")
    }

    pub fn move_sel(&mut self, d: isize) {
        let l = self.top();
        let n = l.items.len() as isize;
        if n == 0 {
            return;
        }
        let mut i = l.sel as isize;
        for _ in 0..n {
            i = (i + d).rem_euclid(n);
            if l.items[i as usize].selectable() {
                l.sel = i as usize;
                return;
            }
        }
    }

    /// Opens the submenu under the selection, if any.
    pub fn open_sub(&mut self) -> bool {
        let depth = self.levels.len();
        let l = self.top();
        let (x, y, w) = (l.x, l.y, menu_width(&l.items));
        let sel = l.sel;
        if let Some(MenuItem::Sub { items, .. }) = l.items.get(sel) {
            let items = items.clone();
            let s = items.iter().position(MenuItem::selectable).unwrap_or(0);
            let _ = depth;
            self.levels.push(Level {
                items,
                sel: s,
                scroll: 0,
                x: x + w,
                y: y + sel as u16,
            });
            return true;
        }
        false
    }
}

/// Inner width of a menu box (without borders).
pub fn menu_width(items: &[MenuItem]) -> u16 {
    let w = items
        .iter()
        .map(|i| match i {
            MenuItem::Entry { label, key, .. } => {
                unicode_width::UnicodeWidthStr::width(label.as_str())
                    + 4
                    + if key.is_empty() {
                        0
                    } else {
                        unicode_width::UnicodeWidthStr::width(key.as_str()) + 3
                    }
            }
            MenuItem::Sub { label, .. } => {
                unicode_width::UnicodeWidthStr::width(label.as_str()) + 6
            }
            MenuItem::Sep => 3,
        })
        .max()
        .unwrap_or(10);
    w.clamp(10, 70) as u16 + 2
}

pub fn titles(view: ViewKind) -> &'static [&'static str] {
    match view {
        ViewKind::History => &["File", "Edit", "View", "Help"],
        ViewKind::Worktree => &[
            "Repository",
            "Edit",
            "Branch",
            "Commit",
            "Merge",
            "Remote",
            "Tools",
            "Help",
        ],
    }
}

/// x position of each menu title in the menu bar (the renderer uses it too).
pub fn title_positions(view: ViewKind) -> Vec<(u16, u16)> {
    let mut x = 1u16;
    titles(view)
        .iter()
        .map(|t| {
            let w = t.len() as u16 + 2;
            let r = (x, w);
            x += w;
            r
        })
        .collect()
}

/// Which context menu to open.
#[derive(Clone, Debug)]
pub enum Ctx {
    Commit(usize),
    Ref(Vec<u8>),
    HFile(usize),
    HDiff(usize),
    WtFile(FileList, usize),
    WtDiff(usize),
}

impl App {
    pub fn open_menu(&mut self, i: usize) {
        let n = titles(self.view).len();
        if n == 0 {
            return;
        }
        let i = i % n;
        let items = self.menu_items(i);
        let x = title_positions(self.view)[i].0;
        self.menu = Some(MenuState::new(Some(i), items, x, 1));
    }

    pub fn open_context(&mut self, c: Ctx, x: u16, y: u16) {
        let (title, items) = self.context_items(&c);
        if items.is_empty() {
            return;
        }
        let w = menu_width(&items) + 2;
        let h = items.len() as u16 + 2;
        let (sw, sh) = self.size;
        let x = x.min(sw.saturating_sub(w));
        let y = if y + 1 + h > sh {
            y.saturating_sub(h)
        } else {
            y + 1
        };
        let mut m = MenuState::new(None, items, x, y);
        m.title = title;
        self.menu = Some(m);
    }

    /// Every command of the current view, plus the context menu of the focused
    /// pane, flattened to "Menu > Submenu > Label (key)". The flag is false when
    /// the command is unavailable in the current state.
    pub fn palette_entries(&self) -> Vec<(String, Action, bool)> {
        let mut out = Vec::new();
        for (i, title) in titles(self.view).iter().enumerate() {
            flatten_menu(title, &self.menu_items(i), &mut out);
        }
        if let Some(c) = self.focused_ctx() {
            let (title, items) = self.context_items(&c);
            let prefix = match title {
                Some(t) => format!("Selection: {t}"),
                None => "Selection".to_string(),
            };
            flatten_menu(&prefix, &items, &mut out);
        }
        out
    }

    /// Activates the selected entry (or opens its submenu).
    pub fn menu_activate(&mut self) {
        let Some(m) = &mut self.menu else { return };
        if m.open_sub() {
            return;
        }
        let l = m.top();
        if let Some(MenuItem::Entry {
            action,
            enabled: true,
            ..
        }) = l.items.get(l.sel)
        {
            let a = action.clone();
            self.menu = None;
            self.dispatch(a);
        }
    }

    fn menu_items(&self, i: usize) -> Vec<MenuItem> {
        match self.view {
            ViewKind::History => self.history_menu(i),
            ViewKind::Worktree => self.worktree_menu(i),
        }
    }

    fn history_menu(&self, i: usize) -> Vec<MenuItem> {
        let h = &self.hist;
        match i {
            0 => vec![
                e("Update", "F5", Action::Update),
                e("Reload", "Shift-F5", Action::Reload),
                e("Reread references", "", Action::RereadRefs),
                e("List references", "F2", Action::ListRefs),
                MenuItem::Sep,
                en("Start git gui", "F6", Action::StartGitGui, !self.is_bare()),
                MenuItem::Sep,
                e("Quit", "Ctrl-Q / Ctrl-W", Action::Quit),
            ],
            1 => vec![
                e("Preferences", "", Action::Preferences),
                e("Copy", "Ctrl-C", Action::Copy),
            ],
            2 => {
                let mut v = vec![
                    e("New view...", "Shift-F4", Action::NewView),
                    en("Edit view...", "F4", Action::EditView, h.cur_view > 0),
                    en("Delete view", "", Action::DeleteView, h.cur_view > 0),
                    MenuItem::Sep,
                ];
                for (k, view) in h.views.iter().enumerate() {
                    v.push(chk(&view.name, "", Action::SelectView(k), k == h.cur_view));
                }
                v.push(MenuItem::Sep);
                v.push(sub(
                    "Diff display",
                    vec![
                        chk(
                            "Diff",
                            "",
                            Action::SetShow(Show::Diff),
                            h.show == Show::Diff,
                        ),
                        chk(
                            "Old version",
                            "",
                            Action::SetShow(Show::Old),
                            h.show == Show::Old,
                        ),
                        chk(
                            "New version",
                            "",
                            Action::SetShow(Show::New),
                            h.show == Show::New,
                        ),
                        MenuItem::Sep,
                        e("More context", "Ctrl-+", Action::ContextMore),
                        e("Less context", "Ctrl--", Action::ContextLess),
                        chk(
                            "Ignore space change",
                            "",
                            Action::ToggleIgnoreSpace,
                            h.ignore_space,
                        ),
                        MenuItem::Sep,
                        chk(
                            "Line diff",
                            "",
                            Action::SetWordMode(WordMode::Line),
                            h.word_mode == WordMode::Line,
                        ),
                        chk(
                            "Markup words",
                            "",
                            Action::SetWordMode(WordMode::Markup),
                            h.word_mode == WordMode::Markup,
                        ),
                        chk(
                            "Color words",
                            "",
                            Action::SetWordMode(WordMode::Color),
                            h.word_mode == WordMode::Color,
                        ),
                    ],
                ));
                v.push(sub(
                    "File list",
                    vec![
                        chk("Patch", "", Action::SetTreeMode(false), !h.tree_mode),
                        chk("Tree", "", Action::SetTreeMode(true), h.tree_mode),
                    ],
                ));
                v.push(chk(
                    "Compact refs",
                    "",
                    Action::ToggleCompactRefs,
                    h.compact_refs,
                ));
                v.push(e("Maximize pane", "F11", Action::ToggleMaximize));
                v.push(en(
                    "Working tree view",
                    "F6",
                    Action::SwitchView(ViewKind::Worktree),
                    !self.is_bare(),
                ));
                v
            }
            _ => vec![
                e("Command palette", "F12", Action::CommandPalette),
                MenuItem::Sep,
                e("About Git Twin (gitt)", "", Action::AboutGitk),
                e("Messages log", "", Action::ShowMessageLog),
                e("Key bindings", "F1", Action::KeyBindings),
            ],
        }
    }

    fn worktree_menu(&self, i: usize) -> Vec<MenuItem> {
        let busy_op = !self.repo_state.operations.is_empty();
        let merging = self
            .repo_state
            .operations
            .contains(&crate::git::repo::Operation::Merge);
        match i {
            0 => vec![
                e("Explore Working Copy", "", Action::ExploreWorkingCopy),
                MenuItem::Sep,
                e(
                    "Browse Current Branch's Files",
                    "",
                    Action::BrowseFiles(None),
                ),
                e("Browse Branch Files...", "", Action::BrowseBranchFiles),
                MenuItem::Sep,
                e(
                    "Visualize Current Branch's History",
                    "F6",
                    Action::VisualizeCurrent,
                ),
                e("Visualize All Branch History", "", Action::VisualizeAll),
                MenuItem::Sep,
                e("Database Statistics", "", Action::DbStats),
                e("Compress Database", "", Action::DbCompress),
                e("Verify Database", "", Action::DbVerify),
                MenuItem::Sep,
                e("Quit", "Ctrl-Q / Ctrl-W", Action::Quit),
            ],
            1 => vec![
                en("Undo", "Ctrl-Z", Action::Undo, self.wt.message.can_undo()),
                en("Redo", "Ctrl-Y", Action::Redo, self.wt.message.can_redo()),
                MenuItem::Sep,
                e("Cut", "Ctrl-X", Action::Cut),
                e("Copy", "Ctrl-C", Action::Copy),
                e("Paste", "Ctrl-V", Action::Paste),
                e("Delete", "Del", Action::DeleteSel),
                MenuItem::Sep,
                e("Select All", "Ctrl-A", Action::SelectAll),
                MenuItem::Sep,
                e(
                    "Edit message in $GIT_EDITOR",
                    "F4",
                    Action::EditMessageExternally,
                ),
                e("Options...", "", Action::WtOptions),
            ],
            2 => vec![
                e("Create...", "Ctrl-N", Action::CreateBranch),
                e("Checkout...", "Ctrl-O", Action::CheckoutBranch),
                e("Rename...", "", Action::RenameBranchDialog),
                e("Delete...", "", Action::DeleteBranchDialog),
                en("Reset...", "", Action::ResetHard, self.head().is_some()),
            ],
            3 => {
                let staged_side = self.wt.focus == super::worktree::WFocus::Staged;
                vec![
                    chk("New Commit", "", Action::SetAmend(false), !self.wt.amend),
                    chk(
                        "Amend Last Commit",
                        "Ctrl-E",
                        Action::SetAmend(true),
                        self.wt.amend,
                    ),
                    MenuItem::Sep,
                    e("Rescan", "F5", Action::Rescan),
                    en(
                        "Stage To Commit",
                        "Ctrl-T",
                        Action::StageSelected,
                        !staged_side,
                    ),
                    e(
                        "Stage Visible Changed Files To Commit",
                        "Ctrl-I",
                        Action::StageChanged,
                    ),
                    en(
                        "Unstage From Commit",
                        "Ctrl-U",
                        Action::UnstageSelected,
                        staged_side,
                    ),
                    e("Revert Changes", "Ctrl-J", Action::RevertSelected),
                    en(
                        "Undo Last Revert",
                        "",
                        Action::UndoLastRevert,
                        self.wt.last_revert.is_some(),
                    ),
                    MenuItem::Sep,
                    e("Show Less Context", "Ctrl--", Action::WtContextLess),
                    e("Show More Context", "Ctrl-+", Action::WtContextMore),
                    MenuItem::Sep,
                    e("Sign Off", "Ctrl-S", Action::SignOff),
                    e("Commit", "Ctrl-Enter", Action::Commit),
                ]
            }
            4 => {
                let mut v = vec![
                    en("Local Merge...", "Ctrl-M", Action::LocalMerge, !busy_op),
                    en("Abort Merge...", "", Action::AbortMerge, merging),
                ];
                if busy_op {
                    let op = self.repo_state.operations[0].label();
                    v.push(MenuItem::Sep);
                    v.push(e(&format!("Continue {op}"), "", Action::ContinueOp));
                    v.push(e(&format!("Abort {op}"), "", Action::AbortOp));
                }
                v
            }
            5 => {
                let mut v = Vec::new();
                if !self.remotes.is_empty() {
                    let mk = |f: fn(String) -> Action| -> Vec<MenuItem> {
                        self.remotes
                            .iter()
                            .map(|r| e(r, "", f(r.clone())))
                            .collect()
                    };
                    v.push(sub("Fetch from", mk(Action::Fetch)));
                    v.push(sub("Prune from", mk(Action::Prune)));
                    v.push(sub("Remove Remote", mk(Action::RemoveRemote)));
                    v.push(MenuItem::Sep);
                }
                v.push(e("Add...", "", Action::AddRemote));
                v.push(e("Push...", "Ctrl-P", Action::Push));
                v.push(e("Delete Branch...", "", Action::DeleteRemoteBranch));
                v
            }
            6 => {
                let mut v: Vec<MenuItem> = self
                    .guitools
                    .iter()
                    .enumerate()
                    .map(|(k, t)| e(&t.name, "", Action::RunTool(k)))
                    .collect();
                if v.is_empty() {
                    v.push(en(
                        "(no tools: set guitool.<name>.cmd)",
                        "",
                        Action::Noop,
                        false,
                    ));
                }
                v
            }
            _ => vec![
                e("Command palette", "F12", Action::CommandPalette),
                MenuItem::Sep,
                e("About Git TUI (git tui)", "", Action::AboutGitGui),
                e("Show SSH Key", "", Action::ShowSshKey),
                e("Messages log", "", Action::ShowMessageLog),
                e("Key bindings", "F1", Action::KeyBindings),
            ],
        }
    }

    fn open_in_editor_item(&self, path: Vec<u8>) -> MenuItem {
        let ed = self.cfg.editor.as_str();
        if ed.is_empty() {
            en(
                "Open in editor (none set in Preferences)",
                "",
                Action::OpenInEditor(path),
                false,
            )
        } else {
            e(&format!("Open in {ed}"), "", Action::OpenInEditor(path))
        }
    }

    fn context_items(&self, c: &Ctx) -> (Option<String>, Vec<MenuItem>) {
        match c {
            Ctx::Commit(row) => {
                let Some(cm) = self.hist.commits.get(*row) else {
                    return (None, vec![]);
                };
                if HistoryState::is_pseudo(&cm.oid) {
                    return (
                        Some("Local changes".into()),
                        vec![
                            e(
                                "Open working tree view",
                                "",
                                Action::SwitchView(ViewKind::Worktree),
                            ),
                            en("Stash changes", "", Action::StashChanges, !self.read_only),
                            e("Rescan", "", Action::Rescan),
                        ],
                    );
                }
                let has_sel = self
                    .hist
                    .selected_oid()
                    .is_some_and(|o| o != cm.oid && !HistoryState::is_pseudo(&o));
                let marked = self.hist.mark.is_some();
                let detached = matches!(self.refs.head, crate::domain::refs::Head::Detached(_));
                let ro = !self.read_only;
                (
                    Some(cm.oid.short(10)),
                    vec![
                        en(
                            "Diff this -> selected",
                            "",
                            Action::DiffThisSelected(*row),
                            has_sel,
                        ),
                        en(
                            "Diff selected -> this",
                            "",
                            Action::DiffSelectedThis(*row),
                            has_sel,
                        ),
                        e("Make patch", "", Action::MakePatch(*row)),
                        en("Create tag...", "", Action::CreateTag(*row), ro),
                        e("Copy commit reference", "", Action::CopyCommitRef(*row)),
                        e("Copy commit ID", "", Action::CopyCommitId(*row)),
                        e("Write commit to file", "", Action::WriteCommit(*row)),
                        en("Create new branch", "", Action::CreateBranchAt(*row), ro),
                        en("Cherry-pick this commit", "", Action::CherryPick(*row), ro),
                        en(
                            if detached {
                                "Detached head: can't reset"
                            } else {
                                "Reset current branch to this commit..."
                            },
                            "",
                            Action::ResetHere(*row),
                            ro && !detached,
                        ),
                        e("Mark this commit", "", Action::MarkCommit(*row)),
                        en("Return to mark", "", Action::ReturnToMark, marked),
                        en(
                            "Find descendant of this and mark",
                            "",
                            Action::FindDescendant(*row),
                            marked,
                        ),
                        en(
                            "Compare with marked commit",
                            "",
                            Action::CompareWithMarked(*row),
                            marked,
                        ),
                        en(
                            "Diff this -> marked commit",
                            "",
                            Action::DiffMarked(*row, true),
                            marked,
                        ),
                        en(
                            "Diff marked commit -> this",
                            "",
                            Action::DiffMarked(*row, false),
                            marked,
                        ),
                        en("Revert this commit", "", Action::RevertCommit(*row), ro),
                        en(
                            "Check out this commit",
                            "",
                            Action::CheckoutCommit(*row),
                            ro,
                        ),
                    ],
                )
            }
            Ctx::Ref(full) => {
                let Some(r) = self.refs.find_full(full) else {
                    return (None, vec![]);
                };
                let local = r.kind == RefKind::LocalBranch;
                let is_head = self.refs.is_head_ref(r);
                let ro = !self.read_only;
                (
                    Some(r.short.clone()),
                    vec![
                        en(
                            "Check out this branch",
                            "",
                            Action::CheckoutRef(full.clone()),
                            ro && !is_head,
                        ),
                        en(
                            "Rename branch...",
                            "",
                            Action::RenameBranch(full.clone()),
                            ro && local,
                        ),
                        en(
                            "Remove this branch",
                            "",
                            Action::RemoveBranch(full.clone()),
                            ro && local && !is_head,
                        ),
                        e("Copy branch name", "", Action::CopyRefName(full.clone())),
                    ],
                )
            }
            Ctx::HFile(i) => {
                let Some(p) = self.hist.file_path(*i) else {
                    return (None, vec![]);
                };
                (
                    Some(crate::domain::sanitize::display_bytes(&p)),
                    vec![
                        e(
                            "Highlight this too",
                            "",
                            Action::HighlightPath(p.clone(), false),
                        ),
                        e(
                            "Highlight this only",
                            "",
                            Action::HighlightPath(p.clone(), true),
                        ),
                        en(
                            "External diff",
                            "",
                            Action::ExternalDiff(p.clone()),
                            !self.hist.tree_mode && !self.read_only,
                        ),
                        e("Blame parent commit", "", Action::BlameParent(p.clone())),
                        self.open_in_editor_item(p.clone()),
                        e("Copy path", "", Action::CopyPath(p)),
                    ],
                )
            }
            Ctx::HDiff(idx) => {
                let has_file = self
                    .hist
                    .diff
                    .lines
                    .get(*idx)
                    .and_then(|l| l.file)
                    .is_some();
                (
                    None,
                    vec![
                        en(
                            "Show origin of this line",
                            "",
                            Action::LineOrigin(*idx),
                            has_file && !self.hist.tree_mode,
                        ),
                        e("Copy", "c", Action::Copy),
                        MenuItem::Sep,
                        chk(
                            "Diff",
                            "",
                            Action::SetShow(Show::Diff),
                            self.hist.show == Show::Diff,
                        ),
                        chk(
                            "Old version",
                            "",
                            Action::SetShow(Show::Old),
                            self.hist.show == Show::Old,
                        ),
                        chk(
                            "New version",
                            "",
                            Action::SetShow(Show::New),
                            self.hist.show == Show::New,
                        ),
                    ],
                )
            }
            Ctx::WtFile(l, i) => {
                let Some(f) = self.wt.list(*l).get(*i) else {
                    return (None, vec![]);
                };
                let mut v = match l {
                    FileList::Unstaged => vec![
                        e("Stage To Commit", "Ctrl-T", Action::StageToggle(*l, *i)),
                        e("Revert Changes", "Ctrl-J", Action::RevertSelected),
                    ],
                    FileList::Staged => vec![e(
                        "Unstage From Commit",
                        "Ctrl-U",
                        Action::StageToggle(*l, *i),
                    )],
                };
                if f.unmerged {
                    v.push(MenuItem::Sep);
                    v.push(e(
                        "Use Local Version (ours)",
                        "",
                        Action::UseVersion(f.path.clone(), true),
                    ));
                    v.push(e(
                        "Use Remote Version (theirs)",
                        "",
                        Action::UseVersion(f.path.clone(), false),
                    ));
                    v.push(e(
                        "Run Merge Tool",
                        "",
                        Action::RunMergeTool(f.path.clone()),
                    ));
                }
                v.push(MenuItem::Sep);
                v.push(self.open_in_editor_item(f.path.clone()));
                v.push(e("Copy path", "", Action::CopyPath(f.path.clone())));
                (Some(f.display.clone()), v)
            }
            Ctx::WtDiff(idx) => {
                let staged = self.wt.diff_side() == Some(FileList::Staged);
                let on_hunk =
                    self.wt.diff.hunk_at(*idx).is_some() && self.wt.diff.text_range().is_none();
                let lines = self.wt.diff.selected_lines().is_some();
                let mut v = if staged {
                    vec![
                        en(
                            "Unstage Hunk From Commit",
                            "s",
                            Action::StageHunk(*idx),
                            on_hunk,
                        ),
                        en(
                            "Unstage Line(s) From Commit",
                            "s",
                            Action::StageLines,
                            lines,
                        ),
                    ]
                } else {
                    vec![
                        en(
                            "Stage Hunk For Commit",
                            "s",
                            Action::StageHunk(*idx),
                            on_hunk,
                        ),
                        en("Stage Line(s) For Commit", "s", Action::StageLines, lines),
                        en("Revert Hunk", "r", Action::RevertHunk(*idx), on_hunk),
                        en("Revert Line(s)", "r", Action::RevertLines, lines),
                    ]
                };
                v.push(en(
                    "Undo Last Revert",
                    "",
                    Action::UndoLastRevert,
                    self.wt.last_revert.is_some(),
                ));
                v.push(MenuItem::Sep);
                v.push(e("Show Less Context", "Ctrl--", Action::WtContextLess));
                v.push(e("Show More Context", "Ctrl-+", Action::WtContextMore));
                v.push(MenuItem::Sep);
                v.push(e("Refresh", "F5", Action::Rescan));
                v.push(e("Copy", "Ctrl-C", Action::Copy));
                v.push(e("Select All", "Ctrl-A", Action::SelectAll));
                v.push(e("Options...", "", Action::WtOptions));
                (None, v)
            }
        }
    }
}
