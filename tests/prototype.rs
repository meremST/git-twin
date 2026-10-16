use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use git_twin::{
    app::{
        App, Handoff, Options, StatusTone,
        action::{Action, FileList},
        dialog::{Dialog, Field, FormKind, Kind},
    },
    config::{Config, IconMode, State, ViewKind},
    git::repo,
    ui::{self, hit::Hit},
};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};

fn git(path: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Demo")
        .env("GIT_AUTHOR_EMAIL", "demo@example.invalid")
        .env("GIT_COMMITTER_NAME", "Demo")
        .env("GIT_COMMITTER_EMAIL", "demo@example.invalid")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Demo"]);
    git(
        dir.path(),
        &["config", "user.email", "demo@example.invalid"],
    );
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    git(dir.path(), &["config", "core.hooksPath", "/dev/null"]);
    fs::write(dir.path().join("hello.txt"), "hello\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "Initial greeting"]);
    fs::write(dir.path().join("hello.txt"), "hello\nworld\n").unwrap();
    fs::write(dir.path().join("staged.txt"), "ready\n").unwrap();
    git(dir.path(), &["add", "staged.txt"]);
    dir
}

fn app(dir: &std::path::Path, read_only: bool) -> App {
    App::new(
        repo::discover(dir).unwrap(),
        Config {
            icons: IconMode::Ascii,
            ..Config::default()
        },
        State::default(),
        Options {
            view: Some(ViewKind::History),
            read_only,
            mouse: Some(true),
            all_refs: false,
            paths: Vec::new(),
        },
    )
}

fn settle(app: &mut App) {
    settle_log_batches(app);
}

fn settle_log_batches(app: &mut App) -> usize {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut idle = Instant::now();
    let mut batches = 0;
    loop {
        while let Ok(msg) = app.workers.rx.try_recv() {
            if matches!(
                &msg.payload,
                git_twin::app::worker::Payload::LogBatch { .. }
            ) {
                batches += 1;
            }
            app.on_msg(msg);
        }
        app.tick();
        if app.busy() {
            idle = Instant::now();
        }
        if idle.elapsed() > Duration::from_millis(200) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "load timed out: {}",
            app.status.0
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    batches
}

fn assert_history_graph(app: &App) {
    use git_twin::domain::graph::GraphBuilder;
    let mut builder = GraphBuilder::with_cut_after(app.cfg.graph_cut_after as usize);
    let graph: Vec<_> = app
        .hist
        .commits
        .iter()
        .map(|c| builder.push(c.oid, &c.parents))
        .collect();
    assert_eq!(app.hist.graph, graph);
    let cuts: std::collections::HashMap<_, _> = builder
        .take_cuts()
        .into_iter()
        .map(|c| ((c.row, c.lane), c.target))
        .collect();
    assert_eq!(app.hist.cut_target, cuts);
    for (i, commit) in app.hist.commits.iter().enumerate() {
        assert_eq!(app.hist.index.get(&commit.oid), Some(&i));
    }
}

#[test]
fn history_local_rows_do_not_reload_commits() {
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    let request = app.req.log;
    let initial = app.hist.commits.last().unwrap().oid;
    let subject = app.hist.commits.last().unwrap().subject.as_ptr();
    app.select_oid(initial, false);
    settle(&mut app);
    for args in [
        vec!["reset", "-q", "HEAD", "--", "staged.txt"],
        vec!["add", "hello.txt"],
        vec!["reset", "--hard", "-q", "HEAD"],
    ] {
        git(dir.path(), &args);
        app.rescan();
        assert_eq!(settle_log_batches(&mut app), 0);
        assert_eq!(
            app.req.log, request,
            "status changes must not restart the log"
        );
        assert_eq!(app.hist.selected_oid(), Some(initial));
        assert_eq!(app.hist.commits.last().unwrap().subject.as_ptr(), subject);
        assert_history_graph(&app);
    }
    assert_eq!(app.hist.local_rows, 0);
}

#[test]
fn history_commit_and_cherry_pick_reuse_loaded_suffix() {
    use git_twin::{app::worker::After, domain::oid::Oid, git::ops};
    let dir = fixture();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "Second"]);
    git(dir.path(), &["checkout", "-qb", "source"]);
    fs::write(dir.path().join("picked.txt"), "picked\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "To pick"]);
    let source = Oid::from_hex(git(dir.path(), &["rev-parse", "HEAD"]).trim()).unwrap();
    git(dir.path(), &["checkout", "-q", "main"]);
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    let selected = app.hist.selected_oid();
    let subject = app.hist.commits.last().unwrap().subject.as_ptr();

    git(
        dir.path(),
        &["commit", "--allow-empty", "-qm", "New commit"],
    );
    app.refresh_all(None);
    assert_eq!(
        app.hist.selected_oid(),
        selected,
        "keep the old view while updating"
    );
    assert_eq!(settle_log_batches(&mut app), 0);
    assert_eq!(&*app.hist.commits[0].subject, "New commit");
    assert_eq!(app.hist.selected_oid(), selected);
    assert!(
        app.hist
            .diff
            .lines
            .iter()
            .any(|line| line.text.starts_with("Child:") && line.text.contains("New commit"))
    );
    assert_history_graph(&app);

    app.run_op(
        "Cherry-pick",
        Box::new(move |r| ops::cherry_pick(r, &source, None)),
        After::Refresh,
        Some(app.head()),
    );
    assert_eq!(settle_log_batches(&mut app), 0);
    assert_eq!(&*app.hist.commits[0].subject, "To pick");
    assert_eq!(app.hist.commits.last().unwrap().subject.as_ptr(), subject);
    assert_eq!(app.hist.selected_oid(), selected);
    assert_history_graph(&app);
}

#[test]
fn history_reset_keeps_base_but_reload_discards_it() {
    let dir = fixture();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "Second"]);
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    let old_head = app.head().unwrap();
    assert!(
        app.hist
            .related
            .as_ref()
            .unwrap()
            .branches
            .iter()
            .any(|b| b == "main")
    );
    git(dir.path(), &["reset", "--hard", "-q", "HEAD~1"]);
    app.refresh_all(None);
    assert_eq!(settle_log_batches(&mut app), 0);
    assert!(app.hist.index.contains_key(&old_head));
    assert_ne!(app.head(), Some(old_head));
    assert!(
        !app.hist
            .related
            .as_ref()
            .unwrap()
            .branches
            .iter()
            .any(|b| b == "main")
    );
    assert_history_graph(&app);

    app.dispatch(Action::Reload);
    assert!(
        app.hist.index.contains_key(&old_head),
        "keep the old list until data arrives"
    );
    assert!(settle_log_batches(&mut app) > 0);
    assert!(!app.hist.index.contains_key(&old_head));
    assert_history_graph(&app);
}

#[test]
fn history_rewritten_tip_and_filtered_views_use_full_reload() {
    let dir = fixture();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "Second"]);
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    git(dir.path(), &["commit", "--allow-empty", "-qm", "Temporary"]);
    app.refresh_all(None);
    assert_eq!(settle_log_batches(&mut app), 0);
    let temporary = app.head().unwrap();
    git(dir.path(), &["reset", "--hard", "-q", "HEAD~1"]);
    app.refresh_all(None);
    assert!(settle_log_batches(&mut app) > 0);
    assert!(!app.hist.index.contains_key(&temporary));
    assert_history_graph(&app);

    app.hist.views[0].max_count = "1".into();
    app.load_history(None);
    settle(&mut app);
    git(dir.path(), &["commit", "--allow-empty", "-qm", "Filtered"]);
    app.refresh_all(None);
    assert!(settle_log_batches(&mut app) > 0);
    assert_eq!(app.hist.commits.len(), 1);
    assert_eq!(&*app.hist.commits[0].subject, "Filtered");
    assert_history_graph(&app);
}

#[test]
fn history_incremental_graph_preserves_merge_lanes_and_cut_targets() {
    let dir = fixture();
    git(dir.path(), &["reset", "--hard", "-q", "HEAD"]);
    git(dir.path(), &["branch", "side"]);
    for _ in 0..8 {
        git(dir.path(), &["commit", "--allow-empty", "-qm", "Main"]);
    }
    git(dir.path(), &["checkout", "-q", "side"]);
    git(dir.path(), &["commit", "--allow-empty", "-qm", "Side"]);
    git(dir.path(), &["checkout", "-q", "main"]);
    git(dir.path(), &["merge", "--no-ff", "-qm", "Merge", "side"]);
    let mut app = app(dir.path(), false);
    app.cfg.graph_cut_after = 1;
    app.start();
    settle(&mut app);
    assert!(
        !app.hist.cut_target.is_empty(),
        "fixture must exercise cut edges"
    );
    assert_history_graph(&app);
    let old_len = app.hist.commits.len();
    git(
        dir.path(),
        &["commit", "--allow-empty", "-qm", "After merge"],
    );
    app.refresh_all(None);
    assert_eq!(settle_log_batches(&mut app), 0);
    assert_eq!(app.hist.commits.len(), old_len + 1);
    assert_history_graph(&app);

    fs::write(dir.path().join("hello.txt"), "local\n").unwrap();
    app.rescan();
    assert_eq!(settle_log_batches(&mut app), 0);
    assert_history_graph(&app);
    git(dir.path(), &["add", "hello.txt"]);
    app.rescan();
    assert_eq!(settle_log_batches(&mut app), 0);
    assert_history_graph(&app);
}

#[test]
fn history_soft_and_mixed_resets_rebuild_local_edges_when_needed() {
    for mode in ["--soft", "--mixed"] {
        let dir = fixture();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", "Second"]);
        let mut app = app(dir.path(), false);
        app.start();
        settle(&mut app);
        let old_head = app.head().unwrap();
        git(dir.path(), &["reset", mode, "-q", "HEAD~1"]);
        app.refresh_all(None);
        settle(&mut app);
        assert!(app.hist.index.contains_key(&old_head));
        assert_eq!(
            app.hist.local_flags,
            if mode == "--soft" {
                (false, true)
            } else {
                (true, false)
            }
        );
        assert_eq!(app.hist.commits[0].parents.as_ref(), &[app.head().unwrap()]);
        assert_history_graph(&app);
    }
}

#[test]
fn history_new_merge_and_all_refs_do_not_use_linear_extension() {
    let dir = fixture();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "Second"]);
    git(dir.path(), &["checkout", "-qb", "side", "HEAD~1"]);
    git(dir.path(), &["commit", "--allow-empty", "-qm", "Side"]);
    git(dir.path(), &["checkout", "-q", "main"]);
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    git(dir.path(), &["merge", "--no-ff", "-qm", "Merge", "side"]);
    app.refresh_all(None);
    assert!(settle_log_batches(&mut app) > 0);
    assert_eq!(app.hist.commits[0].parents.len(), 2);
    assert_history_graph(&app);

    app.hist.views[0].all_refs = true;
    app.load_history(None);
    settle(&mut app);
    git(dir.path(), &["commit", "--allow-empty", "-qm", "All refs"]);
    app.refresh_all(None);
    assert!(settle_log_batches(&mut app) > 0);
    assert_history_graph(&app);
}

#[test]
fn history_full_refresh_keeps_old_rows_until_replacement_and_ignores_stale_results() {
    use git_twin::app::worker::{Msg, Payload};
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    let old: Vec<_> = app.hist.commits.iter().map(|c| c.oid).collect();
    let old_request = app.req.log;
    app.load_history(None);
    assert_eq!(
        app.hist.commits.iter().map(|c| c.oid).collect::<Vec<_>>(),
        old
    );
    app.on_msg(Msg {
        id: old_request,
        generation: app.generation,
        payload: Payload::HistoryUpdate(Ok(None)),
    });
    let request = app.req.log;
    settle(&mut app);
    assert_eq!(
        app.req.log, request,
        "stale result must not restart the log"
    );
    assert_history_graph(&app);

    app.hist.views[0].message = "no matching commit".into();
    git(dir.path(), &["reset", "--hard", "-q", "HEAD"]);
    app.load_history(None);
    settle(&mut app);
    assert!(app.hist.commits.is_empty());
    assert!(app.hist.graph.is_empty());
    assert!(app.hist.index.is_empty());
}

#[test]
fn history_replacements_invalidate_the_cached_ancestry() {
    let dir = fixture();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "Second"]);
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    git(dir.path(), &["replace", "HEAD", "HEAD~1"]);
    app.refresh_all(None);
    assert!(settle_log_batches(&mut app) > 0);
    assert_eq!(
        &*app.hist.commits[app.hist.local_rows].subject,
        "Initial greeting"
    );
    assert_history_graph(&app);
    git(dir.path(), &["replace", "-d", "HEAD"]);
    app.refresh_all(None);
    assert!(settle_log_batches(&mut app) > 0);
    assert_eq!(&*app.hist.commits[0].subject, "Second");
}

#[test]
fn history_linear_probe_is_bounded_and_rejects_rewrites() {
    use git_twin::git::log::linear_extension;
    use std::sync::atomic::AtomicBool;
    let dir = fixture();
    let app = app(dir.path(), false);
    let oid = |rev| {
        git_twin::domain::oid::Oid::from_hex(git(dir.path(), &["rev-parse", rev]).trim()).unwrap()
    };
    let old = oid("HEAD");
    let cancel = AtomicBool::new(false);
    assert_eq!(
        linear_extension(&app.runner, old, old, &cancel)
            .unwrap()
            .unwrap()
            .len(),
        0
    );
    for _ in 0..64 {
        git(dir.path(), &["commit", "--allow-empty", "-qm", "Extension"]);
    }
    let new = oid("HEAD");
    assert_eq!(
        linear_extension(&app.runner, old, new, &cancel)
            .unwrap()
            .unwrap()
            .len(),
        64
    );
    git(
        dir.path(),
        &["commit", "--allow-empty", "-qm", "Beyond limit"],
    );
    assert!(
        linear_extension(&app.runner, old, oid("HEAD"), &cancel)
            .unwrap()
            .is_none()
    );
    assert!(
        linear_extension(&app.runner, new, old, &cancel)
            .unwrap()
            .is_none()
    );
}

#[test]
fn history_shallow_boundary_changes_force_a_reload() {
    let source = fixture();
    git(source.path(), &["add", "."]);
    git(source.path(), &["commit", "-qm", "Second"]);
    let parent = tempfile::tempdir().unwrap();
    git(
        parent.path(),
        &[
            "clone",
            "-q",
            "--depth=1",
            &format!("file://{}", source.path().display()),
            "shallow",
        ],
    );
    let path = parent.path().join("shallow");
    let mut app = app(&path, false);
    app.start();
    settle(&mut app);
    assert_eq!(app.hist.commits.len(), 1);
    git(&path, &["fetch", "-q", "--unshallow"]);
    app.refresh_all(None);
    assert!(settle_log_batches(&mut app) > 0);
    assert_eq!(app.hist.commits.len(), 2);
    assert_history_graph(&app);
}

#[test]
fn operation_success_waits_until_view_results_are_applied() {
    use git_twin::{
        app::worker::{After, Msg, Payload},
        domain::diff::Diff,
    };
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    app.req.diff = u64::MAX;
    app.on_msg(Msg {
        id: 0,
        generation: app.generation,
        payload: Payload::Op {
            label: "Revert".into(),
            result: Ok("done".into()),
            after: After::Nothing,
        },
    });
    app.tick();
    assert!(app.status.0.contains("updating view"));
    assert!(!app.log.iter().any(|entry| entry.text == "Revert: done"));
    assert!(!render(&mut app, 120, 40).contains("Revert: done"));
    assert!(app.working());
    app.on_msg(Msg {
        id: u64::MAX - 1,
        generation: app.generation,
        payload: Payload::CommitDiff(Ok(Diff::default())),
    });
    app.tick();
    assert!(
        app.status.0.contains("updating view"),
        "stale results cannot finish the refresh"
    );
    app.on_msg(Msg {
        id: u64::MAX,
        generation: app.generation,
        payload: Payload::CommitDiff(Ok(Diff::default())),
    });
    app.tick();
    assert_eq!(app.status.0, "Revert: done");
}

#[test]
fn operation_refresh_error_is_not_overwritten_by_success() {
    use git_twin::{
        app::worker::{After, Msg, Payload},
        git::GitError,
    };
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    app.req.diff = u64::MAX;
    app.on_msg(Msg {
        id: 0,
        generation: app.generation,
        payload: Payload::Op {
            label: "Revert".into(),
            result: Ok("done".into()),
            after: After::Nothing,
        },
    });
    app.on_msg(Msg {
        id: u64::MAX,
        generation: app.generation,
        payload: Payload::CommitDiff(Err(GitError::new("diff", "Cannot read updated diff"))),
    });
    app.tick();
    assert_eq!(app.status.1, StatusTone::Error);
    assert!(app.status.0.contains("Cannot read updated diff"));
    assert!(!app.log.iter().any(|entry| entry.text == "Revert: done"));
    assert!(!app.busy());
}

#[test]
fn operation_revert_reports_completion_after_refresh() {
    use git_twin::{
        app::worker::{After, Payload},
        git::ops,
    };
    let dir = fixture();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "Second"]);
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    let old = app.head().unwrap();
    app.run_op(
        "Revert",
        Box::new(move |r| ops::revert_commit(r, &old, None).map(|_| "done".into())),
        After::Refresh,
        Some(Some(old)),
    );
    assert!(app.status.0.contains("in progress"));
    let message = app
        .workers
        .rx
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    assert!(matches!(&message.payload, Payload::Op { .. }));
    app.on_msg(message);
    assert!(app.hist.loading);
    assert!(app.status.0.contains("updating view"));
    app.tick();
    assert!(!app.log.iter().any(|entry| entry.text == "Revert: done"));
    settle(&mut app);
    assert_eq!(app.status.0, "Revert: done");
    assert_ne!(app.head(), Some(old));
    assert!(app.hist.commits[0].subject.starts_with("Revert"));
    assert_history_graph(&app);
}

#[test]
fn operation_reset_finishes_when_active_view_is_updated() {
    use git_twin::{
        app::worker::{After, Payload},
        domain::oid::Oid,
        git::ops::{self, ResetMode},
    };
    for (view, full_reload) in [
        (ViewKind::History, false),
        (ViewKind::History, true),
        (ViewKind::Worktree, false),
    ] {
        let dir = fixture();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", "Second"]);
        let target = Oid::from_hex(git(dir.path(), &["rev-parse", "HEAD~1"]).trim()).unwrap();
        let mut app = app(dir.path(), false);
        app.hist.views[0].all_refs = full_reload;
        app.start();
        settle(&mut app);
        app.select_oid(target, false);
        settle(&mut app);
        app.switch_view(view);
        let old = app.head();
        app.run_op(
            "Reset",
            Box::new(move |r| ops::reset(r, ResetMode::Hard, &target).map(|_| "done".into())),
            After::Refresh,
            Some(old),
        );
        let mut deferred = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.status.0 != "Reset: done" {
            if let Ok(message) = app.workers.rx.recv_timeout(Duration::from_millis(10)) {
                let secondary = match view {
                    ViewKind::History => matches!(
                        &message.payload,
                        Payload::Details { .. } | Payload::Status(_) | Payload::LogDone(_)
                    ),
                    ViewKind::Worktree => matches!(
                        &message.payload,
                        Payload::HistoryUpdate(_) | Payload::LogBatch { .. } | Payload::LogDone(_)
                    ),
                };
                if secondary {
                    deferred.push(message);
                } else {
                    app.on_msg(message);
                }
            }
            app.tick();
            assert!(
                Instant::now() < deadline,
                "reset waited for secondary work: {}",
                app.status.0
            );
        }
        assert_eq!(app.head(), Some(target));
        match view {
            ViewKind::History => {
                assert!(app.hist.view_ready());
                assert_eq!(app.hist.selected_oid(), Some(target));
                assert!(app.wt.loading, "hidden status is deliberately not applied");
                if full_reload {
                    assert!(
                        app.hist.loading,
                        "remaining log completion must not block success"
                    );
                }
            }
            ViewKind::Worktree => {
                assert_eq!(app.wt.snapshot.as_ref().unwrap().head, Some(target));
                assert!(
                    app.hist.loading,
                    "hidden history is deliberately not applied"
                );
            }
        }
        for message in deferred {
            app.on_msg(message);
        }
        settle(&mut app);
    }
}

fn render(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    let b = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            let mut line = String::new();
            let mut x = 0;
            while x < width {
                let symbol = b[(x, y)].symbol();
                line.push_str(symbol);
                x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn details_tags_only_show_tags_on_selected_commit() {
    let dir = fixture();
    for subject in [
        "Release one",
        "Between releases",
        "Release two",
        "After releases",
    ] {
        git(dir.path(), &["commit", "--allow-empty", "-qm", subject]);
        match subject {
            "Release one" => {
                git(dir.path(), &["tag", "v1-light"]);
                git(
                    dir.path(),
                    &["tag", "-a", "v1-annotated", "-m", "Release one"],
                );
            }
            "Release two" => {
                git(dir.path(), &["tag", "v2"]);
            }
            _ => {}
        }
    }
    let mut app = app(dir.path(), true);
    app.start();
    settle(&mut app);

    for (subject, expected) in [
        ("Release one", vec!["v1-annotated", "v1-light"]),
        ("Initial greeting", vec![]),
        ("Release two", vec!["v2"]),
        ("Between releases", vec![]),
        ("After releases", vec![]),
    ] {
        let row = app
            .hist
            .commits
            .iter()
            .position(|c| c.subject.as_ref() == subject)
            .unwrap();
        app.select_row(row);
        settle(&mut app);

        let related = app.hist.related.as_ref().unwrap();
        assert_eq!(related.tags, expected, "{subject}");
        assert!(related.branches.iter().any(|b| b == "main"), "{subject}");
        let tag_lines: Vec<_> = app
            .hist
            .diff
            .lines
            .iter()
            .filter(|line| line.text.starts_with("Tags: "))
            .map(|line| line.text.as_str())
            .collect();
        if expected.is_empty() {
            assert!(tag_lines.is_empty(), "{subject}: {tag_lines:?}");
        } else {
            assert_eq!(tag_lines, vec![format!("Tags: {}", expected.join(", "))]);
        }
    }
}

#[test]
fn terminal_commit_selection_preserves_color_families() {
    use git_twin::{
        app::history::HFocus,
        config::{ColorMode, ThemeName},
        ui::theme::Theme,
    };
    use ratatui::style::{Color, Modifier};

    let dir = fixture();
    let mut app = app(dir.path(), true);
    app.start();
    settle(&mut app);
    let selected = app
        .hist
        .commits
        .iter()
        .position(|c| c.subject.as_ref() == "Initial greeting")
        .unwrap();
    app.hist.sel = Some(selected);
    app.hist.highlight = Some([app.hist.commits[selected].oid].into());
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();

    for colors in [ColorMode::Truecolor, ColorMode::C256, ColorMode::C16] {
        app.theme = Theme::new(&Config {
            theme: ThemeName::Terminal,
            colors,
            icons: IconMode::Ascii,
            ..Config::default()
        });
        for focus in [HFocus::List, HFocus::Diff] {
            app.hist.focus = focus;
            app.hist.sel = None;
            terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
            let baseline = terminal.backend().buffer().clone();
            app.hist.sel = Some(selected);
            terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            let row = app
                .hits
                .zones
                .iter()
                .find(|(_, hit)| *hit == Hit::CommitRow(selected))
                .unwrap()
                .0;
            let content: String = (row.x..row.right())
                .map(|x| buffer[(x, row.y)].symbol())
                .collect();
            for expected in ["*", "[main]", "Initial greeting", "Demo"] {
                assert!(content.contains(expected), "{content}");
            }
            for x in row.x..row.right() {
                let cell = &buffer[(x, row.y)];
                assert_eq!(cell.fg, Color::Reset, "inverted background at {x}");
                let original = baseline[(x, row.y)].fg;
                let rgb = match original {
                    Color::Blue => Some((0, 0, 175)),
                    Color::Green => Some((0, 95, 0)),
                    Color::Yellow => Some((95, 95, 0)),
                    Color::DarkGray => Some((95, 95, 95)),
                    _ => None,
                };
                let expected = match (colors, rgb) {
                    (ColorMode::Truecolor, Some((r, g, b))) => Color::Rgb(r, g, b),
                    (ColorMode::C256, Some((r, g, b))) => {
                        Color::Indexed(ui::theme::to_256(r, g, b))
                    }
                    _ => original,
                };
                assert_eq!(cell.bg, expected, "inverted text at {x}");
                assert!(!cell.modifier.contains(Modifier::UNDERLINED), "cell at {x}");
                assert!(cell.modifier.contains(Modifier::REVERSED), "cell at {x}");
                if focus == HFocus::List {
                    assert!(cell.modifier.contains(Modifier::BOLD), "cell at {x}");
                } else {
                    assert_eq!(
                        cell.modifier.contains(Modifier::BOLD),
                        baseline[(x, row.y)].modifier.contains(Modifier::BOLD),
                        "bold at {x}"
                    );
                }
            }
            let unselected = app
                .hits
                .zones
                .iter()
                .find(|(_, hit)| matches!(hit, Hit::CommitRow(i) if *i != selected))
                .unwrap()
                .0;
            for x in unselected.x..unselected.right() {
                assert_eq!(buffer[(x, unselected.y)].fg, baseline[(x, unselected.y)].fg);
                assert!(
                    !buffer[(x, unselected.y)]
                        .modifier
                        .contains(Modifier::REVERSED)
                );
                assert!(
                    !buffer[(x, unselected.y)]
                        .modifier
                        .contains(Modifier::UNDERLINED)
                );
            }
        }
    }
}

#[test]
fn real_repository_renders_both_views_and_resizes() {
    let dir = fixture();
    let mut app = app(dir.path(), true);
    app.start();
    settle(&mut app);
    let screen = render(&mut app, 120, 35);
    assert!(screen.contains("Initial greeting"), "{screen}");
    assert!(screen.contains("[main]"), "{screen}");
    assert!(screen.contains("READ ONLY"), "{screen}");
    app.on_key(KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE));
    assert_eq!(app.view, ViewKind::Worktree);
    let screen = render(&mut app, 120, 35);
    for expected in [
        "Unstaged Changes (1)",
        "Staged Changes (1)",
        "hello.txt",
        "staged.txt",
        "Commit Message",
    ] {
        assert!(screen.contains(expected), "missing {expected}: {screen}");
    }
    render(&mut app, 79, 24);
    for (r, hit) in &app.hits.zones {
        if matches!(hit, Hit::MenuTitle(_) | Hit::ViewTab(_)) {
            assert_eq!(
                app.hits.at(r.x, r.y).unwrap().1,
                hit,
                "overlapping menu and view tabs"
            );
        }
    }
    for (w, h) in [
        (79, 24),
        (79, 26),
        (80, 24),
        (120, 35),
        (200, 60),
        (78, 26),
        (79, 23),
        (1, 1),
        (0, 0),
    ] {
        for ratio in [0, 100, 500, 900, u16::MAX] {
            app.state.splits.worktree_left = ratio;
            app.state.splits.worktree_diff = ratio;
            app.state.splits.worktree_files = ratio;
            let screen = render(&mut app, w, h);
            if w > 0 && h > 0 {
                assert_eq!(
                    screen.contains("Compact:"),
                    w >= 79 && h >= 24 && (w < 120 || h < 35),
                    "{w} x {h}: {screen}"
                );
            }
            if (w, h) == (78, 26) || (w, h) == (79, 23) {
                assert!(screen.contains("resize to at least 79 x 24"), "{screen}");
                assert!(app.hits.zones.is_empty());
            }
            app.switch_view(ViewKind::History);
            app.state.splits.history_top = ratio;
            app.state.splits.history_bottom_left = ratio;
            render(&mut app, w, h);
            app.switch_view(ViewKind::Worktree);
        }
    }
}

#[test]
fn nerd_font_icons_decorate_both_views_without_overlap() {
    let dir = fixture();
    let mut app = app(dir.path(), true);
    app.cfg.icons = IconMode::Nerd;
    app.theme = git_twin::ui::theme::Theme::new(&app.cfg);
    app.start();
    settle(&mut app);
    let screen = render(&mut app, 120, 35);
    for expected in [
        "\u{f1da} History",
        "\u{e0a0} main",
        "\u{f023} READ ONLY",
        "\u{f060} Back",
        "\u{f002} Find",
        "F1: help",
    ] {
        assert!(screen.contains(expected), "missing {expected}: {screen}");
    }
    assert!(!screen.contains("\u{f059} F1: help"), "{screen}");
    assert!(!screen.contains("[main]"), "{screen}");
    app.on_key(KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE));
    let screen = render(&mut app, 120, 35);
    for expected in [
        "\u{f040} Unstaged Changes (1)",
        "\u{f046} Staged Changes (1)",
        "\u{e64e} hello.txt",
        "\u{f417} Commit",
        "\u{f192} New Commit",
        "\u{f10c} Amend Last Commit",
    ] {
        assert!(screen.contains(expected), "missing {expected}: {screen}");
    }
    for view in [ViewKind::Worktree, ViewKind::History] {
        app.switch_view(view);
        for (w, h) in [(79, 24), (120, 35), (200, 60)] {
            render(&mut app, w, h);
            for (r, hit) in &app.hits.zones {
                if matches!(hit, Hit::MenuTitle(_) | Hit::ViewTab(_)) {
                    assert_eq!(
                        app.hits.at(r.x, r.y).unwrap().1,
                        hit,
                        "overlapping menu and view tabs at {w} x {h}"
                    );
                }
            }
        }
    }
}

#[test]
fn icon_click_stages_file_and_read_only_refuses_mutations() {
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    app.switch_view(ViewKind::Worktree);
    render(&mut app, 120, 35);
    let (r, _) = app
        .hits
        .zones
        .iter()
        .find(|(_, hit)| matches!(hit, Hit::WtFile(FileList::Unstaged, 0, true)))
        .unwrap();
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: r.x,
        row: r.y,
        modifiers: KeyModifiers::NONE,
    });
    settle(&mut app);
    assert!(app.wt.unstaged.is_empty(), "{}", app.status.0);
    assert_eq!(
        git(dir.path(), &["diff", "--cached", "--name-only"]),
        "hello.txt\nstaged.txt\n"
    );
    app.read_only = true;
    app.runner.read_only = true;
    app.dispatch(Action::SelectFile(FileList::Staged, 0));
    app.dispatch(Action::UnstageSelected);
    assert!(app.status.0.contains("read-only"), "{}", app.status.0);
    assert_eq!(
        git(dir.path(), &["diff", "--cached", "--name-only"]),
        "hello.txt\nstaged.txt\n"
    );
}

#[test]
fn double_click_does_not_stage_the_next_file_after_refresh() {
    let dir = fixture();
    fs::write(dir.path().join("z-second.txt"), "leave untracked\n").unwrap();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    app.switch_view(ViewKind::Worktree);
    render(&mut app, 120, 35);
    let (r, _) = app
        .hits
        .zones
        .iter()
        .find(|(_, h)| matches!(h, Hit::WtFile(FileList::Unstaged, 0, true)))
        .unwrap();
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: r.x,
        row: r.y,
        modifiers: KeyModifiers::NONE,
    };
    app.on_mouse(event);
    settle(&mut app);
    render(&mut app, 120, 35);
    assert_eq!(app.wt.unstaged.len(), 1);
    assert_eq!(app.wt.unstaged[0].display, "z-second.txt");
    // Seed a recent first press without relying on Git's execution time.
    app.clicks = git_twin::app::mouse::ClickTracker::new(Duration::from_secs(1));
    app.clicks.press(event.column, event.row, Instant::now());
    app.on_mouse(event);
    settle(&mut app);
    assert_eq!(
        git(dir.path(), &["diff", "--cached", "--name-only"]),
        "hello.txt\nstaged.txt\n"
    );
}

#[test]
fn commit_keeps_unstaged_changes_and_clears_message() {
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    app.switch_view(ViewKind::Worktree);
    app.wt.message.set_text("Add staged file");
    app.dispatch(Action::Commit);
    settle(&mut app);
    assert_eq!(
        git(dir.path(), &["log", "-1", "--format=%s"]),
        "Add staged file\n"
    );
    assert_eq!(git(dir.path(), &["diff", "--name-only"]), "hello.txt\n");
    assert!(git(dir.path(), &["diff", "--cached", "--name-only"]).is_empty());
    assert!(app.wt.message.is_empty());
}

#[test]
fn menus_and_dialogs_have_topmost_click_targets() {
    let dir = fixture();
    let mut app = app(dir.path(), true);
    app.start();
    settle(&mut app);
    app.open_menu(0);
    let screen = render(&mut app, 80, 24);
    assert!(screen.contains("Update"), "{screen}");
    app.dialogs.push(
        Dialog::new("Test dialog", Kind::Form(FormKind::Preferences)).with_fields(vec![
            Field::text("Name", "initial"),
            Field::check("Enabled", false),
        ]),
    );
    render(&mut app, 80, 24);
    app.on_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));
    assert_eq!(app.dialogs.last().unwrap().text(0), "initial!");
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.dialogs.is_empty());
    assert!(app.menu.is_some());
}

#[test]
fn preferences_color_scheme_dropdown_and_persistence() {
    use git_twin::{
        app::dialog::DialogResult,
        config::{self, ColorMode, ColorScheme, Handoff, StageUntracked, ThemeName},
    };
    use ratatui::style::Color;

    // Keep preference writes away from the user's real XDG directories.
    if std::env::var_os("GITT_THEME_TEST").is_none() {
        let state = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "preferences_color_scheme_dropdown_and_persistence",
                "--nocapture",
            ])
            .env("GITT_THEME_TEST", "1")
            .env("XDG_STATE_HOME", state.path().join("state"))
            .env("XDG_CONFIG_HOME", state.path().join("config"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.cfg = Config {
        colors: ColorMode::Truecolor,
        icons: IconMode::Ascii,
        graph_connectors: true,
        osc52: true,
        history_context: 7,
        worktree_context: 9,
        tab_width: 4,
        commit_width: 90,
        stage_untracked: StageUntracked::No,
        commit_handoff: Handoff::Never,
        allow_textconv: true,
        confirm_quit: false,
        ..Config::default()
    };
    let mut expected = app.cfg.clone();
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    let click = |app: &mut App, hit: Hit| {
        let r = app.hits.zones.iter().find(|(_, h)| *h == hit).unwrap().0;
        assert_eq!(app.hits.at(r.x, r.y).unwrap().1, &hit);
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        });
    };
    app.open_preferences();
    assert!(render(&mut app, 120, 40).contains("Color scheme: [ default v ]"));
    click(&mut app, Hit::DlgField(1));
    for (w, h) in [(120, 40), (80, 24), (79, 24)] {
        let screen = render(&mut app, w, h);
        assert!(
            screen.contains("default") && screen.contains("gruvbox"),
            "{screen}"
        );
        assert!(
            app.hits
                .zones
                .iter()
                .any(|(_, h)| *h == Hit::DlgChoice(1, 1))
        );
    }
    click(&mut app, Hit::DlgChoice(1, 1));
    assert_eq!(app.dialogs.last().unwrap().chosen(1), 1);
    assert_eq!(app.dialogs.last().unwrap().open_choice, None);
    assert_eq!(
        app.cfg.color_scheme,
        ColorScheme::Default,
        "apply only on OK"
    );

    render(&mut app, 120, 40);
    click(&mut app, Hit::DlgField(0)); // dark -> light
    click(&mut app, Hit::DlgField(0)); // light -> terminal
    let screen = render(&mut app, 120, 40);
    assert!(screen.contains("Theme: < terminal >"));
    assert!(!screen.contains("Color scheme"));
    assert!(!app.hits.zones.iter().any(|(_, h)| *h == Hit::DlgField(1)));
    app.on_key(key(KeyCode::Tab));
    assert_eq!(
        app.dialogs.last().unwrap().focus,
        2,
        "skip hidden color scheme"
    );
    app.on_key(key(KeyCode::BackTab));
    assert_eq!(app.dialogs.last().unwrap().focus, 0);
    app.on_key(key(KeyCode::Left)); // terminal -> light
    assert!(render(&mut app, 120, 40).contains("Color scheme: [ gruvbox v ]"));
    app.on_key(key(KeyCode::Left)); // light -> dark

    for (mode, mode_index, bg) in [
        (ThemeName::Dark, 0, Color::Rgb(0x28, 0x28, 0x28)),
        (ThemeName::Light, 1, Color::Rgb(0xfb, 0xf1, 0xc7)),
        (ThemeName::Terminal, 2, Color::Reset),
    ] {
        app.dialogs.last_mut().unwrap().select_choice(0, mode_index);
        render(&mut app, 120, 40);
        click(&mut app, Hit::DlgButton(0));
        assert!(app.dialogs.is_empty());
        expected.theme = mode;
        expected.color_scheme = ColorScheme::Gruvbox;
        assert_eq!(
            app.cfg, expected,
            "unrelated preferences must stay unchanged"
        );
        assert_eq!(app.theme.bg, bg);
        assert_eq!(
            Config::load_from(&config::config_file().unwrap()).unwrap(),
            expected
        );
        app.open_preferences();
        assert_eq!(app.dialogs.last().unwrap().chosen(0), mode_index);
        assert_eq!(app.dialogs.last().unwrap().chosen(1), 1);
    }

    // Returning from terminal mode restores the saved color scheme.
    app.on_key(key(KeyCode::Left));
    app.on_key(key(KeyCode::Tab));
    app.on_key(key(KeyCode::Enter));
    app.on_key(key(KeyCode::Home));
    app.on_key(key(KeyCode::Enter));
    assert_eq!(app.dialogs.last().unwrap().chosen(1), 0);
    assert_eq!(
        app.dialogs.len(),
        1,
        "choosing a color must not submit the form"
    );
    app.on_dialog_result(DialogResult::Button(1));
    assert_eq!(
        app.cfg, expected,
        "Cancel must leave the active theme unchanged"
    );
    assert_eq!(
        Config::load_from(&config::config_file().unwrap()).unwrap(),
        expected
    );

    app.open_preferences();
    app.on_key(key(KeyCode::Left));
    app.on_key(key(KeyCode::Tab));
    app.on_key(key(KeyCode::Char(' ')));
    app.on_key(key(KeyCode::Home));
    app.on_key(key(KeyCode::Enter));
    app.on_dialog_result(DialogResult::Button(0));
    assert_eq!(app.cfg.color_scheme, ColorScheme::Default);
    assert_eq!(app.theme.bg, Color::Rgb(0xe1, 0xe2, 0xe7));
    assert_eq!(
        Config::load_from(&config::config_file().unwrap()).unwrap(),
        app.cfg
    );
}

#[test]
fn cli_commands_do_not_need_a_repository() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        &["--help"][..],
        &["--version"],
        &["doctor"],
        &["--theme", "dark", "--color-scheme", "gruvbox", "doctor"],
        &["--theme", "light", "--color-scheme", "gruvbox", "doctor"],
        &["--theme", "terminal", "--color-scheme", "default", "doctor"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gitt"))
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_gitt"))
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!dir.path().join(".git").exists());
}

#[test]
fn terminal_launch_switch_view_and_clean_exit() {
    check_terminal_launcher(
        env!("CARGO_BIN_EXE_gitt"),
        "Initial greeting",
        "Commit Message",
        "worktree",
        120,
        35,
    );
    check_terminal_launcher(
        env!("CARGO_BIN_EXE_git-tui"),
        "Commit Message",
        "Initial greeting",
        "history",
        120,
        35,
    );
}

#[test]
fn terminal_launch_at_79_columns_switches_view_and_exits() {
    for height in [24, 26] {
        check_terminal_launcher(
            env!("CARGO_BIN_EXE_gitt"),
            "Initial greeting",
            "Unstaged Changes",
            "worktree",
            79,
            height,
        );
        check_terminal_launcher(
            env!("CARGO_BIN_EXE_git-tui"),
            "Unstaged Changes",
            "Initial greeting",
            "history",
            79,
            height,
        );
    }
}

fn check_terminal_launcher(
    binary: &str,
    first: &str,
    second: &str,
    previous_view: &str,
    width: u16,
    height: u16,
) {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::{
        io::{Read, Write},
        sync::mpsc,
    };

    let dir = fixture();
    fs::create_dir_all(dir.path().join("state/git-twin")).unwrap();
    fs::write(
        dir.path().join("state/git-twin/state.toml"),
        format!("last_view = \"{previous_view}\"\n"),
    )
    .unwrap();
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: height,
            cols: width,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(binary);
    command.args([
        "--repo",
        dir.path().to_str().unwrap(),
        "--icons",
        "ascii",
        "--no-mouse",
    ]);
    command.env("TERM", "xterm-256color");
    command.env("XDG_CONFIG_HOME", dir.path().join("config"));
    command.env("XDG_STATE_HOME", dir.path().join("state"));
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env("GIT_CONFIG_GLOBAL", "/dev/null");
    let mut child = pty.slave.spawn_command(command).unwrap();
    drop(pty.slave);
    let mut reader = pty.master.try_clone_reader().unwrap();
    let mut writer = pty.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader_thread = std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || tx.send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut parser = vt100::Parser::new(height, width, 0);
    let wait_for = |parser: &mut vt100::Parser, needle: &str| -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(bytes) = rx.recv_timeout(Duration::from_millis(100)) {
                parser.process(&bytes);
                if parser.screen().contents().contains(needle) {
                    return true;
                }
            }
        }
        false
    };
    let loaded = wait_for(&mut parser, first);
    writer.write_all(b"\x1b[17~").unwrap(); // F6
    writer.flush().unwrap();
    let switched = wait_for(&mut parser, second);
    writer.write_all(b"\x11").unwrap(); // Ctrl-Q
    writer.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    drop(writer);
    drop(pty.master);
    reader_thread.join().unwrap();
    assert!(
        loaded,
        "history not rendered: {}",
        parser.screen().contents()
    );
    assert!(
        switched,
        "view not switched: {}",
        parser.screen().contents()
    );
    assert!(
        status.is_some_and(|s| s.success()),
        "terminal did not exit successfully"
    );
    assert!(dir.path().join("state/git-twin/state.toml").exists());
}

#[test]
fn compact_panes_keep_controls_accessible_and_return_to_full_layout() {
    for (width, height) in [(79, 24), (79, 26), (80, 24)] {
        check_compact_panes(width, height);
    }
}

fn check_compact_panes(width: u16, height: u16) {
    use git_twin::ui::hit::PaneId;
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    app.switch_view(ViewKind::Worktree);
    let screen = render(&mut app, width, height);
    assert!(screen.contains("Compact:"), "{screen}");
    for (action, label) in [
        (Action::Rescan, "Rescan"),
        (Action::StageChanged, "Stage Changed"),
        (Action::SignOff, "Sign Off"),
        (Action::Commit, "Commit"),
        (Action::Push, "Push"),
    ] {
        let (r, _) = app
            .hits
            .zones
            .iter()
            .find(|(_, h)| *h == Hit::Button(action.clone()))
            .unwrap();
        assert!(r.x > 0, "compact {label} button should shift right");
    }
    let (push, _) = app
        .hits
        .zones
        .iter()
        .find(|(_, h)| *h == Hit::Button(Action::Push))
        .unwrap();
    assert_eq!(
        push.right(),
        width,
        "compact action group should end at right"
    );
    for label in [
        "[Unstaged]",
        "[Staged]",
        "[Diff]",
        "[Message]",
        "[Stage Changed]",
        "[Commit]",
        "[Push]",
    ] {
        assert!(screen.contains(label), "missing {label}: {screen}");
    }
    for id in [
        PaneId::WMessage,
        PaneId::WDiff,
        PaneId::WStaged,
        PaneId::WUnstaged,
    ] {
        let (r, _) = app
            .hits
            .zones
            .iter()
            .find(|(_, h)| *h == Hit::PaneTab(id))
            .unwrap();
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        });
        let screen = render(&mut app, width, height);
        assert_eq!(app.focused_pane(), id);
        assert!(app.hits.zones.iter().any(|(_, h)| *h == Hit::Pane(id)));
        if id == PaneId::WMessage {
            assert!(screen.contains("[Amend Last Commit]"), "{screen}");
            let (new_commit, _) = app
                .hits
                .zones
                .iter()
                .find(|(_, h)| *h == Hit::Button(Action::SetAmend(false)))
                .unwrap();
            let (amend, _) = app
                .hits
                .zones
                .iter()
                .find(|(_, h)| *h == Hit::Button(Action::SetAmend(true)))
                .unwrap();
            let (message_pane, _) = app
                .hits
                .zones
                .iter()
                .find(|(_, h)| *h == Hit::Pane(PaneId::WMessage))
                .unwrap();
            let (message_area, _) = app
                .hits
                .zones
                .iter()
                .find(|(_, h)| *h == Hit::Message)
                .unwrap();
            assert_eq!(
                amend.x,
                new_commit.right() + 1,
                "amend control should sit beside new commit"
            );
            assert_eq!(amend.y, new_commit.y);
            assert!(
                new_commit.y < message_pane.y,
                "controls should sit outside the frame"
            );
            assert_eq!(
                message_area, message_pane,
                "message field should start at the top of the frame's inner area"
            );
        }
    }
    let screen = render(&mut app, 120, 35);
    assert!(!screen.contains("Compact:"));
    for label in ["Staged Changes", "Unstaged Changes", "Commit Message"] {
        assert!(screen.contains(label));
    }
    let (message, _) = app
        .hits
        .zones
        .iter()
        .find(|(_, h)| *h == Hit::Pane(PaneId::WMessage))
        .unwrap();
    let (message_area, _) = app
        .hits
        .zones
        .iter()
        .find(|(_, h)| *h == Hit::Message)
        .unwrap();
    assert_eq!(message_area, message);
    let (amend, _) = app
        .hits
        .zones
        .iter()
        .find(|(_, h)| *h == Hit::Button(Action::SetAmend(true)))
        .unwrap();
    assert!(
        amend.y < message.y,
        "full-layout amend control should sit outside the frame"
    );
    for (action, label) in [
        (Action::Rescan, "Rescan"),
        (Action::StageChanged, "Stage Changed"),
        (Action::SignOff, "Sign Off"),
        (Action::Commit, "Commit"),
        (Action::Push, "Push"),
    ] {
        let (r, _) = app
            .hits
            .zones
            .iter()
            .find(|(_, h)| *h == Hit::Button(action.clone()))
            .unwrap();
        assert_eq!(
            r.width,
            unicode_width::UnicodeWidthStr::width(label) as u16 + 2,
            "full-layout {label} button should have a compact hit area"
        );
        assert!(
            r.right() <= message.x && message.x - r.right() <= 3,
            "full-layout {label} button should sit next to the commit message pane"
        );
    }
    app.switch_view(ViewKind::History);
    app.hist.focus = git_twin::app::history::HFocus::Sha;
    assert!(render(&mut app, width, height).contains("SHA1:"));
    app.hist.focus = git_twin::app::history::HFocus::Search;
    let screen = render(&mut app, width, height);
    for label in [
        "Search:",
        "[  Old version]",
        "[  New version]",
        "Ignore space",
        "Line diff",
    ] {
        assert!(screen.contains(label), "{screen}");
    }
}

#[test]
fn commit_guide_tracks_display_columns_without_overwriting_text() {
    use git_twin::app::worktree::WFocus;
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.switch_view(ViewKind::Worktree);
    app.wt.focus = WFocus::Message;
    app.cfg.commit_width = 20;
    app.wt.message.set_text("short\n");
    let screen = render(&mut app, 80, 24);
    assert!(screen.contains("col 1/20"), "{screen}");
    let (r, _) = app
        .hits
        .zones
        .iter()
        .find(|(_, h)| *h == Hit::Message)
        .unwrap();
    let r = *r;
    let lines: Vec<_> = screen.lines().collect();
    assert_eq!(
        lines[r.y as usize].chars().nth(r.x as usize + 20),
        Some('|')
    );
    app.wt.message.set_text(&format!("{}界Z", "a".repeat(20)));
    let screen = render(&mut app, 80, 24);
    assert!(
        screen.contains(&format!("{}界Z", "a".repeat(20))),
        "{screen}"
    );
    app.wt.message.left = 10;
    app.wt.message.set_text("short\n");
    app.wt.message.left = 10;
    let screen = render(&mut app, 80, 24);
    let lines: Vec<_> = screen.lines().collect();
    assert_eq!(
        lines[r.y as usize].chars().nth(r.x as usize + 10),
        Some('|')
    );
    app.wt.message.set_text(&format!("{}界Z", "a".repeat(19)));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
    assert_eq!(terminal.backend().buffer()[(r.x + 19, r.y)].symbol(), "界");
    assert_ne!(terminal.backend().buffer()[(r.x + 20, r.y)].symbol(), "|");
    app.wt.message.set_text("e\u{301}👩‍💻Z");
    terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
    assert_eq!(terminal.backend().buffer()[(r.x, r.y)].symbol(), "e\u{301}");
    assert_eq!(terminal.backend().buffer()[(r.x + 1, r.y)].symbol(), "👩‍💻");
    assert_eq!(terminal.backend().buffer()[(r.x + 3, r.y)].symbol(), "Z");
}

#[test]
fn long_dialog_manual_scroll_stays_put_and_buttons_remain_clickable() {
    let dir = fixture();
    let mut app = app(dir.path(), true);
    let mut fields: Vec<_> = (0..30)
        .map(|i| {
            Field::text(
                &format!("Field {i} - a descriptive label"),
                &format!("value {i}"),
            )
        })
        .collect();
    fields.push(Field::area("Multiline text", "first\nsecond\nthird", 30));
    app.dialogs.push(
        Dialog::new("Long form", Kind::Form(FormKind::Preferences))
            .with_fields(fields)
            .with_buttons(&[
                "Save changes",
                "Apply without closing",
                "Restore original settings",
                "Cancel",
            ]),
    );
    render(&mut app, 80, 24);
    let content = app
        .hits
        .zones
        .iter()
        .find(|(_, h)| *h == Hit::Pane(git_twin::ui::hit::PaneId::Dialog))
        .unwrap()
        .0;
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: content.x,
        row: content.y,
        modifiers: KeyModifiers::NONE,
    });
    let wheel_scroll = app.dialogs.last().unwrap().scroll;
    assert!(wheel_scroll > 0);
    render(&mut app, 80, 24);
    assert_eq!(app.dialogs.last().unwrap().scroll, wheel_scroll);
    app.on_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::ALT));
    let before = app.dialogs.last().unwrap().scroll;
    assert!(before > 0);
    let screen = render(&mut app, 80, 24);
    assert_eq!(app.dialogs.last().unwrap().scroll, before);
    for label in [
        "[ Save changes ]",
        "[ Apply without closing ]",
        "[ Restore original settings ]",
        "[ Cancel ]",
    ] {
        assert!(screen.contains(label), "{screen}");
    }
    for b in 0..4 {
        let (r, h) = app
            .hits
            .zones
            .iter()
            .find(|(_, h)| *h == Hit::DlgButton(b))
            .unwrap();
        assert_eq!(app.hits.at(r.x, r.y).unwrap().1, h);
    }
    app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::ALT));
    render(&mut app, 80, 24);
    let scroll = app.dialogs.last().unwrap().scroll;
    render(&mut app, 120, 35);
    assert!(app.dialogs.last().unwrap().scroll > 0);
    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    let screen = render(&mut app, 80, 24);
    assert!(app.dialogs.last().unwrap().scroll < scroll);
    assert!(screen.contains("Field 1"), "{screen}");
    app.dialogs.last_mut().unwrap().focus = 30;
    let screen = render(&mut app, 80, 24);
    assert!(screen.contains("Multiline text"), "{screen}");
    let (r, _) = app
        .hits
        .zones
        .iter()
        .find(|(_, h)| *h == Hit::DlgArea(30, 1))
        .unwrap();
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: r.x + 3,
        row: r.y,
        modifiers: KeyModifiers::NONE,
    });
    if let Field::Area { area, .. } = &app.dialogs.last().unwrap().fields[30] {
        assert_eq!((area.cursor.row, area.cursor.col), (1, 3));
    } else {
        panic!("missing area");
    }
}

#[test]
fn fine_selection_copies_graphemes_and_never_becomes_a_partial_patch() {
    use git_twin::app::{
        diffview::{DKind, DLine, DiffView},
        worktree::WFocus,
    };
    let mut d = DiffView {
        width: 80,
        height: 10,
        ..Default::default()
    };
    d.lines = vec![
        DLine::plain(DKind::Ctx, "Ae\u{301}界👩‍💻Z"),
        DLine::plain(DKind::Add, "+second line"),
    ];
    d.text_click(0, 1, false);
    d.text_click(0, 6, true);
    assert_eq!(d.selected_text(), "e\u{301}界👩‍💻");
    d.text_click(1, 4, true);
    assert_eq!(d.selected_text(), "e\u{301}界👩‍💻Z\n+sec");
    d.text_click(1, 4, false);
    d.text_click(0, 1, true);
    assert_eq!(d.selected_text(), "e\u{301}界👩‍💻Z\n+sec");
    d.set_cursor(0, false);
    d.text_move(true);
    d.text_move(true);
    assert_eq!(d.selected_text(), "Ae\u{301}");
    d.left = 2;
    d.text_click(0, 0, false);
    d.text_click(0, 2, true);
    assert_eq!(d.selected_text(), "界");
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.switch_view(ViewKind::Worktree);
    app.wt.focus = WFocus::Diff;
    app.wt.diff = d;
    app.dispatch(Action::Copy);
    assert_eq!(app.clipboard, "界");
    app.dispatch(Action::StageLines);
    assert!(app.status.0.contains("copying only"));
    assert!(app.wt.diff.selected_lines().is_none());
    assert!(!app.workers.writing());
    app.wt.diff.left = 0;
    render(&mut app, 80, 24);
    let r = app
        .hits
        .zones
        .iter()
        .find(|(_, h)| *h == Hit::DiffLine(git_twin::ui::hit::PaneId::WDiff, 0))
        .unwrap()
        .0;
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: r.x + 1,
        row: r.y,
        modifiers: KeyModifiers::NONE,
    });
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: r.x + 6,
        row: r.y,
        modifiers: KeyModifiers::NONE,
    });
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: r.x + 6,
        row: r.y,
        modifiers: KeyModifiers::NONE,
    });
    app.dispatch(Action::Copy);
    assert_eq!(app.clipboard, "e\u{301}界👩‍💻");
    app.cfg.colors = git_twin::config::ColorMode::Truecolor;
    app.theme = git_twin::ui::theme::Theme::new(&app.cfg);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
    let b = terminal.backend().buffer();
    assert_eq!(b[(r.x + 1, r.y)].bg, app.theme.sel_bg);
    assert_ne!(b[(r.x, r.y)].bg, app.theme.sel_bg);
    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
    assert!(app.wt.diff.text_range().is_none());
    assert!(app.wt.diff.anchor.is_some());
}

#[test]
fn persistence_errors_and_quit_confirmation_are_recoverable() {
    use git_twin::app::dialog::DialogResult;
    use git_twin::config;
    if std::env::var_os("GITT_PERSISTENCE_TEST").is_none() {
        let state = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "persistence_errors_and_quit_confirmation_are_recoverable",
                "--nocapture",
            ])
            .env("GITT_PERSISTENCE_TEST", "1")
            .env("XDG_STATE_HOME", state.path().join("state"))
            .env("XDG_CONFIG_HOME", state.path().join("config"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout).to_string()
                + &String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.wt.message.set_text("Keep this draft");
    app.request_quit();
    assert!(!app.quit);
    assert_eq!(
        app.dialogs.last().unwrap().buttons[0],
        "Save draft and quit"
    );
    assert_eq!(
        app.dialogs.last().unwrap().buttons[1],
        "Quit without saving"
    );
    app.on_dialog_result(DialogResult::Button(2));
    assert!(!app.quit);
    assert_eq!(app.wt.message.text(), "Keep this draft");
    let state_dir = config::state_dir().unwrap();
    fs::create_dir_all(state_dir.parent().unwrap()).unwrap();
    fs::write(&state_dir, "not a directory").unwrap();
    app.request_quit();
    app.on_dialog_result(DialogResult::Button(0));
    assert!(!app.quit);
    assert!(app.wt.message.modified);
    assert!(app.dialogs.last().unwrap().title.contains("cannot save"));
    fs::remove_file(&state_dir).unwrap();
    app.dialogs.clear();
    app.save_draft();
    assert!(!app.wt.message.modified);
    assert_eq!(
        config::load_draft(&app.repo.git_dir).unwrap().as_deref(),
        Some("Keep this draft")
    );
    app.request_quit();
    app.on_dialog_result(DialogResult::Button(0));
    assert!(app.quit);
    assert!(config::state_file().unwrap().exists());
    app.quit = false;
    app.cfg.confirm_quit = false;
    app.request_quit();
    assert!(app.quit);
    app.quit = false;
    let draft = config::draft_path(&app.repo.git_dir).unwrap();
    fs::remove_file(&draft).unwrap();
    fs::create_dir(&draft).unwrap();
    app.wt.message.set_text("");
    app.save_draft();
    assert!(app.wt.message.modified, "failed removal must be retried");
    assert!(app.persistence_error.is_some());
    assert!(config::load_draft(&app.repo.git_dir).is_err());
    fs::remove_dir(&draft).unwrap();
    app.save_draft();
    assert!(!app.wt.message.modified);
    fs::write(config::state_file().unwrap(), "not = [valid").unwrap();
    assert!(State::load().is_err());
    let config_file = config::config_file().unwrap();
    fs::create_dir_all(config_file.parent().unwrap()).unwrap();
    fs::create_dir(&config_file).unwrap();
    assert!(app.cfg.save().is_err());
    app.open_preferences();
    app.on_dialog_result(DialogResult::Button(0));
    assert!(app.dialogs.last().unwrap().title.contains("not saved"));
    app.dialogs.clear();
    app.quit = false;
    app.cfg.confirm_quit = true;
    app.wt.message.set_text("Discard me");
    app.save_draft();
    assert_eq!(
        config::load_draft(&app.repo.git_dir).unwrap().as_deref(),
        Some("Discard me")
    );
    app.request_quit();
    app.on_dialog_result(DialogResult::Button(1));
    assert!(app.quit, "Quit without saving must still quit");
    assert_eq!(config::load_draft(&app.repo.git_dir).unwrap(), None);
}

#[test]
fn both_launchers_and_about_texts_use_the_new_names() {
    let dir = tempfile::tempdir().unwrap();
    for (binary, name) in [
        (env!("CARGO_BIN_EXE_gitt"), "gitt"),
        (env!("CARGO_BIN_EXE_git-tui"), "git tui"),
    ] {
        let output = Command::new(binary)
            .arg("--help")
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("Usage: {name}")));
    }
    let output = Command::new("git")
        .args(["tui", "--version"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                std::path::Path::new(env!("CARGO_BIN_EXE_git-tui"))
                    .parent()
                    .unwrap()
                    .display(),
                std::env::var("PATH").unwrap()
            ),
        )
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let history = git_twin::app::history::about_text("gitk");
    let worktree = git_twin::app::history::about_text("git gui");
    assert!(history.contains("Git Twin") && history.contains("gitt"));
    assert!(worktree.contains("Git TUI") && worktree.contains("git tui"));
}

#[test]
fn clicking_a_reference_in_list_refs_shows_it_in_history() {
    let dir = fixture();
    git(dir.path(), &["tag", "v0", "HEAD"]);
    git(dir.path(), &["commit", "-qm", "Second"]);
    let first = git(dir.path(), &["rev-parse", "HEAD~1"]).trim().to_string();
    git(dir.path(), &["tag", "v-first", &first]);
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    settle(&mut app);
    app.open_list_refs();
    render(&mut app, 100, 40);
    let row = app
        .hits
        .zones
        .iter()
        .find(|(_, h)| matches!(h, Hit::DlgListRow(0, i) if app.dialogs.last().is_some_and(|d| matches!(&d.fields[0], git_twin::app::dialog::Field::List { items, .. } if items[*i].ends_with("v-first")))))
        .unwrap()
        .0;
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: row.x + 1,
        row: row.y,
        modifiers: KeyModifiers::NONE,
    });
    settle(&mut app);
    assert_eq!(app.hist.selected_oid().map(|o| o.to_string()), Some(first));
    assert!(
        app.dialogs.last().is_some(),
        "list stays open after a click"
    );
}

#[test]
fn checkout_moves_head_marker_but_keeps_history_rows() {
    let dir = fixture();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "Second"]);
    git(dir.path(), &["branch", "feature", "HEAD~1"]);
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    settle(&mut app);
    let rows_before: Vec<_> = app.hist.commits.iter().map(|c| c.oid).collect();
    let feature = git(dir.path(), &["rev-parse", "feature"])
        .trim()
        .to_string();
    app.dispatch(Action::CheckoutRef(b"refs/heads/feature".to_vec()));
    settle(&mut app);
    settle(&mut app);
    assert_eq!(
        app.hist.commits.iter().map(|c| c.oid).collect::<Vec<_>>(),
        rows_before,
        "history rows must stay as they were"
    );
    assert_eq!(app.refs.head.label(), "feature");
    let feature_ref = app
        .refs
        .for_commit(&git_twin::domain::oid::Oid::from_hex(&feature).unwrap())
        .find(|r| r.short == "feature")
        .unwrap();
    assert!(app.refs.is_head_ref(feature_ref));
}

#[test]
fn history_keeps_starting_head_after_checkout_until_reload() {
    let dir = fixture();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "Second on main"]);
    git(dir.path(), &["checkout", "-q", "-b", "side", "HEAD~1"]);
    git(
        dir.path(),
        &["commit", "-q", "--allow-empty", "-m", "Only on side"],
    );
    git(dir.path(), &["checkout", "-q", "main"]);
    let main = git(dir.path(), &["rev-parse", "main"]).trim().to_string();
    let side = git(dir.path(), &["rev-parse", "side"]).trim().to_string();
    let has = |app: &App, oid: &str| app.hist.commits.iter().any(|c| c.oid.to_hex() == oid);
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    assert!(has(&app, &main) && !has(&app, &side));

    app.dispatch(Action::CheckoutRef(b"refs/heads/side".to_vec()));
    settle(&mut app);
    settle(&mut app);
    assert!(has(&app, &main), "starting branch stays visible");
    assert!(has(&app, &side));

    app.dispatch(Action::Update);
    settle(&mut app);
    settle(&mut app);
    assert!(has(&app, &main), "update keeps the starting branch");

    app.dispatch(Action::Reload);
    settle(&mut app);
    settle(&mut app);
    assert!(!has(&app, &main), "reload resets to the current branch");
    assert!(has(&app, &side));
}

#[test]
fn reset_dialog_lists_every_mode() {
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    settle(&mut app);
    let oid = app
        .hist
        .commits
        .iter()
        .map(|c| c.oid)
        .find(|o| !o.is_zero())
        .unwrap();
    app.open_reset(oid);
    let screen = render(&mut app, 120, 40);
    for mode in ["Soft:", "Mixed:", "Hard:"] {
        assert!(screen.contains(mode), "reset dialog must show {mode}");
    }
}

#[test]
fn stash_changes_creates_stash_and_refreshes_refs() {
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    settle(&mut app);
    app.dispatch(Action::StashChanges);
    settle(&mut app);
    settle(&mut app);
    assert!(
        !git(dir.path(), &["stash", "list"]).trim().is_empty(),
        "a stash entry must exist"
    );
    assert!(
        app.refs
            .refs
            .iter()
            .any(|r| r.kind == git_twin::domain::refs::RefKind::Stash),
        "history refs must show the new stash"
    );
}

#[test]
fn command_palette_opens_filters_and_runs_a_command() {
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    app.on_key(KeyEvent::new(KeyCode::F(12), KeyModifiers::NONE));
    assert!(
        matches!(
            app.dialogs.last().map(|d| &d.kind),
            Some(git_twin::app::dialog::Kind::Form(
                git_twin::app::dialog::FormKind::CommandPalette
            ))
        ),
        "Ctrl+Shift+P must open the command palette"
    );
    for c in "key bind".chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    settle(&mut app);
    assert!(
        app.dialogs.iter().any(|d| d.title.contains("Key")),
        "selecting Key bindings must run it"
    );
}

#[test]
fn command_palette_lists_only_enabled_commands_with_menu_path() {
    let dir = fixture();
    let app = app(dir.path(), false);
    let entries = app.palette_entries();
    assert!(
        entries
            .iter()
            .any(|(l, a, on)| l.starts_with("File > Update") && *a == Action::Update && *on),
        "{entries:?}"
    );
    assert!(
        entries.iter().any(
            |(l, a, _)| l.starts_with("Help > Command palette") && *a == Action::CommandPalette
        )
    );
}

#[test]
fn command_palette_keeps_unavailable_commands_visible_but_refuses_them() {
    let dir = fixture();
    let mut app = app(dir.path(), false);
    let entries = app.palette_entries();
    let unavailable = entries.iter().find(|(_, _, on)| !on).cloned();
    let Some((label, action, _)) = unavailable else {
        panic!("expected at least one unavailable entry: {entries:?}");
    };
    app.palette_recent.clear();
    let sorted = app.palette_sorted();
    let (_, _, enabled) = sorted
        .iter()
        .find(|(l, _, _)| *l == label)
        .expect("unavailable entry is listed");
    assert!(!enabled);
    assert!(!app.palette_recent.contains(&action));
}

#[test]
fn command_palette_puts_recent_commands_first() {
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.palette_recent = vec![Action::CommandPalette];
    let sorted = app.palette_sorted();
    assert_eq!(sorted[0].1, Action::CommandPalette);
}

#[test]
fn command_palette_includes_context_menu_of_focused_selection() {
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    settle(&mut app);
    app.hist.sel = Some(0);
    let entries = app.palette_entries();
    assert!(
        entries.iter().any(|(l, _, _)| l.starts_with("Selection")),
        "context menu entries must be listed: {entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|(l, _, _)| l.starts_with("File > Update")),
        "menu-bar entries must still be listed"
    );
}

#[test]
fn cut_graph_lines_end_in_arrows_that_lead_to_each_other() {
    let dir = fixture();
    git(dir.path(), &["commit", "-qm", "Base"]);
    git(dir.path(), &["branch", "side"]);
    for k in 0..8 {
        git(
            dir.path(),
            &["commit", "-q", "--allow-empty", "-m", &format!("Main {k}")],
        );
    }
    git(dir.path(), &["checkout", "-q", "side"]);
    git(
        dir.path(),
        &["commit", "-q", "--allow-empty", "-m", "Side work"],
    );
    git(dir.path(), &["checkout", "-q", "main"]);
    git(
        dir.path(),
        &["merge", "-q", "--no-ff", "-m", "Merge side", "side"],
    );
    for (icons, connectors) in [
        (IconMode::Ascii, false),
        (IconMode::Unicode, false),
        (IconMode::Unicode, true),
    ] {
        let mut app = App::new(
            repo::discover(dir.path()).unwrap(),
            Config {
                icons,
                graph_cut_after: 3,
                graph_connectors: connectors,
                ..Config::default()
            },
            State::default(),
            Options {
                view: Some(ViewKind::History),
                read_only: false,
                mouse: Some(true),
                all_refs: false,
                paths: Vec::new(),
            },
        );
        app.start();
        settle(&mut app);
        let show = |app: &mut App| {
            let screen = render(app, 120, 80);
            if std::env::var_os("SHOW_GRAPH").is_some() {
                println!("{}", screen.lines().take(32).collect::<Vec<_>>().join("\n"));
            }
        };
        show(&mut app);
        let click = |app: &mut App, hit: fn(&Hit) -> bool| {
            let (r, h) = app.hits.zones.iter().find(|(_, h)| hit(h)).unwrap().clone();
            app.on_mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: r.x,
                row: r.y,
                modifiers: KeyModifiers::NONE,
            });
            app.on_mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: r.x,
                row: r.y,
                modifiers: KeyModifiers::NONE,
            });
            h
        };
        let Hit::CutArrow(row, lane) = click(&mut app, |h| matches!(h, Hit::CutArrow(..))) else {
            unreachable!()
        };
        let target = app.hist.cut_target[&(row, lane)];
        let node = app.hist.index[&target];
        assert_eq!(app.hist.sel, Some(node));
        show(&mut app);
        let Hit::ResumeArrow { node: up, .. } =
            click(&mut app, |h| matches!(h, Hit::ResumeArrow { .. }))
        else {
            unreachable!()
        };
        assert_eq!(up, node);
        assert_eq!(app.hist.sel, Some(row));
    }
}

#[test]
fn open_in_editor_runs_configured_editor_on_the_working_copy_file() {
    let dir = fixture();
    let tools = tempfile::tempdir().unwrap();
    let script = tools.path().join("fake-editor");
    let record = tools.path().join("args.txt");
    fs::write(
        &script,
        format!("#!/bin/sh\necho \"$@\" > '{}'\n", record.display()),
    )
    .unwrap();
    fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    app.cfg.editor = format!("{} --reuse-window", script.display());

    app.dispatch(Action::OpenInEditor(b"hello.txt".to_vec()));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !record.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let args = fs::read_to_string(&record).expect("editor was not started");
    assert_eq!(
        args.trim(),
        format!("--reuse-window {}", dir.path().join("hello.txt").display())
    );
    assert!(app.status.0.is_empty(), "{}", app.status.0);

    app.dispatch(Action::OpenInEditor(b"missing.txt".to_vec()));
    assert!(
        app.status.0.contains("not in the working copy"),
        "{}",
        app.status.0
    );

    app.cfg.editor = String::new();
    app.dispatch(Action::OpenInEditor(b"hello.txt".to_vec()));
    assert!(app.status.0.contains("No editor set"), "{}", app.status.0);

    app.cfg.editor = "/nonexistent/editor-xyz".into();
    app.dispatch(Action::OpenInEditor(b"hello.txt".to_vec()));
    assert!(app.status.0.contains("Open in editor"), "{}", app.status.0);
}

#[test]
fn editor_setting_defaults_to_code_and_is_editable_in_preferences() {
    assert_eq!(Config::default().editor, "code");
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    app.open_preferences();
    let d = app.dialogs.last().unwrap();
    assert_eq!(d.text(16), "code");
}

#[test]
fn open_in_editor_hands_terminal_editors_to_the_tui_suspend() {
    let dir = fixture();
    let mut app = app(dir.path(), true);
    app.start();
    settle(&mut app);
    app.cfg.editor = "nvim +10".into();

    app.dispatch(Action::OpenInEditor(b"hello.txt".to_vec()));
    match &app.handoff {
        Some(Handoff::OpenFile {
            program,
            args,
            file,
        }) => {
            assert_eq!(program, "nvim");
            assert_eq!(args, &["+10".to_string()]);
            assert_eq!(file, &dir.path().join("hello.txt"));
        }
        other => panic!("expected OpenFile handoff, got {other:?}"),
    }
}

#[test]
fn clicking_status_message_opens_timestamped_history() {
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.set_status("First message", StatusTone::Info);
    app.set_status("Hello history", StatusTone::Info);
    let screen = render(&mut app, 120, 35);
    let bar = screen.lines().last().unwrap();
    let x = bar.find("Hello history").unwrap() as u16;
    assert!(
        app.hits.zones.iter().any(|(_, h)| *h == Hit::StatusMsg),
        "status message is not clickable"
    );
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x + 2,
        row: 34,
        modifiers: KeyModifiers::NONE,
    });
    let dialog = app.dialogs.last().expect("history dialog");
    assert_eq!(dialog.title, "Messages");
    let lines: Vec<&str> = dialog.body.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].ends_with("  First message"), "{lines:?}");
    assert!(lines[1].ends_with("  Hello history"), "{lines:?}");
    assert_eq!(lines[0].len(), "HH:MM:SS  Hello history".len());
}

#[test]
fn message_history_scrolls_with_keys_and_wheel() {
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    let dir = fixture();
    let mut app = app(dir.path(), false);
    for i in 0..60 {
        app.set_status(format!("Message {i:02}"), StatusTone::Info);
    }
    render(&mut app, 120, 35);
    let x = {
        let screen = render(&mut app, 120, 35);
        screen.lines().last().unwrap().find("Message 59").unwrap() as u16
    };
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x + 1,
        row: 34,
        modifiers: KeyModifiers::NONE,
    });
    let screen = render(&mut app, 120, 35);
    assert!(screen.contains("  Message 59 "), "{screen}");
    assert!(!screen.contains("  Message 00 "), "{screen}");

    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    app.dialogs.last_mut().unwrap().on_key(key(KeyCode::Home));
    let screen = render(&mut app, 120, 35);
    assert!(screen.contains("  Message 00 "), "{screen}");
    assert!(!screen.contains("  Message 59 "), "{screen}");

    app.dialogs.last_mut().unwrap().on_key(key(KeyCode::Down));
    render(&mut app, 120, 35);
    assert_eq!(app.dialogs.last().unwrap().scroll, 1);
    app.dialogs.last_mut().unwrap().on_key(key(KeyCode::Up));
    render(&mut app, 120, 35);
    assert_eq!(app.dialogs.last().unwrap().scroll, 0);

    app.dialogs.last_mut().unwrap().on_key(key(KeyCode::End));
    let screen = render(&mut app, 120, 35);
    assert!(screen.contains("  Message 59 "), "{screen}");
    assert!(!screen.contains("  Message 00 "), "{screen}");
}

#[test]
fn hovering_a_menu_item_highlights_it() {
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.open_menu(0);
    render(&mut app, 120, 35);
    let items: Vec<(Rect, usize)> = app
        .hits
        .zones
        .iter()
        .filter_map(|(r, h)| match h {
            Hit::MenuItem { depth: 0, idx } => Some((*r, *idx)),
            _ => None,
        })
        .collect();
    let level = &app.menu.as_ref().unwrap().levels[0];
    let (r, idx) = *items
        .iter()
        .rev()
        .find(|(_, i)| *i != level.sel && level.items[*i].selectable())
        .expect("another selectable item");
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: r.x + 1,
        row: r.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.menu.as_ref().unwrap().levels[0].sel, idx);
}

#[test]
fn tab_moves_focus_out_of_history_text_entries() {
    use git_twin::app::history::HFocus;
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    app.hist.focus = HFocus::List;
    let order = [HFocus::Sha, HFocus::Find, HFocus::Search, HFocus::Diff];
    for expected in order {
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            app.hist.focus, expected,
            "Tab must leave the previous field"
        );
    }
}
