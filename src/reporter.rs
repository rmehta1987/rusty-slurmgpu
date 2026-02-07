use comfy_table::{presets::UTF8_FULL_CONDENSED, Table, ContentArrangement, Cell, CellAlignment, Color, Attribute};

use crate::calculator::EfficiencyCalculator;
use crate::constants::*;
use crate::models::{GPUMetrics, SummaryMetrics};

pub struct GPUReporter;

impl GPUReporter {
    /// Get color for efficiency percentage.
    pub fn get_efficiency_color(value: &str) -> Color {
        if value == "---" || value.is_empty() {
            return Color::White;
        }

        let num_value = match value.trim_end_matches('%').parse::<f64>() {
            Ok(v) => v,
            Err(_) => return Color::White,
        };

        if num_value >= EXCELLENT_EFFICIENCY_THRESHOLD {
            Color::Green
        } else if num_value >= GOOD_EFFICIENCY_THRESHOLD {
            Color::Yellow
        } else {
            Color::Red
        }
    }

    /// Check if efficiency value is critically low (< 30%) for bold formatting.
    pub fn is_critical_efficiency(value: &str) -> bool {
        if value == "---" || value.is_empty() {
            return false;
        }
        match value.trim_end_matches('%').parse::<f64>() {
            Ok(v) => v < POOR_EFFICIENCY_THRESHOLD,
            Err(_) => false,
        }
    }

    /// Create a cell with efficiency color, adding bold for critical values.
    fn efficiency_cell(value: &str) -> Cell {
        let cell = Cell::new(value)
            .fg(Self::get_efficiency_color(value))
            .set_alignment(CellAlignment::Right);
        if Self::is_critical_efficiency(value) {
            cell.add_attribute(Attribute::Bold)
        } else {
            cell
        }
    }

    /// Get color for job state.
    pub fn get_state_color(state: &str) -> Color {
        match state.to_uppercase().as_str() {
            "COMPLETED" => Color::Green,
            "FAILED" => Color::Red,
            "CANCELLED" => Color::Yellow,
            "TIMEOUT" => Color::Yellow,
            "PENDING" => Color::Blue,
            "RUNNING" => Color::Cyan,
            _ => Color::White,
        }
    }

    /// Format metrics into a plain text table report.
    pub fn format_report(
        metrics: &[GPUMetrics],
        show_partition: bool,
        detailed: bool,
        show_weighted_avg: bool,
    ) -> String {
        if metrics.is_empty() {
            return "No jobs found.".to_string();
        }

        let mut lines = Vec::new();

        // Build header
        let mut header_parts = vec![
            format!("{:<11}", "User"),
            format!("{:<11}", "JobID"),
            format!("{:<10}", "State"),
            format!("{:>8}", "Elapsed"),
            format!("{:>7}", "TimeEff"),
            format!("{:>7}", "CPUEff"),
            format!("{:>7}", "MemEff"),
            format!("{:>7}", "GPUEff"),
            format!("{:>8}", "GPUUtil"),
            format!("{:>10}", "GPUMemEff"),
            format!("{:>7}", "GPUMem"),
        ];

        if detailed {
            header_parts.push(format!("{:<12}", "Node"));
            header_parts.push(format!("{:<8}", "GPUType"));
            header_parts.push(format!("{:<12}", "Account"));
        }

        if show_partition {
            header_parts.push("Partition".to_string());
        }

        let header = header_parts.join(" ");
        let separator = "-".repeat(header.len());

        lines.push(header);
        lines.push(separator.clone());

        // Data rows
        for metric in metrics {
            let mut parts = vec![
                format!("{:<11}", metric.user),
                format!("{:<11}", metric.job_id),
                format!("{:<10}", metric.state),
                format!("{:>8}", metric.elapsed),
                format!("{:>7}", metric.time_eff),
                format!("{:>7}", metric.cpu_eff),
                format!("{:>7}", metric.mem_eff),
                format!("{:>7}", metric.gpu_eff),
                format!("{:>8}", metric.gpu_util),
                format!("{:>10}", metric.gpu_mem_eff),
                format!("{:>7}", metric.gpu_mem),
            ];

            if detailed {
                parts.push(format!("{:<12}", metric.node.as_deref().unwrap_or("---")));
                let gpu_type_str = if metric.gpu_count == 0 {
                    "-"
                } else {
                    metric.gpu_type.as_deref().unwrap_or("gpu")
                };
                parts.push(format!("{:<8}", gpu_type_str));
                parts.push(format!(
                    "{:<12}",
                    metric.account.as_deref().unwrap_or("---")
                ));
            }

            if show_partition {
                parts.push(metric.partition.clone());
            }

            lines.push(parts.join(" "));
        }

        // Weighted average
        if show_weighted_avg && !metrics.is_empty() {
            use crate::calculator::EfficiencyCalculator;
            let weighted_avg = EfficiencyCalculator::calculate_time_weighted_average(metrics);

            lines.push(separator);

            let mut avg_parts = vec![
                format!("{:<11}", weighted_avg.user),
                format!("{:<11}", if weighted_avg.job_id.to_string() == "0" { "".to_string() } else { weighted_avg.job_id.to_string() }),
                format!("{:<10}", "WEIGHTED"),
                format!("{:>8}", weighted_avg.elapsed),
                format!("{:>7}", weighted_avg.time_eff),
                format!("{:>7}", weighted_avg.cpu_eff),
                format!("{:>7}", weighted_avg.mem_eff),
                format!("{:>7}", weighted_avg.gpu_eff),
                format!("{:>8}", weighted_avg.gpu_util),
                format!("{:>10}", weighted_avg.gpu_mem_eff),
                format!("{:>7}", weighted_avg.gpu_mem),
            ];

            if detailed {
                avg_parts.push(format!("{:<12}", "---"));
                avg_parts.push(format!("{:<8}", "---"));
                avg_parts.push(format!("{:<12}", "---"));
            }

            if show_partition {
                avg_parts.push(String::new());
            }

            lines.push(avg_parts.join(" "));
        }

        lines.join("\n") + "\n"
    }

    /// Format metrics into a rich table using comfy-table.
    pub fn format_rich_report(
        metrics: &[GPUMetrics],
        show_partition: bool,
        detailed: bool,
        show_weighted_avg: bool,
    ) -> Table {
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL_CONDENSED)
            .set_content_arrangement(ContentArrangement::Dynamic);

        // Build header row — white + bold for clean contrast
        let hdr = |name: &str| Cell::new(name).add_attribute(Attribute::Bold).fg(Color::White);
        let hdr_right = |name: &str| hdr(name).set_alignment(CellAlignment::Right);

        let mut headers: Vec<Cell> = vec![
            hdr("User"),
            hdr("JobID"),
            hdr("State"),
            hdr_right("Elapsed"),
            hdr_right("TimeEff"),
            hdr_right("CPUEff"),
            hdr_right("MemEff"),
            hdr_right("GPUEff"),
            hdr_right("GPUUtil"),
            hdr_right("GPUMemEff"),
            hdr_right("GPUMem"),
        ];

        if detailed {
            headers.push(hdr("Node"));
            headers.push(hdr("GPUType"));
            headers.push(hdr("Account"));
        }

        if show_partition {
            headers.push(hdr("Partition"));
        }

        table.set_header(headers);

        // Data rows
        for metric in metrics {
            let mut row: Vec<Cell> = vec![
                Cell::new(&metric.user).fg(Color::Magenta),
                Cell::new(metric.job_id.to_string()).fg(Color::Cyan),
                Cell::new(&metric.state).fg(Self::get_state_color(&metric.state)),
                Cell::new(&metric.elapsed).set_alignment(CellAlignment::Right),
                Self::efficiency_cell(&metric.time_eff),
                Self::efficiency_cell(&metric.cpu_eff),
                Self::efficiency_cell(&metric.mem_eff),
                Self::efficiency_cell(&metric.gpu_eff),
                Self::efficiency_cell(&metric.gpu_util),
                Self::efficiency_cell(&metric.gpu_mem_eff),
                Cell::new(&metric.gpu_mem).set_alignment(CellAlignment::Right),
            ];

            if detailed {
                row.push(Cell::new(metric.node.as_deref().unwrap_or("---")).fg(Color::Green));
                let gpu_type_str = if metric.gpu_count == 0 {
                    "-"
                } else {
                    metric.gpu_type.as_deref().unwrap_or("gpu")
                };
                row.push(Cell::new(gpu_type_str).fg(Color::Yellow));
                row.push(Cell::new(metric.account.as_deref().unwrap_or("---")).fg(Color::Cyan));
            }

            if show_partition {
                row.push(Cell::new(&metric.partition).fg(Color::Blue));
            }

            table.add_row(row);
        }

        // Weighted average
        if show_weighted_avg && !metrics.is_empty() {
            use crate::calculator::EfficiencyCalculator;
            let weighted_avg = EfficiencyCalculator::calculate_time_weighted_average(metrics);

            let mut avg_row: Vec<Cell> = vec![
                Cell::new("").add_attribute(Attribute::Bold),
                Cell::new("").add_attribute(Attribute::Bold),
                Cell::new("WEIGHTED AVG").add_attribute(Attribute::Bold).fg(Color::Cyan),
                Cell::new(&weighted_avg.elapsed).add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
                Self::efficiency_cell(&weighted_avg.time_eff).add_attribute(Attribute::Bold),
                Self::efficiency_cell(&weighted_avg.cpu_eff).add_attribute(Attribute::Bold),
                Self::efficiency_cell(&weighted_avg.mem_eff).add_attribute(Attribute::Bold),
                Self::efficiency_cell(&weighted_avg.gpu_eff).add_attribute(Attribute::Bold),
                Self::efficiency_cell(&weighted_avg.gpu_util).add_attribute(Attribute::Bold),
                Self::efficiency_cell(&weighted_avg.gpu_mem_eff).add_attribute(Attribute::Bold),
                Cell::new(&weighted_avg.gpu_mem).add_attribute(Attribute::Bold).set_alignment(CellAlignment::Right),
            ];

            if detailed {
                avg_row.push(Cell::new(""));
                avg_row.push(Cell::new(""));
                avg_row.push(Cell::new(""));
            }

            if show_partition {
                avg_row.push(Cell::new(""));
            }

            table.add_row(avg_row);
        }

        table
    }

    /// Print report to console.
    pub fn print_report(
        metrics: &[GPUMetrics],
        use_rich: bool,
        show_partition: bool,
        detailed: bool,
        show_weighted_avg: bool,
    ) {
        if metrics.is_empty() {
            println!("No jobs found.");
            return;
        }

        if use_rich {
            let table = Self::format_rich_report(metrics, show_partition, detailed, show_weighted_avg);
            println!("{table}");
        } else {
            let report = Self::format_report(metrics, show_partition, detailed, show_weighted_avg);
            print!("{report}");
        }
    }

    /// Format summary report as plain text.
    pub fn format_summary_report(
        summaries: &[SummaryMetrics],
        by_partition: bool,
        by_account: bool,
        account_only: bool,
    ) -> String {
        if summaries.is_empty() {
            return "No summary data found.".to_string();
        }

        let mut lines = Vec::new();

        // Header
        if account_only {
            lines.push("Account        Jobs   GPU Hours  Completed  Failed  Running  Pending  Avg GPU Eff  Avg GPU Mem  Avg Time Eff".to_string());
            lines.push("-".repeat(105));
        } else if by_partition {
            lines.push("User        Partition      Jobs   GPU Hours  Completed  Failed  Running  Pending  Avg GPU Eff  Avg GPU Mem  Avg Time Eff".to_string());
            lines.push("-".repeat(115));
        } else if by_account {
            lines.push("User        Account        Jobs   GPU Hours  Completed  Failed  Running  Pending  Avg GPU Eff  Avg GPU Mem  Avg Time Eff".to_string());
            lines.push("-".repeat(115));
        } else {
            lines.push("User        Jobs   GPU Hours  Completed  Failed  Running  Pending  Avg GPU Eff  Avg GPU Mem  Avg Time Eff".to_string());
            lines.push("-".repeat(100));
        }

        for summary in summaries {
            let line = if account_only {
                format!(
                    "{:<14} {:>5} {:>10.1} {:>9} {:>7} {:>8} {:>8} {:>11.1}% {:>11.1}% {:>12.1}%",
                    summary.account.as_deref().unwrap_or("N/A"),
                    summary.job_count, summary.total_gpu_hours,
                    summary.completed_jobs, summary.failed_jobs,
                    summary.running_jobs, summary.pending_jobs,
                    summary.avg_gpu_eff, summary.avg_gpu_mem_eff, summary.avg_time_eff
                )
            } else if by_partition {
                format!(
                    "{:<11} {:<14} {:>5} {:>10.1} {:>9} {:>7} {:>8} {:>8} {:>11.1}% {:>11.1}% {:>12.1}%",
                    summary.user, summary.partition.as_deref().unwrap_or("N/A"),
                    summary.job_count, summary.total_gpu_hours,
                    summary.completed_jobs, summary.failed_jobs,
                    summary.running_jobs, summary.pending_jobs,
                    summary.avg_gpu_eff, summary.avg_gpu_mem_eff, summary.avg_time_eff
                )
            } else if by_account {
                format!(
                    "{:<11} {:<14} {:>5} {:>10.1} {:>9} {:>7} {:>8} {:>8} {:>11.1}% {:>11.1}% {:>12.1}%",
                    summary.user, summary.account.as_deref().unwrap_or("N/A"),
                    summary.job_count, summary.total_gpu_hours,
                    summary.completed_jobs, summary.failed_jobs,
                    summary.running_jobs, summary.pending_jobs,
                    summary.avg_gpu_eff, summary.avg_gpu_mem_eff, summary.avg_time_eff
                )
            } else {
                format!(
                    "{:<11} {:>5} {:>10.1} {:>9} {:>7} {:>8} {:>8} {:>11.1}% {:>11.1}% {:>12.1}%",
                    summary.user, summary.job_count, summary.total_gpu_hours,
                    summary.completed_jobs, summary.failed_jobs,
                    summary.running_jobs, summary.pending_jobs,
                    summary.avg_gpu_eff, summary.avg_gpu_mem_eff, summary.avg_time_eff
                )
            };
            lines.push(line);
        }

        lines.join("\n") + "\n"
    }

    /// Format summary report as rich table.
    pub fn format_rich_summary_report(
        summaries: &[SummaryMetrics],
        by_partition: bool,
        by_account: bool,
        account_only: bool,
    ) -> Table {
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL_CONDENSED)
            .set_content_arrangement(ContentArrangement::Dynamic);

        let hdr = |name: &str| Cell::new(name).add_attribute(Attribute::Bold).fg(Color::White);
        let hdr_right = |name: &str| hdr(name).set_alignment(CellAlignment::Right);

        let mut headers: Vec<Cell> = Vec::new();

        if account_only {
            headers.push(hdr("Account"));
        } else {
            headers.push(hdr("User"));
            if by_partition {
                headers.push(hdr("Partition"));
            } else if by_account {
                headers.push(hdr("Account"));
            }
        }

        headers.extend([
            hdr_right("Jobs"),
            hdr_right("GPU Hours"),
            hdr_right("Completed"),
            hdr_right("Failed"),
            hdr_right("Running"),
            hdr_right("Pending"),
            hdr_right("Avg GPU Eff"),
            hdr_right("Avg GPU Mem"),
            hdr_right("Avg Time Eff"),
        ]);

        table.set_header(headers);

        for summary in summaries {
            let mut row: Vec<Cell> = Vec::new();

            if account_only {
                row.push(Cell::new(summary.account.as_deref().unwrap_or("N/A")).fg(Color::Cyan));
            } else {
                row.push(Cell::new(&summary.user).fg(Color::Magenta));
                if by_partition {
                    row.push(Cell::new(summary.partition.as_deref().unwrap_or("N/A")).fg(Color::Blue));
                } else if by_account {
                    row.push(Cell::new(summary.account.as_deref().unwrap_or("N/A")).fg(Color::Cyan));
                }
            }

            let gpu_eff_str = format!("{:.1}%", summary.avg_gpu_eff);
            let gpu_mem_str = format!("{:.1}%", summary.avg_gpu_mem_eff);
            let time_eff_str = format!("{:.1}%", summary.avg_time_eff);

            row.extend([
                Cell::new(summary.job_count.to_string()).set_alignment(CellAlignment::Right),
                Cell::new(format!("{:.1}", summary.total_gpu_hours)).set_alignment(CellAlignment::Right),
                Cell::new(summary.completed_jobs.to_string()).set_alignment(CellAlignment::Right),
                Cell::new(summary.failed_jobs.to_string()).set_alignment(CellAlignment::Right),
                Cell::new(summary.running_jobs.to_string()).set_alignment(CellAlignment::Right),
                Cell::new(summary.pending_jobs.to_string()).set_alignment(CellAlignment::Right),
                Self::efficiency_cell(&gpu_eff_str),
                Self::efficiency_cell(&gpu_mem_str),
                Self::efficiency_cell(&time_eff_str),
            ]);

            table.add_row(row);
        }

        table
    }

    /// Print summary report to console.
    pub fn print_summary_report(
        summaries: &[SummaryMetrics],
        use_rich: bool,
        by_partition: bool,
        by_account: bool,
        account_only: bool,
    ) {
        if summaries.is_empty() {
            println!("No summary data found.");
            return;
        }

        if use_rich {
            let table = Self::format_rich_summary_report(summaries, by_partition, by_account, account_only);
            println!("{table}");
        } else {
            let report = Self::format_summary_report(summaries, by_partition, by_account, account_only);
            print!("{report}");
        }
    }

    /// Format weighted average metrics in InfluxDB Line Protocol for Telegraf.
    ///
    /// Returns a single line like:
    /// `slurm_gpu_efficiency,user=ukh,partition=h200alloc cpu_eff=1.3,mem_eff=59.9,gpu_eff=100.0,gpu_util=100.0,gpu_mem_eff=89.7 1738800000000000000`
    pub fn format_telegraf_weighted_avg(
        metrics: &[GPUMetrics],
        user: &str,
        partition: Option<&str>,
        account: Option<&str>,
    ) -> String {
        if metrics.is_empty() {
            return String::new();
        }

        let weighted_avg = EfficiencyCalculator::calculate_time_weighted_average(metrics);

        let timestamp_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        // Build tags
        let mut tags = vec![format!("user={}", user)];
        if let Some(p) = partition {
            tags.push(format!("partition={}", p));
        }
        if let Some(a) = account {
            tags.push(format!("account={}", a));
        }

        // Build fields - use 0.0 for missing values so Prometheus gets a consistent field set
        let parse_pct = |s: &str| -> f64 {
            if s == "---" || s.is_empty() {
                return 0.0;
            }
            s.trim_end_matches('%').parse::<f64>().unwrap_or(0.0)
        };

        let gpu_mem_val = if weighted_avg.gpu_mem != "---" && !weighted_avg.gpu_mem.is_empty() {
            weighted_avg.gpu_mem.trim_end_matches('G').parse::<f64>().unwrap_or(0.0)
        } else {
            0.0
        };

        let fields = format!(
            "cpu_eff={:.1},mem_eff={:.1},gpu_eff={:.1},gpu_util={:.1},gpu_mem_eff={:.1},gpu_mem={:.1}",
            parse_pct(&weighted_avg.cpu_eff),
            parse_pct(&weighted_avg.mem_eff),
            parse_pct(&weighted_avg.gpu_eff),
            parse_pct(&weighted_avg.gpu_util),
            parse_pct(&weighted_avg.gpu_mem_eff),
            gpu_mem_val,
        );

        format!(
            "slurm_gpu_efficiency,{} {} {}",
            tags.join(","),
            fields,
            timestamp_ns
        )
    }
}
