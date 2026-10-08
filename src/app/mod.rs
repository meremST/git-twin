//! Application state and behaviour, independent from the terminal.

pub mod action;
pub mod dialog;
pub mod diffview;
pub mod forms;
pub mod history;
pub mod input;
pub mod keys;
pub mod menu;
pub mod mouse;
pub mod worker;
pub mod worktree;

use std::ffi::OsString;
use std::time::{Duration, Instant};

use crate::config::{Config, State, ViewKind};
use crate::domain::oid::Oid;
use crate::domain::refs::RefSet;
use crate::git::read::GuiTool;
use crate::git::repo::{RepoInfo, RepoState};
use crate::git::{GitError, Runner};
use crate::ui::hit::{HitMap, PaneId};
use crate::ui::theme::Theme;

use action::Action;
use dialog::{Dialog, Kind, Tone};
use history::HistoryState;
use menu::MenuState;
use mouse::{ClickTracker, Drag};
use worker::{After, Payload, Workers};
use worktree::WorktreeState;

pub type OpFn = Box<dyn FnOnce(&Runner) -> Result<String, GitError> + Send>;

/// Work handed to the terminal (TUI suspended while it runs).
#[derive(Clone, Debug)]
pub enum Handoff {
    Git {
        args: Vec<OsString>,
        label: String,
    },
    Shell {
        cmd: String,
        label: String,
        envs: Vec<(String, OsString)>,
    },
    Commit {
        message: String,
        amend: bool,
    },
    /// Edit the commit message with $GIT_EDITOR.
    Editor,
}

/// Operation waiting for a confirmation.
pub enum Pending {
    Op {
        label: String,
        f: OpFn,
        after: After,
        /// Refuse when HEAD moved since the dialog was opened.
        expect_head: Option<Option<Oid>>,
    },
    Handoff(Handoff),
    Act(Action),
}

#[derive(Default)]
pub struct Requests {
    pub log: u64,
    pub refs: u64,
    pub details: u64,
    pub diff: u64,
    pub tree: u64,
    pub blob: u64,
    pub find: u64,
    pub highlight: u64,
    pub status: u64,
    pub wtdiff: u64,
    pub goto: u64,
    pub origin: u64,
    pub amend: u64,
    pub tag_info: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusTone {
    Info,
    Busy,
    Error,
}

pub struct App {
    pub repo: RepoInfo,
    pub runner: Runner,
    pub cfg: Config,
    pub state: State,
    pub read_only: bool,
    pub view: ViewKind,
    pub hist: HistoryState,
    pub wt: WorktreeState,
    pub menu: Option<MenuState>,
    pub dialogs: Vec<Dialog>,
    pub status: (String, StatusTone),
    /// Set by [`App::set_busy_status`] to the time a transient busy message
    /// (e.g. "Reloading...", "Rescanning...") was shown and that exact text,
    /// so `tick` can restore the status bar to "Ready." once the triggered
    /// background work completes and at least a minimal duration has passed
    /// (avoids a flicker on fast refreshes).
    pub pending_busy_status: Option<(Instant, &'static str)>,
    pub workers: Workers,
    pub generation: u64,
    pub req: Requests,
    pub hits: HitMap,
    pub clicks: ClickTracker,
    pub drag: Option<Drag>,
    pub maximized: Option<PaneId>,
    pub clipboard: String,
    pub osc52: Option<String>,
    pub quit: bool,
    pub handoff: Option<Handoff>,
    pub refs: RefSet,
    pub repo_state: RepoState,
    pub guitools: Vec<GuiTool>,
    pub remotes: Vec<String>,
    pub theme: Theme,
    pub size: (u16, u16),
    pub last_draft_save: Instant,
    pub git_version: String,
    pub mouse_enabled: bool,
    pub persistence_error: Option<String>,
    /// Timestamp of the previous Ctrl-C press, used to require a quick double
    /// press to quit (Ctrl-C alone still copies, like elsewhere in the app).
    pub last_ctrl_c: Option<Instant>,
}

pub struct Options {
    pub view: Option<ViewKind>,
    pub read_only: bool,
    pub mouse: Option<bool>,
    /// Like `gitk --all`: the default view shows every ref, not just HEAD.
    pub all_refs: bool,
    /// Like `gitk PATH...`: the default view only shows commits touching
    /// these paths (relative to the working tree root).
    pub paths: Vec<String>,
}

impl App {
    pub fn new(repo: RepoInfo, cfg: Config, state: State, opts: Options) -> App {
        let mut runner = Runner::new(repo.cwd().to_path_buf(), opts.read_only);
        runner.allow_ext_diff = cfg.allow_textconv;
        let view = if repo.is_bare() {
            ViewKind::History
        } else {
            opts.view.or(state.last_view).unwrap_or(ViewKind::History)
        };
        let theme = Theme::new(&cfg);
        let mut hist = HistoryState::new(&cfg, &state);
        if let Some(default_view) = hist.views.first_mut() {
            default_view.all_refs |= opts.all_refs;
            if !opts.paths.is_empty() {
                default_view.name = "Command line".into();
                default_view.paths = opts.paths.join("\n");
            }
        }
        hist.views.extend(state.views.iter().cloned());
        let mut wt = WorktreeState::new(&cfg);
        let draft_error = match crate::config::load_draft(&repo.git_dir) {
            Ok(Some(d)) => {
                wt.message = input::TextArea::new(&d);
                None
            }
            Ok(None) => None,
            Err(e) => Some(e),
        };
        let mouse_enabled = opts.mouse.unwrap_or(cfg.mouse);
        App {
            runner,
            read_only: opts.read_only,
            view,
            hist,
            wt,
            menu: None,
            dialogs: draft_error
                .as_ref()
                .map(|e| Dialog::error("Cannot read draft", e))
                .into_iter()
                .collect(),
            status: (String::from("Ready."), StatusTone::Info),
            pending_busy_status: None,
            workers: Workers::new(4),
            generation: 1,
            req: Requests::default(),
            hits: HitMap::default(),
            clicks: ClickTracker::new(Duration::from_millis(cfg.double_click_ms)),
            drag: None,
            maximized: None,
            clipboard: String::new(),
            osc52: None,
            quit: false,
            handoff: None,
            refs: RefSet::empty(),
            repo_state: RepoState::default(),
            guitools: Vec::new(),
            remotes: Vec::new(),
            theme,
            size: (0, 0),
            last_draft_save: Instant::now(),
            git_version: crate::git::repo::git_version()
                .map(|v| v.3)
                .unwrap_or_default(),
            mouse_enabled,
            persistence_error: draft_error,
            last_ctrl_c: None,
            repo,
            cfg,
            state,
        }
    }

    /// Initial loading.
    pub fn start(&mut self) {
        self.refresh_all(None);
        if self.read_only {
            self.set_status(
                "Read-only mode: all mutations are disabled.",
                StatusTone::Info,
            );
        }
    }

    pub fn set_status(&mut self, s: impl Into<String>, tone: StatusTone) {
        self.status = (s.into(), tone);
    }

    /// Shows a transient busy message (e.g. "Reloading...", "Rescanning...")
    /// that `tick` automatically clears back to "Ready." once the triggered
    /// background work finishes, holding it for at least 1s so fast
    /// refreshes don't just flicker.
    pub fn set_busy_status(&mut self, s: &'static str) {
        self.set_status(s, StatusTone::Busy);
        self.pending_busy_status = Some((Instant::now(), s));
    }

    pub fn error(&mut self, title: &str, e: impl std::fmt::Display) {
        let text = e.to_string();
        self.set_status(
            format!("{title}: {}", text.lines().next().unwrap_or("")),
            StatusTone::Error,
        );
        self.dialogs.push(Dialog::error(title, &text));
    }

    pub fn info(&mut self, title: &str, text: &str) {
        self.dialogs.push(Dialog::info(title, text));
    }

    pub fn confirm(&mut self, title: &str, body: &[(&str, Tone)], yes: &str, p: Pending) {
        let mut d = Dialog::new(title, Kind::Confirm(Box::new(p)))
            .with_body(body)
            .with_buttons(&[yes, "Cancel"]);
        d.cancel = Some(1);
        d.focus = 1;
        self.dialogs.push(d);
    }

    pub fn is_bare(&self) -> bool {
        self.repo.is_bare()
    }

    /// Refuses mutations in read-only mode with a visible message.
    pub fn guard_write(&mut self) -> bool {
        if self.read_only {
            self.set_status(
                "Refused: git-twin runs in --read-only mode.",
                StatusTone::Error,
            );
            return false;
        }
        true
    }

    pub fn head(&self) -> Option<Oid> {
        self.refs.head.oid()
    }

    /// Re-reads refs, history and working tree. Increments the generation
    /// so that answers to older requests are ignored.
    pub fn refresh_all(&mut self, select: Option<Oid>) {
        self.generation += 1;
        self.load_refs();
        let keep = select.or_else(|| self.hist.selected_oid());
        self.load_history(keep);
        if !self.is_bare() {
            self.rescan();
        }
        self.repo_state = crate::git::repo::repo_state(&self.repo);
        self.guitools = crate::git::read::guitools(&self.runner);
        self.remotes = crate::git::read::remotes(&self.runner).unwrap_or_default();
    }

    pub fn load_refs(&mut self) {
        let r = self.runner.clone();
        self.req.refs = self.workers.read(self.generation, move || {
            Payload::Refs(crate::git::read::load_refs(&r))
        });
    }

    /// Queues a mutation. `expect_head` lets the job refuse to run if HEAD
    /// moved since the user confirmed.
    pub fn run_op(&mut self, label: &str, f: OpFn, after: After, expect_head: Option<Option<Oid>>) {
        if !self.guard_write() {
            return;
        }
        let r = self.runner.clone();
        let label_s = label.to_string();
        self.set_status(format!("{label}..."), StatusTone::Busy);
        self.workers.write(self.generation, label, move || {
            if let Some(exp) = expect_head {
                let now = crate::git::read::load_refs(&r).ok().and_then(|rs| rs.head.oid());
                if now != exp {
                    return Payload::Op {
                        label: label_s,
                        result: Err(GitError::new(
                            "check",
                            "HEAD changed since this action was prepared; the repository was re-read, please try again.",
                        )),
                        after: After::Refresh,
                    };
                }
            }
            let result = f(&r);
            Payload::Op {
                label: label_s,
                result,
                after,
            }
        });
    }

    pub fn run_pending(&mut self, p: Pending) {
        match p {
            Pending::Op {
                label,
                f,
                after,
                expect_head,
            } => self.run_op(&label, f, after, expect_head),
            Pending::Handoff(h) => self.request_handoff(h),
            Pending::Act(a) => self.dispatch(a),
        }
    }

    pub fn request_handoff(&mut self, h: Handoff) {
        if !matches!(h, Handoff::Editor) && !self.guard_write() {
            return;
        }
        if self.workers.writing() {
            self.set_status(
                "Busy: wait for the running operation to finish.",
                StatusTone::Error,
            );
            return;
        }
        self.handoff = Some(h);
    }

    /// Called by the main loop after a hand-off finished.
    pub fn after_handoff(&mut self, h: &Handoff, result: Result<bool, String>) {
        match (h, result) {
            (Handoff::Editor, Ok(_)) => {}
            (Handoff::Commit { .. }, Ok(true)) => {
                self.on_committed();
                self.set_status("Commit created.", StatusTone::Info);
            }
            (_, Ok(true)) => self.set_status("Done.", StatusTone::Info),
            (_, Ok(false)) => self.set_status(
                "Command failed (see the terminal output).",
                StatusTone::Error,
            ),
            (_, Err(e)) => self.error("Error", e),
        }
        self.refresh_all(None);
    }

    pub fn copy_text(&mut self, s: String) {
        if s.is_empty() {
            return;
        }
        let n = s.chars().count();
        self.clipboard = s.clone();
        if self.cfg.osc52 {
            self.osc52 = Some(s);
            self.set_status(
                format!("Copied {n} characters to the clipboard."),
                StatusTone::Info,
            );
        } else {
            self.set_status(
                format!("Copied {n} characters (internal clipboard; enable osc52 in Preferences for the system clipboard)."),
                StatusTone::Info,
            );
        }
    }

    pub fn save_state(&mut self) {
        if let Err(e) = self.try_save_state() {
            self.report_persistence_error(e);
        }
    }

    fn try_save_state(&mut self) -> Result<(), String> {
        if !self.is_bare() {
            self.state.last_view = Some(self.view);
        }
        self.state.views = self
            .hist
            .views
            .iter()
            .skip(1)
            .filter(|v| v.remember)
            .cloned()
            .collect();
        self.state.save()
    }

    pub fn save_draft(&mut self) {
        if let Err(e) = self.try_save_draft() {
            self.report_persistence_error(e);
        }
    }

    fn try_save_draft(&mut self) -> Result<(), String> {
        self.last_draft_save = Instant::now();
        if self.wt.message.modified {
            crate::config::save_draft(&self.repo.git_dir, &self.wt.message.text())?;
            self.wt.message.modified = false;
        }
        Ok(())
    }

    fn report_persistence_error(&mut self, error: String) {
        self.set_status(format!("Not saved: {error}"), StatusTone::Error);
        if self.persistence_error.as_ref() != Some(&error) {
            self.dialogs.push(Dialog::error("Cannot save", &error));
        }
        self.persistence_error = Some(error);
    }

    /// Periodic work: debounced loads, draft saving.
    pub fn tick(&mut self) {
        let now = Instant::now();
        self.history_tick(now);
        if now.duration_since(self.last_draft_save) > Duration::from_secs(2) {
            self.save_draft();
        }
        if let Some((started, text)) = self.pending_busy_status
            && !self.busy()
            && now.duration_since(started) >= Duration::from_millis(500)
        {
            self.pending_busy_status = None;
            if self.status == (String::from(text), StatusTone::Busy) {
                self.set_status("Ready.", StatusTone::Info);
            }
        }
    }

    pub fn on_msg(&mut self, m: worker::Msg) {
        let fresh = m.generation == self.generation;
        match m.payload {
            Payload::LogBatch { commits, graph } => {
                if m.id == self.req.log {
                    self.on_log_batch(commits, graph);
                }
            }
            Payload::LogDone(r) => {
                if m.id == self.req.log {
                    self.on_log_done(r);
                }
            }
            Payload::Refs(r) => {
                if m.id == self.req.refs && fresh {
                    match r {
                        Ok(rs) => {
                            self.refs = rs;
                            self.wt.branch_label = self.refs.head.label();
                        }
                        Err(e) => {
                            self.set_status(format!("Cannot read refs: {e}"), StatusTone::Error)
                        }
                    }
                }
            }
            Payload::Details {
                oid,
                details,
                related,
            } => {
                if m.id == self.req.details {
                    self.on_details(oid, details, related);
                }
            }
            Payload::CommitDiff(r) => {
                if m.id == self.req.diff {
                    self.on_commit_diff(r);
                }
            }
            Payload::Tree(r) => {
                if m.id == self.req.tree {
                    self.on_tree(r);
                }
            }
            Payload::Blob { path, blob } => {
                if m.id == self.req.blob {
                    self.on_blob(path, blob);
                }
            }
            Payload::Find(r) => {
                if m.id == self.req.find {
                    self.on_find(r);
                }
            }
            Payload::Highlight(r) => {
                if m.id == self.req.highlight {
                    match r {
                        Ok(s) => self.hist.highlight = Some(s),
                        Err(e) => {
                            self.set_status(format!("Highlight failed: {e}"), StatusTone::Error)
                        }
                    }
                }
            }
            Payload::Status(r) => {
                if m.id == self.req.status {
                    self.on_status(r);
                }
            }
            Payload::WtDiff(r) => {
                if m.id == self.req.wtdiff {
                    self.on_wt_diff(r);
                }
            }
            Payload::Op {
                label,
                result,
                after,
            } => self.on_op(label, result, after),
            Payload::Text { title, result } => match result {
                Ok(t) => self.info(&title, &t),
                Err(e) => self.error(&title, e),
            },
            Payload::Goto(r) => {
                if m.id == self.req.goto {
                    match r {
                        Ok(oid) => self.select_oid(oid, true),
                        Err(e) => self.error("Go to commit", e),
                    }
                }
            }
            Payload::Origin(r) => {
                if m.id == self.req.origin {
                    match r {
                        Ok((oid, line)) => {
                            self.select_oid(oid, true);
                            self.set_status(
                                format!("Origin: {} line {line}", oid.short(10)),
                                StatusTone::Info,
                            );
                        }
                        Err(e) => self.error("Show origin of this line", e),
                    }
                }
            }
            Payload::Amend(r) => {
                if m.id == self.req.amend {
                    self.on_amend_message(r);
                }
            }
            Payload::Remotes(v) => self.remotes = v,
            Payload::TagInfo { full, result } => {
                if m.id == self.req.tag_info {
                    self.hist.on_tag_info(full, result);
                }
            }
        }
    }

    fn on_op(&mut self, label: String, result: Result<String, GitError>, after: After) {
        match result {
            Ok(msg) => {
                let first = msg.lines().last().unwrap_or("").to_string();
                if first.is_empty() {
                    self.set_status(format!("{label}: done."), StatusTone::Info);
                } else {
                    self.set_status(format!("{label}: {first}"), StatusTone::Info);
                }
                match after {
                    After::Nothing => {}
                    After::Rescan => {
                        self.generation += 1;
                        self.rescan();
                    }
                    After::Refresh | After::OfferForceDelete(_) => self.refresh_all(None),
                    After::Select(o) => self.refresh_all(Some(o)),
                    After::Committed => {
                        self.on_committed();
                        self.refresh_all(None);
                    }
                    After::Reverted(patch) => {
                        self.wt.last_revert = Some(patch);
                        self.generation += 1;
                        self.rescan();
                    }
                }
            }
            Err(e) => {
                if let After::OfferForceDelete(name) = after
                    && e.message.contains("not fully merged")
                {
                    let shown = crate::domain::sanitize::display_bytes(&name);
                    let body = format!(
                        "The commits on branch {shown} aren't on any other branch.\nReally delete branch {shown}?"
                    );
                    self.confirm(
                        "Confirm",
                        &[(&body, Tone::Warn)],
                        "Delete",
                        Pending::Op {
                            label: format!("Delete branch {shown}"),
                            f: Box::new(move |r| crate::git::ops::delete_branch(r, &name, true)),
                            after: After::Refresh,
                            expect_head: None,
                        },
                    );
                    return;
                }
                self.error(&label, &e);
                // The repository may have changed partially: re-read.
                self.refresh_all(None);
            }
        }
    }

    pub fn on_committed(&mut self) {
        self.wt.message = input::TextArea::new("");
        self.wt.amend = false;
        self.wt.saved_message = None;
        self.wt.message.modified = true;
        self.save_draft();
    }

    pub fn switch_view(&mut self, v: ViewKind) {
        if v == ViewKind::Worktree && self.is_bare() {
            self.set_status(
                "Bare repository: the working tree view is not available.",
                StatusTone::Error,
            );
            return;
        }
        self.view = v;
        self.maximized = None;
        self.menu = None;
    }

    pub fn request_quit(&mut self) {
        if self.workers.writing() {
            let label = self
                .workers
                .write_label
                .lock()
                .map(|l| l.clone())
                .unwrap_or_default();
            let mut d = Dialog::new(
                "Operation in progress",
                Kind::Form(dialog::FormKind::QuitBusy),
            )
            .with_body(&[
                (
                    &format!("A Git operation is still running: {label}"),
                    Tone::Warn,
                ),
                (
                    "It will not be interrupted. Quit anyway, or wait for it to finish?",
                    Tone::Normal,
                ),
            ])
            .with_buttons(&["Wait", "Quit anyway"]);
            d.cancel = Some(0);
            self.dialogs.push(d);
            return;
        }
        if self.cfg.confirm_quit && !self.wt.message.text().trim().is_empty() {
            let mut d = Dialog::new(
                "Quit with a commit message?",
                Kind::Form(dialog::FormKind::QuitWithDraft),
            )
            .with_body(&[("Your message has not been committed.", Tone::Warn)])
            .with_buttons(&["Save draft and quit", "Quit without saving", "Cancel"]);
            d.cancel = Some(2);
            self.dialogs.push(d);
        } else {
            self.finish_quit();
        }
    }

    pub fn finish_quit(&mut self) {
        match self.persist() {
            Ok(()) => self.quit = true,
            Err(e) => self.error("Not quitting: cannot save", e),
        }
    }

    /// Quits without saving the in-progress commit message draft.
    pub fn finish_quit_discard(&mut self) {
        if let Err(e) = crate::config::clear_draft(&self.repo.git_dir) {
            return self.error("Not quitting: cannot clear draft", e);
        }
        match self.try_save_state() {
            Ok(()) => self.quit = true,
            Err(e) => self.error("Not quitting: cannot save", e),
        }
    }

    fn persist(&mut self) -> Result<(), String> {
        if !self.wt.message.text().trim().is_empty() {
            self.wt.message.modified = true;
        }
        let draft = self.try_save_draft();
        let state = self.try_save_state();
        let errors: Vec<_> = [draft, state].into_iter().filter_map(Result::err).collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    pub fn shutdown(&mut self) -> Result<(), String> {
        self.hist.cancel.cancel();
        self.hist.find_cancel.cancel();
        self.persist()
    }

    pub fn dispatch(&mut self, a: Action) {
        self.menu = None;
        if self.dispatch_history(&a) || self.dispatch_worktree(&a) {
            return;
        }
        match a {
            Action::Noop => {}
            Action::Quit => self.request_quit(),
            Action::ForceQuit => self.finish_quit(),
            Action::SwitchView(v) => self.switch_view(v),
            Action::ToggleView => {
                let v = match self.view {
                    ViewKind::History => ViewKind::Worktree,
                    ViewKind::Worktree => ViewKind::History,
                };
                self.switch_view(v);
            }
            Action::ToggleMaximize => self.toggle_maximize(),
            Action::FocusNext => self.focus_cycle(1),
            Action::FocusPrev => self.focus_cycle(-1),
            Action::OpenMenu(i) => self.open_menu(i),
            Action::CloseOverlay => {
                self.menu = None;
            }
            Action::Preferences | Action::WtOptions => self.open_preferences(),
            Action::Copy => self.copy_focused(),
            _ => {}
        }
    }

    fn toggle_maximize(&mut self) {
        if self.maximized.is_some() {
            self.maximized = None;
            return;
        }
        self.maximized = Some(self.focused_pane());
    }

    pub fn focused_pane(&self) -> PaneId {
        match self.view {
            ViewKind::History => match self.hist.focus {
                history::HFocus::List | history::HFocus::Sha | history::HFocus::Find => {
                    PaneId::HList
                }
                history::HFocus::Files => PaneId::HFiles,
                _ => PaneId::HDiff,
            },
            ViewKind::Worktree => match self.wt.focus {
                worktree::WFocus::Unstaged => PaneId::WUnstaged,
                worktree::WFocus::Staged => PaneId::WStaged,
                worktree::WFocus::Diff => PaneId::WDiff,
                worktree::WFocus::Message => PaneId::WMessage,
            },
        }
    }

    fn focus_cycle(&mut self, dir: isize) {
        match self.view {
            ViewKind::History => self.hist.cycle_focus(dir),
            ViewKind::Worktree => self.wt.cycle_focus(dir),
        }
    }

    fn copy_focused(&mut self) {
        let text = match self.view {
            ViewKind::History => match self.hist.focus {
                history::HFocus::Diff | history::HFocus::Search => self.hist.diff.selected_text(),
                history::HFocus::List => self
                    .hist
                    .selected_oid()
                    .map(|o| o.to_hex())
                    .unwrap_or_default(),
                history::HFocus::Files => self
                    .hist
                    .file_path(self.hist.file_sel)
                    .map(|p| crate::domain::sanitize::display_bytes(&p))
                    .unwrap_or_default(),
                history::HFocus::Sha => self.hist.sha.text.clone(),
                history::HFocus::Find => self.hist.find.text.clone(),
            },
            ViewKind::Worktree => match self.wt.focus {
                worktree::WFocus::Message => self.wt.message.selected_text().unwrap_or_default(),
                worktree::WFocus::Diff => self.wt.diff.selected_text(),
                _ => self
                    .wt
                    .cursor_file()
                    .map(|f| f.display.clone())
                    .unwrap_or_default(),
            },
        };
        self.copy_text(text);
    }

    pub fn paste(&mut self, s: &str) {
        if let Some(d) = self.dialogs.last_mut() {
            d.paste(s);
            return;
        }
        match self.view {
            ViewKind::History => match self.hist.focus {
                history::HFocus::Sha => self.hist.sha.insert_str(s),
                history::HFocus::Find => {
                    self.hist.find.insert_str(s);
                    self.find_changed();
                }
                history::HFocus::Search => {
                    self.hist.search.insert_str(s);
                    self.search_changed();
                }
                _ => {}
            },
            ViewKind::Worktree => {
                if self.wt.focus == worktree::WFocus::Message {
                    self.wt.message.insert_str(s);
                    self.wt
                        .message
                        .scroll_to_cursor(self.wt.msg_height, self.wt.msg_width);
                }
            }
        }
    }

    pub fn busy(&self) -> bool {
        self.hist.loading || self.wt.loading || self.workers.busy()
    }
}

/// Minimal base64 for OSC 52.
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_vectors() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}
