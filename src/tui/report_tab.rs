use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};
use ratatui::Frame;

use crate::models::GPUMetrics;
use crate::tui::widgets::{efficiency_cell, state_color};

pub fn render_report_tab(
    f: &mut Frame,
    area: Rect,
    metrics: &[GPUMetrics],
    search: &str,
    state_filter: &str,
    table_state: &mut TableState,
    loading: bool,
    error: Option<&str>,
) {
    if loading {
        let block = Block::default()
            .title(" Report - Loading... ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));
        f.render_widget(block, area);
        return;
    }

    if let Some(err) = error {
        let block = Block::default()
            .title(format!(" Report - Error: {} ", err))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));
        f.render_widget(block, area);
        return;
    }

    let search_lower = search.to_lowercase();
    let filtered: Vec<&GPUMetrics> = metrics
        .iter()
        .filter(|m| {
            if state_filter != "all" && !m.state.matches_filter(state_filter) {
                return false;
            }
            if !search_lower.is_empty() {
                return m.user.to_lowercase().contains(&search_lower)
                    || m.job_id.to_string().contains(&search_lower)
                    || m.state.contains_search(&search_lower)
                    || m.partition.to_lowercase().contains(&search_lower)
                    || m.node
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&search_lower)
                    || m.gpu_type
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&search_lower);
            }
            true
        })
        .collect();

    let header_cells = [
        "User", "JobID", "State", "Elapsed", "TimeEff", "CPUEff",
        "MemEff", "GPUEff", "GPUUtil", "GPUMemEff", "Partition",
    ]
    .iter()
    .map(|h| {
        Cell::from(*h).style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
    });

    let header = Row::new(header_cells).height(1);

    let rows: Vec<Row> = filtered
        .iter()
        .map(|m| {
            Row::new(vec![
                Cell::from(m.user.as_str()),
                Cell::from(m.job_id.to_string()),
                Cell::from(Span::styled(
                    m.state.as_str().to_string(),
                    Style::default().fg(state_color(m.state.as_str())),
                )),
                Cell::from(m.elapsed.as_str()),
                Cell::from(efficiency_cell(&m.time_eff)),
                Cell::from(efficiency_cell(&m.cpu_eff)),
                Cell::from(efficiency_cell(&m.mem_eff)),
                Cell::from(efficiency_cell(&m.gpu_eff)),
                Cell::from(efficiency_cell(&m.gpu_util)),
                Cell::from(efficiency_cell(&m.gpu_mem_eff)),
                Cell::from(m.partition.as_str()),
            ])
        })
        .collect();

    let title = format!(" Report ({} jobs) ", filtered.len());
    let table = Table::new(
        rows,
        [
            Constraint::Min(8),   // User
            Constraint::Min(8),   // JobID
            Constraint::Min(9),   // State
            Constraint::Min(8),   // Elapsed
            Constraint::Min(7),   // TimeEff
            Constraint::Min(6),   // CPUEff
            Constraint::Min(6),   // MemEff
            Constraint::Min(6),   // GPUEff
            Constraint::Min(7),   // GPUUtil
            Constraint::Min(7),   // GPUMemEff
            Constraint::Min(9),   // Partition
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .row_highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol(">> ");

    f.render_stateful_widget(table, area, table_state);
}
