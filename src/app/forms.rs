//! Form dialogs (gitk and git gui) and the handling of dialog results.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

use super::action::Action;
use super::dialog::{Dialog, DialogResult, Field, FormKind, Kind, Tone};
use super::worker::{After, Payload};
use super::{App, Handoff, Pending, StatusTone};
use crate::config::{ColorScheme, Handoff as HandoffCfg, IconMode, StageUntracked, ThemeName};
use crate::domain::oid::Oid;
use crate::domain::refs::RefKind;
use crate::domain::sanitize;
use crate::git::ops::{self, ResetMode};
use crate::git::read;
use crate::git::runner::os;

fn form(title: &str, kind: FormKind, fields: Vec<Field>, buttons: &[&str]) -> Dialog {
    let mut d = Dialog::new(title, Kind::Form(kind))
        .with_fields(fields)
        .with_buttons(buttons);
    d.cancel = Some(buttons.len() - 1);
    d
}

impl App {
    fn subject(&self, oid: &Oid) -> String {
        self.hist
            .index
            .get(oid)
            .map(|i| self.hist.commits[*i].subject.to_string())
            .unwrap_or_default()
    }

    fn commit_label(&self, oid: &Oid) -> Field {
        Field::label(
            &format!("{}  {}", oid.short(10), self.subject(oid)),
            Tone::Code,
        )
    }

    /// (display, full name) of refs of the given kinds.
    fn ref_items(&self, kinds: &[RefKind], skip_head: bool) -> (Vec<String>, Vec<Vec<u8>>) {
        let mut items = Vec::new();
        let mut values = Vec::new();
        for r in &self.refs.refs {
            if !kinds.contains(&r.kind) || (skip_head && self.refs.is_head_ref(r)) {
                continue;
            }
            let k = match r.kind {
                RefKind::LocalBranch => "H",
                RefKind::RemoteBranch => "R",
                RefKind::Tag => "T",
                _ => "o",
            };
            items.push(format!("{k} {}", r.short));
            values.push(r.full.clone());
        }
        (items, values)
    }

    // ------------------------------------------------------------------
    // gitk forms

    pub fn open_preferences(&mut self) {
        let c = &self.cfg;
        let fields = vec![
            Field::choice(
                "Theme",
                &["dark", "light", "terminal"],
                match c.theme {
                    ThemeName::Dark => 0,
                    ThemeName::Light => 1,
                    ThemeName::Terminal => 2,
                },
            ),
            Field::dropdown(
                "Color scheme",
                &["default", "gruvbox"],
                match c.color_scheme {
                    ColorScheme::Default => 0,
                    ColorScheme::Gruvbox => 1,
                },
            ),
            Field::choice(
                "Icons",
                &["unicode", "nerd", "ascii"],
                match c.icons {
                    IconMode::Unicode => 0,
                    IconMode::Nerd => 1,
                    IconMode::Ascii => 2,
                },
            ),
            Field::check("Mouse support (applies on restart)", c.mouse),
            Field::check(
                "Show a connector line below each commit's graph node",
                c.graph_connectors,
            ),
            Field::check("Copy to the system clipboard (OSC 52)", c.osc52),
            Field::text("History: lines of context", &c.history_context.to_string()),
            Field::text(
                "Worktree: lines of context",
                &c.worktree_context.to_string(),
            ),
            Field::text("Tab width", &c.tab_width.to_string()),
            Field::text("Commit message width guide", &c.commit_width.to_string()),
            Field::choice(
                "Stage untracked files with Stage Changed",
                &["ask", "yes", "no"],
                match c.stage_untracked {
                    StageUntracked::Ask => 0,
                    StageUntracked::Yes => 1,
                    StageUntracked::No => 2,
                },
            ),
            Field::choice(
                "Commit in the terminal (signing, hooks)",
                &["auto (commit.gpgsign)", "always", "never"],
                match c.commit_handoff {
                    HandoffCfg::Auto => 0,
                    HandoffCfg::Always => 1,
                    HandoffCfg::Never => 2,
                },
            ),
            Field::check("Allow external diff drivers / textconv", c.allow_textconv),
            Field::check("Ask before quitting with a commit message", c.confirm_quit),
            Field::text(
                "History graph: max columns (0 = as many as fit)",
                &c.graph_max_lanes.to_string(),
            ),
            Field::text(
                "History graph: cut lines idle for N rows (0 = never)",
                &c.graph_cut_after.to_string(),
            ),
            Field::text(
                "External editor for Open in editor (empty = off)",
                &c.editor,
            ),
        ];
        let mut d = form(
            "Preferences",
            FormKind::Preferences,
            fields,
            &["OK", "Cancel"],
        );
        d.width = 74;
        self.dialogs.push(d);
    }

    pub fn open_list_refs(&mut self) {
        let kinds = [
            RefKind::LocalBranch,
            RefKind::RemoteBranch,
            RefKind::Tag,
            RefKind::Other,
            RefKind::Stash,
        ];
        let icons = &self.theme.icons;
        let mut items = Vec::new();
        let mut values = Vec::new();
        let mut keys = Vec::new();
        for r in &self.refs.refs {
            if !kinds.contains(&r.kind) {
                continue;
            }
            let shown = if icons.nerd {
                icons.reference(r.kind).to_string()
            } else {
                match r.kind {
                    RefKind::LocalBranch => "H",
                    RefKind::RemoteBranch => "R",
                    RefKind::Tag => "T",
                    _ => "o",
                }
                .to_string()
            };
            items.push(format!("{shown} {}", r.short));
            values.push(r.full.clone());
            keys.push(r.short.clone());
        }
        let title = format!("Tags and heads: {}", self.repo.display_name());
        let legend = if icons.nerd {
            "type to filter, * wildcard"
        } else {
            "H branch  R remote  T tag  o other   (type to filter, * wildcard)"
        };
        let fields = vec![Field::filter_list(legend, items, values, keys, 14)];
        self.dialogs.push(form(
            &title,
            FormKind::ListRefs,
            fields,
            &["Select", "Close"],
        ));
    }

    /// Shows the reference highlighted in the F2 list in the history view,
    /// leaving the list open.
    pub fn preview_list_ref(&mut self) {
        let Some(full) = self.dialogs.last().and_then(|d| d.list_value(0)) else {
            return;
        };
        if let Some(o) = self.refs.find_full(&full).map(|r| r.target) {
            self.select_oid(o, false);
        }
    }

    pub fn open_view_editor(&mut self, idx: Option<usize>) {
        let v = match idx {
            Some(i) => self.hist.views.get(i).cloned().unwrap_or_default(),
            None => crate::git::log::ViewSpec {
                name: format!("View {}", self.hist.views.len()),
                ..Default::default()
            },
        };
        let fields = vec![
            Field::text("View Name", &v.name),
            Field::check("Remember this view", v.remember),
            Field::label("References (space separated list):", Tone::Strong),
            Field::text("Branches & tags", &v.refs),
            Field::check("All refs", v.all_refs),
            Field::check("All (local) branches", v.branches),
            Field::check("All tags", v.tags),
            Field::check("All remote-tracking branches", v.remotes),
            Field::label("Commit Info (regular expressions):", Tone::Strong),
            Field::text("Author", &v.author),
            Field::text("Committer", &v.committer),
            Field::text("Commit Message", &v.message),
            Field::check("Matches all Commit Info criteria", v.all_match),
            Field::check("Matches no Commit Info criteria", v.match_none),
            Field::label("Changes to Files:", Tone::Strong),
            Field::check("Regular Expression (else fixed string)", v.changes_regex),
            Field::text("Search string", &v.changes),
            Field::label(
                "Commit Dates (\"2 weeks ago\", \"2009-03-17 15:27:38\", \"March 17, 2009 15:27:38\"):",
                Tone::Strong,
            ),
            Field::text("Since", &v.since),
            Field::text("Until", &v.until),
            Field::label(
                "Limit and/or skip a number of revisions (positive integer):",
                Tone::Strong,
            ),
            Field::text("Number to show", &v.max_count),
            Field::text("Number to skip", &v.skip),
            Field::label("Miscellaneous options:", Tone::Strong),
            Field::check("Strictly sort by date", v.date_order),
            Field::check("Mark branch sides", v.left_right),
            Field::check("Limit to first parent", v.first_parent),
            Field::check("Simple history", v.simplify),
            Field::text("Additional arguments to git log", &v.extra_args),
            Field::area(
                "Enter files and directories to include, one per line:",
                &v.paths,
                4,
            ),
        ];
        let title = match idx {
            Some(_) => format!("Gitk view definition: {}", v.name),
            None => "Gitk view definition".to_string(),
        };
        let mut d = form(
            &title,
            FormKind::ViewEdit(idx),
            fields,
            &["OK", "Apply (F5)", "Cancel"],
        );
        d.width = 80;
        self.dialogs.push(d);
    }

    pub fn open_make_patch(&mut self, from: Oid, to: Oid) {
        let n = 1 + self.hist.commits.len() % 1000;
        let fields = vec![
            Field::label("From:", Tone::Strong),
            self.commit_label(&from),
            Field::label("To:", Tone::Strong),
            self.commit_label(&to),
            Field::check("Reverse", false),
            Field::text("Output file", &format!("patch{n}.patch")),
        ];
        self.dialogs.push(form(
            "Generate patch",
            FormKind::MakePatch(from, to),
            fields,
            &["Generate", "Cancel"],
        ));
    }

    pub fn open_create_tag(&mut self, oid: Oid) {
        if !self.guard_write() {
            return;
        }
        let fields = vec![
            self.commit_label(&oid),
            Field::text("Tag name", ""),
            Field::text("Message (optional)", ""),
            Field::label(
                "Empty message: lightweight tag. With a message: annotated tag.",
                Tone::Normal,
            ),
        ];
        self.dialogs.push(form(
            "Create tag",
            FormKind::CreateTag(oid),
            fields,
            &["Create", "Cancel"],
        ));
    }

    pub fn open_write_commit(&mut self, oid: Oid) {
        let fields = vec![
            self.commit_label(&oid),
            Field::label(
                "Format: git diff-tree --stdin -p --pretty=email",
                Tone::Normal,
            ),
            Field::text("Output file", &format!("commit-{}", oid.short(7))),
        ];
        self.dialogs.push(form(
            "Write commit to file",
            FormKind::WriteCommit(oid),
            fields,
            &["Write", "Cancel"],
        ));
    }

    pub fn open_create_branch_at(&mut self, oid: Oid) {
        if !self.guard_write() {
            return;
        }
        let fields = vec![self.commit_label(&oid), Field::text("Name", "")];
        self.dialogs.push(form(
            "Create branch",
            FormKind::CreateBranchAt(oid),
            fields,
            &["Create", "Cancel"],
        ));
    }

    pub fn open_reset(&mut self, oid: Oid) {
        let branch = self.refs.head.label();
        let fields = vec![
            Field::label(
                &format!("Reset branch {branch} to {}?", oid.short(10)),
                Tone::Strong,
            ),
            self.commit_label(&oid),
            Field::radio(
                "Reset type",
                &[
                    "Soft: move the branch only; index and files are kept",
                    "Mixed: move the branch and unstage; files are kept",
                    "Hard: move the branch, index and files (discards local changes)",
                ],
                1,
            ),
        ];
        let mut d = form(
            "Confirm reset",
            FormKind::ResetHere(oid),
            fields,
            &["OK", "Cancel"],
        );
        d.width = 80;
        self.dialogs.push(d);
    }

    pub fn open_rename_branch(&mut self, full: Vec<u8>) {
        let Some(r) = self.refs.find_full(&full).cloned() else {
            return;
        };
        if r.kind != RefKind::LocalBranch {
            self.set_status("Only local branches can be renamed.", StatusTone::Error);
            return;
        }
        let fields = vec![
            Field::label(&format!("Current name: {}", r.short), Tone::Strong),
            Field::text("New name", &r.short),
        ];
        self.dialogs.push(form(
            "Rename branch",
            FormKind::RenameBranch(full),
            fields,
            &["Rename", "Cancel"],
        ));
    }

    pub fn open_cherry_pick_merge(&mut self, oid: Oid, nparents: usize) {
        let opts: Vec<String> = (1..=nparents).map(|i| format!("Parent {i}")).collect();
        let refs: Vec<&str> = opts.iter().map(String::as_str).collect();
        let fields = vec![
            self.commit_label(&oid),
            Field::label(
                "This is a merge commit: choose the mainline parent.",
                Tone::Warn,
            ),
            Field::choice("Mainline", &refs, 0),
        ];
        self.dialogs.push(form(
            "Cherry-pick merge",
            FormKind::CherryPickMerge(oid),
            fields,
            &["Cherry-pick", "Cancel"],
        ));
    }

    // ------------------------------------------------------------------
    // git gui forms

    pub fn open_gui_create_branch(&mut self) {
        if !self.guard_write() {
            return;
        }
        let start = match &self.refs.head {
            crate::domain::refs::Head::Branch {
                short,
                oid: Some(_),
                ..
            } => short.clone(),
            crate::domain::refs::Head::Detached(o) => o.to_hex(),
            _ => "HEAD".into(),
        };
        let fields = vec![
            Field::text("Name", ""),
            Field::text("Starting Revision", &start),
            Field::check("Update existing branch (reset)", false),
            Field::check("Checkout After Creation", true),
        ];
        self.dialogs.push(form(
            "Create Branch",
            FormKind::GuiCreateBranch,
            fields,
            &["Create", "Cancel"],
        ));
    }

    pub fn open_gui_checkout(&mut self) {
        if !self.guard_write() {
            return;
        }
        let (items, values) = self.ref_items(
            &[RefKind::LocalBranch, RefKind::RemoteBranch, RefKind::Tag],
            true,
        );
        let fields = vec![
            Field::list("Revision", items, values, 0, 10),
            Field::check("Detach From Local Branch", false),
        ];
        self.dialogs.push(form(
            "Checkout Branch",
            FormKind::GuiCheckout,
            fields,
            &["Checkout", "Cancel"],
        ));
    }

    pub fn open_gui_rename(&mut self) {
        if !self.guard_write() {
            return;
        }
        let (items, values) = self.ref_items(&[RefKind::LocalBranch], false);
        let cur = self
            .refs
            .head
            .branch_full()
            .and_then(|f| values.iter().position(|v| v == f))
            .unwrap_or(0);
        let fields = vec![
            Field::list("Branch", items, values, cur, 8),
            Field::text("New Name", ""),
        ];
        self.dialogs.push(form(
            "Rename Branch",
            FormKind::GuiRename,
            fields,
            &["Rename", "Cancel"],
        ));
    }

    pub fn open_gui_delete(&mut self) {
        if !self.guard_write() {
            return;
        }
        let (items, values) = self.ref_items(&[RefKind::LocalBranch], true);
        if items.is_empty() {
            self.set_status("No other local branch to delete.", StatusTone::Info);
            return;
        }
        let fields = vec![
            Field::list("Local Branches", items, values, 0, 10),
            Field::check("Always (Do not perform merge checks)", false),
        ];
        self.dialogs.push(form(
            "Delete Local Branch",
            FormKind::GuiDelete,
            fields,
            &["Delete", "Cancel"],
        ));
    }

    pub fn open_gui_merge(&mut self) {
        if !self.guard_write() {
            return;
        }
        if self.wt.amend {
            self.error("Merge", "Cannot merge while amending.\n\nYou must finish amending this commit before starting any type of merge.");
            return;
        }
        if !self.repo_state.operations.is_empty() {
            self.error("Merge", "You are in the middle of a conflicted merge or other operation.\n\nResolve or abort it first.");
            return;
        }
        if !self.wt.staged.is_empty() || self.wt.unstaged.iter().any(|f| !f.untracked) {
            self.error(
                "Merge",
                "You are in the middle of a change.\n\nYou should complete the current commit before starting a merge. Doing so will help you abort a failed merge, should the need arise.",
            );
            return;
        }
        let (items, values) = self.ref_items(
            &[RefKind::LocalBranch, RefKind::RemoteBranch, RefKind::Tag],
            true,
        );
        let title = format!("Merge Into {}", self.refs.head.label());
        let fields = vec![Field::list("Revision To Merge", items, values, 0, 10)];
        self.dialogs.push(form(
            &title,
            FormKind::GuiMerge,
            fields,
            &["Merge", "Cancel"],
        ));
    }

    pub fn open_push(&mut self) {
        if !self.guard_write() {
            return;
        }
        if self.remotes.is_empty() {
            self.error("Push", "No remote configured. Use Remote > Add... first.");
            return;
        }
        let (items, values) = self.ref_items(&[RefKind::LocalBranch], false);
        let cur = self
            .refs
            .head
            .branch_full()
            .and_then(|f| values.iter().position(|v| v == f))
            .unwrap_or(0);
        let remotes: Vec<&str> = self.remotes.iter().map(String::as_str).collect();
        let fields = vec![
            Field::list("Source Branch", items, values, cur, 8),
            Field::choice("Destination Remote", &remotes, 0),
            Field::check(
                "Force overwrite existing branch (may discard changes)",
                false,
            ),
            Field::check("Use thin pack (for slow network connections)", false),
            Field::check("Include tags", false),
        ];
        self.dialogs.push(form(
            "Push Branches",
            FormKind::Push,
            fields,
            &["Push", "Cancel"],
        ));
    }

    pub fn open_add_remote(&mut self) {
        if !self.guard_write() {
            return;
        }
        let fields = vec![Field::text("Name", ""), Field::text("Location", "")];
        self.dialogs.push(form(
            "Add Remote",
            FormKind::AddRemote,
            fields,
            &["Add", "Cancel"],
        ));
    }

    pub fn open_delete_remote_branch(&mut self) {
        if !self.guard_write() {
            return;
        }
        let (items, values) = self.ref_items(&[RefKind::RemoteBranch], false);
        if items.is_empty() {
            self.set_status("No remote-tracking branch.", StatusTone::Info);
            return;
        }
        let fields = vec![Field::list("Branch (remote/name)", items, values, 0, 10)];
        self.dialogs.push(form(
            "Delete Branch Remotely",
            FormKind::DeleteRemoteBranch,
            fields,
            &["Delete", "Cancel"],
        ));
    }

    pub fn open_browse_branch(&mut self) {
        let (items, values) = self.ref_items(
            &[RefKind::LocalBranch, RefKind::RemoteBranch, RefKind::Tag],
            false,
        );
        let fields = vec![Field::list("Revision", items, values, 0, 10)];
        self.dialogs.push(form(
            "Browse Branch Files",
            FormKind::BrowseBranch,
            fields,
            &["Browse", "Cancel"],
        ));
    }

    pub fn open_browser(&mut self, rev: Option<String>) {
        let rev = rev.unwrap_or_else(|| "HEAD".into());
        let oid = match read::resolve_commit(&self.runner, &rev) {
            Ok(o) => o,
            Err(e) => return self.error("Browse files", e),
        };
        match read::tree_paths(&self.runner, &oid) {
            Ok(paths) => {
                let items = paths.iter().map(|p| sanitize::display_bytes(p)).collect();
                let fields = vec![Field::list(&format!("Files of {rev}"), items, paths, 0, 16)];
                let mut d = form(
                    &format!("{rev}: File Browser"),
                    FormKind::Browser(oid),
                    fields,
                    &["Open", "Close"],
                );
                d.width = 80;
                self.dialogs.push(d);
            }
            Err(e) => self.error("Browse files", e),
        }
    }

    // ------------------------------------------------------------------
    // Results

    pub fn on_dialog_result(&mut self, res: DialogResult) {
        let b = match res {
            DialogResult::None => return,
            DialogResult::Close => {
                self.dialogs.pop();
                return;
            }
            DialogResult::Button(b) => b,
        };
        let Some(d) = self.dialogs.pop() else { return };
        match d.kind {
            Kind::Info => {}
            Kind::FileView(ref path) => {
                if b == 0 {
                    let path = path.clone();
                    self.dialogs.push(d);
                    self.open_in_editor(&path);
                }
            }
            Kind::Confirm(p) => {
                if b == 0 {
                    self.run_pending(*p);
                }
            }
            Kind::Form(ref fk) => {
                let fk = fk.clone();
                self.submit_form(fk, d, b);
            }
        }
    }

    /// Puts a form back on screen with an error message in the status bar.
    fn reject(&mut self, d: Dialog, msg: &str) {
        self.set_status(msg.to_string(), StatusTone::Error);
        self.dialogs.push(d);
    }

    fn submit_form(&mut self, fk: FormKind, d: Dialog, b: usize) {
        let cancel = d.buttons.len().saturating_sub(1);
        if b == cancel && !matches!(fk, FormKind::QuitBusy | FormKind::UntrackedStage(_)) {
            return;
        }
        match fk {
            FormKind::QuitBusy => {
                if b == 1 {
                    self.finish_quit();
                }
            }
            FormKind::QuitWithDraft => match b {
                0 => self.finish_quit(),
                1 => self.finish_quit_discard(),
                _ => {}
            },
            FormKind::UntrackedStage(_) => match b {
                0 => self.stage_changed(Some(true)),
                1 => self.stage_changed(Some(false)),
                _ => {}
            },
            FormKind::Preferences => self.submit_preferences(d),
            FormKind::ListRefs => {
                if let Some(full) = d.list_value(0)
                    && let Some(r) = self.refs.find_full(&full)
                {
                    let o = r.target;
                    self.select_oid(o, true);
                }
            }
            FormKind::ViewEdit(idx) => self.submit_view(d, idx, b == 1),
            FormKind::MakePatch(from, to) => {
                let file = d.text(5).trim().to_string();
                if file.is_empty() {
                    return self.reject(d, "Output file name is empty.");
                }
                let (a, z) = if d.checked(4) { (to, from) } else { (from, to) };
                let path = self.repo.cwd().join(&file);
                match read::patch_between(&self.runner, &a, &z)
                    .map_err(|e| e.to_string())
                    .and_then(|data| ops::write_new_file(&path, &data, false))
                {
                    Ok(()) => self.set_status(format!("Patch written to {file}"), StatusTone::Info),
                    Err(e) => self.reject(d, &format!("Error creating patch: {e}")),
                }
            }
            FormKind::WriteCommit(oid) => {
                let file = d.text(2).trim().to_string();
                if file.is_empty() {
                    return self.reject(d, "Output file name is empty.");
                }
                let path = self.repo.cwd().join(&file);
                match read::show_commit_text(&self.runner, &oid)
                    .map_err(|e| e.to_string())
                    .and_then(|data| ops::write_new_file(&path, &data, false))
                {
                    Ok(()) => {
                        self.set_status(format!("Commit written to {file}"), StatusTone::Info)
                    }
                    Err(e) => self.reject(d, &format!("Error writing commit: {e}")),
                }
            }
            FormKind::CreateTag(oid) => {
                let name = d.text(1).trim().to_string();
                let msg = d.text(2);
                if name.is_empty() {
                    return self.reject(d, "No tag name specified");
                }
                if self
                    .refs
                    .refs
                    .iter()
                    .any(|r| r.kind == RefKind::Tag && r.short == name)
                {
                    return self.reject(d, &format!("Tag \"{name}\" already exists"));
                }
                self.run_op(
                    &format!("Create tag {name}"),
                    Box::new(move |r| ops::create_tag(r, &name, &oid, &msg)),
                    After::Refresh,
                    None,
                );
            }
            FormKind::CreateBranchAt(oid) => {
                let name = d.text(1).trim().to_string();
                if name.is_empty() {
                    return self.reject(d, "Please specify a name for the new branch");
                }
                if self.refs.local_branches().any(|r| r.short == name) {
                    let n2 = name.clone();
                    self.confirm(
                        "Create branch",
                        &[(
                            &format!("Branch '{name}' already exists. Overwrite?"),
                            Tone::Warn,
                        )],
                        "Overwrite",
                        Pending::Op {
                            label: format!("Create branch {name}"),
                            f: Box::new(move |r| {
                                ops::create_branch(r, &n2, &oid.to_hex(), true, false)
                            }),
                            after: After::Refresh,
                            expect_head: None,
                        },
                    );
                    return;
                }
                self.run_op(
                    &format!("Create branch {name}"),
                    Box::new(move |r| ops::create_branch(r, &name, &oid.to_hex(), false, false)),
                    After::Refresh,
                    None,
                );
            }
            FormKind::ResetHere(oid) => {
                let mode = match d.chosen(2) {
                    0 => ResetMode::Soft,
                    1 => ResetMode::Mixed,
                    _ => ResetMode::Hard,
                };
                self.dispatch(Action::ResetTo(oid, mode));
            }
            FormKind::RenameBranch(full) => {
                let new = d.text(1).trim().to_string();
                let Some(r) = self.refs.find_full(&full).cloned() else {
                    return;
                };
                if new.is_empty() || new == r.short {
                    return self.reject(d, "Please specify a new name for the branch");
                }
                let old = r.short_bytes().to_vec();
                self.run_op(
                    &format!("Rename branch {}", r.short),
                    Box::new(move |rr| ops::rename_branch(rr, &old, &new, false)),
                    After::Refresh,
                    None,
                );
            }
            FormKind::CherryPickMerge(oid) => {
                let m = d.chosen(2) + 1;
                let head = self.head();
                self.run_op(
                    "Cherry-pick",
                    Box::new(move |r| ops::cherry_pick(r, &oid, Some(m))),
                    After::Refresh,
                    Some(head),
                );
            }
            FormKind::GuiCreateBranch => {
                let name = d.text(0).trim().to_string();
                let start = d.text(1).trim().to_string();
                let (force, checkout) = (d.checked(2), d.checked(3));
                if name.is_empty() {
                    return self.reject(d, "Please supply a branch name.");
                }
                if start.is_empty() {
                    return self.reject(d, "Please select a starting revision.");
                }
                if !force && self.refs.local_branches().any(|r| r.short == name) {
                    return self.reject(d, &format!("Branch '{name}' already exists."));
                }
                self.run_op(
                    &format!("Create branch {name}"),
                    Box::new(move |r| ops::create_branch(r, &name, &start, force, checkout)),
                    After::Refresh,
                    None,
                );
            }
            FormKind::GuiCheckout => {
                let Some(full) = d.list_value(0) else { return };
                let detach = d.checked(1);
                let Some(r) = self.refs.find_full(&full).cloned() else {
                    return;
                };
                if detach || r.kind == RefKind::Tag {
                    let t = r.target;
                    let head = self.head();
                    self.run_op(
                        &format!("Checkout {}", r.short),
                        Box::new(move |rr| ops::checkout_detach(rr, &t)),
                        After::RefsOnly,
                        Some(head),
                    );
                } else {
                    self.dispatch(Action::CheckoutRef(full));
                }
            }
            FormKind::GuiRename => {
                let Some(full) = d.list_value(0) else { return };
                let new = d.text(1).trim().to_string();
                if new.is_empty() {
                    return self.reject(d, "Please supply a branch name.");
                }
                if self.refs.local_branches().any(|r| r.short == new) {
                    return self.reject(d, &format!("Branch '{new}' already exists."));
                }
                let Some(r) = self.refs.find_full(&full).cloned() else {
                    return;
                };
                let old = r.short_bytes().to_vec();
                self.run_op(
                    &format!("Rename branch {}", r.short),
                    Box::new(move |rr| ops::rename_branch(rr, &old, &new, false)),
                    After::Refresh,
                    None,
                );
            }
            FormKind::GuiDelete => {
                let Some(full) = d.list_value(0) else { return };
                let force = d.checked(1);
                let Some(r) = self.refs.find_full(&full).cloned() else {
                    return;
                };
                let name = r.short_bytes().to_vec();
                let n2 = name.clone();
                let body = if force {
                    "Recovering deleted branches is difficult.\n\nDelete the selected branches?"
                } else {
                    "Delete the selected branch?"
                };
                self.confirm(
                    "Delete Branch",
                    &[(&format!("{}\n\n{body}", r.short), Tone::Warn)],
                    "Delete",
                    Pending::Op {
                        label: format!("Delete branch {}", r.short),
                        f: Box::new(move |rr| ops::delete_branch(rr, &n2, force)),
                        after: if force {
                            After::Refresh
                        } else {
                            After::OfferForceDelete(name)
                        },
                        expect_head: None,
                    },
                );
            }
            FormKind::GuiMerge => {
                let Some(full) = d.list_value(0) else { return };
                let Some(r) = self.refs.find_full(&full).cloned() else {
                    return;
                };
                let rev = String::from_utf8_lossy(&r.full).into_owned();
                let head = self.head();
                self.run_op(
                    &format!("Merge {}", r.short),
                    Box::new(move |rr| ops::merge(rr, &rev)),
                    After::Refresh,
                    Some(head),
                );
            }
            FormKind::Push => {
                let Some(full) = d.list_value(0) else { return };
                let Some(remote) = self.remotes.get(d.chosen(1)).cloned() else {
                    return;
                };
                let mut args = vec![os("push"), os("-v")];
                if d.checked(3) {
                    args.push(os("--thin"));
                }
                if d.checked(2) {
                    args.push(os("--force"));
                }
                if d.checked(4) {
                    args.push(os("--tags"));
                }
                args.push(os(&remote));
                let mut spec = full.clone();
                spec.push(b':');
                spec.extend_from_slice(&full);
                args.push(OsString::from_vec(spec));
                self.request_handoff(Handoff::Git {
                    args,
                    label: format!("Push to {remote}"),
                });
            }
            FormKind::AddRemote => {
                let name = d.text(0).trim().to_string();
                let url = d.text(1).trim().to_string();
                if name.is_empty() {
                    return self.reject(d, "Please supply a remote name.");
                }
                if url.is_empty() {
                    return self.reject(d, "Please supply a location.");
                }
                self.run_op(
                    &format!("Add remote {name}"),
                    Box::new(move |r| ops::remote_add(r, &name, &url)),
                    After::Refresh,
                    None,
                );
            }
            FormKind::DeleteRemoteBranch => {
                let Some(full) = d.list_value(0) else { return };
                let Some(r) = self.refs.find_full(&full).cloned() else {
                    return;
                };
                let Some((remote, branch)) = r.short.split_once('/') else {
                    return;
                };
                let (remote, branch) = (remote.to_string(), branch.to_string());
                self.confirm(
                    "Delete Branch Remotely",
                    &[
                        (
                            &format!("Delete branch {branch} on remote {remote}?"),
                            Tone::Warn,
                        ),
                        ("Recovering deleted branches is difficult.", Tone::Normal),
                    ],
                    "Delete",
                    Pending::Handoff(Handoff::Git {
                        args: vec![
                            os("push"),
                            os(&remote),
                            os("--delete"),
                            os(format!("refs/heads/{branch}")),
                        ],
                        label: format!("Delete {branch} on {remote}"),
                    }),
                );
            }
            FormKind::BrowseBranch => {
                if let Some(full) = d.list_value(0) {
                    let rev = String::from_utf8_lossy(&full).into_owned();
                    self.open_browser(Some(rev));
                }
            }
            FormKind::Browser(oid) => {
                let Some(path) = d.list_value(0) else { return };
                self.dialogs.push(d);
                let r = self.runner.clone();
                let tab = self.hist.tab;
                let title = sanitize::display_bytes(&path);
                let view_path = path.clone();
                self.workers.read(self.generation, move || Payload::FileView {
                    title,
                    path: view_path,
                    result: read::blob(&r, &oid, &path).map(|b| {
                        if b.is_binary() {
                            "Binary file, not shown.".to_string()
                        } else {
                            sanitize::display_lines_tabs(&b.data, tab)
                        }
                    }),
                });
            }
        }
    }

    fn submit_preferences(&mut self, d: Dialog) {
        let num = |i: usize| d.text(i).trim().parse::<u64>().ok();
        let mut c = self.cfg.clone();
        c.theme = match d.chosen(0) {
            1 => ThemeName::Light,
            2 => ThemeName::Terminal,
            _ => ThemeName::Dark,
        };
        c.color_scheme = match d.chosen(1) {
            1 => ColorScheme::Gruvbox,
            _ => ColorScheme::Default,
        };
        c.icons = match d.chosen(2) {
            1 => IconMode::Nerd,
            2 => IconMode::Ascii,
            _ => IconMode::Unicode,
        };
        c.mouse = d.checked(3);
        c.graph_connectors = d.checked(4);
        c.osc52 = d.checked(5);
        let (Some(hc), Some(wc), Some(tw), Some(cw)) = (num(6), num(7), num(8), num(9)) else {
            return self.reject(d, "Numeric fields must contain positive integers.");
        };
        c.history_context = hc as u32;
        c.worktree_context = wc as u32;
        c.tab_width = tw.min(16) as u8;
        c.commit_width = cw.min(400) as u16;
        c.stage_untracked = match d.chosen(10) {
            1 => StageUntracked::Yes,
            2 => StageUntracked::No,
            _ => StageUntracked::Ask,
        };
        c.commit_handoff = match d.chosen(11) {
            1 => HandoffCfg::Always,
            2 => HandoffCfg::Never,
            _ => HandoffCfg::Auto,
        };
        c.allow_textconv = d.checked(12);
        c.confirm_quit = d.checked(13);
        let (Some(ml), Some(ca)) = (num(14), num(15)) else {
            return self.reject(d, "Numeric fields must contain positive integers.");
        };
        c.graph_max_lanes = ml.min(500) as u16;
        c.graph_cut_after = ca.min(1_000_000) as u32;
        c.editor = d.text(16).trim().to_string();
        let c = c.clamped();
        let ctx_changed = c.history_context != self.cfg.history_context;
        let wctx_changed = c.worktree_context != self.cfg.worktree_context;
        let cut_changed = c.graph_cut_after != self.cfg.graph_cut_after;
        self.cfg = c;
        self.mouse_enabled = self.cfg.mouse;
        self.theme = crate::ui::theme::Theme::new(&self.cfg);
        self.runner.allow_ext_diff = self.cfg.allow_textconv;
        self.hist.tab = self.cfg.tab_width as usize;
        self.wt.tab = self.cfg.tab_width as usize;
        if ctx_changed {
            self.hist.context = self.cfg.history_context;
        }
        if wctx_changed {
            self.wt.context = self.cfg.worktree_context;
        }
        match self.cfg.save() {
            Ok(()) => self.set_status("Preferences saved.", StatusTone::Info),
            Err(e) => self.error("Preferences applied but not saved", e),
        }
        self.hist.rebuild_diff();
        if cut_changed {
            // The graph layout is computed while loading.
            self.load_history(None);
        }
    }

    fn submit_view(&mut self, d: Dialog, idx: Option<usize>, apply_only: bool) {
        let v = crate::git::log::ViewSpec {
            name: d.text(0).trim().to_string(),
            remember: d.checked(1),
            refs: d.text(3),
            all_refs: d.checked(4),
            branches: d.checked(5),
            tags: d.checked(6),
            remotes: d.checked(7),
            author: d.text(9),
            committer: d.text(10),
            message: d.text(11),
            all_match: d.checked(12),
            match_none: d.checked(13),
            changes_regex: d.checked(15),
            changes: d.text(16),
            since: d.text(18),
            until: d.text(19),
            max_count: d.text(21),
            skip: d.text(22),
            date_order: d.checked(24),
            left_right: d.checked(25),
            first_parent: d.checked(26),
            simplify: d.checked(27),
            extra_args: d.text(28),
            paths: d.text(29),
        };
        if let Err(e) = v.log_args() {
            return self.reject(d, &format!("Error in view definition: {e}"));
        }
        let i = match idx {
            Some(i) if i > 0 && i < self.hist.views.len() => {
                self.hist.views[i] = v;
                i
            }
            _ => {
                self.hist.views.push(v);
                self.hist.views.len() - 1
            }
        };
        if apply_only {
            let mut d2 = d;
            d2.kind = Kind::Form(FormKind::ViewEdit(Some(i)));
            self.dialogs.push(d2);
        }
        self.hist.cur_view = i;
        self.save_state();
        let keep = self.hist.selected_oid();
        self.hist.highlight = None;
        self.load_history(keep);
    }
}
