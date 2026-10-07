//! Colours and icons. Palettes are defined in RGB and downgraded to the
//! 256 or 16 colour palettes when the terminal does not support truecolor.

use ratatui::style::{Color, Modifier, Style};

use crate::config::{ColorMode, Config, IconMode, ThemeName};

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

/// Glyphs used by the renderer.
#[derive(Clone, Debug)]
pub struct Icons {
    pub ascii: bool,
    pub check_on: &'static str,
    pub check_off: &'static str,
    pub radio_on: &'static str,
    pub radio_off: &'static str,
    pub submenu: &'static str,
    pub left: &'static str,
    pub right: &'static str,
    pub up: &'static str,
    pub down: &'static str,
    pub busy: [&'static str; 4],
    pub file: &'static str,
    pub dir: &'static str,
    pub branch: &'static str,
}

impl Icons {
    fn new(mode: IconMode) -> Icons {
        match mode {
            IconMode::Ascii => Icons {
                ascii: true,
                check_on: "[x]",
                check_off: "[ ]",
                radio_on: "(*)",
                radio_off: "( )",
                submenu: ">",
                left: "<",
                right: ">",
                up: "^",
                down: "v",
                busy: ["|", "/", "-", "\\"],
                file: "",
                dir: "/",
                branch: "",
            },
            IconMode::Unicode => Icons {
                ascii: false,
                check_on: "☑",
                check_off: "☐",
                radio_on: "◉",
                radio_off: "○",
                submenu: "▸",
                left: "◂",
                right: "▸",
                up: "▲",
                down: "▼",
                busy: ["⠋", "⠙", "⠸", "⠴"],
                file: "",
                dir: "/",
                branch: "",
            },
            IconMode::Nerd => Icons {
                ascii: false,
                check_on: "\u{f14a}",
                check_off: "\u{f096}",
                radio_on: "\u{f192}",
                radio_off: "\u{f10c}",
                submenu: "\u{f054}",
                left: "\u{f053}",
                right: "\u{f054}",
                up: "\u{f077}",
                down: "\u{f078}",
                busy: ["⠋", "⠙", "⠸", "⠴"],
                file: "\u{f15b} ",
                dir: "/",
                branch: "\u{e0a0} ",
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    depth_true: bool,
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
    /// With 16 colours, backgrounds are not reliable: use reverse video.
    pub low_color: bool,
}

impl Theme {
    pub fn new(cfg: &Config) -> Theme {
        let depth = detect_depth(cfg.colors);
        let p = match cfg.theme {
            ThemeName::Dark => DARK,
            ThemeName::Light => LIGHT,
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
        Theme {
            depth_true: depth == Depth::True,
            bg: bgc(p.bg),
            bg_alt: bgc(p.bg_alt),
            fg: if low { Color::Reset } else { c(p.fg) },
            dim: c(p.dim),
            border: c(p.border),
            accent: c(p.accent),
            sel_bg: if low { Color::Blue } else { c(p.sel_bg) },
            sel_dim_bg: if low {
                Color::DarkGray
            } else {
                c(p.sel_dim_bg)
            },
            bar_bg: bgc(p.bar_bg),
            add: c(p.add),
            del: c(p.del),
            add_bg: bgc(p.add_bg),
            del_bg: bgc(p.del_bg),
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
            low_color: low,
        }
    }

    pub fn truecolor(&self) -> bool {
        self.depth_true
    }

    pub fn base(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
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
        if self.low_color {
            return Style::default().add_modifier(Modifier::REVERSED);
        }
        Style::default().bg(if focused {
            self.sel_bg
        } else {
            self.sel_dim_bg
        })
    }

    pub fn bar(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bar_bg)
    }

    pub fn graph_color(&self, i: u8) -> Color {
        self.graph[usize::from(i) % self.graph.len()]
    }

    pub fn button(&self, enabled: bool) -> Style {
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
                    let cfg = Config {
                        colors,
                        theme,
                        icons,
                        ..Config::default()
                    };
                    let t = Theme::new(&cfg);
                    assert_eq!(t.truecolor(), colors == ColorMode::Truecolor);
                }
            }
        }
    }
}
