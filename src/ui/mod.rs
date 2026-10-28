//! Terminal rendering (ratatui).

pub mod hit;
mod overlays;
pub mod theme;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
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
    mouse::Drag,
};
use crate::config::ViewKind;
use crate::domain::{
    graph,
    oid::Oid,
    refs::{GitRef, RefKind},
    sanitize::sanitize,
};
use hit::{HistoryScrollbar, Hit, HitMap, PaneId, SplitId};
use theme::Theme;

/// Columns kept for the subject when the history graph is wide.
const GRAPH_MIN_SUBJECT: u16 = 24;

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
    app.menu.is_none() && !app.dialog_has_focus()
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    if app.size != (area.width, area.height) && matches!(app.drag, Some(Drag::HistoryScroll { .. }))
    {
        app.drag = None;
    }
    app.size = (area.width, area.height);
    app.hits.clear();
    f.render_widget(Block::default().style(app.theme.base()), area);
    if area.width < 79 || area.height < 24 {
        text(
            f,
            area,
            format!(
                "Terminal too small: resize to at least 79 x 24. Ctrl-Q / Ctrl-W: quit.\n{}",
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
    if matches!(app.drag, Some(Drag::HistoryScroll { .. }))
        && !app
            .hits
            .zones
            .iter()
            .any(|(_, h)| matches!(h, Hit::HistoryScrollbar(_)))
    {
        app.drag = None;
    }
    let color = if app.status.1 == StatusTone::Error {
        app.theme.error
    } else if app.working() {
        app.theme.warn
    } else {
        app.theme.fg
    };
    let icons = &app.theme.icons;
    let sep = if icons.nerd { "\u{2502}" } else { "|" };
    let flags = if app.read_only {
        format!("{} {sep} ", icons.label(icons.lock, "READ ONLY"))
    } else {
        String::new()
    };
    let activity = if app.working() {
        icons.label(icons.busy, "Working")
    } else {
        String::from("Ready")
    };
    let message = if app.status.0.is_empty() {
        String::new()
    } else {
        format!("{}  {sep} ", sanitize(&app.status.0))
    };
    let help = icons.label(icons.help, "F1: help");
    let prefix = format!(" {flags}{activity}  {sep} ");
    let bar = row(area, area.height - 1);
    if !app.status.0.is_empty() {
        let x = bar.x.saturating_add(prefix.width() as u16);
        let w = (sanitize(&app.status.0).width() as u16).min(bar.right().saturating_sub(x));
        app.hits.push(Rect::new(x, bar.y, w, 1), Hit::StatusMsg);
    }
    text(
        f,
        bar,
        format!("{prefix}{message}F6: view  F10: menu  {help}"),
        app.theme.bar().fg(color),
    );
    if app.refs_window_open() {
        overlays::dialog(f, app);
        overlays::menus(f, app);
    } else {
        overlays::menus(f, app);
        overlays::dialog(f, app);
    }
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
            app.theme.bar_active()
        } else {
            app.theme.bar()
        };
        text(f, zone, format!(" {title} "), style);
        app.hits.push(zone, Hit::MenuTitle(i));
    }
    let icons = &app.theme.icons;
    let menu_end = menu::title_positions(app.view)
        .into_iter()
        .map(|(x, w)| r.x + x + w)
        .max()
        .unwrap_or(r.x);
    // Iconised tabs only when they fit beside the menu titles.
    let nerd = icons.nerd && r.right().saturating_sub(menu_end) >= 24;
    let tabs = [
        (ViewKind::History, icons.history, "History"),
        (ViewKind::Worktree, icons.worktree, "Worktree"),
    ]
    .map(|(view, icon, label)| {
        if nerd {
            let label = format!(" {} ", icons.label(icon, label));
            (view, label.width() as u16, label)
        } else {
            (view, 8, label.to_string())
        }
    });
    let mut x = r
        .right()
        .saturating_sub(tabs.iter().map(|(_, w, _)| w).sum());
    for (view, w, label) in tabs {
        let zone = Rect::new(x, r.y, w, 1);
        x += w;
        text(
            f,
            zone,
            label,
            if app.view == view {
                app.theme.bar_active()
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
        let icons = &app.theme.icons;
        let back = icons.label(icons.back, "Back");
        let forward = icons.label(icons.forward, "Forward");
        let back_x = nav.x + 50;
        let back_w = back.width() as u16 + 2;
        let forward_x = back_x + back_w + 1;
        let forward_w = forward.width() as u16 + 2;
        let rows_x = forward_x + forward_w + 1;
        button(
            f,
            app,
            Rect::new(back_x, nav.y, back_w, 1),
            &back,
            Action::GoBack,
            !app.hist.back.is_empty(),
        );
        button(
            f,
            app,
            Rect::new(forward_x, nav.y, forward_w, 1),
            &forward,
            Action::GoForward,
            !app.hist.forward.is_empty(),
        );
        text(
            f,
            Rect::new(rows_x, nav.y, nav.right().saturating_sub(rows_x), 1),
            format!(
                "Row {}/{}",
                app.hist.sel.map_or(0, |i| i + 1),
                app.hist.total
            ),
            app.theme.dimmed(),
        );
        let find = row(top, top.height - 1);
        let find_label = app.theme.icons.label(app.theme.icons.search, "Find");
        let find_w = find_label.width() as u16 + 2;
        button(
            f,
            app,
            Rect::new(find.x + 1, find.y, find_w, 1),
            &find_label,
            Action::FindNext,
            true,
        );
        let type_x = find.x + find_w + 2;
        let label = app.hist.find_type.label();
        button(
            f,
            app,
            Rect::new(type_x, find.y, 24, 1),
            label,
            Action::CycleFindType,
            true,
        );
        let input_x = type_x + 25;
        let ir = Rect::new(
            input_x,
            find.y,
            find.right().saturating_sub(25).saturating_sub(input_x),
            1,
        );
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
    let i = &app.theme.icons;
    let tabs: Vec<(PaneId, String)> = match app.view {
        ViewKind::History => vec![
            (PaneId::HList, i.label(i.history, "History")),
            (PaneId::HDiff, i.label(i.diff, "Details")),
            (PaneId::HFiles, i.label(i.patch, "Files")),
        ],
        ViewKind::Worktree => vec![
            (PaneId::WUnstaged, i.label(i.unstaged, "Unstaged")),
            (PaneId::WStaged, i.label(i.staged, "Staged")),
            (PaneId::WDiff, i.label(i.diff, "Diff")),
            (PaneId::WMessage, i.label(i.message, "Message")),
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
    for (id, label) in tabs {
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
        let buttons = worktree_buttons(app);
        let total_width: u16 = buttons
            .iter()
            .map(|(label, _, _)| label.width() as u16 + 3)
            .sum();
        let mut x = r.right().saturating_sub(total_width);
        for (label, action, write) in buttons {
            let w = label.width() as u16 + 3;
            button(
                f,
                app,
                Rect::new(x, r.bottom() - 1, w, 1),
                &label,
                action,
                !write || !app.read_only,
            );
            x += w;
        }
    }
}

struct ReferenceLabel {
    text: Line<'static>,
    hit: Hit,
    count: usize,
    /// Reference kind and its colour, used to tint the overflow marker.
    kind: Option<(RefKind, Color)>,
}

/// Colour of the reference kind that dominates the hidden labels.
fn dominant_kind_color(hidden: &[(usize, (RefKind, Color))]) -> Option<Color> {
    let mut totals: Vec<(RefKind, Color, usize)> = Vec::new();
    for &(count, (kind, color)) in hidden {
        match totals.iter_mut().find(|t| t.0 == kind) {
            Some(total) => total.2 += count,
            None => totals.push((kind, color, count)),
        }
    }
    totals
        .into_iter()
        .reduce(|best, t| if t.2 > best.2 { t } else { best })
        .map(|t| t.1)
}

fn reference_label(app: &App, reference: &GitRef, row: usize, style: Style) -> ReferenceLabel {
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
    let icons = &app.theme.icons;
    let icon = icons.reference(reference.kind);
    let spans = if app.theme.ref_pills() {
        let cap = Style::default()
            .fg(color)
            .bg(style.bg.unwrap_or(app.theme.bg));
        vec![
            Span::styled(icons.pill_left, cap),
            Span::styled(
                format!("{icon} {}", reference.short),
                Style::default()
                    .fg(app.theme.bg)
                    .bg(color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(icons.pill_right, cap),
            Span::styled(" ", style),
        ]
    } else {
        let label = if icons.nerd {
            format!("{icon} {} ", reference.short)
        } else {
            format!("[{}] ", reference.short)
        };
        vec![Span::styled(
            label,
            app.theme
                .commit_tint(style, color)
                .add_modifier(Modifier::BOLD),
        )]
    };
    ReferenceLabel {
        text: Line::from(spans),
        hit: Hit::RefLabel(row, reference.full.clone()),
        count: 1,
        kind: Some((reference.kind, color)),
    }
}

fn reference_labels(app: &App, oid: &Oid, row: usize, style: Style) -> Vec<ReferenceLabel> {
    let mut refs: Vec<_> = app
        .refs
        .for_commit(oid)
        .filter(|r| app.refs.is_head_ref(r) || !app.hist.hidden_ref_kinds.contains(&r.kind))
        .collect();
    refs.sort_by_key(|r| {
        (
            !app.refs.is_head_ref(r),
            match r.kind {
                RefKind::LocalBranch => 0,
                RefKind::Tag => 1,
                RefKind::RemoteBranch => 2,
                _ => 3,
            },
        )
    });
    let compact = app.hist.compact_refs;
    let open = compact && app.hist.expanded_refs.contains(oid);
    let mut labels = Vec::new();
    if open {
        labels.push(ReferenceLabel {
            text: Line::styled("< ", app.theme.dimmed()),
            hit: Hit::RefToggle(row),
            count: 0,
            kind: None,
        });
    }
    if !compact || open {
        labels.extend(
            refs.into_iter()
                .map(|r| reference_label(app, r, row, style)),
        );
        return labels;
    }
    if let Some(head) = refs.first().filter(|r| app.refs.is_head_ref(r)) {
        labels.push(reference_label(app, head, row, style));
    }
    for (kind, letter, color) in [
        (RefKind::LocalBranch, "B", app.theme.branch),
        (RefKind::Tag, "T", app.theme.tag),
        (RefKind::RemoteBranch, "R", app.theme.remote),
        (RefKind::Stash, "S", app.theme.meta),
        (RefKind::Other, "O", app.theme.meta),
    ] {
        let count = refs
            .iter()
            .filter(|r| r.kind == kind && !app.refs.is_head_ref(r))
            .count();
        if count == 0 {
            continue;
        }
        let text = if app.theme.icons.nerd {
            format!("{} {count} ", app.theme.icons.reference(kind))
        } else {
            format!("[{letter}:{count}] ")
        };
        labels.push(ReferenceLabel {
            text: Line::styled(
                text,
                app.theme
                    .commit_tint(style, color)
                    .add_modifier(Modifier::BOLD),
            ),
            hit: Hit::RefToggle(row),
            count,
            kind: Some((kind, color)),
        });
    }
    labels
}

fn shortened_reference(line: &Line<'_>, width: usize) -> Line<'static> {
    let mut left = width.saturating_sub(3);
    let mut spans = Vec::new();
    for span in &line.spans {
        let mut text = String::new();
        for g in span.content.graphemes(true) {
            if g.width() > left {
                spans.push(Span::styled(format!("{text}..."), span.style));
                return Line::from(spans);
            }
            text.push_str(g);
            left -= g.width();
        }
        spans.push(Span::styled(text, span.style));
    }
    Line::from(spans)
}

fn history_list(f: &mut Frame, app: &mut App, r: Rect) {
    let title = app.theme.icons.label(
        app.theme.icons.history,
        &format!("{} - {}", app.repo.display_name(), app.hist.view().name),
    );
    let mut inner = pane(f, r, &title, PaneId::HList, app);
    if inner.height == 0 {
        return;
    }
    let step: u16 = if app.cfg.graph_connectors { 2 } else { 1 };
    app.hist.height = usize::from((inner.height - 1) / step).max(1);
    app.hist.top = app
        .hist
        .top
        .min(app.hist.commits.len().saturating_sub(app.hist.height));
    let scrollbar = if app.cfg.history_scrollbar && inner.width > 1 {
        let track = Rect::new(inner.right() - 1, inner.y + 1, 1, inner.height - 1);
        HistoryScrollbar::new(track, app.hist.commits.len(), app.hist.height).map(|bar| {
            if let Some(Drag::HistoryScroll { bar: frozen, .. }) = app.drag {
                if frozen.track == track && frozen.max_top <= bar.max_top {
                    return frozen;
                }
                app.drag = None;
            }
            bar
        })
    } else {
        None
    };
    if scrollbar.is_some() {
        inner.width -= 1;
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
    for (line, i) in (app.hist.top..app.hist.commits.len())
        .take(app.hist.height)
        .enumerate()
    {
        let rr = row(inner, line as u16 * step + 1);
        let connector = if app.cfg.graph_connectors {
            row(inner, line as u16 * step + 2)
        } else {
            Rect::new(rr.x, rr.y, rr.width, 0)
        };
        let c = &app.hist.commits[i];
        let selected = app.hist.sel == Some(i);
        let style = if selected {
            app.theme
                .base()
                .patch(app.theme.selected_commit(app.hist.focus == HFocus::List))
        } else {
            app.theme.base()
        };
        f.render_widget(Block::default().style(style), rr);
        f.render_widget(Block::default().style(style), connector);
        app.hits.push(rr, Hit::CommitRow(i));
        app.hits.push(connector, Hit::CommitRow(i));
        let tint = |c| app.theme.commit_tint(style, c);
        let mut x = inner.x;
        if let Some(g) = app.hist.graph.get(i) {
            let ascii = app.theme.icons.ascii;
            let connectors = app.cfg.graph_connectors;
            // Up arrow of a cut line resuming into the next commit: the
            // builder keeps this row's cell empty for it.
            let resume_col = app.hist.graph.get(i + 1).and_then(|n| {
                n.cells
                    .iter()
                    .position(|c| c.bits & graph::RESUME != 0)
                    .map(|k| (k, n.cells[k].color))
            });
            // Dashed box-drawing stubs line up with "│", unlike arrow glyphs.
            let up_arrow = if ascii { "^" } else { "┇" };
            let down_arrow = if ascii { "v" } else { "┇" };
            let room = usize::from(author_x.saturating_sub(inner.x + GRAPH_MIN_SUBJECT)) / 2;
            let cap = match usize::from(app.cfg.graph_max_lanes) {
                0 => room,
                m => m.min(room),
            }
            .max(1);
            let width = g.cells.len().max(resume_col.map_or(0, |(c, _)| c + 1));
            let mut arrows = Vec::new();
            let overflow = width > cap;
            let shown = if overflow { cap - 1 } else { width };
            for lane in 0..shown {
                let cell = g.cells.get(lane).copied().unwrap_or_default();
                let is_node = lane == usize::from(g.node);
                let resume_here = resume_col.filter(|(c, _)| *c == lane);
                let fg = if is_node {
                    if c.oid == crate::git::log::null_unstaged() {
                        app.theme.error
                    } else {
                        app.theme.graph_color(g.node_color)
                    }
                } else if let (Some((_, color)), false) = (resume_here, connectors) {
                    app.theme.graph_color(color)
                } else {
                    app.theme.graph_color(cell.color)
                };
                let ch = if is_node {
                    app.theme.icons.node.to_string()
                } else if cell.bits & graph::CUT != 0 {
                    arrows.push((Rect::new(x, rr.y, 1, 1), Hit::CutArrow(i, lane as u16)));
                    down_arrow.to_string()
                } else if resume_here.is_some() && !connectors {
                    let hit = Hit::ResumeArrow {
                        row: i,
                        node: i + 1,
                    };
                    arrows.push((Rect::new(x, rr.y, 1, 1), hit));
                    up_arrow.to_string()
                } else {
                    graph::glyph(cell.bits, ascii).to_string()
                };
                text(f, Rect::new(x, rr.y, 1, 1), ch, tint(fg));
                let h = if cell.bits & graph::RIGHT != 0 {
                    if ascii { "-" } else { "─" }
                } else {
                    " "
                };
                text(
                    f,
                    Rect::new(x + 1, rr.y, 1, 1),
                    h,
                    tint(app.theme.graph_color(cell.hcolor)),
                );
                // Connector line: a short vertical stub under the lanes that
                // continue on to the next commit, so consecutive nodes in the
                // same lane read as visually linked.
                if cell.bits & graph::DOWN != 0 {
                    let v = if ascii { "|" } else { "│" };
                    text(
                        f,
                        Rect::new(x, connector.y, 1, connector.height),
                        v,
                        tint(app.theme.graph_color(cell.color)),
                    );
                } else if let Some((_, color)) = resume_here {
                    let zone = Rect::new(x, connector.y, 1, connector.height);
                    text(f, zone, up_arrow, tint(app.theme.graph_color(color)));
                    arrows.push((
                        zone,
                        Hit::ResumeArrow {
                            row: i,
                            node: i + 1,
                        },
                    ));
                }
                x += 2;
            }
            if overflow {
                // Columns beyond the limit fold into "…"; a node among them
                // is still drawn so every commit keeps its marker.
                let more = if ascii { "~" } else { "…" };
                if usize::from(g.node) >= shown {
                    let fg = app.theme.graph_color(g.node_color);
                    text(
                        f,
                        Rect::new(x, rr.y, 1, 1),
                        app.theme.icons.node.to_string(),
                        tint(fg),
                    );
                    text(f, Rect::new(x + 1, rr.y, 1, 1), more, app.theme.dimmed());
                    x += 1;
                } else {
                    text(f, Rect::new(x, rr.y, 1, 1), more, app.theme.dimmed());
                }
                x += 2;
            }
            for (zone, hit) in arrows {
                app.hits.push(zone, hit);
            }
        }
        let labels = reference_labels(app, &c.oid, i, style);
        let mut remaining: usize = labels.iter().map(|l| l.count).sum();
        let available = author_x.saturating_sub(x);
        let end = x + available
            .saturating_sub(GRAPH_MIN_SUBJECT)
            .min(available / 2);
        let start = x;
        let mut hidden = Vec::new();
        let mut blocked = false;
        for label in labels {
            if blocked {
                hidden.extend(label.kind.map(|k| (label.count, k)));
                continue;
            }
            let after = remaining - label.count;
            let overflow_width = if after > 0 {
                format!("+{after} ").len() as u16
            } else {
                0
            };
            let room = end.saturating_sub(x + overflow_width);
            if label.text.width() > usize::from(room) {
                blocked = true;
                if label.count == 1 && x == start && room >= 6 {
                    let zone = Rect::new(x, rr.y, room, 1);
                    f.render_widget(
                        Paragraph::new(shortened_reference(&label.text, usize::from(room))),
                        zone,
                    );
                    app.hits
                        .push(zone, Hit::Button(Action::ListCommitRefs(c.oid)));
                    x += room;
                    remaining = after;
                } else {
                    hidden.extend(label.kind.map(|k| (label.count, k)));
                }
                continue;
            }
            let w = label.text.width() as u16;
            let zone = Rect::new(x, rr.y, w, 1);
            f.render_widget(Paragraph::new(label.text), zone);
            app.hits.push(zone, label.hit);
            x += w;
            remaining = after;
        }
        if remaining > 0 {
            let label = format!("+{remaining} ");
            let w = (label.len() as u16).min(end.saturating_sub(x));
            let zone = Rect::new(x, rr.y, w, 1);
            let color = dominant_kind_color(&hidden).unwrap_or(app.theme.meta);
            text(f, zone, label, tint(color).add_modifier(Modifier::BOLD));
            app.hits
                .push(zone, Hit::Button(Action::ListCommitRefs(c.oid)));
            x += w;
        }
        let bold = app
            .hist
            .highlight
            .as_ref()
            .is_some_and(|set| set.contains(&c.oid));
        let subject_style = if bold {
            tint(app.theme.warn).add_modifier(Modifier::BOLD)
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
            tint(app.theme.dim),
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
    if let Some(bar) = scrollbar {
        let thumb_top = bar.thumb_top(app.hist.top);
        for y in bar.track.y..bar.track.bottom() {
            let thumb = y >= thumb_top && y < thumb_top + bar.thumb_height;
            let symbol = match (app.theme.icons.ascii, thumb) {
                (true, true) => "#",
                (true, false) => "|",
                (false, true) => "█",
                (false, false) => "│",
            };
            let color = if thumb {
                app.theme.accent
            } else {
                app.theme.dim
            };
            text(
                f,
                Rect::new(bar.track.x, y, 1, 1),
                symbol,
                app.theme.base().fg(color),
            );
        }
        app.hits.push(bar.track, Hit::HistoryScrollbar(bar));
    }
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
    // Checkbox on the bottom border, right-aligned.
    let icons = &app.theme.icons;
    let mark = if app.hist.compact_refs {
        icons.toggle_on
    } else {
        icons.toggle_off
    };
    let label = format!(" {mark} Compact refs ");
    let w = (label.width() as u16).min(r.width.saturating_sub(2));
    if r.height > 1 && w > 0 {
        let zone = Rect::new(r.right().saturating_sub(w + 1), r.bottom() - 1, w, 1);
        text(f, zone, label, app.theme.dimmed());
        app.hits.push(zone, Hit::Button(Action::ToggleCompactRefs));
    }
}

fn history_files(f: &mut Frame, app: &mut App, r: Rect) {
    let i = &app.theme.icons;
    let patch = i.label(i.patch, "Patch");
    let tree = i.label(i.tree, "Tree");
    let title = if app.hist.tree_mode { &tree } else { &patch }.clone();
    let inner = pane(f, r, &title, PaneId::HFiles, app);
    if inner.height == 0 {
        return;
    }
    let half = inner.width / 2;
    button(
        f,
        app,
        Rect::new(inner.x, inner.y, half, 1),
        &patch,
        Action::SetTreeMode(false),
        true,
    );
    button(
        f,
        app,
        Rect::new(inner.x + half, inner.y, inner.width - half, 1),
        &tree,
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
        let icons = &app.theme.icons;
        let label = match app.hist.file_path(i) {
            Some(p) => {
                let path = crate::domain::sanitize::display_bytes(&p);
                icons.label(icons.file(&path), &path)
            }
            None => icons.label(icons.comment, "Comments"),
        };
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
    let icons = &app.theme.icons;
    let title = icons.label(
        icons.diff,
        if id == PaneId::HDiff {
            "Details / Diff"
        } else {
            "File diff"
        },
    );
    let inner = pane(f, r, &title, id, app);
    if inner.height == 0 {
        return;
    }
    let mut controls = 1;
    if id == PaneId::HDiff {
        let label = app.theme.icons.label(app.theme.icons.search, "Search:");
        let lw = label.width() as u16;
        text(
            f,
            Rect::new(inner.x, inner.y, lw.min(inner.width), 1),
            label,
            app.theme.dimmed(),
        );
        let ir = Rect::new(
            inner.x + lw.min(inner.width),
            inner.y,
            inner.width.saturating_sub(lw),
            1,
        );
        let focused = unobscured(app) && app.hist.focus == HFocus::Search;
        input(f, &mut app.hist.search, ir, focused, &app.theme);
        app.hits.push(ir, Hit::SearchEntry);
        if inner.height > 1 {
            controls = 2;
            let mut x = inner.x;
            for show in [Show::Diff, Show::Old, Show::New] {
                let icons = &app.theme.icons;
                let label = format!(
                    "{} {}",
                    if app.hist.show == show {
                        icons.radio_on
                    } else {
                        icons.radio_off
                    },
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
                        if app.hist.ignore_space {
                            app.theme.icons.toggle_on
                        } else {
                            app.theme.icons.toggle_off
                        }
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
            " {}: {}  {}  {} {}",
            app.theme
                .icons
                .label(app.theme.icons.branch, "Current Branch"),
            app.wt.branch_label,
            operations,
            if app.theme.icons.nerd {
                "\u{2502}"
            } else {
                "|"
            },
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
    for (i, (label, action, write)) in worktree_buttons(app)
        .into_iter()
        .enumerate()
        .take(usize::from(actions.height))
    {
        let row = row(actions, i as u16);
        let width = (label.width() as u16 + 2).min(row.width);
        button(
            f,
            app,
            Rect::new(row.right() - width, row.y, width, row.height),
            &label,
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

/// git gui's worktree action column: label, action, and whether it writes.
fn worktree_buttons(app: &App) -> [(String, Action, bool); 5] {
    let i = &app.theme.icons;
    let stage = if app.wt.filter_unstaged.text.trim().is_empty() {
        "Stage Changed"
    } else {
        "Stage Visible"
    };
    [
        (i.label(i.rescan, "Rescan"), Action::Rescan, false),
        (i.label(i.stage, stage), Action::StageChanged, true),
        (i.label(i.sign_off, "Sign Off"), Action::SignOff, false),
        (i.label(i.commit, "Commit"), Action::Commit, true),
        (i.label(i.push, "Push"), Action::Push, true),
    ]
}

fn worktree_files(f: &mut Frame, app: &mut App, r: Rect, list: FileList) {
    let staged = list == FileList::Staged;
    let id = if staged {
        PaneId::WStaged
    } else {
        PaneId::WUnstaged
    };
    let icons = &app.theme.icons;
    let count = if app.wt.filter(list).text.trim().is_empty() {
        app.wt.all_files(list).len().to_string()
    } else {
        format!(
            "{}/{}",
            app.wt.list(list).len(),
            app.wt.all_files(list).len()
        )
    };
    let title = icons.label(
        if staged { icons.staged } else { icons.unstaged },
        &format!(
            "{} ({count})",
            if staged {
                "Staged Changes"
            } else {
                "Unstaged Changes"
            }
        ),
    );
    let inner = pane(f, r, &title, id, app);
    let filter_row = row(inner, 0);
    text(f, filter_row, "Filter: ", app.theme.dimmed());
    let entry = Rect::new(
        filter_row.x + 8,
        filter_row.y,
        filter_row.width.saturating_sub(12),
        filter_row.height,
    );
    let focused = app.wt.filter_focus == Some(list) && app.focused_pane() == id && unobscured(app);
    input(f, app.wt.filter_mut(list), entry, focused, &app.theme);
    app.hits.push(entry, Hit::WtFilter(list));
    let clear = Rect::new(
        filter_row.right().saturating_sub(3),
        filter_row.y,
        3.min(filter_row.width),
        filter_row.height,
    );
    text(f, clear, "[x]", app.theme.dimmed());
    app.hits
        .push(clear, Hit::Button(Action::ClearFileFilter(list)));
    let inner = Rect::new(
        inner.x,
        inner.y + filter_row.height,
        inner.width,
        inner.height.saturating_sub(1),
    );
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
        let icons = &app.theme.icons;
        let status = file.icon_char(staged);
        let line = match icons.status(status) {
            Some(glyph) => {
                let t = &app.theme;
                let color = match status {
                    'A' => t.add,
                    'D' => t.del,
                    'M' => t.warn,
                    'U' => t.error,
                    '?' | '!' => t.dim,
                    _ => t.accent,
                };
                Line::from(vec![
                    Span::styled(" ", style),
                    Span::styled(glyph, t.commit_tint(style, color)),
                    Span::styled(
                        format!(
                            "  {}",
                            icons.label(icons.file(&file.display), &file.display)
                        ),
                        style,
                    ),
                ])
            }
            None if icons.nerd => Line::styled(
                format!(
                    " {status}  {}",
                    icons.label(icons.file(&file.display), &file.display)
                ),
                style,
            ),
            None => Line::styled(format!(" {status}  {}", file.display), style),
        };
        f.render_widget(Paragraph::new(line).style(style), rr);
        app.hits.push(rr, Hit::WtFile(list, i, false));
        app.hits.push(
            Rect::new(rr.x, rr.y, 3.min(rr.width), 1),
            Hit::WtFile(list, i, true),
        );
    }
    if len == 0 {
        let label = if app.wt.all_files(list).is_empty() {
            "(none)"
        } else {
            "(no matches)"
        };
        text(f, row(inner, 0), label, app.theme.dimmed());
    }
}

fn message(f: &mut Frame, app: &mut App, r: Rect) {
    let column = width_upto(
        &app.wt.message.lines[app.wt.message.cursor.row],
        app.wt.message.cursor.col,
    ) + 1;
    let title = app.theme.icons.label(
        app.theme.icons.message,
        &format!(
            "Commit Message - col {column}/{} - all {} staged",
            app.cfg.commit_width,
            app.wt.staged.len()
        ),
    );
    let controls = Rect::new(
        r.x.saturating_add(1),
        r.y,
        r.width.saturating_sub(2),
        u16::from(r.height > 0),
    );
    if controls.height > 0 {
        let icons = &app.theme.icons;
        let radio = |on: bool, label: &str| {
            if icons.nerd {
                format!(
                    "{} {label}",
                    if on { icons.radio_on } else { icons.radio_off }
                )
            } else if on {
                format!("* {label}")
            } else {
                label.to_string()
            }
        };
        let new_commit_label = radio(!app.wt.amend, "New Commit");
        let amend_label = radio(app.wt.amend, "Amend Last Commit");
        let new_commit_width = (new_commit_label.width() as u16 + 2).min(controls.width);
        button(
            f,
            app,
            Rect::new(controls.x, controls.y, new_commit_width, controls.height),
            &new_commit_label,
            Action::SetAmend(false),
            true,
        );
        let amend_width = (amend_label.width() as u16 + 2)
            .min(controls.width.saturating_sub(new_commit_width + 1));
        button(
            f,
            app,
            Rect::new(
                controls.x + new_commit_width + 1,
                controls.y,
                amend_width,
                controls.height,
            ),
            &amend_label,
            Action::SetAmend(true),
            !app.read_only,
        );
    }
    let frame = Rect::new(
        r.x,
        r.y.saturating_add(1),
        r.width,
        r.height.saturating_sub(1),
    );
    let inner = pane(f, frame, &title, PaneId::WMessage, app);
    if inner.height == 0 {
        return;
    }
    let area = inner;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::diffview::DLine,
        config::{Config, ThemeName},
    };
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn terminal_diff_selection_keeps_colors_with_highlighted_background() {
        let t = Theme::new(&Config {
            theme: ThemeName::Terminal,
            ..Config::default()
        });
        let mut words = DLine::plain(DKind::Words, "-old+new");
        words.segs = Some(vec![
            (DKind::Del, "-old".into()),
            (DKind::Add, "+new".into()),
        ]);
        let mut dv = DiffView {
            lines: vec![
                DLine::plain(DKind::Add, "+new"),
                DLine::plain(DKind::Del, "-old"),
                words,
            ],
            anchor: Some(0),
            cursor: 2,
            ..DiffView::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(12, 3)).unwrap();
        for focused in [true, false] {
            terminal
                .draw(|f| {
                    render_diff(
                        f,
                        &mut dv,
                        Rect::new(0, 0, 12, 3),
                        PaneId::HDiff,
                        focused,
                        &t,
                        &mut HitMap::default(),
                    );
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            for (x, y, fg) in [
                (0, 0, t.add),
                (0, 1, t.del),
                (0, 2, t.del),
                (4, 2, t.add),
                (11, 2, t.fg),
            ] {
                let cell = &buffer[(x, y)];
                assert_eq!(
                    cell.fg, fg,
                    "semantic colour must stay visible when selected"
                );
                assert_eq!(cell.bg, if focused { t.sel_bg } else { t.sel_dim_bg });
                assert_eq!(cell.modifier.contains(Modifier::BOLD), focused);
                assert!(!cell.modifier.contains(Modifier::REVERSED));
                assert!(!cell.modifier.contains(Modifier::UNDERLINED));
            }
        }
    }

    #[test]
    fn overflow_marker_takes_colour_of_dominant_hidden_kind() {
        let (tag, branch) = (Color::Yellow, Color::Green);
        let hidden = [
            (1, (RefKind::Tag, tag)),
            (1, (RefKind::LocalBranch, branch)),
            (3, (RefKind::Tag, tag)),
            (1, (RefKind::LocalBranch, branch)),
        ];
        assert_eq!(dominant_kind_color(&hidden), Some(tag));
        assert_eq!(dominant_kind_color(&hidden[..2]), Some(tag));
        assert_eq!(dominant_kind_color(&[]), None);
    }
}
