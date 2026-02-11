use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;

use crate::models::*;
use crate::tui::widgets::utilization_color;

pub fn render_usage_tab(
    f: &mut Frame,
    area: Rect,
    gpu_summaries: &[GPUTypeSummary],
    cpu_summary: &CPUTypeSummary,
    queue_summary: &QueueSummary,
    loading: bool,
    error: Option<&str>,
) {
    if loading {
        let block = Block::default()
            .title(" Usage - Loading... ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));
        f.render_widget(block, area);
        return;
    }

    if let Some(err) = error {
        let block = Block::default()
            .title(format!(" Usage - Error: {} ", err))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));
        f.render_widget(block, area);
        return;
    }

    let chunks = Layout::vertical([Constraint::Min(10), Constraint::Length(5)]).split(area);

    render_resource_table(f, chunks[0], gpu_summaries, cpu_summary);
    render_queue_summary(f, chunks[1], queue_summary);
}

fn render_resource_table(
    f: &mut Frame,
    area: Rect,
    gpu_summaries: &[GPUTypeSummary],
    cpu_summary: &CPUTypeSummary,
) {
    let header_cells = [
        "Resource",
        "Total",
        "Used",
        "Available",
        "Utilization",
        "Nodes",
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

    let mut rows: Vec<Row> = Vec::new();

    // GPU rows
    for summary in gpu_summaries {
        let util = summary.utilization_percent();
        let util_color = utilization_color(util);
        rows.push(Row::new(vec![
            Cell::from(format!("GPU ({})", summary.gpu_type))
                .style(Style::default().fg(Color::Cyan)),
            Cell::from(summary.total_gpus.to_string()),
            Cell::from(summary.used_gpus.to_string()),
            Cell::from(summary.available_gpus.to_string()),
            Cell::from(format!("{:.1}%", util)).style(Style::default().fg(util_color)),
            Cell::from(format!("{}", summary.nodes_with_type.len())),
        ]));
    }

    // GPU total
    let total_gpus: i32 = gpu_summaries.iter().map(|s| s.total_gpus).sum();
    let used_gpus: i32 = gpu_summaries.iter().map(|s| s.used_gpus).sum();
    let gpu_util = if total_gpus > 0 {
        used_gpus as f64 / total_gpus as f64 * 100.0
    } else {
        0.0
    };
    let gpu_util_color = utilization_color(gpu_util);
    let total_nodes: usize = gpu_summaries
        .iter()
        .flat_map(|s| s.nodes_with_type.iter())
        .collect::<std::collections::HashSet<_>>()
        .len();

    rows.push(Row::new(vec![
        Cell::from("GPU (TOTAL)").style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from(total_gpus.to_string()).style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from(used_gpus.to_string()).style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from((total_gpus - used_gpus).to_string())
            .style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from(format!("{:.1}%", gpu_util)).style(
            Style::default()
                .fg(gpu_util_color)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from(total_nodes.to_string()).style(Style::default().add_modifier(Modifier::BOLD)),
    ]));

    // CPU row
    let cpu_util = cpu_summary.utilization_percent();
    let cpu_util_color = utilization_color(cpu_util);
    rows.push(Row::new(vec![
        Cell::from("CPU (TOTAL)").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from(cpu_summary.total_cpus.to_string())
            .style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from(cpu_summary.used_cpus.to_string())
            .style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from(cpu_summary.available_cpus.to_string())
            .style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from(format!("{:.1}%", cpu_util)).style(
            Style::default()
                .fg(cpu_util_color)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from(cpu_summary.nodes_with_cpus.len().to_string())
            .style(Style::default().add_modifier(Modifier::BOLD)),
    ]));

    let table = Table::new(
        rows,
        [
            Constraint::Min(14), // Resource
            Constraint::Min(6),  // Total
            Constraint::Min(6),  // Used
            Constraint::Min(9),  // Available
            Constraint::Min(11), // Utilization
            Constraint::Min(5),  // Nodes
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(" Cluster Resources ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(table, area);
}

fn render_queue_summary(f: &mut Frame, area: Rect, queue_summary: &QueueSummary) {
    let text = if queue_summary.total_jobs > 0 {
        format!(
            " Queued: {} jobs | {} CPUs | {} GPUs requested",
            queue_summary.total_jobs, queue_summary.total_cpus, queue_summary.total_gpus,
        )
    } else {
        " No queued jobs".to_string()
    };

    let block = Block::default()
        .title(" Queue ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let paragraph = ratatui::widgets::Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}
