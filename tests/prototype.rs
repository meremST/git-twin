use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use git_twin::{
    app::{
        App, Options,
        action::{Action, FileList},
        dialog::{Dialog, Field, FormKind, Kind},
    },
    config::{Config, IconMode, State, ViewKind},
    git::repo,
    ui::{self, hit::Hit},
};
use ratatui::{Terminal, backend::TestBackend};

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
        },
    )
}

fn settle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut idle = Instant::now();
    loop {
        while let Ok(msg) = app.workers.rx.try_recv() {
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
    render(&mut app, 80, 24);
    for (r, hit) in &app.hits.zones {
        if matches!(hit, Hit::MenuTitle(_) | Hit::ViewTab(_)) {
            assert_eq!(
                app.hits.at(r.x, r.y).unwrap().1,
                hit,
                "overlapping menu and view tabs"
            );
        }
    }
    for (w, h) in [(80, 24), (120, 35), (200, 60), (79, 23), (1, 1), (0, 0)] {
        for ratio in [0, 100, 500, 900, u16::MAX] {
            app.state.splits.worktree_left = ratio;
            app.state.splits.worktree_diff = ratio;
            app.state.splits.worktree_files = ratio;
            render(&mut app, w, h);
            app.switch_view(ViewKind::History);
            app.state.splits.history_top = ratio;
            app.state.splits.history_bottom_left = ratio;
            render(&mut app, w, h);
            app.switch_view(ViewKind::Worktree);
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
fn cli_commands_do_not_need_a_repository() {
    let dir = tempfile::tempdir().unwrap();
    for args in [&["--help"][..], &["--version"], &["doctor"]] {
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
    );
    check_terminal_launcher(
        env!("CARGO_BIN_EXE_git-tui"),
        "Commit Message",
        "Initial greeting",
        "history",
    );
}

fn check_terminal_launcher(binary: &str, first: &str, second: &str, previous_view: &str) {
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
            rows: 35,
            cols: 120,
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
    let mut parser = vt100::Parser::new(35, 120, 0);
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
    use git_twin::ui::hit::PaneId;
    let dir = fixture();
    let mut app = app(dir.path(), false);
    app.start();
    settle(&mut app);
    app.switch_view(ViewKind::Worktree);
    let screen = render(&mut app, 80, 24);
    assert!(screen.contains("Compact:"), "{screen}");
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
        let screen = render(&mut app, 80, 24);
        assert_eq!(app.focused_pane(), id);
        assert!(app.hits.zones.iter().any(|(_, h)| *h == Hit::Pane(id)));
        if id == PaneId::WMessage {
            assert!(screen.contains("[Amend Last Commit]"), "{screen}");
        }
    }
    let screen = render(&mut app, 120, 35);
    assert!(!screen.contains("Compact:"));
    for label in ["Staged Changes", "Unstaged Changes", "Commit Message"] {
        assert!(screen.contains(label));
    }
    app.switch_view(ViewKind::History);
    app.hist.focus = git_twin::app::history::HFocus::Sha;
    assert!(render(&mut app, 80, 24).contains("SHA1:"));
    app.hist.focus = git_twin::app::history::HFocus::Search;
    let screen = render(&mut app, 80, 24);
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
    app.on_dialog_result(DialogResult::Button(1));
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
