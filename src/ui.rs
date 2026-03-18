use std::ops::Range;

use rat_widget::{
    list::List,
    scrolled::{Scroll, ScrollArea, ScrollAreaState},
    text::HasScreenCursor as _,
    text_input::TextInput,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize as _},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, ListItem, Paragraph, StatefulWidget as _},
};

use crate::{
    app::App,
    search::CONTEXT_LINES,
    types::{FileMatches, MatchMode, Pane},
};

pub fn render(app: &mut App, frame: &mut Frame) {
    let area = frame.area();

    // main layout: content area + status bar
    let [content_area, status_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

    // split content into left and right columns
    // shrink the file list when the preview pane is focused
    let left_size = if app.focused_pane == Pane::Preview {
        let target = content_area.width / 5;
        Constraint::Length(target.max(10))
    } else {
        let target = content_area.width / 2;
        Constraint::Length(target.min(50))
    };
    let [left, right] = Layout::horizontal([left_size, Constraint::Fill(1)]).areas(content_area);

    // left column: input area + file list
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

    // Search input
    app.search_input
        .focus
        .set(app.focused_pane == Pane::SearchInput);
    let search_block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(focused_border_style(Pane::SearchInput, app.focused_pane))
        .title(mode_label);
    TextInput::new()
        .style(Style::default())
        .focus_style(Style::default())
        .invalid_style(Style::default().fg(Color::Red))
        .block(search_block)
        .render(search_area, frame.buffer_mut(), &mut app.search_input);
    if let Some((cx, cy)) = app.search_input.screen_cursor() {
        frame.set_cursor_position((cx, cy));
    }

    // Replace input
    app.replace_input
        .focus
        .set(app.focused_pane == Pane::ReplaceInput);
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

fn render_file_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let title = if app.searching {
        format!(
            "{} Files ({} matched)",
            app.spinner.frame(),
            app.results.len()
        )
    } else {
        format!("Files ({} matched)", app.results.len())
    };
    let border_style = focused_border_style(Pane::FileList, app.focused_pane);
    let block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(border_style)
        .title(title);

    if app.results.is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new("No matches").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    let selected = app.file_list.selected();
    let compact = app.focused_pane == Pane::Preview;
    let items: Vec<ListItem> = app
        .results
        .iter()
        .enumerate()
        .map(|(i, fm)| {
            let active = fm.active_match_count();
            let total = fm.matches.len();
            let rel = fm.path.strip_prefix(&app.root).unwrap_or(&fm.path);
            let name = if compact {
                rel.file_name()
                    .map_or_else(|| rel.display().to_string(), |n| n.to_string_lossy().into_owned())
            } else {
                rel.display().to_string()
            };
            let label = format!("{name} ({active}/{total})");
            if Some(i) != selected && active == 0 {
                ListItem::new(Line::styled(label, Style::default().fg(Color::DarkGray)))
            } else {
                ListItem::new(label)
            }
        })
        .collect();

    // set up scroll state so scroll_to_selected works before render
    let inner_height = block.inner(area).height as usize;
    app.file_list.scroll.set_page_len(inner_height);
    app.file_list
        .scroll
        .set_max_offset(items.len().saturating_sub(inner_height));
    app.file_list.scroll_to_selected();

    let select_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    List::new(items)
        .block(block)
        .scroll(Scroll::vertical().style(border_style))
        .select_style(select_style)
        .focus_style(select_style)
        .render(area, frame.buffer_mut(), &mut app.file_list);
}

fn build_preview_lines<'a>(
    fm: &'a FileMatches,
    replacement: &'a str,
    is_preview_focused: bool,
    selected_match: usize,
    inner_width: u16,
) -> (Vec<Line<'a>>, Range<usize>) {
    let mut lines: Vec<Line> = Vec::with_capacity(fm.matches.len() * CONTEXT_LINES * 2 + 3);
    let mut selected_range: Range<usize> = 0..0;
    let separator: String = "─".repeat(inner_width as usize);

    for (match_idx, m) in fm.matches.iter().enumerate() {
        let is_selected = is_preview_focused && match_idx == selected_match;

        // Horizontal separator between matches
        if match_idx > 0 {
            lines.push(Line::styled(
                separator.clone(),
                Style::default().fg(Color::DarkGray),
            ));
        }

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
            format!(" line {}:", m.line_number),
            header_style,
        )));

        // Context before
        for ctx in &m.context_before {
            lines.push(Line::from(Span::styled(
                format!(" {}", ctx.content),
                Style::default().fg(Color::DarkGray),
            )));
        }

        // The match line itself: show full line with the matched portion highlighted
        let before_match = &m.line_content[..m.match_col_start];
        let after_match = &m.line_content[m.match_col_end..];

        if m.skip {
            lines.push(Line::from(vec![
                Span::raw(" "),
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
                Span::raw(" "),
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
                Span::raw(" "),
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
                format!(" {}", ctx.content),
                Style::default().fg(Color::DarkGray),
            )));
        }

        if is_selected {
            selected_range = match_start..lines.len();
        }
    }

    (lines, selected_range)
}

fn render_preview(app: &mut App, frame: &mut Frame, area: Rect) {
    let border_style = focused_border_style(Pane::Preview, app.focused_pane);

    let title = app
        .results
        .get(app.selected_file())
        .map(|fm| {
            let rel = fm.path.strip_prefix(&app.root).unwrap_or(&fm.path);
            format!("Preview: {}", rel.display())
        })
        .unwrap_or_else(|| "Preview".to_string());

    let block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(border_style)
        .title(title);

    let Some(fm) = app.results.get(app.selected_file()) else {
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
        build_preview_lines(fm, replacement, is_preview_focused, app.selected_match, inner.width);

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
                "ctrl-r: toggle regex | esc: file list | tab/shift-tab: cycle | q/ctrl-c: quit"
            }
            Pane::FileList => {
                "s: skip file | f: apply file | a: apply all | j/k: navigate | l/enter: preview | tab/shift-tab: cycle | q/ctrl-c: quit"
            }
            Pane::Preview => {
                "space: toggle skip | enter: apply match | s: skip file | a: apply all | j/k: navigate | h/esc: back | tab/shift-tab: cycle | q/ctrl-c: quit"
            }
        };
        Line::from(hints.blue())
    };
    frame.render_widget(line, area);
}
