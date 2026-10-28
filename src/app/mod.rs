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

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use crate::config::{Config, State, ViewKind};
use crate::domain::oid::Oid;
use crate::domain::refs::RefSet;
use crate::git::read::GuiTool;
use crate::git::repo::{RepoInfo, RepoState};
use crate::git::{GitError, Runner, runner::os};
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
    /// Open a file in a terminal editor (no shell, works in read-only mode).
    OpenFile {
        program: String,
        args: Vec<String>,
        file: PathBuf,
    },
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
    Error,
}

/// Minimum time the bar shows "Working", so quick operations stay readable.
const WORKING_MIN_DISPLAY: Duration = Duration::from_millis(150);

/// Number of messages kept for the message history dialog.
const MAX_LOG: usize = 500;

/// A message shown in the status bar, kept for the history dialog.
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub at: SystemTime,
    pub text: String,
    pub tone: StatusTone,
}

/// Local `HH:MM:SS` for a timestamp.
fn clock(at: SystemTime) -> String {
    let secs = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as libc::time_t;
    // SAFETY: `localtime_r` writes into the zeroed `tm` we pass; `tm` is
    // only read when the call succeeds.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&secs, &mut tm).is_null() {
            return String::from("--:--:--");
        }
        format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
    }
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
    /// Commands run from the palette, most recent first (this session only).
    pub palette_recent: Vec<crate::app::action::Action>,
    /// Last message shown in the bar. An empty text means no message.
    pub status: (String, StatusTone),
    /// Every message shown in the bar, oldest first (bounded by `MAX_LOG`).
    pub log: VecDeque<LogEntry>,
    /// Set by [`App::mark_working`] to the time work was started, so the bar
    /// keeps showing "Working" until the work completes and at least a
    /// minimal duration has passed (avoids a flicker on fast refreshes).
    pub pending_busy_status: Option<Instant>,
    pending_successes: Vec<String>,
    applied: Requests,
    /// True while a diff search runs in the background (not counted by `busy`).
    pub searching: bool,
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
            palette_recent: Vec::new(),
            dialogs: draft_error
                .as_ref()
                .map(|e| Dialog::error("Cannot read draft", e))
                .into_iter()
                .collect(),
            status: (String::new(), StatusTone::Info),
            pending_busy_status: None,
            pending_successes: Vec::new(),
            applied: Requests::default(),
            log: VecDeque::new(),
            searching: false,
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
        self.capture_base_tips();
        self.refresh_all(None);
        if self.read_only {
            self.set_status(
                "Read-only mode: all mutations are disabled.",
                StatusTone::Info,
            );
        }
    }

    /// Sets the message shown in the bar. Info messages stay until replaced;
    /// error messages are also dropped on the next user action.
    pub fn set_status(&mut self, s: impl Into<String>, tone: StatusTone) {
        if tone == StatusTone::Error {
            self.pending_successes.clear();
        }
        let text = s.into();
        if !text.is_empty() {
            if self.log.len() == MAX_LOG {
                self.log.pop_front();
            }
            self.log.push_back(LogEntry {
                at: SystemTime::now(),
                text: text.clone(),
                tone,
            });
        }
        self.status = (text, tone);
    }

    /// Opens a dialog listing the messages shown so far, with their times.
    pub fn show_message_log(&mut self) {
        let mut d = Dialog::new("Messages", Kind::Info);
        d.body = if self.log.is_empty() {
            vec![(String::from("No message yet."), Tone::Normal)]
        } else {
            self.log
                .iter()
                .map(|e| {
                    let tone = match e.tone {
                        StatusTone::Error => Tone::Error,
                        StatusTone::Info => Tone::Normal,
                    };
                    let text = e.text.replace(['\n', '\r'], " ");
                    (format!("{}  {text}", clock(e.at)), tone)
                })
                .collect()
        };
        d.buttons = vec![String::from("Close")];
        d.width = 78;
        // Open at the newest message, at the bottom of the list.
        d.scroll = usize::MAX;
        d.reveal_focus = false;
        d.last_focus = Some(d.focus);
        self.dialogs.push(d);
    }

    pub fn clear_message(&mut self) {
        self.status = (String::new(), StatusTone::Info);
    }

    /// Drops an error message once the user acts again.
    pub fn dismiss_error(&mut self) {
        if self.status.1 == StatusTone::Error {
            self.clear_message();
        }
    }

    /// Marks work as started so the bar shows "Working" right away, even
    /// before a worker is queued. `tick` keeps it visible for at least 500ms.
    pub fn mark_working(&mut self) {
        self.pending_busy_status = Some(Instant::now());
    }

    fn defer_success(&mut self, label: &str, message: String) {
        self.pending_successes.push(message);
        self.set_status(format!("{label}: updating view..."), StatusTone::Info);
        self.mark_working();
    }

    fn view_is_current(&self) -> bool {
        let applied = |requested, completed| requested == 0 || requested == completed;
        !self.workers.writing()
            && applied(self.req.refs, self.applied.refs)
            && match self.view {
                ViewKind::History => {
                    self.hist.view_ready()
                        && self.hist.sel_changed_at.is_none()
                        && applied(self.req.diff, self.applied.diff)
                }
                ViewKind::Worktree => {
                    !self.wt.loading
                        && applied(self.req.status, self.applied.status)
                        && applied(self.req.wtdiff, self.applied.wtdiff)
                }
            }
    }

    fn report_refresh_failure<T>(&mut self, result: &Result<T, GitError>) {
        if !self.pending_successes.is_empty()
            && let Err(error) = result
        {
            self.set_status(format!("View refresh failed: {error}"), StatusTone::Error);
        }
    }

    /// True while the bar should show "Working" instead of "Ready".
    pub fn working(&self) -> bool {
        self.busy() || self.pending_busy_status.is_some() || self.searching
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

    /// Shows file content in the syntax-highlighted, "Open in editor"-capable
    /// viewer (used by the file browser).
    pub fn info_file(&mut self, title: &str, path: &[u8], text: &str) {
        let lang = crate::domain::highlight::lang_for_path(path);
        let code = crate::domain::highlight::highlight(text, lang)
            .into_iter()
            .map(|line| line.into_iter().map(|(s, t)| (s, tone_of(t))).collect())
            .collect();
        self.dialogs
            .push(Dialog::file_view(title, path.to_vec(), code));
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
        self.update_history(keep);
        if !self.is_bare() {
            self.rescan();
        }
        self.repo_state = crate::git::repo::repo_state(&self.repo);
        self.guitools = crate::git::read::guitools(&self.runner);
        self.remotes = crate::git::read::remotes(&self.runner).unwrap_or_default();
    }

    /// Remembers where HEAD is now as the history's base (startup, reload).
    pub fn capture_base_tips(&mut self) {
        let head = self
            .runner
            .read(&[
                os("rev-parse"),
                os("--verify"),
                os("--quiet"),
                os("HEAD^{commit}"),
            ])
            .ok()
            .and_then(|o| Oid::from_hex_bytes(o.trim_ascii()));
        self.hist.base_tips = head.into_iter().collect();
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
        self.set_status(format!("{label}: in progress..."), StatusTone::Info);
        self.mark_working();
        let id = self.workers.write(self.generation, label, move || {
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
        if id == 0 {
            self.set_status(
                format!("{label}: cannot queue operation."),
                StatusTone::Error,
            );
        }
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

    /// Starts the configured editor on a file of the working copy. The editor
    /// runs detached, so the terminal stays in the TUI (GUI editors such as VS Code).
    pub fn open_in_editor(&mut self, path: &[u8]) {
        use std::os::unix::ffi::OsStrExt;
        let mut words = self.cfg.editor.split_whitespace();
        let Some(program) = words.next() else {
            self.set_status(
                "No editor set: choose one in Preferences (External editor).",
                StatusTone::Error,
            );
            return;
        };
        let file = self.repo.cwd().join(std::ffi::OsStr::from_bytes(path));
        if !file.exists() {
            self.set_status(
                format!(
                    "{} is not in the working copy.",
                    crate::domain::sanitize::display_bytes(path)
                ),
                StatusTone::Error,
            );
            return;
        }
        let args: Vec<String> = words.map(str::to_string).collect();
        if is_terminal_editor(program, &args) {
            if self.workers.writing() {
                self.set_status(
                    "Busy: wait for the running operation to finish.",
                    StatusTone::Error,
                );
                return;
            }
            self.handoff = Some(Handoff::OpenFile {
                program: program.to_string(),
                args,
                file,
            });
            return;
        }
        let spawned = std::process::Command::new(program)
            .args(&args)
            .arg(&file)
            .current_dir(self.repo.cwd())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match spawned {
            Ok(mut child) => {
                // Reap the launcher in the background so no zombie is left behind.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                self.clear_message();
            }
            Err(e) => self.error("Open in editor", format!("{program}: {e}")),
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
            (Handoff::Editor, Ok(true)) => {}
            (Handoff::Commit { .. }, Ok(true)) => {
                self.defer_success("Commit", "Commit created.".into());
                self.on_committed();
            }
            (Handoff::Git { label, .. } | Handoff::Shell { label, .. }, Ok(true)) => {
                self.defer_success(label, format!("{label}: finished successfully."));
            }
            (_, Ok(true)) => self.defer_success("Command", "Command finished successfully.".into()),
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
                format!("Copied {n} characters to the internal clipboard. Enable OSC 52 in Preferences to also use the system clipboard."),
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
        if !self.pending_successes.is_empty() && self.view_is_current() {
            for message in std::mem::take(&mut self.pending_successes) {
                self.set_status(message, StatusTone::Info);
            }
        }
        let active = self.busy() || self.searching;
        if active && self.pending_busy_status.is_none() {
            self.pending_busy_status = Some(now);
        }
        if let Some(started) = self.pending_busy_status
            && !active
            && now.duration_since(started) >= WORKING_MIN_DISPLAY
        {
            self.pending_busy_status = None;
        }
    }

    pub fn on_msg(&mut self, m: worker::Msg) {
        let fresh = m.generation == self.generation;
        match m.payload {
            Payload::LogHead(head) => {
                if m.id == self.req.log {
                    self.hist.log_head = head;
                }
            }
            Payload::HistoryUpdate(result) => {
                if m.id == self.req.log {
                    self.on_history_update(result);
                }
            }
            Payload::LogBatch {
                commits,
                graph,
                cuts,
            } => {
                if m.id == self.req.log {
                    self.on_log_batch(commits, graph, cuts);
                }
            }
            Payload::LogDone(r) => {
                if m.id == self.req.log {
                    self.on_log_done(r);
                }
            }
            Payload::Refs(r) => {
                if m.id == self.req.refs {
                    self.applied.refs = m.id;
                }
                if m.id == self.req.refs && fresh {
                    match r {
                        Ok(rs) => {
                            self.refs = rs;
                            self.wt.branch_label = self.refs.head.label();
                            self.refresh_ref_browser();
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
                    self.report_refresh_failure(&r);
                    self.applied.diff = m.id;
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
                    self.report_refresh_failure(&blob);
                    self.on_blob(path, blob);
                }
            }
            Payload::Find(r) => {
                if m.id == self.req.find {
                    self.searching = false;
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
                    self.applied.status = m.id;
                    self.on_status(r);
                }
            }
            Payload::WtDiff(r) => {
                if m.id == self.req.wtdiff {
                    self.report_refresh_failure(&r);
                    self.applied.wtdiff = m.id;
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
            Payload::FileView {
                title,
                path,
                result,
            } => match result {
                Ok(t) => self.info_file(&title, &path, &t),
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
                let success = if first.is_empty() {
                    format!("{label}: completed successfully (no output).")
                } else {
                    format!("{label}: {first}")
                };
                self.defer_success(&label, success);
                match after {
                    After::Nothing => {}
                    After::RefsOnly => self.load_refs(),
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
                    After::RevertUndone(patch) => {
                        if self.wt.last_revert.as_ref() == Some(&patch) {
                            self.wt.last_revert = None;
                        }
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
        if matches!(self.drag, Some(mouse::Drag::HistoryScroll { .. })) {
            self.drag = None;
        }
        if v != ViewKind::History {
            self.dialogs.retain(|d| !d.is_modeless());
        }
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
            Action::CommandPalette => self.open_command_palette(),
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
            Action::OpenInEditor(p) => self.open_in_editor(&p),
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

    pub fn refs_window_open(&self) -> bool {
        self.dialogs.len() == 1 && self.dialogs[0].is_modeless()
    }

    pub fn dialog_has_focus(&self) -> bool {
        self.dialogs
            .last()
            .is_some_and(|d| !d.is_modeless() || !d.background_focus)
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
        if self.dialog_has_focus()
            && let Some(d) = self.dialogs.last_mut()
        {
            let before = d.list_value(0);
            d.paste(s);
            if d.is_modeless() && d.list_value(0) != before {
                self.preview_list_ref();
            }
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
                if let Some(l) = self.wt.filter_focus {
                    self.wt.filter_mut(l).insert_str(s);
                    self.worktree_filter_changed(l);
                } else if self.wt.focus == worktree::WFocus::Message {
                    self.wt.message.insert_str(s);
                    self.wt
                        .message
                        .scroll_to_cursor(self.wt.msg_height, self.wt.msg_width);
                }
            }
        }
    }

    pub fn busy(&self) -> bool {
        self.hist.loading
            || self.wt.loading
            || self.workers.busy()
            || !self.pending_successes.is_empty()
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

/// Terminal editors need the TUI suspended; GUI editors are launched detached.
fn is_terminal_editor(program: &str, args: &[String]) -> bool {
    const TERMINAL: &[&str] = &[
        "vi", "vim", "nvim", "nano", "hx", "helix", "micro", "kak", "joe",
    ];
    let name = std::path::Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    TERMINAL.contains(&name)
        || (name == "emacs" && args.iter().any(|a| a == "-nw" || a == "--no-window-system"))
}

/// Maps a syntax-highlighting token to the dialog tone used to render it.
fn tone_of(t: crate::domain::highlight::Token) -> Tone {
    use crate::domain::highlight::Token;
    match t {
        Token::Plain => Tone::Normal,
        Token::Keyword => Tone::Keyword,
        Token::Str => Tone::Str,
        Token::Comment => Tone::Comment,
        Token::Number => Tone::Number,
        Token::Code => Tone::Code,
    }
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
