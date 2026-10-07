//! Render both views without taking over the terminal or changing the repository.
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use git_twin::{
    app::{App, Options},
    config::{Config, IconMode, State, ViewKind},
    git::repo,
    ui,
};
use ratatui::{Terminal, backend::TestBackend};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let argument = std::env::args_os().nth(1);
    let demo = if argument.as_deref() == Some(std::ffi::OsStr::new("--demo")) {
        Some(demo()?)
    } else {
        None
    };
    let path = if let Some(dir) = &demo {
        dir.path().to_path_buf()
    } else {
        argument
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?)
    };
    let mut app = App::new(
        repo::discover(&path).map_err(|e| e.to_string())?,
        Config {
            icons: IconMode::Ascii,
            ..Config::default()
        },
        State::default(),
        Options {
            view: Some(ViewKind::History),
            read_only: true,
            mouse: Some(false),
            all_refs: false,
        },
    );
    app.start();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut idle = Instant::now();
    loop {
        while let Ok(msg) = app.workers.rx.try_recv() {
            app.on_msg(msg);
        }
        app.tick();
        if app.busy() {
            idle = Instant::now();
        }
        if Instant::now().duration_since(idle) > Duration::from_millis(250) {
            break;
        }
        if Instant::now() >= deadline {
            return Err("Timed out loading repository".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut terminal = Terminal::new(TestBackend::new(120, 35))?;
    for view in [ViewKind::History, ViewKind::Worktree] {
        if app.is_bare() && view == ViewKind::Worktree {
            continue;
        }
        app.switch_view(view);
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        println!("\n--- {view:?} ---");
        let buffer = terminal.backend().buffer();
        for y in 0..35 {
            let line: String = (0..120).map(|x| buffer[(x, y)].symbol()).collect();
            println!("{}", line.trim_end());
        }
    }
    Ok(())
}

fn demo() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let dir = tempfile::Builder::new()
        .prefix("git-twin-demo-")
        .tempdir()?;
    let git = |args: &[&str]| -> Result<(), Box<dyn std::error::Error>> {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Demo")
            .env("GIT_AUTHOR_EMAIL", "demo@example.invalid")
            .env("GIT_COMMITTER_NAME", "Demo")
            .env("GIT_COMMITTER_EMAIL", "demo@example.invalid")
            .output()?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
        }
        Ok(())
    };
    git(&["init", "-q", "-b", "main"])?;
    git(&["config", "core.hooksPath", "/dev/null"])?;
    std::fs::write(
        dir.path().join("hello.rs"),
        "fn main() {\n    println!(\"Hello, Git Twin!\");\n}\n",
    )?;
    git(&["add", "."])?;
    git(&["commit", "-qm", "Create greeting application"])?;
    git(&["checkout", "-qb", "feature/docs"])?;
    std::fs::write(
        dir.path().join("README.md"),
        "# Greeting\n\nRun with cargo run.\n",
    )?;
    git(&["add", "."])?;
    git(&["commit", "-qm", "Document the greeting"])?;
    git(&["checkout", "-q", "main"])?;
    std::fs::write(dir.path().join("settings.toml"), "theme = \"dark\"\n")?;
    git(&["add", "."])?;
    git(&["commit", "-qm", "Add theme settings"])?;
    git(&[
        "merge",
        "--no-ff",
        "-qm",
        "Merge documentation branch",
        "feature/docs",
    ])?;
    std::fs::write(
        dir.path().join("hello.rs"),
        "fn main() {\n    println!(\"Hello, Git Twin!\");\n    println!(\"History and worktree in your terminal.\");\n}\n",
    )?;
    std::fs::write(
        dir.path().join("settings.toml"),
        "theme = \"dark\"\nmouse = true\n",
    )?;
    git(&["add", "settings.toml"])?;
    Ok(dir)
}
