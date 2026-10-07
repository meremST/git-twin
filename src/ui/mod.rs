//! Terminal rendering (ratatui).

pub mod hit;
mod overlays;
pub mod theme;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::{
    App, StatusTone,
    action::{Action, FileList},
    diffview::{DKind, DiffView, Show},
    history::HFocus,
    input::{TextArea, TextInput, width_upto},
    menu,
};
use crate::config::ViewKind;
use crate::domain::{graph, refs::RefKind, sanitize::sanitize};
use hit::{Hit, HitMap, PaneId, SplitId};
use theme::Theme;

const ASCII_BORDER: ratatui::symbols::border::Set<'static> = ratatui::symbols::border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

fn border(t: &Theme) -> ratatui::symbols::border::Set<'static> {
    if t.icons.ascii {
        ASCII_BORDER
    } else {
        ratatui::symbols::border::ROUNDED
    }
}

fn unobscured(app: &App) -> bool {
    app.menu.is_none() && app.dialogs.is_empty()
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    app.size = (area.width, area.height);
    app.hits.clear();
    f.render_widget(Block::default().style(app.theme.base()), area);
    if area.width < 80 || area.height < 24 {
        text(
            f,
            area,
            format!(
                "Terminal too small: resize to at least 80 x 24. Ctrl-Q: quit.\n{}",
                sanitize(&app.status.0)
            ),
            app.theme.base(),
        );
        return;
    }
    menu_bar(f, app, row(area, 0));
    let body = Rect::new(area.x, area.y + 1, area.width, area.height - 2);
    if app.maximized.is_some() || area.width < 120 || area.height < 35 {
        single_pane(f, app, body);
    } else {
        match app.view {
            ViewKind::History => history(f, app, body),
            ViewKind::Worktree => worktree(f, app, body),
        }
    }
    let color = match app.status.1 {
        StatusTone::Info => app.theme.fg,
        StatusTone::Busy => app.theme.warn,
        StatusTone::Error => app.theme.error,
    };
    let flags = if app.read_only { "READ ONLY | " } else { "" };
    let busy = if app.busy() { "Working | " } else { "" };
    text(
        f,
        row(area, area.height - 1),
        format!(
            " {flags}{busy}{}  | F6: view  F10: menu  F1: help",
            sanitize(&app.status.0)
        ),
        app.theme.bar().fg(color),
    );
    overlays::menus(f, app);
    overlays::dialog(f, app);
}

fn row(r: Rect, y: u16) -> Rect {
    Rect::new(r.x, r.y + y.min(r.height), r.width, u16::from(y < r.height))
}

fn text(f: &mut Frame, r: Rect, s: impl Into<String>, style: Style) {
    f.render_widget(Paragraph::new(s.into()).style(style), r);
}

fn pane(f: &mut Frame, r: Rect, title: &str, id: PaneId, app: &mut App) -> Rect {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border(&app.theme))
        .border_style(app.theme.border_style(app.focused_pane() == id))
        .title(format!(" {title} "));
    let inner = block.inner(r);
    f.render_widget(block, r);
    app.hits.push(inner, Hit::Pane(id));
    inner
}

fn split(app: &mut App, r: Rect, id: SplitId, vertical: bool, ratio: u16) -> (Rect, Rect) {
    let len = if vertical { r.height } else { r.width };
    let n = ((u32::from(len) * u32::from(ratio.clamp(100, 900))) / 1000) as u16;
    let (left_min, right_min) = match id {
        SplitId::HTop => (6, 9),
        SplitId::HBottom => (60, 20),
        SplitId::WLeft => (24, 68),
        SplitId::WDiff => (10, 8),
        _ => (4, 4),
    };
    let (left_min, right_min) = if len > left_min + right_min {
        (left_min, right_min)
    } else {
        let half = len.saturating_sub(1) / 2;
        (half, half)
    };
    let n = n.clamp(left_min, len.saturating_sub(right_min + 1).max(left_min));
    let (a, bar, b) = if vertical {
        (
            Rect::new(r.x, r.y, r.width, n),
            Rect::new(r.x, r.y + n, r.width, 1),
            Rect::new(r.x, r.y + n + 1, r.width, len.saturating_sub(n + 1)),
        )
    } else {
        (
            Rect::new(r.x, r.y, n, r.height),
            Rect::new(r.x + n, r.y, 1, r.height),
            Rect::new(r.x + n + 1, r.y, len.saturating_sub(n + 1), r.height),
        )
    };
    app.hits.push(bar, Hit::Split(id, r));
    (a, b)
}

fn button(f: &mut Frame, app: &mut App, r: Rect, label: &str, action: Action, enabled: bool) {
    text(f, r, format!("[{label}]"), app.theme.button(enabled));
    if enabled {
        app.hits.push(r, Hit::Button(action));
    }
}

fn menu_bar(f: &mut Frame, app: &mut App, r: Rect) {
    f.render_widget(Block::default().style(app.theme.bar()), r);
    for (i, ((x, w), title)) in menu::title_positions(app.view)
        .into_iter()
        .zip(menu::titles(app.view))
        .enumerate()
    {
        let zone = Rect::new(r.x + x, r.y, w.min(r.width.saturating_sub(x)), 1);
        let style = if app.menu.as_ref().and_then(|m| m.bar) == Some(i) {
            app.theme.bar().patch(app.theme.selected(true))
        } else {
            app.theme.bar()
        };
        text(f, zone, format!(" {title} "), style);
        app.hits.push(zone, Hit::MenuTitle(i));
    }
    let tabs = [
        (ViewKind::History, "History"),
        (ViewKind::Worktree, "Worktree"),
    ];
    let start = r.right().saturating_sub(16);
    for (i, (view, label)) in tabs.into_iter().enumerate() {
        let zone = Rect::new(start + i as u16 * 8, r.y, 8, 1);
        text(
            f,
            zone,
            label,
            if app.view == view {
                app.theme.bar().patch(app.theme.selected(true))
            } else {
                app.theme.bar()
            },
        );
        app.hits.push(zone, Hit::ViewTab(view));
    }
}

fn history(f: &mut Frame, app: &mut App, r: Rect) {
    let (top, bottom) = split(app, r, SplitId::HTop, true, app.state.splits.history_top);
    history_top(f, app, top);
    let (diff, files) = split(
        app,
        bottom,
        SplitId::HBottom,
        false,
        app.state.splits.history_bottom_left,
    );
    diff_pane(f, app, diff, PaneId::HDiff);
    history_files(f, app, files);
}

fn history_top(f: &mut Frame, app: &mut App, top: Rect) {
    let list = Rect::new(top.x, top.y, top.width, top.height.saturating_sub(2));
    history_list(f, app, list);
    if top.height >= 2 {
        let nav = row(top, top.height - 2);
        text(
            f,
            Rect::new(nav.x + 1, nav.y, 6, 1),
            "SHA1:",
            app.theme.base(),
        );
        let input_r = Rect::new(nav.x + 7, nav.y, 42.min(nav.width.saturating_sub(40)), 1);
        let focused = unobscured(app) && app.hist.focus == HFocus::Sha;
        input(f, &mut app.hist.sha, input_r, focused, &app.theme);
        app.hits.push(input_r, Hit::ShaEntry);
        button(
            f,
            app,
            Rect::new(nav.x + 50, nav.y, 6, 1),
            "Back",
            Action::GoBack,
            !app.hist.back.is_empty(),
        );
        button(
            f,
            app,
            Rect::new(nav.x + 57, nav.y, 9, 1),
            "Forward",
            Action::GoForward,
            !app.hist.forward.is_empty(),
        );
        text(
            f,
            Rect::new(nav.x + 67, nav.y, nav.width.saturating_sub(67), 1),
            format!(
                "Row {}/{}",
                app.hist.sel.map_or(0, |i| i + 1),
                app.hist.total
            ),
            app.theme.dimmed(),
        );
        let find = row(top, top.height - 1);
        button(
            f,
            app,
            Rect::new(find.x + 1, find.y, 6, 1),
            "Find",
            Action::FindNext,
            true,
        );
        let label = app.hist.find_type.label();
        button(
            f,
            app,
            Rect::new(find.x + 8, find.y, 24, 1),
            label,
            Action::CycleFindType,
            true,
        );
        let ir = Rect::new(find.x + 33, find.y, find.width.saturating_sub(58), 1);
        let focused = unobscured(app) && app.hist.focus == HFocus::Find;
        input(f, &mut app.hist.find, ir, focused, &app.theme);
        app.hits.push(ir, Hit::FindEntry);
        let label = app.hist.find_match.label();
        button(
            f,
            app,
            Rect::new(find.right() - 24, find.y, 10, 1),
            label,
            Action::CycleFindMatch,
            true,
        );
        let label = app.hist.find_field.label();
        button(
            f,
            app,
            Rect::new(find.right() - 13, find.y, 13, 1),
            label,
            Action::CycleFindField,
            true,
        );
    }
}

fn single_pane(f: &mut Frame, app: &mut App, r: Rect) {
    let current = app.focused_pane();
    let tabs: &[(PaneId, &str)] = match app.view {
        ViewKind::History => &[
            (PaneId::HList, "History"),
            (PaneId::HDiff, "Details"),
            (PaneId::HFiles, "Files"),
        ],
        ViewKind::Worktree => &[
            (PaneId::WUnstaged, "Unstaged"),
            (PaneId::WStaged, "Staged"),
            (PaneId::WDiff, "Diff"),
            (PaneId::WMessage, "Message"),
        ],
    };
    text(
        f,
        row(r, 0),
        if app.maximized.is_some() {
            "Zoom:"
        } else {
            "Compact:"
        },
        app.theme.dimmed(),
    );
    let mut x = r.x + 9;
    for &(id, label) in tabs {
        let w = label.width() as u16 + 3;
        let zone = Rect::new(x, r.y, w, 1);
        text(
            f,
            zone,
            format!("[{label}]"),
            if id == current {
                app.theme.button(true).patch(app.theme.selected(true))
            } else {
                app.theme.button(true)
            },
        );
        app.hits.push(zone, Hit::PaneTab(id));
        x += w;
    }
    text(
        f,
        Rect::new(x, r.y, r.right().saturating_sub(x), 1),
        " Tab: pane",
        app.theme.dimmed(),
    );
    let footer = u16::from(app.view == ViewKind::Worktree);
    let body = Rect::new(r.x, r.y + 1, r.width, r.height.saturating_sub(1 + footer));
    match current {
        PaneId::HList => history_top(f, app, body),
        PaneId::HDiff | PaneId::WDiff => diff_pane(f, app, body, current),
        PaneId::HFiles => history_files(f, app, body),
        PaneId::WUnstaged => worktree_files(f, app, body, FileList::Unstaged),
        PaneId::WStaged => worktree_files(f, app, body, FileList::Staged),
        PaneId::WMessage => message(f, app, body),
        PaneId::Dialog => {}
    }
    if footer > 0 {
        let mut x = r.x;
        for (label, action, write) in [
            ("Rescan", Action::Rescan, false),
            ("Stage Changed", Action::StageChanged, true),
            ("Sign Off", Action::SignOff, false),
            ("Commit", Action::Commit, true),
            ("Push", Action::Push, true),
        ] {
            let w = label.width() as u16 + 3;
            button(
                f,
                app,
                Rect::new(x, r.bottom() - 1, w, 1),
                label,
                action,
                !write || !app.read_only,
            );
            x += w;
        }
    }
}

fn history_list(f: &mut Frame, app: &mut App, r: Rect) {
    let title = format!("{} - {}", app.repo.display_name(), app.hist.view().name);
    let inner = pane(f, r, &title, PaneId::HList, app);
    if inner.height == 0 {
        return;
    }
    let date_w = app.state.splits.history_date_col.min(inner.width / 4);
    let author_w = app.state.splits.history_author_col.min(inner.width / 4);
    let author_x = inner.right().saturating_sub(date_w + author_w + 2);
    let date_x = inner.right().saturating_sub(date_w);
    text(f, row(inner, 0), "Graph / Subject", app.theme.dimmed());
    text(
        f,
        Rect::new(author_x + 1, inner.y, author_w, 1),
        "Author",
        app.theme.dimmed(),
    );
    text(
        f,
        Rect::new(date_x, inner.y, date_w, 1),
        "Date",
        app.theme.dimmed(),
    );
    app.hist.height = usize::from(inner.height - 1);
    app.hist.top = app.hist.top.min(
        app.hist
            .commits
            .len()
            .saturating_sub(app.hist.height.max(1)),
    );
    for (line, i) in (app.hist.top..app.hist.commits.len())
        .take(app.hist.height)
        .enumerate()
    {
        let rr = row(inner, line as u16 + 1);
        let c = &app.hist.commits[i];
        let selected = app.hist.sel == Some(i);
        let style = if selected {
            app.theme
                .base()
                .patch(app.theme.selected(app.hist.focus == HFocus::List))
        } else {
            app.theme.base()
        };
        f.render_widget(Block::default().style(style), rr);
        app.hits.push(rr, Hit::CommitRow(i));
        let mut x = inner.x;
        if let Some(g) = app.hist.graph.get(i) {
            for (lane, cell) in g.cells.iter().enumerate() {
                if x + 2 >= author_x {
                    break;
                }
                let ch = if lane == usize::from(g.node) {
                    if app.theme.icons.ascii { '*' } else { '●' }
                } else {
                    graph::glyph(cell.bits, app.theme.icons.ascii)
                };
                let color = if lane == usize::from(g.node) {
                    g.node_color
                } else {
                    cell.color
                };
                text(
                    f,
                    Rect::new(x, rr.y, 1, 1),
                    ch.to_string(),
                    style.fg(app.theme.graph_color(color)),
                );
                let h = if cell.bits & graph::RIGHT != 0 {
                    if app.theme.icons.ascii { "-" } else { "─" }
                } else {
                    " "
                };
                text(
                    f,
                    Rect::new(x + 1, rr.y, 1, 1),
                    h,
                    style.fg(app.theme.graph_color(cell.hcolor)),
                );
                x += 2;
            }
        }
        for reference in app.refs.for_commit(&c.oid) {
            let color = if app.refs.is_head_ref(reference) {
                app.theme.head
            } else {
                match reference.kind {
                    RefKind::LocalBranch => app.theme.branch,
                    RefKind::RemoteBranch => app.theme.remote,
                    RefKind::Tag => app.theme.tag,
                    _ => app.theme.meta,
                }
            };
            let label = format!("[{}] ", reference.short);
            let w = label.width().min(usize::from(author_x.saturating_sub(x))) as u16;
            let zone = Rect::new(x, rr.y, w, 1);
            text(f, zone, label, style.fg(color).add_modifier(Modifier::BOLD));
            app.hits
                .push(zone, Hit::RefLabel(i, reference.full.clone()));
            x += w;
        }
        let bold = app
            .hist
            .highlight
            .as_ref()
            .is_some_and(|set| set.contains(&c.oid));
        let subject_style = if bold {
            style.fg(app.theme.warn).add_modifier(Modifier::BOLD)
        } else {
            style
        };
        text(
            f,
            Rect::new(x, rr.y, author_x.saturating_sub(x), 1),
            c.subject.to_string(),
            subject_style,
        );
        text(
            f,
            Rect::new(author_x + 1, rr.y, author_w, 1),
            c.author.to_string(),
            style,
        );
        text(
            f,
            Rect::new(date_x, rr.y, date_w, 1),
            c.date.to_string(),
            style.fg(app.theme.dim),
        );
    }
    app.hits.push(
        Rect::new(author_x, inner.y, 1, inner.height),
        Hit::Split(SplitId::HAuthor, inner),
    );
    app.hits.push(
        Rect::new(date_x.saturating_sub(1), inner.y, 1, inner.height),
        Hit::Split(SplitId::HDate, inner),
    );
    if app.hist.commits.is_empty() && inner.height > 1 {
        text(
            f,
            row(inner, 1),
            if app.hist.loading {
                "Loading history..."
            } else {
                "No commits in this view."
            },
            app.theme.dimmed(),
        );
    }
}

fn history_files(f: &mut Frame, app: &mut App, r: Rect) {
    let inner = pane(
        f,
        r,
        if app.hist.tree_mode { "Tree" } else { "Patch" },
        PaneId::HFiles,
        app,
    );
    if inner.height == 0 {
        return;
    }
    let half = inner.width / 2;
    button(
        f,
        app,
        Rect::new(inner.x, inner.y, half, 1),
        "Patch",
        Action::SetTreeMode(false),
        true,
    );
    button(
        f,
        app,
        Rect::new(inner.x + half, inner.y, inner.width - half, 1),
        "Tree",
        Action::SetTreeMode(true),
        true,
    );
    app.hist.file_height = usize::from(inner.height - 1);
    app.hist.file_top = app.hist.file_top.min(
        app.hist
            .file_rows()
            .saturating_sub(app.hist.file_height.max(1)),
    );
    for (line, i) in (app.hist.file_top..app.hist.file_rows())
        .take(app.hist.file_height)
        .enumerate()
    {
        let rr = row(inner, line as u16 + 1);
        let label = app
            .hist
            .file_path(i)
            .map(|p| crate::domain::sanitize::display_bytes(&p))
            .unwrap_or_else(|| "Comments".into());
        let style = if i == app.hist.file_sel {
            app.theme
                .base()
                .patch(app.theme.selected(app.hist.focus == HFocus::Files))
        } else {
            app.theme.base()
        };
        text(f, rr, label, style);
        app.hits.push(rr, Hit::HFileRow(i));
    }
}

fn diff_pane(f: &mut Frame, app: &mut App, r: Rect, id: PaneId) {
    let inner = pane(
        f,
        r,
        if id == PaneId::HDiff {
            "Details / Diff"
        } else {
            "File diff"
        },
        id,
        app,
    );
    if inner.height == 0 {
        return;
    }
    let mut controls = 1;
    if id == PaneId::HDiff {
        text(
            f,
            Rect::new(inner.x, inner.y, 7.min(inner.width), 1),
            "Search:",
            app.theme.dimmed(),
        );
        let ir = Rect::new(
            inner.x + 7.min(inner.width),
            inner.y,
            inner.width.saturating_sub(7),
            1,
        );
        let focused = unobscured(app) && app.hist.focus == HFocus::Search;
        input(f, &mut app.hist.search, ir, focused, &app.theme);
        app.hits.push(ir, Hit::SearchEntry);
        if inner.height > 1 {
            controls = 2;
            let mut x = inner.x;
            for show in [Show::Diff, Show::Old, Show::New] {
                let label = format!(
                    "{} {}",
                    if app.hist.show == show { "*" } else { " " },
                    show.label()
                );
                let w = (label.width() as u16 + 2).min(inner.right().saturating_sub(x));
                let zone = Rect::new(x, inner.y + 1, w, 1);
                text(f, zone, format!("[{label}]"), app.theme.button(true));
                app.hits.push(zone, Hit::ShowBtn(show));
                x += w;
            }
        }
        if inner.height > 2 {
            controls = 3;
            let labels = [
                (
                    format!("Context {} -", app.hist.context),
                    Action::ContextLess,
                ),
                ("+".to_string(), Action::ContextMore),
                (
                    format!(
                        "{} Ignore space",
                        if app.hist.ignore_space { "*" } else { " " }
                    ),
                    Action::ToggleIgnoreSpace,
                ),
                (
                    app.hist.word_mode.label().to_string(),
                    Action::CycleWordMode,
                ),
            ];
            let mut x = inner.x;
            for (label, action) in labels {
                let w = (label.width() as u16 + 2).min(inner.right().saturating_sub(x));
                button(
                    f,
                    app,
                    Rect::new(x, inner.y + 2, w, 1),
                    &label,
                    action,
                    true,
                );
                x += w;
            }
        }
    } else {
        text(
            f,
            row(inner, 0),
            format!(
                "Context: {}  | s: stage hunk  r: revert  Shift+arrows: lines",
                app.wt.context
            ),
            app.theme.dimmed(),
        );
    }
    let content = Rect::new(
        inner.x,
        inner.y + controls,
        inner.width,
        inner.height.saturating_sub(controls),
    );
    // Selection and wheel scrolling use the actual text viewport, not its controls.
    app.hits.zones.retain(|(_, h)| *h != Hit::Pane(id));
    app.hits.push(content, Hit::Pane(id));
    let focused = app.focused_pane() == id;
    let dv = if id == PaneId::HDiff {
        &mut app.hist.diff
    } else {
        &mut app.wt.diff
    };
    render_diff(f, dv, content, id, focused, &app.theme, &mut app.hits);
}

fn diff_style(t: &Theme, kind: DKind) -> Style {
    match kind {
        DKind::Add => t.base().fg(t.add).bg(t.add_bg),
        DKind::Del => t.base().fg(t.del).bg(t.del_bg),
        DKind::Hunk => t.base().fg(t.hunk),
        DKind::Header | DKind::FileTitle => t.base().fg(t.accent).add_modifier(Modifier::BOLD),
        DKind::Meta => t.base().fg(t.meta),
        DKind::Info => t.dimmed(),
        DKind::Error => t.base().fg(t.error),
        _ => t.base(),
    }
}

fn render_diff(
    f: &mut Frame,
    dv: &mut DiffView,
    r: Rect,
    id: PaneId,
    focused: bool,
    t: &Theme,
    hits: &mut HitMap,
) {
    dv.height = usize::from(r.height);
    dv.width = usize::from(r.width);
    dv.top = dv.top.min(dv.max_top());
    for (line, i) in (dv.top..dv.lines.len()).take(dv.height).enumerate() {
        let dl = &dv.lines[i];
        let mut style = diff_style(t, dl.kind);
        if dv.is_selected(i) {
            style = style.patch(t.selected(focused));
        }
        let spans: Vec<Span<'_>> = if let Some(segs) = &dl.segs {
            segs.iter()
                .map(|(kind, text)| {
                    Span::styled(
                        text.clone(),
                        diff_style(t, *kind).patch(if dv.is_selected(i) {
                            t.selected(focused)
                        } else {
                            Style::default()
                        }),
                    )
                })
                .collect()
        } else if let Some(matcher) = &dv.search {
            let mut spans = Vec::new();
            let mut end = 0;
            for (a, b) in matcher.find_all(&dl.text) {
                spans.push(Span::styled(dl.text[end..a].to_string(), style));
                spans.push(Span::styled(
                    dl.text[a..b].to_string(),
                    style.add_modifier(Modifier::REVERSED),
                ));
                end = b;
            }
            spans.push(Span::styled(dl.text[end..].to_string(), style));
            spans
        } else {
            vec![Span::styled(dl.text.clone(), style)]
        };
        let spans = if let Some((a, b)) = dv.text_range() {
            let mut col = 0;
            spans
                .into_iter()
                .flat_map(|span| {
                    span.content
                        .graphemes(true)
                        .map(|g| {
                            let p = crate::app::input::Pos { row: i, col };
                            col += g.chars().count();
                            Span::styled(
                                g.to_owned(),
                                if p >= a && p < b {
                                    span.style.patch(t.selected(focused))
                                } else {
                                    span.style
                                },
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        } else {
            spans
        };
        let rr = row(r, line as u16);
        f.render_widget(
            Paragraph::new(Line::from(spans))
                .style(style)
                .scroll((0, dv.left.min(u16::MAX as usize) as u16)),
            rr,
        );
        hits.push(rr, Hit::DiffLine(id, i));
    }
}

fn worktree(f: &mut Frame, app: &mut App, r: Rect) {
    let operations = app
        .repo_state
        .operations
        .iter()
        .map(|o| o.label())
        .collect::<Vec<_>>()
        .join(", ");
    text(
        f,
        row(r, 0),
        format!(
            " Current Branch: {}  {}  | {}",
            app.wt.branch_label,
            operations,
            app.repo.display_name()
        ),
        app.theme.base().fg(app.theme.head),
    );
    let body = Rect::new(r.x, r.y + 1, r.width, r.height.saturating_sub(1));
    let (top, bottom) = split(
        app,
        body,
        SplitId::WDiff,
        true,
        app.state.splits.worktree_diff,
    );
    let (lists, diff) = split(
        app,
        top,
        SplitId::WLeft,
        false,
        app.state.splits.worktree_left,
    );
    let (unstaged, staged) = split(
        app,
        lists,
        SplitId::WFiles,
        true,
        app.state.splits.worktree_files,
    );
    worktree_files(f, app, unstaged, FileList::Unstaged);
    worktree_files(f, app, staged, FileList::Staged);
    diff_pane(f, app, diff, PaneId::WDiff);
    let left_w = lists.width.min(bottom.width);
    let actions = Rect::new(
        bottom.x + 1,
        bottom.y,
        left_w.saturating_sub(2),
        bottom.height,
    );
    for (i, (label, action, write)) in [
        ("Rescan", Action::Rescan, false),
        ("Stage Changed", Action::StageChanged, true),
        ("Sign Off", Action::SignOff, false),
        ("Commit", Action::Commit, true),
        ("Push", Action::Push, true),
    ]
    .into_iter()
    .enumerate()
    .take(usize::from(actions.height))
    {
        button(
            f,
            app,
            row(actions, i as u16),
            label,
            action,
            !write || !app.read_only,
        );
    }
    message(
        f,
        app,
        Rect::new(
            bottom.x + left_w + 1,
            bottom.y,
            bottom.width.saturating_sub(left_w + 1),
            bottom.height,
        ),
    );
}

fn worktree_files(f: &mut Frame, app: &mut App, r: Rect, list: FileList) {
    let staged = list == FileList::Staged;
    let id = if staged {
        PaneId::WStaged
    } else {
        PaneId::WUnstaged
    };
    let title = format!(
        "{} ({})",
        if staged {
            "Staged Changes"
        } else {
            "Unstaged Changes"
        },
        app.wt.list(list).len()
    );
    let inner = pane(f, r, &title, id, app);
    let height = usize::from(inner.height);
    let len = app.wt.list(list).len();
    let top = if staged {
        app.wt.height_staged = height;
        app.wt.top_staged = app.wt.top_staged.min(len.saturating_sub(height.max(1)));
        app.wt.top_staged
    } else {
        app.wt.height_unstaged = height;
        app.wt.top_unstaged = app.wt.top_unstaged.min(len.saturating_sub(height.max(1)));
        app.wt.top_unstaged
    };
    for (line, i) in (top..len).take(height).enumerate() {
        let file = &app.wt.list(list)[i];
        let style = if app.wt.sel(list).contains(&file.path) {
            app.theme
                .base()
                .patch(app.theme.selected(app.focused_pane() == id))
        } else {
            app.theme.base()
        };
        let rr = row(inner, line as u16);
        text(
            f,
            rr,
            format!(" {}  {}", file.icon_char(staged), file.display),
            style,
        );
        app.hits.push(rr, Hit::WtFile(list, i, false));
        app.hits.push(
            Rect::new(rr.x, rr.y, 3.min(rr.width), 1),
            Hit::WtFile(list, i, true),
        );
    }
    if len == 0 {
        text(f, row(inner, 0), "(none)", app.theme.dimmed());
    }
}

fn message(f: &mut Frame, app: &mut App, r: Rect) {
    let column = width_upto(
        &app.wt.message.lines[app.wt.message.cursor.row],
        app.wt.message.cursor.col,
    ) + 1;
    let title = format!("Commit Message - col {column}/{}", app.cfg.commit_width);
    let inner = pane(f, r, &title, PaneId::WMessage, app);
    if inner.height == 0 {
        return;
    }
    let half = inner.width / 2;
    button(
        f,
        app,
        Rect::new(inner.x, inner.y, half, 1),
        if app.wt.amend {
            "New Commit"
        } else {
            "* New Commit"
        },
        Action::SetAmend(false),
        true,
    );
    button(
        f,
        app,
        Rect::new(inner.x + half, inner.y, inner.width - half, 1),
        if app.wt.amend {
            "* Amend Last Commit"
        } else {
            "Amend Last Commit"
        },
        Action::SetAmend(true),
        !app.read_only,
    );
    let area = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    app.wt.msg_height = usize::from(area.height);
    app.wt.msg_width = usize::from(area.width);
    let focused = unobscured(app) && app.focused_pane() == PaneId::WMessage;
    textarea(f, &mut app.wt.message, area, focused, &app.theme);
    let guide = usize::from(app.cfg.commit_width);
    if guide >= app.wt.message.left && guide - app.wt.message.left < usize::from(area.width) {
        let x = area.x + (guide - app.wt.message.left) as u16;
        for y in area.y..area.bottom() {
            let line = app.wt.message.top + usize::from(y - area.y);
            let occupied = app
                .wt
                .message
                .lines
                .get(line)
                .is_some_and(|s| s.width() > guide);
            let cell = &mut f.buffer_mut()[(x, y)];
            if !occupied {
                cell.set_symbol(if app.theme.icons.ascii { "|" } else { "│" })
                    .set_fg(app.theme.border);
            } else {
                cell.set_style(Style::default().add_modifier(Modifier::UNDERLINED));
            }
        }
    }
    app.hits.push(area, Hit::Message);
}

fn input(f: &mut Frame, input: &mut TextInput, r: Rect, focused: bool, t: &Theme) {
    if r.is_empty() {
        return;
    }
    let (start, cursor) = input.view(usize::from(r.width));
    let visible: String = input.text.chars().skip(start).collect();
    text(
        f,
        r,
        visible,
        t.base().bg(t.bg_alt).add_modifier(Modifier::UNDERLINED),
    );
    if focused {
        f.set_cursor_position((r.x + (cursor as u16).min(r.width - 1), r.y));
    }
}

fn textarea(f: &mut Frame, area: &mut TextArea, r: Rect, focused: bool, t: &Theme) {
    if r.is_empty() {
        return;
    }
    let selection = area.selection();
    for (line, i) in (area.top..area.lines.len())
        .take(usize::from(r.height))
        .enumerate()
    {
        let s = &area.lines[i];
        let mut col = 0;
        let spans: Vec<_> = s
            .graphemes(true)
            .map(|g| {
                let selected = selection.is_some_and(|(a, b)| {
                    let p = crate::app::input::Pos { row: i, col };
                    p >= a && p < b
                });
                col += g.chars().count();
                Span::styled(
                    g.to_string(),
                    if selected {
                        t.base().patch(t.selected(focused))
                    } else {
                        t.base()
                    },
                )
            })
            .collect();
        f.render_widget(
            Paragraph::new(Line::from(spans)).scroll((0, area.left.min(u16::MAX as usize) as u16)),
            row(r, line as u16),
        );
    }
    if focused && area.cursor.row >= area.top && area.cursor.row < area.top + usize::from(r.height)
    {
        let x = width_upto(&area.lines[area.cursor.row], area.cursor.col).saturating_sub(area.left);
        f.set_cursor_position((
            r.x + (x as u16).min(r.width - 1),
            r.y + (area.cursor.row - area.top) as u16,
        ));
    }
}
