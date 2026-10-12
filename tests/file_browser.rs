//! End-to-end check of the built-in file browser: picking a file opens a
//! syntax-highlighted, horizontally-scrollable viewer with an "Open in
//! editor" button.
use std::{fs, process::Command, time::Duration, time::Instant};

use git_twin::{
    app::{
        App, Options,
        action::Action,
        dialog::{DialogResult, Field, FormKind, Kind, Tone},
    },
    config::{Config, IconMode, State, ViewKind},
    git::repo,
};

fn git(path: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Demo")
        .env("GIT_AUTHOR_EMAIL", "demo@example.invalid")
        .env("GIT_COMMITTER_NAME", "Demo")
        .env("GIT_COMMITTER_EMAIL", "demo@example.invalid")
        .status()
        .unwrap();
    assert!(status.success(), "{args:?} failed");
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    git(dir.path(), &["config", "core.hooksPath", "/dev/null"]);
    fs::write(
        dir.path().join("demo.rs"),
        "// a comment\nfn main() {\n    let answer = 42;\n    println!(\"hi\");\n}\n",
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "Add demo.rs"]);
    dir
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
        assert!(Instant::now() < deadline, "load timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn browsing_a_file_shows_highlighted_open_in_editor_view() {
    let dir = fixture();
    let mut app = App::new(
        repo::discover(dir.path()).unwrap(),
        Config {
            icons: IconMode::Ascii,
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
    settle(&mut app);

    app.dispatch(Action::BrowseFiles(None));
    settle(&mut app);
    let d = app.dialogs.last_mut().expect("browser dialog open");
    assert!(matches!(d.kind, Kind::Form(FormKind::Browser(_))));
    if let Field::List { items, sel, .. } = &mut d.fields[0] {
        *sel = items
            .iter()
            .position(|i| i == "demo.rs")
            .expect("demo.rs listed");
    } else {
        panic!("expected a file list field");
    }
    app.on_dialog_result(DialogResult::Button(0)); // "Open"
    settle(&mut app);

    let d = app.dialogs.last().expect("file viewer open");
    assert!(
        matches!(&d.kind, Kind::FileView(p) if p == b"demo.rs"),
        "expected the file viewer, got a different dialog"
    );
    assert_eq!(d.buttons, vec!["Open in editor", "Close"]);
    assert!(!d.code.is_empty(), "file content should be loaded");

    let tones: Vec<Tone> = d
        .code
        .iter()
        .flat_map(|line| line.iter().map(|(_, t)| *t))
        .collect();
    assert!(tones.contains(&Tone::Keyword), "fn/let should be keywords");
    assert!(
        tones.contains(&Tone::Str),
        "the string literal should be coloured"
    );
    assert!(
        tones.contains(&Tone::Comment),
        "the comment should be coloured"
    );
    assert!(tones.contains(&Tone::Number), "42 should be coloured");
}
