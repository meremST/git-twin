//! Working tree view (git gui): unstaged / staged lists, diff with partial
//! staging, commit message, amend, and the git gui menu commands.

use std::collections::{BTreeSet, HashSet};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::sync::Arc;

use super::action::{Action, FileList};
use super::dialog::{Dialog, Field, FormKind, Kind, Tone};
use super::diffview::{BuildOpts, DKind, DLine, DiffView, Show, WordStyle};
use super::input::TextArea;
use super::worker::{After, Payload, WtSnapshot};
use super::{App, Handoff, Pending, StatusTone};
use crate::config::{Config, Handoff as HandoffCfg, StageUntracked};
use crate::domain::diff::Diff;
use crate::domain::patch::{self, Direction};
use crate::domain::sanitize;
use crate::domain::status::EntryKind;
use crate::git::read::{self, DiffOpts};
use crate::git::runner::os;
use crate::git::{GitError, ops};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WFocus {
    Unstaged,
    Staged,
    Diff,
    Message,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WFile {
    pub path: Vec<u8>,
    pub display: String,
    pub kind: EntryKind,
    pub x: u8,
    pub y: u8,
    pub untracked: bool,
    pub unmerged: bool,
    pub typechange: bool,
}

impl WFile {
    /// git gui style two-letter state for the icon column.
    pub fn icon_char(&self, staged: bool) -> char {
        if self.untracked {
            return '?';
        }
        if self.unmerged {
            return 'U';
        }
        let c = if staged { self.x } else { self.y };
        match c {
            b'.' => ' ',
            c => c as char,
        }
    }

    pub fn describe(&self, staged: bool) -> &'static str {
        if self.untracked {
            return "Untracked, not staged";
        }
        if self.unmerged {
            return "Requires merge resolution";
        }
        let c = if staged { self.x } else { self.y };
        match (staged, c) {
            (true, b'A') => "Staged for commit (new file)",
            (true, b'D') => "Staged for removal",
            (true, b'T') => "Staged for commit (type change)",
            (true, _) => "Staged for commit",
            (false, b'D') => "Missing",
            (false, b'T') => "File type changed, not staged",
            (false, _) if self.x != b'.' => "Portions staged for commit",
            (false, _) => "Modified, not staged",
        }
    }
}

pub struct WorktreeState {
    pub unstaged: Vec<WFile>,
    pub staged: Vec<WFile>,
    pub sel_unstaged: BTreeSet<Vec<u8>>,
    pub sel_staged: BTreeSet<Vec<u8>>,
    /// File whose diff is shown: (list, path).
    pub current: Option<(FileList, Vec<u8>)>,
    /// Anchor for shift-click ranges (index in `last_list`).
    pub anchor: Option<usize>,
    pub last_list: FileList,
    pub top_unstaged: usize,
    pub top_staged: usize,
    pub height_unstaged: usize,
    pub height_staged: usize,
    pub message: TextArea,
    pub branch_label: String,
    pub last_revert: Option<Vec<u8>>,
    pub amend: bool,
    pub saved_message: Option<String>,
    pub focus: WFocus,
    pub diff: DiffView,
    pub loading: bool,
    pub context: u32,
    pub snapshot: Option<WtSnapshot>,
    /// Message area geometry (updated when drawing).
    pub msg_height: usize,
    pub msg_width: usize,
    pub tab: usize,
}

impl WorktreeState {
    pub fn new(cfg: &Config) -> WorktreeState {
        WorktreeState {
            unstaged: Vec::new(),
            staged: Vec::new(),
            sel_unstaged: BTreeSet::new(),
            sel_staged: BTreeSet::new(),
            current: None,
            anchor: None,
            last_list: FileList::Unstaged,
            top_unstaged: 0,
            top_staged: 0,
            height_unstaged: 5,
            height_staged: 5,
            message: TextArea::new(""),
            branch_label: String::new(),
            last_revert: None,
            amend: false,
            saved_message: None,
            focus: WFocus::Unstaged,
            diff: DiffView::default(),
            loading: false,
            context: cfg.worktree_context,
            snapshot: None,
            msg_height: 5,
            msg_width: 60,
            tab: cfg.tab_width as usize,
        }
    }

    pub fn cycle_focus(&mut self, dir: isize) {
        const ORDER: [WFocus; 4] = [
            WFocus::Unstaged,
            WFocus::Staged,
            WFocus::Diff,
            WFocus::Message,
        ];
        let i = ORDER.iter().position(|f| *f == self.focus).unwrap_or(0) as isize;
        self.focus = ORDER[(i + dir).rem_euclid(4) as usize];
    }

    pub fn list(&self, l: FileList) -> &[WFile] {
        match l {
            FileList::Unstaged => &self.unstaged,
            FileList::Staged => &self.staged,
        }
    }

    pub fn sel(&self, l: FileList) -> &BTreeSet<Vec<u8>> {
        match l {
            FileList::Unstaged => &self.sel_unstaged,
            FileList::Staged => &self.sel_staged,
        }
    }

    fn sel_mut(&mut self, l: FileList) -> &mut BTreeSet<Vec<u8>> {
        match l {
            FileList::Unstaged => &mut self.sel_unstaged,
            FileList::Staged => &mut self.sel_staged,
        }
    }

    pub fn top_mut(&mut self, l: FileList) -> (&mut usize, usize) {
        match l {
            FileList::Unstaged => (&mut self.top_unstaged, self.height_unstaged),
            FileList::Staged => (&mut self.top_staged, self.height_staged),
        }
    }

    pub fn index_of(&self, l: FileList, path: &[u8]) -> Option<usize> {
        self.list(l).iter().position(|f| f.path == path)
    }

    /// File under the cursor (the one whose diff is displayed).
    pub fn cursor_file(&self) -> Option<&WFile> {
        let (l, p) = self.current.as_ref()?;
        self.list(*l).iter().find(|f| &f.path == p)
    }

    pub fn cursor_index(&self) -> Option<(FileList, usize)> {
        let (l, p) = self.current.as_ref()?;
        self.index_of(*l, p).map(|i| (*l, i))
    }

    /// Paths acted upon in list `l`: the selection, or the cursor file.
    pub fn targets(&self, l: FileList) -> Vec<Vec<u8>> {
        let s = self.sel(l);
        let v: Vec<Vec<u8>> = self
            .list(l)
            .iter()
            .filter(|f| s.contains(&f.path))
            .map(|f| f.path.clone())
            .collect();
        if !v.is_empty() {
            return v;
        }
        match &self.current {
            Some((cl, p)) if *cl == l && self.index_of(l, p).is_some() => vec![p.clone()],
            _ => Vec::new(),
        }
    }

    pub fn scroll_list(&mut self, l: FileList, delta: isize) {
        let n = self.list(l).len();
        let (top, h) = self.top_mut(l);
        let max = n.saturating_sub(h.max(1));
        *top = (*top as isize + delta).clamp(0, max as isize) as usize;
    }

    fn ensure_visible(&mut self, l: FileList, i: usize) {
        let (top, h) = self.top_mut(l);
        let h = h.max(1);
        if i < *top {
            *top = i;
        } else if i >= *top + h {
            *top = i + 1 - h;
        }
    }

    pub fn diff_side(&self) -> Option<FileList> {
        self.current.as_ref().map(|(l, _)| *l)
    }
}

fn wfile(path: Vec<u8>, kind: EntryKind, x: u8, y: u8) -> WFile {
    WFile {
        display: sanitize::display_bytes(&path),
        untracked: kind == EntryKind::Untracked,
        unmerged: kind == EntryKind::Unmerged,
        typechange: x == b'T' || y == b'T',
        path,
        kind,
        x,
        y,
    }
}

fn has_conflict_markers(workdir: &std::path::Path, rel: &[u8]) -> bool {
    let Ok(p) = ops::safe_join(workdir, rel) else {
        return false;
    };
    let Ok(data) = std::fs::read(p) else {
        return false;
    };
    data.split(|c| *c == b'\n')
        .any(|l| l.starts_with(b"<<<<<<< ") || l.starts_with(b">>>>>>> ") || l == b"=======")
}

impl App {
    fn wt_diff_opts(&self) -> DiffOpts {
        DiffOpts {
            context: self.wt.context,
            ..Default::default()
        }
    }

    /// Re-reads the working tree status.
    pub fn rescan(&mut self) {
        if self.is_bare() {
            return;
        }
        self.wt.loading = true;
        let r = self.runner.clone();
        let info = self.repo.clone();
        let amend = self.wt.amend;
        self.req.status = self.workers.read(self.generation, move || {
            let res = (|| -> Result<WtSnapshot, GitError> {
                let status = read::status(&r)?;
                let state = crate::git::repo::repo_state(&info);
                let head = status.branch.oid;
                let (amend_staged, amend_base) = if amend && head.is_some() {
                    let base = read::resolve_commit(&r, "HEAD^").ok();
                    (Some(read::staged_against(&r, base.as_ref())?), base)
                } else {
                    (None, None)
                };
                Ok(WtSnapshot {
                    status,
                    state,
                    amend_staged,
                    amend_base,
                    head,
                })
            })();
            Payload::Status(res)
        });
    }

    pub fn on_status(&mut self, r: Result<WtSnapshot, GitError>) {
        self.wt.loading = false;
        let snap = match r {
            Ok(s) => s,
            Err(e) => {
                self.set_status(
                    format!("Rescan failed: {}", e.message.lines().next().unwrap_or("")),
                    StatusTone::Error,
                );
                return;
            }
        };
        let mut unstaged = Vec::new();
        let mut staged = Vec::new();
        let mut flags = (false, false);
        for e in &snap.status.entries {
            if e.kind == EntryKind::Ignored {
                continue;
            }
            let tracked = e.kind != EntryKind::Untracked;
            if e.has_unstaged() {
                unstaged.push(wfile(e.path.as_bytes().to_vec(), e.kind, e.x, e.y));
                flags.0 |= tracked;
            }
            if e.has_staged() {
                flags.1 = true;
                if snap.amend_staged.is_none() {
                    staged.push(wfile(e.path.as_bytes().to_vec(), e.kind, e.x, e.y));
                }
            }
        }
        if let Some(a) = &snap.amend_staged {
            for (st, p) in a {
                staged.push(wfile(p.clone(), EntryKind::Changed, *st, b'.'));
            }
        }
        unstaged.sort_by(|a, b| a.path.cmp(&b.path));
        staged.sort_by(|a, b| a.path.cmp(&b.path));
        let wt = &mut self.wt;
        let u: HashSet<&[u8]> = unstaged.iter().map(|f| f.path.as_slice()).collect();
        let s: HashSet<&[u8]> = staged.iter().map(|f| f.path.as_slice()).collect();
        wt.sel_unstaged.retain(|p| u.contains(p.as_slice()));
        wt.sel_staged.retain(|p| s.contains(p.as_slice()));
        // Keep the displayed file; if it moved to the other list follow it.
        let cur = wt.current.take();
        wt.current = match cur {
            Some((l, p)) => {
                let (here, there) = match l {
                    FileList::Unstaged => (&u, &s),
                    FileList::Staged => (&s, &u),
                };
                if here.contains(p.as_slice()) {
                    Some((l, p))
                } else if there.contains(p.as_slice()) {
                    let other = if l == FileList::Unstaged {
                        FileList::Staged
                    } else {
                        FileList::Unstaged
                    };
                    Some((other, p))
                } else {
                    None
                }
            }
            None => None,
        };
        drop(u);
        drop(s);
        wt.unstaged = unstaged;
        wt.staged = staged;
        if wt.current.is_none() {
            if let Some(f) = wt.unstaged.first() {
                wt.current = Some((FileList::Unstaged, f.path.clone()));
            } else if let Some(f) = wt.staged.first() {
                wt.current = Some((FileList::Staged, f.path.clone()));
            }
        }
        wt.scroll_list(FileList::Unstaged, 0);
        wt.scroll_list(FileList::Staged, 0);
        self.repo_state = snap.state.clone();
        let head_changed = self.wt.snapshot.as_ref().map(|o| o.head) != Some(snap.head);
        self.wt.snapshot = Some(snap);
        // Keep gitk's local-changes rows in sync.
        if self.hist.view().includes_head()
            && !self.hist.loading
            && (flags != self.hist.local_flags || head_changed)
        {
            if self.hist.local_flags != flags {
                self.update_local_history(crate::git::log::LocalRows {
                    unstaged: flags.0,
                    staged: flags.1,
                    head: self.wt.snapshot.as_ref().and_then(|s| s.head),
                });
            }
        } else if let Some(o) = self.hist.selected_oid()
            && super::history::HistoryState::is_pseudo(&o)
        {
            self.hist.sel_changed_at = Some(std::time::Instant::now());
        }
        self.load_wt_diff(true);
    }

    /// Loads the diff of the current file.
    pub fn load_wt_diff(&mut self, keep_scroll: bool) {
        let Some((list, path)) = self.wt.current.clone() else {
            self.wt.diff.clear();
            self.req.wtdiff = 0;
            return;
        };
        let Some(f) = self.wt.list(list).iter().find(|f| f.path == path).cloned() else {
            self.wt.diff.clear();
            return;
        };
        if !keep_scroll {
            self.wt.diff.reset_scroll();
        }
        let r = self.runner.clone();
        let opts = self.wt_diff_opts();
        let amend = self
            .wt
            .snapshot
            .as_ref()
            .and_then(|s| s.amend_staged.as_ref().map(|_| s.amend_base));
        self.req.wtdiff = self.workers.read(self.generation, move || {
            let p: &[u8] = &path;
            let d = match list {
                FileList::Unstaged if f.untracked => read::untracked_diff(&r, p, &opts),
                FileList::Unstaged => read::worktree_diff(&r, false, None, &opts, &[p]),
                FileList::Staged => match amend {
                    Some(Some(base)) => read::worktree_diff(&r, true, Some(&base), &opts, &[p]),
                    // Amending the root commit: compare with the empty tree.
                    Some(None) => read::empty_tree(&r).and_then(|t| {
                        let base =
                            crate::domain::oid::Oid::from_hex(t.trim()).ok_or_else(|| {
                                GitError::new("git hash-object", "invalid empty tree id")
                            })?;
                        read::worktree_diff(&r, true, Some(&base), &opts, &[p])
                    }),
                    None => read::worktree_diff(&r, true, None, &opts, &[p]),
                },
            };
            Payload::WtDiff(d)
        });
    }

    pub fn on_wt_diff(&mut self, r: Result<Diff, GitError>) {
        let Some(f) = self.wt.cursor_file().cloned() else {
            self.wt.diff.clear();
            return;
        };
        let staged = self.wt.diff_side() == Some(FileList::Staged);
        let mut prefix = vec![
            DLine::plain(DKind::Header, f.describe(staged).to_string()),
            DLine::plain(DKind::FileTitle, format!("File: {}", f.display)),
        ];
        if f.unmerged {
            prefix.push(DLine::plain(
                DKind::Info,
                "Conflict: resolve the file, then stage it (or use Merge > Use version).",
            ));
        }
        prefix.push(DLine::plain(DKind::Header, String::new()));
        match r {
            Ok(d) => {
                let o = BuildOpts {
                    tab: self.wt.tab,
                    show: Show::Diff,
                    words: WordStyle::Markup,
                    compact_headers: true,
                };
                let empty = d.files.is_empty();
                self.wt.diff.set(prefix, Some(Arc::new(d)), &o);
                if empty {
                    self.wt
                        .diff
                        .lines
                        .push(DLine::plain(DKind::Info, "No differences."));
                }
            }
            Err(e) => {
                self.wt.diff.set(
                    prefix,
                    None,
                    &BuildOpts {
                        tab: 8,
                        show: Show::Diff,
                        words: WordStyle::Markup,
                        compact_headers: true,
                    },
                );
                self.wt
                    .diff
                    .lines
                    .push(DLine::plain(DKind::Error, e.to_string()));
            }
        }
    }

    pub fn on_amend_message(&mut self, r: Result<String, GitError>) {
        match r {
            Ok(m) => {
                if self.wt.saved_message.is_none() {
                    self.wt.saved_message = Some(self.wt.message.text());
                }
                self.wt.message.set_text(m.trim_end());
            }
            Err(e) => {
                self.wt.amend = false;
                self.error("Amend Last Commit", e);
            }
        }
    }

    // ------------------------------------------------------------------
    // Selection

    fn select_file(&mut self, l: FileList, i: usize) {
        let Some(f) = self.wt.list(l).get(i).cloned() else {
            return;
        };
        self.wt.last_list = l;
        self.wt.anchor = Some(i);
        self.wt.sel_unstaged.clear();
        self.wt.sel_staged.clear();
        self.wt.sel_mut(l).insert(f.path.clone());
        self.wt.focus = match l {
            FileList::Unstaged => WFocus::Unstaged,
            FileList::Staged => WFocus::Staged,
        };
        self.wt.ensure_visible(l, i);
        let same = self.wt.current.as_ref() == Some(&(l, f.path.clone()));
        self.wt.current = Some((l, f.path));
        if !same {
            self.load_wt_diff(false);
        }
    }

    fn move_file(&mut self, d: isize) {
        let l = match self.wt.focus {
            WFocus::Staged => FileList::Staged,
            WFocus::Unstaged => FileList::Unstaged,
            _ => self.wt.last_list,
        };
        let n = self.wt.list(l).len();
        if n == 0 {
            return;
        }
        let cur = match self.wt.cursor_index() {
            Some((cl, i)) if cl == l => i as isize,
            _ => -1,
        };
        let i = (cur + d).clamp(0, n as isize - 1) as usize;
        self.select_file(l, i);
    }

    // ------------------------------------------------------------------
    // Whole-file staging

    fn stage_paths(&mut self, paths: Vec<Vec<u8>>) {
        if paths.is_empty() {
            self.set_status("No file selected.", StatusTone::Info);
            return;
        }
        let unmerged: Vec<Vec<u8>> = paths
            .iter()
            .filter(|p| self.wt.unstaged.iter().any(|f| &f.path == *p && f.unmerged))
            .cloned()
            .collect();
        let workdir = self.repo.cwd().to_path_buf();
        if let Some(p) = unmerged.iter().find(|p| has_conflict_markers(&workdir, p)) {
            let shown = sanitize::display_bytes(p);
            self.confirm(
                "Stage file",
                &[(
                    &format!("File {shown} seems to have unresolved conflicts, still stage?"),
                    Tone::Warn,
                )],
                "Stage",
                Pending::Op {
                    label: "Stage".into(),
                    f: Box::new(move |r| ops::stage(r, &paths)),
                    after: After::Rescan,
                    expect_head: None,
                },
            );
            return;
        }
        let n = paths.len();
        self.run_op(
            &format!("Stage {n} file(s)"),
            Box::new(move |r| ops::stage(r, &paths)),
            After::Rescan,
            None,
        );
    }

    fn unstage_paths(&mut self, paths: Vec<Vec<u8>>) {
        if paths.is_empty() {
            self.set_status("No file selected.", StatusTone::Info);
            return;
        }
        let snap = self.wt.snapshot.as_ref();
        let base = match snap.and_then(|s| s.amend_staged.as_ref().map(|_| s.amend_base)) {
            Some(b) => b,
            None => snap.and_then(|s| s.head),
        };
        let n = paths.len();
        self.run_op(
            &format!("Unstage {n} file(s)"),
            Box::new(move |r| ops::unstage(r, &paths, base.as_ref())),
            After::Rescan,
            None,
        );
    }

    fn toggle_stage(&mut self, l: FileList, paths: Vec<Vec<u8>>) {
        match l {
            FileList::Unstaged => self.stage_paths(paths),
            FileList::Staged => self.unstage_paths(paths),
        }
    }

    fn focused_list(&self) -> FileList {
        match self.wt.focus {
            WFocus::Staged => FileList::Staged,
            WFocus::Unstaged => FileList::Unstaged,
            _ => self.wt.diff_side().unwrap_or(self.wt.last_list),
        }
    }

    pub fn stage_changed(&mut self, with_untracked: Option<bool>) {
        let tracked: Vec<Vec<u8>> = self
            .wt
            .unstaged
            .iter()
            .filter(|f| !f.untracked && !f.unmerged)
            .map(|f| f.path.clone())
            .collect();
        let untracked: Vec<Vec<u8>> = self
            .wt
            .unstaged
            .iter()
            .filter(|f| f.untracked)
            .map(|f| f.path.clone())
            .collect();
        let with = match with_untracked {
            Some(b) => b,
            None => match self.cfg.stage_untracked {
                StageUntracked::Yes => true,
                StageUntracked::No => false,
                StageUntracked::Ask if untracked.is_empty() => false,
                StageUntracked::Ask => {
                    let mut d = Dialog::new(
                        "Stage changed files",
                        Kind::Form(FormKind::UntrackedStage(untracked.clone())),
                    )
                    .with_body(&[(
                        &format!("Stage {} untracked file(s) as well?", untracked.len()),
                        Tone::Normal,
                    )])
                    .with_buttons(&["Yes", "No", "Cancel"]);
                    d.cancel = Some(2);
                    self.dialogs.push(d);
                    return;
                }
            },
        };
        let mut all = tracked;
        if with {
            all.extend(untracked);
        }
        if all.is_empty() {
            self.set_status("No changes to stage.", StatusTone::Info);
            return;
        }
        self.stage_paths(all);
    }

    fn revert_selected(&mut self) {
        if !self.guard_write() {
            return;
        }
        let l = self.focused_list();
        if l == FileList::Staged {
            self.set_status(
                "Revert only applies to unstaged changes.",
                StatusTone::Error,
            );
            return;
        }
        let paths = self.wt.targets(FileList::Unstaged);
        if paths.is_empty() {
            self.set_status("No file selected.", StatusTone::Info);
            return;
        }
        let (untracked, tracked): (Vec<Vec<u8>>, Vec<Vec<u8>>) = paths
            .into_iter()
            .partition(|p| self.wt.unstaged.iter().any(|f| &f.path == p && f.untracked));
        let names = |v: &[Vec<u8>]| -> String {
            let mut s: Vec<String> = v
                .iter()
                .take(5)
                .map(|p| sanitize::display_bytes(p))
                .collect();
            if v.len() > 5 {
                s.push(format!("... and {} more", v.len() - 5));
            }
            s.join("\n  ")
        };
        if !tracked.is_empty() {
            let body = if tracked.len() == 1 {
                format!("Revert changes in file {}?", names(&tracked))
            } else {
                format!(
                    "Revert changes in these {} files?\n  {}",
                    tracked.len(),
                    names(&tracked)
                )
            };
            let body2 = "Any unstaged changes will be permanently lost by the revert.";
            self.confirm(
                "Revert Changes",
                &[(&body, Tone::Normal), (body2, Tone::Warn)],
                "Revert Changes",
                Pending::Op {
                    label: "Revert changes".into(),
                    f: Box::new(move |r| ops::revert_files(r, &tracked)),
                    after: After::Rescan,
                    expect_head: None,
                },
            );
        }
        if !untracked.is_empty() {
            let body = if untracked.len() == 1 {
                format!("Delete untracked file {}?", names(&untracked))
            } else {
                format!(
                    "Delete these {} untracked files?\n  {}",
                    untracked.len(),
                    names(&untracked)
                )
            };
            let workdir = self.repo.cwd().to_path_buf();
            self.confirm(
                "Delete Files",
                &[
                    (&body, Tone::Normal),
                    ("Files will be permanently deleted.", Tone::Warn),
                ],
                "Delete",
                Pending::Op {
                    label: "Delete untracked".into(),
                    f: Box::new(move |r| ops::delete_untracked(r, &workdir, &untracked)),
                    after: After::Rescan,
                    expect_head: None,
                },
            );
        }
    }

    // ------------------------------------------------------------------
    // Partial staging

    fn partial(&mut self, hunk_line: Option<usize>, revert: bool) {
        if self.wt.diff.text_range().is_some() {
            self.set_status("Text selection is for copying only. Use Shift+Up/Down to select whole patch lines.", StatusTone::Error);
            return;
        }
        if !self.guard_write() {
            return;
        }
        let Some(side) = self.wt.diff_side() else {
            return;
        };
        let Some(f) = self.wt.cursor_file().cloned() else {
            return;
        };
        if f.untracked {
            self.set_status(
                "Untracked file: stage the whole file instead.",
                StatusTone::Error,
            );
            return;
        }
        if f.typechange {
            self.set_status(
                "File type changed: only whole-file operations are supported.",
                StatusTone::Error,
            );
            return;
        }
        if revert && side == FileList::Staged {
            self.set_status(
                "Revert only applies to unstaged changes.",
                StatusTone::Error,
            );
            return;
        }
        let Some(diff) = self.wt.diff.diff.clone() else {
            return;
        };
        let sel = match hunk_line {
            Some(idx) => self.wt.diff.hunk_at(idx).and_then(|(fi, hi)| {
                diff.files
                    .get(fi)
                    .map(|fd| (fi, patch::hunk_selection(fd, hi)))
            }),
            None => self.wt.diff.selected_lines(),
        };
        let Some((fi, set)) = sel else {
            self.set_status(
                "Select a hunk or changed lines first (cursor on a hunk, Shift+Up/Down for lines).",
                StatusTone::Info,
            );
            return;
        };
        let Some(file) = diff.files.get(fi) else {
            return;
        };
        let dir = if side == FileList::Unstaged && !revert {
            Direction::Forward
        } else {
            Direction::Reverse
        };
        let p = match patch::build_patch(file, &set, dir) {
            Ok(p) => p,
            Err(e) => {
                self.set_status(e.to_string(), StatusTone::Error);
                return;
            }
        };
        let what = if hunk_line.is_some() {
            "hunk"
        } else {
            "line(s)"
        };
        if revert {
            let shown = f.display.clone();
            let p2 = p.clone();
            self.confirm(
                "Revert Changes",
                &[
                    (
                        &format!("Revert the selected {what} in {shown}?"),
                        Tone::Normal,
                    ),
                    (
                        "Use Commit > Undo Last Revert to restore them.",
                        Tone::Normal,
                    ),
                ],
                "Revert",
                Pending::Op {
                    label: format!("Revert {what}"),
                    f: Box::new(move |r| ops::apply_patch(r, &p2, false, true, false)),
                    after: After::Reverted(p),
                    expect_head: None,
                },
            );
            return;
        }
        let (label, cached, reverse) = match side {
            FileList::Unstaged => (format!("Stage {what}"), true, false),
            FileList::Staged => (format!("Unstage {what}"), true, true),
        };
        self.run_op(
            &label,
            Box::new(move |r| ops::apply_patch(r, &p, cached, reverse, false)),
            After::Rescan,
            None,
        );
    }

    // ------------------------------------------------------------------
    // Commit

    fn do_commit(&mut self) {
        if !self.guard_write() {
            return;
        }
        let merging = self
            .repo_state
            .operations
            .contains(&crate::git::repo::Operation::Merge);
        if self.wt.staged.is_empty() && !merging && !self.wt.amend {
            self.error(
                "Commit",
                "No changes to commit.\n\nYou must stage at least 1 file before you can commit.",
            );
            return;
        }
        let msg = clean_message(&self.wt.message.text());
        if msg.is_empty() {
            self.error(
                "Commit",
                "Please supply a commit message.\n\nA good commit message has the following format:\n\n- First line: Describe in one sentence what you did.\n- Second line: Blank\n- Remaining lines: Describe why this change is good.",
            );
            return;
        }
        if self.wt.unstaged.iter().any(|f| f.unmerged) {
            self.error(
                "Commit",
                "Unmerged files cannot be committed.\n\nResolve the conflicts and stage the files first.",
            );
            return;
        }
        let amend = self.wt.amend;
        let handoff = match self.cfg.commit_handoff {
            HandoffCfg::Always => true,
            HandoffCfg::Never => false,
            HandoffCfg::Auto => read::config_bool(&self.runner, "commit.gpgsign") == Some(true),
        };
        self.save_draft();
        if handoff {
            self.request_handoff(Handoff::Commit {
                message: msg,
                amend,
            });
        } else {
            self.run_op(
                if amend { "Amend commit" } else { "Commit" },
                Box::new(move |r| {
                    ops::commit(
                        r,
                        &ops::CommitRequest {
                            message: &msg,
                            amend,
                            handoff: false,
                        },
                    )
                }),
                After::Committed,
                None,
            );
        }
    }

    fn set_amend(&mut self, on: bool) {
        if on == self.wt.amend {
            return;
        }
        if on {
            if self.head().is_none() {
                self.error(
                    "Amend Last Commit",
                    "No commit to amend.\n\nYou are about to create the initial commit. There is no commit before this to amend.",
                );
                return;
            }
            if self
                .repo_state
                .operations
                .contains(&crate::git::repo::Operation::Merge)
            {
                self.error(
                    "Amend Last Commit",
                    "Cannot amend while merging.\n\nYou are currently in the middle of a merge that has not been fully completed. You cannot amend the prior commit unless you first abort the current merge activity.",
                );
                return;
            }
            self.wt.amend = true;
            let r = self.runner.clone();
            self.req.amend = self.workers.read(self.generation, move || {
                Payload::Amend(read::message_of(&r, "HEAD"))
            });
        } else {
            self.wt.amend = false;
            if let Some(m) = self.wt.saved_message.take() {
                self.wt.message.set_text(&m);
            }
        }
        self.generation += 1;
        self.rescan();
    }

    fn sign_off(&mut self) {
        match read::committer_ident(&self.runner) {
            Ok(id) => self.wt.message.sign_off(&id),
            Err(e) => self.error("Sign Off", e),
        }
    }

    fn remote_handoff(&mut self, args: Vec<&str>, remote: &str, label: String) {
        let mut a: Vec<OsString> = args.into_iter().map(os).collect();
        a.push(os(remote));
        self.request_handoff(Handoff::Git { args: a, label });
    }

    fn current_branch(&self) -> Option<String> {
        match &self.refs.head {
            crate::domain::refs::Head::Branch { short, .. } => Some(short.clone()),
            _ => None,
        }
    }

    // ------------------------------------------------------------------
    // Dispatch

    pub fn dispatch_worktree(&mut self, a: &Action) -> bool {
        use Action as A;
        match a {
            A::Rescan => {
                self.generation += 1;
                self.load_refs();
                self.rescan();
                self.mark_working();
            }
            A::StageSelected | A::UnstageSelected => {
                if !self.guard_write() {
                    return true;
                }
                let l = self.focused_list();
                let t = self.wt.targets(l);
                self.toggle_stage(l, t);
            }
            A::StageChanged => {
                if self.guard_write() {
                    self.stage_changed(None);
                }
            }
            A::RevertSelected => {
                if self.wt.focus == WFocus::Diff {
                    self.partial(None, true);
                } else {
                    self.revert_selected();
                }
            }
            A::StageToggle(l, i) => {
                if !self.guard_write() {
                    return true;
                }
                if let Some(f) = self.wt.list(*l).get(*i) {
                    let p = vec![f.path.clone()];
                    self.toggle_stage(*l, p);
                }
            }
            A::SelectFile(l, i) => self.select_file(*l, *i),
            A::ExtendFile(l, i) => {
                let n = self.wt.list(*l).len();
                if *i >= n {
                    return true;
                }
                let anchor = if self.wt.last_list == *l {
                    self.wt.anchor.unwrap_or(*i)
                } else {
                    *i
                };
                let (a0, b0) = (anchor.min(*i), anchor.max(*i));
                let paths: Vec<Vec<u8>> = self.wt.list(*l)[a0..=b0]
                    .iter()
                    .map(|f| f.path.clone())
                    .collect();
                let s = self.wt.sel_mut(*l);
                s.clear();
                s.extend(paths);
                self.wt.last_list = *l;
                self.wt.anchor = Some(anchor);
            }
            A::ToggleFileSel(l, i) => {
                if let Some(p) = self.wt.list(*l).get(*i).map(|f| f.path.clone()) {
                    let s = self.wt.sel_mut(*l);
                    if !s.remove(&p) {
                        s.insert(p);
                    }
                    self.wt.last_list = *l;
                    self.wt.anchor = Some(*i);
                }
            }
            A::MoveFile(d) => self.move_file(*d),
            A::WtContextMore | A::WtContextLess => {
                let c = self.wt.context as i64 + if *a == A::WtContextMore { 1 } else { -1 };
                self.wt.context = c.clamp(0, 1000) as u32;
                self.set_status(
                    format!("Diff context: {} lines", self.wt.context),
                    StatusTone::Info,
                );
                self.load_wt_diff(true);
            }
            A::SignOff => self.sign_off(),
            A::Commit => self.do_commit(),
            A::SetAmend(b) => self.set_amend(*b),
            A::ToggleAmend => self.set_amend(!self.wt.amend),
            A::StageHunk(i) => self.partial(Some(*i), false),
            A::StageLines => self.partial(None, false),
            A::RevertHunk(i) => self.partial(Some(*i), true),
            A::RevertLines => self.partial(None, true),
            A::UndoLastRevert => {
                if let Some(p) = self.wt.last_revert.clone() {
                    let undone = p.clone();
                    self.run_op(
                        "Undo last revert",
                        Box::new(move |r| ops::apply_patch(r, &p, false, false, false)),
                        After::RevertUndone(undone),
                        None,
                    );
                } else {
                    self.set_status("Nothing to undo.", StatusTone::Info);
                }
            }
            A::Undo => self.wt.message.undo(),
            A::Redo => self.wt.message.redo(),
            A::Cut => {
                if let Some(t) = self.wt.message.selected_text() {
                    self.wt.message.delete_selection();
                    self.copy_text(t);
                }
            }
            A::Paste => {
                let c = self.clipboard.clone();
                self.wt.focus = WFocus::Message;
                self.wt.message.insert_str(&c);
            }
            A::DeleteSel => {
                self.wt.message.delete_selection();
            }
            A::SelectAll => match self.wt.focus {
                WFocus::Diff => {
                    let n = self.wt.diff.lines.len();
                    if n > 0 {
                        self.wt.diff.anchor = Some(0);
                        self.wt.diff.text_selection = None;
                        self.wt.diff.cursor = n - 1;
                    }
                }
                _ => {
                    self.wt.focus = WFocus::Message;
                    self.wt.message.select_all();
                }
            },
            A::EditMessageExternally => self.request_handoff(Handoff::Editor),
            A::CreateBranch => self.open_gui_create_branch(),
            A::CheckoutBranch => self.open_gui_checkout(),
            A::RenameBranchDialog => self.open_gui_rename(),
            A::DeleteBranchDialog => self.open_gui_delete(),
            A::ResetHard => {
                let Some(h) = self.head() else { return true };
                if !self.guard_write() {
                    return true;
                }
                let branch = self.current_branch().unwrap_or_else(|| "HEAD".into());
                self.confirm(
                    "Reset",
                    &[
                        (&format!("Reset changes?\n\nResetting the changes will cause all uncommitted changes of {branch} to be lost."), Tone::Warn),
                        ("Continue with resetting the current changes?", Tone::Normal),
                    ],
                    "Reset",
                    Pending::Op {
                        label: "Reset --hard".into(),
                        f: Box::new(move |r| ops::reset(r, ops::ResetMode::Hard, &h)),
                        after: After::Refresh,
                        expect_head: Some(Some(h)),
                    },
                );
            }
            A::LocalMerge => self.open_gui_merge(),
            A::AbortMerge | A::AbortOp => {
                let Some(op) = self.repo_state.operations.first().copied() else {
                    self.set_status("No operation in progress.", StatusTone::Info);
                    return true;
                };
                if !self.guard_write() {
                    return true;
                }
                self.confirm(
                    &format!("Abort {}", op.label()),
                    &[
                        (&format!("Abort the {} in progress?", op.label()), Tone::Normal),
                        ("Aborting will cause all uncommitted changes of the operation to be lost.", Tone::Warn),
                    ],
                    "Abort",
                    Pending::Op {
                        label: format!("Abort {}", op.label()),
                        f: Box::new(move |r| ops::abort_operation(r, op)),
                        after: After::Refresh,
                        expect_head: None,
                    },
                );
            }
            A::ContinueOp => {
                let Some(op) = self.repo_state.operations.first().copied() else {
                    self.set_status("No operation in progress.", StatusTone::Info);
                    return true;
                };
                self.request_handoff(Handoff::Git {
                    args: vec![os(op.command()), os("--continue")],
                    label: format!("Continue {}", op.label()),
                });
            }
            A::Push => self.open_push(),
            A::Fetch(remote) => {
                self.remote_handoff(vec!["fetch"], remote, format!("Fetch from {remote}"))
            }
            A::Prune(remote) => self.remote_handoff(
                vec!["remote", "prune"],
                remote,
                format!("Prune from {remote}"),
            ),
            A::RemoveRemote(remote) => {
                if !self.guard_write() {
                    return true;
                }
                let name = remote.clone();
                self.confirm(
                    "Remove Remote",
                    &[(&format!("Remove remote {remote}?"), Tone::Warn)],
                    "Remove",
                    Pending::Op {
                        label: format!("Remove remote {remote}"),
                        f: Box::new(move |r| ops::remote_remove(r, &name)),
                        after: After::Refresh,
                        expect_head: None,
                    },
                );
            }
            A::AddRemote => self.open_add_remote(),
            A::DeleteRemoteBranch => self.open_delete_remote_branch(),
            A::RunTool(i) => self.run_tool(*i),
            A::BrowseFiles(rev) => self.open_browser(rev.clone()),
            A::BrowseBranchFiles => self.open_browse_branch(),
            A::VisualizeCurrent => {
                self.switch_view(crate::config::ViewKind::History);
            }
            A::VisualizeAll => {
                self.switch_view(crate::config::ViewKind::History);
                self.set_status("Showing the current view; use View > New view with \"All refs\" for every branch.", StatusTone::Info);
            }
            A::DbStats => {
                let r = self.runner.clone();
                self.workers.read(self.generation, move || Payload::Text {
                    title: "Database Statistics".into(),
                    result: read::count_objects(&r),
                });
            }
            A::DbVerify => {
                let r = self.runner.clone();
                self.mark_working();
                self.workers.read(self.generation, move || Payload::Text {
                    title: "Verify Database".into(),
                    result: r
                        .read_opts(
                            &[os("fsck"), os("--full"), os("--no-progress")],
                            Default::default(),
                            &[0, 1, 2],
                        )
                        .map(|o| {
                            let mut s = o.stdout_text();
                            s.push_str(&o.stderr_text());
                            if s.trim().is_empty() {
                                "No problems found.".to_string()
                            } else {
                                s
                            }
                        }),
                });
            }
            A::DbCompress => {
                if self.guard_write() {
                    self.confirm(
                        "Compress Database",
                        &[("Run git gc to compress the object database?", Tone::Normal)],
                        "Compress",
                        Pending::Handoff(Handoff::Git {
                            args: vec![os("gc")],
                            label: "Compress Database".into(),
                        }),
                    );
                }
            }
            A::ExploreWorkingCopy => {
                let dir = self.repo.cwd().to_path_buf();
                match std::process::Command::new("xdg-open")
                    .arg(&dir)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    Ok(_) => {
                        self.set_status("Opened the working copy with xdg-open.", StatusTone::Info)
                    }
                    Err(e) => self.error("Explore Working Copy", e),
                }
            }
            A::RunMergeTool(p) => {
                let mut args = vec![os("mergetool"), os("--")];
                args.push(OsString::from_vec(p.clone()));
                self.request_handoff(Handoff::Git {
                    args,
                    label: "Run merge tool".into(),
                });
            }
            A::UseVersion(p, ours) => {
                let p = p.clone();
                let ours = *ours;
                let which = if ours { "--ours" } else { "--theirs" };
                self.run_op(
                    &format!("Use {} version", if ours { "local" } else { "remote" }),
                    Box::new(move |r| {
                        let mut a = vec![os("checkout"), os(which), os("--")];
                        a.push(OsString::from_vec(p.clone()));
                        r.write(
                            &a,
                            None,
                            crate::git::Opts {
                                literal_paths: true,
                                ..Default::default()
                            },
                        )?;
                        ops::stage(r, &[p])
                    }),
                    After::Rescan,
                    None,
                );
            }
            A::ShowSshKey => {
                let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
                let mut text = String::new();
                if let Some(h) = home {
                    for n in [
                        "id_ed25519.pub",
                        "id_ecdsa.pub",
                        "id_rsa.pub",
                        "id_dsa.pub",
                        "identity.pub",
                    ] {
                        if let Ok(s) = std::fs::read_to_string(h.join(".ssh").join(n)) {
                            text = format!("~/.ssh/{n}:\n\n{}", sanitize::sanitize(s.trim()));
                            break;
                        }
                    }
                }
                if text.is_empty() {
                    text = "You don't have a public key yet (no ~/.ssh/*.pub found).\n\nGenerate one with ssh-keygen in a terminal.".into();
                }
                self.info("Your OpenSSH Public Key", &text);
            }
            A::AboutGitGui => self.info(
                "About Git TUI (git tui)",
                &super::history::about_text("git gui"),
            ),
            _ => return false,
        }
        true
    }

    fn run_tool(&mut self, i: usize) {
        let Some(t) = self.guitools.get(i).cloned() else {
            return;
        };
        let mut envs: Vec<(String, OsString)> =
            vec![(String::from("GIT_GUITOOL"), OsString::from(&t.name))];
        if let Some(f) = self.wt.cursor_file() {
            envs.push(("FILENAME".into(), OsString::from_vec(f.path.clone())));
        } else if t.needs_file {
            self.set_status("Tool requires a selected file.", StatusTone::Error);
            return;
        }
        if let Some(b) = self.current_branch() {
            envs.push(("CUR_BRANCH".into(), OsString::from(b)));
        }
        let h = Handoff::Shell {
            cmd: t.cmd.clone(),
            label: t.name.clone(),
            envs,
        };
        if t.confirm {
            self.confirm(
                "Run tool",
                &[
                    (
                        &format!("Are you sure you want to run {}?", t.name),
                        Tone::Normal,
                    ),
                    (&t.cmd, Tone::Code),
                ],
                "Run",
                Pending::Handoff(h),
            );
        } else {
            self.request_handoff(h);
        }
    }
}

/// git gui strips trailing whitespace and leading/trailing blank lines.
pub fn clean_message(s: &str) -> String {
    let lines: Vec<&str> = s.lines().map(str::trim_end).collect();
    let start = lines
        .iter()
        .position(|l| !l.is_empty())
        .unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map(|e| e + 1)
        .unwrap_or(start);
    let mut out = lines[start..end].join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Helper used by forms: a read-only label row.
pub fn info_field(s: &str) -> Field {
    Field::label(s, Tone::Normal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_message_trims() {
        assert_eq!(
            clean_message("\n\n  subject  \n\nbody \n\n"),
            "  subject\n\nbody\n"
        );
        assert_eq!(clean_message("  \n "), "");
    }
}
