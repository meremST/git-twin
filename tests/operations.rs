use std::{
    ffi::OsStr,
    fs,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::Path,
    process::{Command, Output},
    time::{Duration, Instant},
};

use git_twin::{
    app::{
        App, Handoff, Options, StatusTone,
        action::{Action, FileList},
        dialog::{DialogResult, Kind},
        diffview::DKind,
    },
    config::{self, Config, IconMode, State, ViewKind},
    git::{ops, repo, runner::DEFAULT_LIMIT},
};

// A child process isolates Git configuration and draft persistence without
// changing process-global environment variables in parallel Rust tests.
fn isolated(name: &str, scenario: impl FnOnce()) {
    if std::env::var("GITT_OPERATION_TEST").as_deref() == Ok(name) {
        scenario();
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("GITT_OPERATION_TEST", name)
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{name}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(dir: &Path, args: &[&OsStr]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let args: Vec<_> = args.iter().map(OsStr::new).collect();
    let output = git_output(dir, &args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.name", "Regression"],
        vec!["config", "user.email", "regression@example.invalid"],
        vec!["config", "commit.gpgsign", "false"],
        vec!["config", "core.hooksPath", "/dev/null"],
        vec!["config", "core.autocrlf", "false"],
    ] {
        git(dir.path(), &args);
    }
    fs::write(dir.path().join("file.txt"), base_text()).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "Base"]);
    dir
}

fn base_text() -> String {
    (0..30).map(|i| format!("line {i:02}\n")).collect()
}

fn changed_text() -> String {
    base_text()
        .replace("line 02\n", "first change\n")
        .replace("line 25\n", "last change\n")
}

fn settle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut idle = Instant::now();
    loop {
        while let Ok(msg) = app.workers.rx.try_recv() {
            app.on_msg(msg);
            idle = Instant::now();
        }
        app.tick();
        if app.busy() {
            idle = Instant::now();
        }
        if idle.elapsed() >= Duration::from_millis(100) {
            return;
        }
        assert!(Instant::now() < deadline, "timeout: {}", app.status.0);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn start(dir: &Path) -> App {
    let mut app = App::new(
        repo::discover(dir).unwrap(),
        Config {
            icons: IconMode::Ascii,
            ..Config::default()
        },
        State::default(),
        Options {
            view: Some(ViewKind::Worktree),
            read_only: false,
            mouse: Some(false),
            all_refs: false,
            paths: Vec::new(),
        },
    );
    app.start();
    settle(&mut app);
    app
}

fn select(app: &mut App, side: FileList, path: &[u8]) {
    let index = app
        .wt
        .list(side)
        .iter()
        .position(|f| f.path == path)
        .unwrap();
    app.dispatch(Action::SelectFile(side, index));
    settle(app);
}

fn hunk(app: &App, index: usize) -> usize {
    app.wt
        .diff
        .lines
        .iter()
        .position(|l| l.kind == DKind::Hunk && l.hunk == Some(index))
        .unwrap()
}

fn line(app: &App, text: &str) -> usize {
    app.wt
        .diff
        .lines
        .iter()
        .position(|l| l.text == text)
        .unwrap()
}

fn confirm(app: &mut App) {
    assert!(matches!(app.dialogs.last().unwrap().kind, Kind::Confirm(_)));
    app.on_dialog_result(DialogResult::Button(0));
    settle(app);
}

fn assert_error(app: &App, text: &str) {
    assert_eq!(app.status.1, StatusTone::Error, "{}", app.status.0);
    let body: String = app
        .dialogs
        .iter()
        .flat_map(|d| d.body.iter().map(|(s, _)| s.as_str()))
        .collect();
    assert!(
        app.status.0.contains(text) || body.contains(text),
        "missing {text:?}: {}\n{body}",
        app.status.0
    );
}

fn script(path: &Path, contents: &str) {
    fs::write(path, format!("#!/bin/sh\n{contents}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn finish_handoff(app: &mut App) -> bool {
    let h = app.handoff.take().expect("expected terminal handoff");
    let result = match &h {
        Handoff::Git { args, .. } => {
            ops::handoff(&app.runner, args, None).map_err(|e| e.to_string())
        }
        Handoff::Commit { message, amend } => ops::commit(
            &app.runner,
            &ops::CommitRequest {
                message,
                amend: *amend,
                handoff: true,
            },
        )
        .map(|_| true)
        .map_err(|e| e.to_string()),
        _ => panic!("unexpected handoff"),
    };
    let success = matches!(result, Ok(true));
    app.after_handoff(&h, result);
    settle(app);
    success
}

#[test]
fn partial_hunks_and_lines_preserve_unselected_changes() {
    isolated(
        "partial_hunks_and_lines_preserve_unselected_changes",
        || {
            let dir = fixture();
            fs::write(dir.path().join("file.txt"), changed_text()).unwrap();
            let mut app = start(dir.path());
            select(&mut app, FileList::Unstaged, b"file.txt");
            assert_eq!(app.wt.diff.diff.as_ref().unwrap().files[0].hunks.len(), 2);
            app.dispatch(Action::StageHunk(hunk(&app, 0)));
            settle(&mut app);
            let first_only = base_text().replace("line 02\n", "first change\n");
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), first_only);
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                changed_text()
            );

            select(&mut app, FileList::Staged, b"file.txt");
            app.dispatch(Action::StageHunk(hunk(&app, 0)));
            settle(&mut app);
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), base_text());

            select(&mut app, FileList::Unstaged, b"file.txt");
            app.wt.diff.cursor = line(&app, "+first change");
            app.wt.diff.anchor = None;
            app.dispatch(Action::StageLines);
            settle(&mut app);
            assert_eq!(
                git(dir.path(), &["show", ":file.txt"]),
                base_text().replace("line 02\n", "line 02\nfirst change\n")
            );
            select(&mut app, FileList::Staged, b"file.txt");
            app.wt.diff.cursor = line(&app, "+first change");
            app.dispatch(Action::StageLines);
            settle(&mut app);
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), base_text());

            select(&mut app, FileList::Unstaged, b"file.txt");
            app.wt.diff.anchor = Some(line(&app, "-line 02"));
            app.wt.diff.cursor = line(&app, "+last change");
            app.dispatch(Action::StageLines);
            settle(&mut app);
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), changed_text());
            assert!(git(dir.path(), &["diff", "--name-only"]).is_empty());
            select(&mut app, FileList::Staged, b"file.txt");
            app.wt.diff.anchor = Some(line(&app, "-line 02"));
            app.wt.diff.cursor = line(&app, "+last change");
            app.dispatch(Action::StageLines);
            settle(&mut app);
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), base_text());
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                changed_text()
            );
        },
    );
}

#[test]
fn discard_confirmation_and_undo_preserve_index_and_other_hunks() {
    isolated(
        "discard_confirmation_and_undo_preserve_index_and_other_hunks",
        || {
            let dir = fixture();
            let indexed = base_text().replace("line 15\n", "staged change\n");
            git(dir.path(), &["config", "diff.context", "3"]);
            fs::write(dir.path().join("file.txt"), &indexed).unwrap();
            git(dir.path(), &["add", "file.txt"]);
            let work = indexed
                .replace("line 02\n", "first change\n")
                .replace("line 25\n", "last change\n");
            fs::write(dir.path().join("file.txt"), &work).unwrap();
            let mut app = start(dir.path());
            select(&mut app, FileList::Unstaged, b"file.txt");
            app.dispatch(Action::RevertHunk(hunk(&app, 0)));
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                work
            );
            app.on_dialog_result(DialogResult::Button(1));
            assert!(app.wt.last_revert.is_none());
            app.dispatch(Action::RevertHunk(hunk(&app, 0)));
            confirm(&mut app);
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                indexed.replace("line 25\n", "last change\n")
            );
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), indexed);
            app.dispatch(Action::UndoLastRevert);
            settle(&mut app);
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                work
            );
            assert!(app.wt.last_revert.is_none());

            select(&mut app, FileList::Unstaged, b"file.txt");
            app.wt.diff.cursor = line(&app, "+first change");
            app.dispatch(Action::RevertLines);
            confirm(&mut app);
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                work.replace("first change\n", "")
            );
            app.dispatch(Action::UndoLastRevert);
            settle(&mut app);
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                work
            );
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), indexed);
        },
    );
}

#[test]
fn failed_undo_can_be_retried_after_external_edit() {
    isolated("failed_undo_can_be_retried_after_external_edit", || {
        let dir = fixture();
        fs::write(dir.path().join("file.txt"), changed_text()).unwrap();
        let mut app = start(dir.path());
        select(&mut app, FileList::Unstaged, b"file.txt");
        app.dispatch(Action::RevertHunk(hunk(&app, 0)));
        confirm(&mut app);
        let reverted = fs::read(dir.path().join("file.txt")).unwrap();
        let undo = app.wt.last_revert.clone().unwrap();
        fs::write(dir.path().join("file.txt"), "external replacement\n").unwrap();
        app.dispatch(Action::UndoLastRevert);
        settle(&mut app);
        assert_error(&app, "patch");
        assert_eq!(
            fs::read_to_string(dir.path().join("file.txt")).unwrap(),
            "external replacement\n"
        );
        assert_eq!(app.wt.last_revert.as_ref(), Some(&undo));
        app.dialogs.clear();
        fs::write(dir.path().join("file.txt"), reverted).unwrap();
        app.dispatch(Action::UndoLastRevert);
        settle(&mut app);
        assert_eq!(
            fs::read_to_string(dir.path().join("file.txt")).unwrap(),
            changed_text()
        );
        assert!(app.wt.last_revert.is_none());
    });
}

#[test]
fn stale_partial_patch_refuses_changed_index_and_recovers() {
    isolated(
        "stale_partial_patch_refuses_changed_index_and_recovers",
        || {
            let dir = fixture();
            fs::write(dir.path().join("file.txt"), changed_text()).unwrap();
            let mut app = start(dir.path());
            select(&mut app, FileList::Unstaged, b"file.txt");
            let selected = hunk(&app, 0);
            git(dir.path(), &["add", "file.txt"]);
            app.dispatch(Action::StageHunk(selected));
            settle(&mut app);
            assert_error(&app, "patch");
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), changed_text());
            assert!(app.wt.unstaged.is_empty());
            app.dialogs.clear();
            select(&mut app, FileList::Staged, b"file.txt");
            app.dispatch(Action::StageHunk(hunk(&app, 0)));
            settle(&mut app);
            assert_eq!(
                git(dir.path(), &["show", ":file.txt"]),
                base_text().replace("line 25\n", "last change\n")
            );
        },
    );
}

#[test]
fn discard_refuses_changed_context_after_confirmation_opens() {
    isolated(
        "discard_refuses_changed_context_after_confirmation_opens",
        || {
            let dir = fixture();
            fs::write(dir.path().join("file.txt"), changed_text()).unwrap();
            let mut app = start(dir.path());
            select(&mut app, FileList::Unstaged, b"file.txt");
            app.dispatch(Action::RevertHunk(hunk(&app, 0)));
            fs::write(dir.path().join("file.txt"), "external replacement\n").unwrap();
            confirm(&mut app);
            assert_error(&app, "patch");
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                "external replacement\n"
            );
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), base_text());
            assert!(app.wt.last_revert.is_none());
        },
    );
}

#[test]
fn locked_index_reports_failure_preserves_changes_and_allows_retry() {
    isolated(
        "locked_index_reports_failure_preserves_changes_and_allows_retry",
        || {
            let dir = fixture();
            fs::write(dir.path().join("file.txt"), changed_text()).unwrap();
            let mut app = start(dir.path());
            select(&mut app, FileList::Unstaged, b"file.txt");
            let lock = dir.path().join(".git/index.lock");
            fs::write(&lock, "external lock").unwrap();
            app.dispatch(Action::StageSelected);
            settle(&mut app);
            assert_error(&app, "index.lock");
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), base_text());
            assert_eq!(fs::read_to_string(&lock).unwrap(), "external lock");
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                changed_text()
            );
            assert!(app.repo_state.index_locked);
            fs::remove_file(lock).unwrap();
            app.dialogs.clear();
            select(&mut app, FileList::Unstaged, b"file.txt");
            app.dispatch(Action::StageSelected);
            settle(&mut app);
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), changed_text());
            assert!(!app.repo_state.index_locked);
        },
    );
}

#[test]
fn failed_commit_hooks_preserve_draft_index_and_amend_state() {
    isolated(
        "failed_commit_hooks_preserve_draft_index_and_amend_state",
        || {
            for amend in [false, true] {
                let dir = fixture();
                fs::write(dir.path().join("file.txt"), changed_text()).unwrap();
                git(dir.path(), &["add", "file.txt"]);
                fs::write(
                    dir.path().join("file.txt"),
                    format!("{}unstaged\n", changed_text()),
                )
                .unwrap();
                let hooks = dir.path().join(".git/test-hooks");
                fs::create_dir(&hooks).unwrap();
                git(
                    dir.path(),
                    &["config", "core.hooksPath", hooks.to_str().unwrap()],
                );
                let head = git(dir.path(), &["rev-parse", "HEAD"]);
                let index = git(dir.path(), &["write-tree"]);
                let mut app = start(dir.path());
                app.wt.message.set_text("Original draft");
                if amend {
                    app.dispatch(Action::SetAmend(true));
                    settle(&mut app);
                    assert_eq!(app.wt.message.text(), "Base");
                    app.dispatch(Action::SetAmend(false));
                    settle(&mut app);
                    assert_eq!(app.wt.message.text(), "Original draft");
                    app.dispatch(Action::SetAmend(true));
                    settle(&mut app);
                }
                app.wt.message.set_text("Retained commit message");
                for hook in ["pre-commit", "commit-msg"] {
                    let path = hooks.join(hook);
                    script(&path, "echo 'fixture hook rejected commit' >&2\nexit 1");
                    app.dispatch(Action::Commit);
                    settle(&mut app);
                    assert_error(&app, "fixture hook rejected commit");
                    assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head);
                    assert_eq!(git(dir.path(), &["write-tree"]), index);
                    assert_eq!(app.wt.message.text(), "Retained commit message");
                    assert_eq!(app.wt.amend, amend);
                    assert_eq!(
                        config::load_draft(&app.repo.git_dir).unwrap().as_deref(),
                        Some("Retained commit message")
                    );
                    fs::remove_file(path).unwrap();
                    app.dialogs.clear();
                }
                app.dispatch(Action::Commit);
                settle(&mut app);
                assert_ne!(git(dir.path(), &["rev-parse", "HEAD"]), head);
                assert_eq!(
                    git(dir.path(), &["log", "-1", "--format=%s"]),
                    "Retained commit message\n"
                );
                assert_eq!(
                    git(dir.path(), &["rev-list", "--count", "HEAD"]),
                    if amend { "1\n" } else { "2\n" }
                );
                assert_eq!(git(dir.path(), &["show", "HEAD:file.txt"]), changed_text());
                assert_eq!(
                    fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                    format!("{}unstaged\n", changed_text())
                );
                assert!(app.wt.message.is_empty());
                assert!(!app.wt.amend);
                assert!(config::load_draft(&app.repo.git_dir).unwrap().is_none());
            }
        },
    );
}

#[test]
fn rejected_signer_preserves_commit_and_amend_drafts() {
    isolated("rejected_signer_preserves_commit_and_amend_drafts", || {
        for amend in [false, true] {
            let dir = fixture();
            fs::write(dir.path().join("file.txt"), changed_text()).unwrap();
            git(dir.path(), &["add", "file.txt"]);
            let signer = dir.path().join(".git/reject-signature");
            script(
                &signer,
                "cat >/dev/null\necho 'signing cancelled by fixture' >&2\nexit 1",
            );
            git(
                dir.path(),
                &["config", "gpg.program", signer.to_str().unwrap()],
            );
            git(dir.path(), &["config", "commit.gpgsign", "true"]);
            let head = git(dir.path(), &["rev-parse", "HEAD"]);
            let index = git(dir.path(), &["write-tree"]);
            let mut app = start(dir.path());
            if amend {
                app.dispatch(Action::SetAmend(true));
                settle(&mut app);
            }
            app.wt.message.set_text("Keep after cancelled signing");
            app.dispatch(Action::Commit);
            assert!(matches!(&app.handoff, Some(Handoff::Commit { amend: a, .. }) if *a == amend));
            assert!(!finish_handoff(&mut app));
            assert_eq!(app.status.1, StatusTone::Error);
            assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head);
            assert_eq!(git(dir.path(), &["write-tree"]), index);
            assert_eq!(app.wt.message.text(), "Keep after cancelled signing");
            assert_eq!(app.wt.amend, amend);
            assert_eq!(
                config::load_draft(&app.repo.git_dir).unwrap().as_deref(),
                Some("Keep after cancelled signing")
            );
            git(dir.path(), &["config", "commit.gpgsign", "false"]);
            app.dialogs.clear();
            app.dispatch(Action::Commit);
            settle(&mut app);
            assert_eq!(
                git(dir.path(), &["log", "-1", "--format=%s"]),
                "Keep after cancelled signing\n"
            );
            assert!(app.wt.message.is_empty());
        }
    });
}

fn conflict_fixture(operation: repo::Operation) -> tempfile::TempDir {
    let dir = fixture();
    git(dir.path(), &["checkout", "-qb", "other"]);
    fs::write(dir.path().join("file.txt"), "other\n").unwrap();
    git(dir.path(), &["commit", "-qam", "Other change"]);
    git(dir.path(), &["checkout", "-q", "main"]);
    fs::write(dir.path().join("file.txt"), "main\n").unwrap();
    git(dir.path(), &["commit", "-qam", "Main change"]);
    if operation == repo::Operation::Am {
        let patch = git(dir.path(), &["format-patch", "-1", "other", "--stdout"]);
        fs::write(dir.path().join(".git/fixture.patch"), patch).unwrap();
    }
    let args = match operation {
        repo::Operation::Merge => vec!["merge", "--no-edit", "other"],
        repo::Operation::Rebase => vec!["rebase", "other"],
        repo::Operation::CherryPick => vec!["cherry-pick", "other"],
        repo::Operation::Revert => vec!["revert", "--no-edit", "other"],
        repo::Operation::Am => vec!["am", "--3way", ".git/fixture.patch"],
        _ => panic!("unsupported fixture"),
    };
    let args: Vec<_> = args.iter().map(OsStr::new).collect();
    let output = git_output(dir.path(), &args);
    assert!(!output.status.success());
    assert!(!git(dir.path(), &["ls-files", "-u"]).is_empty());
    dir
}

#[test]
fn conflicted_operations_can_abort_without_changing_original_head() {
    isolated(
        "conflicted_operations_can_abort_without_changing_original_head",
        || {
            for operation in [
                repo::Operation::Merge,
                repo::Operation::Rebase,
                repo::Operation::CherryPick,
                repo::Operation::Revert,
                repo::Operation::Am,
            ] {
                let dir = conflict_fixture(operation);
                let original = git(dir.path(), &["rev-parse", "main"]);
                let mut app = start(dir.path());
                assert!(app.repo_state.operations.contains(&operation));
                assert!(app.wt.unstaged.iter().any(|f| f.unmerged));
                app.dispatch(Action::AbortOp);
                app.on_dialog_result(DialogResult::Button(1));
                assert!(repo::repo_state(&app.repo).operations.contains(&operation));
                app.dispatch(Action::AbortOp);
                confirm(&mut app);
                assert!(app.repo_state.operations.is_empty(), "{operation:?}");
                assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), original);
                assert!(git(dir.path(), &["status", "--porcelain"]).is_empty());
                assert_eq!(
                    fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                    "main\n"
                );
            }
        },
    );
}

#[test]
fn conflicted_operations_refuse_commit_then_continue_after_resolution() {
    isolated(
        "conflicted_operations_refuse_commit_then_continue_after_resolution",
        || {
            for operation in [
                repo::Operation::Merge,
                repo::Operation::Rebase,
                repo::Operation::CherryPick,
                repo::Operation::Revert,
                repo::Operation::Am,
            ] {
                let dir = conflict_fixture(operation);
                let head = git(dir.path(), &["rev-parse", "HEAD"]);
                let mut app = start(dir.path());
                app.wt.message.set_text("Resolution draft");
                app.dispatch(Action::Commit);
                settle(&mut app);
                assert_eq!(app.status.1, StatusTone::Error);
                assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head);
                assert_eq!(app.wt.message.text(), "Resolution draft");
                app.dialogs.clear();
                app.dispatch(Action::ContinueOp);
                assert!(!finish_handoff(&mut app));
                assert_eq!(app.status.1, StatusTone::Error);
                assert!(app.repo_state.operations.contains(&operation));
                select(&mut app, FileList::Unstaged, b"file.txt");
                app.dispatch(Action::StageSelected);
                assert!(matches!(app.dialogs.last().unwrap().kind, Kind::Confirm(_)));
                app.on_dialog_result(DialogResult::Button(1));
                assert!(!git(dir.path(), &["ls-files", "-u"]).is_empty());
                fs::write(dir.path().join("file.txt"), "resolved\n").unwrap();
                app.dispatch(Action::StageSelected);
                settle(&mut app);
                assert!(git(dir.path(), &["ls-files", "-u"]).is_empty());
                assert!(!app.wt.unstaged.iter().any(|f| f.unmerged));
                app.dispatch(Action::ContinueOp);
                assert!(finish_handoff(&mut app), "{operation:?}");
                assert!(app.repo_state.operations.is_empty());
                assert!(git(dir.path(), &["status", "--porcelain"]).is_empty());
                assert_eq!(git(dir.path(), &["show", "HEAD:file.txt"]), "resolved\n");
            }
        },
    );
}

#[test]
fn merge_use_version_resolves_and_amend_is_refused() {
    isolated("merge_use_version_resolves_and_amend_is_refused", || {
        for ours in [true, false] {
            let dir = conflict_fixture(repo::Operation::Merge);
            let mut app = start(dir.path());
            app.dispatch(Action::SetAmend(true));
            assert_error(&app, "Cannot amend while merging");
            assert!(!app.wt.amend);
            app.dialogs.clear();
            app.dispatch(Action::UseVersion(b"file.txt".to_vec(), ours));
            settle(&mut app);
            assert!(git(dir.path(), &["ls-files", "-u"]).is_empty());
            assert_eq!(
                git(dir.path(), &["show", ":file.txt"]),
                if ours { "main\n" } else { "other\n" }
            );
            app.wt.message.set_text("Merge resolved");
            app.dispatch(Action::Commit);
            settle(&mut app);
            assert_eq!(
                git(dir.path(), &["show", "-s", "--format=%P"])
                    .split_whitespace()
                    .count(),
                2
            );
            assert!(app.repo_state.operations.is_empty());
        }
    });
}

#[test]
fn unusual_paths_roundtrip_through_partial_and_whole_file_actions() {
    isolated(
        "unusual_paths_roundtrip_through_partial_and_whole_file_actions",
        || {
            for name in [
                b"with space.txt".as_slice(),
                b"with\ttab\nnewline.txt",
                b"escape\x1b[31m.txt",
                b"non-utf8-\xff.txt",
                b":(glob)*.txt",
                b"-option.txt",
            ] {
                let dir = fixture();
                let path = dir.path().join(OsStr::from_bytes(name));
                fs::write(&path, base_text()).unwrap();
                git(dir.path(), &["add", "."]);
                git(dir.path(), &["commit", "-qm", "Unusual path"]);
                fs::write(&path, changed_text()).unwrap();
                let mut app = start(dir.path());
                select(&mut app, FileList::Unstaged, name);
                assert!(!app.wt.cursor_file().unwrap().display.contains('\x1b'));
                app.dispatch(Action::StageHunk(hunk(&app, 0)));
                settle(&mut app);
                let mut object = b":".to_vec();
                object.extend_from_slice(name);
                let output = git_output(
                    dir.path(),
                    &[OsStr::new("show"), OsStr::from_bytes(&object)],
                );
                assert!(output.status.success());
                assert_eq!(
                    output.stdout,
                    base_text()
                        .replace("line 02\n", "first change\n")
                        .as_bytes()
                );
                select(&mut app, FileList::Staged, name);
                app.dispatch(Action::UnstageSelected);
                settle(&mut app);
                assert!(git(dir.path(), &["diff", "--cached", "--name-only"]).is_empty());
                select(&mut app, FileList::Unstaged, name);
                app.dispatch(Action::StageSelected);
                settle(&mut app);
                assert!(git(dir.path(), &["diff", "--name-only"]).is_empty());
                select(&mut app, FileList::Staged, name);
                app.dispatch(Action::UnstageSelected);
                settle(&mut app);
                select(&mut app, FileList::Unstaged, name);
                app.dispatch(Action::RevertSelected);
                confirm(&mut app);
                assert_eq!(fs::read(&path).unwrap(), base_text().as_bytes());
                assert!(git(dir.path(), &["status", "--porcelain"]).is_empty());
            }
        },
    );
}

#[test]
fn binary_rename_and_missing_final_newline_preserve_exact_bytes() {
    isolated(
        "binary_rename_and_missing_final_newline_preserve_exact_bytes",
        || {
            let dir = fixture();
            fs::write(dir.path().join("binary"), b"old\0bytes").unwrap();
            fs::write(dir.path().join("no-eol"), b"first\nlast").unwrap();
            git(dir.path(), &["add", "."]);
            git(dir.path(), &["commit", "-qm", "Binary and no newline"]);
            fs::write(dir.path().join("binary"), b"new\0bytes").unwrap();
            fs::write(dir.path().join("no-eol"), b"first\nchanged").unwrap();
            let mut app = start(dir.path());
            select(&mut app, FileList::Unstaged, b"binary");
            assert!(app.wt.diff.diff.as_ref().unwrap().files[0].binary);
            app.dispatch(Action::StageLines);
            settle(&mut app);
            assert!(git(dir.path(), &["diff", "--cached", "--name-only"]).is_empty());
            assert_eq!(fs::read(dir.path().join("binary")).unwrap(), b"new\0bytes");
            app.dispatch(Action::StageSelected);
            settle(&mut app);
            assert_eq!(
                git(dir.path(), &["show", ":binary"]).as_bytes(),
                b"new\0bytes"
            );
            select(&mut app, FileList::Staged, b"binary");
            app.dispatch(Action::UnstageSelected);
            settle(&mut app);
            select(&mut app, FileList::Unstaged, b"binary");
            app.dispatch(Action::RevertSelected);
            confirm(&mut app);
            assert_eq!(fs::read(dir.path().join("binary")).unwrap(), b"old\0bytes");

            select(&mut app, FileList::Unstaged, b"no-eol");
            app.dispatch(Action::StageHunk(hunk(&app, 0)));
            settle(&mut app);
            assert_eq!(git(dir.path(), &["show", ":no-eol"]), "first\nchanged");
            select(&mut app, FileList::Staged, b"no-eol");
            app.dispatch(Action::StageHunk(hunk(&app, 0)));
            settle(&mut app);
            assert_eq!(git(dir.path(), &["show", ":no-eol"]), "first\nlast");
            select(&mut app, FileList::Unstaged, b"no-eol");
            app.dispatch(Action::RevertHunk(hunk(&app, 0)));
            confirm(&mut app);
            assert_eq!(fs::read(dir.path().join("no-eol")).unwrap(), b"first\nlast");
            app.dispatch(Action::UndoLastRevert);
            settle(&mut app);
            assert_eq!(
                fs::read(dir.path().join("no-eol")).unwrap(),
                b"first\nchanged"
            );

            git(dir.path(), &["mv", "file.txt", "renamed.txt"]);
            app.dispatch(Action::Rescan);
            settle(&mut app);
            select(&mut app, FileList::Staged, b"renamed.txt");
            app.dispatch(Action::UnstageSelected);
            settle(&mut app);
            // The Worktree intentionally shows renames as a deletion and an addition.
            assert_eq!(
                git(dir.path(), &["diff", "--cached", "--name-status"]),
                "D\tfile.txt\n"
            );
            select(&mut app, FileList::Staged, b"file.txt");
            app.dispatch(Action::UnstageSelected);
            settle(&mut app);
            assert!(git(dir.path(), &["diff", "--cached", "--name-only"]).is_empty());
            assert_eq!(
                fs::read_to_string(dir.path().join("renamed.txt")).unwrap(),
                base_text()
            );
            assert!(!dir.path().join("file.txt").exists());
        },
    );
}

#[test]
fn truncated_real_diff_refuses_partial_stage_and_discard() {
    isolated(
        "truncated_real_diff_refuses_partial_stage_and_discard",
        || {
            let dir = fixture();
            // Cross the production capture limit with few lines to keep rendering cheap.
            let huge = format!("{}\n", "x".repeat(DEFAULT_LIMIT + 1024));
            fs::write(dir.path().join("file.txt"), &huge).unwrap();
            let mut app = start(dir.path());
            select(&mut app, FileList::Unstaged, b"file.txt");
            assert!(app.wt.diff.diff.as_ref().unwrap().files[0].truncated);
            app.dispatch(Action::StageHunk(hunk(&app, 0)));
            assert_error(&app, "truncated");
            app.dispatch(Action::RevertHunk(hunk(&app, 0)));
            assert_error(&app, "truncated");
            assert!(app.dialogs.is_empty());
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), base_text());
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                huge
            );
        },
    );
}

#[test]
fn destructive_reset_refuses_head_moved_while_confirming() {
    isolated(
        "destructive_reset_refuses_head_moved_while_confirming",
        || {
            let dir = fixture();
            fs::write(dir.path().join("file.txt"), changed_text()).unwrap();
            let mut app = start(dir.path());
            app.dispatch(Action::ResetHard);
            git(
                dir.path(),
                &["commit", "--allow-empty", "-qm", "External commit"],
            );
            let head = git(dir.path(), &["rev-parse", "HEAD"]);
            confirm(&mut app);
            assert_error(&app, "HEAD changed");
            assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head);
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                changed_text()
            );
            assert_eq!(app.head().unwrap().to_hex(), head.trim());
        },
    );
}

#[test]
fn read_only_refuses_mutations_without_consuming_undo_or_draft() {
    isolated(
        "read_only_refuses_mutations_without_consuming_undo_or_draft",
        || {
            let dir = fixture();
            fs::write(dir.path().join("file.txt"), changed_text()).unwrap();
            let mut app = start(dir.path());
            select(&mut app, FileList::Unstaged, b"file.txt");
            app.dispatch(Action::RevertHunk(hunk(&app, 0)));
            confirm(&mut app);
            let undo = app.wt.last_revert.clone();
            let work = fs::read(dir.path().join("file.txt")).unwrap();
            let head = git(dir.path(), &["rev-parse", "HEAD"]);
            let index = git(dir.path(), &["write-tree"]);
            app.wt.message.set_text("Read-only draft");
            app.read_only = true;
            app.runner.read_only = true;
            for action in [
                Action::StageSelected,
                Action::StageChanged,
                Action::StageHunk(hunk(&app, 0)),
                Action::StageLines,
                Action::RevertSelected,
                Action::RevertHunk(hunk(&app, 0)),
                Action::UndoLastRevert,
                Action::Commit,
                Action::ResetHard,
                Action::UseVersion(b"file.txt".to_vec(), true),
                Action::Fetch("origin".into()),
            ] {
                app.dispatch(action);
                settle(&mut app);
                assert_error(&app, "read-only");
                assert!(app.handoff.is_none());
                assert!(app.dialogs.is_empty());
                assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head);
                assert_eq!(git(dir.path(), &["write-tree"]), index);
                assert_eq!(fs::read(dir.path().join("file.txt")).unwrap(), work);
                assert_eq!(app.wt.last_revert, undo);
                assert_eq!(app.wt.message.text(), "Read-only draft");
            }
        },
    );
}

#[test]
fn new_and_deleted_files_roundtrip_before_initial_commit() {
    isolated(
        "new_and_deleted_files_roundtrip_before_initial_commit",
        || {
            let dir = fixture();
            git(dir.path(), &["checkout", "--orphan", "unborn"]);
            git(dir.path(), &["rm", "--cached", "-q", "file.txt"]);
            let mut app = start(dir.path());
            assert!(app.head().is_none());
            app.dispatch(Action::SetAmend(true));
            assert_error(&app, "No commit to amend");
            app.dialogs.clear();
            select(&mut app, FileList::Unstaged, b"file.txt");
            app.dispatch(Action::StageSelected);
            settle(&mut app);
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), base_text());
            select(&mut app, FileList::Staged, b"file.txt");
            app.dispatch(Action::UnstageSelected);
            settle(&mut app);
            assert!(git(dir.path(), &["ls-files"]).is_empty());
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                base_text()
            );
            select(&mut app, FileList::Unstaged, b"file.txt");
            app.dispatch(Action::StageSelected);
            settle(&mut app);
            app.dispatch(Action::Commit);
            assert_error(&app, "Please supply a commit message");
            assert!(app.head().is_none());
            app.dialogs.clear();
            app.wt.message.set_text("First commit");
            app.dispatch(Action::Commit);
            settle(&mut app);
            assert_eq!(git(dir.path(), &["rev-list", "--count", "HEAD"]), "1\n");
            fs::remove_file(dir.path().join("file.txt")).unwrap();
            app.dispatch(Action::Rescan);
            settle(&mut app);
            select(&mut app, FileList::Unstaged, b"file.txt");
            app.dispatch(Action::StageSelected);
            settle(&mut app);
            assert!(git(dir.path(), &["ls-files"]).is_empty());
            select(&mut app, FileList::Staged, b"file.txt");
            app.dispatch(Action::UnstageSelected);
            settle(&mut app);
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), base_text());
            assert!(!dir.path().join("file.txt").exists());
            select(&mut app, FileList::Unstaged, b"file.txt");
            app.dispatch(Action::RevertSelected);
            confirm(&mut app);
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                base_text()
            );
        },
    );
}

#[test]
fn amend_non_root_commit_preserves_parent_and_unstaged_work() {
    isolated(
        "amend_non_root_commit_preserves_parent_and_unstaged_work",
        || {
            let dir = fixture();
            fs::write(dir.path().join("file.txt"), changed_text()).unwrap();
            git(dir.path(), &["commit", "-qam", "To amend"]);
            let parent = git(dir.path(), &["rev-parse", "HEAD^"]);
            let old_head = git(dir.path(), &["rev-parse", "HEAD"]);
            let work = format!("{}keep unstaged\n", changed_text());
            fs::write(dir.path().join("file.txt"), &work).unwrap();
            let mut app = start(dir.path());
            app.wt.message.set_text("Saved new-commit draft");
            app.dispatch(Action::SetAmend(true));
            settle(&mut app);
            assert_eq!(app.wt.message.text(), "To amend");
            select(&mut app, FileList::Staged, b"file.txt");
            app.dispatch(Action::StageHunk(hunk(&app, 0)));
            settle(&mut app);
            let retained = base_text().replace("line 25\n", "last change\n");
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), retained);
            app.wt.message.set_text("Amended message");
            app.dispatch(Action::Commit);
            settle(&mut app);
            assert_eq!(git(dir.path(), &["rev-parse", "HEAD^"]), parent);
            assert_ne!(git(dir.path(), &["rev-parse", "HEAD"]), old_head);
            assert_eq!(git(dir.path(), &["rev-list", "--count", "HEAD"]), "2\n");
            assert_eq!(git(dir.path(), &["show", "HEAD:file.txt"]), retained);
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                work
            );
            assert!(app.wt.message.is_empty());
            assert!(!app.wt.amend);
        },
    );
}

#[test]
fn whole_file_discard_and_untracked_delete_require_confirmation() {
    isolated(
        "whole_file_discard_and_untracked_delete_require_confirmation",
        || {
            let dir = fixture();
            let indexed = base_text().replace("line 02\n", "staged\n");
            fs::write(dir.path().join("file.txt"), &indexed).unwrap();
            git(dir.path(), &["add", "file.txt"]);
            let work = format!("{indexed}unstaged\n");
            fs::write(dir.path().join("file.txt"), &work).unwrap();
            fs::write(dir.path().join("untracked.txt"), "keep until confirmed\n").unwrap();
            let mut app = start(dir.path());
            for path in [b"file.txt".as_slice(), b"untracked.txt"] {
                select(&mut app, FileList::Unstaged, path);
                app.dispatch(Action::RevertSelected);
                app.on_dialog_result(DialogResult::Button(1));
                assert_eq!(
                    fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                    work
                );
                assert!(dir.path().join("untracked.txt").exists());
            }
            select(&mut app, FileList::Unstaged, b"file.txt");
            app.dispatch(Action::RevertSelected);
            confirm(&mut app);
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                indexed
            );
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), indexed);
            select(&mut app, FileList::Unstaged, b"untracked.txt");
            app.dispatch(Action::RevertSelected);
            confirm(&mut app);
            assert!(!dir.path().join("untracked.txt").exists());
            assert_eq!(git(dir.path(), &["show", ":file.txt"]), indexed);
        },
    );
}

struct TerminalSession {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn std::io::Write + Send>,
    output: std::sync::mpsc::Receiver<Vec<u8>>,
    parser: vt100::Parser,
}

impl TerminalSession {
    fn new(dir: &Path, editor: &Path) -> Self {
        use portable_pty::{CommandBuilder, PtySize, native_pty_system};
        use std::io::Read;

        config::atomic_write(
            &config::config_file().unwrap(),
            b"confirm_quit = false\ncommit_handoff = \"always\"\n",
        )
        .unwrap();
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: 35,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_git-tui"));
        command.args([
            "--repo",
            dir.to_str().unwrap(),
            "--icons",
            "ascii",
            "--no-mouse",
        ]);
        command.env("TERM", "xterm-256color");
        command.env("GIT_EDITOR", editor);
        let child = pty.slave.spawn_command(command).unwrap();
        drop(pty.slave);
        let mut reader = pty.master.try_clone_reader().unwrap();
        let writer = pty.master.take_writer().unwrap();
        let (tx, output) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = [0; 8192];
            while let Ok(n) = reader.read(&mut bytes) {
                if n == 0 || tx.send(bytes[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut session = Self {
            child,
            master: pty.master,
            writer,
            output,
            parser: vt100::Parser::new(35, 120, 0),
        };
        session.wait_for_tui("Ready");
        session
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }

    fn wait_for(&mut self, text: &str) {
        Self::wait_for_screen(&self.output, &mut self.parser, text, false, None);
    }

    fn wait_for_tui(&mut self, text: &str) {
        Self::wait_for_screen(&self.output, &mut self.parser, text, true, None);
    }

    fn wait_for_tui_without(&mut self, text: &str, absent: &str) {
        Self::wait_for_screen(&self.output, &mut self.parser, text, true, Some(absent));
    }

    fn wait_for_screen(
        output: &std::sync::mpsc::Receiver<Vec<u8>>,
        parser: &mut vt100::Parser,
        text: &str,
        tui: bool,
        absent: Option<&str>,
    ) {
        assert!(!text.is_empty());
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut fresh = Vec::new();
        while Instant::now() < deadline {
            match output.recv_timeout(Duration::from_millis(50)) {
                Ok(bytes) => {
                    parser.process(&bytes);
                    fresh.extend_from_slice(&bytes);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
            let contents = parser.screen().contents();
            let matched = if tui {
                contents.contains(text)
                    && absent.is_none_or(|absent| !contents.contains(absent))
                    && parser.screen().alternate_screen()
            } else {
                // Leaving the alternate screen reveals prompts from earlier handoffs.
                fresh
                    .windows(text.len())
                    .any(|window| window == text.as_bytes())
                    && !parser.screen().alternate_screen()
            };
            if matched {
                return;
            }
            let consumed = fresh.len().saturating_sub(text.len() - 1);
            fresh.drain(..consumed);
        }
        panic!(
            "waiting for {text:?} without {absent:?} (tui: {tui}):\n{}",
            parser.screen().contents()
        );
    }

    fn assert_terminal_mode(&self, raw: bool) {
        let fd = self.master.as_raw_fd().unwrap();
        let mut settings = std::mem::MaybeUninit::<libc::termios>::uninit();
        // The master descriptor remains open and tcgetattr initializes the struct.
        assert_eq!(unsafe { libc::tcgetattr(fd, settings.as_mut_ptr()) }, 0);
        let settings = unsafe { settings.assume_init() };
        assert_eq!(settings.c_lflag & libc::ICANON == 0, raw);
        assert_eq!(settings.c_lflag & libc::ECHO == 0, raw);
        assert_eq!(self.parser.screen().alternate_screen(), raw);
    }

    fn quit(&mut self) {
        self.send(b"\x11");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "terminal failed to exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        while let Ok(bytes) = self.output.recv_timeout(Duration::from_millis(100)) {
            self.parser.process(&bytes);
        }
        self.assert_terminal_mode(false);
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            self.child.kill().expect("stop test terminal");
            self.child.wait().expect("reap test terminal");
        }
    }
}

#[test]
fn terminal_wait_ignores_old_handoff_prompt() {
    let prompt = "Press Enter to return to Git Twin.";
    let mut parser = vt100::Parser::new(35, 120, 0);
    parser.process(prompt.as_bytes());
    parser.process(b"\x1b[?1049h");
    let (tx, output) = std::sync::mpsc::channel();
    for bytes in [
        b"\x1b[?1049l".as_slice(),
        b"\r\nnew handoff finished\r\n",
        b"Press Enter to return ",
        b"to Git Twin.",
    ] {
        tx.send(bytes.to_vec()).unwrap();
    }
    drop(tx);

    TerminalSession::wait_for_screen(&output, &mut parser, prompt, false, None);

    assert!(parser.screen().contents().contains("new handoff finished"));
    assert_eq!(parser.screen().contents().matches(prompt).count(), 2);
    assert!(output.try_recv().is_err());
}

#[test]
fn terminal_editor_success_and_failure_restore_terminal_and_preserve_draft() {
    isolated(
        "terminal_editor_success_and_failure_restore_terminal_and_preserve_draft",
        || {
            let dir = fixture();
            let repo = repo::discover(dir.path()).unwrap();
            config::save_draft(&repo.git_dir, "Original draft").unwrap();
            let editor = repo.git_dir.join("fixture-editor");
            script(&editor, "printf 'Edited draft' > \"$1\"");
            let mut terminal = TerminalSession::new(dir.path(), &editor);
            terminal.assert_terminal_mode(true);
            terminal.send(b"\x1bOS"); // F4
            terminal.wait_for("Press Enter to return to Git Twin.");
            terminal.assert_terminal_mode(false);
            terminal.send(b"\r");
            terminal.wait_for_tui("Edited draft");
            terminal.assert_terminal_mode(true);
            script(
                &editor,
                "printf 'Must not replace draft' > \"$1\"\necho 'editor rejected' >&2\nexit 1",
            );
            terminal.send(b"\x1bOS");
            terminal.wait_for("Press Enter to return to Git Twin.");
            terminal.assert_terminal_mode(false);
            terminal.send(b"\r");
            terminal.wait_for_tui("Command failed");
            assert!(terminal.parser.screen().contents().contains("Edited draft"));
            terminal.assert_terminal_mode(true);
            terminal.quit();
            assert_eq!(
                config::load_draft(&repo.git_dir).unwrap().as_deref(),
                Some("Edited draft")
            );
        },
    );
}

#[test]
fn terminal_signing_failure_restores_terminal_and_retains_staged_work() {
    isolated(
        "terminal_signing_failure_restores_terminal_and_retains_staged_work",
        || {
            let dir = fixture();
            fs::write(dir.path().join("file.txt"), changed_text()).unwrap();
            git(dir.path(), &["add", "file.txt"]);
            let head = git(dir.path(), &["rev-parse", "HEAD"]);
            let index = git(dir.path(), &["write-tree"]);
            let repo = repo::discover(dir.path()).unwrap();
            let signer = repo.git_dir.join("fixture-signer");
            script(
                &signer,
                "cat >/dev/null\necho 'signing cancelled' >&2\nexit 1",
            );
            git(
                dir.path(),
                &["config", "gpg.program", signer.to_str().unwrap()],
            );
            git(dir.path(), &["config", "commit.gpgsign", "true"]);
            config::save_draft(&repo.git_dir, "Signing draft").unwrap();
            let mut terminal = TerminalSession::new(dir.path(), Path::new("true"));
            terminal.send(b"\x1b\r"); // Alt-Enter
            terminal.wait_for("Press Enter to return to Git Twin.");
            terminal.assert_terminal_mode(false);
            terminal.send(b"\r");
            terminal.wait_for_tui("commit failed");
            terminal.assert_terminal_mode(true);
            terminal.send(b"\x1b"); // Close the error dialog.
            terminal.wait_for_tui_without("Signing draft", "commit failed");
            terminal.quit();
            assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head);
            assert_eq!(git(dir.path(), &["write-tree"]), index);
            assert_eq!(
                config::load_draft(&repo.git_dir).unwrap().as_deref(),
                Some("Signing draft")
            );
        },
    );
}

#[test]
fn interrupted_terminal_commit_keeps_draft_and_can_be_retried() {
    isolated(
        "interrupted_terminal_commit_keeps_draft_and_can_be_retried",
        || {
            let dir = fixture();
            fs::write(dir.path().join("file.txt"), changed_text()).unwrap();
            git(dir.path(), &["add", "file.txt"]);
            let head = git(dir.path(), &["rev-parse", "HEAD"]);
            let index = git(dir.path(), &["write-tree"]);
            let repo = repo::discover(dir.path()).unwrap();
            let hooks = repo.git_dir.join("fixture-hooks");
            fs::create_dir(&hooks).unwrap();
            script(
                &hooks.join("pre-commit"),
                "echo 'fixture waiting for input'\nread answer </dev/tty",
            );
            git(
                dir.path(),
                &["config", "core.hooksPath", hooks.to_str().unwrap()],
            );
            config::save_draft(&repo.git_dir, "Interrupted draft").unwrap();
            let mut terminal = TerminalSession::new(dir.path(), Path::new("true"));
            terminal.send(b"\x1b\r");
            terminal.wait_for("fixture waiting for input");
            terminal.assert_terminal_mode(false);
            terminal.send(b"\x03"); // Ctrl-C interrupts the foreground Git command.
            terminal.wait_for("Press Enter to return to Git Twin.");
            terminal.send(b"\r");
            terminal.wait_for_tui("commit failed");
            terminal.assert_terminal_mode(true);
            assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head);
            assert_eq!(git(dir.path(), &["write-tree"]), index);
            assert_eq!(
                config::load_draft(&repo.git_dir).unwrap().as_deref(),
                Some("Interrupted draft")
            );
            terminal.send(b"\x1b");
            // The draft is also visible behind the error dialog.
            terminal.wait_for_tui_without("Interrupted draft", "commit failed");
            fs::remove_file(hooks.join("pre-commit")).unwrap();
            terminal.send(b"\x1b\r");
            terminal.wait_for("Press Enter to return to Git Twin.");
            terminal.send(b"\r");
            terminal.wait_for_tui("Commit created.");
            terminal.quit();
            assert_eq!(
                git(dir.path(), &["log", "-1", "--format=%s"]),
                "Interrupted draft\n"
            );
            assert!(config::load_draft(&repo.git_dir).unwrap().is_none());
        },
    );
}
