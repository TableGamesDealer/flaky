use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::model::OptionType;
use crate::tui::app::{App, AppMode, ConfirmKind, ListItem as AppListItem, Screen};

// ── Colour palette ──────────────────────────────────────────────────────────
const NIX_BLUE: Color = Color::Rgb(82, 118, 178);
const NIX_DARK: Color = Color::Rgb(30, 30, 40);
const MUTED: Color = Color::Rgb(120, 120, 140);
const HIGHLIGHT: Color = Color::Rgb(255, 210, 80);
const SUCCESS: Color = Color::Rgb(80, 200, 120);
const DANGER: Color = Color::Rgb(220, 80, 80);
const BORDER: Color = Color::Rgb(60, 60, 80);

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    // Three-row layout: header | body | footer
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(1),    // body
            Constraint::Length(3), // footer / action bar
        ])
        .split(area);

    draw_header(f, app, rows[0]);

    match app.current_screen() {
        Screen::Loading { message } => draw_loading(f, app, rows[1], message),
        Screen::CategoryList { path } => draw_category_list(f, app, rows[1], path),
        Screen::EditOption { option_name } => draw_edit_option(f, app, rows[1], option_name),
        Screen::Search { query } => draw_search(f, app, rows[1], query),
        Screen::Log { lines, title } => draw_log(f, rows[1], lines, title),
        Screen::Error { message } => draw_error(f, rows[1], message),
        Screen::Confirm(kind) => {
            // Draw the underlying screen first, then overlay the dialog.
            if app.screen_stack.len() >= 2 {
                let under = &app.screen_stack[app.screen_stack.len() - 2];
                match under {
                    Screen::CategoryList { path } => draw_category_list(f, app, rows[1], path),
                    _ => {}
                }
            }
            draw_confirm_dialog(f, area, kind);
        }
    }

    draw_footer(f, app, rows[2]);
}

// ── Header ──────────────────────────────────────────────────────────────────

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let dirty = app.state.is_dirty();
    let pending = app.state.pending_count();

    let breadcrumb = match app.current_screen() {
        Screen::CategoryList { path } => {
            if path.is_empty() {
                " nixcfg › system configuration".to_string()
            } else {
                format!(" nixcfg › {}", path.join(" › "))
            }
        }
        Screen::EditOption { option_name } => format!(" nixcfg › {option_name}"),
        Screen::Search { .. } => " nixcfg › search".to_string(),
        _ => " nixcfg".to_string(),
    };

    let dirty_indicator = if dirty {
        Span::styled(
            format!(" ● {pending} unsaved"),
            Style::default().fg(HIGHLIGHT),
        )
    } else {
        Span::styled(" ✓ saved", Style::default().fg(SUCCESS))
    };

    let title = Line::from(vec![
        Span::styled(
            breadcrumb,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        dirty_indicator,
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(NIX_BLUE))
        .style(Style::default().bg(NIX_DARK));

    let para = Paragraph::new(title).block(block);
    f.render_widget(para, area);
}

// ── Category / option list ───────────────────────────────────────────────────

fn draw_category_list(f: &mut Frame, app: &App, area: Rect, path: &[String]) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    // Left: list
    let items: Vec<ListItem> = app
        .current_list_items()
        .iter()
        .map(|item| render_list_item(item, app))
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(app.cursor));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER))
                .title(Span::styled(
                    if path.is_empty() {
                        " Categories "
                    } else {
                        " Options "
                    },
                    Style::default().fg(NIX_BLUE).add_modifier(Modifier::BOLD),
                )),
        )
        .highlight_style(
            Style::default()
                .bg(NIX_BLUE)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    f.render_stateful_widget(list, cols[0], &mut list_state);

    // Right: detail panel for the selected item
    draw_detail_panel(f, app, cols[1]);
}

fn render_list_item<'a>(item: &AppListItem, app: &App) -> ListItem<'a> {
    match item {
        AppListItem::Category(path) => {
            let name = path.rsplit('.').next().unwrap_or(path.as_str());
            ListItem::new(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    format!("{name}"),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  ›", Style::default().fg(MUTED)),
            ]))
        }
        AppListItem::Option(name) => {
            let short = name.rsplit('.').next().unwrap_or(name.as_str());
            let value_str = app
                .state
                .get(name)
                .map(|v| v.display())
                .or_else(|| {
                    app.schema
                        .options
                        .get(name)
                        .and_then(|o| o.default.as_ref())
                        .map(|d| format!("{} (default)", d.display()))
                })
                .unwrap_or_else(|| "—".into());

            let is_dirty = app.state.dirty_names().contains(&name);

            ListItem::new(Line::from(vec![
                Span::styled(
                    if is_dirty { "● " } else { "  " },
                    Style::default().fg(HIGHLIGHT),
                ),
                Span::styled(format!("{short:<28}"), Style::default().fg(Color::White)),
                Span::styled(
                    value_str,
                    Style::default().fg(if is_dirty { HIGHLIGHT } else { MUTED }),
                ),
            ]))
        }
    }
}

fn draw_detail_panel(f: &mut Frame, app: &App, area: Rect) {
    let selected = app.selected_item();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Span::styled(
            " Details ",
            Style::default().fg(NIX_BLUE).add_modifier(Modifier::BOLD),
        ));

    let content: Text = match &selected {
        Some(AppListItem::Option(name)) => {
            if let Some(opt) = app.schema.options.get(name.as_str()) {
                let mut lines = vec![
                    Line::from(Span::styled(
                        &opt.name,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Type:  ", Style::default().fg(MUTED)),
                        Span::styled(type_label(&opt.option_type), Style::default().fg(NIX_BLUE)),
                    ]),
                ];

                if let Some(val) = app.state.get(name) {
                    lines.push(Line::from(vec![
                        Span::styled("Value: ", Style::default().fg(MUTED)),
                        Span::styled(val.display(), Style::default().fg(HIGHLIGHT)),
                    ]));
                }

                if let Some(default) = &opt.default {
                    lines.push(Line::from(vec![
                        Span::styled("Default: ", Style::default().fg(MUTED)),
                        Span::styled(default.display(), Style::default().fg(MUTED)),
                    ]));
                }

                lines.push(Line::from(""));

                // Wrap description to panel width.
                for word_line in
                    wrap_text(&opt.description, (area.width.saturating_sub(4)) as usize)
                {
                    lines.push(Line::from(Span::styled(
                        word_line,
                        Style::default().fg(Color::Gray),
                    )));
                }

                if let Some(src) = &opt.declared_in {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("⟨ {src} ⟩"),
                        Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
                    )));
                }

                Text::from(lines)
            } else {
                Text::raw("No option data")
            }
        }
        Some(AppListItem::Category(path)) => {
            let name = path.rsplit('.').next().unwrap_or(path.as_str());
            let count = app
                .schema
                .options
                .keys()
                .filter(|k| k.starts_with(&format!("{path}.")))
                .count();
            Text::from(vec![
                Line::from(Span::styled(
                    name,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!("{count} options"),
                    Style::default().fg(MUTED),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Press Enter to expand",
                    Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
                )),
            ])
        }
        None => Text::raw("Nothing selected"),
    };

    let para = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

// ── Edit option screen ───────────────────────────────────────────────────────

fn draw_edit_option(f: &mut Frame, app: &App, area: Rect, option_name: &str) {
    let Some(opt) = app.schema.options.get(option_name) else {
        return;
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6), // option info
            Constraint::Length(5), // value editor
            Constraint::Min(1),    // description
        ])
        .split(area);

    // Option info box
    let current_val = app
        .state
        .get(option_name)
        .map(|v| v.display())
        .or_else(|| {
            opt.default
                .as_ref()
                .map(|d| format!("{} (default)", d.display()))
        })
        .unwrap_or_else(|| "not set".into());

    let info_text = Text::from(vec![
        Line::from(Span::styled(
            &opt.name,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("type:    ", Style::default().fg(MUTED)),
            Span::styled(type_label(&opt.option_type), Style::default().fg(NIX_BLUE)),
        ]),
        Line::from(vec![
            Span::styled("current: ", Style::default().fg(MUTED)),
            Span::styled(&current_val, Style::default().fg(HIGHLIGHT)),
        ]),
    ]);

    let info = Paragraph::new(info_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(NIX_BLUE)),
    );
    f.render_widget(info, rows[0]);

    // Value editor — widget depends on type
    draw_value_editor(f, app, rows[1], opt, option_name);

    // Description
    let desc_lines: Vec<Line> =
        wrap_text(&opt.description, (area.width.saturating_sub(4)) as usize)
            .into_iter()
            .map(|l| Line::from(Span::styled(l, Style::default().fg(Color::Gray))))
            .collect();

    let desc = Paragraph::new(Text::from(desc_lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER))
                .title(Span::styled(" Description ", Style::default().fg(MUTED))),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(desc, rows[2]);
}

fn draw_value_editor(
    f: &mut Frame,
    app: &App,
    area: Rect,
    opt: &crate::model::NixOption,
    name: &str,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if app.mode == AppMode::Editing {
            HIGHLIGHT
        } else {
            BORDER
        }))
        .title(Span::styled(" Value ", Style::default().fg(MUTED)));

    match &opt.option_type {
        OptionType::Bool => {
            let val = app
                .state
                .get(name)
                .and_then(|v| {
                    if let crate::model::OptionValue::Bool(b) = v {
                        Some(*b)
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    opt.default.as_ref().and_then(|d| {
                        if let crate::model::OptionValue::Bool(b) = d {
                            Some(*b)
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or(false);

            let toggle = Paragraph::new(Line::from(vec![
                Span::styled(
                    if val {
                        "  [ ON ]  off  "
                    } else {
                        "  on  [ OFF ]  "
                    },
                    Style::default()
                        .fg(if val { SUCCESS } else { DANGER })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("   space to toggle", Style::default().fg(MUTED)),
            ]))
            .block(block);
            f.render_widget(toggle, area);
        }

        OptionType::Enum { values } => {
            let current = app
                .state
                .get(name)
                .and_then(|v| {
                    if let crate::model::OptionValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    opt.default.as_ref().and_then(|d| {
                        if let crate::model::OptionValue::Str(s) = d {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_default();

            let spans: Vec<Span> = values
                .iter()
                .flat_map(|v| {
                    if v == &current {
                        vec![Span::styled(
                            format!(" [ {v} ] "),
                            Style::default().fg(HIGHLIGHT).add_modifier(Modifier::BOLD),
                        )]
                    } else {
                        vec![Span::styled(format!("  {v}  "), Style::default().fg(MUTED))]
                    }
                })
                .collect();

            let mut all = vec![Span::styled("← ", Style::default().fg(MUTED))];
            all.extend(spans);
            all.push(Span::styled(" →", Style::default().fg(MUTED)));

            let para = Paragraph::new(Line::from(all)).block(block);
            f.render_widget(para, area);
        }

        OptionType::Str | OptionType::Path | OptionType::Int { .. } | OptionType::Float => {
            let display = if app.mode == AppMode::Editing {
                // Show the live input buffer with cursor.
                let before = &app.input_buf[..app.input_cursor.min(app.input_buf.len())];
                let after = &app.input_buf[app.input_cursor.min(app.input_buf.len())..];
                Line::from(vec![
                    Span::styled(before.to_string(), Style::default().fg(Color::White)),
                    Span::styled(
                        "│",
                        Style::default()
                            .fg(HIGHLIGHT)
                            .add_modifier(Modifier::SLOW_BLINK),
                    ),
                    Span::styled(after.to_string(), Style::default().fg(Color::White)),
                    Span::styled(
                        "   enter to confirm  esc to cancel",
                        Style::default().fg(MUTED),
                    ),
                ])
            } else {
                let val_str = app
                    .state
                    .get(name)
                    .map(|v| v.display())
                    .or_else(|| opt.default.as_ref().map(|d| d.display()))
                    .unwrap_or_else(|| "—".into());
                Line::from(vec![
                    Span::styled(val_str, Style::default().fg(Color::White)),
                    Span::styled("   enter to edit", Style::default().fg(MUTED)),
                ])
            };

            let para = Paragraph::new(display).block(block);
            f.render_widget(para, area);
        }

        OptionType::Nullable { .. } => {
            let is_null = app
                .state
                .get(name)
                .map(|v| matches!(v, crate::model::OptionValue::Null))
                .unwrap_or(true);

            let para = Paragraph::new(Line::from(vec![
                Span::styled(
                    if is_null {
                        "  [ disabled ]  enable  "
                    } else {
                        "  disable  [ enabled ]  "
                    },
                    Style::default()
                        .fg(if is_null { MUTED } else { SUCCESS })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("   space to toggle", Style::default().fg(MUTED)),
            ]))
            .block(block);
            f.render_widget(para, area);
        }

        _ => {
            let hint = opt.option_type.widget_hint();
            let para = Paragraph::new(format!("Complex type — {hint}")).block(block);
            f.render_widget(para, area);
        }
    }
}

// ── Search screen ────────────────────────────────────────────────────────────

fn draw_search(f: &mut Frame, app: &App, area: Rect, query: &str) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    // Search box
    let search_para = Paragraph::new(Line::from(vec![
        Span::styled(
            "/ ",
            Style::default().fg(NIX_BLUE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(query, Style::default().fg(Color::White)),
        Span::styled(
            "│",
            Style::default()
                .fg(HIGHLIGHT)
                .add_modifier(Modifier::SLOW_BLINK),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(NIX_BLUE)),
    );
    f.render_widget(search_para, rows[0]);

    // Results
    let items: Vec<ListItem> = app
        .current_list_items()
        .iter()
        .map(|item| render_list_item(item, app))
        .collect();

    let count = items.len();
    let mut list_state = ListState::default();
    list_state.select(Some(app.cursor));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER))
                .title(Span::styled(
                    format!(" {count} results "),
                    Style::default().fg(MUTED),
                )),
        )
        .highlight_style(Style::default().bg(NIX_BLUE).fg(Color::White))
        .highlight_symbol("▸ ");

    f.render_stateful_widget(list, rows[1], &mut list_state);
}

// ── Loading / Log / Error ────────────────────────────────────────────────────

fn draw_loading(f: &mut Frame, _app: &App, area: Rect, message: &str) {
    let para = Paragraph::new(format!("⟳  {message}"))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(NIX_BLUE)),
        );
    f.render_widget(para, area);
}

fn draw_log(f: &mut Frame, area: Rect, lines: &[String], title: &str) {
    let text: Vec<Line> = lines
        .iter()
        .map(|l| {
            let color = if l.contains("error") || l.contains("Error") {
                DANGER
            } else if l.contains("warn") {
                HIGHLIGHT
            } else {
                Color::Gray
            };
            Line::from(Span::styled(l.as_str(), Style::default().fg(color)))
        })
        .collect();

    let para = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER))
                .title(Span::styled(
                    format!(" {title} "),
                    Style::default().fg(NIX_BLUE).add_modifier(Modifier::BOLD),
                )),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn draw_error(f: &mut Frame, area: Rect, message: &str) {
    let para = Paragraph::new(message)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(DANGER))
                .title(Span::styled(
                    " Error ",
                    Style::default().fg(DANGER).add_modifier(Modifier::BOLD),
                )),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

// ── Confirm dialog ───────────────────────────────────────────────────────────

fn draw_confirm_dialog(f: &mut Frame, area: Rect, kind: &ConfirmKind) {
    let (title, body, color) = match kind {
        ConfirmKind::Save => (
            " Save changes? ",
            "Write pending changes to configuration.nix",
            SUCCESS,
        ),
        ConfirmKind::Apply => (
            " Apply to system? ",
            "Run nixos-rebuild switch — this modifies your running system",
            HIGHLIGHT,
        ),
        ConfirmKind::Discard => (
            " Discard changes? ",
            "All unsaved edits will be lost",
            DANGER,
        ),
        ConfirmKind::Quit => (
            " Quit with unsaved changes? ",
            "Pending edits will be lost",
            DANGER,
        ),
    };

    let dialog_area = centered_rect(50, 9, area);
    f.render_widget(Clear, dialog_area);

    let text = Text::from(vec![
        Line::from(""),
        Line::from(Span::styled(body, Style::default().fg(Color::White))),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  [ y ] yes   ",
                Style::default().fg(SUCCESS).add_modifier(Modifier::BOLD),
            ),
            Span::styled("[ n ] no / cancel  ", Style::default().fg(MUTED)),
        ]),
    ]);

    let dialog = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(color))
                .title(Span::styled(
                    title,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(dialog, dialog_area);
}

// ── Footer / action bar ──────────────────────────────────────────────────────

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let hint = match app.current_screen() {
        Screen::CategoryList { .. } => {
            "↑↓ navigate  enter expand/edit  / search  s save  a apply  u undo  ? help  q quit"
        }
        Screen::EditOption { option_name } => {
            let opt = app.schema.options.get(option_name.as_str());
            opt.map(|o| o.option_type.widget_hint())
                .unwrap_or("esc back")
        }
        Screen::Search { .. } => "↑↓ navigate  enter select  esc cancel",
        Screen::Confirm(_) => "y confirm  n / esc cancel",
        _ => "esc back  q quit",
    };

    let status = app.status_msg.as_deref().unwrap_or("");

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    let hint_para = Paragraph::new(Span::styled(format!(" {hint}"), Style::default().fg(MUTED)))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER)),
        );
    f.render_widget(hint_para, cols[0]);

    let status_style = if status.starts_with("error") || status.starts_with("Error") {
        Style::default().fg(DANGER)
    } else {
        Style::default().fg(SUCCESS)
    };

    let status_para = Paragraph::new(Span::styled(format!(" {status}"), status_style)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(BORDER)),
    );
    f.render_widget(status_para, cols[1]);
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn type_label(ty: &OptionType) -> String {
    match ty {
        OptionType::Bool => "bool".into(),
        OptionType::Int { .. } => "int".into(),
        OptionType::Float => "float".into(),
        OptionType::Str => "string".into(),
        OptionType::Path => "path".into(),
        OptionType::Package => "package".into(),
        OptionType::Enum { values } => format!("enum ({})", values.join(" | ")),
        OptionType::List { element } => format!("list of {}", type_label(element)),
        OptionType::Attrs { element } => format!("attrs of {}", type_label(element)),
        OptionType::Submodule { .. } => "submodule".into(),
        OptionType::Nullable { inner } => format!("null or {}", type_label(inner)),
        OptionType::Unknown { type_str } => type_str.clone(),
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.len() + 1 + word.len() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(current.clone());
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        if text.contains('\n') {
            lines.push(String::new());
        }
    }
    lines
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let popup_width = area.width * percent_x / 100;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect::new(area.x + x, area.y + y, popup_width, height)
}
