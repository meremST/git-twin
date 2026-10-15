use super::*;
use crate::app::{
    dialog::{Field, Kind, Tone},
    menu::MenuItem,
};
use ratatui::widgets::Clear;

pub(super) fn menus(f: &mut Frame, app: &mut App) {
    let Some(menu) = &mut app.menu else { return };
    let screen = f.area();
    app.hits.push(
        Rect::new(
            screen.x,
            screen.y + 1,
            screen.width,
            screen.height.saturating_sub(1),
        ),
        Hit::MenuBackdrop,
    );
    for (depth, level) in menu.levels.iter_mut().enumerate() {
        let w = crate::app::menu::menu_width(&level.items).min(screen.width);
        let h = (level.items.len() + 2).min(usize::from(screen.height.saturating_sub(1))) as u16;
        level.x = level.x.min(screen.right().saturating_sub(w));
        level.y = level.y.min(screen.bottom().saturating_sub(h)).max(1);
        let r = Rect::new(level.x, level.y, w, h);
        f.render_widget(Clear, r);
        let block = Block::bordered()
            .border_set(border(&app.theme))
            .style(app.theme.base().bg(app.theme.bg_alt))
            .border_style(app.theme.border_style(true));
        let inner = block.inner(r);
        f.render_widget(block, r);
        let visible = usize::from(inner.height).max(1);
        if level.sel < level.scroll {
            level.scroll = level.sel;
        } else if level.sel >= level.scroll + visible {
            level.scroll = level.sel + 1 - visible;
        }
        level.scroll = level.scroll.min(level.items.len().saturating_sub(visible));
        let top = level.scroll;
        for (line, (idx, item)) in level
            .items
            .iter()
            .enumerate()
            .skip(top)
            .take(usize::from(inner.height))
            .enumerate()
        {
            let label = match item {
                MenuItem::Entry {
                    label, key, check, ..
                } => {
                    let prefix = if *check == Some(true) {
                        app.theme.icons.mark
                    } else {
                        " "
                    };
                    let pad =
                        usize::from(inner.width).saturating_sub(label.width() + key.width() + 3);
                    format!("{prefix} {label}{}{key}", " ".repeat(pad))
                }
                MenuItem::Sub { label, .. } => format!("  {label}  {}", app.theme.icons.submenu),
                MenuItem::Sep => app.theme.icons.separator.repeat(usize::from(inner.width)),
            };
            let style = if idx == level.sel {
                app.theme.base().patch(app.theme.selected(true))
            } else if item.selectable() {
                app.theme.base()
            } else {
                app.theme.dimmed()
            };
            let rr = row(inner, line as u16);
            text(f, rr, label, style);
            app.hits.push(rr, Hit::MenuItem { depth, idx });
        }
    }
}

fn tone(t: &Theme, tone: Tone) -> Style {
    match tone {
        Tone::Normal => t.base(),
        Tone::Strong => t.base().add_modifier(Modifier::BOLD),
        Tone::Warn => t.base().fg(t.warn),
        Tone::Error => t.base().fg(t.error),
        Tone::Code => t.base().fg(t.hunk),
        Tone::Keyword => t.base().fg(t.meta).add_modifier(Modifier::BOLD),
        Tone::Str => t.base().fg(t.add),
        Tone::Comment => t.dimmed(),
        Tone::Number => t.base().fg(t.warn),
    }
}

fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for line in s.split('\n') {
        let mut current = String::new();
        let mut cells = 0;
        for g in line.graphemes(true) {
            if cells + g.width() > width.max(1) && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                cells = 0;
            }
            current.push_str(g);
            cells += g.width();
        }
        lines.push(current);
    }
    lines
}

enum FormRow {
    Label(String, Style, Option<Hit>),
    Input(usize),
    Area(usize, usize),
    Item(usize, usize),
    Search(usize),
    Blank,
    /// Line index into `Dialog::code` (file viewer), rendered unwrapped with
    /// a line-number gutter and horizontal scroll.
    Code(usize),
}

pub(super) fn dialog(f: &mut Frame, app: &mut App) {
    let Some(d) = app.dialogs.last_mut() else {
        return;
    };
    let screen = f.area();
    app.hits.push(screen, Hit::DlgBackdrop);
    let w = d.width.min(screen.width.saturating_sub(4)).max(4);
    let width = usize::from(w - 2);
    let mut buttons = Vec::new();
    let (mut bx, mut by) = (0, 0);
    for (i, label) in d.buttons.iter().enumerate() {
        let label = format!("[ {label} ]");
        let bw = label.width().min(width) as u16;
        if bx + bw > w - 2 {
            bx = 0;
            by += 1;
        }
        for (line, s) in wrap(&label, width).into_iter().enumerate() {
            buttons.push((i, bx, by + line as u16, bw, s));
        }
        by = buttons.last().map_or(by, |b| b.2);
        bx += bw;
    }
    let footer = by + 2; // Buttons and a fixed scroll hint.
    let max_content = usize::from(screen.height.saturating_sub(footer + 4)).max(1);
    let reveal = d.reveal_focus || d.last_focus != Some(d.focus);
    let mut rows = Vec::new();
    if matches!(d.kind, Kind::FileView(_)) {
        rows.extend((0..d.code.len()).map(FormRow::Code));
    } else {
        for (s, t) in &d.body {
            rows.extend(
                wrap(&sanitize(s), width)
                    .into_iter()
                    .map(|s| FormRow::Label(s, tone(&app.theme, *t), None)),
            );
        }
    }
    let mut ranges = Vec::new();
    let visible: Vec<bool> = (0..d.fields.len()).map(|i| d.field_visible(i)).collect();
    for (i, field) in d.fields.iter_mut().enumerate() {
        let start = rows.len();
        if !visible[i] {
            ranges.push((start, start));
            continue;
        }
        let focused = d.focus == i;
        let style = if focused {
            app.theme.base().patch(app.theme.selected(true))
        } else {
            app.theme.base()
        };
        match field {
            Field::Text { label, .. } => {
                rows.extend(
                    wrap(label, width)
                        .into_iter()
                        .map(|s| FormRow::Label(s, style, None)),
                );
                rows.push(FormRow::Input(i));
            }
            Field::Area {
                label,
                area,
                rows: height,
            } => {
                let labels = wrap(label, width);
                let h = usize::from(*height).min(max_content.saturating_sub(labels.len()).max(1));
                if focused && reveal {
                    area.scroll_to_cursor(h, width);
                }
                rows.extend(labels.into_iter().map(|s| FormRow::Label(s, style, None)));
                rows.extend((0..h).map(|line| FormRow::Area(i, line)));
            }
            Field::List {
                label,
                items,
                rows: height,
                sel,
                top,
                search,
                ..
            } => {
                let labels = wrap(label, width);
                let searching = search.as_ref().is_some_and(|s| s.active);
                let h = usize::from(*height).min(max_content.saturating_sub(labels.len()).max(1));
                if focused && reveal {
                    if *sel < *top {
                        *top = *sel;
                    } else if *sel >= *top + h {
                        *top = sel.saturating_add(1).saturating_sub(h);
                    }
                }
                *top = (*top).min(items.len().saturating_sub(h));
                rows.extend(labels.into_iter().map(|s| FormRow::Label(s, style, None)));
                if searching {
                    rows.push(FormRow::Search(i));
                }
                let first = rows.len();
                rows.extend(
                    (*top..items.len())
                        .take(h)
                        .map(|item| FormRow::Item(i, item)),
                );
                if items.is_empty() {
                    rows.push(FormRow::Label("(none)".into(), app.theme.dimmed(), None));
                }
                // Searchable lists keep a stable height while filtering.
                if search.is_some() {
                    while rows.len() < first + h {
                        rows.push(FormRow::Blank);
                    }
                }
            }
            Field::Check { label, value } => {
                let icons = &app.theme.icons;
                let s = format!(
                    "{} {label}",
                    if *value {
                        icons.check_on
                    } else {
                        icons.check_off
                    }
                );
                rows.extend(
                    wrap(&s, width)
                        .into_iter()
                        .map(|s| FormRow::Label(s, style, Some(Hit::DlgField(i)))),
                );
            }
            Field::Choice {
                label,
                options,
                sel,
                dropdown,
                radio,
            } if *radio => {
                rows.extend(
                    wrap(label, width)
                        .into_iter()
                        .map(|s| FormRow::Label(s, style, Some(Hit::DlgField(i)))),
                );
                for (option, text) in options.iter().enumerate() {
                    let mark = if option == *sel { "(•)" } else { "( )" };
                    rows.extend(
                        wrap(&format!("  {mark} {text}"), width)
                            .into_iter()
                            .map(|s| FormRow::Label(s, style, Some(Hit::DlgChoice(i, option)))),
                    );
                }
            }
            Field::Choice {
                label,
                options,
                sel,
                dropdown,
                ..
            } => {
                let value = options.get(*sel).map(String::as_str).unwrap_or("");
                let icons = &app.theme.icons;
                let s = if *dropdown {
                    format!("{label}: [ {value} {} ]", icons.dropdown)
                } else {
                    format!("{label}: {} {value} {}", icons.left, icons.right)
                };
                rows.extend(
                    wrap(&s, width)
                        .into_iter()
                        .map(|s| FormRow::Label(s, style, Some(Hit::DlgField(i)))),
                );
                if let Some((field, highlighted)) = d.open_choice
                    && field == i
                {
                    for (option, label) in options.iter().enumerate() {
                        let style = if option == highlighted {
                            app.theme.base().patch(app.theme.selected(true))
                        } else {
                            app.theme.base().bg(app.theme.bg_alt)
                        };
                        let marker = if option == *sel {
                            app.theme.icons.mark
                        } else {
                            " "
                        };
                        rows.extend(
                            wrap(&format!("  {marker} {label}"), width)
                                .into_iter()
                                .map(|s| FormRow::Label(s, style, Some(Hit::DlgChoice(i, option)))),
                        );
                    }
                }
            }
            Field::Label(s, t) => {
                rows.extend(
                    wrap(s, width)
                        .into_iter()
                        .map(|s| FormRow::Label(s, tone(&app.theme, *t), None)),
                );
            }
        }
        ranges.push((start, rows.len()));
    }
    let gutter_w = (d.code.len().max(1).to_string().len() as u16 + 1).max(3);
    let content_h = rows.len().min(max_content).max(1);
    let height = content_h as u16 + footer + 2;
    let r = Rect::new(
        screen.x + (screen.width - w) / 2,
        screen.y + (screen.height - height) / 2,
        w,
        height,
    );
    f.render_widget(Clear, r);
    let block = Block::bordered()
        .border_set(border(&app.theme))
        .title(format!(" {} ", sanitize(&d.title)))
        .style(app.theme.base())
        .border_style(app.theme.border_style(true));
    let inner = block.inner(r);
    f.render_widget(block, r);
    d.viewport_height = content_h;
    d.content_height = rows.len();
    app.hits.push(
        Rect::new(inner.x, inner.y, inner.width, content_h as u16),
        Hit::Pane(PaneId::Dialog),
    );
    if reveal && let Some(&(start, end)) = ranges.get(d.focus) {
        if start < d.scroll {
            d.scroll = start;
        }
        if end > d.scroll + content_h {
            d.scroll = end.saturating_sub(content_h);
        }
    }
    d.scroll = d.scroll.min(rows.len().saturating_sub(content_h));
    d.last_focus = Some(d.focus);
    d.reveal_focus = false;
    for (line, item) in rows.into_iter().skip(d.scroll).take(content_h).enumerate() {
        let rr = row(inner, line as u16);
        match item {
            FormRow::Label(s, style, hit) => {
                text(f, rr, s, style);
                if let Some(hit) = hit {
                    app.hits.push(rr, hit);
                }
            }
            FormRow::Code(i) => {
                if let Some(spans) = d.code.get(i) {
                    let gr = Rect::new(rr.x, rr.y, gutter_w.min(rr.width), 1);
                    text(
                        f,
                        gr,
                        format!("{:>w$} ", i + 1, w = usize::from(gutter_w) - 1),
                        app.theme.dimmed(),
                    );
                    let cr = Rect::new(
                        rr.x + gutter_w.min(rr.width),
                        rr.y,
                        rr.width.saturating_sub(gutter_w),
                        1,
                    );
                    let line_spans: Vec<Span<'_>> = spans
                        .iter()
                        .map(|(s, t)| Span::styled(s.clone(), tone(&app.theme, *t)))
                        .collect();
                    f.render_widget(
                        Paragraph::new(Line::from(line_spans))
                            .style(app.theme.base())
                            .scroll((0, d.hscroll.min(u16::MAX as usize) as u16)),
                        cr,
                    );
                }
            }
            FormRow::Input(i) => {
                if let Field::Text { input: value, .. } = &mut d.fields[i] {
                    input(f, value, rr, d.focus == i, &app.theme);
                }
                app.hits.push(rr, Hit::DlgField(i));
            }
            FormRow::Area(i, offset) => {
                if let Field::Area { area, .. } = &mut d.fields[i] {
                    let top = area.top;
                    area.top += offset;
                    textarea(f, area, rr, d.focus == i, &app.theme);
                    area.top = top;
                }
                app.hits.push(rr, Hit::DlgArea(i, offset));
            }
            FormRow::Search(i) => {
                if let Field::List {
                    items,
                    search: Some(s),
                    ..
                } = &mut d.fields[i]
                {
                    let shown = items.len();
                    let prompt = if app.theme.icons.nerd {
                        format!("{} ", app.theme.icons.search)
                    } else {
                        "Filter: ".to_string()
                    };
                    let pw = prompt.width() as u16;
                    text(f, rr, &prompt, app.theme.base());
                    let count = format!("{}/{}", shown, s.total());
                    let cw = count.len() as u16 + 1;
                    let ir = Rect::new(rr.x + pw, rr.y, rr.width.saturating_sub(pw + cw), 1);
                    input(f, &mut s.input, ir, d.focus == i, &app.theme);
                    let cr = Rect::new(rr.right().saturating_sub(cw), rr.y, cw, 1);
                    text(f, cr, count, app.theme.dimmed());
                }
            }
            FormRow::Blank => {}
            FormRow::Item(i, item) => {
                if let Field::List {
                    items,
                    values,
                    sel,
                    search,
                    ..
                } = &d.fields[i]
                {
                    let disabled = search
                        .as_ref()
                        .and_then(|s| s.dim.get(item))
                        .unwrap_or(&false);
                    let base = if *disabled {
                        app.theme.dimmed()
                    } else {
                        app.theme.base()
                    };
                    let style = if *sel == item {
                        base.patch(app.theme.selected(d.focus == i))
                    } else {
                        base
                    };
                    let line = match (app.theme.icons.nerd, items[item].split_once(' ')) {
                        (true, Some((icon, name))) => {
                            let color = values
                                .get(item)
                                .and_then(|full| app.refs.find_full(full))
                                .map(|r| {
                                    if app.refs.is_head_ref(r) {
                                        app.theme.head
                                    } else {
                                        match r.kind {
                                            RefKind::LocalBranch => app.theme.branch,
                                            RefKind::RemoteBranch => app.theme.remote,
                                            RefKind::Tag => app.theme.tag,
                                            _ => app.theme.meta,
                                        }
                                    }
                                });
                            match color {
                                Some(c) => ratatui::text::Line::from(vec![
                                    ratatui::text::Span::styled(
                                        icon.to_string(),
                                        style.patch(Style::default().fg(c)),
                                    ),
                                    ratatui::text::Span::styled(format!(" {name}"), style),
                                ]),
                                None => ratatui::text::Line::styled(items[item].clone(), style),
                            }
                        }
                        _ => ratatui::text::Line::styled(items[item].clone(), style),
                    };
                    f.render_widget(Paragraph::new(line), rr);
                }
                app.hits.push(rr, Hit::DlgListRow(i, item));
            }
        }
    }
    text(
        f,
        row(inner, content_h as u16),
        format!(
            "Rows {}-{}/{}  | Wheel / Alt-PgUp/PgDn | Tab: fields",
            d.scroll + 1,
            (d.scroll + content_h).min(d.content_height),
            d.content_height
        ),
        app.theme.dimmed(),
    );
    for (i, x, y, w, label) in buttons {
        let rr = Rect::new(inner.x + x, inner.y + content_h as u16 + 1 + y, w, 1);
        text(
            f,
            rr,
            label,
            if d.focus == d.fields.len() + i {
                app.theme.button(true).patch(app.theme.selected(true))
            } else {
                app.theme.button(true)
            },
        );
        app.hits.push(rr, Hit::DlgButton(i));
    }
}
