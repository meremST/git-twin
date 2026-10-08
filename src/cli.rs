use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

use crate::{
    app::{App, Handoff, Options, StatusTone, base64},
    config::{self, ColorMode, ColorScheme, Config, IconMode, State, ThemeName, ViewKind},
    domain::sanitize::sanitize,
    git::{
        ops, repo,
        runner::{Opts, os},
    },
    ui,
};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use crossterm::{
    cursor::{Hide, Show},
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyModifiers, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{Terminal, backend::CrosstermBackend};

#[derive(Parser)]
#[command(
    version,
    about = "Git history and working-tree UI, inspired by gitk and git gui"
)]
struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    repo: Option<PathBuf>,
    #[arg(long, value_enum)]
    view: Option<ViewKind>,
    /// Show history for all refs (branches, tags, remotes), like `gitk --all`.
    #[arg(long)]
    all: bool,
    #[arg(long)]
    read_only: bool,
    #[arg(long)]
    no_mouse: bool,
    #[arg(long, value_enum)]
    icons: Option<IconMode>,
    #[arg(long, value_enum)]
    theme: Option<ThemeName>,
    /// Color scheme for dark/light mode (ignored in terminal mode).
    #[arg(long, value_enum)]
    color_scheme: Option<ColorScheme>,
    #[arg(long, value_enum)]
    colors: Option<ColorMode>,
    #[command(subcommand)]
    command: Option<Commands>,
    /// Limit history to commits touching these paths, like `gitk PATH...`
    /// (relative to the current directory; use `--` before a path named
    /// like a subcommand).
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Report local prerequisites and terminal capabilities (no network).
    Doctor,
}

struct TerminalGuard {
    keyboard: bool,
}

impl TerminalGuard {
    fn enter(mouse: bool) -> io::Result<Self> {
        let mut guard = Self { keyboard: false };
        terminal::enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            terminal::Clear(terminal::ClearType::All),
            crossterm::cursor::MoveTo(0, 0),
            EnableBracketedPaste,
            Hide
        )?;
        if mouse {
            execute!(io::stdout(), EnableMouseCapture)?;
        }
        if terminal::supports_keyboard_enhancement().unwrap_or(false) {
            guard.keyboard = true;
            execute!(
                io::stdout(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )?;
        }
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.keyboard {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        restore_terminal();
    }
}

fn restore_terminal() {
    let _ = execute!(
        io::stdout(),
        Show,
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen
    );
    let _ = terminal::disable_raw_mode();
}

pub fn main(default_view: ViewKind, name: &'static str) {
    if let Err(e) = run(default_view, name) {
        eprintln!("{name}: {}", sanitize(&e));
        std::process::exit(1);
    }
}

fn run(default_view: ViewKind, name: &'static str) -> Result<(), String> {
    let cli = Cli::from_arg_matches(&Cli::command().name(name).bin_name(name).get_matches())
        .unwrap_or_else(|e| e.exit());
    if matches!(cli.command, Some(Commands::Doctor)) {
        doctor();
        return Ok(());
    }
    let (major, minor, _, _) = repo::git_version().ok_or("Git was not found in PATH.")?;
    if (major, minor) < repo::MIN_GIT {
        return Err(format!(
            "Git {}.{} or newer is required.",
            repo::MIN_GIT.0,
            repo::MIN_GIT.1
        ));
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let path = cli.repo.unwrap_or_else(|| cwd.clone());
    let repo = repo::discover(&path).map_err(|e| e.to_string())?;
    let paths = cli
        .paths
        .iter()
        .map(|p| repo::workdir_pathspec(&repo, &cwd, p))
        .collect::<Result<Vec<_>, _>>()?;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            "An interactive terminal is required. Use --help or doctor for non-interactive output."
                .into(),
        );
    }
    let (mut cfg, warning) = Config::load();
    if let Some(v) = cli.theme {
        cfg.theme = v;
    }
    if let Some(v) = cli.color_scheme {
        cfg.color_scheme = v;
    }
    if let Some(v) = cli.icons {
        cfg.icons = v;
    }
    if let Some(v) = cli.colors {
        cfg.colors = v;
    }
    let (state, state_warning) = match State::load() {
        Ok(state) => (state, None),
        Err(e) => (State::default(), Some(e)),
    };
    let warning = [warning, state_warning]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    let mut app = App::new(
        repo,
        cfg,
        state,
        Options {
            view: Some(cli.view.unwrap_or(default_view)),
            read_only: cli.read_only,
            mouse: cli.no_mouse.then_some(false),
            all_refs: cli.all,
            paths,
        },
    );
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        old_hook(info);
    }));
    let result = event_loop(&mut app, (!warning.is_empty()).then_some(warning));
    let saved = app.shutdown();
    match (result, saved) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), Ok(())) => Err(e.to_string()),
        (Ok(()), Err(e)) => Err(e),
        (Err(e), Err(save)) => Err(format!("{e}\nCannot save: {save}")),
    }
}

fn event_loop(app: &mut App, warning: Option<String>) -> io::Result<()> {
    let mut guard = TerminalGuard::enter(app.mouse_enabled)?;
    let mut mouse_enabled = app.mouse_enabled;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    app.start();
    if let Some(warning) = warning {
        app.error("Configuration", warning);
    }
    while !app.quit {
        if mouse_enabled != app.mouse_enabled {
            if app.mouse_enabled {
                execute!(io::stdout(), EnableMouseCapture)?;
            } else {
                execute!(io::stdout(), DisableMouseCapture)?;
            }
            mouse_enabled = app.mouse_enabled;
        }
        while let Ok(msg) = app.workers.rx.try_recv() {
            app.on_msg(msg);
        }
        app.tick();
        terminal.draw(|frame| ui::draw(frame, app))?;
        if let Some(s) = app.osc52.take() {
            write!(
                terminal.backend_mut(),
                "\x1b]52;c;{}\x07",
                base64(s.as_bytes())
            )?;
            terminal.backend_mut().flush()?;
        }
        if let Some(handoff) = app.handoff.take() {
            drop(guard);
            let result = run_handoff(app, &handoff);
            if let Err(e) = &result {
                eprintln!("{}", sanitize(e));
            }
            println!("\nPress Enter to return to Git Twin.");
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            guard = TerminalGuard::enter(app.mouse_enabled)?;
            terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
            app.after_handoff(&handoff, result);
            continue;
        }
        if !event::poll(Duration::from_millis(40))? {
            continue;
        }
        let event = event::read()?;
        if app.size.0 < 79 || app.size.1 < 24 {
            if let Event::Key(k) = event
                && k.code == KeyCode::Char('q')
                && k.modifiers.contains(KeyModifiers::CONTROL)
                && !app.workers.writing()
            {
                if app.cfg.confirm_quit && !app.wt.message.text().trim().is_empty() {
                    app.set_status(
                        "Resize to 79 x 24 to confirm quitting with a draft.",
                        StatusTone::Error,
                    );
                } else {
                    app.finish_quit();
                }
            }
            continue;
        }
        match event {
            Event::Key(key) => app.on_key(key),
            Event::Mouse(mouse) => app.on_mouse(mouse),
            Event::Paste(text) => app.paste(&text),
            Event::Resize(..) => app.drag = None,
            _ => {}
        }
    }
    Ok(())
}

fn run_handoff(app: &mut App, h: &Handoff) -> Result<bool, String> {
    match h {
        Handoff::Git { args, label } => {
            ops::handoff(&app.runner, args, Some(label)).map_err(|e| e.to_string())
        }
        Handoff::Shell { cmd, envs, .. } => {
            let envs: Vec<_> = envs.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            ops::handoff_shell(&app.runner, cmd, &envs).map_err(|e| e.to_string())
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
        Handoff::Editor => edit_message(app),
    }
}

fn edit_message(app: &mut App) -> Result<bool, String> {
    // `git var` needs the real editor environment, not the captured-command override.
    let output = app
        .runner
        .command(
            &[os("var"), os("GIT_EDITOR")],
            Opts {
                interactive: true,
                ..Opts::default()
            },
        )
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let editor = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    let mut temp = tempfile::Builder::new()
        .prefix("git-twin-message-")
        .suffix(".txt")
        .tempfile()
        .map_err(|e| e.to_string())?;
    temp.write_all(app.wt.message.text().as_bytes())
        .and_then(|_| temp.flush())
        .map_err(|e| e.to_string())?;
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{} \"$@\"", editor.trim()))
        .arg("git-twin-editor")
        .arg(temp.path())
        .current_dir(&app.runner.cwd)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(temp.path()).map_err(|e| e.to_string())?;
    app.wt.message.set_text(&text);
    app.set_status("Commit message updated.", StatusTone::Info);
    Ok(true)
}

fn doctor() {
    println!("Git Twin {}", env!("CARGO_PKG_VERSION"));
    println!(
        "Git: {}",
        repo::git_version()
            .map(|v| v.3)
            .unwrap_or_else(|| "not found".into())
    );
    println!(
        "Interactive terminal: stdin={}, stdout={}",
        io::stdin().is_terminal(),
        io::stdout().is_terminal()
    );
    for var in ["TERM", "COLORTERM"] {
        println!(
            "{var}: {}",
            sanitize(&std::env::var(var).unwrap_or_else(|_| "(unset)".into()))
        );
    }
    for (label, path) in [
        ("Config", config::config_file()),
        ("State", config::state_file()),
        ("Cache", config::cache_dir()),
    ] {
        println!(
            "{label}: {}",
            path.map(|p| sanitize(&p.to_string_lossy()))
                .unwrap_or_else(|| "(HOME unset)".into())
        );
    }
    println!(
        "Glyph check: | + - /  |  \u{2502} \u{251c} \u{25cf} \u{25b8}  (font support cannot be detected)"
    );
    println!("Use --icons ascii if glyphs are not displayed correctly.");
}
