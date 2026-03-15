use std::ops::Range;

use rat_widget::scrolled::{Scroll, ScrollArea, ScrollAreaState};
use rat_widget::text::HasScreenCursor as _;
use rat_widget::text_input::TextInput;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize as _};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, Paragraph, StatefulWidget as _};

use crate::app::App;
use crate::types::{FileMatches, MatchMode, Pane};

pub fn render(app: &mut App, frame: &mut Frame) {
    let area = frame.area();

    // Main layout: content area + status bar
    let [content_area, status_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

    // Split content into left and right columns
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Fill(1)]).areas(content_area);

    // Left column: input area + file list
    let [input_area, file_area] =
        Layout::vertical([Constraint::Length(6), Constraint::Fill(1)]).areas(left);

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

fn render_input_area(app: &mut App, frame: &mut Frame, area: Rect) {
    let [search_area, replace_area] =
        Layout::vertical([Constraint::Length(3), Constraint::Length(3)]).areas(area);

    let mode_label = match app.match_mode {
        MatchMode::Literal => "Search (literal)".to_string(),
        MatchMode::Regex => "Search (regex)".to_string(),
    };

    let search_border_style = if app
        .status_message
        .as_ref()
        .is_some_and(|msg| msg.contains("regex parse error"))
    {
        Style::default().fg(Color::Red)
    } else {
        focused_border_style(Pane::SearchInput, app.focused_pane)
    };

    // Search input
    let search_focused = app.focused_pane == Pane::SearchInput;
    if search_focused {
        app.search_input.focus.set(true);
    } else {
        app.search_input.focus.set(false);
    }
    let search_block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(search_border_style)
        .title(mode_label);
    TextInput::new()
        .style(Style::default())
        .focus_style(Style::default())
        .block(search_block)
        .render(search_area, frame.buffer_mut(), &mut app.search_input);
    if let Some((cx, cy)) = app.search_input.screen_cursor() {
        frame.set_cursor_position((cx, cy));
    }

    // Replace input
    let replace_focused = app.focused_pane == Pane::ReplaceInput;
    if replace_focused {
        app.replace_input.focus.set(true);
    } else {
        app.replace_input.focus.set(false);
    }
    let replace_block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(focused_border_style(Pane::ReplaceInput, app.focused_pane))
        .title("Replace");
    TextInput::new()
        .style(Style::default())
        .focus_style(Style::default())
        .block(replace_block)
        .render(replace_area, frame.buffer_mut(), &mut app.replace_input);
    if let Some((cx, cy)) = app.replace_input.screen_cursor() {
        frame.set_cursor_position((cx, cy));
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
            let rel = fm.path.strip_prefix(&app.root).unwrap_or(&fm.path);
            let label = format!("{} ({}/{})", rel.display(), active, total);
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

fn build_preview_lines<'a>(
    fm: &'a FileMatches,
    replacement: &'a str,
    is_preview_focused: bool,
    selected_match: usize,
) -> (Vec<Line<'a>>, Range<usize>) {
    let mut lines: Vec<Line> = vec![];
    let mut selected_range: Range<usize> = 0..0;

    for (match_idx, m) in fm.matches.iter().enumerate() {
        let is_selected = is_preview_focused && match_idx == selected_match;
        let match_start = lines.len();

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

        // The match line itself: show full line with the matched portion highlighted
        let before_match = &m.line_content[..m.match_col_start];
        let after_match = &m.line_content[m.match_col_end..];

        if m.skip {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(before_match, Style::default().fg(Color::DarkGray)),
                Span::styled(
                    &m.matched_text,
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::CROSSED_OUT),
                ),
                Span::styled(after_match, Style::default().fg(Color::DarkGray)),
                Span::styled(" [skipped]", Style::default().fg(Color::DarkGray)),
            ]));
        } else if !replacement.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::raw(before_match),
                Span::styled(
                    &m.matched_text,
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::CROSSED_OUT),
                ),
                Span::styled(replacement, Style::default().fg(Color::Green)),
                Span::raw(after_match),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::raw(before_match),
                Span::styled(
                    &m.matched_text,
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw(after_match),
            ]));
        }

        // Context after
        for ctx in &m.context_after {
            lines.push(Line::from(Span::styled(
                format!("  {}", ctx.content),
                Style::default().fg(Color::DarkGray),
            )));
        }

        lines.push(Line::raw(""));

        if is_selected {
            selected_range = match_start..lines.len();
        }
    }

    (lines, selected_range)
}

fn render_preview(app: &mut App, frame: &mut Frame, area: Rect) {
    let border_style = focused_border_style(Pane::Preview, app.focused_pane);
    let block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(border_style)
        .title("Preview");

    let Some(fm) = app.results.get(app.selected_file) else {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new("Select a file").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    };

    let v_scroll = Scroll::vertical().style(border_style);
    let scroll_area = ScrollArea::new()
        .block(Some(&block))
        .v_scroll(Some(&v_scroll));
    let inner = scroll_area.inner(area, None, Some(&app.preview_scroll));

    let replacement = app.replace_input.text();
    let is_preview_focused = app.focused_pane == Pane::Preview;
    let (lines, selected_range) =
        build_preview_lines(fm, replacement, is_preview_focused, app.selected_match);

    // Update scroll state and auto-scroll to keep selected match visible
    app.preview_scroll.set_page_len(inner.height as usize);
    app.preview_scroll
        .set_max_offset(lines.len().saturating_sub(inner.height as usize));
    app.preview_scroll.scroll_to_range(selected_range);

    let offset = app.preview_scroll.offset;

    scroll_area.render(
        area,
        frame.buffer_mut(),
        &mut ScrollAreaState::new()
            .area(area)
            .v_scroll(&mut app.preview_scroll),
    );

    #[expect(clippy::cast_possible_truncation)]
    let scroll_offset = (offset as u16, 0);
    frame.render_widget(Paragraph::new(lines).scroll(scroll_offset), inner);
}

fn render_status_bar(app: &App, frame: &mut Frame, area: Rect) {
    let line = if let Some(msg) = &app.status_message {
        Line::from(Span::styled(msg.as_str(), Style::default().fg(Color::Red)))
    } else {
        let hints = match app.focused_pane {
            Pane::SearchInput | Pane::ReplaceInput => {
                "tab/shift-tab: cycle | ctrl-r: toggle regex | esc: file list | q/ctrl-c: quit"
            }
            Pane::FileList => {
                "tab/shift-tab: cycle | j/k: navigate | l/enter: preview | s: skip file | a: apply all | f: apply file | q/ctrl-c: quit"
            }
            Pane::Preview => {
                "tab/shift-tab: cycle | j/k: navigate | h/esc: back | space: toggle skip | s: skip file | enter: apply match | a: apply all | q/ctrl-c: quit"
            }
        };
        Line::from(hints.blue())
    };
    frame.render_widget(line, area);
}
