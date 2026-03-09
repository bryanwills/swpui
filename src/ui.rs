use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize as _};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::types::{MatchMode, Pane};

pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Main layout: content area + status bar
    let [content_area, status_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

    // Split content into left and right columns
    let [left, right] = Layout::horizontal([Constraint::Percentage(40), Constraint::Fill(1)])
        .areas(content_area);

    // Left column: input area + file list
    let [input_area, file_area] =
        Layout::vertical([Constraint::Length(4), Constraint::Fill(1)]).areas(left);

    render_input_area(app, frame, input_area);
    render_file_list(app, frame, file_area);
    render_preview(app, frame, right);
    render_status_bar(app, frame, status_area);
}

fn focused_border_style(pane: Pane, current: Pane) -> Style {
    if pane == current {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn render_input_area(app: &App, frame: &mut Frame, area: Rect) {
    let [search_area, replace_area] =
        Layout::vertical([Constraint::Length(2), Constraint::Length(2)]).areas(area);

    let mode_label = match app.match_mode {
        MatchMode::Literal => "Search (literal)".to_string(),
        MatchMode::Regex => "Search (regex)".to_string(),
    };

    let search_border_style =
        if app.status_message.as_ref().is_some_and(|msg| msg.contains("regex parse error")) {
            Style::default().fg(Color::Red)
        } else {
            focused_border_style(Pane::SearchInput, app.focused_pane)
        };

    // Search input
    let search_block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(search_border_style)
        .title(mode_label);
    let search_inner = search_block.inner(search_area);
    frame.render_widget(search_block, search_area);
    frame.render_widget(
        Paragraph::new(app.search_input.value()),
        search_inner,
    );
    if app.focused_pane == Pane::SearchInput {
        frame.set_cursor_position((
            search_inner.x + app.search_input.cursor() as u16,
            search_inner.y,
        ));
    }

    // Replace input
    let replace_block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(focused_border_style(Pane::ReplaceInput, app.focused_pane))
        .title("Replace");
    let replace_inner = replace_block.inner(replace_area);
    frame.render_widget(replace_block, replace_area);
    frame.render_widget(
        Paragraph::new(app.replace_input.value()),
        replace_inner,
    );
    if app.focused_pane == Pane::ReplaceInput {
        frame.set_cursor_position((
            replace_inner.x + app.replace_input.cursor() as u16,
            replace_inner.y,
        ));
    }
}

fn render_file_list(app: &App, frame: &mut Frame, area: Rect) {
    let title = format!("Files ({} matched)", app.results.len());
    let block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(focused_border_style(Pane::FileList, app.focused_pane))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.results.is_empty() {
        frame.render_widget(
            Paragraph::new("No matches").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .results
        .iter()
        .enumerate()
        .map(|(i, fm)| {
            let active = fm.active_match_count();
            let total = fm.matches.len();
            let label = format!("{} ({}/{})", fm.path.display(), active, total);
            let style = if i == app.selected_file {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if active == 0 {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };
            ListItem::new(label).style(style)
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, inner);
}

fn render_preview(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(focused_border_style(Pane::Preview, app.focused_pane))
        .title("Preview");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(fm) = app.results.get(app.selected_file) else {
        frame.render_widget(
            Paragraph::new("Select a file").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    };

    let replacement = app.replace_input.value();
    let mut lines: Vec<Line> = vec![];

    for (match_idx, m) in fm.matches.iter().enumerate() {
        let is_selected = app.focused_pane == Pane::Preview && match_idx == app.selected_match;

        // Header line for this match
        let header_style = if is_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(Span::styled(
            format!("  line {}:", m.line_number),
            header_style,
        )));

        // Context before
        for ctx in &m.context_before {
            lines.push(Line::from(Span::styled(
                format!("  {}", ctx.content),
                Style::default().fg(Color::DarkGray),
            )));
        }

        // The match line itself
        if m.skip {
            lines.push(Line::from(Span::styled(
                format!("  {} [skipped]", m.matched_text),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::CROSSED_OUT),
            )));
        } else if !replacement.is_empty() {
            // Inline diff: old strikethrough, then new
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    &m.matched_text,
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::CROSSED_OUT),
                ),
                Span::styled(" -> ", Style::default().fg(Color::DarkGray)),
                Span::styled(replacement, Style::default().fg(Color::Green)),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                format!("  {}", m.matched_text),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
        }

        // Context after
        for ctx in &m.context_after {
            lines.push(Line::from(Span::styled(
                format!("  {}", ctx.content),
                Style::default().fg(Color::DarkGray),
            )));
        }

        lines.push(Line::raw(""));
    }

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_status_bar(app: &App, frame: &mut Frame, area: Rect) {
    let line = if let Some(msg) = &app.status_message {
        Line::from(Span::styled(
            msg.as_str(),
            Style::default().fg(Color::Red),
        ))
    } else {
        let hints = match app.focused_pane {
            Pane::SearchInput | Pane::ReplaceInput => {
                "Esc: file list | Tab/S-Tab: cycle | Ctrl-r: toggle regex | Ctrl-c: quit"
            }
            Pane::FileList => {
                "j/k: navigate | l/Enter: preview | a: apply all | f: apply file | q: quit"
            }
            Pane::Preview => {
                "j/k: navigate | Space: toggle skip | Enter: apply match | h/Esc: back | a: apply all"
            }
        };
        Line::from(hints.blue())
    };
    frame.render_widget(line, area);
}
