//! History view (gitk): commit list with graph, navigation, find, commit
//! details and diff, file list, and the commit / branch context actions.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::action::Action;
use super::dialog::{Dialog, Kind, Tone};
use super::diffview::{BuildOpts, DKind, DLine, DiffView, Show, WordStyle};
use super::input::TextInput;
use super::worker::{After, Cancel, HistoryUpdate, Payload};
use super::{App, Handoff, Pending, StatusTone};
use crate::config::{Config, State, ViewKind};
use crate::domain::commit::{Commit, CommitDetails};
use crate::domain::diff::Diff;
use crate::domain::graph::{Cut, GraphBuilder, GraphRow};
use crate::domain::oid::Oid;
use crate::domain::refs::{Head, RefKind};
use crate::domain::sanitize;
use crate::domain::textmatch::{MatchKind, Matcher};
use crate::git::log::{self, LocalRows, ViewSpec, null_staged, null_unstaged};
use crate::git::ops;
use crate::git::read::{
    self, Blob, DiffOpts, FindField, FindSpec, FindType, MergeDiff, Related, WordMode,
};
use crate::git::runner::os;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HFocus {
    List,
    Sha,
    Find,
    Files,
    Diff,
    Search,
}

const NAV_LIMIT: usize = 200;
const DETAIL_DEBOUNCE: Duration = Duration::from_millis(40);
const FIND_DEBOUNCE: Duration = Duration::from_millis(400);

pub struct HistoryState {
    /// Index 0 is gitk's "All files" view.
    pub views: Vec<ViewSpec>,
    pub cur_view: usize,
    pub commits: Vec<Commit>,
    pub graph: Vec<GraphRow>,
    /// Commit awaited by the line cut at (row, lane).
    pub cut_target: HashMap<(usize, u16), Oid>,
    /// Rows where lines heading for a commit were cut, top to bottom.
    pub cut_rows: HashMap<Oid, Vec<usize>>,
    pub index: HashMap<Oid, usize>,
    pub sel: Option<usize>,
    pub top: usize,
    /// Visible rows of the commit list (updated when drawing).
    pub height: usize,
    pub back: Vec<Oid>,
    pub forward: Vec<Oid>,
    pub mark: Option<Oid>,
    pub cancel: Cancel,
    pub find_cancel: Cancel,
    /// Commits matching the current find (bold in the list).
    pub highlight: Option<HashSet<Oid>>,
    pub local_rows: usize,
    /// (unstaged, staged) pseudo rows present in the loaded list.
    pub local_flags: (bool, bool),
    pub loading: bool,
    pub load_error: Option<String>,
    pub log_head: Option<Oid>,
    loaded_view: Option<ViewSpec>,
    loaded_base: Vec<Oid>,
    replace_pending: bool,
    update_pending: bool,
    pub pending_select: Option<Oid>,
    pub focus: HFocus,
    pub diff: DiffView,
    pub sha: TextInput,
    pub find: TextInput,
    pub search: TextInput,
    pub find_type: FindType,
    pub find_match: MatchKind,
    pub find_field: FindField,
    pub find_running: bool,
    pub find_dirty: bool,
    pub find_changed_at: Option<Instant>,
    pub find_pending_step: Option<bool>,
    /// Patch mode file list (index 0 is "Comments").
    pub files: Vec<Vec<u8>>,
    pub tree: Vec<Vec<u8>>,
    pub tree_mode: bool,
    /// Keep HEAD named and collapse other refs into counted markers by kind.
    pub compact_refs: bool,
    /// Commits whose refs were opened by a click while compact.
    pub expanded_refs: HashSet<Oid>,
    pub hidden_ref_kinds: HashSet<crate::domain::refs::RefKind>,
    /// Commits HEAD pointed to when the session started or was last
    /// reloaded; the history keeps showing them after checkouts, like gitk.
    pub base_tips: Vec<Oid>,
    pub file_sel: usize,
    pub file_top: usize,
    pub file_height: usize,
    pub show: Show,
    pub context: u32,
    pub ignore_space: bool,
    pub word_mode: WordMode,
    pub merge_diff: MergeDiff,
    /// Explicit comparison (from, to) shown instead of the commit diff.
    pub compare: Option<(Oid, Oid)>,
    pub details: Option<CommitDetails>,
    pub details_error: Option<String>,
    pub related: Option<Related>,
    pub cur_diff: Option<Arc<Diff>>,
    pub detail_oid: Option<Oid>,
    pub sel_changed_at: Option<Instant>,
    pub tab: usize,
    pub total: usize,
    /// Annotated tag details shown in the details pane (set by clicking a tag
    /// label, gitk-style), bound to the commit it was requested for.
    pub tag_info: Option<TagInfoPanel>,
}

/// State of a pending/loaded annotated-tag lookup, shown in the commit
/// details header.
#[derive(Clone, Debug)]
pub struct TagInfoPanel {
    pub oid: Oid,
    pub full: Vec<u8>,
    pub short: String,
    pub loading: bool,
    pub details: Option<read::TagDetails>,
}

impl HistoryState {
    pub fn new(cfg: &Config, state: &State) -> HistoryState {
        HistoryState {
            views: vec![ViewSpec::default_view()],
            cur_view: 0,
            commits: Vec::new(),
            graph: Vec::new(),
            cut_target: HashMap::new(),
            cut_rows: HashMap::new(),
            index: HashMap::new(),
            sel: None,
            top: 0,
            height: 10,
            back: Vec::new(),
            forward: Vec::new(),
            mark: None,
            cancel: Cancel::default(),
            find_cancel: Cancel::default(),
            highlight: None,
            local_rows: 0,
            local_flags: (false, false),
            loading: false,
            load_error: None,
            log_head: None,
            loaded_view: None,
            loaded_base: Vec::new(),
            replace_pending: false,
            update_pending: false,
            pending_select: None,
            focus: HFocus::List,
            diff: DiffView::default(),
            sha: TextInput::default(),
            find: TextInput::default(),
            search: TextInput::default(),
            find_type: FindType::Containing,
            find_match: MatchKind::Exact,
            find_field: FindField::All,
            find_running: false,
            find_dirty: false,
            find_changed_at: None,
            find_pending_step: None,
            files: Vec::new(),
            tree: Vec::new(),
            tree_mode: false,
            compact_refs: state.compact_refs,
            expanded_refs: HashSet::new(),
            hidden_ref_kinds: HashSet::new(),
            base_tips: Vec::new(),
            file_sel: 0,
            file_top: 0,
            file_height: 5,
            show: Show::Diff,
            context: cfg.history_context,
            ignore_space: false,
            word_mode: WordMode::Line,
            merge_diff: MergeDiff::Combined,
            compare: None,
            details: None,
            details_error: None,
            related: None,
            cur_diff: None,
            detail_oid: None,
            sel_changed_at: None,
            tab: cfg.tab_width as usize,
            total: 0,
            tag_info: None,
        }
    }

    pub fn view(&self) -> &ViewSpec {
        &self.views[self.cur_view.min(self.views.len() - 1)]
    }

    pub fn selected_oid(&self) -> Option<Oid> {
        self.sel.and_then(|i| self.commits.get(i)).map(|c| c.oid)
    }

    /// The replacement is visible; older history may still be streaming.
    pub fn view_ready(&self) -> bool {
        !self.replace_pending && !self.update_pending && self.pending_select.is_none()
    }

    pub fn selected(&self) -> Option<&Commit> {
        self.sel.and_then(|i| self.commits.get(i))
    }

    pub fn is_pseudo(oid: &Oid) -> bool {
        *oid == null_unstaged() || *oid == null_staged()
    }

    pub fn cycle_focus(&mut self, dir: isize) {
        const ORDER: [HFocus; 6] = [
            HFocus::List,
            HFocus::Sha,
            HFocus::Find,
            HFocus::Search,
            HFocus::Diff,
            HFocus::Files,
        ];
        let i = ORDER.iter().position(|f| *f == self.focus).unwrap_or(0) as isize;
        self.focus = ORDER[(i + dir).rem_euclid(ORDER.len() as isize) as usize];
    }

    /// Rows of the file list pane.
    pub fn file_rows(&self) -> usize {
        if self.tree_mode {
            self.tree.len()
        } else {
            self.files.len() + 1
        }
    }

    /// Path behind a file list row (None for "Comments").
    pub fn file_path(&self, i: usize) -> Option<Vec<u8>> {
        if self.tree_mode {
            self.tree.get(i).cloned()
        } else if i == 0 {
            None
        } else {
            self.files.get(i - 1).cloned()
        }
    }

    pub fn ensure_visible(&mut self, i: usize) {
        let h = self.height.max(1);
        if i < self.top {
            self.top = i;
        } else if i >= self.top + h {
            self.top = i + 1 - h;
        }
    }

    pub fn scroll(&mut self, delta: isize) {
        let max = self.commits.len().saturating_sub(self.height.max(1));
        self.top = (self.top as isize + delta).clamp(0, max as isize) as usize;
    }

    pub fn scroll_files(&mut self, delta: isize) {
        let max = self.file_rows().saturating_sub(self.file_height.max(1));
        self.file_top = (self.file_top as isize + delta).clamp(0, max as isize) as usize;
    }

    fn reveal_file_selection(&mut self) {
        let h = self.file_height.max(1);
        if self.file_sel < self.file_top {
            self.file_top = self.file_sel;
        } else if self.file_sel >= self.file_top + h {
            self.file_top = self.file_sel + 1 - h;
        }
    }

    pub fn sync_file_selection(&mut self, line: usize) {
        if !self.tree_mode {
            self.file_sel = self.diff.file_at(line).map(|f| f + 1).unwrap_or(0);
            self.reveal_file_selection();
        }
    }

    pub fn children_of(&self, oid: &Oid) -> Vec<Oid> {
        self.commits
            .iter()
            .filter(|c| c.parents.contains(oid))
            .map(|c| c.oid)
            .collect()
    }

    fn subject_of(&self, oid: &Oid) -> Option<&str> {
        self.index.get(oid).map(|i| &*self.commits[*i].subject)
    }

    fn descendants(&self, from: &Oid) -> HashSet<Oid> {
        let mut children: HashMap<Oid, Vec<Oid>> = HashMap::new();
        for c in &self.commits {
            for p in c.parents.iter() {
                children.entry(*p).or_default().push(c.oid);
            }
        }
        let mut seen = HashSet::new();
        let mut q = VecDeque::from([*from]);
        while let Some(o) = q.pop_front() {
            if !seen.insert(o) {
                continue;
            }
            if let Some(cs) = children.get(&o) {
                q.extend(cs.iter().copied());
            }
        }
        seen
    }

    fn diff_opts(&self) -> DiffOpts {
        DiffOpts {
            context: self.context,
            ignore_space: self.ignore_space,
            words: self.word_mode,
        }
    }

    fn header_lines(&self) -> Vec<DLine> {
        let mut v = Vec::new();
        let short = |o: &Oid| o.short(10);
        if let Some((a, b)) = self.compare {
            v.push(DLine::plain(DKind::Header, "Comparison:".to_string()));
            for (lbl, o) in [("From", a), ("To  ", b)] {
                v.push(DLine::plain(
                    DKind::Header,
                    format!(
                        "  {lbl}: {} ({})",
                        o.to_hex(),
                        self.subject_of(&o).unwrap_or("")
                    ),
                ));
            }
            v.push(DLine::plain(DKind::Header, String::new()));
            return v;
        }
        let Some(c) = self.selected() else {
            return v;
        };
        if let Some(t) = &self.tag_info
            && t.oid == c.oid
        {
            v.push(DLine::plain(DKind::Header, format!("Tag: {}", t.short)));
            if t.loading {
                v.push(DLine::plain(DKind::Info, "Loading tag details..."));
            } else if let Some(d) = &t.details {
                v.push(DLine::plain(
                    DKind::Header,
                    format!("Tagger: {}  {}", d.tagger, d.tagger_date),
                ));
                for l in d.message.lines() {
                    v.push(DLine::plain(DKind::Message, l.to_string()));
                }
            } else {
                v.push(DLine::plain(
                    DKind::Info,
                    "Lightweight tag (no annotation).",
                ));
            }
            v.push(DLine::plain(DKind::Header, String::new()));
        }
        if Self::is_pseudo(&c.oid) {
            v.push(DLine::plain(DKind::Header, c.subject.to_string()));
            v.push(DLine::plain(DKind::Header, String::new()));
            return v;
        }
        let Some(d) = &self.details else {
            v.push(match &self.details_error {
                Some(error) => DLine::plain(DKind::Error, error),
                None => DLine::plain(DKind::Info, "Loading commit details..."),
            });
            return v;
        };
        v.push(DLine::plain(
            DKind::Header,
            format!("Author: {}  {}", d.author, d.author_date),
        ));
        v.push(DLine::plain(
            DKind::Header,
            format!("Committer: {}  {}", d.committer, d.committer_date),
        ));
        for p in &d.parents {
            let subj = self.subject_of(p).unwrap_or("");
            v.push(DLine::plain(
                DKind::Header,
                format!("Parent: {} ({subj})", p.to_hex()),
            ));
        }
        for ch in self.children_of(&c.oid) {
            if Self::is_pseudo(&ch) {
                continue;
            }
            let subj = self.subject_of(&ch).unwrap_or("");
            v.push(DLine::plain(
                DKind::Header,
                format!("Child:  {} ({subj})", ch.to_hex()),
            ));
        }
        if let Some(r) = &self.related {
            if !r.branches.is_empty() {
                v.push(DLine::plain(
                    DKind::Header,
                    format!("Branches: {}", r.branches.join(", ")),
                ));
            }
            if !r.tags.is_empty() {
                v.push(DLine::plain(
                    DKind::Header,
                    format!("Tags: {}", r.tags.join(", ")),
                ));
            }
            if let Some(t) = &r.follows {
                v.push(DLine::plain(DKind::Header, format!("Follows: {t}")));
            }
            if let Some(t) = &r.precedes {
                v.push(DLine::plain(DKind::Header, format!("Precedes: {t}")));
            }
        }
        if d.parents.len() > 1 {
            let mode = match self.merge_diff {
                MergeDiff::Combined => "combined diff against all parents (--cc)".to_string(),
                MergeDiff::Parent(n) => {
                    let p = d
                        .parents
                        .get(n.saturating_sub(1))
                        .map(short)
                        .unwrap_or_default();
                    format!("diff against parent {n} ({p})")
                }
            };
            v.push(DLine::plain(DKind::Info, format!("Merge: showing {mode}")));
        }
        v.push(DLine::plain(DKind::Header, String::new()));
        for l in d.message.lines() {
            v.push(DLine::plain(
                DKind::Message,
                format!(
                    "    {}",
                    sanitize::expand_tabs(&sanitize::sanitize(l), self.tab)
                ),
            ));
        }
        v.push(DLine::plain(DKind::Message, String::new()));
        let _ = short;
        v
    }

    /// Rebuilds the diff pane from the cached details and diff.
    pub fn rebuild_diff(&mut self) {
        let prefix = self.header_lines();
        let o = BuildOpts {
            tab: self.tab,
            show: self.show,
            words: if self.word_mode == WordMode::Color {
                WordStyle::Color
            } else {
                WordStyle::Markup
            },
            compact_headers: false,
        };
        // The diff may arrive before the header; do not display it only to
        // push it down when the commit metadata and message arrive.
        let waiting_for_details = self.compare.is_none()
            && self
                .selected_oid()
                .is_some_and(|oid| !Self::is_pseudo(&oid))
            && self.details.is_none()
            && self.details_error.is_none();
        let d = if waiting_for_details {
            None
        } else {
            self.cur_diff.clone()
        };
        self.diff.set(prefix, d, &o);
        self.sync_file_selection(self.diff.top);
    }

    fn push_nav(&mut self, oid: Oid) {
        if self.back.last() != Some(&oid) {
            self.back.push(oid);
            if self.back.len() > NAV_LIMIT {
                self.back.remove(0);
            }
        }
        self.forward.clear();
    }

    /// Applies the result of an annotated-tag lookup requested by
    /// `App::show_tag_info`; stale answers (a different tag meanwhile
    /// selected) are dropped.
    pub fn on_tag_info(
        &mut self,
        full: Vec<u8>,
        result: Result<Option<read::TagDetails>, crate::git::GitError>,
    ) {
        let Some(info) = &mut self.tag_info else {
            return;
        };
        if info.full != full {
            return;
        }
        match result {
            Ok(details) => {
                info.loading = false;
                info.details = details;
            }
            Err(_) => self.tag_info = None,
        }
        self.rebuild_diff();
    }
}

fn bytes_list(paths: &[String]) -> Vec<Vec<u8>> {
    paths.iter().map(|p| p.as_bytes().to_vec()).collect()
}

fn local_rows(status: &crate::domain::status::RepoStatus) -> LocalRows {
    let tracked = |entry: &&crate::domain::status::StatusEntry| {
        !matches!(
            entry.kind,
            crate::domain::status::EntryKind::Untracked | crate::domain::status::EntryKind::Ignored
        )
    };
    LocalRows {
        unstaged: status
            .entries
            .iter()
            .filter(tracked)
            .any(|e| e.has_unstaged()),
        staged: status
            .entries
            .iter()
            .filter(tracked)
            .any(|e| e.has_staged()),
        head: status.branch.oid,
    }
}

impl App {
    // ------------------------------------------------------------------
    // Loading

    pub fn update_history(&mut self, keep: Option<Oid>) {
        let h = &mut self.hist;
        if h.loading
            || h.load_error.is_some()
            || !h.view().incremental_head_view()
            || h.loaded_view.as_ref() != Some(h.view())
            || h.loaded_base != h.base_tips
            || h.log_head.is_none()
            || h.commits.len() == h.local_rows
        {
            self.load_history(keep);
            return;
        }
        let old = h.log_head.expect("checked above");
        h.cancel.cancel();
        h.cancel = Cancel::default();
        h.loading = true;
        h.update_pending = true;
        h.pending_select = keep.or_else(|| h.selected_oid());
        let cancel = h.cancel.clone();
        let r = self.runner.clone();
        let bare = self.repo.is_bare();
        let common_dir = self.repo.common_dir.clone();
        self.req.log = self.workers.stream(self.generation, move |sink| {
            let result = (|| {
                if !log::history_cache_safe(&r, &common_dir)? {
                    return Ok(None);
                }
                let (head, local) = if bare {
                    (read::load_refs(&r)?.head.oid(), None)
                } else {
                    let status = read::status(&r)?;
                    let local = local_rows(&status);
                    (local.head, Some(local))
                };
                let extension = match head {
                    Some(head) => log::linear_extension(&r, old, head, cancel.flag())?,
                    None => None,
                };
                Ok(Some(HistoryUpdate {
                    head,
                    local,
                    extension,
                }))
            })();
            sink.send(Payload::HistoryUpdate(result));
        });
    }

    pub fn on_history_update(
        &mut self,
        result: Result<Option<HistoryUpdate>, crate::git::GitError>,
    ) {
        self.hist.loading = false;
        self.hist.update_pending = false;
        let update = match result {
            Ok(Some(update)) => update,
            Ok(None) => {
                self.load_history(self.hist.pending_select);
                return;
            }
            Err(error) => {
                self.on_log_done(Err(error));
                return;
            }
        };
        let h = &self.hist;
        let keep_existing = update.head == h.log_head
            || (h.log_head.is_some_and(|head| h.base_tips.contains(&head))
                && update.head.is_some_and(|head| h.index.contains_key(&head)));
        let extension = if keep_existing {
            Some(Vec::new())
        } else {
            update.extension.filter(|commits| {
                h.commits.get(h.local_rows).map(|c| c.oid) == h.log_head
                    && commits.iter().all(|c| !h.index.contains_key(&c.oid))
            })
        };
        if let Some(extension) = extension {
            let mut prefix = log::local_commits(update.local);
            prefix.extend(extension);
            if self.replace_history_prefix(prefix) {
                self.hist.log_head = update.head;
                self.on_log_done(Ok(self.hist.total));
                if let Some(oid) = self.hist.selected_oid()
                    && !HistoryState::is_pseudo(&oid)
                    && self.hist.compare.is_none()
                    && self.hist.detail_oid == Some(oid)
                    && self.hist.sel_changed_at.is_none()
                {
                    self.queue_commit_details(oid);
                }
                return;
            }
        }
        self.load_history(self.hist.pending_select);
    }

    /// Reuse the suffix only if its graph builder state is exactly unchanged.
    /// This also handles local rows without visiting Git history.
    fn replace_history_prefix(&mut self, prefix: Vec<Commit>) -> bool {
        let h = &mut self.hist;
        let previous_selection = h.selected_oid();
        if prefix.len() == h.local_rows
            && prefix
                .iter()
                .zip(&h.commits)
                .all(|(a, b)| a.oid == b.oid && a.parents == b.parents)
            && h.pending_select.or(previous_selection) == previous_selection
        {
            h.pending_select = None;
            if previous_selection.is_some_and(|oid| HistoryState::is_pseudo(&oid)) {
                h.sel_changed_at = Some(Instant::now());
            }
            return true;
        }
        let Some(boundary) = h.commits.get(h.local_rows) else {
            return false;
        };
        let mut old = GraphBuilder::with_cut_after(self.cfg.graph_cut_after as usize);
        for commit in h.commits.iter().take(h.local_rows + 1) {
            old.push(commit.oid, &commit.parents);
        }
        let mut new = GraphBuilder::with_cut_after(self.cfg.graph_cut_after as usize);
        let mut graph = Vec::new();
        for commit in prefix.iter().chain(std::iter::once(boundary)) {
            graph.push(new.push(commit.oid, &commit.parents));
        }
        if !old.same_continuation(&new) {
            return false;
        }
        let selected = h.pending_select.or_else(|| h.selected_oid());
        let top = h.commits.get(h.top).map(|c| c.oid);
        let removed = h.local_rows;
        let inserted = prefix.len();
        h.local_rows = prefix
            .iter()
            .take_while(|c| HistoryState::is_pseudo(&c.oid))
            .count();
        h.local_flags = (
            prefix.iter().any(|c| c.oid == null_unstaged()),
            prefix.iter().any(|c| c.oid == null_staged()),
        );
        h.commits.splice(..removed, prefix);
        h.graph.splice(..removed + 1, graph);
        h.cut_target = std::mem::take(&mut h.cut_target)
            .into_iter()
            .filter(|((row, _), _)| *row > removed)
            .map(|((row, lane), oid)| ((row - removed + inserted, lane), oid))
            .collect();
        for cut in new.take_cuts() {
            h.cut_target.insert((cut.row, cut.lane), cut.target);
        }
        h.cut_rows.clear();
        for (&(row, _), &oid) in &h.cut_target {
            h.cut_rows.entry(oid).or_default().push(row);
        }
        for rows in h.cut_rows.values_mut() {
            rows.sort_unstable();
        }
        h.index.remove(&null_unstaged());
        h.index.remove(&null_staged());
        if removed != inserted {
            for index in h.index.values_mut() {
                *index = *index - removed + inserted;
            }
        }
        h.index.extend(
            h.commits
                .iter()
                .take(inserted)
                .enumerate()
                .map(|(i, c)| (c.oid, i)),
        );
        h.total = h.commits.len();
        h.top = top
            .and_then(|oid| h.index.get(&oid).copied())
            .unwrap_or(h.top);
        let selected = selected
            .and_then(|oid| h.index.get(&oid).copied())
            .unwrap_or(0);
        h.pending_select = h.pending_select.filter(|oid| !h.index.contains_key(oid));
        if previous_selection == Some(h.commits[selected].oid) {
            h.sel = Some(selected);
            if previous_selection.is_some_and(|oid| HistoryState::is_pseudo(&oid)) {
                h.sel_changed_at = Some(Instant::now());
            }
        } else {
            h.sel = None;
            self.select_row_ex(selected, false, false);
        }
        self.hist.scroll(0);
        true
    }

    pub fn update_local_history(&mut self, local: LocalRows) {
        if !self.hist.loading
            && self.hist.load_error.is_none()
            && self.hist.loaded_view.as_ref() == Some(self.hist.view())
            && !self.hist.view().has_paths()
            && self.hist.log_head == local.head
            && self.replace_history_prefix(log::local_commits(Some(local)))
        {
            return;
        }
        self.load_history(None);
    }

    pub fn load_history(&mut self, keep: Option<Oid>) {
        if matches!(self.drag, Some(super::mouse::Drag::HistoryScroll { .. })) {
            self.drag = None;
        }
        let h = &mut self.hist;
        h.cancel.cancel();
        h.cancel = Cancel::default();
        let keep = keep.or_else(|| h.selected_oid());
        h.replace_pending = true;
        h.update_pending = false;
        h.log_head = None;
        h.loading = true;
        h.load_error = None;
        h.pending_select = keep;
        let view = h.view().clone();
        h.loaded_view = Some(view.clone());
        h.loaded_base = h.base_tips.clone();
        let base = h.base_tips.clone();
        let cancel = h.cancel.clone();
        let with_local = !self.repo.is_bare() && view.includes_head();
        let cut_after = self.cfg.graph_cut_after as usize;
        let r = self.runner.clone();
        let common_dir = self.repo.common_dir.clone();
        self.req.log = self.workers.stream(self.generation, move |sink| {
            let res = (|| {
                let local = if with_local {
                    Some(local_rows(&read::status_in(&r, &view.path_list())?))
                } else {
                    None
                };
                let mut view = view;
                if view.incremental_head_view() && log::history_cache_safe(&r, &common_dir)? {
                    let head = match local {
                        Some(local) => local.head,
                        None => read::load_refs(&r)?.head.oid(),
                    };
                    sink.send(Payload::LogHead(head));
                    if let Some(head) = head {
                        // Pin HEAD for the lifetime of this stream, retaining base tips.
                        view.refs = std::iter::once(head)
                            .chain(log::existing_commits(&r, &base))
                            .map(|oid| oid.to_hex())
                            .collect::<Vec<_>>()
                            .join(" ");
                    }
                }
                log::stream_log(&r, &view, &base, local, cut_after, cancel.flag(), |b| {
                    sink.send(Payload::LogBatch {
                        commits: b.commits,
                        graph: b.graph,
                        cuts: b.cuts,
                    })
                })
            })();
            sink.send(Payload::LogDone(res));
        });
    }

    pub fn on_log_batch(&mut self, commits: Vec<Commit>, graph: Vec<GraphRow>, cuts: Vec<Cut>) {
        let h = &mut self.hist;
        if h.replace_pending {
            h.commits.clear();
            h.graph.clear();
            h.cut_target.clear();
            h.cut_rows.clear();
            h.index.clear();
            h.sel = None;
            h.local_rows = 0;
            h.local_flags = (false, false);
            h.total = 0;
            h.replace_pending = false;
        }
        for c in cuts {
            h.cut_target.insert((c.row, c.lane), c.target);
            h.cut_rows.entry(c.target).or_default().push(c.row);
        }
        let first = h.commits.is_empty();
        for c in commits {
            if HistoryState::is_pseudo(&c.oid) {
                h.local_rows += 1;
                if c.oid == null_unstaged() {
                    h.local_flags.0 = true;
                } else {
                    h.local_flags.1 = true;
                }
            }
            h.index.insert(c.oid, h.commits.len());
            h.commits.push(c);
        }
        h.graph.extend(graph);
        h.total = h.commits.len();
        if let Some(p) = h.pending_select
            && let Some(&i) = h.index.get(&p)
        {
            h.pending_select = None;
            let keep_top = h.top;
            self.select_row_ex(i, false, false);
            if !first {
                self.hist.top = keep_top;
                self.hist.ensure_visible(i);
            } else {
                self.center_selection();
            }
            return;
        }
        if first && self.hist.sel.is_none() && !self.hist.commits.is_empty() {
            // Provisional selection: HEAD's row (or the first row).
            let head = self.head();
            let i = head
                .and_then(|o| self.hist.index.get(&o).copied())
                .unwrap_or(0);
            self.hist.top = 0;
            self.select_row_ex(i, false, false);
        }
    }

    pub fn on_log_done(&mut self, r: Result<usize, crate::git::GitError>) {
        self.hist.loading = false;
        match r {
            Ok(_) => {
                if self.hist.replace_pending {
                    self.on_log_batch(Vec::new(), Vec::new(), Vec::new());
                }
                if let Some(p) = self.hist.pending_select.take()
                    && !HistoryState::is_pseudo(&p)
                {
                    self.set_status(
                        format!("Commit {} is not in the current view.", p.short(10)),
                        StatusTone::Info,
                    );
                }
                if self.hist.commits.is_empty() {
                    self.hist
                        .diff
                        .set_message(DKind::Info, "No commits in this view.");
                    self.hist.files.clear();
                }
            }
            Err(e) => {
                let msg = e.to_string();
                self.hist.load_error = Some(msg.clone());
                self.set_status(
                    format!("History: {}", msg.lines().next().unwrap_or("")),
                    StatusTone::Error,
                );
                if self.hist.commits.is_empty() {
                    self.hist.diff.set_message(DKind::Error, &msg);
                }
            }
        }
    }

    pub fn history_tick(&mut self, now: Instant) {
        if let Some(t) = self.hist.sel_changed_at
            && now.duration_since(t) >= DETAIL_DEBOUNCE
        {
            self.hist.sel_changed_at = None;
            self.load_selected();
        }
        if self.hist.find_dirty
            && let Some(t) = self.hist.find_changed_at
            && now.duration_since(t) >= FIND_DEBOUNCE
        {
            self.run_find();
        }
    }

    fn center_selection(&mut self) {
        if let Some(i) = self.hist.sel {
            let h = self.hist.height.max(1);
            self.hist.top = i.saturating_sub(h / 2);
            self.hist.scroll(0);
        }
    }

    /// Down arrow of a cut graph line clicked: go to the commit it leads to,
    /// whose matching up arrow is drawn just above it.
    pub fn follow_cut(&mut self, row: usize, lane: u16) {
        let Some(target) = self.hist.cut_target.get(&(row, lane)).copied() else {
            return;
        };
        match self.hist.index.get(&target).copied() {
            Some(i) => {
                self.hist.focus = HFocus::List;
                self.select_row(i);
                self.center_selection();
            }
            None => self.set_status(
                format!(
                    "The line leads to {}, which is not loaded in this view.",
                    target.short(10)
                ),
                StatusTone::Info,
            ),
        }
    }

    /// Up arrow above commit `node` clicked: go to the row where its line
    /// was cut. Repeated clicks cycle through the lines cut towards it.
    pub fn follow_resume(&mut self, node: usize) {
        let Some(oid) = self.hist.commits.get(node).map(|c| c.oid) else {
            return;
        };
        let Some(rows) = self.hist.cut_rows.get(&oid) else {
            return;
        };
        let target = match self
            .hist
            .sel
            .and_then(|s| rows.iter().position(|r| *r == s))
        {
            Some(p) if p > 0 => rows[p - 1],
            _ => *rows.last().expect("non-empty"),
        };
        self.hist.focus = HFocus::List;
        self.select_row(target);
        self.center_selection();
    }

    /// Requests details, diff (and tree) for the selected row.
    fn load_selected(&mut self) {
        let Some(c) = self.hist.selected().cloned() else {
            return;
        };
        let h = &mut self.hist;
        h.detail_oid = Some(c.oid);
        h.details = None;
        h.details_error = None;
        h.related = None;
        h.cur_diff = None;
        h.diff.reset_scroll();
        // Keep tag info set just before this debounced reload (clicking a
        // tag label also selects its commit); drop it once the selection
        // has actually moved elsewhere.
        if h.tag_info.as_ref().is_some_and(|t| t.oid != c.oid) {
            h.tag_info = None;
        }
        h.rebuild_diff();
        if !HistoryState::is_pseudo(&c.oid) && h.compare.is_none() {
            self.queue_commit_details(c.oid);
        }
        self.reload_diff();
        if self.hist.tree_mode {
            self.load_tree();
        }
    }

    fn queue_commit_details(&mut self, oid: Oid) {
        let r = self.runner.clone();
        self.req.details = self
            .workers
            .read(self.generation, move || Payload::Details {
                oid,
                details: read::commit_details(&r, &oid),
                related: read::related(&r, &oid),
            });
    }

    /// Shows an annotated tag's tagger/date/message in the details pane,
    /// gitk-style, when its label is clicked in the commit list.
    pub fn show_tag_info(&mut self, full: Vec<u8>) {
        let Some(r) = self.refs.find_full(&full) else {
            return;
        };
        if r.kind != RefKind::Tag {
            return;
        }
        let Some(&i) = self.hist.index.get(&r.target) else {
            self.select_oid(r.target, false);
            return;
        };
        let oid = r.target;
        let short = r.short.clone();
        self.hist.tag_info = Some(TagInfoPanel {
            oid,
            full: full.clone(),
            short,
            loading: true,
            details: None,
        });
        if self.hist.sel == Some(i) {
            self.hist.rebuild_diff();
        } else {
            self.select_row(i);
        }
        let runner = self.runner.clone();
        self.req.tag_info = self
            .workers
            .read(self.generation, move || Payload::TagInfo {
                full: full.clone(),
                result: read::tag_details(&runner, &full),
            });
    }

    /// Re-requests only the diff (options changed).
    fn reload_diff(&mut self) {
        let Some(c) = self.hist.selected().cloned() else {
            return;
        };
        let opts = self.hist.diff_opts();
        let paths = self.hist.view().path_list();
        let r = self.runner.clone();
        let head = self.head();
        let merge = self.hist.merge_diff;
        let compare = self.hist.compare;
        self.req.diff = self.workers.read(self.generation, move || {
            let pb = bytes_list(&paths);
            let pr: Vec<&[u8]> = pb.iter().map(|p| p.as_slice()).collect();
            let d = if let Some((a, b)) = compare {
                read::diff_commits(&r, &a, &b, &opts)
            } else if c.oid == null_unstaged() {
                read::worktree_diff(&r, false, None, &opts, &pr)
            } else if c.oid == null_staged() {
                read::worktree_diff(&r, true, head.as_ref(), &opts, &pr)
            } else {
                read::commit_diff(&r, &c.oid, &c.parents, merge, &opts, &paths)
            };
            Payload::CommitDiff(d)
        });
    }

    fn load_tree(&mut self) {
        let Some(mut oid) = self.hist.selected_oid() else {
            return;
        };
        if HistoryState::is_pseudo(&oid) {
            match self.head() {
                Some(h) => oid = h,
                None => return,
            }
        }
        let r = self.runner.clone();
        self.req.tree = self.workers.read(self.generation, move || {
            Payload::Tree(read::tree_paths(&r, &oid))
        });
    }

    pub fn on_details(
        &mut self,
        oid: Oid,
        details: Result<CommitDetails, crate::git::GitError>,
        related: Related,
    ) {
        if self.hist.detail_oid != Some(oid) {
            return;
        }
        match details {
            Ok(d) => {
                self.hist.details = Some(d);
                self.hist.details_error = None;
                self.hist.related = Some(related);
                self.hist.rebuild_diff();
            }
            Err(e) => {
                let message = format!("Commit details: {e}");
                self.hist.details_error = Some(message.clone());
                self.hist.rebuild_diff();
                self.set_status(message, StatusTone::Error);
            }
        }
    }

    pub fn on_commit_diff(&mut self, r: Result<Diff, crate::git::GitError>) {
        match r {
            Ok(d) => {
                self.hist.files = d.files.iter().map(|f| f.path().to_vec()).collect();
                if !self.hist.tree_mode {
                    self.hist.file_sel = self.hist.file_sel.min(self.hist.files.len());
                }
                self.hist.cur_diff = Some(Arc::new(d));
                self.hist.rebuild_diff();
            }
            Err(e) => {
                self.hist.cur_diff = None;
                self.hist.files.clear();
                self.hist.rebuild_diff();
                self.hist
                    .diff
                    .lines
                    .push(DLine::plain(DKind::Error, e.to_string()));
            }
        }
    }

    pub fn on_tree(&mut self, r: Result<Vec<Vec<u8>>, crate::git::GitError>) {
        match r {
            Ok(t) => {
                self.hist.tree = t;
                self.hist.file_sel = self
                    .hist
                    .file_sel
                    .min(self.hist.tree.len().saturating_sub(1));
            }
            Err(e) => self.set_status(format!("Tree: {e}"), StatusTone::Error),
        }
    }

    pub fn on_blob(&mut self, path: Vec<u8>, blob: Result<Blob, crate::git::GitError>) {
        let d = &mut self.hist.diff;
        d.clear();
        d.lines.push(DLine::plain(
            DKind::FileTitle,
            sanitize::display_bytes(&path),
        ));
        match blob {
            Ok(b) if b.is_binary() => d
                .lines
                .push(DLine::plain(DKind::Info, "Binary file, not shown.")),
            Ok(b) => {
                for l in b.data.split(|c| *c == b'\n') {
                    d.lines.push(DLine::plain(
                        DKind::Ctx,
                        sanitize::display_bytes_tabs(l, self.hist.tab),
                    ));
                }
                if b.truncated {
                    d.lines.push(DLine::plain(
                        DKind::Error,
                        "[file truncated: too large to display]",
                    ));
                }
            }
            Err(e) => d.lines.push(DLine::plain(DKind::Error, e.to_string())),
        }
    }

    pub fn on_find(&mut self, r: Result<HashSet<Oid>, crate::git::GitError>) {
        self.hist.find_running = false;
        match r {
            Ok(s) => {
                let n = s.len();
                self.hist.highlight = Some(s);
                self.set_status(
                    format!("Find: {n} matching commit(s) in this view."),
                    StatusTone::Info,
                );
                if let Some(fwd) = self.hist.find_pending_step.take() {
                    self.find_step(fwd);
                }
            }
            Err(e) => {
                self.hist.find_pending_step = None;
                self.set_status(
                    format!("Find: {}", e.message.lines().next().unwrap_or("")),
                    StatusTone::Error,
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // Selection and navigation

    pub fn select_row(&mut self, i: usize) {
        self.select_row_ex(i, true, true);
    }

    fn select_row_ex(&mut self, i: usize, push: bool, user: bool) {
        let h = &mut self.hist;
        if i >= h.commits.len() {
            return;
        }
        if h.sel == Some(i) && h.compare.is_none() {
            h.ensure_visible(i);
            return;
        }
        if push && let Some(old) = h.selected_oid() {
            h.push_nav(old);
        }
        if user {
            h.pending_select = None;
        }
        h.sel = Some(i);
        h.compare = None;
        h.ensure_visible(i);
        let oid = h.commits[i].oid;
        h.sha.set(&if HistoryState::is_pseudo(&oid) {
            String::new()
        } else {
            oid.to_hex()
        });
        h.sel_changed_at = Some(Instant::now());
        h.file_sel = 0;
        h.file_top = 0;
    }

    pub fn select_oid(&mut self, oid: Oid, push: bool) {
        if self.view != ViewKind::History {
            self.switch_view(ViewKind::History);
        }
        if let Some(&i) = self.hist.index.get(&oid) {
            self.select_row_ex(i, push, true);
            self.center_selection();
        } else if self.hist.loading {
            self.hist.pending_select = Some(oid);
            self.set_status(
                format!(
                    "Commit {} not loaded yet; it will be selected when it appears.",
                    oid.short(10)
                ),
                StatusTone::Info,
            );
        } else {
            self.set_status(
                format!(
                    "SHA1 id {} is not known in the current view.",
                    oid.short(10)
                ),
                StatusTone::Error,
            );
        }
    }

    fn move_sel(&mut self, delta: isize) {
        let n = self.hist.commits.len();
        if n == 0 {
            return;
        }
        let cur = self.hist.sel.map(|s| s as isize).unwrap_or(-1);
        let i = (cur + delta).clamp(0, n as isize - 1) as usize;
        self.select_row(i);
    }

    fn goto_sha(&mut self, text: String) {
        let t = text.trim().to_string();
        if t.is_empty() {
            return;
        }
        let lower = t.to_ascii_lowercase();
        if lower.len() >= 4 && lower.bytes().all(|b| b.is_ascii_hexdigit()) {
            let hits: Vec<usize> = self
                .hist
                .commits
                .iter()
                .enumerate()
                .filter(|(_, c)| c.oid.starts_with_hex(&lower))
                .map(|(i, _)| i)
                .take(2)
                .collect();
            if hits.len() == 1 {
                self.select_row(hits[0]);
                self.center_selection();
                self.hist.focus = HFocus::List;
                return;
            }
            if hits.len() > 1 {
                self.set_status(format!("Short SHA1 id {t} is ambiguous"), StatusTone::Error);
                return;
            }
        }
        let r = self.runner.clone();
        self.req.goto = self.workers.read(self.generation, move || {
            Payload::Goto(read::resolve_commit(&r, &t))
        });
        self.hist.focus = HFocus::List;
    }

    // ------------------------------------------------------------------
    // Find (whole view, through git)

    pub fn find_changed(&mut self) {
        self.hist.find_dirty = true;
        self.hist.find_changed_at = Some(Instant::now());
        if self.hist.find.text.trim().is_empty() {
            self.hist.highlight = None;
        }
    }

    fn run_find(&mut self) {
        let h = &mut self.hist;
        h.find_dirty = false;
        h.find_changed_at = None;
        h.find_cancel.cancel();
        let pattern = h.find.text.trim().to_string();
        if pattern.is_empty() {
            h.highlight = None;
            h.find_running = false;
            h.find_pending_step = None;
            return;
        }
        if h.find_match == MatchKind::Regexp
            && h.find_type == FindType::Containing
            && let Err(e) = Matcher::new(MatchKind::Regexp, &pattern)
        {
            h.find_pending_step = None;
            self.set_status(
                format!("Find: invalid regular expression: {e}"),
                StatusTone::Error,
            );
            return;
        }
        h.find_cancel = Cancel::default();
        h.find_running = true;
        let cancel = h.find_cancel.clone();
        let spec = FindSpec {
            kind: h.find_type,
            pattern,
            matching: h.find_match,
            field: h.find_field,
        };
        let view = h.view().clone();
        let base = h.base_tips.clone();
        let r = self.runner.clone();
        self.searching = true;
        self.req.find = self.workers.stream(self.generation, move |sink| {
            sink.send(Payload::Find(read::find(
                &r,
                &view,
                &base,
                &spec,
                cancel.flag(),
            )));
        });
    }

    /// Moves to the next match: `forward` = towards older commits (down).
    fn find_step(&mut self, forward: bool) {
        if self.hist.find_dirty
            || (self.hist.highlight.is_none() && !self.hist.find.text.trim().is_empty())
        {
            self.hist.find_pending_step = Some(forward);
            self.run_find();
            return;
        }
        if self.hist.find_running {
            self.hist.find_pending_step = Some(forward);
            return;
        }
        let Some(set) = &self.hist.highlight else {
            self.hist.focus = HFocus::Find;
            return;
        };
        let n = self.hist.commits.len();
        let cur = self.hist.sel.map(|s| s as isize).unwrap_or(-1);
        let found = if forward {
            ((cur + 1).max(0) as usize..n).find(|i| set.contains(&self.hist.commits[*i].oid))
        } else {
            (0..cur.max(0) as usize)
                .rev()
                .find(|i| set.contains(&self.hist.commits[*i].oid))
        };
        match found {
            Some(i) => {
                self.select_row(i);
                self.center_selection();
            }
            None => {
                let msg = if forward && self.hist.loading {
                    "No more matches in the loaded commits (still loading)."
                } else if forward {
                    "No more matches (reached the end of the view)."
                } else {
                    "No more matches (reached the top of the view)."
                };
                self.set_status(msg, StatusTone::Info);
            }
        }
    }

    pub fn search_changed(&mut self) {
        let t = self.hist.search.text.clone();
        if t.is_empty() {
            self.hist.diff.set_search(None);
            return;
        }
        match Matcher::new(MatchKind::IgnCase, &t) {
            Ok(m) => {
                self.hist.diff.set_search(Some(m));
                let top = self.hist.diff.top;
                if let Some(&hit) = self.hist.diff.search_hits.iter().find(|h| **h >= top) {
                    self.hist.diff.cursor = hit;
                    self.hist.diff.anchor = None;
                    self.hist.diff.text_selection = None;
                    self.hist.diff.ensure_visible(hit);
                    self.hist.sync_file_selection(hit);
                }
            }
            Err(e) => self.set_status(e, StatusTone::Error),
        }
    }

    fn select_file_row(&mut self, i: usize) {
        let n = self.hist.file_rows();
        if n == 0 {
            return;
        }
        let i = i.min(n - 1);
        self.hist.file_sel = i;
        self.hist.reveal_file_selection();
        if self.hist.tree_mode {
            let (Some(path), Some(mut oid)) = (self.hist.file_path(i), self.hist.selected_oid())
            else {
                return;
            };
            if HistoryState::is_pseudo(&oid) {
                match self.head() {
                    Some(h) => oid = h,
                    None => return,
                }
            }
            let r = self.runner.clone();
            self.req.blob = self.workers.read(self.generation, move || Payload::Blob {
                blob: read::blob(&r, &oid, &path),
                path,
            });
        } else if i == 0 {
            self.hist.diff.top = 0;
            self.hist.diff.cursor = 0;
        } else {
            self.hist.diff.goto_file(i - 1);
        }
    }

    fn set_tree_mode(&mut self, on: bool) {
        if self.hist.tree_mode == on {
            return;
        }
        self.hist.tree_mode = on;
        self.hist.file_sel = 0;
        self.hist.file_top = 0;
        if on {
            self.load_tree();
        } else {
            self.hist.rebuild_diff();
        }
    }

    fn row_oid(&self, row: usize) -> Option<Oid> {
        self.hist
            .commits
            .get(row)
            .map(|c| c.oid)
            .filter(|o| !HistoryState::is_pseudo(o))
    }

    fn compare_rows(&mut self, from: Oid, to: Oid) {
        if from == to {
            return;
        }
        let h = &mut self.hist;
        h.compare = Some((from, to));
        h.details = None;
        h.diff.reset_scroll();
        h.rebuild_diff();
        self.reload_diff();
    }

    fn head_branch_short(&self) -> Option<String> {
        match &self.refs.head {
            Head::Branch { short, .. } => Some(short.clone()),
            Head::Detached(_) => None,
        }
    }

    fn reselect_after_view_change(&mut self) {
        let keep = self.hist.selected_oid();
        self.hist.highlight = None;
        self.load_history(keep);
    }

    // ------------------------------------------------------------------
    // Dispatch

    pub fn dispatch_history(&mut self, a: &Action) -> bool {
        use Action as A;
        match a {
            A::Update | A::Reload => {
                if *a == A::Reload {
                    self.capture_base_tips();
                    self.hist.loaded_view = None;
                }
                self.refresh_all(None);
                self.mark_working();
            }
            A::RereadRefs => self.load_refs(),
            A::ListRefs => self.open_list_refs(),
            A::ListCommitRefs(oid) => self.open_ref_browser(Some(*oid)),
            A::ShowTagInfo(full) => self.show_tag_info(full.clone()),
            A::ToggleRefKind(kind) => {
                if !self.hist.hidden_ref_kinds.remove(kind) {
                    self.hist.hidden_ref_kinds.insert(*kind);
                }
            }
            A::StartGitGui => self.switch_view(ViewKind::Worktree),
            A::NewView => self.open_view_editor(None),
            A::EditView => {
                if self.hist.cur_view == 0 {
                    self.set_status(
                        "The \"All files\" view cannot be edited.",
                        StatusTone::Error,
                    );
                } else {
                    self.open_view_editor(Some(self.hist.cur_view));
                }
            }
            A::DeleteView => {
                if self.hist.cur_view > 0 {
                    self.hist.views.remove(self.hist.cur_view);
                    self.hist.cur_view = 0;
                    self.save_state();
                    self.reselect_after_view_change();
                }
            }
            A::SelectView(i) => {
                if *i < self.hist.views.len() && *i != self.hist.cur_view {
                    self.hist.cur_view = *i;
                    self.reselect_after_view_change();
                }
            }
            A::AboutGitk => self.info("About Git Twin (gitt)", &about_text("gitk")),
            A::ShowMessageLog => self.show_message_log(),
            A::ToggleCompactRefs => {
                self.hist.compact_refs = !self.hist.compact_refs;
                self.hist.expanded_refs.clear();
                self.state.compact_refs = self.hist.compact_refs;
                self.save_state();
            }
            A::KeyBindings => self.dialogs.push(keybindings_dialog()),
            A::SelectRow(i) => {
                self.select_row(*i);
                self.hist.focus = HFocus::List;
            }
            A::MoveSel(d) => self.move_sel(*d),
            A::SelPage(d) => {
                let h = self.hist.height.max(2) as isize - 1;
                self.move_sel(d * h);
            }
            A::SelFirst => self.move_sel(isize::MIN / 2),
            A::SelLast => self.move_sel(isize::MAX / 2),
            A::ScrollList(d) => self.hist.scroll(*d),
            A::GoBack => {
                if let Some(o) = self.hist.back.pop() {
                    if let Some(cur) = self.hist.selected_oid() {
                        self.hist.forward.push(cur);
                    }
                    self.select_oid(o, false);
                }
            }
            A::GoForward => {
                if let Some(o) = self.hist.forward.pop() {
                    if let Some(cur) = self.hist.selected_oid() {
                        self.hist.back.push(cur);
                    }
                    self.select_oid(o, false);
                }
            }
            A::GotoParent(n) => {
                if let Some(p) = self
                    .hist
                    .selected()
                    .and_then(|c| c.parents.get(n.saturating_sub(1)).copied())
                {
                    self.select_oid(p, true);
                }
            }
            A::FocusSha => {
                self.hist.focus = HFocus::Sha;
                self.hist.sha.apply(super::input::Edit::End);
            }
            A::GotoSha(s) => self.goto_sha(s.clone()),
            A::FindNext => self.find_step(true),
            A::FindPrev => self.find_step(false),
            A::FocusFind => self.hist.focus = HFocus::Find,
            A::SetFindType(t) => {
                self.hist.find_type = *t;
                self.find_changed();
            }
            A::SetFindMatch(m) => {
                self.hist.find_match = *m;
                self.find_changed();
            }
            A::SetFindField(f) => {
                self.hist.find_field = *f;
                self.find_changed();
            }
            A::CycleFindType => {
                let i = FindType::ALL
                    .iter()
                    .position(|t| *t == self.hist.find_type)
                    .unwrap_or(0);
                self.hist.find_type = FindType::ALL[(i + 1) % FindType::ALL.len()];
                self.find_changed();
            }
            A::CycleFindMatch => {
                let i = MatchKind::ALL
                    .iter()
                    .position(|t| *t == self.hist.find_match)
                    .unwrap_or(0);
                self.hist.find_match = MatchKind::ALL[(i + 1) % MatchKind::ALL.len()];
                self.find_changed();
            }
            A::CycleFindField => {
                let i = FindField::ALL
                    .iter()
                    .position(|t| *t == self.hist.find_field)
                    .unwrap_or(0);
                self.hist.find_field = FindField::ALL[(i + 1) % FindField::ALL.len()];
                self.find_changed();
            }
            A::DiffScroll(d) => {
                self.hist.diff.scroll(*d);
                self.hist.sync_file_selection(self.hist.diff.top);
            }
            A::DiffPage(d) => {
                let h = self.hist.diff.height.max(2) as isize - 1;
                self.hist.diff.scroll(d * h);
                self.hist.sync_file_selection(self.hist.diff.top);
            }
            A::DiffTop => {
                self.hist.diff.top = 0;
                self.hist.sync_file_selection(self.hist.diff.top);
            }
            A::DiffBottom => {
                self.hist.diff.top = self.hist.diff.max_top();
                self.hist.sync_file_selection(self.hist.diff.top);
            }
            A::DiffNextFile => {
                self.hist.diff.next_file();
                self.hist.sync_file_selection(self.hist.diff.top);
            }
            A::DiffPrevFile => {
                self.hist.diff.prev_file();
                self.hist.sync_file_selection(self.hist.diff.top);
            }
            A::DiffSearchNext | A::DiffSearchPrev => {
                let fwd = *a == A::DiffSearchNext;
                if self.hist.search.text.is_empty() {
                    self.hist.focus = HFocus::Search;
                } else if !self.hist.diff.search_step(fwd) {
                    self.set_status("Search: no match in the diff.", StatusTone::Info);
                } else {
                    self.hist.sync_file_selection(self.hist.diff.cursor);
                }
            }
            A::FocusDiffSearch => self.hist.focus = HFocus::Search,
            A::SetShow(s) => {
                self.hist.show = *s;
                self.hist.rebuild_diff();
            }
            A::ContextMore | A::ContextLess => {
                let c = self.hist.context as i64 + if *a == A::ContextMore { 1 } else { -1 };
                self.hist.context = c.clamp(0, 10_000) as u32;
                self.set_status(
                    format!("Lines of context: {}", self.hist.context),
                    StatusTone::Info,
                );
                self.reload_diff();
            }
            A::ToggleIgnoreSpace => {
                self.hist.ignore_space = !self.hist.ignore_space;
                self.reload_diff();
            }
            A::SetWordMode(m) => {
                self.hist.word_mode = *m;
                self.reload_diff();
            }
            A::CycleWordMode => {
                self.hist.word_mode = match self.hist.word_mode {
                    WordMode::Line => WordMode::Markup,
                    WordMode::Markup => WordMode::Color,
                    WordMode::Color => WordMode::Line,
                };
                self.reload_diff();
            }
            A::SetTreeMode(b) => self.set_tree_mode(*b),
            A::SelectFileRow(i) | A::OpenFileRow(i) => {
                self.hist.focus = HFocus::Files;
                self.select_file_row(*i);
            }
            A::ParentDiffMode(n) => {
                self.hist.merge_diff = if *n == 0 {
                    MergeDiff::Combined
                } else {
                    MergeDiff::Parent(*n)
                };
                self.hist.rebuild_diff();
                self.reload_diff();
            }
            A::DiffThisSelected(row) | A::DiffSelectedThis(row) => {
                let (Some(this), Some(sel)) = (self.row_oid(*row), self.hist.selected_oid()) else {
                    return true;
                };
                if HistoryState::is_pseudo(&sel) {
                    return true;
                }
                if matches!(a, A::DiffThisSelected(_)) {
                    self.compare_rows(this, sel);
                } else {
                    self.compare_rows(sel, this);
                }
            }
            A::MakePatch(row) => {
                if let Some(this) = self.row_oid(*row) {
                    let sel = self
                        .hist
                        .selected_oid()
                        .filter(|o| !HistoryState::is_pseudo(o));
                    let (from, to) = match sel {
                        Some(s) if s != this => (this, s),
                        _ => {
                            let p = self.hist.commits[*row].parents.first().copied();
                            match p {
                                Some(p) => (p, this),
                                None => {
                                    self.set_status(
                                        "Root commit: no parent to diff against.",
                                        StatusTone::Error,
                                    );
                                    return true;
                                }
                            }
                        }
                    };
                    self.open_make_patch(from, to);
                }
            }
            A::CreateTag(row) => {
                if let Some(o) = self.row_oid(*row) {
                    self.open_create_tag(o);
                }
            }
            A::CopyCommitRef(row) => {
                if let Some(c) = self.hist.commits.get(*row) {
                    let date: String = c.date.chars().take(10).collect();
                    let s = format!("{} (\"{}\", {date})", c.oid.short(7), c.subject);
                    self.copy_text(s);
                }
            }
            A::CopyCommitId(row) => {
                if let Some(o) = self.row_oid(*row) {
                    self.copy_text(o.to_hex());
                }
            }
            A::WriteCommit(row) => {
                if let Some(o) = self.row_oid(*row) {
                    self.open_write_commit(o);
                }
            }
            A::CreateBranchAt(row) => {
                if let Some(o) = self.row_oid(*row) {
                    self.open_create_branch_at(o);
                }
            }
            A::CherryPick(row) => self.cherry_pick_row(*row),
            A::ResetHere(row) => {
                if let Some(o) = self.row_oid(*row) {
                    if self.head_branch_short().is_none() {
                        self.set_status("Detached head: can't reset", StatusTone::Error);
                    } else if self.guard_write() {
                        self.open_reset(o);
                    }
                }
            }
            A::ResetTo(oid, mode) => {
                let (oid, mode) = (*oid, *mode);
                let head = self.head();
                self.run_op(
                    "Reset",
                    Box::new(move |r| ops::reset(r, mode, &oid)),
                    After::Refresh,
                    Some(head),
                );
            }
            A::StashChanges => {
                self.run_op("Stash", Box::new(ops::stash), After::Refresh, None);
            }
            A::MarkCommit(row) => {
                if let Some(o) = self.row_oid(*row) {
                    self.hist.mark = Some(o);
                    self.set_status(format!("Marked commit {}", o.short(10)), StatusTone::Info);
                }
            }
            A::ReturnToMark => match self.hist.mark {
                Some(m) => self.select_oid(m, true),
                None => self.set_status("No marked commit.", StatusTone::Error),
            },
            A::FindDescendant(row) => self.find_descendant(*row),
            A::CompareWithMarked(row) => {
                if let (Some(this), Some(m)) = (self.row_oid(*row), self.hist.mark) {
                    let r = self.runner.clone();
                    self.workers.read(self.generation, move || {
                        let res = r
                            .read(&[
                                os("range-diff"),
                                os("--no-color"),
                                os(format!("{}^!", m.to_hex())),
                                os(format!("{}^!", this.to_hex())),
                            ])
                            .map(|o| sanitize::display_bytes(&o));
                        Payload::Text {
                            title: "Compare with marked commit".into(),
                            result: res,
                        }
                    });
                }
            }
            A::DiffMarked(row, this_to_marked) => {
                if let (Some(this), Some(m)) = (self.row_oid(*row), self.hist.mark) {
                    if *this_to_marked {
                        self.compare_rows(this, m);
                    } else {
                        self.compare_rows(m, this);
                    }
                }
            }
            A::RevertCommit(row) => self.revert_row(*row),
            A::CheckoutCommit(row) => {
                if let Some(o) = self.row_oid(*row) {
                    if !self.guard_write() {
                        return true;
                    }
                    let head = self.head();
                    self.run_pending(Pending::Op {
                        label: "Checkout".into(),
                        f: Box::new(move |r| ops::checkout_detach(r, &o)),
                        after: After::Refresh,
                        expect_head: Some(head),
                    });
                }
            }
            A::CheckoutRef(full) => self.checkout_ref(full.clone()),
            A::RenameBranch(full) => {
                if self.guard_write() {
                    self.open_rename_branch(full.clone());
                }
            }
            A::RemoveBranch(full) => self.remove_branch(full.clone()),
            A::CopyRefName(full) => {
                if let Some(r) = self.refs.find_full(full) {
                    let s = r.short.clone();
                    self.copy_text(s);
                }
            }
            A::HighlightPath(path, only) => {
                let p = sanitize::display_bytes(path);
                let quoted = if p.contains(char::is_whitespace) {
                    format!("\"{p}\"")
                } else {
                    p
                };
                if *only || self.hist.find_type != FindType::TouchingPaths {
                    self.hist.find.set(&quoted);
                } else {
                    let t = format!("{} {quoted}", self.hist.find.text.trim());
                    self.hist.find.set(t.trim());
                }
                self.hist.find_type = FindType::TouchingPaths;
                self.run_find();
            }
            A::ClearHighlight => {
                self.hist.find.clear();
                self.hist.highlight = None;
            }
            A::ExternalDiff(path) => self.external_diff(path.clone()),
            A::BlameParent(path) => self.blame_parent(path.clone()),
            A::CopyPath(p) => self.copy_text(sanitize::display_bytes(p)),
            A::LineOrigin(idx) => self.line_origin(*idx),
            _ => return false,
        }
        true
    }

    fn cherry_pick_row(&mut self, row: usize) {
        let Some(oid) = self.row_oid(row) else { return };
        if !self.guard_write() {
            return;
        }
        let parents = self.hist.commits[row].parents.len();
        if parents > 1 {
            self.open_cherry_pick_merge(oid, parents);
            return;
        }
        let head = self.head();
        self.run_pending(Pending::Op {
            label: "Cherry-pick".into(),
            f: Box::new(move |r| ops::cherry_pick(r, &oid, None)),
            after: After::Refresh,
            expect_head: Some(head),
        });
    }

    fn revert_row(&mut self, row: usize) {
        let Some(oid) = self.row_oid(row) else { return };
        if !self.guard_write() {
            return;
        }
        let head = self.head();
        let mainline = (self.hist.commits[row].parents.len() > 1).then_some(1);
        self.run_pending(Pending::Op {
            label: "Revert".into(),
            f: Box::new(move |r| ops::revert_commit(r, &oid, mainline)),
            after: After::Refresh,
            expect_head: Some(head),
        });
    }

    fn find_descendant(&mut self, row: usize) {
        let (Some(this), Some(m)) = (self.row_oid(row), self.hist.mark) else {
            self.set_status("Mark a commit first.", StatusTone::Error);
            return;
        };
        let a = self.hist.descendants(&this);
        let b = self.hist.descendants(&m);
        let best = a
            .intersection(&b)
            .filter_map(|o| self.hist.index.get(o).copied())
            .filter(|i| !HistoryState::is_pseudo(&self.hist.commits[*i].oid))
            .max();
        match best {
            Some(i) => {
                self.select_row(i);
                self.center_selection();
            }
            None => self.set_status(
                "No common descendant in the loaded commits.",
                StatusTone::Info,
            ),
        }
    }

    fn checkout_ref(&mut self, full: Vec<u8>) {
        if !self.guard_write() {
            return;
        }
        let Some(r) = self.refs.find_full(&full).cloned() else {
            return;
        };
        if self.refs.is_head_ref(&r) {
            self.set_status("Already on this branch.", StatusTone::Info);
            return;
        }
        let head = self.head();
        let shown = r.short.clone();
        let (label, f): (String, super::OpFn) = match r.kind {
            RefKind::LocalBranch => {
                let name = String::from_utf8_lossy(r.short_bytes()).into_owned();
                (
                    format!("Check out branch {shown}"),
                    Box::new(move |rr| ops::checkout_branch(rr, &name)),
                )
            }
            RefKind::RemoteBranch => {
                let remote_short = String::from_utf8_lossy(r.short_bytes()).into_owned();
                let local = remote_short
                    .split_once('/')
                    .map(|(_, b)| b.to_string())
                    .unwrap_or_default();
                let exists = self
                    .refs
                    .local_branches()
                    .any(|b| b.short_bytes() == local.as_bytes());
                if local.is_empty() || local == "HEAD" {
                    return;
                }
                if exists {
                    (
                        format!("Check out branch {local}"),
                        Box::new(move |rr| ops::checkout_branch(rr, &local)),
                    )
                } else {
                    (
                        format!("Create local branch {local} tracking {shown}"),
                        Box::new(move |rr| {
                            ops::create_branch(rr, &local, &remote_short, false, true)
                        }),
                    )
                }
            }
            _ => {
                let t = r.target;
                (
                    format!("Check out {shown} (detached HEAD)"),
                    Box::new(move |rr| ops::checkout_detach(rr, &t)),
                )
            }
        };
        self.run_pending(Pending::Op {
            label,
            f,
            after: After::Refresh,
            expect_head: Some(head),
        });
    }

    fn remove_branch(&mut self, full: Vec<u8>) {
        if !self.guard_write() {
            return;
        }
        let Some(r) = self.refs.find_full(&full).cloned() else {
            return;
        };
        if r.kind != RefKind::LocalBranch {
            self.set_status(
                "Only local branches can be removed here.",
                StatusTone::Error,
            );
            return;
        }
        if self.refs.is_head_ref(&r) {
            self.set_status(
                "Cannot delete the currently checked out branch.",
                StatusTone::Error,
            );
            return;
        }
        let name = r.short_bytes().to_vec();
        let n2 = name.clone();
        self.confirm(
            "Remove branch",
            &[(&format!("Remove branch {}?", r.short), Tone::Warn)],
            "Remove",
            Pending::Op {
                label: format!("Remove branch {}", r.short),
                f: Box::new(move |rr| ops::delete_branch(rr, &n2, false)),
                after: After::OfferForceDelete(name),
                expect_head: None,
            },
        );
    }

    fn external_diff(&mut self, path: Vec<u8>) {
        if self.hist.tree_mode {
            self.set_status(
                "External diff is not available in Tree mode.",
                StatusTone::Error,
            );
            return;
        }
        let Some(c) = self.hist.selected().cloned() else {
            return;
        };
        let mut args = vec![os("difftool"), os("--no-prompt")];
        if let Some((a, b)) = self.hist.compare {
            args.push(os(a.to_hex()));
            args.push(os(b.to_hex()));
        } else if c.oid == null_unstaged() {
        } else if c.oid == null_staged() {
            args.push(os("--cached"));
        } else {
            match c.parents.first() {
                Some(p) => args.push(os(p.to_hex())),
                None => args.push(os(read::empty_tree(&self.runner).unwrap_or_default())),
            }
            args.push(os(c.oid.to_hex()));
        }
        args.push(os("--"));
        args.push(std::os::unix::ffi::OsStringExt::from_vec(path));
        self.request_handoff(Handoff::Git {
            args,
            label: "External diff (git difftool)".into(),
        });
    }

    fn blame_parent(&mut self, path: Vec<u8>) {
        let Some(c) = self.hist.selected().cloned() else {
            return;
        };
        let rev = if HistoryState::is_pseudo(&c.oid) {
            self.head()
        } else {
            c.parents.first().copied()
        };
        let Some(rev) = rev else {
            self.set_status("No parent commit to blame.", StatusTone::Error);
            return;
        };
        let r = self.runner.clone();
        let title = format!(
            "Blame {} at {}",
            sanitize::display_bytes(&path),
            rev.short(10)
        );
        self.workers.read(self.generation, move || {
            let res = r
                .read_opts(
                    &[
                        os("blame"),
                        os("--date=short"),
                        os(rev.to_hex()),
                        os("--"),
                        std::os::unix::ffi::OsStringExt::from_vec(path),
                    ],
                    crate::git::Opts {
                        literal_paths: true,
                        limit: Some(2 * 1024 * 1024),
                        ..Default::default()
                    },
                    &[0],
                )
                .map(|o| sanitize::display_bytes_tabs(&o.stdout, 8).replace("^M", ""));
            Payload::Text { title, result: res }
        });
    }

    fn line_origin(&mut self, idx: usize) {
        let Some(c) = self.hist.selected().cloned() else {
            return;
        };
        let d = &self.hist.diff;
        let Some(fi) = d.lines.get(idx).and_then(|l| l.file) else {
            return;
        };
        let Some(path) = self.hist.files.get(fi).cloned() else {
            return;
        };
        let (rev, line) = if let Some((_, n)) = d.new_line_number(idx) {
            let rev = if HistoryState::is_pseudo(&c.oid) {
                None
            } else {
                Some(self.hist.compare.map(|x| x.1).unwrap_or(c.oid))
            };
            (rev, n)
        } else if let Some((_, n)) = d.old_line_number(idx) {
            let rev = if let Some((a, _)) = self.hist.compare {
                Some(a)
            } else if HistoryState::is_pseudo(&c.oid) {
                self.head()
            } else {
                c.parents.first().copied()
            };
            (rev, n)
        } else {
            return;
        };
        let Some(rev) = rev else {
            self.set_status(
                "Origin of uncommitted lines: not committed yet.",
                StatusTone::Info,
            );
            return;
        };
        let r = self.runner.clone();
        self.req.origin = self.workers.read(self.generation, move || {
            Payload::Origin(read::blame_line(&r, &rev, &path, line))
        });
    }
}

pub fn about_text(which: &str) -> String {
    format!(
        "{} {}\n\n\
         A terminal interface inspired by {which}; not the original application.\n\
         Launch with: {}\n\
         F6 switches between history and working tree.\n\n\
         Repository operations use the Git command line.\n\
         License: Apache-2.0",
        if which == "git gui" {
            "Git TUI"
        } else {
            "Git Twin"
        },
        env!("CARGO_PKG_VERSION"),
        if which == "git gui" {
            "git tui (executable: git-tui)"
        } else {
            "gitt"
        }
    )
}

fn keybindings_dialog() -> Dialog {
    const KEY_WIDTH: usize = 27;
    let sections: &[(&str, &[(&str, &str)])] = &[
        (
            "GLOBAL",
            &[
                ("F10 / Alt+letter", "Open menus"),
                ("Tab / Shift-Tab", "Move to next / previous pane"),
                ("Ctrl-Q / Ctrl-W", "Quit"),
                ("F12", "Command palette (search all commands)"),
                ("F11", "Maximize / restore focused pane"),
                ("Ctrl-1 .. 9", "Select n-th parent (History)"),
                ("Alt-1 .. 4", "Focus pane (Worktree)"),
                ("F6 / Ctrl-Tab", "Switch History / Worktree view"),
                (
                    "Shift-F10 / right click",
                    "Context menu for row / file / line",
                ),
            ],
        ),
        (
            "HISTORY",
            &[
                ("Up / p / k", "Previous commit"),
                ("Down / n / j", "Next commit"),
                ("PageUp / PageDown", "Move by page"),
                ("Home / End", "First / last commit"),
                (
                    "Ctrl-Up/Down/PgUp/PgDn",
                    "Scroll list without changing selection",
                ),
                ("Left / z / h", "Back in selection history"),
                ("Right / x / l", "Forward in selection history"),
                ("Shift-Up / Shift-Down", "Find previous / next match"),
                ("/ or Ctrl-F", "Open find box"),
                ("Enter / Ctrl-G", "Next match"),
                ("?", "Previous match"),
                ("g / f", "Focus SHA entry / next file in diff"),
                ("Space / d", "Scroll diff down"),
                ("Backspace / Delete / b / u", "Scroll diff up"),
                ("Ctrl-S / Ctrl-R", "Next / previous diff search hit"),
                ("Ctrl-+ / Ctrl--", "More / less context"),
                ("F5 / Shift-F5", "Update / reload"),
                ("F2", "Reference browser (History stays interactive)"),
                ("F4 / Shift-F4", "Edit / create view"),
                ("c (diff pane)", "Copy selection"),
                ("Drag / Shift-Left/Right", "Select text (copy only)"),
                ("Shift-Up/Down (diff)", "Select whole lines"),
            ],
        ),
        (
            "WORKTREE",
            &[
                ("Ctrl-T / Ctrl-U", "Stage / unstage selected files"),
                ("Ctrl-I", "Stage visible changed files"),
                ("/ or Ctrl-F (file list)", "Edit this list's path filter"),
                ("Ctrl-U (filter)", "Clear filter; Enter/Esc returns to list"),
                ("Ctrl-A (file list)", "Select all visible files"),
                ("Ctrl-J", "Revert selected changes"),
                ("Ctrl-E / Ctrl-S", "Amend last commit / sign off"),
                ("Ctrl-Enter / Alt-Enter", "Commit"),
                ("F5 / Ctrl-R", "Rescan"),
                ("Ctrl-N / Ctrl-O", "Create branch / checkout"),
                ("Ctrl-M / Ctrl-P", "Merge / push"),
                ("Ctrl-- / Ctrl-+", "Less / more context"),
                ("Space / status click", "Stage / unstage file"),
                ("Up / Down (diff)", "Move between lines"),
                ("Shift-Up/Down (diff)", "Select lines"),
                ("s / r (diff)", "Stage / revert hunk or selected lines"),
                ("Ctrl-C / drag", "Copy selection / select text"),
                ("Ctrl-Z / Ctrl-Y", "Undo / redo message"),
                ("Ctrl-X / Ctrl-C / Ctrl-V", "Cut / copy / paste message"),
                ("Ctrl-A", "Select all message text"),
                ("Ctrl-X Ctrl-E / F4", "Open external editor"),
            ],
        ),
        (
            "COMPACT SCREENS & FORMS",
            &[
                ("Below 120x35", "Click pane tabs or use Tab; F11 zooms pane"),
                ("Form scrolling", "Wheel or Alt-PageUp/PageDown"),
                ("Alt-Home / Alt-End", "Jump to form start / end"),
            ],
        ),
    ];

    let mut dialog = Dialog::new("Key bindings", Kind::Info);
    dialog.width = 78;
    dialog.buttons = vec!["Close".into()];
    for (section, bindings) in sections {
        if !dialog.body.is_empty() {
            dialog.body.push((String::new(), Tone::Normal));
        }
        dialog.body.push((section.to_string(), Tone::Strong));
        for (key, description) in *bindings {
            dialog
                .body
                .push((format!("  {key:<KEY_WIDTH$}  {description}"), Tone::Code));
        }
    }
    dialog
}
