use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::constants::{EXCELLENT_EFFICIENCY_THRESHOLD, GOOD_EFFICIENCY_THRESHOLD, POOR_EFFICIENCY_THRESHOLD};

pub fn efficiency_color(value: &str) -> Color {
    if value == "---" {
        return Color::DarkGray;
    }
    let val = value.trim_end_matches('%').parse::<f64>().unwrap_or(-1.0);
    if val < 0.0 {
        Color::DarkGray
    } else if val >= EXCELLENT_EFFICIENCY_THRESHOLD {
        Color::Green
    } else if val >= GOOD_EFFICIENCY_THRESHOLD {
        Color::Yellow
    } else if val >= POOR_EFFICIENCY_THRESHOLD {
        Color::LightRed
    } else {
        Color::Red
    }
}

pub fn utilization_color(value: f64) -> Color {
    if value >= EXCELLENT_EFFICIENCY_THRESHOLD {
        Color::Red
    } else if value >= GOOD_EFFICIENCY_THRESHOLD {
        Color::Yellow
    } else {
        Color::Green
    }
}

pub fn state_color(state: &str) -> Color {
    match state.to_uppercase().as_str() {
        "COMPLETED" => Color::Green,
        "FAILED" => Color::Red,
        "CANCELLED" | "TIMEOUT" => Color::Yellow,
        "PENDING" => Color::Blue,
        "RUNNING" => Color::Cyan,
        _ => Color::White,
    }
}

pub fn efficiency_cell(value: &str) -> Span<'static> {
    let color = efficiency_color(value);
    let style = if value != "---" {
        let val = value.trim_end_matches('%').parse::<f64>().unwrap_or(-1.0);
        if (0.0..POOR_EFFICIENCY_THRESHOLD).contains(&val) {
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        }
    } else {
        Style::default().fg(color)
    };
    Span::styled(value.to_string(), style)
}

pub fn render_help_popup(f: &mut Frame) {
    let area = centered_rect(60, 70, f.area());
    f.render_widget(Clear, area);

    let help_text = vec![
        Line::from(Span::styled("Key Bindings", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  q       ", Style::default().fg(Color::Yellow)),
            Span::raw("Quit"),
        ]),
        Line::from(vec![
            Span::styled("  1/2/3   ", Style::default().fg(Color::Yellow)),
            Span::raw("Switch tab (Report/Usage/My Jobs)"),
        ]),
        Line::from(vec![
            Span::styled("  Tab     ", Style::default().fg(Color::Yellow)),
            Span::raw("Next tab (Shift+Tab for previous)"),
        ]),
        Line::from(vec![
            Span::styled("  r       ", Style::default().fg(Color::Yellow)),
            Span::raw("Manual refresh (resets timer)"),
        ]),
        Line::from(vec![
            Span::styled("  p       ", Style::default().fg(Color::Yellow)),
            Span::raw("Pause/resume auto-refresh"),
        ]),
        Line::from(vec![
            Span::styled("  s       ", Style::default().fg(Color::Yellow)),
            Span::raw("Cycle state filter (all/RUNNING/PENDING/...)"),
        ]),
        Line::from(vec![
            Span::styled("  +/-     ", Style::default().fg(Color::Yellow)),
            Span::raw("Increase/decrease refresh interval (5s steps)"),
        ]),
        Line::from(vec![
            Span::styled("  /       ", Style::default().fg(Color::Yellow)),
            Span::raw("Focus search input"),
        ]),
        Line::from(vec![
            Span::styled("  Esc     ", Style::default().fg(Color::Yellow)),
            Span::raw("Clear search / unfocus"),
        ]),
        Line::from(vec![
            Span::styled("  ?       ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle this help popup"),
        ]),
        Line::from(vec![
            Span::styled("  Up/Down ", Style::default().fg(Color::Yellow)),
            Span::raw("Navigate table rows"),
        ]),
        Line::from(vec![
            Span::styled("  Home/End", Style::default().fg(Color::Yellow)),
            Span::raw("Jump to first/last row"),
        ]),
        Line::from(""),
        Line::from(Span::styled("Press any key to close", Style::default().fg(Color::DarkGray))),
    ];

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(help_text)
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
