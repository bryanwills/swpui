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
    widgets::{Block, Clear, ListItem, Paragraph, StatefulWidget as _},
};

use crate::{
    app::App,
    search::CONTEXT_LINES,
    types::{FileMatches, MatchMode, Pane},
    utils::{format_file_entry, truncate_match_line},
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

    if app.confirm_apply_all {
        render_confirm_modal(frame, area);
    }
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
    let truncated = if app.truncated {
        " - limit reached"
    } else {
        ""
    };
    let title = if app.searching {
        format!(
            "{} Files ({}{} matched)",
            app.spinner.frame(),
            app.results.len(),
            truncated,
        )
    } else {
        format!("Files ({} matched{})", app.results.len(), truncated)
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
    let inner_width = block.inner(area).width as usize;
    let items: Vec<ListItem> = app
        .results
        .iter()
        .enumerate()
        .map(|(i, fm)| {
            let active = fm.active_match_count();
            let total = fm.matches.len();
            let rel = fm.path.strip_prefix(&app.root).unwrap_or(&fm.path);
            let suffix = format!(" ({active}/{total})");
            let label = format_file_entry(rel, &suffix, inner_width);
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

fn build_match_line<'a>(
    m: &'a crate::types::MatchInfo,
    replacement: &'a str,
    inner_width: u16,
) -> Line<'a> {
    let before_match = &m.line_content[..m.match_col_start];
    let after_match = &m.line_content[m.match_col_end..];
    let dark_gray = Style::default().fg(Color::DarkGray);

    if m.skip {
        let t = truncate_match_line(
            before_match,
            &m.matched_text,
            None,
            after_match,
            inner_width as usize,
        );
        let mut spans = Vec::with_capacity(7);
        spans.push(Span::raw(" "));
        if t.left_ellipsis {
            spans.push(Span::styled("\u{2026}", dark_gray));
        }
        spans.push(Span::styled(t.before, dark_gray));
        spans.push(Span::styled(
            t.matched,
            dark_gray.add_modifier(Modifier::CROSSED_OUT),
        ));
        spans.push(Span::styled(t.after, dark_gray));
        if t.right_ellipsis {
            spans.push(Span::styled("\u{2026}", dark_gray));
        }
        spans.push(Span::styled(" [skipped]", dark_gray));
        Line::from(spans)
    } else if !replacement.is_empty() {
        let t = truncate_match_line(
            before_match,
            &m.matched_text,
            Some(replacement),
            after_match,
            inner_width as usize,
        );
        let mut spans = Vec::with_capacity(7);
        spans.push(Span::raw(" "));
        if t.left_ellipsis {
            spans.push(Span::raw("\u{2026}"));
        }
        spans.push(Span::raw(t.before));
        spans.push(Span::styled(
            t.matched,
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::CROSSED_OUT),
        ));
        if let Some(repl) = t.replacement {
            spans.push(Span::styled(
                repl,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        spans.push(Span::raw(t.after));
        if t.right_ellipsis {
            spans.push(Span::raw("\u{2026}"));
        }
        Line::from(spans)
    } else {
        let t = truncate_match_line(
            before_match,
            &m.matched_text,
            None,
            after_match,
            inner_width as usize,
        );
        let mut spans = Vec::with_capacity(5);
        spans.push(Span::raw(" "));
        if t.left_ellipsis {
            spans.push(Span::raw("\u{2026}"));
        }
        spans.push(Span::raw(t.before));
        spans.push(Span::styled(
            t.matched,
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD | Modifier::CROSSED_OUT),
        ));
        spans.push(Span::raw(t.after));
        if t.right_ellipsis {
            spans.push(Span::raw("\u{2026}"));
        }
        Line::from(spans)
    }
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

        lines.push(build_match_line(m, replacement, inner_width));

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

    let title_max = area.width.saturating_sub(2) as usize; // border chars
    let title = app.results.get(app.selected_file()).map_or_else(
        || "Preview".to_string(),
        |fm| {
            let rel = fm.path.strip_prefix(&app.root).unwrap_or(&fm.path);
            let path_str = rel.display().to_string();
            let prefix = "Preview: ";
            let full = format!("{prefix}{path_str}");
            if full.len() <= title_max {
                full
            } else {
                // truncate path from the left with ellipsis
                let avail = title_max.saturating_sub(prefix.len() + 1); // 1 for ellipsis
                let start = path_str.len().saturating_sub(avail);
                format!("{prefix}\u{2026}{}", &path_str[start..])
            }
        },
    );

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
    let (lines, selected_range) = build_preview_lines(
        fm,
        replacement,
        is_preview_focused,
        app.selected_match,
        inner.width,
    );

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

fn render_confirm_modal(frame: &mut Frame, area: Rect) {
    let width = 30u16.min(area.width);
    let height = 4u16.min(area.height);
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, modal_area);
    let block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(Color::Yellow))
        .title("Confirm");
    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);
    frame.render_widget(
        Paragraph::new("Apply all replacements?\ny / n").alignment(ratatui::layout::Alignment::Center),
        inner,
    );
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
