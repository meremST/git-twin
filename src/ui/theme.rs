//! Colours and icons. Palettes are defined in RGB and downgraded to the
//! 256 or 16 colour palettes when the terminal does not support truecolor.

use ratatui::style::{Color, Modifier, Style};

use crate::config::{ColorMode, ColorScheme, Config, IconMode, ThemeName};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Depth {
    True,
    C256,
    C16,
}

fn detect_depth(mode: ColorMode) -> Depth {
    match mode {
        ColorMode::Truecolor => Depth::True,
        ColorMode::C256 => Depth::C256,
        ColorMode::C16 => Depth::C16,
        ColorMode::Auto => {
            let ct = std::env::var("COLORTERM")
                .unwrap_or_default()
                .to_ascii_lowercase();
            if ct.contains("truecolor") || ct.contains("24bit") {
                return Depth::True;
            }
            let term = std::env::var("TERM").unwrap_or_default();
            if term.contains("256color") || term.contains("kitty") || term.contains("alacritty") {
                Depth::C256
            } else {
                Depth::C16
            }
        }
    }
}

const ANSI16: [(u8, u8, u8, Color); 16] = [
    (0, 0, 0, Color::Black),
    (205, 49, 49, Color::Red),
    (13, 188, 121, Color::Green),
    (229, 229, 16, Color::Yellow),
    (36, 114, 200, Color::Blue),
    (188, 63, 188, Color::Magenta),
    (17, 168, 205, Color::Cyan),
    (204, 204, 204, Color::Gray),
    (102, 102, 102, Color::DarkGray),
    (241, 76, 76, Color::LightRed),
    (35, 209, 139, Color::LightGreen),
    (245, 245, 67, Color::LightYellow),
    (59, 142, 234, Color::LightBlue),
    (214, 112, 214, Color::LightMagenta),
    (41, 184, 219, Color::LightCyan),
    (255, 255, 255, Color::White),
];

fn dist(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).unsigned_abs();
    let (r, g, bl) = (d(a.0, b.0), d(a.1, b.1), d(a.2, b.2));
    2 * r * r + 4 * g * g + 3 * bl * bl
}

/// Nearest xterm-256 colour index (6x6x6 cube or grey ramp).
pub fn to_256(r: u8, g: u8, b: u8) -> u8 {
    let level = |v: u8| -> u8 {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            (v - 35) / 40
        }
    };
    let steps = [0u8, 95, 135, 175, 215, 255];
    let (cr, cg, cb) = (level(r), level(g), level(b));
    let cube = (steps[cr as usize], steps[cg as usize], steps[cb as usize]);
    let avg = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let gi = if avg > 238 {
        23
    } else {
        (avg.saturating_sub(3) / 10) as u8
    };
    let gv = 8 + gi * 10;
    if dist((r, g, b), (gv, gv, gv)) < dist((r, g, b), cube) {
        232 + gi
    } else {
        16 + 36 * cr + 6 * cg + cb
    }
}

fn to_16(r: u8, g: u8, b: u8) -> Color {
    ANSI16
        .iter()
        .min_by_key(|(ar, ag, ab, _)| dist((r, g, b), (*ar, *ag, *ab)))
        .map(|c| c.3)
        .unwrap_or(Color::Reset)
}

#[derive(Clone, Copy)]
struct Palette {
    bg: u32,
    bg_alt: u32,
    fg: u32,
    dim: u32,
    border: u32,
    accent: u32,
    sel_bg: u32,
    sel_dim_bg: u32,
    bar_bg: u32,
    add: u32,
    del: u32,
    add_bg: u32,
    del_bg: u32,
    hunk: u32,
    meta: u32,
    warn: u32,
    error: u32,
    ok: u32,
    head: u32,
    branch: u32,
    remote: u32,
    tag: u32,
    graph: [u32; 8],
}

const DARK: Palette = Palette {
    bg: 0x1a1b26,
    bg_alt: 0x24283b,
    fg: 0xc0caf5,
    dim: 0x737aa2,
    border: 0x3b4261,
    accent: 0x7aa2f7,
    sel_bg: 0x364a82,
    sel_dim_bg: 0x2e3450,
    bar_bg: 0x16161e,
    add: 0x9ece6a,
    del: 0xf7768e,
    add_bg: 0x23392b,
    del_bg: 0x3f2430,
    hunk: 0x7dcfff,
    meta: 0xbb9af7,
    warn: 0xe0af68,
    error: 0xf7768e,
    ok: 0x9ece6a,
    head: 0x9ece6a,
    branch: 0x73daca,
    remote: 0xff9e64,
    tag: 0xe0af68,
    graph: [
        0x7aa2f7, 0x9ece6a, 0xf7768e, 0xe0af68, 0xbb9af7, 0x7dcfff, 0xff9e64, 0x73daca,
    ],
};

const LIGHT: Palette = Palette {
    bg: 0xe1e2e7,
    bg_alt: 0xd5d6db,
    fg: 0x3760bf,
    dim: 0x6172b0,
    border: 0xa8aecb,
    accent: 0x2e7de9,
    sel_bg: 0xb7c1e3,
    sel_dim_bg: 0xc4c8da,
    bar_bg: 0xd0d5e3,
    add: 0x387068,
    del: 0xc64343,
    add_bg: 0xc8dcc8,
    del_bg: 0xe9c6c6,
    hunk: 0x007197,
    meta: 0x7847bd,
    warn: 0x8c6c3e,
    error: 0xc64343,
    ok: 0x587539,
    head: 0x587539,
    branch: 0x118c74,
    remote: 0xb15c00,
    tag: 0x8c6c3e,
    graph: [
        0x2e7de9, 0x587539, 0xc64343, 0x8c6c3e, 0x7847bd, 0x007197, 0xb15c00, 0x118c74,
    ],
};

// Warm Gruvbox bases with softer Gruvbox Material accents and diff backgrounds.
// Light accents are darkened where needed to stay legible on tinted selections.
// https://github.com/sainnhe/gruvbox-material/blob/master/autoload/gruvbox_material.vim
const GRUVBOX_DARK: Palette = Palette {
    bg: 0x282828,
    bg_alt: 0x32302f,
    fg: 0xddc7a1,
    dim: 0xa89984,
    border: 0x504945,
    accent: 0xd8a657,
    sel_bg: 0x45403d,
    sel_dim_bg: 0x32302f,
    bar_bg: 0x1d2021,
    add: 0xa9b665,
    del: 0xea6962,
    add_bg: 0x34381b,
    del_bg: 0x402120,
    hunk: 0x7daea3,
    meta: 0xd3869b,
    warn: 0xd8a657,
    error: 0xea6962,
    ok: 0xa9b665,
    head: 0xd8a657,
    branch: 0x89b482,
    remote: 0x7daea3,
    tag: 0xd3869b,
    graph: [
        0x7daea3, 0xa9b665, 0xea6962, 0xd8a657, 0xd3869b, 0x89b482, 0xe78a4e, 0xddc7a1,
    ],
};

const GRUVBOX_LIGHT: Palette = Palette {
    bg: 0xfbf1c7,
    bg_alt: 0xf4e8be,
    fg: 0x514036,
    dim: 0x6f6258,
    border: 0xbdae93,
    accent: 0x8f5e10,
    sel_bg: 0xddccab,
    sel_dim_bg: 0xf2e5bc,
    bar_bg: 0xebdbb2,
    add: 0x626b25,
    del: 0xaf2528,
    add_bg: 0xe6eabc,
    del_bg: 0xf9e0bb,
    hunk: 0x266b79,
    meta: 0x8f3f71,
    warn: 0x8f5e10,
    error: 0xaf2528,
    ok: 0x626b25,
    head: 0x8f5e10,
    branch: 0x386b49,
    remote: 0x266b79,
    tag: 0x8f3f71,
    graph: [
        0x266b79, 0x626b25, 0xaf2528, 0x8f5e10, 0x8f3f71, 0x386b49, 0xaf3a03, 0x514036,
    ],
};

/// Glyphs used by the renderer.
///
/// ASCII and Unicode keep their historical, text-only rendering. The Nerd Font
/// set adds semantic icons (views, panes, buttons, references, file status and
/// file types); decorative fields are empty outside Nerd mode so callers can
/// prefix them unconditionally through [`Icons::label`].
#[derive(Clone, Debug)]
pub struct Icons {
    pub ascii: bool,
    pub nerd: bool,
    /// Dialog check box.
    pub check_on: &'static str,
    pub check_off: &'static str,
    /// Mutually exclusive choice ("Diff / Old / New", "New Commit / Amend").
    pub radio_on: &'static str,
    pub radio_off: &'static str,
    /// Inline on/off toggle inside a button ("Ignore space").
    pub toggle_on: &'static str,
    pub toggle_off: &'static str,
    /// Checked menu entry / current dropdown option.
    pub mark: &'static str,
    pub submenu: &'static str,
    pub dropdown: &'static str,
    pub left: &'static str,
    pub right: &'static str,
    pub separator: &'static str,
    pub node: &'static str,
    // Decorative icons: empty unless Nerd mode.
    pub history: &'static str,
    pub worktree: &'static str,
    pub commit: &'static str,
    pub diff: &'static str,
    pub patch: &'static str,
    pub tree: &'static str,
    pub unstaged: &'static str,
    pub staged: &'static str,
    pub message: &'static str,
    pub comment: &'static str,
    pub branch: &'static str,
    pub remote: &'static str,
    pub tag: &'static str,
    pub lock: &'static str,
    pub busy: &'static str,
    pub search: &'static str,
    pub back: &'static str,
    pub forward: &'static str,
    pub rescan: &'static str,
    pub stage: &'static str,
    pub sign_off: &'static str,
    pub push: &'static str,
    pub help: &'static str,
    /// Rounded caps drawn around reference "pills" in the history list.
    pub pill_left: &'static str,
    pub pill_right: &'static str,
}

impl Icons {
    fn new(mode: IconMode) -> Icons {
        let text = Icons {
            ascii: true,
            nerd: false,
            check_on: "[x]",
            check_off: "[ ]",
            radio_on: "*",
            radio_off: " ",
            toggle_on: "*",
            toggle_off: " ",
            mark: "*",
            submenu: ">",
            dropdown: "v",
            left: "<",
            right: ">",
            separator: "-",
            node: "*",
            history: "",
            worktree: "",
            commit: "",
            diff: "",
            patch: "",
            tree: "",
            unstaged: "",
            staged: "",
            message: "",
            comment: "",
            branch: "",
            remote: "",
            tag: "",
            lock: "",
            busy: "",
            search: "",
            back: "",
            forward: "",
            rescan: "",
            stage: "",
            sign_off: "",
            push: "",
            help: "",
            pill_left: "",
            pill_right: "",
        };
        match mode {
            IconMode::Ascii => text,
            IconMode::Unicode => Icons {
                ascii: false,
                submenu: "\u{25b8}",
                node: "\u{25cf}",
                ..text
            },
            IconMode::Nerd => Icons {
                ascii: false,
                nerd: true,
                check_on: "\u{f14a}",
                check_off: "\u{f096}",
                radio_on: "\u{f192}",
                radio_off: "\u{f10c}",
                toggle_on: "\u{f14a}",
                toggle_off: "\u{f096}",
                mark: "\u{f00c}",
                submenu: "\u{f054}",
                dropdown: "\u{f078}",
                left: "\u{f053}",
                right: "\u{f054}",
                separator: "\u{2500}",
                // Nerd Font PUA glyphs (`\u{eafc}`, `\u{eb8a}`) rendered poorly/invisibly
                // without the matching font, and terminals don't composite overstruck
                // glyphs into one cell. The plain Unicode circle is supported everywhere
                // and already looks right, so it's reused here instead.
                node: "\u{25cf}",
                history: "\u{f1da}",
                worktree: "\u{f4d2}",
                commit: "\u{f417}",
                diff: "\u{f440}",
                patch: "\u{f0ca}",
                tree: "\u{f1bb}",
                unstaged: "\u{f040}",
                staged: "\u{f046}",
                message: "\u{f075}",
                comment: "\u{f0e5}",
                branch: "\u{e0a0}",
                remote: "\u{f0c2}",
                tag: "\u{f02b}",
                lock: "\u{f023}",
                busy: "\u{f252}",
                search: "\u{f002}",
                back: "\u{f060}",
                forward: "\u{f061}",
                rescan: "\u{f021}",
                stage: "\u{f067}",
                sign_off: "\u{f2b5}",
                push: "\u{f093}",
                help: "\u{f059}",
                pill_left: "\u{e0b6}",
                pill_right: "\u{e0b4}",
            },
        }
    }

    /// `"<icon> text"` in Nerd mode, `text` otherwise.
    pub fn label(&self, icon: &str, text: &str) -> String {
        if icon.is_empty() {
            text.to_string()
        } else {
            format!("{icon} {text}")
        }
    }

    /// Icon for a reference kind (empty outside Nerd mode).
    pub fn reference(&self, kind: crate::domain::refs::RefKind) -> &'static str {
        use crate::domain::refs::RefKind;
        match kind {
            RefKind::LocalBranch => self.branch,
            RefKind::RemoteBranch => self.remote,
            RefKind::Tag => self.tag,
            _ => self.commit,
        }
    }

    /// Nerd Font glyph for a git status letter (`M`, `A`, `D`, ...).
    pub fn status(&self, c: char) -> Option<&'static str> {
        if !self.nerd {
            return None;
        }
        Some(match c {
            'M' => "\u{f459}",
            'A' => "\u{f457}",
            'D' => "\u{f458}",
            'R' => "\u{f45a}",
            'C' => "\u{f0c5}",
            'T' => "\u{f0ec}",
            'U' => "\u{f071}",
            '?' => "\u{f128}",
            '!' => "\u{f474}",
            _ => return None,
        })
    }

    /// Nerd Font file-type glyph for a path (empty outside Nerd mode).
    pub fn file(&self, path: &str) -> &'static str {
        if !self.nerd {
            return "";
        }
        if path.ends_with('/') {
            return "\u{f07b}";
        }
        let name = path.rsplit('/').next().unwrap_or(path);
        let lower = name.to_ascii_lowercase();
        match lower.as_str() {
            "cargo.lock" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" => {
                return "\u{e672}";
            }
            "makefile" | "gnumakefile" | "cmakelists.txt" => return "\u{e673}",
            "dockerfile" | "containerfile" => return "\u{e650}",
            ".gitignore" | ".gitattributes" | ".gitmodules" | ".mailmap" => return "\u{e65d}",
            "license" | "license.md" | "license.txt" | "copying" => return "\u{e60a}",
            _ => {}
        }
        let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
        match ext {
            "rs" => "\u{e7a8}",
            "py" | "pyi" => "\u{e606}",
            "c" | "h" => "\u{e649}",
            "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => "\u{e646}",
            "go" => "\u{e627}",
            "js" | "mjs" | "cjs" | "jsx" => "\u{e60c}",
            "ts" | "tsx" => "\u{e628}",
            "java" => "\u{e66d}",
            "kt" | "kts" => "\u{e634}",
            "swift" => "\u{e699}",
            "rb" => "\u{e605}",
            "php" => "\u{e608}",
            "lua" => "\u{e620}",
            "hs" => "\u{e61f}",
            "ex" | "exs" => "\u{e62d}",
            "zig" => "\u{e6a9}",
            "vim" => "\u{e62b}",
            "sh" | "bash" | "zsh" | "fish" => "\u{e691}",
            "md" | "markdown" | "rst" => "\u{e609}",
            "json" => "\u{e60b}",
            "yml" | "yaml" => "\u{e6a8}",
            "toml" => "\u{e6b2}",
            "ini" | "cfg" | "conf" => "\u{e615}",
            "html" | "htm" => "\u{e60e}",
            "css" | "scss" | "sass" => "\u{e614}",
            "xml" => "\u{e619}",
            "svg" => "\u{e698}",
            "csv" => "\u{e64a}",
            "sql" | "db" | "sqlite" => "\u{e64d}",
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" => "\u{f1c5}",
            "pdf" => "\u{f1c1}",
            "zip" | "gz" | "tgz" | "xz" | "bz2" | "zst" | "tar" | "7z" => "\u{f1c6}",
            "txt" | "log" => "\u{e64e}",
            "lock" => "\u{e672}",
            _ => "\u{f15b}",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    depth: Depth,
    terminal_palette: bool,
    pub bg: Color,
    pub bg_alt: Color,
    pub fg: Color,
    pub dim: Color,
    pub border: Color,
    pub accent: Color,
    pub sel_bg: Color,
    pub sel_dim_bg: Color,
    pub bar_bg: Color,
    pub add: Color,
    pub del: Color,
    pub add_bg: Color,
    pub del_bg: Color,
    pub hunk: Color,
    pub meta: Color,
    pub warn: Color,
    pub error: Color,
    pub ok: Color,
    pub head: Color,
    pub branch: Color,
    pub remote: Color,
    pub tag: Color,
    pub graph: [Color; 8],
    pub icons: Icons,
    /// Fill the active view tab / open menu title with the accent colour.
    accent_tabs: bool,
    /// With 16 colours, backgrounds are not reliable: use reverse video.
    pub low_color: bool,
}

impl Theme {
    pub fn new(cfg: &Config) -> Theme {
        if cfg.theme == ThemeName::Terminal {
            return Theme::terminal(cfg);
        }
        let depth = detect_depth(cfg.colors);
        let p = match (cfg.theme, cfg.color_scheme) {
            (ThemeName::Dark, ColorScheme::Default) => DARK,
            (ThemeName::Light, ColorScheme::Default) => LIGHT,
            (ThemeName::Dark, ColorScheme::Gruvbox) => GRUVBOX_DARK,
            (ThemeName::Light, ColorScheme::Gruvbox) => GRUVBOX_LIGHT,
            (ThemeName::Terminal, _) => unreachable!(),
        };
        let c = |v: u32| -> Color {
            let (r, g, b) = ((v >> 16) as u8, (v >> 8) as u8, v as u8);
            match depth {
                Depth::True => Color::Rgb(r, g, b),
                Depth::C256 => Color::Indexed(to_256(r, g, b)),
                Depth::C16 => to_16(r, g, b),
            }
        };
        let low = depth == Depth::C16;
        // In 16-colour mode keep the terminal's own background.
        let bgc = |v: u32| if low { Color::Reset } else { c(v) };
        // Nearest-RGB conversion collapses the subtle diff tints into the same
        // grey/beige in 256 colors. Keep additions green and deletions red.
        let (add_bg, del_bg) = match (cfg.color_scheme, cfg.theme, depth) {
            (ColorScheme::Gruvbox, ThemeName::Dark, Depth::C256) => {
                (Color::Indexed(22), Color::Indexed(52))
            }
            (ColorScheme::Gruvbox, ThemeName::Light, Depth::C256) => {
                (Color::Indexed(194), Color::Indexed(224))
            }
            _ => (bgc(p.add_bg), bgc(p.del_bg)),
        };
        Theme {
            depth,
            terminal_palette: false,
            bg: bgc(p.bg),
            bg_alt: bgc(p.bg_alt),
            fg: if low { Color::Reset } else { c(p.fg) },
            dim: c(p.dim),
            border: c(p.border),
            accent: c(p.accent),
            sel_bg: match (cfg.color_scheme, cfg.theme, depth) {
                _ if low => Color::Blue,
                // Grey 238 would drop red accents below 3:1.
                (ColorScheme::Gruvbox, ThemeName::Dark, Depth::C256) => Color::Indexed(237),
                _ => c(p.sel_bg),
            },
            sel_dim_bg: if low {
                Color::DarkGray
            } else {
                c(p.sel_dim_bg)
            },
            bar_bg: bgc(p.bar_bg),
            add: c(p.add),
            del: c(p.del),
            add_bg,
            del_bg,
            hunk: c(p.hunk),
            meta: c(p.meta),
            warn: c(p.warn),
            error: c(p.error),
            ok: c(p.ok),
            head: c(p.head),
            branch: c(p.branch),
            remote: c(p.remote),
            tag: c(p.tag),
            graph: p.graph.map(c),
            icons: Icons::new(cfg.icons),
            accent_tabs: cfg.color_scheme == ColorScheme::Gruvbox && !low,
            low_color: low,
        }
    }

    /// Inherits the terminal palette except for coloured text in selected commits.
    fn terminal(cfg: &Config) -> Theme {
        Theme {
            depth: detect_depth(cfg.colors),
            terminal_palette: true,
            bg: Color::Reset,
            bg_alt: Color::Reset,
            fg: Color::Reset,
            dim: Color::DarkGray,
            border: Color::DarkGray,
            accent: Color::Yellow,
            sel_bg: Color::Blue,
            sel_dim_bg: Color::DarkGray,
            bar_bg: Color::Reset,
            add: Color::Green,
            del: Color::Red,
            add_bg: Color::Reset,
            del_bg: Color::Reset,
            hunk: Color::Cyan,
            meta: Color::Magenta,
            warn: Color::Yellow,
            error: Color::Red,
            ok: Color::Green,
            head: Color::Green,
            branch: Color::Cyan,
            remote: Color::Yellow,
            tag: Color::Yellow,
            graph: [
                Color::Blue,
                Color::Green,
                Color::Red,
                Color::Yellow,
                Color::Magenta,
                Color::Cyan,
                Color::LightYellow,
                Color::LightCyan,
            ],
            icons: Icons::new(cfg.icons),
            accent_tabs: false,
            // No reliable background colours to pair with reversed selection.
            low_color: true,
        }
    }

    pub fn truecolor(&self) -> bool {
        !self.terminal_palette && self.depth == Depth::True
    }

    pub fn base(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }

    /// Whether reference labels can be drawn as coloured Nerd Font pills:
    /// needs reliable backgrounds (no 16-colour / terminal palette).
    pub fn ref_pills(&self) -> bool {
        self.icons.nerd && !self.low_color && !self.terminal_palette
    }

    pub fn dimmed(&self) -> Style {
        self.base().fg(self.dim)
    }

    pub fn border_style(&self, focused: bool) -> Style {
        if focused {
            self.base().fg(self.accent)
        } else {
            self.base().fg(self.border)
        }
    }

    pub fn selected(&self, focused: bool) -> Style {
        if self.terminal_palette {
            // A plain colour background (instead of reversing each span) keeps
            // the row's own semantic colours (add/del/branch/...) readable and
            // avoids a stripe of differently-reversed colours on one line.
            let s = Style::default().bg(if focused {
                self.sel_bg
            } else {
                self.sel_dim_bg
            });
            return if focused {
                s.add_modifier(Modifier::BOLD)
            } else {
                s
            };
        }
        if self.low_color {
            return Style::default().add_modifier(Modifier::REVERSED);
        }
        Style::default().bg(if focused {
            self.sel_bg
        } else {
            self.sel_dim_bg
        })
    }

    pub fn selected_commit(&self, focused: bool) -> Style {
        if !self.terminal_palette {
            return self.selected(focused);
        }
        let style = self.base().add_modifier(Modifier::REVERSED);
        if focused {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }

    pub(super) fn commit_tint(&self, style: Style, color: Color) -> Style {
        if style.add_modifier.contains(Modifier::REVERSED) {
            if self.terminal_palette {
                // Reverse video displays the background colour as text, while
                // the default foreground supplies a uniform selection background.
                style.bg(self.selected_commit_color(color))
            } else {
                style
            }
        } else {
            style.fg(color)
        }
    }

    fn selected_commit_color(&self, color: Color) -> Color {
        if self.depth == Depth::C16 {
            return color;
        }
        // Dark variants for the light inverted background. These RGB values
        // also exist exactly in the xterm-256 palette.
        let (r, g, b) = match color {
            Color::Red | Color::LightRed => (135, 0, 0),
            Color::Green | Color::LightGreen => (0, 95, 0),
            Color::Yellow | Color::LightYellow => (95, 95, 0),
            Color::Blue | Color::LightBlue => (0, 0, 175),
            Color::Magenta | Color::LightMagenta => (135, 0, 135),
            Color::Cyan | Color::LightCyan => (0, 95, 95),
            Color::Gray | Color::DarkGray | Color::White => (95, 95, 95),
            _ => return color,
        };
        match self.depth {
            Depth::True => Color::Rgb(r, g, b),
            Depth::C256 => Color::Indexed(to_256(r, g, b)),
            Depth::C16 => unreachable!(),
        }
    }

    pub fn bar(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bar_bg)
    }

    /// Active view tab or open menu title in the top bar.
    pub fn bar_active(&self) -> Style {
        if self.accent_tabs {
            return Style::default()
                .fg(self.bg)
                .bg(self.accent)
                .add_modifier(Modifier::BOLD);
        }
        self.bar().patch(self.selected(true))
    }

    pub fn graph_color(&self, i: u8) -> Color {
        self.graph[usize::from(i) % self.graph.len()]
    }

    pub fn button(&self, enabled: bool) -> Style {
        if self.terminal_palette {
            return if enabled {
                self.base().fg(self.accent).add_modifier(Modifier::BOLD)
            } else {
                self.dimmed()
            };
        }
        let s = Style::default()
            .bg(self.bg_alt)
            .fg(if enabled { self.fg } else { self.dim });
        if self.low_color {
            s.add_modifier(Modifier::REVERSED)
        } else {
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_downgrade() {
        assert_eq!(to_256(0, 0, 0), 16);
        assert_eq!(to_256(255, 255, 255), 231);
        assert_eq!(to_256(128, 128, 128), 244);
        assert_eq!(to_16(250, 10, 10), Color::Red);
        assert_eq!(to_16(0, 0, 0), Color::Black);
    }

    #[test]
    fn themes_build_in_all_modes() {
        for colors in [ColorMode::Truecolor, ColorMode::C256, ColorMode::C16] {
            for theme in [ThemeName::Dark, ThemeName::Light] {
                for icons in [IconMode::Unicode, IconMode::Ascii, IconMode::Nerd] {
                    for color_scheme in [ColorScheme::Default, ColorScheme::Gruvbox] {
                        let cfg = Config {
                            colors,
                            theme,
                            icons,
                            color_scheme,
                            ..Config::default()
                        };
                        let t = Theme::new(&cfg);
                        assert_eq!(t.truecolor(), colors == ColorMode::Truecolor);
                        assert_eq!(t.low_color, colors == ColorMode::C16);
                        for color in [t.bg, t.fg, t.add, t.del, t.accent] {
                            match colors {
                                ColorMode::Truecolor => assert!(matches!(color, Color::Rgb(..))),
                                ColorMode::C256 => assert!(matches!(color, Color::Indexed(_))),
                                ColorMode::C16 => {
                                    assert!(!matches!(color, Color::Rgb(..) | Color::Indexed(_)));
                                }
                                ColorMode::Auto => unreachable!(),
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn gruvbox_follows_theme_mode_and_terminal_ignores_scheme() {
        for (theme, bg, fg, add, del) in [
            (ThemeName::Dark, 0x282828, 0xddc7a1, 0xa9b665, 0xea6962),
            (ThemeName::Light, 0xfbf1c7, 0x514036, 0x626b25, 0xaf2528),
        ] {
            let cfg = Config {
                theme,
                color_scheme: ColorScheme::Gruvbox,
                colors: ColorMode::Truecolor,
                ..Config::default()
            };
            let rgb = |v: u32| Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
            let t = Theme::new(&cfg);
            assert_eq!(
                (t.bg, t.fg, t.add, t.del),
                (rgb(bg), rgb(fg), rgb(add), rgb(del))
            );
            assert_ne!(t.bg, t.sel_bg);
            assert_ne!(t.sel_bg, t.sel_dim_bg);
            assert_ne!(
                t.bg,
                Theme::new(&Config {
                    color_scheme: ColorScheme::Default,
                    ..cfg
                })
                .bg
            );
        }
        for colors in [ColorMode::Truecolor, ColorMode::C256, ColorMode::C16] {
            let cfg = Config {
                theme: ThemeName::Terminal,
                colors,
                ..Config::default()
            };
            let default = Theme::new(&cfg);
            let gruvbox = Theme::new(&Config {
                color_scheme: ColorScheme::Gruvbox,
                ..cfg
            });
            assert_eq!(format!("{default:?}"), format!("{gruvbox:?}"));
        }
    }

    #[test]
    fn gruvbox_text_contrast_and_diff_tints_survive_color_downgrade() {
        fn luminance(color: Color) -> f64 {
            let (r, g, b) = match color {
                Color::Rgb(r, g, b) => (r, g, b),
                Color::Indexed(i @ 16..=231) => {
                    let levels = [0, 95, 135, 175, 215, 255];
                    let n = usize::from(i - 16);
                    (levels[n / 36], levels[n / 6 % 6], levels[n % 6])
                }
                Color::Indexed(i @ 232..=255) => {
                    let grey = 8 + (i - 232) * 10;
                    (grey, grey, grey)
                }
                _ => panic!("expected a fixed RGB or xterm color, got {color:?}"),
            };
            let linear = |v: u8| {
                let c = f64::from(v) / 255.0;
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
        }
        let contrast = |fg: Color, bg: Color, minimum: f64| {
            let (a, b) = (luminance(fg), luminance(bg));
            let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
            assert!(
                ratio >= minimum,
                "{fg:?} on {bg:?}: {ratio:.2}:1, expected at least {minimum}:1"
            );
        };
        for theme in [ThemeName::Dark, ThemeName::Light] {
            for colors in [ColorMode::Truecolor, ColorMode::C256] {
                let t = Theme::new(&Config {
                    theme,
                    colors,
                    color_scheme: ColorScheme::Gruvbox,
                    ..Config::default()
                });
                for bg in [t.bg, t.bg_alt, t.bar_bg, t.sel_bg, t.sel_dim_bg] {
                    contrast(t.fg, bg, 6.0);
                    contrast(t.dim, bg, 3.0);
                }
                contrast(t.dim, t.bg, 4.5);
                for fg in [
                    t.accent, t.add, t.del, t.hunk, t.meta, t.warn, t.error, t.ok, t.head,
                    t.branch, t.remote, t.tag,
                ]
                .into_iter()
                .chain(t.graph)
                {
                    contrast(fg, t.bg, if t.truecolor() { 4.5 } else { 4.0 });
                    contrast(fg, t.sel_bg, 3.0);
                    contrast(fg, t.sel_dim_bg, 3.0);
                }
                for (fg, bg) in [(t.add, t.add_bg), (t.del, t.del_bg)] {
                    contrast(fg, bg, if t.truecolor() { 4.5 } else { 3.0 });
                    assert_ne!(bg, t.bg);
                }
                assert_ne!(t.add_bg, t.del_bg);
                assert_ne!(t.sel_bg, t.sel_dim_bg);
                let tab = t.bar_active();
                assert_eq!((tab.fg, tab.bg), (Some(t.bg), Some(t.accent)));
                contrast(t.bg, t.accent, 4.5);
                contrast(t.sel_bg, t.bg, 1.3);
                assert_eq!(t.head, t.accent);
                assert_ne!(t.branch, t.remote);
                assert_ne!(t.remote, t.tag);
            }
        }
    }

    #[test]
    fn terminal_theme_uses_reset_and_named_colors() {
        for colors in [
            ColorMode::Auto,
            ColorMode::Truecolor,
            ColorMode::C256,
            ColorMode::C16,
        ] {
            let t = Theme::new(&Config {
                theme: ThemeName::Terminal,
                colors,
                ..Config::default()
            });
            assert!(!t.truecolor());
            for color in [t.bg, t.bg_alt, t.fg, t.bar_bg, t.add_bg, t.del_bg] {
                assert_eq!(color, Color::Reset);
            }
            assert_eq!(t.accent, Color::Yellow);
            for color in [
                t.dim,
                t.border,
                t.accent,
                t.sel_bg,
                t.sel_dim_bg,
                t.add,
                t.del,
                t.hunk,
                t.meta,
                t.warn,
                t.error,
                t.ok,
                t.head,
                t.branch,
                t.remote,
                t.tag,
            ]
            .into_iter()
            .chain(t.graph)
            {
                assert!(!matches!(color, Color::Rgb(..) | Color::Indexed(_)));
            }
        }
    }

    #[test]
    fn terminal_selection_and_buttons_respect_focus() {
        let t = Theme::new(&Config {
            theme: ThemeName::Terminal,
            ..Config::default()
        });
        for fg in [t.fg, t.add, t.del, t.meta] {
            let base = t.base().fg(fg);
            let active = base.patch(t.selected(true));
            // The row's own semantic colour (add/del/meta/...) stays visible;
            // only a coloured background and bold are added.
            assert_eq!(active.fg, Some(fg));
            assert_eq!(active.bg, Some(t.sel_bg));
            assert!(active.add_modifier.contains(Modifier::BOLD));
            assert!(!active.add_modifier.contains(Modifier::REVERSED));
            let inactive = base.patch(t.selected(false));
            assert_eq!(inactive.fg, Some(fg));
            assert_eq!(inactive.bg, Some(t.sel_dim_bg));
            assert!(!inactive.add_modifier.contains(Modifier::BOLD));
            assert!(!inactive.add_modifier.contains(Modifier::REVERSED));
        }
        for enabled in [true, false] {
            let button = t.button(enabled);
            assert_eq!(button.bg, Some(Color::Reset));
            assert_eq!(button.fg, Some(if enabled { t.accent } else { t.dim }));
            assert_eq!(button.add_modifier.contains(Modifier::BOLD), enabled);
            assert!(!button.add_modifier.contains(Modifier::REVERSED));
        }
        let focused_button = t.button(true).patch(t.selected(true));
        assert_eq!(focused_button.fg, Some(t.accent));
        assert_eq!(focused_button.bg, Some(t.sel_bg));
        assert!(focused_button.add_modifier.contains(Modifier::BOLD));
        assert!(!focused_button.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn terminal_commit_selection_preserves_color_families() {
        for colors in [
            ColorMode::Auto,
            ColorMode::Truecolor,
            ColorMode::C256,
            ColorMode::C16,
        ] {
            let t = Theme::new(&Config {
                theme: ThemeName::Terminal,
                colors,
                ..Config::default()
            });
            for focused in [true, false] {
                for fg in [t.fg, t.branch, t.head, t.dim, t.warn, t.graph_color(0)] {
                    let style = t.commit_tint(t.selected_commit(focused), fg);
                    assert_eq!(style.fg, Some(Color::Reset));
                    assert_eq!(style.bg, Some(t.selected_commit_color(fg)));
                    assert!(!style.add_modifier.contains(Modifier::UNDERLINED));
                    assert!(style.add_modifier.contains(Modifier::REVERSED));
                    assert_eq!(style.add_modifier.contains(Modifier::BOLD), focused);
                }
                assert_eq!(t.commit_tint(t.base(), t.branch), t.base().fg(t.branch));
            }
        }
    }

    #[test]
    fn selected_commit_colors_contrast_with_light_backgrounds() {
        let luminance = |rgb: (u8, u8, u8)| {
            let linear = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(rgb.0) + 0.7152 * linear(rgb.1) + 0.0722 * linear(rgb.2)
        };
        for (color, rgb) in [
            (Color::Red, (135, 0, 0)),
            (Color::Green, (0, 95, 0)),
            (Color::Yellow, (95, 95, 0)),
            (Color::Blue, (0, 0, 175)),
            (Color::Magenta, (135, 0, 135)),
            (Color::Cyan, (0, 95, 95)),
            (Color::DarkGray, (95, 95, 95)),
            (Color::LightYellow, (95, 95, 0)),
            (Color::LightCyan, (0, 95, 95)),
        ] {
            for colors in [ColorMode::Truecolor, ColorMode::C256, ColorMode::C16] {
                let t = Theme::new(&Config {
                    theme: ThemeName::Terminal,
                    colors,
                    ..Config::default()
                });
                let expected = match colors {
                    ColorMode::Truecolor => Color::Rgb(rgb.0, rgb.1, rgb.2),
                    ColorMode::C256 => Color::Indexed(to_256(rgb.0, rgb.1, rgb.2)),
                    _ => color,
                };
                assert_eq!(t.selected_commit_color(color), expected);
                assert_eq!(t.selected_commit_color(Color::Reset), Color::Reset);
            }
            for background in [(229, 229, 229), (255, 255, 255)] {
                let contrast = (luminance(background) + 0.05) / (luminance(rgb) + 0.05);
                assert!(contrast >= 4.5, "{color:?}: contrast {contrast}");
            }
        }
    }

    #[test]
    fn fixed_palette_selection_and_buttons_are_unchanged() {
        for theme in [ThemeName::Dark, ThemeName::Light] {
            for colors in [ColorMode::Truecolor, ColorMode::C256, ColorMode::C16] {
                let t = Theme::new(&Config {
                    theme,
                    colors,
                    ..Config::default()
                });
                for focused in [true, false] {
                    let expected = if colors == ColorMode::C16 {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default().bg(if focused { t.sel_bg } else { t.sel_dim_bg })
                    };
                    assert_eq!(t.selected(focused), expected);
                    assert_eq!(t.selected_commit(focused), expected);
                    assert_eq!(
                        t.commit_tint(expected, t.branch),
                        if colors == ColorMode::C16 {
                            expected
                        } else {
                            expected.fg(t.branch)
                        }
                    );
                }
                for enabled in [true, false] {
                    let mut expected =
                        Style::default()
                            .bg(t.bg_alt)
                            .fg(if enabled { t.fg } else { t.dim });
                    if colors == ColorMode::C16 {
                        expected = expected.add_modifier(Modifier::REVERSED);
                    }
                    assert_eq!(t.button(enabled), expected);
                }
            }
        }
    }
}
